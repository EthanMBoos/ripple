//! When each fleet manager is called and with what, and what becomes of its
//! orders. A round calls every manager, one over the region or one per
//! cluster, then records and applies the orders, then calls the manager
//! above the clusters if there is one.

use anyhow::Result;
use ripple_core::plugin::AgentContext;

use super::{Dispatcher, Tally};
use crate::event::Event;
use crate::messages::{CALL_TOPIC, ManagerCall, ManagerKind, ORDER_TOPIC, OrderRecord};
use crate::model::Asked;
use crate::order::{Order, parse_veto};
use crate::replanner;

/// An order as a manager gave it: from which cluster, if any, and whether
/// the call had nothing new to act on.
pub(super) struct Given {
    pub order: Order,
    pub cluster: Option<Vec<u32>>,
    pub quiet: bool,
}

/// One round of calls: the orders given, the calls made, the events of the
/// round for the manager above the clusters, and the orders in a model's
/// replies that did not read.
#[derive(Default)]
struct Round {
    orders: Vec<Given>,
    calls: Vec<ManagerCall>,
    due_events: Vec<u32>,
    unreadable: u32,
}

impl Dispatcher {
    /// The clusters a manager is called for: every hub at once, or blocks.
    pub(super) fn clusters(&self) -> Vec<Option<Vec<u32>>> {
        match self.config.manager.cluster_side {
            0 => vec![None],
            side => self.region.clusters(side).into_iter().map(Some).collect(),
        }
    }

    /// Whether the fleet manager is called now, and with which events: the
    /// ones queued since the last call, once the call budget allows it, or a
    /// check-in with none.
    fn due(&mut self, now_s: f64, new_events: &[usize]) -> Option<Vec<usize>> {
        let manager = &self.config.manager;
        self.queued_events.extend_from_slice(new_events);
        let spacing_s = if manager.calls_per_hour > 0.0 {
            3600.0 / manager.calls_per_hour
        } else {
            0.0
        };
        if now_s < self.last_call_s + spacing_s {
            return None;
        }
        let check_in = now_s >= self.last_call_s + manager.check_in_s && !self.fleet.is_empty();
        if self.queued_events.is_empty() && !check_in {
            return None;
        }
        Some(std::mem::take(&mut self.queued_events))
    }

    /// The events that just started, by their indices from `due`.
    fn started(&self, due: &[usize]) -> Vec<&Event> {
        due.iter().map(|&index| &self.events[index]).collect()
    }

    /// Calls the fleet manager when events start and at check-ins, and
    /// applies what it orders.
    pub(super) fn manage(
        &mut self,
        context: &mut AgentContext,
        now_s: f64,
        new_events: &[usize],
    ) -> Result<()> {
        let kind = self.config.manager.kind;
        let top_kind = self.config.manager.top_kind;
        if kind == ManagerKind::None && top_kind == ManagerKind::None {
            return Ok(());
        }
        let round = if kind == ManagerKind::Scripted {
            let round = self.scripted_round(now_s);
            if round.orders.is_empty() {
                return Ok(());
            }
            round
        } else {
            let Some(due) = self.due(now_s, new_events) else {
                return Ok(());
            };
            let events = self.started(&due);
            match kind {
                ManagerKind::None => Round {
                    due_events: events.iter().map(|e| e.id).collect(),
                    ..Round::default()
                },
                ManagerKind::Replanner | ManagerKind::Policy => {
                    self.planned_round(now_s, &events, kind)
                }
                ManagerKind::Veto => self.veto_round(now_s, &events)?,
                ManagerKind::Model => self.model_round(now_s, &events)?,
                ManagerKind::Scripted => unreachable!("handled above"),
            }
        };
        if kind.uses_model() {
            // The feedback and the tally start again from this call.
            self.feedback.clear();
            self.tally = Tally::default();
            self.move_costs.clear();
            self.late_by_hub.clear();
            for _ in 0..round.unreadable {
                self.feedback
                    .push("an order could not be read and was dropped".into());
            }
        }
        self.last_call_s = now_s;
        self.record_orders(context, now_s, kind, round.orders)?;
        for call in round.calls {
            context
                .messages
                .publish("GlobalNetwork", CALL_TOPIC, call)?;
        }
        if top_kind != ManagerKind::None {
            self.manage_top(context, now_s, &round.due_events)?;
        }
        Ok(())
    }

    /// The mission's hand-written orders whose minute has come.
    fn scripted_round(&mut self, now_s: f64) -> Round {
        let scripted = &self.config.manager.scripted;
        let mut round = Round::default();
        while self.scripted_fired < scripted.len()
            && scripted[self.scripted_fired].at_min * 60.0 <= now_s
        {
            round.orders.push(Given {
                order: scripted[self.scripted_fired].order.clone(),
                cluster: None,
                quiet: false,
            });
            self.scripted_fired += 1;
        }
        round
    }

    /// The replanner's or the policy's orders, cluster by cluster.
    fn planned_round(&self, now_s: f64, events: &[&Event], kind: ManagerKind) -> Round {
        let mut round = Round {
            due_events: events.iter().map(|e| e.id).collect(),
            ..Round::default()
        };
        for cluster in self.clusters() {
            let snapshot = self.snapshot_of(now_s, cluster.as_deref());
            let mine = events_touching(events, cluster.as_deref());
            let mine: Vec<&Event> = mine.iter().collect();
            let planned = match &self.policy {
                Some(policy) => policy.plan(&mine, &snapshot),
                None => replanner::plan(&mine, &snapshot),
            };
            let quiet = mine.is_empty() && !snapshot.has_exceptions();
            round.calls.push(ManagerCall {
                time_s: now_s,
                source: kind,
                top: false,
                quiet,
                events: mine.iter().map(|e| e.id).collect(),
                prompt_chars: 0,
                reply_chars: 0,
                queries: 0,
                orders: planned.len() as u32,
                dropped: 0,
                parse_failed: false,
            });
            round.orders.extend(planned.into_iter().map(|order| Given {
                order,
                cluster: cluster.clone(),
                quiet,
            }));
        }
        round
    }

    /// The replanner plans and the model drops what it objects to, cluster
    /// by cluster; a cluster with nothing proposed makes no call.
    fn veto_round(&self, now_s: f64, events: &[&Event]) -> Result<Round> {
        let kind = ManagerKind::Veto;
        let mut round = Round {
            due_events: events.iter().map(|e| e.id).collect(),
            ..Round::default()
        };
        for cluster in self.clusters() {
            let snapshot = self.snapshot_of(now_s, cluster.as_deref());
            let mine = events_touching(events, cluster.as_deref());
            let mine: Vec<&Event> = mine.iter().collect();
            let planned = replanner::plan(&mine, &snapshot);
            let quiet = mine.is_empty() && !snapshot.has_exceptions();
            let situation = self.situation(&snapshot, &mine, Some(&planned));
            let model = self.model.as_ref().expect("a model manager");
            let (reply, parse_failed, dropped) = if planned.is_empty() {
                // Nothing to review: no call.
                (String::new(), false, Vec::new())
            } else {
                let reply = model.ask_text(&situation)?;
                match parse_veto(&reply) {
                    Some(drop) => (reply, false, drop),
                    None => (reply, true, Vec::new()),
                }
            };
            let kept: Vec<Order> = planned
                .iter()
                .enumerate()
                .filter(|(i, _)| !dropped.contains(&(i + 1)))
                .map(|(_, o)| o.clone())
                .collect();
            if !planned.is_empty() {
                round.calls.push(ManagerCall {
                    time_s: now_s,
                    source: kind,
                    top: false,
                    quiet,
                    events: mine.iter().map(|e| e.id).collect(),
                    prompt_chars: situation.len(),
                    reply_chars: reply.len(),
                    queries: 0,
                    orders: kept.len() as u32,
                    dropped: (planned.len() - kept.len()) as u32,
                    parse_failed,
                });
            }
            round.orders.extend(kept.into_iter().map(|order| Given {
                order,
                cluster: cluster.clone(),
                quiet,
            }));
        }
        Ok(round)
    }

    /// The model's orders, cluster by cluster.
    fn model_round(&self, now_s: f64, events: &[&Event]) -> Result<Round> {
        let kind = ManagerKind::Model;
        let mut round = Round {
            due_events: events.iter().map(|e| e.id).collect(),
            ..Round::default()
        };
        for cluster in self.clusters() {
            let snapshot = self.snapshot_of(now_s, cluster.as_deref());
            let mine = events_touching(events, cluster.as_deref());
            let mine: Vec<&Event> = mine.iter().collect();
            let situation = self.situation(&snapshot, &mine, None);
            let quiet = mine.is_empty() && !snapshot.has_exceptions();
            let asked: Asked = self
                .model
                .as_ref()
                .expect("a model manager")
                .ask(&situation, &snapshot)?;
            round.calls.push(ManagerCall {
                time_s: now_s,
                source: kind,
                top: false,
                quiet,
                events: mine.iter().map(|e| e.id).collect(),
                prompt_chars: asked.prompt_chars,
                reply_chars: asked.reply_chars,
                queries: asked.queries,
                orders: asked.orders.len() as u32 + asked.unreadable,
                dropped: 0,
                parse_failed: asked.parse_failed,
            });
            round.unreadable += asked.unreadable;
            round
                .orders
                .extend(asked.orders.into_iter().map(|order| Given {
                    order,
                    cluster: cluster.clone(),
                    quiet,
                }));
        }
        Ok(round)
    }

    /// Checks each order against its cluster, applies it, publishes what
    /// became of it, and keeps the feedback for a model's next call.
    fn record_orders(
        &mut self,
        context: &mut AgentContext,
        now_s: f64,
        kind: ManagerKind,
        orders: Vec<Given>,
    ) -> Result<()> {
        for Given {
            order,
            cluster,
            quiet,
        } in orders
        {
            let outside = cluster
                .as_ref()
                .is_some_and(|c| order.hubs_named().iter().any(|h| !c.contains(h)));
            let (outcome, feedback) = if order.is_top() {
                (
                    "refused_top_only".to_string(),
                    "only the manager above the clusters moves between clusters".to_string(),
                )
            } else if outside {
                (
                    "refused_outside_cluster".to_string(),
                    "names a hub outside your cluster".to_string(),
                )
            } else {
                self.apply(context, now_s, &order)?
            };
            let record = OrderRecord {
                time_s: now_s,
                source: kind,
                top: false,
                quiet,
                order: order.clone(),
                outcome: outcome.clone(),
                feedback: feedback.clone(),
            };
            context
                .messages
                .publish("GlobalNetwork", ORDER_TOPIC, record)?;
            if kind.uses_model() {
                self.feedback
                    .push(format!("{}: {outcome}; {feedback}", order.describe()));
            }
            self.changed = true;
        }
        Ok(())
    }

    /// The call above the clusters, after theirs: it sees cluster totals and
    /// may only move vehicles and stock between clusters.
    fn manage_top(
        &mut self,
        context: &mut AgentContext,
        now_s: f64,
        event_ids: &[u32],
    ) -> Result<()> {
        let top_kind = self.config.manager.top_kind;
        let clusters = self.region.clusters(self.config.manager.cluster_side);
        let snapshot = self.snapshot_of(now_s, None);
        let events: Vec<&Event> = self
            .events
            .iter()
            .filter(|e| event_ids.contains(&e.id))
            .collect();
        let quiet = events.is_empty() && !snapshot.has_exceptions();
        let (orders, call) = match top_kind {
            ManagerKind::Replanner => {
                let planned = replanner::plan_top(&events, &snapshot, &clusters);
                let call = ManagerCall {
                    time_s: now_s,
                    source: top_kind,
                    top: true,
                    quiet,
                    events: event_ids.to_vec(),
                    prompt_chars: 0,
                    reply_chars: 0,
                    queries: 0,
                    orders: planned.len() as u32,
                    dropped: 0,
                    parse_failed: false,
                };
                (planned, call)
            }
            ManagerKind::Model => {
                let situation = self.top_situation(&snapshot, &events, &clusters);
                let asked: Asked = self
                    .top_model
                    .as_ref()
                    .expect("a top model")
                    .ask(&situation, &snapshot)?;
                let call = ManagerCall {
                    time_s: now_s,
                    source: top_kind,
                    top: true,
                    quiet,
                    events: event_ids.to_vec(),
                    prompt_chars: asked.prompt_chars,
                    reply_chars: asked.reply_chars,
                    queries: 0,
                    orders: asked.orders.len() as u32 + asked.unreadable,
                    dropped: 0,
                    parse_failed: asked.parse_failed,
                };
                self.top_feedback.clear();
                for _ in 0..asked.unreadable {
                    self.top_feedback
                        .push("an order could not be read and was dropped".into());
                }
                (asked.orders, call)
            }
            _ => unreachable!("checked in configure"),
        };
        for order in orders {
            let (outcome, feedback) = if order.is_top() {
                self.apply(context, now_s, &order)?
            } else {
                (
                    "refused_moves_only".to_string(),
                    "the manager above the clusters only moves vehicles and stock between clusters"
                        .to_string(),
                )
            };
            let record = OrderRecord {
                time_s: now_s,
                source: top_kind,
                top: true,
                quiet,
                order: order.clone(),
                outcome: outcome.clone(),
                feedback: feedback.clone(),
            };
            context
                .messages
                .publish("GlobalNetwork", ORDER_TOPIC, record)?;
            if top_kind == ManagerKind::Model {
                self.top_feedback
                    .push(format!("{}: {outcome}; {feedback}", order.describe()));
            }
            self.changed = true;
        }
        context
            .messages
            .publish("GlobalNetwork", CALL_TOPIC, call)?;
        Ok(())
    }
}

/// The events a cluster's manager is told about: those naming one of its
/// hubs, with the hubs outside the cluster left out, and those naming none
/// (an area only).
fn events_touching(events: &[&Event], cluster: Option<&[u32]>) -> Vec<Event> {
    events
        .iter()
        .filter(|e| {
            cluster.is_none_or(|c| e.hubs.is_empty() || e.hubs.iter().any(|h| c.contains(h)))
        })
        .map(|e| {
            let mut event = (*e).clone();
            if let Some(cluster) = cluster {
                event.hubs.retain(|h| cluster.contains(h));
            }
            event
        })
        .collect()
}

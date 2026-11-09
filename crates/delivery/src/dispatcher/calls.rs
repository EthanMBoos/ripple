//! When each fleet manager is called and with what, and what becomes of its
//! orders. A round calls every manager, one over the region or one per
//! cluster, then records and applies the orders, then calls the manager
//! above the clusters if there is one.

use anyhow::Result;
use ripple_core::plugin::AgentContext;

use super::Dispatcher;
use crate::event::{Event, EventsConfig};
use crate::messages::{CALL_TOPIC, ManagerCall, ManagerKind, ORDER_TOPIC, OrderRecord};
use crate::model::Asked;
use crate::order::{Order, parse_veto};
use crate::region::Region;
use crate::replanner;

/// An order as a manager gave it: from which cluster, if any (its position
/// among the clusters and its hubs; none for a manager over the whole
/// region), and whether the call had nothing new to act on.
pub(super) struct Given {
    pub order: Order,
    pub index: usize,
    pub cluster: Option<Vec<u32>>,
    pub quiet: bool,
}

/// One round of calls: the orders given, the calls made, the events of the
/// round for the manager above the clusters, and how many orders in each
/// cluster's model reply did not read.
#[derive(Default)]
struct Round {
    orders: Vec<Given>,
    calls: Vec<ManagerCall>,
    due_events: Vec<u32>,
    unreadable: Vec<(usize, u32)>,
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
            self.tallies.clear();
            self.move_costs.clear();
            self.late_by_hub.clear();
            for (index, count) in &round.unreadable {
                let lines = self.feedback.entry(*index).or_default();
                for _ in 0..*count {
                    lines.push("an order could not be read and was dropped".into());
                }
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
                index: 0,
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
        for (index, cluster) in self.clusters().into_iter().enumerate() {
            let snapshot = self.snapshot_of(now_s, cluster.as_deref());
            let mine = events_touching(events, cluster.as_deref(), &self.config.events);
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
                index,
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
        for (index, cluster) in self.clusters().into_iter().enumerate() {
            let snapshot = self.snapshot_of(now_s, cluster.as_deref());
            let mine = events_touching(events, cluster.as_deref(), &self.config.events);
            let mine: Vec<&Event> = mine.iter().collect();
            let planned = replanner::plan(&mine, &snapshot);
            let quiet = mine.is_empty() && !snapshot.has_exceptions();
            let situation = self.situation(index, &snapshot, &mine, Some(&planned));
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
                index,
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
        for (index, cluster) in self.clusters().into_iter().enumerate() {
            let snapshot = self.snapshot_of(now_s, cluster.as_deref());
            let mine = events_touching(events, cluster.as_deref(), &self.config.events);
            let mine: Vec<&Event> = mine.iter().collect();
            let situation = self.situation(index, &snapshot, &mine, None);
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
            round.unreadable.push((index, asked.unreadable));
            round
                .orders
                .extend(asked.orders.into_iter().map(|order| Given {
                    order,
                    index,
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
            index,
            cluster,
            quiet,
        } in orders
        {
            let outside = cluster
                .as_deref()
                .is_some_and(|c| names_outside(&order, c, &self.region));
            let (outcome, feedback) = if order.is_top() {
                (
                    "refused_top_only".to_string(),
                    "only the manager above the clusters moves between clusters".to_string(),
                )
            } else if outside {
                (
                    "refused_outside_cluster".to_string(),
                    "names a hub or site outside your cluster".to_string(),
                )
            } else {
                self.apply(context, now_s, &order, cluster.as_deref())?
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
                    .entry(index)
                    .or_default()
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
                self.apply(context, now_s, &order, None)?
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

/// Whether an order names a hub or a site outside the cluster. A site is
/// placed by its hub; a site that does not exist is left for `apply` to
/// refuse by name.
fn names_outside(order: &Order, cluster: &[u32], region: &Region) -> bool {
    order.hubs_named().iter().any(|h| !cluster.contains(h))
        || order
            .sites_named()
            .iter()
            .filter_map(|code| region.site(code))
            .any(|site| !cluster.contains(&site.hub))
}

/// The events a cluster's manager is told about: those naming one of its
/// hubs, with the hubs outside the cluster left out and the text written
/// again for the shorter list, and those naming none (an area only).
pub(super) fn events_touching(
    events: &[&Event],
    cluster: Option<&[u32]>,
    texts: &EventsConfig,
) -> Vec<Event> {
    events
        .iter()
        .filter(|e| {
            cluster.is_none_or(|c| e.hubs.is_empty() || e.hubs.iter().any(|h| c.contains(h)))
        })
        .map(|e| {
            let mut event = (*e).clone();
            if let Some(cluster) = cluster {
                let before = event.hubs.len();
                event.hubs.retain(|h| cluster.contains(h));
                if event.hubs.len() < before {
                    event.text = texts.text(&event);
                }
            }
            event
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::event::EventKind;
    use crate::messages::Urgency;
    use crate::region::RegionConfig;

    fn four_hubs() -> Region {
        Region::new(&RegionConfig {
            hubs: 4,
            hub_spacing_m: 15_000.0,
            ..RegionConfig::default()
        })
        .unwrap()
    }

    #[test]
    fn a_trimmed_events_text_names_only_the_clusters_hubs() {
        let texts = EventsConfig {
            text: BTreeMap::from([(
                EventKind::Surge,
                "A surge near {hubs} from minute {from}.".into(),
            )]),
            ..EventsConfig::default()
        };
        let mut surge = Event {
            id: 1,
            kind: EventKind::Surge,
            at_min: 10.0,
            until_min: 70.0,
            hubs: vec![1, 2, 3],
            ..Event::default()
        };
        surge.text = texts.text(&surge);
        let corridor = Event {
            id: 2,
            kind: EventKind::Corridor,
            at_min: 20.0,
            until_min: 80.0,
            text: "a corridor".into(),
            ..Event::default()
        };
        let all = [&surge, &corridor];
        let mine = events_touching(&all, Some(&[1, 2]), &texts);
        assert_eq!(mine.len(), 2);
        assert_eq!(mine[0].hubs, vec![1, 2]);
        assert_eq!(mine[0].text, "A surge near hubs 1, 2 from minute 10.");
        assert_eq!(
            mine[1].text, "a corridor",
            "an event naming no hub is shared"
        );
        let theirs = events_touching(&all, Some(&[4]), &texts);
        assert_eq!(theirs.len(), 1, "only the corridor reaches hub 4's cluster");
        let flat = events_touching(&all, None, &texts);
        assert_eq!(
            flat[0].text, surge.text,
            "a flat manager's text is unchanged"
        );
    }

    #[test]
    fn a_site_outside_the_cluster_is_outside() {
        let region = four_hubs();
        let cluster = [1, 2];
        let inside = Order::Reserve {
            hub: 1,
            count: 1,
            site: "1H1".into(),
            until_min: 60.0,
        };
        let pad = Order::UsePad {
            site: "3H1".into(),
            until_min: 60.0,
        };
        let lend = Order::ServeFrom {
            site: "1H1".into(),
            hubs: vec![3],
        };
        let trauma = Order::Prioritize {
            site: None,
            urgency: Some(Urgency::Trauma),
            until_min: 60.0,
        };
        let unknown = Order::UsePad {
            site: "9H9".into(),
            until_min: 60.0,
        };
        assert!(!names_outside(&inside, &cluster, &region));
        assert!(names_outside(&pad, &cluster, &region));
        assert!(names_outside(&lend, &cluster, &region));
        assert!(
            !names_outside(&trauma, &cluster, &region),
            "scoped when applied"
        );
        assert!(
            !names_outside(&unknown, &cluster, &region),
            "refused by name later"
        );
    }
}

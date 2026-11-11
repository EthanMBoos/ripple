//! The fleet managers: who they are, when they are called, with what, and
//! what becomes of their orders. Every manager is a `Manager`: a scope, a
//! way of deciding, and what it was last told. A call goes to one tier at a
//! time, the managers over the region or over each cluster first, then the
//! help manager. A tier's orders are recorded and applied once
//! all of its managers have decided, so they decide on the same state.

use anyhow::Result;
use ripple_core::plugin::AgentContext;

use super::Dispatcher;
use crate::event::{Event, EventKind};
use crate::help::{self, HelpRequest};
use crate::messages::{CALL_TOPIC, ManagerCall, ManagerKind, ORDER_TOPIC, OrderRecord};
use crate::model::ModelManager;
use crate::order::{Order, Outcome, parse_veto};
use crate::policy::Policy;
use crate::replanner;
use crate::scope::Scope;

/// How a manager decides what to order.
#[derive(Clone)]
pub(super) enum Decider {
    /// Fixed rules reading the events (`replanner.rs`).
    Replanner,
    /// Rules written once as data (`policy.rs`).
    Policy(Policy),
    /// A language model reading the prompt (`prompt.rs`, `model.rs`).
    Model(ModelManager),
    /// The replanner plans and the model drops what it objects to.
    Veto(ModelManager),
}

impl Decider {
    pub fn kind(&self) -> ManagerKind {
        match self {
            Decider::Replanner => ManagerKind::Replanner,
            Decider::Policy(_) => ManagerKind::Policy,
            Decider::Model(_) => ManagerKind::Model,
            Decider::Veto(_) => ManagerKind::Veto,
        }
    }
}

/// One fleet manager: what it is responsible for, how it decides, and what
/// it is told about its last orders at its next call. The help manager is
/// one too: its scope is the region, it is shown the help asked for and
/// offered with the hub view, and it only grants help.
pub(super) struct Manager {
    pub scope: Scope,
    pub decider: Decider,
    pub above: bool,
    pub feedback: Vec<String>,
}

/// An order as it was given: by which manager (its position in `managers`;
/// none for an order scripted in the mission, which no scope limits), and
/// whether the call had nothing new to act on.
struct Given {
    order: Order,
    manager: Option<usize>,
    quiet: bool,
}

/// What one tier decided this call, before any of it is applied: the orders
/// given, the calls made, and for each manager that asked a model, how many
/// orders in its reply did not read.
#[derive(Default)]
struct Round {
    orders: Vec<Given>,
    calls: Vec<ManagerCall>,
    replied: Vec<(usize, u32)>,
}

/// One manager's decision.
struct Decided {
    orders: Vec<Order>,
    /// None when the manager had nothing to decide and made no call.
    call: Option<ManagerCall>,
    unreadable: u32,
    quiet: bool,
}

impl Dispatcher {
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

    /// Calls the fleet managers when events start and at check-ins, and
    /// applies what they order: first the managers over the region or the
    /// clusters, then the help manager.
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
        let due: Vec<usize>;
        let round = if kind == ManagerKind::Scripted {
            let round = self.scripted_round(now_s);
            if round.orders.is_empty() {
                return Ok(());
            }
            due = Vec::new();
            round
        } else {
            let Some(queued) = self.due(now_s, new_events) else {
                return Ok(());
            };
            due = queued;
            if top_kind != ManagerKind::None {
                self.ask_for_help(now_s, &due);
            }
            self.decide_tier(now_s, &self.started(&due), false)?
        };
        if kind.uses_model() {
            // What is reported since the last call starts again from it.
            self.tallies.clear();
            self.move_costs.clear();
            self.late_by_hub.clear();
        }
        self.last_call_s = now_s;
        self.record(context, now_s, round)?;
        if top_kind != ManagerKind::None {
            let round = self.decide_tier(now_s, &self.started(&due), true)?;
            self.record(context, now_s, round)?;
        }
        Ok(())
    }

    /// Raises a help request for each hub where a surge or an outage just
    /// started, until the event ends, and drops the requests that have
    /// ended. A hub asks once; a granted request is dropped when applied.
    /// Raised before the first tier decides, so a help manager over the
    /// region sees them in the same call.
    fn ask_for_help(&mut self, now_s: f64, due: &[usize]) {
        self.help_requests.retain(|r| r.until_s > now_s);
        let day_end_s = self.config.requests.until_s + 3600.0;
        let mut asked = Vec::new();
        for event in self.started(due) {
            if !matches!(event.kind, EventKind::Surge | EventKind::Outage) {
                continue;
            }
            let until_s = if event.until_min.is_finite() {
                event.until_min * 60.0
            } else {
                day_end_s
            };
            for &hub in &event.hubs {
                asked.push(HelpRequest {
                    hub,
                    kind: event.kind,
                    until_s,
                });
            }
        }
        for request in asked {
            if !self.help_requests.iter().any(|r| r.hub == request.hub) {
                self.help_requests.push(request);
            }
        }
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
                manager: None,
                quiet: false,
            });
            self.scripted_fired += 1;
        }
        round
    }

    /// Every manager of one tier decides, each on the same state.
    fn decide_tier(&self, now_s: f64, events: &[&Event], above: bool) -> Result<Round> {
        let mut round = Round::default();
        for (index, manager) in self.managers.iter().enumerate() {
            if manager.above != above {
                continue;
            }
            let decided = self.decide(manager, now_s, events)?;
            round.calls.extend(decided.call);
            if manager.decider.kind().uses_model() {
                round.replied.push((index, decided.unreadable));
            }
            round
                .orders
                .extend(decided.orders.into_iter().map(|order| Given {
                    order,
                    manager: Some(index),
                    quiet: decided.quiet,
                }));
        }
        Ok(round)
    }

    /// One manager's decision: its scope's share of the events and view,
    /// then its orders by its decider.
    fn decide(&self, manager: &Manager, now_s: f64, events: &[&Event]) -> Result<Decided> {
        let snapshot = self.snapshot_of(now_s, &manager.scope);
        let mine = manager.scope.events_shown(events, &self.config.events);
        let mine: Vec<&Event> = mine.iter().collect();
        let quiet = mine.is_empty() && !snapshot.has_exceptions();
        let mut call = ManagerCall {
            time_s: now_s,
            source: manager.decider.kind(),
            top: manager.above,
            quiet,
            events: mine.iter().map(|e| e.id).collect(),
            prompt_chars: 0,
            reply_chars: 0,
            orders: 0,
            dropped: 0,
            parse_failed: false,
        };
        let mut unreadable = 0;
        let orders = match &manager.decider {
            Decider::Replanner if manager.above => help::broker(&self.help_requests, &snapshot),
            Decider::Replanner => replanner::plan(&mine, &snapshot),
            Decider::Policy(policy) => policy.plan(&mine, &snapshot),
            Decider::Model(model) => {
                let situation = if manager.above {
                    self.top_situation(manager, &snapshot, &mine)
                } else {
                    self.situation(manager, &snapshot, &mine, None)
                };
                let asked = model.ask(&situation)?;
                call.prompt_chars = asked.prompt_chars;
                call.reply_chars = asked.reply_chars;
                call.parse_failed = asked.parse_failed;
                unreadable = asked.unreadable;
                asked.orders
            }
            Decider::Veto(model) => {
                let planned = replanner::plan(&mine, &snapshot);
                if planned.is_empty() {
                    // Nothing to review: no call.
                    return Ok(Decided {
                        orders: Vec::new(),
                        call: None,
                        unreadable: 0,
                        quiet,
                    });
                }
                let situation = self.situation(manager, &snapshot, &mine, Some(&planned));
                let reply = model.ask_text(&situation)?;
                let dropped = parse_veto(&reply).unwrap_or_else(|| {
                    call.parse_failed = true;
                    Vec::new()
                });
                let kept: Vec<Order> = planned
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| !dropped.contains(&(i + 1)))
                    .map(|(_, o)| o.clone())
                    .collect();
                call.prompt_chars = situation.len();
                call.reply_chars = reply.len();
                call.dropped = (planned.len() - kept.len()) as u32;
                kept
            }
        };
        call.orders = orders.len() as u32 + unreadable;
        Ok(Decided {
            orders,
            call: Some(call),
            unreadable,
            quiet,
        })
    }

    /// Applies a tier's round: each order is checked against its giver's
    /// scope and tier, applied, and published with what became of it, and a
    /// model manager is told about its orders at its next call.
    fn record(&mut self, context: &mut AgentContext, now_s: f64, round: Round) -> Result<()> {
        for (index, unreadable) in round.replied {
            let feedback = &mut self.managers[index].feedback;
            feedback.clear();
            for _ in 0..unreadable {
                feedback.push("an order could not be read and was dropped".into());
            }
        }
        for Given {
            order,
            manager,
            quiet,
        } in round.orders
        {
            let (scope, above, kind) = match manager {
                Some(index) => {
                    let who = &self.managers[index];
                    (who.scope.clone(), who.above, who.decider.kind())
                }
                None => (Scope::Region, false, ManagerKind::Scripted),
            };
            let (outcome, feedback) = if order.is_top() && !above {
                (
                    Outcome::Refused("top_only"),
                    "only the help manager grants help".to_string(),
                )
            } else if above && !order.is_top() {
                (
                    Outcome::Refused("help_only"),
                    "the help manager only grants help, a hub serving another hub's sites for a time".to_string(),
                )
            } else if !scope.covers_order(&order, &self.region) {
                (
                    Outcome::Refused("outside_cluster"),
                    "names a hub or site outside your cluster".to_string(),
                )
            } else {
                self.apply(context, now_s, &order, &scope)?
            };
            let moved = std::mem::take(&mut self.moved);
            let record = OrderRecord {
                time_s: now_s,
                source: kind,
                top: above,
                quiet,
                order: order.clone(),
                outcome,
                moved,
                feedback: feedback.clone(),
            };
            context
                .messages
                .publish("GlobalNetwork", ORDER_TOPIC, record)?;
            if kind.uses_model()
                && let Some(index) = manager
            {
                self.managers[index]
                    .feedback
                    .push(format!("{}: {outcome}; {feedback}", order.describe()));
            }
            self.changed = true;
        }
        for call in round.calls {
            context
                .messages
                .publish("GlobalNetwork", CALL_TOPIC, call)?;
        }
        Ok(())
    }
}

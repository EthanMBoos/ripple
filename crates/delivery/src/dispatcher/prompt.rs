//! What a fleet manager is shown: the snapshot of the fleet at one call and
//! the prompt written from it, for a manager over its hubs and for the
//! manager above the clusters. The words are fixed so a saved reply matches
//! a rerun (`model.rs`).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::{Dispatcher, Manager, Tally};
use crate::event::{Event, hub_list};
use crate::help;
use crate::messages::{Phase, RequestKind, VehicleClass};
use crate::order::{HELP_SYNTAX, Order, SYNTAX, VETO_SYNTAX};
use crate::scope::Scope;
use crate::view::{HubView, Snapshot};

impl Dispatcher {
    /// The state at one call, limited to the hubs the scope covers.
    pub(super) fn snapshot_of(&self, now_s: f64, scope: &Scope) -> Snapshot<'_> {
        let waiting = self.waiting();
        let mut hubs: Vec<HubView> = self
            .region
            .hubs
            .iter()
            .filter(|hub| scope.covers_hub(hub.id))
            .map(|hub| HubView {
                id: hub.id,
                position_world_m: hub.position_world_m,
                stock: self.stock.get(&hub.id).cloned().unwrap_or_default(),
                out: self.in_outage(hub.id, now_s),
                ..HubView::default()
            })
            .collect();
        for vehicle in self.fleet.values() {
            let Some(view) = hubs.iter_mut().find(|h| h.id == vehicle.hub) else {
                continue;
            };
            *view.total.entry(vehicle.card.class).or_insert(0) += 1;
            match vehicle.phase {
                Phase::Idle
                    if vehicle.free()
                        && !self.pending_relocations.contains_key(&vehicle.vehicle_id) =>
                {
                    *view.idle.entry(vehicle.card.class).or_insert(0) += 1;
                }
                Phase::Lost => view.lost += 1,
                Phase::Relocating => view.incoming += 1,
                _ => view.flying += 1,
            }
        }
        for id in &self.open {
            let request = &self.requests[id];
            if let Some(to_hub) = request.to_hub
                && let Some(view) = hubs.iter_mut().find(|h| h.id == to_hub)
            {
                *view.arriving.entry(request.item.clone()).or_insert(0) += request.units;
            }
        }
        for request in &waiting {
            if request.kind != RequestKind::Delivery {
                continue;
            }
            if let Some(view) = hubs.iter_mut().find(|h| h.id == request.hub) {
                *view.waiting.entry(request.urgency).or_insert(0) += 1;
                if request.deadline_s < now_s {
                    view.late += 1;
                }
            }
        }
        let active: Vec<&Event> = self.events.iter().filter(|e| e.active(now_s)).collect();
        let mut cards = BTreeMap::new();
        for vehicle in self.fleet.values() {
            cards.entry(vehicle.card.class).or_insert(vehicle.card);
        }
        Snapshot {
            time_s: now_s,
            day_s: self.config.requests.until_s,
            region: &self.region,
            hubs,
            cards,
            range_margin: self.config.range_margin,
            active_events: scope.events_shown(&active, &self.config.events),
            items: &self.config.stock.items,
            units_per_item: self.config.stock.units_per_item,
        }
    }

    /// What happened since the last call at the hubs the scope covers.
    fn tally_for(&self, scope: &Scope) -> Tally {
        let mut total = Tally::default();
        for (hub, tally) in &self.tallies {
            if !scope.covers_hub(*hub) {
                continue;
            }
            total.delivered += tally.delivered;
            total.on_time += tally.on_time;
            total.late += tally.late;
            total.lost += tally.lost;
            for (reason, count) in &tally.missed {
                *total.missed.entry(*reason).or_insert(0) += count;
            }
        }
        total
    }

    /// The prompt for one manager's call, everything the model reads: the
    /// time, the events, the feedback and tally for its scope, the fleet
    /// view, the rules in force that reach it, and the orders available.
    /// With `proposed`, the replanner's orders are listed for the veto
    /// manager instead of the order syntax.
    pub(super) fn situation(
        &self,
        who: &Manager,
        snapshot: &Snapshot<'_>,
        new_events: &[&Event],
        proposed: Option<&[Order]>,
    ) -> String {
        let manager = &self.config.manager;
        let scope = &who.scope;
        let minute = |s: f64| format!("{:.0}", s / 60.0);
        let mut text = String::new();
        let _ = writeln!(
            text,
            "Time: minute {} of the day; requests stop coming at minute {}.",
            minute(snapshot.time_s),
            minute(snapshot.day_s)
        );
        if let Scope::Cluster(hubs) = scope {
            let _ = writeln!(
                text,
                "You manage hubs {} only, one cluster of the region; the other clusters \
                 have their own fleet manager. An order naming a hub or site outside yours \
                 is refused, and your avoid and priority orders reach only your own hubs' \
                 flights and requests.",
                hub_list(hubs)
            );
        }
        let event_line = |event: &Event| event.text.clone();
        if new_events.is_empty() {
            text.push_str("\nNo new events; this is a check-in.\n");
        } else {
            text.push_str("\nNew events:\n");
            for event in new_events {
                let _ = writeln!(text, "- {}", event_line(event));
            }
        }
        let ongoing: Vec<&Event> = snapshot
            .active_events
            .iter()
            .filter(|e| !new_events.iter().any(|n| n.id == e.id))
            .collect();
        if !ongoing.is_empty() {
            text.push_str("\nEvents still in force:\n");
            for event in ongoing {
                let _ = writeln!(text, "- {}", event_line(event));
            }
        }
        if manager.feedback {
            if !who.feedback.is_empty() {
                text.push_str("\nYour last orders:\n");
                for line in &who.feedback {
                    let _ = writeln!(text, "- {line}");
                }
            }
            let tally = self.tally_for(scope);
            let mut since = format!(
                "\nSince your last call: {} delivered ({} on time, {} late)",
                tally.delivered, tally.on_time, tally.late
            );
            for (reason, count) in &tally.missed {
                let _ = write!(
                    since,
                    ", {count} missed for {}",
                    reason.as_str().replace('_', " ")
                );
            }
            if tally.lost > 0 {
                let _ = write!(since, ", {} vehicles lost to endurance", tally.lost);
            }
            text.push_str(&since);
            text.push_str(".\n");
            if manager.cost_feedback {
                let lines = self.cost_lines(scope);
                if !lines.is_empty() {
                    text.push_str("\nAt the hubs your moves took from:\n");
                    for line in lines {
                        let _ = writeln!(text, "- {line}");
                    }
                }
            }
        }
        text.push('\n');
        text.push_str(&snapshot.write(manager.view));
        let rules = self.rules.shown_to(scope, &self.region);
        if !rules.is_empty() {
            text.push_str("\nOrders in force:\n");
            for line in rules {
                let _ = writeln!(text, "- {line}");
            }
        }
        text.push('\n');
        if let Some(proposed) = proposed {
            text.push_str("Proposed orders from the dispatcher's planner:\n");
            for (i, order) in proposed.iter().enumerate() {
                let _ = writeln!(text, "{}. {}", i + 1, order.describe());
            }
            text.push('\n');
            text.push_str(VETO_SYNTAX);
            text.push('\n');
            return text;
        }
        text.push_str(SYNTAX);
        text.push('\n');
        text.push_str(
            "\nReply with {\"orders\": [...]} and nothing else; {\"orders\": []} changes nothing.\n",
        );
        text
    }

    /// The prompt for the manager above the clusters: the time, the events,
    /// its feedback, one line per cluster, and its two orders.
    pub(super) fn top_situation(
        &self,
        who: &Manager,
        snapshot: &Snapshot<'_>,
        new_events: &[&Event],
        clusters: &[Vec<u32>],
    ) -> String {
        let manager = &self.config.manager;
        let minute = |s: f64| format!("{:.0}", s / 60.0);
        let mut text = String::new();
        let _ = writeln!(
            text,
            "Time: minute {} of the day; requests stop coming at minute {}.",
            minute(snapshot.time_s),
            minute(snapshot.day_s)
        );
        let _ = writeln!(
            text,
            "You are the fleet manager above {} cluster managers, each running its own hubs' \
             flights and local rules. You see each cluster's totals and its surges, outages, \
             shortages, and weather, and the help its hubs ask for with the hubs outside the \
             cluster that could give it. You may only grant help: a hub serves another hub's \
             sites for a time, and its aircraft fly each request and return home.",
            clusters.len(),
        );
        let event_line = |event: &Event| event.text.clone();
        // Only the events the manager above acts on, and the events still in
        // force are not listed again: each cluster's line below carries its
        // live ones with their hubs, item, and class. On the 256-hub pilot of
        // 2026-11-10 the full list of every kind ran to 25,000 characters by
        // minute 400, more than half the prompt, and priorities, closures,
        // and rooftop closures filled most of the cluster lines.
        let new_events: Vec<&&Event> = new_events.iter().filter(|e| e.kind.shown_above()).collect();
        if new_events.is_empty() {
            text.push_str("\nNo new surge, outage, shortage, or weather since your last call.\n");
        } else {
            text.push_str("\nNew events:\n");
            for event in new_events {
                let _ = writeln!(text, "- {}", event_line(event));
            }
        }
        if manager.feedback && !who.feedback.is_empty() {
            text.push_str("\nYour last orders:\n");
            for line in &who.feedback {
                let _ = writeln!(text, "- {line}");
            }
        }
        text.push('\n');
        text.push_str(&help::describe(&self.help_requests, snapshot, clusters));
        text.push('\n');
        text.push_str(&snapshot.clusters(clusters));
        text.push('\n');
        text.push_str(HELP_SYNTAX);
        text.push_str(
            "\n\nReply with {\"orders\": [...]} and nothing else; {\"orders\": []} changes nothing.\n",
        );
        text
    }

    /// One line per move since the last call from a donor hub the scope
    /// covers: the hub's idle vehicles now and its late deliveries since.
    pub(super) fn cost_lines(&self, scope: &Scope) -> Vec<String> {
        self.move_costs
            .iter()
            .filter(|(_, donor)| scope.covers_hub(*donor))
            .map(|(words, donor)| {
                let idle = |class: VehicleClass| {
                    self.fleet
                        .values()
                        .filter(|v| v.hub == *donor && v.card.class == class && v.free())
                        .count()
                };
                format!(
                    "{words}: hub {donor} now has {} fixed-wing and {} multirotor idle, {} of its deliveries late since",
                    idle(VehicleClass::FixedWing),
                    idle(VehicleClass::Multirotor),
                    self.late_by_hub.get(donor).copied().unwrap_or(0)
                )
            })
            .collect()
    }
}

//! What a fleet manager is shown: the snapshot of the fleet at one call and
//! the prompt written from it, for a manager over its hubs and for the
//! manager above the clusters. The words are fixed so a saved reply matches
//! a rerun (`model.rs`).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::calls::events_touching;
use super::config::EventsAs;
use super::{Dispatcher, Tally};
use crate::event::{Event, hub_list};
use crate::messages::{Phase, RequestKind, VehicleClass};
use crate::order::{Order, QUERY_SYNTAX, SYNTAX, TOP_SYNTAX, VETO_SYNTAX};
use crate::view::{HubView, Snapshot, ViewKind};

impl Dispatcher {
    /// The state at one call, limited to a cluster's hubs when given one.
    pub(super) fn snapshot_of(&self, now_s: f64, cluster: Option<&[u32]>) -> Snapshot<'_> {
        let waiting = self.waiting();
        let mut hubs: Vec<HubView> = self
            .region
            .hubs
            .iter()
            .filter(|hub| cluster.is_none_or(|c| c.contains(&hub.id)))
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
            vehicles: self
                .fleet
                .values()
                .filter(|v| cluster.is_none_or(|c| c.contains(&v.hub)))
                .collect(),
            active_events: events_touching(&active, cluster, &self.config.events),
            items: &self.config.stock.items,
            units_per_item: self.config.stock.units_per_item,
            cluster: cluster.map(<[u32]>::to_vec),
        }
    }

    /// The prompt for one call: the time, the events, the feedback, the fleet
    /// view, the rules in force, and the orders available. With `proposed`,
    /// the replanner's orders are listed for the veto manager instead of the
    /// order syntax.
    /// What happened since the last call at the hubs in view.
    fn tally_for(&self, cluster: Option<&[u32]>) -> Tally {
        let mut total = Tally::default();
        for (hub, tally) in &self.tallies {
            if cluster.is_some_and(|c| !c.contains(hub)) {
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

    pub(super) fn situation(
        &self,
        index: usize,
        snapshot: &Snapshot<'_>,
        new_events: &[&Event],
        proposed: Option<&[Order]>,
    ) -> String {
        let manager = &self.config.manager;
        let cluster = snapshot.cluster.as_deref();
        let minute = |s: f64| format!("{:.0}", s / 60.0);
        let mut text = String::new();
        let _ = writeln!(
            text,
            "Time: minute {} of the day; requests stop coming at minute {}.",
            minute(snapshot.time_s),
            minute(snapshot.day_s)
        );
        if let Some(cluster) = &snapshot.cluster {
            let _ = writeln!(
                text,
                "You manage hubs {} only, one cluster of the region; the other clusters \
                 have their own fleet manager, and an order naming a hub or site outside \
                 yours is refused.",
                hub_list(cluster)
            );
        }
        let event_line = |event: &Event| match manager.events_as {
            EventsAs::Text => event.text.clone(),
            EventsAs::Structured => event.structured(),
        };
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
            if let Some(lines) = self.feedback.get(&index).filter(|l| !l.is_empty()) {
                text.push_str("\nYour last orders:\n");
                for line in lines {
                    let _ = writeln!(text, "- {line}");
                }
            }
            let tally = self.tally_for(cluster);
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
                let lines = self.cost_lines(cluster);
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
        let site_hub = |code: &str| self.region.site(code).map(|site| site.hub);
        let rules = self.rules.describe_within(cluster, &site_hub);
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
        if manager.view == ViewKind::Queries {
            text.push_str(QUERY_SYNTAX);
            text.push('\n');
        }
        text.push_str(
            "\nReply with {\"orders\": [...]} and nothing else; {\"orders\": []} changes nothing.\n",
        );
        text
    }

    /// The prompt for the manager above the clusters.
    pub(super) fn top_situation(
        &self,
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
             flights and local rules. You see each cluster's totals, and you may only move \
             vehicles and stock between clusters.",
            clusters.len()
        );
        let event_line = |event: &Event| match manager.events_as {
            EventsAs::Text => event.text.clone(),
            EventsAs::Structured => event.structured(),
        };
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
        if manager.feedback && !self.top_feedback.is_empty() {
            text.push_str("\nYour last orders:\n");
            for line in &self.top_feedback {
                let _ = writeln!(text, "- {line}");
            }
            if manager.cost_feedback && !self.move_costs.is_empty() {
                text.push_str("\nAt the hubs your moves took from:\n");
                for line in self.cost_lines(None) {
                    let _ = writeln!(text, "- {line}");
                }
            }
        }
        text.push('\n');
        text.push_str(&snapshot.clusters(clusters));
        text.push('\n');
        text.push_str(TOP_SYNTAX);
        text.push_str(
            "\n\nReply with {\"orders\": [...]} and nothing else; {\"orders\": []} changes nothing.\n",
        );
        text
    }

    /// One line per move since the last call: the donor hub's idle vehicles
    /// now and its late deliveries since.
    pub(super) fn cost_lines(&self, cluster: Option<&[u32]>) -> Vec<String> {
        self.move_costs
            .iter()
            .filter(|(_, donor)| cluster.is_none_or(|c| c.contains(donor)))
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

//! What the fleet manager sees of the fleet: a snapshot the dispatcher builds
//! at each call, and the three ways of writing it out. The full list names
//! every vehicle; the summaries give one line per hub; the queries give the
//! hub list and answer questions. The words around them are the same for
//! every interface.
//!
//! Distances are kilometers and times minutes, rounded, so the prompt depends
//! only on the state and a saved reply matches a rerun.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use ripple_core::Vec3;
use serde::Deserialize;

use crate::event::Event;
use crate::messages::{Phase, Status, Urgency, VehicleCard, VehicleClass};
use crate::order::Query;
use crate::region::{Region, horizontal_m};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewKind {
    FullList,
    Summaries,
    Queries,
    /// Only what is going wrong: late or stranded requests, low stock,
    /// outages, lost vehicles.
    Exceptions,
}

/// One hub as the fleet manager may see it.
#[derive(Clone, Debug, Default)]
pub struct HubView {
    pub id: u32,
    pub position_world_m: Vec3,
    pub idle: BTreeMap<VehicleClass, u32>,
    pub total: BTreeMap<VehicleClass, u32>,
    /// Vehicles flying for this hub, or relocating to it.
    pub flying: u32,
    pub incoming: u32,
    pub lost: u32,
    pub stock: BTreeMap<String, u32>,
    /// Units of each item on transfer flights to this hub, ordered but not
    /// yet landed.
    pub arriving: BTreeMap<String, u32>,
    pub waiting: BTreeMap<Urgency, u32>,
    pub late: u32,
    /// In an outage now.
    pub out: bool,
}

/// One cluster's totals, as the manager above the clusters sees it.
#[derive(Clone, Debug, Default)]
pub struct ClusterView {
    /// 1-based, in the order `Region::clusters` gives.
    pub index: u32,
    pub hubs: Vec<u32>,
    pub idle: BTreeMap<VehicleClass, u32>,
    pub total: BTreeMap<VehicleClass, u32>,
    pub flying: u32,
    pub lost: u32,
    pub stock: BTreeMap<String, u32>,
    pub waiting: BTreeMap<Urgency, u32>,
    pub late: u32,
    /// Hubs in an outage now.
    pub out: u32,
}

/// The state at one call.
pub struct Snapshot<'a> {
    pub time_s: f64,
    pub day_s: f64,
    pub region: &'a Region,
    pub hubs: Vec<HubView>,
    /// One vehicle card per class, for range estimates.
    pub cards: BTreeMap<VehicleClass, VehicleCard>,
    pub range_margin: f64,
    /// Every vehicle's latest report, by ID.
    pub vehicles: Vec<&'a Status>,
    pub active_events: Vec<&'a Event>,
    pub items: &'a [String],
    /// A hub's starting stock of each item, the scale for stock decisions.
    pub units_per_item: u32,
    /// The hubs this view is limited to, for a fleet manager per cluster.
    pub cluster: Option<Vec<u32>>,
}

pub fn km(point: Vec3) -> String {
    format!("({:.1}, {:.1}) km", point.x / 1000.0, point.y / 1000.0)
}

fn counts(map: &BTreeMap<VehicleClass, u32>, class: VehicleClass) -> u32 {
    map.get(&class).copied().unwrap_or(0)
}

impl Snapshot<'_> {
    pub fn hub(&self, id: u32) -> Option<&HubView> {
        self.hubs.iter().find(|hub| hub.id == id)
    }

    /// How far a class flies one way on a full charge, within the margin.
    pub fn reach_m(&self, class: VehicleClass) -> f64 {
        self.cards.get(&class).map_or(0.0, |card| {
            card.cruise_speed_mps * card.endurance_s * self.range_margin
        })
    }

    /// A hub number or a site code as a point.
    pub fn locate(&self, near: &str) -> Option<Vec3> {
        if let Ok(id) = near.trim().trim_start_matches("hub").trim().parse::<u32>() {
            return self.region.hub(id).map(|hub| hub.position_world_m);
        }
        self.region.site(near).map(|site| site.position_world_m)
    }

    /// Flight time to and from a point for a class, from its hub.
    fn minutes(&self, from: Vec3, to: Vec3, speed_mps: f64) -> f64 {
        horizontal_m(from, to) / speed_mps / 60.0
    }

    pub fn stock_line(&self, hub: &HubView) -> String {
        hub.stock
            .iter()
            .map(|(item, units)| match hub.arriving.get(item) {
                Some(arriving) if *arriving > 0 => format!("{item} {units} (+{arriving} arriving)"),
                _ => format!("{item} {units}"),
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn waiting_line(&self, hub: &HubView) -> String {
        let total: u32 = hub.waiting.values().sum();
        let mut text = format!("{total} waiting");
        let parts: Vec<String> = [Urgency::Trauma, Urgency::Urgent, Urgency::Routine]
            .into_iter()
            .filter_map(|u| {
                hub.waiting
                    .get(&u)
                    .filter(|n| **n > 0)
                    .map(|n| format!("{n} {}", u.as_str()))
            })
            .collect();
        if !parts.is_empty() {
            let _ = write!(text, " ({})", parts.join(", "));
        }
        if hub.late > 0 {
            let _ = write!(text, ", {} late", hub.late);
        }
        text
    }

    /// One line per hub: the summaries view, and the hub status answer.
    pub fn hub_line(&self, hub: &HubView) -> String {
        let mut text = format!(
            "- hub {} at {}: {}/{} fixed-wing idle, {}/{} multirotor idle",
            hub.id,
            km(hub.position_world_m),
            counts(&hub.idle, VehicleClass::FixedWing),
            counts(&hub.total, VehicleClass::FixedWing),
            counts(&hub.idle, VehicleClass::Multirotor),
            counts(&hub.total, VehicleClass::Multirotor),
        );
        if hub.flying > 0 {
            let _ = write!(text, ", {} flying", hub.flying);
        }
        if hub.incoming > 0 {
            let _ = write!(text, ", {} arriving", hub.incoming);
        }
        if hub.lost > 0 {
            let _ = write!(text, ", {} lost", hub.lost);
        }
        if hub.out {
            text.push_str(", cannot launch (outage)");
        }
        let _ = write!(
            text,
            "; stock {}; {}",
            self.stock_line(hub),
            self.waiting_line(hub)
        );
        text
    }

    pub fn summaries(&self) -> String {
        let mut text = String::from("Hubs:\n");
        for hub in &self.hubs {
            let _ = writeln!(text, "{}", self.hub_line(hub));
        }
        text.push_str(&self.region_line());
        text
    }

    /// Totals over the region, one line.
    pub fn region_line(&self) -> String {
        let idle: u32 = self.hubs.iter().map(|h| h.idle.values().sum::<u32>()).sum();
        let total: u32 = self
            .hubs
            .iter()
            .map(|h| h.total.values().sum::<u32>())
            .sum();
        let waiting: u32 = self
            .hubs
            .iter()
            .map(|h| h.waiting.values().sum::<u32>())
            .sum();
        let late: u32 = self.hubs.iter().map(|h| h.late).sum();
        format!(
            "Region: {} hubs, {idle}/{total} vehicles idle, {waiting} requests waiting, {late} late.\n",
            self.hubs.len()
        )
    }

    pub fn full_list(&self) -> String {
        let mut text = String::from("Hubs:\n");
        for hub in &self.hubs {
            let mut line = format!(
                "- hub {} at {}: stock {}; {}",
                hub.id,
                km(hub.position_world_m),
                self.stock_line(hub),
                self.waiting_line(hub)
            );
            if hub.out {
                line.push_str("; cannot launch (outage)");
            }
            let _ = writeln!(text, "{line}");
        }
        text.push_str("Vehicles:\n");
        for vehicle in &self.vehicles {
            let state = match vehicle.phase {
                Phase::Idle if vehicle.request_id.is_none() => "idle".to_string(),
                Phase::Lost => "lost (out of endurance)".to_string(),
                phase => format!("busy ({})", phase.as_str()),
            };
            let _ = writeln!(
                text,
                "- vehicle {}: {}, hub {}, {state}, at {}, {:.0} min of flight left",
                vehicle.vehicle_id,
                vehicle.card.class.as_str(),
                vehicle.hub,
                km(vehicle.position_world_m),
                vehicle.endurance_left_s / 60.0
            );
        }
        text
    }

    pub fn hub_list(&self) -> String {
        let mut text = String::from("Hubs:\n");
        for hub in &self.hubs {
            let mut line = format!(
                "- hub {} at {}: {} fixed-wing, {} multirotors",
                hub.id,
                km(hub.position_world_m),
                counts(&hub.total, VehicleClass::FixedWing),
                counts(&hub.total, VehicleClass::Multirotor),
            );
            if hub.out {
                line.push_str(", cannot launch (outage)");
            }
            let _ = writeln!(text, "{line}");
        }
        text
    }

    pub fn write(&self, kind: ViewKind) -> String {
        match kind {
            ViewKind::FullList => self.full_list(),
            ViewKind::Summaries => self.summaries(),
            ViewKind::Queries => self.hub_list(),
            ViewKind::Exceptions => self.exceptions(),
        }
    }

    /// What is wrong at a hub: late or stranded requests, low stock, lost
    /// vehicles, an outage. Empty when nothing is.
    fn faults(&self, hub: &HubView) -> Vec<String> {
        let low = (f64::from(self.units_per_item) * 0.25).round() as u32;
        let idle: u32 = hub.idle.values().sum();
        let waiting: u32 = hub.waiting.values().sum();
        let mut faults = Vec::new();
        if hub.late > 0 {
            faults.push(format!("{} late", hub.late));
        }
        if waiting > 0 && idle == 0 {
            faults.push(format!("{} with no idle vehicle", self.waiting_line(hub)));
        }
        let low_items: Vec<String> = hub
            .stock
            .iter()
            .filter(|(_, units)| **units < low)
            .map(|(item, units)| format!("{item} {units}"))
            .collect();
        if !low_items.is_empty() {
            faults.push(format!("stock low: {}", low_items.join(", ")));
        }
        if hub.lost > 0 {
            faults.push(format!("{} lost", hub.lost));
        }
        if hub.out {
            faults.push("cannot launch (outage)".into());
        }
        faults
    }

    /// Something is wrong at some hub in view.
    pub fn has_exceptions(&self) -> bool {
        self.hubs.iter().any(|hub| !self.faults(hub).is_empty())
    }

    /// Hubs with something wrong, one line each, then the region line.
    pub fn exceptions(&self) -> String {
        let mut lines = Vec::new();
        for hub in &self.hubs {
            let faults = self.faults(hub);
            if !faults.is_empty() {
                lines.push(format!("- hub {}: {}", hub.id, faults.join("; ")));
            }
        }
        let mut text = if lines.is_empty() {
            String::from("No exceptions at any hub.\n")
        } else {
            format!("Exceptions:\n{}\n", lines.join("\n"))
        };
        text.push_str(&self.region_line());
        text
    }

    /// One cluster's totals, for the manager above the clusters.
    pub fn cluster_view(&self, index: u32, hubs: &[u32]) -> ClusterView {
        let mut view = ClusterView {
            index,
            hubs: hubs.to_vec(),
            ..ClusterView::default()
        };
        for hub in self.hubs.iter().filter(|h| hubs.contains(&h.id)) {
            for (class, n) in &hub.idle {
                *view.idle.entry(*class).or_insert(0) += n;
            }
            for (class, n) in &hub.total {
                *view.total.entry(*class).or_insert(0) += n;
            }
            view.flying += hub.flying;
            view.lost += hub.lost;
            for (item, units) in &hub.stock {
                *view.stock.entry(item.clone()).or_insert(0) += units;
            }
            for (urgency, n) in &hub.waiting {
                *view.waiting.entry(*urgency).or_insert(0) += n;
            }
            view.late += hub.late;
            view.out += u32::from(hub.out);
        }
        view
    }

    /// One line per cluster, then the region line: what the manager above
    /// the clusters sees.
    pub fn clusters(&self, clusters: &[Vec<u32>]) -> String {
        let mut text = String::from("Clusters:\n");
        for (i, hubs) in clusters.iter().enumerate() {
            let view = self.cluster_view(i as u32 + 1, hubs);
            let mut line = format!(
                "- cluster {} (hubs {}): {}/{} fixed-wing idle, {}/{} multirotor idle",
                view.index,
                crate::event::hub_list(&view.hubs),
                counts(&view.idle, VehicleClass::FixedWing),
                counts(&view.total, VehicleClass::FixedWing),
                counts(&view.idle, VehicleClass::Multirotor),
                counts(&view.total, VehicleClass::Multirotor),
            );
            if view.flying > 0 {
                let _ = write!(line, ", {} flying", view.flying);
            }
            if view.lost > 0 {
                let _ = write!(line, ", {} lost", view.lost);
            }
            if view.out > 0 {
                let _ = write!(line, ", {} hubs in outage", view.out);
            }
            let stock: Vec<String> = view
                .stock
                .iter()
                .map(|(item, units)| format!("{item} {units}"))
                .collect();
            let total: u32 = view.waiting.values().sum();
            let _ = write!(line, "; stock {}; {total} waiting", stock.join(", "));
            if let Some(trauma) = view.waiting.get(&Urgency::Trauma).filter(|n| **n > 0) {
                let _ = write!(line, " ({trauma} trauma)");
            }
            if view.late > 0 {
                let _ = write!(line, ", {} late", view.late);
            }
            let events: Vec<String> = self
                .active_events
                .iter()
                .filter(|e| e.hubs.iter().any(|h| hubs.contains(h)))
                .map(|e| {
                    let hubs: Vec<u32> = e
                        .hubs
                        .iter()
                        .copied()
                        .filter(|h| hubs.contains(h))
                        .collect();
                    let mut text = format!(
                        "{} at hubs {}",
                        e.kind.as_str(),
                        crate::event::hub_list(&hubs)
                    );
                    if e.until_min.is_finite() {
                        let _ = write!(text, " until minute {:.0}", e.until_min);
                    }
                    text
                })
                .collect();
            if !events.is_empty() {
                let _ = write!(line, "; events: {}", events.join("; "));
            }
            let _ = writeln!(text, "{line}");
        }
        text.push_str(&self.region_line());
        text
    }

    /// The answer to a query, in a few lines.
    pub fn answer(&self, query: &Query) -> String {
        match query {
            Query::FindVehicles {
                near,
                within_min,
                class,
            } => {
                let Some(point) = self.locate(near) else {
                    return format!("No hub or site named {near}.");
                };
                let mut per_hub: BTreeMap<(u32, VehicleClass), (u32, f64)> = BTreeMap::new();
                for vehicle in &self.vehicles {
                    if !vehicle.free() || class.is_some_and(|c| c != vehicle.card.class) {
                        continue;
                    }
                    let minutes = self.minutes(
                        vehicle.position_world_m,
                        point,
                        vehicle.card.cruise_speed_mps,
                    );
                    if minutes <= *within_min {
                        let entry = per_hub
                            .entry((vehicle.hub, vehicle.card.class))
                            .or_insert((0, minutes));
                        entry.0 += 1;
                        entry.1 = entry.1.min(minutes);
                    }
                }
                if per_hub.is_empty() {
                    return format!("No idle vehicles within {within_min:.0} min of {near}.");
                }
                let mut text = format!("Idle vehicles within {within_min:.0} min of {near}:\n");
                for ((hub, class), (count, minutes)) in per_hub {
                    let _ = writeln!(
                        text,
                        "- hub {hub}: {count} {} ({minutes:.0} min away)",
                        class.as_str()
                    );
                }
                text
            }
            Query::HubStatus { hub } => match self.hub(*hub) {
                Some(view) => {
                    let mut text = self.hub_line(view);
                    text.push('\n');
                    let sites: Vec<String> = self
                        .region
                        .sites_of(*hub)
                        .map(|site| format!("{} {}", site.code, km(site.position_world_m)))
                        .collect();
                    let _ = writeln!(text, "  sites: {}", sites.join("; "));
                    text
                }
                None => format!("No hub {hub}."),
            },
            Query::FindStock { item, near } => {
                let Some(point) = self.locate(near) else {
                    return format!("No hub or site named {near}.");
                };
                if !self.items.iter().any(|i| i.eq_ignore_ascii_case(item)) {
                    return format!(
                        "No item named {item}; the items are {}.",
                        self.items.join(", ")
                    );
                }
                let mut holders: Vec<(&HubView, u32, f64)> = self
                    .hubs
                    .iter()
                    .filter_map(|hub| {
                        let units = hub
                            .stock
                            .iter()
                            .find(|(name, _)| name.eq_ignore_ascii_case(item))
                            .map(|(_, units)| *units)?;
                        (units > 0).then_some((
                            hub,
                            units,
                            horizontal_m(hub.position_world_m, point) / 1000.0,
                        ))
                    })
                    .collect();
                holders.sort_by(|a, b| a.2.total_cmp(&b.2));
                if holders.is_empty() {
                    return format!("No hub has {item}.");
                }
                let mut text = format!("Hubs with {item}, nearest {near} first:\n");
                for (hub, units, distance_km) in holders.iter().take(8) {
                    let _ = writeln!(
                        text,
                        "- hub {}: {units} units, {distance_km:.0} km away",
                        hub.id
                    );
                }
                text
            }
        }
    }
}

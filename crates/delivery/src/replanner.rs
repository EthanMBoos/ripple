//! The structured replanner: fixed rules that read each event's fields
//! and gives the same orders a fleet manager can. It is meant to be
//! competent, not a strawman. Regional events get fixed rules and a greedy
//! reassignment: spare vehicles and stock come from the nearest hubs with
//! slack, by flight time. At each check-in it also tops up any hub running
//! low on an item from the nearest hub with spare. It sees the whole fleet
//! state.

use crate::event::{Event, EventKind};
use crate::messages::{Urgency, VehicleClass};
use crate::order::Order;
use crate::region::horizontal_m;
use crate::view::{HubView, Snapshot};

/// A donor keeps at least this share of its vehicles of a class.
pub(crate) const KEEP_SHARE: f64 = 0.5;
/// Fixed-wing aircraft moved to each surge hub, at most, and multirotors.
const SURGE_FIXED_WING: u32 = 4;
const SURGE_MULTIROTOR: u32 = 2;
/// Stock amounts as shares of a hub's starting stock of an item: what a
/// transfer moves, what a donor keeps, and below what a hub is topped up.
const TRANSFER_SHARE: f64 = 0.5;
const KEEP_SHARE_UNITS: f64 = 0.25;
const LOW_SHARE: f64 = 0.25;

pub(crate) fn share(snapshot: &Snapshot<'_>, part: f64) -> u32 {
    ((f64::from(snapshot.units_per_item) * part).round() as u32).max(1)
}

pub fn plan(new_events: &[&Event], snapshot: &Snapshot<'_>) -> Vec<Order> {
    let mut orders = Vec::new();
    if new_events.is_empty() {
        return top_up(
            snapshot,
            share(snapshot, LOW_SHARE),
            share(snapshot, TRANSFER_SHARE),
        );
    }
    for event in new_events {
        let until_min = if event.until_min.is_finite() {
            event.until_min
        } else {
            snapshot.day_s / 60.0 + 60.0
        };
        match event.kind {
            EventKind::Surge => {
                for &hub in &event.hubs {
                    for (class, count) in [
                        (VehicleClass::FixedWing, SURGE_FIXED_WING),
                        (VehicleClass::Multirotor, SURGE_MULTIROTOR),
                    ] {
                        orders.extend(repositions(snapshot, hub, &event.hubs, class, count));
                    }
                    for site in snapshot.region.sites_of(hub) {
                        let donors = nearest_donors(snapshot, hub, &event.hubs, 2);
                        if !donors.is_empty() {
                            orders.push(Order::ServeFrom {
                                site: site.code.clone(),
                                hubs: donors,
                            });
                        }
                    }
                }
                orders.push(Order::Prioritize {
                    site: None,
                    urgency: Some(Urgency::Trauma),
                    until_min,
                });
            }
            EventKind::Shortage => {
                for &hub in &event.hubs {
                    let donors = stock_donors(snapshot, hub, &event.hubs, &event.item);
                    if let Some((donor, spare)) = donors.first() {
                        orders.push(Order::Transfer {
                            item: event.item.clone(),
                            units: share(snapshot, TRANSFER_SHARE).min(*spare),
                            from_hub: *donor,
                            to_hub: hub,
                        });
                        for site in snapshot.region.sites_of(hub) {
                            orders.push(Order::ServeFrom {
                                site: site.code.clone(),
                                hubs: vec![*donor],
                            });
                        }
                    }
                }
            }
            EventKind::Closure | EventKind::Corridor => {
                if let Some(area) = event.area {
                    orders.push(Order::Avoid { area, until_min });
                }
            }
            EventKind::Outage => {
                for &hub in &event.hubs {
                    // An outage known in advance: cover the hub's sites from
                    // neighbours, but do not ground it before it starts.
                    if event.at_s() <= snapshot.time_s {
                        orders.push(Order::Ground {
                            class: None,
                            hubs: vec![hub],
                            until_min,
                        });
                    }
                    let donors = nearest_donors(snapshot, hub, &event.hubs, 2);
                    for site in snapshot.region.sites_of(hub) {
                        if !donors.is_empty() {
                            orders.push(Order::ServeFrom {
                                site: site.code.clone(),
                                hubs: donors.clone(),
                            });
                        }
                    }
                }
            }
            EventKind::Weather => {
                orders.push(Order::Ground {
                    class: event.class,
                    hubs: event.hubs.clone(),
                    until_min,
                });
            }
            EventKind::Priority => {
                orders.push(Order::Prioritize {
                    site: Some(event.site.clone()),
                    urgency: None,
                    until_min,
                });
            }
            EventKind::RooftopClosed => {
                orders.push(Order::UsePad {
                    site: event.site.clone(),
                    until_min,
                });
            }
        }
    }
    orders
}

fn distance_km(snapshot: &Snapshot<'_>, a: u32, b: u32) -> f64 {
    match (snapshot.hub(a), snapshot.hub(b)) {
        (Some(a), Some(b)) => horizontal_m(a.position_world_m, b.position_world_m) / 1000.0,
        _ => f64::INFINITY,
    }
}

/// Hubs outside `excluded`, not in an outage, nearest `hub` first.
fn candidates<'a>(snapshot: &'a Snapshot<'_>, hub: u32, excluded: &[u32]) -> Vec<&'a HubView> {
    let mut hubs: Vec<&HubView> = snapshot
        .hubs
        .iter()
        .filter(|h| !excluded.contains(&h.id) && !h.out)
        .collect();
    hubs.sort_by(|a, b| {
        distance_km(snapshot, a.id, hub)
            .total_cmp(&distance_km(snapshot, b.id, hub))
            .then(a.id.cmp(&b.id))
    });
    hubs
}

pub(crate) fn nearest_donors(
    snapshot: &Snapshot<'_>,
    hub: u32,
    excluded: &[u32],
    count: usize,
) -> Vec<u32> {
    candidates(snapshot, hub, excluded)
        .iter()
        .take(count)
        .map(|h| h.id)
        .collect()
}

/// Move up to `count` idle vehicles of `class` to `hub` from the nearest
/// hubs that can spare them.
pub(crate) fn repositions(
    snapshot: &Snapshot<'_>,
    hub: u32,
    excluded: &[u32],
    class: VehicleClass,
    count: u32,
) -> Vec<Order> {
    let mut orders = Vec::new();
    let mut wanted = count;
    for donor in candidates(snapshot, hub, excluded) {
        if wanted == 0 {
            break;
        }
        // Only from hubs the class can reach on one charge.
        if distance_km(snapshot, donor.id, hub) * 1000.0 > snapshot.reach_m(class) {
            continue;
        }
        let idle = donor.idle.get(&class).copied().unwrap_or(0);
        let total = donor.total.get(&class).copied().unwrap_or(0);
        let keep = (f64::from(total) * KEEP_SHARE).ceil() as u32;
        let spare = idle.saturating_sub(keep);
        if spare == 0 {
            continue;
        }
        let moved = spare.min(wanted);
        orders.push(Order::Reposition {
            class: Some(class),
            count: moved,
            from_hub: donor.id,
            to_hub: hub,
            until_min: None,
        });
        wanted -= moved;
    }
    orders
}

/// Hubs with spare units of the item, nearest first, with what they can spare.
pub(crate) fn stock_donors(
    snapshot: &Snapshot<'_>,
    hub: u32,
    excluded: &[u32],
    item: &str,
) -> Vec<(u32, u32)> {
    candidates(snapshot, hub, excluded)
        .iter()
        .filter_map(|donor| {
            let units = donor.stock.get(item).copied().unwrap_or(0);
            let spare = units.saturating_sub(share(snapshot, KEEP_SHARE_UNITS));
            (spare > 0).then_some((donor.id, spare))
        })
        .collect()
}

/// Transfers of `units` to every hub holding under `below` of an item, from
/// its nearest donor with spare. Units already on their way count, so a slow
/// transfer is not ordered twice.
pub(crate) fn top_up(snapshot: &Snapshot<'_>, below: u32, units: u32) -> Vec<Order> {
    let mut orders = Vec::new();
    for hub in &snapshot.hubs {
        for item in snapshot.items {
            let held = hub.stock.get(item).copied().unwrap_or(0)
                + hub.arriving.get(item).copied().unwrap_or(0);
            if held >= below {
                continue;
            }
            let donors = stock_donors(snapshot, hub.id, &[hub.id], item);
            if let Some((donor, spare)) = donors.first() {
                orders.push(Order::Transfer {
                    item: item.clone(),
                    units: units.min(*spare),
                    from_hub: *donor,
                    to_hub: hub.id,
                });
            }
        }
    }
    orders
}

//! Help across clusters, the manager above the clusters' temporary order.
//! A hub hit by a surge or an outage asks for aircraft. The dispatcher lists
//! the offers: hubs outside the cluster that can spare aircraft and reach
//! every one of the hub's sites and return. The manager above grants one
//! offer with a `help` order or grants nothing. A granted hub serves the
//! asking hub's sites until the minute given; its aircraft fly each request
//! and come home, so nothing is relocated (`Lend` in `rules.rs`).

use std::collections::BTreeMap;
use std::fmt::Write;

use crate::event::EventKind;
use crate::messages::{Urgency, VehicleClass};
use crate::order::Order;
use crate::region::horizontal_m;
use crate::replanner::KEEP_SHARE;
use crate::view::Snapshot;

/// Offers listed per request, nearest first.
pub const OFFERS: usize = 5;

/// A hub asking for aircraft: raised by the dispatcher when a surge or an
/// outage starts at the hub, dropped when help is granted or the event ends.
#[derive(Clone, Debug, PartialEq)]
pub struct HelpRequest {
    pub hub: u32,
    /// 1-based, as `Region::clusters` orders them.
    pub cluster: u32,
    pub kind: EventKind,
    pub until_s: f64,
}

/// A hub outside the asking hub's cluster that could serve it: how many
/// aircraft of each class it can spare and still keep half, and how far it
/// is. Only classes that reach the asking hub's farthest site and return on
/// one charge are offered.
#[derive(Clone, Debug, PartialEq)]
pub struct Offer {
    pub from_hub: u32,
    pub cluster: u32,
    pub spare: BTreeMap<VehicleClass, u32>,
    pub distance_km: f64,
}

/// The 1-based cluster a hub belongs to.
pub fn cluster_of(hub: u32, clusters: &[Vec<u32>]) -> Option<u32> {
    clusters
        .iter()
        .position(|c| c.contains(&hub))
        .map(|i| i as u32 + 1)
}

/// The offers for one request, nearest first, at most `OFFERS`.
pub fn offers(snapshot: &Snapshot<'_>, request: &HelpRequest, clusters: &[Vec<u32>]) -> Vec<Offer> {
    let Some(asking) = snapshot.region.hub(request.hub) else {
        return Vec::new();
    };
    let mut offers = Vec::new();
    for hub in &snapshot.hubs {
        if hub.id == request.hub || hub.out {
            continue;
        }
        let Some(cluster) = cluster_of(hub.id, clusters) else {
            continue;
        };
        if cluster == request.cluster {
            continue;
        }
        // A round trip to the farthest of the asking hub's sites.
        let farthest_m = snapshot
            .region
            .sites_of(request.hub)
            .map(|site| horizontal_m(hub.position_world_m, site.position_world_m))
            .fold(0.0, f64::max);
        let mut spare = BTreeMap::new();
        for class in snapshot.cards.keys() {
            if 2.0 * farthest_m > snapshot.reach_m(*class) {
                continue;
            }
            let idle = hub.idle.get(class).copied().unwrap_or(0);
            let total = hub.total.get(class).copied().unwrap_or(0);
            let keep = (f64::from(total) * KEEP_SHARE).ceil() as u32;
            let free = idle.saturating_sub(keep);
            if free > 0 {
                spare.insert(*class, free);
            }
        }
        if spare.is_empty() {
            continue;
        }
        offers.push(Offer {
            from_hub: hub.id,
            cluster,
            spare,
            distance_km: horizontal_m(hub.position_world_m, asking.position_world_m) / 1000.0,
        });
    }
    offers.sort_by(|a, b| {
        a.distance_km
            .total_cmp(&b.distance_km)
            .then(a.from_hub.cmp(&b.from_hub))
    });
    offers.truncate(OFFERS);
    offers
}

/// The fixed rule for the manager above: each request gets its nearest
/// offer whose hub has not been granted to another request this call, the
/// requests with the most trauma waiting first. One donor per request, one
/// request per donor, nothing when no offer is left.
pub fn broker(
    requests: &[HelpRequest],
    snapshot: &Snapshot<'_>,
    clusters: &[Vec<u32>],
) -> Vec<Order> {
    let trauma = |hub: u32| {
        snapshot
            .hub(hub)
            .and_then(|h| h.waiting.get(&Urgency::Trauma).copied())
            .unwrap_or(0)
    };
    let mut ordered: Vec<&HelpRequest> = requests.iter().collect();
    ordered.sort_by(|a, b| trauma(b.hub).cmp(&trauma(a.hub)).then(a.hub.cmp(&b.hub)));
    let mut granted = Vec::new();
    let mut orders = Vec::new();
    for request in ordered {
        let Some(offer) = offers(snapshot, request, clusters)
            .into_iter()
            .find(|o| !granted.contains(&o.from_hub))
        else {
            continue;
        };
        granted.push(offer.from_hub);
        orders.push(Order::Help {
            hub: request.hub,
            from_hub: offer.from_hub,
            until_min: request.until_s / 60.0,
        });
    }
    orders
}

/// The requests and their offers as the manager above is told them.
pub fn describe(
    requests: &[HelpRequest],
    snapshot: &Snapshot<'_>,
    clusters: &[Vec<u32>],
) -> String {
    if requests.is_empty() {
        return "No hub is asking for help.\n".into();
    }
    let mut text = String::from("Help requested:\n");
    for request in requests {
        let view = snapshot.hub(request.hub);
        let waiting: u32 = view.map_or(0, |h| h.waiting.values().sum());
        let trauma = view.map_or(0, |h| h.waiting.get(&Urgency::Trauma).copied().unwrap_or(0));
        let idle: u32 = view.map_or(0, |h| h.idle.values().sum());
        let total: u32 = view.map_or(0, |h| h.total.values().sum());
        let _ = write!(
            text,
            "- hub {} (cluster {}): {} until minute {:.0}; {waiting} waiting ({trauma} trauma), {idle} idle of {total}.",
            request.hub,
            request.cluster,
            request.kind.as_str(),
            request.until_s / 60.0
        );
        let offers = offers(snapshot, request, clusters);
        if offers.is_empty() {
            text.push_str(" No hub outside the cluster can reach it with spare aircraft.\n");
            continue;
        }
        let lines: Vec<String> = offers
            .iter()
            .map(|o| {
                let spare: Vec<String> = o
                    .spare
                    .iter()
                    .map(|(class, n)| format!("{n} {}", class.as_str()))
                    .collect();
                format!(
                    "hub {} (cluster {}), {:.0} km, {} spare",
                    o.from_hub,
                    o.cluster,
                    o.distance_km,
                    spare.join(" and ")
                )
            })
            .collect();
        let _ = writeln!(text, " Offers: {}.", lines.join("; "));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::VehicleCard;
    use crate::region::{Region, RegionConfig};
    use crate::view::HubView;

    fn region() -> Region {
        Region::new(&RegionConfig {
            hubs: 4,
            hub_spacing_m: 15_000.0,
            ..RegionConfig::default()
        })
        .unwrap()
    }

    /// Four hubs, each its own cluster; `idle` fixed-wing of 8 at each.
    fn snapshot<'a>(region: &'a Region, idle: [u32; 4], endurance_s: f64) -> Snapshot<'a> {
        let hubs = (1..=4u32)
            .map(|id| HubView {
                id,
                position_world_m: region.hub(id).unwrap().position_world_m,
                idle: BTreeMap::from([(VehicleClass::FixedWing, idle[id as usize - 1])]),
                total: BTreeMap::from([(VehicleClass::FixedWing, 8)]),
                ..HubView::default()
            })
            .collect();
        Snapshot {
            time_s: 600.0,
            day_s: 28_800.0,
            region,
            hubs,
            cards: BTreeMap::from([(
                VehicleClass::FixedWing,
                VehicleCard {
                    class: VehicleClass::FixedWing,
                    cruise_speed_mps: 25.0,
                    endurance_s,
                },
            )]),
            range_margin: 1.0,
            active_events: Vec::new(),
            items: &[],
            units_per_item: 0,
        }
    }

    fn request(hub: u32) -> HelpRequest {
        HelpRequest {
            hub,
            cluster: hub,
            kind: EventKind::Surge,
            until_s: 3600.0,
        }
    }

    #[test]
    fn offers_keep_half_skip_the_own_cluster_and_need_the_range() {
        let region = region();
        let clusters = region.clusters(1);
        let snapshot = snapshot(&region, [8, 8, 4, 6], 3600.0);
        let offers = offers(&snapshot, &request(1), &clusters);
        // Hub 4 keeps 4 of 8 and spares 2; hub 3 has nothing over half;
        // hub 1 is the asker. Nearest first: hub 2 before hub 4.
        assert_eq!(
            offers.iter().map(|o| o.from_hub).collect::<Vec<_>>(),
            vec![2, 4]
        );
        assert_eq!(offers[0].spare[&VehicleClass::FixedWing], 4);
        assert_eq!(offers[1].spare[&VehicleClass::FixedWing], 2);
        let short = snapshot_short(&region);
        assert!(
            offers_for(&short, &clusters).is_empty(),
            "a 1 km reach offers nothing"
        );
    }

    fn snapshot_short(region: &Region) -> Snapshot<'_> {
        snapshot(region, [8, 8, 8, 8], 40.0)
    }

    fn offers_for(snapshot: &Snapshot<'_>, clusters: &[Vec<u32>]) -> Vec<Offer> {
        offers(snapshot, &request(1), clusters)
    }

    #[test]
    fn the_broker_grants_one_hub_per_request_and_each_hub_once() {
        let region = region();
        let clusters = region.clusters(1);
        let snapshot = snapshot(&region, [8, 8, 8, 8], 3600.0);
        let orders = broker(&[request(1), request(4)], &snapshot, &clusters);
        assert_eq!(orders.len(), 2);
        let donors: Vec<u32> = orders
            .iter()
            .map(|o| match o {
                Order::Help { from_hub, .. } => *from_hub,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_ne!(donors[0], donors[1], "one request per donor hub");
        assert!(!donors.contains(&1) || !donors.contains(&4));
    }
}

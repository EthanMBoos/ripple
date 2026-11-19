//! Help between hubs, the help manager's only order. A hub hit by a surge
//! or an outage asks for aircraft. The dispatcher lists the offers: hubs
//! that are not asking or lending themselves and can spare aircraft above
//! the half they keep, whose aircraft reach one of the asking hub's sites
//! within a trauma deadline and return on one charge. An offer gives the
//! hub's distance, the sites each class reaches in time, the hub's own
//! backlog, and the minute its first aircraft would arrive, rounded as the
//! help manager reads them, so a fixed rule and a model choose from the
//! same numbers. The help manager grants one offer with a `help` order or
//! grants nothing; a pair that is not offered is refused, and so is a
//! second grant to a hub already helped. A granted hub serves the asking
//! hub's sites until the event ends; its aircraft fly each request and
//! come home, so nothing is relocated (`Lend` in `rules.rs`). At every
//! assignment (`assign.rs`) the lender keeps half of each class idle and
//! flies only a request it can deliver on time.
//!
//! Three fixed rules stand in for the help manager (`Rule`): the nearest
//! offer, the offer with the most to spare over its own backlog, and a
//! random offer, which measures what the choice is worth once the offers
//! are listed. `Fact` names the parts of an offer line a model can be
//! shown without.

use std::collections::BTreeMap;
use std::fmt::Write;

use ripple_core::plugin::PluginRandom;
use serde::Deserialize;

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
    pub kind: EventKind,
    pub until_s: f64,
}

/// A hub that could serve the asking hub.
#[derive(Clone, Debug, PartialEq)]
pub struct Offer {
    pub from_hub: u32,
    /// Idle aircraft of each class above the half the hub keeps; only
    /// classes that reach one of the asking hub's sites in time are listed.
    pub spare: BTreeMap<VehicleClass, u32>,
    /// How many of the asking hub's sites each class reaches within a trauma
    /// deadline and returns from on one charge.
    pub reaches: BTreeMap<VehicleClass, u32>,
    /// To a tenth of a kilometer.
    pub distance_km: f64,
    /// Requests waiting at the offering hub for its own sites.
    pub backlog: u32,
    /// Whole minutes to the nearest of the asking hub's sites, by the
    /// fastest class offered.
    pub arrival_min: f64,
}

impl Offer {
    /// Aircraft to spare over the hub's own waiting requests: the measure of
    /// what the hub can give without losing its own deliveries.
    pub fn net_spare(&self) -> i64 {
        i64::from(self.spare.values().sum::<u32>()) - i64::from(self.backlog)
    }
}

/// The fixed rules that stand in for the help manager.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rule {
    /// The nearest offer.
    #[default]
    Nearest,
    /// The offer with the most aircraft to spare over its own backlog, then
    /// the earliest arrival.
    Spare,
    /// Any offer, drawn by the asking hub and the time of the call.
    Random,
}

/// The parts of an offer line that `describe` can leave out, to test which
/// the help manager's decision rests on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fact {
    /// The aircraft to spare by class, with how many sites they reach.
    Spare,
    /// How many of the asking hub's sites each class reaches in time.
    Reach,
    /// The hub's own waiting requests.
    Backlog,
    /// The minute its first aircraft arrives.
    Arrival,
}

/// The offers for one request, nearest first, at most `OFFERS`. `requests`
/// are all the open requests: an asking hub does not offer, nor does a hub
/// lending now (`Snapshot::lending`).
pub fn offers(
    snapshot: &Snapshot<'_>,
    request: &HelpRequest,
    requests: &[HelpRequest],
) -> Vec<Offer> {
    let Some(asking) = snapshot.region.hub(request.hub) else {
        return Vec::new();
    };
    let sites: Vec<_> = snapshot.region.sites_of(request.hub).collect();
    let mut offers = Vec::new();
    for hub in &snapshot.hubs {
        if hub.id == request.hub
            || hub.out
            || requests.iter().any(|r| r.hub == hub.id)
            || snapshot.lending.contains(&hub.id)
        {
            continue;
        }
        let mut spare = BTreeMap::new();
        let mut reaches = BTreeMap::new();
        let mut arrival_min = f64::INFINITY;
        for (class, card) in &snapshot.cards {
            let idle = hub.idle.get(class).copied().unwrap_or(0);
            let total = hub.total.get(class).copied().unwrap_or(0);
            let free = idle.saturating_sub(keep(total));
            if free == 0 {
                continue;
            }
            // A site counts when the round trip fits one charge and the
            // flight out fits a trauma deadline from launch.
            let out_m = |site: &crate::region::Site| {
                horizontal_m(hub.position_world_m, site.position_world_m)
            };
            let in_time = sites
                .iter()
                .filter(|site| {
                    let out_m = out_m(site);
                    2.0 * out_m <= snapshot.reach_m(*class)
                        && out_m / card.cruise_speed_mps <= snapshot.trauma_deadline_s
                })
                .count() as u32;
            if in_time == 0 {
                continue;
            }
            let nearest_m = sites
                .iter()
                .map(|site| out_m(site))
                .fold(f64::INFINITY, f64::min);
            arrival_min = arrival_min.min(nearest_m / card.cruise_speed_mps / 60.0);
            spare.insert(*class, free);
            reaches.insert(*class, in_time);
        }
        if spare.is_empty() {
            continue;
        }
        let distance_km = horizontal_m(hub.position_world_m, asking.position_world_m) / 1000.0;
        offers.push(Offer {
            from_hub: hub.id,
            spare,
            reaches,
            distance_km: (distance_km * 10.0).round() / 10.0,
            backlog: hub.waiting.values().sum(),
            arrival_min: arrival_min.round(),
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

/// How many of `total` aircraft a lender keeps for its own hub.
pub fn keep(total: u32) -> u32 {
    (f64::from(total) * KEEP_SHARE).ceil() as u32
}

/// Whether `from_hub` is offered to `hub` now: `hub` is asking and
/// `from_hub` is in its offers.
pub fn offered(hub: u32, from_hub: u32, snapshot: &Snapshot<'_>, requests: &[HelpRequest]) -> bool {
    requests.iter().any(|request| {
        request.hub == hub
            && offers(snapshot, request, requests)
                .iter()
                .any(|o| o.from_hub == from_hub)
    })
}

/// The fixed help manager: each request, the ones with the most trauma
/// waiting first, gets one offer chosen by the rule, and each hub is granted
/// once per call. `random` is keyed by the asking hub and the call's time;
/// only the random rule draws from it. Nothing is granted when no offer is
/// left.
pub fn grant(
    rule: Rule,
    requests: &[HelpRequest],
    snapshot: &Snapshot<'_>,
    random: &PluginRandom,
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
        let mut offers = offers(snapshot, request, requests);
        offers.retain(|o| !granted.contains(&o.from_hub));
        match rule {
            Rule::Nearest => {}
            Rule::Spare => offers.sort_by(|a, b| {
                b.net_spare()
                    .cmp(&a.net_spare())
                    .then(a.arrival_min.total_cmp(&b.arrival_min))
                    .then(a.from_hub.cmp(&b.from_hub))
            }),
            Rule::Random if !offers.is_empty() => {
                let draw =
                    random.uniform(&[u64::from(request.hub), snapshot.time_s.round() as u64]);
                let pick = ((draw * offers.len() as f64) as usize).min(offers.len() - 1);
                offers.swap(0, pick);
            }
            Rule::Random => {}
        }
        let Some(offer) = offers.first() else {
            continue;
        };
        granted.push(offer.from_hub);
        orders.push(Order::Help {
            hub: request.hub,
            from_hub: offer.from_hub,
        });
    }
    orders
}

/// The requests and their offers as the help manager is told them. The
/// facts in `hide` are left off every offer line.
pub fn describe(requests: &[HelpRequest], snapshot: &Snapshot<'_>, hide: &[Fact]) -> String {
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
        let sites = snapshot.region.sites_of(request.hub).count();
        let _ = write!(
            text,
            "- hub {}: {} until minute {:.0}; {waiting} waiting ({trauma} trauma), {idle} idle of {total}.",
            request.hub,
            request.kind.as_str(),
            request.until_s / 60.0
        );
        let offers = offers(snapshot, request, requests);
        if offers.is_empty() {
            text.push_str(" No hub can reach its sites in time with spare aircraft.\n");
            continue;
        }
        text.push_str(" Offers, nearest first:\n");
        for o in &offers {
            let spare: Vec<String> = o
                .spare
                .iter()
                .map(|(class, n)| {
                    if hide.contains(&Fact::Reach) {
                        format!("{n} {}", class.as_str())
                    } else {
                        format!(
                            "{n} {} for {} of {sites} sites",
                            class.as_str(),
                            o.reaches.get(class).copied().unwrap_or(0)
                        )
                    }
                })
                .collect();
            let mut facts = Vec::new();
            if !hide.contains(&Fact::Spare) {
                facts.push(format!("{} spare", spare.join(" and ")));
            }
            if !hide.contains(&Fact::Backlog) {
                facts.push(format!("{} waiting of its own", o.backlog));
            }
            if !hide.contains(&Fact::Arrival) {
                facts.push(format!("nearest site in {:.0} min", o.arrival_min));
            }
            let _ = writeln!(
                text,
                "  - hub {}, {:.1} km away: {}.",
                o.from_hub,
                o.distance_km,
                facts.join("; ")
            );
        }
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

    /// Four hubs; `idle` fixed-wing of 8 at each, `waiting` requests at each.
    fn fixture<'a>(
        region: &'a Region,
        idle: [u32; 4],
        waiting: [u32; 4],
        endurance_s: f64,
    ) -> Snapshot<'a> {
        let hubs = (1..=4u32)
            .map(|id| HubView {
                id,
                position_world_m: region.hub(id).unwrap().position_world_m,
                idle: BTreeMap::from([(VehicleClass::FixedWing, idle[id as usize - 1])]),
                total: BTreeMap::from([(VehicleClass::FixedWing, 8)]),
                waiting: BTreeMap::from([(Urgency::Routine, waiting[id as usize - 1])]),
                ..HubView::default()
            })
            .collect();
        Snapshot {
            time_s: 600.0,
            day_s: 28_800.0,
            trauma_deadline_s: 900.0,
            region,
            hubs,
            lending: Vec::new(),
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
            kind: EventKind::Surge,
            until_s: 3600.0,
        }
    }

    fn donors(orders: &[Order]) -> Vec<u32> {
        orders
            .iter()
            .map(|o| match o {
                Order::Help { from_hub, .. } => *from_hub,
                other => panic!("{other:?}"),
            })
            .collect()
    }

    #[test]
    fn offers_keep_half_and_need_the_range_and_the_time() {
        let region = region();
        let snapshot = fixture(&region, [8, 8, 4, 6], [0, 2, 0, 0], 3600.0);
        let asking = [request(1)];
        let listed = offers(&snapshot, &asking[0], &asking);
        // Hub 4 keeps 4 of 8 and spares 2; hub 3 has nothing over half;
        // hub 1 is the asker. Nearest first: hub 2 before hub 4.
        assert_eq!(
            listed.iter().map(|o| o.from_hub).collect::<Vec<_>>(),
            vec![2, 4]
        );
        assert_eq!(listed[0].spare[&VehicleClass::FixedWing], 4);
        assert_eq!(listed[0].backlog, 2);
        assert_eq!(listed[0].net_spare(), 2);
        assert_eq!(listed[1].spare[&VehicleClass::FixedWing], 2);
        // Hub 2 is 15 km from hub 1; the near sites are in a 15-minute
        // flight at 25 m/s (22.5 km), the far ones are not.
        let reached = listed[0].reaches[&VehicleClass::FixedWing];
        assert!((1..8).contains(&reached), "{reached}");
        assert!(listed[0].arrival_min < 15.0);
        // A 1 km reach offers nothing.
        let short = fixture(&region, [8, 8, 8, 8], [0; 4], 40.0);
        assert!(offers(&short, &asking[0], &asking).is_empty());
        // Nor does a class whose flight out takes longer than the deadline.
        let mut slow = fixture(&region, [8, 8, 8, 8], [0; 4], 36_000.0);
        slow.trauma_deadline_s = 60.0;
        assert!(offers(&slow, &asking[0], &asking).is_empty());
    }

    #[test]
    fn an_asking_hub_does_not_offer() {
        let region = region();
        let snapshot = fixture(&region, [8, 8, 8, 8], [0; 4], 3600.0);
        let asking = [request(1), request(2)];
        let offers_1 = offers(&snapshot, &asking[0], &asking);
        assert_eq!(
            offers_1.iter().map(|o| o.from_hub).collect::<Vec<_>>(),
            vec![3, 4],
            "hub 2 is asking too"
        );
        assert!(offered(1, 3, &snapshot, &asking));
        assert!(!offered(1, 2, &snapshot, &asking));
        assert!(!offered(3, 4, &snapshot, &asking), "hub 3 is not asking");
        let text = describe(&asking, &snapshot, &[]);
        assert!(text.contains("- hub 1: surge"), "{text}");
        assert!(
            text.contains("  - hub 3, 15.0 km away: 4 fixed-wing for"),
            "{text}"
        );
        assert!(
            text.contains("; 0 waiting of its own; nearest site in"),
            "{text}"
        );
        // Hidden facts leave the line, the rest keep their order.
        let hidden = describe(&asking, &snapshot, &[Fact::Backlog, Fact::Reach]);
        assert!(
            hidden.contains("  - hub 3, 15.0 km away: 4 fixed-wing spare; nearest site in"),
            "{hidden}"
        );
        assert!(!hidden.contains("waiting of its own"), "{hidden}");
        let bare = describe(&asking, &snapshot, &[Fact::Spare]);
        assert!(
            bare.contains("  - hub 3, 15.0 km away: 0 waiting of its own; nearest site in"),
            "{bare}"
        );
        // A hub lending now is not offered again.
        let mut lending = fixture(&region, [8, 8, 8, 8], [0; 4], 3600.0);
        lending.lending = vec![3];
        assert_eq!(
            offers(&lending, &asking[0], &asking)
                .iter()
                .map(|o| o.from_hub)
                .collect::<Vec<_>>(),
            vec![4]
        );
    }

    #[test]
    fn the_nearest_rule_grants_one_hub_per_request_and_each_hub_once() {
        let region = region();
        let snapshot = fixture(&region, [8, 8, 8, 8], [0; 4], 3600.0);
        let random = PluginRandom::new(1, 1, "test");
        let orders = grant(Rule::Nearest, &[request(1), request(4)], &snapshot, &random);
        assert_eq!(orders.len(), 2);
        let donors = donors(&orders);
        assert_ne!(donors[0], donors[1], "one request per donor hub");
        assert!(!donors.contains(&1) || !donors.contains(&4));
    }

    #[test]
    fn the_random_rule_takes_an_offer_and_repeats_its_draw() {
        let region = region();
        let snapshot = fixture(&region, [8, 8, 8, 8], [0; 4], 3600.0);
        let asking = [request(1)];
        let listed: Vec<u32> = offers(&snapshot, &asking[0], &asking)
            .iter()
            .map(|o| o.from_hub)
            .collect();
        let mut seen = std::collections::BTreeSet::new();
        for seed in 1..20 {
            let random = PluginRandom::new(seed, 1, "test");
            let picked = donors(&grant(Rule::Random, &asking, &snapshot, &random));
            assert_eq!(picked.len(), 1);
            assert!(listed.contains(&picked[0]), "{picked:?} not in {listed:?}");
            assert_eq!(
                picked,
                donors(&grant(Rule::Random, &asking, &snapshot, &random)),
                "the same key draws the same offer"
            );
            seen.insert(picked[0]);
        }
        assert!(seen.len() > 1, "twenty seeds never varied: {seen:?}");
    }

    #[test]
    fn the_spare_rule_takes_the_spare_over_the_backlog() {
        let region = region();
        // Hub 2 is nearest to hub 1 (tied with hub 3, lower id) but has 6
        // waiting; hub 4 is farther, with 4 to spare and nothing waiting.
        let snapshot = fixture(&region, [8, 8, 4, 8], [0, 6, 0, 0], 3600.0);
        let asking = [request(1)];
        let random = PluginRandom::new(1, 1, "test");
        assert_eq!(
            donors(&grant(Rule::Nearest, &asking, &snapshot, &random)),
            vec![2]
        );
        assert_eq!(
            donors(&grant(Rule::Spare, &asking, &snapshot, &random)),
            vec![4]
        );
    }
}

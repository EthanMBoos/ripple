//! The orders a fleet manager can give, the rules they leave in force, and
//! how a model's reply is read. An order changes how the dispatcher and the
//! couriers behave; it never assigns a single request. The dispatcher checks
//! each order against the region and applies it (`dispatcher.rs`).
//!
//! Orders are written as JSON objects with one key, the order's name, so a
//! model's reply and a mission's scripted list use the same form:
//! `{"reposition": {"class": "fixed_wing", "count": 4, "from_hub": 2, "to_hub": 5}}`.
//! Times are minutes of the day, distances kilometers.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::area::{Area, tagged};
use crate::messages::{Request, Urgency, VehicleClass};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Order {
    /// Keep `count` vehicles at `hub` for `site` alone.
    Reserve {
        hub: u32,
        count: u32,
        site: String,
        until_min: f64,
    },
    /// Route every flight around the area.
    Avoid {
        #[serde(deserialize_with = "tagged")]
        area: Area,
        until_min: f64,
    },
    /// Fly idle vehicles to another hub, which becomes theirs; with
    /// `until_min`, they fly home once idle after that minute.
    Reposition {
        #[serde(default)]
        class: Option<VehicleClass>,
        count: u32,
        from_hub: u32,
        to_hub: u32,
        #[serde(default)]
        until_min: Option<f64>,
    },
    /// Fly stock to another hub, in packages of at most `package_units`.
    Transfer {
        item: String,
        units: u32,
        from_hub: u32,
        to_hub: u32,
    },
    /// Stop a class (or every vehicle) at these hubs from launching.
    Ground {
        #[serde(default)]
        class: Option<VehicleClass>,
        hubs: Vec<u32>,
        until_min: f64,
    },
    /// Serve a site's requests, or an urgency's, before everything else.
    Prioritize {
        #[serde(default)]
        site: Option<String>,
        #[serde(default)]
        urgency: Option<Urgency>,
        until_min: f64,
    },
    /// Let these hubs serve the site besides its own.
    ServeFrom { site: String, hubs: Vec<u32> },
    /// Deliver to a hospital's ground pad by drop instead of landing on its
    /// rooftop.
    UsePad { site: String, until_min: f64 },
    /// From the manager above the clusters only: idle vehicles from one
    /// cluster to another. The dispatcher picks the hubs (`dispatcher.rs`).
    Move {
        #[serde(default)]
        class: Option<VehicleClass>,
        count: u32,
        from_cluster: u32,
        to_cluster: u32,
    },
    /// From the manager above the clusters only: stock from one cluster to
    /// another.
    Send {
        item: String,
        units: u32,
        from_cluster: u32,
        to_cluster: u32,
    },
}

/// What the fleet manager is shown about the orders, once per call.
pub const SYNTAX: &str = "\
Orders, each a JSON object with one key:
- {\"reserve\": {\"hub\": H, \"count\": N, \"site\": \"CODE\", \"until_min\": M}}: keep N vehicles at hub H for that site only.
- {\"avoid\": {\"area\": {\"circle\": {\"center_km\": [x, y], \"radius_km\": r}}, \"until_min\": M}} or {\"avoid\": {\"area\": {\"corridor\": {\"from_km\": [x, y], \"to_km\": [x, y], \"half_width_km\": w}}, \"until_min\": M}}: route every flight around the area.
- {\"reposition\": {\"class\": \"fixed_wing\" or \"multirotor\" or null, \"count\": N, \"from_hub\": A, \"to_hub\": B, \"until_min\": M or null}}: fly N idle vehicles from hub A to hub B, which becomes their hub; with until_min they fly home once idle after that minute.
- {\"transfer\": {\"item\": \"O-\", \"units\": N, \"from_hub\": A, \"to_hub\": B}}: fly N units of stock from hub A to hub B.
- {\"ground\": {\"class\": \"fixed_wing\" or \"multirotor\" or null, \"hubs\": [H, ...], \"until_min\": M}}: no launches of that class (null: any) from those hubs.
- {\"prioritize\": {\"site\": \"CODE\" or null, \"urgency\": \"trauma\" or \"urgent\" or \"routine\" or null, \"until_min\": M}}: serve those requests first.
- {\"serve_from\": {\"site\": \"CODE\", \"hubs\": [H, ...]}}: let those hubs serve the site besides its own.
- {\"use_pad\": {\"site\": \"CODE\", \"until_min\": M}}: deliver to that hospital's ground pad by drop instead of landing on its rooftop.";

impl Order {
    pub fn name(&self) -> &'static str {
        match self {
            Order::Reserve { .. } => "reserve",
            Order::Avoid { .. } => "avoid",
            Order::Reposition { .. } => "reposition",
            Order::Transfer { .. } => "transfer",
            Order::Ground { .. } => "ground",
            Order::Prioritize { .. } => "prioritize",
            Order::ServeFrom { .. } => "serve_from",
            Order::UsePad { .. } => "use_pad",
            Order::Move { .. } => "move",
            Order::Send { .. } => "send",
        }
    }

    /// An order only the manager above the clusters may give.
    pub fn is_top(&self) -> bool {
        matches!(self, Order::Move { .. } | Order::Send { .. })
    }

    /// The hubs an order names, so a cluster's manager stays within its own.
    pub fn hubs_named(&self) -> Vec<u32> {
        match self {
            Order::Reserve { hub, .. } => vec![*hub],
            Order::Reposition {
                from_hub, to_hub, ..
            }
            | Order::Transfer {
                from_hub, to_hub, ..
            } => vec![*from_hub, *to_hub],
            Order::Ground { hubs, .. } | Order::ServeFrom { hubs, .. } => hubs.clone(),
            Order::Avoid { .. }
            | Order::Prioritize { .. }
            | Order::UsePad { .. }
            | Order::Move { .. }
            | Order::Send { .. } => Vec::new(),
        }
    }

    /// The order in a few words, for feedback and traces.
    pub fn describe(&self) -> String {
        let class = |class: &Option<VehicleClass>| class.map_or("vehicles", |c| c.as_str());
        match self {
            Order::Reserve {
                hub,
                count,
                site,
                until_min,
            } => format!("reserve {count} at hub {hub} for {site} until minute {until_min:.0}"),
            Order::Avoid { area, until_min } => {
                format!("avoid {} until minute {until_min:.0}", area.describe())
            }
            Order::Reposition {
                class: c,
                count,
                from_hub,
                to_hub,
                until_min,
            } => match until_min {
                Some(until_min) => format!(
                    "reposition {count} {} from hub {from_hub} to hub {to_hub} until minute {until_min:.0}",
                    class(c)
                ),
                None => format!(
                    "reposition {count} {} from hub {from_hub} to hub {to_hub}",
                    class(c)
                ),
            },
            Order::Transfer {
                item,
                units,
                from_hub,
                to_hub,
            } => format!("transfer {units} units of {item} from hub {from_hub} to hub {to_hub}"),
            Order::Ground {
                class: c,
                hubs,
                until_min,
            } => format!(
                "ground {} at hubs {} until minute {until_min:.0}",
                class(c),
                crate::event::hub_list(hubs)
            ),
            Order::Prioritize {
                site,
                urgency,
                until_min,
            } => format!(
                "prioritize {} until minute {until_min:.0}",
                match (site, urgency) {
                    (Some(site), _) => site.clone(),
                    (None, Some(urgency)) => format!("{} requests", urgency.as_str()),
                    (None, None) => "nothing".into(),
                }
            ),
            Order::ServeFrom { site, hubs } => {
                format!("serve {site} from hubs {}", crate::event::hub_list(hubs))
            }
            Order::UsePad { site, until_min } => {
                format!("use the ground pad at {site} until minute {until_min:.0}")
            }
            Order::Move {
                class: c,
                count,
                from_cluster,
                to_cluster,
            } => format!(
                "move {count} {} from cluster {from_cluster} to cluster {to_cluster}",
                class(c)
            ),
            Order::Send {
                item,
                units,
                from_cluster,
                to_cluster,
            } => format!(
                "send {units} units of {item} from cluster {from_cluster} to cluster {to_cluster}"
            ),
        }
    }
}

/// What the manager above the clusters is shown, once per call.
pub const TOP_SYNTAX: &str = "\
Orders, each a JSON object with one key:
- {\"move\": {\"class\": \"fixed_wing\" or \"multirotor\" or null, \"count\": N, \"from_cluster\": A, \"to_cluster\": B}}: fly N idle vehicles from cluster A to cluster B; they go to B's hub with the most waiting, from A's hubs nearest it that can spare them.
- {\"send\": {\"item\": \"O-\", \"units\": N, \"from_cluster\": A, \"to_cluster\": B}}: fly N units of stock from cluster A's hub holding the most of it to cluster B's hub holding the least.";

/// A question the fleet manager can ask under the queries interface.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Query {
    /// Idle vehicles within `within_min` of flight of a hub number or a site
    /// code.
    FindVehicles {
        near: String,
        within_min: f64,
        #[serde(default)]
        class: Option<VehicleClass>,
    },
    HubStatus {
        hub: u32,
    },
    /// Hubs holding the item, nearest a hub number or site code first.
    FindStock {
        item: String,
        near: String,
    },
}

pub const QUERY_SYNTAX: &str = "\
Instead of orders you may ask one question, and you will be asked again with the answer:
- {\"query\": {\"find_vehicles\": {\"near\": \"H\" or \"CODE\", \"within_min\": M, \"class\": \"fixed_wing\" or \"multirotor\" or null}}}
- {\"query\": {\"hub_status\": {\"hub\": H}}}
- {\"query\": {\"find_stock\": {\"item\": \"O-\", \"near\": \"H\" or \"CODE\"}}}";

/// What the veto manager is told after the proposed orders.
pub const VETO_SYNTAX: &str = "\
Reply with {\"drop\": [numbers]}, the proposed orders to drop, and nothing else; {\"drop\": []} keeps them all.";

/// Reads a veto reply: the numbers of the proposed orders to drop, or None
/// when the reply cannot be read.
pub fn parse_veto(text: &str) -> Option<Vec<usize>> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    let value: serde_json::Value = serde_json::from_str(text.get(start..=end)?).ok()?;
    let list = value.get("drop")?.as_array()?;
    list.iter()
        .map(|n| n.as_u64().map(|n| n as usize))
        .collect()
}

/// What a reply said.
#[derive(Debug, PartialEq)]
pub enum Reply {
    /// The orders that read, and how many did not.
    Orders {
        orders: Vec<Order>,
        unreadable: u32,
    },
    Query(Query),
    Unreadable,
}

/// Reads a reply, taking the outermost braces so a model that wraps its JSON
/// in text or a code fence still parses.
pub fn parse_reply(text: &str) -> Reply {
    let Some(start) = text.find('{') else {
        return Reply::Unreadable;
    };
    let Some(end) = text.rfind('}') else {
        return Reply::Unreadable;
    };
    let Some(slice) = text.get(start..=end) else {
        return Reply::Unreadable;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(slice) else {
        return Reply::Unreadable;
    };
    if let Some(query) = value.get("query") {
        return match serde_json::from_value::<Query>(query.clone()) {
            Ok(query) => Reply::Query(query),
            Err(_) => Reply::Unreadable,
        };
    }
    let Some(list) = value.get("orders").and_then(|orders| orders.as_array()) else {
        return Reply::Unreadable;
    };
    let mut orders = Vec::new();
    let mut unreadable = 0;
    for item in list {
        match serde_json::from_value::<Order>(item.clone()) {
            Ok(order) => orders.push(order),
            Err(_) => unreadable += 1,
        }
    }
    Reply::Orders { orders, unreadable }
}

/// The orders in force, kept by the dispatcher.
#[derive(Clone, Debug, Default)]
pub struct Rules {
    pub reserves: Vec<Reserve>,
    pub avoids: Vec<(Area, f64)>,
    pub grounded: Vec<Grounded>,
    pub priorities: Vec<Priority>,
    /// Extra hubs allowed to serve a site, by site code.
    pub serve_from: BTreeMap<String, Vec<u32>>,
    /// Hospitals whose pad is in use, by site code, until when.
    pub pads: BTreeMap<String, f64>,
}

#[derive(Clone, Debug)]
pub struct Reserve {
    pub hub: u32,
    pub count: u32,
    pub site: String,
    pub until_s: f64,
}

#[derive(Clone, Debug)]
pub struct Grounded {
    pub class: Option<VehicleClass>,
    pub hubs: Vec<u32>,
    pub until_s: f64,
}

#[derive(Clone, Debug)]
pub struct Priority {
    pub site: Option<String>,
    pub urgency: Option<Urgency>,
    pub until_s: f64,
}

impl Rules {
    pub fn expire(&mut self, now_s: f64) {
        self.reserves.retain(|r| r.until_s >= now_s);
        self.avoids.retain(|(_, until_s)| *until_s >= now_s);
        self.grounded.retain(|g| g.until_s >= now_s);
        self.priorities.retain(|p| p.until_s >= now_s);
        self.pads.retain(|_, until_s| *until_s >= now_s);
    }

    pub fn is_grounded(&self, hub: u32, class: VehicleClass) -> bool {
        self.grounded
            .iter()
            .any(|g| g.hubs.contains(&hub) && g.class.is_none_or(|c| c == class))
    }

    pub fn prioritized(&self, request: &Request) -> bool {
        self.priorities.iter().any(|p| {
            p.site
                .as_ref()
                .is_some_and(|site| site.eq_ignore_ascii_case(&request.site))
                || p.urgency.is_some_and(|u| u == request.urgency)
        })
    }

    pub fn uses_pad(&self, site: &str) -> bool {
        self.pads.contains_key(site)
    }

    pub fn active_areas(&self) -> Vec<&Area> {
        self.avoids.iter().map(|(area, _)| area).collect()
    }

    /// Each rule in a line, for the prompt.
    pub fn describe(&self) -> Vec<String> {
        let minute = |s: f64| format!("{:.0}", s / 60.0);
        let mut lines = Vec::new();
        for r in &self.reserves {
            lines.push(format!(
                "{} vehicles at hub {} reserved for {} until minute {}",
                r.count,
                r.hub,
                r.site,
                minute(r.until_s)
            ));
        }
        for (area, until_s) in &self.avoids {
            lines.push(format!(
                "flights avoid {} until minute {}",
                area.describe(),
                minute(*until_s)
            ));
        }
        for g in &self.grounded {
            lines.push(format!(
                "{} grounded at hubs {} until minute {}",
                g.class.map_or("all vehicles", |c| c.as_str()),
                crate::event::hub_list(&g.hubs),
                minute(g.until_s)
            ));
        }
        for p in &self.priorities {
            lines.push(format!(
                "{} first until minute {}",
                match (&p.site, p.urgency) {
                    (Some(site), _) => format!("requests for {site}"),
                    (None, Some(u)) => format!("{} requests", u.as_str()),
                    (None, None) => "nothing".into(),
                },
                minute(p.until_s)
            ));
        }
        for (site, hubs) in &self.serve_from {
            lines.push(format!(
                "{site} may be served from hubs {}",
                crate::event::hub_list(hubs)
            ));
        }
        for (site, until_s) in &self.pads {
            lines.push(format!(
                "{site} takes drops at its ground pad until minute {}",
                minute(*until_s)
            ));
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replies_parse_as_orders_queries_or_nothing() {
        let reply = "Sure:\n```json\n{\"orders\": [{\"reposition\": {\"class\": \"fixed_wing\", \
                     \"count\": 4, \"from_hub\": 2, \"to_hub\": 5}}, {\"fly\": {}}, \
                     {\"avoid\": {\"area\": {\"circle\": {\"center_km\": [1, 2], \"radius_km\": 3}}, \
                     \"until_min\": 90}}]}\n```";
        match parse_reply(reply) {
            Reply::Orders { orders, unreadable } => {
                assert_eq!(unreadable, 1);
                assert_eq!(orders.len(), 2);
                assert_eq!(
                    orders[0],
                    Order::Reposition {
                        class: Some(VehicleClass::FixedWing),
                        count: 4,
                        from_hub: 2,
                        to_hub: 5,
                        until_min: None,
                    }
                );
                assert_eq!(
                    orders[1].describe(),
                    "avoid the circle of radius 3.0 km centered at (1.0, 2.0) km until minute 90"
                );
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            parse_reply("{\"query\": {\"hub_status\": {\"hub\": 3}}}"),
            Reply::Query(Query::HubStatus { hub: 3 })
        );
        assert_eq!(
            parse_reply("{\"orders\": []}"),
            Reply::Orders {
                orders: vec![],
                unreadable: 0
            }
        );
        assert_eq!(parse_reply("no json"), Reply::Unreadable);
        assert_eq!(parse_reply("{\"plan\": 1}"), Reply::Unreadable);
    }

    #[test]
    fn veto_replies_name_the_orders_to_drop() {
        assert_eq!(parse_veto("ok {\"drop\": [2, 5]}"), Some(vec![2, 5]));
        assert_eq!(parse_veto("{\"drop\": []}"), Some(vec![]));
        assert_eq!(parse_veto("{\"orders\": []}"), None);
        assert_eq!(parse_veto("none"), None);
    }

    #[test]
    fn rules_expire_and_answer_grounding_and_priority() {
        let mut rules = Rules::default();
        rules.grounded.push(Grounded {
            class: Some(VehicleClass::Multirotor),
            hubs: vec![1, 2],
            until_s: 600.0,
        });
        assert!(rules.is_grounded(1, VehicleClass::Multirotor));
        assert!(!rules.is_grounded(1, VehicleClass::FixedWing));
        assert!(!rules.is_grounded(3, VehicleClass::Multirotor));
        rules.expire(601.0);
        assert!(rules.grounded.is_empty());
        assert_eq!(rules.describe(), Vec::<String>::new());
    }
}

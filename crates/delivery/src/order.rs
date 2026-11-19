//! The orders a fleet manager can give, the rules they leave in force, and
//! how a model's reply is read. An order changes how the dispatcher and the
//! couriers behave; it never assigns a single request. The dispatcher checks
//! each order against the region and applies it (`dispatcher.rs`).
//!
//! Orders are written as JSON objects with one key, the order's name, so a
//! model's reply and a mission's scripted list use the same form:
//! `{"reposition": {"class": "fixed_wing", "count": 4, "from_hub": 2, "to_hub": 5}}`.
//! Times are minutes of the day, distances kilometers.

use serde::{Deserialize, Serialize};

use crate::area::{Area, tagged};
use crate::messages::{Urgency, VehicleClass};

/// What became of an order. A refusal names its reason, which is also the
/// score column that counts it (`refused_<reason>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Applied,
    /// Applied for what was there, less than the order asked.
    Partial,
    Refused(&'static str),
}

impl Outcome {
    pub fn is_refused(self) -> bool {
        matches!(self, Self::Refused(_))
    }
}

impl std::fmt::Display for Outcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Applied => f.write_str("applied"),
            Self::Partial => f.write_str("partial"),
            Self::Refused(reason) => write!(f, "refused_{reason}"),
        }
    }
}

impl Serialize for Outcome {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

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
    /// From the help manager only: let `from_hub` serve `hub`'s sites
    /// until the event `hub` asked over ends; its aircraft fly each request
    /// and return home (`help.rs`).
    Help { hub: u32, from_hub: u32 },
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
            Order::Help { .. } => "help",
        }
    }

    /// An order only the help manager may give.
    pub fn is_top(&self) -> bool {
        matches!(self, Order::Help { .. })
    }

    /// The sites an order names; with `hubs_named`, what a cluster's manager
    /// must keep within its own cluster.
    pub fn sites_named(&self) -> Vec<&str> {
        match self {
            Order::Reserve { site, .. }
            | Order::ServeFrom { site, .. }
            | Order::UsePad { site, .. } => {
                vec![site.as_str()]
            }
            Order::Prioritize { site, .. } => site.iter().map(String::as_str).collect(),
            _ => Vec::new(),
        }
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
            Order::Help { hub, from_hub, .. } => vec![*hub, *from_hub],
            Order::Ground { hubs, .. } | Order::ServeFrom { hubs, .. } => hubs.clone(),
            Order::Avoid { .. } | Order::Prioritize { .. } | Order::UsePad { .. } => Vec::new(),
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
            Order::Help { hub, from_hub } => format!("help hub {hub} from hub {from_hub}"),
        }
    }
}

/// What the help manager is shown, once per call.
pub const HELP_SYNTAX: &str = "\
Orders, each a JSON object with one key:
- {\"help\": {\"hub\": H, \"from_hub\": D}}: let hub D serve hub H's sites until H's event ends; D's aircraft fly each request and return home, D keeps half of each class idle for its own sites, and flies only what it can deliver on time. A hub helps one hub at a time. Take D from an offer, or grant nothing.";

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replies_parse_as_orders_or_nothing() {
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
}

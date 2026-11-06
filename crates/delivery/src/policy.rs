//! A written policy: the fleet manager's rules as data, one rule per event
//! kind and one for check-ins, each a list of order templates whose hubs,
//! sites, items and amounts are filled from the event and the fleet state.
//! This is the form a model writes once instead of being called per event
//! (plan direction 3). The hand-written replanner is expressed in it rule for
//! rule in `policies/replanner.yaml`, which checks that the form can say what
//! the replanner does.
//!
//! The language is written out for the model in `policies/SYNTAX.md`. The
//! templates use the replanner's own helpers for "nearest" and "spare", so a
//! policy decides what to do and how much; the dispatcher still checks every
//! order it produces.
//!
//! Steps that act on one hub (a reposition or transfer to the event's hubs,
//! serving a hub's sites, grounding or reserving at each hub) run for each of
//! the event's hubs in turn, in the order written; steps about the event as
//! a whole (prioritize, avoid, use_pad, a ground or reserve naming hubs) run
//! once after them. That is the replanner's order of orders.

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::area::tagged;
use crate::event::{Event, EventKind};
use crate::messages::{Urgency, VehicleClass};
use crate::order::Order;
use crate::replanner::{nearest_donors, repositions, share, stock_donors, top_up};
use crate::view::Snapshot;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub rules: Vec<Rule>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub when: When,
    #[serde(rename = "do", deserialize_with = "tagged")]
    pub steps: Vec<Step>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum When {
    Surge,
    Shortage,
    Closure,
    Outage,
    Weather,
    Priority,
    RooftopClosed,
    Corridor,
    CheckIn,
}

impl When {
    fn kind(self) -> Option<EventKind> {
        Some(match self {
            When::Surge => EventKind::Surge,
            When::Shortage => EventKind::Shortage,
            When::Closure => EventKind::Closure,
            When::Outage => EventKind::Outage,
            When::Weather => EventKind::Weather,
            When::Priority => EventKind::Priority,
            When::RooftopClosed => EventKind::RooftopClosed,
            When::Corridor => EventKind::Corridor,
            When::CheckIn => return None,
        })
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Step {
    Reposition {
        class: VehicleClass,
        count: u32,
        /// `event_end`: the vehicles fly home once idle after the event.
        #[serde(default)]
        until: Option<UntilWord>,
    },
    Transfer {
        #[serde(default)]
        item: Item,
        units: Units,
        from: Donors,
    },
    ServeFrom {
        hubs: Donors,
    },
    Ground {
        hubs: Hubs,
        #[serde(default)]
        class: ClassRef,
    },
    Reserve {
        hub: Hubs,
        count: u32,
        site: SiteRef,
    },
    Prioritize {
        #[serde(default)]
        site: Option<SiteRef>,
        #[serde(default)]
        urgency: Option<Urgency>,
    },
    Avoid {},
    UsePad {},
    TopUp {
        below: Units,
        units: Units,
    },
}

impl Step {
    fn name(&self) -> &'static str {
        match self {
            Step::Reposition { .. } => "reposition",
            Step::Transfer { .. } => "transfer",
            Step::ServeFrom { .. } => "serve_from",
            Step::Ground { .. } => "ground",
            Step::Reserve { .. } => "reserve",
            Step::Prioritize { .. } => "prioritize",
            Step::Avoid {} => "avoid",
            Step::UsePad {} => "use_pad",
            Step::TopUp { .. } => "top_up",
        }
    }

    /// Runs once per event hub, rather than once per event.
    fn per_hub(&self) -> bool {
        match self {
            Step::Reposition { .. } | Step::Transfer { .. } | Step::ServeFrom { .. } => true,
            Step::Ground { hubs, .. } | Step::Reserve { hub: hubs, .. } => *hubs == Hubs::EachHub,
            Step::Prioritize { .. } | Step::Avoid {} | Step::UsePad {} | Step::TopUp { .. } => {
                false
            }
        }
    }
}

/// Which hubs a step acts on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Hubs {
    /// All of the event's hubs in one order.
    EventHubs,
    /// One order per event hub, in turn.
    EachHub,
}

/// Where stock or service comes from, relative to a hub.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Donors {
    #[serde(rename = "nearest")]
    Nearest(u32),
    #[serde(rename = "nearest_with_stock")]
    NearestWithStock(u32),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum Item {
    Word(ItemWord),
    Named(String),
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemWord {
    EventItem,
}

impl Default for Item {
    fn default() -> Self {
        Item::Word(ItemWord::EventItem)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum SiteRef {
    Word(SiteWord),
    Code(String),
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SiteWord {
    EventSite,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(untagged)]
pub enum Units {
    Fixed(u32),
    Word(UnitsWord),
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitsWord {
    HalfStock,
    QuarterStock,
}

impl Units {
    fn of(self, snapshot: &Snapshot<'_>) -> u32 {
        match self {
            Units::Fixed(units) => units,
            Units::Word(UnitsWord::HalfStock) => share(snapshot, 0.5),
            Units::Word(UnitsWord::QuarterStock) => share(snapshot, 0.25),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UntilWord {
    EventEnd,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClassRef {
    #[default]
    EventClass,
    FixedWing,
    Multirotor,
    Any,
}

impl Policy {
    pub fn parse(text: &str) -> Result<Self> {
        let rules: Vec<Rule> = serde_yaml_ng::from_str(text).context("reading the policy")?;
        let policy = Policy { rules };
        policy.validate()?;
        Ok(policy)
    }

    fn validate(&self) -> Result<()> {
        for rule in &self.rules {
            for step in &rule.steps {
                let is_top_up = matches!(step, Step::TopUp { .. });
                ensure!(
                    (rule.when == When::CheckIn) == is_top_up,
                    "policy: {} does not belong to a {:?} rule",
                    step.name(),
                    rule.when
                );
                ensure!(
                    !matches!(step, Step::Avoid {})
                        || matches!(rule.when, When::Closure | When::Corridor),
                    "policy: avoid needs an event with an area (closure or corridor)"
                );
            }
        }
        Ok(())
    }

    /// The orders for the events that just started, or the check-in rule.
    pub fn plan(&self, new_events: &[&Event], snapshot: &Snapshot<'_>) -> Vec<Order> {
        let mut orders = Vec::new();
        if new_events.is_empty() {
            for rule in self.rules.iter().filter(|r| r.when == When::CheckIn) {
                for step in &rule.steps {
                    if let Step::TopUp { below, units } = step {
                        orders.extend(top_up(snapshot, below.of(snapshot), units.of(snapshot)));
                    }
                }
            }
            return orders;
        }
        for event in new_events {
            for rule in self
                .rules
                .iter()
                .filter(|r| r.when.kind() == Some(event.kind))
            {
                for &hub in &event.hubs {
                    for step in rule.steps.iter().filter(|s| s.per_hub()) {
                        orders.extend(apply(step, event, Some(hub), snapshot));
                    }
                }
                for step in rule.steps.iter().filter(|s| !s.per_hub()) {
                    orders.extend(apply(step, event, None, snapshot));
                }
            }
        }
        orders
    }
}

/// The orders one step gives for an event, at one of its hubs or for all.
fn apply(step: &Step, event: &Event, hub: Option<u32>, snapshot: &Snapshot<'_>) -> Vec<Order> {
    let until_min = if event.until_min.is_finite() {
        event.until_min
    } else {
        snapshot.day_s / 60.0 + 60.0
    };
    let hubs = match hub {
        Some(hub) => vec![hub],
        None => event.hubs.clone(),
    };
    let site_of = |site: &SiteRef| match site {
        SiteRef::Word(SiteWord::EventSite) => event.site.clone(),
        SiteRef::Code(code) => code.clone(),
    };
    let donors_of = |donors: &Donors, hub: u32, item: &str| -> Vec<u32> {
        match *donors {
            Donors::Nearest(count) => nearest_donors(snapshot, hub, &event.hubs, count as usize),
            Donors::NearestWithStock(count) => stock_donors(snapshot, hub, &event.hubs, item)
                .iter()
                .take(count as usize)
                .map(|(donor, _)| *donor)
                .collect(),
        }
    };
    let mut orders = Vec::new();
    match step {
        Step::Reposition {
            class,
            count,
            until,
        } => {
            for hub in hubs {
                let mut planned = repositions(snapshot, hub, &event.hubs, *class, *count);
                if until.is_some() {
                    for order in &mut planned {
                        if let Order::Reposition { until_min: u, .. } = order {
                            *u = Some(until_min);
                        }
                    }
                }
                orders.extend(planned);
            }
        }
        Step::Transfer { item, units, from } => {
            let item = match item {
                Item::Word(ItemWord::EventItem) => event.item.clone(),
                Item::Named(name) => name.clone(),
            };
            for hub in hubs {
                let spare = stock_donors(snapshot, hub, &event.hubs, &item);
                for donor in donors_of(from, hub, &item) {
                    let can = spare
                        .iter()
                        .find(|(d, _)| *d == donor)
                        .map_or(0, |(_, spare)| *spare);
                    orders.push(Order::Transfer {
                        item: item.clone(),
                        units: units.of(snapshot).min(can),
                        from_hub: donor,
                        to_hub: hub,
                    });
                }
            }
        }
        Step::ServeFrom { hubs: donors } => {
            for hub in hubs {
                let donors = donors_of(donors, hub, &event.item);
                if donors.is_empty() {
                    continue;
                }
                for site in snapshot.region.sites_of(hub) {
                    orders.push(Order::ServeFrom {
                        site: site.code.clone(),
                        hubs: donors.clone(),
                    });
                }
            }
        }
        Step::Ground { class, .. } => {
            // An outage known in advance is not grounded before it starts.
            if event.at_s() <= snapshot.time_s {
                orders.push(Order::Ground {
                    class: match class {
                        ClassRef::EventClass => event.class,
                        ClassRef::FixedWing => Some(VehicleClass::FixedWing),
                        ClassRef::Multirotor => Some(VehicleClass::Multirotor),
                        ClassRef::Any => None,
                    },
                    hubs,
                    until_min,
                });
            }
        }
        Step::Reserve { count, site, .. } => {
            for hub in hubs {
                orders.push(Order::Reserve {
                    hub,
                    count: *count,
                    site: site_of(site),
                    until_min,
                });
            }
        }
        Step::Prioritize { site, urgency } => orders.push(Order::Prioritize {
            site: site.as_ref().map(site_of),
            urgency: *urgency,
            until_min,
        }),
        Step::Avoid {} => {
            if let Some(area) = event.area {
                orders.push(Order::Avoid { area, until_min });
            }
        }
        Step::UsePad {} => orders.push(Order::UsePad {
            site: event.site.clone(),
            until_min,
        }),
        Step::TopUp { .. } => {}
    }
    orders
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_replanner_policy_reads_and_a_misplaced_step_is_refused() {
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/policies/replanner.yaml"
        ))
        .unwrap();
        let policy = Policy::parse(&text).unwrap();
        assert_eq!(policy.rules.len(), 9);
        assert!(Policy::parse("- when: surge\n  do: [{avoid: {}}]").is_err());
        assert!(Policy::parse("- when: surge\n  do: [{top_up: {below: 1, units: 2}}]").is_err());
        assert!(Policy::parse("- when: surge\n  do: [{fly: {}}]").is_err());
    }
}

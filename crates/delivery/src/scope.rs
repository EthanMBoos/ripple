//! What one fleet manager is responsible for: the whole region, or one
//! cluster's hubs. The scope decides three things. It filters what the
//! manager is shown: the hubs and vehicles in its snapshot, the events
//! (`events_shown`), the rules in force, its feedback and tally, all through
//! `covers_hub`. It gates what the manager may order: an order naming a hub
//! or site outside is refused (`covers_order`). And it places the rules the
//! manager gives that name no place of their own: an avoid or a priority by
//! urgency carries the giver's scope and binds only within it (`Rules` in
//! `rules.rs`). Every other rule is placed by the hub or site it names.
//!
//! The three fleet APIs of the paper are three lists of managers: one with
//! `Region`; one `Cluster` per block of hubs; and the clusters with a
//! `Region` manager above them that only grants help between clusters.

use crate::event::{Event, EventsConfig};
use crate::order::Order;
use crate::region::Region;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    /// Every hub: one manager over the region, or the manager above the
    /// clusters.
    Region,
    /// One cluster's hubs.
    Cluster(Vec<u32>),
}

impl Scope {
    pub fn covers_hub(&self, hub: u32) -> bool {
        match self {
            Scope::Region => true,
            Scope::Cluster(hubs) => hubs.contains(&hub),
        }
    }

    /// Whether a site is at one of the scope's hubs. A site that does not
    /// exist is covered: the order naming it is refused by name when applied.
    pub fn covers_site(&self, code: &str, region: &Region) -> bool {
        match region.site(code) {
            Some(site) => self.covers_hub(site.hub),
            None => true,
        }
    }

    /// Whether an event concerns the scope: it names one of its hubs, or
    /// names no hub (an area only).
    pub fn covers_event(&self, event: &Event) -> bool {
        event.hubs.is_empty() || event.hubs.iter().any(|h| self.covers_hub(*h))
    }

    /// Whether every hub and site an order names is within the scope.
    pub fn covers_order(&self, order: &Order, region: &Region) -> bool {
        order.hubs_named().iter().all(|h| self.covers_hub(*h))
            && order
                .sites_named()
                .iter()
                .all(|code| self.covers_site(code, region))
    }

    /// Whether a rule placed by this scope reaches a manager of `other`: the
    /// two share a hub, or either is the whole region.
    pub fn reaches(&self, other: &Scope) -> bool {
        match (self, other) {
            (Scope::Cluster(mine), Scope::Cluster(theirs)) => {
                mine.iter().any(|h| theirs.contains(h))
            }
            _ => true,
        }
    }

    /// The events as the scope's manager is told them: those that concern
    /// it, with the hubs outside left out and the text written again for the
    /// shorter list.
    pub fn events_shown(&self, events: &[&Event], texts: &EventsConfig) -> Vec<Event> {
        events
            .iter()
            .filter(|e| self.covers_event(e))
            .map(|e| {
                let mut event = (*e).clone();
                if let Scope::Cluster(hubs) = self {
                    let before = event.hubs.len();
                    event.hubs.retain(|h| hubs.contains(h));
                    if event.hubs.len() < before {
                        event.text = texts.text(&event);
                    }
                }
                event
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::event::EventKind;
    use crate::messages::Urgency;
    use crate::region::RegionConfig;

    fn four_hubs() -> Region {
        Region::new(&RegionConfig {
            hubs: 4,
            hub_spacing_m: 15_000.0,
            ..RegionConfig::default()
        })
        .unwrap()
    }

    #[test]
    fn a_trimmed_events_text_names_only_the_clusters_hubs() {
        let texts = EventsConfig {
            text: BTreeMap::from([(
                EventKind::Surge,
                "A surge near {hubs} from minute {from}.".into(),
            )]),
            ..EventsConfig::default()
        };
        let mut surge = Event {
            id: 1,
            kind: EventKind::Surge,
            at_min: 10.0,
            until_min: 70.0,
            hubs: vec![1, 2, 3],
            ..Event::default()
        };
        surge.text = texts.text(&surge);
        let corridor = Event {
            id: 2,
            kind: EventKind::Corridor,
            at_min: 20.0,
            until_min: 80.0,
            text: "a corridor".into(),
            ..Event::default()
        };
        let all = [&surge, &corridor];
        let mine = Scope::Cluster(vec![1, 2]).events_shown(&all, &texts);
        assert_eq!(mine.len(), 2);
        assert_eq!(mine[0].hubs, vec![1, 2]);
        assert_eq!(mine[0].text, "A surge near hubs 1, 2 from minute 10.");
        assert_eq!(
            mine[1].text, "a corridor",
            "an event naming no hub is shared"
        );
        let theirs = Scope::Cluster(vec![4]).events_shown(&all, &texts);
        assert_eq!(theirs.len(), 1, "only the corridor reaches hub 4's cluster");
        let flat = Scope::Region.events_shown(&all, &texts);
        assert_eq!(
            flat[0].text, surge.text,
            "a flat manager's text is unchanged"
        );
    }

    #[test]
    fn an_order_naming_a_hub_or_site_outside_is_not_covered() {
        let region = four_hubs();
        let cluster = Scope::Cluster(vec![1, 2]);
        let inside = Order::Reserve {
            hub: 1,
            count: 1,
            site: "1H1".into(),
            until_min: 60.0,
        };
        let pad = Order::UsePad {
            site: "3H1".into(),
            until_min: 60.0,
        };
        let lend = Order::ServeFrom {
            site: "1H1".into(),
            hubs: vec![3],
        };
        let trauma = Order::Prioritize {
            site: None,
            urgency: Some(Urgency::Trauma),
            until_min: 60.0,
        };
        let unknown = Order::UsePad {
            site: "9H9".into(),
            until_min: 60.0,
        };
        assert!(cluster.covers_order(&inside, &region));
        assert!(!cluster.covers_order(&pad, &region));
        assert!(!cluster.covers_order(&lend, &region));
        assert!(
            cluster.covers_order(&trauma, &region),
            "placed by the scope when applied"
        );
        assert!(
            cluster.covers_order(&unknown, &region),
            "refused by name later"
        );
        assert!(Scope::Region.covers_order(&lend, &region));
    }

    #[test]
    fn a_rule_reaches_the_scopes_that_share_a_hub() {
        let one = Scope::Cluster(vec![1, 2]);
        let two = Scope::Cluster(vec![3, 4]);
        let both = Scope::Cluster(vec![2, 3]);
        assert!(!one.reaches(&two));
        assert!(one.reaches(&both));
        assert!(Scope::Region.reaches(&two));
        assert!(two.reaches(&Scope::Region));
    }
}

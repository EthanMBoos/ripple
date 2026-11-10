//! The rules the fleet managers' orders leave in force, kept by the
//! dispatcher and read at every assignment: reserves, avoided areas,
//! groundings, priorities, extra serving hubs, and open pads. Each rule
//! binds where it is placed. A reserve, a grounding, a pad, a serve-from, and
//! a site priority are placed by the hub or site they name. An avoid and a
//! priority by urgency name no place, so they carry the scope of the manager
//! that gave them (`scope.rs`): a cluster's binds its own hubs' flights and
//! requests, a region manager's binds everything.

use std::collections::BTreeMap;

use crate::area::Area;
use crate::messages::{Request, Urgency, VehicleClass};
use crate::region::Region;
use crate::scope::Scope;

/// The rules in force.
#[derive(Clone, Debug, Default)]
pub struct Rules {
    pub reserves: Vec<Reserve>,
    pub avoids: Vec<Avoid>,
    pub grounded: Vec<Grounded>,
    pub priorities: Vec<Priority>,
    /// Extra hubs allowed to serve a site, by site code.
    pub serve_from: BTreeMap<String, Vec<u32>>,
    /// Hospitals whose pad is in use, by site code, until when.
    pub pads: BTreeMap<String, f64>,
    /// Hubs serving another hub's sites for a time, granted by the manager
    /// above the clusters (`help.rs`).
    pub lends: Vec<Lend>,
}

/// One hub's aircraft may serve another hub's sites, and come home after
/// each flight, until `until_s`.
#[derive(Clone, Debug)]
pub struct Lend {
    pub hub: u32,
    pub from_hub: u32,
    pub until_s: f64,
}

#[derive(Clone, Debug)]
pub struct Reserve {
    pub hub: u32,
    pub count: u32,
    pub site: String,
    pub until_s: f64,
}

#[derive(Clone, Debug)]
pub struct Avoid {
    pub area: Area,
    pub until_s: f64,
    /// Whose flights keep out: those from the giver's hubs.
    pub scope: Scope,
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
    /// Whose requests come first by urgency: those at the giver's hubs. A
    /// priority naming a site is placed by the site and carries `Region`.
    pub scope: Scope,
}

impl Rules {
    pub fn expire(&mut self, now_s: f64) {
        self.reserves.retain(|r| r.until_s >= now_s);
        self.avoids.retain(|a| a.until_s >= now_s);
        self.grounded.retain(|g| g.until_s >= now_s);
        self.priorities.retain(|p| p.until_s >= now_s);
        self.pads.retain(|_, until_s| *until_s >= now_s);
        self.lends.retain(|l| l.until_s >= now_s);
    }

    /// The hubs lent to `hub`'s sites now.
    pub fn lenders(&self, hub: u32) -> impl Iterator<Item = u32> + '_ {
        self.lends
            .iter()
            .filter(move |l| l.hub == hub)
            .map(|l| l.from_hub)
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
                || (p.urgency.is_some_and(|u| u == request.urgency)
                    && p.scope.covers_hub(request.hub))
        })
    }

    pub fn uses_pad(&self, site: &str) -> bool {
        self.pads.contains_key(site)
    }

    /// The areas a flight launching from `hub` keeps out of.
    pub fn areas_for(&self, hub: u32) -> Vec<&Area> {
        self.avoids
            .iter()
            .filter(|a| a.scope.covers_hub(hub))
            .map(|a| &a.area)
            .collect()
    }

    /// The rules in force as a manager of `scope` is shown them, one line
    /// each: those placed at its hubs or sites, and those whose giver's
    /// scope reaches it. A region manager is shown every rule.
    pub fn shown_to(&self, scope: &Scope, region: &Region) -> Vec<String> {
        let minute = |s: f64| format!("{:.0}", s / 60.0);
        let at_hub = |hub: u32| scope.covers_hub(hub);
        let at_site = |site: &str| scope.covers_site(site, region);
        let mut lines = Vec::new();
        for r in self.reserves.iter().filter(|r| at_hub(r.hub)) {
            lines.push(format!(
                "{} vehicles at hub {} reserved for {} until minute {}",
                r.count,
                r.hub,
                r.site,
                minute(r.until_s)
            ));
        }
        for a in self.avoids.iter().filter(|a| a.scope.reaches(scope)) {
            lines.push(format!(
                "flights avoid {} until minute {}",
                a.area.describe(),
                minute(a.until_s)
            ));
        }
        for g in self
            .grounded
            .iter()
            .filter(|g| g.hubs.iter().any(|h| at_hub(*h)))
        {
            lines.push(format!(
                "{} grounded at hubs {} until minute {}",
                g.class.map_or("all vehicles", |c| c.as_str()),
                crate::event::hub_list(&g.hubs),
                minute(g.until_s)
            ));
        }
        for p in self.priorities.iter().filter(|p| match &p.site {
            Some(site) => at_site(site),
            None => p.scope.reaches(scope),
        }) {
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
        for (site, hubs) in self
            .serve_from
            .iter()
            .filter(|(site, hubs)| at_site(site) || hubs.iter().any(|h| at_hub(*h)))
        {
            lines.push(format!(
                "{site} may be served from hubs {}",
                crate::event::hub_list(hubs)
            ));
        }
        for (site, until_s) in self.pads.iter().filter(|(site, _)| at_site(site)) {
            lines.push(format!(
                "{site} takes drops at its ground pad until minute {}",
                minute(*until_s)
            ));
        }
        for l in self
            .lends
            .iter()
            .filter(|l| at_hub(l.hub) || at_hub(l.from_hub))
        {
            lines.push(format!(
                "hub {} serves hub {}'s sites until minute {}",
                l.from_hub,
                l.hub,
                minute(l.until_s)
            ));
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::RequestKind;
    use crate::region::RegionConfig;

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
        assert_eq!(
            rules.shown_to(&Scope::Region, &four_hubs()),
            Vec::<String>::new()
        );
    }

    fn four_hubs() -> Region {
        Region::new(&RegionConfig {
            hubs: 4,
            hub_spacing_m: 15_000.0,
            ..RegionConfig::default()
        })
        .unwrap()
    }

    fn request_at(hub: u32, site: &str, urgency: Urgency) -> Request {
        Request {
            id: 1,
            kind: RequestKind::Delivery,
            hub,
            site: site.into(),
            site_world_m: ripple_core::Vec3::zeros(),
            rooftop: false,
            pad_world_m: ripple_core::Vec3::zeros(),
            item: "O-".into(),
            units: 1,
            urgency,
            opened_at_s: 0.0,
            deadline_s: 600.0,
            to_hub: None,
        }
    }

    #[test]
    fn a_clusters_avoid_and_urgency_priority_reach_only_its_own_hubs() {
        let area = Area::Circle {
            center_km: [0.0, 0.0],
            radius_km: 2.0,
        };
        let mut rules = Rules::default();
        rules.avoids.push(Avoid {
            area,
            until_s: 600.0,
            scope: Scope::Cluster(vec![1, 2]),
        });
        rules.priorities.push(Priority {
            site: None,
            urgency: Some(Urgency::Trauma),
            until_s: 600.0,
            scope: Scope::Cluster(vec![1, 2]),
        });
        assert_eq!(rules.areas_for(1).len(), 1);
        assert!(rules.areas_for(3).is_empty(), "another cluster's flights");
        assert!(rules.prioritized(&request_at(2, "2A", Urgency::Trauma)));
        assert!(!rules.prioritized(&request_at(3, "3A", Urgency::Trauma)));
        // A region manager's rules reach everything.
        rules.avoids[0].scope = Scope::Region;
        rules.priorities[0].scope = Scope::Region;
        assert_eq!(rules.areas_for(3).len(), 1);
        assert!(rules.prioritized(&request_at(3, "3A", Urgency::Trauma)));
    }

    #[test]
    fn a_cluster_is_shown_only_the_rules_that_reach_it() {
        let region = four_hubs();
        let mut rules = Rules::default();
        rules.reserves.push(Reserve {
            hub: 1,
            count: 2,
            site: "1H1".into(),
            until_s: 600.0,
        });
        rules.grounded.push(Grounded {
            class: None,
            hubs: vec![3],
            until_s: 600.0,
        });
        rules.priorities.push(Priority {
            site: Some("3H1".into()),
            urgency: None,
            until_s: 600.0,
            scope: Scope::Region,
        });
        rules.priorities.push(Priority {
            site: None,
            urgency: Some(Urgency::Trauma),
            until_s: 600.0,
            scope: Scope::Region,
        });
        rules.serve_from.insert("1A".into(), vec![2]);
        rules.pads.insert("3H1".into(), 600.0);
        rules.avoids.push(Avoid {
            area: Area::Circle {
                center_km: [0.0, 0.0],
                radius_km: 2.0,
            },
            until_s: 600.0,
            scope: Scope::Cluster(vec![3, 4]),
        });
        let all = rules.shown_to(&Scope::Region, &region);
        assert_eq!(all.len(), 7);
        let mine = rules.shown_to(&Scope::Cluster(vec![1, 2]), &region);
        // The reserve at hub 1, the region-wide trauma priority, and the
        // serve-from at site 1A; nothing at hubs 3 and 4.
        assert_eq!(mine.len(), 3, "{mine:?}");
        assert!(mine.iter().all(|line| !line.contains('3')), "{mine:?}");
        let theirs = rules.shown_to(&Scope::Cluster(vec![3, 4]), &region);
        assert_eq!(theirs.len(), 5, "{theirs:?}");
    }
}

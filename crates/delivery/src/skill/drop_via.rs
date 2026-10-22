//! Drop via a waypoint: fly to `via`, on to the site, release the package as
//! a drop does, and come back through `via`, where the courier takes over the
//! last leg home. The model picks the waypoint, so it can route a delivery
//! around an area, both ways, instead of giving it up.

use ripple_core::Vec3;

use super::{Settings, Skill, SkillInfo};
use crate::messages::{Assignment, Phase, SkillName};

pub const INFO: SkillInfo = SkillInfo {
    name: SkillName::DropVia,
    description: "like drop, but fly to the site and back through a waypoint you choose; use \
                  it to route around an area.",
    multirotor_only: false,
    rooftops: false,
    lands: false,
    via: true,
};

pub struct DropVia {
    request_id: i32,
    via_world_m: Vec3,
    site_world_m: Vec3,
    arrival_radius_m: f64,
    passed_via: bool,
    released: bool,
    back_at_via: bool,
}

impl DropVia {
    pub fn new(assignment: &Assignment, settings: Settings) -> Self {
        Self {
            request_id: assignment.request_id,
            // The dispatcher refuses a call without one; without it this is
            // a plain drop.
            via_world_m: assignment.via_world_m.unwrap_or(assignment.site_world_m),
            site_world_m: assignment.site_world_m,
            arrival_radius_m: settings.arrival_radius_m,
            passed_via: false,
            released: false,
            back_at_via: false,
        }
    }
}

fn horizontal_m(a: Vec3, b: Vec3) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}

impl Skill for DropVia {
    fn phase(&self) -> Phase {
        if self.released {
            Phase::Returning
        } else {
            Phase::Outbound
        }
    }

    fn advance(&mut self, _time_s: f64, position_world_m: Vec3) -> Option<i32> {
        let near = |point| horizontal_m(position_world_m, point) <= self.arrival_radius_m;
        if !self.passed_via {
            self.passed_via = near(self.via_world_m);
        } else if !self.released {
            if near(self.site_world_m) {
                self.released = true;
                return Some(self.request_id);
            }
        } else {
            self.back_at_via = near(self.via_world_m);
        }
        None
    }

    fn done(&self) -> bool {
        self.back_at_via
    }

    fn target(&self) -> (Vec3, bool) {
        let target = if self.passed_via && !self.released {
            self.site_world_m
        } else {
            self.via_world_m
        };
        (target, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_goes_and_returns_through_the_waypoint() {
        let assignment = Assignment {
            vehicle_id: 2,
            request_id: 7,
            skill: SkillName::DropVia,
            site_world_m: Vec3::new(1000.0, 0.0, 0.0),
            via_world_m: Some(Vec3::new(500.0, 500.0, 0.0)),
        };
        let settings = Settings {
            arrival_radius_m: 10.0,
            dwell_s: 30.0,
        };
        let mut skill = DropVia::new(&assignment, settings);
        assert_eq!(skill.target().0, Vec3::new(500.0, 500.0, 0.0));
        // Over the site before the waypoint: no release yet.
        assert_eq!(skill.advance(0.0, Vec3::new(1000.0, 0.0, 100.0)), None);
        assert_eq!(skill.advance(1.0, Vec3::new(500.0, 495.0, 100.0)), None);
        assert_eq!(skill.target().0, Vec3::new(1000.0, 0.0, 0.0));
        assert_eq!(skill.advance(2.0, Vec3::new(1005.0, 0.0, 100.0)), Some(7));
        assert!(!skill.done());
        assert_eq!(skill.target().0, Vec3::new(500.0, 500.0, 0.0));
        assert_eq!(skill.advance(3.0, Vec3::new(505.0, 500.0, 100.0)), None);
        assert!(skill.done());
    }
}

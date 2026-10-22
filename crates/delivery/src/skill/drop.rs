//! Drop: fly over the site and release the package by parachute once within
//! `arrival_radius_m` horizontally, on a pass for a fixed-wing aircraft or
//! hovering at cruise altitude for a multirotor. Done on release.

use ripple_core::Vec3;

use super::{Settings, Skill, SkillInfo};
use crate::messages::{Assignment, Phase, SkillName};

pub const INFO: SkillInfo = SkillInfo {
    name: SkillName::Drop,
    description: "fly over the site and drop the package by parachute (a multirotor hovers \
                  over it to release), then return to reload.",
    multirotor_only: false,
    rooftops: false,
    via: false,
    lands: false,
};

pub struct Drop {
    request_id: i32,
    site_world_m: Vec3,
    arrival_radius_m: f64,
    released: bool,
}

impl Drop {
    pub fn new(assignment: &Assignment, settings: Settings) -> Self {
        Self {
            request_id: assignment.request_id,
            site_world_m: assignment.site_world_m,
            arrival_radius_m: settings.arrival_radius_m,
            released: false,
        }
    }
}

impl Skill for Drop {
    fn phase(&self) -> Phase {
        Phase::Outbound
    }

    fn advance(&mut self, _time_s: f64, position_world_m: Vec3) -> Option<i32> {
        let offset_m = position_world_m - self.site_world_m;
        if !self.released && offset_m.x.hypot(offset_m.y) <= self.arrival_radius_m {
            self.released = true;
            return Some(self.request_id);
        }
        None
    }

    fn done(&self) -> bool {
        self.released
    }

    fn target(&self) -> (Vec3, bool) {
        (self.site_world_m, false)
    }
}

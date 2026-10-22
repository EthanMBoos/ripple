//! Land (multirotor only): fly to the site, descend onto it, hand over the
//! package on touchdown within `arrival_radius_m`, and stay `dwell_s`. Done
//! when the dwell ends.

use ripple_core::Vec3;

use super::{Settings, Skill, SkillInfo};
use crate::messages::{Assignment, Phase, SkillName};

pub const INFO: SkillInfo = SkillInfo {
    name: SkillName::Land,
    description: "fly to the site, land, hand over the package, then return to reload.",
    multirotor_only: true,
    rooftops: true,
    via: false,
    lands: true,
};

pub struct Land {
    request_id: i32,
    site_world_m: Vec3,
    arrival_radius_m: f64,
    dwell_s: f64,
    /// When the dwell ends, once landed.
    leaves_at_s: Option<f64>,
    left: bool,
}

impl Land {
    pub fn new(assignment: &Assignment, settings: Settings) -> Self {
        Self {
            request_id: assignment.request_id,
            site_world_m: assignment.site_world_m,
            arrival_radius_m: settings.arrival_radius_m,
            dwell_s: settings.dwell_s,
            leaves_at_s: None,
            left: false,
        }
    }
}

impl Skill for Land {
    fn phase(&self) -> Phase {
        if self.leaves_at_s.is_some() {
            Phase::Landed
        } else {
            Phase::Outbound
        }
    }

    fn advance(&mut self, time_s: f64, position_world_m: Vec3) -> Option<i32> {
        match self.leaves_at_s {
            None if (position_world_m - self.site_world_m).norm() <= self.arrival_radius_m => {
                self.leaves_at_s = Some(time_s + self.dwell_s);
                Some(self.request_id)
            }
            Some(leaves_at_s) if time_s >= leaves_at_s => {
                self.left = true;
                None
            }
            _ => None,
        }
    }

    fn done(&self) -> bool {
        self.left
    }

    fn target(&self) -> (Vec3, bool) {
        (self.site_world_m, true)
    }
}

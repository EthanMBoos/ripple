//! The skills a courier can be told to run at a site. A skill is a behavior
//! packaged so the dispatcher can call it: a name, the rules the dispatcher
//! checks before sending a call, and the flying itself, behind the `Skill`
//! trait. The courier flies the legs to and from the site, through a waypoint
//! when the assignment gives one, and the skill takes over at the site.
//!
//! To add a skill: write it in its own file under `skill/` with an `INFO` and
//! a struct implementing `Skill`, add its name to `SkillName` (messages.rs),
//! and add it to `LIBRARY` and `start` below.

use ripple_core::Vec3;

use crate::messages::{Assignment, Phase, SkillName};

mod drop;
mod land;

/// One running skill on one courier.
pub trait Skill: Send {
    /// What the courier reports while it runs.
    fn phase(&self) -> Phase;
    /// Advance one tick; returns the request it delivered this tick, if any.
    fn advance(&mut self, time_s: f64, position_world_m: Vec3) -> Option<i32>;
    fn done(&self) -> bool;
    /// Where to fly, and whether a multirotor descends onto it.
    fn target(&self) -> (Vec3, bool);
}

/// What the dispatcher knows about a skill.
pub struct SkillInfo {
    pub name: SkillName,
    /// Only a multirotor can run it.
    pub multirotor_only: bool,
    /// It stays at the site for the courier's dwell time.
    pub lands: bool,
}

pub const LIBRARY: [SkillInfo; 2] = [drop::INFO, land::INFO];

pub fn info(name: SkillName) -> &'static SkillInfo {
    LIBRARY
        .iter()
        .find(|skill| skill.name == name)
        .expect("every skill name is in the library")
}

/// The courier's settings a skill uses.
#[derive(Clone, Copy)]
pub struct Settings {
    pub arrival_radius_m: f64,
    pub dwell_s: f64,
}

/// Start the skill an assignment calls for.
pub fn start(assignment: &Assignment, settings: Settings) -> Box<dyn Skill> {
    match assignment.skill {
        SkillName::Drop => Box::new(drop::Drop::new(assignment, settings)),
        SkillName::Land => Box::new(land::Land::new(assignment, settings)),
    }
}

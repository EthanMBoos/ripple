//! The skills a courier can be told to run. A skill is a behavior packaged so
//! the dispatcher can call it: a name and a description the model reads, the
//! rules the dispatcher checks before sending a call, and the flying itself,
//! behind the `Skill` trait.
//!
//! To add a skill: write it in its own file under `skill/` with an `INFO` and
//! a struct implementing `Skill`, add its name to `SkillName` (messages.rs),
//! and add it to `LIBRARY` and `start` below. The prompt and the call check
//! pick it up from there.

use ripple_core::Vec3;

use crate::messages::{Assignment, Phase, SkillName};

mod drop;
mod drop_via;
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

/// What the dispatcher and the model know about a skill.
pub struct SkillInfo {
    pub name: SkillName,
    /// The model's description, after `- <name>: ` in the prompt.
    pub description: &'static str,
    /// Only a multirotor can run it.
    pub multirotor_only: bool,
    /// It can serve a rooftop.
    pub rooftops: bool,
    /// It stays at the site for the courier's dwell time.
    pub lands: bool,
    /// The call gives a waypoint, `via`, to pass on the way.
    pub via: bool,
}

/// Every skill, in the order the prompt lists them. A mission offers the model
/// some of them (the Dispatcher's `skills`).
pub const LIBRARY: [SkillInfo; 3] = [drop::INFO, land::INFO, drop_via::INFO];

pub fn info(name: SkillName) -> &'static SkillInfo {
    LIBRARY
        .iter()
        .find(|skill| skill.name == name)
        .expect("every skill name is in the library")
}

/// The skill a model's reply names, ignoring case and spaces.
pub fn named(text: &str) -> Option<SkillName> {
    let text = text.trim().to_lowercase();
    LIBRARY
        .iter()
        .find(|skill| skill.name.as_str() == text)
        .map(|skill| skill.name)
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
        SkillName::DropVia => Box::new(drop_via::DropVia::new(assignment, settings)),
    }
}

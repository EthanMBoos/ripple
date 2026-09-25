//! SCRIMMAGE simulation and legacy mission compatibility.
//! Based on SCRIMMAGE's Ubuntu-24.04 branch.
mod autonomy;
mod common;
mod controller;
mod entity;
mod entity_interaction;
mod math;
mod metrics;
mod motion;
mod parse;
pub mod plugin;
mod plugin_manager;
mod protocol;
mod pubsub;
mod scenario;
mod sensor;
mod simcontrol;

pub use common::{Pid, PidGains};
pub use entity::{Entity, EntityKind, EntitySnapshot};
pub use math::{EulerAngles, KinematicState, Quaternion, Vec3, rk4};
pub use parse::{Mission, Params};
pub use protocol::{read_frames, write_frame};
pub use scenario::{
    EndConditions, EntityGroupConfig, PluginConfig, RunConfig, ScenarioConfig, Spawn,
};
pub use simcontrol::{Event, EventKind, Simulation, SimulationFrame, TerminationReason};

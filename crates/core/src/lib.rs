//! Ripple's simulation core: missions, entities, plugins, and the tick.
mod autonomy;
mod common;
mod controller;
mod entity;
mod entity_interaction;
mod estimator;
mod math;
mod metrics;
mod motion;
mod parse;
pub mod plugin;
mod plugin_manager;
mod pubsub;
pub mod record;
mod scenario;
mod sensor;
mod simcontrol;

pub use common::{AngleGains, GeographicOrigin, LengthGains, Pid, PidGains, SpeedGains, Terrain};
pub use entity::{Entity, EntitySnapshot};
pub use math::{EulerAngles, KinematicState, Quaternion, Vec3, rk4};
pub use parse::{Mission, Params};
pub use scenario::{
    EndConditions, EntityGroupConfig, PluginConfig, RunConfig, ScenarioConfig, SpatialLookup, Spawn,
};
pub use simcontrol::{Event, EventKind, Simulation, SimulationFrame, TerminationReason, TraceSink};

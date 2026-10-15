//! Ripple's simulation core: missions, entities, plugins, and the tick.
mod common;
mod entity;
mod math;
mod mission;
pub mod plugin;
mod ports;
mod pubsub;
pub mod record;
mod scenario;
mod simulation;

pub use common::{AngleGains, GeographicOrigin, LengthGains, Pid, PidGains, SpeedGains, Terrain};
pub use entity::{Entity, EntitySnapshot};
pub use math::{EulerAngles, KinematicState, Quaternion, Vec3, rk4};
pub use mission::{Mission, Params};
pub use scenario::{
    EndConditions, EntityGroupConfig, PluginConfig, RunConfig, ScenarioConfig, SpatialLookup, Spawn,
};
pub use simulation::{Event, EventKind, Simulation, SimulationFrame, TerminationReason, TraceSink};

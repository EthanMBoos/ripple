//! Ripple's simulation core: missions, entities, plugins, and the tick.
mod common;
mod drawing;
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
pub use drawing::{Drawing, PluginDrawings};
pub use entity::{Entity, EntitySnapshot};
pub use math::{EulerAngles, Integrable, KinematicState, Quaternion, Vec3, rk4};
pub use mission::{Mission, Params};
pub use scenario::{
    EndConditions, EntityGroupConfig, PluginConfig, RunConfig, ScenarioConfig, SpatialLookup, Spawn,
};
pub use simulation::{
    Event, EventKind, PluginOverlay, PluginSlot, Simulation, SimulationFrame, SpawnRequest,
    TerminationReason, TraceSink,
};

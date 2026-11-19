//! Ripple's simulation core: missions, entities, plugins, and the tick.
// A trusted contract for a map method names the map's allocator parameter,
// which a nightly feature gates; `cargo verus` turns `verus_only` on, and a
// plain build never sees the attribute (it is behind `cfg_attr`) or this line.
#![cfg_attr(verus_only, feature(allocator_api))]
// The `verus!` macro prints a struct pattern's shorthand fields as `name:
// name`; the source stays shorthand.
#![allow(non_shorthand_field_patterns)]
mod common;
mod drawing;
mod entity;
mod math;
mod mission;
pub mod plugin;
mod ports;
mod proof;
mod pubsub;
pub mod record;
mod scenario;
mod simulation;

pub use common::{AngleGains, GeographicOrigin, LengthGains, Pid, PidGains, SpeedGains, Terrain};
pub use drawing::{Drawing, PluginDrawings};
pub use entity::{Entity, EntitySnapshot};
pub use math::{
    EulerAngles, GaussianState, Integrable, KinematicState, Matrix3, PositionMeasurement,
    Quaternion, StateCovariance, Vec3, rk4,
};
pub use mission::{Mission, Params};
pub use scenario::{
    EndConditions, EntityGroupConfig, PluginConfig, RunConfig, ScenarioConfig, SpatialLookup, Spawn,
};
pub use simulation::{
    Event, EventKind, PluginOverlay, PluginSlot, Simulation, SimulationFrame, SpawnRequest,
    TerminationReason, TraceSink,
};

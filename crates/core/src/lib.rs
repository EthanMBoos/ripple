//! Ripple's simulation core: missions, entities, plugins, and the tick.
// Verus reads a loop's invariant from an attribute on the loop, which only a
// nightly feature lets rustc parse, and a trusted contract for a map method
// names the map's allocator parameter, which another feature gates; `cargo
// verus` turns `verus_only` on, and a plain build never sees the attribute (it
// is behind `cfg_attr`) or these lines.
#![cfg_attr(verus_only, feature(proc_macro_hygiene, allocator_api))]
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
pub use math::{EulerAngles, Integrable, KinematicState, Quaternion, Vec3, rk4};
pub use mission::{Mission, Params};
pub use scenario::{
    EndConditions, EntityGroupConfig, PluginConfig, RunConfig, ScenarioConfig, SpatialLookup, Spawn,
};
pub use simulation::{
    Event, EventKind, PluginOverlay, PluginSlot, Simulation, SimulationFrame, SpawnRequest,
    TerminationReason, TraceSink,
};

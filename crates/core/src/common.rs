//! Shared control wiring, update rates, deterministic random streams, and spatial lookup.
mod loop_rate;
mod pid;
mod plugin_random;
pub(crate) mod random;
mod spatial;
pub(crate) mod variable_io;
mod waypoints;

pub(crate) use loop_rate::Rate;
pub use pid::{Pid, PidGains};
pub use plugin_random::PluginRandom;
pub(crate) use spatial::is_within;
pub use spatial::{Pairs, SpatialIndex, TruthContacts};
pub use variable_io::{Frame, PluginIo, Port, Ports, Unit};
pub use waypoints::{WAYPOINT_TOPIC, WaypointList};

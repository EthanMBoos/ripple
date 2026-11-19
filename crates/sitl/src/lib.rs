//! Shared by the autopilot adapters (crates/ardupilot, crates/px4): the
//! `SitlState` sensor that samples an aircraft's true state after motion in
//! the autopilots' frames (NED world, forward-right-down body), and the
//! Docker containers each adapter runs its SITL in, one per aircraft, stopped
//! when the run ends.
//!
//! The `ripple` command registers `SitlState` once when either adapter
//! feature is on (crates/cli/src/main.rs); a program that registers an
//! adapter itself calls [`register`] as well.
use anyhow::Result;
use ripple_core::plugin::PluginRegistry;

mod containers;
mod sitl_state;
pub use containers::{Container, Containers, home};
pub use sitl_state::{SITL_STATE_TOPIC, SitlSample, SitlState};

/// Adds the shared sensor to `registry`.
pub fn register(registry: &mut PluginRegistry) -> Result<()> {
    registry.register_sensor::<SitlState>("SitlState")?;
    Ok(())
}

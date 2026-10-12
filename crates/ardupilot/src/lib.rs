//! ArduPilot SITL adapter: fly FixedWing6DOF aircraft with ArduPlane SITL over
//! ArduPilot's JSON backend, in lockstep, one SITL per aircraft.
//!
//! `SitlState` (a sensor) samples each aircraft's state after motion and
//! publishes it on LocalNetwork. `ArduPilotBridge` (an autonomy) sends that
//! sample to its SITL as the reply to the servo frame it applied last tick,
//! waits for the next frame, and writes the four FixedWing6DOF inputs.
//! User guide: book/src/guides/ardupilot.md.
//!
//! `SitlLauncher` starts and stops one ArduPlane SITL container per aircraft
//! around a `ripple run`.
//!
//! The `ripple` command registers these plugins only when built with
//! `--features ardupilot`.
use anyhow::Result;
use ripple_core::plugin::PluginRegistry;

mod ardupilot_bridge;
mod json_sitl;
mod sitl_launcher;
mod sitl_state;
pub use ardupilot_bridge::ArduPilotBridge;
pub use sitl_launcher::SitlLauncher;
pub use sitl_state::{SITL_STATE_TOPIC, SitlSample, SitlState};

/// Adds this crate's plugins to `registry`.
pub fn register(registry: &mut PluginRegistry) -> Result<()> {
    registry.register_sensor::<SitlState>("SitlState")?;
    registry.register_autonomy::<ArduPilotBridge>("ArduPilotBridge")?;
    Ok(())
}

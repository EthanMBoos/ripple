//! PX4 SITL adapter: fly FixedWing6DOF aircraft with PX4 over its simulator
//! MAVLink API, in lockstep, one SITL per aircraft.
//!
//! The shared `SitlState` sensor (crates/sitl) samples each aircraft's state
//! after motion and publishes it on LocalNetwork. `Px4Bridge` (an autonomy)
//! turns that sample into the sensor readings PX4 expects (IMU, magnetometer,
//! barometer, pitot, GPS), sends them, waits for PX4's actuator controls, and
//! writes the FixedWing6DOF inputs. User guide: book/src/guides/px4.md.
//!
//! `Px4Launcher` starts and stops one PX4 container per aircraft around a
//! `ripple run`.
//!
//! The `ripple` command registers these plugins, and `sitl::register`'s
//! sensor, only when built with `--features px4`.
use anyhow::Result;
use ripple_core::plugin::PluginRegistry;

mod hil;
mod px4_bridge;
mod px4_launcher;
pub use px4_bridge::Px4Bridge;
pub use px4_launcher::Px4Launcher;

/// Adds this crate's plugins to `registry`; the `SitlState` sensor they need
/// is registered separately by `sitl::register`.
pub fn register(registry: &mut PluginRegistry) -> Result<()> {
    registry.register_autonomy::<Px4Bridge>("Px4Bridge")?;
    Ok(())
}

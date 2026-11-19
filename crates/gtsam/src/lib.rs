//! GTSAM in a Ripple mission: a fixed-lag smoother over position as a
//! localization plugin, written against the C++ library through a small
//! bridge (src/ffi.rs and src/smoother.cc). Optional: see
//! book/src/guides/gtsam.md for the GTSAM install and the `gtsam` feature.
use anyhow::Result;
use ripple_core::plugin::PluginRegistry;

mod ffi;
mod gtsam_smoother;

pub use gtsam_smoother::GtsamSmoother;

pub fn register(registry: &mut PluginRegistry) -> Result<()> {
    registry.register_localization::<GtsamSmoother>("GtsamSmoother")?;
    Ok(())
}

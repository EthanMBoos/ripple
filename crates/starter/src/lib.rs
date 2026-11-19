//! User plugins, available to the stock `ripple` command.
//!
//! To add a plugin: write it in its own file (see `follow_nearest.rs` or
//! `position_kalman.rs`), add a `mod` line below, and register it in `register`
//! under the name missions use.
//! The `ripple` command calls `register` at startup (crates/cli/src/main.rs).
use anyhow::Result;
use ripple_core::plugin::PluginRegistry;

mod follow_nearest;
mod position_kalman;
pub use follow_nearest::FollowNearest;
pub use position_kalman::PositionKalman;

/// Adds this crate's plugins to `registry`.
pub fn register(registry: &mut PluginRegistry) -> Result<()> {
    registry.register_autonomy::<FollowNearest>("FollowNearest")?;
    registry.register_localization::<PositionKalman>("PositionKalman")?;
    Ok(())
}

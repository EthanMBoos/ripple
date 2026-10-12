//! User plugins, available to the stock `ripple` command.
//!
//! To add a plugin: write it in its own file (see `follow_nearest.rs` or
//! `dead_reckoning.rs`), add a `mod` line below, and register it in `register`
//! under the name missions use.
//! The `ripple` command calls `register` at startup (crates/cli/src/main.rs).
use anyhow::Result;
use ripple_core::plugin::PluginRegistry;

mod dead_reckoning;
mod follow_nearest;
pub use dead_reckoning::DeadReckoning;
pub use follow_nearest::FollowNearest;

/// Adds this crate's plugins to `registry`.
pub fn register(registry: &mut PluginRegistry) -> Result<()> {
    registry.register_autonomy::<FollowNearest>("FollowNearest")?;
    registry.register_localization::<DeadReckoning>("DeadReckoning")?;
    Ok(())
}

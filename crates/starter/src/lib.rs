//! User plugins, available to the stock `scrimmage` command.
//!
//! To add a plugin: write it in its own file (see `follow_nearest.rs`), add a
//! `mod` line below, and register it in `register` under the name missions use.
//! The `scrimmage` command calls `register` at startup (crates/cli/src/main.rs).
use anyhow::Result;
use scrimmage_core::plugin::PluginRegistry;

mod follow_nearest;
pub use follow_nearest::FollowNearest;

/// Adds this crate's plugins to `registry`.
pub fn register(registry: &mut PluginRegistry) -> Result<()> {
    registry.register_autonomy::<FollowNearest>("FollowNearest")?;
    Ok(())
}

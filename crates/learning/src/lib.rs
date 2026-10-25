//! Reinforcement-learning demos: autonomies that act from a trained network.
//!
//! Each task is one autonomy plugin with an `observe` function. Python trains
//! a policy on that function (`python/`, Gymnasium and PettingZoo), saves the
//! network as ONNX, and the same plugin runs it in a mission. The `ripple`
//! command includes this crate with `--features learning`
//! (crates/cli/src/main.rs).
use anyhow::Result;
use ripple_core::plugin::PluginRegistry;

pub mod hover;
pub mod policy;
pub mod spread;
pub use hover::{HoverConfig, HoverPolicy};
pub use policy::TrainerActions;
pub use spread::{SpreadConfig, SpreadPolicy};

/// Adds this crate's plugins to `registry`, acting from their policy files.
pub fn register(registry: &mut PluginRegistry) -> Result<()> {
    registry.register_autonomy::<HoverPolicy>("HoverPolicy")?;
    registry.register_autonomy::<SpreadPolicy>("SpreadPolicy")?;
    Ok(())
}

/// Adds this crate's plugins to `registry`, acting on what a trainer sets in
/// `actions` instead of their policy files.
pub fn register_for_training(
    registry: &mut PluginRegistry,
    actions: &TrainerActions,
) -> Result<()> {
    let hover_actions = actions.clone();
    registry.register_autonomy_with("HoverPolicy", move |config| {
        HoverPolicy::trained_by(config, hover_actions.clone())
    })?;
    let spread_actions = actions.clone();
    registry.register_autonomy_with("SpreadPolicy", move |config| {
        SpreadPolicy::trained_by(config, spread_actions.clone())
    })?;
    Ok(())
}

//! Medical delivery by a mixed fleet: fixed-wing aircraft drop packages by
//! parachute, multirotors land them on rooftops, and a dispatcher at the depot
//! asks a language model who goes where.
//!
//! - `Dispatcher` (autonomy, on the depot): opens each episode's requests and
//!   assigns idle vehicles, refusing calls a vehicle cannot fly.
//! - `Courier` (autonomy, on each vehicle): runs the skill it is assigned
//!   from the library in `skill.rs` (`drop`, `land`) and reports its status.
//! - `DeliveryScore` (metrics): on-time deliveries, refused calls, and
//!   whether the fleet followed a mid-mission instruction (`directive.rs`).
//!
//! The `ripple` command calls `register` at startup (crates/cli/src/main.rs).
use anyhow::Result;
use ripple_core::plugin::PluginRegistry;

mod courier;
mod directive;
mod dispatcher;
mod messages;
mod planner;
mod score;
mod skill;
pub use courier::Courier;
pub use dispatcher::Dispatcher;
pub use score::DeliveryScore;

/// Adds this crate's plugins to `registry`.
pub fn register(registry: &mut PluginRegistry) -> Result<()> {
    registry.register_autonomy::<Dispatcher>("Dispatcher")?;
    registry.register_autonomy::<Courier>("Courier")?;
    registry.register_metrics::<DeliveryScore>("DeliveryScore")?;
    Ok(())
}

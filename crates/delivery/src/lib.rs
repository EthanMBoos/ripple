//! One day of blood delivery across a region of hubs. A dispatcher
//! assigns requests to couriers at each hub; a fleet manager above it, a
//! language model or fixed rules, reads the day's events and gives orders
//! that change how the dispatcher and the couriers behave.
//!
//! - `Dispatcher` (autonomy, on the region entity): the hubs and their stock,
//!   the day's requests and events, the fleet manager and its orders, and the
//!   assignment of requests to idle vehicles.
//! - `Courier` (autonomy, on each vehicle): runs the skill it is assigned
//!   from the library in `skill.rs` (`drop`, `land`), relocates when told to,
//!   and reports its status.
//! - `DeliveryScore` (metrics): what was flown: on-time deliveries by urgency,
//!   missed requests by reason, breaches of each event's rule, lost vehicles,
//!   orders and calls.
//!
//! The model is asked through `client.rs`: the `claude` or `codex` command,
//! the Anthropic or OpenAI API, or a local Ollama server.
//!
//! The `ripple` command calls `register` at startup (crates/cli/src/main.rs).
//! The study's claim and results are in docs/TODO.md.
use anyhow::Result;
use ripple_core::plugin::PluginRegistry;

mod area;
mod client;
mod courier;
mod dispatcher;
mod event;
mod help;
mod messages;
mod model;
mod order;
mod policy;
mod region;
mod replanner;
mod rules;
mod scope;
mod score;
mod skill;
mod view;
pub use courier::Courier;
pub use dispatcher::Dispatcher;
pub use region::{Region, RegionConfig, hub_position_m};
pub use score::DeliveryScore;

/// Adds this crate's plugins to `registry`.
pub fn register(registry: &mut PluginRegistry) -> Result<()> {
    registry.register_autonomy::<Dispatcher>("Dispatcher")?;
    registry.register_autonomy::<Courier>("Courier")?;
    registry.register_metrics::<DeliveryScore>("DeliveryScore")?;
    Ok(())
}

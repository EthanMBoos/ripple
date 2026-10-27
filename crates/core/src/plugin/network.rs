//! Networks: the plugin interface and the built-in communication policies.
//! `NetworkContext` lives with the message engine in `pubsub`.
use super::NetworkContext;
use crate::plugin::{StepTime, Update, WorldContext};
use anyhow::Result;

pub trait Network: Send {
    fn initialize(&mut self, _context: &mut WorldContext<'_>) -> Result<()> {
        Ok(())
    }
    fn step(&mut self, context: &mut NetworkContext<'_, '_>) -> Result<Update>;
    fn close(&mut self, _time: StepTime) -> Result<()> {
        Ok(())
    }
}

// Built-ins selected by missions.
#[path = "network/global_network/global_network.rs"]
mod global_network;
#[path = "network/local_network/local_network.rs"]
mod local_network;
pub use global_network::GlobalNetwork;
pub use local_network::LocalNetwork;
#[path = "network/sphere_network/sphere_network.rs"]
mod sphere_network;
pub use sphere_network::SphereNetwork;

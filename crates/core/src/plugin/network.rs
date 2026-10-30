//! Networks: the plugin interface and the built-in communication policies.
//! `NetworkContext` lives with the message engine in `pubsub`.
use super::NetworkContext;
use crate::plugin::{PluginParams, StepTime, WorldContext};
use anyhow::Result;
use serde::de::DeserializeOwned;

pub trait Network: Send {
    // How it is built, answered from the concrete type while the mission
    // loads. `where Self: Sized` keeps these out of `Box<dyn Network>`, which
    // only steps; see the module doc in `plugin.rs`.
    /// Immutable, validated configuration, parsed once per mission and shared
    /// by every instance.
    type Config: DeserializeOwned + Send + Sync + 'static
    where
        Self: Sized;
    /// Parses the mission's parameters; override it to check them.
    fn configure(params: &PluginParams) -> Result<Self::Config>
    where
        Self: Sized,
    {
        params.parse()
    }
    fn new(config: &Self::Config) -> Self
    where
        Self: Sized;

    fn initialize(&mut self, _context: &mut WorldContext) -> Result<()> {
        Ok(())
    }
    fn step(&mut self, context: &mut NetworkContext) -> Result<()>;
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

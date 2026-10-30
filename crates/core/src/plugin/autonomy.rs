//! Autonomy: the plugin interface and the built-in algorithms.
use crate::{
    plugin::{AgentContext, PluginParams, StepTime},
    ports::{PluginIo, Ports},
};
use anyhow::Result;
use serde::de::DeserializeOwned;

/// What an autonomy does each tick, and how it is built.
/// `Send`, like every category trait, because entities step on worker threads.
pub trait Autonomy: Send {
    // How it is built, answered from the concrete type while the mission
    // loads. `where Self: Sized` keeps these out of `Box<dyn Autonomy>`, which
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
    fn ports(_config: &Self::Config) -> Ports
    where
        Self: Sized,
    {
        Ports::default()
    }

    fn initialize(&mut self, _context: &mut AgentContext) -> Result<()> {
        Ok(())
    }
    fn step(&mut self, context: &mut AgentContext, io: &mut PluginIo) -> Result<()>;
    /// Runs once: when the entity is removed, the run ends, or it fails. Not
    /// a destructor: it has the simulation time, it can fail, and the plugin
    /// stays in memory until the simulation is dropped.
    fn close(&mut self, _time: StepTime) -> Result<()> {
        Ok(())
    }
}

// Built-ins selected by missions.
#[path = "autonomy/straight/straight.rs"]
mod straight;
pub use straight::Straight;
#[path = "autonomy/waypoint_follower/waypoint_follower.rs"]
mod waypoint_follower;
pub use waypoint_follower::WaypointFollower;
#[path = "autonomy/auction_assign/auction_assign.rs"]
mod auction_assign;
pub use auction_assign::{
    AuctionAssign, AuctionBid, AuctionResult, AuctionStart, BID_AUCTION_TOPIC,
    RESULT_AUCTION_TOPIC, START_AUCTION_TOPIC,
};
#[path = "autonomy/constant_velocity/constant_velocity.rs"]
mod constant_velocity;
pub use constant_velocity::ConstantVelocity;

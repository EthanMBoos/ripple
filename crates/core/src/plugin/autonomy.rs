//! Autonomy: the plugin interface and the built-in algorithms.
use crate::{
    plugin::{AgentContext, StepTime, Update},
    ports::PluginIo,
};
use anyhow::Result;

/// What an autonomy does each tick. How it is built is its `Plugin` impl.
/// `Send`, like every category trait, because entities step on worker threads.
pub trait Autonomy: Send {
    fn initialize(&mut self, _context: &mut AgentContext<'_>) -> Result<()> {
        Ok(())
    }
    fn step(&mut self, context: &mut AgentContext<'_>, io: &mut PluginIo) -> Result<Update>;
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

//! Autonomy implementations selected by mission XML.
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

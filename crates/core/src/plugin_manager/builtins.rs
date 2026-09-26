//! Registration names for the bundled concrete plugins.
use super::PluginRegistry;
use crate::plugin::{autonomy, controller, interaction, metrics, motion, network, sensor};
use anyhow::Result;

pub(crate) fn register_builtins(registry: &mut PluginRegistry) -> Result<()> {
    registry.register_autonomy::<autonomy::Straight>("Straight")?;
    registry.register_autonomy::<autonomy::WaypointFollower>("WaypointFollower")?;
    registry.register_autonomy::<autonomy::AuctionAssign>("AuctionAssign")?;
    registry.register_autonomy::<autonomy::ConstantVelocity>("ConstantVelocity")?;
    registry.register_controller::<controller::SingleIntegratorControllerSimple>(
        "SingleIntegratorControllerSimple",
    )?;
    registry.register_motion::<motion::SingleIntegrator>("SingleIntegrator")?;
    registry.register_interaction::<interaction::WaypointBroadcast>("WaypointBroadcast")?;
    registry.register_controller::<controller::SimpleAircraftControllerPid>(
        "SimpleAircraftControllerPID",
    )?;
    registry.register_motion::<motion::SimpleAircraft>("SimpleAircraft")?;
    registry.register_motion::<motion::FixedWing6Dof>("FixedWing6DOF")?;
    registry.register_controller::<controller::AircraftPidController>("AircraftPIDController")?;
    registry.register_motion::<motion::Multirotor>("Multirotor")?;
    registry.register_controller::<controller::MotorSpeeds>("MotorSpeeds")?;
    registry.register_sensor::<sensor::NoisyPosition>("NoisyPosition")?;
    registry.register_sensor::<sensor::NoisyState>("NoisyState")?;
    registry.register_sensor::<sensor::NoisyContacts>("NoisyContacts")?;
    registry.register_interaction::<interaction::SimpleCollision>("SimpleCollision")?;
    registry.register_interaction::<interaction::Boundary>("Boundary")?;
    registry.register_interaction::<interaction::GroundCollision>("GroundCollision")?;
    registry.register_network::<network::LocalNetwork>("LocalNetwork")?;
    registry.register_network::<network::GlobalNetwork>("GlobalNetwork")?;
    registry.register_network::<network::SphereNetwork>("SphereNetwork")?;
    registry.register_metrics::<metrics::SimpleCollisionMetrics>("SimpleCollisionMetrics")?;
    Ok(())
}

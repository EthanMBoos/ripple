//! Controllers: the plugin interface and the built-in control laws.
use crate::{
    plugin::{AgentContext, Plugin, Update},
    ports::PluginIo,
};
use anyhow::Result;

pub trait Controller: Plugin {
    fn initialize(&mut self, _context: &mut AgentContext<'_>) -> Result<()> {
        Ok(())
    }
    fn step(&mut self, context: &mut AgentContext<'_>, io: &mut PluginIo) -> Result<Update>;
}

// Built-ins selected by missions.
#[path = "controller/aircraft_pid/aircraft_pid.rs"]
mod aircraft_pid;
pub use aircraft_pid::AircraftPidController;
#[path = "controller/aircraft_velocity/aircraft_velocity.rs"]
mod aircraft_velocity;
pub use aircraft_velocity::AircraftVelocityController;
#[path = "controller/motor_speeds/motor_speeds.rs"]
mod motor_speeds;
pub use motor_speeds::MotorSpeeds;
#[path = "controller/multirotor_velocity/multirotor_velocity.rs"]
mod multirotor_velocity;
pub use multirotor_velocity::MultirotorVelocityController;
#[path = "controller/simple_aircraft_pid/simple_aircraft_pid.rs"]
mod simple_aircraft_pid;
pub use simple_aircraft_pid::SimpleAircraftControllerPid;
#[path = "controller/single_integrator_simple/single_integrator_simple.rs"]
mod single_integrator_simple;
pub use single_integrator_simple::SingleIntegratorControllerSimple;

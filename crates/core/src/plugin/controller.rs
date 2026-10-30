//! Controllers: the plugin interface and the built-in control laws.
use crate::{
    plugin::{AgentContext, PluginParams, StepTime},
    ports::{PluginIo, Ports},
};
use anyhow::Result;
use serde::de::DeserializeOwned;

pub trait Controller: Send {
    // How it is built, answered from the concrete type while the mission
    // loads. `where Self: Sized` keeps these out of `Box<dyn Controller>`, which
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
    fn close(&mut self, _time: StepTime) -> Result<()> {
        Ok(())
    }
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

//! Rust-only open-loop test driver, not a flight controller: MotorSpeeds's
//! fixed-wing twin.
//! Controller phase: write a constant throttle and constant elevator, aileron,
//! and rudder angles, for flights whose inputs must be known exactly, such as
//! reference/jsbsim_check.py's glides against JSBSim.

use anyhow::Result;
use serde::Deserialize;

use crate::plugin::{AgentContext, Controller, Frame, PluginIo, Port, Ports, Unit};

/// Mission parameters; `Default` supplies any key the mission leaves out, so
/// the default is a hands-off glide: no thrust, every surface neutral.
#[derive(Clone, Copy, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HeldControlsConfig {
    /// Throttle, 0 to 1, which the motion model maps to thrust.
    throttle: f64,
    /// Surface angles in radians, with the signs the motion model's
    /// coefficients take.
    elevator_rad: f64,
    aileron_rad: f64,
    rudder_rad: f64,
}

pub struct HeldControls {
    config: HeldControlsConfig,
}

impl Controller for HeldControls {
    type Config = HeldControlsConfig;

    fn new(config: &Self::Config) -> Self {
        Self { config: *config }
    }

    fn ports(_: &Self::Config) -> Ports {
        Ports::default()
            .output(Port::new("throttle", Unit::Dimensionless, Frame::None))
            .output(Port::new("elevator", Unit::Radians, Frame::Model))
            .output(Port::new("aileron", Unit::Radians, Frame::Model))
            .output(Port::new("rudder", Unit::Radians, Frame::Model))
    }

    fn step(&mut self, _: &mut AgentContext, io: &mut PluginIo) -> Result<()> {
        io.write("throttle", self.config.throttle)?;
        io.write("elevator", self.config.elevator_rad)?;
        io.write("aileron", self.config.aileron_rad)?;
        io.write("rudder", self.config.rudder_rad)?;
        Ok(())
    }
}

//! Rust-only open-loop motor test driver, not a flight controller.
//! Controller phase: write configured constant shaft speeds to motor_0, motor_1, ...
//! The reference fixture supplies the same commands to the upstream C++ motion model.

use anyhow::{Result, ensure};
use serde::Deserialize;

use crate::plugin::{
    AgentContext, Controller, Frame, Plugin, PluginIo, PluginParams, Port, Ports, Unit, Update,
};

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct MotorSpeedsParams {
    /// Shaft speeds in rad/s written to motor_0, motor_1, ...; a mission must
    /// list one per rotor.
    #[serde(rename = "speeds")]
    speeds_radps: Vec<f64>,
}

impl Default for MotorSpeedsParams {
    fn default() -> Self {
        Self {
            speeds_radps: vec![0.0; 4],
        }
    }
}

#[derive(Clone)]
pub struct MotorSpeedsConfig {
    commands: Vec<MotorCommand>,
}

#[derive(Clone)]
struct MotorCommand {
    port: String,
    speed_radps: f64,
}

pub struct MotorSpeeds {
    config: MotorSpeedsConfig,
}

impl Plugin for MotorSpeeds {
    type Config = MotorSpeedsConfig;

    fn configure(params: &PluginParams<'_>) -> Result<Self::Config> {
        let params: MotorSpeedsParams = params.parse()?;
        let mut commands = Vec::new();
        for (index, speed_radps) in params.speeds_radps.into_iter().enumerate() {
            commands.push(MotorCommand {
                port: format!("motor_{index}"),
                speed_radps,
            });
        }
        ensure!(
            !commands.is_empty(),
            "MotorSpeeds requires at least one speed"
        );
        Ok(MotorSpeedsConfig { commands })
    }

    fn new(config: &Self::Config) -> Self {
        Self {
            config: config.clone(),
        }
    }

    fn ports(config: &Self::Config) -> Ports {
        let mut ports = Ports::default();
        for command in &config.commands {
            ports = ports.output(Port::new(
                &command.port,
                Unit::RadiansPerSecond,
                Frame::None,
            ));
        }
        ports
    }
}

impl Controller for MotorSpeeds {
    fn step(&mut self, _: &mut AgentContext<'_>, io: &mut PluginIo) -> Result<Update> {
        for command in &self.config.commands {
            io.write(&command.port, command.speed_radps)?;
        }
        Ok(Update::Applied)
    }
}

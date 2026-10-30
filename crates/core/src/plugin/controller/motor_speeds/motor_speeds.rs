//! Rust-only open-loop motor test driver, not a flight controller.
//! Controller phase: write configured constant shaft speeds to motor_0, motor_1, ...
//! The reference fixture supplies the same commands to the upstream C++ motion model.

use anyhow::{Result, ensure};
use serde::Deserialize;

use crate::plugin::{AgentContext, Controller, Frame, PluginIo, Port, Ports, Unit};

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct MotorSpeedsParams {
    /// Shaft speeds in rad/s written to motor_0, motor_1, ...; a mission must
    /// list one per rotor.
    speeds_radps: Vec<f64>,
}

impl Default for MotorSpeedsParams {
    fn default() -> Self {
        Self {
            speeds_radps: vec![0.0; 4],
        }
    }
}

#[derive(Clone, Deserialize)]
#[serde(try_from = "MotorSpeedsParams")]
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

// The mission's parameters become the config through serde's `try_from`, so
// the trait's default `configure` (`params.parse()`) fits this plugin too.
impl TryFrom<MotorSpeedsParams> for MotorSpeedsConfig {
    type Error = anyhow::Error;
    fn try_from(params: MotorSpeedsParams) -> Result<Self> {
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
}

impl Controller for MotorSpeeds {
    type Config = MotorSpeedsConfig;

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

    fn step(&mut self, _: &mut AgentContext, io: &mut PluginIo) -> Result<()> {
        for command in &self.config.commands {
            io.write(&command.port, command.speed_radps)?;
        }
        Ok(())
    }
}

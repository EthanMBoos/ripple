//! A minimal controller demonstrating composable named input/output channels.
//! Each port name has one writer on an entity, so a scaled speed leaves under
//! its own name; a chain of these names each link.
use anyhow::Result;
use ripple_core::plugin::*;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ScaleSpeedConfig {
    gain: f64,
    input: String,
    output: String,
}
impl Default for ScaleSpeedConfig {
    fn default() -> Self {
        Self {
            gain: 1.0,
            input: "speed".into(),
            output: "scaled_speed".into(),
        }
    }
}

pub struct ScaleSpeed {
    gain: f64,
    input: String,
    output: String,
}
impl Controller for ScaleSpeed {
    type Config = ScaleSpeedConfig;
    fn new(config: &ScaleSpeedConfig) -> Self {
        Self {
            gain: config.gain,
            input: config.input.clone(),
            output: config.output.clone(),
        }
    }
    fn ports(config: &ScaleSpeedConfig) -> Ports {
        Ports::default()
            .input(Port::new(
                &config.input,
                Unit::MetersPerSecond,
                Frame::World,
            ))
            .output(Port::new(
                &config.output,
                Unit::MetersPerSecond,
                Frame::World,
            ))
    }

    fn step(&mut self, _: &mut AgentContext, io: &mut PluginIo) -> Result<()> {
        let speed_world_mps = io.read(&self.input)?;
        io.write(&self.output, self.gain * speed_world_mps)?;
        Ok(())
    }
}

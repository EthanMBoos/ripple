//! A minimal controller demonstrating composable named input/output channels.
use anyhow::Result;
use ripple_core::plugin::*;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ScaleSpeedConfig {
    gain: f64,
}
impl Default for ScaleSpeedConfig {
    fn default() -> Self {
        Self { gain: 1.0 }
    }
}

pub struct ScaleSpeed {
    gain: f64,
}
impl Controller for ScaleSpeed {
    type Config = ScaleSpeedConfig;
    fn new(config: &ScaleSpeedConfig) -> Self {
        Self { gain: config.gain }
    }
    fn ports(_: &ScaleSpeedConfig) -> Ports {
        let speed = Port::new("speed", Unit::MetersPerSecond, Frame::World);
        Ports::default().input(speed.clone()).output(speed)
    }

    fn step(&mut self, _: &mut AgentContext, io: &mut PluginIo) -> Result<()> {
        let speed_world_mps = io.read("speed")?;
        io.write("speed", self.gain * speed_world_mps)?;
        Ok(())
    }
}

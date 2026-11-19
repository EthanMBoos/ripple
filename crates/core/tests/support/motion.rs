//! One-dimensional constant-velocity motion along the world's east axis.
use anyhow::Result;
use ripple_core::{Vec3, plugin::*};
use serde::Deserialize;

/// The port the speed is read from: `speed` straight from an autonomy, or
/// whatever name the last controller writes.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EastwardMotionConfig {
    input: String,
}
impl Default for EastwardMotionConfig {
    fn default() -> Self {
        Self {
            input: "speed".into(),
        }
    }
}

pub struct EastwardMotion {
    input: String,
}
impl MotionModel for EastwardMotion {
    type Config = EastwardMotionConfig;
    fn new(config: &EastwardMotionConfig) -> Self {
        Self {
            input: config.input.clone(),
        }
    }
    fn ports(config: &EastwardMotionConfig) -> Ports {
        Ports::default().input(Port::new(
            &config.input,
            Unit::MetersPerSecond,
            Frame::World,
        ))
    }

    fn step(&mut self, context: &mut MotionContext, io: &mut PluginIo) -> Result<()> {
        let speed_world_mps = io.read(&self.input)?;
        context.truth.position_world_m.x += speed_world_mps * context.time.dt_s;
        context.truth.velocity_world_mps = Vec3::new(speed_world_mps, 0.0, 0.0);
        Ok(())
    }
}

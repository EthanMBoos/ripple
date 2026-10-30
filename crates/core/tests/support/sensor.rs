//! Measure east position. A bias lets the mission distinguish belief from truth.
use anyhow::Result;
use ripple_core::plugin::*;
use serde::Deserialize;

pub struct PositionReading {
    pub x_world_m: f64,
}
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PositionSensorConfig {
    bias_m: f64,
}
pub struct PositionSensor {
    bias_m: f64,
}
impl Sensor for PositionSensor {
    type Config = PositionSensorConfig;
    fn new(config: &PositionSensorConfig) -> Self {
        Self {
            bias_m: config.bias_m,
        }
    }

    fn step(&mut self, context: &mut SensorContext) -> Result<()> {
        let measured_x_world_m = context.truth.position_world_m.x + self.bias_m;
        context.publish_reading(PositionReading {
            x_world_m: measured_x_world_m,
        })?;
        Ok(())
    }
}

//! Measure east position. A bias lets the mission distinguish belief from truth.
use anyhow::Result;
use scrimmage_core::plugin::*;
use serde::{Deserialize, Serialize};

pub struct PositionReading {
    pub x_world_m: f64,
}
#[derive(Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct PositionSensorConfig {
    bias_m: f64,
}
pub struct PositionSensor {
    bias_m: f64,
}
impl Plugin for PositionSensor {
    type Config = PositionSensorConfig;
    fn configure(params: &PluginParams<'_>) -> Result<PositionSensorConfig> {
        params.parse()
    }
    fn new(config: &PositionSensorConfig) -> Self {
        Self {
            bias_m: config.bias_m,
        }
    }
}
impl Sensor for PositionSensor {
    fn step(&mut self, context: &mut SensorContext<'_>) -> Result<Update> {
        let measured_x_world_m = context.truth.position_world_m.x + self.bias_m;
        context.publish_local(
            "position",
            PositionReading {
                x_world_m: measured_x_world_m,
            },
        )?;
        Ok(Update::Applied)
    }
}

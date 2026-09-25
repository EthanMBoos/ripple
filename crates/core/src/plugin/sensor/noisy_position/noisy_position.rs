//! Small authoring example: world position with configurable bias and Gaussian noise.
//! This is a new example sensor, not a parity claim for C++ NoisyState.
//! Sensor phase: sample post-motion truth, apply bias/noise, queue a local observation.

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use crate::{
    Vec3,
    plugin::{Plugin, PluginParams, Sensor, SensorContext, Update},
};

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct SensorConfig {
    bias_world_m: [f64; 3],
    stddev_m: f64,
    topic: String,
}

impl Default for SensorConfig {
    fn default() -> Self {
        Self {
            bias_world_m: [0.0; 3],
            stddev_m: 0.1,
            topic: "position".into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PositionObservation {
    pub position_world_m: Vec3,
}

pub struct NoisyPosition {
    bias_world_m: Vec3,
    stddev_m: f64,
    topic: String,
}

impl Plugin for NoisyPosition {
    type Config = SensorConfig;

    fn configure(params: &PluginParams<'_>) -> Result<SensorConfig> {
        let config: SensorConfig = params.parse()?;
        ensure!(config.stddev_m >= 0.0, "stddev_m must be nonnegative");
        Ok(config)
    }

    fn new(config: &SensorConfig) -> Self {
        Self {
            bias_world_m: Vec3::from(config.bias_world_m),
            stddev_m: config.stddev_m,
            topic: config.topic.clone(),
        }
    }
}

impl Sensor for NoisyPosition {
    fn step(&mut self, context: &mut SensorContext<'_>) -> Result<Update> {
        let ideal_position_world_m = context.truth.position_world_m;

        // Preserve x/y/z draw order, even when the configured noise is zero.
        let noise_world_m = Vec3::new(
            context.random.normal(0.0, self.stddev_m)?,
            context.random.normal(0.0, self.stddev_m)?,
            context.random.normal(0.0, self.stddev_m)?,
        );
        let measured_position_world_m = ideal_position_world_m + self.bias_world_m + noise_world_m;

        let observation = PositionObservation {
            position_world_m: measured_position_world_m,
        };
        context.publish_local(&self.topic, observation)?;
        Ok(Update::Applied)
    }
}

#[cfg(test)]
mod tests {
    use super::{NoisyPosition, Plugin, PositionObservation, Sensor, SensorConfig, SensorContext};
    use crate::plugin::{EntityInfo, Messages, Observations, PluginRandom, StepTime, Update};
    use crate::{KinematicState, Vec3};

    #[test]
    fn zero_noise_applies_bias_without_modifying_truth() -> anyhow::Result<()> {
        check_observation(0.0)
    }

    #[test]
    fn seeded_noise_preserves_axis_order_and_stream_position() -> anyhow::Result<()> {
        check_observation(0.2)
    }

    fn check_observation(stddev_m: f64) -> anyhow::Result<()> {
        let config = SensorConfig {
            bias_world_m: [1.0, -2.0, 3.0],
            stddev_m,
            topic: "position".into(),
        };
        let mut sensor = NoisyPosition::new(&config);
        let truth = KinematicState {
            position_world_m: Vec3::new(10.0, 20.0, 30.0),
            ..KinematicState::default()
        };
        let mut random = PluginRandom::new(12345, 1, "NoisyPosition:0");
        let mut expected_random = PluginRandom::new(12345, 1, "NoisyPosition:0");
        let expected_noise_world_m = Vec3::new(
            expected_random.normal(0.0, stddev_m)?,
            expected_random.normal(0.0, stddev_m)?,
            expected_random.normal(0.0, stddev_m)?,
        );
        let expected_position_world_m =
            truth.position_world_m + Vec3::from(config.bias_world_m) + expected_noise_world_m;
        let mut messages = Messages::default();
        let mut pending = Observations::default();
        let mut belief = None;
        let update = sensor.step(&mut SensorContext {
            messages: &mut messages,
            entity: EntityInfo {
                id: 1,
                team_id: 1,
                sub_swarm_id: 0,
            },
            time: StepTime {
                time_s: 2.0,
                dt_s: 0.1,
            },
            truth: &truth,
            contacts_truth: &[],
            random: &mut random,
            belief: &mut belief,
            pending: &mut pending,
        })?;
        assert_eq!(update, Update::Applied);
        let observation = pending
            .get::<PositionObservation>("position")?
            .expect("sensor output");
        // Exact equality protects the per-axis draw order, including zero-noise draws.
        assert_eq!(
            observation.value.position_world_m,
            expected_position_world_m
        );
        assert_eq!(truth.position_world_m, Vec3::new(10.0, 20.0, 30.0));
        assert!((observation.sampled_at_s - 2.1).abs() < 1e-12);
        assert_eq!(random.normal(0.0, 1.0)?, expected_random.normal(0.0, 1.0)?);
        Ok(())
    }
}

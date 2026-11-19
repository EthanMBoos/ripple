//! Small authoring example: world position with configurable bias and Gaussian noise.
//! A new example sensor, separate from NoisyState.
//! Sensor phase: sample post-motion truth, apply bias/noise, hand the
//! estimators a `PositionMeasurement` with the covariance of the noise added.

use anyhow::{Result, ensure};
use serde::Deserialize;

use crate::{
    PositionMeasurement, Vec3,
    plugin::{PluginParams, Sensor, SensorContext},
};

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NoisyPositionConfig {
    bias_m: [f64; 3],
    stddev_m: f64,
}

impl Default for NoisyPositionConfig {
    fn default() -> Self {
        Self {
            bias_m: [0.0; 3],
            stddev_m: 0.1,
        }
    }
}

pub struct NoisyPosition {
    bias_m: Vec3,
    stddev_m: f64,
}

impl Sensor for NoisyPosition {
    type Config = NoisyPositionConfig;
    type Reading = PositionMeasurement;

    fn configure(params: &PluginParams) -> Result<NoisyPositionConfig> {
        let config: NoisyPositionConfig = params.parse()?;
        ensure!(config.stddev_m >= 0.0, "stddev_m must be nonnegative");
        Ok(config)
    }

    fn new(config: &NoisyPositionConfig) -> Self {
        Self {
            bias_m: Vec3::from(config.bias_m),
            stddev_m: config.stddev_m,
        }
    }

    fn step(&mut self, context: &mut SensorContext) -> Result<()> {
        let ideal_position_world_m = context.truth.position_world_m;

        // Each axis's error is the number keyed by that axis (0, 1, 2) at this
        // tick: a different key for every number.
        let random = context.random;
        let noise_world_m = Vec3::new(
            random.normal(&[0], 0.0, self.stddev_m)?,
            random.normal(&[1], 0.0, self.stddev_m)?,
            random.normal(&[2], 0.0, self.stddev_m)?,
        );
        let measured_position_world_m = ideal_position_world_m + self.bias_m + noise_world_m;

        // The same noise on each axis, independent, so the covariance is a
        // scaled identity. The bias is a mean error and is not reported.
        let measurement = PositionMeasurement {
            position_world_m: measured_position_world_m,
            covariance_m2: nalgebra::Matrix3::identity() * (self.stddev_m * self.stddev_m),
        };
        context.publish_reading(measurement)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{NoisyPosition, NoisyPositionConfig, Sensor, SensorContext};
    use crate::plugin::sensor::ReadingType;
    use crate::plugin::{EntityInfo, Messages, PluginRandom, Readings, StepTime, TruthContacts};
    use crate::{KinematicState, PositionMeasurement, Vec3};

    #[test]
    fn zero_noise_applies_bias_without_modifying_truth() -> anyhow::Result<()> {
        check_observation(0.0)
    }

    #[test]
    fn seeded_noise_preserves_axis_order_and_stream_position() -> anyhow::Result<()> {
        check_observation(0.2)
    }

    fn check_observation(stddev_m: f64) -> anyhow::Result<()> {
        let config = NoisyPositionConfig {
            bias_m: [1.0, -2.0, 3.0],
            stddev_m,
        };
        let mut sensor = NoisyPosition::new(&config);
        let truth = KinematicState {
            position_world_m: Vec3::new(10.0, 20.0, 30.0),
            ..KinematicState::default()
        };
        let random = PluginRandom::new(12345, 1, "NoisyPosition:0").at_tick(20);
        let expected_noise_world_m = Vec3::new(
            random.normal(&[0], 0.0, stddev_m)?,
            random.normal(&[1], 0.0, stddev_m)?,
            random.normal(&[2], 0.0, stddev_m)?,
        );
        let expected_position_world_m =
            truth.position_world_m + Vec3::from(config.bias_m) + expected_noise_world_m;
        let mut messages = Messages::default();
        let mut pending = Readings::default();
        sensor.step(&mut SensorContext {
            messages: &mut messages,
            entity: EntityInfo {
                id: 1,
                team_id: 1,
                group_index: 0,
            },
            time: StepTime {
                time_s: 2.0,
                dt_s: 0.1,
                tick: 20,
            },
            truth: &truth,
            contacts_truth: TruthContacts::from(&[][..]),
            random,
            name: "gps",
            reading: ReadingType::of::<PositionMeasurement>(),
            pending: &mut pending,
        })?;
        let observation = pending
            .get::<PositionMeasurement>("gps")?
            .expect("sensor output");
        // Exact equality checks each axis's key.
        assert_eq!(
            observation.value.position_world_m,
            expected_position_world_m
        );
        assert_eq!(
            observation.value.covariance_m2,
            nalgebra::Matrix3::identity() * (stddev_m * stddev_m)
        );
        assert_eq!(truth.position_world_m, Vec3::new(10.0, 20.0, 30.0));
        assert!((observation.sampled_at_s - 2.1).abs() < 1e-12);
        Ok(())
    }
}

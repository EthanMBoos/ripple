//! Noisy own-state measurements, as a reading.
//!
//! Derived from SCRIMMAGE: src/plugins/sensor/NoisyState/NoisyState.cpp (LGPL-3.0-or-later).
//! Sensor phase: sample truth, perturb position/velocity/attitude, publish the reading.
//! Localization (by default PassThrough) turns the reading into belief.
//! Each error is keyed by the reading, quantity, and axis it perturbs.

use anyhow::{Result, ensure};
use nalgebra::UnitQuaternion;
use serde::Deserialize;

use crate::math::{KinematicState, Quaternion, Vec3};
use crate::plugin::{
    Plugin, PluginParams, PluginRandom, ReadingKind, Sensor, SensorContext, Update,
};

/// Gaussian measurement error, shared by NoisyState and NoisyContacts so both
/// take the same parameters, keys, and attitude perturbation. Each error has a
/// bias (its mean) and a standard deviation, per axis; every one defaults to
/// zero, so a mission sets only the noise it wants. Attitude axes are roll,
/// pitch, and yaw.
#[derive(Clone, Copy, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StateNoise {
    pub(super) position_bias_m: [f64; 3],
    pub(super) position_stddev_m: [f64; 3],
    pub(super) velocity_bias_mps: [f64; 3],
    pub(super) velocity_stddev_mps: [f64; 3],
    pub(super) attitude_bias_rad: [f64; 3],
    pub(super) attitude_stddev_rad: [f64; 3],
}

pub struct NoisyState {
    noise: StateNoise,
}

impl Plugin for NoisyState {
    type Config = StateNoise;

    fn configure(params: &PluginParams<'_>) -> Result<StateNoise> {
        let noise: StateNoise = params.parse()?;
        noise.validate()?;
        Ok(noise)
    }

    fn new(noise: &StateNoise) -> Self {
        Self { noise: *noise }
    }

    fn reading_kind(_config: &StateNoise) -> ReadingKind {
        ReadingKind::State
    }
}

impl Sensor for NoisyState {
    /// The first reading, sampled like every later one, so the first decision
    /// sees noise too. It is taken in the tick of the sensor phase's next
    /// reading, so it has its own key.
    fn initialize(&mut self, context: &mut SensorContext<'_>) -> Result<()> {
        let measured = self.measure(context.truth, &context.random, FIRST_READING)?;
        context.publish_reading(measured)
    }

    fn step(&mut self, context: &mut SensorContext<'_>) -> Result<Update> {
        let measured = self.measure(context.truth, &context.random, SENSOR_PHASE_READING)?;
        context.publish_reading(measured)?;
        Ok(Update::Applied)
    }
}

/// Which reading of the tick an error belongs to: the first key part.
const SENSOR_PHASE_READING: u64 = 0;
const FIRST_READING: u64 = 1;

impl NoisyState {
    /// Angular velocity is reported exactly: there is no rate noise.
    fn measure(
        &self,
        truth: &KinematicState,
        random: &PluginRandom,
        reading: u64,
    ) -> Result<KinematicState> {
        self.noise.apply(truth, random, reading)
    }
}

impl StateNoise {
    pub(super) fn validate(&self) -> Result<()> {
        for (name, stddevs) in [
            ("position_stddev_m", self.position_stddev_m),
            ("velocity_stddev_mps", self.velocity_stddev_mps),
            ("attitude_stddev_rad", self.attitude_stddev_rad),
        ] {
            ensure!(
                stddevs.iter().all(|&stddev| stddev >= 0.0),
                "{name} must be nonnegative"
            );
        }
        Ok(())
    }

    /// Returns truth with noisy position, velocity, and attitude. Each error's
    /// key is `[subject, quantity, axis]`: `subject` says what is measured
    /// (a contact's ID, or which own-state reading), quantity is 0 for
    /// position, 1 for velocity, and 2 for attitude, and axis is x, y, z (roll,
    /// pitch, yaw for attitude) as 0, 1, 2. Angular velocity is copied
    /// unchanged; each sensor sets its own.
    pub(super) fn apply(
        &self,
        truth: &KinematicState,
        random: &PluginRandom,
        subject: u64,
    ) -> Result<KinematicState> {
        // DEVNOTE: the key's prefixes are hashed once, not for every number.
        // Hashing every whole key made perf.py's sensing-128 workload (one
        // worker, NoisyContacts on 128 aircraft) 31% slower than the random
        // streams it replaced; rerun perf.py after changing this.
        let random = random.with(&[subject]);
        let quantities = [0, 1, 2].map(|quantity| random.with(&[quantity]));
        let error = |quantity: usize, axis: usize, bias: [f64; 3], stddev: [f64; 3]| {
            quantities[quantity].normal(&[axis as u64], bias[axis], stddev[axis])
        };
        let mut measured = truth.clone();
        for axis in 0..3 {
            measured.position_world_m[axis] = truth.position_world_m[axis]
                + error(0, axis, self.position_bias_m, self.position_stddev_m)?;
            measured.velocity_world_mps[axis] = truth.velocity_world_mps[axis]
                + error(1, axis, self.velocity_bias_mps, self.velocity_stddev_mps)?;
        }

        let attitude_error =
            |axis| error(2, axis, self.attitude_bias_rad, self.attitude_stddev_rad);
        let roll_noise_rad = attitude_error(0)?;
        let pitch_noise_rad = attitude_error(1)?;
        let yaw_noise_rad = attitude_error(2)?;
        let roll_error =
            UnitQuaternion::from_axis_angle(&Vec3::x_axis(), roll_noise_rad).into_inner();
        let pitch_error =
            UnitQuaternion::from_axis_angle(&Vec3::y_axis(), pitch_noise_rad).into_inner();
        let yaw_error =
            UnitQuaternion::from_axis_angle(&Vec3::z_axis(), yaw_noise_rad).into_inner();
        let attitude = truth.orientation_world_from_body;
        let truth_orientation =
            nalgebra::Quaternion::new(attitude.w, attitude.x, attitude.y, attitude.z);
        let orientation = truth_orientation * roll_error * pitch_error * yaw_error;
        // Right multiplication perturbs body axes in roll, pitch, yaw order.
        // Do not replace this with additive Euler angles or normalize the result.
        measured.orientation_world_from_body = Quaternion {
            w: orientation.w,
            x: orientation.i,
            y: orientation.j,
            z: orientation.k,
        };
        Ok(measured)
    }
}

#[cfg(test)]
mod tests {
    use super::{FIRST_READING, NoisyState, SENSOR_PHASE_READING, StateNoise};
    use crate::math::{EulerAngles, KinematicState, Quaternion, Vec3};
    use crate::plugin::{Plugin, PluginParams, PluginRandom};

    fn sensor(stddev: f64) -> NoisyState {
        NoisyState::new(&StateNoise {
            position_bias_m: [2.0; 3],
            position_stddev_m: [stddev; 3],
            velocity_bias_mps: [-1.0; 3],
            velocity_stddev_mps: [stddev; 3],
            ..StateNoise::default()
        })
    }

    fn random() -> PluginRandom {
        PluginRandom::new(123, 1, "NoisyState:0")
    }

    #[test]
    fn without_parameters_there_is_no_noise() -> anyhow::Result<()> {
        let sensor = NoisyState::new(&NoisyState::configure(&PluginParams::yaml("{}"))?);
        let truth = KinematicState {
            position_world_m: Vec3::new(10.0, 20.0, 30.0),
            velocity_world_mps: Vec3::new(3.0, 4.0, 5.0),
            ..KinematicState::default()
        };
        let sample = sensor.measure(&truth, &random(), SENSOR_PHASE_READING)?;
        assert_eq!(sample.position_world_m, truth.position_world_m);
        assert_eq!(sample.velocity_world_mps, truth.velocity_world_mps);
        let (q, expected) = (
            sample.orientation_world_from_body,
            truth.orientation_world_from_body,
        );
        assert_eq!(
            [q.w, q.x, q.y, q.z],
            [expected.w, expected.x, expected.y, expected.z]
        );
        Ok(())
    }

    #[test]
    fn zero_noise_reports_the_bias_and_the_true_angular_velocity() -> anyhow::Result<()> {
        let truth = KinematicState {
            position_world_m: Vec3::new(10.0, 20.0, 30.0),
            velocity_world_mps: Vec3::new(3.0, 4.0, 5.0),
            angular_velocity_world_radps: Vec3::repeat(7.0),
            ..KinematicState::default()
        };
        let sample = sensor(0.0).measure(&truth, &random(), SENSOR_PHASE_READING)?;
        assert_eq!(sample.position_world_m, Vec3::new(12.0, 22.0, 32.0));
        assert_eq!(sample.velocity_world_mps, Vec3::new(2.0, 3.0, 4.0));
        assert_eq!(sample.angular_velocity_world_radps, Vec3::repeat(7.0));
        assert_eq!(truth.position_world_m, Vec3::new(10.0, 20.0, 30.0));
        Ok(())
    }

    #[test]
    fn attitude_noise_rotates_body_axes_in_roll_pitch_yaw_order() -> anyhow::Result<()> {
        let mut sensor = sensor(0.0);
        sensor.noise.attitude_bias_rad = [
            std::f64::consts::FRAC_PI_2,
            std::f64::consts::FRAC_PI_2,
            0.0,
        ];
        let truth = KinematicState {
            orientation_world_from_body: Quaternion::from_euler(EulerAngles {
                yaw_world_from_body_rad: std::f64::consts::FRAC_PI_2,
                ..EulerAngles::default()
            }),
            ..KinematicState::default()
        };
        let sample = sensor.measure(&truth, &random(), SENSOR_PHASE_READING)?;
        let nose = sample
            .orientation_world_from_body
            .rotate_body_to_world(Vec3::x());
        assert!((nose - Vec3::new(-1.0, 0.0, 0.0)).norm() < 1e-12);
        Ok(())
    }

    #[test]
    fn each_error_is_the_number_keyed_by_reading_quantity_and_axis() -> anyhow::Result<()> {
        let random = random().at_tick(4);
        let sample =
            sensor(0.5).measure(&KinematicState::default(), &random, SENSOR_PHASE_READING)?;
        for axis in 0..3 {
            let key = |quantity| [SENSOR_PHASE_READING, quantity, axis as u64];
            assert_eq!(
                sample.position_world_m[axis],
                random.normal(&key(0), 2.0, 0.5)?
            );
            assert_eq!(
                sample.velocity_world_mps[axis],
                random.normal(&key(1), -1.0, 0.5)?
            );
        }
        // Another tick, another number.
        let later = sensor(0.5).measure(
            &KinematicState::default(),
            &random.at_tick(5),
            SENSOR_PHASE_READING,
        )?;
        assert_ne!(later.position_world_m, sample.position_world_m);
        // The first reading, taken in the same tick, has its own numbers.
        let first = sensor(0.5).measure(&KinematicState::default(), &random, FIRST_READING)?;
        assert_ne!(first.position_world_m, sample.position_world_m);
        Ok(())
    }

    #[test]
    fn position_noise_has_the_configured_mean_and_variance() -> anyhow::Result<()> {
        let sensor = sensor(0.5);
        let mut sum = 0.0;
        let mut sum_squared = 0.0;
        for tick in 0..10000 {
            let x = sensor
                .measure(
                    &KinematicState::default(),
                    &random().at_tick(tick),
                    SENSOR_PHASE_READING,
                )?
                .position_world_m
                .x;
            sum += x;
            sum_squared += x * x;
        }
        let mean = sum / 10000.0;
        let variance = sum_squared / 10000.0 - mean * mean;
        assert!((mean - 2.0).abs() < 0.03);
        assert!((variance - 0.25).abs() < 0.02);
        Ok(())
    }

    #[test]
    fn invalid_noise_parameters_are_rejected() {
        // Non-finite values never reach a plugin: loading a mission rejects them.
        for params in [
            "position_stddev_m: [0, -1, 0]",
            "position_bias_m: [0, 1]",
            "pos_noise_0: [0, 5]",
        ] {
            assert!(
                NoisyState::configure(&PluginParams::yaml(params)).is_err(),
                "{params}"
            );
        }
    }
}

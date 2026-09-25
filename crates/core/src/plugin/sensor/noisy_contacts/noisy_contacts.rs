//! Noisy measurements of every other contact, delivered on LocalNetwork.
//!
//! C++ counterpart: src/plugins/sensor/NoisyContacts/NoisyContacts.cpp (LGPL-3.0-or-later).
//! Sensor phase: sample contact truth and publish one complete measured snapshot.
//! Own belief is unchanged. Uses Rust's per-instance RNG, not C++'s shared random sequence.

use anyhow::{Result, ensure};
use nalgebra::Matrix3;
use serde::Deserialize;

use super::noisy_state::{AxisNoise, StateNoise};
use crate::math::KinematicState;
use crate::plugin::sensor::StateWithCovariance;
use crate::plugin::{
    EntityInfo, Plugin, PluginParams, PluginRandom, Sensor, SensorContext, Update,
};

pub const CONTACTS_TOPIC: &str = "ContactsWithCovariances";

/// Mission parameters: the topic plus NoisyState's nine `mean standard_deviation`
/// keys. `Default` supplies any key the mission leaves out.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct NoisyContactsParams {
    topic_name: String,
    pos_noise_0: AxisNoise,
    pos_noise_1: AxisNoise,
    pos_noise_2: AxisNoise,
    vel_noise_0: AxisNoise,
    vel_noise_1: AxisNoise,
    vel_noise_2: AxisNoise,
    orient_noise_0: AxisNoise,
    orient_noise_1: AxisNoise,
    orient_noise_2: AxisNoise,
}

impl Default for NoisyContactsParams {
    fn default() -> Self {
        let noise = StateNoise::default();
        Self {
            topic_name: CONTACTS_TOPIC.into(),
            pos_noise_0: noise.position_m[0],
            pos_noise_1: noise.position_m[1],
            pos_noise_2: noise.position_m[2],
            vel_noise_0: noise.velocity_mps[0],
            vel_noise_1: noise.velocity_mps[1],
            vel_noise_2: noise.velocity_mps[2],
            orient_noise_0: noise.orientation_rad[0],
            orient_noise_1: noise.orientation_rad[1],
            orient_noise_2: noise.orientation_rad[2],
        }
    }
}

pub struct NoisyContactsConfig {
    topic: String,
    noise: StateNoise,
}

/// One measured contact.
#[derive(Clone, Debug)]
pub struct ContactWithCovariance {
    pub entity: EntityInfo,
    pub measurement: StateWithCovariance,
}

/// A complete snapshot in entity-ID order, excluding the observing entity.
/// Empty snapshots clear previous detections; this is not a persistent tracker.
#[derive(Clone, Debug)]
pub struct ContactsWithCovariances {
    pub contacts: Vec<ContactWithCovariance>,
}

pub struct NoisyContacts {
    topic: String,
    noise: StateNoise,
}

impl Plugin for NoisyContacts {
    type Config = NoisyContactsConfig;

    fn configure(params: &PluginParams<'_>) -> Result<NoisyContactsConfig> {
        let params: NoisyContactsParams = params.parse()?;
        ensure!(
            !params.topic_name.trim().is_empty(),
            "NoisyContacts topic_name must not be empty"
        );
        let noise = StateNoise {
            position_m: [params.pos_noise_0, params.pos_noise_1, params.pos_noise_2],
            velocity_mps: [params.vel_noise_0, params.vel_noise_1, params.vel_noise_2],
            orientation_rad: [
                params.orient_noise_0,
                params.orient_noise_1,
                params.orient_noise_2,
            ],
        };
        noise.validate()?;
        Ok(NoisyContactsConfig {
            topic: params.topic_name,
            noise,
        })
    }

    fn new(config: &NoisyContactsConfig) -> Self {
        Self {
            topic: config.topic.clone(),
            noise: config.noise,
        }
    }
}

impl Sensor for NoisyContacts {
    fn step(&mut self, context: &mut SensorContext<'_>) -> Result<Update> {
        // Matches C++ for now, though a real sensor would have limited range and
        // field of view: every other live entity is measured, however far away.
        let mut contacts: Vec<_> = context
            .contacts_truth
            .iter()
            .filter(|contact| contact.id != context.entity.id && contact.active)
            .collect();
        contacts.sort_by_key(|contact| contact.id);
        let mut measured = Vec::with_capacity(contacts.len());
        for contact in contacts {
            measured.push(ContactWithCovariance {
                entity: EntityInfo {
                    id: contact.id,
                    team_id: contact.team_id,
                    sub_swarm_id: contact.sub_swarm_id,
                },
                measurement: self.measure(&contact.truth, context.random)?,
            });
        }
        context.messages.publish(
            "LocalNetwork",
            &self.topic,
            ContactsWithCovariances { contacts: measured },
        )?;
        Ok(Update::Applied)
    }
}

impl NoisyContacts {
    fn measure(
        &self,
        truth: &KinematicState,
        random: &mut PluginRandom,
    ) -> Result<StateWithCovariance> {
        // C++ copies the contact's state, so angular velocity passes through unchanged.
        // Matches C++ for now: 5I is a placeholder, not the configured noise variance.
        Ok(StateWithCovariance {
            state: self.noise.apply(truth, random)?,
            covariance: Matrix3::identity() * 5.0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::noisy_state::{AxisNoise, StateNoise};
    use super::{NoisyContacts, NoisyContactsConfig};
    use crate::Params;
    use crate::math::{EulerAngles, KinematicState, Quaternion, Vec3};
    use crate::plugin::{Plugin, PluginParams, PluginRandom};

    fn sensor(stddev: f64) -> NoisyContacts {
        NoisyContacts::new(&NoisyContactsConfig {
            topic: super::CONTACTS_TOPIC.into(),
            noise: StateNoise {
                position_m: [AxisNoise { mean: 2.0, stddev }; 3],
                velocity_mps: [AxisNoise { mean: -1.0, stddev }; 3],
                orientation_rad: [AxisNoise {
                    mean: 0.0,
                    stddev: 0.0,
                }; 3],
            },
        })
    }

    #[test]
    fn zero_noise_preserves_legacy_bias_covariance_and_angular_velocity() -> anyhow::Result<()> {
        let truth = KinematicState {
            position_world_m: Vec3::new(10.0, 20.0, 30.0),
            velocity_world_mps: Vec3::new(3.0, 4.0, 5.0),
            angular_velocity_world_radps: Vec3::repeat(7.0),
            ..KinematicState::default()
        };
        let sample =
            sensor(0.0).measure(&truth, &mut PluginRandom::new(123, 1, "NoisyContacts:0"))?;
        assert_eq!(sample.state.position_world_m, Vec3::new(12.0, 22.0, 32.0));
        assert_eq!(sample.state.velocity_world_mps, Vec3::new(2.0, 3.0, 4.0));
        assert_eq!(sample.state.angular_velocity_world_radps, Vec3::repeat(7.0));
        assert_eq!(sample.covariance, nalgebra::Matrix3::identity() * 5.0);
        assert_eq!(truth.position_world_m, Vec3::new(10.0, 20.0, 30.0));
        Ok(())
    }

    #[test]
    fn attitude_noise_rotates_body_axes_in_roll_pitch_yaw_order() -> anyhow::Result<()> {
        let mut sensor = sensor(0.0);
        sensor.noise.orientation_rad[0].mean = std::f64::consts::FRAC_PI_2;
        sensor.noise.orientation_rad[1].mean = std::f64::consts::FRAC_PI_2;
        let truth = KinematicState {
            orientation_world_from_body: Quaternion::from_euler(EulerAngles {
                yaw_world_from_body_rad: std::f64::consts::FRAC_PI_2,
                ..EulerAngles::default()
            }),
            ..KinematicState::default()
        };
        let sample = sensor.measure(&truth, &mut PluginRandom::new(123, 1, "NoisyContacts:0"))?;
        let nose = sample
            .state
            .orientation_world_from_body
            .rotate_body_to_world(Vec3::x());
        assert!((nose - Vec3::new(-1.0, 0.0, 0.0)).norm() < 1e-12);
        Ok(())
    }

    #[test]
    fn seeded_samples_follow_interleaved_axis_order() -> anyhow::Result<()> {
        let mut random = PluginRandom::new(123, 1, "NoisyContacts:0");
        let mut expected = PluginRandom::new(123, 1, "NoisyContacts:0");
        let sample = sensor(0.5).measure(&KinematicState::default(), &mut random)?;
        for axis in 0..3 {
            assert_eq!(
                sample.state.position_world_m[axis],
                expected.normal(2.0, 0.5)?
            );
            assert_eq!(
                sample.state.velocity_world_mps[axis],
                expected.normal(-1.0, 0.5)?
            );
        }
        for _ in 0..3 {
            expected.normal(0.0, 0.0)?;
        }
        assert_eq!(random.normal(0.0, 1.0)?, expected.normal(0.0, 1.0)?);
        Ok(())
    }

    #[test]
    fn position_noise_has_the_configured_mean_and_variance() -> anyhow::Result<()> {
        let sensor = sensor(0.5);
        let mut random = PluginRandom::new(123, 1, "NoisyContacts:0");
        let mut sum = 0.0;
        let mut sum_squared = 0.0;
        for _ in 0..10000 {
            let x = sensor
                .measure(&KinematicState::default(), &mut random)?
                .state
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
        for value in ["0 -1", "NaN 1", "0 inf", "0 1 2"] {
            let params = Params::from([("pos_noise_0".into(), value.into())]);
            assert!(NoisyContacts::configure(&PluginParams(&params)).is_err());
        }
    }
}

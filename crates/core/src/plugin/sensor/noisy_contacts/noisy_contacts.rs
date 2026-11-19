//! Noisy measurements of every other contact, as a reading.
//!
//! Derived from SCRIMMAGE: src/plugins/sensor/NoisyContacts/NoisyContacts.cpp (LGPL-3.0-or-later).
//! Sensor phase: sample contact truth and publish one complete snapshot of
//! detections, each with the covariance of the noise added and the contact's
//! identity (this is a cooperative sensor: it knows who it measures).
//! Tracking (by default PassThrough) turns the reading into tracks.
//! Each error is keyed by the contact's ID, the quantity, and the axis, so an
//! observer's error on a contact doesn't depend on which others it sees.

use anyhow::Result;

use super::noisy_state::StateNoise;
use crate::math::{GaussianState, KinematicState};
use crate::plugin::estimator::Identity;
use crate::plugin::{Detection, DetectionList, PluginParams, PluginRandom, Sensor, SensorContext};

/// Takes NoisyState's parameters: a bias and standard deviation per axis for
/// position, velocity, and attitude, each defaulting to zero.
pub struct NoisyContacts {
    noise: StateNoise,
}

impl Sensor for NoisyContacts {
    type Config = StateNoise;
    type Reading = DetectionList;

    fn configure(params: &PluginParams) -> Result<StateNoise> {
        let noise: StateNoise = params.parse()?;
        noise.validate()?;
        Ok(noise)
    }

    fn new(noise: &StateNoise) -> Self {
        Self { noise: *noise }
    }

    fn step(&mut self, context: &mut SensorContext) -> Result<()> {
        // Every other live entity, however far: an ideal sensor. A
        // range-limited one should ask `contacts_truth.within` so the grid
        // helps; here 98% of the time is generating noise.
        let mut contacts: Vec<_> = context
            .contacts_truth
            .iter()
            .filter(|contact| contact.id != context.entity.id && contact.active)
            .collect();
        contacts.sort_by_key(|contact| contact.id);
        // A complete snapshot in entity-ID order, excluding the observing entity;
        // an empty one clears earlier detections.
        let mut reading = Vec::with_capacity(contacts.len());
        for contact in contacts {
            reading.push(Detection {
                state: self.measure(&contact.truth, &context.random, contact.id)?,
                identity: Some(Identity::of(contact)),
            });
        }
        context.publish_reading(DetectionList::from(reading))?;
        Ok(())
    }
}

impl NoisyContacts {
    /// The perturbed truth with the covariance of the noise added. Angular
    /// velocity passes through unchanged.
    fn measure(
        &self,
        truth: &KinematicState,
        random: &PluginRandom,
        contact_id: i32,
    ) -> Result<GaussianState> {
        Ok(GaussianState {
            mean: self
                .noise
                .apply(truth, random, i64::from(contact_id) as u64)?,
            covariance: self.noise.covariance(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::noisy_state::StateNoise;
    use super::NoisyContacts;
    use crate::math::{KinematicState, Vec3};
    use crate::plugin::{PluginParams, PluginRandom, Sensor};

    fn sensor(stddev: f64) -> NoisyContacts {
        NoisyContacts::new(&StateNoise {
            position_bias_m: [2.0; 3],
            position_stddev_m: [stddev; 3],
            velocity_bias_mps: [-1.0; 3],
            velocity_stddev_mps: [stddev; 3],
            ..StateNoise::default()
        })
    }

    #[test]
    fn zero_noise_reports_the_bias_and_the_true_angular_velocity() -> anyhow::Result<()> {
        let truth = KinematicState {
            position_world_m: Vec3::new(10.0, 20.0, 30.0),
            velocity_world_mps: Vec3::new(3.0, 4.0, 5.0),
            angular_velocity_world_radps: Vec3::repeat(7.0),
            ..KinematicState::default()
        };
        let random = PluginRandom::new(123, 1, "NoisyContacts:0");
        let sample = sensor(0.0).measure(&truth, &random, 7)?;
        assert_eq!(sample.mean.position_world_m, Vec3::new(12.0, 22.0, 32.0));
        assert_eq!(sample.mean.velocity_world_mps, Vec3::new(2.0, 3.0, 4.0));
        assert_eq!(sample.mean.angular_velocity_world_radps, Vec3::repeat(7.0));
        assert_eq!(sample.covariance, crate::StateCovariance::zero());
        Ok(())
    }

    #[test]
    fn each_error_is_the_number_keyed_by_contact_quantity_and_axis() -> anyhow::Result<()> {
        let random = PluginRandom::new(123, 1, "NoisyContacts:0").at_tick(4);
        let sample = sensor(0.5).measure(&KinematicState::default(), &random, 7)?;
        for axis in 0..3 {
            let key = |quantity| [7, quantity, axis as u64];
            assert_eq!(
                sample.mean.position_world_m[axis],
                random.normal(&key(0), 2.0, 0.5)?
            );
            assert_eq!(
                sample.mean.velocity_world_mps[axis],
                random.normal(&key(1), -1.0, 0.5)?
            );
        }
        // The reading reports the variance of the noise it added.
        assert_eq!(sample.covariance.position()[(1, 1)], 0.25);
        assert_eq!(sample.covariance.velocity()[(2, 2)], 0.25);
        // Another contact, another number.
        let other = sensor(0.5).measure(&KinematicState::default(), &random, 8)?;
        assert_ne!(other.mean.position_world_m, sample.mean.position_world_m);
        Ok(())
    }

    #[test]
    fn invalid_noise_parameters_are_rejected() {
        for params in ["position_stddev_m: [0, -1, 0]", "vel_noise_0: [0, 5]"] {
            assert!(
                NoisyContacts::configure(&PluginParams::yaml(params)).is_err(),
                "{params}"
            );
        }
    }
}

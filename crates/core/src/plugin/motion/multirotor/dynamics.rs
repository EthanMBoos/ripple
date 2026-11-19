//! Multirotor integration state: forward/left/up body, ENU world.
//! Unlike FixedWing6DOF, this model needs no body-axis flips.

use crate::math::{Integrable, KinematicState, Quaternion, Vec3};

#[derive(Clone, Copy, Default)]
pub(super) struct RotorcraftState {
    pub velocity_body_mps: Vec3,
    pub rates_body_radps: Vec3,
    pub position_world_m: Vec3,
    pub attitude: Quaternion,
}

/// The rate of each field of `RotorcraftState`.
#[derive(Clone, Copy)]
pub(super) struct RotorcraftRate {
    pub acceleration_body_mps2: Vec3,
    pub angular_acceleration_body_radps2: Vec3,
    pub velocity_world_mps: Vec3,
    pub attitude_rate: Quaternion,
}

impl RotorcraftState {
    /// Truth is the one copy of the vehicle's motion: body velocity and rates
    /// are rotated out of it every step, so an initial or interaction-set
    /// velocity moves the vehicle, and world and body velocity cannot drift
    /// apart.
    pub fn read_truth(&mut self, truth: &KinematicState) {
        self.attitude = truth.orientation_world_from_body;
        let rotation = self.attitude.normalized();
        self.velocity_body_mps = rotation.rotate_world_to_body(truth.velocity_world_mps);
        self.rates_body_radps = rotation.rotate_world_to_body(truth.angular_velocity_world_radps);
        self.position_world_m = truth.position_world_m;
    }

    pub fn write_truth(self, truth: &mut KinematicState) {
        let rotation = self.attitude.normalized();
        truth.orientation_world_from_body = rotation;
        truth.position_world_m = self.position_world_m;
        truth.velocity_world_mps = rotation.rotate_body_to_world(self.velocity_body_mps);
        truth.angular_velocity_world_radps = rotation.rotate_body_to_world(self.rates_body_radps);
    }

    pub fn is_finite(self) -> bool {
        let q = self.attitude;
        [
            self.velocity_body_mps,
            self.rates_body_radps,
            self.position_world_m,
        ]
        .iter()
        .all(|vector| vector.iter().all(|value| value.is_finite()))
            && [q.w, q.x, q.y, q.z].iter().all(|value| value.is_finite())
    }
}

impl Integrable for RotorcraftState {
    type Rate = RotorcraftRate;
    fn plus_scaled(self, scale: f64, rate: RotorcraftRate) -> Self {
        Self {
            velocity_body_mps: self.velocity_body_mps + scale * rate.acceleration_body_mps2,
            rates_body_radps: self.rates_body_radps + scale * rate.angular_acceleration_body_radps2,
            position_world_m: self.position_world_m + scale * rate.velocity_world_mps,
            attitude: self.attitude + scale * rate.attitude_rate,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RotorcraftState;
    use crate::math::{EulerAngles, KinematicState, Quaternion, Vec3};

    #[test]
    fn body_velocity_and_rates_round_trip_through_world_truth() {
        let orientation = Quaternion::from_euler(EulerAngles {
            roll_world_from_body_rad: 0.2,
            pitch_world_from_body_rad: -0.3,
            yaw_world_from_body_rad: 1.1,
        });
        let mut truth = KinematicState {
            orientation_world_from_body: orientation,
            ..KinematicState::default()
        };
        let mut state = RotorcraftState::default();
        state.read_truth(&truth);
        state.velocity_body_mps = Vec3::new(4.0, -5.0, 6.0);
        state.rates_body_radps = Vec3::new(1.0, 2.0, 3.0);
        state.write_truth(&mut truth);
        let expected = orientation.rotate_body_to_world(state.rates_body_radps);
        assert!((truth.angular_velocity_world_radps - expected).norm() < 1e-12);
        let expected = orientation.rotate_body_to_world(state.velocity_body_mps);
        assert!((truth.velocity_world_mps - expected).norm() < 1e-12);

        let mut read_back = RotorcraftState::default();
        read_back.read_truth(&truth);
        assert!((read_back.velocity_body_mps - state.velocity_body_mps).norm() < 1e-12);
        assert!((read_back.rates_body_radps - state.rates_body_radps).norm() < 1e-12);
    }
}

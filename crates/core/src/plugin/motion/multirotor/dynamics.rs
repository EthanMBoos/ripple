//! Multirotor integration coordinates: forward/left/up body, ENU world.
//! Unlike FixedWing6DOF, this model needs no body-axis flips.

use nalgebra::{Quaternion as BodyQuaternion, UnitQuaternion};

use crate::math::{KinematicState, Quaternion, Vec3};

#[derive(Clone, Copy)]
pub(super) struct RotorcraftState {
    pub velocity_body_mps: Vec3,
    pub rates_body_radps: Vec3,
    pub position_world_m: Vec3,
    pub attitude: BodyQuaternion<f64>,
}

impl Default for RotorcraftState {
    fn default() -> Self {
        Self {
            velocity_body_mps: Vec3::zeros(),
            rates_body_radps: Vec3::zeros(),
            position_world_m: Vec3::zeros(),
            attitude: BodyQuaternion::identity(),
        }
    }
}

impl RotorcraftState {
    /// Truth is the one copy of the vehicle's motion: body velocity and rates
    /// are rotated out of it every step, so an initial or interaction-set
    /// velocity moves the vehicle, and world and body velocity cannot drift
    /// apart.
    pub fn read_truth(&mut self, truth: &KinematicState) {
        let attitude = truth.orientation_world_from_body;
        self.attitude = BodyQuaternion::new(attitude.w, attitude.x, attitude.y, attitude.z);
        let rotation = UnitQuaternion::new_normalize(self.attitude);
        self.velocity_body_mps = rotation.inverse_transform_vector(&truth.velocity_world_mps);
        self.rates_body_radps =
            rotation.inverse_transform_vector(&truth.angular_velocity_world_radps);
        self.position_world_m = truth.position_world_m;
    }

    pub fn write_truth(self, truth: &mut KinematicState) {
        let rotation = UnitQuaternion::new_normalize(self.attitude);
        let attitude = rotation.quaternion();
        truth.orientation_world_from_body = Quaternion {
            w: attitude.w,
            x: attitude.i,
            y: attitude.j,
            z: attitude.k,
        };
        truth.position_world_m = self.position_world_m;
        truth.velocity_world_mps = rotation.transform_vector(&self.velocity_body_mps);
        truth.angular_velocity_world_radps = rotation.transform_vector(&self.rates_body_radps);
    }

    // Flatten only at the RK4 boundary; physical equations use the named fields.
    pub fn coordinates(self) -> [f64; 13] {
        let velocity = self.velocity_body_mps;
        let rates = self.rates_body_radps;
        let position = self.position_world_m;
        let attitude = self.attitude;
        [
            velocity.x, velocity.y, velocity.z, rates.x, rates.y, rates.z, position.x, position.y,
            position.z, attitude.w, attitude.i, attitude.j, attitude.k,
        ]
    }

    pub fn from_coordinates(coordinates: [f64; 13]) -> Self {
        let [u, v, w, p, q, r, x, y, z, qw, qx, qy, qz] = coordinates;
        Self {
            velocity_body_mps: Vec3::new(u, v, w),
            rates_body_radps: Vec3::new(p, q, r),
            position_world_m: Vec3::new(x, y, z),
            attitude: BodyQuaternion::new(qw, qx, qy, qz),
        }
    }
}

pub(super) fn derivative(
    state: RotorcraftState,
    acceleration_body_mps2: Vec3,
    angular_acceleration_body_radps2: Vec3,
) -> [f64; 13] {
    let rates = state.rates_body_radps;
    let attitude = state.attitude;
    let qw = attitude.w;
    let qx = attitude.i;
    let qy = attitude.j;
    let qz = attitude.k;
    let lambda = 1.0 - (qw * qw + qx * qx + qy * qy + qz * qz);
    let attitude_derivative = BodyQuaternion::new(
        -0.5 * (qx * rates.x + qy * rates.y + qz * rates.z) + lambda * qw,
        0.5 * (qw * rates.x + qy * rates.z - qz * rates.y) + lambda * qx,
        0.5 * (qw * rates.y + qz * rates.x - qx * rates.z) + lambda * qy,
        0.5 * (qw * rates.z + qx * rates.y - qy * rates.x) + lambda * qz,
    );
    let rotation = UnitQuaternion::new_normalize(attitude);
    RotorcraftState {
        velocity_body_mps: state.velocity_body_mps.cross(&rates) + acceleration_body_mps2,
        rates_body_radps: angular_acceleration_body_radps2,
        position_world_m: rotation.transform_vector(&state.velocity_body_mps),
        attitude: attitude_derivative,
    }
    .coordinates()
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

//! Integration state in the aerospace frames: forward/right/down body and
//! north/east/down world. Truth is the engine's forward/left/up and
//! east/north/up; `from_truth` and `write_truth` convert.

use std::f64::consts::FRAC_1_SQRT_2;

use crate::math::{Integrable, KinematicState, Quaternion, Vec3};

#[derive(Clone, Copy)]
pub(super) struct AircraftState {
    pub velocity_frd_mps: Vec3,
    pub rates_frd_radps: Vec3,
    pub velocity_ned_mps: Vec3,
    pub position_ned_m: Vec3,
    pub attitude_ned_from_frd: Quaternion,
}

/// The rate of each field of `AircraftState`.
#[derive(Clone, Copy)]
pub(super) struct AircraftRate {
    pub acceleration_frd_mps2: Vec3,
    pub angular_acceleration_frd_radps2: Vec3,
    pub acceleration_ned_mps2: Vec3,
    pub velocity_ned_mps: Vec3,
    pub attitude_rate: Quaternion,
}

/// Swap the horizontal axes and point z down; its own inverse.
pub(super) fn ned_from_enu(vector_enu: Vec3) -> Vec3 {
    Vec3::new(vector_enu.y, vector_enu.x, -vector_enu.z)
}

pub(super) fn enu_from_ned(vector_ned: Vec3) -> Vec3 {
    ned_from_enu(vector_ned)
}

/// `ned_from_enu * enu_from_flu * flu_from_frd`: a half-turn about the
/// north-east diagonal and one about the nose. Its own inverse.
fn switch_frames(attitude: Quaternion) -> Quaternion {
    let ned_from_enu = Quaternion {
        w: 0.0,
        x: FRAC_1_SQRT_2,
        y: FRAC_1_SQRT_2,
        z: 0.0,
    };
    let flu_from_frd = Quaternion {
        w: 0.0,
        x: 1.0,
        y: 0.0,
        z: 0.0,
    };
    ned_from_enu * attitude * flu_from_frd
}

impl AircraftState {
    pub fn from_truth(truth: &KinematicState) -> Self {
        let attitude_ned_from_frd = switch_frames(truth.orientation_world_from_body);
        Self {
            velocity_frd_mps: attitude_ned_from_frd
                .rotate_world_to_body(ned_from_enu(truth.velocity_world_mps)),
            rates_frd_radps: attitude_ned_from_frd
                .rotate_world_to_body(ned_from_enu(truth.angular_velocity_world_radps)),
            velocity_ned_mps: ned_from_enu(truth.velocity_world_mps),
            position_ned_m: ned_from_enu(truth.position_world_m),
            attitude_ned_from_frd,
        }
    }

    pub fn write_truth(self, truth: &mut KinematicState) {
        let rotation = self.attitude_ned_from_frd.normalized();
        truth.orientation_world_from_body = switch_frames(rotation);
        truth.position_world_m = enu_from_ned(self.position_ned_m);
        truth.velocity_world_mps = enu_from_ned(self.velocity_ned_mps);
        truth.angular_velocity_world_radps =
            enu_from_ned(rotation.rotate_body_to_world(self.rates_frd_radps));
    }

    pub fn is_finite(self) -> bool {
        let q = self.attitude_ned_from_frd;
        [
            self.velocity_frd_mps,
            self.rates_frd_radps,
            self.velocity_ned_mps,
            self.position_ned_m,
        ]
        .iter()
        .all(|vector| vector.iter().all(|value| value.is_finite()))
            && [q.w, q.x, q.y, q.z].iter().all(|value| value.is_finite())
    }
}

impl Integrable for AircraftState {
    type Rate = AircraftRate;
    fn plus_scaled(self, scale: f64, rate: AircraftRate) -> Self {
        Self {
            velocity_frd_mps: self.velocity_frd_mps + scale * rate.acceleration_frd_mps2,
            rates_frd_radps: self.rates_frd_radps + scale * rate.angular_acceleration_frd_radps2,
            velocity_ned_mps: self.velocity_ned_mps + scale * rate.acceleration_ned_mps2,
            position_ned_m: self.position_ned_m + scale * rate.velocity_ned_mps,
            attitude_ned_from_frd: self.attitude_ned_from_frd + scale * rate.attitude_rate,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::FRAC_PI_2;

    use super::{AircraftState, ned_from_enu, switch_frames};
    use crate::math::{EulerAngles, KinematicState, Quaternion, Vec3};

    fn frd_from_flu(vector_flu: Vec3) -> Vec3 {
        Vec3::new(vector_flu.x, -vector_flu.y, -vector_flu.z)
    }

    #[test]
    fn the_switched_attitude_rotates_frd_vectors_into_ned() {
        let attitude_enu_from_flu = Quaternion::from_euler(EulerAngles {
            roll_world_from_body_rad: 0.3,
            pitch_world_from_body_rad: -0.2,
            yaw_world_from_body_rad: 1.1,
        });
        let attitude_ned_from_frd = switch_frames(attitude_enu_from_flu);
        let vector_flu = Vec3::new(1.0, -2.0, 3.0);
        let expected_ned = ned_from_enu(attitude_enu_from_flu.rotate_body_to_world(vector_flu));
        let actual_ned = attitude_ned_from_frd.rotate_body_to_world(frd_from_flu(vector_flu));
        assert!((actual_ned - expected_ned).norm() < 1e-12);
        let back = switch_frames(attitude_ned_from_frd);
        for axis in [Vec3::x(), Vec3::y(), Vec3::z()] {
            let expected = attitude_enu_from_flu.rotate_body_to_world(axis);
            assert!((back.rotate_body_to_world(axis) - expected).norm() < 1e-12);
        }
    }

    #[test]
    fn frd_rates_become_world_rates_at_the_truth_boundary() {
        for (yaw_rad, expected_world_radps) in [
            (0.0, Vec3::new(1.0, -2.0, -3.0)),
            (FRAC_PI_2, Vec3::new(2.0, 1.0, -3.0)),
        ] {
            let mut truth = KinematicState {
                orientation_world_from_body: Quaternion::from_euler(EulerAngles {
                    yaw_world_from_body_rad: yaw_rad,
                    ..EulerAngles::default()
                }),
                ..KinematicState::default()
            };
            let mut aircraft = AircraftState::from_truth(&truth);
            aircraft.rates_frd_radps = Vec3::new(1.0, 2.0, 3.0);
            aircraft.write_truth(&mut truth);
            assert!((truth.angular_velocity_world_radps - expected_world_radps).norm() < 1e-12);
        }
    }

    #[test]
    fn world_rates_become_frd_rates_when_reading_truth() {
        let truth = KinematicState {
            orientation_world_from_body: Quaternion::from_euler(EulerAngles {
                yaw_world_from_body_rad: FRAC_PI_2,
                ..EulerAngles::default()
            }),
            angular_velocity_world_radps: Vec3::new(2.0, 1.0, -3.0),
            ..KinematicState::default()
        };
        let aircraft = AircraftState::from_truth(&truth);
        assert!((aircraft.rates_frd_radps - Vec3::new(1.0, 2.0, 3.0)).norm() < 1e-12);
    }

    #[test]
    fn tilted_aircraft_round_trips_world_state() {
        let original = KinematicState {
            position_world_m: Vec3::new(100.0, -20.0, 500.0),
            velocity_world_mps: Vec3::new(80.0, 30.0, -5.0),
            angular_velocity_world_radps: Vec3::new(0.3, -0.2, 0.1),
            orientation_world_from_body: Quaternion::from_euler(EulerAngles {
                roll_world_from_body_rad: 0.3,
                pitch_world_from_body_rad: -0.2,
                yaw_world_from_body_rad: 1.1,
            }),
        };
        let mut restored = KinematicState::default();
        AircraftState::from_truth(&original).write_truth(&mut restored);
        assert!(
            (restored.angular_velocity_world_radps - original.angular_velocity_world_radps).norm()
                < 1e-12
        );
        assert_eq!(restored.position_world_m, original.position_world_m);
        assert_eq!(restored.velocity_world_mps, original.velocity_world_mps);
        // Compare rotations, not quaternion signs: q and -q describe the same attitude.
        for axis in [Vec3::x(), Vec3::y(), Vec3::z()] {
            let expected = original
                .orientation_world_from_body
                .rotate_body_to_world(axis);
            let actual = restored
                .orientation_world_from_body
                .rotate_body_to_world(axis);
            assert!((actual - expected).norm() < 1e-12);
        }
    }
}

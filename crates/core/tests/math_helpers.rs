//! Math helpers available to student plugins in downstream Rust crates.
use ripple_core::{EulerAngles, Pid, PidGains, Quaternion, Vec3, rk4};

#[test]
fn construct_attitude_and_rotate_a_body_vector() {
    let attitude = Quaternion::from_euler(EulerAngles {
        yaw_world_from_body_rad: std::f64::consts::FRAC_PI_2,
        ..EulerAngles::default()
    });
    let world = attitude.rotate_body_to_world(Vec3::new(1.0, 0.0, 0.0));
    assert!((world - Vec3::new(0.0, 1.0, 0.0)).norm() < 1e-12);
}

#[test]
fn construct_and_step_linear_and_angular_controllers() {
    let gains = PidGains {
        kp: 2.0,
        ki: 0.0,
        kd: 0.0,
        integrate_within: 10.0,
    };
    let mut linear = Pid::linear(gains);
    assert!((linear.step(3.0, 1.0, 0.1) - 4.0).abs() < 1e-12);

    let mut angular = Pid::angular(gains);
    let output = angular.step((-179.0_f64).to_radians(), 179.0_f64.to_radians(), 0.1);
    assert!((output - 4.0_f64.to_radians()).abs() < 1e-12);
}

#[test]
fn integrate_constant_acceleration() {
    let mut position_and_velocity = [0.0, 1.0];
    rk4(&mut position_and_velocity, 0.5, |state| [state[1], 2.0]);
    assert!((position_and_velocity[0] - 0.75).abs() < 1e-12);
    assert!((position_and_velocity[1] - 2.0).abs() < 1e-12);
}

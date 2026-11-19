use super::*;
use std::f64::consts::{PI, TAU};

#[test]
fn positive_yaw_rotates_body_forward_toward_world_y() {
    let orientation = Quaternion::from_euler(EulerAngles {
        yaw_world_from_body_rad: PI / 2.0,
        ..EulerAngles::default()
    });
    let forward_world = orientation.rotate_body_to_world(Vec3::new(1.0, 0.0, 0.0));
    assert!(norm(forward_world - Vec3::new(0.0, 1.0, 0.0)) < 1e-12);
    assert!((orientation.yaw_world_from_body_rad() - PI / 2.0).abs() < 1e-12);
}

#[test]
fn angle_wrapping_preserves_reference_positive_pi_boundary() {
    assert_eq!(angle_pi(-PI), PI);
    assert!((angle_2pi(-0.1) - (TAU - 0.1)).abs() < 1e-12);
}

#[test]
fn quaternion_product_and_normalization_match_nalgebra() {
    let a = Quaternion::from_euler(EulerAngles {
        roll_world_from_body_rad: 0.3,
        pitch_world_from_body_rad: -0.2,
        yaw_world_from_body_rad: 1.1,
    });
    let b = Quaternion {
        w: 0.9,
        x: 0.2,
        y: -0.3,
        z: 0.4,
    };
    let nalgebra_a = nalgebra::Quaternion::new(a.w, a.x, a.y, a.z);
    let nalgebra_b = nalgebra::Quaternion::new(b.w, b.x, b.y, b.z);

    let product = a * b;
    let expected = nalgebra_a * nalgebra_b;
    for (ours, theirs) in [
        (product.w, expected.w),
        (product.x, expected.i),
        (product.y, expected.j),
        (product.z, expected.k),
    ] {
        assert!((ours - theirs).abs() < 1e-15, "{ours} vs {theirs}");
    }

    let unit = b.normalized();
    let expected = nalgebra_b.normalize();
    assert!((unit.w - expected.w).abs() < 1e-15);
    assert!((unit.x - expected.i).abs() < 1e-15);
    assert!((unit.y - expected.j).abs() < 1e-15);
    assert!((unit.z - expected.k).abs() < 1e-15);

    let v = Vec3::new(1.0, -2.0, 3.0);
    let rotation = nalgebra::UnitQuaternion::new_normalize(nalgebra_b);
    assert!((unit.rotate_body_to_world(v) - rotation.transform_vector(&v)).norm() < 1e-14);
    assert!((unit.rotate_world_to_body(v) - rotation.inverse_transform_vector(&v)).norm() < 1e-14);
}

#[test]
fn rk4_integrates_constant_velocity() {
    let position_m = rk4([0.0], 0.1, |_| [21.0]);
    assert!((position_m[0] - 2.1).abs() < 1e-12);
}

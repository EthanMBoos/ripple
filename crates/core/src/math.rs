//! Coordinate conventions and numerical helpers shared by physical models.
mod angles;
mod integration;
mod quaternion;
mod state;

pub(crate) use angles::{angle_2pi, angle_pi};
pub use integration::rk4;
pub use quaternion::{EulerAngles, Quaternion};
pub use state::KinematicState;

pub type Vec3 = nalgebra::Vector3<f64>;

/// Degrees to radians with C++ SCRIMMAGE's rounding (`Angles::deg2rad`):
/// `deg * PI / 180`. `f64::to_radians` multiplies by `PI / 180` instead and can
/// differ in the last bit, which long runs amplify.
pub(crate) fn deg_to_rad(deg: f64) -> f64 {
    deg * std::f64::consts::PI / 180.0
}

pub(crate) fn norm(vector: Vec3) -> f64 {
    (vector.x * vector.x + vector.y * vector.y + vector.z * vector.z).sqrt()
}

pub(crate) fn sub(left: Vec3, right: Vec3) -> Vec3 {
    Vec3::new(left.x - right.x, left.y - right.y, left.z - right.z)
}

#[cfg(test)]
mod tests;

use std::ops::{Add, Mul};

use super::Vec3;
use serde::{Deserialize, Serialize};

/// Scalar-first quaternion representing the rotation from body to world.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Quaternion {
    pub w: f64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EulerAngles {
    pub roll_world_from_body_rad: f64,
    pub pitch_world_from_body_rad: f64,
    pub yaw_world_from_body_rad: f64,
}

impl Quaternion {
    pub fn from_euler(attitude: EulerAngles) -> Self {
        let (sin_roll, cos_roll) = (attitude.roll_world_from_body_rad * 0.5).sin_cos();
        let (sin_pitch, cos_pitch) = (attitude.pitch_world_from_body_rad * 0.5).sin_cos();
        let (sin_yaw, cos_yaw) = (attitude.yaw_world_from_body_rad * 0.5).sin_cos();

        Self {
            w: cos_roll * cos_pitch * cos_yaw + sin_roll * sin_pitch * sin_yaw,
            x: sin_roll * cos_pitch * cos_yaw - cos_roll * sin_pitch * sin_yaw,
            y: cos_roll * sin_pitch * cos_yaw + sin_roll * cos_pitch * sin_yaw,
            z: cos_roll * cos_pitch * sin_yaw - sin_roll * sin_pitch * cos_yaw,
        }
    }

    /// Rotate a body-frame vector into world coordinates; assumes a unit quaternion.
    pub fn rotate_body_to_world(self, vector_body: Vec3) -> Vec3 {
        let quaternion_vector = Vec3::new(self.x, self.y, self.z);
        let first_cross = quaternion_vector.cross(&vector_body);
        let second_cross = quaternion_vector.cross(&first_cross);

        // Keep the reference operation order instead of introducing normalization.
        Vec3::new(
            vector_body.x + 2.0 * self.w * first_cross.x + 2.0 * second_cross.x,
            vector_body.y + 2.0 * self.w * first_cross.y + 2.0 * second_cross.y,
            vector_body.z + 2.0 * self.w * first_cross.z + 2.0 * second_cross.z,
        )
    }

    /// Inverse rotation; assumes a unit quaternion.
    pub fn rotate_world_to_body(self, vector_world: Vec3) -> Vec3 {
        Self {
            w: self.w,
            x: -self.x,
            y: -self.y,
            z: -self.z,
        }
        .rotate_body_to_world(vector_world)
    }

    pub fn pitch_world_from_body_rad(self) -> f64 {
        (2.0 * (self.w * self.y - self.z * self.x))
            .clamp(-1.0, 1.0)
            .asin()
    }

    pub fn yaw_world_from_body_rad(self) -> f64 {
        (2.0 * (self.w * self.z + self.x * self.y))
            .atan2(1.0 - 2.0 * (self.y * self.y + self.z * self.z))
    }

    pub fn roll_world_from_body_rad(self) -> f64 {
        (2.0 * (self.w * self.x + self.y * self.z))
            .atan2(1.0 - 2.0 * (self.x * self.x + self.y * self.y))
    }

    /// The same attitude at unit length.
    pub fn normalized(self) -> Self {
        let norm = (self.w * self.w + self.x * self.x + self.y * self.y + self.z * self.z).sqrt();
        Self {
            w: self.w / norm,
            x: self.x / norm,
            y: self.y / norm,
            z: self.z / norm,
        }
    }
}

impl Add for Quaternion {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self {
            w: self.w + other.w,
            x: self.x + other.x,
            y: self.y + other.y,
            z: self.z + other.z,
        }
    }
}

impl Mul<Quaternion> for f64 {
    type Output = Quaternion;
    fn mul(self, q: Quaternion) -> Quaternion {
        Quaternion {
            w: self * q.w,
            x: self * q.x,
            y: self * q.y,
            z: self * q.z,
        }
    }
}

/// The Hamilton product: `a * b` is the rotation `b` followed by `a`.
impl Mul for Quaternion {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        let (a, b) = (self, rhs);
        Self {
            w: a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z,
            x: a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y,
            y: a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x,
            z: a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w,
        }
    }
}

impl Default for Quaternion {
    fn default() -> Self {
        Self {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }
}

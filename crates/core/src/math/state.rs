use super::{Quaternion, Vec3};
use serde::{Deserialize, Serialize};

/// Position and both velocities use the local ENU world frame.
/// Attitude rotates forward/left/up body axes into that world frame.
/// Model-specific state remains private to the motion plugin.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct KinematicState {
    pub position_world_m: Vec3,
    pub orientation_world_from_body: Quaternion,
    pub velocity_world_mps: Vec3,
    pub angular_velocity_world_radps: Vec3,
}

//! Landing gear: wheels that roll on the ground, after JSBSim's FGLGear
//! (`BOGEY` contacts), simplified where noted. Without it, FixedWing6DOF has
//! only a flat spring at `z = 0`.
//!
//! Each wheel is a point on a strut along the aircraft's down axis. Below the
//! ground (`Terrain::ground_height`, or flat `z = 0` without terrain) it
//! pushes along the ground normal and adds friction in the ground plane, with
//! the moments of both about the centre of gravity. Per wheel, as in JSBSim:
//!
//! - **Strut:** compression is the depth below the ground measured along the
//!   strut; force is spring times compression plus damper times compression
//!   speed, never pulling. On first touching, compression speed is capped at
//!   compression / dt, so the damper builds up with the compression.
//! - **Friction:** the force that stops the wheel sliding within one step, up
//!   to a limit. Rolling it is limited to `rolling_friction` times the load
//!   (no brakes). Sideways the limit is Pacejka's Magic Formula of the slip
//!   angle, with JSBSim's default factors and peak `static_friction`. A
//!   castered wheel swivels to its motion, so it has no side force.
//! - **Steering:** a wheel `steered_by_rudder` turns by the rudder's angle,
//!   pointing right for positive rudder (which yaws the nose left), as a
//!   tail wheel linked to the rudder does on RC taildraggers. JSBSim steers
//!   by a separate command scaled by `max_steer`.
//!
//! Simplified from JSBSim: friction is solved per wheel, each stopping its
//! share of the mass (the mass over the wheels touching), where JSBSim solves
//! all contacts together with the full mass and inertia; and forces act at
//! the uncompressed wheel point, not the compressed one (millimetres here).
//! TODO: solve all wheels' friction together, as JSBSim does, if a mission
//! needs better coupling than a mass share (hard braking, sharp turns).
//!
//! The forces are recomputed at every RK4 stage. The strut damper is stiff:
//! `check_step` refuses a step where `dt × total damping / mass` is beyond
//! RK4's stable range. That is a basic damping check, not a stability
//! guarantee (it leaves out the springs, rotation, and wheel layout); 0.0025 s
//! is the tested Rascal setting.

use anyhow::{Result, ensure};
use nalgebra::UnitQuaternion;
use serde::Deserialize;

use super::dynamics::{AircraftState, flip_yz};
use crate::{Terrain, Vec3};

/// The furthest RK4 stays stable along the negative real axis: a damper
/// with `dt × damping / mass` beyond this grows instead of settling.
const RK4_STABLE_LIMIT: f64 = 2.785;
/// JSBSim's Magic Formula factors for side friction (FGLGear's defaults):
/// stiffness per degree of slip, shape, and curvature.
const SLIP_STIFFNESS_PER_DEG: f64 = 0.06;
const SLIP_SHAPE: f64 = 2.8;
const SLIP_CURVATURE: f64 = 1.03;
/// Below this ground-plane speed (JSBSim's 0.001 ft/s) JSBSim keeps the last
/// slip angle; here it is zero, so a wheel at rest has no side-force limit.
const SLIP_MIN_SPEED_MPS: f64 = 1e-3 * 0.3048;
/// Below this speed (JSBSim's 0.1 ft/s) a castered wheel keeps its heading;
/// here it faces forward.
const CASTER_MIN_SPEED_MPS: f64 = 0.1 * 0.3048;

/// One wheel, an entry in FixedWing6DOF's `landing_gear` list.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Wheel {
    /// Where the uncompressed wheel touches, from the centre of gravity:
    /// forward, right, down (m).
    position_frd_m: [f64; 3],
    /// Strut spring (N/m).
    spring_n_per_m: f64,
    /// Strut damper, compressing and rebounding (N·s/m).
    damping_n_s_per_m: f64,
    /// Peak side friction coefficient (JSBSim's `static_friction`).
    static_friction: f64,
    rolling_friction: f64,
    /// Swivels freely to its direction of motion (JSBSim's max_steer 360).
    #[serde(default)]
    castered: bool,
    /// Turns with the rudder, by the same angle.
    #[serde(default)]
    steered_by_rudder: bool,
}

impl Wheel {
    fn position_m(&self) -> Vec3 {
        Vec3::from(self.position_frd_m)
    }
}

/// Check a mission's wheels.
pub(super) fn check_wheels(wheels: &[Wheel]) -> Result<()> {
    for wheel in wheels {
        ensure!(
            wheel.position_frd_m.iter().all(|value| value.is_finite())
                && wheel.spring_n_per_m > 0.0
                && wheel.damping_n_s_per_m >= 0.0
                && wheel.static_friction >= 0.0
                && wheel.rolling_friction >= 0.0,
            "FixedWing6DOF landing_gear wheels need a finite position, a positive spring, \
             and nonnegative damping and friction"
        );
        ensure!(
            !(wheel.castered && wheel.steered_by_rudder),
            "a FixedWing6DOF wheel is castered or steered_by_rudder, not both"
        );
    }
    Ok(())
}

/// Refuse a motion step too long for the gear's dampers: with every wheel
/// down, `dt × total damping / mass` must stay inside RK4's stable range. A
/// basic check only; passing it does not guarantee a stable step.
pub(super) fn check_step(wheels: &[Wheel], mass_kg: f64, dt_s: f64) -> Result<()> {
    let damping_n_s_per_m: f64 = wheels.iter().map(|wheel| wheel.damping_n_s_per_m).sum();
    let ratio = dt_s * damping_n_s_per_m / mass_kg;
    ensure!(
        ratio < RK4_STABLE_LIMIT,
        "FixedWing6DOF landing gear needs a shorter motion step: dt {dt_s} s gives \
         dt x damping / mass = {ratio:.2}, above RK4's stable {RK4_STABLE_LIMIT}; \
         lower dt_s or raise motion_multiplier below {:.4} s",
        RK4_STABLE_LIMIT * mass_kg / damping_n_s_per_m
    );
    Ok(())
}

/// A wheel touching the ground, before friction.
struct Contact<'a> {
    wheel: &'a Wheel,
    /// Upward ground normal, model axes.
    normal_model: Vec3,
    /// The wheel point's velocity, model axes.
    velocity_model_mps: Vec3,
    normal_force_n: f64,
}

/// The gear's force and moment about the centre of gravity, model axes.
pub(super) fn loads(
    wheels: &[Wheel],
    state: &AircraftState,
    rotation: &UnitQuaternion<f64>,
    terrain: Option<&Terrain>,
    rudder_rad: f64,
    mass_kg: f64,
    dt_s: f64,
) -> (Vec3, Vec3) {
    let mut contacts = Vec::new();
    for wheel in wheels {
        let offset_model_m = wheel.position_m();
        // Model world axes are east, south, down; flip_yz turns them to ENU.
        let position_m =
            state.position_world_m + flip_yz(rotation.transform_vector(&offset_model_m));
        let (ground_z, normal_world) = match terrain {
            Some(terrain) => (
                terrain.ground_height(position_m.x, position_m.y),
                terrain.ground_normal(position_m.x, position_m.y),
            ),
            None => (Terrain::OFF_GRID_Z, Vec3::z()),
        };
        let height_m = position_m.z - ground_z;
        if height_m >= 0.0 {
            continue;
        }
        let normal_model = rotation.inverse_transform_vector(&flip_yz(normal_world));
        // How far the strut (the model down axis) points into the ground.
        let strut_projection = -normal_model.z;
        if strut_projection <= 0.0 {
            continue;
        }
        let velocity_model_mps =
            state.velocity_model_mps + state.rates_model_radps.cross(&offset_model_m);
        let compression_m = -height_m * normal_world.z / strut_projection;
        let max_speed_mps = compression_m / dt_s;
        let compression_speed_mps = (-velocity_model_mps.dot(&normal_model) / strut_projection)
            .clamp(-max_speed_mps, max_speed_mps);
        let strut_force_n = (wheel.spring_n_per_m * compression_m
            + wheel.damping_n_s_per_m * compression_speed_mps)
            .max(0.0);
        contacts.push(Contact {
            wheel,
            normal_model,
            velocity_model_mps,
            normal_force_n: strut_force_n / strut_projection,
        });
    }

    let (mut force_model_n, mut moment_model_nm) = (Vec3::zeros(), Vec3::zeros());
    let mass_share_kg = mass_kg / contacts.len().max(1) as f64;
    for contact in &contacts {
        let normal = contact.normal_model;
        let velocity = contact.velocity_model_mps;
        let along_ground = velocity - velocity.dot(&normal) * normal;
        // Rolling direction: the wheel's heading on the ground, or for a
        // castered wheel the way it is moving. Steering turns it about the
        // model down axis, so a positive angle points it right.
        let wheel_angle_rad = if contact.wheel.steered_by_rudder {
            rudder_rad
        } else {
            0.0
        };
        let pointing = Vec3::new(wheel_angle_rad.cos(), wheel_angle_rad.sin(), 0.0);
        let heading = if contact.wheel.castered && along_ground.norm() > CASTER_MIN_SPEED_MPS {
            along_ground
        } else {
            pointing - pointing.dot(&normal) * normal
        };
        let roll = heading.normalize();
        let side = normal.cross(&roll);
        let (roll_speed, side_speed) = (velocity.dot(&roll), velocity.dot(&side));
        let stopping_force =
            |speed: f64, limit_n: f64| (-mass_share_kg * speed / dt_s).clamp(-limit_n, limit_n);
        let load_n = contact.normal_force_n;
        let roll_force_n = stopping_force(roll_speed, contact.wheel.rolling_friction * load_n);
        let side_force_n = if contact.wheel.castered {
            0.0
        } else {
            let slip_deg = if along_ground.norm() > SLIP_MIN_SPEED_MPS {
                side_speed.atan2(roll_speed.abs()).to_degrees()
            } else {
                0.0
            };
            let limit = side_friction(contact.wheel.static_friction, slip_deg).abs();
            stopping_force(side_speed, limit * load_n)
        };
        let force_n = load_n * normal + roll_force_n * roll + side_force_n * side;
        force_model_n += force_n;
        moment_model_nm += contact.wheel.position_m().cross(&force_n);
    }
    (force_model_n, moment_model_nm)
}

/// Side friction coefficient at a slip angle: Pacejka's Magic Formula with
/// JSBSim's default factors, reaching `peak` at about 12 degrees of slip.
fn side_friction(peak: f64, slip_deg: f64) -> f64 {
    let stiff_slip = SLIP_STIFFNESS_PER_DEG * slip_deg;
    peak * (SLIP_SHAPE * (stiff_slip - SLIP_CURVATURE * (stiff_slip - stiff_slip.atan())).atan())
        .sin()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn side_friction_rises_to_its_peak_and_keeps_the_slip_sign() {
        assert_eq!(side_friction(0.8, 0.0), 0.0);
        let peak = (1..90)
            .map(|deg| side_friction(0.8, f64::from(deg)))
            .fold(0.0, f64::max);
        assert!((peak - 0.8).abs() < 1e-3, "{peak}");
        assert!(side_friction(0.8, -10.0) < 0.0);
    }
}

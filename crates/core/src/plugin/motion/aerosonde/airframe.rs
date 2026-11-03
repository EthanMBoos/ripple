//! Aerosonde vehicles: named, sourced sets of an aircraft's own numbers.
//!
//! A mission either names one with `airframe` (and may override any of its
//! keys) or gives every vehicle number itself. There is no default vehicle.
//! The environment (air density, gravity, wind) is not part of a vehicle.

use anyhow::Result;
use nalgebra::Matrix3;
use serde::Deserialize;

use super::inertia;

/// A built-in aircraft.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AerosondeAirframe {
    /// Beard and McLain's educational Aerosonde: the numbers of
    /// `parameters/aerosonde_parameters.py` in `mavsim_public` at commit
    /// `10363e0e`. Not the aircraft: that file gives 11 kg and starts at
    /// 25 m/s, while the authors' README at the same commit says the real
    /// Aerosonde weighs about 25 kg and cruises near 35 m/s.
    Aerosonde,
}

/// An aircraft's own numbers, SI unless named otherwise. The coefficients
/// are the book's, per radian and dimensionless; the rates enter as
/// p b / 2V, q c / 2V, and r b / 2V.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Vehicle {
    pub mass_kg: f64,
    /// With the product of inertia J_xz entering as -J_xz off the diagonal,
    /// the book's matrix.
    pub inertia_kgm2: Matrix3<f64>,
    pub wing_span_m: f64,
    pub wing_area_m2: f64,
    pub mean_chord_m: f64,
    /// Oswald efficiency in the induced drag C_L^2 / (pi e AR).
    pub oswald_efficiency: f64,
    pub c_lift_0: f64,
    pub c_lift_alpha: f64,
    pub c_lift_q: f64,
    pub c_lift_elevator: f64,
    /// Parasitic drag C_Dp, the constant of the book's parabolic drag.
    pub c_drag_p: f64,
    pub c_drag_q: f64,
    pub c_drag_elevator: f64,
    pub c_pitch_0: f64,
    pub c_pitch_alpha: f64,
    pub c_pitch_q: f64,
    pub c_pitch_elevator: f64,
    /// The stall blend: the sigmoid's rate M and center alpha_0, so the
    /// lift is linear below the stall and a flat plate's past it.
    pub stall_blend_rate: f64,
    pub stall_alpha_rad: f64,
    pub c_side_0: f64,
    pub c_side_beta: f64,
    pub c_side_p: f64,
    pub c_side_r: f64,
    pub c_side_aileron: f64,
    pub c_side_rudder: f64,
    pub c_roll_0: f64,
    pub c_roll_beta: f64,
    pub c_roll_p: f64,
    pub c_roll_r: f64,
    pub c_roll_aileron: f64,
    pub c_roll_rudder: f64,
    pub c_yaw_0: f64,
    pub c_yaw_beta: f64,
    pub c_yaw_p: f64,
    pub c_yaw_r: f64,
    pub c_yaw_aileron: f64,
    pub c_yaw_rudder: f64,
    // The motor and propeller, from the addendum (see `propeller.rs`).
    pub propeller_diameter_m: f64,
    /// The battery's voltage at full throttle.
    pub max_voltage_v: f64,
    /// The motor's back-EMF constant KV, V s/rad.
    pub motor_speed_constant_vspr: f64,
    /// The motor's torque constant KQ, N m/A.
    pub motor_torque_constant_nmpa: f64,
    pub motor_resistance_ohm: f64,
    pub no_load_current_a: f64,
    /// Thrust and torque coefficients against advance ratio J, as the
    /// polynomials C_T2 J^2 + C_T1 J + C_T0 and C_Q2 J^2 + C_Q1 J + C_Q0.
    pub c_thrust_2: f64,
    pub c_thrust_1: f64,
    pub c_thrust_0: f64,
    pub c_torque_2: f64,
    pub c_torque_1: f64,
    pub c_torque_0: f64,
}

impl AerosondeAirframe {
    pub(super) fn vehicle(self) -> Result<Vehicle> {
        match self {
            AerosondeAirframe::Aerosonde => aerosonde(),
        }
    }
}

/// Beard and McLain's Aerosonde, every number from
/// `parameters/aerosonde_parameters.py` in `mavsim_public` at commit
/// `10363e0e`, in the file's own units (SI). The file's `C_D_0` and
/// `C_D_alpha` are the linear drag of the book's chapter 5 linear models,
/// not of chapter 4's parabolic drag, and are not kept; nor are its
/// `S_prop` (the printed book's simple propeller, replaced by the
/// addendum's) and `epsilon`. The motor's KQ is set equal to KV, as the
/// file does.
fn aerosonde() -> Result<Vehicle> {
    let (jx, jy, jz, jxz) = (0.8244, 1.135, 1.759, 0.1204);
    // KV from the datasheet's 145 rpm/V to V s/rad, as the file converts it.
    let kv = (1.0 / 145.0) * 60.0 / (2.0 * std::f64::consts::PI);
    Ok(Vehicle {
        mass_kg: 11.0,
        inertia_kgm2: inertia([[jx, 0.0, -jxz], [0.0, jy, 0.0], [-jxz, 0.0, jz]])?,
        wing_span_m: 2.8956,
        wing_area_m2: 0.55,
        mean_chord_m: 0.18994,
        oswald_efficiency: 0.9,
        c_lift_0: 0.23,
        c_lift_alpha: 5.61,
        c_lift_q: 7.95,
        c_lift_elevator: 0.13,
        c_drag_p: 0.043,
        c_drag_q: 0.0,
        c_drag_elevator: 0.0135,
        c_pitch_0: 0.0135,
        c_pitch_alpha: -2.74,
        c_pitch_q: -38.21,
        c_pitch_elevator: -0.99,
        stall_blend_rate: 50.0,
        stall_alpha_rad: 0.47,
        c_side_0: 0.0,
        c_side_beta: -0.98,
        c_side_p: 0.0,
        c_side_r: 0.0,
        c_side_aileron: 0.075,
        c_side_rudder: 0.19,
        c_roll_0: 0.0,
        c_roll_beta: -0.13,
        c_roll_p: -0.51,
        c_roll_r: 0.25,
        c_roll_aileron: 0.17,
        c_roll_rudder: 0.0024,
        c_yaw_0: 0.0,
        c_yaw_beta: 0.073,
        c_yaw_p: 0.069,
        c_yaw_r: -0.095,
        c_yaw_aileron: -0.011,
        c_yaw_rudder: -0.069,
        propeller_diameter_m: 20.0 * 0.0254,
        max_voltage_v: 3.7 * 12.0,
        motor_speed_constant_vspr: kv,
        motor_torque_constant_nmpa: kv,
        motor_resistance_ohm: 0.042,
        no_load_current_a: 1.5,
        c_thrust_2: -0.1079,
        c_thrust_1: -0.06044,
        c_thrust_0: 0.09357,
        c_torque_2: -0.01664,
        c_torque_1: 0.004970,
        c_torque_0: 0.005230,
    })
}

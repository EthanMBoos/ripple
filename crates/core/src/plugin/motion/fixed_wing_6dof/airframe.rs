//! FixedWing6DOF vehicles: named, sourced sets of an aircraft's own numbers.
//!
//! A mission either names one with `airframe` (and may override any of its
//! keys) or gives every vehicle number itself. There is no default vehicle.
//! The landing gear and the environment (air density, speed of sound,
//! gravity, wind) are not part of a vehicle; its tables and extra terms are,
//! and each is off (empty or zero) unless the vehicle or the mission sets it.

use std::f64::consts::PI;

use anyhow::Result;
use nalgebra::Matrix3;
use serde::Deserialize;

use super::inertia;
use crate::math::Vec3;

/// A built-in aircraft.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FixedWingAirframe {
    /// Douglas A-4D Skyhawk, from Nelson, *Flight Stability and Automatic
    /// Control*, 2nd ed., p. 405, with Pratt and McGhee's NPSNET model.
    /// SCRIMMAGE's default aircraft, kept as the C++ comparison's fixture.
    A4d,
    /// Rascal 110 RC plane, ArduPilot's JSBSim Rascal as JSBSim flies it.
    Rascal,
}

/// An aircraft's own numbers, SI unless named otherwise.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Vehicle {
    pub mass_kg: f64,
    pub inertia_kgm2: Matrix3<f64>,
    pub wing_span_m: f64,
    pub wing_area_m2: f64,
    pub mean_chord_m: f64,
    /// Oswald efficiency in the induced drag C_L^2 / (pi e AR).
    pub oswald_efficiency: f64,
    pub min_thrust_n: f64,
    pub max_thrust_n: f64,
    pub elevator_limits_rad: [f64; 2],
    pub aileron_limits_rad: [f64; 2],
    pub rudder_limits_rad: [f64; 2],
    pub c_drag_0: f64,
    pub c_drag_alpha: f64,
    pub c_drag_elevator: f64,
    pub c_lift_0: f64,
    pub c_lift_alpha: f64,
    pub c_lift_q: f64,
    pub c_lift_elevator: f64,
    pub c_side_beta: f64,
    pub c_side_rudder: f64,
    pub c_roll_beta: f64,
    pub c_roll_p: f64,
    pub c_roll_r: f64,
    pub c_roll_aileron: f64,
    pub c_roll_rudder: f64,
    pub c_pitch_0: f64,
    pub c_pitch_alpha: f64,
    pub c_pitch_q: f64,
    pub c_pitch_elevator: f64,
    pub c_yaw_beta: f64,
    pub c_yaw_p: f64,
    pub c_yaw_r: f64,
    pub c_yaw_aileron: f64,
    pub c_yaw_rudder: f64,
    /// Where the coefficients act, relative to the center of mass, in the
    /// forward/right/down body axes: the moment about the center of mass is
    /// the moment there plus this arm crossed with the force. Zero applies
    /// the loads at the center of mass.
    pub aero_reference_frd_m: Vec3,
    /// Pitch damping from the rate of change of angle of attack, C_m_alphadot.
    pub c_pitch_alpha_dot: f64,
    /// Yaw moment at zero sideslip, rates, and deflection, C_n0.
    pub c_yaw_0: f64,
    /// `[alpha_rad, C_L]` rows replacing the linear lift; empty keeps it.
    pub lift_table: Vec<[f64; 2]>,
    /// `[alpha_rad, C_D]` rows replacing the linear drag; empty keeps it.
    pub drag_table: Vec<[f64; 2]>,
    /// `[beta_rad, C_D]` rows of drag added from sideslip; empty adds none.
    pub sideslip_drag_table: Vec<[f64; 2]>,
    /// `[mach, C_l_delta_a]` rows replacing `c_roll_aileron`; empty keeps it.
    pub aileron_power_mach_table: Vec<[f64; 2]>,
    /// `[mach, C_m_delta_e]` rows replacing `c_pitch_elevator`; empty keeps it.
    pub elevator_power_mach_table: Vec<[f64; 2]>,
}

/// Surface deflection limit 0.5236 rad (30 degrees, rounded as given).
#[allow(clippy::approx_constant)]
const A4D_ELEVATOR_AILERON_LIMIT_RAD: f64 = 0.5236;

impl FixedWingAirframe {
    pub(super) fn vehicle(self) -> Result<Vehicle> {
        match self {
            FixedWingAirframe::A4d => a4d(),
            FixedWingAirframe::Rascal => rascal(),
        }
    }
}

/// The A-4D Skyhawk, from Nelson, *Flight Stability and Automatic Control*,
/// 2nd ed., p. 405, and Pratt and McGhee, "NPSNET: Flight Simulation Dynamic
/// Modeling Using Quaternions". The geometry is the A-4D's in SI (260 ft^2
/// wing, 27.5 ft span, 10.8 ft chord, 17,578 lb). The thrust range, surface
/// limits, and Oswald efficiency have no stated source.
fn a4d() -> Result<Vehicle> {
    Ok(Vehicle {
        mass_kg: 7973.2467,
        inertia_kgm2: inertia(
            [
                [8090.0, 0.0, 1300.0],
                [0.0, 25900.0, 0.0],
                [1300.0, 0.0, 29200.0],
            ],
            true,
        )?,
        wing_span_m: 8.382,
        wing_area_m2: 24.1548,
        mean_chord_m: 3.29184,
        oswald_efficiency: 0.995,
        min_thrust_n: -36000.0,
        max_thrust_n: 36000.0,
        elevator_limits_rad: [
            -A4D_ELEVATOR_AILERON_LIMIT_RAD,
            A4D_ELEVATOR_AILERON_LIMIT_RAD,
        ],
        aileron_limits_rad: [
            -A4D_ELEVATOR_AILERON_LIMIT_RAD,
            A4D_ELEVATOR_AILERON_LIMIT_RAD,
        ],
        rudder_limits_rad: [-0.2618, 0.2618],
        c_drag_0: 0.03,
        c_drag_alpha: 0.3,
        c_drag_elevator: 0.01,
        c_lift_0: 0.28,
        c_lift_alpha: 3.45,
        c_lift_q: 0.0,
        c_lift_elevator: 0.36,
        c_side_beta: -0.98,
        c_side_rudder: 0.17,
        c_roll_beta: -0.12,
        c_roll_p: -0.26,
        c_roll_r: 0.14,
        c_roll_aileron: 0.08,
        c_roll_rudder: -0.105,
        c_pitch_0: 0.0,
        c_pitch_alpha: -0.38,
        c_pitch_q: -3.6,
        c_pitch_elevator: -0.5,
        c_yaw_beta: 0.25,
        c_yaw_p: 0.022,
        c_yaw_r: -0.35,
        c_yaw_aileron: 0.06,
        c_yaw_rudder: 0.032,
        aero_reference_frd_m: Vec3::zeros(),
        c_pitch_alpha_dot: 0.0,
        c_yaw_0: 0.0,
        lift_table: Vec::new(),
        drag_table: Vec::new(),
        sideslip_drag_table: Vec::new(),
        aileron_power_mach_table: Vec::new(),
        elevator_power_mach_table: Vec::new(),
    })
}

/// Feet and inches to meters, pounds to kilograms, for numbers read from a
/// JSBSim file.
const FT: f64 = 0.3048;
const IN: f64 = 0.0254;
const LB: f64 = 0.45359237;

/// The Rascal 110: ArduPilot's JSBSim Rascal (`Rascal.xml`), as JSBSim flies
/// it (`reference/jsbsim_check.py` is the proof). The file's geometry,
/// mass with its 1.5 lb of fuel and the inertia JSBSim resolves for it,
/// derivatives, and tables, in SI. JSBSim's forms kept here: induced drag
/// 0.04 C_L^2 as an Oswald efficiency; the elevator's drag 0.03 per unit of
/// normalized deflection (elevator / 0.30), 0.1 per radian; the aileron and
/// elevator powers as tables against Mach; the loads at the file's
/// aerodynamic reference point, 1.0 in aft and 3.4 in below the fueled
/// center of mass. Not kept: the sign of the elevator drag (JSBSim's falls
/// with up-elevator, a defect of the file; `signed_elevator_drag` restores
/// it for the comparison). The thrust range (0 to 25 N) has no JSBSim
/// counterpart: JSBSim turns a propeller.
fn rascal() -> Result<Vehicle> {
    let (span_m, chord_m) = (9.17 * FT, 1.15 * FT);
    // The fueled center of mass, from the 13 lb airframe at (36.4, 0, 4) in
    // and the 1.5 lb tank at (36.36, 0, -1.89375) in, in the file's
    // structural axes (aft, right, up); the reference point is at 37.4 in.
    let (cg_aft_in, cg_up_in) = (
        (13.0 * 36.4 + 1.5 * 36.36) / 14.5,
        (13.0 * 4.0 - 1.5 * 1.89375) / 14.5,
    );
    Ok(Vehicle {
        mass_kg: 14.5 * LB,
        // The file's empty inertia moved to the fueled center of mass, plus
        // the tank's, as JSBSim resolves them (read through
        // reference/jsbsim/rascal.py).
        inertia_kgm2: inertia(
            [
                [1.960082815452557, 0.0, -6.843056086570756e-05],
                [0.0, 1.5600832798805415, 0.0],
                [-6.843056086570756e-05, 0.0, 1.9100004644279847],
            ],
            true,
        )?,
        wing_span_m: span_m,
        wing_area_m2: 10.57 * (FT * FT),
        mean_chord_m: chord_m,
        oswald_efficiency: 1.0 / (0.04 * PI * (span_m / chord_m)),
        min_thrust_n: 0.0,
        max_thrust_n: 25.0,
        elevator_limits_rad: [-0.35, 0.30],
        aileron_limits_rad: [-0.35, 0.35],
        rudder_limits_rad: [-0.35, 0.35],
        c_drag_0: 0.028,
        c_drag_alpha: 0.0,
        c_drag_elevator: 0.1,
        c_lift_0: 0.25,
        c_lift_alpha: 5.0,
        c_lift_q: 0.0,
        c_lift_elevator: 0.2,
        c_side_beta: -1.0,
        c_side_rudder: 0.0,
        c_roll_beta: -0.1,
        c_roll_p: -0.4,
        c_roll_r: 0.15,
        c_roll_aileron: 0.13,
        c_roll_rudder: 0.01,
        c_pitch_0: 0.0,
        c_pitch_alpha: -0.5,
        c_pitch_q: -12.0,
        c_pitch_elevator: -0.5,
        c_yaw_beta: 0.12,
        c_yaw_p: 0.0,
        c_yaw_r: -0.15,
        c_yaw_aileron: -0.03,
        c_yaw_rudder: -0.05,
        aero_reference_frd_m: Vec3::new(-(37.4 - cg_aft_in) * IN, 0.0, cg_up_in * IN),
        c_pitch_alpha_dot: -7.0,
        c_yaw_0: 0.0007,
        lift_table: vec![[-0.2, -0.75], [0.0, 0.25], [0.23, 1.4], [0.6, 0.71]],
        drag_table: vec![
            [-1.57, 1.5],
            [-0.26, 0.056],
            [0.0, 0.028],
            [0.26, 0.056],
            [1.57, 1.5],
        ],
        sideslip_drag_table: vec![
            [-1.57, 1.23],
            [-0.26, 0.05],
            [0.0, 0.0],
            [0.26, 0.05],
            [1.57, 1.23],
        ],
        aileron_power_mach_table: vec![[0.0, 0.13], [2.0, 0.057]],
        elevator_power_mach_table: vec![[0.0, -0.5], [2.0, -0.275]],
    })
}

//! FixedWing6DOF vehicles: named, sourced sets of an aircraft's own numbers.
//!
//! A mission either names one with `airframe` (and may override any of its
//! keys) or gives every vehicle number itself. There is no default vehicle.
//! Optional effects (`lift_table`, `drag_table`, `landing_gear`) and the
//! environment (air density, gravity, wind) are not part of a vehicle.

use anyhow::Result;
use nalgebra::Matrix3;
use serde::Deserialize;

use super::inertia;

/// A built-in aircraft.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FixedWingAirframe {
    /// Douglas A-4D Skyhawk, from Nelson, *Flight Stability and Automatic
    /// Control*, 2nd ed., p. 405, with Pratt and McGhee's NPSNET model.
    A4d,
    /// Rascal 110 RC plane, from ArduPilot's JSBSim Rascal.
    Rascal,
}

/// An aircraft's own numbers, SI unless named otherwise.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Vehicle {
    pub mass_kg: f64,
    pub inertia_kgm2: Matrix3<f64>,
    pub span_m: f64,
    pub area_m2: f64,
    pub chord_m: f64,
    /// Oswald efficiency in the induced drag C_L^2 / (pi e AR).
    pub efficiency: f64,
    pub thrust_min_n: f64,
    pub thrust_max_n: f64,
    pub elevator_limits_rad: [f64; 2],
    pub aileron_limits_rad: [f64; 2],
    pub rudder_limits_rad: [f64; 2],
    pub c_d0: f64,
    pub c_d_alpha: f64,
    pub c_d_elevator: f64,
    pub c_l0: f64,
    pub c_l_alpha: f64,
    pub c_lq: f64,
    pub c_l_elevator: f64,
    pub c_y_beta: f64,
    pub c_y_rudder: f64,
    pub c_roll_beta: f64,
    pub c_roll_p: f64,
    pub c_roll_r: f64,
    pub c_roll_aileron: f64,
    pub c_roll_rudder: f64,
    pub c_m0: f64,
    pub c_m_alpha: f64,
    pub c_mq: f64,
    pub c_m_elevator: f64,
    pub c_n_beta: f64,
    pub c_np: f64,
    pub c_nr: f64,
    pub c_n_aileron: f64,
    pub c_n_rudder: f64,
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
        span_m: 8.382,
        area_m2: 24.1548,
        chord_m: 3.29184,
        efficiency: 0.995,
        thrust_min_n: -36000.0,
        thrust_max_n: 36000.0,
        elevator_limits_rad: [
            -A4D_ELEVATOR_AILERON_LIMIT_RAD,
            A4D_ELEVATOR_AILERON_LIMIT_RAD,
        ],
        aileron_limits_rad: [
            -A4D_ELEVATOR_AILERON_LIMIT_RAD,
            A4D_ELEVATOR_AILERON_LIMIT_RAD,
        ],
        rudder_limits_rad: [-0.2618, 0.2618],
        c_d0: 0.03,
        c_d_alpha: 0.3,
        c_d_elevator: 0.01,
        c_l0: 0.28,
        c_l_alpha: 3.45,
        c_lq: 0.0,
        c_l_elevator: 0.36,
        c_y_beta: -0.98,
        c_y_rudder: 0.17,
        c_roll_beta: -0.12,
        c_roll_p: -0.26,
        c_roll_r: 0.14,
        c_roll_aileron: 0.08,
        c_roll_rudder: -0.105,
        c_m0: 0.0,
        c_m_alpha: -0.38,
        c_mq: -3.6,
        c_m_elevator: -0.5,
        c_n_beta: 0.25,
        c_np: 0.022,
        c_nr: -0.35,
        c_n_aileron: 0.06,
        c_n_rudder: 0.032,
    })
}

/// The Rascal 110: ArduPilot's JSBSim Rascal (`Rascal.xml`). Geometry,
/// inertia, and derivatives are JSBSim's, its tables read at zero angle of
/// attack and low Mach (C_D0 0.028, C_L 0.25 rising 5 per rad, aileron 0.13,
/// elevator -0.5). Inherited modeling choices that differ from JSBSim, kept
/// because they are what has been flown:
/// - mass 5.91 kg (JSBSim's 13 lb empty weight is 5.90 kg);
/// - span 2.70 m (JSBSim 9.17 ft, 2.795 m);
/// - C_D_alpha 0 (JSBSim's drag table rises about 0.11 per rad;
///   `drag_table` restores it);
/// - induced drag through efficiency 0.995 (JSBSim states 0.04 C_L^2);
/// - elevator drag 0.01 times |elevator| in radians, where JSBSim's is 0.03
///   times the signed normalized deflection (elevator / 0.30), so the units
///   and the sign treatment differ, not only the number;
/// - elevator limits +-0.30 rad, where JSBSim's elevator moves -0.35 to
///   +0.30; aileron and rudder +-0.35 match JSBSim.
///
/// The thrust range (0 to 25 N) has no JSBSim counterpart.
fn rascal() -> Result<Vehicle> {
    Ok(Vehicle {
        mass_kg: 5.91,
        inertia_kgm2: inertia([[1.95, 0.0, 0.0], [0.0, 1.55, 0.0], [0.0, 0.0, 1.91]], true)?,
        span_m: 2.70,
        area_m2: 0.982,
        chord_m: 0.351,
        efficiency: 0.995,
        thrust_min_n: 0.0,
        thrust_max_n: 25.0,
        elevator_limits_rad: [-0.30, 0.30],
        aileron_limits_rad: [-0.35, 0.35],
        rudder_limits_rad: [-0.35, 0.35],
        c_d0: 0.028,
        c_d_alpha: 0.0,
        c_d_elevator: 0.01,
        c_l0: 0.25,
        c_l_alpha: 5.0,
        c_lq: 0.0,
        c_l_elevator: 0.2,
        c_y_beta: -1.0,
        c_y_rudder: 0.0,
        c_roll_beta: -0.1,
        c_roll_p: -0.4,
        c_roll_r: 0.15,
        c_roll_aileron: 0.13,
        c_roll_rudder: 0.01,
        c_m0: 0.0,
        c_m_alpha: -0.5,
        c_mq: -12.0,
        c_m_elevator: -0.5,
        c_n_beta: 0.12,
        c_np: 0.0,
        c_nr: -0.15,
        c_n_aileron: -0.03,
        c_n_rudder: -0.05,
    })
}

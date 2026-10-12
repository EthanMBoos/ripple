//! FixedWing6DOF's coefficient equations in the forward/right/down model frame.
//! GTRI SCRIMMAGE reference: FixedWing6DOF.cpp (LGPL-3.0-or-later).

use std::f64::consts::PI;

use crate::math::Vec3;

use super::Control;
use super::airframe::Vehicle;

/// Coefficient names deliberately match the aircraft equations and XML keys.
#[derive(Clone)]
pub(super) struct Aerodynamics {
    c_d0: f64,
    c_d_alpha: f64,
    c_d_elevator: f64,
    c_l0: f64,
    c_l_alpha: f64,
    c_lq: f64,
    c_l_elevator: f64,
    c_y_beta: f64,
    c_y_rudder: f64,
    c_roll_beta: f64,
    c_roll_p: f64,
    c_roll_r: f64,
    c_roll_aileron: f64,
    c_roll_rudder: f64,
    c_m0: f64,
    c_m_alpha: f64,
    c_mq: f64,
    c_m_elevator: f64,
    c_n_beta: f64,
    c_np: f64,
    c_nr: f64,
    c_n_aileron: f64,
    c_n_rudder: f64,
    lift_table: Vec<[f64; 2]>,
    drag_table: Vec<[f64; 2]>,
}

pub(super) struct Airframe {
    pub density_kgpm3: f64,
    pub span_m: f64,
    pub area_m2: f64,
    pub chord_m: f64,
    pub efficiency: f64,
}

pub(super) struct Loads {
    pub force_model_n: Vec3,
    pub moment_model_nm: Vec3,
}

impl Aerodynamics {
    pub(super) fn new(vehicle: &Vehicle, lift_table: &[[f64; 2]], drag_table: &[[f64; 2]]) -> Self {
        let params = vehicle;
        Self {
            c_d0: params.c_d0,
            c_d_alpha: params.c_d_alpha,
            c_d_elevator: params.c_d_elevator,
            c_l0: params.c_l0,
            c_l_alpha: params.c_l_alpha,
            c_lq: params.c_lq,
            c_l_elevator: params.c_l_elevator,
            c_y_beta: params.c_y_beta,
            c_y_rudder: params.c_y_rudder,
            c_roll_beta: params.c_roll_beta,
            c_roll_p: params.c_roll_p,
            c_roll_r: params.c_roll_r,
            c_roll_aileron: params.c_roll_aileron,
            c_roll_rudder: params.c_roll_rudder,
            c_m0: params.c_m0,
            c_m_alpha: params.c_m_alpha,
            c_mq: params.c_mq,
            c_m_elevator: params.c_m_elevator,
            c_n_beta: params.c_n_beta,
            c_np: params.c_np,
            c_nr: params.c_nr,
            c_n_aileron: params.c_n_aileron,
            c_n_rudder: params.c_n_rudder,
            lift_table: lift_table.to_vec(),
            drag_table: drag_table.to_vec(),
        }
    }

    pub fn loads(
        &self,
        airframe: &Airframe,
        air_velocity_model_mps: Vec3,
        rates_model_radps: Vec3,
        control: Control,
    ) -> Loads {
        let mut airspeed_mps = air_velocity_model_mps.norm();
        if airspeed_mps.abs() < f64::EPSILON {
            airspeed_mps = 0.00001;
        }
        let alpha_rad = air_velocity_model_mps.z.atan2(air_velocity_model_mps.x);
        let beta_rad = (air_velocity_model_mps.y / airspeed_mps)
            .clamp(-1.0, 1.0)
            .asin();
        let twice_speed_mps = 2.0 * airspeed_mps;
        let span_m = airframe.span_m;
        let chord_m = airframe.chord_m;
        let aspect_ratio = span_m / chord_m;
        let pressure_area_n =
            airframe.density_kgpm3 * airspeed_mps.powi(2) * airframe.area_m2 / 2.0;
        let p_radps = rates_model_radps.x;
        let q_radps = rates_model_radps.y;
        let r_radps = rates_model_radps.z;

        // Linear lift, as in C++, unless the mission gives a lift table, whose
        // peak and fall past the critical angle model a stall (Rust-only).
        // Accepted limitations: both wings stall together, so there is no
        // spin, and drag ignores sideslip. A deep stall or flying far sideways
        // is treated as already a failure; the model is not meant to recover
        // from it faithfully.
        let lift_alpha = if self.lift_table.is_empty() {
            self.c_l0 + self.c_l_alpha * alpha_rad
        } else {
            interpolate_clamped(&self.lift_table, alpha_rad)
        };
        let lift_coefficient = lift_alpha
            + self.c_lq * q_radps * chord_m / twice_speed_mps
            + self.c_l_elevator * control.elevator_rad;
        let lift_n = lift_coefficient * pressure_area_n;
        // Likewise drag, whose table can rise steeply past the stall.
        let drag_alpha = if self.drag_table.is_empty() {
            self.c_d0 + self.c_d_alpha * alpha_rad.abs()
        } else {
            interpolate_clamped(&self.drag_table, alpha_rad)
        };
        let drag_n = (drag_alpha
            + lift_coefficient * lift_coefficient / (PI * aspect_ratio * airframe.efficiency)
            + self.c_d_elevator * control.elevator_rad.abs())
            * pressure_area_n;
        let side_n =
            (self.c_y_beta * beta_rad + self.c_y_rudder * control.rudder_rad) * pressure_area_n;
        let force_model_n = Vec3::new(
            lift_n * alpha_rad.sin() - drag_n * alpha_rad.cos() - side_n * beta_rad.sin(),
            side_n * beta_rad.cos(),
            -lift_n * alpha_rad.cos() - drag_n * alpha_rad.sin(),
        );

        let roll_nm = (self.c_roll_beta * beta_rad
            + self.c_roll_p * p_radps * span_m / twice_speed_mps
            + self.c_roll_r * r_radps * span_m / twice_speed_mps
            + self.c_roll_aileron * control.aileron_rad
            + self.c_roll_rudder * control.rudder_rad)
            * pressure_area_n
            * span_m;
        let pitch_nm = (self.c_m0
            + self.c_m_alpha * alpha_rad
            + self.c_mq * q_radps * chord_m / twice_speed_mps
            + self.c_m_elevator * control.elevator_rad)
            * pressure_area_n
            * chord_m;
        let yaw_nm = (self.c_n_beta * beta_rad
            + self.c_np * p_radps * span_m / twice_speed_mps
            + self.c_nr * r_radps * span_m / twice_speed_mps
            + self.c_n_aileron * control.aileron_rad
            + self.c_n_rudder * control.rudder_rad)
            * pressure_area_n
            * span_m;
        Loads {
            force_model_n,
            moment_model_nm: Vec3::new(roll_nm, pitch_nm, yaw_nm),
        }
    }
}

/// Linear interpolation in a table of `[x, y]` rows sorted by `x`, holding the
/// end values outside it, as a JSBSim `<table>` does.
fn interpolate_clamped(table: &[[f64; 2]], x: f64) -> f64 {
    let first = table[0];
    let last = table[table.len() - 1];
    if x <= first[0] {
        return first[1];
    }
    if x >= last[0] {
        return last[1];
    }
    let upper = table.partition_point(|row| row[0] <= x);
    let [x0, y0] = table[upper - 1];
    let [x1, y1] = table[upper];
    y0 + (y1 - y0) * (x - x0) / (x1 - x0)
}

#[cfg(test)]
mod tests {
    use super::super::airframe::{FixedWingAirframe, Vehicle};
    use super::{Aerodynamics, Airframe, Control, interpolate_clamped};
    use crate::math::Vec3;

    /// The A-4D's coefficients, as a base the tests change.
    fn a4d() -> Vehicle {
        FixedWingAirframe::A4d
            .vehicle()
            .expect("the A-4D profile parses")
    }

    #[test]
    fn lift_table_stalls_past_its_peak_and_matches_linear_lift_below() {
        let mut linear_vehicle = a4d();
        linear_vehicle.c_l0 = 0.25;
        linear_vehicle.c_l_alpha = 5.0;
        let linear = Aerodynamics::new(&linear_vehicle, &[], &[]);
        let lift_table = [[-0.2, -0.75], [0.0, 0.25], [0.23, 1.4], [0.6, 0.71]];
        let table = Aerodynamics::new(&a4d(), &lift_table, &[]);
        let airframe = Airframe {
            density_kgpm3: 1.225,
            span_m: 2.7,
            area_m2: 0.982,
            chord_m: 0.351,
            efficiency: 0.9,
        };
        // Lift is the force against the air's up component, at 15 m/s.
        let lift_n = |aerodynamics: &Aerodynamics, alpha_rad: f64| {
            let air_velocity = Vec3::new(alpha_rad.cos(), 0.0, alpha_rad.sin()) * 15.0;
            let loads =
                aerodynamics.loads(&airframe, air_velocity, Vec3::zeros(), Control::default());
            -loads.force_model_n.z * alpha_rad.cos() + loads.force_model_n.x * alpha_rad.sin()
        };
        assert!((lift_n(&linear, 0.1) - lift_n(&table, 0.1)).abs() < 1e-9);
        assert!(lift_n(&table, 0.4) < lift_n(&table, 0.23));
        assert!(lift_n(&table, 0.4) < 0.7 * lift_n(&linear, 0.4));
    }

    #[test]
    fn drag_table_rises_past_the_stall_where_induced_drag_falls() {
        let lift_table = [[-0.2, -0.75], [0.0, 0.25], [0.23, 1.4], [0.6, 0.71]];
        let mut lift_only_vehicle = a4d();
        lift_only_vehicle.c_d0 = 0.028;
        lift_only_vehicle.c_d_alpha = 0.0;
        let lift_only = Aerodynamics::new(&lift_only_vehicle, &lift_table, &[]);
        let drag_table = [
            [-1.57, 1.5],
            [-0.26, 0.056],
            [0.0, 0.028],
            [0.26, 0.056],
            [1.57, 1.5],
        ];
        let with_drag = Aerodynamics::new(&a4d(), &lift_table, &drag_table);
        let airframe = Airframe {
            density_kgpm3: 1.225,
            span_m: 2.7,
            area_m2: 0.982,
            chord_m: 0.351,
            efficiency: 0.995,
        };
        // Drag is the force along the air's direction, at 15 m/s.
        let drag_n = |aerodynamics: &Aerodynamics, alpha_rad: f64| {
            let air_velocity = Vec3::new(alpha_rad.cos(), 0.0, alpha_rad.sin()) * 15.0;
            let loads =
                aerodynamics.loads(&airframe, air_velocity, Vec3::zeros(), Control::default());
            -loads.force_model_n.x * alpha_rad.cos() - loads.force_model_n.z * alpha_rad.sin()
        };
        assert!((drag_n(&lift_only, 0.0) - drag_n(&with_drag, 0.0)).abs() < 1e-9);
        // Past the stall, lift and with it induced drag fall; the table rises.
        assert!(drag_n(&lift_only, 0.6) < drag_n(&lift_only, 0.23));
        assert!(drag_n(&with_drag, 0.6) > 3.0 * drag_n(&with_drag, 0.23));
    }

    #[test]
    fn lift_table_interpolates_and_holds_its_ends() {
        // JSBSim's Rascal lift table.
        let table = [[-0.2, -0.75], [0.0, 0.25], [0.23, 1.4], [0.6, 0.71]];
        for (alpha_rad, expected) in [
            (-1.0, -0.75),
            (-0.2, -0.75),
            (-0.1, -0.25),
            (0.0, 0.25),
            (0.1, 0.75),
            (0.23, 1.4),
            (0.415, 1.055),
            (0.6, 0.71),
            (1.5, 0.71),
        ] {
            let lift = interpolate_clamped(&table, alpha_rad);
            assert!((lift - expected).abs() < 1e-12, "{alpha_rad}: {lift}");
        }
    }
}

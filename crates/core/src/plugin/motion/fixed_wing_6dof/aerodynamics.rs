//! FixedWing6DOF's coefficient equations in the forward/right/down model frame.
//! GTRI SCRIMMAGE reference: FixedWing6DOF.cpp (LGPL-3.0-or-later).

use std::f64::consts::PI;

use crate::math::Vec3;

use super::Control;
use super::airframe::Vehicle;

/// Coefficient names deliberately match the aircraft equations and mission keys.
#[derive(Clone)]
pub(super) struct Aerodynamics {
    c_drag_0: f64,
    c_drag_alpha: f64,
    c_drag_elevator: f64,
    c_lift_0: f64,
    c_lift_alpha: f64,
    c_lift_q: f64,
    c_lift_elevator: f64,
    c_side_beta: f64,
    c_side_rudder: f64,
    c_roll_beta: f64,
    c_roll_p: f64,
    c_roll_r: f64,
    c_roll_aileron: f64,
    c_roll_rudder: f64,
    c_pitch_0: f64,
    c_pitch_alpha: f64,
    c_pitch_q: f64,
    c_pitch_elevator: f64,
    c_yaw_beta: f64,
    c_yaw_p: f64,
    c_yaw_r: f64,
    c_yaw_aileron: f64,
    c_yaw_rudder: f64,
    lift_table: Vec<[f64; 2]>,
    drag_table: Vec<[f64; 2]>,
}

pub(super) struct Airframe {
    pub air_density_kgpm3: f64,
    pub wing_span_m: f64,
    pub wing_area_m2: f64,
    pub mean_chord_m: f64,
    pub oswald_efficiency: f64,
}

pub(super) struct Loads {
    pub force_model_n: Vec3,
    pub moment_model_nm: Vec3,
}

impl Aerodynamics {
    pub(super) fn new(vehicle: &Vehicle, lift_table: &[[f64; 2]], drag_table: &[[f64; 2]]) -> Self {
        let params = vehicle;
        Self {
            c_drag_0: params.c_drag_0,
            c_drag_alpha: params.c_drag_alpha,
            c_drag_elevator: params.c_drag_elevator,
            c_lift_0: params.c_lift_0,
            c_lift_alpha: params.c_lift_alpha,
            c_lift_q: params.c_lift_q,
            c_lift_elevator: params.c_lift_elevator,
            c_side_beta: params.c_side_beta,
            c_side_rudder: params.c_side_rudder,
            c_roll_beta: params.c_roll_beta,
            c_roll_p: params.c_roll_p,
            c_roll_r: params.c_roll_r,
            c_roll_aileron: params.c_roll_aileron,
            c_roll_rudder: params.c_roll_rudder,
            c_pitch_0: params.c_pitch_0,
            c_pitch_alpha: params.c_pitch_alpha,
            c_pitch_q: params.c_pitch_q,
            c_pitch_elevator: params.c_pitch_elevator,
            c_yaw_beta: params.c_yaw_beta,
            c_yaw_p: params.c_yaw_p,
            c_yaw_r: params.c_yaw_r,
            c_yaw_aileron: params.c_yaw_aileron,
            c_yaw_rudder: params.c_yaw_rudder,
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
        let wing_span_m = airframe.wing_span_m;
        let mean_chord_m = airframe.mean_chord_m;
        let aspect_ratio = wing_span_m / mean_chord_m;
        let pressure_area_n =
            airframe.air_density_kgpm3 * airspeed_mps.powi(2) * airframe.wing_area_m2 / 2.0;
        let p_radps = rates_model_radps.x;
        let q_radps = rates_model_radps.y;
        let r_radps = rates_model_radps.z;

        // Linear lift, unless the mission gives a lift table, whose peak and
        // fall past the critical angle model a stall.
        // Accepted limitations: both wings stall together, so there is no
        // spin, and drag ignores sideslip. A deep stall or flying far sideways
        // is treated as already a failure; the model is not meant to recover
        // from it faithfully.
        let lift_alpha = if self.lift_table.is_empty() {
            self.c_lift_0 + self.c_lift_alpha * alpha_rad
        } else {
            interpolate_clamped(&self.lift_table, alpha_rad)
        };
        let lift_coefficient = lift_alpha
            + self.c_lift_q * q_radps * mean_chord_m / twice_speed_mps
            + self.c_lift_elevator * control.elevator_rad;
        let lift_n = lift_coefficient * pressure_area_n;
        // Likewise drag, whose table can rise steeply past the stall.
        let drag_alpha = if self.drag_table.is_empty() {
            self.c_drag_0 + self.c_drag_alpha * alpha_rad.abs()
        } else {
            interpolate_clamped(&self.drag_table, alpha_rad)
        };
        let drag_n = (drag_alpha
            + lift_coefficient * lift_coefficient
                / (PI * aspect_ratio * airframe.oswald_efficiency)
            + self.c_drag_elevator * control.elevator_rad.abs())
            * pressure_area_n;
        let side_n = (self.c_side_beta * beta_rad + self.c_side_rudder * control.rudder_rad)
            * pressure_area_n;
        let force_model_n = Vec3::new(
            lift_n * alpha_rad.sin() - drag_n * alpha_rad.cos() - side_n * beta_rad.sin(),
            side_n * beta_rad.cos(),
            -lift_n * alpha_rad.cos() - drag_n * alpha_rad.sin(),
        );

        let roll_nm = (self.c_roll_beta * beta_rad
            + self.c_roll_p * p_radps * wing_span_m / twice_speed_mps
            + self.c_roll_r * r_radps * wing_span_m / twice_speed_mps
            + self.c_roll_aileron * control.aileron_rad
            + self.c_roll_rudder * control.rudder_rad)
            * pressure_area_n
            * wing_span_m;
        let pitch_nm = (self.c_pitch_0
            + self.c_pitch_alpha * alpha_rad
            + self.c_pitch_q * q_radps * mean_chord_m / twice_speed_mps
            + self.c_pitch_elevator * control.elevator_rad)
            * pressure_area_n
            * mean_chord_m;
        let yaw_nm = (self.c_yaw_beta * beta_rad
            + self.c_yaw_p * p_radps * wing_span_m / twice_speed_mps
            + self.c_yaw_r * r_radps * wing_span_m / twice_speed_mps
            + self.c_yaw_aileron * control.aileron_rad
            + self.c_yaw_rudder * control.rudder_rad)
            * pressure_area_n
            * wing_span_m;
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
        linear_vehicle.c_lift_0 = 0.25;
        linear_vehicle.c_lift_alpha = 5.0;
        let linear = Aerodynamics::new(&linear_vehicle, &[], &[]);
        let lift_table = [[-0.2, -0.75], [0.0, 0.25], [0.23, 1.4], [0.6, 0.71]];
        let table = Aerodynamics::new(&a4d(), &lift_table, &[]);
        let airframe = Airframe {
            air_density_kgpm3: 1.225,
            wing_span_m: 2.7,
            wing_area_m2: 0.982,
            mean_chord_m: 0.351,
            oswald_efficiency: 0.9,
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
        lift_only_vehicle.c_drag_0 = 0.028;
        lift_only_vehicle.c_drag_alpha = 0.0;
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
            air_density_kgpm3: 1.225,
            wing_span_m: 2.7,
            wing_area_m2: 0.982,
            mean_chord_m: 0.351,
            oswald_efficiency: 0.995,
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

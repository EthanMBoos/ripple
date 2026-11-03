//! FixedWing6DOF's coefficient equations in the forward/right/down body frame.
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
    c_pitch_alpha_dot: f64,
    c_pitch_elevator: f64,
    c_yaw_0: f64,
    c_yaw_beta: f64,
    c_yaw_p: f64,
    c_yaw_r: f64,
    c_yaw_aileron: f64,
    c_yaw_rudder: f64,
    lift_table: Vec<[f64; 2]>,
    drag_table: Vec<[f64; 2]>,
    sideslip_drag_table: Vec<[f64; 2]>,
    aileron_power_mach_table: Vec<[f64; 2]>,
    elevator_power_mach_table: Vec<[f64; 2]>,
    aero_reference_frd_m: Vec3,
    signed_elevator_drag: bool,
    air_density_kgpm3: f64,
    speed_of_sound_mps: f64,
    wing_span_m: f64,
    wing_area_m2: f64,
    mean_chord_m: f64,
    oswald_efficiency: f64,
}

#[cfg(test)]
pub(super) struct Loads {
    pub force_frd_n: Vec3,
    pub moment_frd_nm: Vec3,
}

impl Aerodynamics {
    pub(super) fn new(
        vehicle: &Vehicle,
        air_density_kgpm3: f64,
        speed_of_sound_mps: f64,
        signed_elevator_drag: bool,
    ) -> Self {
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
            c_pitch_alpha_dot: params.c_pitch_alpha_dot,
            c_pitch_elevator: params.c_pitch_elevator,
            c_yaw_0: params.c_yaw_0,
            c_yaw_beta: params.c_yaw_beta,
            c_yaw_p: params.c_yaw_p,
            c_yaw_r: params.c_yaw_r,
            c_yaw_aileron: params.c_yaw_aileron,
            c_yaw_rudder: params.c_yaw_rudder,
            lift_table: params.lift_table.clone(),
            drag_table: params.drag_table.clone(),
            sideslip_drag_table: params.sideslip_drag_table.clone(),
            aileron_power_mach_table: params.aileron_power_mach_table.clone(),
            elevator_power_mach_table: params.elevator_power_mach_table.clone(),
            aero_reference_frd_m: params.aero_reference_frd_m,
            signed_elevator_drag,
            air_density_kgpm3,
            speed_of_sound_mps,
            wing_span_m: vehicle.wing_span_m,
            wing_area_m2: vehicle.wing_area_m2,
            mean_chord_m: vehicle.mean_chord_m,
            oswald_efficiency: vehicle.oswald_efficiency,
        }
    }

    /// The force and the moment about the center of mass, for a given rate of
    /// change of angle of attack (the equations of motion compute it from
    /// the force first; see `alpha_dot_radps`).
    #[cfg(test)]
    pub fn loads(
        &self,
        air_velocity_frd_mps: Vec3,
        rates_frd_radps: Vec3,
        control: Control,
        alpha_dot_radps: f64,
    ) -> Loads {
        let force_frd_n = self.force(air_velocity_frd_mps, rates_frd_radps, control);
        let moment_frd_nm = self.moment(
            air_velocity_frd_mps,
            rates_frd_radps,
            control,
            alpha_dot_radps,
            force_frd_n,
        );
        Loads {
            force_frd_n,
            moment_frd_nm,
        }
    }

    /// The aerodynamic force in the body axes.
    pub fn force(
        &self,
        air_velocity_frd_mps: Vec3,
        rates_frd_radps: Vec3,
        control: Control,
    ) -> Vec3 {
        let Flow {
            v,
            alpha,
            beta,
            qbar_s,
        } = self.flow(air_velocity_frd_mps);
        let q = rates_frd_radps.y;
        let (delta_e, delta_r) = (control.elevator_rad, control.rudder_rad);
        let (b, c) = (self.wing_span_m, self.mean_chord_m);
        let aspect_ratio = b / c;
        let two_v = 2.0 * v;

        // Linear lift, unless the vehicle has a lift table, whose peak and
        // fall past the critical angle model a stall.
        // Accepted limitations: both wings stall together, so there is no
        // spin. A deep stall or flying far sideways is treated as already a
        // failure; the model is not meant to recover from it faithfully.
        let c_lift_of_alpha = if self.lift_table.is_empty() {
            self.c_lift_0 + self.c_lift_alpha * alpha
        } else {
            interpolate_clamped(&self.lift_table, alpha)
        };
        let c_lift =
            c_lift_of_alpha + self.c_lift_q * q * c / two_v + self.c_lift_elevator * delta_e;
        let lift_n = c_lift * qbar_s;
        // Likewise drag, whose table can rise steeply past the stall; drag
        // from sideslip is its own table. The elevator's drag is from the
        // deflection's size, or, in JSBSim's Rascal's form, from the signed
        // deflection (so up-elevator reduces it).
        let c_drag_of_alpha = if self.drag_table.is_empty() {
            self.c_drag_0 + self.c_drag_alpha * alpha.abs()
        } else {
            interpolate_clamped(&self.drag_table, alpha)
        };
        let c_drag_of_beta = if self.sideslip_drag_table.is_empty() {
            0.0
        } else {
            interpolate_clamped(&self.sideslip_drag_table, beta)
        };
        let elevator_drag = if self.signed_elevator_drag {
            self.c_drag_elevator * delta_e
        } else {
            self.c_drag_elevator * delta_e.abs()
        };
        let drag_n = (c_drag_of_alpha
            + c_lift * c_lift / (PI * aspect_ratio * self.oswald_efficiency)
            + elevator_drag
            + c_drag_of_beta)
            * qbar_s;
        let side_n = (self.c_side_beta * beta + self.c_side_rudder * delta_r) * qbar_s;
        // Wind axes (-drag, side, -lift) to the forward/right/down body
        // axes: the rotation by sideslip about the down axis, then by angle
        // of attack about the right axis.
        let (ca, sa, cb, sb) = (alpha.cos(), alpha.sin(), beta.cos(), beta.sin());
        Vec3::new(
            -drag_n * ca * cb - side_n * ca * sb + lift_n * sa,
            -drag_n * sb + side_n * cb,
            -drag_n * sa * cb - side_n * sa * sb - lift_n * ca,
        )
    }

    /// The aerodynamic moment about the center of mass, given the force.
    pub fn moment(
        &self,
        air_velocity_frd_mps: Vec3,
        rates_frd_radps: Vec3,
        control: Control,
        alpha_dot_radps: f64,
        force_frd_n: Vec3,
    ) -> Vec3 {
        let Flow {
            v,
            alpha,
            beta,
            qbar_s,
        } = self.flow(air_velocity_frd_mps);
        let (p, q, r) = (rates_frd_radps.x, rates_frd_radps.y, rates_frd_radps.z);
        let (delta_e, delta_a, delta_r) = (
            control.elevator_rad,
            control.aileron_rad,
            control.rudder_rad,
        );
        let (b, c) = (self.wing_span_m, self.mean_chord_m);
        let two_v = 2.0 * v;
        // Control powers constant, or read from a table against Mach.
        let mach = v / self.speed_of_sound_mps;
        let c_roll_aileron = if self.aileron_power_mach_table.is_empty() {
            self.c_roll_aileron
        } else {
            interpolate_clamped(&self.aileron_power_mach_table, mach)
        };
        let c_pitch_elevator = if self.elevator_power_mach_table.is_empty() {
            self.c_pitch_elevator
        } else {
            interpolate_clamped(&self.elevator_power_mach_table, mach)
        };

        let roll_nm = (self.c_roll_beta * beta
            + self.c_roll_p * p * b / two_v
            + self.c_roll_r * r * b / two_v
            + c_roll_aileron * delta_a
            + self.c_roll_rudder * delta_r)
            * qbar_s
            * b;
        let pitch_nm = (self.c_pitch_0
            + self.c_pitch_alpha * alpha
            + self.c_pitch_q * q * c / two_v
            + self.c_pitch_alpha_dot * alpha_dot_radps * c / two_v
            + c_pitch_elevator * delta_e)
            * qbar_s
            * c;
        let yaw_nm = (self.c_yaw_0
            + self.c_yaw_beta * beta
            + self.c_yaw_p * p * b / two_v
            + self.c_yaw_r * r * b / two_v
            + self.c_yaw_aileron * delta_a
            + self.c_yaw_rudder * delta_r)
            * qbar_s
            * b;
        // The coefficients are about the aerodynamic reference point; the
        // moment about the center of mass adds that point's arm crossed with
        // the force (a zero arm, the usual case, adds nothing).
        Vec3::new(roll_nm, pitch_nm, yaw_nm) + self.aero_reference_frd_m.cross(&force_frd_n)
    }

    /// Airspeed, angle of attack, sideslip, and dynamic pressure times wing
    /// area (q-bar S), as in the coefficient equations.
    fn flow(&self, air_velocity_frd_mps: Vec3) -> Flow {
        let mut airspeed_mps = air_velocity_frd_mps.norm();
        if airspeed_mps.abs() < f64::EPSILON {
            airspeed_mps = 0.00001;
        }
        let v = airspeed_mps;
        Flow {
            v,
            alpha: air_velocity_frd_mps.z.atan2(air_velocity_frd_mps.x),
            beta: (air_velocity_frd_mps.y / v).clamp(-1.0, 1.0).asin(),
            qbar_s: self.air_density_kgpm3 * v.powi(2) * self.wing_area_m2 / 2.0,
        }
    }
}

struct Flow {
    v: f64,
    alpha: f64,
    beta: f64,
    qbar_s: f64,
}

/// The rate of change of angle of attack, from the air velocity (u, w) and the
/// body's acceleration (u_dot, w_dot) in the forward and down axes:
/// alpha_dot = (u w_dot - w u_dot) / (u^2 + w^2), as JSBSim computes it. Zero
/// when the air velocity in that plane is too small to define an angle.
pub(super) fn alpha_dot_radps(air_velocity_frd_mps: Vec3, acceleration_frd_mps2: Vec3) -> f64 {
    let (u, w) = (air_velocity_frd_mps.x, air_velocity_frd_mps.z);
    let (u_dot, w_dot) = (acceleration_frd_mps2.x, acceleration_frd_mps2.z);
    let uw_squared = u * u + w * w;
    if uw_squared < 1e-6 {
        0.0
    } else {
        (u * w_dot - w * u_dot) / uw_squared
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
    use std::collections::BTreeMap;

    use serde_json::{Value, json};

    use super::super::FixedWing6Dof;
    use super::super::airframe::{FixedWingAirframe, Vehicle};
    use super::{Aerodynamics, Control, interpolate_clamped};
    use crate::math::Vec3;
    use crate::plugin::{MotionModel, PluginParams};

    /// In sea-level air, with the elevator drag from the deflection's size.
    fn aerodynamics(vehicle: &Vehicle) -> Aerodynamics {
        Aerodynamics::new(vehicle, 1.225, 340.294, false)
    }

    /// The A-4D's coefficients on the Rascal's wing, as a base the tests
    /// change.
    fn a4d(oswald_efficiency: f64) -> Vehicle {
        let mut vehicle = FixedWingAirframe::A4d
            .vehicle()
            .expect("the A-4D profile parses");
        vehicle.wing_span_m = 2.7;
        vehicle.wing_area_m2 = 0.982;
        vehicle.mean_chord_m = 0.351;
        vehicle.oswald_efficiency = oswald_efficiency;
        vehicle
    }

    #[test]
    fn lift_table_stalls_past_its_peak_and_matches_linear_lift_below() {
        let mut linear_vehicle = a4d(0.9);
        linear_vehicle.c_lift_0 = 0.25;
        linear_vehicle.c_lift_alpha = 5.0;
        let linear = aerodynamics(&linear_vehicle);
        let mut table_vehicle = a4d(0.9);
        table_vehicle.lift_table = vec![[-0.2, -0.75], [0.0, 0.25], [0.23, 1.4], [0.6, 0.71]];
        let table = aerodynamics(&table_vehicle);
        // Lift is the force against the air's up component, at 15 m/s.
        let lift_n = |aerodynamics: &Aerodynamics, alpha_rad: f64| {
            let air_velocity = Vec3::new(alpha_rad.cos(), 0.0, alpha_rad.sin()) * 15.0;
            let loads = aerodynamics.loads(air_velocity, Vec3::zeros(), Control::default(), 0.0);
            -loads.force_frd_n.z * alpha_rad.cos() + loads.force_frd_n.x * alpha_rad.sin()
        };
        assert!((lift_n(&linear, 0.1) - lift_n(&table, 0.1)).abs() < 1e-9);
        assert!(lift_n(&table, 0.4) < lift_n(&table, 0.23));
        assert!(lift_n(&table, 0.4) < 0.7 * lift_n(&linear, 0.4));
    }

    #[test]
    fn drag_table_rises_past_the_stall_where_induced_drag_falls() {
        let lift_table = vec![[-0.2, -0.75], [0.0, 0.25], [0.23, 1.4], [0.6, 0.71]];
        let mut lift_only_vehicle = a4d(0.995);
        lift_only_vehicle.c_drag_0 = 0.028;
        lift_only_vehicle.c_drag_alpha = 0.0;
        lift_only_vehicle.lift_table = lift_table.clone();
        let lift_only = aerodynamics(&lift_only_vehicle);
        let mut with_drag_vehicle = a4d(0.995);
        with_drag_vehicle.lift_table = lift_table;
        with_drag_vehicle.drag_table = vec![
            [-1.57, 1.5],
            [-0.26, 0.056],
            [0.0, 0.028],
            [0.26, 0.056],
            [1.57, 1.5],
        ];
        let with_drag = aerodynamics(&with_drag_vehicle);
        // Drag is the force along the air's direction, at 15 m/s.
        let drag_n = |aerodynamics: &Aerodynamics, alpha_rad: f64| {
            let air_velocity = Vec3::new(alpha_rad.cos(), 0.0, alpha_rad.sin()) * 15.0;
            let loads = aerodynamics.loads(air_velocity, Vec3::zeros(), Control::default(), 0.0);
            -loads.force_frd_n.x * alpha_rad.cos() - loads.force_frd_n.z * alpha_rad.sin()
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

    fn vector(value: &Value) -> Vec3 {
        Vec3::new(
            value[0].as_f64().unwrap(),
            value[1].as_f64().unwrap(),
            value[2].as_f64().unwrap(),
        )
    }

    /// Every sample of ArduPilot's JSBSim Rascal that reference/jsbsim_check.py
    /// read at a grid of states, reproduced from the same inputs by the
    /// `rascal` preset in JSBSim's elevator-drag form (the fixture's mission
    /// keys). Also the residual of the preset as flown, whose elevator drag
    /// is from the deflection's size. Prints both for that script, with the
    /// size of each term beyond the textbook equations.
    #[test]
    fn jsbsim_rascal_samples() {
        // Rounding in unit conversions and in the terms' arithmetic, with headroom.
        const LIMIT: f64 = 1e-12;
        let text = match std::env::var("RIPPLE_JSBSIM_SAMPLES") {
            Ok(path) => std::fs::read_to_string(path).expect("samples file"),
            Err(_) => include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/jsbsim_rascal_samples.json"
            ))
            .to_string(),
        };
        let fixture: Value = serde_json::from_str(&text).expect("samples JSON");
        let keys = fixture["mission_keys"].as_object().unwrap();
        let configure = |keys: &Value| {
            FixedWing6Dof::configure(&PluginParams::yaml(&keys.to_string()))
                .expect("the keys parse")
        };
        let jsbsim_form = configure(&Value::Object(keys.clone()));
        let mut as_flown = keys.clone();
        as_flown.remove("signed_elevator_drag");
        let preset = configure(&Value::Object(as_flown));
        let wing_area_m2 = 10.57 * 0.3048_f64.powi(2);
        let wing_span_m = 9.17 * 0.3048;

        let mut largest: BTreeMap<&str, f64> = BTreeMap::new();
        let mut largest_term: BTreeMap<String, f64> = BTreeMap::new();
        let mut count = 0;
        for sample in fixture["samples"].as_array().unwrap() {
            let air_velocity = vector(&sample["air_velocity_frd_mps"]);
            let rates = vector(&sample["rates_frd_radps"]);
            let alpha_dot_radps = sample["alpha_dot_radps"].as_f64().unwrap();
            let control = Control {
                throttle: 0.0,
                elevator_rad: sample["elevator_rad"].as_f64().unwrap(),
                aileron_rad: sample["aileron_rad"].as_f64().unwrap(),
                rudder_rad: sample["rudder_rad"].as_f64().unwrap(),
            };
            // Errors relative to q S for forces and q S b for moments.
            let qbar_s = sample["dynamic_pressure_pa"].as_f64().unwrap() * wing_area_m2;
            let expected_force = vector(&sample["force_frd_n"]);
            let expected_moment = vector(&sample["moment_cg_frd_nm"]);
            for (name, term) in sample["terms"]["force_frd_n"].as_object().unwrap() {
                let size = largest_term.entry(name.clone()).or_default();
                *size = size.max(vector(term).norm() / qbar_s);
            }
            for (name, term) in sample["terms"]["moment_frd_nm"].as_object().unwrap() {
                let size = largest_term.entry(name.clone()).or_default();
                *size = size.max(vector(term).norm() / (qbar_s * wing_span_m));
            }
            let errors = |config: &super::super::FixedWingConfig| {
                let loads =
                    config
                        .aerodynamics
                        .loads(air_velocity, rates, control, alpha_dot_radps);
                (
                    (loads.force_frd_n - expected_force).norm() / qbar_s,
                    (loads.moment_frd_nm - expected_moment).norm() / (qbar_s * wing_span_m),
                )
            };
            let (force_error, moment_error) = errors(&jsbsim_form);
            assert!(
                force_error <= LIMIT,
                "sample {count}: force error {force_error:e} of q S"
            );
            assert!(
                moment_error <= LIMIT,
                "sample {count}: moment error {moment_error:e} of q S b"
            );
            let (preset_force, preset_moment) = errors(&preset);
            for (name, error) in [
                ("force", force_error),
                ("moment", moment_error),
                ("preset_force", preset_force),
                ("preset_moment", preset_moment),
            ] {
                let size = largest.entry(name).or_default();
                *size = size.max(error);
            }
            count += 1;
        }
        assert!(count > 0);
        println!(
            "JSBSIM_ERRORS {}",
            json!({
                "samples": count,
                "limit": LIMIT,
                "jsbsim_form": {"force": largest["force"], "moment": largest["moment"]},
                "preset": {"force": largest["preset_force"], "moment": largest["preset_moment"]},
                "terms": largest_term,
            })
        );
    }
}

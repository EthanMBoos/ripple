//! Beard and McLain's Aerosonde: a six-degree-of-freedom aircraft from
//! *Small Unmanned Aircraft* chapter 4, with the addendum's motor and
//! propeller, RK4 propagated.
//! Motion phase: read throttle and surface angles, update entity truth.
//! Private frames are the aerospace ones, forward/right/down body and
//! north/east/down world (`motion/aircraft_state.rs`); public truth is the
//! engine's ENU and forward/left/up.
//!
//! Fidelity
//! - Source: Beard and McLain, *Small Unmanned Aircraft: Theory and
//!   Practice* (Princeton, 2012), chapter 4, "Forces and Moments": gravity,
//!   the longitudinal lift, drag, and pitching moment with the sigmoid
//!   stall blend into a flat plate and the parabolic drag, the lateral side
//!   force and the roll and yaw moments, each with its rates and surfaces,
//!   as written (`aerodynamics.rs`); the rotational equations of chapter 3
//!   with the product of inertia, as the inertia matrix rather than the
//!   book's Gamma coefficients, which are that matrix inverted by hand; and
//!   the authors' addendum on propeller and motor models for the thrust and
//!   torque, in place of the printed chapter's simple propeller
//!   (`propeller.rs`). Neither document is in the repository, so the
//!   citations are by section. The state is the quaternion the authors'
//!   code uses, where the printed book integrates Euler angles. The numbers
//!   are those of `parameters/aerosonde_parameters.py` in `mavsim_public`
//!   at commit `10363e0e` (`airframe.rs`), the book's educational Aerosonde
//!   and not the aircraft: 11 kg starting at 25 m/s, where the authors'
//!   README at that commit says the real Aerosonde weighs about 25 kg and
//!   cruises near 35 m/s.
//! - Validated against: a second, independent transcription of the book's
//!   equations, in Python in `reference/aerosonde_check.py`, which the Rust
//!   test `book_samples` agrees with to rounding at held states (loads,
//!   thrust, torque, propeller speed, and the derivative's accelerations at
//!   tilted attitudes), and the longitudinal trim at 25 m/s that script
//!   solves, flown open loop in `missions/aerosonde.yaml` with the small
//!   side force its ailerons and rudder leave. That repository stubs the
//!   force and motor functions, so there is no second implementation to
//!   run; two transcriptions agreeing shows neither misread the other, not
//!   that either matches the book. Not against another implementation or
//!   flight data.
//! - Valid range: the book's: attached flow below the stall blend, where
//!   the linear derivatives hold, and the flat plate's rough lift past it;
//!   low subsonic speed; near the 25 m/s the file starts at. The thrust
//!   and torque polynomials are fits over the propeller's working range of
//!   advance ratio: a cut motor at cruise extrapolates them to a windmilling
//!   drag of about 23 N, far beyond a stopped propeller's.
//! - Not modeled: the book's Dryden gusts (section 4.4), a named zero in
//!   the air velocity. Also: propeller slipstream and gyroscopic moments,
//!   ground effect, a ground (GroundCollision or TerrainCollision removes an
//!   aircraft that reaches it), motor and propeller dynamics (the propeller
//!   is at its steady speed at once), fuel burn, an altitude-dependent
//!   atmosphere, and surface and throttle rate limits. The surfaces have no
//!   deflection limits: the book gives none; the throttle is 0 to 1, the
//!   range the motor's voltage is defined on.
//! - To go further: the book's Dryden gusts, as keyed random draws, would
//!   fill the named zero; measured thrust and torque of the actual motor
//!   and propeller would replace the addendum's fitted polynomials; a
//!   standard atmosphere would let long climbs change the air density.

mod aerodynamics;
mod airframe;
mod propeller;

use anyhow::{Context, Result, bail, ensure};
use nalgebra::Matrix3;
use serde::Deserialize;

use crate::math::{self, Quaternion, Vec3};
use crate::plugin::{Frame, MotionContext, MotionModel, PluginIo, Port, Ports, Unit};

use super::aircraft_state::{AircraftRate, AircraftState, ned_from_enu};
pub(crate) use airframe::AerosondeAirframe;
use airframe::Vehicle;

/// Mission parameters in SI units. `airframe` names a built-in aircraft
/// whose numbers the vehicle keys below override; without it the mission
/// gives every vehicle key (see `airframe.rs`). The environment keys have
/// defaults.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct AerosondeParams {
    /// `aerosonde`, or absent for an aircraft given in full.
    airframe: Option<AerosondeAirframe>,
    mass_kg: Option<f64>,
    /// Three rows, kg m^2, with the product of inertia entered as the
    /// book's matrix has it (-J_xz off the diagonal).
    inertia_kgm2: Option<[[f64; 3]; 3]>,
    gravity_mps2: f64,
    air_density_kgpm3: f64,
    /// Steady wind: the air's velocity over the ground, `[east, north, up]`.
    wind_mps: [f64; 3],
    wing_span_m: Option<f64>,
    wing_area_m2: Option<f64>,
    mean_chord_m: Option<f64>,
    oswald_efficiency: Option<f64>,
    c_lift_0: Option<f64>,
    c_lift_alpha: Option<f64>,
    c_lift_q: Option<f64>,
    c_lift_elevator: Option<f64>,
    c_drag_p: Option<f64>,
    c_drag_q: Option<f64>,
    c_drag_elevator: Option<f64>,
    c_pitch_0: Option<f64>,
    c_pitch_alpha: Option<f64>,
    c_pitch_q: Option<f64>,
    c_pitch_elevator: Option<f64>,
    /// The stall blend's rate M and center alpha_0 (radians).
    stall_blend_rate: Option<f64>,
    stall_alpha_rad: Option<f64>,
    c_side_0: Option<f64>,
    c_side_beta: Option<f64>,
    c_side_p: Option<f64>,
    c_side_r: Option<f64>,
    c_side_aileron: Option<f64>,
    c_side_rudder: Option<f64>,
    c_roll_0: Option<f64>,
    c_roll_beta: Option<f64>,
    c_roll_p: Option<f64>,
    c_roll_r: Option<f64>,
    c_roll_aileron: Option<f64>,
    c_roll_rudder: Option<f64>,
    c_yaw_0: Option<f64>,
    c_yaw_beta: Option<f64>,
    c_yaw_p: Option<f64>,
    c_yaw_r: Option<f64>,
    c_yaw_aileron: Option<f64>,
    c_yaw_rudder: Option<f64>,
    propeller_diameter_m: Option<f64>,
    max_voltage_v: Option<f64>,
    motor_speed_constant_vspr: Option<f64>,
    motor_torque_constant_nmpa: Option<f64>,
    motor_resistance_ohm: Option<f64>,
    no_load_current_a: Option<f64>,
    c_thrust_2: Option<f64>,
    c_thrust_1: Option<f64>,
    c_thrust_0: Option<f64>,
    c_torque_2: Option<f64>,
    c_torque_1: Option<f64>,
    c_torque_0: Option<f64>,
}

impl Default for AerosondeParams {
    fn default() -> Self {
        Self {
            airframe: None,
            mass_kg: None,
            inertia_kgm2: None,
            gravity_mps2: 9.81,
            air_density_kgpm3: 1.225,
            wind_mps: [0.0; 3],
            wing_span_m: None,
            wing_area_m2: None,
            mean_chord_m: None,
            oswald_efficiency: None,
            c_lift_0: None,
            c_lift_alpha: None,
            c_lift_q: None,
            c_lift_elevator: None,
            c_drag_p: None,
            c_drag_q: None,
            c_drag_elevator: None,
            c_pitch_0: None,
            c_pitch_alpha: None,
            c_pitch_q: None,
            c_pitch_elevator: None,
            stall_blend_rate: None,
            stall_alpha_rad: None,
            c_side_0: None,
            c_side_beta: None,
            c_side_p: None,
            c_side_r: None,
            c_side_aileron: None,
            c_side_rudder: None,
            c_roll_0: None,
            c_roll_beta: None,
            c_roll_p: None,
            c_roll_r: None,
            c_roll_aileron: None,
            c_roll_rudder: None,
            c_yaw_0: None,
            c_yaw_beta: None,
            c_yaw_p: None,
            c_yaw_r: None,
            c_yaw_aileron: None,
            c_yaw_rudder: None,
            propeller_diameter_m: None,
            max_voltage_v: None,
            motor_speed_constant_vspr: None,
            motor_torque_constant_nmpa: None,
            motor_resistance_ohm: None,
            no_load_current_a: None,
            c_thrust_2: None,
            c_thrust_1: None,
            c_thrust_0: None,
            c_torque_2: None,
            c_torque_1: None,
            c_torque_0: None,
        }
    }
}

#[derive(Clone, Deserialize)]
#[serde(try_from = "AerosondeParams")]
pub struct AerosondeConfig {
    vehicle: Vehicle,
    inverse_inertia: Matrix3<f64>,
    gravity_mps2: f64,
    air_density_kgpm3: f64,
    wind_world_mps: Vec3,
}

#[derive(Clone, Copy, Default)]
struct Control {
    throttle: f64,
    elevator_rad: f64,
    aileron_rad: f64,
    rudder_rad: f64,
}

pub struct Aerosonde {
    config: AerosondeConfig,
}

// The mission's parameters become the config through serde's `try_from`, so
// the trait's default `configure` (`params.parse()`) fits this plugin too.
impl TryFrom<AerosondeParams> for AerosondeConfig {
    type Error = anyhow::Error;
    fn try_from(params: AerosondeParams) -> Result<Self> {
        let vehicle = vehicle(&params)?;
        ensure!(
            vehicle.mass_kg > 0.0
                && vehicle.wing_span_m > 0.0
                && vehicle.wing_area_m2 > 0.0
                && vehicle.mean_chord_m > 0.0
                && vehicle.oswald_efficiency > 0.0
                && vehicle.propeller_diameter_m > 0.0
                && vehicle.motor_resistance_ohm > 0.0
                && vehicle.c_torque_0 > 0.0,
            "Aerosonde requires positive mass, geometry, propeller diameter, motor resistance, \
             and c_torque_0"
        );
        // The propeller's speed divides by a coefficient proportional to the
        // air density, so a vacuum is an error here, not a nonfinite state.
        ensure!(
            params.air_density_kgpm3 > 0.0 && params.gravity_mps2 >= 0.0,
            "Aerosonde requires positive air density and nonnegative gravity"
        );
        let inverse_inertia = vehicle
            .inertia_kgm2
            .try_inverse()
            .context("Aerosonde inertia must be invertible")?;
        Ok(AerosondeConfig {
            vehicle,
            inverse_inertia,
            gravity_mps2: params.gravity_mps2,
            air_density_kgpm3: params.air_density_kgpm3,
            wind_world_mps: Vec3::from(params.wind_mps),
        })
    }
}

impl MotionModel for Aerosonde {
    type Config = AerosondeConfig;

    fn new(config: &AerosondeConfig) -> Self {
        Self {
            config: config.clone(),
        }
    }

    fn ports(_config: &AerosondeConfig) -> Ports {
        Ports::default()
            .input(Port::new("throttle", Unit::Dimensionless, Frame::None))
            .input(Port::new("elevator", Unit::Radians, Frame::Model))
            .input(Port::new("aileron", Unit::Radians, Frame::Model))
            .input(Port::new("rudder", Unit::Radians, Frame::Model))
    }

    fn step(&mut self, context: &mut MotionContext, io: &mut PluginIo) -> Result<()> {
        let throttle: f64 = io.read("throttle")?;
        let control = Control {
            // The motor's voltage is defined for a throttle of 0 to 1.
            throttle: throttle.clamp(0.0, 1.0),
            elevator_rad: io.read("elevator")?,
            aileron_rad: io.read("aileron")?,
            rudder_rad: io.read("rudder")?,
        };
        let config = &self.config;
        // Forces and moments are recomputed from each RK4 stage's state.
        let state = math::rk4(
            AircraftState::from_truth(context.truth),
            context.time.dt_s,
            |stage| config.derivative(&stage, control),
        );
        ensure!(
            state.is_finite(),
            "Aerosonde integration produced nonfinite state"
        );
        state.write_truth(context.truth);
        Ok(())
    }
}

impl AerosondeConfig {
    /// The equations of motion: the rate of each state field for one stage.
    fn derivative(&self, state: &AircraftState, control: Control) -> AircraftRate {
        // Symbols, as in the book: the body's velocity and rates, the
        // attitude quaternion, mass, gravity, the inertia matrix, and the
        // air density.
        let (v, omega, q) = (
            state.velocity_frd_mps,
            state.rates_frd_radps,
            state.attitude_ned_from_frd,
        );
        let (m, g, inertia, rho) = (
            self.vehicle.mass_kg,
            self.gravity_mps2,
            self.vehicle.inertia_kgm2,
            self.air_density_kgpm3,
        );
        let rotation = q.normalized();
        // The air velocity: the ground velocity less the wind's, steady plus
        // gusts. The book's Dryden gusts are not modeled; they would be
        // keyed random draws filtered by the book's transfer functions.
        let wind_frd_mps = rotation.rotate_world_to_body(ned_from_enu(self.wind_world_mps));
        let gust_frd_mps = Vec3::zeros();
        let air_velocity_frd_mps = v - wind_frd_mps - gust_frd_mps;
        // Weight m g, down in NED, in the body axes.
        let weight_n = rotation.rotate_world_to_body(Vec3::new(0.0, 0.0, m * g));
        let aero_force_n =
            aerodynamics::force(&self.vehicle, rho, air_velocity_frd_mps, omega, control);
        let propeller = propeller::loads(
            &self.vehicle,
            rho,
            air_velocity_frd_mps.norm(),
            control.throttle,
        );
        // The propeller pulls along the nose; the airframe feels the
        // reaction to the torque that spins it, the opposite way about the
        // nose. Not modeled, zeros in the sums: the propeller's gyroscopic
        // moment (it needs the propeller's inertia) and its slipstream over
        // the wing and tail (it needs the wake's induced velocity).
        let thrust_n = Vec3::new(propeller.thrust_n, 0.0, 0.0);
        let propeller_reaction_nm = Vec3::new(-propeller.torque_nm, 0.0, 0.0);
        let gyroscopic_nm = Vec3::zeros();
        let slipstream_n = Vec3::zeros();
        let force_n = weight_n + thrust_n + aero_force_n + slipstream_n;
        let moment_nm =
            aerodynamics::moment(&self.vehicle, rho, air_velocity_frd_mps, omega, control)
                + propeller_reaction_nm
                + gyroscopic_nm;
        // Newton: a = F / m. Euler: I omega_dot = M - omega x (I omega).
        let acceleration = force_n / m;
        let angular_acceleration =
            self.inverse_inertia * (moment_nm - omega.cross(&(inertia * omega)));
        // q_dot = 1/2 q (0, omega)
        let q_dot = Quaternion {
            w: -0.5 * (q.x * omega.x + q.y * omega.y + q.z * omega.z),
            x: 0.5 * (q.w * omega.x + q.y * omega.z - q.z * omega.y),
            y: 0.5 * (q.w * omega.y + q.z * omega.x - q.x * omega.z),
            z: 0.5 * (q.w * omega.z + q.x * omega.y - q.y * omega.x),
        };
        AircraftRate {
            acceleration_frd_mps2: v.cross(&omega) + acceleration,
            angular_acceleration_frd_radps2: angular_acceleration,
            acceleration_ned_mps2: rotation.rotate_body_to_world(acceleration),
            velocity_ned_mps: rotation.rotate_body_to_world(v),
            attitude_rate: q_dot,
        }
    }
}

/// The vehicle: the named airframe's numbers, each overridden by the
/// mission's key when given, or every number from the mission.
fn vehicle(params: &AerosondeParams) -> Result<Vehicle> {
    let preset = params
        .airframe
        .map(AerosondeAirframe::vehicle)
        .transpose()?;
    let preset = preset.as_ref();
    let pick = |key: &str, value: Option<f64>, preset_value: Option<f64>| {
        value.or(preset_value).with_context(|| {
            format!(
                "Aerosonde: without `airframe`, the mission must give every vehicle number; \
                 `{key}` is missing"
            )
        })
    };
    let inertia_kgm2 = match params.inertia_kgm2 {
        Some(rows) => inertia(rows)?,
        None => preset
            .map(|vehicle| vehicle.inertia_kgm2)
            .context("Aerosonde: without `airframe`, the mission must give an inertia matrix")?,
    };
    macro_rules! field {
        ($name:ident) => {
            pick(stringify!($name), params.$name, preset.map(|v| v.$name))?
        };
    }
    Ok(Vehicle {
        mass_kg: field!(mass_kg),
        inertia_kgm2,
        wing_span_m: field!(wing_span_m),
        wing_area_m2: field!(wing_area_m2),
        mean_chord_m: field!(mean_chord_m),
        oswald_efficiency: field!(oswald_efficiency),
        c_lift_0: field!(c_lift_0),
        c_lift_alpha: field!(c_lift_alpha),
        c_lift_q: field!(c_lift_q),
        c_lift_elevator: field!(c_lift_elevator),
        c_drag_p: field!(c_drag_p),
        c_drag_q: field!(c_drag_q),
        c_drag_elevator: field!(c_drag_elevator),
        c_pitch_0: field!(c_pitch_0),
        c_pitch_alpha: field!(c_pitch_alpha),
        c_pitch_q: field!(c_pitch_q),
        c_pitch_elevator: field!(c_pitch_elevator),
        stall_blend_rate: field!(stall_blend_rate),
        stall_alpha_rad: field!(stall_alpha_rad),
        c_side_0: field!(c_side_0),
        c_side_beta: field!(c_side_beta),
        c_side_p: field!(c_side_p),
        c_side_r: field!(c_side_r),
        c_side_aileron: field!(c_side_aileron),
        c_side_rudder: field!(c_side_rudder),
        c_roll_0: field!(c_roll_0),
        c_roll_beta: field!(c_roll_beta),
        c_roll_p: field!(c_roll_p),
        c_roll_r: field!(c_roll_r),
        c_roll_aileron: field!(c_roll_aileron),
        c_roll_rudder: field!(c_roll_rudder),
        c_yaw_0: field!(c_yaw_0),
        c_yaw_beta: field!(c_yaw_beta),
        c_yaw_p: field!(c_yaw_p),
        c_yaw_r: field!(c_yaw_r),
        c_yaw_aileron: field!(c_yaw_aileron),
        c_yaw_rudder: field!(c_yaw_rudder),
        propeller_diameter_m: field!(propeller_diameter_m),
        max_voltage_v: field!(max_voltage_v),
        motor_speed_constant_vspr: field!(motor_speed_constant_vspr),
        motor_torque_constant_nmpa: field!(motor_torque_constant_nmpa),
        motor_resistance_ohm: field!(motor_resistance_ohm),
        no_load_current_a: field!(no_load_current_a),
        c_thrust_2: field!(c_thrust_2),
        c_thrust_1: field!(c_thrust_1),
        c_thrust_0: field!(c_thrust_0),
        c_torque_2: field!(c_torque_2),
        c_torque_1: field!(c_torque_1),
        c_torque_0: field!(c_torque_0),
    })
}

/// Three rows in kg m^2, which must be symmetric positive definite.
fn inertia(rows: [[f64; 3]; 3]) -> Result<Matrix3<f64>> {
    let inertia = Matrix3::from_row_slice(rows.as_flattened());
    if (inertia - inertia.transpose()).norm() >= 1e-9 || inertia.cholesky().is_none() {
        bail!("Aerosonde inertia must be symmetric positive definite");
    }
    Ok(inertia)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::{Value, json};

    use super::super::aircraft_state::AircraftState;
    use super::{Aerosonde, AerosondeConfig, Control, MotionModel, aerodynamics, propeller};
    use crate::math::{EulerAngles, Quaternion, Vec3};
    use crate::plugin::PluginParams;

    fn configure(yaml: &str) -> anyhow::Result<AerosondeConfig> {
        Aerosonde::configure(&PluginParams::yaml(yaml))
    }

    #[test]
    fn the_airframe_fills_every_number_and_a_key_overrides_one() {
        let preset = configure("airframe: aerosonde").unwrap();
        assert_eq!(preset.vehicle.mass_kg, 11.0);
        assert_eq!(preset.air_density_kgpm3, 1.225);
        let heavier =
            configure("{airframe: aerosonde, mass_kg: 13.5, air_density_kgpm3: 1.2682}").unwrap();
        assert_eq!(heavier.vehicle.mass_kg, 13.5);
        assert_eq!(heavier.air_density_kgpm3, 1.2682);
        assert_eq!(heavier.vehicle.c_lift_alpha, preset.vehicle.c_lift_alpha);
        let error = configure("mass_kg: 11")
            .err()
            .expect("no airframe and not every number");
        assert!(error.to_string().contains("without `airframe`"), "{error}");
        let error = configure("{airframe: aerosonde, c_drag_0: 0.04}")
            .err()
            .expect("not a key");
        assert!(error.to_string().contains("c_drag_0"), "{error}");
        let error = configure("{airframe: aerosonde, air_density_kgpm3: 0}")
            .err()
            .expect("the propeller needs air");
        assert!(error.to_string().contains("air density"), "{error}");
    }

    fn vector(value: &Value) -> Vec3 {
        let v: Vec<f64> = value
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_f64().unwrap())
            .collect();
        Vec3::new(v[0], v[1], v[2])
    }

    /// Every held state of reference/aerosonde_check.py's transcription of
    /// the book's equations, reproduced from the same inputs: the
    /// aerodynamic force and moment, the propeller's thrust, torque, and
    /// speed, and the derivative's body and angular accelerations and
    /// world velocity at the sample's attitude in still air (gravity, the
    /// propeller's reaction, and the product of inertia enter there). Also
    /// the vehicle's mass, inertia, and geometry against the fixture's.
    /// Prints the largest errors for that script.
    #[test]
    fn book_samples() {
        // Rounding in the terms' arithmetic, with headroom.
        const LIMIT: f64 = 1e-12;
        let text = match std::env::var("RIPPLE_AEROSONDE_SAMPLES") {
            Ok(path) => std::fs::read_to_string(path).expect("samples file"),
            Err(_) => include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/aerosonde_book_samples.json"
            ))
            .to_string(),
        };
        let fixture: Value = serde_json::from_str(&text).expect("samples JSON");
        let config = configure(&fixture["mission_keys"].to_string()).expect("the keys parse");
        let vehicle = &config.vehicle;
        let rho = config.air_density_kgpm3;
        let numbers = &fixture["vehicle"];
        assert_eq!(vehicle.mass_kg, numbers["mass_kg"].as_f64().unwrap());
        assert_eq!(
            vehicle.wing_span_m,
            numbers["wing_span_m"].as_f64().unwrap()
        );
        assert_eq!(
            vehicle.wing_area_m2,
            numbers["wing_area_m2"].as_f64().unwrap()
        );
        assert_eq!(
            vehicle.mean_chord_m,
            numbers["mean_chord_m"].as_f64().unwrap()
        );
        for (row, values) in numbers["inertia_kgm2"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            for (column, value) in values.as_array().unwrap().iter().enumerate() {
                assert_eq!(vehicle.inertia_kgm2[(row, column)], value.as_f64().unwrap());
            }
        }

        let mut largest: BTreeMap<&str, f64> = BTreeMap::new();
        let mut count = 0;
        for sample in fixture["samples"].as_array().unwrap() {
            let air_velocity = vector(&sample["air_velocity_frd_mps"]);
            let rates = vector(&sample["rates_frd_radps"]);
            let control = Control {
                throttle: sample["throttle"].as_f64().unwrap(),
                elevator_rad: sample["elevator_rad"].as_f64().unwrap(),
                aileron_rad: sample["aileron_rad"].as_f64().unwrap(),
                rudder_rad: sample["rudder_rad"].as_f64().unwrap(),
            };
            // Errors relative to q S for forces, q S b for moments, and the
            // standing propeller's full-throttle thrust, torque, and speed.
            let qbar_s = 0.5 * rho * air_velocity.norm_squared() * vehicle.wing_area_m2;
            let full = propeller::loads(vehicle, rho, 0.0, 1.0);
            let full_speed = propeller::speed_radps(vehicle, rho, 0.0, 1.0);
            let force = aerodynamics::force(vehicle, rho, air_velocity, rates, control);
            let moment = aerodynamics::moment(vehicle, rho, air_velocity, rates, control);
            let propeller = propeller::loads(vehicle, rho, air_velocity.norm(), control.throttle);
            let speed = propeller::speed_radps(vehicle, rho, air_velocity.norm(), control.throttle);
            // The derivative at the sample's attitude, in still air, so the
            // body velocity is the air velocity.
            let euler = vector(&sample["attitude_ned_euler_rad"]);
            let attitude = Quaternion::from_euler(EulerAngles {
                roll_world_from_body_rad: euler.x,
                pitch_world_from_body_rad: euler.y,
                yaw_world_from_body_rad: euler.z,
            });
            let state = AircraftState {
                velocity_frd_mps: air_velocity,
                rates_frd_radps: rates,
                velocity_ned_mps: attitude.rotate_body_to_world(air_velocity),
                position_ned_m: Vec3::zeros(),
                attitude_ned_from_frd: attitude,
            };
            let rate = config.derivative(&state, control);
            let relative = |actual: Vec3, expected: Vec3| {
                (actual - expected).norm() / expected.norm().max(1.0)
            };
            let errors = [
                (
                    "force",
                    (force - vector(&sample["aero_force_frd_n"])).norm() / qbar_s,
                ),
                (
                    "moment",
                    (moment - vector(&sample["aero_moment_frd_nm"])).norm()
                        / (qbar_s * vehicle.wing_span_m),
                ),
                (
                    "thrust",
                    (propeller.thrust_n - sample["thrust_n"].as_f64().unwrap()).abs()
                        / full.thrust_n,
                ),
                (
                    "torque",
                    (propeller.torque_nm - sample["propeller_torque_nm"].as_f64().unwrap()).abs()
                        / full.torque_nm,
                ),
                (
                    "propeller_speed",
                    (speed - sample["propeller_speed_radps"].as_f64().unwrap()).abs() / full_speed,
                ),
                (
                    "acceleration",
                    relative(
                        rate.acceleration_frd_mps2,
                        vector(&sample["acceleration_frd_mps2"]),
                    ),
                ),
                (
                    "angular_acceleration",
                    relative(
                        rate.angular_acceleration_frd_radps2,
                        vector(&sample["angular_acceleration_frd_radps2"]),
                    ),
                ),
                (
                    "velocity_ned",
                    relative(rate.velocity_ned_mps, vector(&sample["velocity_ned_mps"])),
                ),
            ];
            for (name, error) in errors {
                assert!(error <= LIMIT, "sample {count}: {name} error {error:e}");
                let size = largest.entry(name).or_default();
                *size = size.max(error);
            }
            count += 1;
        }
        assert!(count > 0);
        println!(
            "AEROSONDE_ERRORS {}",
            json!({"samples": count, "limit": LIMIT, "largest": largest})
        );
    }
}

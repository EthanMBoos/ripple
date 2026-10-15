//! Multirotor vehicle constants, shared by the Multirotor motion model and
//! MultirotorVelocityController (a flight controller carries its own copy of
//! the vehicle's numbers, so the two can be set apart to study model error).
//!
//! A mission either names a measured vehicle with `airframe` (and may
//! override any of its keys) or gives every vehicle number itself. There is
//! no default vehicle. The one built-in is a Crazyflie 2.0 from Förster's
//! 2015 ETH thesis (handle 20.500.11850/214143). Equation numbers below are
//! the thesis's; its body axes (Fig. 2.4) run along the motor arms, so the
//! Crazyflie here is a "+" quad whose forward is an arm, 45 degrees from the
//! real vehicle's front. That keeps the measured inertia matrix as published.
//!
//! A preset holds only the vehicle's own numbers. Optional effects (motor
//! lag, rotor drag) are switched on in the mission, like FixedWing6DOF's stall
//! tables; the book chapter "Flying a multirotor" gives Förster's values, and
//! the tests below derive them.

use anyhow::{Context, Result, ensure};
use nalgebra::Matrix3;
use serde::Deserialize;

use crate::math::{self, EulerAngles, Quaternion, Vec3};

/// A measured vehicle a mission can start from.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AirframeName {
    /// Crazyflie 2.0, from Förster's measurements (see `crazyflie2`).
    Crazyflie2,
}

/// Keys a mission may set; with a preset, `None` keeps its value, and
/// without one every key but gravity is required.
#[derive(Default)]
pub(crate) struct AirframeOverrides {
    pub mass_kg: Option<f64>,
    pub gravity_mps2: Option<f64>,
    pub inertia_kgm2: Option<[[f64; 3]; 3]>,
    pub thrust_coefficient: Option<f64>,
    pub torque_coefficient: Option<f64>,
    pub omega_min_radps: Option<f64>,
    pub omega_max_radps: Option<f64>,
    pub rotors: Option<Vec<RotorParams>>,
}

/// One rotor as a mission gives it, such as
/// `{direction: ccw, position_m: [0, 0.175, 0]}`.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RotorParams {
    pub direction: Direction,
    /// Hub offset from the center of mass, body forward/left/up.
    pub position_m: [f64; 3],
    /// The thrust axis's tilt from body +z, as roll, pitch, and yaw (a
    /// rotation like an entity's attitude); untilted when left out.
    #[serde(default)]
    pub tilt_deg: [f64; 3],
}

#[derive(Clone, Debug)]
pub(crate) struct Airframe {
    pub mass_kg: f64,
    pub gravity_mps2: f64,
    pub inertia_kgm2: Matrix3<f64>,
    /// Thrust = c_T w^2 (N).
    pub thrust_coefficient: f64,
    /// Reaction torque = c_Q w^2 (N m).
    pub torque_coefficient: f64,
    /// An armed motor's idle speed; there is no "off" below it.
    pub omega_min_radps: f64,
    pub omega_max_radps: f64,
    pub rotors: Vec<Rotor>,
}

#[derive(Clone, Debug)]
pub(crate) struct Rotor {
    /// Input (motion) or output (controller) port, motor_0, motor_1, ...
    pub port: String,
    pub direction: Direction,
    /// Hub position relative to the center of mass.
    pub offset_body_m: Vec3,
    /// Unit vector along which the rotor pushes.
    pub thrust_axis_body: Vec3,
}

/// Spin direction seen from above: `cw` or `ccw`.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
pub(crate) enum Direction {
    #[serde(rename = "cw")]
    Clockwise,
    #[serde(rename = "ccw")]
    Counterclockwise,
}

impl Direction {
    /// A clockwise rotor (seen from above) twists the body counterclockwise,
    /// about +z of an untilted rotor.
    pub fn reaction_sign(self) -> f64 {
        match self {
            Direction::Clockwise => 1.0,
            Direction::Counterclockwise => -1.0,
        }
    }
}

impl Airframe {
    pub fn new(name: Option<AirframeName>, overrides: AirframeOverrides) -> Result<Self> {
        let preset = match name {
            Some(AirframeName::Crazyflie2) => crazyflie2(),
            None => return Self::from_keys(overrides),
        };
        let inertia_kgm2 = match overrides.inertia_kgm2 {
            Some(rows) => inertia(rows)?,
            None => preset.inertia_kgm2,
        };
        let rotors = match overrides.rotors {
            Some(params) => rotors(&params)?,
            None => preset.rotors,
        };
        let airframe = Self {
            mass_kg: overrides.mass_kg.unwrap_or(preset.mass_kg),
            gravity_mps2: overrides.gravity_mps2.unwrap_or(preset.gravity_mps2),
            inertia_kgm2,
            thrust_coefficient: overrides
                .thrust_coefficient
                .unwrap_or(preset.thrust_coefficient),
            torque_coefficient: overrides
                .torque_coefficient
                .unwrap_or(preset.torque_coefficient),
            omega_min_radps: overrides.omega_min_radps.unwrap_or(preset.omega_min_radps),
            omega_max_radps: overrides.omega_max_radps.unwrap_or(preset.omega_max_radps),
            rotors,
        };
        airframe.check()
    }

    /// A vehicle given entirely by the mission.
    fn from_keys(keys: AirframeOverrides) -> Result<Self> {
        let missing = |key: &str| {
            format!(
                "Multirotor: without `airframe`, the mission must give every vehicle number; \
                 `{key}` is missing"
            )
        };
        let airframe = Self {
            mass_kg: keys.mass_kg.with_context(|| missing("mass"))?,
            gravity_mps2: keys.gravity_mps2.unwrap_or(9.81),
            inertia_kgm2: inertia(keys.inertia_kgm2.with_context(|| missing("inertia_kgm2"))?)?,
            thrust_coefficient: keys.thrust_coefficient.with_context(|| missing("c_T"))?,
            torque_coefficient: keys.torque_coefficient.with_context(|| missing("c_Q"))?,
            omega_min_radps: keys.omega_min_radps.with_context(|| missing("omega_min"))?,
            omega_max_radps: keys.omega_max_radps.with_context(|| missing("omega_max"))?,
            rotors: rotors(&keys.rotors.with_context(|| missing("rotors"))?)?,
        };
        airframe.check()
    }

    fn check(self) -> Result<Self> {
        ensure!(
            self.mass_kg > 0.0
                && self.gravity_mps2 >= 0.0
                && self.thrust_coefficient >= 0.0
                && self.torque_coefficient >= 0.0,
            "Multirotor requires positive mass and nonnegative gravity and coefficients"
        );
        ensure!(
            self.omega_min_radps >= 0.0 && self.omega_min_radps <= self.omega_max_radps,
            "Multirotor requires 0 <= omega_min <= omega_max"
        );
        Ok(self)
    }
}

/// Crazyflie 2.0 after Förster (2015). Measured: mass, inertia, and the
/// thrust, speed, and torque fits against the 16-bit motor command. Derived
/// here: c_T, c_Q, omega_max, and the arm length (see each line). The
/// quadratic c_T w^2 matches the thrust fit exactly at hover and within 10.4%
/// from command 8000 to 64000 (`thrust_coefficient_tracks_forster_fit`).
fn crazyflie2() -> Airframe {
    // Appendix A / Table 2.2: weighed, 28.0 g.
    let mass_kg = 0.028;
    let gravity_mps2 = 9.81;
    // Choose c_T so that four rotors hold the vehicle up at exactly the
    // command where eq. (3.3) gives a quarter of its weight.
    let hover_thrust_n = mass_kg * gravity_mps2 / 4.0;
    let hover_command = forster_command_for_thrust(hover_thrust_n);
    let hover_speed_radps = forster_speed_radps(hover_command);
    let thrust_coefficient = hover_thrust_n / (hover_speed_radps * hover_speed_radps);
    // Eq. (3.5): torque = 0.005964552 * thrust + 1.563383e-5 N m. The offset
    // (4% of the hover torque) is dropped so torque stays c_Q w^2.
    let torque_coefficient = 0.005964552 * thrust_coefficient;
    // The largest thrust eq. (3.3) gives (command 65535) sets the top speed,
    // so the vehicle has its measured thrust margin; eq. (3.4) itself gives
    // 3052 rad/s there, where c_T w^2 would overstate thrust by 5%.
    let omega_max_radps = (forster_thrust_n(65535.0) / thrust_coefficient).sqrt();
    // Table 2.2: dCF = 0.092 m between opposite motor axes. Rotors on the
    // body axes (Fig. 2.4), listed counterclockwise from +x. The thesis does
    // not say which pair spins which way (Fig. 3.1 only that diagonal
    // pairs match); mirroring it flips only the sign of yaw.
    let arm_m = 0.092 / 2.0;
    let rotor = |direction, position_m| RotorParams {
        direction,
        position_m,
        tilt_deg: [0.0; 3],
    };
    Airframe {
        mass_kg,
        gravity_mps2,
        // Eq. (2.18)-(2.19), chosen by the thesis over eq. (2.17).
        inertia_kgm2: Matrix3::new(
            16.571710, 0.830806, 0.718277, //
            0.830806, 16.655602, 1.800197, //
            0.718277, 1.800197, 29.261652,
        ) * 1e-6,
        thrust_coefficient,
        torque_coefficient,
        // Commands below 1000 stop the motor (Sec. 3.3.2).
        omega_min_radps: 0.0,
        omega_max_radps,
        rotors: rotors(&[
            rotor(Direction::Clockwise, [arm_m, 0.0, 0.0]),
            rotor(Direction::Counterclockwise, [0.0, arm_m, 0.0]),
            rotor(Direction::Clockwise, [-arm_m, 0.0, 0.0]),
            rotor(Direction::Counterclockwise, [0.0, -arm_m, 0.0]),
        ])
        .expect("the Crazyflie's rotors are valid"),
    }
}

/// Förster eq. (3.3): one rotor's thrust (N) against the motor command
/// (0 to 65535).
fn forster_thrust_n(command: f64) -> f64 {
    2.130295e-11 * command * command + 1.032633e-6 * command + 5.484560e-4
}

/// Förster eq. (3.4), second series, recommended there: rotor speed (rad/s)
/// against the motor command.
fn forster_speed_radps(command: f64) -> f64 {
    0.04076521 * command + 380.8359
}

/// Invert eq. (3.3) on its rising branch.
fn forster_command_for_thrust(thrust_n: f64) -> f64 {
    let (a, b, c) = (2.130295e-11, 1.032633e-6, 5.484560e-4 - thrust_n);
    (-b + (b * b - 4.0 * a * c).sqrt()) / (2.0 * a)
}

/// Three rows, kg m^2.
pub(crate) fn inertia(rows: [[f64; 3]; 3]) -> Result<Matrix3<f64>> {
    let inertia_kgm2 = Matrix3::from_row_slice(rows.as_flattened());
    ensure!(
        (inertia_kgm2 - inertia_kgm2.transpose()).norm() < 1e-9
            && inertia_kgm2.cholesky().is_some(),
        "Multirotor inertia must be symmetric positive definite"
    );
    Ok(inertia_kgm2)
}

/// Rotor i reads (or a controller writes) port motor_i.
pub(crate) fn rotors(params: &[RotorParams]) -> Result<Vec<Rotor>> {
    ensure!(!params.is_empty(), "Multirotor requires at least one rotor");
    let mut rotors = Vec::new();
    for (index, rotor) in params.iter().enumerate() {
        let [x, y, z] = rotor.position_m;
        let [roll, pitch, yaw] = rotor.tilt_deg;
        // The rotor pushes along body +z turned by its tilt.
        let tilt = Quaternion::from_euler(EulerAngles {
            roll_world_from_body_rad: math::deg_to_rad(roll),
            pitch_world_from_body_rad: math::deg_to_rad(pitch),
            yaw_world_from_body_rad: math::deg_to_rad(yaw),
        });
        rotors.push(Rotor {
            port: format!("motor_{index}"),
            direction: rotor.direction,
            offset_body_m: Vec3::new(x, y, z),
            thrust_axis_body: tilt.rotate_body_to_world(Vec3::new(0.0, 0.0, 1.0)),
        });
    }
    Ok(rotors)
}

#[cfg(test)]
mod tests {
    use anyhow::Result;

    use super::{
        Airframe, AirframeName, AirframeOverrides, Direction, RotorParams, crazyflie2,
        forster_speed_radps, forster_thrust_n, rotors,
    };
    use crate::math::Vec3;

    #[test]
    fn crazyflie_hovers_at_forsters_hover_command() {
        let airframe = crazyflie2();
        let thrust_n = |speed_radps: f64| airframe.thrust_coefficient * speed_radps * speed_radps;
        // Hover: c_T w^2 = m g / 4 at the speed eq. (3.4) gives the hover
        // command; worked by hand, command 37287 and 1900.85 rad/s.
        let hover_speed_radps = (0.028 * 9.81 / 4.0 / airframe.thrust_coefficient).sqrt();
        assert!((hover_speed_radps - 1900.85).abs() < 0.01);
        assert!(
            (thrust_n(forster_speed_radps(37286.94)) - forster_thrust_n(37286.94)).abs() < 1e-7
        );
        assert!((thrust_n(airframe.omega_max_radps) - 0.15971).abs() < 1e-5);
        assert!(
            (airframe.torque_coefficient / airframe.thrust_coefficient - 0.005964552).abs() < 1e-15
        );
        // Opposite motors 92 mm apart.
        let span_m = (airframe.rotors[0].offset_body_m - airframe.rotors[2].offset_body_m).norm();
        assert!((span_m - 0.092).abs() < 1e-15);
    }

    #[test]
    fn thrust_coefficient_tracks_forster_fit() {
        let airframe = crazyflie2();
        for command in 8000..=64000 {
            let command = f64::from(command);
            let speed_radps = forster_speed_radps(command);
            let model_n = airframe.thrust_coefficient * speed_radps * speed_radps;
            let measured_n = forster_thrust_n(command);
            assert!(
                ((model_n - measured_n) / measured_n).abs() < 0.104,
                "command {command}"
            );
        }
    }

    /// Förster eqs. (3.16)-(3.17): command to thrust is first order,
    /// H(z) = b / (1 - 0.9695404 z^-1), sampled at 500 Hz (Sec. 3.4.2). Its
    /// time constant -T / ln(a) is the `motor_time_constant_up` and `_down`
    /// that Crazyflie missions write, 0.0647 s. Förster's lag is command to
    /// thrust; Ripple, like RotorS, lags shaft speed, and thrust goes with
    /// speed squared. The two share this time constant for small changes
    /// about a speed (near hover) but differ for large steps. The thesis did
    /// not remove
    /// radio or load cell latency from the fit, so the true lag may be shorter.
    #[test]
    fn crazyflie_motor_lag_comes_from_the_500_hz_fit() {
        let sample_period_s = 1.0 / 500.0;
        let time_constant_s = -sample_period_s / 0.9695404_f64.ln();
        assert!((time_constant_s - 0.0647).abs() < 5e-5);
    }

    #[test]
    fn overrides_replace_single_preset_values() -> Result<()> {
        let airframe = Airframe::new(
            Some(AirframeName::Crazyflie2),
            AirframeOverrides {
                mass_kg: Some(0.032),
                ..AirframeOverrides::default()
            },
        )?;
        assert_eq!(airframe.mass_kg, 0.032);
        assert_eq!(airframe.inertia_kgm2, crazyflie2().inertia_kgm2);
        let invalid = AirframeOverrides {
            omega_min_radps: Some(2000.0),
            omega_max_radps: Some(1000.0),
            ..AirframeOverrides::default()
        };
        assert!(Airframe::new(Some(AirframeName::Crazyflie2), invalid).is_err());
        Ok(())
    }

    #[test]
    fn a_vehicle_without_a_preset_must_give_every_number() -> Result<()> {
        let keys = || AirframeOverrides {
            mass_kg: Some(1.0),
            inertia_kgm2: Some([[0.01, 0.0, 0.0], [0.0, 0.01, 0.0], [0.0, 0.0, 0.02]]),
            thrust_coefficient: Some(1e-5),
            torque_coefficient: Some(1e-7),
            omega_min_radps: Some(0.0),
            omega_max_radps: Some(1000.0),
            rotors: Some(vec![
                RotorParams {
                    direction: Direction::Clockwise,
                    position_m: [0.2, 0.0, 0.0],
                    tilt_deg: [0.0; 3],
                },
                RotorParams {
                    direction: Direction::Counterclockwise,
                    position_m: [0.0, 0.2, 0.0],
                    tilt_deg: [0.0; 3],
                },
            ]),
            ..AirframeOverrides::default()
        };
        let airframe = Airframe::new(None, keys())?;
        assert_eq!(airframe.gravity_mps2, 9.81);
        let error = Airframe::new(
            None,
            AirframeOverrides {
                thrust_coefficient: None,
                ..keys()
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("`c_T` is missing"), "{error}");
        Ok(())
    }

    #[test]
    fn tilted_rotor_axis_follows_its_roll_pitch_yaw() -> Result<()> {
        let rotors = rotors(&[
            RotorParams {
                direction: Direction::Clockwise,
                position_m: [0.1, 0.0, 0.2],
                tilt_deg: [0.0, 90.0, 0.0],
            },
            RotorParams {
                direction: Direction::Counterclockwise,
                position_m: [0.0, 0.1, 0.0],
                tilt_deg: [30.0, 0.0, 45.0],
            },
        ])?;
        assert!((rotors[0].thrust_axis_body - Vec3::new(1.0, 0.0, 0.0)).norm() < 1e-15);
        // Roll 30 degrees tips +z toward -y; yaw 45 degrees then turns that
        // about body z, as for an entity's attitude.
        let half = 0.5_f64;
        let expected = Vec3::new(
            45_f64.to_radians().sin() * half,
            -45_f64.to_radians().cos() * half,
            3_f64.sqrt() * half,
        );
        assert!((rotors[1].thrust_axis_body - expected).norm() < 1e-12);
        assert!(super::rotors(&[]).is_err());
        for invalid in [
            "[{direction: up, position_m: [0, 0, 0]}]",
            "[{direction: cw, position_m: [0, 0]}]",
            "[{direction: cw, position: [0, 0, 0]}]",
        ] {
            assert!(serde_yaml_ng::from_str::<Vec<RotorParams>>(invalid).is_err());
        }
        Ok(())
    }
}

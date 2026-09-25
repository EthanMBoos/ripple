//! C++ Multirotor motion port (GTRI, LGPL-3.0-or-later).
//! Motion phase: read motor shaft speeds in rad/s, integrate, write world truth.
//! Body axes are forward/left/up. This preserves the reference equations,
//! including frozen RK4 loads; it is not a newly validated flight model.
//! External-force contact, teleport, acceleration sensor access and private CSV
//! output are not exposed by this port. Use GroundCollision for removal, not landing.
//! No closed-loop controller is ported yet; MotorSpeeds only supplies fixed rad/s.

mod dynamics;

use anyhow::{Context, Result, ensure};
use nalgebra::Matrix3;
use serde::Deserialize;

use crate::math::{self, KinematicState, Vec3};
use crate::plugin::{
    Frame, MotionContext, MotionModel, Plugin, PluginIo, PluginParams, Port, Ports, Unit, Update,
};

use dynamics::{RotorcraftState, derivative};

/// Mission parameters; `Default` supplies any key the mission leaves out
/// (the C++ quadrotor defaults).
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct MultirotorParams {
    #[serde(rename = "mass")]
    mass_kg: f64,
    #[serde(rename = "gravity_magnitude")]
    gravity_mps2: f64,
    /// Three bracketed rows, kg m^2.
    inertia_matrix: String,
    /// Drag D = 0.5 c_D |V| V.
    #[serde(rename = "c_D")]
    drag_coefficient: f64,
    /// Thrust = c_T w^2 (N).
    #[serde(rename = "c_T")]
    thrust_coefficient: f64,
    /// Torque = c_Q w^2 (N m).
    #[serde(rename = "c_Q")]
    torque_coefficient: f64,
    #[serde(rename = "omega_min")]
    omega_min_radps: f64,
    #[serde(rename = "omega_max")]
    omega_max_radps: f64,
    /// One row per rotor, `[direction x y z roll pitch yaw]` in body forward/left/up.
    /// Rotor i reads input motor_i (rad/s).
    rotor_config: String,
    /// Not supported; use the run's frames and Rerun. Must stay false.
    write_csv: bool,
    show_shapes: bool,
}

impl Default for MultirotorParams {
    fn default() -> Self {
        Self {
            mass_kg: 1.5,
            gravity_mps2: 9.81,
            inertia_matrix: "[0.0122 0 0] [0 0.0122 0] [0 0 0.0244]".into(),
            drag_coefficient: 0.058,
            thrust_coefficient: 5.45e-6,
            torque_coefficient: 2.284e-7,
            omega_min_radps: 346.41,
            omega_max_radps: 1200.0,
            rotor_config: "[CCW 0 -0.175 0 0 0 0] [CCW 0 0.175 0 0 0 0] \
                           [CW 0.175 0 0 0 0 0] [CW -0.175 0 0 0 0 0]"
                .into(),
            write_csv: false,
            show_shapes: false,
        }
    }
}

#[derive(Clone)]
pub struct MultirotorConfig {
    mass_kg: f64,
    gravity_mps2: f64,
    inertia_kgm2: Matrix3<f64>,
    inverse_inertia: Matrix3<f64>,
    drag_coefficient: f64,
    thrust_coefficient: f64,
    torque_coefficient: f64,
    omega_min_radps: f64,
    omega_max_radps: f64,
    rotors: Vec<Rotor>,
}

#[derive(Clone)]
struct Rotor {
    port: String,
    direction: Direction,
    offset_length_m: f64,
    xy_angle_rad: f64,
}

#[derive(Clone, Copy)]
enum Direction {
    Clockwise,
    Counterclockwise,
}

pub struct Multirotor {
    config: MultirotorConfig,
    state: RotorcraftState,
    motor_speeds_radps: Vec<f64>,
}

impl Plugin for Multirotor {
    type Config = MultirotorConfig;

    fn configure(params: &PluginParams<'_>) -> Result<Self::Config> {
        let params: MultirotorParams = params.parse()?;
        ensure!(
            !params.write_csv,
            "Multirotor write_csv=true is not supported; use run frames and Rerun"
        );
        ensure!(
            !params.show_shapes,
            "Multirotor show_shapes=true is not supported; use run frames and Rerun"
        );
        let values = numbers(&params.inertia_matrix.replace(['[', ']'], " "))?;
        ensure!(values.len() == 9, "Multirotor inertia requires nine values");
        let inertia_kgm2 = Matrix3::from_row_slice(&values);
        ensure!(
            (inertia_kgm2 - inertia_kgm2.transpose()).norm() < 1e-9
                && inertia_kgm2.cholesky().is_some(),
            "Multirotor inertia must be symmetric positive definite"
        );
        let inverse_inertia = inertia_kgm2
            .try_inverse()
            .context("Multirotor inertia must be invertible")?;
        let config = MultirotorConfig {
            mass_kg: params.mass_kg,
            gravity_mps2: params.gravity_mps2,
            inertia_kgm2,
            inverse_inertia,
            drag_coefficient: params.drag_coefficient,
            thrust_coefficient: params.thrust_coefficient,
            torque_coefficient: params.torque_coefficient,
            omega_min_radps: params.omega_min_radps,
            omega_max_radps: params.omega_max_radps,
            rotors: parse_rotors(&params.rotor_config)?,
        };
        ensure!(
            config.mass_kg > 0.0
                && config.gravity_mps2 >= 0.0
                && config.drag_coefficient >= 0.0
                && config.thrust_coefficient >= 0.0
                && config.torque_coefficient >= 0.0,
            "Multirotor requires positive mass and nonnegative gravity and coefficients"
        );
        ensure!(
            config.omega_min_radps >= 0.0 && config.omega_min_radps <= config.omega_max_radps,
            "Multirotor requires 0 <= omega_min <= omega_max"
        );
        Ok(config)
    }

    fn new(config: &Self::Config) -> Self {
        Self {
            config: config.clone(),
            state: RotorcraftState::default(),
            motor_speeds_radps: vec![0.0; config.rotors.len()],
        }
    }

    fn ports(config: &Self::Config) -> Ports {
        let mut ports = Ports::default();
        for rotor in &config.rotors {
            ports = ports.input(Port::new(&rotor.port, Unit::RadiansPerSecond, Frame::None));
        }
        ports
    }
}

impl MotionModel for Multirotor {
    fn step(&mut self, context: &mut MotionContext<'_>, io: &mut PluginIo) -> Result<Update> {
        for (rotor, speed_radps) in self.config.rotors.iter().zip(&mut self.motor_speeds_radps) {
            *speed_radps = io.read(&rotor.port)?;
        }
        self.integrate(context.truth, context.time.dt_s)?;
        Ok(Update::Applied)
    }
}

impl Multirotor {
    fn integrate(&mut self, truth: &mut KinematicState, dt_s: f64) -> Result<()> {
        self.state.read_truth(truth);
        let config = &self.config;
        let mut thrust_body_n = Vec3::zeros();
        let mut moment_body_nm = Vec3::zeros();
        for (rotor, speed_radps) in config.rotors.iter().zip(&self.motor_speeds_radps) {
            let speed_radps = speed_radps.clamp(config.omega_min_radps, config.omega_max_radps);
            let speed_squared = speed_radps * speed_radps;
            let mut thrust_n = config.thrust_coefficient * speed_squared;
            // Matches C++ for now, though it is discontinuous: thrust is zero only when the
            // clamped speed equals omega_min exactly, and yaw torque is still applied.
            if (speed_radps - config.omega_min_radps).abs() < f64::EPSILON {
                thrust_n = 0.0;
            }
            let sign = match rotor.direction {
                Direction::Clockwise => 1.0,
                Direction::Counterclockwise => -1.0,
            };
            thrust_body_n.z += thrust_n;
            moment_body_nm.x += thrust_n * rotor.offset_length_m * rotor.xy_angle_rad.sin();
            moment_body_nm.y -= thrust_n * rotor.offset_length_m * rotor.xy_angle_rad.cos();
            moment_body_nm.z += sign * config.torque_coefficient * speed_squared;
        }
        let weight_body_n = truth
            .orientation_world_from_body
            .rotate_world_to_body(Vec3::new(0.0, 0.0, -config.mass_kg * config.gravity_mps2));
        let velocity_body_mps = self.state.velocity_body_mps;
        let drag_body_n =
            velocity_body_mps * (-0.5 * config.drag_coefficient * velocity_body_mps.norm());
        // With no external contact force, the C++ normal-force term remains zero.
        let acceleration_body_mps2 = (thrust_body_n + weight_body_n + drag_body_n) / config.mass_kg;
        let rates = self.state.rates_body_radps;
        let angular_acceleration_body_radps2 =
            config.inverse_inertia * (moment_body_nm - rates.cross(&(config.inertia_kgm2 * rates)));

        // Matches C++ for now, though it is not full RK4: drag, gravity, thrust moments and
        // gyroscopic terms use start-of-step state and stay fixed across the stages.
        // Only the kinematics (velocity cross rates, attitude, position) use stage state.
        let mut coordinates = self.state.coordinates();
        math::rk4(&mut coordinates, dt_s, |coordinates| {
            derivative(
                RotorcraftState::from_coordinates(*coordinates),
                acceleration_body_mps2,
                angular_acceleration_body_radps2,
            )
        });
        ensure!(
            coordinates.iter().all(|value| value.is_finite()),
            "Multirotor integration produced nonfinite state"
        );
        self.state = RotorcraftState::from_coordinates(coordinates);
        self.state.write_truth(truth);
        Ok(())
    }
}

fn numbers(text: &str) -> Result<Vec<f64>> {
    let mut values = Vec::new();
    for value in text.split_whitespace() {
        let value: f64 = value.parse().context("invalid Multirotor numeric value")?;
        ensure!(value.is_finite(), "Multirotor values must be finite");
        values.push(value);
    }
    Ok(values)
}

fn parse_rotors(mut text: &str) -> Result<Vec<Rotor>> {
    let mut rotors = Vec::new();
    while !text.trim().is_empty() {
        text = text
            .trim()
            .strip_prefix('[')
            .context("rotor_config requires bracketed rows")?;
        let (row, rest) = text.split_once(']').context("rotor_config is missing ]")?;
        let (direction, values) = row
            .trim()
            .split_once(char::is_whitespace)
            .context("rotor row requires direction and six numbers")?;
        let direction = match direction {
            "CW" => Direction::Clockwise,
            "CCW" => Direction::Counterclockwise,
            _ => anyhow::bail!("rotor direction must be CW or CCW"),
        };
        let values = numbers(values)?;
        ensure!(values.len() == 6, "rotor row requires x y z roll pitch yaw");
        let offset_m = Vec3::new(values[0], values[1], values[2]);
        // Matches C++ for now, though we don't consider it correct: rotor roll/pitch/yaw
        // are parsed but unused, and the moment arm is the full 3D offset length even
        // though the C++ XML says z is unused.
        rotors.push(Rotor {
            port: format!("motor_{}", rotors.len()),
            direction,
            offset_length_m: offset_m.norm(),
            xy_angle_rad: offset_m.y.atan2(offset_m.x),
        });
        text = rest;
    }
    ensure!(!rotors.is_empty(), "Multirotor requires at least one rotor");
    Ok(rotors)
}

#[cfg(test)]
mod tests {
    use anyhow::Result;

    use super::{Multirotor, parse_rotors};
    use crate::Params;
    use crate::math::{EulerAngles, KinematicState, Quaternion, Vec3};
    use crate::plugin::{Plugin, PluginParams};

    fn quad() -> Result<Multirotor> {
        let params = Params::from([
            ("mass".into(), "1.5".into()),
            ("c_D".into(), "0".into()),
            // Unit inertia keeps the expected rates equal to the applied torques.
            ("inertia_matrix".into(), "[1 0 0] [0 1 0] [0 0 1]".into()),
            ("rotor_config".into(), "[CCW 0 -0.175 0 0 0 0] [CCW 0 0.175 0 0 0 0] [CW 0.175 0 0 0 0 0] [CW -0.175 0 0 0 0 0]".into()),
        ]);
        Ok(Multirotor::new(&Multirotor::configure(&PluginParams(
            &params,
        ))?))
    }

    #[test]
    fn balanced_rotors_hover() -> Result<()> {
        let mut model = quad()?;
        let hover_radps = (model.config.mass_kg * model.config.gravity_mps2
            / (4.0 * model.config.thrust_coefficient))
            .sqrt();
        model.motor_speeds_radps.fill(hover_radps);
        let mut truth = KinematicState::default();
        for _ in 0..1000 {
            model.integrate(&mut truth, 0.001)?;
        }
        assert!(truth.position_world_m.norm() < 1e-12);
        assert!(truth.velocity_world_mps.norm() < 1e-12);
        assert!(truth.angular_velocity_world_radps.norm() < 1e-12);
        Ok(())
    }

    #[test]
    fn minimum_speed_removes_thrust_but_not_yaw_torque() -> Result<()> {
        let mut model = quad()?;
        model.config.rotors.truncate(1);
        model.motor_speeds_radps.truncate(1);
        model.motor_speeds_radps[0] = -100.0; // Clamps to omega_min.
        let mut truth = KinematicState::default();
        model.integrate(&mut truth, 0.01)?;
        assert!((truth.velocity_world_mps.z + 9.81 * 0.01).abs() < 1e-12);
        let expected_yaw_rate =
            -model.config.torque_coefficient * model.config.omega_min_radps.powi(2) * 0.01;
        assert!((truth.angular_velocity_world_radps.z - expected_yaw_rate).abs() < 1e-12);
        Ok(())
    }

    #[test]
    fn maximum_speed_clamps_and_cw_torque_is_positive() -> Result<()> {
        let mut high = quad()?;
        let mut limit = quad()?;
        high.motor_speeds_radps = vec![0.0, 0.0, 5000.0, 5000.0];
        limit.motor_speeds_radps = vec![0.0, 0.0, 1200.0, 1200.0];
        let mut high_truth = KinematicState::default();
        let mut limit_truth = KinematicState::default();
        high.integrate(&mut high_truth, 0.001)?;
        limit.integrate(&mut limit_truth, 0.001)?;
        assert_eq!(high.state.coordinates(), limit.state.coordinates());
        assert!(high_truth.angular_velocity_world_radps.z > 0.0);
        Ok(())
    }

    #[test]
    fn tilted_thrust_uses_forward_left_up_body_axes() -> Result<()> {
        let mut model = quad()?;
        model.motor_speeds_radps.fill(850.0);
        let attitude = Quaternion::from_euler(EulerAngles {
            roll_world_from_body_rad: 0.1,
            pitch_world_from_body_rad: 0.2,
            yaw_world_from_body_rad: 0.6,
        });
        let mut truth = KinematicState {
            orientation_world_from_body: attitude,
            ..KinematicState::default()
        };
        let acceleration = attitude.rotate_body_to_world(Vec3::new(
            0.0,
            0.0,
            4.0 * model.config.thrust_coefficient * 850.0_f64.powi(2) / model.config.mass_kg,
        )) + Vec3::new(0.0, 0.0, -9.81);
        model.integrate(&mut truth, 0.001)?;
        assert!((truth.velocity_world_mps - acceleration * 0.001).norm() < 1e-12);
        Ok(())
    }

    #[test]
    fn initial_world_velocity_does_not_initialize_private_body_velocity() -> Result<()> {
        let mut model = quad()?;
        model.config.gravity_mps2 = 0.0;
        let mut truth = KinematicState {
            velocity_world_mps: Vec3::new(4.0, 5.0, 6.0),
            ..KinematicState::default()
        };
        model.integrate(&mut truth, 0.01)?;
        assert_eq!(truth.position_world_m, Vec3::zeros());
        assert_eq!(truth.velocity_world_mps, Vec3::new(4.0, 5.0, 6.0));
        Ok(())
    }

    #[test]
    fn drag_is_frozen_at_start_of_rk4_step() -> Result<()> {
        let mut model = quad()?;
        model.config.gravity_mps2 = 0.0;
        model.config.drag_coefficient = 0.3;
        model.state.velocity_body_mps = Vec3::new(10.0, 0.0, 0.0);
        let mut truth = KinematicState {
            velocity_world_mps: model.state.velocity_body_mps,
            ..KinematicState::default()
        };
        model.integrate(&mut truth, 0.1)?;
        assert!((truth.velocity_world_mps.x - 9.0).abs() < 1e-12);
        assert!((truth.position_world_m.x - 0.95).abs() < 1e-12);
        Ok(())
    }

    #[test]
    fn rotor_geometry_retains_cpp_full_offset_length() -> Result<()> {
        let rotors = parse_rotors("[CW 3 0 4 10 20 30]")?;
        assert_eq!(rotors[0].offset_length_m, 5.0);
        for invalid in ["", "[bad 0 0 0 0 0 0]", "[CW 0 0]", "[CW NaN 0 0 0 0 0]"] {
            assert!(parse_rotors(invalid).is_err());
        }
        Ok(())
    }
}

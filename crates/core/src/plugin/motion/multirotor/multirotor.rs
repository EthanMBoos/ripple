//! Rigid-body multirotor with optional RotorS motor and rotor effects and a
//! built-in Crazyflie 2.0. Derived from SCRIMMAGE's Multirotor (GTRI,
//! LGPL-3.0-or-later).
//! Motion phase: read motor shaft speed commands in rad/s, integrate, write
//! world truth. Body axes are forward/left/up.
//!
//! Fidelity
//! - Source: the standard quadrotor model (thrust and reaction torque with
//!   rotor speed squared, rotor moments r x F along each rotor's axis, a
//!   rigid body, RK4 with every load evaluated at each stage). The optional
//!   motor lag, rotor drag, and rolling moment port RotorS's motor plugin
//!   (`rotor.rs`). Vehicle numbers come from `airframe.rs`: the Crazyflie 2.0
//!   measured by Förster (2015), or a vehicle the mission gives in full;
//!   c_T, c_Q, omega_max and the arm are derived from his fits there.
//! - Validated against: RotorS's motor-plugin source, sample by sample, for
//!   motor lag and per-rotor loads (`reference/rotors_motor.py`); Crazyflow's
//!   first-principles dynamics with the Crazyflie numbers
//!   (`reference/crazyflow_check.py`); Förster's drag, by the steady tilt in
//!   closed loop (`reference/multirotor_control.py`). Matching a source is not matching a vehicle: nothing here has been
//!   compared with real flights.
//! - Valid range: near-hover and moderate flight of a rigid quad or hexa in
//!   still air, away from the ground, with motor commands inside Förster's
//!   measured range for the Crazyflie (thrust fit within 10.4% from command
//!   8000 up). Drag terms are linear fits at the airspeeds Förster's wind
//!   tunnel reached (about 1.5 to 4 m/s).
//! - Not modeled (named zeros in `integrate`; inflow in `rotor.rs`): ground
//!   effect, rotor and body interference, inflow (thrust change with climb or
//!   forward speed), rotor gyroscopic and spin-up torques, wind, battery sag,
//!   and contact
//!   (use GroundCollision for removal, not landing). No external force
//!   input, teleport, or acceleration output.
//! - To go further: replay NanoBench's recorded Crazyflie motor commands
//!   (arXiv 2603.09908; 172 flights with Vicon truth, already in
//!   paper/literature_review) and report prediction error 10, 100, and
//!   500 ms ahead, as its Task 1 does, with each effect on and off. Its
//!   vehicle is a Crazyflie 2.1 at 40.85 g, not Förster's 28 g 2.0. The motor
//!   lag is the least certain value: Förster's fit is command to thrust and
//!   includes uncompensated latency, and Crazyflow's identified Crazyflies
//!   (learnsyslab/crazyflow, MIT) are slower still; sweep it. A measured
//!   thrust curve, inflow model, or ground effect would go in `rotor_loads`
//!   as data or one function.

mod airframe;
mod dynamics;
mod rotor;

pub(crate) use airframe::{Airframe, AirframeName, AirframeOverrides};

use anyhow::{Context, Result, ensure};
use nalgebra::{Matrix3, UnitQuaternion};
use serde::Deserialize;

use crate::math::{self, KinematicState, Vec3};
use crate::plugin::{
    Frame, MotionContext, MotionModel, Plugin, PluginIo, PluginParams, Port, Ports, Unit, Update,
};

use dynamics::{RotorcraftState, derivative};
use rotor::{MotorLag, RotorAerodynamics, RotorCoefficients, rotor_loads};

/// Mission parameters. `airframe` names a measured vehicle whose numbers
/// the other vehicle keys may override; without it, the mission gives every
/// vehicle number (see `airframe.rs`). Body drag and the RotorS effects
/// (motor lag, rotor drag, rolling moment) are off unless the mission sets
/// them.
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct MultirotorParams {
    /// `crazyflie2`, or absent for a vehicle given in full.
    airframe: Option<AirframeName>,
    #[serde(rename = "mass")]
    mass_kg: Option<f64>,
    #[serde(rename = "gravity_magnitude")]
    gravity_mps2: Option<f64>,
    /// Three bracketed rows, kg m^2.
    inertia_matrix: Option<String>,
    /// Quadratic body drag D = 0.5 c_D |V| V. Förster's Crazyflie
    /// drag is rotor drag instead.
    #[serde(rename = "c_D")]
    drag_coefficient: f64,
    /// Thrust = c_T w^2 (N).
    #[serde(rename = "c_T")]
    thrust_coefficient: Option<f64>,
    /// Reaction torque = c_Q w^2 (N m).
    #[serde(rename = "c_Q")]
    torque_coefficient: Option<f64>,
    #[serde(rename = "omega_min")]
    omega_min_radps: Option<f64>,
    #[serde(rename = "omega_max")]
    omega_max_radps: Option<f64>,
    /// One row per rotor, `[direction x y z roll pitch yaw]`: the hub offset in
    /// body forward/left/up metres and the rotor's tilt in degrees (a rotation
    /// of body +z, like an entity's attitude). Rotor i reads input motor_i.
    rotor_config: Option<String>,
    /// RotorS motor lag (s); 0 is none.
    #[serde(rename = "motor_time_constant_up")]
    motor_time_constant_up_s: f64,
    #[serde(rename = "motor_time_constant_down")]
    motor_time_constant_down_s: f64,
    /// RotorS rotor drag, kg/rad; see `rotor.rs`.
    rotor_drag_coefficient: f64,
    /// Drag along the rotor axis (Förster's K_aero z), kg/rad.
    rotor_drag_coefficient_axial: f64,
    /// RotorS rolling moment, kg m/rad.
    rolling_moment_coefficient: f64,
    /// Not supported; use the run's frames and Rerun. Must stay false.
    write_csv: bool,
    show_shapes: bool,
}

#[derive(Clone)]
pub struct MultirotorConfig {
    airframe: Airframe,
    inverse_inertia: Matrix3<f64>,
    /// Quadratic body drag, D = 0.5 c_D |V| V.
    body_drag_coefficient: f64,
    motor_lag: MotorLag,
    aerodynamics: RotorAerodynamics,
}

pub struct Multirotor {
    config: MultirotorConfig,
    state: RotorcraftState,
    /// This step's commands, as read.
    motor_commands_radps: Vec<f64>,
    /// Shaft speeds at the start of the step; empty until the first step,
    /// which starts each rotor at its command (the vehicle spawns flying).
    rotor_speeds_radps: Vec<f64>,
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
        let airframe = Airframe::new(
            params.airframe,
            AirframeOverrides {
                mass_kg: params.mass_kg,
                gravity_mps2: params.gravity_mps2,
                inertia_matrix: params.inertia_matrix,
                thrust_coefficient: params.thrust_coefficient,
                torque_coefficient: params.torque_coefficient,
                omega_min_radps: params.omega_min_radps,
                omega_max_radps: params.omega_max_radps,
                rotor_config: params.rotor_config,
            },
        )?;
        let inverse_inertia = airframe
            .inertia_kgm2
            .try_inverse()
            .context("Multirotor inertia must be invertible")?;
        let config = MultirotorConfig {
            airframe,
            inverse_inertia,
            body_drag_coefficient: params.drag_coefficient,
            motor_lag: MotorLag {
                time_constant_up_s: params.motor_time_constant_up_s,
                time_constant_down_s: params.motor_time_constant_down_s,
            },
            aerodynamics: RotorAerodynamics {
                drag_coefficient: params.rotor_drag_coefficient,
                axial_drag_coefficient: params.rotor_drag_coefficient_axial,
                rolling_moment_coefficient: params.rolling_moment_coefficient,
            },
        };
        let lag = config.motor_lag;
        let aerodynamics = config.aerodynamics;
        ensure!(
            config.body_drag_coefficient >= 0.0
                && lag.time_constant_up_s >= 0.0
                && lag.time_constant_down_s >= 0.0
                && aerodynamics.drag_coefficient >= 0.0
                && aerodynamics.axial_drag_coefficient >= 0.0
                && aerodynamics.rolling_moment_coefficient >= 0.0,
            "Multirotor drag coefficients and motor time constants must be nonnegative"
        );
        Ok(config)
    }

    fn new(config: &Self::Config) -> Self {
        Self {
            config: config.clone(),
            state: RotorcraftState::default(),
            motor_commands_radps: vec![0.0; config.airframe.rotors.len()],
            rotor_speeds_radps: Vec::new(),
        }
    }

    fn ports(config: &Self::Config) -> Ports {
        let mut ports = Ports::default();
        for rotor in &config.airframe.rotors {
            ports = ports.input(Port::new(&rotor.port, Unit::RadiansPerSecond, Frame::None));
        }
        ports
    }
}

impl MotionModel for Multirotor {
    fn step(&mut self, context: &mut MotionContext<'_>, io: &mut PluginIo) -> Result<Update> {
        let rotors = &self.config.airframe.rotors;
        for (rotor, command_radps) in rotors.iter().zip(&mut self.motor_commands_radps) {
            *command_radps = io.read(&rotor.port)?;
        }
        self.integrate(context.truth, context.time.dt_s)?;
        Ok(Update::Applied)
    }
}

impl Multirotor {
    fn integrate(&mut self, truth: &mut KinematicState, dt_s: f64) -> Result<()> {
        self.state.read_truth(truth);
        let config = &self.config;
        let airframe = &config.airframe;
        // omega_min is an armed motor's idle speed: it still pushes and
        // twists, continuously with the speeds above it.
        let commands_radps: Vec<f64> = self
            .motor_commands_radps
            .iter()
            .map(|command| command.clamp(airframe.omega_min_radps, airframe.omega_max_radps))
            .collect();
        if self.rotor_speeds_radps.is_empty() {
            self.rotor_speeds_radps = commands_radps.clone();
        }
        let start_speeds_radps = &self.rotor_speeds_radps;
        let coefficients = RotorCoefficients {
            thrust_coefficient: airframe.thrust_coefficient,
            torque_coefficient: airframe.torque_coefficient,
            aerodynamics: config.aerodynamics,
        };

        // Every load is recomputed from each RK4 stage's state and time, so the
        // step keeps RK4's accuracy.
        let accelerations =
            |elapsed_s: f64, state: &RotorcraftState| -> (Vec3, Vec3) {
                let mut rotor_force_body_n = Vec3::zeros();
                let mut rotor_moment_body_nm = Vec3::zeros();
                for (index, rotor) in airframe.rotors.iter().enumerate() {
                    let speed_radps = config.motor_lag.speed_radps(
                        start_speeds_radps[index],
                        commands_radps[index],
                        elapsed_s,
                    );
                    let loads = rotor_loads(
                        rotor,
                        &coefficients,
                        speed_radps,
                        state.velocity_body_mps,
                        state.rates_body_radps,
                    );
                    rotor_force_body_n += loads.force_body_n;
                    rotor_moment_body_nm += loads.moment_body_nm;
                }
                let weight_body_n =
                    UnitQuaternion::new_normalize(state.attitude).inverse_transform_vector(
                        &Vec3::new(0.0, 0.0, -airframe.mass_kg * airframe.gravity_mps2),
                    );
                let velocity_body_mps = state.velocity_body_mps;
                let drag_body_n = velocity_body_mps
                    * (-0.5 * config.body_drag_coefficient * velocity_body_mps.norm());
                // Not modeled, each a zero in the sums below. Ground effect adds
                // thrust within about a rotor diameter of the ground; it needs height
                // over rotor radius and a measured or published correction. Downwash
                // between rotors and onto the body needs a wake model or measured data.
                // A rotor's gyroscopic torque (J_r w x body rates) and its spin-up
                // reaction (J_r dw/dt) need the rotor's inertia J_r.
                let ground_effect_body_n = Vec3::zeros();
                let downwash_body_n = Vec3::zeros();
                let rotor_gyroscopic_body_nm = Vec3::zeros();
                let spin_up_reaction_body_nm = Vec3::zeros();
                let force_body_n = rotor_force_body_n
                    + weight_body_n
                    + drag_body_n
                    + ground_effect_body_n
                    + downwash_body_n;
                let moment_body_nm =
                    rotor_moment_body_nm + rotor_gyroscopic_body_nm + spin_up_reaction_body_nm;
                let rates = state.rates_body_radps;
                (
                    force_body_n / airframe.mass_kg,
                    config.inverse_inertia
                        * (moment_body_nm - rates.cross(&(airframe.inertia_kgm2 * rates))),
                )
            };
        let mut coordinates = self.state.coordinates();
        math::rk4_timed(&mut coordinates, dt_s, |elapsed_s, coordinates| {
            let stage = RotorcraftState::from_coordinates(*coordinates);
            let (acceleration_body_mps2, angular_acceleration_body_radps2) =
                accelerations(elapsed_s, &stage);
            derivative(
                stage,
                acceleration_body_mps2,
                angular_acceleration_body_radps2,
            )
        });
        ensure!(
            coordinates.iter().all(|value| value.is_finite()),
            "Multirotor integration produced nonfinite state"
        );
        for (speed_radps, command_radps) in self.rotor_speeds_radps.iter_mut().zip(&commands_radps)
        {
            *speed_radps = config
                .motor_lag
                .speed_radps(*speed_radps, *command_radps, dt_s);
        }
        self.state = RotorcraftState::from_coordinates(coordinates);
        self.state.write_truth(truth);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Result;

    use super::Multirotor;
    use crate::Params;
    use crate::math::{EulerAngles, KinematicState, Quaternion, Vec3};
    use crate::plugin::{Plugin, PluginParams};

    fn multirotor(params: &[(&str, &str)]) -> Result<Multirotor> {
        let params: Params = params
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        Ok(Multirotor::new(&Multirotor::configure(
            &PluginParams::text(&params),
        )?))
    }

    /// A 1.5 kg test quad.
    fn quad() -> Result<Multirotor> {
        multirotor(&[
            ("mass", "1.5"),
            ("c_T", "5.45e-6"),
            ("c_Q", "2.284e-7"),
            ("omega_max", "1200"),
            // Idle rotors push nothing unless a test sets omega_min.
            ("omega_min", "0"),
            // Unit inertia keeps the expected rates equal to the applied torques.
            ("inertia_matrix", "[1 0 0] [0 1 0] [0 0 1]"),
            (
                "rotor_config",
                "[CCW 0 -0.175 0 0 0 0] [CCW 0 0.175 0 0 0 0] [CW 0.175 0 0 0 0 0] [CW -0.175 0 0 0 0 0]",
            ),
        ])
    }

    #[test]
    fn balanced_rotors_hover() -> Result<()> {
        let mut model = quad()?;
        let airframe = &model.config.airframe;
        let hover_radps =
            (airframe.mass_kg * airframe.gravity_mps2 / (4.0 * airframe.thrust_coefficient)).sqrt();
        model.motor_commands_radps.fill(hover_radps);
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
    fn minimum_speed_keeps_idle_thrust_and_torque() -> Result<()> {
        let mut model = quad()?;
        model.config.airframe.rotors.truncate(1);
        model.config.airframe.omega_min_radps = 346.41;
        model.motor_commands_radps = vec![-100.0]; // Clamps to omega_min.
        let airframe = model.config.airframe.clone();
        let idle_speed_squared = airframe.omega_min_radps.powi(2);
        let mut truth = KinematicState::default();
        model.integrate(&mut truth, 0.01)?;
        let thrust_acceleration =
            airframe.thrust_coefficient * idle_speed_squared / airframe.mass_kg;
        assert!((truth.velocity_world_mps.z - (thrust_acceleration - 9.81) * 0.01).abs() < 1e-12);
        let expected_yaw_rate = -airframe.torque_coefficient * idle_speed_squared * 0.01;
        assert!((truth.angular_velocity_world_radps.z - expected_yaw_rate).abs() < 1e-12);
        Ok(())
    }

    #[test]
    fn maximum_speed_clamps_and_cw_torque_is_positive() -> Result<()> {
        let mut high = quad()?;
        let mut limit = quad()?;
        high.motor_commands_radps = vec![0.0, 0.0, 5000.0, 5000.0];
        limit.motor_commands_radps = vec![0.0, 0.0, 1200.0, 1200.0];
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
        model.motor_commands_radps.fill(850.0);
        let attitude = Quaternion::from_euler(EulerAngles {
            roll_world_from_body_rad: 0.1,
            pitch_world_from_body_rad: 0.2,
            yaw_world_from_body_rad: 0.6,
        });
        let mut truth = KinematicState {
            orientation_world_from_body: attitude,
            ..KinematicState::default()
        };
        let airframe = &model.config.airframe;
        let acceleration = attitude.rotate_body_to_world(Vec3::new(
            0.0,
            0.0,
            4.0 * airframe.thrust_coefficient * 850.0_f64.powi(2) / airframe.mass_kg,
        )) + Vec3::new(0.0, 0.0, -9.81);
        model.integrate(&mut truth, 0.001)?;
        assert!((truth.velocity_world_mps - acceleration * 0.001).norm() < 1e-12);
        Ok(())
    }

    #[test]
    fn initial_world_velocity_moves_the_vehicle() -> Result<()> {
        let mut model = quad()?;
        model.config.airframe.gravity_mps2 = 0.0;
        let mut truth = KinematicState {
            orientation_world_from_body: Quaternion::from_euler(EulerAngles {
                yaw_world_from_body_rad: 0.7,
                ..EulerAngles::default()
            }),
            velocity_world_mps: Vec3::new(4.0, 5.0, 6.0),
            ..KinematicState::default()
        };
        model.integrate(&mut truth, 0.01)?;
        assert!((truth.position_world_m - Vec3::new(0.04, 0.05, 0.06)).norm() < 1e-12);
        assert!((truth.velocity_world_mps - Vec3::new(4.0, 5.0, 6.0)).norm() < 1e-12);
        Ok(())
    }

    #[test]
    fn drag_is_evaluated_at_every_rk4_stage() -> Result<()> {
        let mut model = quad()?;
        model.config.airframe.gravity_mps2 = 0.0;
        model.config.body_drag_coefficient = 0.3;
        let mut truth = KinematicState {
            velocity_world_mps: Vec3::new(10.0, 0.0, 0.0),
            ..KinematicState::default()
        };
        model.integrate(&mut truth, 0.1)?;
        // Exact solution of m dv/dt = -0.5 c v^2; frozen drag gives 9.0 and 0.95.
        let rate = 0.5 * 0.3 * 10.0 / model.config.airframe.mass_kg;
        let velocity = 10.0 / (1.0 + rate * 0.1);
        let distance = 10.0 / rate * (1.0 + rate * 0.1).ln();
        assert!((truth.velocity_world_mps.x - velocity).abs() < 1e-5);
        assert!((truth.position_world_m.x - distance).abs() < 1e-5);
        Ok(())
    }

    #[test]
    fn rotor_moments_are_offset_cross_tilted_thrust() -> Result<()> {
        // A rotor 0.1 m forward and 0.2 m up, tilted 90 degrees in pitch,
        // pushes along body +x. Only r x F gives it a pitch moment
        // (0.2 m * F), and the reaction torque turns with the axis.
        let mut model = multirotor(&[
            ("mass", "1.5"),
            ("gravity_magnitude", "0"),
            ("c_T", "5.45e-6"),
            ("c_Q", "2.284e-7"),
            ("omega_min", "0"),
            ("omega_max", "1200"),
            ("inertia_matrix", "[1 0 0] [0 1 0] [0 0 1]"),
            ("rotor_config", "[CW 0.1 0 0.2 0 90 0]"),
        ])?;
        model.motor_commands_radps = vec![800.0];
        let thrust_n = model.config.airframe.thrust_coefficient * 800.0_f64.powi(2);
        let reaction_nm = model.config.airframe.torque_coefficient * 800.0_f64.powi(2);
        let mut truth = KinematicState::default();
        model.integrate(&mut truth, 1e-6)?;
        // Unit inertia: body rates after a tiny step are moment * dt.
        let expected_moment = Vec3::new(reaction_nm, 0.2 * thrust_n, 0.0);
        assert!((truth.angular_velocity_world_radps / 1e-6 - expected_moment).norm() < 1e-9);
        Ok(())
    }

    #[test]
    fn untilted_rotor_height_does_not_change_moments() -> Result<()> {
        let flat = super::airframe::parse_rotors("[CW 0.1 0.2 0 0 0 0]")?;
        let raised = super::airframe::parse_rotors("[CW 0.1 0.2 0.5 0 0 0]")?;
        let thrust = flat[0].thrust_axis_body * 3.0;
        assert_eq!(
            flat[0].offset_body_m.cross(&thrust),
            raised[0].offset_body_m.cross(&thrust)
        );
        Ok(())
    }

    #[test]
    fn motor_lag_reaches_the_rotors_filter_value_each_step() -> Result<()> {
        let mut model = quad()?;
        model.config.motor_lag.time_constant_up_s = 1.0 / 80.0;
        model.config.motor_lag.time_constant_down_s = 1.0 / 40.0;
        let mut truth = KinematicState {
            position_world_m: Vec3::new(0.0, 0.0, 100.0),
            ..KinematicState::default()
        };
        model.motor_commands_radps.fill(500.0);
        model.integrate(&mut truth, 0.002)?;
        assert_eq!(model.rotor_speeds_radps, vec![500.0; 4]);
        model.motor_commands_radps.fill(900.0);
        model.integrate(&mut truth, 0.002)?;
        let alpha_up = (-0.002_f64 / (1.0 / 80.0)).exp();
        assert_eq!(
            model.rotor_speeds_radps[0],
            alpha_up * 500.0 + (1.0 - alpha_up) * 900.0
        );
        let spun_up = model.rotor_speeds_radps[0];
        model.motor_commands_radps.fill(100.0);
        model.integrate(&mut truth, 0.002)?;
        let alpha_down = (-0.002_f64 / (1.0 / 40.0)).exp();
        assert_eq!(
            model.rotor_speeds_radps[0],
            alpha_down * spun_up + (1.0 - alpha_down) * 100.0
        );
        Ok(())
    }

    #[test]
    fn lagging_motor_climbs_like_its_integrated_thrust() -> Result<()> {
        // Level vehicle, all motors stepped from 0 to w_c with lag tau: the
        // vertical velocity after dt is the integral of c_T w(t)^2 / m - g,
        // with w(t) = w_c (1 - exp(-t / tau)), done here in closed form.
        let mut model = quad()?;
        let tau_s = 0.05;
        model.config.motor_lag.time_constant_up_s = tau_s;
        model.rotor_speeds_radps = vec![0.0; 4];
        let command_radps = 1000.0;
        model.motor_commands_radps.fill(command_radps);
        let dt_s = 0.001;
        let mut truth = KinematicState::default();
        model.integrate(&mut truth, dt_s)?;
        let decay = (-dt_s / tau_s).exp();
        let mean_square = command_radps.powi(2)
            * (dt_s - 2.0 * tau_s * (1.0 - decay) + 0.5 * tau_s * (1.0 - decay * decay));
        let airframe = &model.config.airframe;
        let expected =
            4.0 * airframe.thrust_coefficient * mean_square / airframe.mass_kg - 9.81 * dt_s;
        assert!((truth.velocity_world_mps.z - expected).abs() < 1e-9);
        Ok(())
    }

    #[test]
    fn rotor_drag_slows_a_hovering_crazyflie_like_forsters_model() -> Result<()> {
        // Förster's simple model: f_a = K_aero * (sum of rotor speeds) * v.
        // A level, hovering Crazyflie moving at 1 m/s along x feels
        // -9.1785e-7 * 4 w_h * 1 N; RotorS's per-rotor form sums to the same.
        let mut model = multirotor(&[
            ("airframe", "crazyflie2"),
            ("rotor_drag_coefficient", "9.1785e-7"),
            ("rotor_drag_coefficient_axial", "10.311e-7"),
        ])?;
        let airframe = model.config.airframe.clone();
        let hover_radps =
            (airframe.mass_kg * airframe.gravity_mps2 / (4.0 * airframe.thrust_coefficient)).sqrt();
        model.motor_commands_radps.fill(hover_radps);
        let mut truth = KinematicState {
            velocity_world_mps: Vec3::new(1.0, 0.0, 0.0),
            ..KinematicState::default()
        };
        model.integrate(&mut truth, 1e-5)?;
        let drag_n = -9.1785e-7 * 4.0 * hover_radps * 1.0;
        let expected = 1.0 + drag_n / airframe.mass_kg * 1e-5;
        assert!((truth.velocity_world_mps.x - expected).abs() < 1e-9);
        Ok(())
    }

    #[test]
    fn hub_velocity_includes_body_rotation() -> Result<()> {
        // Yawing at r with no translation, a hub at +x moves along +y at
        // r * x, so rotor drag pushes it back along -y, and r x F about z
        // slows the yaw.
        let mut model = quad()?;
        model.config.airframe.gravity_mps2 = 0.0;
        model.config.airframe.rotors.truncate(1);
        model.config.airframe.rotors[0].offset_body_m = Vec3::new(0.2, 0.0, 0.0);
        model.config.airframe.torque_coefficient = 0.0;
        model.config.aerodynamics.drag_coefficient = 1e-4;
        model.motor_commands_radps = vec![1000.0];
        let mut truth = KinematicState {
            angular_velocity_world_radps: Vec3::new(0.0, 0.0, 2.0),
            ..KinematicState::default()
        };
        model.integrate(&mut truth, 1e-6)?;
        let drag_y_n = -1000.0 * 1e-4 * (2.0 * 0.2);
        let yaw_moment_nm = 0.2 * drag_y_n;
        let yaw_rate = 2.0 + yaw_moment_nm * 1e-6;
        assert!((truth.angular_velocity_world_radps.z - yaw_rate).abs() < 1e-9);
        Ok(())
    }
}

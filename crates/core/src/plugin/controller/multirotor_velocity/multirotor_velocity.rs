//! Rust-only cascaded flight controller for Multirotor: velocity, attitude,
//! body rate, then motor mixing.
//! Controller phase: read the ENU velocity command (`velocity_x`, `velocity_y`,
//! `velocity_z`, the same ports SingleIntegrator's controller reads) and own-
//! state belief; write motor shaft speeds `motor_0`, `motor_1`, ... in rad/s.
//!
//! The loops follow the usual quadrotor cascade (as in PX4 and the Crazyflie
//! firmware), written in SI units with the airframe's mass and inertia so the
//! gains are bandwidths:
//!
//! 1. Velocity: a = Kv (v_cmd - v) + Ki * integral, tilt-limited; the thrust
//!    vector is f = m (a + g z_world).
//! 2. Attitude: body z along f, heading held at the first belief's yaw; the
//!    collective thrust is f along the current body z, and the rate setpoint
//!    is -Ka * e_R, the geometric attitude error of Lee et al. (2010).
//! 3. Rate: torque = J Kr (w_sp - w) + w x J w.
//! 4. Mixing: per-rotor thrusts from the pseudo-inverse of the airframe's
//!    allocation (rotor axes, r x n, and c_Q / c_T), clamped to the motor
//!    range, then w = sqrt(thrust / c_T).
//!
//! It holds its own copy of the airframe (`airframe` and overrides, as for
//! Multirotor); set the two apart to study model error. A velocity command
//! is a rate of travel, not a place: holding position is the autonomy's job.
//! Not modeled: position hold, yaw commands, motor-saturation priorities
//! (thrusts are clamped one by one), and any knowledge of motor lag.

use anyhow::{Context, Result, ensure};
use nalgebra::{DMatrix, Matrix3, UnitQuaternion};
use serde::Deserialize;

use crate::math::{self, Vec3};
use crate::plugin::motion::{Airframe, AirframeName, AirframeOverrides, RotorParams};
use crate::plugin::{AgentContext, Controller, Frame, PluginIo, Port, Ports, Unit};

const VELOCITY_PORTS: [&str; 3] = ["velocity_x", "velocity_y", "velocity_z"];

/// Mission parameters. The vehicle keys follow Multirotor's rules: name
/// `airframe`, or give every vehicle number. Each gain key overrides one
/// default gain.
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct MultirotorVelocityParams {
    /// `crazyflie2`, `firefly`, or absent for a vehicle given in full, as for
    /// Multirotor. It also picks the default gains.
    airframe: Option<AirframeName>,
    mass_kg: Option<f64>,
    gravity_mps2: Option<f64>,
    inertia_kgm2: Option<[[f64; 3]; 3]>,
    thrust_coefficient: Option<f64>,
    torque_coefficient: Option<f64>,
    min_rotor_speed_radps: Option<f64>,
    max_rotor_speed_radps: Option<f64>,
    rotors: Option<Vec<RotorParams>>,
    /// Velocity proportional gain, 1/s.
    velocity_kp: Option<f64>,
    /// Velocity integral gain, 1/s^2.
    velocity_ki: Option<f64>,
    /// Largest acceleration the integral may add, m/s^2.
    velocity_integral_limit_mps2: Option<f64>,
    /// Attitude gain, 1/s.
    attitude_kp: Option<f64>,
    /// Body-rate gain, 1/s.
    body_rate_kp: Option<f64>,
    /// Largest tilt the velocity loop asks for, degrees.
    max_tilt_deg: Option<f64>,
    /// Largest vertical acceleration the velocity loop asks for, m/s^2.
    max_vertical_acceleration_mps2: Option<f64>,
}

/// Loop gains. Units make them bandwidths: each loop closes on its own
/// time scale, the inner ones faster.
#[derive(Clone, Copy, Debug)]
struct Gains {
    velocity_per_s: f64,
    velocity_integral_per_s2: f64,
    velocity_integral_limit_mps2: f64,
    attitude_per_s: f64,
    rate_per_s: f64,
    max_tilt_rad: f64,
    max_vertical_acceleration_mps2: f64,
}

/// Default gains for an airframe, each chosen by step-response tuning
/// (`reference/multirotor_control.py`, evidence in paper/data); a vehicle
/// given in full starts from the Crazyflie's. Retune them for another
/// vehicle or a longer motor lag.
fn default_gains(airframe: Option<AirframeName>) -> Gains {
    match airframe {
        // Flown with Förster's 65 ms motor lag. The lag adds a pole at 15
        // rad/s, and with no derivative term the attitude poles must sum to
        // 1 / tau; rate and attitude gains place all three near -5 rad/s.
        Some(AirframeName::Crazyflie2) | None => Gains {
            velocity_per_s: 1.2,
            velocity_integral_per_s2: 0.36,
            velocity_integral_limit_mps2: 2.0,
            attitude_per_s: 1.7,
            rate_per_s: 5.1,
            max_tilt_rad: math::deg_to_rad(20.0),
            max_vertical_acceleration_mps2: 4.0,
        },
        // Flown with RotorS's 12.5 ms / 25 ms motor lag. RotorS's Lee gains
        // for the Firefly (`lee_controller_firefly.yaml`) in this
        // controller's form: velocity 4.7 / m = 3.0 /s, attitude K_R / K_w =
        // 5.8 /s, rate K_w / J = 15 /s about x and 11 /s about y (one rate
        // gain here, so between them). Lee has no integral; 0.6 /s^2 removes
        // the rotor drag's steady error with 5-7% overshoot, where 2.25
        // (Crazyflie's Ki / Kv^2) gave 23%.
        Some(AirframeName::Firefly) => Gains {
            velocity_per_s: 3.0,
            velocity_integral_per_s2: 0.6,
            velocity_integral_limit_mps2: 2.0,
            attitude_per_s: 5.8,
            rate_per_s: 13.0,
            max_tilt_rad: math::deg_to_rad(20.0),
            max_vertical_acceleration_mps2: 4.0,
        },
    }
}

#[derive(Clone, Deserialize)]
#[serde(try_from = "MultirotorVelocityParams")]
pub struct MultirotorVelocityConfig {
    airframe: Airframe,
    gains: Gains,
    /// Per-rotor thrust (N) from [collective thrust along body z (N),
    /// body moment (N m)].
    thrust_from_wrench: DMatrix<f64>,
}

pub struct MultirotorVelocityController {
    config: MultirotorVelocityConfig,
    velocity_error_integral_world_m: Vec3,
    /// Heading held throughout; taken from the first belief.
    yaw_world_from_body_rad: Option<f64>,
}

// The mission's parameters become the config through serde's `try_from`, so
// the trait's default `configure` (`params.parse()`) fits this plugin too.
impl TryFrom<MultirotorVelocityParams> for MultirotorVelocityConfig {
    type Error = anyhow::Error;
    fn try_from(params: MultirotorVelocityParams) -> Result<Self> {
        let airframe = Airframe::new(
            params.airframe,
            AirframeOverrides {
                mass_kg: params.mass_kg,
                gravity_mps2: params.gravity_mps2,
                inertia_kgm2: params.inertia_kgm2,
                thrust_coefficient: params.thrust_coefficient,
                torque_coefficient: params.torque_coefficient,
                min_rotor_speed_radps: params.min_rotor_speed_radps,
                max_rotor_speed_radps: params.max_rotor_speed_radps,
                rotors: params.rotors,
            },
        )?;
        ensure!(
            airframe.thrust_coefficient > 0.0,
            "MultirotorVelocityController requires thrust_coefficient > 0"
        );
        let defaults = default_gains(params.airframe);
        let gains = Gains {
            velocity_per_s: params.velocity_kp.unwrap_or(defaults.velocity_per_s),
            velocity_integral_per_s2: params
                .velocity_ki
                .unwrap_or(defaults.velocity_integral_per_s2),
            velocity_integral_limit_mps2: params
                .velocity_integral_limit_mps2
                .unwrap_or(defaults.velocity_integral_limit_mps2),
            attitude_per_s: params.attitude_kp.unwrap_or(defaults.attitude_per_s),
            rate_per_s: params.body_rate_kp.unwrap_or(defaults.rate_per_s),
            max_tilt_rad: params
                .max_tilt_deg
                .map(math::deg_to_rad)
                .unwrap_or(defaults.max_tilt_rad),
            max_vertical_acceleration_mps2: params
                .max_vertical_acceleration_mps2
                .unwrap_or(defaults.max_vertical_acceleration_mps2),
        };
        ensure!(
            gains.velocity_per_s >= 0.0
                && gains.velocity_integral_per_s2 >= 0.0
                && gains.velocity_integral_limit_mps2 >= 0.0
                && gains.attitude_per_s >= 0.0
                && gains.rate_per_s >= 0.0
                && gains.max_vertical_acceleration_mps2 >= 0.0,
            "MultirotorVelocityController gains and limits must be nonnegative"
        );
        ensure!(
            gains.max_tilt_rad > 0.0 && gains.max_tilt_rad < std::f64::consts::FRAC_PI_2,
            "MultirotorVelocityController max_tilt_deg must be between 0 and 90 degrees"
        );
        let thrust_from_wrench = allocation_inverse(&airframe)?;
        Ok(MultirotorVelocityConfig {
            airframe,
            gains,
            thrust_from_wrench,
        })
    }
}

impl Controller for MultirotorVelocityController {
    type Config = MultirotorVelocityConfig;

    fn new(config: &Self::Config) -> Self {
        Self {
            config: config.clone(),
            velocity_error_integral_world_m: Vec3::zeros(),
            yaw_world_from_body_rad: None,
        }
    }

    fn ports(config: &Self::Config) -> Ports {
        let mut ports = Ports::default();
        for name in VELOCITY_PORTS {
            ports = ports.input(Port::new(name, Unit::MetersPerSecond, Frame::World));
        }
        for rotor in &config.airframe.rotors {
            ports = ports.output(Port::new(&rotor.port, Unit::RadiansPerSecond, Frame::None));
        }
        ports
    }

    fn step(&mut self, context: &mut AgentContext, io: &mut PluginIo) -> Result<()> {
        let dt_s = context.time.dt_s;
        let belief = context.state;
        let airframe = &self.config.airframe;
        let gains = self.config.gains;
        let attitude = belief.orientation_world_from_body;
        let rotation_world_from_body = UnitQuaternion::new_normalize(nalgebra::Quaternion::new(
            attitude.w, attitude.x, attitude.y, attitude.z,
        ))
        .to_rotation_matrix();
        let yaw_rad = *self
            .yaw_world_from_body_rad
            .get_or_insert_with(|| attitude.yaw_world_from_body_rad());

        // 1. Velocity loop, in the world frame.
        let command_world_mps = Vec3::new(
            io.read(VELOCITY_PORTS[0])?,
            io.read(VELOCITY_PORTS[1])?,
            io.read(VELOCITY_PORTS[2])?,
        );
        let velocity_error_world_mps = command_world_mps - belief.velocity_world_mps;
        self.velocity_error_integral_world_m += velocity_error_world_mps * dt_s;
        let mut integral_mps2 =
            self.velocity_error_integral_world_m * gains.velocity_integral_per_s2;
        // Anti-windup: hold the integral's contribution inside its limit.
        if integral_mps2.norm() > gains.velocity_integral_limit_mps2 {
            integral_mps2 *= gains.velocity_integral_limit_mps2 / integral_mps2.norm();
            if gains.velocity_integral_per_s2 > 0.0 {
                self.velocity_error_integral_world_m =
                    integral_mps2 / gains.velocity_integral_per_s2;
            }
        }
        let mut acceleration_world_mps2 =
            velocity_error_world_mps * gains.velocity_per_s + integral_mps2;
        acceleration_world_mps2.z = acceleration_world_mps2.z.clamp(
            -gains.max_vertical_acceleration_mps2,
            gains.max_vertical_acceleration_mps2,
        );
        // Tilt limit: the horizontal part may not lean the thrust past
        // max_tilt for this vertical part.
        let lift_mps2 = acceleration_world_mps2.z + airframe.gravity_mps2;
        let max_horizontal_mps2 = lift_mps2.max(0.0) * gains.max_tilt_rad.tan();
        let horizontal_mps2 = acceleration_world_mps2.xy().norm();
        if horizontal_mps2 > max_horizontal_mps2 {
            let scale = max_horizontal_mps2 / horizontal_mps2;
            acceleration_world_mps2.x *= scale;
            acceleration_world_mps2.y *= scale;
        }
        let thrust_world_n = (acceleration_world_mps2 + Vec3::new(0.0, 0.0, airframe.gravity_mps2))
            * airframe.mass_kg;

        // 2. Attitude: body z along the thrust vector, heading held.
        let desired = desired_attitude(thrust_world_n, yaw_rad);
        let body_z_world = rotation_world_from_body * Vec3::z();
        let collective_thrust_n = thrust_world_n.dot(&body_z_world).max(0.0);
        let attitude_error = 0.5
            * vee(desired.transpose() * rotation_world_from_body.matrix()
                - rotation_world_from_body.matrix().transpose() * desired);
        let rate_setpoint_body_radps = -attitude_error * gains.attitude_per_s;

        // 3. Body rates.
        let rates_body_radps =
            rotation_world_from_body.inverse() * belief.angular_velocity_world_radps;
        let inertia = airframe.inertia_kgm2;
        let angular_acceleration_body_radps2 =
            (rate_setpoint_body_radps - rates_body_radps) * gains.rate_per_s;
        let moment_body_nm = inertia * angular_acceleration_body_radps2
            + rates_body_radps.cross(&(inertia * rates_body_radps));

        // 4. Mixing: per-rotor thrust, clamped to the motor range, to speed.
        let wrench = nalgebra::DVector::from_column_slice(&[
            collective_thrust_n,
            moment_body_nm.x,
            moment_body_nm.y,
            moment_body_nm.z,
        ]);
        let rotor_thrusts_n = &self.config.thrust_from_wrench * wrench;
        let min_thrust_n = airframe.thrust_coefficient * airframe.min_rotor_speed_radps.powi(2);
        let max_thrust_n = airframe.thrust_coefficient * airframe.max_rotor_speed_radps.powi(2);
        for (rotor, thrust_n) in airframe.rotors.iter().zip(rotor_thrusts_n.iter()) {
            let thrust_n = thrust_n.clamp(min_thrust_n, max_thrust_n);
            io.write(&rotor.port, (thrust_n / airframe.thrust_coefficient).sqrt())?;
        }
        Ok(())
    }
}

/// World-from-body rotation with body z along `thrust_world_n` and body x as
/// close to heading `yaw_rad` as that allows.
fn desired_attitude(thrust_world_n: Vec3, yaw_rad: f64) -> Matrix3<f64> {
    let body_z = if thrust_world_n.norm() > 0.0 {
        thrust_world_n.normalize()
    } else {
        Vec3::z()
    };
    let heading = Vec3::new(yaw_rad.cos(), yaw_rad.sin(), 0.0);
    let body_y = body_z.cross(&heading).normalize();
    let body_x = body_y.cross(&body_z);
    Matrix3::from_columns(&[body_x, body_y, body_z])
}

/// The vector of a skew-symmetric matrix.
fn vee(skew: Matrix3<f64>) -> Vec3 {
    Vec3::new(skew[(2, 1)], skew[(0, 2)], skew[(1, 0)])
}

/// Inverse of the map from per-rotor thrust to [collective thrust along body
/// z, body moment]: each rotor pushes along its axis at its hub and twists
/// about its axis by c_Q / c_T per newton.
fn allocation_inverse(airframe: &Airframe) -> Result<DMatrix<f64>> {
    let torque_per_thrust_m = airframe.torque_coefficient / airframe.thrust_coefficient;
    let mut wrench_from_thrust = DMatrix::zeros(4, airframe.rotors.len());
    for (column, rotor) in airframe.rotors.iter().enumerate() {
        let axis = rotor.thrust_axis_body;
        let moment = rotor.offset_body_m.cross(&axis)
            + axis * (rotor.direction.reaction_sign() * torque_per_thrust_m);
        wrench_from_thrust[(0, column)] = axis.z;
        wrench_from_thrust[(1, column)] = moment.x;
        wrench_from_thrust[(2, column)] = moment.y;
        wrench_from_thrust[(3, column)] = moment.z;
    }
    let rank = wrench_from_thrust.rank(1e-12);
    ensure!(
        rank == 4,
        "MultirotorVelocityController needs rotors that can produce thrust, roll, pitch, and yaw"
    );
    wrench_from_thrust
        .pseudo_inverse(1e-12)
        .ok()
        .context("MultirotorVelocityController could not invert the rotor allocation")
}

#[cfg(test)]
mod tests {
    use anyhow::Result;

    use nalgebra::{Matrix3, Rotation3};

    use super::{MultirotorVelocityController, desired_attitude, vee};
    use crate::math::Vec3;
    use crate::plugin::{Controller, PluginParams};

    fn configure(params: &[(&str, &str)]) -> Result<super::MultirotorVelocityConfig> {
        MultirotorVelocityController::configure(&PluginParams::pairs(params))
    }

    /// A vehicle given in full, with this rotor layout.
    fn custom_quad(rotors: &str) -> Vec<(&str, &str)> {
        vec![
            ("mass_kg", "1.5"),
            (
                "inertia_kgm2",
                "[[0.0122, 0, 0], [0, 0.0122, 0], [0, 0, 0.0244]]",
            ),
            ("thrust_coefficient", "5.45e-6"),
            ("torque_coefficient", "2.284e-7"),
            ("min_rotor_speed_radps", "0"),
            ("max_rotor_speed_radps", "1200"),
            ("rotors", rotors),
        ]
    }

    fn rotation_from_euler(roll_rad: f64, pitch_rad: f64, yaw_rad: f64) -> Matrix3<f64> {
        *Rotation3::from_euler_angles(roll_rad, pitch_rad, yaw_rad).matrix()
    }

    #[test]
    fn allocation_splits_hover_thrust_evenly_and_maps_moments_back() -> Result<()> {
        let custom = custom_quad(
            "[{direction: ccw, position_flu_m: [0, -0.175, 0]}, {direction: ccw, position_flu_m: [0, 0.175, 0]}, \
             {direction: cw, position_flu_m: [0.175, 0, 0]}, {direction: cw, position_flu_m: [-0.175, 0, 0]}]",
        );
        for vehicle in [vec![("airframe", "crazyflie2")], custom] {
            let config = configure(&vehicle)?;
            let weight_n = config.airframe.mass_kg * config.airframe.gravity_mps2;
            let hover = &config.thrust_from_wrench
                * nalgebra::DVector::from_column_slice(&[weight_n, 0.0, 0.0, 0.0]);
            for thrust_n in hover.iter() {
                assert!((thrust_n - weight_n / 4.0).abs() < 1e-12 * weight_n);
            }
            // A roll moment: rotor thrusts whose r x F sums back to it.
            let wrench = nalgebra::DVector::from_column_slice(&[weight_n, 1e-3, -2e-3, 5e-5]);
            let thrusts = &config.thrust_from_wrench * &wrench;
            let ratio = config.airframe.torque_coefficient / config.airframe.thrust_coefficient;
            let mut moment = Vec3::zeros();
            for (rotor, thrust_n) in config.airframe.rotors.iter().zip(thrusts.iter()) {
                let force = rotor.thrust_axis_body * *thrust_n;
                moment += rotor.offset_body_m.cross(&force)
                    + rotor.thrust_axis_body * (rotor.direction.reaction_sign() * ratio * thrust_n);
            }
            assert!((moment - Vec3::new(1e-3, -2e-3, 5e-5)).norm() < 1e-12);
        }
        Ok(())
    }

    #[test]
    fn level_thrust_with_matching_heading_has_no_attitude_error() {
        let desired = desired_attitude(Vec3::new(0.0, 0.0, 10.0), 0.4);
        let current = rotation_from_euler(0.0, 0.0, 0.4);
        let error = 0.5 * vee(desired.transpose() * current - current.transpose() * desired);
        assert!(error.norm() < 1e-12);
    }

    #[test]
    fn small_roll_gives_a_proportional_roll_error() {
        // Rolled right by 0.01 rad while level is wanted: the error is +0.01
        // about body x, so the rate setpoint -Ka e rolls back.
        let desired = desired_attitude(Vec3::new(0.0, 0.0, 10.0), 0.0);
        let current = rotation_from_euler(0.01, 0.0, 0.0);
        let error = 0.5 * vee(desired.transpose() * current - current.transpose() * desired);
        assert!((error - Vec3::new(0.01_f64.sin(), 0.0, 0.0)).norm() < 1e-12);
    }

    #[test]
    fn thrust_toward_east_tilts_body_z_east() {
        let desired = desired_attitude(Vec3::new(1.0, 0.0, 10.0), 0.0);
        let body_z = desired.column(2);
        assert!(body_z.x > 0.0 && body_z.y.abs() < 1e-15);
        // Pitching toward +x is a positive pitch about body y here.
        assert!((body_z.x / body_z.z - 0.1).abs() < 1e-12);
    }

    #[test]
    fn rotors_that_cannot_yaw_are_rejected() {
        // Two coaxial-looking rotors on one axis cannot make all four axes.
        let result = configure(&custom_quad(
            "[{direction: cw, position_flu_m: [0.1, 0, 0]}, {direction: ccw, position_flu_m: [-0.1, 0, 0]}]",
        ));
        assert!(result.is_err());
    }
}

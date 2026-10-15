//! SimpleAircraft dynamics, limits, and RK4 integration.
//!
//! Derived from SCRIMMAGE: src/plugins/motion/SimpleAircraft/SimpleAircraft.cpp.
//! Motion phase: read throttle and roll and pitch rates, integrate from truth, write truth.
//! The model's state is read from truth at every step, so a change another
//! plugin makes to position, attitude, or speed (an interaction's push)
//! carries into the integration. Velocity direction is not state: the aircraft
//! flies along its nose, so a sideways velocity keeps its speed and turns to
//! the heading.
//!
//! Fidelity
//! - Source: a kinematic aircraft: speed, roll, and pitch follow commands
//!   within rate and range limits, and it turns as a coordinated turn does, at
//!   g tan(bank) / speed. `airframe` names a set of limits fit to a real
//!   aircraft; without it the limits are generic round numbers, not any
//!   aircraft.
//! - Validated against: SCRIMMAGE's SimpleAircraft (the comparison missions, its
//!   generated scenarios, and the long runs), with the same turn law.
//! - Valid range: a stand-in for an aircraft that turns and climbs within its
//!   limits; not a physical model.
//! - Not modeled: forces, aerodynamics, inertia, wind, stall, sideslip
//!   (velocity is always along the nose), and energy (speed and climb are
//!   independent).
//! - To go further: FixedWing6DOF for anything aerodynamic.

use anyhow::{Result, ensure};
use serde::Deserialize;

use crate::{
    math::{self, EulerAngles, KinematicState, Quaternion, Vec3},
    plugin::{
        Frame, MotionContext, MotionModel, Plugin, PluginIo, PluginParams, Port, Ports, Unit,
        Update,
    },
};

/// Gravity, as FixedWing6DOF's.
const GRAVITY_MPS2: f64 = 9.81;

/// Mission parameters, in the mission file's units. `airframe` loads a named
/// aircraft's limits, which any key overrides; without it, a key left out
/// takes the generic value. The angle keys are in degrees.
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct SimpleAircraftParams {
    airframe: Option<SimpleAirframe>,
    min_speed_mps: Option<f64>,
    max_speed_mps: Option<f64>,
    /// Bank limit.
    max_roll_deg: Option<f64>,
    max_pitch_deg: Option<f64>,
    max_roll_rate_degps: Option<f64>,
    max_pitch_rate_degps: Option<f64>,
    max_acceleration_mps2: Option<f64>,
    max_deceleration_mps2: Option<f64>,
    max_climb_rate_mps: Option<f64>,
    max_descent_rate_mps: Option<f64>,
}

/// Named aircraft whose limits SimpleAircraft can load.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SimpleAirframe {
    /// FixedWing6DOF's Rascal as AircraftPIDController flies it.
    Rascal,
}

#[derive(Clone, Debug)]
pub struct AircraftConfig {
    min_speed_mps: f64,
    max_speed_mps: f64,
    max_roll_rad: f64,
    max_pitch_rad: f64,
    max_roll_rate_radps: f64,
    max_pitch_rate_radps: f64,
    max_acceleration_mps2: f64,
    max_deceleration_mps2: f64,
    max_climb_rate_mps: f64,
    max_descent_rate_mps: f64,
}

impl AircraftConfig {
    /// Generic limits: no aircraft in particular. Throttle's
    /// [-100, 100] range at its scale (below) is +-20 m/s^2, and only
    /// the pitch limit bounds climb and descent.
    fn generic() -> Self {
        Self {
            min_speed_mps: 15.0,
            max_speed_mps: 40.0,
            max_roll_rad: math::deg_to_rad(30.0),
            max_pitch_rad: math::deg_to_rad(30.0),
            max_roll_rate_radps: math::deg_to_rad(57.3),
            max_pitch_rate_radps: math::deg_to_rad(57.3),
            max_acceleration_mps2: 20.0,
            max_deceleration_mps2: 20.0,
            max_climb_rate_mps: f64::INFINITY,
            max_descent_rate_mps: f64::INFINITY,
        }
    }

    fn airframe(airframe: SimpleAirframe) -> Self {
        match airframe {
            // Fit to FixedWing6DOF's Rascal under AircraftPIDController (both
            // `airframe: rascal`) by `reference/simple_aircraft_fit.py`
            // (evidence in paper/data/simple-aircraft-fit), rounded to 0.1:
            // the steady level speed at full throttle (38.1 m/s); the slowest
            // steady speed holding altitude, over speeds asked from 7 to 14 m/s
            // (9.0); the largest change of speed over 1 s speeding up and
            // slowing from 20 m/s (2.8, 1.2 m/s^2); steady climb and descent at
            // 20 m/s asked (5.8, 9.7 m/s); the largest roll and pitch rates
            // (31, 52 deg/s). The Rascal holds 47 degrees of bank but turns at
            // 26.3 deg/s, 13% slower than a coordinated turn at that bank; 43
            // degrees is the bank that gives 26.3 deg/s here, and a bank only
            // sets the turn rate here. The pitch limit only has to let climb and
            // descent bind at cruise (asin(9.7 / 20) is 29 degrees). Rerun that
            // script after changing either model or these numbers.
            SimpleAirframe::Rascal => Self {
                min_speed_mps: 9.0,
                max_speed_mps: 38.1,
                max_roll_rad: math::deg_to_rad(43.0),
                max_pitch_rad: math::deg_to_rad(30.0),
                max_roll_rate_radps: math::deg_to_rad(31.0),
                max_pitch_rate_radps: math::deg_to_rad(52.0),
                max_acceleration_mps2: 2.8,
                max_deceleration_mps2: 1.2,
                max_climb_rate_mps: 5.8,
                max_descent_rate_mps: 9.7,
            },
        }
    }

    /// Pitch limits at `speed_mps` (negative is nose up): the attitude limit,
    /// narrowed so that climb and descent, speed x sin(pitch), stay within
    /// their limits.
    #[inline]
    fn pitch_limits_rad(&self, speed_mps: f64) -> (f64, f64) {
        // Without a rate limit there is nothing to narrow, and no asin to pay
        // for in every derivative.
        let limit = |rate_mps: f64| {
            if rate_mps.is_finite() {
                self.max_pitch_rad
                    .min((rate_mps / speed_mps).min(1.0).asin())
            } else {
                self.max_pitch_rad
            }
        };
        (
            -limit(self.max_climb_rate_mps),
            limit(self.max_descent_rate_mps),
        )
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct AircraftControl {
    /// Throttle, in [-100, 100]: it asks for throttle / 5 m/s^2.
    throttle: f64,
    roll_rate_radps: f64,
    pitch_rate_radps: f64,
}

/// Roll, pitch, and yaw are truth's: positive roll is right wing down and turns
/// right, positive pitch is nose down and descends.
#[derive(Clone, Copy, Debug, Default)]
struct AircraftState {
    position_world_m: Vec3,
    roll_world_from_body_rad: f64,
    pitch_world_from_body_rad: f64,
    yaw_world_from_body_rad: f64,
    speed_mps: f64,
}

#[derive(Clone, Debug)]
pub struct SimpleAircraft {
    config: AircraftConfig,
}

impl Plugin for SimpleAircraft {
    type Config = AircraftConfig;

    fn configure(params: &PluginParams<'_>) -> Result<AircraftConfig> {
        let params: SimpleAircraftParams = params.parse()?;
        let base = params
            .airframe
            .map_or_else(AircraftConfig::generic, AircraftConfig::airframe);
        let angle =
            |degrees: Option<f64>, base_rad: f64| degrees.map_or(base_rad, math::deg_to_rad);
        let config = AircraftConfig {
            min_speed_mps: params.min_speed_mps.unwrap_or(base.min_speed_mps),
            max_speed_mps: params.max_speed_mps.unwrap_or(base.max_speed_mps),
            max_roll_rad: angle(params.max_roll_deg, base.max_roll_rad),
            max_pitch_rad: angle(params.max_pitch_deg, base.max_pitch_rad),
            max_roll_rate_radps: angle(params.max_roll_rate_degps, base.max_roll_rate_radps),
            max_pitch_rate_radps: angle(params.max_pitch_rate_degps, base.max_pitch_rate_radps),
            max_acceleration_mps2: params
                .max_acceleration_mps2
                .unwrap_or(base.max_acceleration_mps2),
            max_deceleration_mps2: params
                .max_deceleration_mps2
                .unwrap_or(base.max_deceleration_mps2),
            max_climb_rate_mps: params.max_climb_rate_mps.unwrap_or(base.max_climb_rate_mps),
            max_descent_rate_mps: params
                .max_descent_rate_mps
                .unwrap_or(base.max_descent_rate_mps),
        };

        ensure!(
            config.min_speed_mps > 0.0 && config.min_speed_mps <= config.max_speed_mps,
            "SimpleAircraft needs 0 < min_speed_mps <= max_speed_mps"
        );
        ensure!(
            [
                config.max_roll_rad,
                config.max_pitch_rad,
                config.max_roll_rate_radps,
                config.max_pitch_rate_radps,
                config.max_acceleration_mps2,
                config.max_deceleration_mps2,
                config.max_climb_rate_mps,
                config.max_descent_rate_mps,
            ]
            .iter()
            .all(|&limit| limit >= 0.0),
            "SimpleAircraft limits must be nonnegative"
        );
        ensure!(
            config.max_roll_rad < std::f64::consts::FRAC_PI_2,
            "SimpleAircraft max_roll_deg must be below 90 degrees"
        );
        Ok(config)
    }

    fn new(config: &AircraftConfig) -> Self {
        Self {
            config: config.clone(),
        }
    }

    fn ports(_config: &AircraftConfig) -> Ports {
        Ports::default()
            .input(Port::new("throttle", Unit::Dimensionless, Frame::None))
            .input(Port::new("roll_rate", Unit::RadiansPerSecond, Frame::Model))
            .input(Port::new(
                "pitch_rate",
                Unit::RadiansPerSecond,
                Frame::Model,
            ))
    }
}

impl MotionModel for SimpleAircraft {
    /// Brings the initial truth within the limits, with velocity along the
    /// heading and climb angle, and at rest (no commanded rates yet).
    fn initialize(&mut self, context: &mut MotionContext<'_>) -> Result<()> {
        let state = self.limited(AircraftState::from_truth(context.truth));
        self.write_truth(context.truth, state, AircraftControl::default());
        Ok(())
    }

    fn step(&mut self, context: &mut MotionContext<'_>, io: &mut PluginIo) -> Result<Update> {
        let control = AircraftControl {
            throttle: io.read("throttle")?,
            roll_rate_radps: io.read("roll_rate")?,
            pitch_rate_radps: io.read("pitch_rate")?,
        };
        self.integrate(context.truth, control, context.time.dt_s)?;
        Ok(Update::Applied)
    }
}

impl SimpleAircraft {
    /// Roll, pitch, and speed within their limits.
    fn limited(&self, mut state: AircraftState) -> AircraftState {
        state.roll_world_from_body_rad = state
            .roll_world_from_body_rad
            .clamp(-self.config.max_roll_rad, self.config.max_roll_rad);
        state.speed_mps = state
            .speed_mps
            .clamp(self.config.min_speed_mps, self.config.max_speed_mps);
        let (min_pitch_rad, max_pitch_rad) = self.config.pitch_limits_rad(state.speed_mps);
        state.pitch_world_from_body_rad = state
            .pitch_world_from_body_rad
            .clamp(min_pitch_rad, max_pitch_rad);
        state
    }

    fn integrate(
        &mut self,
        truth: &mut KinematicState,
        control: AircraftControl,
        dt_s: f64,
    ) -> Result<()> {
        let state = self.limited(AircraftState::from_truth(truth));

        let limited_control = AircraftControl {
            throttle: control.throttle.clamp(-100.0, 100.0),
            roll_rate_radps: control.roll_rate_radps.clamp(
                -self.config.max_roll_rate_radps,
                self.config.max_roll_rate_radps,
            ),
            pitch_rate_radps: control.pitch_rate_radps.clamp(
                -self.config.max_pitch_rate_radps,
                self.config.max_pitch_rate_radps,
            ),
        };

        let mut coordinates = state.coordinates();
        math::rk4(&mut coordinates, dt_s, |coordinates| {
            aircraft_derivative(
                AircraftState::from_coordinates(*coordinates),
                limited_control,
                &self.config,
            )
        });
        ensure!(
            coordinates.iter().all(|value| value.is_finite()),
            "SimpleAircraft integration produced nonfinite state"
        );

        // A step that reaches a limit can pass it before the derivative holds;
        // the limits apply again so truth never holds a state beyond them.
        let state = self.limited(AircraftState::from_coordinates(coordinates));
        self.write_truth(truth, state, limited_control);
        Ok(())
    }

    /// Writes `state` to truth with the velocity and angular velocity it has
    /// under `control`: the rates the next step starts with.
    fn write_truth(
        &self,
        truth: &mut KinematicState,
        state: AircraftState,
        control: AircraftControl,
    ) {
        let [
            vx,
            vy,
            vz,
            roll_rate_radps,
            pitch_rate_radps,
            yaw_rate_radps,
            _,
        ] = aircraft_derivative(state, control, &self.config);
        truth.position_world_m = state.position_world_m;
        truth.orientation_world_from_body = state.orientation_world_from_body();
        truth.velocity_world_mps = Vec3::new(vx, vy, vz);
        // Euler rates in world axes, for attitude = yaw * pitch * roll: yaw turns
        // about up, pitch about the yawed y axis, roll about the nose.
        let (sin_yaw, cos_yaw) = state.yaw_world_from_body_rad.sin_cos();
        let (sin_pitch, cos_pitch) = state.pitch_world_from_body_rad.sin_cos();
        truth.angular_velocity_world_radps = Vec3::new(0.0, 0.0, yaw_rate_radps)
            + Vec3::new(-sin_yaw, cos_yaw, 0.0) * pitch_rate_radps
            + Vec3::new(cos_yaw * cos_pitch, sin_yaw * cos_pitch, -sin_pitch) * roll_rate_radps;
    }
}

/// How close to a limit counts as at it: far above the rounding in reading an
/// angle back from a quaternion (about 1e-16 rad), far below any motion.
const AT_LIMIT: f64 = 1e-9;

// DEVNOTE: inlined into the RK4 stages. Not inlined, and with f64::clamp
// below, perf.py's substeps-512 workload (one worker, ten substeps) ran in
// 0.92 s against 0.78 s for the build before the acceleration and climb
// limits; with both changes, 0.79 s. Rerun perf.py after changing this.
#[inline]
fn aircraft_derivative(
    state: AircraftState,
    control: AircraftControl,
    config: &AircraftConfig,
) -> [f64; 7] {
    let horizontal_speed_mps = state.speed_mps * state.pitch_world_from_body_rad.cos();
    // A coordinated turn: the lift's horizontal part, g tan(roll) per unit
    // mass, turns the velocity at g tan(roll) / speed. Banked right (positive
    // roll), it turns right: heading decreases.
    let yaw_rate_world_radps =
        -GRAVITY_MPS2 * state.roll_world_from_body_rad.tan() / state.speed_mps;
    // A value at a limit stays there while the command pushes it outward.
    // "At" allows for rounding: truth stores attitude as a quaternion, and
    // reading an angle back from it can land a bit inside the limit.
    let held = |value: f64, min: f64, max: f64, rate: f64| {
        if (value >= max - AT_LIMIT && rate > 0.0) || (value <= min + AT_LIMIT && rate < 0.0) {
            0.0
        } else {
            rate
        }
    };
    let max_roll_rad = config.max_roll_rad;
    let (min_pitch_rad, max_pitch_rad) = config.pitch_limits_rad(state.speed_mps);
    // The throttle scale, within the acceleration limits.
    // max/min, not f64::clamp, whose check that its bounds are in order
    // (it can panic) costs speed here; see the DEVNOTE above.
    let acceleration_mps2 = (control.throttle / 5.0)
        .max(-config.max_deceleration_mps2)
        .min(config.max_acceleration_mps2);

    [
        horizontal_speed_mps * state.yaw_world_from_body_rad.cos(),
        horizontal_speed_mps * state.yaw_world_from_body_rad.sin(),
        -state.pitch_world_from_body_rad.sin() * state.speed_mps,
        held(
            state.roll_world_from_body_rad,
            -max_roll_rad,
            max_roll_rad,
            control.roll_rate_radps,
        ),
        held(
            state.pitch_world_from_body_rad,
            min_pitch_rad,
            max_pitch_rad,
            control.pitch_rate_radps,
        ),
        yaw_rate_world_radps,
        held(
            state.speed_mps,
            config.min_speed_mps,
            config.max_speed_mps,
            acceleration_mps2,
        ),
    ]
}

impl AircraftState {
    /// The model's state as truth holds it, with speed the speed along the
    /// velocity.
    fn from_truth(truth: &KinematicState) -> Self {
        let orientation = truth.orientation_world_from_body;
        Self {
            position_world_m: truth.position_world_m,
            roll_world_from_body_rad: orientation.roll_world_from_body_rad(),
            pitch_world_from_body_rad: orientation.pitch_world_from_body_rad(),
            yaw_world_from_body_rad: orientation.yaw_world_from_body_rad(),
            speed_mps: math::norm(truth.velocity_world_mps),
        }
    }

    // Numerical integration coordinates are defined only at this conversion boundary.
    fn coordinates(self) -> [f64; 7] {
        [
            self.position_world_m.x,
            self.position_world_m.y,
            self.position_world_m.z,
            self.roll_world_from_body_rad,
            self.pitch_world_from_body_rad,
            self.yaw_world_from_body_rad,
            self.speed_mps,
        ]
    }

    fn from_coordinates(coordinates: [f64; 7]) -> Self {
        let [
            x_world_m,
            y_world_m,
            z_world_m,
            roll_world_from_body_rad,
            pitch_world_from_body_rad,
            yaw_world_from_body_rad,
            speed_mps,
        ] = coordinates;
        Self {
            position_world_m: Vec3::new(x_world_m, y_world_m, z_world_m),
            roll_world_from_body_rad,
            pitch_world_from_body_rad,
            yaw_world_from_body_rad,
            speed_mps,
        }
    }

    fn orientation_world_from_body(self) -> Quaternion {
        Quaternion::from_euler(EulerAngles {
            roll_world_from_body_rad: self.roll_world_from_body_rad,
            pitch_world_from_body_rad: self.pitch_world_from_body_rad,
            yaw_world_from_body_rad: self.yaw_world_from_body_rad,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{AircraftControl, AircraftState, Plugin, PluginParams, SimpleAircraft};
    use super::{MotionModel, aircraft_derivative};
    use crate::math::{EulerAngles, KinematicState, Quaternion, Vec3};
    use crate::plugin::{EntityInfo, Messages, MotionContext, PluginIo, StepTime};

    fn aircraft() -> anyhow::Result<SimpleAircraft> {
        let config = SimpleAircraft::configure(&PluginParams::yaml("{}"))?;
        Ok(SimpleAircraft::new(&config))
    }

    fn context<'a>(
        truth: &'a mut KinematicState,
        messages: &'a mut Messages,
        dt_s: f64,
    ) -> MotionContext<'a> {
        MotionContext {
            entity: EntityInfo {
                id: 1,
                team_id: 1,
                group_index: 0,
            },
            time: StepTime {
                time_s: 0.0,
                dt_s,
                tick: 0,
            },
            truth,
            messages,
            terrain: None,
            applied_forces_world_n: &[],
        }
    }

    fn initialize(aircraft: &mut SimpleAircraft, truth: &mut KinematicState) -> anyhow::Result<()> {
        aircraft.initialize(&mut context(truth, &mut Messages::default(), 0.0))
    }

    /// One motion step with `[throttle, roll_rate, pitch_rate]`.
    fn step(
        aircraft: &mut SimpleAircraft,
        truth: &mut KinematicState,
        [throttle, roll_rate, pitch_rate]: [f64; 3],
        dt_s: f64,
    ) -> anyhow::Result<()> {
        let mut io = PluginIo::new(&SimpleAircraft::ports(&aircraft.config));
        io.receive(
            &[
                ("throttle".into(), throttle),
                ("roll_rate".into(), roll_rate),
                ("pitch_rate".into(), pitch_rate),
            ]
            .into(),
        )?;
        aircraft.step(&mut context(truth, &mut Messages::default(), dt_s), &mut io)?;
        Ok(())
    }

    /// Climbing (pitch -10 degrees, nose up) and banked 20 degrees right, at 25 m/s.
    fn turning_climb() -> KinematicState {
        let attitude = Quaternion::from_euler(EulerAngles {
            roll_world_from_body_rad: 20_f64.to_radians(),
            pitch_world_from_body_rad: -10_f64.to_radians(),
            yaw_world_from_body_rad: 1.0,
        });
        KinematicState {
            position_world_m: Vec3::new(5.0, -3.0, 100.0),
            velocity_world_mps: attitude.rotate_body_to_world(Vec3::new(25.0, 0.0, 0.0)),
            orientation_world_from_body: attitude,
            ..KinematicState::default()
        }
    }

    /// World angular velocity that turns `before` into `after` over `dt_s`.
    fn rotation_rate(before: Quaternion, after: Quaternion, dt_s: f64) -> Vec3 {
        let before = nalgebra::UnitQuaternion::from_quaternion(nalgebra::Quaternion::new(
            before.w, before.x, before.y, before.z,
        ));
        let after = nalgebra::UnitQuaternion::from_quaternion(nalgebra::Quaternion::new(
            after.w, after.x, after.y, after.z,
        ));
        (after * before.inverse()).scaled_axis() / dt_s
    }

    #[test]
    fn nonfinite_integration_returns_an_explanatory_error() -> anyhow::Result<()> {
        let mut aircraft = aircraft()?;
        let mut truth = crate::KinematicState {
            position_world_m: Vec3::new(f64::NAN, 0.0, 0.0),
            ..crate::KinematicState::default()
        };
        let error = aircraft
            .integrate(&mut truth, AircraftControl::default(), 0.1)
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "SimpleAircraft integration produced nonfinite state"
        );
        Ok(())
    }

    #[test]
    fn level_uncontrolled_aircraft_moves_forward_at_constant_speed() -> anyhow::Result<()> {
        let config = SimpleAircraft::configure(&PluginParams::yaml("{}"))?;
        let state = AircraftState {
            speed_mps: 20.0,
            ..AircraftState::default()
        };
        let derivative = aircraft_derivative(state, AircraftControl::default(), &config);
        let expected = [20.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        for (actual, expected) in derivative.into_iter().zip(expected) {
            assert!((actual - expected).abs() < 1e-12);
        }
        Ok(())
    }

    #[test]
    fn reported_velocity_is_the_change_in_position() -> anyhow::Result<()> {
        let mut aircraft = aircraft()?;
        let mut truth = turning_climb();
        initialize(&mut aircraft, &mut truth)?;
        for _ in 0..3 {
            let (position_m, velocity_mps) = (truth.position_world_m, truth.velocity_world_mps);
            step(&mut aircraft, &mut truth, [2.0, 0.1, -0.05], 1e-4)?;
            let moved_mps = (truth.position_world_m - position_m) / 1e-4;
            assert!(
                (moved_mps - velocity_mps).norm() < 1e-3,
                "{moved_mps:?} vs {velocity_mps:?}"
            );
        }
        // Nose up climbs.
        assert!(truth.velocity_world_mps.z > 4.0);
        Ok(())
    }

    #[test]
    fn reported_angular_velocity_is_the_change_in_attitude() -> anyhow::Result<()> {
        let mut aircraft = aircraft()?;
        let mut truth = turning_climb();
        initialize(&mut aircraft, &mut truth)?;
        let command = [0.0, 0.2, -0.1];
        step(&mut aircraft, &mut truth, command, 1e-4)?;
        for _ in 0..3 {
            let (attitude, rate_radps) = (
                truth.orientation_world_from_body,
                truth.angular_velocity_world_radps,
            );
            step(&mut aircraft, &mut truth, command, 1e-4)?;
            let turned_radps = rotation_rate(attitude, truth.orientation_world_from_body, 1e-4);
            assert!(
                (turned_radps - rate_radps).norm() < 1e-4,
                "{turned_radps:?} vs {rate_radps:?}"
            );
        }
        // Banked right, so it turns right: yaw rate -g tan(bank) / speed.
        let yaw_rate_radps = -9.81 * 20_f64.to_radians().tan() / 25.0;
        let reported_radps = truth.angular_velocity_world_radps.z;
        assert!(
            (reported_radps - yaw_rate_radps).abs() < 0.05,
            "{reported_radps}"
        );
        Ok(())
    }

    #[test]
    fn a_banked_turn_is_a_coordinated_turn_whose_radius_grows_with_speed_squared()
    -> anyhow::Result<()> {
        // Level at the 30 degree bank limit and constant speed: the turn rate is
        // g tan(bank) / speed, so the radius is speed^2 / (g tan(bank)).
        let mut radii_m = Vec::new();
        for speed_mps in [20.0, 25.0, 40.0] {
            let mut aircraft = aircraft()?;
            let attitude = Quaternion::from_euler(EulerAngles {
                roll_world_from_body_rad: 30_f64.to_radians(),
                ..EulerAngles::default()
            });
            let mut truth = KinematicState {
                velocity_world_mps: Vec3::new(speed_mps, 0.0, 0.0),
                orientation_world_from_body: attitude,
                ..KinematicState::default()
            };
            initialize(&mut aircraft, &mut truth)?;
            let heading_rad = |truth: &KinematicState| {
                truth.orientation_world_from_body.yaw_world_from_body_rad()
            };
            let before_rad = heading_rad(&truth);
            for _ in 0..10 {
                step(&mut aircraft, &mut truth, [0.0, 0.0, 0.0], 0.1)?;
            }
            let rate_radps = (heading_rad(&truth) - before_rad) / 1.0;
            let expected_radps = -9.81 * 30_f64.to_radians().tan() / speed_mps;
            assert!((rate_radps - expected_radps).abs() < 1e-12, "{rate_radps}");
            radii_m.push(speed_mps / rate_radps.abs());
        }
        // 110 m at 25 m/s, and four times the radius at twice the speed.
        assert!((radii_m[1] - 110.4).abs() < 0.1, "{radii_m:?}");
        assert!((radii_m[2] / radii_m[0] - 4.0).abs() < 1e-9, "{radii_m:?}");
        Ok(())
    }

    #[test]
    fn an_angle_held_at_its_limit_reports_no_rate() -> anyhow::Result<()> {
        let mut aircraft = aircraft()?;
        let mut truth = turning_climb();
        initialize(&mut aircraft, &mut truth)?;
        // Roll rate -1 rad/s banks left to the 30 degree limit.
        for _ in 0..20 {
            step(&mut aircraft, &mut truth, [0.0, -1.0, 0.0], 0.1)?;
        }
        let attitude = truth.orientation_world_from_body;
        assert!((attitude.roll_world_from_body_rad() + 30_f64.to_radians()).abs() < 1e-12);
        let rate_radps = truth.angular_velocity_world_radps;
        step(&mut aircraft, &mut truth, [0.0, -1.0, 0.0], 0.1)?;
        let after = truth.orientation_world_from_body;
        assert!((after.roll_world_from_body_rad() + 30_f64.to_radians()).abs() < 1e-12);
        // Only the turn remains, and the reported rate is that turn.
        let turned_radps = rotation_rate(attitude, after, 0.1);
        assert!(
            (turned_radps - rate_radps).norm() < 1e-9,
            "{turned_radps:?} vs {rate_radps:?}"
        );
        // Banked 30 degrees left at 25 m/s: 9.81 tan(30 degrees) / 25 rad/s.
        assert!((rate_radps.z - 0.2266).abs() < 1e-3, "{rate_radps:?}");
        Ok(())
    }

    #[test]
    fn a_change_to_position_attitude_or_speed_carries_into_the_next_step() -> anyhow::Result<()> {
        let mut aircraft = aircraft()?;
        let mut truth = turning_climb();
        initialize(&mut aircraft, &mut truth)?;
        step(&mut aircraft, &mut truth, [0.0, 0.0, 0.0], 0.1)?;
        // An interaction moves the aircraft 50 m east, turns it north, and slows it to 20 m/s.
        let north = Quaternion::from_euler(EulerAngles {
            yaw_world_from_body_rad: std::f64::consts::FRAC_PI_2,
            ..EulerAngles::default()
        });
        let pushed_m = truth.position_world_m + Vec3::new(50.0, 0.0, 0.0);
        truth.position_world_m = pushed_m;
        truth.orientation_world_from_body = north;
        truth.velocity_world_mps = Vec3::new(0.0, 20.0, 0.0);
        step(&mut aircraft, &mut truth, [0.0, 0.0, 0.0], 0.1)?;
        let moved_m = truth.position_world_m - pushed_m;
        assert!(
            (moved_m - Vec3::new(0.0, 2.0, 0.0)).norm() < 1e-9,
            "{moved_m:?}"
        );
        Ok(())
    }

    #[test]
    fn a_sideways_velocity_keeps_its_speed_and_turns_to_the_nose() -> anyhow::Result<()> {
        let mut aircraft = aircraft()?;
        // Heading east at 20 m/s, then pushed to 20 m/s north.
        let mut truth = KinematicState {
            velocity_world_mps: Vec3::new(20.0, 0.0, 0.0),
            ..KinematicState::default()
        };
        initialize(&mut aircraft, &mut truth)?;
        truth.velocity_world_mps = Vec3::new(0.0, 20.0, 0.0);
        step(&mut aircraft, &mut truth, [0.0, 0.0, 0.0], 0.1)?;
        assert!((truth.position_world_m - Vec3::new(2.0, 0.0, 0.0)).norm() < 1e-12);
        assert!((truth.velocity_world_mps - Vec3::new(20.0, 0.0, 0.0)).norm() < 1e-12);
        Ok(())
    }

    #[test]
    fn initialization_keeps_the_mission_attitude_within_limits() -> anyhow::Result<()> {
        let mut aircraft = aircraft()?;
        let mut truth = turning_climb();
        let given = truth.orientation_world_from_body;
        initialize(&mut aircraft, &mut truth)?;
        let attitude = truth.orientation_world_from_body;
        assert!(
            (attitude.roll_world_from_body_rad() - given.roll_world_from_body_rad()).abs() < 1e-12
        );
        assert!(
            (attitude.pitch_world_from_body_rad() - given.pitch_world_from_body_rad()).abs()
                < 1e-12
        );
        // Velocity along the nose, climbing.
        let nose = attitude.rotate_body_to_world(Vec3::x());
        assert!((truth.velocity_world_mps - nose * 25.0).norm() < 1e-12);
        Ok(())
    }

    #[test]
    fn limits_apply_before_and_after_integration() -> anyhow::Result<()> {
        let mut aircraft = aircraft()?;
        let attitude = Quaternion::from_euler(EulerAngles {
            roll_world_from_body_rad: -2.0,
            pitch_world_from_body_rad: 1.0,
            ..EulerAngles::default()
        });
        let mut truth = KinematicState {
            velocity_world_mps: Vec3::new(100.0, 0.0, 0.0),
            orientation_world_from_body: attitude,
            ..KinematicState::default()
        };
        step(&mut aircraft, &mut truth, [1000.0, -1000.0, 1000.0], 0.01)?;
        let attitude = truth.orientation_world_from_body;
        assert!((truth.velocity_world_mps.norm() - 40.0).abs() < 1e-12);
        assert!((attitude.roll_world_from_body_rad() + 30_f64.to_radians()).abs() < 1e-12);
        assert!((attitude.pitch_world_from_body_rad() - 30_f64.to_radians()).abs() < 1e-12);
        // Positive pitch descends; negative roll (banked left) turns left.
        assert!(truth.velocity_world_mps.z < 0.0);
        assert!(truth.angular_velocity_world_radps.z > 0.0);
        Ok(())
    }

    #[test]
    fn a_positive_roll_rate_banks_right_and_turns_right() -> anyhow::Result<()> {
        let mut aircraft = aircraft()?;
        let mut truth = KinematicState {
            velocity_world_mps: Vec3::new(25.0, 0.0, 0.0),
            ..KinematicState::default()
        };
        initialize(&mut aircraft, &mut truth)?;
        for _ in 0..10 {
            step(&mut aircraft, &mut truth, [0.0, 0.5, 0.0], 0.1)?;
        }
        let attitude = truth.orientation_world_from_body;
        // Truth's roll, with no change of sign: right wing down.
        assert!((attitude.roll_world_from_body_rad() - 0.5).abs() < 1e-12);
        let right_wing = attitude.rotate_body_to_world(-Vec3::y());
        assert!(right_wing.z < 0.0);
        // Heading east, so turning right heads south of east.
        assert!(attitude.yaw_world_from_body_rad() < 0.0);
        assert!(truth.angular_velocity_world_radps.z < 0.0);
        Ok(())
    }
}

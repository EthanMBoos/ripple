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
//!   within rate and range limits, and turn rate comes from bank angle and a
//!   turning radius. The default limits are round numbers, not any aircraft.
//! - Validated against: SCRIMMAGE's SimpleAircraft (the comparison missions, its
//!   generated scenarios, and the long runs).
//! - Valid range: a stand-in for an aircraft that turns and climbs within its
//!   limits; not a physical model.
//! - Not modeled: forces, aerodynamics, inertia, wind, stall, sideslip
//!   (velocity is always along the nose), and energy (speed and climb are
//!   independent).
//! - To go further: FixedWing6DOF for anything aerodynamic; to stand in for a
//!   real aircraft, fit the limits to its turn and climb performance.

use anyhow::{Result, ensure};
use serde::Deserialize;

use crate::{
    math::{self, EulerAngles, KinematicState, Quaternion, Vec3},
    plugin::{
        Frame, MotionContext, MotionModel, Plugin, PluginIo, PluginParams, Port, Ports, Unit,
        Update,
    },
};

/// Mission parameters in the mission file's units; `Default` supplies any key the mission leaves out.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct SimpleAircraftParams {
    #[serde(rename = "min_velocity")]
    min_speed_mps: f64,
    #[serde(rename = "max_velocity")]
    max_speed_mps: f64,
    #[serde(rename = "max_roll")]
    max_roll_deg: f64,
    #[serde(rename = "max_pitch")]
    max_pitch_deg: f64,
    #[serde(rename = "max_roll_rate")]
    max_roll_rate_degps: f64,
    #[serde(rename = "max_pitch_rate")]
    max_pitch_rate_degps: f64,
    #[serde(rename = "turning_radius")]
    turning_radius_m: f64,
    #[serde(rename = "speed_target")]
    reference_speed_mps: f64,
    #[serde(rename = "radius_slope_per_speed")]
    radius_slope_s: f64,
}

impl Default for SimpleAircraftParams {
    fn default() -> Self {
        Self {
            min_speed_mps: 15.0,
            max_speed_mps: 40.0,
            max_roll_deg: 30.0,
            max_pitch_deg: 30.0,
            max_roll_rate_degps: 57.3,
            max_pitch_rate_degps: 57.3,
            turning_radius_m: 13.0,
            reference_speed_mps: 50.0,
            radius_slope_s: 0.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct AircraftConfig {
    min_speed_mps: f64,
    max_speed_mps: f64,
    max_roll_rad: f64,
    max_pitch_rad: f64,
    max_roll_rate_radps: f64,
    max_pitch_rate_radps: f64,
    turning_radius_m: f64,
    reference_speed_mps: f64,
    radius_slope_s: f64,
}

#[derive(Clone, Copy, Debug, Default)]
struct AircraftControl {
    // Legacy dimensionless control, saturated to [-100, 100].
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
        let config = AircraftConfig {
            min_speed_mps: params.min_speed_mps,
            max_speed_mps: params.max_speed_mps,
            max_roll_rad: crate::math::deg_to_rad(params.max_roll_deg),
            max_pitch_rad: crate::math::deg_to_rad(params.max_pitch_deg),
            max_roll_rate_radps: crate::math::deg_to_rad(params.max_roll_rate_degps),
            max_pitch_rate_radps: crate::math::deg_to_rad(params.max_pitch_rate_degps),
            turning_radius_m: params.turning_radius_m,
            reference_speed_mps: params.reference_speed_mps,
            radius_slope_s: params.radius_slope_s,
        };

        ensure!(
            config.min_speed_mps <= config.max_speed_mps
                && config.max_roll_rad >= 0.0
                && config.max_pitch_rad >= 0.0
                && config.max_roll_rate_radps >= 0.0
                && config.max_pitch_rate_radps >= 0.0,
            "invalid aircraft limits"
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
        state.pitch_world_from_body_rad = state
            .pitch_world_from_body_rad
            .clamp(-self.config.max_pitch_rad, self.config.max_pitch_rad);
        state.speed_mps = state
            .speed_mps
            .clamp(self.config.min_speed_mps, self.config.max_speed_mps);
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

fn aircraft_derivative(
    state: AircraftState,
    control: AircraftControl,
    config: &AircraftConfig,
) -> [f64; 7] {
    let horizontal_speed_mps = state.speed_mps * state.pitch_world_from_body_rad.cos();
    let effective_turning_radius_m = config.turning_radius_m
        + config.radius_slope_s * (state.speed_mps - config.reference_speed_mps);
    // Banked right (positive roll), it turns right: heading decreases.
    let yaw_rate_world_radps =
        -state.speed_mps / effective_turning_radius_m * state.roll_world_from_body_rad.tan();
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
    let max_pitch_rad = config.max_pitch_rad;

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
            -max_pitch_rad,
            max_pitch_rad,
            control.pitch_rate_radps,
        ),
        yaw_rate_world_radps,
        held(
            state.speed_mps,
            config.min_speed_mps,
            config.max_speed_mps,
            control.throttle / 5.0,
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
    use crate::Params;
    use crate::math::{EulerAngles, KinematicState, Quaternion, Vec3};
    use crate::plugin::{EntityInfo, Messages, MotionContext, PluginIo, StepTime};

    fn aircraft() -> anyhow::Result<SimpleAircraft> {
        let config = SimpleAircraft::configure(&PluginParams::text(&Params::new()))?;
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
                sub_swarm_id: 0,
            },
            time: StepTime { time_s: 0.0, dt_s },
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
        let params = Params::from([("turning_radius".into(), "0".into())]);
        let config = SimpleAircraft::configure(&PluginParams::text(&params))?;
        let mut aircraft = SimpleAircraft::new(&config);
        let mut truth = crate::KinematicState::default();
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
        let params = Params::new();
        let config = SimpleAircraft::configure(&PluginParams::text(&params))?;
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
        // Banked right, so it turns right: yaw rate -speed / radius * tan(bank).
        let yaw_rate_radps = -25.0 / 13.0 * 20_f64.to_radians().tan();
        let reported_radps = truth.angular_velocity_world_radps.z;
        assert!(
            (reported_radps - yaw_rate_radps).abs() < 0.05,
            "{reported_radps}"
        );
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
        assert!(rate_radps.z > 1.0, "{rate_radps:?}");
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

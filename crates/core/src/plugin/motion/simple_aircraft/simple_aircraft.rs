//! SimpleAircraft dynamics, limits, and RK4 integration from the C++ reference.
//!
//! C++ counterpart: src/plugins/motion/SimpleAircraft/SimpleAircraft.cpp.
//! Motion phase: read throttle/model angular rates, integrate, update mutable truth.

use anyhow::{Result, ensure};
use serde::Deserialize;

use crate::{
    math::{self, EulerAngles, KinematicState, Quaternion, Vec3},
    plugin::{
        Frame, MotionContext, MotionModel, Plugin, PluginIo, PluginParams, Port, Ports, Unit,
        Update,
    },
};

/// Mission parameters in the C++ units; `Default` supplies any key the mission leaves out.
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
    max_roll_model_rad: f64,
    max_pitch_model_rad: f64,
    max_roll_rate_model_radps: f64,
    max_pitch_rate_model_radps: f64,
    turning_radius_m: f64,
    reference_speed_mps: f64,
    radius_slope_s: f64,
}

#[derive(Clone, Copy, Debug, Default)]
struct AircraftControl {
    // Legacy dimensionless control, saturated to [-100, 100].
    throttle: f64,
    roll_rate_model_radps: f64,
    pitch_rate_model_radps: f64,
}

/// Model roll has the opposite sign to the roll stored in the truth quaternion.
#[derive(Clone, Copy, Debug, Default)]
struct AircraftState {
    position_world_m: Vec3,
    roll_model_rad: f64,
    pitch_model_rad: f64,
    yaw_world_from_body_rad: f64,
    speed_mps: f64,
}

#[derive(Clone, Debug)]
pub struct SimpleAircraft {
    state: AircraftState,
    config: AircraftConfig,
}

impl Plugin for SimpleAircraft {
    type Config = AircraftConfig;

    fn configure(params: &PluginParams<'_>) -> Result<AircraftConfig> {
        let params: SimpleAircraftParams = params.parse()?;
        let config = AircraftConfig {
            min_speed_mps: params.min_speed_mps,
            max_speed_mps: params.max_speed_mps,
            max_roll_model_rad: params.max_roll_deg.to_radians(),
            max_pitch_model_rad: params.max_pitch_deg.to_radians(),
            max_roll_rate_model_radps: params.max_roll_rate_degps.to_radians(),
            max_pitch_rate_model_radps: params.max_pitch_rate_degps.to_radians(),
            turning_radius_m: params.turning_radius_m,
            reference_speed_mps: params.reference_speed_mps,
            radius_slope_s: params.radius_slope_s,
        };

        ensure!(
            config.min_speed_mps <= config.max_speed_mps
                && config.max_roll_model_rad >= 0.0
                && config.max_pitch_model_rad >= 0.0
                && config.max_roll_rate_model_radps >= 0.0
                && config.max_pitch_rate_model_radps >= 0.0,
            "invalid aircraft limits"
        );
        Ok(config)
    }

    fn new(config: &AircraftConfig) -> Self {
        Self {
            config: config.clone(),
            state: AircraftState::default(),
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
    fn initialize(&mut self, context: &mut MotionContext<'_>) -> Result<()> {
        let truth = &mut context.truth;
        let orientation = truth.orientation_world_from_body;
        let pitch_sine = 2.0 * (orientation.w * orientation.y - orientation.z * orientation.x);
        self.state = AircraftState {
            position_world_m: truth.position_world_m,
            roll_model_rad: orientation.roll_world_from_body_rad(),
            pitch_model_rad: pitch_sine.clamp(-1.0, 1.0).asin(),
            yaw_world_from_body_rad: orientation.yaw_world_from_body_rad(),
            speed_mps: math::norm(truth.velocity_world_mps),
        };
        self.clamp_state();

        let state = self.state;
        truth.orientation_world_from_body = state.orientation_world_from_body();
        // Reference initialization ignores pitch when constructing initial velocity.
        truth.velocity_world_mps = Vec3::new(
            state.speed_mps * state.yaw_world_from_body_rad.cos(),
            state.speed_mps * state.yaw_world_from_body_rad.sin(),
            0.0,
        );
        Ok(())
    }

    fn step(&mut self, context: &mut MotionContext<'_>, io: &mut PluginIo) -> Result<Update> {
        let control = AircraftControl {
            throttle: io.read("throttle")?,
            roll_rate_model_radps: io.read("roll_rate")?,
            pitch_rate_model_radps: io.read("pitch_rate")?,
        };
        self.integrate(context.truth, control, context.time.dt_s)?;
        Ok(Update::Applied)
    }
}

impl SimpleAircraft {
    fn clamp_state(&mut self) {
        self.state.roll_model_rad = self.state.roll_model_rad.clamp(
            -self.config.max_roll_model_rad,
            self.config.max_roll_model_rad,
        );
        self.state.pitch_model_rad = self.state.pitch_model_rad.clamp(
            -self.config.max_pitch_model_rad,
            self.config.max_pitch_model_rad,
        );
        self.state.speed_mps = self
            .state
            .speed_mps
            .clamp(self.config.min_speed_mps, self.config.max_speed_mps);
    }

    fn integrate(
        &mut self,
        truth: &mut KinematicState,
        control: AircraftControl,
        dt_s: f64,
    ) -> Result<()> {
        // Matches C++ for now, though we don't consider it correct: integration continues
        // from this private state, so changes an interaction makes to truth (position,
        // velocity, attitude) are overwritten on the next motion step.
        self.clamp_state();

        let limited_control = AircraftControl {
            throttle: control.throttle.clamp(-100.0, 100.0),
            roll_rate_model_radps: control.roll_rate_model_radps.clamp(
                -self.config.max_roll_rate_model_radps,
                self.config.max_roll_rate_model_radps,
            ),
            pitch_rate_model_radps: control.pitch_rate_model_radps.clamp(
                -self.config.max_pitch_rate_model_radps,
                self.config.max_pitch_rate_model_radps,
            ),
        };

        let mut coordinates = self.state.coordinates();
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

        self.state = AircraftState::from_coordinates(coordinates);
        let state = self.state;
        truth.position_world_m = state.position_world_m;
        truth.orientation_world_from_body = state.orientation_world_from_body();
        // Matches C++ for now, though we don't consider it correct: reported vertical
        // velocity uses +sin(pitch), while altitude integration below uses -sin(pitch).
        // Angular velocity is also never set, so turning aircraft report zero rates.
        truth.velocity_world_mps = Vec3::new(
            state.speed_mps * state.yaw_world_from_body_rad.cos() * state.pitch_model_rad.cos(),
            state.speed_mps * state.yaw_world_from_body_rad.sin() * state.pitch_model_rad.cos(),
            state.speed_mps * state.pitch_model_rad.sin(),
        );
        Ok(())
    }
}

fn aircraft_derivative(
    state: AircraftState,
    control: AircraftControl,
    config: &AircraftConfig,
) -> [f64; 7] {
    let horizontal_speed_mps = state.speed_mps * state.pitch_model_rad.cos();
    let effective_turning_radius_m = config.turning_radius_m
        + config.radius_slope_s * (state.speed_mps - config.reference_speed_mps);
    let yaw_rate_world_radps =
        state.speed_mps / effective_turning_radius_m * state.roll_model_rad.tan();

    [
        horizontal_speed_mps * state.yaw_world_from_body_rad.cos(),
        horizontal_speed_mps * state.yaw_world_from_body_rad.sin(),
        -state.pitch_model_rad.sin() * state.speed_mps,
        control.roll_rate_model_radps,
        control.pitch_rate_model_radps,
        yaw_rate_world_radps,
        control.throttle / 5.0,
    ]
}

impl AircraftState {
    // Numerical integration coordinates are defined only at this conversion boundary.
    fn coordinates(self) -> [f64; 7] {
        [
            self.position_world_m.x,
            self.position_world_m.y,
            self.position_world_m.z,
            self.roll_model_rad,
            self.pitch_model_rad,
            self.yaw_world_from_body_rad,
            self.speed_mps,
        ]
    }

    fn from_coordinates(coordinates: [f64; 7]) -> Self {
        let [
            x_world_m,
            y_world_m,
            z_world_m,
            roll_model_rad,
            pitch_model_rad,
            yaw_world_from_body_rad,
            speed_mps,
        ] = coordinates;
        Self {
            position_world_m: Vec3::new(x_world_m, y_world_m, z_world_m),
            roll_model_rad,
            pitch_model_rad,
            yaw_world_from_body_rad,
            speed_mps,
        }
    }

    fn orientation_world_from_body(self) -> Quaternion {
        Quaternion::from_euler(EulerAngles {
            roll_world_from_body_rad: -self.roll_model_rad,
            pitch_world_from_body_rad: self.pitch_model_rad,
            yaw_world_from_body_rad: self.yaw_world_from_body_rad,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AircraftControl, AircraftState, Plugin, PluginParams, SimpleAircraft, aircraft_derivative,
    };
    use crate::Params;

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
    fn model_pitch_roll_and_limits_retain_legacy_signs_and_operation_order() -> anyhow::Result<()> {
        let config = SimpleAircraft::configure(&PluginParams::text(&Params::new()))?;
        let mut aircraft = SimpleAircraft::new(&config);
        aircraft.state = AircraftState {
            speed_mps: 100.0,
            roll_model_rad: 2.0,
            pitch_model_rad: 2.0,
            ..AircraftState::default()
        };
        aircraft.clamp_state();
        assert!((aircraft.state.speed_mps - 40.0).abs() < 1e-12);
        assert!((aircraft.state.roll_model_rad.to_degrees() - 30.0).abs() < 1e-12);
        assert!((aircraft.state.pitch_model_rad.to_degrees() - 30.0).abs() < 1e-12);
        let derivative = aircraft_derivative(aircraft.state, AircraftControl::default(), &config);
        assert!(derivative[2] < 0.0); // Positive model pitch descends.
        assert!(derivative[5] > 0.0); // Positive model roll turns toward positive yaw.
        let attitude = aircraft.state.orientation_world_from_body();
        assert!(attitude.roll_world_from_body_rad() < 0.0);

        let mut truth = crate::KinematicState::default();
        aircraft.integrate(
            &mut truth,
            AircraftControl {
                throttle: 1000.0,
                roll_rate_model_radps: 1000.0,
                pitch_rate_model_radps: -1000.0,
            },
            0.01,
        )?;
        // Limits are applied before RK4, not after; preserve the small overshoot.
        assert!((aircraft.state.speed_mps - 40.2).abs() < 1e-12);
        assert!(
            (aircraft.state.roll_model_rad - (30.0_f64 + 57.3 * 0.01).to_radians()).abs() < 1e-12
        );
        assert!(
            (aircraft.state.pitch_model_rad - (30.0_f64 - 57.3 * 0.01).to_radians()).abs() < 1e-12
        );
        assert!(truth.velocity_world_mps.z > 0.0); // Legacy reported vz opposes altitude derivative.
        Ok(())
    }
}

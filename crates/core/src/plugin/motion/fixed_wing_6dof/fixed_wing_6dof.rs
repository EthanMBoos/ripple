//! Aerodynamic fixed-wing six-degree-of-freedom motion, with RK4 propagation.
//!
//! C++ counterpart: motion/FixedWing6DOF (GTRI, LGPL-3.0-or-later).
//! Motion phase: read throttle and surface angles, update entity truth.
//! Private body axes are forward/right/down; public truth remains ENU world.
//! The two-aircraft mission checks the port, not trimmed flight or ArduPilot readiness.

mod aerodynamics;
mod dynamics;

use anyhow::{Context, Result, bail, ensure};
use nalgebra::{Matrix3, UnitQuaternion};
use serde::Deserialize;

use crate::math::{self, KinematicState, Vec3};
use crate::plugin::{
    Frame, MotionContext, MotionModel, Plugin, PluginIo, PluginParams, Port, Ports, Unit, Update,
};

use aerodynamics::{Aerodynamics, Airframe};
use dynamics::{AircraftState, derivative, flip_yz};

// Preserve the C++ rounded limit rather than replacing it with exactly pi/6.
#[allow(clippy::approx_constant)]
const LEGACY_SURFACE_LIMIT_RAD: f64 = 0.5236;
/// The C++ `FixedWing6DOF.xml` inertia.
const DEFAULT_INERTIA_SLUG_FT_SQ: &str = "[8090 0 1300] [0 25900 0] [1300 0 29200]";
const SLUG_FT_SQ_TO_KGM2: f64 = 1.35581795;

/// Mission parameters in SI units unless marked; `Default` supplies any key the
/// mission leaves out (the C++ large-aircraft configuration).
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct FixedWingParams {
    #[serde(rename = "mass")]
    mass_kg: f64,
    #[serde(rename = "gravity_magnitude")]
    gravity_mps2: f64,
    /// Three bracketed rows, in slug·ft² or SI. Give at most one; with neither,
    /// the C++ default `DEFAULT_INERTIA_SLUG_FT_SQ` applies.
    inertia_matrix_slug_ft_sq: Option<String>,
    inertia_matrix: Option<String>,
    #[serde(rename = "air_density")]
    density_kgpm3: f64,
    #[serde(rename = "wing_span")]
    span_m: f64,
    #[serde(rename = "surface_area_of_wing")]
    area_m2: f64,
    #[serde(rename = "chord_length")]
    chord_m: f64,
    #[serde(rename = "efficiency_factor")]
    efficiency: f64,
    #[serde(rename = "wind_E")]
    wind_east_mps: f64,
    #[serde(rename = "wind_N")]
    wind_north_mps: f64,
    #[serde(rename = "wind_U")]
    wind_up_mps: f64,
    #[serde(rename = "throttle_input_min")]
    throttle_min: f64,
    #[serde(rename = "throttle_input_max")]
    throttle_max: f64,
    #[serde(rename = "thrust_min")]
    thrust_min_n: f64,
    #[serde(rename = "thrust_max")]
    thrust_max_n: f64,
    #[serde(rename = "delta_elevator_min")]
    elevator_min_rad: f64,
    #[serde(rename = "delta_elevator_max")]
    elevator_max_rad: f64,
    #[serde(rename = "delta_aileron_min")]
    aileron_min_rad: f64,
    #[serde(rename = "delta_aileron_max")]
    aileron_max_rad: f64,
    #[serde(rename = "delta_rudder_min")]
    rudder_min_rad: f64,
    #[serde(rename = "delta_rudder_max")]
    rudder_max_rad: f64,
    #[serde(rename = "C_D0")]
    c_d0: f64,
    #[serde(rename = "C_D_alpha")]
    c_d_alpha: f64,
    /// C++ reads the key WITH the trailing underscore.
    #[serde(rename = "C_D_delta_elevator_")]
    c_d_elevator: f64,
    #[serde(rename = "C_L0")]
    c_l0: f64,
    #[serde(rename = "C_L_alpha")]
    c_l_alpha: f64,
    #[serde(rename = "C_LQ")]
    c_lq: f64,
    /// Always zero in the reference; read only so a bad value is still an error.
    #[serde(rename = "C_L_alpha_dot")]
    _c_l_alpha_dot: f64,
    #[serde(rename = "C_L_delta_elevator")]
    c_l_elevator: f64,
    #[serde(rename = "C_Y_beta")]
    c_y_beta: f64,
    #[serde(rename = "C_Y_delta_rudder")]
    c_y_rudder: f64,
    #[serde(rename = "C_L_beta")]
    c_roll_beta: f64,
    #[serde(rename = "C_LP")]
    c_roll_p: f64,
    #[serde(rename = "C_LR")]
    c_roll_r: f64,
    #[serde(rename = "C_L_delta_aileron")]
    c_roll_aileron: f64,
    #[serde(rename = "C_L_delta_rudder")]
    c_roll_rudder: f64,
    #[serde(rename = "C_M0")]
    c_m0: f64,
    #[serde(rename = "C_M_alpha")]
    c_m_alpha: f64,
    #[serde(rename = "C_MQ")]
    c_mq: f64,
    /// Always zero in the reference; read only so a bad value is still an error.
    #[serde(rename = "C_M_alpha_dot")]
    _c_m_alpha_dot: f64,
    #[serde(rename = "C_M_delta_elevator")]
    c_m_elevator: f64,
    #[serde(rename = "C_N_beta")]
    c_n_beta: f64,
    #[serde(rename = "C_NP")]
    c_np: f64,
    #[serde(rename = "C_NR")]
    c_nr: f64,
    #[serde(rename = "C_N_delta_aileron")]
    c_n_aileron: f64,
    #[serde(rename = "C_N_delta_rudder")]
    c_n_rudder: f64,
    /// The key C++ does NOT read (no trailing underscore); rejected so a tuning
    /// value is not silently ignored.
    #[serde(rename = "C_D_delta_elevator")]
    c_d_elevator_ignored_by_cpp: Option<f64>,
    use_ground_model: bool,
    use_launcher: bool,
    #[serde(rename = "launch_time")]
    launch_time_s: f64,
    #[serde(rename = "launch_accel")]
    launch_accel_mps2: f64,
    #[serde(rename = "launch_speed")]
    launch_speed_mps: f64,
    /// Not supported; use the run's frames and Rerun recording. Must stay false.
    write_csv: bool,
    draw_vel: bool,
    draw_ang_vel: bool,
}

impl Default for FixedWingParams {
    fn default() -> Self {
        Self {
            mass_kg: 7973.2467,
            gravity_mps2: 9.81,
            inertia_matrix_slug_ft_sq: None,
            inertia_matrix: None,
            density_kgpm3: 1.225,
            span_m: 8.382,
            area_m2: 24.1548,
            chord_m: 3.29184,
            efficiency: 0.995,
            wind_east_mps: 0.0,
            wind_north_mps: 0.0,
            wind_up_mps: 0.0,
            throttle_min: 0.0,
            throttle_max: 1.0,
            thrust_min_n: -36000.0,
            thrust_max_n: 36000.0,
            elevator_min_rad: -LEGACY_SURFACE_LIMIT_RAD,
            elevator_max_rad: LEGACY_SURFACE_LIMIT_RAD,
            aileron_min_rad: -LEGACY_SURFACE_LIMIT_RAD,
            aileron_max_rad: LEGACY_SURFACE_LIMIT_RAD,
            rudder_min_rad: -0.2618,
            rudder_max_rad: 0.2618,
            c_d0: 0.03,
            c_d_alpha: 0.3,
            c_d_elevator: 0.01,
            c_l0: 0.28,
            c_l_alpha: 3.45,
            c_lq: 0.0,
            _c_l_alpha_dot: 0.72,
            c_l_elevator: 0.36,
            c_y_beta: -0.98,
            c_y_rudder: 0.17,
            c_roll_beta: -0.12,
            c_roll_p: -0.26,
            c_roll_r: 0.14,
            c_roll_aileron: 0.08,
            c_roll_rudder: -0.105,
            c_m0: 0.0,
            c_m_alpha: -0.38,
            c_mq: -3.6,
            _c_m_alpha_dot: -1.1,
            c_m_elevator: -0.5,
            c_n_beta: 0.25,
            c_np: 0.022,
            c_nr: -0.35,
            c_n_aileron: 0.06,
            c_n_rudder: 0.032,
            c_d_elevator_ignored_by_cpp: None,
            use_ground_model: true,
            use_launcher: false,
            launch_time_s: 30.0,
            launch_accel_mps2: 200.0,
            launch_speed_mps: 20.0,
            write_csv: false,
            draw_vel: false,
            draw_ang_vel: false,
        }
    }
}

#[derive(Clone)]
pub struct FixedWingConfig {
    mass_kg: f64,
    gravity_mps2: f64,
    inertia_kgm2: Matrix3<f64>,
    inverse_inertia: Matrix3<f64>,
    density_kgpm3: f64,
    span_m: f64,
    area_m2: f64,
    chord_m: f64,
    efficiency: f64,
    wind_world_mps: Vec3,
    throttle_min: f64,
    throttle_max: f64,
    thrust_min_n: f64,
    thrust_max_n: f64,
    elevator_limits_rad: [f64; 2],
    aileron_limits_rad: [f64; 2],
    rudder_limits_rad: [f64; 2],
    aerodynamics: Aerodynamics,
    use_ground_model: bool,
    use_launcher: bool,
    launch_time_s: f64,
    launch_accel_mps2: f64,
    launch_speed_mps: f64,
}

#[derive(Clone, Copy, Default)]
struct Control {
    throttle: f64,
    elevator_rad: f64,
    aileron_rad: f64,
    rudder_rad: f64,
}

enum LaunchState {
    Waiting,
    Launching { start_s: f64 },
    Flying,
}

pub struct FixedWing6Dof {
    config: FixedWingConfig,
    launch: LaunchState,
}

impl Plugin for FixedWing6Dof {
    type Config = FixedWingConfig;

    fn configure(params: &PluginParams<'_>) -> Result<FixedWingConfig> {
        let params: FixedWingParams = params.parse()?;
        for (key, enabled) in [
            ("write_csv", params.write_csv),
            ("draw_vel", params.draw_vel),
            ("draw_ang_vel", params.draw_ang_vel),
        ] {
            ensure!(
                !enabled,
                "FixedWing6DOF {key}=true is not supported; use the run's frames and Rerun recording"
            );
        }
        ensure!(
            params.c_d_elevator_ignored_by_cpp.is_none(),
            "use C_D_delta_elevator_ (trailing underscore), the key read by C++ FixedWing6DOF"
        );
        ensure!(
            params.elevator_min_rad <= params.elevator_max_rad
                && params.aileron_min_rad <= params.aileron_max_rad
                && params.rudder_min_rad <= params.rudder_max_rad,
            "FixedWing6DOF requires each delta_*_min <= delta_*_max"
        );
        let inertia_kgm2 = parse_inertia(&params)?;
        let inverse_inertia = inertia_kgm2
            .try_inverse()
            .context("FixedWing6DOF inertia must be invertible")?;
        let config = FixedWingConfig {
            mass_kg: params.mass_kg,
            gravity_mps2: params.gravity_mps2,
            inertia_kgm2,
            inverse_inertia,
            density_kgpm3: params.density_kgpm3,
            span_m: params.span_m,
            area_m2: params.area_m2,
            chord_m: params.chord_m,
            efficiency: params.efficiency,
            wind_world_mps: Vec3::new(
                params.wind_east_mps,
                params.wind_north_mps,
                params.wind_up_mps,
            ),
            throttle_min: params.throttle_min,
            throttle_max: params.throttle_max,
            thrust_min_n: params.thrust_min_n,
            thrust_max_n: params.thrust_max_n,
            elevator_limits_rad: [params.elevator_min_rad, params.elevator_max_rad],
            aileron_limits_rad: [params.aileron_min_rad, params.aileron_max_rad],
            rudder_limits_rad: [params.rudder_min_rad, params.rudder_max_rad],
            aerodynamics: Aerodynamics::new(&params),
            use_ground_model: params.use_ground_model,
            use_launcher: params.use_launcher,
            launch_time_s: params.launch_time_s,
            launch_accel_mps2: params.launch_accel_mps2,
            launch_speed_mps: params.launch_speed_mps,
        };
        ensure!(
            config.mass_kg > 0.0
                && config.density_kgpm3 >= 0.0
                && config.gravity_mps2 >= 0.0
                && config.span_m > 0.0
                && config.area_m2 > 0.0
                && config.chord_m > 0.0
                && config.efficiency > 0.0,
            "FixedWing6DOF requires positive mass and geometry, nonnegative air density and gravity"
        );
        ensure!(
            config.throttle_min < config.throttle_max && config.thrust_min_n <= config.thrust_max_n,
            "FixedWing6DOF has invalid throttle/thrust limits"
        );
        ensure!(
            !config.use_launcher
                || (config.launch_accel_mps2 > 0.0 && config.launch_speed_mps >= 0.0),
            "FixedWing6DOF launcher requires positive acceleration and nonnegative speed"
        );
        Ok(config)
    }

    fn new(config: &FixedWingConfig) -> Self {
        Self {
            config: config.clone(),
            launch: if config.use_launcher {
                LaunchState::Waiting
            } else {
                LaunchState::Flying
            },
        }
    }

    fn ports(_config: &FixedWingConfig) -> Ports {
        Ports::default()
            .input(Port::new("throttle", Unit::Dimensionless, Frame::None))
            .input(Port::new("elevator", Unit::Radians, Frame::Model))
            .input(Port::new("aileron", Unit::Radians, Frame::Model))
            .input(Port::new("rudder", Unit::Radians, Frame::Model))
    }
}

impl MotionModel for FixedWing6Dof {
    fn step(&mut self, context: &mut MotionContext<'_>, io: &mut PluginIo) -> Result<Update> {
        let control = Control {
            throttle: io.read("throttle")?,
            elevator_rad: io.read("elevator")?,
            aileron_rad: io.read("aileron")?,
            rudder_rad: io.read("rudder")?,
        };
        self.integrate(
            context.truth,
            control,
            context.time.time_s,
            context.time.dt_s,
        )?;
        Ok(Update::Applied)
    }
}

impl FixedWing6Dof {
    fn integrate(
        &mut self,
        truth: &mut KinematicState,
        control: Control,
        time_s: f64,
        dt_s: f64,
    ) -> Result<()> {
        let catapult_force_n = self.advance_launcher(time_s);
        let config = &self.config;
        let state = AircraftState::from_truth(truth);
        let rotation = UnitQuaternion::new_unchecked(state.attitude_model);
        let control = Control {
            throttle: control.throttle.clamp(-1.0, 1.0),
            elevator_rad: control
                .elevator_rad
                .clamp(config.elevator_limits_rad[0], config.elevator_limits_rad[1]),
            aileron_rad: control
                .aileron_rad
                .clamp(config.aileron_limits_rad[0], config.aileron_limits_rad[1]),
            rudder_rad: control
                .rudder_rad
                .clamp(config.rudder_limits_rad[0], config.rudder_limits_rad[1]),
        };
        let thrust_n = config.thrust_n(control.throttle);
        let wind_model_mps = rotation.inverse_transform_vector(&flip_yz(config.wind_world_mps));
        // Deliberate reference quirk: wind is ADDED to velocity, not subtracted.
        let air_velocity_model_mps = state.velocity_model_mps + wind_model_mps;
        let airframe = Airframe {
            density_kgpm3: config.density_kgpm3,
            span_m: config.span_m,
            area_m2: config.area_m2,
            chord_m: config.chord_m,
            efficiency: config.efficiency,
        };
        let mut loads = config.aerodynamics.loads(
            &airframe,
            air_velocity_model_mps,
            state.rates_model_radps,
            control,
        );
        let weight_model_n = rotation.inverse_transform_vector(&Vec3::new(
            0.0,
            0.0,
            config.mass_kg * config.gravity_mps2,
        ));
        loads.force_model_n = weight_model_n + Vec3::new(thrust_n, 0.0, 0.0) + loads.force_model_n;

        if config.use_ground_model && state.position_world_m.z < 0.0 {
            // Inherited ground model: damping is only 40 N*s/m, not mass-scaled.
            // This is not a validated landing model for the default eight-tonne aircraft.
            let ground_force_n = (-400.0 * config.mass_kg * state.position_world_m.z
                - 40.0 * state.velocity_world_mps.z)
                .max(0.0);
            loads.force_model_n +=
                rotation.inverse_transform_vector(&Vec3::new(0.0, 0.0, -ground_force_n));
        }
        if !matches!(self.launch, LaunchState::Flying) {
            loads.force_model_n = Vec3::zeros();
            loads.moment_model_nm = Vec3::zeros();
        }
        loads.force_model_n.x += catapult_force_n;
        let acceleration_model_mps2 = loads.force_model_n / config.mass_kg;
        let rates = state.rates_model_radps;
        let angular_acceleration_model_radps2 = config.inverse_inertia
            * (loads.moment_model_nm - rates.cross(&(config.inertia_kgm2 * rates)));

        // C++ evaluates loads from x_ (the start of the step), not RK4 stage x.
        // Freeze loads here; only kinematics and attitude vary inside the stages.
        let mut coordinates = state.coordinates();
        math::rk4(&mut coordinates, dt_s, |coordinates| {
            derivative(
                AircraftState::from_coordinates(*coordinates),
                acceleration_model_mps2,
                angular_acceleration_model_radps2,
            )
        });
        ensure!(
            coordinates.iter().all(|value| value.is_finite()),
            "FixedWing6DOF integration produced nonfinite state"
        );
        AircraftState::from_coordinates(coordinates).write_truth(truth);
        Ok(())
    }

    fn advance_launcher(&mut self, time_s: f64) -> f64 {
        match self.launch {
            LaunchState::Waiting => {
                if time_s > self.config.launch_time_s {
                    self.launch = LaunchState::Launching { start_s: time_s };
                }
                0.0
            }
            LaunchState::Launching { start_s } => {
                if self.config.launch_speed_mps / self.config.launch_accel_mps2 < time_s - start_s {
                    self.launch = LaunchState::Flying;
                }
                // Reference applies this force even on the transition-to-flight tick.
                self.config.launch_accel_mps2 * self.config.mass_kg
            }
            LaunchState::Flying => 0.0,
        }
    }
}

impl FixedWingConfig {
    fn thrust_n(&self, throttle: f64) -> f64 {
        // C++ Utilities::scale clamps again to the configured input range.
        // Defaults map zero throttle to -36,000 N (reverse thrust), not idle.
        // A non-reversing engine needs different limits; don't compensate in the PID.
        let throttle = throttle
            .clamp(-1.0, 1.0)
            .clamp(self.throttle_min, self.throttle_max);
        let scale =
            (self.thrust_max_n - self.thrust_min_n) / (self.throttle_max - self.throttle_min);
        (throttle - self.throttle_min) * scale + self.thrust_min_n
    }
}

fn parse_inertia(params: &FixedWingParams) -> Result<Matrix3<f64>> {
    let (text, factor) = match (&params.inertia_matrix_slug_ft_sq, &params.inertia_matrix) {
        (Some(_), Some(_)) => {
            bail!("FixedWing6DOF takes inertia_matrix_slug_ft_sq or inertia_matrix, not both")
        }
        (Some(text), None) => (text.as_str(), SLUG_FT_SQ_TO_KGM2),
        (None, Some(text)) => (text.as_str(), 1.0),
        (None, None) => (DEFAULT_INERTIA_SLUG_FT_SQ, SLUG_FT_SQ_TO_KGM2),
    };
    let text = text.replace(['[', ']'], " ");
    let mut values = Vec::new();
    for value in text.split_whitespace() {
        values.push(
            value
                .parse::<f64>()
                .context("invalid FixedWing6DOF inertia value")?
                * factor,
        );
    }
    ensure!(
        values.len() == 9 && values.iter().all(|value| value.is_finite()),
        "FixedWing6DOF inertia requires nine finite values"
    );
    let inertia = Matrix3::from_row_slice(&values);
    ensure!(
        (inertia - inertia.transpose()).norm() < 1e-9 && inertia.cholesky().is_some(),
        "FixedWing6DOF inertia must be symmetric positive definite"
    );
    Ok(inertia)
}

#[cfg(test)]
mod tests {
    use super::FixedWing6Dof;
    use crate::{
        Params,
        plugin::{Plugin, PluginParams},
    };
    use nalgebra::{Matrix3, Vector3};

    #[test]
    fn throttle_scaling_clamps_like_cpp_utilities_scale() -> anyhow::Result<()> {
        let params = Params::from([
            ("thrust_min".into(), "-36000".into()),
            ("thrust_max".into(), "36000".into()),
        ]);
        let config = FixedWing6Dof::configure(&PluginParams::text(&params))?;
        for (input, expected_n) in [
            (-2.0, -36000.0),
            (-1.0, -36000.0),
            (0.0, -36000.0),
            (0.5, 0.0),
            (1.0, 36000.0),
            (2.0, 36000.0),
        ] {
            assert!((config.thrust_n(input) - expected_n).abs() < 1e-9);
        }
        Ok(())
    }

    #[test]
    fn either_inertia_unit_takes_effect_but_not_both() -> anyhow::Result<()> {
        let slug = "[8090 0 1300] [0 25900 0] [1300 0 29200]";
        let default = FixedWing6Dof::configure(&PluginParams::text(&Params::new()))?;
        let params = Params::from([("inertia_matrix_slug_ft_sq".into(), slug.into())]);
        let explicit = FixedWing6Dof::configure(&PluginParams::text(&params))?;
        assert_eq!(default.inertia_kgm2, explicit.inertia_kgm2);

        let si = "[2 0 0] [0 3 0] [0 0 4]";
        let params = Params::from([("inertia_matrix".into(), si.into())]);
        let config = FixedWing6Dof::configure(&PluginParams::text(&params))?;
        assert_eq!(
            config.inertia_kgm2,
            Matrix3::from_diagonal(&Vector3::new(2.0, 3.0, 4.0))
        );

        let params = Params::from([("inertia_matrix".into(), "[1 2 3] [4 5 6]".into())]);
        assert!(FixedWing6Dof::configure(&PluginParams::text(&params)).is_err());
        let params = Params::from([
            ("inertia_matrix".into(), si.into()),
            ("inertia_matrix_slug_ft_sq".into(), slug.into()),
        ]);
        assert!(FixedWing6Dof::configure(&PluginParams::text(&params)).is_err());
        Ok(())
    }
}

//! Aerodynamic fixed-wing six-degree-of-freedom motion, with RK4 propagation.
//!
//! C++ counterpart: motion/FixedWing6DOF (GTRI, LGPL-3.0-or-later).
//! Motion phase: read throttle and surface angles, update entity truth.
//! Private body axes are forward/right/down; public truth remains ENU world.
//! The two-aircraft mission checks the port, not trimmed flight or ArduPilot readiness.

mod aerodynamics;
mod airframe;
mod dynamics;
mod landing_gear;

use anyhow::{Context, Result, bail, ensure};
use nalgebra::{Matrix3, UnitQuaternion};
use serde::Deserialize;

use crate::Terrain;
use crate::math::{self, KinematicState, Vec3};
use crate::plugin::{
    Frame, MotionContext, MotionModel, Plugin, PluginIo, PluginParams, Port, Ports, Unit, Update,
};

use aerodynamics::{Aerodynamics, Airframe};
pub(crate) use airframe::FixedWingAirframe;
use airframe::Vehicle;
use dynamics::{AircraftState, derivative, flip_yz};
use landing_gear::Wheel;

const SLUG_FT_SQ_TO_KGM2: f64 = 1.35581795;

/// Mission parameters in SI units unless marked. `airframe` names a built-in
/// aircraft whose numbers the vehicle keys below override; without it the
/// mission gives every vehicle key (see `airframe.rs`). The environment and
/// option keys have defaults.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct FixedWingParams {
    /// `a4d` or `rascal`, or absent for an aircraft given in full.
    airframe: Option<FixedWingAirframe>,
    #[serde(rename = "mass")]
    mass_kg: Option<f64>,
    #[serde(rename = "gravity_magnitude")]
    gravity_mps2: f64,
    /// Three bracketed rows, in slug·ft² or SI. Give at most one; with neither,
    /// the airframe's applies.
    inertia_matrix_slug_ft_sq: Option<String>,
    inertia_matrix: Option<String>,
    #[serde(rename = "air_density")]
    density_kgpm3: f64,
    #[serde(rename = "wing_span")]
    span_m: Option<f64>,
    #[serde(rename = "surface_area_of_wing")]
    area_m2: Option<f64>,
    #[serde(rename = "chord_length")]
    chord_m: Option<f64>,
    #[serde(rename = "efficiency_factor")]
    efficiency: Option<f64>,
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
    thrust_min_n: Option<f64>,
    #[serde(rename = "thrust_max")]
    thrust_max_n: Option<f64>,
    #[serde(rename = "delta_elevator_min")]
    elevator_min_rad: Option<f64>,
    #[serde(rename = "delta_elevator_max")]
    elevator_max_rad: Option<f64>,
    #[serde(rename = "delta_aileron_min")]
    aileron_min_rad: Option<f64>,
    #[serde(rename = "delta_aileron_max")]
    aileron_max_rad: Option<f64>,
    #[serde(rename = "delta_rudder_min")]
    rudder_min_rad: Option<f64>,
    #[serde(rename = "delta_rudder_max")]
    rudder_max_rad: Option<f64>,
    #[serde(rename = "C_D0")]
    c_d0: Option<f64>,
    #[serde(rename = "C_D_alpha")]
    c_d_alpha: Option<f64>,
    /// C++ reads the key WITH the trailing underscore.
    #[serde(rename = "C_D_delta_elevator_")]
    c_d_elevator: Option<f64>,
    #[serde(rename = "C_L0")]
    c_l0: Option<f64>,
    #[serde(rename = "C_L_alpha")]
    c_l_alpha: Option<f64>,
    #[serde(rename = "C_LQ")]
    c_lq: Option<f64>,
    /// Always zero in the reference; read only so a bad value is still an error.
    #[serde(rename = "C_L_alpha_dot")]
    _c_l_alpha_dot: Option<f64>,
    #[serde(rename = "C_L_delta_elevator")]
    c_l_elevator: Option<f64>,
    #[serde(rename = "C_Y_beta")]
    c_y_beta: Option<f64>,
    #[serde(rename = "C_Y_delta_rudder")]
    c_y_rudder: Option<f64>,
    #[serde(rename = "C_L_beta")]
    c_roll_beta: Option<f64>,
    #[serde(rename = "C_LP")]
    c_roll_p: Option<f64>,
    #[serde(rename = "C_LR")]
    c_roll_r: Option<f64>,
    #[serde(rename = "C_L_delta_aileron")]
    c_roll_aileron: Option<f64>,
    #[serde(rename = "C_L_delta_rudder")]
    c_roll_rudder: Option<f64>,
    #[serde(rename = "C_M0")]
    c_m0: Option<f64>,
    #[serde(rename = "C_M_alpha")]
    c_m_alpha: Option<f64>,
    #[serde(rename = "C_MQ")]
    c_mq: Option<f64>,
    /// Always zero in the reference; read only so a bad value is still an error.
    #[serde(rename = "C_M_alpha_dot")]
    _c_m_alpha_dot: Option<f64>,
    #[serde(rename = "C_M_delta_elevator")]
    c_m_elevator: Option<f64>,
    #[serde(rename = "C_N_beta")]
    c_n_beta: Option<f64>,
    #[serde(rename = "C_NP")]
    c_np: Option<f64>,
    #[serde(rename = "C_NR")]
    c_nr: Option<f64>,
    #[serde(rename = "C_N_delta_aileron")]
    c_n_aileron: Option<f64>,
    #[serde(rename = "C_N_delta_rudder")]
    c_n_rudder: Option<f64>,
    /// The key C++ does NOT read (no trailing underscore); rejected so a tuning
    /// value is not silently ignored.
    #[serde(rename = "C_D_delta_elevator")]
    c_d_elevator_ignored_by_cpp: Option<f64>,
    /// Lift coefficient against angle of attack, as `[alpha_rad, C_L]` rows
    /// sorted by angle (YAML only). Replaces `C_L0 + C_L_alpha * alpha`, so a
    /// lift that peaks and falls past the critical angle models a stall;
    /// interpolated linearly and held at the end values, like a JSBSim table.
    /// Rust-only: C++ has linear lift, which empty (the default) keeps.
    lift_table: Vec<[f64; 2]>,
    /// Drag coefficient against angle of attack, as `[alpha_rad, C_D]` rows
    /// sorted by angle (YAML only), read like `lift_table`. Replaces
    /// `C_D0 + C_D_alpha * |alpha|`, so drag can rise steeply past the stall;
    /// induced and elevator drag are still added. Rust-only; empty keeps C++.
    drag_table: Vec<[f64; 2]>,
    use_ground_model: bool,
    /// Wheels that roll on the ground (YAML only; see `landing_gear.rs`).
    /// Replaces the flat spring, so `use_ground_model` must be false.
    landing_gear: Vec<Wheel>,
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
            airframe: None,
            mass_kg: None,
            gravity_mps2: 9.81,
            inertia_matrix_slug_ft_sq: None,
            inertia_matrix: None,
            density_kgpm3: 1.225,
            span_m: None,
            area_m2: None,
            chord_m: None,
            efficiency: None,
            wind_east_mps: 0.0,
            wind_north_mps: 0.0,
            wind_up_mps: 0.0,
            throttle_min: 0.0,
            throttle_max: 1.0,
            thrust_min_n: None,
            thrust_max_n: None,
            elevator_min_rad: None,
            elevator_max_rad: None,
            aileron_min_rad: None,
            aileron_max_rad: None,
            rudder_min_rad: None,
            rudder_max_rad: None,
            c_d0: None,
            c_d_alpha: None,
            c_d_elevator: None,
            c_l0: None,
            c_l_alpha: None,
            c_lq: None,
            _c_l_alpha_dot: None,
            c_l_elevator: None,
            c_y_beta: None,
            c_y_rudder: None,
            c_roll_beta: None,
            c_roll_p: None,
            c_roll_r: None,
            c_roll_aileron: None,
            c_roll_rudder: None,
            c_m0: None,
            c_m_alpha: None,
            c_mq: None,
            _c_m_alpha_dot: None,
            c_m_elevator: None,
            c_n_beta: None,
            c_np: None,
            c_nr: None,
            c_n_aileron: None,
            c_n_rudder: None,
            c_d_elevator_ignored_by_cpp: None,
            lift_table: Vec::new(),
            drag_table: Vec::new(),
            use_ground_model: true,
            landing_gear: Vec::new(),
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
    landing_gear: Vec<Wheel>,
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
        let vehicle = vehicle(&params)?;
        ensure!(
            vehicle.elevator_limits_rad[0] <= vehicle.elevator_limits_rad[1]
                && vehicle.aileron_limits_rad[0] <= vehicle.aileron_limits_rad[1]
                && vehicle.rudder_limits_rad[0] <= vehicle.rudder_limits_rad[1],
            "FixedWing6DOF requires each delta_*_min <= delta_*_max"
        );
        ensure!(
            params.landing_gear.is_empty() || !params.use_ground_model,
            "FixedWing6DOF: set use_ground_model to false with landing_gear; the wheels replace it"
        );
        check_alpha_table("lift_table", "C_L", &params.lift_table)?;
        check_alpha_table("drag_table", "C_D", &params.drag_table)?;
        landing_gear::check_wheels(&params.landing_gear)?;
        let inertia_kgm2 = vehicle.inertia_kgm2;
        let inverse_inertia = inertia_kgm2
            .try_inverse()
            .context("FixedWing6DOF inertia must be invertible")?;
        let config = FixedWingConfig {
            mass_kg: vehicle.mass_kg,
            gravity_mps2: params.gravity_mps2,
            inertia_kgm2,
            inverse_inertia,
            density_kgpm3: params.density_kgpm3,
            span_m: vehicle.span_m,
            area_m2: vehicle.area_m2,
            chord_m: vehicle.chord_m,
            efficiency: vehicle.efficiency,
            wind_world_mps: Vec3::new(
                params.wind_east_mps,
                params.wind_north_mps,
                params.wind_up_mps,
            ),
            throttle_min: params.throttle_min,
            throttle_max: params.throttle_max,
            thrust_min_n: vehicle.thrust_min_n,
            thrust_max_n: vehicle.thrust_max_n,
            elevator_limits_rad: vehicle.elevator_limits_rad,
            aileron_limits_rad: vehicle.aileron_limits_rad,
            rudder_limits_rad: vehicle.rudder_limits_rad,
            aerodynamics: Aerodynamics::new(&vehicle, &params.lift_table, &params.drag_table),
            use_ground_model: params.use_ground_model,
            landing_gear: params.landing_gear,
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
    fn initialize(&mut self, context: &mut MotionContext<'_>) -> Result<()> {
        // The inherited ground model puts flat ground at z = 0, which terrain
        // replaces; with both, an aircraft over a valley below z = 0 would be
        // pushed up in mid-air.
        ensure!(
            context.terrain.is_none() || !self.config.use_ground_model,
            "FixedWing6DOF: set use_ground_model to false when terrain is configured; \
             add TerrainCollision to remove aircraft that hit terrain"
        );
        if let Some(terrain) = context.terrain {
            ensure!(
                self.config.landing_gear.is_empty() || terrain.missing_samples() == 0,
                "FixedWing6DOF landing gear needs complete terrain; {} samples have no height",
                terrain.missing_samples()
            );
        }
        Ok(())
    }

    fn step(&mut self, context: &mut MotionContext<'_>, io: &mut PluginIo) -> Result<Update> {
        let control = Control {
            throttle: io.read("throttle")?,
            elevator_rad: io.read("elevator")?,
            aileron_rad: io.read("aileron")?,
            rudder_rad: io.read("rudder")?,
        };
        if !self.config.landing_gear.is_empty() {
            landing_gear::check_step(
                &self.config.landing_gear,
                self.config.mass_kg,
                context.time.dt_s,
            )?;
        }
        self.integrate(
            context.truth,
            control,
            context.terrain,
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
        terrain: Option<&Terrain>,
        time_s: f64,
        dt_s: f64,
    ) -> Result<()> {
        let catapult_force_n = self.advance_launcher(time_s);
        let flying = matches!(self.launch, LaunchState::Flying);
        let config = &self.config;
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
        // Forces and moments are recomputed from each RK4 stage's state. C++
        // holds them at the start-of-step state, which makes the step first
        // order; the comparison branch fixes that (docs/REFERENCE_NOTES.md).
        let mut coordinates = AircraftState::from_truth(truth).coordinates();
        math::rk4(&mut coordinates, dt_s, |coordinates| {
            let stage = AircraftState::from_coordinates(*coordinates);
            let (acceleration_model_mps2, angular_acceleration_model_radps2) =
                config.accelerations(&stage, control, flying, catapult_force_n, terrain, dt_s);
            derivative(
                stage,
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
    /// When the launcher releases the aircraft, or `None` without a launcher.
    pub fn launch_time_s(&self) -> Option<f64> {
        self.use_launcher.then_some(self.launch_time_s)
    }

    /// Linear and angular acceleration in the model frame for one state.
    fn accelerations(
        &self,
        state: &AircraftState,
        control: Control,
        flying: bool,
        catapult_force_n: f64,
        terrain: Option<&Terrain>,
        dt_s: f64,
    ) -> (Vec3, Vec3) {
        let rotation = UnitQuaternion::new_normalize(state.attitude_model);
        let wind_model_mps = rotation.inverse_transform_vector(&flip_yz(self.wind_world_mps));
        // Deliberate reference quirk: wind is ADDED to velocity, not subtracted.
        let air_velocity_model_mps = state.velocity_model_mps + wind_model_mps;
        let airframe = Airframe {
            density_kgpm3: self.density_kgpm3,
            span_m: self.span_m,
            area_m2: self.area_m2,
            chord_m: self.chord_m,
            efficiency: self.efficiency,
        };
        let mut loads = self.aerodynamics.loads(
            &airframe,
            air_velocity_model_mps,
            state.rates_model_radps,
            control,
        );
        let weight_model_n = rotation.inverse_transform_vector(&Vec3::new(
            0.0,
            0.0,
            self.mass_kg * self.gravity_mps2,
        ));
        let thrust_model_n = Vec3::new(self.thrust_n(control.throttle), 0.0, 0.0);
        loads.force_model_n = weight_model_n + thrust_model_n + loads.force_model_n;

        if !self.landing_gear.is_empty() {
            let (force_model_n, moment_model_nm) = landing_gear::loads(
                &self.landing_gear,
                state,
                &rotation,
                terrain,
                control.rudder_rad,
                self.mass_kg,
                dt_s,
            );
            loads.force_model_n += force_model_n;
            loads.moment_model_nm += moment_model_nm;
        }
        if self.use_ground_model && state.position_world_m.z < 0.0 {
            // Inherited ground model: damping is only 40 N*s/m, not mass-scaled.
            // This is not a validated landing model for the default eight-tonne
            // aircraft; `landing_gear` is the real ground contact.
            let ground_force_n = (-400.0 * self.mass_kg * state.position_world_m.z
                - 40.0 * state.velocity_world_mps.z)
                .max(0.0);
            loads.force_model_n +=
                rotation.inverse_transform_vector(&Vec3::new(0.0, 0.0, -ground_force_n));
        }
        if !flying {
            loads.force_model_n = Vec3::zeros();
            loads.moment_model_nm = Vec3::zeros();
        }
        loads.force_model_n.x += catapult_force_n;
        let rates = state.rates_model_radps;
        (
            loads.force_model_n / self.mass_kg,
            self.inverse_inertia
                * (loads.moment_model_nm - rates.cross(&(self.inertia_kgm2 * rates))),
        )
    }

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

/// An angle-of-attack table is empty (unused) or has two or more finite rows
/// in increasing angle.
fn check_alpha_table(key: &str, coefficient: &str, table: &[[f64; 2]]) -> Result<()> {
    ensure!(
        table.is_empty()
            || (table.len() >= 2
                && table.iter().flatten().all(|value| value.is_finite())
                && table.windows(2).all(|rows| rows[0][0] < rows[1][0])),
        "FixedWing6DOF {key} needs at least two finite [alpha_rad, {coefficient}] rows in increasing angle"
    );
    Ok(())
}

/// The aircraft's numbers: the mission's keys over the named airframe's.
fn vehicle(params: &FixedWingParams) -> Result<Vehicle> {
    let preset = params
        .airframe
        .map(FixedWingAirframe::vehicle)
        .transpose()?;
    let preset = preset.as_ref();
    let pick = |key: &str, value: Option<f64>, preset_value: Option<f64>| {
        value.or(preset_value).with_context(|| {
            format!(
                "FixedWing6DOF: without `airframe`, the mission must give every vehicle \
                 number; `{key}` is missing"
            )
        })
    };
    let inertia_kgm2 = match (&params.inertia_matrix_slug_ft_sq, &params.inertia_matrix) {
        (Some(_), Some(_)) => {
            bail!("FixedWing6DOF takes inertia_matrix_slug_ft_sq or inertia_matrix, not both")
        }
        (Some(text), None) => parse_inertia_text(text, true)?,
        (None, Some(text)) => parse_inertia_text(text, false)?,
        (None, None) => preset.map(|vehicle| vehicle.inertia_kgm2).context(
            "FixedWing6DOF: without `airframe`, the mission must give an inertia matrix",
        )?,
    };
    Ok(Vehicle {
        mass_kg: pick("mass", params.mass_kg, preset.map(|v| v.mass_kg))?,
        inertia_kgm2,
        span_m: pick("wing_span", params.span_m, preset.map(|v| v.span_m))?,
        area_m2: pick(
            "surface_area_of_wing",
            params.area_m2,
            preset.map(|v| v.area_m2),
        )?,
        chord_m: pick("chord_length", params.chord_m, preset.map(|v| v.chord_m))?,
        efficiency: pick(
            "efficiency_factor",
            params.efficiency,
            preset.map(|v| v.efficiency),
        )?,
        thrust_min_n: pick(
            "thrust_min",
            params.thrust_min_n,
            preset.map(|v| v.thrust_min_n),
        )?,
        thrust_max_n: pick(
            "thrust_max",
            params.thrust_max_n,
            preset.map(|v| v.thrust_max_n),
        )?,
        elevator_limits_rad: [
            pick(
                "delta_elevator_min",
                params.elevator_min_rad,
                preset.map(|v| v.elevator_limits_rad[0]),
            )?,
            pick(
                "delta_elevator_max",
                params.elevator_max_rad,
                preset.map(|v| v.elevator_limits_rad[1]),
            )?,
        ],
        aileron_limits_rad: [
            pick(
                "delta_aileron_min",
                params.aileron_min_rad,
                preset.map(|v| v.aileron_limits_rad[0]),
            )?,
            pick(
                "delta_aileron_max",
                params.aileron_max_rad,
                preset.map(|v| v.aileron_limits_rad[1]),
            )?,
        ],
        rudder_limits_rad: [
            pick(
                "delta_rudder_min",
                params.rudder_min_rad,
                preset.map(|v| v.rudder_limits_rad[0]),
            )?,
            pick(
                "delta_rudder_max",
                params.rudder_max_rad,
                preset.map(|v| v.rudder_limits_rad[1]),
            )?,
        ],
        c_d0: pick("C_D0", params.c_d0, preset.map(|v| v.c_d0))?,
        c_d_alpha: pick("C_D_alpha", params.c_d_alpha, preset.map(|v| v.c_d_alpha))?,
        c_d_elevator: pick(
            "C_D_delta_elevator_",
            params.c_d_elevator,
            preset.map(|v| v.c_d_elevator),
        )?,
        c_l0: pick("C_L0", params.c_l0, preset.map(|v| v.c_l0))?,
        c_l_alpha: pick("C_L_alpha", params.c_l_alpha, preset.map(|v| v.c_l_alpha))?,
        c_lq: pick("C_LQ", params.c_lq, preset.map(|v| v.c_lq))?,
        c_l_elevator: pick(
            "C_L_delta_elevator",
            params.c_l_elevator,
            preset.map(|v| v.c_l_elevator),
        )?,
        c_y_beta: pick("C_Y_beta", params.c_y_beta, preset.map(|v| v.c_y_beta))?,
        c_y_rudder: pick(
            "C_Y_delta_rudder",
            params.c_y_rudder,
            preset.map(|v| v.c_y_rudder),
        )?,
        c_roll_beta: pick(
            "C_L_beta",
            params.c_roll_beta,
            preset.map(|v| v.c_roll_beta),
        )?,
        c_roll_p: pick("C_LP", params.c_roll_p, preset.map(|v| v.c_roll_p))?,
        c_roll_r: pick("C_LR", params.c_roll_r, preset.map(|v| v.c_roll_r))?,
        c_roll_aileron: pick(
            "C_L_delta_aileron",
            params.c_roll_aileron,
            preset.map(|v| v.c_roll_aileron),
        )?,
        c_roll_rudder: pick(
            "C_L_delta_rudder",
            params.c_roll_rudder,
            preset.map(|v| v.c_roll_rudder),
        )?,
        c_m0: pick("C_M0", params.c_m0, preset.map(|v| v.c_m0))?,
        c_m_alpha: pick("C_M_alpha", params.c_m_alpha, preset.map(|v| v.c_m_alpha))?,
        c_mq: pick("C_MQ", params.c_mq, preset.map(|v| v.c_mq))?,
        c_m_elevator: pick(
            "C_M_delta_elevator",
            params.c_m_elevator,
            preset.map(|v| v.c_m_elevator),
        )?,
        c_n_beta: pick("C_N_beta", params.c_n_beta, preset.map(|v| v.c_n_beta))?,
        c_np: pick("C_NP", params.c_np, preset.map(|v| v.c_np))?,
        c_nr: pick("C_NR", params.c_nr, preset.map(|v| v.c_nr))?,
        c_n_aileron: pick(
            "C_N_delta_aileron",
            params.c_n_aileron,
            preset.map(|v| v.c_n_aileron),
        )?,
        c_n_rudder: pick(
            "C_N_delta_rudder",
            params.c_n_rudder,
            preset.map(|v| v.c_n_rudder),
        )?,
    })
}

/// Three bracketed rows, in slug ft^2 (`slug_ft_sq`) or kg m^2.
fn parse_inertia_text(text: &str, slug_ft_sq: bool) -> Result<Matrix3<f64>> {
    let factor = if slug_ft_sq { SLUG_FT_SQ_TO_KGM2 } else { 1.0 };
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
            ("airframe".into(), "a4d".into()),
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
        let a4d = || -> Params { Params::from([("airframe".into(), "a4d".into())]) };
        let profile = FixedWing6Dof::configure(&PluginParams::text(&a4d()))?;
        let mut params = a4d();
        params.insert("inertia_matrix_slug_ft_sq".into(), slug.into());
        let explicit = FixedWing6Dof::configure(&PluginParams::text(&params))?;
        assert_eq!(profile.inertia_kgm2, explicit.inertia_kgm2);

        let si = "[2 0 0] [0 3 0] [0 0 4]";
        let mut params = a4d();
        params.insert("inertia_matrix".into(), si.into());
        let config = FixedWing6Dof::configure(&PluginParams::text(&params))?;
        assert_eq!(
            config.inertia_kgm2,
            Matrix3::from_diagonal(&Vector3::new(2.0, 3.0, 4.0))
        );

        let mut params = a4d();
        params.insert("inertia_matrix".into(), "[1 2 3] [4 5 6]".into());
        assert!(FixedWing6Dof::configure(&PluginParams::text(&params)).is_err());
        let mut params = a4d();
        params.insert("inertia_matrix".into(), si.into());
        params.insert("inertia_matrix_slug_ft_sq".into(), slug.into());
        assert!(FixedWing6Dof::configure(&PluginParams::text(&params)).is_err());
        Ok(())
    }

    #[test]
    fn a_named_airframe_with_an_override_equals_the_same_aircraft_written_out() -> anyhow::Result<()>
    {
        // The Rascal in full, every vehicle key, with C_MQ changed.
        let full: Params = [
            ("mass", "5.91"),
            (
                "inertia_matrix_slug_ft_sq",
                "[1.95 0 0] [0 1.55 0] [0 0 1.91]",
            ),
            ("wing_span", "2.70"),
            ("surface_area_of_wing", "0.982"),
            ("chord_length", "0.351"),
            ("efficiency_factor", "0.995"),
            ("thrust_min", "0"),
            ("thrust_max", "25"),
            ("delta_elevator_min", "-0.30"),
            ("delta_elevator_max", "0.30"),
            ("delta_aileron_min", "-0.35"),
            ("delta_aileron_max", "0.35"),
            ("delta_rudder_min", "-0.35"),
            ("delta_rudder_max", "0.35"),
            ("C_D0", "0.028"),
            ("C_D_alpha", "0"),
            ("C_D_delta_elevator_", "0.01"),
            ("C_L0", "0.25"),
            ("C_L_alpha", "5"),
            ("C_LQ", "0"),
            ("C_L_delta_elevator", "0.2"),
            ("C_Y_beta", "-1"),
            ("C_Y_delta_rudder", "0"),
            ("C_L_beta", "-0.1"),
            ("C_LP", "-0.4"),
            ("C_LR", "0.15"),
            ("C_L_delta_aileron", "0.13"),
            ("C_L_delta_rudder", "0.01"),
            ("C_M0", "0"),
            ("C_M_alpha", "-0.5"),
            ("C_MQ", "-10"),
            ("C_M_delta_elevator", "-0.5"),
            ("C_N_beta", "0.12"),
            ("C_NP", "0"),
            ("C_NR", "-0.15"),
            ("C_N_delta_aileron", "-0.03"),
            ("C_N_delta_rudder", "-0.05"),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect();
        let named = Params::from([
            ("airframe".into(), "rascal".into()),
            ("C_MQ".into(), "-10".into()),
        ]);
        let resolve = |params: &Params| -> anyhow::Result<_> {
            super::vehicle(&PluginParams::text(params).parse()?)
        };
        let from_name = resolve(&named)?;
        assert_eq!(from_name, resolve(&full)?);
        assert_eq!(from_name.c_mq, -10.0);
        // Without the override, the profile's own value.
        let plain = resolve(&Params::from([("airframe".into(), "rascal".into())]))?;
        assert_eq!(plain.c_mq, -12.0);
        Ok(())
    }

    #[test]
    fn without_an_airframe_every_vehicle_number_is_required() {
        let error = FixedWing6Dof::configure(&PluginParams::text(&Params::new()))
            .err()
            .expect("no airframe and no numbers");
        assert!(error.to_string().contains("without `airframe`"), "{error}");
        let params = Params::from([("airframe".into(), "cessna".into())]);
        assert!(FixedWing6Dof::configure(&PluginParams::text(&params)).is_err());
    }
}

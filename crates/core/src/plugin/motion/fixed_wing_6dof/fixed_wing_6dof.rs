//! Aerodynamic fixed-wing six-degree-of-freedom motion, with RK4 propagation.
//!
//! Derived from SCRIMMAGE: motion/FixedWing6DOF (GTRI, LGPL-3.0-or-later).
//! Motion phase: read throttle and surface angles, update entity truth.
//! Private frames are the aerospace ones, forward/right/down body and
//! north/east/down world; public truth is the engine's ENU and forward/left/up.
//! The two-aircraft mission checks the port, not trimmed flight or ArduPilot readiness.
//!
//! Fidelity
//! - Source: a rigid body under aerodynamic forces and moments built from
//!   linear stability derivatives (lift, drag, side force, roll, pitch, yaw),
//!   induced drag from an Oswald efficiency, and thrust linear in throttle;
//!   RK4 with every load evaluated at each stage. Beyond SCRIMMAGE's
//!   equations, each off unless the airframe or mission sets it: lift, drag,
//!   and sideslip-drag tables and Mach tables for the control powers, all
//!   read like a JSBSim `<table>`; loads applied at an aerodynamic reference
//!   point, with the moment transferred to the center of mass; alpha-dot
//!   pitch damping; a constant yaw moment; and JSBSim's signed elevator
//!   drag. Optional landing gear follows JSBSim's FGLGear
//!   (`landing_gear.rs`). Aircraft numbers come from `airframe.rs`: the A-4D
//!   (Nelson) or the Rascal (ArduPilot's JSBSim Rascal, with its tables and
//!   terms), or a mission gives them in full.
//! - Validated against: SCRIMMAGE's FixedWing6DOF on both aircraft
//!   (`fixed-wing-6dof.yaml`, `fixed-wing-6dof-rascal.yaml`, the latter with
//!   the Rascal's extra terms off); step-size convergence
//!   (`reference/rk4_loads.py`); the landing gear's resting height, drops,
//!   slopes, friction stops, and convergence
//!   (`crates/core/tests/landing_gear.rs`); closed-loop flight by ArduPlane,
//!   takeoff to landing, repeated byte for byte (`reference/stall_landing.py`);
//!   and the Rascal against ArduPilot's JSBSim Rascal run in JSBSim: loads
//!   to rounding at 378 held states (`jsbsim_rascal_samples` in
//!   `aerodynamics.rs`) and 5 s open-loop glides within a set tolerance
//!   (`reference/jsbsim_check.py`). Not against any flight data.
//! - Valid range: attached flow at moderate angle of attack, where linear
//!   derivatives or the tables hold (past the stall only with the tables);
//!   subsonic flight, since only the control powers change with Mach; near
//!   the flight condition an aircraft's derivatives were taken at (the
//!   Rascal around 20 m/s).
//! - Not modeled (named zeros in `derivative`): propeller slipstream over
//!   the wing and tail, propeller torque and gyroscopic moments, and ground
//!   effect. Also: separate wing stalls (so no spin), unsteady aerodynamics
//!   beyond the alpha-dot pitch damping, engine and propeller dynamics
//!   (thrust follows throttle at once), compressibility beyond the Mach
//!   tables, fuel burn, an altitude-dependent atmosphere, and gusts.
//! - To go further: a propeller model from measured data (thrust and torque
//!   against advance ratio, as published for small propellers) would replace
//!   the linear thrust and fill the slipstream and torque zeros; a measured
//!   lift and moment change near the ground would fill ground effect; a
//!   standard atmosphere would let long descents change the air density.

mod aerodynamics;
mod airframe;
mod dynamics;
mod landing_gear;

use anyhow::{Context, Result, bail, ensure};
use nalgebra::Matrix3;
use serde::Deserialize;

use crate::Terrain;
use crate::math::{self, KinematicState, Quaternion, Vec3};
use crate::plugin::{Frame, MotionContext, MotionModel, PluginIo, Port, Ports, Unit};

use aerodynamics::Aerodynamics;
pub(crate) use airframe::FixedWingAirframe;
use airframe::Vehicle;
use dynamics::{AircraftRate, AircraftState, ned_from_enu};
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
    mass_kg: Option<f64>,
    gravity_mps2: f64,
    /// Three rows, in slug·ft² or SI. Give at most one; with neither, the
    /// airframe's applies.
    inertia_slug_ft2: Option<[[f64; 3]; 3]>,
    inertia_kgm2: Option<[[f64; 3]; 3]>,
    air_density_kgpm3: f64,
    wing_span_m: Option<f64>,
    wing_area_m2: Option<f64>,
    mean_chord_m: Option<f64>,
    oswald_efficiency: Option<f64>,
    /// Steady wind: the air's velocity over the ground, `[east, north, up]`.
    wind_mps: [f64; 3],
    min_throttle: f64,
    max_throttle: f64,
    min_thrust_n: Option<f64>,
    max_thrust_n: Option<f64>,
    min_elevator_rad: Option<f64>,
    max_elevator_rad: Option<f64>,
    min_aileron_rad: Option<f64>,
    max_aileron_rad: Option<f64>,
    min_rudder_rad: Option<f64>,
    max_rudder_rad: Option<f64>,
    c_drag_0: Option<f64>,
    c_drag_alpha: Option<f64>,
    /// The key has a trailing underscore.
    c_drag_elevator: Option<f64>,
    c_lift_0: Option<f64>,
    c_lift_alpha: Option<f64>,
    c_lift_q: Option<f64>,
    c_lift_elevator: Option<f64>,
    c_side_beta: Option<f64>,
    c_side_rudder: Option<f64>,
    c_roll_beta: Option<f64>,
    c_roll_p: Option<f64>,
    c_roll_r: Option<f64>,
    c_roll_aileron: Option<f64>,
    c_roll_rudder: Option<f64>,
    c_pitch_0: Option<f64>,
    c_pitch_alpha: Option<f64>,
    c_pitch_q: Option<f64>,
    c_pitch_elevator: Option<f64>,
    c_yaw_beta: Option<f64>,
    c_yaw_p: Option<f64>,
    c_yaw_r: Option<f64>,
    c_yaw_aileron: Option<f64>,
    c_yaw_rudder: Option<f64>,
    // The tables and terms below are off (empty or zero) unless the airframe
    // or the mission sets them; a mission without an airframe may leave them
    // out.
    /// Where the coefficients act, relative to the center of mass,
    /// `[forward, right, down]` in meters; the moment about the center of
    /// mass adds this arm crossed with the force. Zero applies the loads at
    /// the center of mass.
    aero_reference_frd_m: Option<[f64; 3]>,
    /// Pitch damping from the rate of change of angle of attack, C_m_alphadot,
    /// per radian, entering as alpha_dot c / 2V.
    c_pitch_alpha_dot: Option<f64>,
    /// Yaw moment coefficient at zero sideslip, rates, and deflection, C_n0.
    c_yaw_0: Option<f64>,
    /// Lift coefficient against angle of attack, as `[alpha_rad, C_L]` rows
    /// sorted by angle (YAML only). Replaces `c_lift_0 + c_lift_alpha * alpha`, so a
    /// lift that peaks and falls past the critical angle models a stall;
    /// interpolated linearly and held at the end values, like a JSBSim table.
    /// Empty keeps linear lift.
    lift_table: Option<Vec<[f64; 2]>>,
    /// Drag coefficient against angle of attack, as `[alpha_rad, C_D]` rows
    /// sorted by angle (YAML only), read like `lift_table`. Replaces
    /// `c_drag_0 + c_drag_alpha * |alpha|`, so drag can rise steeply past the stall;
    /// induced, elevator, and sideslip drag are still added. Empty keeps the
    /// linear drag.
    drag_table: Option<Vec<[f64; 2]>>,
    /// Drag coefficient added from sideslip, as `[beta_rad, C_D]` rows sorted
    /// by angle (YAML only). Empty adds none.
    sideslip_drag_table: Option<Vec<[f64; 2]>>,
    /// Aileron roll power against Mach, as `[mach, C_l_delta_a]` rows (YAML
    /// only), replacing `c_roll_aileron`; and the elevator's pitch power
    /// likewise, replacing `c_pitch_elevator`. Mach is airspeed over
    /// `speed_of_sound_mps`. Empty keeps the constant.
    aileron_power_mach_table: Option<Vec<[f64; 2]>>,
    elevator_power_mach_table: Option<Vec<[f64; 2]>>,
    /// The elevator's drag from the signed deflection (up-elevator reduces
    /// it), as JSBSim's Rascal file has it, instead of from its size. A
    /// JSBSim-compatibility form, not part of any airframe.
    signed_elevator_drag: bool,
    /// Environment: the speed of sound, for the Mach tables only.
    speed_of_sound_mps: f64,
    enable_ground_spring: bool,
    /// Wheels that roll on the ground (YAML only; see `landing_gear.rs`).
    /// Replaces the flat spring, so `enable_ground_spring` must be false.
    landing_gear: Vec<Wheel>,
    enable_launcher: bool,
    launch_at_s: f64,
    launch_acceleration_mps2: f64,
    launch_speed_mps: f64,
}

impl Default for FixedWingParams {
    fn default() -> Self {
        Self {
            airframe: None,
            mass_kg: None,
            gravity_mps2: 9.81,
            inertia_slug_ft2: None,
            inertia_kgm2: None,
            air_density_kgpm3: 1.225,
            wing_span_m: None,
            wing_area_m2: None,
            mean_chord_m: None,
            oswald_efficiency: None,
            wind_mps: [0.0; 3],
            min_throttle: 0.0,
            max_throttle: 1.0,
            min_thrust_n: None,
            max_thrust_n: None,
            min_elevator_rad: None,
            max_elevator_rad: None,
            min_aileron_rad: None,
            max_aileron_rad: None,
            min_rudder_rad: None,
            max_rudder_rad: None,
            c_drag_0: None,
            c_drag_alpha: None,
            c_drag_elevator: None,
            c_lift_0: None,
            c_lift_alpha: None,
            c_lift_q: None,
            c_lift_elevator: None,
            c_side_beta: None,
            c_side_rudder: None,
            c_roll_beta: None,
            c_roll_p: None,
            c_roll_r: None,
            c_roll_aileron: None,
            c_roll_rudder: None,
            c_pitch_0: None,
            c_pitch_alpha: None,
            c_pitch_q: None,
            c_pitch_elevator: None,
            c_yaw_beta: None,
            c_yaw_p: None,
            c_yaw_r: None,
            c_yaw_aileron: None,
            c_yaw_rudder: None,
            aero_reference_frd_m: None,
            c_pitch_alpha_dot: None,
            c_yaw_0: None,
            lift_table: None,
            drag_table: None,
            sideslip_drag_table: None,
            aileron_power_mach_table: None,
            elevator_power_mach_table: None,
            signed_elevator_drag: false,
            // The standard atmosphere's sea-level value.
            speed_of_sound_mps: 340.294,
            enable_ground_spring: true,
            landing_gear: Vec::new(),
            enable_launcher: false,
            launch_at_s: 30.0,
            launch_acceleration_mps2: 200.0,
            launch_speed_mps: 20.0,
        }
    }
}

#[derive(Clone, Deserialize)]
#[serde(try_from = "FixedWingParams")]
pub struct FixedWingConfig {
    mass_kg: f64,
    gravity_mps2: f64,
    inertia_kgm2: Matrix3<f64>,
    inverse_inertia: Matrix3<f64>,
    wind_world_mps: Vec3,
    min_throttle: f64,
    max_throttle: f64,
    min_thrust_n: f64,
    max_thrust_n: f64,
    elevator_limits_rad: [f64; 2],
    aileron_limits_rad: [f64; 2],
    rudder_limits_rad: [f64; 2],
    aerodynamics: Aerodynamics,
    enable_ground_spring: bool,
    landing_gear: Vec<Wheel>,
    enable_launcher: bool,
    launch_at_s: f64,
    launch_acceleration_mps2: f64,
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

// The mission's parameters become the config through serde's `try_from`, so
// the trait's default `configure` (`params.parse()`) fits this plugin too.
impl TryFrom<FixedWingParams> for FixedWingConfig {
    type Error = anyhow::Error;
    fn try_from(params: FixedWingParams) -> Result<Self> {
        let vehicle = vehicle(&params)?;
        ensure!(
            vehicle.elevator_limits_rad[0] <= vehicle.elevator_limits_rad[1]
                && vehicle.aileron_limits_rad[0] <= vehicle.aileron_limits_rad[1]
                && vehicle.rudder_limits_rad[0] <= vehicle.rudder_limits_rad[1],
            "FixedWing6DOF requires each min_*_rad <= max_*_rad"
        );
        ensure!(
            params.landing_gear.is_empty() || !params.enable_ground_spring,
            "FixedWing6DOF: set enable_ground_spring to false with landing_gear; the wheels replace it"
        );
        check_table("lift_table", "alpha_rad", "C_L", &vehicle.lift_table)?;
        check_table("drag_table", "alpha_rad", "C_D", &vehicle.drag_table)?;
        check_table(
            "sideslip_drag_table",
            "beta_rad",
            "C_D",
            &vehicle.sideslip_drag_table,
        )?;
        check_table(
            "aileron_power_mach_table",
            "mach",
            "C_l_delta_a",
            &vehicle.aileron_power_mach_table,
        )?;
        check_table(
            "elevator_power_mach_table",
            "mach",
            "C_m_delta_e",
            &vehicle.elevator_power_mach_table,
        )?;
        ensure!(
            params.speed_of_sound_mps > 0.0,
            "FixedWing6DOF speed_of_sound_mps must be positive"
        );
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
            wind_world_mps: Vec3::from(params.wind_mps),
            min_throttle: params.min_throttle,
            max_throttle: params.max_throttle,
            min_thrust_n: vehicle.min_thrust_n,
            max_thrust_n: vehicle.max_thrust_n,
            elevator_limits_rad: vehicle.elevator_limits_rad,
            aileron_limits_rad: vehicle.aileron_limits_rad,
            rudder_limits_rad: vehicle.rudder_limits_rad,
            aerodynamics: Aerodynamics::new(
                &vehicle,
                params.air_density_kgpm3,
                params.speed_of_sound_mps,
                params.signed_elevator_drag,
            ),
            enable_ground_spring: params.enable_ground_spring,
            landing_gear: params.landing_gear,
            enable_launcher: params.enable_launcher,
            launch_at_s: params.launch_at_s,
            launch_acceleration_mps2: params.launch_acceleration_mps2,
            launch_speed_mps: params.launch_speed_mps,
        };
        ensure!(
            vehicle.mass_kg > 0.0
                && params.air_density_kgpm3 >= 0.0
                && params.gravity_mps2 >= 0.0
                && vehicle.wing_span_m > 0.0
                && vehicle.wing_area_m2 > 0.0
                && vehicle.mean_chord_m > 0.0
                && vehicle.oswald_efficiency > 0.0,
            "FixedWing6DOF requires positive mass and geometry, nonnegative air density and gravity"
        );
        ensure!(
            config.min_throttle < config.max_throttle && config.min_thrust_n <= config.max_thrust_n,
            "FixedWing6DOF has invalid throttle/thrust limits"
        );
        ensure!(
            !config.enable_launcher
                || (config.launch_acceleration_mps2 > 0.0 && config.launch_speed_mps >= 0.0),
            "FixedWing6DOF launcher requires positive acceleration and nonnegative speed"
        );
        Ok(config)
    }
}

impl MotionModel for FixedWing6Dof {
    type Config = FixedWingConfig;

    fn new(config: &FixedWingConfig) -> Self {
        Self {
            config: config.clone(),
            launch: if config.enable_launcher {
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

    fn initialize(&mut self, context: &mut MotionContext) -> Result<()> {
        // The inherited ground model puts flat ground at z = 0, which terrain
        // replaces; with both, an aircraft over a valley below z = 0 would be
        // pushed up in mid-air.
        ensure!(
            context.terrain.is_none() || !self.config.enable_ground_spring,
            "FixedWing6DOF: set enable_ground_spring to false when terrain is configured; \
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

    fn step(&mut self, context: &mut MotionContext, io: &mut PluginIo) -> Result<()> {
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
        Ok(())
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
        // Forces and moments are recomputed from each RK4 stage's state; holding
        // them at the start-of-step state would make the step first order.
        let state = math::rk4(AircraftState::from_truth(truth), dt_s, |stage| {
            config.derivative(&stage, control, flying, catapult_force_n, terrain, dt_s)
        });
        ensure!(
            state.is_finite(),
            "FixedWing6DOF integration produced nonfinite state"
        );
        state.write_truth(truth);
        Ok(())
    }

    fn advance_launcher(&mut self, time_s: f64) -> f64 {
        match self.launch {
            LaunchState::Waiting => {
                // A launch time exactly on a substep boundary can fall on
                // either side of it by rounding; if that matters, compare
                // within half a substep, as spawning does within half a tick.
                if time_s > self.config.launch_at_s {
                    self.launch = LaunchState::Launching { start_s: time_s };
                }
                0.0
            }
            LaunchState::Launching { start_s } => {
                if self.config.launch_speed_mps / self.config.launch_acceleration_mps2
                    < time_s - start_s
                {
                    self.launch = LaunchState::Flying;
                }
                // Reference applies this force even on the transition-to-flight tick.
                self.config.launch_acceleration_mps2 * self.config.mass_kg
            }
            LaunchState::Flying => 0.0,
        }
    }
}

impl FixedWingConfig {
    /// When the launcher releases the aircraft, or `None` without a launcher.
    pub fn launch_at_s(&self) -> Option<f64> {
        self.enable_launcher.then_some(self.launch_at_s)
    }

    /// The equations of motion: the rate of each state field for one stage.
    fn derivative(
        &self,
        state: &AircraftState,
        control: Control,
        flying: bool,
        catapult_force_n: f64,
        terrain: Option<&Terrain>,
        dt_s: f64,
    ) -> AircraftRate {
        // Symbols, as in the model's description: the body's velocity and
        // rates, the attitude quaternion, mass, gravity, and the inertia
        // tensor.
        let (v, omega, q) = (
            state.velocity_frd_mps,
            state.rates_frd_radps,
            state.attitude_ned_from_frd,
        );
        let (m, g) = (self.mass_kg, self.gravity_mps2);
        let inertia = self.inertia_kgm2;
        let rotation = q.normalized();
        let wind_frd_mps = rotation.rotate_world_to_body(ned_from_enu(self.wind_world_mps));
        // Velocity through the air: ground velocity minus the air's (the wind's).
        let air_velocity_frd_mps = v - wind_frd_mps;
        let aero_force_n = self
            .aerodynamics
            .force(air_velocity_frd_mps, omega, control);
        // Weight m g, down in NED.
        let weight_n = rotation.rotate_world_to_body(Vec3::new(0.0, 0.0, m * g));
        let thrust_n = Vec3::new(self.thrust_n(control.throttle), 0.0, 0.0);
        // Not modeled, each a zero in the sums below. Propeller slipstream
        // speeds the air over the wing root and tail; it needs the propeller's
        // induced velocity (momentum theory or measured wake data). Propeller
        // torque and gyroscopic moments need the propeller's torque and
        // inertia. Ground effect raises lift and cuts induced drag within about
        // a span of the ground; it needs height over span and a measured or
        // published correction.
        let slipstream_n = Vec3::zeros();
        let ground_effect_n = Vec3::zeros();
        let propeller_moment_nm = Vec3::zeros();
        let mut force_n = weight_n + thrust_n + aero_force_n + slipstream_n + ground_effect_n;
        let mut gear_moment_nm = Vec3::zeros();
        if !self.landing_gear.is_empty() {
            let (gear_force_n, moment_nm) = landing_gear::loads(
                &self.landing_gear,
                state,
                rotation,
                terrain,
                control.rudder_rad,
                self.mass_kg,
                dt_s,
            );
            force_n += gear_force_n;
            gear_moment_nm = moment_nm;
        }
        let (depth_m, sink_mps) = (state.position_ned_m.z, state.velocity_ned_mps.z);
        if self.enable_ground_spring && depth_m > 0.0 {
            // Inherited ground model: damping is only 40 N*s/m, not mass-scaled.
            // This is not a validated landing model for the default eight-tonne
            // aircraft; `landing_gear` is the real ground contact.
            let ground_force_n = (400.0 * m * depth_m + 40.0 * sink_mps).max(0.0);
            force_n += rotation.rotate_world_to_body(Vec3::new(0.0, 0.0, -ground_force_n));
        }
        if !flying {
            force_n = Vec3::zeros();
        }
        force_n.x += catapult_force_n;
        // Newton: a = F / m. The angle of attack's rate follows from the
        // body's acceleration, so the aerodynamic moment comes after the
        // force.
        let acceleration = force_n / m;
        let alpha_dot_radps =
            aerodynamics::alpha_dot_radps(air_velocity_frd_mps, v.cross(&omega) + acceleration);
        let mut moment_nm = self.aerodynamics.moment(
            air_velocity_frd_mps,
            omega,
            control,
            alpha_dot_radps,
            aero_force_n,
        ) + propeller_moment_nm
            + gear_moment_nm;
        if !flying {
            moment_nm = Vec3::zeros();
        }
        // Euler: I omega_dot = M - omega x (I omega).
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

    fn thrust_n(&self, throttle: f64) -> f64 {
        // Scaling clamps again to the configured input range.
        // Defaults map zero throttle to -36,000 N (reverse thrust), not idle.
        // A non-reversing engine needs different limits; don't compensate in the PID.
        let throttle = throttle
            .clamp(-1.0, 1.0)
            .clamp(self.min_throttle, self.max_throttle);
        let scale =
            (self.max_thrust_n - self.min_thrust_n) / (self.max_throttle - self.min_throttle);
        (throttle - self.min_throttle) * scale + self.min_thrust_n
    }
}

/// A table is empty (unused) or has two or more finite rows in increasing
/// abscissa.
fn check_table(key: &str, abscissa: &str, coefficient: &str, table: &[[f64; 2]]) -> Result<()> {
    ensure!(
        table.is_empty()
            || (table.len() >= 2
                && table.iter().flatten().all(|value| value.is_finite())
                && table.windows(2).all(|rows| rows[0][0] < rows[1][0])),
        "FixedWing6DOF {key} needs at least two finite [{abscissa}, {coefficient}] rows in increasing {abscissa}"
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
    let inertia_kgm2 = match (params.inertia_slug_ft2, params.inertia_kgm2) {
        (Some(_), Some(_)) => {
            bail!("FixedWing6DOF takes inertia_slug_ft2 or inertia_kgm2, not both")
        }
        (Some(rows), None) => inertia(rows, true)?,
        (None, Some(rows)) => inertia(rows, false)?,
        (None, None) => preset.map(|vehicle| vehicle.inertia_kgm2).context(
            "FixedWing6DOF: without `airframe`, the mission must give an inertia matrix",
        )?,
    };
    Ok(Vehicle {
        mass_kg: pick("mass_kg", params.mass_kg, preset.map(|v| v.mass_kg))?,
        inertia_kgm2,
        wing_span_m: pick(
            "wing_span_m",
            params.wing_span_m,
            preset.map(|v| v.wing_span_m),
        )?,
        wing_area_m2: pick(
            "wing_area_m2",
            params.wing_area_m2,
            preset.map(|v| v.wing_area_m2),
        )?,
        mean_chord_m: pick(
            "mean_chord_m",
            params.mean_chord_m,
            preset.map(|v| v.mean_chord_m),
        )?,
        oswald_efficiency: pick(
            "oswald_efficiency",
            params.oswald_efficiency,
            preset.map(|v| v.oswald_efficiency),
        )?,
        min_thrust_n: pick(
            "min_thrust_n",
            params.min_thrust_n,
            preset.map(|v| v.min_thrust_n),
        )?,
        max_thrust_n: pick(
            "max_thrust_n",
            params.max_thrust_n,
            preset.map(|v| v.max_thrust_n),
        )?,
        elevator_limits_rad: [
            pick(
                "min_elevator_rad",
                params.min_elevator_rad,
                preset.map(|v| v.elevator_limits_rad[0]),
            )?,
            pick(
                "max_elevator_rad",
                params.max_elevator_rad,
                preset.map(|v| v.elevator_limits_rad[1]),
            )?,
        ],
        aileron_limits_rad: [
            pick(
                "min_aileron_rad",
                params.min_aileron_rad,
                preset.map(|v| v.aileron_limits_rad[0]),
            )?,
            pick(
                "max_aileron_rad",
                params.max_aileron_rad,
                preset.map(|v| v.aileron_limits_rad[1]),
            )?,
        ],
        rudder_limits_rad: [
            pick(
                "min_rudder_rad",
                params.min_rudder_rad,
                preset.map(|v| v.rudder_limits_rad[0]),
            )?,
            pick(
                "max_rudder_rad",
                params.max_rudder_rad,
                preset.map(|v| v.rudder_limits_rad[1]),
            )?,
        ],
        c_drag_0: pick("c_drag_0", params.c_drag_0, preset.map(|v| v.c_drag_0))?,
        c_drag_alpha: pick(
            "c_drag_alpha",
            params.c_drag_alpha,
            preset.map(|v| v.c_drag_alpha),
        )?,
        c_drag_elevator: pick(
            "c_drag_elevator",
            params.c_drag_elevator,
            preset.map(|v| v.c_drag_elevator),
        )?,
        c_lift_0: pick("c_lift_0", params.c_lift_0, preset.map(|v| v.c_lift_0))?,
        c_lift_alpha: pick(
            "c_lift_alpha",
            params.c_lift_alpha,
            preset.map(|v| v.c_lift_alpha),
        )?,
        c_lift_q: pick("c_lift_q", params.c_lift_q, preset.map(|v| v.c_lift_q))?,
        c_lift_elevator: pick(
            "c_lift_elevator",
            params.c_lift_elevator,
            preset.map(|v| v.c_lift_elevator),
        )?,
        c_side_beta: pick(
            "c_side_beta",
            params.c_side_beta,
            preset.map(|v| v.c_side_beta),
        )?,
        c_side_rudder: pick(
            "c_side_rudder",
            params.c_side_rudder,
            preset.map(|v| v.c_side_rudder),
        )?,
        c_roll_beta: pick(
            "c_roll_beta",
            params.c_roll_beta,
            preset.map(|v| v.c_roll_beta),
        )?,
        c_roll_p: pick("c_roll_p", params.c_roll_p, preset.map(|v| v.c_roll_p))?,
        c_roll_r: pick("c_roll_r", params.c_roll_r, preset.map(|v| v.c_roll_r))?,
        c_roll_aileron: pick(
            "c_roll_aileron",
            params.c_roll_aileron,
            preset.map(|v| v.c_roll_aileron),
        )?,
        c_roll_rudder: pick(
            "c_roll_rudder",
            params.c_roll_rudder,
            preset.map(|v| v.c_roll_rudder),
        )?,
        c_pitch_0: pick("c_pitch_0", params.c_pitch_0, preset.map(|v| v.c_pitch_0))?,
        c_pitch_alpha: pick(
            "c_pitch_alpha",
            params.c_pitch_alpha,
            preset.map(|v| v.c_pitch_alpha),
        )?,
        c_pitch_q: pick("c_pitch_q", params.c_pitch_q, preset.map(|v| v.c_pitch_q))?,
        c_pitch_elevator: pick(
            "c_pitch_elevator",
            params.c_pitch_elevator,
            preset.map(|v| v.c_pitch_elevator),
        )?,
        c_yaw_beta: pick(
            "c_yaw_beta",
            params.c_yaw_beta,
            preset.map(|v| v.c_yaw_beta),
        )?,
        c_yaw_p: pick("c_yaw_p", params.c_yaw_p, preset.map(|v| v.c_yaw_p))?,
        c_yaw_r: pick("c_yaw_r", params.c_yaw_r, preset.map(|v| v.c_yaw_r))?,
        c_yaw_aileron: pick(
            "c_yaw_aileron",
            params.c_yaw_aileron,
            preset.map(|v| v.c_yaw_aileron),
        )?,
        c_yaw_rudder: pick(
            "c_yaw_rudder",
            params.c_yaw_rudder,
            preset.map(|v| v.c_yaw_rudder),
        )?,
        // Off unless the mission or the airframe sets them.
        aero_reference_frd_m: params
            .aero_reference_frd_m
            .map(Vec3::from)
            .or(preset.map(|v| v.aero_reference_frd_m))
            .unwrap_or_default(),
        c_pitch_alpha_dot: params
            .c_pitch_alpha_dot
            .or(preset.map(|v| v.c_pitch_alpha_dot))
            .unwrap_or_default(),
        c_yaw_0: params
            .c_yaw_0
            .or(preset.map(|v| v.c_yaw_0))
            .unwrap_or_default(),
        lift_table: table(&params.lift_table, preset.map(|v| &v.lift_table)),
        drag_table: table(&params.drag_table, preset.map(|v| &v.drag_table)),
        sideslip_drag_table: table(
            &params.sideslip_drag_table,
            preset.map(|v| &v.sideslip_drag_table),
        ),
        aileron_power_mach_table: table(
            &params.aileron_power_mach_table,
            preset.map(|v| &v.aileron_power_mach_table),
        ),
        elevator_power_mach_table: table(
            &params.elevator_power_mach_table,
            preset.map(|v| &v.elevator_power_mach_table),
        ),
    })
}

/// The mission's table (an empty one turns the airframe's off), else the
/// airframe's, else none.
fn table(given: &Option<Vec<[f64; 2]>>, preset: Option<&Vec<[f64; 2]>>) -> Vec<[f64; 2]> {
    given.as_ref().or(preset).cloned().unwrap_or_default()
}

/// Three rows, in slug ft^2 (`slug_ft2`) or kg m^2, as kg m^2.
fn inertia(rows: [[f64; 3]; 3], slug_ft2: bool) -> Result<Matrix3<f64>> {
    let factor = if slug_ft2 { SLUG_FT_SQ_TO_KGM2 } else { 1.0 };
    let values: Vec<f64> = rows
        .as_flattened()
        .iter()
        .map(|value| value * factor)
        .collect();
    let inertia = Matrix3::from_row_slice(&values);
    ensure!(
        (inertia - inertia.transpose()).norm() < 1e-9 && inertia.cholesky().is_some(),
        "FixedWing6DOF inertia must be symmetric positive definite"
    );
    Ok(inertia)
}

#[cfg(test)]
mod tests {
    use super::{Control, FixedWing6Dof, MotionModel};
    use crate::{KinematicState, plugin::PluginParams};
    use nalgebra::{Matrix3, Vector3};

    #[test]
    fn flight_in_wind_is_flight_in_still_air_carried_by_the_wind() -> anyhow::Result<()> {
        // Airspeed is velocity minus wind, so an aircraft flying at v + w in wind
        // w feels exactly the air an aircraft at v feels in still air.
        let wind_mps = Vector3::new(-3.0, 2.0, 0.5);
        let aircraft = |wind: Vector3<f64>| -> anyhow::Result<FixedWing6Dof> {
            let params = PluginParams::yaml(&format!(
                "{{airframe: a4d, wind_mps: [{:?}, {:?}, {:?}]}}",
                wind.x, wind.y, wind.z
            ));
            Ok(FixedWing6Dof::new(&FixedWing6Dof::configure(&params)?))
        };
        let (mut still, mut windy) = (aircraft(Vector3::zeros())?, aircraft(wind_mps)?);
        let start = KinematicState {
            position_world_m: Vector3::new(0.0, 0.0, 3000.0),
            velocity_world_mps: Vector3::new(120.0, 0.0, 0.0),
            ..KinematicState::default()
        };
        let (mut still_truth, mut windy_truth) = (start.clone(), start.clone());
        windy_truth.velocity_world_mps += wind_mps;
        let control = Control {
            throttle: 0.6,
            elevator_rad: -0.02,
            aileron_rad: 0.01,
            rudder_rad: 0.0,
        };
        let dt_s = 0.01;
        for step in 0..200 {
            let time_s = step as f64 * dt_s;
            still.integrate(&mut still_truth, control, None, time_s, dt_s)?;
            windy.integrate(&mut windy_truth, control, None, time_s, dt_s)?;
        }
        let carried_m = wind_mps * 2.0;
        let offset_m = windy_truth.position_world_m - still_truth.position_world_m;
        assert!((offset_m - carried_m).norm() < 1e-9, "{offset_m:?}");
        let relative_mps = windy_truth.velocity_world_mps - still_truth.velocity_world_mps;
        assert!((relative_mps - wind_mps).norm() < 1e-9, "{relative_mps:?}");
        let (a, b) = (
            windy_truth.orientation_world_from_body,
            still_truth.orientation_world_from_body,
        );
        assert!(
            (a.w - b.w).abs() + (a.x - b.x).abs() + (a.y - b.y).abs() + (a.z - b.z).abs() < 1e-12
        );
        // The aircraft did maneuver: this is not a trivial straight line.
        assert!(still_truth.angular_velocity_world_radps.norm() > 1e-3);
        Ok(())
    }

    #[test]
    fn throttle_scaling_clamps_like_cpp_utilities_scale() -> anyhow::Result<()> {
        let params =
            PluginParams::yaml("{airframe: a4d, min_thrust_n: -36000, max_thrust_n: 36000}");
        let config = FixedWing6Dof::configure(&params)?;
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
        let slug = "inertia_slug_ft2: [[8090, 0, 1300], [0, 25900, 0], [1300, 0, 29200]]";
        let si = "inertia_kgm2: [[2, 0, 0], [0, 3, 0], [0, 0, 4]]";
        let configure = |extra: &str| {
            FixedWing6Dof::configure(&PluginParams::yaml(&format!("airframe: a4d\n{extra}")))
        };
        let profile = configure("")?;
        let explicit = configure(slug)?;
        assert_eq!(profile.inertia_kgm2, explicit.inertia_kgm2);

        let config = configure(si)?;
        assert_eq!(
            config.inertia_kgm2,
            Matrix3::from_diagonal(&Vector3::new(2.0, 3.0, 4.0))
        );

        assert!(configure("inertia_kgm2: [[1, 2, 3], [4, 5, 6]]").is_err());
        assert!(configure("inertia_kgm2: [[1, 2, 3], [2, 1, 0], [3, 0, 1]]").is_err());
        assert!(configure(&format!("{si}\n{slug}")).is_err());
        Ok(())
    }

    #[test]
    fn a_named_airframe_with_an_override_equals_the_same_aircraft_written_out() -> anyhow::Result<()>
    {
        // The Rascal in full, every vehicle key, with C_MQ changed.
        let full = [
            ("mass_kg", "6.577089365"),
            ("wing_span_m", "2.795016"),
            ("wing_area_m2", "0.9819851328000001"),
            ("mean_chord_m", "0.35052"),
            ("oswald_efficiency", "0.9979726529753524"),
            ("min_thrust_n", "0"),
            ("max_thrust_n", "25"),
            ("min_elevator_rad", "-0.35"),
            ("max_elevator_rad", "0.30"),
            ("min_aileron_rad", "-0.35"),
            ("max_aileron_rad", "0.35"),
            ("min_rudder_rad", "-0.35"),
            ("max_rudder_rad", "0.35"),
            ("c_drag_0", "0.028"),
            ("c_drag_alpha", "0"),
            ("c_drag_elevator", "0.1"),
            ("c_lift_0", "0.25"),
            ("c_lift_alpha", "5"),
            ("c_lift_q", "0"),
            ("c_lift_elevator", "0.2"),
            ("c_side_beta", "-1"),
            ("c_side_rudder", "0"),
            ("c_roll_beta", "-0.1"),
            ("c_roll_p", "-0.4"),
            ("c_roll_r", "0.15"),
            ("c_roll_aileron", "0.13"),
            ("c_roll_rudder", "0.01"),
            ("c_pitch_0", "0"),
            ("c_pitch_alpha", "-0.5"),
            ("c_pitch_q", "-10"),
            ("c_pitch_elevator", "-0.5"),
            ("c_yaw_beta", "0.12"),
            ("c_yaw_p", "0"),
            ("c_yaw_r", "-0.15"),
            ("c_yaw_aileron", "-0.03"),
            ("c_yaw_rudder", "-0.05"),
            (
                "aero_reference_frd_m",
                "[-0.02550510344827574, 0, 0.08611366379310344]",
            ),
            ("c_pitch_alpha_dot", "-7"),
            ("c_yaw_0", "0.0007"),
            (
                "lift_table",
                "[[-0.2, -0.75], [0.0, 0.25], [0.23, 1.4], [0.6, 0.71]]",
            ),
            (
                "drag_table",
                "[[-1.57, 1.5], [-0.26, 0.056], [0.0, 0.028], [0.26, 0.056], [1.57, 1.5]]",
            ),
            (
                "sideslip_drag_table",
                "[[-1.57, 1.23], [-0.26, 0.05], [0.0, 0.0], [0.26, 0.05], [1.57, 1.23]]",
            ),
            ("aileron_power_mach_table", "[[0.0, 0.13], [2.0, 0.057]]"),
            ("elevator_power_mach_table", "[[0.0, -0.5], [2.0, -0.275]]"),
        ];
        let resolve = |text: &str| -> anyhow::Result<_> {
            super::vehicle(&PluginParams::yaml(text).parse()?)
        };
        let full: String = full
            .iter()
            .map(|(key, value)| format!("{key}: {value}\n"))
            .chain([
                "inertia_slug_ft2: [[1.960082815452557, 0, -6.843056086570756e-05], \
                 [0, 1.5600832798805415, 0], [-6.843056086570756e-05, 0, 1.9100004644279847]]"
                    .into(),
            ])
            .collect();
        let from_name = resolve("{airframe: rascal, c_pitch_q: -10}")?;
        assert_eq!(from_name, resolve(&full)?);
        assert_eq!(from_name.c_pitch_q, -10.0);
        // Without the override, the profile's own value.
        let plain = resolve("airframe: rascal")?;
        assert_eq!(plain.c_pitch_q, -12.0);
        Ok(())
    }

    #[test]
    fn without_an_airframe_every_vehicle_number_is_required() {
        let error = FixedWing6Dof::configure(&PluginParams::yaml("{}"))
            .err()
            .expect("no airframe and no numbers");
        assert!(error.to_string().contains("without `airframe`"), "{error}");
        assert!(FixedWing6Dof::configure(&PluginParams::yaml("airframe: cessna")).is_err());
    }
}

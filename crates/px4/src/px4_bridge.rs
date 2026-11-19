//! Exchange sensor readings for actuator controls with one PX4 SITL, in
//! lockstep, over PX4's simulator MAVLink API (`hil.rs`).
//!
//! Autonomy phase, every tick:
//!   1. make this tick's sensor readings from the `SitlSample` and send them
//!      (HIL_SENSOR every tick, HIL_GPS at `gps_rate_hz`);
//!   2. wait for PX4's HIL_ACTUATOR_CONTROLS for that tick;
//!   3. map the controls to the FixedWing6DOF inputs.
//!
//! PX4's clock is the time stamp of our sensor message, and it sends controls
//! only once its controllers run, seconds of its time after boot. So until the
//! first controls message arrives the bridge sends sensors without waiting
//! and holds zero inputs (PX4 would otherwise never advance), and from then
//! on it waits for exactly one controls message per tick, stamped with the
//! time it answers. Silence for `frame_timeout_s`, or a controls message
//! stamped later than the readings it answers, fails the run.
//!
//! Every phase waits for all entities, so N bridges keep N PX4s on one clock.
//! Use no controller after it: its outputs are the motion model's inputs. It
//! must run every tick (no `loop_rate_hz`). `ripple run` stops PX4 when the
//! run ends (`px4_launcher.rs`). See book/src/guides/px4.md.

use std::collections::BTreeMap;
use std::net::TcpListener;
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use ripple_core::Vec3;
use ripple_core::plugin::{
    AgentContext, Autonomy, Frame, PluginIo, PluginParams, Port, Ports, StepTime, Unit,
};
use serde::Deserialize;
use sitl::{SITL_STATE_TOPIC, SitlSample};

use crate::hil::{self, Controls, Home, Link, Noise};

/// How one PX4 output maps to a motion input: the control from `input_min`
/// to `input_max` maps linearly onto `output_min..output_max` (swap the
/// outputs to reverse). A mission gives all five fields, e.g. `throttle:
/// {channel: 5, input_min: 0, input_max: 1, output_min: 0, output_max: 1}`.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlMap {
    /// PX4 output number, 1-based (PWM_MAIN_FUNC1 is 1).
    channel: f64,
    input_min: f64,
    input_max: f64,
    output_min: f64,
    output_max: f64,
}

/// Mission parameters; `Default` supplies any key the mission leaves out.
/// `mode`, `waypoints_m`, `takeoff_altitude_m`, `land`, `speedup`, and
/// `px4_params` set up this aircraft's PX4 session (`px4_launcher.rs`,
/// `docker/session.py`); the bridge itself only exchanges messages.
///
/// The default maps follow PX4's own Rascal airframe (`1033_jsbsim_rascal`)
/// as `docker/4100_rascal` keeps it: motor on output 5, left aileron 6,
/// elevator 8, rudder 3, and the steering wheel on 4 (our addition), onto the
/// Rascal's surface throws in radians. PX4's servo outputs are -1 to 1 and
/// its motor 0 to 1. The airframe file reverses the three surfaces, as PX4's
/// JSBSim bridge does for this Rascal, so the maps here stay plain.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Px4BridgeConfig {
    /// TCP port this aircraft's PX4 connects to: 4560 plus its instance.
    port: u16,
    /// Longest wait for PX4 to connect after the run starts.
    first_frame_timeout_s: f64,
    /// Longest wait for each controls message once PX4 is answering.
    frame_timeout_s: f64,
    throttle: ControlMap,
    elevator: ControlMap,
    aileron: ControlMap,
    rudder: ControlMap,
    /// The steered wheel's angle, for a landing-gear wheel `steered_by_input`.
    wheel: ControlMap,
    /// Where local (0, 0, 0) is: latitude and longitude (degrees) and
    /// altitude above sea level (m), for the GPS and barometer readings. It
    /// must be the mission terrain's origin, or McMillan Airfield (the
    /// default) without terrain; `ripple run` checks.
    home: [f64; 3],
    /// The Earth's magnetic field at home, north-east-down in gauss. The
    /// default is McMillan Airfield from the World Magnetic Model 2025 for
    /// 2026-11 (BGS calculator: 22928 nT north, 4931 east, 40063 down).
    magnetic_field_ned_gauss: [f64; 3],
    /// How often HIL_GPS is sent; PX4's Gazebo plugin sends 5 Hz.
    gps_rate_hz: f64,
    /// Sensor noise, as white noise densities (per root hertz) scaled to the
    /// tick, with PX4's Gazebo Classic defaults; 0 turns one off.
    gyro_noise_radps_per_rthz: f64,
    accel_noise_mps2_per_rthz: f64,
    mag_noise_gauss_per_rthz: f64,
    /// Barometer noise per sample (Pa).
    baro_noise_pa: f64,
    /// The PX4 mode the session arms in: TAKEOFF by default (a climb to
    /// PX4's takeoff altitude, then circling there), MISSION with
    /// `waypoints_m`. PX4's LOITER does nothing when entered on the ground.
    pub(crate) mode: Option<String>,
    /// A mission route: points `[east, north, up]` in metres from where the
    /// aircraft starts (PX4 sets home where it arms). The route repeats
    /// unless it ends in a landing.
    pub(crate) waypoints_m: Vec<[f64; 3]>,
    /// Start the route with a takeoff climbing to this height (m), towards the
    /// first waypoint.
    pub(crate) takeoff_altitude_m: Option<f64>,
    /// End the route with a landing at its last waypoint; that waypoint's
    /// `up` is the ground's height there relative to the start.
    pub(crate) land: bool,
    /// PX4's speed factor, its cap on running faster than real time.
    pub(crate) speedup: f64,
    /// PX4 parameters for this aircraft, set after the airframe file.
    pub(crate) px4_params: BTreeMap<String, f64>,
}

impl Default for Px4BridgeConfig {
    fn default() -> Self {
        Self {
            port: 4560,
            first_frame_timeout_s: 60.0,
            frame_timeout_s: 10.0,
            throttle: ControlMap::from([5.0, 0.0, 1.0, 0.0, 1.0]),
            aileron: ControlMap::from([6.0, -1.0, 1.0, -0.35, 0.35]),
            elevator: ControlMap::from([8.0, -1.0, 1.0, -0.30, 0.30]),
            rudder: ControlMap::from([3.0, -1.0, 1.0, -0.35, 0.35]),
            wheel: ControlMap::from([4.0, -1.0, 1.0, -0.35, 0.35]),
            home: [35.721025, -120.767925, 300.0],
            magnetic_field_ned_gauss: [0.22928, 0.04931, 0.40063],
            gps_rate_hz: 5.0,
            gyro_noise_radps_per_rthz: 0.05f64.to_radians(),
            accel_noise_mps2_per_rthz: 0.00637,
            mag_noise_gauss_per_rthz: 0.0004,
            baro_noise_pa: 1.0,
            mode: None,
            waypoints_m: Vec::new(),
            takeoff_altitude_m: None,
            land: false,
            speedup: 100.0,
            px4_params: BTreeMap::new(),
        }
    }
}

pub struct Px4Bridge {
    port: u16,
    first_frame_timeout: Duration,
    frame_timeout: Duration,
    throttle: ControlMap,
    elevator: ControlMap,
    aileron: ControlMap,
    rudder: ControlMap,
    wheel: ControlMap,
    field_ned_gauss: Vec3,
    gps_rate_hz: f64,
    noise_density: Noise,
    listener: Option<TcpListener>,
    link: Option<Link>,
    /// Set once PX4 has sent controls; before that the bridge free-wheels.
    lockstep: bool,
    /// Time stamp of the readings sent this tick, which the controls answer.
    sent_time_usec: u64,
    /// Ticks since the start, for the GPS cadence.
    ticks: u64,
    /// Time of the previous step, to check that no tick was skipped.
    previous_time_s: Option<f64>,
    /// Motion inputs last written, held while PX4 has not answered yet.
    inputs: MotionInputs,
    home: Home,
}

/// The FixedWing6DOF inputs.
#[derive(Clone, Copy, Default)]
struct MotionInputs {
    throttle: f64,
    elevator_rad: f64,
    aileron_rad: f64,
    rudder_rad: f64,
    wheel_rad: f64,
}

impl Autonomy for Px4Bridge {
    type Config = Px4BridgeConfig;

    fn configure(params: &PluginParams) -> Result<Px4BridgeConfig> {
        let config: Px4BridgeConfig = params.parse()?;
        ensure!(
            config.first_frame_timeout_s > 0.0 && config.frame_timeout_s > 0.0,
            "Px4Bridge timeouts must be positive"
        );
        for (name, map) in [
            ("throttle", config.throttle),
            ("elevator", config.elevator),
            ("aileron", config.aileron),
            ("rudder", config.rudder),
            ("wheel", config.wheel),
        ] {
            map.validate(name)?;
        }
        ensure!(
            config.home[0].abs() <= 90.0
                && config.home[1].abs() <= 180.0
                && config.home[2].is_finite(),
            "Px4Bridge home must be [latitude_deg, longitude_deg, altitude_m]"
        );
        ensure!(
            config
                .magnetic_field_ned_gauss
                .iter()
                .all(|v| v.is_finite()),
            "Px4Bridge magnetic_field_ned_gauss must be finite"
        );
        ensure!(
            config.gps_rate_hz > 0.0,
            "Px4Bridge gps_rate_hz must be positive"
        );
        ensure!(
            [
                config.gyro_noise_radps_per_rthz,
                config.accel_noise_mps2_per_rthz,
                config.mag_noise_gauss_per_rthz,
                config.baro_noise_pa,
            ]
            .iter()
            .all(|noise| noise.is_finite() && *noise >= 0.0),
            "Px4Bridge noise levels must be zero or positive"
        );
        config.mode()?;
        ensure!(config.speedup > 0.0, "Px4Bridge speedup must be positive");
        Ok(config)
    }

    fn new(config: &Px4BridgeConfig) -> Self {
        Self {
            port: config.port,
            first_frame_timeout: Duration::from_secs_f64(config.first_frame_timeout_s),
            frame_timeout: Duration::from_secs_f64(config.frame_timeout_s),
            throttle: config.throttle,
            elevator: config.elevator,
            aileron: config.aileron,
            rudder: config.rudder,
            wheel: config.wheel,
            field_ned_gauss: Vec3::from(config.magnetic_field_ned_gauss),
            gps_rate_hz: config.gps_rate_hz,
            noise_density: Noise {
                rates_radps: config.gyro_noise_radps_per_rthz,
                specific_force_mps2: config.accel_noise_mps2_per_rthz,
                pressure_pa: config.baro_noise_pa,
                field_gauss: config.mag_noise_gauss_per_rthz,
            },
            listener: None,
            link: None,
            lockstep: false,
            sent_time_usec: 0,
            ticks: 0,
            previous_time_s: None,
            // Zero throttle and centred surfaces until PX4 answers.
            inputs: MotionInputs::default(),
            home: Home {
                latitude_deg: config.home[0],
                longitude_deg: config.home[1],
                altitude_m: config.home[2],
            },
        }
    }

    fn ports(_config: &Px4BridgeConfig) -> Ports {
        Ports::default()
            .output(Port::new("throttle", Unit::Dimensionless, Frame::None))
            .output(Port::new("elevator", Unit::Radians, Frame::Model))
            .output(Port::new("aileron", Unit::Radians, Frame::Model))
            .output(Port::new("rudder", Unit::Radians, Frame::Model))
            .output(Port::new("wheel", Unit::Radians, Frame::Model))
    }

    fn initialize(&mut self, context: &mut AgentContext) -> Result<()> {
        context
            .messages
            .subscribe::<SitlSample>("LocalNetwork", SITL_STATE_TOPIC)?;
        // PX4 connects to us; the launcher starts it after the simulation is
        // built, so the port is listening by then and `step` accepts.
        let listener = TcpListener::bind(("0.0.0.0", self.port))
            .with_context(|| format!("Px4Bridge could not bind TCP port {}", self.port))?;
        listener.set_nonblocking(true)?;
        self.listener = Some(listener);
        Ok(())
    }

    fn step(&mut self, context: &mut AgentContext, io: &mut PluginIo) -> Result<()> {
        let first_tick = self.previous_time_s.is_none();
        if let Some(previous_time_s) = self.previous_time_s {
            let elapsed_s = context.time.time_s - previous_time_s;
            ensure!(
                (elapsed_s - context.time.dt_s).abs() <= 1e-9 * context.time.dt_s.max(1.0),
                "Px4Bridge must run every tick (remove its loop_rate_hz)"
            );
        }
        self.previous_time_s = Some(context.time.time_s);

        if self.link.is_none() {
            let listener = self
                .listener
                .as_ref()
                .context("Px4Bridge is not initialized")?;
            self.link = Some(Link::accept(listener, self.first_frame_timeout)?);
        }
        // Sensors run after autonomy, so the first tick has no sample yet and
        // only holds the inputs; PX4 gets its first readings next tick.
        let Some(sample) = context
            .messages
            .receive::<SitlSample>("LocalNetwork", SITL_STATE_TOPIC)?
            .pop()
        else {
            ensure!(
                first_tick,
                "no SitlState sample to send PX4; \
                 add a SitlState sensor running every tick and LocalNetwork"
            );
            return self.write_inputs(io);
        };

        // 1. This tick's readings. White noise densities become a standard
        // deviation per sample at the tick rate.
        let time_usec = (context.time.time_s * 1e6).round() as u64;
        let per_sample = 1.0 / context.time.dt_s.sqrt();
        let noise = Noise {
            rates_radps: self.noise_density.rates_radps * per_sample,
            specific_force_mps2: self.noise_density.specific_force_mps2 * per_sample,
            pressure_pa: self.noise_density.pressure_pa,
            field_gauss: self.noise_density.field_gauss * per_sample,
        };
        let readings = hil::readings(&sample.value, self.home, self.field_ned_gauss)
            .with_noise(&context.random, noise)?;
        let link = self.link.as_mut().context("no PX4 link")?;
        link.send(&hil::sensor_message(time_usec, &readings))?;
        let ticks_per_fix = (1.0 / (context.time.dt_s * self.gps_rate_hz))
            .round()
            .max(1.0) as u64;
        if self.ticks.is_multiple_of(ticks_per_fix) {
            link.send(&hil::gps_message(time_usec, &sample.value, self.home))?;
        }
        self.ticks += 1;
        self.sent_time_usec = time_usec;

        // 2. PX4's controls: whatever has arrived while it boots, then exactly
        // one message per tick.
        let controls = if self.lockstep {
            Some(self.answer()?)
        } else {
            self.drained()?
        };
        if let Some(controls) = controls {
            self.lockstep = true;
            self.inputs = MotionInputs {
                throttle: self.throttle.output(&controls),
                elevator_rad: self.elevator.output(&controls),
                aileron_rad: self.aileron.output(&controls),
                rudder_rad: self.rudder.output(&controls),
                wheel_rad: self.wheel.output(&controls),
            };
        }

        // 3. Motion inputs.
        self.write_inputs(io)
    }

    fn close(&mut self, _time: StepTime) -> Result<()> {
        // Free the port now; the simulation keeps closed plugins until it is dropped.
        self.link = None;
        self.listener = None;
        Ok(())
    }
}

impl Px4Bridge {
    fn write_inputs(&self, io: &mut PluginIo) -> Result<()> {
        io.write("throttle", self.inputs.throttle)?;
        io.write("elevator", self.inputs.elevator_rad)?;
        io.write("aileron", self.inputs.aileron_rad)?;
        io.write("rudder", self.inputs.rudder_rad)?;
        io.write("wheel", self.inputs.wheel_rad)?;
        Ok(())
    }

    /// The last controls message PX4 has sent so far, without waiting.
    fn drained(&mut self) -> Result<Option<Controls>> {
        let link = self.link.as_mut().context("no PX4 link")?;
        let mut latest = None;
        while let Some(message) = link.receive(Duration::ZERO)? {
            if let Some(controls) = hil::controls(&message) {
                latest = Some(controls);
            }
        }
        Ok(latest)
    }

    /// The controls answering this tick's readings. Controls stamped earlier
    /// are leftovers from the free-wheeling start and are dropped.
    fn answer(&mut self) -> Result<Controls> {
        let link = self.link.as_mut().context("no PX4 link")?;
        let deadline = std::time::Instant::now() + self.frame_timeout;
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            let Some(message) = link.receive(remaining)? else {
                bail!(
                    "no controls from PX4 on TCP port {} within {:.1} s",
                    self.port,
                    self.frame_timeout.as_secs_f64()
                );
            };
            let Some(controls) = hil::controls(&message) else {
                continue;
            };
            if controls.time_usec < self.sent_time_usec {
                continue;
            }
            ensure!(
                controls.time_usec == self.sent_time_usec,
                "PX4 answered time {} us, but the readings sent were at {} us; lockstep is lost",
                controls.time_usec,
                self.sent_time_usec
            );
            return Ok(controls);
        }
    }
}

impl Px4BridgeConfig {
    pub(crate) fn port(&self) -> u16 {
        self.port
    }

    pub(crate) fn home(&self) -> [f64; 3] {
        self.home
    }

    /// The mode the session flies, checked against the route.
    pub(crate) fn mode(&self) -> Result<&str> {
        ensure!(
            !self.waypoints_m.is_empty() || (self.takeoff_altitude_m.is_none() && !self.land),
            "Px4Bridge takeoff_altitude_m and land need waypoints_m"
        );
        let needed = (!self.waypoints_m.is_empty()).then_some("MISSION");
        match (self.mode.as_deref(), needed) {
            (Some(mode), Some(needed)) if mode != needed => {
                bail!("Px4Bridge waypoints_m needs mode {needed}, not {mode}")
            }
            (Some(mode), _) => Ok(mode),
            (None, Some(needed)) => Ok(needed),
            (None, None) => Ok("TAKEOFF"),
        }
    }
}

impl ControlMap {
    fn validate(&self, name: &str) -> Result<()> {
        ensure!(
            self.channel.fract() == 0.0 && (1.0..=16.0).contains(&self.channel),
            "Px4Bridge {name} channel must be a whole number from 1 to 16"
        );
        ensure!(
            self.input_max > self.input_min,
            "Px4Bridge {name} input_max must exceed input_min"
        );
        Ok(())
    }

    /// The motion input for this channel's control, clamped to the input range.
    fn output(&self, controls: &Controls) -> f64 {
        let value = f64::from(controls.values[self.channel as usize - 1])
            .clamp(self.input_min, self.input_max);
        let fraction = (value - self.input_min) / (self.input_max - self.input_min);
        self.output_min + fraction * (self.output_max - self.output_min)
    }
}

/// `[channel, input_min, input_max, output_min, output_max]`, for the
/// defaults and tests.
impl From<[f64; 5]> for ControlMap {
    fn from([channel, input_min, input_max, output_min, output_max]: [f64; 5]) -> Self {
        Self {
            channel,
            input_min,
            input_max,
            output_min,
            output_max,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn controls_with(channel: usize, value: f32) -> Controls {
        let mut values = [0.0; 16];
        values[channel - 1] = value;
        Controls {
            time_usec: 0,
            values,
            armed: true,
        }
    }

    #[test]
    fn controls_map_linearly_and_clamp() {
        let aileron = Px4BridgeConfig::default().aileron;
        assert_eq!(aileron.output(&controls_with(6, 0.0)), 0.0);
        assert!((aileron.output(&controls_with(6, 1.0)) - 0.35).abs() < 1e-12);
        assert!((aileron.output(&controls_with(6, -1.0)) + 0.35).abs() < 1e-12);
        assert!((aileron.output(&controls_with(6, -2.0)) + 0.35).abs() < 1e-12);
        let throttle = Px4BridgeConfig::default().throttle;
        assert!((throttle.output(&controls_with(5, 0.25)) - 0.25).abs() < 1e-12);
        // Disarmed, every control is 0: no throttle, centred surfaces.
        assert_eq!(throttle.output(&controls_with(1, 0.0)), 0.0);
    }

    #[test]
    fn swapped_outputs_reverse_a_channel() {
        let reversed = ControlMap::from([8.0, -1.0, 1.0, 0.30, -0.30]);
        assert!((reversed.output(&controls_with(8, 1.0)) + 0.30).abs() < 1e-12);
    }

    #[test]
    fn a_mission_names_each_field() {
        let map: ControlMap = serde_json::from_str(
            r#"{"channel": 5, "input_min": 0, "input_max": 1, "output_min": 0, "output_max": 1}"#,
        )
        .unwrap();
        assert!((map.output(&controls_with(5, 0.25)) - 0.25).abs() < 1e-12);
    }

    #[test]
    fn the_mode_follows_the_route_and_conflicts_are_refused() {
        let config = |change: fn(&mut Px4BridgeConfig)| {
            let mut config = Px4BridgeConfig::default();
            change(&mut config);
            config
        };
        assert_eq!(config(|_| {}).mode().unwrap(), "TAKEOFF");
        assert_eq!(
            config(|config| config.waypoints_m = vec![[1.0, 2.0, 3.0]])
                .mode()
                .unwrap(),
            "MISSION"
        );
        assert!(
            config(|config| {
                config.waypoints_m = vec![[1.0, 2.0, 3.0]];
                config.mode = Some("LOITER".into());
            })
            .mode()
            .is_err()
        );
        assert!(config(|config| config.land = true).mode().is_err());
        assert_eq!(
            config(|config| config.mode = Some("LOITER".into()))
                .mode()
                .unwrap(),
            "LOITER"
        );
    }

    #[test]
    fn bad_maps_are_rejected() {
        assert!(
            ControlMap::from([0.0, -1.0, 1.0, -1.0, 1.0])
                .validate("x")
                .is_err()
        );
        assert!(
            ControlMap::from([17.0, -1.0, 1.0, -1.0, 1.0])
                .validate("x")
                .is_err()
        );
        assert!(
            ControlMap::from([1.0, 1.0, -1.0, -1.0, 1.0])
                .validate("x")
                .is_err()
        );
    }
}

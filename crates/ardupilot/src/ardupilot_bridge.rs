//! Exchange state for servo commands with one ArduPilot SITL, in lockstep.
//!
//! Talks to ArduPilot SITL through its JSON backend.
//! Autonomy phase, every tick:
//!   1. reply to the frame applied last tick with this tick's `SitlSample`
//!      (skipped on the first tick);
//!   2. wait for SITL's next new frame, dropping its resends of the old one;
//!   3. map the servo PWM to the FixedWing6DOF inputs.
//!
//! Every phase waits for all entities, so N bridges keep N SITLs on one clock.
//! Use no controller after it: its outputs are the motion model's inputs.
//!
//! The exchange must be exactly one reply per frame: a reply carries no frame
//! number, so an extra one would leave SITL a frame behind for the rest of the
//! run. A frame gap, SITL restarting, or a timeout therefore fails the run
//! instead of being smoothed over. It must run every tick (no `loop_rate_hz`).
//!
//! The last frame applied is never answered, because the run ends first;
//! `ripple run` stops SITL when the run ends (`sitl_launcher.rs`). See
//! book/src/guides/ardupilot.md.

use std::collections::BTreeMap;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use ripple_core::plugin::{
    AgentContext, Autonomy, Frame, Plugin, PluginIo, PluginParams, Port, Ports, StepTime, Unit,
    Update,
};
use serde::Deserialize;

use crate::json_sitl::{ServoFrame, parse_servo_packet, state_reply};
use crate::sitl_state::{SITL_STATE_TOPIC, SitlSample};

/// How one servo channel maps to a motion input: PWM `pwm_min_us..pwm_max_us`
/// maps linearly onto `output_min..output_max` (swap the outputs to reverse).
/// A mission gives all five fields, e.g. `throttle: {channel: 3, pwm_min_us:
/// 1000, pwm_max_us: 2000, output_min: 0, output_max: 1}`.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServoMap {
    /// ArduPilot output number, 1-based (SERVO1 is 1).
    channel: f64,
    pwm_min_us: f64,
    pwm_max_us: f64,
    output_min: f64,
    output_max: f64,
}

/// Mission parameters; `Default` supplies any key the mission leaves out.
/// `mode`, `waypoints_m`, `goto_m`, and `speedup` set up this aircraft's SITL
/// session (`sitl_launcher.rs`, `docker/session.py`); the bridge itself only
/// exchanges frames.
/// The default maps are ArduPlane's default outputs (SERVO1 aileron, SERVO2
/// elevator, SERVO3 throttle, SERVO4 rudder) onto the Rascal's real surface
/// throws in radians, from C++'s FixedWing6DOF-Rascal.xml before commit
/// 2cab6d119. The maps are plain; the elevator and rudder reversal FixedWing6DOF
/// needs is set in ArduPilot (docker/rascal.parm), so do not reverse them here too.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ArduPilotBridgeConfig {
    /// UDP port this aircraft's SITL sends to (its `--sim-port-out`).
    port: u16,
    first_frame_timeout_s: f64,
    /// Longest wait for each later frame; must exceed SITL's 1 s resend.
    frame_timeout_s: f64,
    throttle: ServoMap,
    elevator: ServoMap,
    aileron: ServoMap,
    rudder: ServoMap,
    /// The ArduPlane mode to arm in: LOITER by default, AUTO with
    /// `waypoints_m`, GUIDED with `goto_m`.
    pub(crate) mode: Option<String>,
    /// An AUTO route that repeats: points `[east, north, up]` in metres from
    /// where the aircraft starts (ArduPlane moves home to where it arms).
    pub(crate) waypoints_m: Vec<[f64; 3]>,
    /// Start the AUTO route with a takeoff climbing to this height (m).
    pub(crate) takeoff_altitude_m: Option<f64>,
    /// End the AUTO route with a landing at its last waypoint instead of
    /// repeating it; that waypoint's `up` is the ground's height there
    /// relative to the start.
    pub(crate) land: bool,
    /// A GUIDED target `[east, north, up]` from the start, sent once armed;
    /// ArduPlane flies there and circles it.
    pub(crate) goto_m: Option<[f64; 3]>,
    /// SITL's `--speedup`, its cap on running faster than real time.
    pub(crate) speedup: f64,
    /// ArduPilot parameters for this aircraft, loaded after
    /// `docker/rascal.parm`, such as ground steering for a runway mission.
    pub(crate) ardupilot_params: BTreeMap<String, f64>,
}

impl Default for ArduPilotBridgeConfig {
    fn default() -> Self {
        Self {
            port: 9002,
            first_frame_timeout_s: 60.0,
            frame_timeout_s: 10.0,
            throttle: ServoMap::from([3.0, 1000.0, 2000.0, 0.0, 1.0]),
            elevator: ServoMap::from([2.0, 1000.0, 2000.0, -0.30, 0.30]),
            aileron: ServoMap::from([1.0, 1000.0, 2000.0, -0.35, 0.35]),
            rudder: ServoMap::from([4.0, 1000.0, 2000.0, -0.35, 0.35]),
            mode: None,
            waypoints_m: Vec::new(),
            takeoff_altitude_m: None,
            land: false,
            goto_m: None,
            speedup: 100.0,
            ardupilot_params: BTreeMap::new(),
        }
    }
}

pub struct ArduPilotBridge {
    port: u16,
    first_frame_timeout: Duration,
    frame_timeout: Duration,
    throttle: ServoMap,
    elevator: ServoMap,
    aileron: ServoMap,
    rudder: ServoMap,
    socket: Option<UdpSocket>,
    /// Where SITL sends from; replies go back to that address.
    sitl_address: Option<SocketAddr>,
    /// The frame applied last tick, which this tick's reply answers.
    applied_frame: Option<u32>,
    /// Time of the previous step, to check that no tick was skipped.
    previous_time_s: Option<f64>,
    /// Motion inputs last written, held for channels SITL is not driving yet.
    inputs: MotionInputs,
}

/// The four FixedWing6DOF inputs.
#[derive(Clone, Copy, Default)]
struct MotionInputs {
    throttle: f64,
    elevator_rad: f64,
    aileron_rad: f64,
    rudder_rad: f64,
}

impl Plugin for ArduPilotBridge {
    type Config = ArduPilotBridgeConfig;

    fn configure(params: &PluginParams<'_>) -> Result<ArduPilotBridgeConfig> {
        let config: ArduPilotBridgeConfig = params.parse()?;
        ensure!(
            config.first_frame_timeout_s > 0.0 && config.frame_timeout_s > 1.0,
            "ArduPilotBridge timeouts must be positive, and frame_timeout longer than SITL's 1 s resend"
        );
        for (name, map) in [
            ("throttle", config.throttle),
            ("elevator", config.elevator),
            ("aileron", config.aileron),
            ("rudder", config.rudder),
        ] {
            map.validate(name)?;
        }
        config.mode()?;
        ensure!(
            config.speedup > 0.0,
            "ArduPilotBridge speedup must be positive"
        );
        Ok(config)
    }

    fn new(config: &ArduPilotBridgeConfig) -> Self {
        Self {
            port: config.port,
            first_frame_timeout: Duration::from_secs_f64(config.first_frame_timeout_s),
            frame_timeout: Duration::from_secs_f64(config.frame_timeout_s),
            throttle: config.throttle,
            elevator: config.elevator,
            aileron: config.aileron,
            rudder: config.rudder,
            socket: None,
            sitl_address: None,
            applied_frame: None,
            previous_time_s: None,
            // Zero throttle and centred surfaces until SITL drives a channel.
            inputs: MotionInputs::default(),
        }
    }

    fn close(&mut self, _time: StepTime) -> Result<()> {
        // Free the port now; the simulation keeps closed plugins until it is dropped.
        self.socket = None;
        Ok(())
    }

    fn ports(_config: &ArduPilotBridgeConfig) -> Ports {
        Ports::default()
            .output(Port::new("throttle", Unit::Dimensionless, Frame::None))
            .output(Port::new("elevator", Unit::Radians, Frame::Model))
            .output(Port::new("aileron", Unit::Radians, Frame::Model))
            .output(Port::new("rudder", Unit::Radians, Frame::Model))
    }
}

impl Autonomy for ArduPilotBridge {
    fn initialize(&mut self, context: &mut AgentContext<'_>) -> Result<()> {
        context
            .messages
            .subscribe::<SitlSample>("LocalNetwork", SITL_STATE_TOPIC)?;
        let socket = UdpSocket::bind(("0.0.0.0", self.port))
            .with_context(|| format!("ArduPilotBridge could not bind UDP port {}", self.port))?;
        self.socket = Some(socket);
        Ok(())
    }

    fn step(&mut self, context: &mut AgentContext<'_>, io: &mut PluginIo) -> Result<Update> {
        if let Some(previous_time_s) = self.previous_time_s {
            let elapsed_s = context.time.time_s - previous_time_s;
            ensure!(
                (elapsed_s - context.time.dt_s).abs() <= 1e-9 * context.time.dt_s.max(1.0),
                "ArduPilotBridge must run every tick (remove its loop_rate_hz)"
            );
        }
        self.previous_time_s = Some(context.time.time_s);

        // 1. Answer last tick's frame with the state that frame produced.
        let latest_sample = context
            .messages
            .receive::<SitlSample>("LocalNetwork", SITL_STATE_TOPIC)?
            .pop();
        if let Some(frame_count) = self.applied_frame {
            let Some(sample) = latest_sample else {
                bail!(
                    "no SitlState sample to answer SITL frame {frame_count}; \
                     add a SitlState sensor running every tick and LocalNetwork"
                );
            };
            let reply = state_reply(context.time.time_s, &sample.value);
            let (socket, address) = (
                self.socket()?,
                self.sitl_address.context("no SITL address")?,
            );
            socket.send_to(reply.as_bytes(), address)?;
        }

        // 2. Wait for the next new frame.
        let frame = self.next_frame()?;
        self.applied_frame = Some(frame.frame_count);

        // 3. Servo outputs to motion inputs. While booting, SITL sends PWM 0 on
        // channels it is not driving yet; those keep their last input instead
        // of being read as full deflection.
        let inputs = &mut self.inputs;
        inputs.throttle = self.throttle.output(&frame).unwrap_or(inputs.throttle);
        inputs.elevator_rad = self.elevator.output(&frame).unwrap_or(inputs.elevator_rad);
        inputs.aileron_rad = self.aileron.output(&frame).unwrap_or(inputs.aileron_rad);
        inputs.rudder_rad = self.rudder.output(&frame).unwrap_or(inputs.rudder_rad);
        io.write("throttle", inputs.throttle)?;
        io.write("elevator", inputs.elevator_rad)?;
        io.write("aileron", inputs.aileron_rad)?;
        io.write("rudder", inputs.rudder_rad)?;
        Ok(Update::Applied)
    }
}

impl ArduPilotBridgeConfig {
    pub(crate) fn port(&self) -> u16 {
        self.port
    }

    /// The mode to arm in, checked against `waypoints_m` and `goto_m`.
    pub(crate) fn mode(&self) -> Result<&str> {
        ensure!(
            !self.waypoints_m.is_empty() || (self.takeoff_altitude_m.is_none() && !self.land),
            "ArduPilotBridge takeoff_altitude_m and land need waypoints_m"
        );
        ensure!(
            self.waypoints_m.is_empty() || self.goto_m.is_none(),
            "ArduPilotBridge takes waypoints_m (AUTO) or goto_m (GUIDED), not both"
        );
        let needed = if !self.waypoints_m.is_empty() {
            Some(("AUTO", "waypoints_m"))
        } else {
            self.goto_m.map(|_| ("GUIDED", "goto_m"))
        };
        match (self.mode.as_deref(), needed) {
            (Some(mode), Some((needed, key))) if mode != needed => {
                bail!("ArduPilotBridge {key} needs mode {needed}, not {mode}")
            }
            (Some(mode), _) => Ok(mode),
            (None, Some((needed, _))) => Ok(needed),
            (None, None) => Ok("LOITER"),
        }
    }
}

impl ArduPilotBridge {
    fn socket(&self) -> Result<&UdpSocket> {
        self.socket
            .as_ref()
            .context("ArduPilotBridge is not initialized")
    }

    /// Read datagrams until SITL's next new frame. Resends of the frame
    /// already answered are dropped; anything else out of order is an error.
    fn next_frame(&mut self) -> Result<ServoFrame> {
        let timeout = match self.applied_frame {
            None => self.first_frame_timeout,
            Some(_) => self.frame_timeout,
        };
        let deadline = Instant::now() + timeout;
        let mut buffer = [0u8; 512];
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                bail!(
                    "no new frame from ArduPilot SITL on UDP port {} within {:.1} s",
                    self.port,
                    timeout.as_secs_f64()
                );
            }
            let socket = self.socket()?;
            socket.set_read_timeout(Some(remaining))?;
            let (length, source) = match socket.recv_from(&mut buffer) {
                Ok(received) => received,
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    continue;
                }
                Err(error) => return Err(error.into()),
            };
            let frame = parse_servo_packet(&buffer[..length])?;
            match self.applied_frame {
                None => {}
                Some(last) if frame.frame_count == last => continue,
                Some(last) if frame.frame_count == last.wrapping_add(1) => {}
                Some(last) => bail!(
                    "ArduPilot SITL sent frame {} after frame {last}; \
                     a gap means lost lockstep and a lower count means SITL restarted",
                    frame.frame_count
                ),
            }
            self.sitl_address = Some(source);
            return Ok(frame);
        }
    }
}

impl ServoMap {
    fn validate(&self, name: &str) -> Result<()> {
        ensure!(
            self.channel.fract() == 0.0 && (1.0..=16.0).contains(&self.channel),
            "ArduPilotBridge {name} channel must be a whole number from 1 to 16"
        );
        ensure!(
            self.pwm_max_us > self.pwm_min_us,
            "ArduPilotBridge {name} pwm_max_us must exceed pwm_min_us"
        );
        Ok(())
    }

    /// The motion input for this channel's PWM, clamped to the PWM range, or
    /// `None` when the channel reads 0 (SITL is not driving it yet).
    fn output(&self, frame: &ServoFrame) -> Option<f64> {
        let raw_pwm_us = frame.pwm_us[self.channel as usize - 1];
        if raw_pwm_us == 0 {
            return None;
        }
        let pwm_us = f64::from(raw_pwm_us).clamp(self.pwm_min_us, self.pwm_max_us);
        let fraction = (pwm_us - self.pwm_min_us) / (self.pwm_max_us - self.pwm_min_us);
        Some(self.output_min + fraction * (self.output_max - self.output_min))
    }
}

/// `[channel, pwm_min_us, pwm_max_us, output_min, output_max]`, for the
/// defaults and tests.
impl From<[f64; 5]> for ServoMap {
    fn from([channel, pwm_min_us, pwm_max_us, output_min, output_max]: [f64; 5]) -> Self {
        Self {
            channel,
            pwm_min_us,
            pwm_max_us,
            output_min,
            output_max,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame_with(channel: usize, pwm_us: u16) -> ServoFrame {
        let mut pwm = [1500; 16];
        pwm[channel - 1] = pwm_us;
        ServoFrame {
            frame_rate_hz: 400,
            frame_count: 0,
            pwm_us: pwm,
        }
    }

    #[test]
    fn pwm_maps_linearly_and_clamps() {
        let aileron = ArduPilotBridgeConfig::default().aileron;
        let output = |map: ServoMap, frame| map.output(&frame).unwrap();
        assert_eq!(output(aileron, frame_with(1, 1500)), 0.0);
        assert!((output(aileron, frame_with(1, 2000)) - 0.35).abs() < 1e-12);
        assert!((output(aileron, frame_with(1, 1000)) + 0.35).abs() < 1e-12);
        // An out-of-range value is clamped.
        assert!((output(aileron, frame_with(1, 900)) + 0.35).abs() < 1e-12);
        let throttle = ArduPilotBridgeConfig::default().throttle;
        assert!((output(throttle, frame_with(3, 1250)) - 0.25).abs() < 1e-12);
    }

    #[test]
    fn pwm_zero_means_not_driven_yet() {
        let aileron = ArduPilotBridgeConfig::default().aileron;
        assert_eq!(aileron.output(&frame_with(1, 0)), None);
    }

    #[test]
    fn a_mission_names_each_field() {
        let map: ServoMap = serde_json::from_str(
            r#"{"channel": 3, "pwm_min_us": 1000, "pwm_max_us": 2000, "output_min": 0, "output_max": 1}"#,
        )
        .unwrap();
        assert!((map.output(&frame_with(3, 1250)).unwrap() - 0.25).abs() < 1e-12);
    }

    #[test]
    fn swapped_outputs_reverse_a_channel() {
        let reversed = ServoMap::from([2.0, 1000.0, 2000.0, 0.30, -0.30]);
        assert!((reversed.output(&frame_with(2, 2000)).unwrap() + 0.30).abs() < 1e-12);
    }

    #[test]
    fn the_mode_follows_the_route_and_conflicts_are_refused() {
        let config = |change: fn(&mut ArduPilotBridgeConfig)| {
            let mut config = ArduPilotBridgeConfig::default();
            change(&mut config);
            config
        };
        assert_eq!(config(|_| {}).mode().unwrap(), "LOITER");
        let route = |config: &mut ArduPilotBridgeConfig| config.waypoints_m = vec![[1.0, 2.0, 3.0]];
        assert_eq!(config(route).mode().unwrap(), "AUTO");
        assert_eq!(
            config(|config| config.goto_m = Some([1.0, 2.0, 3.0]))
                .mode()
                .unwrap(),
            "GUIDED"
        );
        assert!(
            config(|config| {
                config.waypoints_m = vec![[1.0, 2.0, 3.0]];
                config.goto_m = Some([1.0, 2.0, 3.0]);
            })
            .mode()
            .is_err()
        );
        assert!(
            config(|config| {
                config.waypoints_m = vec![[1.0, 2.0, 3.0]];
                config.mode = Some("GUIDED".into());
            })
            .mode()
            .is_err()
        );
        assert!(config(|config| config.land = true).mode().is_err());
    }

    #[test]
    fn bad_maps_are_rejected() {
        assert!(
            ServoMap::from([0.0, 1000.0, 2000.0, -1.0, 1.0])
                .validate("x")
                .is_err()
        );
        assert!(
            ServoMap::from([17.0, 1000.0, 2000.0, -1.0, 1.0])
                .validate("x")
                .is_err()
        );
        assert!(
            ServoMap::from([1.5, 1000.0, 2000.0, -1.0, 1.0])
                .validate("x")
                .is_err()
        );
        assert!(
            ServoMap::from([1.0, 2000.0, 1000.0, -1.0, 1.0])
                .validate("x")
                .is_err()
        );
    }
}

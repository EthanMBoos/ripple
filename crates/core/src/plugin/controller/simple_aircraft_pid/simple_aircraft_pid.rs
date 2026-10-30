//! Heading, altitude, and airspeed PID loops for SimpleAircraft.
//!
//! Derived from SCRIMMAGE: src/plugins/controller/SimpleAircraftControllerPID/SimpleAircraftControllerPID.cpp.
//! Controller phase: read desired/current state, write throttle and roll and pitch rates.
//!
//! The altitude loop commands pitch rate, and SimpleAircraft climbs at speed x
//! sin(pitch), so the loop's derivative gain is effectively speed x D. Run
//! every dt, it is stable only while speed x D x dt < 2: with the default D of
//! 0.8 and a 0.1 s step, below 25 m/s. Faster, the pitch flips between the
//! rate limits every step. Use motion substeps (`motion_multiplier`) for
//! faster aircraft: at 0.02 s the bound is 125 m/s.

use anyhow::Result;
use serde::Deserialize;

use crate::plugin::{AgentContext, Controller, Frame, PluginIo, Port, Ports, Unit};
use crate::{
    common::{AngleGains, LengthGains, Pid, SpeedGains},
    math,
};

/// Mission parameters; `Default` supplies any key the mission leaves out.
/// Each PID is `{kp, ki, kd, integrate_within_*}`.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ControllerConfig {
    heading_pid: AngleGains,
    altitude_pid: LengthGains,
    speed_pid: SpeedGains,
}

impl Default for ControllerConfig {
    fn default() -> Self {
        Self {
            heading_pid: AngleGains {
                kp: 1.0,
                ki: 0.01,
                kd: 2.0,
                integrate_within_deg: 9.0,
            },
            altitude_pid: LengthGains {
                kp: 1.0,
                ki: 0.0,
                kd: 0.8,
                integrate_within_m: 1.0,
            },
            speed_pid: SpeedGains {
                kp: 1.0,
                ki: 0.1,
                kd: 0.0,
                integrate_within_mps: 1.0,
            },
        }
    }
}

pub struct SimpleAircraftControllerPid {
    heading_pid: Pid,
    altitude_pid: Pid,
    speed_pid: Pid,
}

impl Controller for SimpleAircraftControllerPid {
    type Config = ControllerConfig;

    fn new(config: &ControllerConfig) -> Self {
        Self {
            heading_pid: Pid::angular(config.heading_pid.into()),
            altitude_pid: Pid::linear(config.altitude_pid.into()),
            speed_pid: Pid::linear(config.speed_pid.into()),
        }
    }

    fn ports(_config: &ControllerConfig) -> Ports {
        Ports::default()
            .input(Port::new("desired_heading", Unit::Radians, Frame::World))
            .input(Port::new("desired_altitude", Unit::Meters, Frame::World))
            .input(Port::new(
                "desired_speed",
                Unit::MetersPerSecond,
                Frame::None,
            ))
            .output(Port::new("throttle", Unit::Dimensionless, Frame::None))
            .output(Port::new("roll_rate", Unit::RadiansPerSecond, Frame::Model))
            .output(Port::new(
                "pitch_rate",
                Unit::RadiansPerSecond,
                Frame::Model,
            ))
    }

    fn step(&mut self, context: &mut AgentContext, io: &mut PluginIo) -> Result<()> {
        let desired_heading_world_rad = io.read("desired_heading")?;
        let desired_altitude_world_m = io.read("desired_altitude")?;
        let desired_speed_mps = io.read("desired_speed")?;
        let state = context.state;
        let dt_s = context.time.dt_s;
        let heading_world_rad = state.orientation_world_from_body.yaw_world_from_body_rad();
        let altitude_world_m = state.position_world_m.z;
        let speed_mps = math::norm(state.velocity_world_mps);

        // Heading increases counterclockwise, and a right bank (positive roll)
        // turns clockwise, so a heading short of the goal calls for rolling left.
        let roll_rate_radps =
            -self
                .heading_pid
                .step(desired_heading_world_rad, heading_world_rad, dt_s);
        let altitude_output =
            self.altitude_pid
                .step(desired_altitude_world_m, altitude_world_m, dt_s);
        // Positive pitch is nose down.
        let pitch_rate_radps = -altitude_output;
        let throttle = self.speed_pid.step(desired_speed_mps, speed_mps, dt_s);

        io.write("throttle", throttle)?;
        io.write("roll_rate", roll_rate_radps)?;
        io.write("pitch_rate", pitch_rate_radps)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Controller, SimpleAircraftControllerPid};
    use crate::plugin::PluginParams;

    #[test]
    fn a_mission_pid_replaces_only_that_loop() -> anyhow::Result<()> {
        let params =
            PluginParams::yaml("heading_pid: {kp: 2, ki: 3, kd: 4, integrate_within_deg: 9}");
        let config = SimpleAircraftControllerPid::configure(&params)?;
        assert!((config.heading_pid.kp - 2.0).abs() < 1e-12);
        assert!((config.heading_pid.ki - 3.0).abs() < 1e-12);
        assert!((config.heading_pid.kd - 4.0).abs() < 1e-12);
        assert!((config.heading_pid.integrate_within_deg - 9.0).abs() < 1e-12);
        assert!((config.altitude_pid.kd - 0.8).abs() < 1e-12);
        assert!((config.speed_pid.ki - 0.1).abs() < 1e-12);
        Ok(())
    }

    #[test]
    fn a_band_in_the_wrong_unit_is_an_error() {
        let params =
            PluginParams::yaml("altitude_pid: {kp: 1, ki: 0, kd: 0, integrate_within_deg: 1}");
        let error = SimpleAircraftControllerPid::configure(&params)
            .err()
            .unwrap();
        assert!(
            format!("{error:#}").contains("integrate_within_deg"),
            "{error:#}"
        );
    }
}

//! Heading, altitude, and airspeed PID loops for SimpleAircraft.
//!
//! C++ counterpart: src/plugins/controller/SimpleAircraftControllerPID/SimpleAircraftControllerPID.cpp.
//! Controller phase: read desired/current state, write throttle and model angular rates.

use anyhow::{Result, ensure};
use serde::Deserialize;

use crate::plugin::{
    AgentContext, Controller, Frame, Plugin, PluginIo, PluginParams, Port, Ports, Unit, Update,
};
use crate::{
    common::{Pid, PidGains},
    math,
};

/// Mission parameters; `Default` supplies any key the mission leaves out.
/// Each PID is `P, I, D, integral band`.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ControllerConfig {
    #[serde(rename = "heading_pid")]
    heading_gains: PidGains,
    #[serde(rename = "alt_pid")]
    altitude_gains: PidGains,
    #[serde(rename = "vel_pid")]
    speed_gains: PidGains,
    /// Not yet implemented; must stay false.
    use_roll: bool,
    use_glide_slope: bool,
}

impl Default for ControllerConfig {
    fn default() -> Self {
        Self {
            heading_gains: PidGains::from([1.0, 0.01, 2.0, 9.0]),
            altitude_gains: PidGains::from([1.0, 0.0, 0.8, 1.0]),
            speed_gains: PidGains::from([1.0, 0.1, 0.0, 1.0]),
            use_roll: false,
            use_glide_slope: false,
        }
    }
}

pub struct SimpleAircraftControllerPid {
    heading_pid: Pid,
    altitude_pid: Pid,
    speed_pid: Pid,
}

impl Plugin for SimpleAircraftControllerPid {
    type Config = ControllerConfig;

    fn configure(params: &PluginParams<'_>) -> Result<ControllerConfig> {
        let config: ControllerConfig = params.parse()?;
        ensure!(
            !config.use_roll && !config.use_glide_slope,
            "roll/glide-slope PID inputs are not yet implemented"
        );
        Ok(config)
    }

    fn new(config: &ControllerConfig) -> Self {
        Self {
            heading_pid: Pid::angular(config.heading_gains),
            altitude_pid: Pid::linear(config.altitude_gains),
            speed_pid: Pid::linear(config.speed_gains),
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
}

impl Controller for SimpleAircraftControllerPid {
    fn step(&mut self, context: &mut AgentContext<'_>, io: &mut PluginIo) -> Result<Update> {
        let desired_heading_world_rad = io.read("desired_heading")?;
        let desired_altitude_world_m = io.read("desired_altitude")?;
        let desired_speed_mps = io.read("desired_speed")?;
        let state = context.state;
        let dt_s = context.time.dt_s;
        let heading_world_rad = state.orientation_world_from_body.yaw_world_from_body_rad();
        let altitude_world_m = state.position_world_m.z;
        let speed_mps = math::norm(state.velocity_world_mps);

        let roll_rate_model_radps =
            self.heading_pid
                .step(desired_heading_world_rad, heading_world_rad, dt_s);
        let altitude_output =
            self.altitude_pid
                .step(desired_altitude_world_m, altitude_world_m, dt_s);
        // SimpleAircraft's positive model pitch points downward.
        let pitch_rate_model_radps = -altitude_output;
        let throttle = self.speed_pid.step(desired_speed_mps, speed_mps, dt_s);

        io.write("throttle", throttle)?;
        io.write("roll_rate", roll_rate_model_radps)?;
        io.write("pitch_rate", pitch_rate_model_radps)?;
        Ok(Update::Applied)
    }
}

/// The positional C++ XML format ends here; controller code uses named gains.
#[cfg(test)]
mod tests {
    use super::{Plugin, PluginParams, SimpleAircraftControllerPid};
    use crate::Params;

    #[test]
    fn xml_gains_retain_the_legacy_order_and_heading_band_units() -> anyhow::Result<()> {
        let params = Params::from([("heading_pid".into(), "2,3,4,9".into())]);
        let config = SimpleAircraftControllerPid::configure(&PluginParams::text(&params))?;
        assert!((config.heading_gains.proportional - 2.0).abs() < 1e-12);
        assert!((config.heading_gains.integral - 3.0).abs() < 1e-12);
        assert!((config.heading_gains.derivative - 4.0).abs() < 1e-12);
        assert!((config.heading_gains.integral_band - 9.0).abs() < 1e-12);
        assert!((config.altitude_gains.derivative - 0.8).abs() < 1e-12);
        assert!((config.speed_gains.integral - 0.1).abs() < 1e-12);
        Ok(())
    }
}

//! Velocity commands for a fixed-wing aircraft, the way an autopilot takes
//! them. Rust-only.
//! Controller phase: read the ENU velocity command (`velocity_x`,
//! `velocity_y`, `velocity_z`, the ports SingleIntegrator's and
//! MultirotorVelocityController's commands use) and write the heading,
//! altitude, and speed that SimpleAircraftControllerPID and
//! AircraftPIDController follow. Put it before either in the controller list.
//!
//! - Heading: the command's horizontal direction; a command with no
//!   horizontal part keeps the last heading (at first, the belief's).
//! - Speed: the command's magnitude. The aircraft's speed limits bound it.
//! - Altitude: a target that moves at the commanded climb rate, from the
//!   belief's first altitude. The aircraft's climb and descent limits bound
//!   the climb; the target stays within `max_altitude_error_m` of the belief's
//!   altitude, so a climb the aircraft can't make doesn't leave the target far
//!   above it.
//!
//! There is no holding or tracking mode: when a command can't be flown (too
//! slow, too steep, a turn too tight), the aircraft does what its limits allow.

use anyhow::{Result, ensure};
use serde::Deserialize;

use crate::plugin::{AgentContext, Controller, Frame, PluginIo, PluginParams, Port, Ports, Unit};

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AircraftVelocityConfig {
    max_altitude_error_m: f64,
}

impl Default for AircraftVelocityConfig {
    fn default() -> Self {
        Self {
            max_altitude_error_m: 10.0,
        }
    }
}

pub struct AircraftVelocityController {
    max_altitude_error_m: f64,
    heading_world_rad: f64,
    altitude_world_m: f64,
}

impl Controller for AircraftVelocityController {
    type Config = AircraftVelocityConfig;

    fn configure(params: &PluginParams) -> Result<AircraftVelocityConfig> {
        let config: AircraftVelocityConfig = params.parse()?;
        ensure!(
            config.max_altitude_error_m >= 0.0,
            "max_altitude_error_m must be nonnegative"
        );
        Ok(config)
    }

    fn new(config: &AircraftVelocityConfig) -> Self {
        Self {
            max_altitude_error_m: config.max_altitude_error_m,
            heading_world_rad: 0.0,
            altitude_world_m: 0.0,
        }
    }

    fn ports(_config: &AircraftVelocityConfig) -> Ports {
        Ports::default()
            .input(Port::new("velocity_x", Unit::MetersPerSecond, Frame::World))
            .input(Port::new("velocity_y", Unit::MetersPerSecond, Frame::World))
            .input(Port::new("velocity_z", Unit::MetersPerSecond, Frame::World))
            .output(Port::new("desired_heading", Unit::Radians, Frame::World))
            .output(Port::new("desired_altitude", Unit::Meters, Frame::World))
            .output(Port::new(
                "desired_speed",
                Unit::MetersPerSecond,
                Frame::None,
            ))
    }

    fn initialize(&mut self, context: &mut AgentContext) -> Result<()> {
        self.heading_world_rad = context
            .state
            .orientation_world_from_body
            .yaw_world_from_body_rad();
        self.altitude_world_m = context.state.position_world_m.z;
        Ok(())
    }

    fn step(&mut self, context: &mut AgentContext, io: &mut PluginIo) -> Result<()> {
        let velocity_x_mps = io.read("velocity_x")?;
        let velocity_y_mps = io.read("velocity_y")?;
        let velocity_z_mps = io.read("velocity_z")?;
        if velocity_x_mps != 0.0 || velocity_y_mps != 0.0 {
            self.heading_world_rad = velocity_y_mps.atan2(velocity_x_mps);
        }
        let altitude_m = context.state.position_world_m.z;
        self.altitude_world_m = (self.altitude_world_m + velocity_z_mps * context.time.dt_s).clamp(
            altitude_m - self.max_altitude_error_m,
            altitude_m + self.max_altitude_error_m,
        );
        let speed_mps =
            (velocity_x_mps.powi(2) + velocity_y_mps.powi(2) + velocity_z_mps.powi(2)).sqrt();
        io.write("desired_heading", self.heading_world_rad)?;
        io.write("desired_altitude", self.altitude_world_m)?;
        io.write("desired_speed", speed_mps)?;
        Ok(())
    }
}

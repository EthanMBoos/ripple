//! Fly toward a distant point along the initial heading, holding altitude and speed.
//!
//! Derived from SCRIMMAGE: src/plugins/autonomy/Straight/Straight.cpp.
//! Autonomy phase: read current state, write desired altitude, speed, and heading.

use anyhow::Result;
use serde::Deserialize;

use crate::math::{self, Vec3};
use crate::plugin::interaction::{BOUNDARY_TOPIC, BoundaryRegion};
use crate::plugin::{
    AgentContext, Autonomy, Frame, Plugin, PluginIo, PluginParams, Port, Ports, Unit, Update,
};

const ALTITUDE: &str = "desired_altitude";
const SPEED: &str = "desired_speed";
const HEADING: &str = "desired_heading";

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StraightConfig {
    speed_mps: f64,
    enable_boundary_control: bool,
}

impl Default for StraightConfig {
    fn default() -> Self {
        Self {
            speed_mps: 21.0,
            enable_boundary_control: false,
        }
    }
}

pub struct Straight {
    speed_mps: f64,
    goal_world_m: Vec3,
    enable_boundary_control: bool,
    boundary: Option<BoundaryRegion>,
}

impl Plugin for Straight {
    type Config = StraightConfig;

    fn configure(params: &PluginParams<'_>) -> Result<StraightConfig> {
        params.parse()
    }

    fn new(config: &StraightConfig) -> Self {
        Self {
            speed_mps: config.speed_mps,
            goal_world_m: Vec3::zeros(),
            enable_boundary_control: config.enable_boundary_control,
            boundary: None,
        }
    }

    fn ports(_config: &StraightConfig) -> Ports {
        Ports::default()
            .output(Port::new(ALTITUDE, Unit::Meters, Frame::World))
            .output(Port::new(SPEED, Unit::MetersPerSecond, Frame::None))
            .output(Port::new(HEADING, Unit::Radians, Frame::World))
    }
}

impl Autonomy for Straight {
    fn initialize(&mut self, context: &mut AgentContext<'_>) -> Result<()> {
        let forward_world_m = context
            .state
            .orientation_world_from_body
            .rotate_body_to_world(Vec3::new(1e6, 0.0, 0.0));
        self.goal_world_m = context.state.position_world_m + forward_world_m;
        self.goal_world_m.z = context.state.position_world_m.z;
        context
            .messages
            .subscribe::<BoundaryRegion>("GlobalNetwork", BOUNDARY_TOPIC)?;
        Ok(())
    }

    fn step(&mut self, context: &mut AgentContext<'_>, io: &mut PluginIo) -> Result<Update> {
        for message in context
            .messages
            .receive::<BoundaryRegion>("GlobalNetwork", BOUNDARY_TOPIC)?
        {
            self.boundary = Some(*message.value);
        }
        let state = context.state;
        if self.enable_boundary_control
            && let Some(boundary) = self.boundary
            && !boundary.contains(state.position_world_m)
        {
            let mut center_world_m = boundary.center_world_m;
            center_world_m.z = state.position_world_m.z;
            let toward_center_m = center_world_m - state.position_world_m;
            let distance_m = toward_center_m.norm();
            // Directly above/below the center gives no horizontal direction to turn toward.
            if distance_m > 0.0 {
                self.goal_world_m = state.position_world_m + toward_center_m / distance_m * 1e6;
            }
        }
        let displacement_world_m = math::sub(self.goal_world_m, state.position_world_m);
        let distance_m = math::norm(displacement_world_m);
        // Normalize first, then scale: the rounding order matters over long runs.
        let velocity_world_mps =
            displacement_world_m.map(|component_m| self.speed_mps * (component_m / distance_m));
        let desired_speed_mps = math::norm(velocity_world_mps);
        let desired_heading_world_rad =
            math::angle_2pi(velocity_world_mps.y.atan2(velocity_world_mps.x));

        io.write(ALTITUDE, self.goal_world_m.z)?;
        io.write(SPEED, desired_speed_mps)?;
        io.write(HEADING, desired_heading_world_rad)?;
        Ok(Update::Applied)
    }
}

#[cfg(test)]
mod tests {
    use super::{Plugin, PluginParams, Straight};

    #[test]
    fn configured_speed_is_copied_into_independent_autonomy_instances() -> anyhow::Result<()> {
        let config = Straight::configure(&PluginParams::yaml("speed_mps: 24"))?;
        let mut first = Straight::new(&config);
        let second = Straight::new(&config);
        first.goal_world_m.x = 100.0;
        assert!((second.speed_mps - 24.0).abs() < 1e-12);
        assert!(second.goal_world_m.x.abs() < 1e-12);
        Ok(())
    }
}

//! Head toward the nearest active entity on another team, matching its altitude.
//! Autonomy phase: read contacts, write desired heading, altitude, and speed.
//! This is the plugin from the book's "Your first autonomy plugin" tutorial.

use anyhow::{Result, ensure};
use scrimmage_core::plugin::{
    AgentContext, Autonomy, Frame, Plugin, PluginIo, PluginParams, Port, Ports, Unit, Update,
};
use serde::Deserialize;

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FollowNearestConfig {
    #[serde(rename = "speed")]
    speed_mps: f64,
}

impl Default for FollowNearestConfig {
    fn default() -> Self {
        Self { speed_mps: 25.0 }
    }
}

pub struct FollowNearest {
    speed_mps: f64,
}

impl Plugin for FollowNearest {
    type Config = FollowNearestConfig;

    fn configure(params: &PluginParams<'_>) -> Result<FollowNearestConfig> {
        let config: FollowNearestConfig = params.parse()?;
        ensure!(
            config.speed_mps > 0.0,
            "FollowNearest speed must be positive"
        );
        Ok(config)
    }

    fn new(config: &FollowNearestConfig) -> Self {
        Self {
            speed_mps: config.speed_mps,
        }
    }

    fn ports(_config: &FollowNearestConfig) -> Ports {
        Ports::default()
            .output(Port::new("desired_heading", Unit::Radians, Frame::World))
            .output(Port::new("desired_altitude", Unit::Meters, Frame::World))
            .output(Port::new(
                "desired_speed",
                Unit::MetersPerSecond,
                Frame::None,
            ))
    }
}

impl Autonomy for FollowNearest {
    fn step(&mut self, context: &mut AgentContext<'_>, io: &mut PluginIo) -> Result<Update> {
        // Input: our own state and every other entity's true state.
        let own_position_world_m = context.state.position_world_m;

        // Calculation: find the closest active opponent.
        let mut nearest = None;
        let mut nearest_distance_m = f64::INFINITY;
        for contact in context.contacts_truth {
            if !contact.active || contact.team_id == context.entity.team_id {
                continue;
            }
            let distance_m = (contact.truth.position_world_m - own_position_world_m).norm();
            if distance_m < nearest_distance_m {
                nearest_distance_m = distance_m;
                nearest = Some(contact);
            }
        }

        // Output: head toward it, or hold the current heading and altitude if none remain.
        let (heading_world_rad, altitude_world_m) = match nearest {
            Some(target) => {
                let offset_world_m = target.truth.position_world_m - own_position_world_m;
                (
                    offset_world_m.y.atan2(offset_world_m.x),
                    target.truth.position_world_m.z,
                )
            }
            None => (
                context
                    .state
                    .orientation_world_from_body
                    .yaw_world_from_body_rad(),
                own_position_world_m.z,
            ),
        };
        io.write("desired_heading", heading_world_rad)?;
        io.write("desired_altitude", altitude_world_m)?;
        io.write("desired_speed", self.speed_mps)?;
        Ok(Update::Applied)
    }
}

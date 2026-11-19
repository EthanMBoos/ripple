//! Head toward the nearest known contact on another team, matching its altitude.
//! Autonomy phase: read contacts, write desired heading, altitude, and speed.
//! This is the plugin from the book's "Your first autonomy plugin" tutorial.

use anyhow::{Result, ensure};
use ripple_core::plugin::{
    AgentContext, Autonomy, Frame, PluginIo, PluginParams, Port, Ports, Unit,
};
use serde::Deserialize;

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FollowNearestConfig {
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

impl Autonomy for FollowNearest {
    type Config = FollowNearestConfig;

    fn configure(params: &PluginParams) -> Result<FollowNearestConfig> {
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

    fn step(&mut self, context: &mut AgentContext, io: &mut PluginIo) -> Result<()> {
        // Input: our own estimated state and the tracks we keep on others.
        // Each is a Gaussian; the policy here reads the means.
        let own_position_world_m = context.state.mean.position_world_m;

        // Calculation: find the closest opponent.
        let mut nearest = None;
        let mut nearest_distance_m = f64::INFINITY;
        for track in context.tracks.iter() {
            // A track the sensor could not identify might be a teammate; the
            // stock sensors identify every one.
            let on_own_team = track
                .identity
                .is_some_and(|identity| identity.team_id == context.entity.team_id);
            if on_own_team {
                continue;
            }
            let distance_m = (track.state.mean.position_world_m - own_position_world_m).norm();
            if distance_m < nearest_distance_m {
                nearest_distance_m = distance_m;
                nearest = Some(track);
            }
        }

        // Output: head toward it, or hold the current heading and altitude if none remain.
        let (heading_world_rad, altitude_world_m) = match nearest {
            Some(target) => {
                let offset_world_m = target.state.mean.position_world_m - own_position_world_m;
                (
                    offset_world_m.y.atan2(offset_world_m.x),
                    target.state.mean.position_world_m.z,
                )
            }
            None => (
                context
                    .state
                    .mean
                    .orientation_world_from_body
                    .yaw_world_from_body_rad(),
                own_position_world_m.z,
            ),
        };
        io.write("desired_heading", heading_world_rad)?;
        io.write("desired_altitude", altitude_world_m)?;
        io.write("desired_speed", self.speed_mps)?;
        Ok(())
    }
}

//! Remove live entities at or below a flat ground height and publish GroundCollision.
//! C++ counterpart: interaction/GroundCollision; startup check and interaction phase.
//! No terrain lookup, geodetic conversion, or external-force response in this version.

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::plugin::{Interaction, InteractionContext, Plugin, PluginParams, Update};
use crate::{Entity, EventKind, Vec3};

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct GroundCollisionParams {
    ground_collision_z: f64,
    enable_startup_collisions: bool,
    /// `all`, or one team ID.
    team: String,
    /// The force response is not implemented; must stay true.
    remove_on_collision: bool,
    /// Geodetic altitude is not implemented; must be absent.
    ground_collision_altitude: Option<f64>,
}

impl Default for GroundCollisionParams {
    fn default() -> Self {
        Self {
            ground_collision_z: 0.0,
            enable_startup_collisions: true,
            team: "all".into(),
            remove_on_collision: true,
            ground_collision_altitude: None,
        }
    }
}

#[derive(Clone, Copy)]
pub struct GroundCollisionConfig {
    height_world_m: f64,
    startup_collisions: bool,
    team_id: Option<i32>,
}

pub struct GroundCollision {
    config: GroundCollisionConfig,
}

impl Plugin for GroundCollision {
    type Config = GroundCollisionConfig;

    fn configure(params: &PluginParams<'_>) -> Result<GroundCollisionConfig> {
        let params: GroundCollisionParams = params.parse()?;
        ensure!(
            params.remove_on_collision,
            "GroundCollision force response is not implemented; use remove_on_collision=true"
        );
        ensure!(
            params.ground_collision_altitude.is_none(),
            "GroundCollision geodetic altitude is not implemented; use ground_collision_z in local meters"
        );
        let team_id = match params.team.as_str() {
            "all" => None,
            team => Some(
                team.parse()
                    .context("GroundCollision team must be 'all' or an integer")?,
            ),
        };
        Ok(GroundCollisionConfig {
            height_world_m: params.ground_collision_z,
            startup_collisions: params.enable_startup_collisions,
            team_id,
        })
    }

    fn new(config: &GroundCollisionConfig) -> Self {
        Self { config: *config }
    }
}

impl Interaction for GroundCollision {
    fn collision_exists(&self, _entities: &[Entity], position_world_m: Vec3) -> bool {
        // Like C++, startup candidates have no team filter here.
        self.config.startup_collisions && position_world_m.z <= self.config.height_world_m
    }

    fn step(&mut self, context: &mut InteractionContext<'_>) -> Result<Update> {
        for index in 0..context.entities.len() {
            let entity = &mut context.entities[index];
            if entity.health() <= 0
                || self
                    .config
                    .team_id
                    .is_some_and(|team| team != entity.team_id())
            {
                continue;
            }
            if entity.truth().position_world_m.z <= self.config.height_world_m {
                entity.set_health(0);
                let id = entity.id();
                context.emit(EventKind::GroundCollision, vec![id])?;
            }
        }
        Ok(Update::Applied)
    }
}

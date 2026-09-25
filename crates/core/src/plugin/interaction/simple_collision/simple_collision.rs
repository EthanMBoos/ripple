//! Fixed-distance collision detection, matching the C++ SimpleCollision plugin.
//!
//! C++ counterpart: src/plugins/interaction/SimpleCollision/SimpleCollision.cpp.
//! Startup checks candidate positions; the interaction phase marks collisions and emits events.

use anyhow::{Result, ensure};
use serde::Deserialize;

use crate::plugin::{Interaction, InteractionContext, Plugin, PluginParams, Update};
use crate::{Entity, EventKind, Vec3};

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CollisionConfig {
    #[serde(rename = "collision_range")]
    collision_range_m: f64,
    #[serde(rename = "startup_collision_range")]
    startup_collision_range_m: f64,
    #[serde(rename = "startup_collisions_only")]
    startup_only: bool,
    #[serde(rename = "enable_team_collisions")]
    same_team_enabled: bool,
    #[serde(rename = "enable_non_team_collisions")]
    opposing_team_enabled: bool,
    #[serde(rename = "init_alt_deconflict")]
    altitude_deconfliction: bool,
}

impl Default for CollisionConfig {
    fn default() -> Self {
        Self {
            collision_range_m: 2.0,
            startup_collision_range_m: 2.0,
            startup_only: false,
            same_team_enabled: true,
            opposing_team_enabled: true,
            altitude_deconfliction: false,
        }
    }
}

pub struct SimpleCollision {
    config: CollisionConfig,
}

impl Plugin for SimpleCollision {
    type Config = CollisionConfig;

    fn configure(params: &PluginParams<'_>) -> Result<CollisionConfig> {
        let config: CollisionConfig = params.parse()?;
        ensure!(
            config.collision_range_m >= 0.0 && config.startup_collision_range_m >= 0.0,
            "collision ranges must be nonnegative"
        );
        Ok(config)
    }

    fn new(config: &CollisionConfig) -> Self {
        Self {
            config: config.clone(),
        }
    }
}

impl Interaction for SimpleCollision {
    fn collision_exists(&self, entities: &[Entity], position_world_m: Vec3) -> bool {
        entities.iter().any(|entity| {
            let separation_world_m = position_world_m - entity.truth().position_world_m;
            if self.config.altitude_deconfliction {
                separation_world_m.z.abs() <= self.config.startup_collision_range_m
            } else {
                separation_world_m.norm() <= self.config.startup_collision_range_m
            }
        })
    }

    fn step(&mut self, context: &mut InteractionContext<'_>) -> Result<Update> {
        if self.config.startup_only {
            return Ok(Update::Applied);
        }

        for first_index in 0..context.entities.len() {
            for second_index in first_index + 1..context.entities.len() {
                // Split the slice so each contact is borrowed mutably and exclusively.
                // Pair order is observable when one collision disables later pairs.
                let (preceding, following) = context.entities.split_at_mut(second_index);
                let first = &mut preceding[first_index];
                let second = &mut following[0];
                if first.health() <= 0 || second.health() <= 0 {
                    continue;
                }

                let same_team = first.team_id() == second.team_id();
                let enabled = if same_team {
                    self.config.same_team_enabled
                } else {
                    self.config.opposing_team_enabled
                };
                let separation_m =
                    (first.truth().position_world_m - second.truth().position_world_m).norm();
                if !enabled || separation_m >= self.config.collision_range_m {
                    continue;
                }

                first.set_health(0);
                second.set_health(0);
                let ids = vec![first.id(), second.id()];
                let kind = if same_team {
                    EventKind::TeamCollision
                } else {
                    EventKind::NonTeamCollision
                };
                context.emit(kind, ids)?;
            }
        }
        Ok(Update::Applied)
    }
}

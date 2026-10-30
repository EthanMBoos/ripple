//! Fixed-distance collision detection.
//!
//! Derived from SCRIMMAGE: src/plugins/interaction/SimpleCollision/SimpleCollision.cpp.
//! Startup checks candidate positions; the interaction phase marks collisions and emits events.
//! The interaction phase asks the engine for pairs in range (`SpatialIndex::pairs_within`),
//! so a mission's `spatial_lookup` grid speeds it up without changing its results.

use anyhow::{Result, ensure};
use serde::Deserialize;

use crate::plugin::{Interaction, InteractionContext, PluginParams, Update};
use crate::{Entity, EventKind, Vec3};

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CollisionConfig {
    collision_range_m: f64,
    startup_collision_range_m: f64,
    startup_collisions_only: bool,
    enable_team_collisions: bool,
    enable_non_team_collisions: bool,
    startup_altitude_only: bool,
}

impl Default for CollisionConfig {
    fn default() -> Self {
        Self {
            collision_range_m: 2.0,
            startup_collision_range_m: 2.0,
            startup_collisions_only: false,
            enable_team_collisions: true,
            enable_non_team_collisions: true,
            startup_altitude_only: false,
        }
    }
}

pub struct SimpleCollision {
    config: CollisionConfig,
}

impl Interaction for SimpleCollision {
    type Config = CollisionConfig;

    fn configure(params: &PluginParams) -> Result<CollisionConfig> {
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

    fn collision_exists(&self, entities: &[Entity], position_world_m: Vec3) -> bool {
        entities.iter().any(|entity| {
            let separation_world_m = position_world_m - entity.truth().position_world_m;
            if self.config.startup_altitude_only {
                separation_world_m.z.abs() <= self.config.startup_collision_range_m
            } else {
                separation_world_m.norm() <= self.config.startup_collision_range_m
            }
        })
    }

    fn step(&mut self, context: &mut InteractionContext) -> Result<Update> {
        if self.config.startup_collisions_only {
            return Ok(Update::Applied);
        }

        // Pairs come in the order a loop over every pair (first < second) visits
        // them, skipping pairs out of range. The order is observable: a collision
        // disables its agents, so the pair visited first decides who survives.
        for (first_index, second_index) in
            context.nearby.pairs_within(self.config.collision_range_m)
        {
            // Split the slice so each contact is borrowed mutably and exclusively.
            let (preceding, following) = context.entities.split_at_mut(second_index);
            let first = &mut preceding[first_index];
            let second = &mut following[0];
            if first.health() <= 0 || second.health() <= 0 {
                continue;
            }

            let same_team = first.team_id() == second.team_id();
            let enabled = if same_team {
                self.config.enable_team_collisions
            } else {
                self.config.enable_non_team_collisions
            };
            if !enabled {
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
        Ok(Update::Applied)
    }
}

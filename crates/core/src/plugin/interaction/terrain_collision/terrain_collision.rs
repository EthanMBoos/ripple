//! Remove live entities that reach the mission's terrain and publish GroundCollision.
//! Needs a mission `terrain:`; see `common/terrain.rs`. No C++ counterpart: C++
//! terrain was display-only (see docs/REFERENCE_NOTES.md, "Terrain").
//!
//! Each step checks the straight line from where an entity was at the end of
//! the last step to where it is now, exactly against the interpolated ground.
//! Only step endpoints are seen, so a path that curves within a step (through
//! motion substeps, say) can clip terrain the straight line misses, and more
//! substeps do not change that. An entity spawned during the
//! run has already moved one step when this plugin first sees it, and only
//! where it ended that step is checked (see `step`).
//! Entities are points: set `clearance_m` to collide that far above the
//! ground. Off the terrain grid the ground is flat at `off_terrain_z`; inside
//! it, the terrain must have no missing samples.
//!
//! FixedWing6DOF's own flat ground at `z = 0` (`use_ground_model`) must be off
//! in terrain missions; FixedWing6DOF refuses to start otherwise.
// TODO(terrain): any touch removes the entity. Once motion models support
// ground contact, aircraft meant to land need a crash rule that tells a
// touchdown on the gear from a wingtip or fuselage strike.

use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::collections::BTreeMap;

use crate::plugin::{Interaction, InteractionContext, Plugin, PluginParams, Update, WorldContext};
use crate::{Entity, EventKind, Terrain, Vec3};

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct TerrainCollisionParams {
    /// Collide this far above the ground (m).
    clearance_m: f64,
    /// Ground height (local z) outside the terrain grid.
    off_terrain_z: f64,
    enable_startup_collisions: bool,
    /// `all`, or one team ID.
    team: String,
}

impl Default for TerrainCollisionParams {
    fn default() -> Self {
        Self {
            clearance_m: 0.0,
            off_terrain_z: 0.0,
            enable_startup_collisions: true,
            team: "all".into(),
        }
    }
}

#[derive(Clone)]
pub struct TerrainCollisionConfig {
    clearance_m: f64,
    off_terrain_z: f64,
    startup_collisions: bool,
    team_id: Option<i32>,
}

pub struct TerrainCollision {
    config: TerrainCollisionConfig,
    /// Set in `initialize` from the mission's terrain.
    terrain: Option<Terrain>,
    /// Where each live, checked entity was at the end of the last step.
    last_position_world_m: BTreeMap<i32, Vec3>,
}

impl Plugin for TerrainCollision {
    type Config = TerrainCollisionConfig;

    fn configure(params: &PluginParams<'_>) -> Result<TerrainCollisionConfig> {
        let params: TerrainCollisionParams = params.parse()?;
        ensure!(
            params.clearance_m.is_finite() && params.clearance_m >= 0.0,
            "TerrainCollision clearance_m must be zero or more"
        );
        let team_id = match params.team.as_str() {
            "all" => None,
            team => Some(
                team.parse()
                    .context("TerrainCollision team must be 'all' or an integer")?,
            ),
        };
        Ok(TerrainCollisionConfig {
            clearance_m: params.clearance_m,
            off_terrain_z: params.off_terrain_z,
            startup_collisions: params.enable_startup_collisions,
            team_id,
        })
    }

    fn new(config: &TerrainCollisionConfig) -> Self {
        Self {
            config: config.clone(),
            terrain: None,
            last_position_world_m: BTreeMap::new(),
        }
    }
}

impl TerrainCollision {
    /// Height above the ground plus clearance; zero or less is a collision.
    fn clearance_left_m(&self, position_world_m: Vec3) -> f64 {
        let terrain = self.terrain.as_ref().expect("set in initialize");
        let ground_z = terrain
            .height_at(position_world_m.x, position_world_m.y)
            .unwrap_or(self.config.off_terrain_z);
        position_world_m.z - ground_z - self.config.clearance_m
    }

    /// Whether the straight line from `start` to `end` touches the ground.
    ///
    /// Within one grid cell the interpolated ground along a straight line is a
    /// quadratic in distance, so the clearance left is too (the entity's height
    /// is linear). The line is cut where it crosses grid lines, and each piece
    /// is checked at its ends, its middle, and the quadratic's turning point.
    fn path_touches_ground(&self, start_world_m: Vec3, end_world_m: Vec3) -> bool {
        let terrain = self.terrain.as_ref().expect("set in initialize");
        let [west_m, north_m, _] = terrain.sample(0, 0);
        let [east_m, south_m, _] = terrain.sample(terrain.rows() - 1, terrain.columns() - 1);
        let mut cuts = vec![0.0, 1.0];
        for (from_m, to_m, grid_origin_m) in [
            (start_world_m.x, end_world_m.x, west_m),
            (start_world_m.y, end_world_m.y, north_m),
        ] {
            let (from, to) = (
                (from_m - grid_origin_m) / terrain.cell_m(),
                (to_m - grid_origin_m) / terrain.cell_m(),
            );
            if from != to {
                for line in from.min(to).ceil() as i64..=from.max(to).floor() as i64 {
                    cuts.push((line as f64 - from) / (to - from));
                }
            }
        }
        cuts.sort_by(f64::total_cmp);
        let position_at = |t: f64| start_world_m + (end_world_m - start_world_m) * t;
        cuts.windows(2).any(|piece| {
            // All three values must come from one surface, or the fitted curve
            // is meaningless: a piece ending on the grid's edge would mix the
            // flat ground outside with the terrain at the edge. So decide from
            // the piece's middle whether it lies on the grid, and on the grid
            // keep its ends inside it despite rounding at the edge.
            let on_grid = {
                let middle_m = position_at((piece[0] + piece[1]) / 2.0);
                terrain.height_at(middle_m.x, middle_m.y).is_some()
            };
            let clearance_at = |t: f64| {
                let position_m = position_at(t);
                let ground_z = if on_grid {
                    terrain
                        .height_at(
                            position_m.x.clamp(west_m, east_m),
                            position_m.y.clamp(south_m, north_m),
                        )
                        .expect("inside the grid")
                } else {
                    self.config.off_terrain_z
                };
                position_m.z - ground_z - self.config.clearance_m
            };
            let (start, end) = (clearance_at(piece[0]), clearance_at(piece[1]));
            let middle = clearance_at((piece[0] + piece[1]) / 2.0);
            // With u from -1 (start) to 1 (end), clearance is
            // middle + (end - start) / 2 * u + curvature / 2 * u^2.
            let curvature = start + end - 2.0 * middle;
            let turning_u = (start - end) / (2.0 * curvature);
            let lowest_between = if curvature > 0.0 && turning_u.abs() < 1.0 {
                middle - (end - start).powi(2) / (8.0 * curvature)
            } else {
                f64::INFINITY
            };
            start.min(end).min(middle).min(lowest_between) <= 0.0
        })
    }
}

impl Interaction for TerrainCollision {
    fn initialize(&mut self, context: &mut WorldContext<'_>) -> Result<()> {
        let terrain = context
            .terrain
            .context("TerrainCollision needs the mission to name a terrain")?;
        ensure!(
            terrain.missing_samples() == 0,
            "TerrainCollision needs complete terrain; {} samples have no height",
            terrain.missing_samples()
        );
        self.terrain = Some(terrain.clone());
        Ok(())
    }

    fn collision_exists(&self, _entities: &[Entity], position_world_m: Vec3) -> bool {
        // Like GroundCollision, startup candidates have no team filter here.
        self.config.startup_collisions && self.clearance_left_m(position_world_m) <= 0.0
    }

    fn step(&mut self, context: &mut InteractionContext<'_>) -> Result<Update> {
        let last_position_world_m = std::mem::take(&mut self.last_position_world_m);
        for index in 0..context.entities.len() {
            let entity = &context.entities[index];
            if entity.health() <= 0
                || self
                    .config
                    .team_id
                    .is_some_and(|team| team != entity.team_id())
            {
                continue;
            }
            let (id, end_world_m) = (entity.id(), entity.truth().position_world_m);
            // An entity seen for the first time has no earlier position here:
            // either it was just created and has not moved, or it spawned
            // during the run and has moved one step. Interactions only see
            // positions after motion, and guessing the start from velocity is
            // wrong after a turn or acceleration (and SimpleAircraft's vertical
            // velocity has the opposite sign to its climb), so only its end is
            // checked. A new spawn can therefore cross a thin ridge unnoticed
            // on its first step only.
            let start_world_m = last_position_world_m
                .get(&id)
                .copied()
                .unwrap_or(end_world_m);
            if self.path_touches_ground(start_world_m, end_world_m) {
                context.entities[index].set_health(0);
                context.emit(EventKind::GroundCollision, vec![id])?;
            } else {
                self.last_position_world_m.insert(id, end_world_m);
            }
        }
        Ok(Update::Applied)
    }
}

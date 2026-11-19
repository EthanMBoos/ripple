//! Generation: the mission's spawn schedules, the spawn requests plugins
//! publish, and the random draws behind a start. The entities themselves are
//! built in spawning.rs.

use super::Simulation;
use crate::entity::{EntityDefinition, SpawnConfig};
use crate::pubsub::messages::MessageEndpoint;
use crate::scenario::{EntityGroupConfig, PluginConfig};
use crate::{common::random, math::Vec3};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_yaml_ng::Mapping;
use vstd::prelude::*;

verus! {

/// A plugin's request that the engine build entities from one of the
/// mission's `recipes`, published on `GlobalNetwork` under
/// `SpawnRequest::TOPIC`. The recipe says what; the request says where, how
/// many, and which plugin values differ. The engine builds them at the
/// start of the next tick, in delivery order, so a request published while
/// an entity initializes gives entities present from tick 1.
///
/// ```ignore
/// context.messages.publish(
///     "GlobalNetwork",
///     SpawnRequest::TOPIC,
///     SpawnRequest::recipe("fixed_wing")
///         .count(3)
///         .at(hub_world_m)
///         .autonomy("Courier", CourierHome { hub, region })?,
/// )?;
/// ```
#[verifier::external_derive]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpawnRequest {
    /// The label of a recipe in the mission.
    pub recipe: String,
    /// Replaces the recipe's `position_m`; the recipe's spread still applies.
    pub position_m: Vec3,
    /// Replaces the recipe's `heading_deg` when given.
    pub heading_deg: Option<f64>,
    pub count: usize,
    /// Plugin values laid over the recipe's.
    pub overlays: Vec<PluginOverlay>,
}

/// Values for one of a recipe's plugins, named by its label in the mission,
/// laid over the values the recipe gives it key by key.
#[verifier::external_derive]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginOverlay {
    pub slot: PluginSlot,
    pub plugin: String,
    pub values: Mapping,
}

/// An entity's plugin slots, as the mission names them.
#[verifier::external_derive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginSlot {
    Autonomy,
    Controller,
    MotionModel,
    Sensors,
    Localization,
    Tracking,
}

// Declared here so the spawn claims can read a request; the rest of this
// file is not proved and stays outside the block.
impl SpawnRequest {
    pub const TOPIC: &'static str = "SpawnRequest";
}

} // verus!

impl PluginSlot {
    fn name(self) -> &'static str {
        match self {
            Self::Autonomy => "autonomy",
            Self::Controller => "controller",
            Self::MotionModel => "motion_model",
            Self::Sensors => "sensors",
            Self::Localization => "localization",
            Self::Tracking => "tracking",
        }
    }
}

impl SpawnRequest {
    /// One entity from the recipe, at the recipe's position and heading.
    pub fn recipe(label: impl Into<String>) -> Self {
        Self {
            recipe: label.into(),
            position_m: Vec3::zeros(),
            heading_deg: None,
            count: 1,
            overlays: Vec::new(),
        }
    }

    pub fn at(mut self, position_m: Vec3) -> Self {
        self.position_m = position_m;
        self
    }

    pub fn heading(mut self, heading_deg: f64) -> Self {
        self.heading_deg = Some(heading_deg);
        self
    }

    pub fn count(mut self, count: usize) -> Self {
        self.count = count;
        self
    }

    /// Values for the recipe's autonomy plugin `plugin`, from any struct or
    /// map that serializes to a map.
    pub fn autonomy(self, plugin: impl Into<String>, values: impl Serialize) -> Result<Self> {
        self.overlay(PluginSlot::Autonomy, plugin, values)
    }

    /// Values for one plugin of the recipe's `slot`.
    pub fn overlay(
        mut self,
        slot: PluginSlot,
        plugin: impl Into<String>,
        values: impl Serialize,
    ) -> Result<Self> {
        let serde_yaml_ng::Value::Mapping(values) = serde_yaml_ng::to_value(values)? else {
            bail!("plugin values must serialize to a map");
        };
        self.overlays.push(PluginOverlay {
            slot,
            plugin: plugin.into(),
            values,
        });
        Ok(self)
    }
}

#[derive(Clone)]
pub(crate) struct Generator {
    pub(crate) definition_index: usize,
    pub(crate) remaining: usize,
    pub(crate) next_spawn_times_s: Vec<f64>,
    pub(crate) rate_hz: f64,
    pub(crate) time_stddev_s: f64,
    /// Entities built from this group so far, scheduled or requested; the
    /// next one's number in its group.
    pub(crate) spawned: u64,
}

/// What a spawn's random number is for, the fourth part of its label.
#[derive(Clone, Copy)]
enum SpawnField {
    NextTime = 0,
    Heading = 1,
    X = 2,
    Y = 3,
    Z = 4,
}

impl Simulation {
    /// How many each schedule builds this tick, in group order, and when
    /// its next batches come. A batch spawns on the tick nearest its time,
    /// so rounding (t + 1/rate landing just after a tick) cannot delay it a
    /// tick.
    pub(super) fn due_now(&mut self) -> Vec<(usize, usize)> {
        let mut due = Vec::new();
        let half_tick_s = self.config.run.dt_s / 2.0;
        for generator in &mut self.generators {
            let mut count = 0;
            for next_spawn_s in &mut generator.next_spawn_times_s {
                if self.time_s < *next_spawn_s - half_tick_s || generator.remaining == 0 {
                    continue;
                }
                generator.remaining -= 1;
                let number = generator.spawned + count as u64;
                count += 1;

                if generator.rate_hz > 0.0 {
                    let expected_next_spawn_s = self.time_s + 1.0 / generator.rate_hz;
                    let label = spawn_label(
                        self.config.run.seed,
                        generator.definition_index,
                        number,
                        SpawnField::NextTime,
                        0,
                    );
                    *next_spawn_s =
                        random::normal(label, expected_next_spawn_s, generator.time_stddev_s);
                    if *next_spawn_s <= self.time_s {
                        *next_spawn_s = expected_next_spawn_s;
                    }
                }
            }
            if count > 0 {
                due.push((generator.definition_index, count));
            }
        }
        due
    }

    /// The request's recipe with the request's values laid over it, as a
    /// definition ready to build from, and its group index (the recipes are
    /// numbered after the entities).
    pub(super) fn requested_definition(
        &self,
        request: &SpawnRequest,
    ) -> Result<(usize, EntityDefinition)> {
        let position = self
            .config
            .recipes
            .iter()
            .position(|recipe| recipe.label == request.recipe)
            .with_context(|| format!("the mission has no recipe `{}`", request.recipe))?;
        let group = overlaid(&self.config.recipes[position], request)?;
        group.validate()?;
        let definition = EntityDefinition::parse(&group, &self.registry)?;
        Ok((self.config.entities.len() + position, definition))
    }

    /// Chooses a start: position and heading sampled around the spawn's.
    /// `number` is the spawn's number in its group; each try at a
    /// collision-free position has its own numbers.
    pub(super) fn sample_spawn_start(
        &self,
        group_index: usize,
        number: u64,
        spawn: &SpawnConfig,
    ) -> Result<(Vec3, f64)> {
        let seed = self.config.run.seed;
        let draw = |field, attempt, mean, stddev| {
            let label = spawn_label(seed, group_index, number, field, attempt);
            random::normal(label, mean, stddev)
        };
        let origin_world_m = spawn.position_world_m;
        let heading_world_deg = draw(
            SpawnField::Heading,
            0,
            spawn.heading_world_deg,
            spawn.heading_stddev_deg,
        );
        let mut position_world_m = origin_world_m;

        // A spread is always sampled; an exact start only moves off something
        // already there.
        let stddev_world_m = spawn.position_stddev_world_m;
        if stddev_world_m != Vec3::zeros() || self.startup_collision_exists(position_world_m) {
            let mut found_free_position = false;

            for attempt in 0..1_000_000 {
                position_world_m = Vec3::new(
                    draw(SpawnField::X, attempt, origin_world_m.x, stddev_world_m.x),
                    draw(SpawnField::Y, attempt, origin_world_m.y, stddev_world_m.y),
                    draw(SpawnField::Z, attempt, origin_world_m.z, stddev_world_m.z),
                );
                if !self.startup_collision_exists(position_world_m) {
                    found_free_position = true;
                    break;
                }
            }
            ensure!(
                found_free_position,
                "unable to find collision-free spawn position"
            );
        }
        Ok((position_world_m, heading_world_deg))
    }
}

/// The recipe with the request's position, heading and plugin values.
fn overlaid(recipe: &EntityGroupConfig, request: &SpawnRequest) -> Result<EntityGroupConfig> {
    let mut group = recipe.clone();
    group.position_m = request.position_m;
    if let Some(heading_deg) = request.heading_deg {
        group.heading_deg = heading_deg;
    }
    for overlay in &request.overlays {
        let plugins: &mut [PluginConfig] = match overlay.slot {
            PluginSlot::Autonomy => &mut group.autonomy,
            PluginSlot::Controller => &mut group.controller,
            PluginSlot::Sensors => &mut group.sensors,
            PluginSlot::MotionModel => std::slice::from_mut(&mut group.motion_model),
            PluginSlot::Localization => std::slice::from_mut(&mut group.localization),
            PluginSlot::Tracking => std::slice::from_mut(&mut group.tracking),
        };
        let plugin = plugins
            .iter_mut()
            .find(|plugin| plugin.instance.as_deref().unwrap_or(&plugin.name) == overlay.plugin)
            .with_context(|| {
                format!(
                    "the recipe has no {} plugin `{}`",
                    overlay.slot.name(),
                    overlay.plugin
                )
            })?;
        for (key, value) in &overlay.values {
            plugin.params.insert(key.clone(), value.clone());
        }
    }
    Ok(group)
}

pub(super) fn describe(sender: &MessageEndpoint) -> String {
    match sender.entity_id {
        Some(id) => format!("{} of entity {id}", sender.plugin),
        None => sender.plugin.clone(),
    }
}

impl Generator {
    /// The next spawn's number in the group, moving the count on by one.
    pub(crate) fn next_number(&mut self) -> u64 {
        let number = self.spawned;
        self.spawned += 1;
        number
    }

    pub(crate) fn new(
        config: &EntityGroupConfig,
        definition_index: usize,
        start_s: f64,
    ) -> Result<Self> {
        let (rate_hz, batch, first_spawn_s) = match &config.spawn {
            Some(schedule) => (schedule.rate_hz, schedule.batch_size, schedule.start_s),
            None => (-1.0, config.count, start_s - 1.0),
        };
        let time_stddev_s = config
            .spawn
            .as_ref()
            .map_or(0.0, |spawn| spawn.time_stddev_s);
        ensure!(time_stddev_s >= 0.0, "negative generation variance");
        Ok(Self {
            definition_index,
            remaining: config.count,
            next_spawn_times_s: vec![first_spawn_s; batch],
            rate_hz,
            time_stddev_s,
            spawned: 0,
        })
    }
}

/// The label of a spawn's random number: the seed, "spawn", the group, the
/// spawn's number in it, what the number is for, and the try (0 except for a
/// position retried after a collision).
fn spawn_label(seed: u32, group: usize, number: u64, field: SpawnField, attempt: u64) -> u64 {
    random::label(&[
        u64::from(seed),
        random::name("spawn"),
        group as u64,
        number,
        field as u64,
        attempt,
    ])
}

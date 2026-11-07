//! Generation: the mission's spawn schedules and the spawn requests plugins
//! publish. Every entity is built through `spawn`.

use super::{Event, EventKind, Simulation};
use crate::entity::{Entity, EntityDefinition, SpawnConfig};
use crate::plugin::StepTime;
use crate::pubsub::messages::MessageEndpoint;
use crate::scenario::{EntityGroupConfig, PluginConfig};
use crate::{common::random, math::Vec3};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_yaml_ng::Mapping;
use std::sync::Arc;

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
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginOverlay {
    pub slot: PluginSlot,
    pub plugin: String,
    pub values: Mapping,
}

/// An entity's plugin slots, as the mission names them.
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
    pub const TOPIC: &'static str = "SpawnRequest";

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
    /// The start of a tick: the entities plugins asked for since the last
    /// tick, then the batches the mission's schedules have due now.
    pub(super) fn generate_entities(&mut self) -> Result<()> {
        self.spawn_requested()?;
        for (group_index, count) in self.due_now() {
            // Shared, so the definition can be read while the batch changes
            // the rest of the simulation.
            let definition = Arc::clone(&self.definitions[group_index]);
            self.spawn_batch(group_index, &definition, count)?;
        }
        Ok(())
    }

    /// How many each schedule builds this tick, in group order, and when
    /// its next batches come. A batch spawns on the tick nearest its time,
    /// so rounding (t + 1/rate landing just after a tick) cannot delay it a
    /// tick.
    fn due_now(&mut self) -> Vec<(usize, usize)> {
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

    /// Builds what plugins requested since the last tick, in delivery order:
    /// entity and plugin order, the same at any worker count.
    fn spawn_requested(&mut self) -> Result<()> {
        let requests = self
            .messages
            .receive::<SpawnRequest>("GlobalNetwork", SpawnRequest::TOPIC)?;
        for message in requests {
            let request = &message.value;
            self.requested_definition(request)
                .and_then(|(group_index, definition)| {
                    self.spawn_batch(group_index, &definition, request.count)
                })
                .with_context(|| {
                    format!(
                        "spawn request from {} for recipe `{}`",
                        describe(&message.sender),
                        request.recipe
                    )
                })?;
        }
        Ok(())
    }

    /// The request's recipe with the request's values laid over it, as a
    /// definition ready to build from, and its group index (the recipes are
    /// numbered after the entities).
    fn requested_definition(&self, request: &SpawnRequest) -> Result<(usize, EntityDefinition)> {
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

    /// Builds `count` entities of one group, numbered on from the group's
    /// count so far. Each start is sampled just before its entity is built,
    /// because the collision check sees the entities built earlier.
    fn spawn_batch(
        &mut self,
        group_index: usize,
        definition: &EntityDefinition,
        count: usize,
    ) -> Result<()> {
        for _ in 0..count {
            let number = self.generators[group_index].spawned;
            self.generators[group_index].spawned += 1;
            let (position_world_m, heading_world_deg) =
                self.sample_spawn_start(group_index, number, &definition.spawn)?;
            self.spawn(group_index, definition, position_world_m, heading_world_deg)?;
        }
        Ok(())
    }

    /// Chooses a start: position and heading sampled around the spawn's.
    /// `number` is the spawn's number in its group; each try at a
    /// collision-free position has its own numbers.
    fn sample_spawn_start(
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

    /// Builds one entity of `group_index`'s group from `definition` at a
    /// chosen start: assigns its ID, adds it, and records and publishes
    /// `EntityGenerated`. No randomness is drawn here.
    fn spawn(
        &mut self,
        group_index: usize,
        definition: &EntityDefinition,
        position_world_m: Vec3,
        heading_world_deg: f64,
    ) -> Result<()> {
        let mut id = definition.spawn.requested_id.unwrap_or(0);
        while !self.used_ids.insert(id) {
            id = self
                .used_ids
                .last()
                .copied()
                .unwrap_or(0)
                .checked_add(1)
                .context("entity ID exhausted")?;
        }

        let entity = Entity::new(
            id,
            i32::try_from(group_index).context("entity block ID exceeds i32")?,
            definition,
            position_world_m,
            heading_world_deg,
            StepTime {
                time_s: self.time_s,
                dt_s: 0.0,
                tick: self.tick,
            },
            self.config.run.seed,
            self.terrain.as_ref(),
        )?;
        self.entity_teams.insert(id, entity.team_id);
        let event = Event {
            time_s: self.time_s,
            kind: EventKind::EntityGenerated,
            entity_ids: vec![id],
        };
        self.messages.time_s = self.time_s;
        self.messages
            .publish_engine_event("GlobalNetwork", event.kind.topic(), event.clone());
        self.events.push(event);
        self.entities.push(entity);
        Ok(())
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

fn describe(sender: &MessageEndpoint) -> String {
    match sender.entity_id {
        Some(id) => format!("{} of entity {id}", sender.plugin),
        None => sender.plugin.clone(),
    }
}

impl Generator {
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

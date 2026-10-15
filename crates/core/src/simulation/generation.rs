//! Mission-controlled generation schedules.

use super::{Event, EventKind, Simulation};
use crate::plugin::StepTime;
use crate::scenario::EntityGroupConfig;
use crate::{common::random, entity::Entity, math::Vec3};
use anyhow::{Context, Result, ensure};

#[derive(Clone)]
pub(crate) struct Generator {
    pub(crate) definition_index: usize,
    pub(crate) remaining: usize,
    pub(crate) next_spawn_times_s: Vec<f64>,
    pub(crate) rate_hz: f64,
    pub(crate) time_stddev_s: f64,
    /// Spawns scheduled so far; the next one's number in its group.
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

/// All creation goes through `spawn`: callers supply a sampled or fixed start.
/// Currently only schedules produce requests. Velocity and roll/pitch still
/// come from the group definition; extend the starting state with the first
/// connection pilot.
struct SpawnRequest {
    definition_index: usize,
    position_world_m: Vec3,
    heading_world_deg: f64,
}

impl Simulation {
    pub(super) fn generate_entities(&mut self) -> Result<()> {
        // Requests: which groups are due to spawn this tick. A batch spawns on
        // the tick nearest its time, so rounding (t + 1/rate landing just
        // after a tick) cannot delay it a tick.
        let mut due_definitions = Vec::new();
        let half_tick_s = self.config.run.dt_s / 2.0;
        for generator in &mut self.generators {
            for next_spawn_s in &mut generator.next_spawn_times_s {
                if self.time_s < *next_spawn_s - half_tick_s || generator.remaining == 0 {
                    continue;
                }
                generator.remaining -= 1;
                let number = generator.spawned;
                generator.spawned += 1;
                due_definitions.push((generator.definition_index, number));

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
        }

        // Each start is sampled just before its entity is built, because the
        // collision check sees the entities spawned earlier in this tick.
        for (definition_index, number) in due_definitions {
            let (position_world_m, heading_world_deg) =
                self.sample_spawn_start(definition_index, number)?;
            self.spawn(SpawnRequest {
                definition_index,
                position_world_m,
                heading_world_deg,
            })?;
        }
        Ok(())
    }

    /// Chooses a scheduled start: position and heading sampled around the
    /// spawn's. `number` is the spawn's number in its group; each try at a
    /// collision-free position has its own numbers.
    fn sample_spawn_start(&self, definition_index: usize, number: u64) -> Result<(Vec3, f64)> {
        let seed = self.config.run.seed;
        let draw = |field, attempt, mean, stddev| {
            let label = spawn_label(seed, definition_index, number, field, attempt);
            random::normal(label, mean, stddev)
        };
        let spawn = &self.definitions[definition_index].spawn;
        let origin_world_m = spawn.position_world_m;
        let heading_world_deg = draw(
            SpawnField::Heading,
            0,
            spawn.heading_world_deg,
            spawn.heading_variance_deg2.sqrt(),
        );
        let mut position_world_m = origin_world_m;

        if self.startup_collision_exists(position_world_m) || spawn.randomize_every_spawn {
            let stddev_world_m = spawn.position_variance_world_m2.map(f64::sqrt);
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

    /// Builds one entity from a request: assigns its ID, adds it, and records
    /// and publishes `EntityGenerated`. No randomness is drawn here.
    fn spawn(&mut self, request: SpawnRequest) -> Result<()> {
        let definition = &self.definitions[request.definition_index];
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
            i32::try_from(request.definition_index).context("entity block ID exceeds i32")?,
            definition,
            request.position_world_m,
            request.heading_world_deg,
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

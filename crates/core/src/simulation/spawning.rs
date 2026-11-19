//! The entities a tick starts with: the ones plugins requested since the
//! last tick, then the batches the mission's schedules have due. Each is
//! built from its definition at a sampled start, given an ID no entity has
//! had, started by the tick's next call, and announced on the global network.
//! The schedules, the request types, and the random draws are in
//! generation.rs; this file is what is proved about building.
use super::generation::{SpawnRequest, describe};
use super::{Event, EventKind, Simulation};
use crate::entity::{Entity, EntityDefinition};
use crate::math::Vec3;
use crate::plugin::StepTime;
use crate::pubsub::messages::Message;
use anyhow::{Context, Result};
use std::sync::Arc;
use vstd::prelude::*;

pub(crate) mod proof;

verus! {

broadcast use proof::spawning_facts;

impl Simulation {
    /// The start of a tick: the entities plugins asked for since the last
    /// tick, then the batches the mission's schedules have due now.
    pub(super) fn generate_entities(&mut self) -> (r: Result<()>)
        ensures proof::generated(old(self), final(self), r is Ok)
    {
        self.spawn_requested()?;
        for (group_index, count) in it: self.due_now()
            invariant proof::due_so_far(old(self), self, it)
        {
            // Shared, so the definition can be read while the batch changes
            // the rest of the simulation.
            let definition = Arc::clone(&self.definitions[group_index]);
            self.spawn_batch(group_index, &definition, count)?;
        }
        Ok(())
    }

    /// Builds what plugins requested since the last tick, in delivery order:
    /// entity and plugin order, the same at any worker count.
    fn spawn_requested(&mut self) -> (r: Result<()>)
        ensures proof::generated(old(self), final(self), r is Ok)
    {
        let requests = self
            .messages
            .receive::<SpawnRequest>("GlobalNetwork", SpawnRequest::TOPIC)?;
        for message in it: requests
            invariant proof::growing(old(self), self)
        {
            let request = &message.value;
            let (group_index, definition) = self
                .requested_definition(request)
                .map_err(|error| error.context(request_context(&message)))?;
            self.spawn_batch(group_index, &definition, request.count)
                .map_err(|error| error.context(request_context(&message)))?;
        }
        Ok(())
    }

    /// Builds `count` entities of one group, numbered on from the group's
    /// count so far. Each start is sampled just before its entity is built,
    /// because the collision check sees the entities built earlier.
    fn spawn_batch(
        &mut self,
        group_index: usize,
        definition: &EntityDefinition,
        count: usize,
    ) -> (r: Result<()>)
        requires group_index < old(self).generators@.len()
        ensures proof::batch_spawned(old(self), final(self), count, r is Ok)
    {
        for _ in it: 0..count
            invariant proof::batch_so_far(old(self), self, it, group_index)
        {
            let number = self.generators[group_index].next_number();
            let (position_world_m, heading_world_deg) =
                self.sample_spawn_start(group_index, number, &definition.spawn)?;
            self.spawn(group_index, definition, position_world_m, heading_world_deg)?;
        }
        Ok(())
    }

    /// Builds one entity of `group_index`'s group from `definition` at a
    /// chosen start: assigns its ID, adds it, and records and publishes
    /// `EntityGenerated`. No randomness is drawn here.
    // The ID search ends after at most two tries, since the set's largest ID
    // plus one is free; Verus cannot see that from the set's contract.
    #[verifier::exec_allows_no_decreases_clause]
    fn spawn(
        &mut self,
        group_index: usize,
        definition: &EntityDefinition,
        position_world_m: Vec3,
        heading_world_deg: f64,
    ) -> (r: Result<()>)
        ensures proof::spawned(old(self), final(self), group_index, definition, r is Ok)
    {
        let mut id = definition.spawn.requested_id.unwrap_or(0);
        while !self.used_ids.insert(id)
            invariant proof::choosing_id(old(self), self)
        {
            // `with_context` rather than `context`: Verus has a contract for the former.
            id = self
                .used_ids
                .last()
                .copied()
                .unwrap_or(0)
                .checked_add(1)
                .with_context(|| "entity ID exhausted")?;
        }

        let entity = Entity::new(
            id,
            i32::try_from(group_index).map_err(|_| anyhow::anyhow!("entity block ID exceeds i32"))?,
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

/// The error context of a failed request: who sent it and for what recipe.
#[verifier::external_body]
fn request_context(message: &Message<SpawnRequest>) -> String {
    format!(
        "spawn request from {} for recipe `{}`",
        describe(&message.sender),
        message.value.recipe
    )
}

} // verus!

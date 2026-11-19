//! What the tick claims, in words a reader of simulation.rs can check: the
//! phase loop, the substep loop, the interactions phase, the removal of dead
//! entities, and the step that runs the phases in order.
#[cfg(verus_only)]
pub(crate) use super::spawning::proof::generated;
use super::trace::{Trace, TraceSink};
#[cfg(verus_only)]
use super::world_plugins::proof as world;
#[cfg(verus_only)]
use super::{Event, EventKind, Simulation, SimulationFrame, TerminationReason};
use crate::common::SpatialIndex;
#[cfg(verus_only)]
use crate::common::{Terrain, TruthContacts};
use crate::drawing::PluginDrawings;
#[cfg(verus_only)]
use crate::entity::plugin_stack::proof as stack;
#[cfg(verus_only)]
use crate::entity::proof::{
    closed, deactivated, forces_cleared, is_active, is_alive, phase_stepped,
    started as entity_started,
};
#[cfg(verus_only)]
use crate::entity::{Entity, EntitySnapshot, Phase};
use crate::plugin::PluginRegistry;
#[cfg(verus_only)]
use crate::plugin::estimator::PerfectContacts;
#[cfg(verus_only)]
use crate::plugin::{Messages, StepTime};
#[cfg(verus_only)]
use crate::pubsub::messages::Publication;
#[cfg(verus_only)]
use crate::pubsub::messages::proof as mailbox;
use crate::scenario::{EndConditions, EntityGroupConfig, PluginConfig};
#[cfg(verus_only)]
use crate::scenario::{RunConfig, SpatialLookup};
use std::path::PathBuf;
use vstd::prelude::*;

verus! {

// Declarations of what the tick carries but never reads: each is a value
// with no visible structure, so nothing about it is assumed.
#[allow(dead_code)]
#[verifier::external_type_specification]
#[verifier::external_body]
pub struct ExPathBuf(PathBuf);
#[allow(dead_code)]
#[verifier::external_type_specification]
#[verifier::external_body]
pub struct ExEntityGroupConfig(EntityGroupConfig);
#[allow(dead_code)]
#[verifier::external_type_specification]
#[verifier::external_body]
pub struct ExPluginConfig(PluginConfig);
#[allow(dead_code)]
#[verifier::external_type_specification]
#[verifier::external_body]
pub struct ExEndConditions(EndConditions);
#[allow(dead_code)]
#[verifier::external_type_specification]
#[verifier::external_body]
pub struct ExPluginRegistry(PluginRegistry);
#[allow(dead_code)]
#[verifier::external_type_specification]
#[verifier::external_body]
pub struct ExSpatialIndex(SpatialIndex);
#[allow(dead_code)]
#[verifier::external_type_specification]
#[verifier::external_body]
pub struct ExPluginDrawings(PluginDrawings);
#[allow(dead_code)]
#[verifier::external_type_specification]
#[verifier::external_body]
pub(crate) struct ExTrace<'a>(Trace<'a>);

/// Names the trace sink trait so `dyn TraceSink` is a type Verus knows; no
/// method is listed.
#[allow(dead_code)]
#[verifier::external_trait_specification]
pub trait ExTraceSink {
    type ExternalTraitSpecificationFor: TraceSink;
}

/// The facts the tick's proofs need, so simulation.rs names one group.
#[allow(dead_code)]
pub(crate) broadcast group simulation_facts {
    started_up_by,
    autonomy_done_by,
    moved_by,
    sensed_up_by,
    interacted_up_by,
    routed_up_by,
    estimated_up_by,
    scored_up_by,
    axioms::count_cast_is_ieee,
    axioms::step_div_is_ieee,
    axioms::elapsed_mul_is_ieee,
    axioms::start_add_is_ieee,
    stack::axioms::end_add_is_ieee,
}

/// An active entity ran the phase, Ok or not; an inactive one is untouched.
pub open(crate) spec fn visited(before: &Entity, after: &Entity, phase: Phase) -> bool {
    if is_active(before) {
        phase_stepped(before, after, phase, true) || phase_stepped(before, after, phase, false)
    } else {
        *after == *before
    }
}

/// The loop walking the entities, as Verus sees it: the entities it will
/// visit and how many it has.
#[cfg(verus_only)]
pub type Walk<'a> = vstd::std_specs::iter::VerusForLoopWrapper<core::slice::IterMut<'a, Entity>>;

/// Partway through the loop: the entities visited so far ran the phase or
/// were skipped as inactive, the rest are as they were, and no error kept
/// means every active entity so far returned Ok while an error kept means
/// one of them returned Err.
#[verifier::prophetic]
pub open(crate) spec fn phase_so_far(before: &Vec<Entity>, it: Walk, phase: Phase, first_error: &Option<anyhow::Error>) -> bool {
    let entities = it.seq();
    let count = it.index();
    &&& entities.len() == before@.len()
    &&& forall|j: int| #![trigger entities[j]] 0 <= j < count ==> visited(&before@[j], &*final(entities[j]), phase)
    &&& forall|j: int| #![trigger entities[j]] count <= j < before@.len() ==> *entities[j] == before@[j]
    &&& first_error is None ==> forall|j: int| #![trigger entities[j]] 0 <= j < count && is_active(&before@[j]) ==> phase_stepped(&before@[j], &*final(entities[j]), phase, true)
    &&& first_error is Some ==> exists|j: int| #![trigger entities[j]] 0 <= j < count && is_active(&before@[j]) && phase_stepped(&before@[j], &*final(entities[j]), phase, false)
}

/// `run_phase` visited every entity, in order: each active one ran the phase
/// once and each inactive one is untouched. It returned Ok exactly when every
/// active entity did; when one returned Err, so did the phase, after every
/// entity had run.
pub open(crate) spec fn phase_ran(before: &Vec<Entity>, after: &Vec<Entity>, phase: Phase, ok: bool) -> bool {
    let before = before@;
    let after = after@;
    &&& after.len() == before.len()
    &&& forall|j: int| #![trigger after[j]] 0 <= j < before.len() ==> visited(&before[j], &after[j], phase)
    &&& ok ==> forall|j: int| #![trigger after[j]] 0 <= j < before.len() && is_active(&before[j]) ==> phase_stepped(&before[j], &after[j], phase, true)
    &&& !ok ==> exists|j: int| #![trigger after[j]] 0 <= j < before.len() && is_active(&before[j]) && phase_stepped(&before[j], &after[j], phase, false)
}

/// An entity with no health left was closed, Ok or not, and kept its health;
/// one with health left is untouched.
pub open(crate) spec fn closed_dead(before: &Entity, after: &Entity, time: StepTime) -> bool {
    if is_alive(before) {
        *after == *before
    } else {
        &&& closed(before, after, time, true) || closed(before, after, time, false)
        &&& after.health == before.health
    }
}

/// Partway through `close_dead`: the entities visited so far were closed if
/// dead and left alone if alive, the rest are as they were, and no error kept
/// means every dead entity so far closed Ok while an error kept means one of
/// them did not.
#[verifier::prophetic]
pub open(crate) spec fn closed_so_far(before: &Vec<Entity>, it: Walk, time: StepTime, first_error: &Option<anyhow::Error>) -> bool {
    let entities = it.seq();
    let count = it.index();
    &&& entities.len() == before@.len()
    &&& forall|j: int| #![trigger entities[j]] 0 <= j < count ==> closed_dead(&before@[j], &*final(entities[j]), time)
    &&& forall|j: int| #![trigger entities[j]] count <= j < before@.len() ==> *entities[j] == before@[j]
    &&& first_error is None ==> forall|j: int| #![trigger entities[j]] 0 <= j < count && !is_alive(&before@[j]) ==> closed(&before@[j], &*final(entities[j]), time, true)
    &&& first_error is Some ==> exists|j: int| #![trigger entities[j]] 0 <= j < count && !is_alive(&before@[j]) && closed(&before@[j], &*final(entities[j]), time, false)
}

/// `close_dead` closed every entity with no health left, in order, and touched
/// no other. It returned Ok exactly when every close did; when one failed, so
/// did `close_dead`, after every entity had run.
pub open(crate) spec fn dead_closed(before: &Vec<Entity>, after: &Vec<Entity>, time: StepTime, ok: bool) -> bool {
    let before = before@;
    let after = after@;
    &&& after.len() == before.len()
    &&& forall|j: int| #![trigger after[j]] 0 <= j < before.len() ==> closed_dead(&before[j], &after[j], time)
    &&& ok ==> forall|j: int| #![trigger after[j]] 0 <= j < before.len() && !is_alive(&before[j]) ==> closed(&before[j], &after[j], time, true)
    &&& !ok ==> exists|j: int| #![trigger after[j]] 0 <= j < before.len() && !is_alive(&before[j]) && closed(&before[j], &after[j], time, false)
}

/// The entities `Entity::is_alive` keeps, in order.
pub open(crate) spec fn survivors(entities: Seq<Entity>) -> Seq<Entity> {
    entities.filter(|entity: Entity| call_ensures(Entity::is_alive, (&entity,), true))
}

/// `remove_dead` closed every entity with no health left and dropped it; the
/// entities with health left remain, in order, untouched. Ok exactly when every
/// close was.
pub open(crate) spec fn dead_removed(before: &Vec<Entity>, after: &Vec<Entity>, time: StepTime, ok: bool) -> bool {
    exists|closed: Vec<Entity>| #[trigger] dead_closed(before, &closed, time, ok) && after@ == survivors(closed@)
}

/// The step time of the tick being run: the simulation's time and tick, with
/// the mission's step.
pub open(crate) spec fn step_time_of(sim: &Simulation) -> StepTime {
    StepTime { time_s: sim.time_s, dt_s: sim.config.run.dt_s, tick: sim.tick }
}

/// The mission's terrain as the motion phase is handed it.
pub open(crate) spec fn terrain_of(sim: &Simulation) -> Option<&Terrain> {
    match sim.terrain {
        Some(ref terrain) => Some(terrain),
        None => None,
    }
}

/// The first substep of a tick at `time`: the tick's time, with the step
/// divided by the mission's `count` substeps.
pub open(crate) spec fn first_substep(time: StepTime, count: usize) -> StepTime {
    StepTime { dt_s: time.dt_s / (count as f64), ..time }
}

/// Substep `k` of the tick: the first substep moved on by its own length `k`
/// times, one IEEE addition each, as the loop accumulates it.
pub open(crate) spec fn substep(time: StepTime, count: usize, k: nat) -> StepTime
    decreases k
{
    if k == 0 {
        first_substep(time, count)
    } else {
        let previous = substep(time, count, (k - 1) as nat);
        StepTime { time_s: previous.time_s + previous.dt_s, ..previous }
    }
}

/// The controllers phase of substep `k`: at its time, with belief following
/// truth on every substep but the first.
pub open(crate) spec fn controllers_of(time: StepTime, count: usize, k: nat) -> Phase<'static> {
    Phase::Controllers { time: substep(time, count, k), after_motion: k > 0 }
}

/// The motion phase of substep `k`: at its time, over the mission's terrain.
pub open(crate) spec fn motion_of<'a>(time: StepTime, count: usize, k: nat, terrain: Option<&'a Terrain>) -> Phase<'a> {
    Phase::Motion { time: substep(time, count, k), terrain }
}

/// `k` substeps ran, from `before` to `after`: after `k - 1` of them the
/// entities were `start`, then substep `k - 1` ran every entity's controllers
/// and then every entity's motion, each phase Ok.
pub open(crate) spec fn substeps_ran(before: &Vec<Entity>, after: &Vec<Entity>, terrain: Option<&Terrain>, time: StepTime, count: usize, k: nat) -> bool
    decreases k
{
    if k == 0 {
        *after == *before
    } else {
        let last = (k - 1) as nat;
        exists|start: Vec<Entity>, controlled: Vec<Entity>|
            substeps_ran(before, &start, terrain, time, count, last)
            && #[trigger] phase_ran(&start, &controlled, controllers_of(time, count, last), true)
            && phase_ran(&controlled, after, motion_of(time, count, last, terrain), true)
    }
}

/// Every one of the mission's `count` substeps ran when Ok; otherwise one
/// substep's controllers or motion phase failed, after the substeps before it
/// ran.
pub open(crate) spec fn entities_substepped(before: &Vec<Entity>, after: &Vec<Entity>, terrain: Option<&Terrain>, time: StepTime, count: usize, ok: bool) -> bool {
    if ok {
        substeps_ran(before, after, terrain, time, count, count as nat)
    } else {
        exists|k: nat, start: Vec<Entity>| k < count
            && #[trigger] substeps_ran(before, &start, terrain, time, count, k)
            && {
                ||| phase_ran(&start, after, controllers_of(time, count, k), false)
                ||| exists|controlled: Vec<Entity>|
                    #[trigger] phase_ran(&start, &controlled, controllers_of(time, count, k), true)
                    && phase_ran(&controlled, after, motion_of(time, count, k, terrain), false)
            }
    }
}

/// `controllers_and_motion` ran the mission's substeps over the entities and
/// changed nothing else of the simulation.
pub open(crate) spec fn substepped(before: &Simulation, after: &Simulation, time: StepTime, ok: bool) -> bool {
    &&& entities_substepped(&before.entities, &after.entities, terrain_of(before), time, before.config.run.motion_multiplier, ok)
    &&& *after == Simulation { entities: after.entities, ..*before }
}

/// The substep loop as Verus sees it: how many substeps it has run.
#[cfg(verus_only)]
pub type Substeps = vstd::std_specs::iter::VerusForLoopWrapper<core::ops::Range<usize>>;

/// Partway through the substeps: the ones so far ran, `next` is the time of
/// the next, and nothing else of the simulation changed.
#[verifier::prophetic]
pub open(crate) spec fn substeps_so_far(before: &Simulation, sim: &Simulation, it: Substeps, time: StepTime, next: StepTime) -> bool {
    let done = it.index() as nat;
    let count = before.config.run.motion_multiplier;
    &&& next == substep(time, count, done)
    &&& substeps_ran(&before.entities, &sim.entities, terrain_of(before), time, count, done)
    &&& *sim == Simulation { entities: sim.entities, ..*before }
}

/// What no phase changes: the step count, the time, and whether the run is
/// finished. Only `advance_time` and `finish` touch them.
pub open(crate) spec fn same_clock(before: &Simulation, after: &Simulation) -> bool {
    &&& after.step_count == before.step_count
    &&& after.time_s == before.time_s
    &&& after.finished == before.finished
}

/// Partway through `start_each`: the entities so far were started from the
/// perfect detections, each Ok, and the rest are as they were.
#[verifier::prophetic]
pub open(crate) spec fn started_so_far(before: &Vec<Entity>, it: Walk, perfect_contacts: &PerfectContacts) -> bool {
    let entities = it.seq();
    let count = it.index();
    &&& entities.len() == before@.len()
    &&& forall|j: int| #![trigger entities[j]] 0 <= j < count ==> entity_started(&before@[j], &*final(entities[j]), perfect_contacts, true)
    &&& forall|j: int| #![trigger entities[j]] count <= j < before@.len() ==> *entities[j] == before@[j]
}
/// `start_each`: every entity was started once from the perfect detections, in
/// order, or some entity failed to start after the ones before it did, and
/// the loop stopped there, so nothing is said of the ones after it.
pub open(crate) spec fn all_started(before: &Vec<Entity>, after: &Vec<Entity>, perfect_contacts: &PerfectContacts, ok: bool) -> bool {
    let before = before@;
    let after = after@;
    &&& after.len() == before.len()
    &&& ok ==> forall|j: int| #![trigger after[j]] 0 <= j < before.len() ==> entity_started(&before[j], &after[j], perfect_contacts, true)
    &&& !ok ==> exists|k: int| #![trigger after[k]] 0 <= k < before.len()
        && entity_started(&before[k], &after[k], perfect_contacts, false)
        && (forall|j: int| #![trigger after[j]] 0 <= j < k ==> entity_started(&before[j], &after[j], perfect_contacts, true))
}
/// `start_entities`: every entity not yet started was started from the latest
/// perfect detections; nothing else of the simulation changed.
pub open(crate) spec fn started(before: &Simulation, after: &Simulation, ok: bool) -> bool {
    &&& all_started(&before.entities, &after.entities, &before.perfect_contacts, ok)
    &&& *after == Simulation { entities: after.entities, ..*before }
}
/// `sense`: the sensors phase ran over every entity with the tick's time and
/// the truth contacts built from the snapshot of the entities as motion left
/// them and the spatial index built from them; nothing else of the
/// simulation changed. The index returned is that one; the claim does not
/// say so, since the interactions refresh it before anything reads it.
pub open(crate) spec fn sensed(before: &Simulation, after: &Simulation, time: StepTime, ok: bool) -> bool {
    let truth = axioms::truth_contacts(axioms::snapshot(before).entities@, index_of(before));
    &&& phase_ran(&before.entities, &after.entities, Phase::Sensors { time, truth }, ok)
    &&& *after == Simulation { entities: after.entities, ..*before }
}
/// The spatial index `sense` builds: the mission's lookup over the entities.
pub open(crate) spec fn index_of(sim: &Simulation) -> SpatialIndex {
    axioms::spatial_index(sim.config.spatial_lookup, sim.entities@)
}
/// `apply_interactions`: every entity's applied forces were cleared, the
/// world's interactions ran over them with the tick's time and terrain and
/// the stop flag folded into the run's, then every entity they left active
/// with no health was taken out of play, recorded, and announced at the
/// simulation's time; nothing else of the simulation changed. A failed
/// interaction stops the phase with the entities and events as they were
/// left.
pub open(crate) spec fn interacted(before: &Simulation, after: &Simulation, ok: bool) -> bool {
    exists|cleared: Seq<Entity>, interacted: Seq<Entity>, events: Seq<Event>, r: anyhow::Result<bool>|
        #![trigger all_forces_cleared(before.entities@, cleared), world::interacted(&before.world, &after.world, step_time_of(before), terrain_of(before), cleared, interacted, before.events@, events, &r)]
    {
        &&& all_forces_cleared(before.entities@, cleared)
        &&& world::interacted(&before.world, &after.world, step_time_of(before), terrain_of(before), cleared, interacted, before.events@, events, &r)
        &&& ok == r is Ok
        &&& r is Ok ==> {
            &&& dead_deactivated(interacted, after.entities@, before.time_s, events, after.events@, &Messages { time_s: before.time_s, ..before.messages }, &after.messages)
            &&& after.world_stop == (before.world_stop || r->Ok_0)
            &&& *after == Simulation { entities: after.entities, events: after.events, world: after.world, messages: after.messages, world_stop: after.world_stop, ..*before }
        }
        &&& r is Err ==> {
            &&& after.entities@ == interacted
            &&& after.events@ == events
            &&& *after == Simulation { entities: after.entities, events: after.events, world: after.world, ..*before }
        }
    }
}

/// The topic an engine event is published on: its kind's name.
pub open(crate) spec fn topic_of(kind: EventKind) -> Seq<char> {
    match kind {
        EventKind::EntityGenerated => "EntityGenerated"@,
        EventKind::EntityRemoved => "EntityRemoved"@,
        EventKind::EntityPresentAtEnd => "EntityPresentAtEnd"@,
        EventKind::TeamCollision => "TeamCollision"@,
        EventKind::NonTeamCollision => "NonTeamCollision"@,
        EventKind::GroundCollision => "GroundCollision"@,
    }
}

/// Two events with the same time, kind, and entity ids.
pub open(crate) spec fn same_event(a: &Event, b: &Event) -> bool {
    a.time_s == b.time_s && a.kind == b.kind && a.entity_ids@ == b.entity_ids@
}

/// Partway through `clear_applied_forces`: the entities visited so far have
/// no applied forces and the rest are as they were.
#[verifier::prophetic]
pub open(crate) spec fn forces_cleared_so_far(before: &Vec<Entity>, it: Walk) -> bool {
    let entities = it.seq();
    let count = it.index();
    &&& entities.len() == before@.len()
    &&& forall|j: int| #![trigger entities[j]] 0 <= j < count ==> forces_cleared(&before@[j], &*final(entities[j]))
    &&& forall|j: int| #![trigger entities[j]] count <= j < before@.len() ==> *entities[j] == before@[j]
}

/// Every entity's applied forces are gone and nothing else of any changed.
pub open(crate) spec fn all_forces_cleared(before: Seq<Entity>, after: Seq<Entity>) -> bool {
    &&& after.len() == before.len()
    &&& forall|j: int| #![trigger after[j]] 0 <= j < before.len() ==> forces_cleared(&before[j], &after[j])
}

/// An entity the interactions phase takes out of play: active, with no health left.
pub open(crate) spec fn leaving(entity: &Entity) -> bool {
    !is_alive(entity) && is_active(entity)
}

/// The ids of the entities leaving play among the first `k`, in entity order.
pub open(crate) spec fn leaving_ids(entities: Seq<Entity>, k: nat) -> Seq<i32>
    decreases k
{
    if k == 0 {
        Seq::empty()
    } else {
        let rest = leaving_ids(entities, (k - 1) as nat);
        if leaving(&entities[k - 1]) { rest.push(entities[k - 1].id) } else { rest }
    }
}

/// The event records the removal of entity `id` at `time_s`.
pub open(crate) spec fn is_removal(event: &Event, id: i32, time_s: f64) -> bool {
    event.time_s == time_s && event.kind == EventKind::EntityRemoved && event.entity_ids@ =~= seq![id]
}

/// The events gained one removal per id, in order, after the ones before.
pub open(crate) spec fn recorded(before: Seq<Event>, after: Seq<Event>, ids: Seq<i32>, time_s: f64) -> bool {
    let n = before.len() as int;
    &&& after.len() == n + ids.len()
    &&& after.take(n) =~= before
    &&& forall|j: int| 0 <= j < ids.len() ==> is_removal(&#[trigger] after[n + j], ids[j], time_s)
}

/// The publication, sent at `sent_at_s`, carries a removal of entity `id` at
/// `time_s` on the global network's removal topic.
pub open(crate) spec fn removal_announced(publication: &Publication, id: i32, time_s: f64, sent_at_s: f64) -> bool {
    &&& publication.channel.0@ == "GlobalNetwork"@
    &&& publication.channel.1@ == topic_of(EventKind::EntityRemoved)
    &&& publication.sent_at_s == sent_at_s
    &&& exists|event: Event| is_removal(&event, id, time_s) && #[trigger] mailbox::carries::<Event>(publication.payload, event)
}

/// The mailbox published one removal per id, in order, after the ones before,
/// each sent at the mailbox's time, and changed nothing else.
pub open(crate) spec fn announced(before: &Messages, after: &Messages, ids: Seq<i32>, time_s: f64) -> bool {
    let n = before.outbox@.len() as int;
    &&& after.outbox@.len() == n + ids.len()
    &&& after.outbox@.take(n) =~= before.outbox@
    &&& forall|j: int| 0 <= j < ids.len() ==> removal_announced(&#[trigger] after.outbox@[n + j], ids[j], time_s, before.time_s)
    &&& *after == Messages { outbox: after.outbox, ..*before }
}

/// An entity leaving play is inactive and otherwise as it was; any other is untouched.
pub open(crate) spec fn left_play(before: &Entity, after: &Entity) -> bool {
    if leaving(before) { deactivated(before, after) } else { *after == *before }
}

/// Partway through `deactivate_dead`: the entities visited so far have left
/// play if they were to, the rest are as they were, and the events and the
/// mailbox hold one removal for each that left.
#[verifier::prophetic]
pub open(crate) spec fn deactivated_so_far(before: &Vec<Entity>, it: Walk, time_s: f64, events_before: Seq<Event>, events: &Vec<Event>, messages_before: &Messages, messages: &Messages) -> bool {
    let entities = it.seq();
    let count = it.index();
    &&& entities.len() == before@.len()
    &&& forall|j: int| #![trigger entities[j]] 0 <= j < count ==> left_play(&before@[j], &*final(entities[j]))
    &&& forall|j: int| #![trigger entities[j]] count <= j < before@.len() ==> *entities[j] == before@[j]
    &&& recorded(events_before, events@, leaving_ids(before@, count as nat), time_s)
    &&& announced(messages_before, messages, leaving_ids(before@, count as nat), time_s)
}

/// `deactivate_dead` took every active entity with no health left out of
/// play, in entity order, recorded a removal event for each at `time_s`, and
/// announced each on the global network; nothing else changed.
pub open(crate) spec fn dead_deactivated(before: Seq<Entity>, after: Seq<Entity>, time_s: f64, events_before: Seq<Event>, events_after: Seq<Event>, messages_before: &Messages, messages_after: &Messages) -> bool {
    &&& after.len() == before.len()
    &&& forall|j: int| #![trigger after[j]] 0 <= j < before.len() ==> left_play(&before[j], &after[j])
    &&& recorded(events_before, events_after, leaving_ids(before, before.len()), time_s)
    &&& announced(messages_before, messages_after, leaving_ids(before, before.len()), time_s)
}
pub open(crate) spec fn routed(before: &Simulation, after: &Simulation, time: StepTime, contacts: Seq<EntitySnapshot>, ok: bool) -> bool {
    axioms::route_messages(before, after, time, contacts, ok) && same_clock(before, after)
}
/// `estimate`: the perfect detections were taken from the contacts, every
/// entity still in play, then the estimation phase ran over every entity with
/// the tick's time and those detections;
/// nothing else of the simulation changed.
pub open(crate) spec fn estimated(before: &Simulation, after: &Simulation, time: StepTime, contacts: Seq<EntitySnapshot>, ok: bool) -> bool {
    let perfect_contacts = axioms::perfect_measurement(contacts, time.time_s + time.dt_s);
    &&& phase_ran(&before.entities, &after.entities, Phase::Estimate { time, perfect_contacts: &perfect_contacts }, ok)
    &&& *after == Simulation { entities: after.entities, perfect_contacts, ..*before }
}
/// `score`: the world's metrics plugins ran over the truth contacts built
/// from the snapshot and the index, the run's teams, and the terrain, and
/// their stop request was folded into the run's; nothing else of the
/// simulation changed; a failed plugin stops the phase with the flag as it was.
pub open(crate) spec fn scored(before: &Simulation, after: &Simulation, time: StepTime, contacts: Seq<EntitySnapshot>, ok: bool) -> bool {
    exists|nearby: SpatialIndex, r: anyhow::Result<bool>|
        #![trigger world::measured(&before.world, &after.world, time, axioms::truth_contacts(contacts, nearby), &before.entity_teams, terrain_of(before), &r)]
    {
        &&& world::measured(&before.world, &after.world, time, axioms::truth_contacts(contacts, nearby), &before.entity_teams, terrain_of(before), &r)
        &&& ok == r is Ok
        &&& r is Ok ==> after.world_stop == (before.world_stop || r->Ok_0)
            && *after == Simulation { world: after.world, world_stop: after.world_stop, ..*before }
        &&& r is Err ==> *after == Simulation { world: after.world, ..*before }
    }
}
/// `advance_time`: the step count moved on by one, the tick is that count,
/// the time is the run's start plus the count times the step (counted, not
/// accumulated, so it does not drift), and the end condition was read from
/// the state that leaves; nothing else changed.
pub open(crate) spec fn advanced(before: &Simulation, after: &Simulation) -> bool {
    let run = before.config.run;
    &&& after.step_count == before.step_count + 1
    &&& after.tick == after.step_count as i64
    &&& after.time_s == run.start_s + (after.step_count as f64) * run.dt_s
    &&& after.termination == axioms::end_condition(&Simulation { termination: before.termination, ..*after })
    &&& *after == Simulation { step_count: after.step_count, tick: after.tick, time_s: after.time_s, termination: after.termination, ..*before }
}
/// `close_plugins` closed the plugins and left the termination and finished flags as they were.
pub open(crate) spec fn plugins_closed(before: &Simulation, after: &Simulation) -> bool {
    &&& axioms::close_plugins(before, after)
    &&& after.termination == before.termination
    &&& after.finished == before.finished
}

/// One tick: the phases ran once each, in the order of the book's phase
/// table, with the frame and the step time taken once the entities were
/// started, and routing, estimation, and scoring given the contacts
/// snapshotted after the interactions. The entity loops and the substep loop
/// are proved; the rest are witnesses.
pub open(crate) spec fn ticked(before: &Simulation, after: &Simulation, frame: SimulationFrame) -> bool {
    exists|time: StepTime, s1: Simulation, s2: Simulation, s3: Simulation, s4: Simulation, s5: Simulation, s6: Simulation, s7: Simulation, s8: Simulation, s9: Simulation, s10: Simulation|
        #![trigger generated(before, &s1, true), started(&s1, &s2, true), substepped(&s3, &s4, time, true), sensed(&s4, &s5, time, true), interacted(&s5, &s6, true), routed(&s6, &s7, time, axioms::snapshot(&s6).entities@, true), estimated(&s7, &s8, time, axioms::snapshot(&s6).entities@, true), scored(&s8, &s9, time, axioms::snapshot(&s6).entities@, true), advanced(&s10, after)]
    {
        &&& generated(before, &s1, true)
        &&& started(&s1, &s2, true)
        &&& time == step_time_of(&s2)
        &&& frame == SimulationFrame { drawings: axioms::drawings(&s2), ..axioms::snapshot(&s2) }
        &&& phase_ran(&s2.entities, &s3.entities, Phase::Autonomy(time), true)
        &&& s3 == Simulation { entities: s3.entities, ..s2 }
        &&& substepped(&s3, &s4, time, true)
        &&& sensed(&s4, &s5, time, true)
        &&& interacted(&s5, &s6, true)
        &&& routed(&s6, &s7, time, axioms::snapshot(&s6).entities@, true)
        &&& estimated(&s7, &s8, time, axioms::snapshot(&s6).entities@, true)
        &&& scored(&s8, &s9, time, axioms::snapshot(&s6).entities@, true)
        &&& dead_removed(&s9.entities, &s10.entities, time, true)
        &&& s10 == Simulation { entities: s10.entities, ..s9 }
        &&& advanced(&s10, after)
    }
}

/// The tick so far, phase by phase, for the failure case: each predicate is
/// the one before it and one more phase run Ok. The tick's time is read once
/// after the start, as the code reads it, and carried from there with the
/// contacts snapshotted after the interactions.
pub open(crate) spec fn started_up(before: &Simulation, s2: &Simulation) -> bool {
    exists|s1: Simulation| #![trigger generated(before, &s1, true), started(&s1, s2, true)]
        generated(before, &s1, true) && started(&s1, s2, true)
}
pub open(crate) spec fn autonomy_done(before: &Simulation, s3: &Simulation, time: StepTime) -> bool {
    exists|s2: Simulation| #![trigger started_up(before, &s2), phase_ran(&s2.entities, &s3.entities, Phase::Autonomy(time), true)]
        started_up(before, &s2) && time == step_time_of(&s2)
        && phase_ran(&s2.entities, &s3.entities, Phase::Autonomy(time), true)
        && *s3 == Simulation { entities: s3.entities, ..s2 }
}
pub open(crate) spec fn moved(before: &Simulation, s4: &Simulation, time: StepTime) -> bool {
    exists|s3: Simulation| #![trigger autonomy_done(before, &s3, time), substepped(&s3, s4, time, true)]
        autonomy_done(before, &s3, time) && substepped(&s3, s4, time, true)
}
pub open(crate) spec fn sensed_up(before: &Simulation, s5: &Simulation, time: StepTime) -> bool {
    exists|s4: Simulation| #![trigger moved(before, &s4, time), sensed(&s4, s5, time, true)]
        moved(before, &s4, time) && sensed(&s4, s5, time, true)
}
pub open(crate) spec fn interacted_up(before: &Simulation, s6: &Simulation, time: StepTime) -> bool {
    exists|s5: Simulation| #![trigger sensed_up(before, &s5, time), interacted(&s5, s6, true)]
        sensed_up(before, &s5, time) && interacted(&s5, s6, true)
}
pub open(crate) spec fn routed_up(before: &Simulation, s7: &Simulation, time: StepTime, contacts: Seq<EntitySnapshot>) -> bool {
    exists|s6: Simulation| #![trigger interacted_up(before, &s6, time), routed(&s6, s7, time, contacts, true)]
        interacted_up(before, &s6, time) && contacts == axioms::snapshot(&s6).entities@ && routed(&s6, s7, time, contacts, true)
}
pub open(crate) spec fn estimated_up(before: &Simulation, s8: &Simulation, time: StepTime, contacts: Seq<EntitySnapshot>) -> bool {
    exists|s7: Simulation| #![trigger routed_up(before, &s7, time, contacts), estimated(&s7, s8, time, contacts, true)]
        routed_up(before, &s7, time, contacts) && estimated(&s7, s8, time, contacts, true)
}
pub open(crate) spec fn scored_up(before: &Simulation, s9: &Simulation, time: StepTime, contacts: Seq<EntitySnapshot>) -> bool {
    exists|s8: Simulation| #![trigger estimated_up(before, &s8, time, contacts), scored(&s8, s9, time, contacts, true)]
        estimated_up(before, &s8, time, contacts) && scored(&s8, s9, time, contacts, true)
}

/// Each prefix follows from the one before it and the phase's own claim.
/// Stated as lemmas because a prefix is an `exists`, and the prover finds
/// its witness only once the prefix is named for that state.
pub(crate) broadcast proof fn started_up_by(before: &Simulation, s1: Simulation, s2: &Simulation)
    requires #[trigger] generated(before, &s1, true), #[trigger] started(&s1, s2, true)
    ensures started_up(before, s2)
{}
pub(crate) broadcast proof fn autonomy_done_by(before: &Simulation, s2: Simulation, s3: &Simulation, phase: Phase)
    requires
        #[trigger] started_up(before, &s2),
        #[trigger] phase_ran(&s2.entities, &s3.entities, phase, true),
        phase == Phase::Autonomy(step_time_of(&s2)),
        (*s3 == Simulation { entities: s3.entities, ..s2 }),
    ensures autonomy_done(before, s3, step_time_of(&s2))
{}
pub(crate) broadcast proof fn moved_by(before: &Simulation, s3: Simulation, s4: &Simulation, time: StepTime)
    requires #[trigger] autonomy_done(before, &s3, time), #[trigger] substepped(&s3, s4, time, true)
    ensures moved(before, s4, time)
{}
pub(crate) broadcast proof fn sensed_up_by(before: &Simulation, s4: Simulation, s5: &Simulation, time: StepTime)
    requires #[trigger] moved(before, &s4, time), #[trigger] sensed(&s4, s5, time, true)
    ensures sensed_up(before, s5, time)
{}
pub(crate) broadcast proof fn interacted_up_by(before: &Simulation, s5: Simulation, s6: &Simulation, time: StepTime)
    requires #[trigger] sensed_up(before, &s5, time), #[trigger] interacted(&s5, s6, true)
    ensures interacted_up(before, s6, time)
{}
pub(crate) broadcast proof fn routed_up_by(before: &Simulation, s6: Simulation, s7: &Simulation, time: StepTime, contacts: Seq<EntitySnapshot>)
    requires
        #[trigger] interacted_up(before, &s6, time),
        #[trigger] routed(&s6, s7, time, contacts, true),
        contacts == axioms::snapshot(&s6).entities@,
    ensures routed_up(before, s7, time, contacts)
{}
pub(crate) broadcast proof fn estimated_up_by(before: &Simulation, s7: Simulation, s8: &Simulation, time: StepTime, contacts: Seq<EntitySnapshot>)
    requires #[trigger] routed_up(before, &s7, time, contacts), #[trigger] estimated(&s7, s8, time, contacts, true)
    ensures estimated_up(before, s8, time, contacts)
{}
pub(crate) broadcast proof fn scored_up_by(before: &Simulation, s8: Simulation, s9: &Simulation, time: StepTime, contacts: Seq<EntitySnapshot>)
    requires #[trigger] estimated_up(before, &s8, time, contacts), #[trigger] scored(&s8, s9, time, contacts, true)
    ensures scored_up(before, s9, time, contacts)
{}

/// A tick that failed: the phases before the failing one ran as the Ok
/// chain says, the failing one stopped as its own claim says, and nothing
/// after it ran. The trace, which reads the entities and cannot change
/// them, may also have failed between two phases.
pub open(crate) spec fn failed(before: &Simulation, after: &Simulation) -> bool {
    ||| generated(before, after, false)
    ||| exists|s1: Simulation| #[trigger] generated(before, &s1, true) && started(&s1, after, false)
    ||| exists|s2: Simulation| #[trigger] started_up(before, &s2) && {
        ||| *after == s2
        ||| phase_ran(&s2.entities, &after.entities, Phase::Autonomy(step_time_of(&s2)), false) && *after == Simulation { entities: after.entities, ..s2 }
    }
    ||| exists|s3: Simulation, time: StepTime| #[trigger] autonomy_done(before, &s3, time) && substepped(&s3, after, time, false)
    ||| exists|s4: Simulation, time: StepTime| #[trigger] moved(before, &s4, time) && (*after == s4 || sensed(&s4, after, time, false))
    ||| exists|s5: Simulation, time: StepTime| #[trigger] sensed_up(before, &s5, time) && (*after == s5 || interacted(&s5, after, false))
    ||| exists|s6: Simulation, time: StepTime| #[trigger] interacted_up(before, &s6, time) && routed(&s6, after, time, axioms::snapshot(&s6).entities@, false)
    ||| exists|s7: Simulation, time: StepTime, contacts: Seq<EntitySnapshot>| #[trigger] routed_up(before, &s7, time, contacts) && estimated(&s7, after, time, contacts, false)
    ||| exists|s8: Simulation, time: StepTime, contacts: Seq<EntitySnapshot>| #[trigger] estimated_up(before, &s8, time, contacts) && scored(&s8, after, time, contacts, false)
    ||| exists|s9: Simulation, time: StepTime, contacts: Seq<EntitySnapshot>| #[trigger] scored_up(before, &s9, time, contacts)
        && dead_removed(&s9.entities, &after.entities, time, false) && *after == Simulation { entities: after.entities, ..s9 }
}

/// `step_inner`: a finished run returns nothing and changes nothing; a run
/// with a termination set returns the terminal frame through `finish`; any
/// other returns the pre-step frame after one tick, or an error from the
/// phase that failed, with the phases before it run, nothing after it run,
/// and the clock left alone.
pub open(crate) spec fn stepped(before: &Simulation, after: &Simulation, r: anyhow::Result<Option<SimulationFrame>>) -> bool {
    if before.finished {
        r matches Ok(None) && *after == *before
    } else if before.termination is Some {
        match r {
            Ok(Some(_)) => axioms::finish(before, after, true),
            Ok(None) => false,
            Err(_) => axioms::finish(before, after, false),
        }
    } else {
        match r {
            Ok(Some(frame)) => ticked(before, after, frame),
            Ok(None) => false,
            Err(_) => failed(before, after) && same_clock(before, after),
        }
    }
}

/// `step`: what `stepped` says, and after a failure the run is finished with
/// the plugin failure recorded and the plugins closed.
pub open(crate) spec fn stepped_and_settled(before: &Simulation, after: &Simulation, r: anyhow::Result<Option<SimulationFrame>>) -> bool {
    match r {
        Ok(_) => stepped(before, after, r),
        Err(_) => exists|mid: Simulation|
            #[trigger] stepped(before, &mid, r)
            && plugins_closed(&Simulation { termination: Some(TerminationReason::PluginFailure), finished: true, ..mid }, after)
            && after.termination == Some(TerminationReason::PluginFailure)
            && after.finished,
    }
}

/// Trusted facts about what the tick calls and cannot read.
pub(crate) mod axioms {
    use super::*;

    verus! {

    /// The witness that a phase of the tick ran: with these inputs, taking
    /// the simulation from `before` to `after` and returning Ok when `ok`.
    /// Nothing about what it computed is assumed; each leaves this module
    /// when a later item proves the function.
    pub uninterp spec fn route_messages(before: &Simulation, after: &Simulation, time: StepTime, contacts: Seq<EntitySnapshot>, ok: bool) -> bool;
    pub uninterp spec fn finish(before: &Simulation, after: &Simulation, ok: bool) -> bool;
    pub uninterp spec fn close_plugins(before: &Simulation, after: &Simulation) -> bool;
    /// The frame `snapshot` takes of a simulation, and what its plugins draw.
    pub uninterp spec fn snapshot(sim: &Simulation) -> SimulationFrame;
    pub uninterp spec fn drawings(sim: &Simulation) -> Vec<PluginDrawings>;

    /// Trusted: each phase method is its witness and leaves the clock alone
    /// (`advance_time` moves it by one step; `close_plugins` leaves the
    /// termination and finished flags), as the relations above say.
    pub assume_specification[ Simulation::route_messages ](sim: &mut Simulation, time: StepTime, contacts: &[EntitySnapshot], nearby: &SpatialIndex, trace: Option<&mut Trace<'_>>) -> (r: anyhow::Result<()>)
        ensures super::routed(old(sim), final(sim), time, contacts@, r is Ok);

    /// Trusted: the step count moves on by one; a run never counts 2^64
    /// steps. The method exists because an exec `+= 1` on a `usize` needs an
    /// overflow proof.
    pub assume_specification[ Simulation::count_step ](sim: &mut Simulation)
        ensures
            final(sim).step_count == old(sim).step_count + 1,
            (*final(sim) == Simulation { step_count: final(sim).step_count, ..*old(sim) });

    /// The end condition a state has reached, if any: a plugin's stop
    /// request, the time limit, no entities left, or one team left, as the
    /// mission enables them. Trusted as one function of the state, since the
    /// check reads the mission's end conditions and collects the teams left
    /// through an iterator chain.
    pub uninterp spec fn end_condition(sim: &Simulation) -> Option<TerminationReason>;
    pub assume_specification[ Simulation::reached_end_condition ](sim: &Simulation) -> (r: Option<TerminationReason>)
        ensures r == end_condition(sim);
    pub assume_specification[ Simulation::finish ](sim: &mut Simulation, trace: Option<&mut Trace<'_>>) -> (r: anyhow::Result<SimulationFrame>)
        ensures finish(old(sim), final(sim), r is Ok);
    pub assume_specification[ Simulation::close_plugins ](sim: &mut Simulation) -> (r: anyhow::Result<()>)
        ensures super::plugins_closed(old(sim), final(sim));

    /// The spatial index built from the entities with the mission's lookup,
    /// and the perfect contact measurement of a snapshot: every entity still
    /// in play, known exactly.
    pub uninterp spec fn spatial_index(lookup: SpatialLookup, entities: Seq<Entity>) -> SpatialIndex;
    pub uninterp spec fn perfect_measurement(snapshot: Seq<EntitySnapshot>, sampled_at_s: f64) -> PerfectContacts;

    /// Trusted: `SpatialIndex::build` and `perfect_contacts` build those
    /// values from what they are given; the claims never look inside either.
    pub assume_specification[ SpatialIndex::build ](lookup: SpatialLookup, entities: &[Entity]) -> (r: SpatialIndex)
        ensures r == spatial_index(lookup, entities@);
    pub assume_specification[ super::super::perfect_contacts ](snapshot: &[EntitySnapshot], sampled_at_s: f64) -> (r: PerfectContacts)
        ensures r == perfect_measurement(snapshot@, sampled_at_s);

    /// The truth contacts the world plugins are handed: a view of the snapshot
    /// answering nearness through the index.
    pub uninterp spec fn truth_contacts<'a>(contacts: Seq<EntitySnapshot>, nearby: SpatialIndex) -> TruthContacts<'a>;

    /// Trusted: `TruthContacts::new` builds that view of the list and the
    /// index it is given; the claims never look inside it.
    pub assume_specification<'a>[ TruthContacts::new ](list: &'a [EntitySnapshot], index: &'a SpatialIndex) -> (r: TruthContacts<'a>)
        ensures r == truth_contacts(list@, *index);

    /// Trusted: `snapshot` and `drawings` read the simulation and return its
    /// frame and drawings.
    pub assume_specification[ Simulation::snapshot ](sim: &Simulation) -> (r: SimulationFrame)
        ensures r == snapshot(sim);
    pub assume_specification[ Simulation::drawings ](sim: &Simulation) -> (r: Vec<PluginDrawings>)
        ensures r == drawings(sim);

    /// Trusted: a substep count cast to a double is the IEEE conversion.
    /// Verus's library leaves the exec cast's result uninterpreted, as it
    /// does the float operators (see `common/loop_rate/proof.rs`).
    #[verifier::external_body]
    pub broadcast proof fn count_cast_is_ieee(count: usize, o: f64)
        requires #[trigger] vstd::float::float_cast_spec(count, o)
        ensures o == count as f64
    {}

    /// Trusted: dividing a step by the substep count is IEEE division,
    /// stated over `StepTime` because a float field has no range fact in
    /// this release (see `common/loop_rate/proof.rs`).
    #[verifier::external_body]
    pub broadcast proof fn step_div_is_ieee(time: StepTime, count: f64, o: f64)
        requires #[trigger] vstd::std_specs::ops::div_ensures(time.dt_s, count, o)
        ensures o == time.dt_s / count
    {}

    /// Trusted: the time of a step is the run's start plus the count times
    /// the step, by IEEE multiplication and addition, stated over `RunConfig`
    /// because a float field has no range fact in this Verus release.
    #[verifier::external_body]
    pub broadcast proof fn elapsed_mul_is_ieee(run: RunConfig, count: f64, o: f64)
        requires #[trigger] vstd::std_specs::ops::mul_ensures(count, run.dt_s, o)
        ensures o == count * run.dt_s
    {}
    #[verifier::external_body]
    pub broadcast proof fn start_add_is_ieee(run: RunConfig, elapsed: f64, o: f64)
        requires #[trigger] vstd::std_specs::ops::add_ensures(run.start_s, elapsed, o)
        ensures o == run.start_s + elapsed
    {}

    /// Trusted: promise nothing. The index is rebuilt and the trace appends
    /// records; the claims read neither.
    pub assume_specification[ SpatialIndex::refresh ](index: &mut SpatialIndex, entities: &[Entity]);
    pub assume_specification<'a>[ Trace::<'a>::new ](out: &'a mut dyn TraceSink) -> Trace<'a>;
    pub assume_specification<'a>[ Trace::<'a>::beliefs ](trace: &mut Trace<'a>, time_s: f64, entities: &[Entity]) -> anyhow::Result<()>;
    pub assume_specification<'a>[ Trace::<'a>::outputs ](trace: &mut Trace<'a>, time_s: f64, entities: &[Entity]) -> anyhow::Result<()>;
    pub assume_specification<'a>[ Trace::<'a>::readings ](trace: &mut Trace<'a>, time_s: f64, entities: &[Entity]) -> anyhow::Result<()>;

    /// Trusted: std documents `retain` as keeping, in order, exactly the
    /// elements the predicate answers true for.
    pub assume_specification<T, A: core::alloc::Allocator, F: FnMut(&T) -> bool>[ Vec::<T, A>::retain::<F> ](v: &mut Vec<T, A>, keep: F)
        requires forall|t: T| #[trigger] call_requires(keep, (&t,))
        ensures final(v)@ == old(v)@.filter(|t: T| call_ensures(keep, (&t,), true));

    } // verus!
}

} // verus!

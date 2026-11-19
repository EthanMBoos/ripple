//! What spawning claims, in words a reader of spawning.rs can check: the
//! entities that existed are untouched and keep their order, every new one
//! is appended, born at the tick's time and not yet started, with an ID no
//! entity has had, and the IDs in use, the team map, the events, and the
//! mailbox grow by exactly what the new entities add.
use super::super::generation::Generator;
#[cfg(verus_only)]
use super::super::generation::SpawnRequest;
#[cfg(verus_only)]
use super::super::proof::topic_of;
#[cfg(verus_only)]
use super::super::{Event, EventKind, Simulation};
#[cfg(verus_only)]
use crate::entity::proof::built;
#[cfg(verus_only)]
use crate::entity::{Entity, EntityDefinition, SpawnConfig};
#[cfg(verus_only)]
use crate::math::Vec3;
#[cfg(verus_only)]
use crate::plugin::{Messages, StepTime};
#[cfg(verus_only)]
use crate::pubsub::messages::proof as mailbox;
#[cfg(verus_only)]
use crate::pubsub::messages::{Message, Publication};
use vstd::prelude::*;

verus! {

/// Declares a group's spawn schedule, and the YAML mapping a request lays
/// over a recipe's plugin, as values with no visible structure: the claims
/// never read either.
#[allow(dead_code)]
#[verifier::external_type_specification]
#[verifier::external_body]
pub(crate) struct ExGenerator(Generator);
#[allow(dead_code)]
#[verifier::external_type_specification]
#[verifier::external_body]
pub struct ExMapping(serde_yaml_ng::Mapping);

/// The facts a spawning proof needs, so spawning.rs names one group.
#[allow(dead_code)]
pub(crate) broadcast group spawning_facts {
    one_spawn_grows,
    growth_adds_up,
    keeping_adds_up,
    crate::proof::string_key_facts,
    vstd::seq_lib::group_seq_lib_default,
    vstd::seq_lib::group_seq_properties,
    vstd::set_lib::group_set_lib_default,
}

/// The time a new entity is born: the simulation's time and tick, with no step.
pub open(crate) spec fn born_at(sim: &Simulation) -> StepTime {
    StepTime { time_s: sim.time_s, dt_s: 0.0, tick: sim.tick }
}

/// The event records the generation of entity `id` at `time_s`.
pub open(crate) spec fn is_generation(event: &Event, id: i32, time_s: f64) -> bool {
    event.time_s == time_s && event.kind == EventKind::EntityGenerated && event.entity_ids@ =~= seq![id]
}

/// The publication, sent at `time_s`, carries a generation of entity `id` at
/// `time_s` on the global network's generation topic.
pub open(crate) spec fn generation_announced(publication: &Publication, id: i32, time_s: f64) -> bool {
    &&& publication.channel.0@ == "GlobalNetwork"@
    &&& publication.channel.1@ == topic_of(EventKind::EntityGenerated)
    &&& publication.sent_at_s == time_s
    &&& exists|event: Event| is_generation(&event, id, time_s) && #[trigger] mailbox::carries::<Event>(publication.payload, event)
}

/// Partway through the ID search: nothing has changed, and the IDs in use
/// are still the ones there were.
pub open(crate) spec fn choosing_id(before: &Simulation, sim: &Simulation) -> bool {
    &&& *sim == Simulation { used_ids: sim.used_ids, ..*before }
    &&& sim.used_ids@ =~= before.used_ids@
}

/// `spawn` built one entity of the group from the definition: its ID is one
/// no entity has had and is now in use; the entity is appended after the
/// ones that existed, born at the simulation's time and tick, with its team
/// recorded; one generation event names it at that time, recorded and
/// published on the global network from the mailbox stamped with that time.
/// Nothing else changed. A failure leaves everything but the IDs set aside
/// as it was.
pub open(crate) spec fn spawned(before: &Simulation, after: &Simulation, group_index: usize, definition: &EntityDefinition, ok: bool) -> bool {
    &&& *after == Simulation {
        used_ids: after.used_ids, entities: after.entities, entity_teams: after.entity_teams, events: after.events,
        messages: after.messages, ..*before
    }
    &&& before.used_ids@.subset_of(after.used_ids@)
    &&& ok ==> {
        let entity = after.entities@.last();
        let id = entity.id;
        &&& after.entities@ == before.entities@.push(entity)
        &&& built(&entity, id, group_index as i32, definition, born_at(before))
        &&& !before.used_ids@.contains(id)
        &&& after.used_ids@ == before.used_ids@.insert(id)
        &&& after.entity_teams@ == before.entity_teams@.insert(id, entity.team_id)
        &&& after.events@ == before.events@.push(after.events@.last())
        &&& is_generation(&after.events@.last(), id, before.time_s)
        &&& after.messages.outbox@ == before.messages.outbox@.push(after.messages.outbox@.last())
        &&& generation_announced(&after.messages.outbox@.last(), id, before.time_s)
        &&& after.messages == Messages { outbox: after.messages.outbox, time_s: before.time_s, ..before.messages }
    }
    &&& !ok ==> *after == Simulation { used_ids: after.used_ids, ..*before }
}

/// Nothing of the simulation changed but what spawning touches, the
/// entities that existed are untouched and keep their order, and no ID in
/// use was given up.
pub open(crate) spec fn kept(before: &Simulation, after: &Simulation) -> bool {
    &&& *after == Simulation {
        used_ids: after.used_ids, entities: after.entities, entity_teams: after.entity_teams, events: after.events,
        messages: after.messages, generators: after.generators, ..*before
    }
    &&& after.generators@.len() == before.generators@.len()
    &&& after.entities@.len() >= before.entities@.len()
    &&& after.entities@.take(before.entities@.len() as int) =~= before.entities@
    &&& before.used_ids@.subset_of(after.used_ids@)
}

/// A new entity: born at the simulation's time and tick, in play, not yet
/// started, with no force on it.
pub open(crate) spec fn new_entity(before: &Simulation, entity: &Entity) -> bool {
    &&& entity.born == born_at(before)
    &&& entity.active
    &&& !entity.started
    &&& entity.applied_forces_world_n@ == Seq::<Vec3>::empty()
}

/// The IDs of a run of entities.
pub open(crate) spec fn ids_of(entities: Seq<Entity>) -> Set<i32>
    decreases entities.len()
{
    if entities.len() == 0 {
        Set::empty()
    } else {
        ids_of(entities.drop_last()).insert(entities.last().id)
    }
}

/// The events gained one generation per new entity, in order, after the
/// ones before.
pub open(crate) spec fn generations_recorded(before: Seq<Event>, after: Seq<Event>, new: Seq<Entity>, time_s: f64) -> bool {
    let n = before.len() as int;
    &&& after.len() == n + new.len()
    &&& after.take(n) =~= before
    &&& forall|j: int| 0 <= j < new.len() ==> is_generation(&#[trigger] after[n + j], new[j].id, time_s)
}

/// The mailbox published one generation per new entity, in order, after the
/// ones before, stamped with `time_s` when there was one; nothing else of it
/// changed.
pub open(crate) spec fn generations_announced(before: &Messages, after: &Messages, new: Seq<Entity>, time_s: f64) -> bool {
    let n = before.outbox@.len() as int;
    &&& after.outbox@.len() == n + new.len()
    &&& after.outbox@.take(n) =~= before.outbox@
    &&& forall|j: int| 0 <= j < new.len() ==> generation_announced(&#[trigger] after.outbox@[n + j], new[j].id, time_s)
    &&& *after == Messages { outbox: after.outbox, time_s: if new.len() == 0 { before.time_s } else { time_s }, ..*before }
}

/// The simulation grew: what existed is kept, every new entity is a new
/// entity with an ID no entity had, the new IDs are distinct, the IDs in use
/// and the team map grew by exactly them with each new entity's team
/// recorded and no other team changed, and one generation event per new
/// entity was recorded and announced.
pub open(crate) spec fn grew(before: &Simulation, after: &Simulation) -> bool {
    let new = after.entities@.skip(before.entities@.len() as int);
    &&& kept(before, after)
    &&& forall|j: int| 0 <= j < new.len() ==> new_entity(before, &#[trigger] new[j])
    &&& forall|j: int| 0 <= j < new.len() ==> !before.used_ids@.contains(#[trigger] new[j].id)
    &&& forall|i: int, j: int| 0 <= i < j < new.len() ==> #[trigger] new[i].id != #[trigger] new[j].id
    &&& after.used_ids@ =~= before.used_ids@.union(ids_of(new))
    &&& forall|j: int| 0 <= j < new.len() ==> after.entity_teams@[#[trigger] new[j].id] == new[j].team_id
    &&& after.entity_teams@.dom() =~= before.entity_teams@.dom().union(ids_of(new))
    &&& forall|id: i32| before.entity_teams@.contains_key(id) && !ids_of(new).contains(id) ==> #[trigger] after.entity_teams@[id] == before.entity_teams@[id]
    &&& generations_recorded(before.events@, after.events@, new, before.time_s)
    &&& generations_announced(&before.messages, &after.messages, new, before.time_s)
}

/// The batch loop as Verus sees it: how many spawns it has run.
#[cfg(verus_only)]
pub type Batch = vstd::std_specs::iter::VerusForLoopWrapper<core::ops::Range<usize>>;

/// Partway through a batch: the simulation grew by one entity per spawn so far.
pub open(crate) spec fn batch_so_far(before: &Simulation, sim: &Simulation, it: Batch, group_index: usize) -> bool {
    &&& group_index < before.generators@.len()
    &&& grew(before, sim)
    &&& sim.entities@.len() == before.entities@.len() + it.index()
}

/// `spawn_batch` grew the simulation by `count` entities, or failed on one
/// of them, keeping what existed and what it had built.
pub open(crate) spec fn batch_spawned(before: &Simulation, after: &Simulation, count: usize, ok: bool) -> bool {
    &&& ok ==> grew(before, after) && after.entities@.len() == before.entities@.len() + count
    &&& !ok ==> kept(before, after)
}

/// The requests were received from the global network's spawn topic as typed
/// messages, leaving the mailbox as `mailbox`. The mailbox is named rather
/// than the simulation holding it because the receive is a borrow of the
/// field, so the mailbox value is what the prover is handed.
pub open(crate) spec fn requests_received(before: &Simulation, mailbox: &Messages, network: &str, topic: &str, r: anyhow::Result<Vec<Message<SpawnRequest>>>) -> bool {
    &&& network@ == "GlobalNetwork"@
    &&& topic@ == SpawnRequest::TOPIC@
    &&& mailbox::received::<SpawnRequest>(&before.messages, mailbox, network, topic, &r)
}

/// The simulation once its mailbox has handed over the requests.
pub open(crate) spec fn with_mailbox(before: &Simulation, mailbox: Messages) -> Simulation {
    Simulation { messages: mailbox, ..*before }
}

/// Partway through the requests or the due batches: the requests were
/// received and the simulation grew from there.
pub open(crate) spec fn growing(before: &Simulation, sim: &Simulation) -> bool {
    exists|mailbox: Messages, network: &str, topic: &str, r: anyhow::Result<Vec<Message<SpawnRequest>>>|
        #![trigger mailbox::received::<SpawnRequest>(&before.messages, &mailbox, network, topic, &r)]
    {
        &&& requests_received(before, &mailbox, network, topic, r)
        &&& r is Ok
        &&& grew(&with_mailbox(before, mailbox), sim)
    }
}

/// `spawn_requested`, and `generate_entities` as a whole: the mailbox handed
/// over the spawn requests and the simulation grew from there; a failed
/// receive stops it with nothing else changed, and a failed spawn keeps what
/// existed and what was built before it.
pub open(crate) spec fn generated(before: &Simulation, after: &Simulation, ok: bool) -> bool {
    exists|mailbox: Messages, network: &str, topic: &str, r: anyhow::Result<Vec<Message<SpawnRequest>>>|
        #![trigger mailbox::received::<SpawnRequest>(&before.messages, &mailbox, network, topic, &r)]
    {
        &&& requests_received(before, &mailbox, network, topic, r)
        &&& r is Err ==> !ok && *after == with_mailbox(before, mailbox)
        &&& r is Ok && ok ==> grew(&with_mailbox(before, mailbox), after)
        &&& r is Ok && !ok ==> kept(&with_mailbox(before, mailbox), after)
    }
}

/// The due loop as Verus sees it: the batches due and how many it has built.
#[cfg(verus_only)]
pub type Due = vstd::std_specs::iter::VerusForLoopWrapper<std::vec::IntoIter<(usize, usize)>>;

/// Partway through the due batches: growing, and every batch due names a
/// group the simulation has a generator and a definition for.
#[verifier::prophetic]
pub open(crate) spec fn due_so_far(before: &Simulation, sim: &Simulation, it: Due) -> bool {
    &&& growing(before, sim)
    &&& forall|i: int| 0 <= i < it.seq().len() ==> #[trigger] it.seq()[i].0 < sim.generators@.len() && it.seq()[i].0 < sim.definitions@.len()
}

/// `due_now` moved the schedules on and listed the batches due, each naming
/// a group the simulation has a generator and a definition for; nothing else
/// changed.
pub open(crate) spec fn due_computed(before: &Simulation, after: &Simulation, due: Seq<(usize, usize)>) -> bool {
    &&& *after == Simulation { generators: after.generators, ..*before }
    &&& after.generators@.len() == before.generators@.len()
    &&& forall|i: int| 0 <= i < due.len() ==> #[trigger] due[i].0 < before.generators@.len() && due[i].0 < before.definitions@.len()
}

/// An ID is in the run's IDs exactly when some entity of the run has it.
proof fn ids_of_are_the_ids(entities: Seq<Entity>)
    ensures
        forall|j: int| 0 <= j < entities.len() ==> ids_of(entities).contains(#[trigger] entities[j].id),
        forall|id: i32| #[trigger] ids_of(entities).contains(id) ==> exists|j: int| 0 <= j < entities.len() && #[trigger] entities[j].id == id,
    decreases entities.len()
{
    if entities.len() > 0 {
        ids_of_are_the_ids(entities.drop_last());
        assert forall|j: int| 0 <= j < entities.len() implies ids_of(entities).contains(#[trigger] entities[j].id) by {
            if j < entities.len() - 1 {
                assert(entities.drop_last()[j] == entities[j]);
            }
        }
        assert forall|id: i32| #[trigger] ids_of(entities).contains(id) implies exists|j: int| 0 <= j < entities.len() && #[trigger] entities[j].id == id by {
            if id != entities.last().id {
                assert(ids_of(entities.drop_last()).contains(id));
                let j = choose|j: int| 0 <= j < entities.drop_last().len() && #[trigger] entities.drop_last()[j].id == id;
                assert(entities[j].id == id);
            } else {
                assert(entities[entities.len() - 1].id == id);
            }
        }
    }
}

/// The IDs of two runs together are the IDs of each.
proof fn ids_of_two_runs(first: Seq<Entity>, second: Seq<Entity>)
    ensures ids_of(first + second) =~= ids_of(first).union(ids_of(second))
    decreases second.len()
{
    if second.len() == 0 {
        assert(first + second =~= first);
    } else {
        ids_of_two_runs(first, second.drop_last());
        assert((first + second).drop_last() =~= first + second.drop_last());
        assert((first + second).last() == second.last());
    }
}

/// One more spawn after a growth is a growth: the spawn's exact pushes extend
/// each of the growth's runs by one. The spawn starts from the state with the
/// group's count moved on, which changes only the generators.
pub(crate) broadcast proof fn one_spawn_grows(before: &Simulation, mid: &Simulation, numbered: &Simulation, after: &Simulation, group_index: usize, definition: &EntityDefinition)
    requires
        #[trigger] grew(before, mid),
        (*numbered == Simulation { generators: numbered.generators, ..*mid }),
        numbered.generators@.len() == mid.generators@.len(),
        #[trigger] spawned(numbered, after, group_index, definition, true),
    ensures grew(before, after)
{
    let first = before.entities@.len() as int;
    let new = mid.entities@.skip(first);
    let grown = after.entities@.skip(first);
    let entity = after.entities@.last();
    assert(grown =~= new.push(entity));
    assert(grown.drop_last() =~= new);
    assert(ids_of(grown) == ids_of(new).insert(entity.id));
    ids_of_are_the_ids(new);
    assert(after.used_ids@ =~= before.used_ids@.union(ids_of(grown)));
    assert(after.entity_teams@.dom() =~= before.entity_teams@.dom().union(ids_of(grown)));
    assert(after.events@.take(before.events@.len() as int) =~= before.events@);
    assert(after.messages.outbox@.take(before.messages.outbox@.len() as int) =~= before.messages.outbox@);
    assert(after.entities@.take(first) =~= before.entities@);
}

/// Two growths in a row are one growth: the second's new entities follow the
/// first's, and nothing the second adds collides with what the first did,
/// since the first's IDs were in use when the second chose its own.
pub(crate) broadcast proof fn growth_adds_up(before: &Simulation, mid: &Simulation, after: &Simulation)
    requires
        #[trigger] grew(before, mid),
        #[trigger] grew(mid, after),
    ensures grew(before, after)
{
    let first = before.entities@.len() as int;
    let second = mid.entities@.len() as int;
    let early = mid.entities@.skip(first);
    let late = after.entities@.skip(second);
    let all = after.entities@.skip(first);
    assert(all =~= early + late);
    ids_of_two_runs(early, late);
    ids_of_are_the_ids(early);
    ids_of_are_the_ids(late);
    assert(after.entities@.take(first) =~= before.entities@);
    assert(after.used_ids@ =~= before.used_ids@.union(ids_of(all)));
    assert(after.entity_teams@.dom() =~= before.entity_teams@.dom().union(ids_of(all)));
    assert forall|i: int, j: int| 0 <= i < j < all.len() implies #[trigger] all[i].id != #[trigger] all[j].id by {
        if i < early.len() && j >= early.len() {
            assert(mid.used_ids@.contains(early[i].id));
        }
    }
    assert forall|j: int| 0 <= j < all.len() implies after.entity_teams@[#[trigger] all[j].id] == all[j].team_id by {
        if j < early.len() {
            assert(!ids_of(late).contains(early[j].id));
        }
    }
    assert(after.events@.take(before.events@.len() as int) =~= before.events@);
    assert(after.messages.outbox@.take(before.messages.outbox@.len() as int) =~= before.messages.outbox@);
}

/// Two keeps in a row are one keep.
pub(crate) broadcast proof fn keeping_adds_up(before: &Simulation, mid: &Simulation, after: &Simulation)
    requires
        #[trigger] kept(before, mid),
        #[trigger] kept(mid, after),
    ensures kept(before, after)
{
    assert(after.entities@.take(before.entities@.len() as int) =~= before.entities@);
}

/// Trusted facts about what spawning calls and cannot read.
pub(crate) mod axioms {
    use super::*;

    verus! {

    /// Trusted: `due_now` changes only the generators, keeps their count, and
    /// names groups below it, since the generators are built one per entity
    /// group and recipe in that order, as are the definitions (`resolve` in
    /// scenario.rs). Which batches are due is not modeled.
    pub assume_specification[ Simulation::due_now ](sim: &mut Simulation) -> (r: Vec<(usize, usize)>)
        ensures super::due_computed(old(sim), final(sim), r@);

    /// Trusted: a resolved request names a group below the generators' and
    /// the definitions' counts, since the recipes are numbered after the
    /// entity groups and both have a generator and a definition. Which
    /// definition it resolves to is not modeled.
    pub assume_specification[ Simulation::requested_definition ](sim: &Simulation, request: &SpawnRequest) -> (r: anyhow::Result<(usize, EntityDefinition)>)
        ensures r is Ok ==> r->Ok_0.0 < sim.generators@.len() && r->Ok_0.0 < sim.definitions@.len();

    /// Trusted: a sampled start is some position and heading, or an error
    /// when no free position was found; nothing about it is modeled.
    pub assume_specification[ Simulation::sample_spawn_start ](sim: &Simulation, group_index: usize, number: u64, spawn: &SpawnConfig) -> (r: anyhow::Result<(Vec3, f64)>);

    /// Trusted: promises nothing. The group's count moves on by one, which
    /// the claims never read; the method exists because an exec `+= 1` on a
    /// `u64` needs an overflow proof.
    pub assume_specification[ Generator::next_number ](generator: &mut Generator) -> (n: u64);

    /// Trusted: `last` answers an element of the set, or none when it is
    /// empty, as std documents.
    pub assume_specification<Key: Ord, A: core::alloc::Allocator + Clone>[ std::collections::BTreeSet::<Key, A>::last ](set: &std::collections::BTreeSet<Key, A>) -> (r: Option<&Key>)
        ensures
            r is Some ==> set@.contains(*r->Some_0),
            r is None ==> set@.len() == 0;

    } // verus!
}

} // verus!

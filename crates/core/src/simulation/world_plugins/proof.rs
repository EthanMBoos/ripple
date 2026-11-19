//! What the world interactions and metrics phases claim, in words a reader
//! of world_plugins.rs can check: each interaction stepped once, in order,
//! over the entities and the events the one before left; each metrics plugin
//! stepped once, in order, with the truth contacts, the teams, and the terrain.
use super::{InteractionStep, MetricsStep};
#[cfg(verus_only)]
use super::{WorldPlugins, WorldSlot};
#[cfg(verus_only)]
use crate::common::{SpatialIndex, Terrain, TruthContacts};
#[cfg(verus_only)]
use crate::entity::Entity;
#[cfg(verus_only)]
use crate::plugin::StepTime;
use crate::plugin::{Interaction, InteractionContext, Metrics, Network, Update, WorldContext};
#[cfg(verus_only)]
use crate::pubsub::Messages;
#[cfg(verus_only)]
use crate::simulation::Event;
#[cfg(verus_only)]
use std::collections::BTreeMap;
use vstd::prelude::*;

verus! {

/// Lets Verus read an `Update`.
#[allow(dead_code)]
#[verifier::external_type_specification]
pub struct ExUpdate(Update);

/// Lets Verus read what the engine hands a metrics plugin; every field is public.
#[allow(dead_code)]
#[verifier::external_type_specification]
pub struct ExWorldContext<'a>(WorldContext<'a>);

// The world plugin traits, declared so a `Box<dyn Interaction>` and the
// others are types Verus knows. No method is listed: what a plugin computes
// is never assumed.
#[allow(dead_code)]
#[verifier::external_trait_specification]
pub trait ExInteraction: Send {
    type ExternalTraitSpecificationFor: Interaction;
}
#[allow(dead_code)]
#[verifier::external_trait_specification]
pub trait ExNetwork: Send {
    type ExternalTraitSpecificationFor: Network;
}
#[allow(dead_code)]
#[verifier::external_trait_specification]
pub trait ExMetrics: Send {
    type ExternalTraitSpecificationFor: Metrics;
}

/// Trusted: the one call into an interaction. It promises only
/// `axioms::interacted`, a witness that the call happened with this step
/// time, mailbox, entities, index, terrain, and events and answered Applied,
/// Stop, or an error; nothing about what the plugin computed is assumed. The
/// mailbox is passed as the value before the call because the plugin may
/// publish into it.
#[allow(dead_code)]
#[verifier::external_trait_specification]
pub(crate) trait ExInteractionStep {
    type ExternalTraitSpecificationFor: InteractionStep;
    fn step(&mut self, context: &mut InteractionContext<'_>) -> (r: anyhow::Result<Update>)
        ensures axioms::interacted(
            old(self), final(self), old(context).time, *old(old(context).messages), old(old(context).entities)@,
            final(old(context).entities)@, *old(context).nearby, old(context).terrain, old(old(context).events)@,
            final(old(context).events)@, outcome(&r),
        );
}

/// Trusted: the one call into a metrics plugin. It promises only
/// `axioms::measured`, a witness that the call happened with this step time,
/// mailbox, truth contacts, teams, and terrain and answered Applied, Stop, or
/// an error; nothing about what the plugin computed is assumed.
#[allow(dead_code)]
#[verifier::external_trait_specification]
pub(crate) trait ExMetricsStep {
    type ExternalTraitSpecificationFor: MetricsStep;
    fn step(&mut self, context: &mut WorldContext<'_>) -> (r: anyhow::Result<Update>)
        ensures axioms::measured(
            old(self), final(self), old(context).time, *old(old(context).messages), old(context).contacts_truth,
            *old(context).entity_teams, old(context).terrain, outcome(&r),
        );
}

/// The facts the interactions proofs need, so world_plugins.rs names one group.
#[allow(dead_code)]
pub(crate) broadcast group world_facts {
    ran_with_same_slots,
}

/// The slot's mailbox stamped with the phase time, as the plugin sees it.
pub open(crate) spec fn at_time<T: ?Sized>(slot: &WorldSlot<T>, time: StepTime) -> Messages {
    Messages { time_s: time.time_s, ..slot.messages }
}

/// What a plugin's step answered: its update, or nothing on an error.
pub open(crate) spec fn outcome(r: &anyhow::Result<Update>) -> Option<Update> {
    match r {
        Ok(update) => Some(*update),
        Err(_) => None,
    }
}

/// The slot ran once: its mailbox stamped with `time`, its plugin called with
/// the entities, the index, the terrain, and the events, answering `update`.
/// Nothing else of the slot changed.
pub open(crate) spec fn interaction_slot_stepped<T: ?Sized>(
    before: &WorldSlot<T>, after: &WorldSlot<T>, time: StepTime, entities_before: Seq<Entity>, entities_after: Seq<Entity>,
    nearby: SpatialIndex, terrain: Option<&Terrain>, events_before: Seq<Event>, events_after: Seq<Event>, update: Option<Update>,
) -> bool {
    &&& *after == WorldSlot { plugin: after.plugin, messages: after.messages, ..*before }
    &&& axioms::interacted(
        &*before.plugin, &*after.plugin, time, at_time(before, time), entities_before, entities_after, nearby, terrain,
        events_before, events_after, update,
    )
}

/// `interaction_step` ran the slot once and returned what the plugin answered.
pub open(crate) spec fn interaction_step_returned<T: ?Sized>(
    before: &WorldSlot<T>, after: &WorldSlot<T>, time: StepTime, entities_before: Seq<Entity>, entities_after: Seq<Entity>,
    nearby: SpatialIndex, terrain: Option<&Terrain>, events_before: Seq<Event>, events_after: Seq<Event>, r: &anyhow::Result<Update>,
) -> bool {
    interaction_slot_stepped(before, after, time, entities_before, entities_after, nearby, terrain, events_before, events_after, outcome(r))
}

/// The slot ran once at `time` over the terrain and answered `update`; which
/// entities, index, and events it saw is not said.
pub open(crate) spec fn slot_ran<T: ?Sized>(before: &WorldSlot<T>, after: &WorldSlot<T>, time: StepTime, terrain: Option<&Terrain>, update: Option<Update>) -> bool {
    exists|entities_before: Seq<Entity>, entities_after: Seq<Entity>, nearby: SpatialIndex, events_before: Seq<Event>, events_after: Seq<Event>|
        #[trigger] interaction_slot_stepped(before, after, time, entities_before, entities_after, nearby, terrain, events_before, events_after, update)
}

/// The slot ran and answered, Applied or Stop.
pub open(crate) spec fn slot_answered<T: ?Sized>(before: &WorldSlot<T>, after: &WorldSlot<T>, time: StepTime, terrain: Option<&Terrain>) -> bool {
    slot_ran(before, after, time, terrain, Some(Update::Applied)) || slot_ran(before, after, time, terrain, Some(Update::Stop))
}

/// The first `k` slots ran in order, each over the entities and the events
/// the one before left, taking the entities from `entities_before` to
/// `entities_after` and the events from `events_before` to `events_after`,
/// and each answered.
pub open(crate) spec fn interactions_ran<T: ?Sized>(
    before: Seq<WorldSlot<T>>, after: Seq<WorldSlot<T>>, time: StepTime, terrain: Option<&Terrain>, entities_before: Seq<Entity>,
    entities_after: Seq<Entity>, events_before: Seq<Event>, events_after: Seq<Event>, k: nat,
) -> bool
    decreases k
{
    if k == 0 {
        entities_after == entities_before && events_after == events_before
    } else {
        let last = (k - 1) as nat;
        exists|entities_mid: Seq<Entity>, events_mid: Seq<Event>, nearby: SpatialIndex, update: Update|
            interactions_ran(before, after, time, terrain, entities_before, entities_mid, events_before, events_mid, last)
            && #[trigger] interaction_slot_stepped(
                &before[last as int], &after[last as int], time, entities_mid, entities_after, nearby, terrain, events_mid, events_after,
                Some(update),
            )
    }
}

/// The chain of the first `k` slots names only those slots, so it holds for
/// any sequence that agrees with `after` on them. The loop states its chain
/// over the slots' end values and the function over the vector's; this joins
/// the two.
pub broadcast proof fn ran_with_same_slots<T: ?Sized>(
    before: Seq<WorldSlot<T>>, after: Seq<WorldSlot<T>>, same: Seq<WorldSlot<T>>, time: StepTime, terrain: Option<&Terrain>,
    entities_before: Seq<Entity>, entities_after: Seq<Entity>, events_before: Seq<Event>, events_after: Seq<Event>, k: nat,
)
    requires
        #[trigger] interactions_ran(before, after, time, terrain, entities_before, entities_after, events_before, events_after, k),
        forall|j: int| 0 <= j < k ==> after[j] == same[j],
    ensures #[trigger] interactions_ran(before, same, time, terrain, entities_before, entities_after, events_before, events_after, k)
    decreases k
{
    if k > 0 {
        let last = (k - 1) as nat;
        let (entities_mid, events_mid, nearby, update) = choose|entities_mid: Seq<Entity>, events_mid: Seq<Event>, nearby: SpatialIndex, update: Update|
            interactions_ran(before, after, time, terrain, entities_before, entities_mid, events_before, events_mid, last)
            && #[trigger] interaction_slot_stepped(
                &before[last as int], &after[last as int], time, entities_mid, entities_after, nearby, terrain, events_mid, events_after,
                Some(update),
            );
        ran_with_same_slots(before, after, same, time, terrain, entities_before, entities_mid, events_before, events_mid, last);
    }
}

/// The loop walking the slots, as Verus sees it: the slots it will visit and
/// how many it has.
#[cfg(verus_only)]
pub type Walk<'a, T> = vstd::std_specs::iter::VerusForLoopWrapper<core::slice::IterMut<'a, WorldSlot<T>>>;

/// The slots the loop walks, at their end values.
#[verifier::prophetic]
pub open(crate) spec fn finals<T: ?Sized>(slots: Seq<&mut WorldSlot<T>>) -> Seq<WorldSlot<T>> {
    Seq::new(slots.len(), |j: int| *final(slots[j]))
}

/// Partway through the slots: the ones so far ran in order over the entities
/// and the events, taking them from the loop's start to now, each answered,
/// `stop` is raised exactly when one of them asked to stop, and the rest are
/// as they were. `after` names the slots' end values; the clause saying so
/// is how the solver reaches them.
#[verifier::prophetic]
pub open(crate) spec fn interactions_so_far<T: ?Sized>(
    before: &Vec<WorldSlot<T>>, it: Walk<T>, time: StepTime, terrain: Option<&Terrain>, entities_before: Seq<Entity>, entities: &[Entity],
    events_before: Seq<Event>, events: &Vec<Event>, stop: bool,
) -> bool {
    let slots = it.seq();
    let count = it.index();
    let after = finals(slots);
    &&& slots.len() == before@.len()
    &&& forall|j: int| #![trigger slots[j]] 0 <= j < slots.len() ==> after[j] == *final(slots[j])
    &&& interactions_ran(before@, after, time, terrain, entities_before, entities@, events_before, events@, count as nat)
    &&& forall|j: int| #![trigger slots[j]] 0 <= j < count ==> slot_answered(&before@[j], &after[j], time, terrain)
    &&& stop ==> exists|j: int| #![trigger slots[j]] 0 <= j < count && slot_ran(&before@[j], &after[j], time, terrain, Some(Update::Stop))
    &&& !stop ==> forall|j: int| #![trigger slots[j]] 0 <= j < count ==> slot_ran(&before@[j], &after[j], time, terrain, Some(Update::Applied))
    &&& forall|j: int| #![trigger slots[j]] count <= j < before@.len() ==> *slots[j] == before@[j]
}

/// `step_interactions` ran the slots in order. Ok means every slot ran over
/// the entities and the events the one before left, taking them from before
/// to after, and the answer is whether one asked to stop. Err means some slot
/// failed after every slot before it answered.
pub open(crate) spec fn interactions_stepped<T: ?Sized>(
    before: &Vec<WorldSlot<T>>, after: &Vec<WorldSlot<T>>, time: StepTime, terrain: Option<&Terrain>, entities_before: Seq<Entity>,
    entities_after: Seq<Entity>, events_before: Seq<Event>, events_after: Seq<Event>, r: &anyhow::Result<bool>,
) -> bool {
    let before = before@;
    let after = after@;
    &&& after.len() == before.len()
    &&& r is Ok ==> {
        &&& interactions_ran(before, after, time, terrain, entities_before, entities_after, events_before, events_after, before.len())
        &&& r->Ok_0 ==> exists|j: int| #![trigger after[j]] 0 <= j < before.len() && slot_ran(&before[j], &after[j], time, terrain, Some(Update::Stop))
        &&& !r->Ok_0 ==> forall|j: int| #![trigger after[j]] 0 <= j < before.len() ==> slot_ran(&before[j], &after[j], time, terrain, Some(Update::Applied))
    }
    &&& r is Err ==> exists|k: int| #![trigger after[k]] 0 <= k < before.len()
        && slot_ran(&before[k], &after[k], time, terrain, None)
        && (forall|j: int| #![trigger after[j]] 0 <= j < k ==> slot_answered(&before[j], &after[j], time, terrain))
}

/// `interactions` stepped the interaction slots over the entities and the
/// events and changed nothing else of the world plugins.
pub open(crate) spec fn interacted(
    before: &WorldPlugins, after: &WorldPlugins, time: StepTime, terrain: Option<&Terrain>, entities_before: Seq<Entity>,
    entities_after: Seq<Entity>, events_before: Seq<Event>, events_after: Seq<Event>, r: &anyhow::Result<bool>,
) -> bool {
    &&& interactions_stepped(&before.interactions, &after.interactions, time, terrain, entities_before, entities_after, events_before, events_after, r)
    &&& *after == WorldPlugins { interactions: after.interactions, ..*before }
}

// ---- The metrics.

/// The slot ran once: its mailbox stamped with `time`, its plugin called with
/// the truth contacts, the teams, and the terrain, answering `update`.
/// Nothing else of the slot changed.
pub open(crate) spec fn metrics_slot_stepped<T: ?Sized>(
    before: &WorldSlot<T>, after: &WorldSlot<T>, time: StepTime, contacts: TruthContacts, teams: &BTreeMap<i32, i32>,
    terrain: Option<&Terrain>, update: Option<Update>,
) -> bool {
    &&& *after == WorldSlot { plugin: after.plugin, messages: after.messages, ..*before }
    &&& axioms::measured(&*before.plugin, &*after.plugin, time, at_time(before, time), contacts, *teams, terrain, update)
}

/// `metrics_step` ran the slot once and returned what the plugin answered.
pub open(crate) spec fn metrics_step_returned<T: ?Sized>(
    before: &WorldSlot<T>, after: &WorldSlot<T>, time: StepTime, contacts: TruthContacts, teams: &BTreeMap<i32, i32>,
    terrain: Option<&Terrain>, r: &anyhow::Result<Update>,
) -> bool {
    metrics_slot_stepped(before, after, time, contacts, teams, terrain, outcome(r))
}

/// The slot ran and answered, Applied or Stop.
pub open(crate) spec fn metric_answered<T: ?Sized>(
    before: &WorldSlot<T>, after: &WorldSlot<T>, time: StepTime, contacts: TruthContacts, teams: &BTreeMap<i32, i32>, terrain: Option<&Terrain>,
) -> bool {
    ||| metrics_slot_stepped(before, after, time, contacts, teams, terrain, Some(Update::Applied))
    ||| metrics_slot_stepped(before, after, time, contacts, teams, terrain, Some(Update::Stop))
}

/// Partway through the metrics slots: the ones so far ran, each answered,
/// `stop` is raised exactly when one of them asked to stop, and the rest are
/// as they were.
#[verifier::prophetic]
pub open(crate) spec fn metrics_so_far<T: ?Sized>(
    before: &Vec<WorldSlot<T>>, it: Walk<T>, time: StepTime, contacts: TruthContacts, teams: &BTreeMap<i32, i32>, terrain: Option<&Terrain>, stop: bool,
) -> bool {
    let slots = it.seq();
    let count = it.index();
    &&& slots.len() == before@.len()
    &&& forall|j: int| #![trigger slots[j]] 0 <= j < count ==> metric_answered(&before@[j], final(slots[j]), time, contacts, teams, terrain)
    &&& stop ==> exists|j: int| #![trigger slots[j]] 0 <= j < count && metrics_slot_stepped(&before@[j], final(slots[j]), time, contacts, teams, terrain, Some(Update::Stop))
    &&& !stop ==> forall|j: int| #![trigger slots[j]] 0 <= j < count ==> metrics_slot_stepped(&before@[j], final(slots[j]), time, contacts, teams, terrain, Some(Update::Applied))
    &&& forall|j: int| #![trigger slots[j]] count <= j < before@.len() ==> *slots[j] == before@[j]
}

/// `step_metrics` ran the slots in order. Ok means every slot ran and
/// answered, and the answer is whether one asked to stop. Err means some slot
/// failed after every slot before it answered.
pub open(crate) spec fn metrics_stepped<T: ?Sized>(
    before: &Vec<WorldSlot<T>>, after: &Vec<WorldSlot<T>>, time: StepTime, contacts: TruthContacts, teams: &BTreeMap<i32, i32>,
    terrain: Option<&Terrain>, r: &anyhow::Result<bool>,
) -> bool {
    let before = before@;
    let after = after@;
    &&& after.len() == before.len()
    &&& r is Ok ==> {
        &&& forall|j: int| #![trigger after[j]] 0 <= j < before.len() ==> metric_answered(&before[j], &after[j], time, contacts, teams, terrain)
        &&& r->Ok_0 ==> exists|j: int| #![trigger after[j]] 0 <= j < before.len() && metrics_slot_stepped(&before[j], &after[j], time, contacts, teams, terrain, Some(Update::Stop))
        &&& !r->Ok_0 ==> forall|j: int| #![trigger after[j]] 0 <= j < before.len() ==> metrics_slot_stepped(&before[j], &after[j], time, contacts, teams, terrain, Some(Update::Applied))
    }
    &&& r is Err ==> exists|k: int| #![trigger after[k]] 0 <= k < before.len()
        && metrics_slot_stepped(&before[k], &after[k], time, contacts, teams, terrain, None)
        && (forall|j: int| #![trigger after[j]] 0 <= j < k ==> metric_answered(&before[j], &after[j], time, contacts, teams, terrain))
}

/// `metrics` stepped the metrics slots and changed nothing else of the world plugins.
pub open(crate) spec fn measured(
    before: &WorldPlugins, after: &WorldPlugins, time: StepTime, contacts: TruthContacts, teams: &BTreeMap<i32, i32>,
    terrain: Option<&Terrain>, r: &anyhow::Result<bool>,
) -> bool {
    &&& metrics_stepped(&before.metrics, &after.metrics, time, contacts, teams, terrain, r)
    &&& *after == WorldPlugins { metrics: after.metrics, ..*before }
}

/// Trusted facts about what the phase calls and cannot read.
pub(crate) mod axioms {
    use super::*;

    verus! {

    /// The witness that an interaction was called: with this step time,
    /// mailbox, entities, index, terrain, and events, taking the plugin from
    /// `before` to `after`, the entities and events to their after values,
    /// and answering `update`. Nothing about what it computed is assumed.
    pub uninterp spec fn interacted<T: ?Sized>(
        before: &T, after: &T, time: StepTime, messages: Messages, entities_before: Seq<Entity>, entities_after: Seq<Entity>,
        nearby: SpatialIndex, terrain: Option<&Terrain>, events_before: Seq<Event>, events_after: Seq<Event>, update: Option<Update>,
    ) -> bool;

    /// The witness that a metrics plugin was called: with this step time,
    /// mailbox, truth contacts, teams, and terrain, taking the plugin from
    /// `before` to `after` and answering `update`. Nothing about what it
    /// computed is assumed.
    pub uninterp spec fn measured<T: ?Sized>(
        before: &T, after: &T, time: StepTime, messages: Messages, contacts: TruthContacts, teams: BTreeMap<i32, i32>,
        terrain: Option<&Terrain>, update: Option<Update>,
    ) -> bool;

    /// Trusted: the derived `==` on `Update` compares the variants, which is
    /// what `#[derive(PartialEq)]` on a fieldless enum generates.
    #[cfg(verus_only)]
    impl vstd::std_specs::cmp::PartialEqSpecImpl for Update {
        open spec fn obeys_eq_spec() -> bool { true }
        open spec fn eq_spec(&self, other: &Update) -> bool { self == other }
    }

    /// Trusted: `x |= y` on a `bool` is `x = x || y`, as Rust defines it;
    /// `vstd` states the operator for the integer types only.
    pub assume_specification[ <bool as core::ops::BitOrAssign>::bitor_assign ](x: &mut bool, y: bool)
        ensures *final(x) == (*old(x) || y);

    } // verus!
}

} // verus!

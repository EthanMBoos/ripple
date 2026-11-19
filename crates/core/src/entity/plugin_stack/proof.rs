//! What one plugin step claims, in words a reader of plugin_stack.rs can
//! check. The plugins themselves are not read: the one call into a plugin
//! promises only that it happened.
use super::{Agent, LocalizationStep, MotionStep, SensorStep, TrackingStep};
#[cfg(verus_only)]
use super::{PluginStack, Slot};
#[cfg(verus_only)]
use crate::Vec3;
#[cfg(verus_only)]
use crate::common::Terrain;
#[cfg(verus_only)]
use crate::common::TruthContacts;
#[cfg(verus_only)]
use crate::common::loop_rate::proof as rate;
use crate::plugin::Readings;
#[cfg(verus_only)]
use crate::plugin::estimator::TrackList;
#[cfg(verus_only)]
use crate::plugin::estimator::{Commands, Inputs};
use crate::plugin::estimator::{Detection, PerfectContacts, Source, Track};
use crate::plugin::sensor::ReadingType;
#[cfg(verus_only)]
use crate::plugin::sensor::readings::proof as readings;
use crate::plugin::{
    AgentContext, Autonomy, Controller, Drawing, EntityInfo, KnownTracks, Localization,
    LocalizationContext, MotionContext, MotionModel, PluginIo, PluginRandom, Sensor, SensorContext,
    StepTime, Tracking, TrackingContext,
};
#[cfg(verus_only)]
use crate::ports::{Port, Ports, Signals};
#[cfg(verus_only)]
use crate::pubsub::Messages;
use crate::{GaussianState, KinematicState};
use vstd::prelude::*;

verus! {

/// Lets Verus read a `StepTime`'s fields.
#[allow(dead_code)]
#[verifier::external_type_specification]
pub struct ExStepTime(StepTime);

/// Lets Verus read an `EntityInfo`'s fields.
#[allow(dead_code)]
#[verifier::external_type_specification]
pub struct ExEntityInfo(EntityInfo);

/// Lets Verus read an `AgentContext`'s fields.
#[allow(dead_code)]
#[verifier::external_type_specification]
pub struct ExAgentContext<'a>(AgentContext<'a>);

/// Lets Verus read a `MotionContext`'s fields.
#[allow(dead_code)]
#[verifier::external_type_specification]
pub struct ExMotionContext<'a>(MotionContext<'a>);

/// Declares `KinematicState` to Verus as a value with no visible structure.
#[allow(dead_code)]
#[verifier::external_type_specification]
#[verifier::external_body]
pub struct ExKinematicState(KinematicState);

/// Declares `GaussianState` as a value with no visible structure.
#[allow(dead_code)]
#[verifier::external_type_specification]
#[verifier::external_body]
pub struct ExGaussianState(GaussianState);

/// Declares `Detection` as a value with no visible structure.
#[allow(dead_code)]
#[verifier::external_type_specification]
#[verifier::external_body]
pub struct ExDetection(Detection);

/// Declares `Track` as a value with no visible structure.
#[allow(dead_code)]
#[verifier::external_type_specification]
#[verifier::external_body]
pub struct ExTrack(Track);

/// Declares `KnownTracks` as a value with no visible structure.
#[allow(dead_code)]
#[verifier::external_type_specification]
#[verifier::external_body]
pub struct ExKnownTracks<'a>(KnownTracks<'a>);

/// Declares `Drawing` as a value with no visible structure.
#[allow(dead_code)]
#[verifier::external_type_specification]
#[verifier::external_body]
pub struct ExDrawing(Drawing);

/// Declares `PluginRandom` as a value with no visible structure.
#[allow(dead_code)]
#[verifier::external_type_specification]
#[verifier::external_body]
pub struct ExPluginRandom(PluginRandom);

/// Declares `Source` as a value with no visible structure.
#[allow(dead_code)]
#[verifier::external_type_specification]
#[verifier::external_body]
pub(crate) struct ExSource(Source);

/// Declares `ReadingType` as a value with no visible structure.
#[allow(dead_code)]
#[verifier::external_type_specification]
#[verifier::external_body]
pub(crate) struct ExReadingType(ReadingType);

// The plugin traits, declared so a `Box<dyn Autonomy>` and the others are
// types Verus knows. No method is listed: what a plugin computes is never
// assumed.
#[allow(dead_code)]
#[verifier::external_trait_specification]
pub trait ExAutonomy: Send {
    type ExternalTraitSpecificationFor: Autonomy;
}
#[allow(dead_code)]
#[verifier::external_trait_specification]
pub trait ExController: Send {
    type ExternalTraitSpecificationFor: Controller;
}
#[allow(dead_code)]
#[verifier::external_trait_specification]
pub trait ExMotionModel: Send {
    type ExternalTraitSpecificationFor: MotionModel;
}
#[allow(dead_code)]
#[verifier::external_trait_specification]
pub trait ExSensor: Send {
    type ExternalTraitSpecificationFor: Sensor;
}
#[allow(dead_code)]
#[verifier::external_trait_specification]
pub trait ExLocalization: Send {
    type ExternalTraitSpecificationFor: Localization;
}
#[allow(dead_code)]
#[verifier::external_trait_specification]
pub trait ExTracking: Send {
    type ExternalTraitSpecificationFor: Tracking;
}

/// Trusted: the one call the engine makes into a plugin. It promises only
/// `axioms::stepped`, a witness that the call happened with this step time,
/// mailbox, belief, tracks, and port values, which it may change, and
/// returned Ok or not; nothing about what the plugin computed is assumed.
#[allow(dead_code)]
#[verifier::external_trait_specification]
pub(crate) trait ExAgent {
    type ExternalTraitSpecificationFor: Agent;
    fn step(&mut self, context: &mut AgentContext<'_>, io: &mut PluginIo<'_>) -> (r: anyhow::Result<()>)
        ensures axioms::stepped(
            old(self), final(self), old(context).time, *old(old(context).messages), old(context).state, old(context).tracks,
            *old(old(io).signals), *final(old(io).signals), r is Ok,
        );
}

/// Trusted: the one call into the motion model. It promises only
/// `axioms::moved`, a witness that the call happened with this entity, step
/// time, mailbox, truth, applied forces, terrain, and port values and
/// returned Ok or not; nothing about what the model computed is assumed.
#[allow(dead_code)]
#[verifier::external_trait_specification]
pub(crate) trait ExMotionStep {
    type ExternalTraitSpecificationFor: MotionStep;
    fn step(&mut self, context: &mut MotionContext<'_>, io: &mut PluginIo<'_>) -> (r: anyhow::Result<()>)
        ensures axioms::moved(
            old(self), final(self), old(context).entity, old(context).time, *old(old(context).messages),
            *old(old(context).truth), *final(old(context).truth), old(context).applied_forces_world_n@,
            old(context).terrain, *old(old(io).signals), *final(old(io).signals), r is Ok,
        );
}

/// Trusted: the one call into a sensor. It promises only `axioms::sensed`,
/// a witness that the call happened with this entity, step time, mailbox,
/// truth, truth contacts, instance name, declared reading type, and pending
/// readings and returned Ok or not; nothing about what the sensor read is
/// assumed.
#[allow(dead_code)]
#[verifier::external_trait_specification]
pub(crate) trait ExSensorStep {
    type ExternalTraitSpecificationFor: SensorStep;
    fn step(&mut self, context: &mut SensorContext<'_>) -> (r: anyhow::Result<()>)
        ensures axioms::sensed(
            old(self), final(self), old(context).entity, old(context).time, *old(old(context).messages),
            *old(context).truth, old(context).contacts_truth, old(context).name@, old(context).reading,
            *old(old(context).pending), *final(old(context).pending), r is Ok,
        );
}

/// Trusted: the one call into the localization estimator. It promises only
/// `axioms::localized`, a witness that the call happened with this entity,
/// step time, mailbox, inputs, commands, and belief and returned Ok or not;
/// nothing about what the estimator computed is assumed.
#[allow(dead_code)]
#[verifier::external_trait_specification]
pub(crate) trait ExLocalizationStep {
    type ExternalTraitSpecificationFor: LocalizationStep;
    fn step(&mut self, context: &mut LocalizationContext<'_>) -> (r: anyhow::Result<()>)
        ensures axioms::localized(
            old(self), final(self), old(context).entity, old(context).time, *old(old(context).messages),
            inputs_view(old(context).inputs), commands_view(old(context).commands), *old(old(context).state),
            *final(old(context).state), r is Ok,
        );
}

/// Trusted: the one call into the tracking estimator. It promises only
/// `axioms::tracked`, a witness that the call happened with this entity, step
/// time, mailbox, inputs, pose, and tracks and returned Ok or not.
#[allow(dead_code)]
#[verifier::external_trait_specification]
pub(crate) trait ExTrackingStep {
    type ExternalTraitSpecificationFor: TrackingStep;
    fn step(&mut self, context: &mut TrackingContext<'_>) -> (r: anyhow::Result<()>)
        ensures axioms::tracked(
            old(self), final(self), old(context).entity, old(context).time, *old(old(context).messages),
            inputs_view(old(context).inputs), *old(context).pose, *old(old(context).tracks),
            *final(old(context).tracks), r is Ok,
        );
}

/// The facts a stack proof needs, so plugin_stack.rs names one group.
#[allow(dead_code)]
pub(crate) broadcast group stack_facts {
    crate::proof::string_key_facts,
    crate::proof::text_facts,
    vstd::seq_lib::group_seq_lib_default,
    vstd::seq_lib::group_seq_properties,
    vstd::set_lib::group_set_lib_default,
    keys_in_their_set,
    agents_ran_with_same_slots,
    sensors_ran_with_same_slots,
    axioms::end_add_is_ieee,
}

/// Each key of a key sequence is in the sequence's set, so a key the
/// commands loop reaches is one of the map's.
pub broadcast proof fn keys_in_their_set(keys: Seq<&String>, j: int)
    requires 0 <= j < keys.len()
    ensures keys.unref().to_set().contains(*#[trigger] keys[j])
{
    assert(keys.unref()[j] == *keys[j]);
    assert(keys.unref().to_set().contains(keys.unref()[j]));
}

/// The slot's mailbox stamped with the phase time, as every plugin call sees it.
pub open(crate) spec fn at_time<T: ?Sized>(slot: &Slot<T>, time: StepTime) -> Messages {
    Messages { time_s: time.time_s, ..slot.messages }
}

/// The step time a plugin due this call sees: the phase time and tick, with
/// `dt_s` the time since the plugin last ran.
pub open(crate) spec fn plugin_time(rate: &crate::common::Rate, time: StepTime) -> StepTime {
    StepTime { time_s: time.time_s, dt_s: rate.elapsed_s + time.dt_s, tick: time.tick }
}

/// `agent_step` advanced the slot's rate as `Rate::due` says. A plugin that was
/// due was called with its mailbox stamped with the phase time, the belief,
/// the tracks, the entity's port values, and its own step time, and the
/// step returned Ok exactly when the plugin did. A plugin not due was left
/// as it was, and so were its mailbox and the port values, and the result is
/// Ok. The slot's name, address, ports, random stream, and fixed drawings
/// never change.
pub open(crate) spec fn agent_stepped<T: ?Sized>(
    before: &Slot<T>, after: &Slot<T>, time: StepTime, state: &GaussianState, tracks: KnownTracks,
    signals_before: &Signals, signals_after: &Signals, ok: bool,
) -> bool {
    &&& rate::stepped(&before.rate, &after.rate, time.dt_s, rate::ran(&before.rate, time.dt_s))
    &&& after.name == before.name
    &&& after.endpoint == before.endpoint
    &&& after.ports == before.ports
    &&& after.instance_name == before.instance_name
    &&& after.reading == before.reading
    &&& after.random == before.random
    &&& after.fixed_drawings == before.fixed_drawings
    &&& !rate::is_due(&before.rate, time.dt_s) ==> {
        &&& after.plugin == before.plugin
        &&& after.messages == before.messages
        &&& signals_after == signals_before
        &&& ok
    }
    &&& rate::is_due(&before.rate, time.dt_s) ==> axioms::stepped(
        &*before.plugin, &*after.plugin, plugin_time(&before.rate, time), at_time(before, time), state, tracks,
        *signals_before, *signals_after, ok,
    )
}

/// The first `k` slots ran in order, each over the port board the one before
/// left, taking the board from `signals_before` to `signals_after`, and each
/// returned Ok. This is the handoff a plugin author relies on: what one
/// plugin wrote is what the next one read.
pub open(crate) spec fn agents_ran<T: ?Sized>(
    before: Seq<Slot<T>>, after: Seq<Slot<T>>, time: StepTime, state: &GaussianState, tracks: KnownTracks,
    signals_before: Signals, signals_after: Signals, k: nat,
) -> bool
    decreases k
{
    if k == 0 {
        signals_after == signals_before
    } else {
        let last = (k - 1) as nat;
        exists|signals_mid: Signals|
            agents_ran(before, after, time, state, tracks, signals_before, signals_mid, last)
            && #[trigger] agent_stepped(&before[last as int], &after[last as int], time, state, tracks, &signals_mid, &signals_after, true)
    }
}

/// The chain of the first `k` slots names only those slots, so it holds for
/// any sequence that agrees with `after` on them. The loop states its chain
/// over the slots' end values and the function over the vector's; this joins
/// the two.
pub broadcast proof fn agents_ran_with_same_slots<T: ?Sized>(
    before: Seq<Slot<T>>, after: Seq<Slot<T>>, same: Seq<Slot<T>>, time: StepTime, state: &GaussianState,
    tracks: KnownTracks, signals_before: Signals, signals_after: Signals, k: nat,
)
    requires
        #[trigger] agents_ran(before, after, time, state, tracks, signals_before, signals_after, k),
        forall|j: int| 0 <= j < k ==> after[j] == same[j],
    ensures #[trigger] agents_ran(before, same, time, state, tracks, signals_before, signals_after, k)
    decreases k
{
    if k > 0 {
        let last = (k - 1) as nat;
        let signals_mid = choose|signals_mid: Signals|
            agents_ran(before, after, time, state, tracks, signals_before, signals_mid, last)
            && #[trigger] agent_stepped(&before[last as int], &after[last as int], time, state, tracks, &signals_mid, &signals_after, true);
        agents_ran_with_same_slots(before, after, same, time, state, tracks, signals_before, signals_mid, last);
    }
}

/// The loop walking the slots, as Verus sees it: the slots it will visit and
/// how many it has.
#[cfg(verus_only)]
pub(crate) type Walk<'a, T> = vstd::std_specs::iter::VerusForLoopWrapper<core::slice::IterMut<'a, Slot<T>>>;

/// The slots the loop walks, at their end values.
#[verifier::prophetic]
pub open(crate) spec fn finals<T: ?Sized>(slots: Seq<&mut Slot<T>>) -> Seq<Slot<T>> {
    Seq::new(slots.len(), |j: int| *final(slots[j]))
}

/// Partway through the loop: the slots so far ran in order over the port
/// board, taking it from the loop's start to now, each returned Ok, and the
/// rest are as they were. `after` names the slots' end values; the clause
/// saying so is how the solver reaches them.
#[verifier::prophetic]
pub open(crate) spec fn agents_so_far<T: ?Sized>(
    before: &Vec<Slot<T>>, it: Walk<T>, time: StepTime, state: &GaussianState, tracks: KnownTracks,
    signals_before: Signals, signals: &Signals,
) -> bool {
    let slots = it.seq();
    let count = it.index();
    let after = finals(slots);
    &&& slots.len() == before@.len()
    &&& forall|j: int| #![trigger slots[j]] 0 <= j < slots.len() ==> after[j] == *final(slots[j])
    &&& agents_ran(before@, after, time, state, tracks, signals_before, *signals, count as nat)
    &&& forall|j: int| #![trigger slots[j]] count <= j < before@.len() ==> *slots[j] == before@[j]
}

/// `step_agents` stepped the slots in order over the port board. Ok means
/// every slot ran over the board the one before left and returned Ok, taking
/// the board from before to after. Err means some slot returned Err over the
/// board the slots before it left, each of which returned Ok, and left the
/// board as it is after; the loop stopped there, so nothing is said of the
/// slots after it.
pub open(crate) spec fn agents_stepped<T: ?Sized>(
    before: &Vec<Slot<T>>, after: &Vec<Slot<T>>, time: StepTime, state: &GaussianState, tracks: KnownTracks,
    signals_before: &Signals, signals_after: &Signals, ok: bool,
) -> bool {
    let before = before@;
    let after = after@;
    &&& after.len() == before.len()
    &&& ok ==> agents_ran(before, after, time, state, tracks, *signals_before, *signals_after, before.len())
    &&& !ok ==> exists|k: int, signals_mid: Signals| #![trigger after[k], agent_stepped(&before[k], &after[k], time, state, tracks, &signals_mid, signals_after, false)]
        0 <= k < before.len()
        && agents_ran(before, after, time, state, tracks, *signals_before, signals_mid, k as nat)
        && agent_stepped(&before[k], &after[k], time, state, tracks, &signals_mid, signals_after, false)
}

/// `autonomy` stepped the autonomy slots with the stack's belief and the
/// entity's known tracks over the stack's port board, and changed nothing
/// else.
pub open(crate) spec fn autonomy_stepped(before: &PluginStack, after: &PluginStack, entity: EntityInfo, time: StepTime, ok: bool) -> bool {
    &&& agents_stepped(
        &before.autonomies, &after.autonomies, time, &before.belief, axioms::known_tracks(before.tracks, entity.id),
        &before.signals, &after.signals, ok,
    )
    &&& *after == PluginStack { autonomies: after.autonomies, signals: after.signals, ..*before }
}

/// The stack after `follow_perfect_state`: belief is the truth when the
/// localization is the perfect state measurement passed through, else
/// nothing changes.
pub open(crate) spec fn followed(stack: &PluginStack, truth: &KinematicState) -> PluginStack {
    if stack.belief_is_perfect_state { PluginStack { belief: axioms::perfect_state(*truth), ..*stack } } else { *stack }
}

/// `motion_step` stamped the mailbox with the phase time and called the motion
/// model once with the entity, the time, the truth, the applied forces, the
/// terrain, and the port values; nothing else of the slot changed.
pub open(crate) spec fn motion_slot_stepped<T: ?Sized>(
    before: &Slot<T>, after: &Slot<T>, entity: EntityInfo, time: StepTime, truth_before: &KinematicState,
    truth_after: &KinematicState, forces: Seq<Vec3>, terrain: Option<&Terrain>, signals_before: &Signals,
    signals_after: &Signals, ok: bool,
) -> bool {
    &&& *after == Slot { plugin: after.plugin, messages: after.messages, ..*before }
    &&& axioms::moved(
        &*before.plugin, &*after.plugin, entity, time, at_time(before, time), *truth_before, *truth_after, forces, terrain,
        *signals_before, *signals_after, ok,
    )
}

/// `record_commands` appended the value of each key of `ports.inputs`, in
/// increasing key order, and returned Ok; or some input had no value.
pub open(crate) spec fn commands_recorded(ports: &Ports, signals: &Signals, before: &Vec<f64>, after: &Vec<f64>, ok: bool) -> bool {
    &&& ok ==> exists|keys: Seq<&String>| {
        &&& #[trigger] keys.unref().to_set() == ports.inputs@.dom()
        &&& keys.unref().no_duplicates()
        &&& vstd::std_specs::btree::increasing_seq(keys)
        &&& (forall|j: int| 0 <= j < keys.len() ==> signals@.contains_key(*#[trigger] keys[j]))
        &&& after@ =~= before@ + keys.map_values(|k: &String| signals@[*k])
    }
    &&& !ok ==> exists|name: String| #[trigger] ports.inputs@.contains_key(name) && !signals@.contains_key(name)
}

/// The loop walking the input keys, as Verus sees it.
#[cfg(verus_only)]
pub(crate) type KeyWalk<'a> = vstd::std_specs::iter::VerusForLoopWrapper<std::collections::btree_map::Keys<'a, String, Port>>;

/// Partway through the commands loop: the keys are the inputs in increasing
/// order, every key reached had a value, and those values are appended so far.
#[verifier::prophetic]
pub open(crate) spec fn commands_so_far(ports: &Ports, signals: &Signals, before: &Vec<f64>, now: &Vec<f64>, it: KeyWalk) -> bool {
    let keys = it.seq();
    let count = it.index();
    &&& keys.unref().to_set() == ports.inputs@.dom()
    &&& keys.unref().no_duplicates()
    &&& vstd::std_specs::btree::increasing_seq(keys)
    &&& (forall|j: int| 0 <= j < keys.len() ==> ports.inputs@.contains_key(*#[trigger] keys[j]))
    &&& (forall|j: int| 0 <= j < count ==> signals@.contains_key(*#[trigger] keys[j]))
    &&& now@ =~= before@ + keys.take(count).map_values(|k: &String| signals@[*k])
}

/// `sensor_step` stamped the mailbox with the phase time and advanced the
/// slot's rate as `Rate::due` says, with the tick's own dt. A sensor that was
/// due was called with the entity, the time, the truth, the truth contacts,
/// its instance name and reading type, and the pending readings, and the
/// step returned Ok exactly when the sensor did. A sensor not due was left as
/// it was, and so were the pending readings, and the result is Ok. Nothing
/// else of the slot changes.
pub open(crate) spec fn sensor_stepped<T: ?Sized>(
    before: &Slot<T>, after: &Slot<T>, entity: EntityInfo, time: StepTime, truth: &KinematicState, contacts: TruthContacts,
    pending_before: &Readings, pending_after: &Readings, ok: bool,
) -> bool {
    &&& rate::stepped(&before.rate, &after.rate, time.dt_s, rate::ran(&before.rate, time.dt_s))
    &&& *after == Slot { plugin: after.plugin, rate: after.rate, messages: after.messages, ..*before }
    &&& !rate::is_due(&before.rate, time.dt_s) ==> {
        &&& after.plugin == before.plugin
        &&& after.messages == at_time(before, time)
        &&& pending_after == pending_before
        &&& ok
    }
    &&& rate::is_due(&before.rate, time.dt_s) ==> axioms::sensed(
        &*before.plugin, &*after.plugin, entity, time, at_time(before, time), *truth, contacts, before.instance_name@,
        before.reading, *pending_before, *pending_after, ok,
    )
}

/// The first `k` sensors ran in order, each over the pending readings the one
/// before left, taking them from `pending_before` to `pending_after`, and
/// each returned Ok. So a reading a sensor published is still pending when
/// the loop ends, for the estimation phase to deliver.
pub open(crate) spec fn sensors_ran<T: ?Sized>(
    before: Seq<Slot<T>>, after: Seq<Slot<T>>, entity: EntityInfo, time: StepTime, truth: &KinematicState,
    contacts: TruthContacts, pending_before: Readings, pending_after: Readings, k: nat,
) -> bool
    decreases k
{
    if k == 0 {
        pending_after == pending_before
    } else {
        let last = (k - 1) as nat;
        exists|pending_mid: Readings|
            sensors_ran(before, after, entity, time, truth, contacts, pending_before, pending_mid, last)
            && #[trigger] sensor_stepped(&before[last as int], &after[last as int], entity, time, truth, contacts, &pending_mid, &pending_after, true)
    }
}

/// As `agents_ran_with_same_slots`, for the sensor chain.
pub broadcast proof fn sensors_ran_with_same_slots<T: ?Sized>(
    before: Seq<Slot<T>>, after: Seq<Slot<T>>, same: Seq<Slot<T>>, entity: EntityInfo, time: StepTime, truth: &KinematicState,
    contacts: TruthContacts, pending_before: Readings, pending_after: Readings, k: nat,
)
    requires
        #[trigger] sensors_ran(before, after, entity, time, truth, contacts, pending_before, pending_after, k),
        forall|j: int| 0 <= j < k ==> after[j] == same[j],
    ensures #[trigger] sensors_ran(before, same, entity, time, truth, contacts, pending_before, pending_after, k)
    decreases k
{
    if k > 0 {
        let last = (k - 1) as nat;
        let pending_mid = choose|pending_mid: Readings|
            sensors_ran(before, after, entity, time, truth, contacts, pending_before, pending_mid, last)
            && #[trigger] sensor_stepped(&before[last as int], &after[last as int], entity, time, truth, contacts, &pending_mid, &pending_after, true);
        sensors_ran_with_same_slots(before, after, same, entity, time, truth, contacts, pending_before, pending_mid, last);
    }
}

/// Partway through the sensor loop: the sensors so far ran in order over the
/// pending readings, taking them from the loop's start to now, each returned
/// Ok, and the rest are as they were.
#[verifier::prophetic]
pub open(crate) spec fn sensors_so_far<T: ?Sized>(
    before: &Vec<Slot<T>>, it: Walk<T>, entity: EntityInfo, time: StepTime, truth: &KinematicState, contacts: TruthContacts,
    pending_before: Readings, pending: &Readings,
) -> bool {
    let slots = it.seq();
    let count = it.index();
    let after = finals(slots);
    &&& slots.len() == before@.len()
    &&& forall|j: int| #![trigger slots[j]] 0 <= j < slots.len() ==> after[j] == *final(slots[j])
    &&& sensors_ran(before@, after, entity, time, truth, contacts, pending_before, *pending, count as nat)
    &&& forall|j: int| #![trigger slots[j]] count <= j < before@.len() ==> *slots[j] == before@[j]
}

/// `step_sensors` stepped the sensors in order over the pending readings, as
/// `agents_stepped` says of the agent slots over the port board: Ok means
/// every sensor ran over the readings the one before left and returned Ok;
/// Err means some sensor returned Err over the readings the sensors before it
/// left, each of which returned Ok, and left the readings as they are after.
pub open(crate) spec fn sensors_stepped<T: ?Sized>(
    before: &Vec<Slot<T>>, after: &Vec<Slot<T>>, entity: EntityInfo, time: StepTime, truth: &KinematicState, contacts: TruthContacts,
    pending_before: &Readings, pending_after: &Readings, ok: bool,
) -> bool {
    let before = before@;
    let after = after@;
    &&& after.len() == before.len()
    &&& ok ==> sensors_ran(before, after, entity, time, truth, contacts, *pending_before, *pending_after, before.len())
    &&& !ok ==> exists|k: int, pending_mid: Readings| #![trigger after[k], sensor_stepped(&before[k], &after[k], entity, time, truth, contacts, &pending_mid, pending_after, false)]
        0 <= k < before.len()
        && sensors_ran(before, after, entity, time, truth, contacts, *pending_before, pending_mid, k as nat)
        && sensor_stepped(&before[k], &after[k], entity, time, truth, contacts, &pending_mid, pending_after, false)
}

/// `sensors` stepped the sensor slots over the stack's pending readings and
/// changed nothing else.
pub open(crate) spec fn sensed(
    before: &PluginStack, after: &PluginStack, entity: EntityInfo, time: StepTime, truth: &KinematicState, contacts: TruthContacts, ok: bool,
) -> bool {
    &&& sensors_stepped(&before.sensors, &after.sensors, entity, time, truth, contacts, &before.pending, &after.pending, ok)
    &&& *after == PluginStack { sensors: after.sensors, pending: after.pending, ..*before }
}

/// What an estimator's inputs are, as a value: the sources, the readings, the
/// perfect measurements, and the time the estimate is for.
#[allow(dead_code)]
pub struct InputsView {
    pub sources: Seq<(String, Source)>,
    pub readings: Readings,
    pub perfect_state: GaussianState,
    pub perfect_contacts: PerfectContacts,
    pub now_s: f64,
}

/// The inputs an estimator was handed, as a value.
pub open(crate) spec fn inputs_view(inputs: Inputs) -> InputsView {
    InputsView {
        sources: inputs.sources@, readings: *inputs.readings, perfect_state: *inputs.perfect_state,
        perfect_contacts: *inputs.perfect_contacts, now_s: inputs.now_s,
    }
}

/// The inputs of one estimation at `time`: these sources over these readings
/// and perfect measurements, for the end of the step.
pub open(crate) spec fn inputs(
    sources: Seq<(String, Source)>, readings: Readings, perfect_state: GaussianState, perfect_contacts: PerfectContacts, time: StepTime,
) -> InputsView {
    InputsView { sources, readings, perfect_state, perfect_contacts, now_s: time.time_s + time.dt_s }
}

/// The commands an estimator was handed, as a value.
#[allow(dead_code)]
pub struct CommandsView {
    pub names: Seq<String>,
    pub values: Seq<f64>,
    pub times: Seq<StepTime>,
}

pub open(crate) spec fn commands_view(commands: Commands) -> CommandsView {
    CommandsView { names: commands.names@, values: commands.values@, times: commands.times@ }
}

/// The commands the stack recorded this tick: what the motion model read on each substep.
pub open(crate) spec fn commands_of(stack: &PluginStack) -> CommandsView {
    CommandsView { names: stack.command_names@, values: stack.command_values@, times: stack.command_times@ }
}

/// `localization_step` stamped the mailbox with the phase time and called
/// localization once with the entity, the time, these inputs and commands,
/// and the belief it may change; nothing else of the slot changed.
pub open(crate) spec fn localization_stepped<T: ?Sized>(
    before: &Slot<T>, after: &Slot<T>, entity: EntityInfo, time: StepTime, inputs: InputsView, commands: CommandsView,
    belief_before: &GaussianState, belief_after: &GaussianState, ok: bool,
) -> bool {
    &&& *after == Slot { plugin: after.plugin, messages: after.messages, ..*before }
    &&& axioms::localized(
        &*before.plugin, &*after.plugin, entity, time, at_time(before, time), inputs, commands, *belief_before, *belief_after, ok,
    )
}

/// `tracking_step` stamped the mailbox with the phase time and called tracking
/// once with the entity, the time, these inputs, the pose, and the tracks it
/// may change; nothing else of the slot changed.
pub open(crate) spec fn tracking_stepped<T: ?Sized>(
    before: &Slot<T>, after: &Slot<T>, entity: EntityInfo, time: StepTime, inputs: InputsView, pose: &GaussianState,
    tracks_before: &TrackList, tracks_after: &TrackList, ok: bool,
) -> bool {
    &&& *after == Slot { plugin: after.plugin, messages: after.messages, ..*before }
    &&& axioms::tracked(
        &*before.plugin, &*after.plugin, entity, time, at_time(before, time), inputs, *pose, *tracks_before, *tracks_after, ok,
    )
}

/// `estimate` delivered this tick's readings, stepped localization on them
/// with this tick's commands and the perfect measurements, writing belief,
/// stepped tracking on them with that belief as the pose, writing the
/// tracks, then cleared the fresh marks and the commands; nothing else of
/// the stack changed. Err means an estimator returned Err after the readings
/// were delivered, and what follows it did not run.
pub open(crate) spec fn estimated(
    before: &PluginStack, after: &PluginStack, entity: EntityInfo, time: StepTime, truth: &KinematicState,
    perfect_contacts: &PerfectContacts, ok: bool,
) -> bool {
    &&& *after == PluginStack {
        localization: after.localization, tracking: after.tracking, readings: after.readings, pending: after.pending,
        belief: after.belief, tracks: after.tracks, command_values: after.command_values,
        command_times: after.command_times, ..*before
    }
    &&& exists|delivered: Readings| {
        let localization_inputs = inputs(before.localization_sources@, delivered, axioms::perfect_state(*truth), *perfect_contacts, time);
        let tracking_inputs = inputs(before.tracking_sources@, delivered, axioms::perfect_state(*truth), *perfect_contacts, time);
        &&& #[trigger] readings::delivered(&before.readings, &delivered, &before.pending, &after.pending)
        &&& ok ==> {
            &&& localization_stepped(
                &before.localization, &after.localization, entity, time, localization_inputs, commands_of(before),
                &before.belief, &after.belief, true,
            )
            &&& tracking_stepped(
                &before.tracking, &after.tracking, entity, time, tracking_inputs, &after.belief, &before.tracks,
                &after.tracks, true,
            )
            &&& readings::cleared(&delivered, &after.readings)
            &&& after.command_values@ == Seq::<f64>::empty()
            &&& after.command_times@ == Seq::<StepTime>::empty()
        }
        &&& !ok ==> {
            ||| localization_stepped(
                &before.localization, &after.localization, entity, time, localization_inputs, commands_of(before),
                &before.belief, &after.belief, false,
            ) && after.tracking == before.tracking && after.tracks == before.tracks && after.readings == delivered
            ||| tracking_stepped(
                &before.tracking, &after.tracking, entity, time, tracking_inputs, &after.belief, &before.tracks,
                &after.tracks, false,
            ) && after.readings == delivered
        }
    }
}

/// `motion` recorded the motion model's input values and the substep time
/// for localization, then stepped the model once; nothing else of the stack
/// changed. Err means an input had no value and the model did not run, or
/// the model returned Err.
pub open(crate) spec fn motion_stepped(
    before: &PluginStack, after: &PluginStack, entity: EntityInfo, time: StepTime, truth_before: &KinematicState,
    truth_after: &KinematicState, forces: Seq<Vec3>, terrain: Option<&Terrain>, ok: bool,
) -> bool {
    &&& *after == PluginStack {
        motion: after.motion, signals: after.signals, command_values: after.command_values,
        command_times: after.command_times, ..*before
    }
    &&& ok ==> {
        &&& commands_recorded(&before.motion.ports, &before.signals, &before.command_values, &after.command_values, true)
        &&& after.command_times@ == before.command_times@.push(time)
        &&& motion_slot_stepped(
            &before.motion, &after.motion, entity, time, truth_before, truth_after, forces, terrain, &before.signals,
            &after.signals, true,
        )
    }
    &&& !ok ==> {
        ||| commands_recorded(&before.motion.ports, &before.signals, &before.command_values, &after.command_values, false)
            && after.motion == before.motion && after.signals == before.signals && truth_after == truth_before
        ||| motion_slot_stepped(
            &before.motion, &after.motion, entity, time, truth_before, truth_after, forces, terrain, &before.signals,
            &after.signals, false,
        )
    }
}

/// `controllers` stepped the controller slots the same way.
pub open(crate) spec fn controllers_stepped(before: &PluginStack, after: &PluginStack, entity: EntityInfo, time: StepTime, ok: bool) -> bool {
    &&& agents_stepped(
        &before.controllers, &after.controllers, time, &before.belief, axioms::known_tracks(before.tracks, entity.id),
        &before.signals, &after.signals, ok,
    )
    &&& *after == PluginStack { controllers: after.controllers, signals: after.signals, ..*before }
}

/// Trusted facts about what the step calls and cannot read.
pub(crate) mod axioms {
    use super::*;

    verus! {

    /// The witness that a plugin was called: with this step time, its mailbox,
    /// the belief, the tracks, and the port values it may change, taking it
    /// from `before` to `after` and returning Ok when `ok`. Nothing else is
    /// said, so the proof holds for any plugin.
    pub uninterp spec fn stepped<T: ?Sized>(
        before: &T, after: &T, time: StepTime, messages: Messages, state: &GaussianState, tracks: KnownTracks,
        signals_before: Signals, signals_after: Signals, ok: bool,
    ) -> bool;

    /// The witness that the motion model was called: told of this entity, at
    /// this step time, with its mailbox, the truth it may change, the applied
    /// forces, the terrain, and the port values it may change, taking the
    /// model from `before` to `after` and returning Ok when `ok`.
    pub uninterp spec fn moved<T: ?Sized>(
        before: &T, after: &T, entity: EntityInfo, time: StepTime, messages: Messages, truth_before: KinematicState,
        truth_after: KinematicState, forces: Seq<Vec3>, terrain: Option<&Terrain>, signals_before: Signals,
        signals_after: Signals, ok: bool,
    ) -> bool;

    /// The witness that a sensor was called: told of this entity, at this
    /// step time, with its mailbox, the truth, the truth contacts, its instance
    /// name and reading type, and the pending readings it may publish into,
    /// taking the sensor from `before` to `after` and returning Ok when `ok`.
    pub uninterp spec fn sensed<T: ?Sized>(
        before: &T, after: &T, entity: EntityInfo, time: StepTime, messages: Messages, truth: KinematicState,
        contacts: TruthContacts, name: Seq<char>, reading: ReadingType, pending_before: Readings, pending_after: Readings,
        ok: bool,
    ) -> bool;

    /// The witness that localization was called: told of this entity, at this
    /// step time, with its mailbox, these inputs and commands, and the belief it
    /// may change, taking it from `before` to `after` and returning Ok when `ok`.
    pub uninterp spec fn localized<T: ?Sized>(
        before: &T, after: &T, entity: EntityInfo, time: StepTime, messages: Messages, inputs: InputsView,
        commands: CommandsView, belief_before: GaussianState, belief_after: GaussianState, ok: bool,
    ) -> bool;

    /// The witness that tracking was called: told of this entity, at this step
    /// time, with its mailbox, these inputs, the pose, and the tracks it may
    /// change, taking it from `before` to `after` and returning Ok when `ok`.
    pub uninterp spec fn tracked<T: ?Sized>(
        before: &T, after: &T, entity: EntityInfo, time: StepTime, messages: Messages, inputs: InputsView,
        pose: GaussianState, tracks_before: TrackList, tracks_after: TrackList, ok: bool,
    ) -> bool;

    /// Trusted: the end of a step is IEEE addition of its start and its
    /// length, stated over `StepTime` because a float field has no range fact
    /// in this release (see `common/loop_rate/proof.rs`).
    #[verifier::external_body]
    pub broadcast proof fn end_add_is_ieee(time: StepTime, o: f64)
        requires #[trigger] vstd::std_specs::ops::add_ensures(time.time_s, time.dt_s, o)
        ensures o == time.time_s + time.dt_s
    {}

    /// The known-tracks view of a list for one entity, as `KnownTracks::new` builds it.
    pub uninterp spec fn known_tracks<'a>(list: TrackList, own_id: i32) -> KnownTracks<'a>;

    /// Trusted: `KnownTracks::new` builds the view of its list for the entity.
    pub assume_specification<'a>[ KnownTracks::<'a>::new ](list: &'a TrackList, own_id: i32) -> (r: KnownTracks<'a>)
        ensures r == known_tracks(*list, own_id);

    /// The perfect state measurement of a truth: that state, known exactly, as
    /// `GaussianState::perfect` builds it.
    pub uninterp spec fn perfect_state(truth: KinematicState) -> GaussianState;

    /// Trusted: `GaussianState::perfect` builds the perfect measurement of its
    /// argument; the claims never look inside it.
    pub assume_specification[ GaussianState::perfect ](mean: KinematicState) -> (r: GaussianState)
        ensures r == perfect_state(mean);

    /// Trusted: a plugin's random numbers at a tick are some value; the claims
    /// say nothing about which.
    pub assume_specification[ PluginRandom::at_tick ](random: PluginRandom, tick: i64) -> PluginRandom;

    /// Trusted: `context` wraps an error in a message and does nothing else.
    pub assume_specification<C: core::fmt::Display + Send + Sync + 'static>[ anyhow::Error::context::<C> ](error: anyhow::Error, context: C) -> anyhow::Error;

    /// The witness that the stack's plugins were closed at `time`, taking it
    /// from `before` to `after` and returning Ok when `ok`. Nothing else is
    /// said: `close` records each plugin's error through a closure over a
    /// mutable reference, which Verus does not read.
    pub uninterp spec fn closed(before: &PluginStack, after: &PluginStack, time: StepTime, ok: bool) -> bool;

    /// Trusted: `close` is the close of every plugin in the stack.
    pub assume_specification[ PluginStack::close ](stack: &mut PluginStack, time: StepTime) -> (r: anyhow::Result<()>)
        ensures closed(old(stack), final(stack), time, r is Ok);

    /// The witness that the stack's estimators and decision plugins were
    /// started for this entity at `time` with its truth and the perfect
    /// detections, taking the stack from `before` to `after` and returning Ok
    /// when `ok`. Nothing else is said: `start` initializes each plugin
    /// through its own context, which is a plugin call.
    pub uninterp spec fn started(
        before: &PluginStack, after: &PluginStack, entity: EntityInfo, time: StepTime, truth: &KinematicState,
        perfect_contacts: &PerfectContacts, ok: bool,
    ) -> bool;

    /// Trusted: `start` is the start of the stack's plugins.
    pub assume_specification[ PluginStack::start ](
        stack: &mut PluginStack, entity: EntityInfo, time: StepTime, truth: &KinematicState, perfect_contacts: &PerfectContacts,
    ) -> (r: anyhow::Result<()>)
        ensures started(old(stack), final(stack), entity, time, truth, perfect_contacts, r is Ok);

    } // verus!
}

} // verus!

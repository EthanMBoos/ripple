//! What the routing functions claim, in words a reader of routing.rs can check:
//! the publications a phase collects, the subscriber index, the offers, the
//! deliveries, and the phase as their chain.
#[cfg(verus_only)]
use super::{
    Channel, Delivery, DeliveryPhase, Mailbox, NetworkContext, ScheduledMessage, Transmission,
};
#[cfg(verus_only)]
use super::{
    Envelope, MessageEndpoint, MessageType, Messages, PluginRandom, Publication, StepTime,
};
use super::{NetworkTrace, TruthContacts};
#[cfg(verus_only)]
use crate::pubsub::messages::proof::{axioms::payload_type, left, queued, taken};
use vstd::prelude::*;

verus! {

/// Declares the truth contact list as a value with no visible structure; the
/// claims only say a link carries the one the context holds.
#[allow(dead_code)]
#[verifier::external_type_specification]
#[verifier::external_body]
pub struct ExTruthContacts<'a>(TruthContacts<'a>);

/// Declares the trace as a value with no visible structure.
#[allow(dead_code)]
#[verifier::external_type_specification]
#[verifier::external_body]
pub(crate) struct ExNetworkTrace(NetworkTrace);

/// The facts a routing proof needs, so routing.rs names one group.
#[allow(dead_code)]
pub broadcast group routing_facts {
    crate::proof::string_key_facts,
    vstd::seq_lib::group_seq_lib_default,
    vstd::seq_lib::group_seq_properties,
    crate::pubsub::messages::proof::axioms::channel_obeys_cmp,
    crate::pubsub::messages::proof::axioms::endpoint_obeys_cmp,
    axioms::time_add_is_ieee,
    axioms::delivery_le_is_ieee,
    axioms::ge_is_ieee,
    slice_len_bounded,
    subscribers_within,
    index_step,
    links_step,
    offers_step,
    deliveries_step,
}

/// A slice's view has at most `usize::MAX` elements, so a count over one
/// cannot overflow.
pub broadcast proof fn slice_len_bounded<T>(slice: &[T])
    ensures #[trigger] slice@.len() <= usize::MAX
{
    broadcast use vstd::slice::axiom_spec_len;
    assert(vstd::slice::spec_slice_len(slice) == slice@.len());
}

/// The loops' iterators, as the invariants see them.
#[cfg(verus_only)]
pub(crate) type MailboxWalk<'a, 'm> = vstd::std_specs::iter::VerusForLoopWrapper<core::slice::IterMut<'a, Mailbox<'m>>>;
#[cfg(verus_only)]
pub(crate) type Count = vstd::std_specs::iter::VerusForLoopWrapper<core::ops::Range<usize>>;
#[cfg(verus_only)]
pub(crate) type SubscriptionWalk<'a> = vstd::std_specs::iter::VerusForLoopWrapper<std::collections::btree_map::Iter<'a, Channel, MessageType>>;
#[cfg(verus_only)]
pub(crate) type PublicationWalk<'a, 'm> = vstd::std_specs::iter::VerusForLoopWrapper<core::slice::Iter<'a, (&'m MessageEndpoint, Publication)>>;
#[cfg(verus_only)]
pub(crate) type ReceiverWalk<'a> = vstd::std_specs::iter::VerusForLoopWrapper<core::slice::Iter<'a, (usize, MessageType)>>;
/// The subscriber index, as a map.
#[cfg(verus_only)]
pub(crate) type SubscriberMap<'a> = Map<(Option<i32>, &'a Channel), Vec<(usize, MessageType)>>;

// ---- The publications.

/// The messages each mailbox holds.
pub open(crate) spec fn messages_of(mailboxes: Seq<Mailbox>) -> Seq<Messages> {
    Seq::new(mailboxes.len(), |j: int| *mailboxes[j].messages)
}

/// The mailboxes' end values.
#[verifier::prophetic]
pub open(crate) spec fn finals<'m>(mailboxes: Seq<&mut Mailbox<'m>>) -> Seq<Mailbox<'m>> {
    Seq::new(mailboxes.len(), |j: int| *final(mailboxes[j]))
}

/// The network's publications of the first `k` mailboxes, in mailbox order,
/// each with its sender.
pub open(crate) spec fn gathered(mailboxes: Seq<Mailbox<'_>>, network: Seq<char>, k: nat) -> Seq<(&MessageEndpoint, Publication)>
    decreases k
{
    if k == 0 {
        Seq::empty()
    } else {
        let last = (k - 1) as int;
        gathered(mailboxes, network, (k - 1) as nat) + taken(mailboxes[last].messages.outbox@, network, mailboxes[last].endpoint, mailboxes[last].messages.outbox@.len() as int)
    }
}

/// The mailbox gave up the network's publications, kept the others in order,
/// and kept its endpoint.
pub open(crate) spec fn gave_up(before: Mailbox, after: Mailbox, network: &str) -> bool {
    &&& after.endpoint == before.endpoint
    &&& (*after.messages).outbox@ == left((*before.messages).outbox@, network@, (*before.messages).outbox@.len() as int)
    &&& *after.messages == Messages { outbox: after.messages.outbox, ..*before.messages }
}

/// Partway through the mailboxes: the ones so far gave up the network's
/// publications, which are the publications gathered in order, and the rest
/// are as they were.
#[verifier::prophetic]
pub open(crate) spec fn collected_so_far(before: Seq<Mailbox>, it: MailboxWalk, network: &str, publications: Seq<(&MessageEndpoint, Publication)>) -> bool {
    let mailboxes = it.seq();
    let count = it.index();
    let after = finals(mailboxes);
    &&& mailboxes.len() == before.len()
    &&& forall|j: int| #![trigger mailboxes[j]] 0 <= j < mailboxes.len() ==> after[j] == *final(mailboxes[j])
    &&& forall|j: int| #![trigger mailboxes[j]] 0 <= j < count ==> gave_up(before[j], after[j], network)
    &&& publications == gathered(before, network@, count as nat)
    &&& forall|j: int| #![trigger mailboxes[j]] count <= j < before.len() ==> *mailboxes[j] == before[j]
}

/// `collect_publications` took the network's publications from every mailbox,
/// in mailbox order, each with its sender, and changed nothing else.
pub open(crate) spec fn collected(before: Seq<Mailbox>, after: Seq<Mailbox>, network: &str, publications: Seq<(&MessageEndpoint, Publication)>) -> bool {
    &&& after.len() == before.len()
    &&& forall|j: int| #![trigger after[j]] 0 <= j < before.len() ==> gave_up(before[j], after[j], network)
    &&& publications == gathered(before, network@, before.len())
}

// ---- The subscriber index.

/// The group a mailbox routes in: its entity when routing within entities,
/// else none.
pub open(crate) spec fn group_of(mailbox: Mailbox, within_entity: bool) -> Option<i32> {
    if within_entity { mailbox.endpoint.entity_id } else { None }
}

/// The subscribers of the channel in the group among the first `k` mailboxes,
/// in mailbox order: each mailbox's index and the type it expects.
pub open(crate) spec fn subscribers_of(mailboxes: Seq<Mailbox>, within_entity: bool, group: Option<i32>, channel: Channel, k: nat) -> Seq<(usize, MessageType)>
    decreases k
{
    if k == 0 {
        Seq::empty()
    } else {
        let last = (k - 1) as int;
        let subscriptions = (*mailboxes[last].messages).subscriptions@;
        let so_far = subscribers_of(mailboxes, within_entity, group, channel, (k - 1) as nat);
        if group_of(mailboxes[last], within_entity) == group && subscriptions.contains_key(channel) {
            so_far.push((last as usize, subscriptions[channel]))
        } else {
            so_far
        }
    }
}

/// Every subscriber listed names a mailbox among the first `k`.
pub(crate) broadcast proof fn subscribers_within(mailboxes: Seq<Mailbox>, within_entity: bool, group: Option<i32>, channel: Channel, k: nat)
    requires k <= usize::MAX
    ensures
        #![trigger subscribers_of(mailboxes, within_entity, group, channel, k)]
        forall|j: int| 0 <= j < subscribers_of(mailboxes, within_entity, group, channel, k).len()
            ==> (#[trigger] subscribers_of(mailboxes, within_entity, group, channel, k)[j]).0 < k
    decreases k
{
    if k > 0 {
        let last = (k - 1) as nat;
        subscribers_within(mailboxes, within_entity, group, channel, last);
        let so_far = subscribers_of(mailboxes, within_entity, group, channel, last);
        let now = subscribers_of(mailboxes, within_entity, group, channel, k);
        assert forall|j: int| 0 <= j < now.len() implies (#[trigger] now[j]).0 < k by {
            if j < so_far.len() {
                assert(now[j] == so_far[j]);
            } else {
                assert(now[j] == ((k - 1) as usize, (*mailboxes[k - 1].messages).subscriptions@[channel]));
            }
        }
    }
}

/// The subscribers name mailboxes that exist.
pub open(crate) spec fn receivers_within(receivers: Seq<(usize, MessageType)>, mailboxes: Seq<Mailbox>) -> bool {
    forall|j: int| 0 <= j < receivers.len() ==> (#[trigger] receivers[j]).0 < mailboxes.len()
}

/// The index's list for the group and channel; none when it has no entry.
pub open(crate) spec fn listed(subscribers: SubscriberMap, group: Option<i32>, channel: Channel) -> Seq<(usize, MessageType)> {
    if subscribers.contains_key((group, &channel)) { subscribers[(group, &channel)]@ } else { Seq::empty() }
}

/// `index_subscribers` lists, for every group and channel, the subscribers in
/// mailbox order.
pub open(crate) spec fn indexed(mailboxes: Seq<Mailbox>, within_entity: bool, subscribers: SubscriberMap) -> bool {
    forall|group: Option<i32>, channel: Channel|
        #![trigger listed(subscribers, group, channel)]
        #![trigger subscribers.contains_key((group, &channel))]
        #![trigger subscribers_of(mailboxes, within_entity, group, channel, mailboxes.len())]
        listed(subscribers, group, channel) == subscribers_of(mailboxes, within_entity, group, channel, mailboxes.len())
}

/// Partway through the mailboxes: the index lists the subscribers among the
/// ones so far.
#[verifier::prophetic]
pub open(crate) spec fn indexed_so_far(mailboxes: Seq<Mailbox>, within_entity: bool, subscribers: SubscriberMap, it: Count) -> bool {
    &&& 0 <= it.index() <= mailboxes.len()
    &&& forall|group: Option<i32>, channel: Channel| #[trigger] listed(subscribers, group, channel) == subscribers_of(mailboxes, within_entity, group, channel, it.index() as nat)
}

/// `index_subscriptions` appended the mailbox, as `index`, to the list of each
/// channel it subscribes to in its group, and changed no other list.
pub open(crate) spec fn mailbox_indexed(before: SubscriberMap, after: SubscriberMap, index: usize, mailbox: &Mailbox, within_entity: bool) -> bool {
    let subscriptions = (*mailbox.messages).subscriptions@;
    forall|group: Option<i32>, channel: Channel| #[trigger] listed(after, group, channel) == (
        if group == group_of(*mailbox, within_entity) && subscriptions.contains_key(channel) { listed(before, group, channel).push((index, subscriptions[channel])) } else { listed(before, group, channel) })
}

/// The channel is among the first `k` entries walked.
pub open(crate) spec fn walked_by(entries: Seq<(&Channel, &MessageType)>, k: int, channel: Channel) -> bool {
    exists|j: int| 0 <= j < k && *(#[trigger] entries[j]).0 == channel
}

/// The entries a walk of the subscriptions yields: each an entry of the map,
/// no entry twice, and every entry once.
pub open(crate) spec fn walks(entries: Seq<(&Channel, &MessageType)>, subscriptions: Map<Channel, MessageType>) -> bool {
    &&& forall|j: int| 0 <= j < entries.len() ==> subscriptions.contains_key(*(#[trigger] entries[j]).0) && subscriptions[*entries[j].0] == *entries[j].1
    &&& entries.no_duplicates()
    &&& forall|channel: Channel| #[trigger] subscriptions.contains_key(channel) ==> exists|j: int| 0 <= j < entries.len() && *(#[trigger] entries[j]).0 == channel
}

/// After the first `k` entries: the channels walked have the mailbox appended
/// in its group, and no other list changed.
pub open(crate) spec fn index_so_far(before: SubscriberMap, now: SubscriberMap, index: usize, entity: Option<i32>, subscriptions: Map<Channel, MessageType>, entries: Seq<(&Channel, &MessageType)>, k: int) -> bool {
    &&& 0 <= k <= entries.len()
    &&& walks(entries, subscriptions)
    &&& forall|group: Option<i32>, channel: Channel| #[trigger] listed(now, group, channel) == (
        if group == entity && walked_by(entries, k, channel) { listed(before, group, channel).push((index, subscriptions[channel])) } else { listed(before, group, channel) })
}

/// Partway through a mailbox's subscriptions.
#[verifier::prophetic]
pub open(crate) spec fn subscriptions_so_far(before: SubscriberMap, now: SubscriberMap, index: usize, entity: Option<i32>, subscriptions: Map<Channel, MessageType>, it: SubscriptionWalk) -> bool {
    index_so_far(before, now, index, entity, subscriptions, it.seq(), it.index())
}

/// `add_subscriber` appended the subscriber to the list under `key` and
/// changed no other list. Stated with `=~=`: inside a predicate, `==` on a map
/// or sequence is not extensional, and these facts come from `push` and `insert`.
pub open(crate) spec fn subscriber_added(before: SubscriberMap, after: SubscriberMap, key: (Option<i32>, &Channel), subscriber: (usize, MessageType)) -> bool {
    &&& after =~= before.insert(key, after[key])
    &&& after[key]@ =~= listed(before, key.0, *key.1).push(subscriber)
}

/// Adding entry `k` to its list, in the mailbox's group, is one more step.
pub(crate) broadcast proof fn index_step(before: SubscriberMap, now: SubscriberMap, after: SubscriberMap, index: usize, entity: Option<i32>, subscriptions: Map<Channel, MessageType>, entries: Seq<(&Channel, &MessageType)>, k: int)
    requires
        index_so_far(before, now, index, entity, subscriptions, entries, k),
        k < entries.len(),
        subscriber_added(now, after, (entity, entries[k].0), (index, *entries[k].1)),
    ensures
        #![trigger index_so_far(before, after, index, entity, subscriptions, entries, k + 1), index_so_far(before, now, index, entity, subscriptions, entries, k)]
        index_so_far(before, after, index, entity, subscriptions, entries, k + 1),
{
    let channel = *entries[k].0;
    assert(!walked_by(entries, k, channel)) by {
        if walked_by(entries, k, channel) {
            let j = choose|j: int| 0 <= j < k && *(#[trigger] entries[j]).0 == channel;
            assert(*entries[j].1 == subscriptions[channel]);
            assert(*entries[k].1 == subscriptions[channel]);
            assert(entries[j] == entries[k]);
        }
    }
    assert forall|group: Option<i32>, c: Channel| #[trigger] listed(after, group, c) == (
        if group == entity && walked_by(entries, k + 1, c) { listed(before, group, c).push((index, subscriptions[c])) } else { listed(before, group, c) }) by {
        if group == entity && c == channel {
            assert(after.contains_key((group, &c)));
            assert(walked_by(entries, k + 1, c));
        } else {
            assert((group, &c) != (entity, entries[k].0));
            assert(listed(after, group, c) == listed(now, group, c));
            if c != channel {
                assert(walked_by(entries, k + 1, c) == walked_by(entries, k, c)) by {
                    if walked_by(entries, k + 1, c) {
                        let j = choose|j: int| 0 <= j < k + 1 && *(#[trigger] entries[j]).0 == c;
                        assert(j < k);
                    }
                }
            }
        }
    }
}

// ---- The offers.

/// A publication's stream: the sender's entity and the topic. Messages are
/// numbered within it.
pub open(crate) spec fn stream(publication: (&MessageEndpoint, Publication)) -> (Option<i32>, Seq<char>) {
    (publication.0.entity_id, publication.1.channel.1@)
}

/// How many of the first `k` publications are in the stream.
pub open(crate) spec fn in_stream(publications: Seq<(&MessageEndpoint, Publication)>, s: (Option<i32>, Seq<char>), k: nat) -> nat
    decreases k
{
    if k == 0 { 0 } else { in_stream(publications, s, (k - 1) as nat) + if stream(publications[k - 1]) == s { 1nat } else { 0nat } }
}

/// The publication's number in its stream: how many came before it.
pub open(crate) spec fn sequence_of(publications: Seq<(&MessageEndpoint, Publication)>, k: int) -> nat {
    in_stream(publications, stream(publications[k]), k as nat)
}

/// The group a publication is offered in, or none when a world plugin's
/// publication is routed within entities.
pub open(crate) spec fn group_for(sender: &MessageEndpoint, within_entity: bool) -> Option<Option<i32>> {
    if !within_entity { Some(None) } else if sender.entity_id is Some { Some(sender.entity_id) } else { None }
}

/// The link offered to one subscriber: this phase's time, the sender, the
/// receiver, the publication's topic, and its number in the stream.
pub open(crate) spec fn link_for(link: Transmission, sender: &MessageEndpoint, receiver: &MessageEndpoint, publication: &Publication, sequence: nat, time: StepTime) -> bool {
    &&& link.time == time
    &&& link.sender == sender
    &&& link.receiver == receiver
    &&& link.topic@ == publication.channel.1@
    &&& link.sequence == sequence
}

/// One link offered: the subscriber expects the publication's type, the
/// network decided the link, and an accepted link scheduled its message
/// `delay_s` after now.
pub open(crate) spec fn link_offered(sender: &MessageEndpoint, publication: &Publication, sequence: nat, receiver: (usize, MessageType), mailboxes: Seq<Mailbox>, time: StepTime, random: &PluginRandom, before: Seq<ScheduledMessage>, after: Seq<ScheduledMessage>) -> bool {
    &&& receiver.0 < mailboxes.len()
    &&& receiver.1 == payload_type(publication.payload)
    &&& exists|link: Transmission, decision: Delivery| {
        &&& link_for(link, sender, mailboxes[receiver.0 as int].endpoint, publication, sequence, time)
        &&& #[trigger] axioms::decided(&link, random, Some(decision))
        &&& match decision {
            Delivery::Drop => after == before,
            Delivery::After { delay_s } => {
                &&& crate::proof::is_finite(delay_s) && delay_s >= 0.0f64 && crate::proof::is_finite(time.time_s + delay_s)
                &&& after == before.push(scheduled_message(&link, publication, time.time_s + delay_s))
            }
        }
    }
}

/// The first `k` subscribers were offered the publication in order. The
/// trigger is the step's offer: the recursive call is encoded at a lower fuel
/// than any term a caller supplies, so it never matches.
pub open(crate) spec fn links_ran(sender: &MessageEndpoint, publication: &Publication, sequence: nat, receivers: Seq<(usize, MessageType)>, mailboxes: Seq<Mailbox>, time: StepTime, random: &PluginRandom, before: Seq<ScheduledMessage>, after: Seq<ScheduledMessage>, k: nat) -> bool
    decreases k
{
    if k == 0 {
        after == before
    } else {
        let last = (k - 1) as nat;
        exists|mid: Seq<ScheduledMessage>| links_ran(sender, publication, sequence, receivers, mailboxes, time, random, before, mid, last)
            && #[trigger] link_offered(sender, publication, sequence, receivers[last as int], mailboxes, time, random, mid, after)
    }
}

/// `offer_to_subscribers` offered the publication to every subscriber in
/// order, or failed after offering it to the first `j`.
#[verifier::prophetic]
pub open(crate) spec fn links_offered(sender: &MessageEndpoint, publication: &Publication, sequence: nat, receivers: Seq<(usize, MessageType)>, mailboxes: Seq<Mailbox>, random: &PluginRandom, before: &DeliveryPhase, after: &DeliveryPhase, r: &anyhow::Result<()>) -> bool {
    let time = before.time;
    &&& same_phase(before, after)
    &&& r is Ok ==> links_ran(sender, publication, sequence, receivers, mailboxes, time, random, (*before.scheduled)@, (*after.scheduled)@, receivers.len())
    &&& r is Err ==> exists|j: nat| j < receivers.len() && links_ran(sender, publication, sequence, receivers, mailboxes, time, random, (*before.scheduled)@, (*after.scheduled)@, j)
}

/// Partway through the subscribers: the ones so far were offered the publication.
#[verifier::prophetic]
pub open(crate) spec fn links_so_far(sender: &MessageEndpoint, publication: &Publication, sequence: nat, receivers: Seq<(usize, MessageType)>, mailboxes: Seq<Mailbox>, random: &PluginRandom, before: &DeliveryPhase, now: &DeliveryPhase, it: ReceiverWalk) -> bool {
    let time = before.time;
    &&& same_phase(before, now)
    &&& it.seq().len() == receivers.len()
    &&& forall|j: int| 0 <= j < receivers.len() ==> *(#[trigger] it.seq()[j]) == receivers[j]
    &&& forall|j: int| 0 <= j < receivers.len() ==> (#[trigger] receivers[j]).0 < mailboxes.len()
    &&& 0 <= it.index() <= receivers.len()
    &&& links_ran(sender, publication, sequence, receivers, mailboxes, time, random, (*before.scheduled)@, (*now.scheduled)@, it.index() as nat)
}

/// One publication offered: skipped when it has no group, else offered to its
/// channel's subscribers in its group, in mailbox order.
pub open(crate) spec fn publication_offered(publication: (&MessageEndpoint, Publication), sequence: nat, mailboxes: Seq<Mailbox>, within_entity: bool, time: StepTime, random: &PluginRandom, before: Seq<ScheduledMessage>, after: Seq<ScheduledMessage>) -> bool {
    match group_for(publication.0, within_entity) {
        None => after == before,
        Some(group) => {
            let receivers = subscribers_of(mailboxes, within_entity, group, publication.1.channel, mailboxes.len());
            links_ran(publication.0, &publication.1, sequence, receivers, mailboxes, time, random, before, after, receivers.len())
        }
    }
}

/// The first `k` publications were offered in order, each numbered in its stream.
pub open(crate) spec fn offers_ran(publications: Seq<(&MessageEndpoint, Publication)>, mailboxes: Seq<Mailbox>, within_entity: bool, time: StepTime, random: &PluginRandom, before: Seq<ScheduledMessage>, after: Seq<ScheduledMessage>, k: nat) -> bool
    decreases k
{
    if k == 0 {
        after == before
    } else {
        let last = (k - 1) as nat;
        exists|mid: Seq<ScheduledMessage>| offers_ran(publications, mailboxes, within_entity, time, random, before, mid, last)
            && #[trigger] publication_offered(publications[last as int], sequence_of(publications, last as int), mailboxes, within_entity, time, random, mid, after)
    }
}

/// `offer_publications` offered every publication in order, or failed partway
/// through one after offering the ones before it.
#[verifier::prophetic]
pub open(crate) spec fn offered(publications: Seq<(&MessageEndpoint, Publication)>, mailboxes: Seq<Mailbox>, within_entity: bool, random: &PluginRandom, before: &DeliveryPhase, after: &DeliveryPhase, r: &anyhow::Result<()>) -> bool {
    let time = before.time;
    &&& same_phase(before, after)
    &&& r is Ok ==> offers_ran(publications, mailboxes, within_entity, time, random, (*before.scheduled)@, (*after.scheduled)@, publications.len())
    &&& r is Err ==> offer_failed(publications, mailboxes, within_entity, time, random, (*before.scheduled)@, (*after.scheduled)@)
}

/// Publication `k` failed in an offer: the ones before it were offered in
/// order, and the schedule is what the links offered so far left.
pub open(crate) spec fn offer_failed(publications: Seq<(&MessageEndpoint, Publication)>, mailboxes: Seq<Mailbox>, within_entity: bool, time: StepTime, random: &PluginRandom, before: Seq<ScheduledMessage>, after: Seq<ScheduledMessage>) -> bool {
    exists|k: nat, j: nat, mid: Seq<ScheduledMessage>, receivers: Seq<(usize, MessageType)>, sequence: nat|
        #![trigger offers_ran(publications, mailboxes, within_entity, time, random, before, mid, k),
            links_ran(publications[k as int].0, &publications[k as int].1, sequence, receivers, mailboxes, time, random, mid, after, j)]
    {
        &&& k < publications.len()
        &&& offers_ran(publications, mailboxes, within_entity, time, random, before, mid, k)
        &&& group_for(publications[k as int].0, within_entity) is Some
        &&& receivers == subscribers_of(mailboxes, within_entity, group_for(publications[k as int].0, within_entity)->Some_0, publications[k as int].1.channel, mailboxes.len())
        &&& sequence == sequence_of(publications, k as int)
        &&& links_ran(publications[k as int].0, &publications[k as int].1, sequence, receivers, mailboxes, time, random, mid, after, j)
    }
}

/// Partway through the publications: the ones so far were offered, the index
/// lists every subscriber, and the stream counts are of the publications so far.
#[verifier::prophetic]
pub open(crate) spec fn offers_so_far(publications: Seq<(&MessageEndpoint, Publication)>, mailboxes: Seq<Mailbox>, within_entity: bool, random: &PluginRandom, before: &DeliveryPhase, now: &DeliveryPhase, subscribers: SubscriberMap, sequences: Map<(Option<i32>, String), u64>, it: PublicationWalk) -> bool {
    let count = it.index();
    let time = before.time;
    &&& same_phase(before, now)
    &&& it.seq().len() == publications.len()
    &&& forall|j: int| 0 <= j < publications.len() ==> *(#[trigger] it.seq()[j]) == publications[j]
    &&& 0 <= count <= publications.len()
    &&& indexed(mailboxes, within_entity, subscribers)
    &&& forall|s: (Option<i32>, String)| #[trigger] sequences.contains_key(s) <==> in_stream(publications, (s.0, s.1@), count as nat) > 0
    &&& forall|s: (Option<i32>, String)| #[trigger] sequences.contains_key(s) ==> sequences[s] == in_stream(publications, (s.0, s.1@), count as nat) && sequences[s] <= count
    &&& offers_ran(publications, mailboxes, within_entity, time, random, (*before.scheduled)@, (*now.scheduled)@, count as nat)
}

// ---- The deliveries.

/// Mailbox `index` is the first with the endpoint.
pub open(crate) spec fn first_with(mailboxes: Seq<Mailbox>, endpoint: &MessageEndpoint, index: int) -> bool {
    &&& 0 <= index < mailboxes.len()
    &&& *mailboxes[index].endpoint == *endpoint
    &&& forall|j: int| 0 <= j < index ==> *(#[trigger] mailboxes[j]).endpoint != *endpoint
}

/// `receivers_by_endpoint` maps each endpoint with a mailbox to its first mailbox.
pub open(crate) spec fn receivers_indexed(mailboxes: Seq<Mailbox>, receivers: Map<&MessageEndpoint, usize>) -> bool {
    &&& forall|endpoint: &MessageEndpoint| #[trigger] receivers.contains_key(endpoint) ==> first_with(mailboxes, endpoint, receivers[endpoint] as int)
    &&& forall|j: int| 0 <= j < mailboxes.len() ==> receivers.contains_key((#[trigger] mailboxes[j]).endpoint)
}

/// Partway through the mailboxes: the ones so far are indexed.
#[verifier::prophetic]
pub open(crate) spec fn receivers_so_far(mailboxes: Seq<Mailbox>, receivers: Map<&MessageEndpoint, usize>, it: Count) -> bool {
    &&& 0 <= it.index() <= mailboxes.len()
    &&& forall|endpoint: &MessageEndpoint| #[trigger] receivers.contains_key(endpoint) ==> first_with(mailboxes, endpoint, receivers[endpoint] as int) && receivers[endpoint] < it.index()
    &&& forall|j: int| 0 <= j < it.index() ==> receivers.contains_key((#[trigger] mailboxes[j]).endpoint)
}

/// The same messages, mailbox by mailbox.
pub open(crate) spec fn same_messages(a: Seq<Messages>, b: Seq<Messages>) -> bool {
    a.len() == b.len() && forall|j: int| 0 <= j < a.len() ==> a[j] == b[j]
}

/// One message in flight was routed: with no mailbox for its receiver it was
/// dropped; due, it was queued on the receiver's first mailbox, stamped with
/// now; not yet due, it was kept.
pub open(crate) spec fn message_routed(message: ScheduledMessage, time: StepTime, mailboxes: Seq<Mailbox>, messages_before: Seq<Messages>, messages_after: Seq<Messages>, kept_before: Seq<ScheduledMessage>, kept_after: Seq<ScheduledMessage>) -> bool {
    &&& messages_after.len() == messages_before.len() == mailboxes.len()
    &&& (forall|j: int| 0 <= j < mailboxes.len() ==> *(#[trigger] mailboxes[j]).endpoint != message.receiver)
        ==> same_messages(messages_after, messages_before) && kept_after == kept_before
    &&& forall|index: int| #[trigger] first_with(mailboxes, &message.receiver, index) ==> {
        &&& due(&message, time) ==> {
            &&& queued(&messages_before[index], &messages_after[index], &message.channel, &stamped(message.envelope, time.time_s))
            &&& forall|j: int| 0 <= j < mailboxes.len() && j != index ==> messages_after[j] == #[trigger] messages_before[j]
            &&& kept_after == kept_before
        }
        &&& !due(&message, time) ==> same_messages(messages_after, messages_before) && kept_after == kept_before.push(message)
    }
}

/// The first `k` messages in flight were routed in order.
pub open(crate) spec fn deliveries_ran(in_flight: Seq<ScheduledMessage>, time: StepTime, mailboxes: Seq<Mailbox>, messages_before: Seq<Messages>, messages_after: Seq<Messages>, kept_before: Seq<ScheduledMessage>, kept_after: Seq<ScheduledMessage>, k: nat) -> bool
    decreases k
{
    if k == 0 {
        same_messages(messages_after, messages_before) && kept_after == kept_before
    } else {
        let last = (k - 1) as nat;
        exists|messages_mid: Seq<Messages>, kept_mid: Seq<ScheduledMessage>|
            deliveries_ran(in_flight, time, mailboxes, messages_before, messages_mid, kept_before, kept_mid, last)
            && #[trigger] message_routed(in_flight[last as int], time, mailboxes, messages_mid, messages_after, kept_mid, kept_after)
    }
}

/// The mailboxes keep their endpoints.
pub open(crate) spec fn same_endpoints(before: Seq<Mailbox>, after: Seq<Mailbox>) -> bool {
    after.len() == before.len() && forall|j: int| 0 <= j < before.len() ==> (#[trigger] after[j]).endpoint == before[j].endpoint
}

/// `deliver_scheduled` routed every scheduled message in order: the schedule
/// is what was kept, and each mailbox's messages are what was queued on it.
#[verifier::prophetic]
pub open(crate) spec fn all_delivered(before: &DeliveryPhase, after: &DeliveryPhase, mailboxes_before: Seq<Mailbox>, mailboxes_after: Seq<Mailbox>) -> bool {
    &&& same_phase(before, after)
    &&& same_endpoints(mailboxes_before, mailboxes_after)
    &&& deliveries_ran((*before.scheduled)@, before.time, mailboxes_before, messages_of(mailboxes_before), messages_of(mailboxes_after), Seq::empty(), (*after.scheduled)@, (*before.scheduled)@.len())
}

/// Partway through the messages in flight: the ones so far were routed.
#[verifier::prophetic]
pub open(crate) spec fn deliveries_so_far(in_flight: Seq<ScheduledMessage>, before: &DeliveryPhase, now: &DeliveryPhase, mailboxes_before: Seq<Mailbox>, mailboxes: &[Mailbox], receivers: Map<&MessageEndpoint, usize>, count: int) -> bool {
    &&& same_phase(before, now)
    &&& same_endpoints(mailboxes_before, mailboxes@)
    &&& receivers_indexed(mailboxes_before, receivers)
    &&& 0 <= count <= in_flight.len()
    &&& deliveries_ran(in_flight, before.time, mailboxes_before, messages_of(mailboxes_before), messages_of(mailboxes@), Seq::empty(), (*now.scheduled)@, count as nat)
}

/// One more subscriber offered extends the chain. Stated for the loop, whose
/// step adds one to the count it carries.
pub(crate) broadcast proof fn links_step(sender: &MessageEndpoint, publication: &Publication, sequence: nat, receivers: Seq<(usize, MessageType)>, mailboxes: Seq<Mailbox>, time: StepTime, random: &PluginRandom, before: Seq<ScheduledMessage>, mid: Seq<ScheduledMessage>, after: Seq<ScheduledMessage>, k: nat)
    requires
        k < receivers.len(),
        links_ran(sender, publication, sequence, receivers, mailboxes, time, random, before, mid, k),
        link_offered(sender, publication, sequence, receivers[k as int], mailboxes, time, random, mid, after),
    ensures
        #![trigger links_ran(sender, publication, sequence, receivers, mailboxes, time, random, before, after, k + 1), links_ran(sender, publication, sequence, receivers, mailboxes, time, random, before, mid, k)]
        links_ran(sender, publication, sequence, receivers, mailboxes, time, random, before, after, k + 1),
{
    assert(links_ran(sender, publication, sequence, receivers, mailboxes, time, random, before, mid, ((k + 1) - 1) as nat));
    assert(link_offered(sender, publication, sequence, receivers[(k + 1) - 1], mailboxes, time, random, mid, after));
}

/// One more publication offered extends the chain.
pub(crate) broadcast proof fn offers_step(publications: Seq<(&MessageEndpoint, Publication)>, mailboxes: Seq<Mailbox>, within_entity: bool, time: StepTime, random: &PluginRandom, before: Seq<ScheduledMessage>, mid: Seq<ScheduledMessage>, after: Seq<ScheduledMessage>, k: nat)
    requires
        k < publications.len(),
        offers_ran(publications, mailboxes, within_entity, time, random, before, mid, k),
        publication_offered(publications[k as int], sequence_of(publications, k as int), mailboxes, within_entity, time, random, mid, after),
    ensures
        #![trigger offers_ran(publications, mailboxes, within_entity, time, random, before, after, k + 1), offers_ran(publications, mailboxes, within_entity, time, random, before, mid, k)]
        offers_ran(publications, mailboxes, within_entity, time, random, before, after, k + 1),
{
    assert(offers_ran(publications, mailboxes, within_entity, time, random, before, mid, ((k + 1) - 1) as nat));
    assert(publication_offered(publications[(k + 1) - 1], sequence_of(publications, (k + 1) - 1), mailboxes, within_entity, time, random, mid, after));
}

/// One more message routed extends the chain.
pub(crate) broadcast proof fn deliveries_step(in_flight: Seq<ScheduledMessage>, time: StepTime, mailboxes: Seq<Mailbox>, messages_before: Seq<Messages>, messages_mid: Seq<Messages>, messages_after: Seq<Messages>, kept_before: Seq<ScheduledMessage>, kept_mid: Seq<ScheduledMessage>, kept_after: Seq<ScheduledMessage>, k: nat)
    requires
        k < in_flight.len(),
        deliveries_ran(in_flight, time, mailboxes, messages_before, messages_mid, kept_before, kept_mid, k),
        message_routed(in_flight[k as int], time, mailboxes, messages_mid, messages_after, kept_mid, kept_after),
    ensures
        #![trigger deliveries_ran(in_flight, time, mailboxes, messages_before, messages_after, kept_before, kept_after, k + 1), deliveries_ran(in_flight, time, mailboxes, messages_before, messages_mid, kept_before, kept_mid, k)]
        deliveries_ran(in_flight, time, mailboxes, messages_before, messages_after, kept_before, kept_after, k + 1),
{
    assert(deliveries_ran(in_flight, time, mailboxes, messages_before, messages_mid, kept_before, kept_mid, ((k + 1) - 1) as nat));
    assert(message_routed(in_flight[(k + 1) - 1], time, mailboxes, messages_mid, messages_after, kept_mid, kept_after));
}

// ---- The phase.

/// The context's fields other than the mailboxes, the schedule, the trace, and
/// the routed flag are as they were.
pub open(crate) spec fn context_kept(before: &NetworkContext, after: &NetworkContext) -> bool {
    &&& after.time == before.time
    &&& after.contacts_truth == before.contacts_truth
    &&& after.name == before.name
    &&& after.endpoint == before.endpoint
    &&& after.random == before.random
}

/// The phase ran through: the network's publications were taken from every
/// mailbox in order, every one was offered to its channel's subscribers in
/// order with the network deciding each link, and the scheduled messages were
/// delivered or kept.
pub open(crate) spec fn routed_through(before: &NetworkContext, after: &NetworkContext, within_entity: bool) -> bool {
    exists|publications: Seq<(&MessageEndpoint, Publication)>, taken: Seq<Mailbox>, scheduled_mid: Seq<ScheduledMessage>|
        #![trigger collected((*before.mailboxes)@, taken, before.name, publications), offers_ran(publications, taken, within_entity, before.time, &before.random, (*before.scheduled)@, scheduled_mid, publications.len())]
    {
        &&& collected((*before.mailboxes)@, taken, before.name, publications)
        &&& offers_ran(publications, taken, within_entity, before.time, &before.random, (*before.scheduled)@, scheduled_mid, publications.len())
        &&& same_endpoints(taken, (*after.mailboxes)@)
        &&& deliveries_ran(scheduled_mid, before.time, taken, messages_of(taken), messages_of((*after.mailboxes)@), Seq::empty(), (*after.scheduled)@, scheduled_mid.len())
    }
}

/// The phase failed in an offer: the publications were taken, the ones before
/// it were offered, and the schedule is what the links offered so far left.
pub open(crate) spec fn routing_failed(before: &NetworkContext, after: &NetworkContext, within_entity: bool) -> bool {
    exists|publications: Seq<(&MessageEndpoint, Publication)>, taken: Seq<Mailbox>|
        #![trigger collected((*before.mailboxes)@, taken, before.name, publications)]
    {
        &&& collected((*before.mailboxes)@, taken, before.name, publications)
        &&& (*after.mailboxes)@ == taken
        &&& offer_failed(publications, taken, within_entity, before.time, &before.random, (*before.scheduled)@, (*after.scheduled)@)
    }
}

/// `route_links` routes once per phase, running it through or failing in an
/// offer; a second call in a phase fails and changes nothing.
pub open(crate) spec fn routed(before: &NetworkContext, after: &NetworkContext, within_entity: bool, r: &anyhow::Result<()>) -> bool {
    &&& before.routed ==> r is Err && *after == *before
    &&& !before.routed ==> {
        &&& after.routed
        &&& context_kept(before, after)
        &&& r is Ok ==> routed_through(before, after, within_entity)
        &&& r is Err ==> routing_failed(before, after, within_entity)
    }
}

/// The phase kept its time and writes the schedule it was lent: a helper
/// taking the phase by `&mut` must say so, or the context that lent the
/// schedule loses track of it.
#[verifier::prophetic]
pub open(crate) spec fn same_phase(before: &DeliveryPhase, after: &DeliveryPhase) -> bool {
    &&& after.time == before.time
    &&& *final(after.scheduled) == *final(before.scheduled)
}

/// The message a link schedules: to the link's receiver on the publication's
/// channel, carrying the sender, the sent time, the payload, and its delivery time.
pub open(crate) spec fn scheduled_message(link: &Transmission, publication: &Publication, delivered_at_s: f64) -> ScheduledMessage {
    ScheduledMessage {
        receiver: *link.receiver,
        channel: publication.channel,
        envelope: Envelope {
            sender: *link.sender,
            sent_at_s: publication.sent_at_s,
            delivered_at_s,
            payload: publication.payload,
        },
    }
}

/// `schedule` returned Ok exactly when the delay is finite and nonnegative and
/// the delivery time it gives is finite; then one message was scheduled for the
/// link, due `delay_s` after now, and otherwise nothing was.
#[verifier::prophetic]
pub open(crate) spec fn scheduled(before: &DeliveryPhase, after: &DeliveryPhase, link: &Transmission, publication: &Publication, delay_s: f64, r: &anyhow::Result<()>) -> bool {
    let delivered_at_s = before.time.time_s + delay_s;
    let valid = crate::proof::is_finite(delay_s) && delay_s >= 0.0f64 && crate::proof::is_finite(delivered_at_s);
    &&& same_phase(before, after)
    &&& (r is Ok) == valid
    &&& valid ==> (*after.scheduled)@ == (*before.scheduled)@.push(scheduled_message(link, publication, delivered_at_s))
    &&& !valid ==> (*after.scheduled)@ == (*before.scheduled)@
}

/// The message's time has come: its delivery time is at or before now, within rounding.
pub open(crate) spec fn due(message: &ScheduledMessage, time: StepTime) -> bool {
    message.envelope.delivered_at_s <= time.time_s + 1e-12f64
}

/// The envelope stamped with its actual delivery time.
pub open(crate) spec fn stamped(envelope: Envelope, time_s: f64) -> Envelope {
    Envelope { delivered_at_s: time_s, ..envelope }
}

/// `deliver` put a due message's envelope, stamped with now, on the receiver's
/// queue for its channel and nothing else, and left the schedule as it was; it
/// put a message not yet due back on the schedule and left the receiver as it was.
#[verifier::prophetic]
pub open(crate) spec fn delivered(before: &DeliveryPhase, after: &DeliveryPhase, message: &ScheduledMessage, receiver_before: &Messages, receiver_after: &Messages) -> bool {
    &&& same_phase(before, after)
    &&& due(message, before.time) ==> (*after.scheduled)@ == (*before.scheduled)@
        && queued(receiver_before, receiver_after, &message.channel, &stamped(message.envelope, before.time.time_s))
    &&& !due(message, before.time) ==> *receiver_after == *receiver_before
        && (*after.scheduled)@ == (*before.scheduled)@.push(*message)
}

/// Trusted: what routing clones, that `trace.rs` records, that the network is
/// asked about each link, and which float operations routing uses.
pub mod axioms {
    #[cfg(verus_only)]
    use super::super::{Delivery, NetworkTrace, PluginRandom, ScheduledMessage, StepTime, Transmission};
    #[cfg(verus_only)]
    use crate::pubsub::messages::Payload;
    use vstd::prelude::*;
    #[cfg(verus_only)]
    use vstd::std_specs::cmp::{ge_ensures, le_ensures};
    #[cfg(verus_only)]
    use vstd::std_specs::ops::add_ensures;

    verus! {

    /// Trusted: cloning a payload shares the same value.
    pub assume_specification[ <Payload as Clone>::clone ](p: &Payload) -> (r: Payload)
        ensures r == *p;

    /// Trusted: the trace takes its record of a scheduled message; the claims
    /// say nothing about the record.
    pub assume_specification[ NetworkTrace::scheduled ](t: &mut NetworkTrace, time_s: f64, delay_s: f64, message: &ScheduledMessage);

    /// Trusted: the same for a delivered message.
    pub assume_specification[ NetworkTrace::delivered ](t: &mut NetworkTrace, time_s: f64, message: &ScheduledMessage);

    /// What the network answered, if it did not fail.
    pub open(crate) spec fn outcome(r: &anyhow::Result<Delivery>) -> Option<Delivery> {
        match r {
            Ok(decision) => Some(*decision),
            Err(_) => None,
        }
    }

    /// The network was asked about the link and answered, or failed.
    pub uninterp spec fn decided(link: &Transmission, random: &PluginRandom, decision: Option<Delivery>) -> bool;

    /// Trusted: `decide_link` asks the network about the link; what it answers
    /// is the network's own.
    pub assume_specification<F: FnMut(&Transmission<'_>, &PluginRandom) -> anyhow::Result<Delivery>>[ super::super::decide_link::<F> ](decide: &mut F, link: &Transmission<'_>, random: &PluginRandom) -> (r: anyhow::Result<Delivery>)
        ensures decided(link, random, outcome(&r));

    /// Trusted: adding a delay to the phase time is IEEE addition. Stated over
    /// the time struct because a float field has no range fact (the plan's
    /// float row).
    #[verifier::external_body]
    pub broadcast proof fn time_add_is_ieee(time: StepTime, b: f64, o: f64)
        requires #[trigger] add_ensures(time.time_s, b, o)
        ensures o == time.time_s + b
    {}

    /// Trusted: comparing a scheduled message's delivery time is IEEE
    /// less-or-equal; over the message, for the same reason.
    #[verifier::external_body]
    pub(crate) broadcast proof fn delivery_le_is_ieee(message: ScheduledMessage, b: f64, o: bool)
        requires #[trigger] le_ensures(message.envelope.delivered_at_s, b, o)
        ensures o == (message.envelope.delivered_at_s <= b)
    {}

    /// Trusted: comparing two doubles is IEEE greater-or-equal.
    #[verifier::external_body]
    pub broadcast proof fn ge_is_ieee(a: f64, b: f64, o: bool)
        requires #[trigger] ge_ensures(a, b, o)
        ensures o == (a >= b)
    {}

    } // verus!
}

} // verus!

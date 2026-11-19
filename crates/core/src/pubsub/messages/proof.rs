//! What the mailbox functions claim, in words a reader of messages.rs can check.
#[cfg(verus_only)]
use super::{Channel, Messages};
#[cfg(verus_only)]
use super::{Envelope, Message, MessageEndpoint, Publication};
use super::{MessageType, Payload};
#[cfg(verus_only)]
use std::collections::BTreeMap;
use vstd::prelude::*;

verus! {

/// Declares `Payload` to Verus as a value with no visible structure.
#[allow(dead_code)]
#[verifier::external_type_specification]
#[verifier::external_body]
pub struct ExPayload(Payload);

/// Declares `MessageType` to Verus as a value with no visible structure.
#[allow(dead_code)]
#[verifier::external_type_specification]
#[verifier::external_body]
pub struct ExMessageType(MessageType);

/// Names serde's `Serialize` so a publish of a serializable value is a bound
/// Verus knows; no method is listed.
#[allow(dead_code)]
#[verifier::external_trait_specification]
pub trait ExSerialize {
    type ExternalTraitSpecificationFor: serde::Serialize;
}

/// The facts a mailbox proof needs, so messages.rs names one group.
#[allow(dead_code)]
pub broadcast group message_facts {
    crate::proof::string_key_facts,
    crate::proof::text_facts,
    vstd::seq_lib::group_seq_lib_default,
    vstd::set_lib::group_set_lib_default,
    axioms::channel_obeys_cmp,
    channel_texts_identify,
    taken_step,
    left_step,
    taken_start,
    left_start,
}

/// The entries of a channel-keyed map, by the network and topic texts.
pub open(crate) spec fn channels<V>(m: &BTreeMap<Channel, V>) -> Map<(Seq<char>, Seq<char>), V> {
    crate::proof::by_text(m@)
}

/// The messages waiting on a channel, in delivery order; none if the channel has no queue.
pub open(crate) spec fn queue(io: &Messages, channel: (Seq<char>, Seq<char>)) -> Seq<Envelope> {
    if channels(&io.inbox).contains_key(channel) { channels(&io.inbox)[channel]@ } else { Seq::empty() }
}

/// The mailbox subscribes to the channel and expects messages of type `T` on it.
pub open(crate) spec fn subscribed_as<T>(io: &Messages, channel: (Seq<char>, Seq<char>)) -> bool {
    let subscriptions = channels(&io.subscriptions);
    subscriptions.contains_key(channel) && subscriptions[channel] == axioms::message_type::<T>()
}

/// `subscribe` recorded the channel as carrying `T` and touched nothing else,
/// or returned an error and changed nothing, when a name is empty or the channel
/// was already subscribed with another type.
pub open(crate) spec fn subscribed<T>(before: &Messages, after: &Messages, network: &str, topic: &str, r: &Result<(), anyhow::Error>) -> bool {
    let channel = (network@, topic@);
    let subscriptions = channels(&before.subscriptions);
    let valid = network@.len() > 0 && topic@.len() > 0
        && (!subscriptions.contains_key(channel) || subscribed_as::<T>(before, channel));
    &&& (r is Ok) == valid
    &&& valid ==> {
        &&& channels(&after.subscriptions) == subscriptions.insert(channel, axioms::message_type::<T>())
        &&& after.inbox@ == before.inbox@
        &&& after.outbox@ == before.outbox@
        &&& after.time_s == before.time_s
    }
    &&& !valid ==> *after == *before
}

/// `message` is what `envelope` holds, read as a `T`: the same sender and times,
/// and the published value.
pub open(crate) spec fn opens_to<T>(envelope: &Envelope, message: &Message<T>) -> bool {
    &&& message.sender == envelope.sender
    &&& message.sent_at_s == envelope.sent_at_s
    &&& message.delivered_at_s == envelope.delivered_at_s
    &&& axioms::holds::<T>(envelope.payload)
    &&& message.value == axioms::contents::<T>(envelope.payload)
}

/// `open` returned the envelope's message exactly when its value is a `T`.
pub open(crate) spec fn opened<T>(envelope: &Envelope, r: &Result<Message<T>, anyhow::Error>) -> bool {
    &&& (r is Ok) == axioms::holds::<T>(envelope.payload)
    &&& r is Ok ==> opens_to::<T>(envelope, &r->Ok_0)
}

/// The first `count` envelopes of the queue have been opened, in order.
pub open(crate) spec fn opened_so_far<T>(queue: Seq<Envelope>, messages: &Vec<Message<T>>, count: int) -> bool {
    &&& messages@.len() == count
    &&& forall|i: int| #![trigger queue[i]] #![trigger messages@[i]] 0 <= i < count ==> opens_to::<T>(&queue[i], &messages@[i])
}

/// Every envelope in the queue holds a `T`.
pub open(crate) spec fn all_hold<T>(queue: Seq<Envelope>) -> bool {
    forall|i: int| 0 <= i < queue.len() ==> axioms::holds::<T>(#[trigger] queue[i].payload)
}

/// `open_all` returned the whole queue, opened in order, exactly when every
/// envelope holds a `T`.
pub open(crate) spec fn all_opened<T>(queue: &Vec<Envelope>, r: &Result<Vec<Message<T>>, anyhow::Error>) -> bool {
    &&& (r is Ok) == all_hold::<T>(queue@)
    &&& r is Ok ==> opened_so_far::<T>(queue@, &r->Ok_0, queue@.len() as int)
}

/// `receive` returned the asked channel's queue, opened in order, and removed
/// that queue and nothing else, exactly when the mailbox subscribes to the
/// channel as `T` and every queued envelope holds a `T`; without such a
/// subscription it returned an error and changed nothing.
pub open(crate) spec fn received<T>(before: &Messages, after: &Messages, network: &str, topic: &str, r: &Result<Vec<Message<T>>, anyhow::Error>) -> bool {
    let channel = (network@, topic@);
    &&& (r is Ok) == (subscribed_as::<T>(before, channel) && all_hold::<T>(queue(before, channel)))
    &&& !subscribed_as::<T>(before, channel) ==> *after == *before
    &&& r is Ok ==> {
        &&& subscribed_as::<T>(before, channel)
        &&& opened_so_far::<T>(queue(before, channel), &r->Ok_0, queue(before, channel).len() as int)
        &&& channels(&after.inbox) == channels(&before.inbox).remove(channel)
        &&& after.subscriptions@ == before.subscriptions@
        &&& after.outbox@ == before.outbox@
        &&& after.time_s == before.time_s
    }
}

/// `enqueue` appended the envelope to the channel's queue, added no other
/// queue, and changed nothing else.
pub open(crate) spec fn queued(before: &Messages, after: &Messages, channel: &Channel, envelope: &Envelope) -> bool {
    let text = (channel.0@, channel.1@);
    &&& queue(after, text) =~= queue(before, text).push(*envelope)
    &&& forall|other: (Seq<char>, Seq<char>)| other != text ==> #[trigger] queue(after, other) == queue(before, other)
    &&& channels(&after.inbox).dom() == channels(&before.inbox).dom().insert(text)
    &&& after.subscriptions@ == before.subscriptions@
    &&& after.outbox@ == before.outbox@
    &&& after.time_s == before.time_s
}

/// Two channels with the same network and topic texts are the same channel.
pub broadcast proof fn channel_texts_identify()
    ensures #[trigger] crate::proof::text_is_identity::<Channel>(),
{
    broadcast use crate::proof::string_text_is_identity;
    assert forall|a: Channel, b: Channel| #[trigger] a@ == #[trigger] b@ implies a == b by {
        assert(a.0@ == b.0@);
        assert(a.1@ == b.1@);
    }
}

/// The publication is of `payload` on the network's topic, sent at `sent_at_s`.
pub open(crate) spec fn publication_of(publication: &Publication, network: &str, topic: &str, sent_at_s: f64, payload: Payload) -> bool {
    &&& publication.channel.0@ == network@
    &&& publication.channel.1@ == topic@
    &&& publication.sent_at_s == sent_at_s
    &&& publication.payload == payload
}

/// The erased value is `value`.
pub open(crate) spec fn carries<T>(payload: Payload, value: T) -> bool {
    axioms::holds::<T>(payload) && *axioms::contents::<T>(payload) == value
}

/// `publish_engine_event` appended one publication of `value` on the network's
/// topic, sent at the mailbox's time, and changed nothing else.
pub open(crate) spec fn published<T>(before: &Messages, after: &Messages, network: &str, topic: &str, value: T) -> bool {
    &&& after.outbox@ == before.outbox@.push(after.outbox@.last())
    &&& publication_of(&after.outbox@.last(), network, topic, before.time_s, after.outbox@.last().payload)
    &&& carries::<T>(after.outbox@.last().payload, value)
    &&& *after == Messages { outbox: after.outbox, ..*before }
}

/// The publications on the network.
pub open(crate) spec fn on_network(network: Seq<char>) -> spec_fn(Publication) -> bool {
    |p: Publication| p.channel.0@ == network
}

/// The publications on other networks.
pub open(crate) spec fn off_network(network: Seq<char>) -> spec_fn(Publication) -> bool {
    |p: Publication| p.channel.0@ != network
}

/// A publication paired with its sender.
pub open(crate) spec fn from<'m>(sender: &'m MessageEndpoint) -> spec_fn(Publication) -> (&'m MessageEndpoint, Publication) {
    |p: Publication| (sender, p)
}

/// The network's publications among the first `count` of the outbox, in
/// order, each with the sender.
pub open(crate) spec fn taken<'m>(outbox: Seq<Publication>, network: Seq<char>, sender: &'m MessageEndpoint, count: int) -> Seq<(&'m MessageEndpoint, Publication)> {
    outbox.take(count).filter(on_network(network)).map_values(from(sender))
}

/// The other networks' publications among the first `count`, in order.
pub open(crate) spec fn left(outbox: Seq<Publication>, network: Seq<char>, count: int) -> Seq<Publication> {
    outbox.take(count).filter(off_network(network))
}

/// Partway through the outbox: the first `count` publications have been
/// split by network, the network's appended to `publications` and the others
/// kept in order.
#[verifier::prophetic]
pub open(crate) spec fn split_so_far(outbox: Seq<Publication>, network: Seq<char>, sender: &MessageEndpoint, publications_before: Seq<(&MessageEndpoint, Publication)>, publications: Seq<(&MessageEndpoint, Publication)>, others: Seq<Publication>, count: int) -> bool {
    &&& publications == publications_before + taken(outbox, network, sender, count)
    &&& others == left(outbox, network, count)
}

/// `take_publications` appended the outbox's publications on the network, in
/// order, each with the sender, kept the others in order, and changed nothing else.
pub open(crate) spec fn took_publications(before: &Messages, after: &Messages, network: &str, sender: &MessageEndpoint, publications_before: Seq<(&MessageEndpoint, Publication)>, publications: Seq<(&MessageEndpoint, Publication)>) -> bool {
    &&& publications == publications_before + taken(before.outbox@, network@, sender, before.outbox@.len() as int)
    &&& after.outbox@ == left(before.outbox@, network@, before.outbox@.len() as int)
    &&& *after == Messages { outbox: after.outbox, ..*before }
}

/// Taking one more publication extends what is taken when it is the
/// network's. Triggered by the two prefixes a loop step relates, since the
/// step's new index is not written `i + 1`.
pub broadcast proof fn taken_step(outbox: Seq<Publication>, network: Seq<char>, sender: &MessageEndpoint, i: int, j: int)
    requires 0 <= i < outbox.len(), j == i + 1,
    ensures #![trigger taken(outbox, network, sender, i), taken(outbox, network, sender, j)]
        taken(outbox, network, sender, j) == (if on_network(network)(outbox[i]) {
            taken(outbox, network, sender, i).push((sender, outbox[i]))
        } else {
            taken(outbox, network, sender, i)
        }),
{
    broadcast use vstd::seq_lib::group_seq_lib_default;
    reveal(Seq::filter);
    assert(outbox.take(j) == outbox.take(i).push(outbox[i]));
    assert(outbox.take(j).drop_last() =~= outbox.take(i));
    assert(outbox.take(j).last() == outbox[i]);
    if on_network(network)(outbox[i]) {
        assert(taken(outbox, network, sender, j) =~= taken(outbox, network, sender, i).push((sender, outbox[i])));
    }
}

/// Leaving one more publication extends what is left when it is another network's.
pub broadcast proof fn left_step(outbox: Seq<Publication>, network: Seq<char>, i: int, j: int)
    requires 0 <= i < outbox.len(), j == i + 1,
    ensures #![trigger left(outbox, network, i), left(outbox, network, j)]
        left(outbox, network, j) == (if off_network(network)(outbox[i]) {
            left(outbox, network, i).push(outbox[i])
        } else {
            left(outbox, network, i)
        }),
{
    broadcast use vstd::seq_lib::group_seq_lib_default;
    reveal(Seq::filter);
    assert(outbox.take(j) == outbox.take(i).push(outbox[i]));
    assert(outbox.take(j).drop_last() =~= outbox.take(i));
    assert(outbox.take(j).last() == outbox[i]);
}

/// Nothing taken is nothing. Triggered by any prefix, since a loop's starting
/// index is not written `0`.
pub broadcast proof fn taken_start(outbox: Seq<Publication>, network: Seq<char>, sender: &MessageEndpoint, i: int)
    requires i == 0,
    ensures #[trigger] taken(outbox, network, sender, i) == Seq::<(&MessageEndpoint, Publication)>::empty(),
{
    reveal(Seq::filter);
    assert(outbox.take(i) =~= Seq::<Publication>::empty());
    assert(taken(outbox, network, sender, i) =~= Seq::<(&MessageEndpoint, Publication)>::empty());
}

/// Nothing left is nothing.
pub broadcast proof fn left_start(outbox: Seq<Publication>, network: Seq<char>, i: int)
    requires i == 0,
    ensures #[trigger] left(outbox, network, i) == Seq::<Publication>::empty(),
{
    reveal(Seq::filter);
    assert(outbox.take(i) =~= Seq::<Publication>::empty());
}

/// Trusted: what the methods of `payload.rs` return, and how channels and
/// endpoints order.
pub mod axioms {
    #[cfg(verus_only)]
    use super::{Channel, MessageEndpoint, MessageType, Payload};
    use vstd::prelude::*;

    verus! {

    /// The type a subscription for `T` records.
    pub uninterp spec fn message_type<T>() -> MessageType;

    /// The erased value is a `T`.
    pub uninterp spec fn holds<T>(p: Payload) -> bool;

    /// The shared `T` the erased value holds, when it holds one.
    pub uninterp spec fn contents<T>(p: Payload) -> std::sync::Arc<T>;

    /// Trusted: `MessageType`'s derived equality is equality of the value.
    #[cfg(verus_only)]
    impl vstd::std_specs::cmp::PartialEqSpecImpl for MessageType {
        open spec fn obeys_eq_spec() -> bool { true }
        open spec fn eq_spec(&self, other: &MessageType) -> bool { self == other }
    }

    /// Trusted: `of::<T>()` is the one type for `T`.
    pub assume_specification<T: 'static>[ MessageType::of::<T> ]() -> (r: MessageType)
        ensures r == message_type::<T>();

    /// The type a payload was published as.
    pub uninterp spec fn payload_type(p: Payload) -> MessageType;

    /// Trusted: `message_type` reads the type the payload was published as.
    pub assume_specification[ Payload::message_type ](p: &Payload) -> (r: MessageType)
        ensures r == payload_type(*p);

    /// Trusted: `new` erases the value it is given, so `downcast` to its type recovers it.
    pub assume_specification<T: serde::Serialize + Send + Sync + 'static>[ Payload::new::<T> ](value: T) -> (r: Payload)
        ensures holds::<T>(r) && *contents::<T>(r) == value;

    /// Trusted: `downcast` recovers the shared value exactly when it is of the asked type.
    pub assume_specification<T: Send + Sync + 'static>[ Payload::downcast::<T> ](p: Payload) -> (got: Option<std::sync::Arc<T>>)
        ensures
            got is Some <==> holds::<T>(p),
            got is Some ==> got->0 == contents::<T>(p);

    /// Trusted: pairs of `String`s order lexicographically, a total order
    /// consistent with their equality, so a channel-keyed `BTreeMap` behaves as a map.
    #[verifier::external_body]
    pub broadcast proof fn channel_obeys_cmp()
        ensures #[trigger] vstd::laws_cmp::obeys_cmp::<Channel>(),
    {
    }

    /// Trusted: the derived ordering on an endpoint, an `Option<i32>` then a
    /// `String`, is lexicographic over total orders consistent with equality,
    /// so an endpoint-keyed `BTreeMap` behaves as a map.
    #[verifier::external_body]
    pub broadcast proof fn endpoint_obeys_cmp()
        ensures #[trigger] vstd::laws_cmp::obeys_cmp::<MessageEndpoint>(),
    {
    }

    } // verus!
}

} // verus!

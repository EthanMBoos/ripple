// Routing and delayed-delivery contracts for production bodies from build.rs.
// Run: python3 reference/creusot/check.py --routing --mutations
//
// Checked: matching subscriber indices and selected links/records; exact recipient
// lookup (first matching mailbox); successful delivery retains precisely the
// not-due entries with present recipients and appends precisely the due envelopes
// to their recipient/channel, in queue order. Absent recipients are discarded.
// The caller performs this transition even without new publications. Repeated
// routing returns an error without changing the context.
//
// Due means the actual IEEE comparison: deadline <= time_s + 1e-12. This is a
// successful-call queue proof, not a proof of engine phase invocation, clock
// progression, endpoint lifetime, or delivery to every eligible subscriber.
// Policy/serialization errors may abort; partial changes on errors are outside
// the contract. Pending-message provenance and subscription lifetime are outside
// this call's proof. Callback callability and mailbox-count bounds are required.
// Standard-library/IEEE models, derived ordering, payload type lookup,
// serialization, trace event IDs, and error construction are trusted boundaries.
// The pinned proof targets 64-bit hosts. Normal builds need no Creusot dependency.

// The proof checks ensure's branch and return, but trusts error formatting.
macro_rules! ensure {
    ($condition:expr, $($message:tt)*) => {
        if !$condition {
            return Err(routing_error());
        }
    };
}
#[trusted]
fn routing_error() -> anyhow::Error {
    anyhow::anyhow!("routing error")
}

impl DeepModel for MessageEndpoint {
    type DeepModelTy = (Option<Int>, Seq<char>);
    #[logic(open, inline)]
    fn deep_model(self) -> Self::DeepModelTy {
        (self.entity_id.deep_model(), self.plugin.deep_model())
    }
}

#[logic(opaque)]
fn payload_kind<P>(_value: P) -> TypeId {
    dead
}

extern_spec! {
    impl<K: Ord + DeepModel, V> BTreeMap<K, V> {
        #[ensures(result@ == creusot_std::logic::FMap::empty())]
        fn new() -> Self;
    }
    impl<T, A: std::alloc::Allocator> Vec<T, A> {
        #[ensures(result == (self@.len() == 0))]
        fn is_empty(&self) -> bool;
        #[ensures(result@ == self@)]
        fn as_slice(&self) -> &[T];
    }
    impl f64 {
        fn is_finite(self) -> bool;
    }
}

// These are specifications of standard operations, not assumptions about routing.
extern_spec! {
    impl Clone for String {
        #[ensures(result@ == self@)]
        fn clone(&self) -> String;
    }
}

// Rust's derived endpoint ordering is lexicographic. Model the compiler's
// generated comparison here; the routing algorithm itself remains checked.
impl Ord for MessageEndpoint {
    #[trusted]
    #[ensures(result == self.deep_model().cmp_log(other.deep_model()))]
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.entity_id
            .cmp(&other.entity_id)
            .then(self.plugin.cmp(&other.plugin))
    }
}
impl PartialOrd for MessageEndpoint {
    #[trusted]
    #[ensures(result == self.deep_model().partial_cmp_log(other.deep_model()))]
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

type ChannelModel = (Seq<char>, Seq<char>);
type SubscriberKey = (Option<Int>, ChannelModel);

#[logic(inline)]
fn subscriber_matches(
    key: SubscriberKey,
    receiver: (usize, TypeId),
    mailboxes: Seq<Mailbox<'_>>,
    local: bool,
) -> bool {
    pearlite! {
        receiver.0@ < mailboxes.len() &&
        key.0 == (if local { mailboxes[receiver.0@].endpoint.entity_id.deep_model() }
                  else { None }) &&
        mailboxes[receiver.0@].messages.subscriptions@.get(key.1) == Some(receiver.1)
    }
}

#[logic]
fn matching_entries(
    key: SubscriberKey,
    entries: Seq<(usize, TypeId)>,
    mailboxes: Seq<Mailbox<'_>>,
    local: bool,
) -> bool {
    pearlite! { forall<i> 0 <= i && i < entries.len() ==>
    subscriber_matches(key, entries[i], mailboxes, local) }
}

#[logic]
fn valid_subscribers(index: Subscribers<'_>, mailboxes: Seq<Mailbox<'_>>, local: bool) -> bool {
    pearlite! {
        forall<key: SubscriberKey, entries: Vec<(usize, TypeId)>>
            index@.get(key) == Some(entries) ==>
                matching_entries(key, entries@, mailboxes, local)
    }
}

#[logic(inline)]
fn valid_receivers(index: BTreeMap<&MessageEndpoint, usize>, mailboxes: Seq<Mailbox<'_>>) -> bool {
    pearlite! {
        forall<key: (Option<Int>, Seq<char>), i: usize>
            index@.get(key) == Some(i) ==>
                i@ < mailboxes.len() && mailboxes[i@].endpoint.deep_model() == key
    }
}

// Generic payloads avoid a redundant trait-object coercion in the logic term.
#[logic]
fn same_contents<P>(envelope: Envelope<P>, publication: Publication<P>) -> bool {
    pearlite! {
        envelope.sent_at_s == publication.sent_at_s &&
        envelope.value == publication.value && envelope.serialize == publication.serialize
    }
}

// Keep recursive definitions inside this module. The routing proof uses their
// checked empty/one-entry lemmas, avoiding uncontrolled recursive unfolding.
mod delayed {
    use super::*;
    // Use Creusot's executable IEEE comparison in logic as well. The cutoff is
    // computed with ordinary Rust arithmetic in the annotated production body.
    #[logic]
    #[creusot_std::macros::builtin("creusot.float.Float64.le")]
    pub(super) fn delivery_due(_delivered_at_s: f64, _cutoff_s: f64) -> bool {
        dead
    }

    pub(super) type ReceiverMap = creusot_std::logic::FMap<(Option<Int>, Seq<char>), usize>;

    // Exactly the original entries still waiting, in their original order.
    #[logic]
    #[variant(messages.len())]
    pub(super) fn pending_messages(
        messages: Seq<ScheduledMessage>,
        receivers: ReceiverMap,
        cutoff_s: f64,
    ) -> Seq<ScheduledMessage> {
        pearlite! {
            if messages.len() == 0 { Seq::empty() } else {
                let prefix = pending_messages(messages.pop_back(), receivers, cutoff_s);
                let last = messages[messages.len() - 1];
                if receivers.get(last.receiver.deep_model()) != None &&
                    !delivery_due(last.envelope.delivered_at_s, cutoff_s) {
                    prefix.push_back(last)
                } else { prefix }
            }
        }
    }

    // Delivery changes only the delivery timestamp of an envelope.
    #[logic(open(super), inline)]
    pub(super) fn delivered_envelope<P>(envelope: Envelope<P>, now_s: f64) -> Envelope<P> {
        Envelope {
            delivered_at_s: now_s,
            ..envelope
        }
    }

    // The exact suffix appended to one mailbox/channel during this routing call.
    #[logic]
    #[variant(messages.len())]
    pub(super) fn delivered_messages(
        messages: Seq<ScheduledMessage>,
        receivers: ReceiverMap,
        mailbox: Int,
        channel: ChannelModel,
        cutoff_s: f64,
        now_s: f64,
    ) -> Seq<Envelope> {
        pearlite! {
            if messages.len() == 0 { Seq::empty() } else {
                let prefix = delivered_messages(messages.pop_back(), receivers,
                    mailbox, channel, cutoff_s, now_s);
                let last = messages[messages.len() - 1];
                let selected = match receivers.get(last.receiver.deep_model()) {
                    Some(index) => index@ == mailbox,
                    None => false,
                };
                if selected && last.channel.deep_model() == channel &&
                    delivery_due(last.envelope.delivered_at_s, cutoff_s) {
                    prefix.push_back(delivered_envelope(last.envelope, now_s))
                } else { prefix }
            }
        }
    }

    pub(super) type InboxMap = creusot_std::logic::FMap<ChannelModel, Vec<Envelope>>;

    #[logic(open(super), inline)]
    pub(super) fn mailbox_queues(mailboxes: Seq<Mailbox<'_>>) -> Seq<InboxMap> {
        pearlite! { Seq::create(mailboxes.len(), |i: Int| mailboxes[i].messages.inbox@) }
    }

    #[logic(open(super), inline)]
    pub(super) fn queue_appended(
        before: Option<Vec<Envelope>>,
        after: Option<Vec<Envelope>>,
        suffix: Seq<Envelope>,
    ) -> bool {
        pearlite! {
            match (before, after) {
                (None, None) => suffix.len() == 0,
                (None, Some(q)) => suffix.len() > 0 && q@ == suffix,
                (Some(p), Some(q)) => q@ == p@.concat(suffix),
                (Some(_), None) => false,
            }
        }
    }

    #[logic(inline)]
    pub(super) fn delivery_queues(
        before: Seq<InboxMap>,
        after: Seq<InboxMap>,
        messages: Seq<ScheduledMessage>,
        receivers: ReceiverMap,
        cutoff_s: f64,
        now_s: f64,
    ) -> bool {
        pearlite! {
            before.len() == after.len() &&
            forall<i, channel> 0 <= i && i < after.len() ==>
                queue_appended(before[i].get(channel), after[i].get(channel),
                    delivered_messages(messages, receivers, i, channel, cutoff_s, now_s))
        }
    }

    pub(super) const DELIVERY_TOLERANCE_S: f64 = 1e-12;
    #[logic]
    #[creusot_std::macros::builtin("creusot.float.Float64.add")]
    pub(super) fn float_add(_a: f64, _b: f64) -> f64 {
        dead
    }
    #[logic(open(super), inline)]
    pub(super) fn delivery_cutoff(now_s: f64) -> f64 {
        float_add(now_s, DELIVERY_TOLERANCE_S)
    }

    // Match the production lookup's first-mailbox-wins rule, even for duplicates.
    #[logic]
    #[variant(endpoints.len())]
    pub(super) fn endpoint_index(endpoints: Seq<(Option<Int>, Seq<char>)>) -> ReceiverMap {
        pearlite! {
            if endpoints.len() == 0 { creusot_std::logic::FMap::empty() } else {
                let prefix = endpoint_index(endpoints.pop_back());
                let key = endpoints[endpoints.len() - 1];
                match prefix.get(key) {
                    Some(_) => prefix,
                    None => prefix.insert(key, machine_index(endpoints.len() - 1)),
                }
            }
        }
    }

    #[logic(open(super), inline)]
    pub(super) fn mailbox_endpoints(mailboxes: Seq<Mailbox<'_>>) -> Seq<(Option<Int>, Seq<char>)> {
        pearlite! { Seq::create(mailboxes.len(), |i: Int| mailboxes[i].endpoint.deep_model()) }
    }

    #[logic(open(super), inline)]
    pub(super) fn mailbox_outboxes(mailboxes: Seq<Mailbox<'_>>) -> Seq<Seq<Publication>> {
        pearlite! { Seq::create(mailboxes.len(), |i: Int| mailboxes[i].messages.outbox@) }
    }

    #[logic(open(super))]
    pub(super) fn no_publications(context: NetworkContext<'_, '_>) -> bool {
        pearlite! {
            forall<i, j> 0 <= i && i < context.mailboxes@.len() &&
                0 <= j && j < context.mailboxes@[i].messages.outbox@.len() ==>
                context.mailboxes@[i].messages.outbox@[j].channel.0@ != context.name@
        }
    }

    // The pinned proof toolchain targets 64-bit hosts. This is the same conversion
    // model used by Creusot for machine integers, with no new arithmetic axioms.
    #[cfg(target_pointer_width = "64")]
    #[logic]
    #[creusot_std::macros::builtin("creusot.int.UInt64.of_int")]
    pub(super) fn machine_index(_index: Int) -> usize {
        dead
    }

    #[logic]
    #[ensures(sequence.len() == 0 ==> sequence == Seq::empty())]
    #[ensures(sequence.subsequence(0, 0) == Seq::empty())]
    #[ensures(sequence.subsequence(0, sequence.len()) == sequence)]
    #[ensures(forall<i> 0 <= i && i < sequence.len() ==>
        sequence.subsequence(0, i + 1) ==
            sequence.subsequence(0, i).push_back(sequence[i]))]
    #[ensures(forall<item> sequence.concat(Seq::singleton(item)) == sequence.push_back(item))]
    #[ensures(forall<item> sequence.push_back(item).pop_back() == sequence)]
    pub(super) fn sequence_prefixes<T>(sequence: Seq<T>) {
        proof_assert!(sequence.len() == 0 ==> sequence.ext_eq(Seq::empty()));
        proof_assert!(forall<item> sequence.concat(Seq::singleton(item)).ext_eq(sequence.push_back(item)));
        proof_assert!(forall<item> sequence.push_back(item).pop_back().ext_eq(sequence));
        proof_assert!(sequence.subsequence(0, 0).ext_eq(Seq::empty()));
        proof_assert!(sequence.subsequence(0, sequence.len()).ext_eq(sequence));
        proof_assert!(forall<i> 0 <= i && i < sequence.len() ==>
            sequence.subsequence(0, i + 1).ext_eq(
                sequence.subsequence(0, i).push_back(sequence[i])));
    }

    #[logic]
    #[ensures(prefix.concat(suffix).push_back(item) == prefix.concat(suffix.push_back(item)))]
    pub(super) fn append_suffix<T>(prefix: Seq<T>, suffix: Seq<T>, item: T) {
        proof_assert!(
            prefix
                .concat(suffix)
                .push_back(item)
                .ext_eq(prefix.concat(suffix.push_back(item)))
        );
    }

    #[logic]
    #[requires(0 <= index && index < mailboxes.len())]
    #[ensures(mailbox_queues(mailboxes.set(index, mailbox)) ==
        mailbox_queues(mailboxes).set(index, mailbox.messages.inbox@))]
    #[ensures(mailbox_endpoints(mailboxes.set(index, mailbox)) ==
        mailbox_endpoints(mailboxes).set(index, mailbox.endpoint.deep_model()))]
    pub(super) fn mailbox_update<'a>(
        mailboxes: Seq<Mailbox<'a>>,
        index: Int,
        mailbox: Mailbox<'a>,
    ) {
        proof_assert!(mailbox_queues(mailboxes.set(index, mailbox)).ext_eq(
            mailbox_queues(mailboxes).set(index, mailbox.messages.inbox@)));
        proof_assert!(
            mailbox_endpoints(mailboxes.set(index, mailbox))
                .ext_eq(mailbox_endpoints(mailboxes).set(index, mailbox.endpoint.deep_model()))
        );
    }

    #[logic]
    #[requires(forall<i> 0 <= i && i < prefix.concat(suffix).len() ==>
        prefix.concat(suffix)[i].channel.0@ != network)]
    #[ensures(forall<i> 0 <= i && i < suffix.len() ==> suffix[i].channel.0@ != network)]
    pub(super) fn quiet_suffix(
        prefix: Seq<Publication>,
        suffix: Seq<Publication>,
        network: Seq<char>,
    ) {
        proof_assert!(forall<i> 0 <= i && i < suffix.len() ==>
            suffix[i] == prefix.concat(suffix)[prefix.len() + i]);
    }

    // Small induction steps keep the routing proof from repeatedly unfolding the
    // recursive definitions inside its mutable-borrow bookkeeping.
    #[logic]
    #[ensures(pending_messages(messages.push_back(message), receivers, cutoff_s) ==
    if receivers.get(message.receiver.deep_model()) != None &&
        !delivery_due(message.envelope.delivered_at_s, cutoff_s) {
        pending_messages(messages, receivers, cutoff_s).push_back(message)
    } else { pending_messages(messages, receivers, cutoff_s) })]
    pub(super) fn pending_step(
        messages: Seq<ScheduledMessage>,
        message: ScheduledMessage,
        receivers: ReceiverMap,
        cutoff_s: f64,
    ) {
        proof_assert!(messages.push_back(message).pop_back().ext_eq(messages));
        proof_assert!(messages.push_back(message)[messages.len()] == message);
    }

    #[logic]
    #[ensures(delivered_messages(messages.push_back(message), receivers, mailbox, channel, cutoff_s, now_s) ==
    if (match receivers.get(message.receiver.deep_model()) {
        Some(index) => index@ == mailbox, None => false,
    }) && message.channel.deep_model() == channel &&
        delivery_due(message.envelope.delivered_at_s, cutoff_s) {
        delivered_messages(messages, receivers, mailbox, channel, cutoff_s, now_s)
            .push_back(delivered_envelope(message.envelope, now_s))
    } else { delivered_messages(messages, receivers, mailbox, channel, cutoff_s, now_s) })]
    pub(super) fn delivered_step(
        messages: Seq<ScheduledMessage>,
        message: ScheduledMessage,
        receivers: ReceiverMap,
        mailbox: Int,
        channel: ChannelModel,
        cutoff_s: f64,
        now_s: f64,
    ) {
        proof_assert!(messages.push_back(message).pop_back().ext_eq(messages));
        proof_assert!(messages.push_back(message)[messages.len()] == message);
    }

    #[logic]
    #[ensures(delivery_queues(queues, queues, Seq::empty(), receivers, cutoff_s, now_s))]
    #[ensures(pending_messages(Seq::empty(), receivers, cutoff_s) == Seq::empty())]
    #[ensures(forall<mailbox, channel> delivered_messages(Seq::empty(), receivers,
        mailbox, channel, cutoff_s, now_s) == Seq::empty())]
    pub(super) fn empty_delivery(
        queues: Seq<InboxMap>,
        receivers: ReceiverMap,
        cutoff_s: f64,
        now_s: f64,
    ) {
        let _ = Seq::<Envelope>::concat_empty;
        proof_assert!(forall<mailbox, channel> delivered_messages(Seq::empty(), receivers,
            mailbox, channel, cutoff_s, now_s) == Seq::empty());
    }

    // One pending entry either leaves every inbox alone or appends to one queue.
    #[logic]
    #[requires(delivery_queues(before, current, messages, receivers, cutoff_s, now_s))]
    #[requires(receivers.get(message.receiver.deep_model()) == None ||
        !delivery_due(message.envelope.delivered_at_s, cutoff_s))]
    #[ensures(delivery_queues(before, current, messages.push_back(message), receivers, cutoff_s, now_s))]
    pub(super) fn delivery_keep_step(
        before: Seq<InboxMap>,
        current: Seq<InboxMap>,
        messages: Seq<ScheduledMessage>,
        message: ScheduledMessage,
        receivers: ReceiverMap,
        cutoff_s: f64,
        now_s: f64,
    ) {
        let _ = delivered_step;
    }

    #[logic]
    #[requires(delivery_queues(before, current, messages, receivers, cutoff_s, now_s))]
    #[requires(receivers.get(message.receiver.deep_model()) == Some(index))]
    #[requires(index@ < current.len())]
    #[requires(delivery_due(message.envelope.delivered_at_s, cutoff_s))]
    #[requires(queue_appended(current[index@].get(message.channel.deep_model()),
        updated.get(message.channel.deep_model()), Seq::singleton(delivered_envelope(message.envelope, now_s))))]
    #[requires(forall<channel> channel != message.channel.deep_model() ==>
        updated.get(channel) == current[index@].get(channel))]
    #[ensures(delivery_queues(before, current.set(index@, updated), messages.push_back(message),
        receivers, cutoff_s, now_s))]
    pub(super) fn delivery_append_step(
        before: Seq<InboxMap>,
        current: Seq<InboxMap>,
        messages: Seq<ScheduledMessage>,
        message: ScheduledMessage,
        receivers: ReceiverMap,
        cutoff_s: f64,
        now_s: f64,
        index: usize,
        updated: InboxMap,
    ) {
        let _ = delivered_step;
        let _ = append_suffix::<Envelope>;
        let _ = sequence_prefixes::<Envelope>;
        proof_assert!(forall<channel> queue_appended(before[index@].get(channel), updated.get(channel),
            delivered_messages(messages.push_back(message), receivers, index@, channel, cutoff_s, now_s)));
    }

    #[logic]
    #[ensures(endpoint_index(Seq::empty()).ext_eq(creusot_std::logic::FMap::empty()))]
    #[ensures(endpoint_index(endpoints.push_back(endpoint)).ext_eq(
    match endpoint_index(endpoints).get(endpoint) {
        Some(_) => endpoint_index(endpoints),
        None => endpoint_index(endpoints).insert(endpoint, machine_index(endpoints.len())),
    }))]
    pub(super) fn endpoint_step(
        endpoints: Seq<(Option<Int>, Seq<char>)>,
        endpoint: (Option<Int>, Seq<char>),
    ) {
        proof_assert!(endpoints.push_back(endpoint).pop_back().ext_eq(endpoints));
        proof_assert!(endpoints.push_back(endpoint)[endpoints.len()] == endpoint);
    }
}
#[cfg(creusot)]
use delayed::*;

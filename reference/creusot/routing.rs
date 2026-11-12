// Routing safety contracts for the production body extracted by build.rs.
// Run: python3 reference/creusot/check.py --routing --mutations
//
// Checked: index entries identify matching subscribers; links use this network,
// channel, payload type, and local/global scope; new scheduled records preserve
// the selected recipient, channel, and contents; delivery selects the recorded
// endpoint and appends to its channel without changing its other queues; a
// repeated routing call returns an error without changing the context.
//
// Scope: this is not a completeness, latency, or whole-simulator proof. The
// policy may drop/delay/fail, and serialization may fail. Pending messages are
// matched by endpoint; their earlier creation and subscription lifetime are
// outside this call's proof. Callback callability and mailbox-count bounds are
// preconditions. Standard-library models, derived ordering, payload type lookup,
// serialization, trace event IDs, and error construction are trusted boundaries.
// No contracts or Creusot dependency are added to normal simulator builds.

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

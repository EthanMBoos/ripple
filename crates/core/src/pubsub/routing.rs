//! The network phase: a network's publications offered to their subscribers,
//! the links it accepts scheduled, and due messages delivered to subscriber
//! queues. Networks decide; the engine owns the queues and the clock.
use super::messages::{Channel, Envelope, MessageEndpoint, MessageType, Messages, Publication};
use crate::{
    common::{PluginRandom, TruthContacts},
    plugin::StepTime,
};
use anyhow::{Result, bail};
use std::collections::BTreeMap;
use vstd::prelude::*;

mod proof;
// Verus skips this module; proof.rs declares its methods as calls that promise nothing.
#[cfg_attr(verus_only, verus_verify(external))]
mod trace;
pub(crate) use trace::NetworkTrace;

verus! {

broadcast use proof::routing_facts;

/// Networks decide topology, loss and delay; the engine owns queues and
/// delivery.
pub struct NetworkContext<'a, 'mailbox> {
    pub time: StepTime,
    pub contacts_truth: TruthContacts<'a>,
    pub(crate) name: &'a str,
    pub(crate) endpoint: &'a MessageEndpoint,
    pub(crate) mailboxes: &'a mut [Mailbox<'mailbox>],
    pub(crate) scheduled: &'a mut Vec<ScheduledMessage>,
    pub(crate) random: PluginRandom,
    pub(crate) routed: bool,
    /// Present only when the simulation writes an observation trace.
    pub(crate) trace: Option<&'a mut NetworkTrace>,
}

/// Each channel's subscribers (per entity, for same-entity routing), in mailbox
/// order: the mailbox's index and the message type it expects. The entity comes
/// first in the key so most comparisons are of integers, not channel names.
type Subscribers<'a> = BTreeMap<(Option<i32>, &'a Channel), Vec<(usize, MessageType)>>;
pub struct Transmission<'a> {
    pub time: StepTime,
    pub sender: &'a MessageEndpoint,
    pub receiver: &'a MessageEndpoint,
    pub topic: &'a str,
    /// Which of the sender entity's messages on this network and topic this
    /// tick it is, from 0, in publication order. With the entities and topic
    /// it names the message, as a random key.
    pub sequence: u64,
    pub contacts_truth: TruthContacts<'a>,
}
#[derive(Clone, Copy, Debug)]
pub enum Delivery {
    Drop,
    After { delay_s: f64 },
}
pub(crate) struct Mailbox<'a> {
    pub endpoint: &'a MessageEndpoint,
    pub messages: &'a mut Messages,
}
/// A message in flight, until the routing phase at or after its delivery time.
pub(crate) struct ScheduledMessage {
    pub receiver: MessageEndpoint,
    pub channel: Channel,
    pub envelope: Envelope,
}
/// What scheduling and delivery touch: the phase time, the messages in
/// flight, and the trace. `route_links` builds one from its context so the
/// mailboxes stay borrowed by the loops around the calls. The trace is
/// borrowed as the context holds it, an `Option` of a reference, because
/// Verus has no contract for `as_deref_mut`.
pub(crate) struct DeliveryPhase<'a, 'trace> {
    pub time: StepTime,
    pub scheduled: &'a mut Vec<ScheduledMessage>,
    pub trace: &'a mut Option<&'trace mut NetworkTrace>,
}

#[verifier::external]
impl NetworkContext<'_, '_> {
    /// This network plugin can also publish and subscribe like every other plugin type.
    pub fn messages(&mut self) -> &mut Messages {
        self.mailboxes
            .iter_mut()
            .find(|mailbox| mailbox.endpoint == self.endpoint)
            .expect("network plugin has a registered mailbox")
            .messages
    }
    /// Decide each link from a publisher to a subscriber of its channel, in stable
    /// order: publications in mailbox order, then subscribers in mailbox order.
    pub fn route(
        &mut self,
        decide: impl FnMut(&Transmission<'_>, &PluginRandom) -> Result<Delivery>,
    ) -> Result<()> {
        self.route_links(false, decide)
    }
    /// Like `route`, for a network that delivers only between plugins of the same
    /// entity (LocalNetwork): `decide` sees only links to subscribers in the
    /// sender's own entity, and none from world plugins, which have no entity.
    pub fn route_within_entity(
        &mut self,
        decide: impl FnMut(&Transmission<'_>, &PluginRandom) -> Result<Delivery>,
    ) -> Result<()> {
        self.route_links(true, decide)
    }
}

impl NetworkContext<'_, '_> {
    /// The loops of a routing phase: this network's publications are collected
    /// in mailbox order, each is offered to the subscribers of its channel in
    /// mailbox order and the network decides each link, and the scheduled
    /// messages are delivered or kept.
    fn route_links<F: FnMut(&Transmission<'_>, &PluginRandom) -> Result<Delivery>>(
        &mut self,
        within_entity: bool,
        mut decide: F,
    ) -> (r: Result<()>)
        ensures proof::routed(old(self), final(self), within_entity, &r)
    {
        if self.routed {
            bail!("network may route once per phase");
        }
        self.routed = true;
        let publications = collect_publications(self.mailboxes, self.name);
        let mut phase = DeliveryPhase {
            time: self.time,
            scheduled: self.scheduled,
            trace: &mut self.trace,
        };
        offer_publications(
            &publications,
            self.mailboxes,
            within_entity,
            self.contacts_truth,
            &self.random,
            &mut decide,
            &mut phase,
        )?;
        deliver_scheduled(&mut phase, self.mailboxes);
        Ok(())
    }
}

/// This network's publications in mailbox order, each with its sender.
/// Publisher order is stable entity/slot order, never worker completion order.
fn collect_publications<'m>(
    mailboxes: &mut [Mailbox<'m>],
    network: &str,
) -> (publications: Vec<(&'m MessageEndpoint, Publication)>)
    ensures proof::collected(old(mailboxes)@, final(mailboxes)@, network, publications@)
{
    let mut publications = Vec::new();
    for mailbox in it: mailboxes.iter_mut()
        invariant proof::collected_so_far(old(mailboxes)@, it, network, publications@)
    {
        mailbox
            .messages
            .take_publications(network, mailbox.endpoint, &mut publications);
    }
    publications
}

/// Each channel's subscribers in mailbox order.
// DEVNOTE: each channel's subscribers, in mailbox order, are found once per
// phase that has messages, so a message visits only its subscribers; within an
// entity, only its own entity's. Visiting every mailbox for every message made
// routing grow with the square of the team: with NoisyState on every aircraft
// (every Straight subscribed to its message), a 4,096-agent step took 388 ms,
// nearly all in routing (macOS `sample`); with the index, 9.7 ms. C++ keeps
// per-topic subscriber lists but still checks every subscriber, so it costs
// the same there. Measured with reference/message_benchmark.py, which still
// times C++ against this routing (the earlier routing is gone). Building the
// index in phases without messages made steps without messages 27-49% slower,
// so `offer_publications` skips it then.
fn index_subscribers<'a>(
    mailboxes: &'a [Mailbox<'_>],
    within_entity: bool,
) -> (subscribers: Subscribers<'a>)
    ensures proof::indexed(mailboxes@, within_entity, subscribers@)
{
    let mut subscribers = Subscribers::new();
    for index in it: 0..mailboxes.len()
        invariant proof::indexed_so_far(mailboxes@, within_entity, subscribers@, it)
    {
        index_subscriptions(&mut subscribers, index, &mailboxes[index], within_entity);
    }
    subscribers
}

/// Adds one mailbox's subscriptions to the index, in its group.
fn index_subscriptions<'a>(
    subscribers: &mut Subscribers<'a>,
    index: usize,
    mailbox: &'a Mailbox<'_>,
    within_entity: bool,
)
    ensures proof::mailbox_indexed(old(subscribers)@, final(subscribers)@, index, mailbox, within_entity)
{
    let entity = if within_entity {
        mailbox.endpoint.entity_id
    } else {
        None
    };
    for pair in it: mailbox.messages.subscriptions.iter()
        invariant proof::subscriptions_so_far(old(subscribers)@, subscribers@, index, entity, mailbox.messages.subscriptions@, it)
    {
        let (channel, expected_type) = pair;
        add_subscriber(subscribers, (entity, channel), (index, *expected_type));
    }
}

/// Appends the subscriber to the list under `key`, starting the list if there
/// is none. Written out because Verus has no contract for `entry`.
fn add_subscriber<'a>(
    subscribers: &mut Subscribers<'a>,
    key: (Option<i32>, &'a Channel),
    subscriber: (usize, MessageType),
)
    ensures proof::subscriber_added(old(subscribers)@, final(subscribers)@, key, subscriber)
{
    match subscribers.get_mut(&key) {
        Some(list) => list.push(subscriber),
        None => {
            subscribers.insert(key, vec![subscriber]);
        }
    }
}

/// Offers each publication to the subscribers of its channel, in order, each
/// numbered among its sender entity's messages on its topic.
fn offer_publications<F: FnMut(&Transmission<'_>, &PluginRandom) -> Result<Delivery>>(
    publications: &[(&MessageEndpoint, Publication)],
    mailboxes: &[Mailbox<'_>],
    within_entity: bool,
    contacts_truth: TruthContacts<'_>,
    random: &PluginRandom,
    decide: &mut F,
    phase: &mut DeliveryPhase<'_, '_>,
) -> (r: Result<()>)
    ensures proof::offered(publications@, mailboxes@, within_entity, random, old(phase), final(phase), &r)
{
    if publications.is_empty() {
        return Ok(());
    }
    let subscribers = index_subscribers(mailboxes, within_entity);
    let mut sequences: BTreeMap<(Option<i32>, String), u64> = BTreeMap::new();
    for pair in it: publications.iter()
        invariant proof::offers_so_far(publications@, mailboxes@, within_entity, random, old(phase), phase, subscribers@, sequences@, it)
    {
        let (sender, publication) = pair;
        let key = (sender.entity_id, publication.channel.1.clone());
        let sequence = match sequences.get_mut(&key) {
            Some(count) => {
                let sequence = *count;
                *count += 1;
                sequence
            }
            None => {
                sequences.insert(key, 1);
                0
            }
        };
        let group = match (within_entity, sender.entity_id) {
            (false, _) => None,
            (true, Some(entity_id)) => Some(entity_id),
            (true, None) => continue,
        };
        let Some(receivers) = subscribers.get(&(group, &publication.channel)) else {
            continue;
        };
        offer_to_subscribers(
            sender,
            publication,
            sequence,
            receivers,
            mailboxes,
            contacts_truth,
            random,
            decide,
            phase,
        )?;
    }
    Ok(())
}

/// Offers one publication to each of its subscribers in order; a subscriber
/// expecting another type is an error, and the network decides each link.
#[allow(clippy::too_many_arguments)]
fn offer_to_subscribers<F: FnMut(&Transmission<'_>, &PluginRandom) -> Result<Delivery>>(
    sender: &MessageEndpoint,
    publication: &Publication,
    sequence: u64,
    receivers: &[(usize, MessageType)],
    mailboxes: &[Mailbox<'_>],
    contacts_truth: TruthContacts<'_>,
    random: &PluginRandom,
    decide: &mut F,
    phase: &mut DeliveryPhase<'_, '_>,
) -> (r: Result<()>)
    requires proof::receivers_within(receivers@, mailboxes@)
    ensures proof::links_offered(sender, publication, sequence as nat, receivers@, mailboxes@, random, old(phase), final(phase), &r)
{
    for pair in it: receivers.iter()
        invariant proof::links_so_far(sender, publication, sequence as nat, receivers@, mailboxes@, random, old(phase), phase, it)
    {
        let (index, expected_type) = *pair;
        if expected_type != publication.payload.message_type() {
            let (network, topic) = &publication.channel;
            bail!("message type mismatch on {network}/{topic}");
        }
        let link = Transmission {
            time: phase.time,
            sender,
            receiver: mailboxes[index].endpoint,
            topic: &publication.channel.1,
            sequence,
            contacts_truth,
        };
        match decide_link(decide, &link, random)? {
            Delivery::Drop => {}
            Delivery::After { delay_s } => phase.schedule(&link, publication, delay_s)?,
        }
    }
    Ok(())
}

/// The network's decision on one link. Verus does not read it; proof.rs
/// states only that the network was asked and answered.
#[verifier::external]
fn decide_link<F: FnMut(&Transmission<'_>, &PluginRandom) -> Result<Delivery>>(
    decide: &mut F,
    link: &Transmission<'_>,
    random: &PluginRandom,
) -> Result<Delivery> {
    decide(link, random)
}

/// Each endpoint's first mailbox, found once rather than by searching every
/// mailbox for every message. A check and an insert, because Verus has no
/// contract for `entry`.
#[allow(clippy::map_entry)]
fn receivers_by_endpoint<'m>(mailboxes: &[Mailbox<'m>]) -> (receivers: BTreeMap<&'m MessageEndpoint, usize>)
    ensures proof::receivers_indexed(mailboxes@, receivers@)
{
    let mut receivers = BTreeMap::new();
    for index in it: 0..mailboxes.len()
        invariant proof::receivers_so_far(mailboxes@, receivers@, it)
    {
        let endpoint = mailboxes[index].endpoint;
        if !receivers.contains_key(&endpoint) {
            receivers.insert(endpoint, index);
        }
    }
    receivers
}

/// Delivers the scheduled messages whose time has come and keeps the rest.
fn deliver_scheduled(phase: &mut DeliveryPhase<'_, '_>, mailboxes: &mut [Mailbox<'_>])
    ensures proof::all_delivered(old(phase), final(phase), old(mailboxes)@, final(mailboxes)@)
{
    if phase.scheduled.is_empty() {
        return;
    }
    let receivers = receivers_by_endpoint(mailboxes);
    let mut in_flight = Vec::new();
    std::mem::swap(phase.scheduled, &mut in_flight);
    for message in it: in_flight
        invariant proof::deliveries_so_far(it.seq(), old(phase), phase, old(mailboxes)@, mailboxes, receivers@, it.index())
    {
        let receiver = &message.receiver;
        let Some(&index) = receivers.get(&receiver) else {
            // A removed entity cannot receive delayed messages.
            continue;
        };
        phase.deliver(message, mailboxes[index].messages);
    }
}

impl DeliveryPhase<'_, '_> {
    /// Schedules the link's message for `delay_s` after now.
    fn schedule(
        &mut self,
        link: &Transmission<'_>,
        publication: &Publication,
        delay_s: f64,
    ) -> (r: Result<()>)
        ensures proof::scheduled(old(self), final(self), link, publication, delay_s, &r)
    {
        if !(delay_s.is_finite() && delay_s >= 0.0) {
            bail!("network delay must be finite and nonnegative");
        }
        let delivered_at_s = self.time.time_s + delay_s;
        if !delivered_at_s.is_finite() {
            bail!("network delivery time overflow");
        }
        let message = ScheduledMessage {
            receiver: link.receiver.clone(),
            // Cloned a name at a time: Verus has no contract for a tuple's clone.
            channel: (publication.channel.0.clone(), publication.channel.1.clone()),
            envelope: Envelope {
                sender: link.sender.clone(),
                sent_at_s: publication.sent_at_s,
                delivered_at_s,
                payload: publication.payload.clone(),
            },
        };
        if let Some(trace) = &mut *self.trace {
            trace.scheduled(self.time.time_s, delay_s, &message);
        }
        self.scheduled.push(message);
        Ok(())
    }
    /// Delivers one scheduled message when its time has come, onto the
    /// receiver's queue for its channel, stamped with this phase's time; a
    /// message not yet due goes back on the schedule.
    fn deliver(&mut self, mut message: ScheduledMessage, receiver: &mut Messages)
        ensures proof::delivered(old(self), final(self), &message, old(receiver), final(receiver))
    {
        if message.envelope.delivered_at_s <= self.time.time_s + 1e-12 {
            message.envelope.delivered_at_s = self.time.time_s;
            if let Some(trace) = &mut *self.trace {
                trace.delivered(self.time.time_s, &message);
            }
            receiver.enqueue(message.channel, message.envelope);
        } else {
            self.scheduled.push(message);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::{
        Network, PluginParams,
        network::{GlobalNetwork, LocalNetwork},
    };

    struct TestMailboxes {
        sender: Messages,
        same_entity: Messages,
        other_entity: Messages,
        scheduled: Vec<ScheduledMessage>,
    }
    impl TestMailboxes {
        fn new() -> Result<Self> {
            let mut same_entity = Messages::default();
            let mut other_entity = Messages::default();
            same_entity.subscribe::<i32>("test", "sample")?;
            other_entity.subscribe::<i32>("test", "sample")?;
            Ok(Self {
                sender: Messages::default(),
                same_entity,
                other_entity,
                scheduled: Vec::new(),
            })
        }
        fn phase(
            &mut self,
            time_s: f64,
            run: impl FnOnce(&mut NetworkContext<'_, '_>) -> Result<()>,
        ) -> Result<()> {
            let endpoint = MessageEndpoint {
                entity_id: None,
                plugin: "test".into(),
            };
            let mut network_messages = Messages::default();
            let mut mailboxes = [
                Mailbox {
                    endpoint: &MessageEndpoint {
                        entity_id: Some(1),
                        plugin: "sensor".into(),
                    },
                    messages: &mut self.sender,
                },
                Mailbox {
                    endpoint: &MessageEndpoint {
                        entity_id: Some(1),
                        plugin: "autonomy".into(),
                    },
                    messages: &mut self.same_entity,
                },
                Mailbox {
                    endpoint: &MessageEndpoint {
                        entity_id: Some(2),
                        plugin: "autonomy".into(),
                    },
                    messages: &mut self.other_entity,
                },
                Mailbox {
                    endpoint: &endpoint,
                    messages: &mut network_messages,
                },
            ];
            run(&mut NetworkContext {
                time: StepTime {
                    time_s,
                    dt_s: 0.1,
                    tick: 0,
                },
                contacts_truth: TruthContacts::from(&[][..]),
                name: "test",
                endpoint: &endpoint,
                mailboxes: &mut mailboxes,
                scheduled: &mut self.scheduled,
                random: PluginRandom::new(1, 0, "test"),
                routed: false,
                trace: None,
            })?;
            Ok(())
        }
    }

    #[test]
    fn local_network_is_same_entity_and_global_network_is_not() -> Result<()> {
        let params = PluginParams::yaml("{}");
        let mut local = LocalNetwork::new(&LocalNetwork::configure(&params)?);
        let mut global = GlobalNetwork::new(&GlobalNetwork::configure(&params)?);
        let mut mailboxes = TestMailboxes::new()?;
        mailboxes.sender.publish("test", "sample", 42_i32)?;
        mailboxes.phase(0.0, |context| Network::step(&mut local, context))?;
        assert_eq!(
            *mailboxes.same_entity.receive::<i32>("test", "sample")?[0].value,
            42
        );
        assert!(
            mailboxes
                .other_entity
                .receive::<i32>("test", "sample")?
                .is_empty()
        );
        mailboxes.sender.publish("test", "sample", 17_i32)?;
        mailboxes.phase(0.1, |context| Network::step(&mut global, context))?;
        let same_entity = mailboxes.same_entity.receive::<i32>("test", "sample")?;
        let other_entity = mailboxes.other_entity.receive::<i32>("test", "sample")?;
        assert_eq!(*same_entity[0].value, 17);
        assert_eq!(*other_entity[0].value, 17);
        assert!(
            mailboxes
                .same_entity
                .receive::<i32>("test", "sample")?
                .is_empty()
        );
        Ok(())
    }

    #[test]
    fn local_network_delivers_nothing_a_world_plugin_publishes() -> Result<()> {
        // The network's own mailbox has no entity, like every world plugin.
        let params = PluginParams::yaml("{}");
        let mut local = LocalNetwork::new(&LocalNetwork::configure(&params)?);
        let mut global = GlobalNetwork::new(&GlobalNetwork::configure(&params)?);
        let mut mailboxes = TestMailboxes::new()?;
        mailboxes.phase(0.0, |context| {
            context.messages().publish("test", "sample", 7_i32)?;
            Network::step(&mut local, context)
        })?;
        assert!(
            mailboxes
                .same_entity
                .receive::<i32>("test", "sample")?
                .is_empty()
        );
        assert!(
            mailboxes
                .other_entity
                .receive::<i32>("test", "sample")?
                .is_empty()
        );
        mailboxes.phase(0.1, |context| {
            context.messages().publish("test", "sample", 7_i32)?;
            Network::step(&mut global, context)
        })?;
        assert_eq!(
            mailboxes
                .same_entity
                .receive::<i32>("test", "sample")?
                .len(),
            1
        );
        assert_eq!(
            mailboxes
                .other_entity
                .receive::<i32>("test", "sample")?
                .len(),
            1
        );
        Ok(())
    }

    #[test]
    fn global_network_delay_arrives_at_the_first_tick_at_or_after_it() -> Result<()> {
        let params = PluginParams::yaml("delay_s: 0.25");
        let mut global = GlobalNetwork::new(&GlobalNetwork::configure(&params)?);
        let mut mailboxes = TestMailboxes::new()?;
        mailboxes.sender.publish("test", "sample", 5_i32)?;
        for time_s in [0.0, 0.1, 0.2] {
            mailboxes.phase(time_s, |context| Network::step(&mut global, context))?;
            assert!(
                mailboxes
                    .other_entity
                    .receive::<i32>("test", "sample")?
                    .is_empty()
            );
        }
        mailboxes.phase(0.3, |context| Network::step(&mut global, context))?;
        let received = mailboxes.other_entity.receive::<i32>("test", "sample")?;
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].sent_at_s, 0.0);
        assert_eq!(received[0].delivered_at_s, 0.3);
        Ok(())
    }

    #[test]
    fn delayed_messages_arrive_during_silent_publisher_phases() -> Result<()> {
        let mut mailboxes = TestMailboxes::new()?;
        mailboxes.sender.publish("test", "sample", 3_i32)?;
        mailboxes.phase(0.0, |context| {
            context.route(|_, _| Ok(Delivery::After { delay_s: 0.2 }))
        })?;
        assert!(
            mailboxes
                .same_entity
                .receive::<i32>("test", "sample")?
                .is_empty()
        );
        mailboxes.phase(0.1, |context| context.route(|_, _| Ok(Delivery::Drop)))?;
        assert!(
            mailboxes
                .same_entity
                .receive::<i32>("test", "sample")?
                .is_empty()
        );
        mailboxes.phase(0.2, |context| context.route(|_, _| Ok(Delivery::Drop)))?;
        let received = mailboxes.same_entity.receive::<i32>("test", "sample")?;
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].sent_at_s, 0.0);
        assert_eq!(received[0].delivered_at_s, 0.2);
        assert_eq!(*received[0].value, 3);
        Ok(())
    }

    #[test]
    fn wrong_message_types_and_invalid_delays_are_errors() -> Result<()> {
        let mut mailboxes = TestMailboxes::new()?;
        assert!(
            mailboxes
                .same_entity
                .subscribe::<String>("test", "sample")
                .is_err()
        );
        mailboxes.sender.publish("test", "sample", "wrong type")?;
        assert!(
            mailboxes
                .phase(0.0, |context| context
                    .route(|_, _| Ok(Delivery::After { delay_s: 0.0 })))
                .is_err()
        );
        for delay_s in [-1.0, f64::NAN, f64::INFINITY] {
            let mut mailboxes = TestMailboxes::new()?;
            mailboxes.sender.publish("test", "sample", 1_i32)?;
            assert!(
                mailboxes
                    .phase(0.0, |context| context
                        .route(|_, _| Ok(Delivery::After { delay_s })))
                    .is_err()
            );
        }
        Ok(())
    }

    #[test]
    fn network_plugins_can_publish_and_receive_messages() -> Result<()> {
        let mut mailboxes = TestMailboxes::new()?;
        mailboxes.phase(0.0, |context| {
            context
                .messages()
                .subscribe::<i32>("test", "network_status")?;
            context
                .messages()
                .publish("test", "network_status", 9_i32)?;
            context.route(|_, _| Ok(Delivery::After { delay_s: 0.0 }))?;
            assert_eq!(
                *context
                    .messages()
                    .receive::<i32>("test", "network_status")?[0]
                    .value,
                9
            );
            Ok(())
        })?;
        Ok(())
    }

    #[test]
    fn missing_network_and_repeated_routing_are_errors() -> Result<()> {
        let mut messages = Messages::default();
        messages.subscribe::<i32>("missing", "topic")?;
        assert!(messages.validate_networks(&["GlobalNetwork"]).is_err());
        let mut mailboxes = TestMailboxes::new()?;
        assert!(
            mailboxes
                .phase(0.0, |context| {
                    context.route(|_, _| Ok(Delivery::Drop))?;
                    context.route(|_, _| Ok(Delivery::Drop))
                })
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn delayed_delivery_is_discarded_when_the_receiver_is_removed() -> Result<()> {
        let mut mailboxes = TestMailboxes::new()?;
        mailboxes.sender.publish("test", "sample", 42_i32)?;
        mailboxes.phase(0.0, |context| {
            context.route(|_, _| Ok(Delivery::After { delay_s: 1.0 }))
        })?;
        assert_eq!(mailboxes.scheduled.len(), 2);
        mailboxes.phase(1.0, |context| {
            // Omit the other entity's mailbox, as the engine does after removal.
            let mailboxes = std::mem::take(&mut context.mailboxes);
            context.mailboxes = &mut mailboxes[..2];
            context.route(|_, _| Ok(Delivery::Drop))
        })?;
        assert!(mailboxes.scheduled.is_empty());
        assert_eq!(
            mailboxes
                .same_entity
                .receive::<i32>("test", "sample")?
                .len(),
            1
        );
        assert!(
            mailboxes
                .other_entity
                .receive::<i32>("test", "sample")?
                .is_empty()
        );
        Ok(())
    }

    #[test]
    fn a_network_plugin_keeps_its_mailbox_after_a_failed_route() -> Result<()> {
        let mut mailboxes = TestMailboxes::new()?;
        mailboxes.sender.publish("test", "sample", 1_i32)?;
        mailboxes.phase(0.0, |context| {
            assert!(
                context
                    .route(|_, _| Ok(Delivery::After { delay_s: -1.0 }))
                    .is_err()
            );
            context.messages().publish("test", "status", 1_i32)
        })
    }
}

} // verus!

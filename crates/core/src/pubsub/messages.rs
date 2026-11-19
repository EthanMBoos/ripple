//! Plugin-owned mailboxes: typed subscriptions, publications, and one queue per
//! subscriber. `routing.rs` moves messages between them.
use anyhow::{Result, bail, ensure};
use serde::Serialize;
use std::{collections::BTreeMap, sync::Arc};
use vstd::prelude::*;

pub(crate) mod proof;
// Verus skips this module; proof.rs states what its methods return.
#[cfg_attr(verus_only, verus_verify(external))]
mod payload;
pub use payload::{MessageType, Payload};

verus! {

broadcast use proof::message_facts;

/// One mailbox per plugin instance. Multicast shares immutable payloads, never
/// plugin state. Plugins go through `subscribe`, `publish`, and `receive`.
#[derive(Default)]
pub struct Messages {
    /// The message type each subscribed channel carries.
    pub(crate) subscriptions: BTreeMap<Channel, MessageType>,
    /// Delivered messages by channel, in delivery order, until `receive` drains them.
    pub(crate) inbox: BTreeMap<Channel, Vec<Envelope>>,
    /// Published this tick, until the channel's network takes them.
    pub(crate) outbox: Vec<Publication>,
    /// The current simulation time, set by the engine before each phase.
    pub(crate) time_s: f64,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MessageEndpoint {
    pub entity_id: Option<i32>,
    pub plugin: String,
}
impl Clone for MessageEndpoint {
    fn clone(&self) -> (r: Self)
        ensures r == *self
    {
        MessageEndpoint {
            entity_id: self.entity_id,
            plugin: self.plugin.clone(),
        }
    }
}

pub struct Message<T> {
    pub sender: MessageEndpoint,
    pub sent_at_s: f64,
    pub delivered_at_s: f64,
    pub value: Arc<T>,
}

pub(crate) type Channel = (String, String);

pub struct Publication {
    pub channel: Channel,
    pub sent_at_s: f64,
    pub payload: Payload,
}
impl Publication {
    /// A publication of `payload` on the network's topic, sent at `sent_at_s`.
    fn new(network: &str, topic: &str, sent_at_s: f64, payload: Payload) -> (r: Self)
        ensures proof::publication_of(&r, network, topic, sent_at_s, payload)
    {
        Publication {
            channel: (network.to_owned(), topic.to_owned()),
            sent_at_s,
            payload,
        }
    }
}
/// A delivered message as the mailbox holds it, before a subscriber reads it.
pub struct Envelope {
    pub sender: MessageEndpoint,
    pub sent_at_s: f64,
    pub delivered_at_s: f64,
    pub payload: Payload,
}
impl Envelope {
    /// The message a subscriber reading type `T` gets from this envelope.
    fn open<T: Send + Sync + 'static>(self) -> (message: Result<Message<T>>)
        ensures proof::opened(&self, &message)
    {
        let value = self
            .payload
            .downcast::<T>()
            .ok_or_else(|| anyhow::anyhow!("message type mismatch"))?;
        Ok(Message {
            sender: self.sender,
            sent_at_s: self.sent_at_s,
            delivered_at_s: self.delivered_at_s,
            value,
        })
    }
}
/// The messages of a queue, opened in order.
fn open_all<T: Send + Sync + 'static>(queue: Vec<Envelope>) -> (messages: Result<Vec<Message<T>>>)
    ensures proof::all_opened(&queue, &messages)
{
    let mut messages = Vec::new();
    for envelope in it: queue
        invariant proof::opened_so_far(it.seq(), &messages, it.index())
    {
        messages.push(envelope.open()?);
    }
    Ok(messages)
}

impl Messages {
    pub fn subscribe<T: Send + Sync + 'static>(
        &mut self,
        network: &str,
        topic: &str,
    ) -> (subscribed: Result<()>)
        ensures proof::subscribed::<T>(old(self), final(self), network, topic, &subscribed)
    {
        if network.is_empty() || topic.is_empty() {
            bail!("network and topic must not be empty");
        }
        let channel = (network.to_owned(), topic.to_owned());
        let message_type = MessageType::of::<T>();
        let previous = self.subscriptions.get(&channel);
        if previous.is_some() && previous != Some(&message_type) {
            bail!("subscription changed message type: {network}/{topic}");
        }
        self.subscriptions.insert(channel, message_type);
        Ok(())
    }
    /// Drain this plugin's delivered messages. Other subscribers have independent queues.
    pub fn receive<T: Send + Sync + 'static>(
        &mut self,
        network: &str,
        topic: &str,
    ) -> (messages: Result<Vec<Message<T>>>)
        ensures proof::received(old(self), final(self), network, topic, &messages)
    {
        let channel = (network.to_owned(), topic.to_owned());
        if self.subscriptions.get(&channel) != Some(&MessageType::of::<T>()) {
            bail!("missing or differently typed subscription: {network}/{topic}");
        }
        open_all(self.inbox.remove(&channel).unwrap_or_default())
    }
    /// Appends a delivered envelope to the channel's queue.
    /// Engine lifecycle events are published by the engine's own mailboxes.
    pub(crate) fn publish_engine_event<T: Serialize + Send + Sync + 'static>(
        &mut self,
        network: &str,
        topic: &str,
        value: T,
    )
        ensures proof::published::<T>(old(self), final(self), network, topic, value)
    {
        self.outbox
            .push(Publication::new(network, topic, self.time_s, Payload::new(value)));
    }
    pub(crate) fn enqueue(&mut self, channel: Channel, envelope: Envelope)
        ensures proof::queued(old(self), final(self), &channel, &envelope)
    {
        match self.inbox.get_mut(&channel) {
            Some(queue) => queue.push(envelope),
            None => {
                self.inbox.insert(channel, vec![envelope]);
            }
        }
    }
    /// Appends this network's publications, in order, each with `sender`, to
    /// `publications`; the others stay in the outbox.
    pub(crate) fn take_publications<'m>(
        &mut self,
        network: &str,
        sender: &'m MessageEndpoint,
        publications: &mut Vec<(&'m MessageEndpoint, Publication)>,
    )
        ensures proof::took_publications(old(self), final(self), network, sender, old(publications)@, final(publications)@)
    {
        // Swapped out rather than taken: Verus has no contract for `mem::take`.
        let mut outbox = Vec::new();
        std::mem::swap(&mut self.outbox, &mut outbox);
        let mut others = Vec::new();
        for publication in it: outbox
            invariant proof::split_so_far(it.seq(), network@, sender, old(publications)@, publications@, others@, it.index())
        {
            if publication.channel.0.as_str() == network {
                publications.push((sender, publication));
            } else {
                others.push(publication);
            }
        }
        self.outbox = others;
    }
}

#[verifier::external]
impl Messages {
    pub fn publish<T: Serialize + Send + Sync + 'static>(
        &mut self,
        network: &str,
        topic: &str,
        value: T,
    ) -> Result<()> {
        ensure!(
            !network.is_empty() && !topic.is_empty(),
            "network and topic must not be empty"
        );
        self.publish_engine_event(network, topic, value);
        Ok(())
    }
    /// No subscriptions and nothing to send: networks have nothing to do here.
    pub(crate) fn is_idle(&self) -> bool {
        self.subscriptions.is_empty() && self.outbox.is_empty()
    }
    pub(crate) fn validate_networks(&self, names: &[&str]) -> Result<()> {
        for (network, _) in self
            .subscriptions
            .keys()
            .chain(self.outbox.iter().map(|message| &message.channel))
        {
            ensure!(
                names.contains(&network.as_str()),
                "unconfigured network '{network}'"
            );
        }
        Ok(())
    }
}

} // verus!

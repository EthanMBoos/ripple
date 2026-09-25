//! Plugin-owned mailboxes; only the coordinator routes immutable message payloads.
use crate::{
    EntitySnapshot,
    common::PluginRandom,
    plugin::{StepTime, Update},
};
use anyhow::{Result, ensure};
use std::{
    any::{Any, TypeId},
    collections::BTreeMap,
    sync::Arc,
};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MessageEndpoint {
    pub entity_id: Option<i32>,
    pub plugin: String,
}

pub struct Message<T> {
    pub sender: MessageEndpoint,
    pub sent_at_s: f64,
    pub delivered_at_s: f64,
    pub value: Arc<T>,
}

type Channel = (String, String);
type Payload = Arc<dyn Any + Send + Sync>;

// Message counts, not byte limits. Fail clearly instead of silently losing data.
// Keep these ordinary constants until a real experiment needs per-queue tuning.
const OUTBOX_CAPACITY: usize = 1024;
const SUBSCRIBER_CAPACITY: usize = 1024;
const NETWORK_CAPACITY: usize = 65_536;

struct Publication {
    channel: Channel,
    sent_at_s: f64,
    value: Payload,
}
struct Envelope {
    sender: MessageEndpoint,
    sent_at_s: f64,
    delivered_at_s: f64,
    value: Payload,
}

/// One mailbox per plugin instance. Multicast shares immutable payloads, never plugin state.
#[derive(Default)]
pub struct Messages {
    subscriptions: BTreeMap<Channel, TypeId>,
    inbox: BTreeMap<Channel, Vec<Envelope>>,
    outbox: Vec<Publication>,
    pub(crate) time_s: f64,
}
impl Messages {
    pub fn subscribe<T: Send + Sync + 'static>(
        &mut self,
        network: &str,
        topic: &str,
    ) -> Result<()> {
        ensure!(
            !network.is_empty() && !topic.is_empty(),
            "network and topic must not be empty"
        );
        let channel = (network.to_owned(), topic.to_owned());
        if let Some(previous) = self.subscriptions.get(&channel) {
            ensure!(
                *previous == TypeId::of::<T>(),
                "subscription changed message type: {network}/{topic}"
            );
        }
        self.subscriptions.insert(channel, TypeId::of::<T>());
        Ok(())
    }
    pub fn publish<T: Send + Sync + 'static>(
        &mut self,
        network: &str,
        topic: &str,
        value: T,
    ) -> Result<()> {
        ensure!(
            !network.is_empty() && !topic.is_empty(),
            "network and topic must not be empty"
        );
        ensure!(
            self.outbox.len() < OUTBOX_CAPACITY,
            "publisher queue full ({OUTBOX_CAPACITY} messages), publishing {network}/{topic}"
        );
        self.outbox.push(Publication {
            channel: (network.to_owned(), topic.to_owned()),
            sent_at_s: self.time_s,
            value: Arc::new(value),
        });
        Ok(())
    }
    /// Drain this plugin's delivered messages. Other subscribers have independent queues.
    pub fn receive<T: Send + Sync + 'static>(
        &mut self,
        network: &str,
        topic: &str,
    ) -> Result<Vec<Message<T>>> {
        let channel = (network.to_owned(), topic.to_owned());
        ensure!(
            self.subscriptions.get(&channel) == Some(&TypeId::of::<T>()),
            "missing or differently typed subscription: {network}/{topic}"
        );
        self.inbox
            .remove(&channel)
            .unwrap_or_default()
            .into_iter()
            .map(|message| {
                let value = message
                    .value
                    .downcast::<T>()
                    .map_err(|_| anyhow::anyhow!("message type mismatch"))?;
                Ok(Message {
                    sender: message.sender,
                    sent_at_s: message.sent_at_s,
                    delivered_at_s: message.delivered_at_s,
                    value,
                })
            })
            .collect()
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

pub struct Transmission<'a> {
    pub time: StepTime,
    pub sender: &'a MessageEndpoint,
    pub receiver: &'a MessageEndpoint,
    pub topic: &'a str,
    pub contacts_truth: &'a [EntitySnapshot],
}
#[derive(Clone, Copy, Debug)]
pub enum Delivery {
    Drop,
    After { delay_s: f64 },
}
pub(crate) struct Mailbox<'a> {
    pub endpoint: MessageEndpoint,
    pub messages: &'a mut Messages,
}
pub(crate) struct ScheduledMessage {
    receiver: MessageEndpoint,
    channel: Channel,
    envelope: Envelope,
}

/// Networks decide topology, loss and delay; the engine owns queues and delivery.
pub struct NetworkContext<'a, 'mailbox> {
    pub time: StepTime,
    pub contacts_truth: &'a [EntitySnapshot],
    pub(crate) name: &'a str,
    pub(crate) endpoint: &'a MessageEndpoint,
    pub(crate) mailboxes: &'a mut [Mailbox<'mailbox>],
    pub(crate) scheduled: &'a mut Vec<ScheduledMessage>,
    pub(crate) random: &'a mut PluginRandom,
    pub(crate) routed: bool,
}
impl NetworkContext<'_, '_> {
    /// This network plugin can also publish and subscribe like every other plugin type.
    pub fn messages(&mut self) -> &mut Messages {
        self.mailboxes
            .iter_mut()
            .find(|mailbox| &mailbox.endpoint == self.endpoint)
            .expect("network plugin has a registered mailbox")
            .messages
    }
    pub fn route(
        &mut self,
        mut decide: impl FnMut(&Transmission<'_>, &mut PluginRandom) -> Result<Delivery>,
    ) -> Result<Update> {
        ensure!(!self.routed, "network may route once per phase");
        self.routed = true;
        // Publisher order is stable entity/slot order, never worker completion order.
        let mut publications = Vec::new();
        for mailbox in self.mailboxes.iter_mut() {
            let mut other_networks = Vec::new();
            for publication in mailbox.messages.outbox.drain(..) {
                if publication.channel.0 == self.name {
                    publications.push((mailbox.endpoint.clone(), publication));
                } else {
                    other_networks.push(publication);
                }
            }
            mailbox.messages.outbox = other_networks;
        }
        for (sender, publication) in publications {
            for mailbox in self.mailboxes.iter() {
                let Some(expected_type) = mailbox.messages.subscriptions.get(&publication.channel)
                else {
                    continue;
                };
                ensure!(
                    *expected_type == publication.value.as_ref().type_id(),
                    "message type mismatch on {}/{}",
                    publication.channel.0,
                    publication.channel.1
                );
                let link = Transmission {
                    time: self.time,
                    sender: &sender,
                    receiver: &mailbox.endpoint,
                    topic: &publication.channel.1,
                    contacts_truth: self.contacts_truth,
                };
                if let Delivery::After { delay_s } = decide(&link, self.random)? {
                    ensure!(
                        delay_s.is_finite() && delay_s >= 0.0,
                        "network delay must be finite and nonnegative"
                    );
                    let delivered_at_s = self.time.time_s + delay_s;
                    ensure!(delivered_at_s.is_finite(), "network delivery time overflow");
                    ensure!(
                        self.scheduled.len() < NETWORK_CAPACITY,
                        "network '{}' queue full ({NETWORK_CAPACITY} deliveries)",
                        self.name
                    );
                    self.scheduled.push(ScheduledMessage {
                        receiver: mailbox.endpoint.clone(),
                        channel: publication.channel.clone(),
                        envelope: Envelope {
                            sender: sender.clone(),
                            sent_at_s: publication.sent_at_s,
                            delivered_at_s,
                            value: Arc::clone(&publication.value),
                        },
                    });
                }
            }
        }
        let mut pending = Vec::new();
        for mut message in self.scheduled.drain(..) {
            let Some(receiver) = self
                .mailboxes
                .iter_mut()
                .find(|mailbox| mailbox.endpoint == message.receiver)
            else {
                // A removed entity cannot receive delayed messages.
                continue;
            };
            if message.envelope.delivered_at_s <= self.time.time_s + 1e-12 {
                message.envelope.delivered_at_s = self.time.time_s;
                let inbox = receiver
                    .messages
                    .inbox
                    .entry(message.channel.clone())
                    .or_default();
                ensure!(
                    inbox.len() < SUBSCRIBER_CAPACITY,
                    "subscriber {:?} queue full ({SUBSCRIBER_CAPACITY} messages) on {}/{}",
                    receiver.endpoint,
                    message.channel.0,
                    message.channel.1
                );
                inbox.push(message.envelope);
            } else {
                pending.push(message);
            }
        }
        *self.scheduled = pending;
        Ok(Update::Applied)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Params,
        plugin::{
            Network, Plugin, PluginParams,
            network::{GlobalNetwork, LocalNetwork},
        },
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
            run: impl FnOnce(&mut NetworkContext<'_, '_>) -> Result<Update>,
        ) -> Result<()> {
            let endpoint = MessageEndpoint {
                entity_id: None,
                plugin: "test".into(),
            };
            let mut network_messages = Messages::default();
            let mut mailboxes = [
                Mailbox {
                    endpoint: MessageEndpoint {
                        entity_id: Some(1),
                        plugin: "sensor".into(),
                    },
                    messages: &mut self.sender,
                },
                Mailbox {
                    endpoint: MessageEndpoint {
                        entity_id: Some(1),
                        plugin: "autonomy".into(),
                    },
                    messages: &mut self.same_entity,
                },
                Mailbox {
                    endpoint: MessageEndpoint {
                        entity_id: Some(2),
                        plugin: "autonomy".into(),
                    },
                    messages: &mut self.other_entity,
                },
                Mailbox {
                    endpoint: endpoint.clone(),
                    messages: &mut network_messages,
                },
            ];
            run(&mut NetworkContext {
                time: StepTime { time_s, dt_s: 0.1 },
                contacts_truth: &[],
                name: "test",
                endpoint: &endpoint,
                mailboxes: &mut mailboxes,
                scheduled: &mut self.scheduled,
                random: &mut PluginRandom::new(1, 0, "test"),
                routed: false,
            })?;
            Ok(())
        }
    }

    #[test]
    fn local_network_is_same_entity_and_global_network_is_not() -> Result<()> {
        let params = Params::new();
        let mut local = LocalNetwork::new(&LocalNetwork::configure(&PluginParams::new(&params))?);
        let mut global =
            GlobalNetwork::new(&GlobalNetwork::configure(&PluginParams::new(&params))?);
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
            Ok(Update::Applied)
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
    fn publisher_overflow_is_an_error_without_discarding_queued_messages() -> Result<()> {
        let mut messages = Messages::default();
        for value in 0..OUTBOX_CAPACITY {
            messages.publish("test", "sample", value)?;
        }
        let error = messages.publish("test", "sample", 0).unwrap_err();
        assert!(error.to_string().contains("publisher queue full"));
        assert_eq!(messages.outbox.len(), OUTBOX_CAPACITY);
        Ok(())
    }

    #[test]
    fn a_slow_subscriber_fills_only_its_own_queue_then_errors() -> Result<()> {
        let mut mailboxes = TestMailboxes::new()?;
        for value in 0..SUBSCRIBER_CAPACITY {
            mailboxes.sender.publish("test", "sample", value as i32)?;
            mailboxes.phase(value as f64, |context| {
                context.route(|_, _| Ok(Delivery::After { delay_s: 0.0 }))
            })?;
            let fast = mailboxes.same_entity.receive::<i32>("test", "sample")?;
            assert_eq!(fast.len(), 1);
            assert_eq!(*fast[0].value, value as i32);
        }
        mailboxes.sender.publish("test", "sample", -1_i32)?;
        let error = mailboxes
            .phase(2000.0, |context| {
                context.route(|_, _| Ok(Delivery::After { delay_s: 0.0 }))
            })
            .unwrap_err();
        assert!(error.to_string().contains("subscriber"));
        assert!(error.to_string().contains("queue full"));
        let slow = mailboxes.other_entity.receive::<i32>("test", "sample")?;
        assert_eq!(slow.len(), SUBSCRIBER_CAPACITY);
        assert_eq!(*slow[0].value, 0);
        Ok(())
    }

    #[test]
    fn pending_network_deliveries_have_a_capacity() -> Result<()> {
        let mut mailboxes = TestMailboxes::new()?;
        for _ in 0..NETWORK_CAPACITY {
            mailboxes.scheduled.push(ScheduledMessage {
                receiver: MessageEndpoint {
                    entity_id: Some(1),
                    plugin: "autonomy".into(),
                },
                channel: ("test".into(), "sample".into()),
                envelope: Envelope {
                    sender: MessageEndpoint {
                        entity_id: Some(1),
                        plugin: "sensor".into(),
                    },
                    sent_at_s: 0.0,
                    delivered_at_s: 100.0,
                    value: Arc::new(42_i32),
                },
            });
        }
        mailboxes.sender.publish("test", "sample", 1_i32)?;
        let error = mailboxes
            .phase(0.0, |context| {
                context.route(|_, _| Ok(Delivery::After { delay_s: 1.0 }))
            })
            .unwrap_err();
        assert!(error.to_string().contains("network 'test' queue full"));
        assert_eq!(mailboxes.scheduled.len(), NETWORK_CAPACITY);
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
}

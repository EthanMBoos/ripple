//! The observation trace's network records (see `simulation/trace.rs`). Plain
//! Rust that Verus skips; `proof.rs` declares its two methods as trusted calls
//! that promise nothing. A record keeps the payload and is serialized when the
//! trace is written, so recording cannot fail a routing phase.
use super::ScheduledMessage;
use crate::pubsub::messages::{MessageEndpoint, Payload};
use anyhow::Result;
use serde_json::Value;

#[derive(Default)]
pub(crate) struct NetworkTrace {
    pub deliveries: Vec<DeliveryRecord>,
}
/// One message reaching one subscriber, for the observation trace.
pub(crate) struct DeliveryRecord {
    pub time_s: f64,
    pub network: String,
    pub topic: String,
    pub sender: MessageEndpoint,
    pub receiver: MessageEndpoint,
    pub payload: Payload,
    /// Set when the message was queued with a positive delay.
    pub deliver_at_s: Option<f64>,
}
impl DeliveryRecord {
    /// The payload as JSON, for the trace file.
    pub(crate) fn payload_json(&self) -> Result<Value> {
        self.payload.to_json()
    }
    /// Entity IDs for engine lifecycle and collision events.
    pub(crate) fn entity_ids(&self) -> Option<&[i32]> {
        self.payload
            .downcast_ref::<crate::Event>()
            .map(|event| event.entity_ids.as_slice())
    }
}
impl NetworkTrace {
    /// Records a message queued with a positive delay; one due now is recorded
    /// only when it is delivered.
    pub(crate) fn scheduled(&mut self, time_s: f64, delay_s: f64, message: &ScheduledMessage) {
        if delay_s > 0.0 {
            self.record(time_s, message, Some(message.envelope.delivered_at_s));
        }
    }
    /// Records a message reaching its receiver.
    pub(crate) fn delivered(&mut self, time_s: f64, message: &ScheduledMessage) {
        self.record(time_s, message, None);
    }
    fn record(&mut self, time_s: f64, message: &ScheduledMessage, deliver_at_s: Option<f64>) {
        self.deliveries.push(DeliveryRecord {
            time_s,
            network: message.channel.0.clone(),
            topic: message.channel.1.clone(),
            sender: message.envelope.sender.clone(),
            receiver: message.receiver.clone(),
            payload: message.envelope.payload.clone(),
            deliver_at_s,
        });
    }
}

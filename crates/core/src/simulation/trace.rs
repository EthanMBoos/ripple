//! Opt-in observation records for the MCAP writer.
//!
//! These records name Ripple's state, sensor, and message fields directly.
//! `reference/traces.py` converts them to SCRIMMAGE's comparison shape.
//! Tracing only reads simulation state, so the run's ordinary outputs are
//! identical with it on or off.

use anyhow::Result;
use serde_json::{Value, json};

use crate::{
    entity::Entity,
    math::{KinematicState, Vec3},
    pubsub::messages::{MessageEndpoint, NetworkTrace},
};

/// Receives one observation without a temporary JSON-lines file or parser.
pub trait TraceSink {
    fn record(&mut self, kind: &str, time_s: f64, value: Value) -> Result<()>;
}

pub(crate) struct Trace<'a> {
    out: &'a mut dyn TraceSink,
}

impl<'a> Trace<'a> {
    pub(crate) fn new(out: &'a mut dyn TraceSink) -> Self {
        Self { out }
    }

    fn record(&mut self, kind: &str, time_s: f64, value: Value) -> Result<()> {
        self.out.record(kind, time_s, value)
    }

    pub(crate) fn beliefs(&mut self, time_s: f64, entities: &[Entity]) -> Result<()> {
        for entity in entities {
            let state: &KinematicState = entity.belief();
            let contacts: Vec<Value> = entity
                .contacts_belief()
                .iter()
                .filter(|contact| contact.id != entity.id())
                .map(|contact| {
                    json!({
                        "entity_id": contact.id,
                        "team_id": contact.team_id,
                        "sampled_at_s": number(contact.sampled_at_s),
                        "state": state_record(&contact.state),
                    })
                })
                .collect();
            self.record(
                "belief",
                time_s,
                json!({
                    "kind": "belief",
                    "time_s": number(time_s),
                    "entity_id": entity.id(),
                    "team_id": entity.team_id(),
                    "state": state_record(state),
                    "contacts": contacts,
                }),
            )?;
        }
        Ok(())
    }

    pub(crate) fn outputs(&mut self, time_s: f64, entities: &[Entity]) -> Result<()> {
        for entity in entities {
            for (plugin, port, value) in entity.plugin_outputs() {
                self.record(
                    "output",
                    time_s,
                    json!({
                        "kind": "output",
                        "time_s": number(time_s),
                        "entity_id": entity.id(),
                        "plugin": plugin,
                        "port": port,
                        "value": number(value),
                    }),
                )?;
            }
        }
        Ok(())
    }

    /// Each own-state and contact reading the sensors took this tick.
    pub(crate) fn readings(&mut self, time_s: f64, entities: &[Entity]) -> Result<()> {
        for entity in entities {
            for (sensor, states) in entity.pending_measurements()? {
                let states: Vec<Value> = states
                    .into_iter()
                    .map(|(id, state)| json!({"entity_id": id, "state": state_record(state)}))
                    .collect();
                self.record(
                    "reading",
                    time_s,
                    json!({
                        "kind": "reading",
                        "time_s": number(time_s),
                        "sensor": endpoint(sensor),
                        "states": states,
                    }),
                )?;
            }
        }
        Ok(())
    }

    /// Deliveries from one network phase.
    pub(crate) fn network(&mut self, network: &NetworkTrace) -> Result<()> {
        for delivery in &network.deliveries {
            let kind = if delivery.deliver_at_s.is_some() {
                "scheduled_delivery"
            } else {
                "delivery"
            };
            let mut record = json!({
                "kind": kind,
                "time_s": number(delivery.time_s),
                "network": delivery.network,
                "topic": delivery.topic,
                "sender": endpoint(&delivery.sender),
                "receiver": endpoint(&delivery.receiver),
            });
            if let Some(deliver_at_s) = delivery.deliver_at_s {
                record["deliver_at_s"] = number(deliver_at_s);
            }
            if let Some(ids) = &delivery.entity_ids {
                record["entity_ids"] = json!(ids);
            }
            record["payload"] = delivery.payload.clone();
            self.record(kind, delivery.time_s, record)?;
        }
        Ok(())
    }
}

fn vector(v: Vec3) -> [Value; 3] {
    [number(v.x), number(v.y), number(v.z)]
}

fn state_record(state: &KinematicState) -> Value {
    let q = state.orientation_world_from_body;
    json!({
        "position_world_m": vector(state.position_world_m),
        "velocity_world_mps": vector(state.velocity_world_mps),
        "orientation_world_from_body": [number(q.w), number(q.x), number(q.y), number(q.z)],
        "angular_velocity_world_radps": vector(state.angular_velocity_world_radps),
    })
}

fn endpoint(endpoint: &MessageEndpoint) -> Value {
    json!({"entity_id": endpoint.entity_id, "plugin": endpoint.plugin})
}

/// JSON has no NaN or infinity; keep them explicit for the comparison reader.
fn number(value: f64) -> Value {
    if value.is_finite() {
        json!(value)
    } else if value.is_nan() {
        json!("NaN")
    } else if value > 0.0 {
        json!("Infinity")
    } else {
        json!("-Infinity")
    }
}

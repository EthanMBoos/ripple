//! Opt-in comparison trace, one JSON object per line.
//!
//! The records match the C++ `benchmarking-edits` branch
//! (`src/common/Trace.cpp`) so `reference/traces.py` can compare them:
//! message deliveries, sensor readings, each entity's belief when its frame is
//! logged, and autonomy/controller outputs after all controller substeps.
//! A reading record holds what C++ publishes as a sensor message, which
//! `reference/traces.py` pairs with it. Tracing only reads simulation state, so
//! outputs are identical with it on or off.

use std::io::Write;

use anyhow::Result;
use serde_json::{Value, json};

use crate::{
    entity::Entity,
    math::{KinematicState, Vec3},
    pubsub::messages::{MessageEndpoint, NetworkTrace},
};

pub(crate) struct Trace {
    out: Box<dyn Write + Send>,
}

impl Trace {
    pub(crate) fn new(out: Box<dyn Write + Send>) -> Self {
        Self { out }
    }

    fn record(&mut self, value: Value) -> Result<()> {
        serde_json::to_writer(&mut self.out, &value)?;
        self.out.write_all(b"\n")?;
        Ok(())
    }

    pub(crate) fn beliefs(&mut self, time_s: f64, entities: &[Entity]) -> Result<()> {
        for entity in entities {
            let state: &KinematicState = entity.belief();
            let q = state.orientation_world_from_body;
            self.record(json!({
                "kind": "belief",
                "t": number(time_s),
                "id": entity.id(),
                // Rust-only: lets reference/metrics_model.py map entities to teams.
                "team": entity.team_id(),
                "p": vector(state.position_world_m),
                "v": vector(state.velocity_world_mps),
                "q": [number(q.w), number(q.x), number(q.y), number(q.z)],
                "w": vector(state.angular_velocity_world_radps),
            }))?;
        }
        Ok(())
    }

    pub(crate) fn outputs(&mut self, time_s: f64, entities: &[Entity]) -> Result<()> {
        for entity in entities {
            for (plugin, port, value) in entity.plugin_outputs() {
                self.record(json!({
                    "kind": "output",
                    "t": time_s,
                    "id": entity.id(),
                    "plugin": plugin,
                    "port": port,
                    "value": number(value),
                }))?;
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
                    .map(|(id, state)| state_record(id, state))
                    .collect();
                self.record(json!({
                    "kind": "reading",
                    "t": time_s,
                    "from": endpoint(sensor),
                    "states": states,
                }))?;
            }
        }
        Ok(())
    }

    /// Deliveries from one network phase.
    pub(crate) fn network(&mut self, network: &NetworkTrace) -> Result<()> {
        for delivery in &network.deliveries {
            let mut record = json!({
                "kind": if delivery.deliver_at_s.is_some() { "scheduled_delivery" } else { "delivery" },
                "t": delivery.time_s,
                "network": delivery.network,
                "topic": delivery.topic,
                "from": endpoint(&delivery.sender),
                "to": endpoint(&delivery.receiver),
            });
            if let Some(deliver_at_s) = delivery.deliver_at_s {
                record["deliver_at"] = json!(deliver_at_s);
            }
            if let Some(ids) = &delivery.entity_ids {
                record["ids"] = json!(ids);
            }
            self.record(record)?;
        }
        Ok(())
    }

    pub(crate) fn flush(&mut self) -> Result<()> {
        self.out.flush()?;
        Ok(())
    }
}

fn vector(v: Vec3) -> [Value; 3] {
    [number(v.x), number(v.y), number(v.z)]
}

fn state_record(id: Option<i32>, state: &KinematicState) -> Value {
    let q = state.orientation_world_from_body;
    json!({
        "id": id,
        "p": vector(state.position_world_m),
        "v": vector(state.velocity_world_mps),
        "q": [number(q.w), number(q.x), number(q.y), number(q.z)],
        "w": vector(state.angular_velocity_world_radps),
    })
}

/// `[entity_id or null, plugin name]` using C++ plugin names: Rust endpoints are
/// `category/Name:slot`, and C++ names sensors `Name<slot>` and others `Name`.
fn endpoint(endpoint: &MessageEndpoint) -> Value {
    let (category, rest) = endpoint
        .plugin
        .split_once('/')
        .unwrap_or(("", endpoint.plugin.as_str()));
    let (name, slot) = rest.split_once(':').unwrap_or((rest, ""));
    let name = if category == "sensor" {
        format!("{name}{slot}")
    } else {
        name.to_owned()
    };
    json!([endpoint.entity_id, name])
}

/// JSON has no NaN or infinity; both traces write them as strings.
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

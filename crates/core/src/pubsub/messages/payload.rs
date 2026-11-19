//! The published value as the engine carries it: shared by every receiver,
//! with its type erased, and able to write itself as JSON for the trace; and
//! the type a subscription records. Verus skips this module; proof.rs states
//! what its methods return.
use anyhow::Result;
use serde::Serialize;
use serde_json::Value;
use std::{
    any::{Any, TypeId},
    sync::Arc,
};

/// A published value, shared by every receiver of the message.
#[derive(Clone)]
pub struct Payload {
    value: Arc<dyn Any + Send + Sync>,
    /// Writes the value as JSON, remembering the type it had when published.
    to_json: fn(&(dyn Any + Send + Sync)) -> Result<Value>,
}
impl Payload {
    pub(super) fn new<T: Serialize + Send + Sync + 'static>(value: T) -> Self {
        Self {
            value: Arc::new(value),
            to_json: serialize_as::<T>,
        }
    }
    /// The shared value, if it has type `T`.
    pub(super) fn downcast<T: Send + Sync + 'static>(self) -> Option<Arc<T>> {
        self.value.downcast().ok()
    }
    pub(crate) fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        self.value.downcast_ref()
    }
    /// The type the value was published as.
    pub(crate) fn message_type(&self) -> MessageType {
        MessageType(self.value.as_ref().type_id())
    }
    pub(crate) fn to_json(&self) -> Result<Value> {
        (self.to_json)(self.value.as_ref())
    }
}

fn serialize_as<T: Serialize + 'static>(value: &(dyn Any + Send + Sync)) -> Result<Value> {
    Ok(serde_json::to_value(
        value.downcast_ref::<T>().expect("payload type changed"),
    )?)
}

/// The message type a subscription expects, recorded when the plugin subscribes
/// and checked against every publication routed to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MessageType(TypeId);
impl MessageType {
    pub(super) fn of<T: 'static>() -> Self {
        Self(TypeId::of::<T>())
    }
}

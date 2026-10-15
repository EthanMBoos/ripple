//! Typed publications, independent subscriber queues, and network delivery.
pub(crate) mod messages;

pub use messages::{Delivery, Message, MessageEndpoint, Messages, NetworkContext, Transmission};

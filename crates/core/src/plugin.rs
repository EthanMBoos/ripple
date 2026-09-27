//! Stable plugin-authoring imports and the single concrete implementation tree.
//! Category interfaces live in framework modules; this facade keeps author imports simple.
pub mod autonomy;
pub mod controller;
pub mod estimator;
pub mod interaction;
pub mod metrics;
pub mod motion;
pub mod network;
pub mod sensor;

pub use crate::autonomy::Autonomy;
pub use crate::common::{Frame, PluginIo, PluginRandom, Port, Ports, Unit};
pub use crate::controller::Controller;
pub use crate::entity_interaction::{Interaction, InteractionContext};
pub use crate::estimator::{
    Commands, Contact, ContactList, Input, Inputs, KnownContacts, Localization,
    LocalizationContext, Sample, Tracking, TrackingContext,
};
pub use crate::metrics::{MetricReport, Metrics, TeamMetrics};
pub use crate::motion::{MotionContext, MotionModel};
pub use crate::plugin_manager::{
    PluginRegistry,
    entity_plugin::{
        AgentContext, EntityInfo, Plugin, PluginParams, StepTime, Update, WorldContext,
    },
};
pub use crate::pubsub::{
    Delivery, Message, MessageEndpoint, Messages, Network, NetworkContext, Transmission,
};
pub use crate::sensor::{Reading, ReadingKind, Readings, Sensor, SensorContext};

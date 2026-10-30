//! The plugin API: the shared configuration and contexts here; each
//! category's trait and built-ins in `plugin/<category>.rs`; registration in
//! `plugin/registry.rs`. Authors import everything from `ripple_core::plugin`.
//!
//! A plugin is one type with one trait, its category's (`Autonomy`, `Sensor`,
//! ...). The trait has two halves. The first says how the plugin is built:
//! `Config`, `configure`, `new`, and the facts the engine needs before any
//! instance exists (`ports`, `inputs`, `reading_kind`). The second is what it
//! does each tick: `initialize`, `step`, `close`.
//!
//! Why the engine holds plugins as trait objects (`Box<dyn Autonomy>`). The
//! mission file names the plugin at run time, and this crate is compiled
//! before user plugin crates exist, so the code that steps a vehicle is
//! written once for a list of plugins it cannot see. A plugin is written in
//! its own crate without editing core, and a mission is a file changed
//! without recompiling. Drop either goal and an enum of every plugin with a
//! `match` would be plainer.
//!
//! Why the first half is marked `where Self: Sized`. A trait object can only
//! carry methods that take `self`; it cannot name an associated type, call a
//! constructor, or return `Self`. Marking those items `where Self: Sized`
//! leaves them out of the object, so the trait can still be boxed. They are
//! answered once from the concrete type, by `PluginRegistry::register_*`
//! while the mission loads, and the box built there only ever steps. The
//! alternative is one `init(&mut self, params)` on an empty object, which
//! would parse per instance and could not check ports before anything is
//! spawned. `Config` is parsed and checked once per mission and shared by
//! every instance; a plugin's own fields are per-entity state.
pub mod autonomy;
pub mod controller;
pub mod estimator;
pub mod interaction;
pub mod metrics;
pub mod motion;
pub mod network;
mod registry;
pub mod sensor;
#[cfg(test)]
mod tests;

pub use crate::common::{Pairs, PluginRandom, SpatialIndex, TruthContacts};
pub use crate::ports::{Frame, PluginIo, Port, Ports, Unit};
pub use crate::pubsub::{
    Delivery, Message, MessageEndpoint, Messages, NetworkContext, Transmission,
};
pub use autonomy::Autonomy;
pub use controller::Controller;
pub use estimator::{
    Commands, Contact, ContactList, Input, Inputs, KnownContacts, Localization,
    LocalizationContext, Sample, Tracking, TrackingContext,
};
pub use interaction::{Interaction, InteractionContext};
pub use metrics::{MetricReport, Metrics, TeamMetrics};
pub use motion::{MotionContext, MotionModel};
pub use network::Network;
pub use registry::PluginRegistry;
pub(crate) use registry::{Catalog, CompiledPlugin};
pub use sensor::{Reading, ReadingKind, Readings, Sensor, SensorContext};

use crate::{KinematicState, common::Terrain, scenario::PluginConfig};
use anyhow::Result;
use serde::de::DeserializeOwned;
use serde_yaml_ng::{Mapping, Value};
use std::{borrow::Cow, collections::BTreeMap};

/// A plugin's mission parameters, as its mission file gave them.
pub struct PluginParams<'a> {
    values: Cow<'a, Mapping>,
    pub(crate) loop_rate_hz: f64,
}
impl<'a> PluginParams<'a> {
    /// A plugin's parameters from a resolved mission; tools that read a
    /// mission, such as the ArduPilot SITL launcher, pass them to `configure`.
    pub fn new(config: &'a PluginConfig) -> Self {
        Self {
            values: Cow::Borrowed(&config.params),
            loop_rate_hz: config.loop_rate_hz,
        }
    }

    /// Parameters written as YAML, for unit tests of a single plugin.
    #[cfg(test)]
    pub(crate) fn yaml(text: &str) -> PluginParams<'static> {
        PluginParams {
            values: Cow::Owned(serde_yaml_ng::from_str(text).expect("test parameters are a map")),
            loop_rate_hz: 0.0,
        }
    }

    /// (key, YAML value) pairs, for unit tests: `("mass_kg", "1.5")`,
    /// `("airframe", "crazyflie2")`, `("speeds_radps", "[800, 800]")`.
    #[cfg(test)]
    pub(crate) fn pairs(pairs: &[(&str, &str)]) -> PluginParams<'static> {
        let values = pairs
            .iter()
            .map(|(key, text)| {
                let value = serde_yaml_ng::from_str(text).expect("test values are YAML");
                (Value::from(*key), value)
            })
            .collect();
        PluginParams {
            values: Cow::Owned(values),
            loop_rate_hz: 0.0,
        }
    }

    /// Fills a `#[derive(Deserialize)]` parameter struct. Give the struct
    /// `#[serde(default, deny_unknown_fields)]` so omitted keys take its `Default`
    /// and a misspelled key is an error instead of being silently ignored.
    pub fn parse<T: DeserializeOwned>(&self) -> Result<T> {
        if self.values.is_empty() {
            // No values also fits a plugin whose config is `()`, which a map cannot express.
            return serde_yaml_ng::from_value(Value::Null)
                .or_else(|_| serde_yaml_ng::from_value(Value::Mapping(Mapping::new())))
                .map_err(|error| anyhow::anyhow!("{error}"));
        }
        // Going through text makes serde_yaml name the key in a type error. Its
        // line numbers would refer to that text, not the mission file, so drop them.
        serde_yaml_ng::from_str(&serde_yaml_ng::to_string(&*self.values)?).map_err(|error| {
            let message = error.to_string();
            let message = message.split(" at line ").next().unwrap_or(&message);
            if message.contains("expected unit") {
                anyhow::anyhow!("this plugin takes no parameters")
            } else {
                anyhow::anyhow!("{message}")
            }
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct StepTime {
    /// Reference phase time, not wall time.
    pub time_s: f64,
    /// This phase's integration interval (smaller during controller/motion substeps).
    pub dt_s: f64,
    /// The tick, counted from 0 at the start time (-1 for entities created
    /// one step before it). The same in every phase and substep of a tick.
    pub tick: i64,
}

#[derive(Clone, Copy, Debug)]
pub struct EntityInfo {
    pub id: i32,
    pub team_id: i32,
    /// Index of the mission entity group this entity came from.
    pub group_index: i32,
}

/// What an interaction's or a metrics plugin's `step` answers: `Stop` ends the
/// run after this tick (`TerminationReason::PluginRequestedStop`). The entity
/// slots and networks have no such answer; their `step` returns `Result<()>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Update {
    Applied,
    Stop,
}

/// Autonomies and controllers see the entity's belief only: its estimated own
/// state and the contacts it knows about, as its estimators last wrote them.
/// Neither can be mutated through this context.
pub struct AgentContext<'a> {
    pub messages: &'a mut Messages,
    pub entity: EntityInfo,
    pub time: StepTime,
    /// Own-state belief, from localization.
    pub state: &'a KinematicState,
    /// Known contacts, from tracking.
    pub contacts: KnownContacts<'a>,
    /// This plugin instance's random numbers at this tick.
    pub random: PluginRandom,
}

pub struct WorldContext<'a> {
    pub time: StepTime,
    pub contacts_truth: TruthContacts<'a>,
    /// Retains team membership after an entity has been removed.
    pub entity_teams: &'a BTreeMap<i32, i32>,
    pub messages: &'a mut Messages,
    /// The mission's ground heights, if it names terrain. `Terrain` is cheap to
    /// clone, so a plugin can keep it from `initialize`.
    pub terrain: Option<&'a Terrain>,
}

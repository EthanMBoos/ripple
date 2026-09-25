//! Shared plugin lifecycle, configuration, and execution contexts.
//! Category-specific interfaces live in the corresponding framework modules.
use crate::{
    EntitySnapshot, KinematicState, Params,
    common::{PluginRandom, Ports},
    parse::{PluginConfig, PluginValues},
    pubsub::Messages,
    sensor::Observations,
};
use anyhow::Result;
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;

/// A plugin's mission parameters, as its mission file gave them.
pub struct PluginParams<'a> {
    values: Values<'a>,
    pub(crate) loop_rate_hz: f64,
}
enum Values<'a> {
    Text(&'a Params),
    Yaml(&'a serde_yaml_ng::Mapping),
}
impl<'a> PluginParams<'a> {
    pub(crate) fn new(config: &'a PluginConfig) -> Self {
        let values = match &config.params {
            PluginValues::Text(params) => Values::Text(params),
            PluginValues::Yaml(params) => Values::Yaml(params),
        };
        Self {
            values,
            loop_rate_hz: config.loop_rate_hz,
        }
    }

    /// XML-style text values, for unit tests of a single plugin.
    #[cfg(test)]
    pub(crate) fn text(params: &'a Params) -> Self {
        Self {
            values: Values::Text(params),
            loop_rate_hz: 0.0,
        }
    }

    /// Fills a `#[derive(Deserialize)]` parameter struct. Give the struct
    /// `#[serde(default, deny_unknown_fields)]` so omitted keys take its `Default`
    /// and a misspelled key is an error instead of being silently ignored.
    pub fn parse<T: DeserializeOwned>(&self) -> Result<T> {
        match self.values {
            Values::Text(params) => crate::parse::deserialize(params),
            // No values: the same as an XML tag without attributes. This also
            // covers plugins whose config is `()`, which YAML cannot express.
            Values::Yaml(params) if params.is_empty() => crate::parse::deserialize(&Params::new()),
            // Going through text makes serde_yaml name the key in a type error. Its
            // line numbers would refer to that text, not the mission file, so drop them.
            Values::Yaml(params) => serde_yaml_ng::from_str(&serde_yaml_ng::to_string(params)?)
                .map_err(|error| {
                    let message = error.to_string();
                    let message = message.split(" at line ").next().unwrap_or(&message);
                    if message.contains("expected unit") {
                        anyhow::anyhow!("this plugin takes no parameters")
                    } else {
                        anyhow::anyhow!("{message}")
                    }
                }),
        }
    }
}

/// Immutable, validated configuration is distinct from per-entity mutable plugin state.
pub trait Plugin: Send + Sized + 'static {
    type Config: Send + Sync + 'static;
    fn configure(params: &PluginParams<'_>) -> Result<Self::Config>;
    fn new(config: &Self::Config) -> Self;
    fn close(&mut self, _time: StepTime) -> Result<()> {
        Ok(())
    }
    fn ports(_config: &Self::Config) -> Ports {
        Ports::default()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct StepTime {
    /// Reference phase time, not wall time.
    pub time_s: f64,
    /// This phase's integration interval (smaller during controller/motion substeps).
    pub dt_s: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct EntityInfo {
    pub id: i32,
    pub team_id: i32,
    pub sub_swarm_id: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Update {
    Applied,
    Stop,
}

/// Autonomies and controllers receive state and delivered observations.
/// State reads the entity's owned belief, or truth when no estimate is installed.
/// Neither can be mutated through this context.
/// Truth-backed contacts are explicitly named for legacy SCRIMMAGE compatibility.
pub struct AgentContext<'a> {
    pub messages: &'a mut Messages,
    pub entity: EntityInfo,
    pub time: StepTime,
    pub state: &'a KinematicState,
    pub observations: &'a Observations,
    pub contacts_truth: &'a [EntitySnapshot],
    /// This plugin instance's mission-seeded random stream.
    pub random: &'a mut PluginRandom,
}

pub struct WorldContext<'a> {
    pub time: StepTime,
    pub contacts_truth: &'a [EntitySnapshot],
    /// Retains team membership after an entity has been removed.
    pub entity_teams: &'a BTreeMap<i32, i32>,
    pub messages: &'a mut Messages,
}

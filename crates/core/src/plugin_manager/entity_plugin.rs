//! Shared plugin lifecycle, configuration, and execution contexts.
//! Category-specific interfaces live in the corresponding framework modules.
use crate::{
    EntitySnapshot, KinematicState, Params,
    common::{PluginRandom, Ports},
    pubsub::Messages,
    sensor::Observations,
};
use anyhow::Result;
use serde::{Serialize, de::DeserializeOwned};
use std::cell::RefCell;
use std::collections::BTreeMap;

/// A plugin's mission parameters: its tag's attributes, `param_common` groups, and
/// any `SCRIMMAGE_PLUGIN_PATH` overlay file, all still as text.
pub struct PluginParams<'a> {
    pub(crate) text: &'a Params,
    /// The parsed values, defaults included, for the run manifest.
    pub(crate) effective: RefCell<serde_json::Value>,
}
impl<'a> PluginParams<'a> {
    pub(crate) fn new(text: &'a Params) -> Self {
        Self {
            text,
            effective: RefCell::new(serde_json::Value::Null),
        }
    }

    /// Fills a `#[derive(Deserialize, Serialize)]` parameter struct. Give the struct
    /// `#[serde(default, deny_unknown_fields)]` so omitted keys take its `Default`
    /// and a misspelled key is an error instead of being silently ignored.
    pub fn parse<T: DeserializeOwned + Serialize>(&self) -> Result<T> {
        let value: T = crate::parse::deserialize(self.text)?;
        *self.effective.borrow_mut() = serde_json::to_value(&value)?;
        Ok(value)
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

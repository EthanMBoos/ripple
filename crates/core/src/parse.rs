//! Legacy mission XML parsing and validated scenario configuration.
mod mission;
mod params;
mod xml;
mod xml_mission;
mod yaml_mission;

pub(crate) use mission::{EndConditions, EntityConfig, PluginConfig, PluginValues};
pub use mission::{ResolvedScenario, ScenarioConfig};
pub use params::Params;
pub(crate) use params::{boolean, deserialize, integer, number, vector};

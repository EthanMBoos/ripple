//! Legacy mission XML parsing and validated scenario configuration.
mod mission;
mod params;
mod xml;

pub(crate) use mission::{EndConditions, EntityConfig, PluginConfig};
pub use mission::{ResolvedScenario, ScenarioConfig};
pub use params::Params;
pub(crate) use params::{boolean, deserialize, integer, number, vector};

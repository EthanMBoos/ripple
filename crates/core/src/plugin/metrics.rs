//! Metrics: the plugin interface, its reports, and the built-in scoring.
use crate::plugin::{PluginParams, StepTime, Update, WorldContext};
use anyhow::Result;
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;

#[derive(Default, Clone, Debug)]
pub struct TeamMetrics {
    pub score: f64,
    pub values: BTreeMap<String, f64>,
}
#[derive(Default, Clone, Debug)]
pub struct MetricReport {
    pub headers: Vec<String>,
    pub teams: BTreeMap<i32, TeamMetrics>,
}
pub trait Metrics: Send {
    // How it is built, answered from the concrete type while the mission
    // loads. `where Self: Sized` keeps these out of `Box<dyn Metrics>`, which
    // only steps; see the module doc in `plugin.rs`.
    /// Immutable, validated configuration, parsed once per mission and shared
    /// by every instance.
    type Config: DeserializeOwned + Send + Sync + 'static
    where
        Self: Sized;
    /// Parses the mission's parameters; override it to check them.
    fn configure(params: &PluginParams) -> Result<Self::Config>
    where
        Self: Sized,
    {
        params.parse()
    }
    fn new(config: &Self::Config) -> Self
    where
        Self: Sized;

    fn initialize(&mut self, _context: &mut WorldContext) -> Result<()> {
        Ok(())
    }
    fn step(&mut self, context: &mut WorldContext) -> Result<Update>;
    fn report(&self, time_s: f64) -> MetricReport;
    fn close(&mut self, _time: StepTime) -> Result<()> {
        Ok(())
    }
}

// Built-ins selected by missions.
#[path = "metrics/simple_collision_metrics/simple_collision_metrics.rs"]
mod simple_collision_metrics;
pub use simple_collision_metrics::SimpleCollisionMetrics;

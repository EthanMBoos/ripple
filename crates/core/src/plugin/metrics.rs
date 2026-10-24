//! Metrics: the plugin interface, its reports, and the built-in scoring.
use crate::plugin::{Plugin, Update, WorldContext};
use anyhow::Result;
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
pub trait Metrics: Plugin {
    fn initialize(&mut self, _context: &mut WorldContext<'_>) -> Result<()> {
        Ok(())
    }
    fn step(&mut self, context: &mut WorldContext<'_>) -> Result<Update>;
    fn report(&self, time_s: f64) -> MetricReport;
}

// Built-ins selected by missions.
#[path = "metrics/simple_collision_metrics/simple_collision_metrics.rs"]
mod simple_collision_metrics;
pub use simple_collision_metrics::SimpleCollisionMetrics;

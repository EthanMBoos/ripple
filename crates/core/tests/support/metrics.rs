//! Statistics from delivered messages; no access to an interaction's private state.
use super::interaction::Population;
use anyhow::Result;
use scrimmage_core::plugin::{
    MetricReport, Metrics, Plugin, PluginParams, StepTime, TeamMetrics, Update, WorldContext,
};
use std::collections::BTreeMap;

pub struct PopulationMetrics {
    samples: usize,
    entity_samples: usize,
    steps: usize,
    closed: bool,
}
impl Plugin for PopulationMetrics {
    type Config = ();
    fn configure(params: &PluginParams<'_>) -> Result<()> {
        params.parse()
    }
    fn new(_config: &()) -> Self {
        Self {
            samples: 0,
            entity_samples: 0,
            steps: 0,
            closed: false,
        }
    }
    fn close(&mut self, _time: StepTime) -> Result<()> {
        self.closed = true;
        Ok(())
    }
}
impl Metrics for PopulationMetrics {
    fn initialize(&mut self, context: &mut WorldContext<'_>) -> Result<()> {
        context
            .messages
            .subscribe::<Population>("ExampleNetwork", "population")
    }
    fn step(&mut self, context: &mut WorldContext<'_>) -> Result<Update> {
        for message in context
            .messages
            .receive::<Population>("ExampleNetwork", "population")?
        {
            self.samples += 1;
            self.entity_samples += message.value.entity_count;
        }
        self.steps += 1;
        Ok(Update::Applied)
    }
    fn report(&self, _time_s: f64) -> MetricReport {
        MetricReport {
            headers: ["samples", "entity_samples", "steps", "closed"]
                .map(str::to_owned)
                .to_vec(),
            teams: BTreeMap::from([(
                1,
                TeamMetrics {
                    score: self.samples as f64,
                    values: BTreeMap::from([
                        ("samples".into(), self.samples as f64),
                        ("entity_samples".into(), self.entity_samples as f64),
                        ("steps".into(), self.steps as f64),
                        ("closed".into(), f64::from(self.closed)),
                    ]),
                },
            )]),
        }
    }
}

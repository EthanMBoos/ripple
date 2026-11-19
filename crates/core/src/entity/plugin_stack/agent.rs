//! Each plugin kind as the engine steps it: a few lines that forward to the
//! plugin's own trait. Plain Rust; Verus skips the forwarding, and proof.rs
//! gives each `step` its one contract, that the plugin was called.
use super::{Agent, LocalizationStep, MotionStep, SensorStep, TrackingStep};
use crate::plugin::{
    AgentContext, Autonomy, Controller, Interaction, InteractionContext, Localization,
    LocalizationContext, Metrics, MotionContext, MotionModel, PluginIo, Sensor, SensorContext,
    Tracking, TrackingContext, Update, WorldContext,
};
use crate::simulation::world_plugins::{InteractionStep, MetricsStep};
use anyhow::Result;

impl Agent for dyn Autonomy {
    fn initialize(&mut self, context: &mut AgentContext<'_>) -> Result<()> {
        Autonomy::initialize(self, context)
    }
    fn step(&mut self, context: &mut AgentContext<'_>, io: &mut PluginIo) -> Result<()> {
        Autonomy::step(self, context, io)
    }
}
impl Agent for dyn Controller {
    fn initialize(&mut self, context: &mut AgentContext<'_>) -> Result<()> {
        Controller::initialize(self, context)
    }
    fn step(&mut self, context: &mut AgentContext<'_>, io: &mut PluginIo) -> Result<()> {
        Controller::step(self, context, io)
    }
}
impl MotionStep for dyn MotionModel {
    fn step(&mut self, context: &mut MotionContext<'_>, io: &mut PluginIo) -> Result<()> {
        MotionModel::step(self, context, io)
    }
}
impl SensorStep for dyn Sensor {
    fn step(&mut self, context: &mut SensorContext<'_>) -> Result<()> {
        Sensor::step(self, context)
    }
}
impl LocalizationStep for dyn Localization {
    fn step(&mut self, context: &mut LocalizationContext<'_>) -> Result<()> {
        Localization::step(self, context)
    }
}
impl TrackingStep for dyn Tracking {
    fn step(&mut self, context: &mut TrackingContext<'_>) -> Result<()> {
        Tracking::step(self, context)
    }
}
impl InteractionStep for dyn Interaction {
    fn step(&mut self, context: &mut InteractionContext<'_>) -> Result<Update> {
        Interaction::step(self, context)
    }
}
impl MetricsStep for dyn Metrics {
    fn step(&mut self, context: &mut WorldContext<'_>) -> Result<Update> {
        Metrics::step(self, context)
    }
}

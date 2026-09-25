//! Minimal test-only models for public plugin, messaging, and lifecycle contracts.
pub(super) mod autonomy;
pub(super) mod controller;
pub(super) mod interaction;
pub(super) mod metrics;
pub(super) mod motion;
pub(super) mod network;
mod perception_communication;
pub(super) mod sensor;

use anyhow::Result;
use scrimmage_core::{Mission, Params, Simulation, plugin::PluginRegistry};
use std::path::PathBuf;

pub(super) fn registry() -> Result<PluginRegistry> {
    let mut registry = PluginRegistry::with_builtins();
    registry.register_autonomy::<autonomy::DriveToGoal>("DriveToGoal")?;
    registry.register_controller::<controller::ScaleSpeed>("ScaleSpeed")?;
    registry.register_motion::<motion::EastwardMotion>("EastwardMotion")?;
    registry.register_sensor::<sensor::PositionSensor>("PositionSensor")?;
    registry.register_interaction::<interaction::Floor>("Floor")?;
    registry.register_network::<network::ExampleNetwork>("ExampleNetwork")?;
    registry.register_metrics::<metrics::PopulationMetrics>("PopulationMetrics")?;
    Ok(registry)
}
pub(super) fn scenario(
    overrides: &Params,
    registry: &PluginRegistry,
) -> Result<scrimmage_core::ScenarioConfig> {
    let core = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = core.join("../..");
    Ok(Mission::load_with_registry(
        &core.join("tests/fixtures/plugin_contracts.xml"),
        &root,
        overrides,
        registry,
    )?
    .scenario)
}
pub(super) fn simulate(workers: usize, overrides: &Params) -> Result<Simulation> {
    let mut simulation =
        Simulation::new(scenario(overrides, &registry()?)?, &registry()?, workers)?;
    while simulation.step()?.is_some() {}
    Ok(simulation)
}

//! A mission-level test: run the whole mission with the same plugins as the
//! `ripple` command, and check the outcome.
use anyhow::Result;
use ripple_core::{EventKind, Mission, Params, Simulation, plugin::PluginRegistry};
use std::path::Path;

#[test]
fn chaser_catches_the_target() -> Result<()> {
    let crate_folder = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut registry = PluginRegistry::with_builtins();
    starter::register(&mut registry)?;
    let mission = Mission::load(
        &crate_folder.join("missions/follow-nearest.yaml"),
        crate_folder,
        &Params::new(),
    )?;
    let mut simulation = Simulation::new(mission.scenario, &registry, 1)?;
    while simulation.step()?.is_some() {}

    let caught = simulation
        .events()
        .iter()
        .any(|event| event.kind == EventKind::NonTeamCollision);
    assert!(caught, "the chaser never reached the target");
    Ok(())
}

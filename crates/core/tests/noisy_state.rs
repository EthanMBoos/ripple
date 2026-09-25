use std::path::PathBuf;

use anyhow::Result;
use scrimmage_core::{Mission, Params, Simulation};

fn simulation(altitude_bias: f64, sensor_rate: f64) -> Result<Simulation> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let overrides = Params::from([
        ("position_sigma".into(), "0".into()),
        ("velocity_sigma".into(), "0".into()),
        ("altitude_bias".into(), altitude_bias.to_string()),
        ("sensor_rate".into(), sensor_rate.to_string()),
    ]);
    let mission = Mission::load(&root.join("missions/noisy-state.xml"), &root, &overrides)?;
    Simulation::new(
        mission.scenario,
        &scrimmage_core::plugin::PluginRegistry::with_builtins(),
        2,
    )
}

#[test]
fn biased_belief_does_not_change_truth_until_the_next_control_update() -> Result<()> {
    let mut ideal = simulation(0.0, 10.0)?;
    let mut biased = simulation(5.0, 10.0)?;
    ideal.step()?;
    biased.step()?;
    for (ideal, biased) in ideal.entities().iter().zip(biased.entities()) {
        assert_eq!(
            ideal.truth().position_world_m,
            biased.truth().position_world_m
        );
        assert_eq!(
            biased.belief().position_world_m.z - biased.truth().position_world_m.z,
            5.0
        );
    }
    ideal.step()?;
    biased.step()?;
    // The actual built-in PID sees the biased altitude and commands a descent.
    for (ideal, biased) in ideal.entities().iter().zip(biased.entities()) {
        assert!(biased.truth().position_world_m.z < ideal.truth().position_world_m.z);
    }
    Ok(())
}

#[test]
fn slower_sensor_holds_owned_belief_while_truth_keeps_moving() -> Result<()> {
    let mut simulation = simulation(5.0, 1.0)?;
    simulation.step()?;
    let belief = simulation.entities()[0].belief().position_world_m;
    let truth = simulation.entities()[0].truth().position_world_m;
    simulation.step()?;
    assert_eq!(simulation.entities()[0].belief().position_world_m, belief);
    assert_ne!(simulation.entities()[0].truth().position_world_m, truth);
    Ok(())
}

#[test]
fn no_sensor_keeps_ideal_feedback() -> Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mission = Mission::load(
        &root.join("missions/straight-no-gui.xml"),
        &root,
        &Params::new(),
    )?;
    let mut simulation = Simulation::new(
        mission.scenario,
        &scrimmage_core::plugin::PluginRegistry::with_builtins(),
        1,
    )?;
    for _ in 0..3 {
        simulation.step()?;
        for entity in simulation.entities() {
            assert!(std::ptr::eq(entity.belief(), entity.truth()));
        }
    }
    Ok(())
}

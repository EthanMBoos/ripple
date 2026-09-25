use std::path::PathBuf;

use anyhow::Result;
use scrimmage_core::{Mission, Params, Simulation, Vec3};

fn run(name: &str, update_at: f64) -> Result<Simulation> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let overrides = Params::from([("update_at".into(), update_at.to_string())]);
    let mission = Mission::load(&root.join("missions").join(name), &root, &overrides)?;
    let mut simulation = Simulation::new(
        mission.scenario,
        &scrimmage_core::plugin::PluginRegistry::with_builtins(),
        2,
    )?;
    while simulation.step()?.is_some() {}
    Ok(simulation)
}

#[test]
fn both_point_agents_receive_the_replacement_route_and_stop_at_its_end() -> Result<()> {
    for (update_at, destination) in [
        (3.0, Vec3::new(-10.0, 20.0, 0.0)),
        (100.0, Vec3::new(10.0, 10.0, 0.0)),
    ] {
        let simulation = run("waypoints-point-agents.xml", update_at)?;
        assert_eq!(simulation.entities().len(), 2);
        for entity in simulation.entities() {
            assert!((entity.truth().position_world_m - destination).norm() < 0.01);
            assert_eq!(entity.truth().velocity_world_mps, Vec3::zeros());
        }
    }
    Ok(())
}

#[test]
fn shared_route_update_changes_both_aircraft_paths() -> Result<()> {
    let updated = run("waypoints-aircraft.xml", 12.0)?;
    let original = run("waypoints-aircraft.xml", 100.0)?;
    assert_eq!(updated.entities().len(), 2);
    for (updated, original) in updated.entities().iter().zip(original.entities()) {
        assert!(
            (updated.truth().position_world_m - original.truth().position_world_m).norm() > 100.0
        );
    }
    Ok(())
}

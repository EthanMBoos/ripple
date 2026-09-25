use std::path::PathBuf;

use anyhow::Result;
use scrimmage_core::{EventKind, Mission, Params, Simulation};

fn run(overrides: Params) -> Result<Simulation> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mission = Mission::load(
        &root.join("missions/networks-local-global.xml"),
        &root,
        &overrides,
    )?;
    let mut simulation = Simulation::new(
        mission.scenario,
        &scrimmage_core::plugin::PluginRegistry::with_builtins(),
        2,
    )?;
    while simulation.step()?.is_some() {}
    Ok(simulation)
}

#[test]
fn global_boundary_changes_both_aircraft_paths_and_ground_hits_reach_metrics() -> Result<()> {
    let bounded = run(Params::new())?;
    let straight = run(Params::from([("boundary_control".into(), "false".into())]))?;
    assert_eq!(bounded.entities().len(), 2);
    for (bounded, straight) in bounded.entities().iter().zip(straight.entities()) {
        assert!(
            bounded.truth().position_world_m.x.abs() < straight.truth().position_world_m.x.abs()
        );
        assert!(bounded.truth().position_world_m.y.abs() > 1.0);
    }
    let hits: Vec<_> = bounded
        .events()
        .iter()
        .filter(|event| event.kind == EventKind::GroundCollision)
        .collect();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].entity_ids, vec![3]);
    assert!(
        bounded
            .events()
            .iter()
            .any(|event| event.kind == EventKind::EntityRemoved && event.entity_ids == vec![3])
    );
    assert_eq!(
        bounded.metric_reports()[0].1.teams[&3].values["ground_coll"],
        1.0
    );
    Ok(())
}

#[test]
fn ground_collision_respects_team_filter() -> Result<()> {
    let simulation = run(Params::from([("ground_team".into(), "1".into())]))?;
    assert_eq!(simulation.entities().len(), 3);
    assert!(
        !simulation
            .events()
            .iter()
            .any(|event| event.kind == EventKind::GroundCollision)
    );
    Ok(())
}

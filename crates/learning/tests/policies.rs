//! The shipped policies, replayed in Ripple from their ONNX files, do their
//! tasks on mission seeds Python never trained on.

use ripple_core::plugin::PluginRegistry;
use ripple_core::{Mission, Params, Simulation, Vec3};
use std::path::Path;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

/// Runs a learning mission to its end and returns the final positions.
fn final_positions(mission: &str, plugin: &str, entity: &str, seed: u32) -> Vec<Vec3> {
    let root = Path::new(ROOT);
    let mut overrides = Params::new();
    // The missions name their policies relative to the repository root.
    let policy = root.join(format!("crates/learning/policies/{mission}.onnx"));
    overrides.insert(
        format!("entities.{entity}.autonomy.{plugin}.policy"),
        policy.display().to_string(),
    );
    overrides.insert("run.seed".into(), seed.to_string());
    let path = root.join(format!("crates/learning/missions/{mission}.yaml"));
    let scenario = Mission::load(&path, root, &overrides).unwrap().scenario;
    let mut registry = PluginRegistry::with_builtins();
    learning::register(&mut registry).unwrap();
    let mut simulation = Simulation::new(scenario, &registry, 1).unwrap();
    while simulation.step().unwrap().is_some() {}
    simulation
        .entities()
        .iter()
        .map(|entity| entity.truth().position_world_m)
        .collect()
}

#[test]
fn the_hover_policy_holds_the_goal() {
    for seed in 100..105 {
        let [position_m] = final_positions("hover", "HoverPolicy", "drone", seed)[..] else {
            panic!("one drone");
        };
        let miss_m = (position_m - Vec3::new(0.0, 0.0, 5.0)).norm();
        assert!(miss_m < 0.2, "seed {seed}: {miss_m:.3} m from the goal");
    }
}

#[test]
fn the_spread_team_covers_every_landmark() {
    let landmarks_m = [[-4.0, -3.0], [4.0, -3.0], [0.0, 4.0]];
    for seed in 100..105 {
        let positions_m = final_positions("spread", "SpreadPolicy", "team", seed);
        for [east_m, north_m] in landmarks_m {
            let gap_m = positions_m
                .iter()
                .map(|position_m| (position_m.x - east_m).hypot(position_m.y - north_m))
                .fold(f64::INFINITY, f64::min);
            assert!(
                gap_m < 0.5,
                "seed {seed}: landmark [{east_m}, {north_m}] is {gap_m:.3} m from the nearest agent"
            );
        }
    }
}

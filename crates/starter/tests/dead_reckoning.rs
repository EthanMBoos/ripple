//! Run the dead-reckoning mission and compare each tick's belief with the truth.
use anyhow::Result;
use scrimmage_core::{Mission, Params, Simulation, plugin::PluginRegistry};
use std::path::Path;

/// After every tick: time, the dead-reckoning robot's belief error, and how far
/// it really is from the robot with perfect localization.
fn run(overrides: &[(&str, &str)]) -> Result<Vec<(f64, f64, f64)>> {
    let crate_folder = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut registry = PluginRegistry::with_builtins();
    starter::register(&mut registry)?;
    let mut params = Params::new();
    for (name, value) in overrides {
        params.insert(name.to_string(), value.to_string());
    }
    let mission = Mission::load_with_registry(
        &crate_folder.join("missions/dead-reckoning.yaml"),
        crate_folder,
        &params,
        &registry,
    )?;
    let mut simulation = Simulation::new(mission.scenario, &registry, 1)?;
    let mut ticks = Vec::new();
    while simulation.step()?.is_some() {
        // Entities in mission order: the dead-reckoning robot, then the perfect one.
        let [robot, perfect] = simulation.entities() else {
            panic!("the mission has two robots");
        };
        let error_m = (robot.belief().position_world_m - robot.truth().position_world_m).norm();
        let gap_m = (robot.truth().position_world_m - perfect.truth().position_world_m).norm();
        ticks.push((simulation.time_s(), error_m, gap_m));
    }
    Ok(ticks)
}

fn belief_errors(overrides: &[(&str, &str)]) -> Result<Vec<(f64, f64)>> {
    Ok(run(overrides)?
        .iter()
        .map(|&(t, error_m, _)| (t, error_m))
        .collect())
}

fn worst_gap_m(overrides: &[(&str, &str)]) -> Result<f64> {
    Ok(run(overrides)?
        .iter()
        .map(|&(_, _, gap_m)| gap_m)
        .fold(0.0, f64::max))
}

const GAIN: &str = "entities.dead_reckoning.localization.DeadReckoning.speed_gain";

fn error_at(errors: &[(f64, f64)], time_s: f64) -> f64 {
    let (_, error_m) = errors
        .iter()
        .find(|(t, _)| (t - time_s).abs() < 1e-6)
        .expect("a tick at that time");
    *error_m
}

#[test]
fn assuming_the_commanded_speed_drifts_until_the_next_fix() -> Result<()> {
    // It believes it moves at 10 m/s but moves at 8: 0.2 m more error per tick.
    let errors = belief_errors(&[])?;
    assert!(error_at(&errors, 0.1) < 1e-9);
    assert!((error_at(&errors, 2.1) - 4.0).abs() < 1e-9);
    // The fix sampled at 5.1 s replaces the prediction, so 5.0 s is the worst.
    assert!((error_at(&errors, 5.0) - 9.8).abs() < 1e-9);
    assert!(error_at(&errors, 5.1) < 1e-9);
    Ok(())
}

#[test]
fn the_right_speed_gain_predicts_exactly() -> Result<()> {
    let errors = belief_errors(&[(GAIN, "0.8")])?;
    let worst_m = errors.iter().map(|(_, e)| *e).fold(0.0, f64::max);
    assert!(worst_m < 1e-9, "worst error {worst_m} m");
    Ok(())
}

#[test]
fn noisy_fixes_leave_an_error_after_every_fix() -> Result<()> {
    let noise = "[0, 1]";
    let errors = belief_errors(&[
        (GAIN, "0.8"),
        ("entities.dead_reckoning.sensors.gps.pos_noise_0", noise),
        ("entities.dead_reckoning.sensors.gps.pos_noise_1", noise),
    ])?;
    // Right after a fix the error is that fix's noise; the right gain keeps it.
    assert!(error_at(&errors, 5.1) > 0.0);
    assert!((error_at(&errors, 5.1) - error_at(&errors, 10.0)).abs() < 1e-9);
    Ok(())
}

#[test]
fn a_wrong_belief_changes_the_real_path() -> Result<()> {
    // Believing it is ahead, the robot turns early and cuts the square.
    assert!(worst_gap_m(&[])? > 10.0);
    assert!(worst_gap_m(&[(GAIN, "0.8")])? < 1e-9);
    Ok(())
}

//! Run the Kalman filter mission and compare each tick's belief, mean and
//! covariance, with the truth.
use anyhow::Result;
use ripple_core::{Mission, Params, Simulation, plugin::PluginRegistry};
use std::path::Path;
use std::sync::Arc;

/// After every tick: time, the filtering robot's belief error, its reported
/// position standard deviation along x, and how far it really is from the
/// robot with perfect localization.
fn run(overrides: &[(&str, &str)]) -> Result<Vec<(f64, f64, f64, f64)>> {
    let crate_folder = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut registry = PluginRegistry::with_builtins();
    starter::register(&mut registry)?;
    let mut params = Params::new();
    for (name, value) in overrides {
        params.insert(name.to_string(), value.to_string());
    }
    let mission = Mission::load(
        &crate_folder.join("missions/position-kalman.yaml"),
        crate_folder,
        &params,
    )?;
    let mut simulation = Simulation::new(mission.scenario, Arc::new(registry))?;
    let mut ticks = Vec::new();
    while simulation.step()?.is_some() {
        // Entities in mission order: the filtering robot, then the perfect one.
        let [robot, perfect] = simulation.entities() else {
            panic!("the mission has two robots");
        };
        let error_m =
            (robot.belief().mean.position_world_m - robot.truth().position_world_m).norm();
        let sigma_x_m = robot.belief().covariance.position()[(0, 0)].sqrt();
        let gap_m = (robot.truth().position_world_m - perfect.truth().position_world_m).norm();
        ticks.push((simulation.time_s(), error_m, sigma_x_m, gap_m));
    }
    Ok(ticks)
}

fn belief_errors(overrides: &[(&str, &str)]) -> Result<Vec<(f64, f64)>> {
    Ok(run(overrides)?
        .iter()
        .map(|&(t, error_m, _, _)| (t, error_m))
        .collect())
}

fn worst_gap_m(overrides: &[(&str, &str)]) -> Result<f64> {
    Ok(run(overrides)?
        .iter()
        .map(|&(_, _, _, gap_m)| gap_m)
        .fold(0.0, f64::max))
}

const GAIN: &str = "entities.kalman.localization.PositionKalman.speed_gain";
const FIX_NOISE: &str = "entities.kalman.sensors.gps.position_stddev_m";

fn error_at(errors: &[(f64, f64)], time_s: f64) -> f64 {
    let (_, error_m) = errors
        .iter()
        .find(|(t, _)| (t - time_s).abs() < 1e-6)
        .expect("a tick at that time");
    *error_m
}

fn sigma_at(ticks: &[(f64, f64, f64, f64)], time_s: f64) -> f64 {
    ticks
        .iter()
        .find(|(t, ..)| (t - time_s).abs() < 1e-6)
        .map(|&(_, _, sigma_m, _)| sigma_m)
        .expect("a tick at that time")
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
    let errors = belief_errors(&[(GAIN, "0.8"), (FIX_NOISE, "[1, 1, 0]")])?;
    // Right after a fix the error is what the fix left; the right gain keeps it.
    assert!(error_at(&errors, 5.1) > 0.0);
    assert!((error_at(&errors, 5.1) - error_at(&errors, 10.0)).abs() < 1e-9);
    Ok(())
}

#[test]
fn the_covariance_grows_between_fixes_and_collapses_at_an_exact_one() -> Result<()> {
    let ticks = run(&[])?;
    // Exact fixes: certain at each one, then 1 m²/s of process noise adds
    // 0.1 m² a tick, so after 49 predictions sigma is sqrt(4.9).
    assert!(sigma_at(&ticks, 0.1) < 1e-9);
    assert!((sigma_at(&ticks, 5.0) - 4.9f64.sqrt()).abs() < 1e-9);
    assert!(sigma_at(&ticks, 5.1) < 1e-9);
    Ok(())
}

#[test]
fn a_noisy_fix_is_weighed_against_the_prediction() -> Result<()> {
    let ticks = run(&[(GAIN, "0.8"), (FIX_NOISE, "[1, 1, 0]")])?;
    let variance_at = |time_s| sigma_at(&ticks, time_s).powi(2);
    // Between the fixes at 0.1 s and 5.1 s, 49 predictions each add 0.1 m².
    assert!((variance_at(5.0) - (variance_at(0.1) + 4.9)).abs() < 1e-9);
    // At the fix (variance 1 m²), after the tick's own prediction, the
    // precisions add: 1 / P' = 1 / (P + 0.1) + 1 / R.
    let predicted = variance_at(5.0) + 0.1;
    let expected = 1.0 / (1.0 / predicted + 1.0);
    assert!((variance_at(5.1) - expected).abs() < 1e-9);
    assert!(variance_at(5.1) < predicted);
    Ok(())
}

#[test]
fn a_wrong_belief_changes_the_real_path() -> Result<()> {
    // Believing it is ahead, the robot turns early and cuts the square.
    assert!(worst_gap_m(&[])? > 10.0);
    assert!(worst_gap_m(&[(GAIN, "0.8")])? < 1e-9);
    Ok(())
}

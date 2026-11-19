//! Run the GTSAM smoother mission and compare its belief, tick by tick, with
//! the starter's Kalman filter on the same problem: the newest marginal of a
//! linear-Gaussian smoother is the filter's estimate.
use anyhow::Result;
use ripple_core::{GaussianState, Mission, Params, Simulation, Vec3, plugin::PluginRegistry};
use std::path::Path;
use std::sync::Arc;

/// After every tick: time, the smoother's belief, the filter's belief, and
/// the smoother robot's true position.
struct Tick {
    time_s: f64,
    smoother: GaussianState,
    kalman: GaussianState,
    truth_m: Vec3,
}

fn run(overrides: &[(&str, &str)]) -> Result<Vec<Tick>> {
    let crate_folder = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut registry = PluginRegistry::with_builtins();
    starter::register(&mut registry)?;
    gtsam::register(&mut registry)?;
    let mut params = Params::new();
    for (name, value) in overrides {
        params.insert(name.to_string(), value.to_string());
    }
    let mission = Mission::load(
        &crate_folder.join("missions/position-smoother.yaml"),
        crate_folder,
        &params,
    )?;
    let mut simulation = Simulation::new(mission.scenario, Arc::new(registry))?;
    let mut ticks = Vec::new();
    while simulation.step()?.is_some() {
        // Entities in mission order: the smoother, the filter, the perfect one.
        let [smoother, kalman, _perfect] = simulation.entities() else {
            panic!("the mission has three robots");
        };
        ticks.push(Tick {
            time_s: simulation.time_s(),
            smoother: *smoother.belief(),
            kalman: *kalman.belief(),
            truth_m: smoother.truth().position_world_m,
        });
    }
    Ok(ticks)
}

const SMOOTHER: &str = "entities.smoother.localization.GtsamSmoother";
const KALMAN: &str = "entities.kalman.localization.PositionKalman";
const SMOOTHER_FIX_NOISE: &str = "entities.smoother.sensors.gps.position_stddev_m";
const KALMAN_FIX_NOISE: &str = "entities.kalman.sensors.gps.position_stddev_m";

fn sigma_x_m(belief: &GaussianState) -> f64 {
    belief.covariance.position()[(0, 0)].sqrt()
}

/// The largest difference between the two beliefs over the run: in position,
/// and in the position standard deviation along x.
fn worst_disagreement(ticks: &[Tick]) -> (f64, f64) {
    ticks
        .iter()
        .fold((0.0, 0.0), |(position_m, sigma_m), tick| {
            let gap_m =
                (tick.smoother.mean.position_world_m - tick.kalman.mean.position_world_m).norm();
            let sigma_gap_m = (sigma_x_m(&tick.smoother) - sigma_x_m(&tick.kalman)).abs();
            (position_m.max(gap_m), sigma_m.max(sigma_gap_m))
        })
}

fn at(ticks: &[Tick], time_s: f64) -> &Tick {
    ticks
        .iter()
        .find(|tick| (tick.time_s - time_s).abs() < 1e-6)
        .expect("a tick at that time")
}

#[test]
fn the_smoother_agrees_with_the_filter_on_exact_fixes() -> Result<()> {
    // The filter collapses to an exact fix; the smoother's millimetre floor
    // on a fix leaves it a few micrometres and a millimetre of sigma away.
    let (position_m, sigma_m) = worst_disagreement(&run(&[])?);
    assert!(
        position_m < 1e-4,
        "positions differ by up to {position_m} m"
    );
    assert!(sigma_m < 2e-3, "sigmas differ by up to {sigma_m} m");
    Ok(())
}

#[test]
fn the_smoother_agrees_with_the_filter_on_noisy_fixes() -> Result<()> {
    // The two robots draw different fix noise, so their positions differ;
    // the covariance recursion does not depend on the draws and must match
    // to rounding.
    let ticks = run(&[
        (SMOOTHER_FIX_NOISE, "[1, 1, 1]"),
        (KALMAN_FIX_NOISE, "[1, 1, 1]"),
        (&format!("{SMOOTHER}.speed_gain"), "0.8"),
        (&format!("{KALMAN}.speed_gain"), "0.8"),
    ])?;
    let (_, sigma_m) = worst_disagreement(&ticks);
    assert!(sigma_m < 1e-9, "sigmas differ by up to {sigma_m} m");
    // Between the fixes at 0.1 s and 5.1 s, 49 predictions each add 0.1 m².
    let variance_at = |time_s| sigma_x_m(&at(&ticks, time_s).smoother).powi(2);
    assert!((variance_at(5.0) - (variance_at(0.1) + 4.9)).abs() < 1e-9);
    // At the fix (variance 1 m²), after the tick's own prediction, the
    // precisions add: 1 / P' = 1 / (P + 0.1) + 1 / R.
    let predicted = variance_at(5.0) + 0.1;
    assert!((variance_at(5.1) - 1.0 / (1.0 / predicted + 1.0)).abs() < 1e-9);
    Ok(())
}

#[test]
fn a_short_lag_marginalizes_without_changing_the_newest_estimate() -> Result<()> {
    // With a 1 s lag the window holds ten ticks and every fix but the newest
    // has been folded into the prior; the newest estimate is unchanged.
    let (position_m, sigma_m) = worst_disagreement(&run(&[(&format!("{SMOOTHER}.lag_s"), "1")])?);
    assert!(
        position_m < 1e-4,
        "positions differ by up to {position_m} m"
    );
    assert!(sigma_m < 2e-3, "sigmas differ by up to {sigma_m} m");
    Ok(())
}

#[test]
fn assuming_the_commanded_speed_drifts_until_the_next_fix() -> Result<()> {
    // It believes it moves at 10 m/s but moves at 8: 0.2 m more error per
    // tick, until the fix sampled at 5.1 s pulls it back. The millimetre
    // floor on a fix leaves a few micrometres of each tick's error behind.
    let ticks = run(&[])?;
    let error_m = |time_s| {
        let tick = at(&ticks, time_s);
        (tick.smoother.mean.position_world_m - tick.truth_m).norm()
    };
    assert!(
        (error_m(2.1) - 4.0).abs() < 1e-5,
        "error at 2.1 s: {} m",
        error_m(2.1)
    );
    assert!(
        (error_m(5.0) - 9.8).abs() < 1e-5,
        "error at 5.0 s: {} m",
        error_m(5.0)
    );
    assert!(error_m(5.1) < 1e-4, "error at 5.1 s: {} m", error_m(5.1));
    Ok(())
}

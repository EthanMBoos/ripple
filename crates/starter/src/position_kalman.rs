//! A Kalman filter over position for a velocity-commanded vehicle, with
//! occasional position fixes.
//! Localization: predict position from the commanded velocity and grow the
//! covariance by the process noise; on a fix, move toward it by the Kalman
//! gain, which weighs the fix's covariance against the belief's.
//! It estimates position only. Velocity is the commanded one; velocity,
//! attitude, and angular velocity are reported as unknown.
//! This is the estimator from the book's "Your first localization estimator" tutorial.

use anyhow::{Context, Result, ensure};
use ripple_core::plugin::{Input, Localization, LocalizationContext, PluginParams, ReadingKind};
use ripple_core::{GaussianState, KinematicState, Matrix3, StateCovariance, Vec3};
use serde::Deserialize;

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PositionKalmanConfig {
    /// How fast the estimator assumes the vehicle moves, as a fraction of the
    /// commanded velocity. 1.0 assumes it moves exactly as commanded.
    speed_gain: f64,
    /// How fast the filter loses confidence in its prediction: the position
    /// variance it adds per second on each axis, in m²/s, a random walk, so
    /// the added variance depends on the time elapsed and not on how many
    /// substeps it was integrated in. With 0 the filter trusts its prediction
    /// completely, so once it is certain it ignores every fix.
    process_noise_m2ps: f64,
}

impl Default for PositionKalmanConfig {
    fn default() -> Self {
        Self {
            speed_gain: 1.0,
            process_noise_m2ps: 1.0,
        }
    }
}

pub struct PositionKalman {
    speed_gain: f64,
    process_noise_m2ps: f64,
}

impl Localization for PositionKalman {
    type Config = PositionKalmanConfig;

    fn configure(params: &PluginParams) -> Result<PositionKalmanConfig> {
        let config: PositionKalmanConfig = params.parse()?;
        ensure!(
            config.speed_gain >= 0.0,
            "PositionKalman speed_gain must be nonnegative"
        );
        ensure!(
            config.process_noise_m2ps >= 0.0,
            "PositionKalman process_noise_m2ps must be nonnegative"
        );
        Ok(config)
    }

    fn new(config: &PositionKalmanConfig) -> Self {
        Self {
            speed_gain: config.speed_gain,
            process_noise_m2ps: config.process_noise_m2ps,
        }
    }

    fn inputs(_config: &PositionKalmanConfig) -> Vec<Input> {
        // A full-state reading; only its position and position covariance are used.
        vec![Input::new("fix", ReadingKind::State)]
    }

    fn initialize(&mut self, context: &mut LocalizationContext) -> Result<()> {
        // Start from the first fix, at rest, as sure of the position as the
        // fix is; nothing else is estimated, so everything else is unknown.
        let fix = context
            .inputs
            .latest::<GaussianState>("fix")?
            .context("PositionKalman needs a position fix at startup")?;
        let mut covariance = StateCovariance::from_sigmas(
            [0.0; 3],
            [f64::INFINITY; 3],
            [f64::INFINITY; 3],
            [f64::INFINITY; 3],
        );
        covariance.set_position(&fix.value.covariance.position());
        *context.state = GaussianState {
            mean: KinematicState {
                position_world_m: fix.value.mean.position_world_m,
                ..KinematicState::default()
            },
            covariance,
        };
        Ok(())
    }

    fn step(&mut self, context: &mut LocalizationContext) -> Result<()> {
        // Predict: move by the velocity the motion model was told to fly, on each substep.
        for substep in 0..context.commands.substeps() {
            let commanded_world_mps = Vec3::new(
                context.commands.get(substep, "velocity_x")?,
                context.commands.get(substep, "velocity_y")?,
                context.commands.get(substep, "velocity_z")?,
            );
            predict(
                context.state,
                commanded_world_mps * self.speed_gain,
                context.commands.time(substep).dt_s,
                self.process_noise_m2ps,
            );
        }

        // Correct: a new fix pulls the prediction toward it.
        if let Some(fix) = context.inputs.fresh::<GaussianState>("fix")? {
            correct(
                context.state,
                fix.value.mean.position_world_m,
                &fix.value.covariance.position(),
            );
        }
        Ok(())
    }
}

/// One prediction step: the position moves by `velocity_world_mps` for
/// `dt_s`, and its covariance grows by the process noise over that time,
/// `process_noise_m2ps * dt_s` on each axis. Linear in time, so ten steps
/// of a tenth add what one step of the whole adds.
pub fn predict(
    state: &mut GaussianState,
    velocity_world_mps: Vec3,
    dt_s: f64,
    process_noise_m2ps: f64,
) {
    state.mean.velocity_world_mps = velocity_world_mps;
    state.mean.position_world_m += velocity_world_mps * dt_s;
    let grown = state.covariance.position() + Matrix3::identity() * (process_noise_m2ps * dt_s);
    state.covariance.set_position(&grown);
}

/// One correction with a position fix of covariance `fix_covariance_m2`: the
/// Kalman gain `K = P (P + R)^-1` moves the position toward the fix by the
/// share of the error the belief owns, and the covariance shrinks to
/// `(I - K) P`. An exact fix (`R = 0`) replaces the position; an exact
/// belief (`P = 0`) ignores the fix. When both are exact on an axis, the
/// pseudo-inverse gives `K = 0` there and the prediction stands; its cutoff
/// is relative to the matrix, so a belief that is merely tiny still takes an
/// exact fix.
pub fn correct(state: &mut GaussianState, fix_world_m: Vec3, fix_covariance_m2: &Matrix3) {
    let belief = state.covariance.position();
    let innovation_covariance = belief + fix_covariance_m2;
    let gain = belief
        * innovation_covariance
            .pseudo_inverse(1e-12 * innovation_covariance.norm())
            .expect("a 3x3 has a pseudo-inverse");
    state.mean.position_world_m += gain * (fix_world_m - state.mean.position_world_m);
    let corrected = (Matrix3::identity() - gain) * belief;
    state.covariance.set_position(&corrected);
}

#[cfg(test)]
mod tests {
    use super::{correct, predict};
    use ripple_core::plugin::PluginRandom;
    use ripple_core::{GaussianState, Matrix3, Vec3};

    #[test]
    fn an_exact_fix_replaces_the_prediction_and_an_exact_belief_ignores_the_fix() {
        let mut state = GaussianState::default();
        predict(&mut state, Vec3::new(10.0, 0.0, 0.0), 0.1, 1.0);
        assert_eq!(state.mean.position_world_m, Vec3::new(1.0, 0.0, 0.0));
        assert!((state.covariance.position()[(0, 0)] - 0.1).abs() < 1e-15);
        correct(&mut state, Vec3::new(0.8, 0.0, 0.0), &Matrix3::zeros());
        assert!((state.mean.position_world_m.x - 0.8).abs() < 1e-12);
        assert!(state.covariance.position().norm() < 1e-12);
        // Now certain: a noisy fix changes nothing.
        correct(
            &mut state,
            Vec3::new(5.0, 0.0, 0.0),
            &(Matrix3::identity() * 4.0),
        );
        assert!((state.mean.position_world_m.x - 0.8).abs() < 1e-12);
    }

    #[test]
    fn the_added_variance_depends_on_the_time_and_not_on_the_substeps() {
        let mut whole = GaussianState::default();
        predict(&mut whole, Vec3::zeros(), 0.1, 2.0);
        let mut tenths = GaussianState::default();
        for _ in 0..10 {
            predict(&mut tenths, Vec3::zeros(), 0.01, 2.0);
        }
        assert!((whole.covariance.position()[(0, 0)] - 0.2).abs() < 1e-12);
        assert!((tenths.covariance.position()[(0, 0)] - 0.2).abs() < 1e-12);
    }

    #[test]
    fn a_tiny_belief_still_takes_an_exact_fix() {
        let mut state = GaussianState::default();
        state
            .covariance
            .set_position(&(Matrix3::identity() * 1e-14));
        correct(&mut state, Vec3::new(1.0, 0.0, 0.0), &Matrix3::zeros());
        assert!((state.mean.position_world_m.x - 1.0).abs() < 1e-9);
    }

    #[test]
    fn equal_covariances_meet_halfway() {
        let mut state = GaussianState::default();
        state.covariance.set_position(&(Matrix3::identity() * 4.0));
        correct(
            &mut state,
            Vec3::new(2.0, 0.0, 0.0),
            &(Matrix3::identity() * 4.0),
        );
        assert!((state.mean.position_world_m.x - 1.0).abs() < 1e-12);
        assert!((state.covariance.position()[(0, 0)] - 2.0).abs() < 1e-12);
    }

    /// The filter's consistency check from the estimation literature: when the
    /// truth really does wander from the commanded path by the process noise
    /// and the fixes really carry their covariance, the normalized estimation
    /// error squared, `e^T P^-1 e`, averages the number of estimated axes
    /// (three). A filter that is too sure of itself averages more; one that
    /// is too unsure averages less.
    #[test]
    fn the_filter_is_consistent_with_matching_process_and_fix_noise() -> anyhow::Result<()> {
        let (dt_s, process_noise_m2ps, fix_sigma_m, ticks) = (0.1, 1.0, 2.0, 20_000_u64);
        let random = PluginRandom::new(7, 1, "consistency");
        let commanded_world_mps = Vec3::new(3.0, -1.0, 0.5);
        let mut truth_world_m = Vec3::zeros();
        let mut state = GaussianState::default();
        state
            .covariance
            .set_position(&(Matrix3::identity() * (fix_sigma_m * fix_sigma_m)));
        let mut nees_sum = 0.0;
        // The truth's wander each tick: a random walk whose variance grows by
        // the process noise times the time, so each step adds a normal draw
        // with that variance.
        let step_sigma_m = f64::sqrt(process_noise_m2ps * dt_s);
        for tick in 1..=ticks {
            // One number per tick, quantity, and axis.
            let draw = |quantity: u64, axis: u64, sigma: f64| {
                random.normal(&[tick, quantity, axis], 0.0, sigma)
            };
            let wander_world_m = Vec3::new(
                draw(0, 0, step_sigma_m)?,
                draw(0, 1, step_sigma_m)?,
                draw(0, 2, step_sigma_m)?,
            );
            truth_world_m += commanded_world_mps * dt_s + wander_world_m;
            predict(&mut state, commanded_world_mps, dt_s, process_noise_m2ps);
            // A fix every second.
            if tick % 10 == 0 {
                let fix_world_m = truth_world_m
                    + Vec3::new(
                        draw(1, 0, fix_sigma_m)?,
                        draw(1, 1, fix_sigma_m)?,
                        draw(1, 2, fix_sigma_m)?,
                    );
                correct(
                    &mut state,
                    fix_world_m,
                    &(Matrix3::identity() * (fix_sigma_m * fix_sigma_m)),
                );
            }
            let error_m = state.mean.position_world_m - truth_world_m;
            let inverse = state
                .covariance
                .position()
                .try_inverse()
                .expect("the covariance stays positive definite");
            nees_sum += (error_m.transpose() * inverse * error_m)[(0, 0)];
        }
        let nees = nees_sum / ticks as f64;
        // The mean of this many chi-square(3) draws has a standard deviation
        // near 0.02 if they were independent; the errors are correlated from
        // tick to tick, so allow several times that.
        assert!((nees - 3.0).abs() < 0.3, "NEES {nees}");
        Ok(())
    }
}

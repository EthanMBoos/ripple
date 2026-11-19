//! A fixed-lag smoother over position from GTSAM, posed as the problem the
//! starter's PositionKalman filter solves: a vehicle moves by its commanded
//! velocity with a random-walk process noise and takes a position fix now and
//! then. Every tick is a variable. The commanded displacement ties it to the
//! previous one, a fix ties it to the world, and GTSAM solves the window
//! incrementally (iSAM2), folding what leaves the window into a prior. The
//! newest variable's marginal is the belief.
//! It estimates position only. Velocity is the commanded one; velocity,
//! attitude, and angular velocity are reported as unknown.
//!
//! The same linear-Gaussian problem makes the newest marginal equal to the
//! Kalman filter's estimate, which tests/position_smoother.rs checks.

use anyhow::{Context, Result, ensure};
use cxx::UniquePtr;
use ripple_core::plugin::{Input, Localization, LocalizationContext, PluginParams, ReadingKind};
use ripple_core::{GaussianState, KinematicState, Matrix3, StateCovariance, Vec3};
use serde::Deserialize;

use crate::ffi::bridge;

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GtsamSmootherConfig {
    /// How fast the smoother assumes the vehicle moves, as a fraction of the
    /// commanded velocity. 1.0 assumes it moves exactly as commanded.
    speed_gain: f64,
    /// The position variance the motion adds per second on each axis, in
    /// m²/s: the error of a tick's commanded displacement grows with the time
    /// the tick spans. Must be positive; GTSAM inverts it.
    process_noise_m2ps: f64,
    /// How long a tick stays in the window before GTSAM marginalizes it out,
    /// in seconds. The newest estimate is the same for any lag; a longer one
    /// costs more per tick.
    lag_s: f64,
    /// A fix's standard deviation on each axis is raised to at least this, in
    /// m. GTSAM's Gaussian noise model inverts the covariance, so an exact fix
    /// (zero covariance) cannot be a factor as it stands; a millimetre keeps
    /// it exact for any purpose here.
    min_fix_stddev_m: f64,
}

impl Default for GtsamSmootherConfig {
    fn default() -> Self {
        Self {
            speed_gain: 1.0,
            process_noise_m2ps: 1.0,
            lag_s: 10.0,
            min_fix_stddev_m: 1e-3,
        }
    }
}

pub struct GtsamSmoother {
    speed_gain: f64,
    process_noise_m2ps: f64,
    min_fix_stddev_m: f64,
    smoother: UniquePtr<bridge::Smoother>,
    /// The newest variable: the position at the end of the last step.
    latest_key: u64,
}

impl Localization for GtsamSmoother {
    type Config = GtsamSmootherConfig;

    fn configure(params: &PluginParams) -> Result<GtsamSmootherConfig> {
        let config: GtsamSmootherConfig = params.parse()?;
        ensure!(
            config.speed_gain >= 0.0,
            "GtsamSmoother speed_gain must be nonnegative"
        );
        ensure!(
            config.process_noise_m2ps > 0.0,
            "GtsamSmoother process_noise_m2ps must be positive"
        );
        ensure!(config.lag_s > 0.0, "GtsamSmoother lag_s must be positive");
        ensure!(
            config.min_fix_stddev_m > 0.0,
            "GtsamSmoother min_fix_stddev_m must be positive"
        );
        Ok(config)
    }

    fn new(config: &GtsamSmootherConfig) -> Self {
        Self {
            speed_gain: config.speed_gain,
            process_noise_m2ps: config.process_noise_m2ps,
            min_fix_stddev_m: config.min_fix_stddev_m,
            smoother: bridge::new_smoother(config.lag_s),
            latest_key: 0,
        }
    }

    fn inputs(_config: &GtsamSmootherConfig) -> Vec<Input> {
        // A full-state reading; only its position and position covariance are used.
        vec![Input::new("fix", ReadingKind::State)]
    }

    fn initialize(&mut self, context: &mut LocalizationContext) -> Result<()> {
        // The first variable is the first fix, and the fix is its only factor.
        let fix = context
            .inputs
            .latest::<GaussianState>("fix")?
            .context("GtsamSmoother needs a position fix at startup")?;
        let position = array(fix.value.mean.position_world_m);
        let covariance = rows(&self.fix_covariance(&fix.value.covariance.position()));
        self.latest_key = 0;
        let mut smoother = self.smoother.pin_mut();
        smoother
            .as_mut()
            .add_initial(0, context.time.time_s, &position)?;
        smoother.as_mut().add_prior(0, &position, &covariance)?;
        smoother.update()?;
        self.write_belief(context.state, Vec3::zeros())
    }

    fn step(&mut self, context: &mut LocalizationContext) -> Result<()> {
        // The tick's commanded displacement, substep by substep, and the time it spans.
        let mut displacement_m = Vec3::zeros();
        let mut elapsed_s = 0.0;
        let mut velocity_world_mps = Vec3::zeros();
        for substep in 0..context.commands.substeps() {
            let commanded_world_mps = Vec3::new(
                context.commands.get(substep, "velocity_x")?,
                context.commands.get(substep, "velocity_y")?,
                context.commands.get(substep, "velocity_z")?,
            );
            let dt_s = context.commands.time(substep).dt_s;
            velocity_world_mps = commanded_world_mps * self.speed_gain;
            displacement_m += velocity_world_mps * dt_s;
            elapsed_s += dt_s;
        }
        ensure!(
            elapsed_s > 0.0,
            "GtsamSmoother needs a tick with at least one motion substep"
        );

        // A new variable, tied to the last by the displacement; the process
        // noise over the tick is its error.
        let key = self.latest_key + 1;
        let guess = array(context.state.mean.position_world_m + displacement_m);
        let motion_covariance =
            rows(&(Matrix3::identity() * (self.process_noise_m2ps * elapsed_s)));
        // A new fix ties the new variable to the world.
        let fix = context.inputs.fresh::<GaussianState>("fix")?.map(|fix| {
            let covariance = self.fix_covariance(&fix.value.covariance.position());
            (array(fix.value.mean.position_world_m), rows(&covariance))
        });

        let mut smoother = self.smoother.pin_mut();
        smoother
            .as_mut()
            .add_initial(key, context.time.time_s + context.time.dt_s, &guess)?;
        smoother.as_mut().add_between(
            self.latest_key,
            key,
            &array(displacement_m),
            &motion_covariance,
        )?;
        if let Some((position, covariance)) = fix {
            smoother.as_mut().add_prior(key, &position, &covariance)?;
        }
        smoother.update()?;
        self.latest_key = key;
        self.write_belief(context.state, velocity_world_mps)
    }
}

impl GtsamSmoother {
    /// A fix's covariance with each axis's variance raised to the floor.
    fn fix_covariance(&self, covariance_m2: &Matrix3) -> Matrix3 {
        let floor_m2 = self.min_fix_stddev_m * self.min_fix_stddev_m;
        let mut floored = *covariance_m2;
        for axis in 0..3 {
            floored[(axis, axis)] = floored[(axis, axis)].max(floor_m2);
        }
        floored
    }

    /// The newest variable's solution and marginal become the belief.
    fn write_belief(&self, state: &mut GaussianState, velocity_world_mps: Vec3) -> Result<()> {
        let mut position = [0.0; 3];
        self.smoother.estimate(self.latest_key, &mut position)?;
        let mut covariance = [0.0; 9];
        self.smoother.marginal(self.latest_key, &mut covariance)?;
        let mut state_covariance = StateCovariance::from_sigmas(
            [0.0; 3],
            [f64::INFINITY; 3],
            [f64::INFINITY; 3],
            [f64::INFINITY; 3],
        );
        state_covariance.set_position(&matrix(&covariance));
        *state = GaussianState {
            mean: KinematicState {
                position_world_m: Vec3::from(position),
                velocity_world_mps,
                ..KinematicState::default()
            },
            covariance: state_covariance,
        };
        Ok(())
    }
}

fn array(vector: Vec3) -> [f64; 3] {
    [vector.x, vector.y, vector.z]
}

/// A 3x3 as the row-major array the bridge carries.
fn rows(matrix: &Matrix3) -> [f64; 9] {
    let mut rows = [0.0; 9];
    for row in 0..3 {
        for column in 0..3 {
            rows[3 * row + column] = matrix[(row, column)];
        }
    }
    rows
}

fn matrix(rows: &[f64; 9]) -> Matrix3 {
    let mut matrix = Matrix3::zeros();
    for row in 0..3 {
        for column in 0..3 {
            matrix[(row, column)] = rows[3 * row + column];
        }
    }
    matrix
}

#[cfg(test)]
mod tests {
    use super::{matrix, rows};
    use ripple_core::Matrix3;

    #[test]
    fn a_matrix_crosses_the_bridge_row_by_row_and_comes_back() {
        let original = Matrix3::new(1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0);
        let crossed = rows(&original);
        assert_eq!(crossed, [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]);
        assert_eq!(matrix(&crossed), original);
    }
}

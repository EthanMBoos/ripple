//! Dead reckoning with occasional position fixes, for a velocity-commanded vehicle.
//! Localization: predict position from the commanded velocity, replace it on a fix.
//! It estimates position and velocity only; attitude stays at the default.
//! This is the estimator from the book's "Your first localization estimator" tutorial.

use anyhow::{Context, Result, ensure};
use ripple_core::plugin::{Input, Localization, LocalizationContext, PluginParams, ReadingKind};
use ripple_core::{KinematicState, Vec3};
use serde::Deserialize;

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DeadReckoningConfig {
    /// How fast the estimator assumes the vehicle moves, as a fraction of the
    /// commanded velocity. 1.0 assumes it moves exactly as commanded.
    speed_gain: f64,
}

impl Default for DeadReckoningConfig {
    fn default() -> Self {
        Self { speed_gain: 1.0 }
    }
}

pub struct DeadReckoning {
    speed_gain: f64,
}

impl Localization for DeadReckoning {
    type Config = DeadReckoningConfig;

    fn configure(params: &PluginParams) -> Result<DeadReckoningConfig> {
        let config: DeadReckoningConfig = params.parse()?;
        ensure!(
            config.speed_gain >= 0.0,
            "DeadReckoning speed_gain must be nonnegative"
        );
        Ok(config)
    }

    fn new(config: &DeadReckoningConfig) -> Self {
        Self {
            speed_gain: config.speed_gain,
        }
    }

    fn inputs(_config: &DeadReckoningConfig) -> Vec<Input> {
        // A full-state reading; only its position is used.
        vec![Input::new("fix", ReadingKind::State)]
    }

    fn initialize(&mut self, context: &mut LocalizationContext) -> Result<()> {
        // Start from the first fix, at rest. Attitude is not estimated.
        let fix = context
            .inputs
            .latest::<KinematicState>("fix")?
            .context("DeadReckoning needs a position fix at startup")?;
        *context.state = KinematicState {
            position_world_m: fix.value.position_world_m,
            ..KinematicState::default()
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
            let velocity_world_mps = commanded_world_mps * self.speed_gain;
            context.state.velocity_world_mps = velocity_world_mps;
            context.state.position_world_m +=
                velocity_world_mps * context.commands.time(substep).dt_s;
        }

        // Correct: a new fix replaces the predicted position.
        if let Some(fix) = context.inputs.fresh::<KinematicState>("fix")? {
            context.state.position_world_m = fix.value.position_world_m;
        }
        Ok(())
    }
}

//! A unit-mass point pushed by a force, with MPE's damping. Rust-only.
//! Motion phase. Read with the MpeContact interaction, which applies MPE's
//! push between overlapping agents as forces added to the commanded one.
//!
//! Fidelity
//! - Source: the Multi-Agent Particle Environment as Farama's MPE2 v1.1.1
//!   (commit `147a8e8`) has it, `World.integrate_state` in
//!   `mpe2/_mpe_utils/core.py`. Each step, in this order: position moves by
//!   the old velocity, velocity is damped, then velocity gains force * dt.
//!   OpenAI's original MPE, and MAPPO's copy, update velocity first.
//! - Validated against: MPE2's `simple_spread_v3`, step by step
//!   (`reference/mpe_parity.py`).
//! - Valid range: MPE's 0.1 s step with one motion substep. The damping is a
//!   fraction removed per step, not per second, so another step size is a
//!   different model.
//! - Not modeled: mass other than 1 (every MPE2 scenario but
//!   `collect_treasure`), MPE's `max_speed` cap (`simple_spread` has none),
//!   and anything physical: it is a benchmark's point, not a vehicle.
//! - To go further: nothing; the vehicle models start where this stops.

use anyhow::{Result, ensure};
use serde::Deserialize;

use crate::Vec3;
use crate::plugin::{
    Frame, MotionContext, MotionModel, Plugin, PluginIo, PluginParams, Port, Ports, Unit, Update,
};

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MpeParticleConfig {
    /// Fraction of velocity removed each step (MPE's `World.damping`).
    damping: f64,
}

impl Default for MpeParticleConfig {
    fn default() -> Self {
        Self { damping: 0.25 }
    }
}

pub struct MpeParticle {
    damping: f64,
}

impl Plugin for MpeParticle {
    type Config = MpeParticleConfig;

    fn configure(params: &PluginParams<'_>) -> Result<Self::Config> {
        let config: MpeParticleConfig = params.parse()?;
        ensure!(
            (0.0..=1.0).contains(&config.damping),
            "MpeParticle damping must be between 0 and 1"
        );
        Ok(config)
    }

    fn new(config: &Self::Config) -> Self {
        Self {
            damping: config.damping,
        }
    }

    fn ports(_: &Self::Config) -> Ports {
        Ports::default()
            .input(Port::new("force_x", Unit::Newtons, Frame::World))
            .input(Port::new("force_y", Unit::Newtons, Frame::World))
    }
}

impl MotionModel for MpeParticle {
    fn step(&mut self, context: &mut MotionContext<'_>, io: &mut PluginIo) -> Result<Update> {
        let mut force_n = Vec3::new(io.read("force_x")?, io.read("force_y")?, 0.0);
        // MPE adds each push to the agent's force in turn.
        for push_n in context.applied_forces_world_n {
            force_n = push_n + force_n;
        }
        let dt_s = context.time.dt_s;
        let state = &mut context.truth;
        state.position_world_m += state.velocity_world_mps * dt_s;
        state.velocity_world_mps *= 1.0 - self.damping;
        // Unit mass, so force / mass is the force itself.
        state.velocity_world_mps += force_n * dt_s;
        ensure!(
            state.position_world_m.iter().all(|value| value.is_finite()),
            "MpeParticle produced nonfinite position"
        );
        Ok(Update::Applied)
    }
}

#[cfg(test)]
mod tests {
    use super::{MpeParticle, MpeParticleConfig};
    use crate::plugin::{
        EntityInfo, Messages, MotionContext, MotionModel, Plugin, PluginIo, StepTime,
    };
    use crate::{KinematicState, Vec3};

    #[test]
    fn position_moves_by_the_old_velocity_before_the_force_acts() -> anyhow::Result<()> {
        let config = MpeParticleConfig::default();
        let mut model = MpeParticle::new(&config);
        let mut io = PluginIo::new(&MpeParticle::ports(&config));
        io.receive(&[("force_x".into(), 5.0), ("force_y".into(), -5.0)].into())?;
        let mut truth = KinematicState {
            velocity_world_mps: Vec3::new(1.0, 0.0, 0.0),
            ..KinematicState::default()
        };
        model.step(
            &mut MotionContext {
                entity: EntityInfo {
                    id: 1,
                    team_id: 1,
                    group_index: 0,
                },
                time: StepTime {
                    time_s: 0.0,
                    dt_s: 0.1,
                    tick: 0,
                },
                truth: &mut truth,
                messages: &mut Messages::default(),
                terrain: None,
                applied_forces_world_n: &[],
            },
            &mut io,
        )?;
        assert_eq!(truth.position_world_m, Vec3::new(0.1, 0.0, 0.0));
        assert_eq!(truth.velocity_world_mps, Vec3::new(0.75 + 0.5, -0.5, 0.0));
        Ok(())
    }
}

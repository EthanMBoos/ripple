//! Single-agent task: a multirotor flies to a goal and holds there.
//! Autonomy phase: observe the goal offset and own velocity from belief, ask
//! the policy for an action, and write it as a velocity command
//! (velocity_x/y/z) for MultirotorVelocityController.
//! Book chapter: "Training a single agent".

use crate::policy::{Policy, TrainerActions};
use anyhow::{Result, ensure};
use ripple_core::KinematicState;
use ripple_core::plugin::{
    AgentContext, Autonomy, Frame, PluginIo, PluginParams, Port, Ports, Unit,
};
use serde::Deserialize;
use std::path::PathBuf;

const PORTS: [&str; 3] = ["velocity_x", "velocity_y", "velocity_z"];
pub const OBSERVATION_SIZE: usize = 6;
pub const ACTION_SIZE: usize = 3;

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HoverConfig {
    /// Where to hold, `[east, north, up]`.
    pub goal_m: [f64; 3],
    /// The command for an action of 1: each action number in [-1, 1] scales it.
    pub max_speed_mps: f64,
    /// The trained network; relative paths start where `ripple` runs. Unused
    /// while Python trains.
    pub policy: PathBuf,
}

impl Default for HoverConfig {
    fn default() -> Self {
        Self {
            goal_m: [0.0, 0.0, 5.0],
            max_speed_mps: 2.0,
            policy: PathBuf::from("crates/learning/policies/hover.onnx"),
        }
    }
}

impl HoverConfig {
    /// What the policy sees: the offset to the goal (m), then own velocity
    /// (m/s), both from belief. Python trains on this same function.
    pub fn observe(&self, state: &KinematicState) -> Vec<f32> {
        let offset_m = ripple_core::Vec3::from(self.goal_m) - state.position_world_m;
        let velocity_mps = state.velocity_world_mps;
        [offset_m.x, offset_m.y, offset_m.z]
            .into_iter()
            .chain([velocity_mps.x, velocity_mps.y, velocity_mps.z])
            .map(|value| value as f32)
            .collect()
    }
}

pub struct HoverPolicy {
    config: HoverConfig,
    policy: Policy,
}

impl HoverPolicy {
    /// While training: actions come from Python instead of the policy file.
    pub fn trained_by(config: &HoverConfig, actions: TrainerActions) -> Self {
        Self {
            config: config.clone(),
            policy: Policy::Trainer(actions),
        }
    }
}

impl Autonomy for HoverPolicy {
    type Config = HoverConfig;

    fn configure(params: &PluginParams) -> Result<HoverConfig> {
        let config: HoverConfig = params.parse()?;
        ensure!(
            config.max_speed_mps > 0.0,
            "HoverPolicy max_speed_mps must be positive"
        );
        Ok(config)
    }

    fn new(config: &HoverConfig) -> Self {
        Self {
            config: config.clone(),
            policy: Policy::File(config.policy.clone()),
        }
    }

    fn ports(_: &HoverConfig) -> Ports {
        let mut ports = Ports::default();
        for name in PORTS {
            ports = ports.output(Port::new(name, Unit::MetersPerSecond, Frame::World));
        }
        ports
    }

    fn initialize(&mut self, _: &mut AgentContext) -> Result<()> {
        self.policy.load(OBSERVATION_SIZE)
    }

    fn step(&mut self, context: &mut AgentContext, io: &mut PluginIo) -> Result<()> {
        let observation = self.config.observe(context.state);
        let action = self.policy.act(context.entity.id, &observation)?;
        ensure!(
            action.len() == ACTION_SIZE,
            "HoverPolicy needs {ACTION_SIZE} action numbers, got {}",
            action.len()
        );
        for (name, value) in PORTS.into_iter().zip(action) {
            io.write(name, value as f64 * self.config.max_speed_mps)?;
        }
        Ok(())
    }
}

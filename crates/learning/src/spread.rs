//! Multi-agent task: a team spreads out to cover one landmark each, MPE's
//! simple_spread on Ripple's point model. Every agent runs the same network.
//! Autonomy phase: observe own velocity, the landmarks, and teammates from
//! belief, ask the policy for an action, and write it as a horizontal
//! velocity command (velocity_x/y/z) for SingleIntegrator.
//! Book chapter: "Training a team".

use crate::policy::{Policy, TrainerActions};
use anyhow::{Result, ensure};
use ripple_core::KinematicState;
use ripple_core::plugin::{
    AgentContext, Autonomy, Contact, Frame, PluginIo, PluginParams, Port, Ports, Unit,
};
use serde::Deserialize;
use std::path::PathBuf;

const PORTS: [&str; 3] = ["velocity_x", "velocity_y", "velocity_z"];
pub const ACTION_SIZE: usize = 2;

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SpreadConfig {
    /// Landmarks, `[east, north]`; the team has one agent per landmark.
    // TODO (nice to have): draw the landmarks in the viewer, which now draws
    // only Boundary boxes.
    pub landmarks_m: Vec<[f64; 2]>,
    /// The command for an action of 1: each action number in [-1, 1] scales it.
    pub max_speed_mps: f64,
    /// The trained network; relative paths start where `ripple` runs. Unused
    /// while Python trains.
    pub policy: PathBuf,
}

impl Default for SpreadConfig {
    fn default() -> Self {
        Self {
            landmarks_m: vec![[-4.0, -3.0], [4.0, -3.0], [0.0, 4.0]],
            max_speed_mps: 2.0,
            policy: PathBuf::from("crates/learning/policies/spread.onnx"),
        }
    }
}

impl SpreadConfig {
    /// Own velocity, each landmark's offset, and each teammate's offset in
    /// ID order (m/s and m, east and north only), all from belief. Teammates
    /// are the known contacts on `team_id`; there must be exactly one per
    /// landmark besides this agent, so a teammate the tracker has lost is an
    /// error. Python trains on this same function.
    pub fn observe<'a>(
        &self,
        state: &KinematicState,
        team_id: i32,
        contacts: impl IntoIterator<Item = &'a Contact>,
    ) -> Result<Vec<f32>> {
        let position_m = state.position_world_m;
        let mut observation = vec![state.velocity_world_mps.x, state.velocity_world_mps.y];
        for [east_m, north_m] in &self.landmarks_m {
            observation.extend([east_m - position_m.x, north_m - position_m.y]);
        }
        let mut teammates: Vec<_> = contacts
            .into_iter()
            .filter(|contact| contact.team_id == team_id)
            .collect();
        teammates.sort_by_key(|contact| contact.id);
        ensure!(
            teammates.len() + 1 == self.landmarks_m.len(),
            "SpreadPolicy needs one agent per landmark: {} landmarks, {} agents",
            self.landmarks_m.len(),
            teammates.len() + 1
        );
        for contact in teammates {
            let offset_m = contact.state.position_world_m - position_m;
            observation.extend([offset_m.x, offset_m.y]);
        }
        Ok(observation.into_iter().map(|value| value as f32).collect())
    }

    pub fn observation_size(&self) -> usize {
        2 + 4 * self.landmarks_m.len() - 2
    }
}

pub struct SpreadPolicy {
    config: SpreadConfig,
    policy: Policy,
}

impl SpreadPolicy {
    /// While training: actions come from Python instead of the policy file.
    pub fn trained_by(config: &SpreadConfig, actions: TrainerActions) -> Self {
        Self {
            config: config.clone(),
            policy: Policy::Trainer(actions),
        }
    }
}

impl Autonomy for SpreadPolicy {
    type Config = SpreadConfig;

    fn configure(params: &PluginParams) -> Result<SpreadConfig> {
        let config: SpreadConfig = params.parse()?;
        ensure!(
            !config.landmarks_m.is_empty(),
            "SpreadPolicy needs at least one landmark"
        );
        ensure!(
            config.max_speed_mps > 0.0,
            "SpreadPolicy max_speed_mps must be positive"
        );
        Ok(config)
    }

    fn new(config: &SpreadConfig) -> Self {
        Self {
            config: config.clone(),
            policy: Policy::File(config.policy.clone()),
        }
    }

    fn ports(_: &SpreadConfig) -> Ports {
        let mut ports = Ports::default();
        for name in PORTS {
            ports = ports.output(Port::new(name, Unit::MetersPerSecond, Frame::World));
        }
        ports
    }

    fn initialize(&mut self, _: &mut AgentContext) -> Result<()> {
        self.policy.load(self.config.observation_size())
    }

    fn step(&mut self, context: &mut AgentContext, io: &mut PluginIo) -> Result<()> {
        let observation = self.config.observe(
            context.state,
            context.entity.team_id,
            context.contacts.iter(),
        )?;
        let action = self.policy.act(context.entity.id, &observation)?;
        ensure!(
            action.len() == ACTION_SIZE,
            "SpreadPolicy needs {ACTION_SIZE} action numbers, got {}",
            action.len()
        );
        let speed_mps = self.config.max_speed_mps;
        io.write("velocity_x", action[0] as f64 * speed_mps)?;
        io.write("velocity_y", action[1] as f64 * speed_mps)?;
        io.write("velocity_z", 0.0)?;
        Ok(())
    }
}

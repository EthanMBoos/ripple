//! Drive until belief says the goal has been reached.
use anyhow::Result;
use ripple_core::plugin::*;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DriveToGoalConfig {
    goal_world_m: f64,
}
impl Default for DriveToGoalConfig {
    fn default() -> Self {
        Self { goal_world_m: 1.0 }
    }
}

pub struct DriveToGoal {
    goal_world_m: f64,
}
impl Autonomy for DriveToGoal {
    type Config = DriveToGoalConfig;
    fn new(config: &DriveToGoalConfig) -> Self {
        Self {
            goal_world_m: config.goal_world_m,
        }
    }
    fn ports(_: &DriveToGoalConfig) -> Ports {
        Ports::default().output(Port::new("speed", Unit::MetersPerSecond, Frame::World))
    }

    fn step(&mut self, context: &mut AgentContext, io: &mut PluginIo) -> Result<()> {
        let reached_goal = context.state.position_world_m.x >= self.goal_world_m;
        io.write("speed", if reached_goal { 0.0 } else { 2.0 })?;
        Ok(())
    }
}

//! Drive until the position sensor reports that the goal has been reached.
use super::sensor::PositionReading;
use anyhow::Result;
use scrimmage_core::plugin::*;
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
impl Plugin for DriveToGoal {
    type Config = DriveToGoalConfig;
    fn configure(params: &PluginParams<'_>) -> Result<DriveToGoalConfig> {
        params.parse()
    }
    fn new(config: &DriveToGoalConfig) -> Self {
        Self {
            goal_world_m: config.goal_world_m,
        }
    }
    fn ports(_: &DriveToGoalConfig) -> Ports {
        Ports::default().output(Port::new("speed", Unit::MetersPerSecond, Frame::World))
    }
}
impl Autonomy for DriveToGoal {
    fn step(&mut self, context: &mut AgentContext<'_>, io: &mut PluginIo) -> Result<Update> {
        let position = context.observations.get::<PositionReading>("position")?;
        let reached_goal =
            position.is_some_and(|reading| reading.value.x_world_m >= self.goal_world_m);
        io.write("speed", if reached_goal { 0.0 } else { 2.0 })?;
        Ok(Update::Applied)
    }
}

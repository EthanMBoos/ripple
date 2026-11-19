//! An autonomy that asks the engine to build other entities.
use anyhow::Result;
use ripple_core::{Vec3, plugin::*};
use serde::{Deserialize, Serialize};

/// Asks the engine, while it initializes, to build one `DriveToGoal` entity
/// of `recipe` at each start, with the goal laid over the recipe's.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LauncherConfig {
    recipe: String,
    starts_m: Vec<[f64; 3]>,
    goal_world_m: f64,
    /// The recipe's autonomy plugin the goal is for.
    goal_plugin: String,
}
impl Default for LauncherConfig {
    fn default() -> Self {
        Self {
            recipe: "scouts".into(),
            starts_m: Vec::new(),
            goal_world_m: 1.0,
            goal_plugin: "DriveToGoal".into(),
        }
    }
}

#[derive(Serialize)]
struct Goal {
    goal_world_m: f64,
}

pub struct Launcher {
    recipe: String,
    starts_m: Vec<[f64; 3]>,
    goal_world_m: f64,
    goal_plugin: String,
}
impl Autonomy for Launcher {
    type Config = LauncherConfig;
    fn new(config: &LauncherConfig) -> Self {
        Self {
            recipe: config.recipe.clone(),
            starts_m: config.starts_m.clone(),
            goal_world_m: config.goal_world_m,
            goal_plugin: config.goal_plugin.clone(),
        }
    }
    fn ports(_: &LauncherConfig) -> Ports {
        Ports::default().output(Port::new("speed", Unit::MetersPerSecond, Frame::World))
    }

    fn initialize(&mut self, context: &mut AgentContext) -> Result<()> {
        for start_m in &self.starts_m {
            let request = SpawnRequest::recipe(&self.recipe)
                .at(Vec3::new(start_m[0], start_m[1], start_m[2]))
                .autonomy(
                    &self.goal_plugin,
                    Goal {
                        goal_world_m: self.goal_world_m,
                    },
                )?;
            context
                .messages
                .publish("GlobalNetwork", SpawnRequest::TOPIC, request)?;
        }
        Ok(())
    }

    fn step(&mut self, _: &mut AgentContext, io: &mut PluginIo) -> Result<()> {
        io.write("speed", 0.0)
    }
}

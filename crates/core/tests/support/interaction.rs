//! A world interaction: keep every entity above a floor and publish the resulting population.
use anyhow::Result;
use scrimmage_core::plugin::{Interaction, InteractionContext, Plugin, PluginParams, Update};
use serde::{Deserialize, Serialize};

pub struct Population {
    pub entity_count: usize,
}
#[derive(Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct FloorConfig {
    height_world_m: f64,
}
pub struct Floor {
    height_world_m: f64,
}
impl Plugin for Floor {
    type Config = FloorConfig;
    fn configure(params: &PluginParams<'_>) -> Result<FloorConfig> {
        params.parse()
    }
    fn new(config: &FloorConfig) -> Self {
        Self {
            height_world_m: config.height_world_m,
        }
    }
}
impl Interaction for Floor {
    fn step(&mut self, context: &mut InteractionContext<'_>) -> Result<Update> {
        for entity in context.entities.iter_mut() {
            let state = entity.truth_mut();
            state.position_world_m.z = state.position_world_m.z.max(self.height_world_m);
        }
        context.messages.publish(
            "ExampleNetwork",
            "population",
            Population {
                entity_count: context.entities.len(),
            },
        )?;
        Ok(Update::Applied)
    }
}

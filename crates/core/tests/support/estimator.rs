//! Localization from east-position fixes alone. The initial position is a known
//! prior, not a reading, so belief differs from truth when the sensor is biased.
use super::sensor::PositionReading;
use anyhow::Result;
use ripple_core::plugin::*;
use serde::Deserialize;

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EastFixConfig {
    initial_x_world_m: f64,
}
pub struct EastFix {
    initial_x_world_m: f64,
}
impl Plugin for EastFix {
    type Config = EastFixConfig;
    fn configure(params: &PluginParams<'_>) -> Result<EastFixConfig> {
        params.parse()
    }
    fn new(config: &EastFixConfig) -> Self {
        Self {
            initial_x_world_m: config.initial_x_world_m,
        }
    }
}
impl Localization for EastFix {
    fn inputs(_: &EastFixConfig) -> Vec<Input> {
        vec![Input::new("fix", ReadingKind::Other)]
    }
    fn initialize(&mut self, context: &mut LocalizationContext<'_>) -> Result<()> {
        context.state.position_world_m.x = self.initial_x_world_m;
        Ok(())
    }
    fn step(&mut self, context: &mut LocalizationContext<'_>) -> Result<Update> {
        if let Some(fix) = context.inputs.fresh::<PositionReading>("fix")? {
            context.state.position_world_m.x = fix.value.x_world_m;
        }
        Ok(Update::Applied)
    }
}

//! A communication model: configurable fixed delay or complete packet loss.
use anyhow::{Result, ensure};
use scrimmage_core::plugin::{Delivery, Network, NetworkContext, Plugin, PluginParams, Update};
use serde::Deserialize;

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RadioConfig {
    delay_s: f64,
    drop_messages: bool,
}
pub struct ExampleNetwork {
    config: RadioConfig,
}
impl Plugin for ExampleNetwork {
    type Config = RadioConfig;
    fn configure(params: &PluginParams<'_>) -> Result<Self::Config> {
        let config: RadioConfig = params.parse()?;
        ensure!(config.delay_s >= 0.0, "delay_s must be nonnegative");
        Ok(config)
    }
    fn new(config: &Self::Config) -> Self {
        Self { config: *config }
    }
}
impl Network for ExampleNetwork {
    fn step(&mut self, context: &mut NetworkContext<'_, '_>) -> Result<Update> {
        context.route(|_link, _random| {
            if self.config.drop_messages {
                Ok(Delivery::Drop)
            } else {
                Ok(Delivery::After {
                    delay_s: self.config.delay_s,
                })
            }
        })
    }
}

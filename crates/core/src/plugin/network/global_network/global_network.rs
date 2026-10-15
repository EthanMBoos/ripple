//! All endpoints are reachable, after a fixed delay.
//!
//! Derived from SCRIMMAGE: src/plugins/network/GlobalNetwork/GlobalNetwork.cpp.
//! Network phase: route every matching publisher/subscriber link. A message
//! arrives `delay_s` after it was sent, at the first tick at or after that
//! time; 0 (the default) delivers it in this network phase.

use anyhow::{Result, ensure};
use serde::Deserialize;

use crate::plugin::{Delivery, Network, NetworkContext, Plugin, PluginParams, Update};

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GlobalNetworkConfig {
    delay_s: f64,
}

pub struct GlobalNetwork {
    delay_s: f64,
}

impl Plugin for GlobalNetwork {
    type Config = GlobalNetworkConfig;

    fn configure(params: &PluginParams<'_>) -> Result<GlobalNetworkConfig> {
        let config: GlobalNetworkConfig = params.parse()?;
        ensure!(
            config.delay_s.is_finite() && config.delay_s >= 0.0,
            "GlobalNetwork delay_s must be finite and nonnegative"
        );
        Ok(config)
    }

    fn new(config: &GlobalNetworkConfig) -> Self {
        Self {
            delay_s: config.delay_s,
        }
    }
}

impl Network for GlobalNetwork {
    fn step(&mut self, context: &mut NetworkContext<'_, '_>) -> Result<Update> {
        let delay_s = self.delay_s;
        context.route(|_link, _random| Ok(Delivery::After { delay_s }))
    }
}

#[cfg(test)]
mod tests {
    use super::{GlobalNetwork, Plugin, PluginParams};

    #[test]
    fn negative_delays_and_old_delay_keys_are_errors() {
        for params in ["delay_s: -1", "comm_delay: 1", "is_stochastic_delay: true"] {
            assert!(
                GlobalNetwork::configure(&PluginParams::yaml(params)).is_err(),
                "{params}"
            );
        }
    }
}

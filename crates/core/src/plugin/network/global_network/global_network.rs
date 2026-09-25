//! All endpoints are reachable; the engine handles queued delivery.
//!
//! C++ counterpart: src/plugins/network/GlobalNetwork/GlobalNetwork.cpp.
//! Network phase: route every matching publisher/subscriber link immediately.

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use crate::plugin::{Delivery, Network, NetworkContext, Plugin, PluginParams, Update};

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct GlobalNetworkConfig {
    /// Stochastic delay is not implemented; must stay false.
    is_stochastic_delay: bool,
    /// C++ uses a negative value to mean no delay; queued delay is not ported.
    #[serde(rename = "comm_delay")]
    comm_delay_s: f64,
}

impl Default for GlobalNetworkConfig {
    fn default() -> Self {
        Self {
            is_stochastic_delay: false,
            comm_delay_s: -1.0,
        }
    }
}

pub struct GlobalNetwork;

impl Plugin for GlobalNetwork {
    type Config = GlobalNetworkConfig;

    fn configure(params: &PluginParams<'_>) -> Result<GlobalNetworkConfig> {
        let config: GlobalNetworkConfig = params.parse()?;
        ensure!(
            !config.is_stochastic_delay,
            "stochastic delay is not implemented by GlobalNetwork"
        );
        ensure!(
            config.comm_delay_s < 0.0,
            "legacy queued comm_delay is not yet ported; use a custom Network for explicit delay semantics"
        );
        Ok(config)
    }

    fn new(_config: &GlobalNetworkConfig) -> Self {
        Self
    }
}

impl Network for GlobalNetwork {
    fn step(&mut self, context: &mut NetworkContext<'_, '_>) -> Result<Update> {
        context.route(|_link, _random| Ok(Delivery::After { delay_s: 0.0 }))
    }
}

#[cfg(test)]
mod tests {
    use super::{GlobalNetwork, Plugin, PluginParams};
    use crate::Params;

    #[test]
    fn negative_legacy_delay_is_accepted() -> anyhow::Result<()> {
        let params = Params::from([("comm_delay".into(), "-2".into())]);
        GlobalNetwork::configure(&PluginParams::new(&params))?;
        Ok(())
    }

    #[test]
    fn unsupported_delay_modes_remain_errors() {
        for (key, value) in [
            ("comm_delay", "0"),
            ("comm_delay", "1"),
            ("is_stochastic_delay", "true"),
        ] {
            let params = Params::from([(key.into(), value.into())]);
            assert!(GlobalNetwork::configure(&PluginParams::new(&params)).is_err());
        }
    }
}

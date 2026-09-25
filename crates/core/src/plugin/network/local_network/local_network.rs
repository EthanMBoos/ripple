//! Same-entity communication, not distance-limited communication.
//!
//! C++ counterpart: src/plugins/network/LocalNetwork/LocalNetwork.cpp.
//! Network phase: inspect endpoint owners, route same-entity messages immediately.

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use crate::plugin::{Delivery, Network, NetworkContext, Plugin, PluginParams, Update};

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct LocalNetworkConfig {
    /// Stochastic delay is not implemented; must stay false.
    is_stochastic_delay: bool,
    /// C++ uses a negative value to mean no delay; queued delay is not ported.
    #[serde(rename = "comm_delay")]
    comm_delay_s: f64,
}

impl Default for LocalNetworkConfig {
    fn default() -> Self {
        Self {
            is_stochastic_delay: false,
            comm_delay_s: -1.0,
        }
    }
}

pub struct LocalNetwork;

impl Plugin for LocalNetwork {
    type Config = LocalNetworkConfig;

    fn configure(params: &PluginParams<'_>) -> Result<LocalNetworkConfig> {
        let config: LocalNetworkConfig = params.parse()?;
        ensure!(
            !config.is_stochastic_delay,
            "stochastic delay is not implemented by LocalNetwork"
        );
        ensure!(
            config.comm_delay_s < 0.0,
            "legacy queued comm_delay is not yet ported; use a custom Network for explicit delay semantics"
        );
        Ok(config)
    }

    fn new(_config: &LocalNetworkConfig) -> Self {
        Self
    }
}

impl Network for LocalNetwork {
    fn step(&mut self, context: &mut NetworkContext<'_, '_>) -> Result<Update> {
        context.route(|link, _random| {
            let Some(sender_entity_id) = link.sender.entity_id else {
                return Ok(Delivery::Drop);
            };
            if link.receiver.entity_id != Some(sender_entity_id) {
                return Ok(Delivery::Drop);
            }
            Ok(Delivery::After { delay_s: 0.0 })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{LocalNetwork, Plugin, PluginParams};
    use crate::Params;

    #[test]
    fn negative_legacy_delay_is_accepted() -> anyhow::Result<()> {
        let params = Params::from([("comm_delay".into(), "-2".into())]);
        LocalNetwork::configure(&PluginParams::new(&params))?;
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
            assert!(LocalNetwork::configure(&PluginParams::new(&params)).is_err());
        }
    }
}

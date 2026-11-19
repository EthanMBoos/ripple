//! Same-entity communication, not distance-limited communication.
//!
//! Derived from SCRIMMAGE: src/plugins/network/LocalNetwork/LocalNetwork.cpp.
//! Network phase: inspect endpoint owners, route same-entity messages immediately.

use anyhow::Result;

use crate::plugin::{Delivery, Network, NetworkContext};

/// LocalNetwork takes no parameters: it delivers within the entity at once.
pub struct LocalNetwork;

impl Network for LocalNetwork {
    type Config = ();

    fn new(_config: &()) -> Self {
        Self
    }

    fn step(&mut self, context: &mut NetworkContext) -> Result<()> {
        // Only links within the sender's entity; world plugins have none.
        context.route_within_entity(|_link, _random| Ok(Delivery::After { delay_s: 0.0 }))
    }
}

#[cfg(test)]
mod tests {
    use super::{LocalNetwork, Network};
    use crate::plugin::PluginParams;

    #[test]
    fn delay_keys_are_errors() {
        for params in ["comm_delay: 1", "is_stochastic_delay: true"] {
            assert!(LocalNetwork::configure(&PluginParams::yaml(params)).is_err());
        }
    }
}

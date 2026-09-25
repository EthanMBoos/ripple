//! Distance-limited communication with independent loss per reachable delivery.
//!
//! C++ counterpart: src/plugins/network/SphereNetwork/SphereNetwork.cpp (LGPL-3.0-or-later).
//! Network phase: use current post-motion truth, then apply the optional altitude plane and loss.
//! Loss draws come from this network's mission-seeded stream, not C++'s shared generator.

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use crate::plugin::{
    Delivery, Network, NetworkContext, Plugin, PluginParams, Transmission, Update,
};

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct SphereNetworkConfig {
    #[serde(rename = "range")]
    range_m: f64,
    #[serde(rename = "prob_transmit")]
    probability_transmit: f64,
    filter_comms_plane: bool,
    #[serde(rename = "comms_boundary_altitude")]
    boundary_altitude_m: f64,
    #[serde(rename = "comms_boundary_epsilon")]
    boundary_epsilon_m: f64,
    /// Legacy delay modes are not implemented; must stay false and negative.
    is_stochastic_delay: bool,
    #[serde(rename = "comm_delay")]
    comm_delay_s: f64,
}

impl Default for SphereNetworkConfig {
    fn default() -> Self {
        Self {
            range_m: 100.0,
            probability_transmit: 1.0,
            filter_comms_plane: false,
            boundary_altitude_m: 0.0,
            boundary_epsilon_m: 0.0,
            is_stochastic_delay: false,
            comm_delay_s: -1.0,
        }
    }
}

pub struct SphereNetwork {
    range_m: f64,
    probability_transmit: f64,
    filter_comms_plane: bool,
    boundary_altitude_m: f64,
    boundary_epsilon_m: f64,
}

impl Plugin for SphereNetwork {
    type Config = SphereNetworkConfig;

    fn configure(params: &PluginParams<'_>) -> Result<SphereNetworkConfig> {
        let config: SphereNetworkConfig = params.parse()?;
        ensure!(
            config.range_m >= 0.0,
            "SphereNetwork range must be nonnegative"
        );
        ensure!(
            (0.0..=1.0).contains(&config.probability_transmit),
            "prob_transmit must be in [0, 1]"
        );
        ensure!(
            config.boundary_epsilon_m >= 0.0,
            "comms_boundary_epsilon must be nonnegative"
        );
        ensure!(
            !config.is_stochastic_delay && config.comm_delay_s < 0.0,
            "SphereNetwork supports immediate delivery only; legacy delay modes are not implemented"
        );
        Ok(config)
    }

    fn new(config: &SphereNetworkConfig) -> Self {
        Self {
            range_m: config.range_m,
            probability_transmit: config.probability_transmit,
            filter_comms_plane: config.filter_comms_plane,
            boundary_altitude_m: config.boundary_altitude_m,
            boundary_epsilon_m: config.boundary_epsilon_m,
        }
    }
}

impl Network for SphereNetwork {
    fn step(&mut self, context: &mut NetworkContext<'_, '_>) -> Result<Update> {
        context.route(|link, random| {
            // Like C++, draw only for reachable links, one draw per publisher/subscriber pair.
            if self.reachable(link) && random.uniform() <= self.probability_transmit {
                Ok(Delivery::After { delay_s: 0.0 })
            } else {
                Ok(Delivery::Drop)
            }
        })
    }
}

impl SphereNetwork {
    fn reachable(&self, link: &Transmission<'_>) -> bool {
        let (Some(sender_id), Some(receiver_id)) = (link.sender.entity_id, link.receiver.entity_id)
        else {
            // World plugins have no antenna position. Use GlobalNetwork for their traffic.
            return false;
        };
        if sender_id == receiver_id {
            return true;
        }
        let sender = link
            .contacts_truth
            .iter()
            .find(|entity| entity.id == sender_id);
        let receiver = link
            .contacts_truth
            .iter()
            .find(|entity| entity.id == receiver_id);
        let (Some(sender), Some(receiver)) = (sender, receiver) else {
            return false;
        };
        // C++ differs slightly here, and we don't copy it: it builds its RTree when
        // entities are generated (before motion), then queries it with the publisher's
        // post-motion position, and caches the answer for both directions of the pair.
        // So C++ compares a new position with an old one, and whichever endpoint
        // publishes first decides the pair. Rust uses post-motion positions for both
        // endpoints and decides each link independently. Results differ only for
        // moving vehicles near the range edge; exact parity would need C++'s
        // publisher iteration order.
        let sender_position_m = sender.truth.position_world_m;
        let receiver_position_m = receiver.truth.position_world_m;
        // C++ RTree::neighbors_in_range uses a strict distance bound.
        if (sender_position_m - receiver_position_m).norm() >= self.range_m {
            return false;
        }
        if !self.filter_comms_plane {
            return true;
        }
        let both_above = sender_position_m.z >= self.boundary_altitude_m - self.boundary_epsilon_m
            && receiver_position_m.z >= self.boundary_altitude_m - self.boundary_epsilon_m;
        let both_below = sender_position_m.z <= self.boundary_altitude_m + self.boundary_epsilon_m
            && receiver_position_m.z <= self.boundary_altitude_m + self.boundary_epsilon_m;
        both_above || both_below
    }
}

#[cfg(test)]
mod tests {
    use super::SphereNetwork;
    use crate::plugin::{MessageEndpoint, Plugin, PluginParams, StepTime, Transmission};
    use crate::{EntityKind, EntitySnapshot, KinematicState, Params, Vec3};

    fn reachable(
        network: &SphereNetwork,
        sender: Option<i32>,
        receiver: Option<i32>,
        positions: [Vec3; 2],
    ) -> bool {
        let contacts = positions
            .into_iter()
            .enumerate()
            .map(|(index, position_world_m)| EntitySnapshot {
                id: index as i32 + 1,
                team_id: 1,
                sub_swarm_id: 0,
                active: true,
                kind: EntityKind::Sphere,
                truth: KinematicState {
                    position_world_m,
                    ..KinematicState::default()
                },
            })
            .collect::<Vec<_>>();
        network.reachable(&Transmission {
            time: StepTime {
                time_s: 0.0,
                dt_s: 0.1,
            },
            sender: &MessageEndpoint {
                entity_id: sender,
                plugin: "publisher".into(),
            },
            receiver: &MessageEndpoint {
                entity_id: receiver,
                plugin: "subscriber".into(),
            },
            topic: "test",
            contacts_truth: &contacts,
        })
    }

    #[test]
    fn range_is_three_dimensional_and_uses_each_current_snapshot() -> anyhow::Result<()> {
        let params = Params::from([("range".into(), "5".into())]);
        let network = SphereNetwork::new(&SphereNetwork::configure(&PluginParams::new(&params))?);
        for (position, expected) in [
            (Vec3::new(3.0, 0.0, 3.999), true),
            (Vec3::new(3.0, 0.0, 4.0), false),
            (Vec3::new(3.0, 0.0, 4.001), false),
            (Vec3::new(0.0, 0.0, 5.001), false),
            (Vec3::zeros(), true),
        ] {
            assert_eq!(
                reachable(&network, Some(1), Some(2), [Vec3::zeros(), position]),
                expected
            );
        }
        assert!(!reachable(&network, None, Some(2), [Vec3::zeros(); 2]));
        assert!(!reachable(&network, Some(1), None, [Vec3::zeros(); 2]));
        assert!(!reachable(&network, Some(1), Some(3), [Vec3::zeros(); 2]));
        Ok(())
    }

    #[test]
    fn altitude_plane_includes_epsilon_boundaries_and_same_parent_bypasses_geometry()
    -> anyhow::Result<()> {
        let params = Params::from([
            ("filter_comms_plane".into(), "true".into()),
            ("comms_boundary_altitude".into(), "10".into()),
            ("comms_boundary_epsilon".into(), "1".into()),
        ]);
        let network = SphereNetwork::new(&SphereNetwork::configure(&PluginParams::new(&params))?);
        for (z1, z2, expected) in [
            (8.0, 12.0, false),
            (9.0, 12.0, true),
            (8.0, 11.0, true),
            (8.0, 8.0, true),
        ] {
            assert_eq!(
                reachable(
                    &network,
                    Some(1),
                    Some(2),
                    [Vec3::new(0.0, 0.0, z1), Vec3::new(0.0, 0.0, z2)]
                ),
                expected
            );
        }
        assert!(reachable(
            &network,
            Some(1),
            Some(1),
            [Vec3::zeros(), Vec3::repeat(1000.0)]
        ));
        Ok(())
    }

    #[test]
    fn invalid_range_probability_and_unsupported_delays_are_rejected() {
        for (key, value) in [
            ("range", "-1"),
            ("range", "NaN"),
            ("prob_transmit", "1.1"),
            ("prob_transmit", "-0.1"),
            ("comms_boundary_epsilon", "-1"),
            ("comm_delay", "0"),
            ("is_stochastic_delay", "true"),
        ] {
            let params = Params::from([(key.into(), value.into())]);
            assert!(
                SphereNetwork::configure(&PluginParams::new(&params)).is_err(),
                "{key}={value}"
            );
        }
    }
}

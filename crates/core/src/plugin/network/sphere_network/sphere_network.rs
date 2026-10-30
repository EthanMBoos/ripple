//! Distance-limited communication with independent loss per reachable delivery.
//!
//! Derived from SCRIMMAGE: src/plugins/network/SphereNetwork/SphereNetwork.cpp (LGPL-3.0-or-later).
//! Network phase: use current post-motion truth, then apply the optional altitude plane, loss,
//! and delay (`delay_s`, default 0: delivered in this network phase).
//! A message reaches an entity or not as one draw, keyed by the sending and
//! receiving entities, the topic, and the message's sequence: the receiving
//! entity's radio either hears it or doesn't, for all its subscribers.

use anyhow::{Result, ensure};
use serde::Deserialize;

use crate::plugin::{Delivery, Network, NetworkContext, PluginParams, PluginRandom, Transmission};

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SphereNetworkConfig {
    range_m: f64,
    transmit_probability: f64,
    enable_altitude_split: bool,
    split_altitude_m: f64,
    split_margin_m: f64,
    /// A message arrives this long after it was sent, at the first tick at or
    /// after that time; 0 delivers it in this network phase.
    delay_s: f64,
}

impl Default for SphereNetworkConfig {
    fn default() -> Self {
        Self {
            range_m: 100.0,
            transmit_probability: 1.0,
            enable_altitude_split: false,
            split_altitude_m: 0.0,
            split_margin_m: 0.0,
            delay_s: 0.0,
        }
    }
}

pub struct SphereNetwork {
    range_m: f64,
    transmit_probability: f64,
    enable_altitude_split: bool,
    split_altitude_m: f64,
    split_margin_m: f64,
    delay_s: f64,
}

impl Network for SphereNetwork {
    type Config = SphereNetworkConfig;

    fn configure(params: &PluginParams) -> Result<SphereNetworkConfig> {
        let config: SphereNetworkConfig = params.parse()?;
        ensure!(
            config.range_m >= 0.0,
            "SphereNetwork range_m must be nonnegative"
        );
        ensure!(
            (0.0..=1.0).contains(&config.transmit_probability),
            "transmit_probability must be in [0, 1]"
        );
        ensure!(
            config.split_margin_m >= 0.0,
            "split_margin_m must be nonnegative"
        );
        ensure!(
            config.delay_s.is_finite() && config.delay_s >= 0.0,
            "SphereNetwork delay_s must be finite and nonnegative"
        );
        Ok(config)
    }

    fn new(config: &SphereNetworkConfig) -> Self {
        Self {
            range_m: config.range_m,
            transmit_probability: config.transmit_probability,
            enable_altitude_split: config.enable_altitude_split,
            split_altitude_m: config.split_altitude_m,
            split_margin_m: config.split_margin_m,
            delay_s: config.delay_s,
        }
    }

    fn step(&mut self, context: &mut NetworkContext) -> Result<()> {
        context.route(|link, random| {
            let entity = |id: Option<i32>| i64::from(id.unwrap_or(0)) as u64;
            let key = [
                entity(link.sender.entity_id),
                entity(link.receiver.entity_id),
                PluginRandom::name(link.topic),
                link.sequence,
            ];
            if self.reachable(link) && random.uniform(&key) <= self.transmit_probability {
                Ok(Delivery::After {
                    delay_s: self.delay_s,
                })
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
        // If SphereNetwork becomes slow with many agents: these two searches scan
        // every contact for every link. The engine's contacts are sorted by id, so
        // a binary search (or an id-to-contact map built once per step) fixes that.
        // This is a lookup by id, not a range query, so `within` doesn't help.
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
        // Both endpoints use post-motion positions, and each link is decided on its
        // own.
        let sender_position_m = sender.truth.position_world_m;
        let receiver_position_m = receiver.truth.position_world_m;
        // Strict distance bound.
        if (sender_position_m - receiver_position_m).norm() >= self.range_m {
            return false;
        }
        if !self.enable_altitude_split {
            return true;
        }
        let both_above = sender_position_m.z >= self.split_altitude_m - self.split_margin_m
            && receiver_position_m.z >= self.split_altitude_m - self.split_margin_m;
        let both_below = sender_position_m.z <= self.split_altitude_m + self.split_margin_m
            && receiver_position_m.z <= self.split_altitude_m + self.split_margin_m;
        both_above || both_below
    }
}

#[cfg(test)]
mod tests {
    use super::{Network, SphereNetwork};
    use crate::plugin::{MessageEndpoint, PluginParams, StepTime, Transmission, TruthContacts};
    use crate::{EntitySnapshot, KinematicState, Vec3};

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
                group_index: 0,
                active: true,
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
                tick: 0,
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
            sequence: 0,
            contacts_truth: TruthContacts::from(&contacts[..]),
        })
    }

    #[test]
    fn range_is_three_dimensional_and_uses_each_current_snapshot() -> anyhow::Result<()> {
        let network = SphereNetwork::new(&SphereNetwork::configure(&PluginParams::yaml(
            "range_m: 5",
        ))?);
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
        let params = PluginParams::yaml(
            "{enable_altitude_split: true, split_altitude_m: 10, split_margin_m: 1}",
        );
        let network = SphereNetwork::new(&SphereNetwork::configure(&params)?);
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
    fn invalid_range_probability_delay_and_old_delay_keys_are_rejected() {
        for params in [
            "range_m: -1",
            "range_m: .nan",
            "transmit_probability: 1.1",
            "transmit_probability: -0.1",
            "split_margin_m: -1",
            "delay_s: -1",
            "comm_delay: 0",
            "is_stochastic_delay: true",
        ] {
            assert!(
                SphereNetwork::configure(&PluginParams::yaml(params)).is_err(),
                "{params}"
            );
        }
    }
}

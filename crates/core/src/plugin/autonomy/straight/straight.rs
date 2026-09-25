//! Fly toward a distant point along the initial heading, holding altitude and speed.
//!
//! C++ counterpart: src/plugins/autonomy/Straight/Straight.cpp.
//! Autonomy phase: read current state, write desired altitude, speed, and heading.

use anyhow::{Result, ensure};
use serde::Deserialize;

use crate::math::{self, Vec3};
use crate::plugin::interaction::{BOUNDARY_TOPIC, BoundaryRegion};
use crate::plugin::sensor::{STATE_TOPIC, StateWithCovariance};
use crate::plugin::{
    AgentContext, Autonomy, Frame, Plugin, PluginIo, PluginParams, Port, Ports, Unit, Update,
};

const ALTITUDE: &str = "desired_altitude";
const SPEED: &str = "desired_speed";
const HEADING: &str = "desired_heading";

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StraightConfig {
    #[serde(rename = "speed")]
    speed_mps: f64,
    #[serde(rename = "enable_boundary_control")]
    boundary_control: bool,
    /// Excluded; missions schedule entities instead. Must stay false.
    generate_entities: bool,
    /// Not supported; visualization belongs to Rerun. Must stay false.
    show_camera_images: bool,
    save_camera_images: bool,
    /// Accepted and ignored: the Rerun adapter already labels every entity.
    #[serde(rename = "show_text_label")]
    _show_text_label: bool,
}

impl Default for StraightConfig {
    fn default() -> Self {
        Self {
            speed_mps: 21.0,
            boundary_control: false,
            generate_entities: false,
            show_camera_images: false,
            save_camera_images: false,
            _show_text_label: false,
        }
    }
}

pub struct Straight {
    speed_mps: f64,
    goal_world_m: Vec3,
    boundary_control: bool,
    boundary: Option<BoundaryRegion>,
}

impl Plugin for Straight {
    type Config = StraightConfig;

    fn configure(params: &PluginParams<'_>) -> Result<StraightConfig> {
        let config: StraightConfig = params.parse()?;
        ensure!(
            !config.generate_entities,
            "Straight.generate_entities is excluded; use mission-scheduled entities"
        );
        ensure!(
            !config.show_camera_images,
            "Straight.show_camera_images is not supported; visualization belongs to Rerun"
        );
        ensure!(
            !config.save_camera_images,
            "Straight.save_camera_images is not supported; visualization belongs to Rerun"
        );
        Ok(config)
    }

    fn new(config: &StraightConfig) -> Self {
        Self {
            speed_mps: config.speed_mps,
            goal_world_m: Vec3::zeros(),
            boundary_control: config.boundary_control,
            boundary: None,
        }
    }

    fn ports(_config: &StraightConfig) -> Ports {
        Ports::default()
            .output(Port::new(ALTITUDE, Unit::Meters, Frame::World))
            .output(Port::new(SPEED, Unit::MetersPerSecond, Frame::None))
            .output(Port::new(HEADING, Unit::Radians, Frame::World))
    }
}

impl Autonomy for Straight {
    fn initialize(&mut self, context: &mut AgentContext<'_>) -> Result<()> {
        let forward_world_m = context
            .state
            .orientation_world_from_body
            .rotate_body_to_world(Vec3::new(1e6, 0.0, 0.0));
        self.goal_world_m = context.state.position_world_m + forward_world_m;
        self.goal_world_m.z = context.state.position_world_m.z;
        context
            .messages
            .subscribe::<StateWithCovariance>("LocalNetwork", STATE_TOPIC)?;
        context
            .messages
            .subscribe::<BoundaryRegion>("GlobalNetwork", BOUNDARY_TOPIC)?;
        Ok(())
    }

    fn step(&mut self, context: &mut AgentContext<'_>, io: &mut PluginIo) -> Result<Update> {
        for message in context
            .messages
            .receive::<BoundaryRegion>("GlobalNetwork", BOUNDARY_TOPIC)?
        {
            self.boundary = Some(*message.value);
        }
        let messages = context
            .messages
            .receive::<StateWithCovariance>("LocalNetwork", STATE_TOPIC)?;
        // Use the newest delivered sample, or current belief if none arrived.
        // Do not cache an old message across steps: this matches C++ Straight's fallback.
        let state = messages
            .last()
            .map_or(context.state, |message| &message.value.state);
        if self.boundary_control
            && let Some(boundary) = self.boundary
            && !boundary.contains(state.position_world_m)
        {
            let mut center_world_m = boundary.center_world_m;
            center_world_m.z = state.position_world_m.z;
            let toward_center_m = center_world_m - state.position_world_m;
            let distance_m = toward_center_m.norm();
            // Directly above/below the center gives no horizontal direction to turn toward.
            if distance_m > 0.0 {
                self.goal_world_m = state.position_world_m + toward_center_m / distance_m * 1e6;
            }
        }
        let displacement_world_m = math::sub(self.goal_world_m, state.position_world_m);
        let distance_m = math::norm(displacement_world_m);
        let velocity_world_mps =
            displacement_world_m.map(|component_m| self.speed_mps * component_m / distance_m);
        let desired_speed_mps = math::norm(velocity_world_mps);
        let desired_heading_world_rad =
            math::angle_2pi(velocity_world_mps.y.atan2(velocity_world_mps.x));

        io.write(ALTITUDE, self.goal_world_m.z)?;
        io.write(SPEED, desired_speed_mps)?;
        io.write(HEADING, desired_heading_world_rad)?;
        Ok(Update::Applied)
    }
}

#[cfg(test)]
mod tests {
    use super::{Plugin, PluginParams, Straight};
    use crate::Params;

    #[test]
    fn local_messages_override_belief_only_on_the_receiving_entity() -> anyhow::Result<()> {
        use crate::plugin::{
            AgentContext, Autonomy, EntityInfo, Messages, Observations, PluginIo, PluginRandom,
            StepTime,
        };
        use crate::plugin::{
            Network,
            network::LocalNetwork,
            sensor::{STATE_TOPIC, StateWithCovariance},
        };
        use crate::pubsub::messages::Mailbox;
        use crate::pubsub::{MessageEndpoint, NetworkContext};
        use crate::{KinematicState, Vec3};

        let params = Params::new();
        let config = Straight::configure(&PluginParams(&params))?;
        let state = KinematicState::default();
        let observations = Observations::default();
        let time = StepTime {
            time_s: 0.0,
            dt_s: 0.1,
        };
        let mut autonomies = [Straight::new(&config), Straight::new(&config)];
        let mut subscribers = [Messages::default(), Messages::default()];
        for (index, (autonomy, messages)) in autonomies.iter_mut().zip(&mut subscribers).enumerate()
        {
            autonomy.speed_mps = 20.0;
            autonomy.initialize(&mut AgentContext {
                entity: EntityInfo {
                    id: index as i32 + 1,
                    team_id: 1,
                    sub_swarm_id: 0,
                },
                time,
                state: &state,
                observations: &observations,
                contacts_truth: &[],
                messages,
                random: &mut PluginRandom::new(1, 1, "test"),
            })?;
        }
        let mut publisher = Messages::default();
        for y in [100.0, 200.0] {
            publisher.publish(
                "LocalNetwork",
                STATE_TOPIC,
                StateWithCovariance {
                    state: KinematicState {
                        position_world_m: Vec3::new(0.0, y, 0.0),
                        ..KinematicState::default()
                    },
                    covariance: nalgebra::Matrix3::identity(),
                },
            )?;
        }
        let mut network = LocalNetwork::new(&LocalNetwork::configure(&PluginParams(&params))?);
        let endpoint = MessageEndpoint {
            entity_id: None,
            plugin: "LocalNetwork".into(),
        };
        let [first, second] = &mut subscribers;
        let mut mailboxes = [
            Mailbox {
                endpoint: MessageEndpoint {
                    entity_id: Some(1),
                    plugin: "sensor".into(),
                },
                messages: &mut publisher,
            },
            Mailbox {
                endpoint: MessageEndpoint {
                    entity_id: Some(1),
                    plugin: "autonomy".into(),
                },
                messages: first,
            },
            Mailbox {
                endpoint: MessageEndpoint {
                    entity_id: Some(2),
                    plugin: "autonomy".into(),
                },
                messages: second,
            },
        ];
        network.step(&mut NetworkContext {
            time,
            contacts_truth: &[],
            name: "LocalNetwork",
            endpoint: &endpoint,
            mailboxes: &mut mailboxes,
            scheduled: &mut Vec::new(),
            random: &mut PluginRandom::new(1, 0, "test"),
            routed: false,
        })?;
        for (index, (autonomy, messages)) in autonomies.iter_mut().zip(&mut subscribers).enumerate()
        {
            let mut context = AgentContext {
                entity: EntityInfo {
                    id: index as i32 + 1,
                    team_id: 1,
                    sub_swarm_id: 0,
                },
                time,
                state: &state,
                observations: &observations,
                contacts_truth: &[],
                messages,
                random: &mut PluginRandom::new(1, 1, "test"),
            };
            let mut io = PluginIo::new(&Straight::ports(&config));
            autonomy.step(&mut context, &mut io)?;
            let expected = if index == 0 {
                crate::math::angle_2pi((-200.0_f64).atan2(1e6))
            } else {
                0.0
            };
            assert!((io.outputs["desired_heading"] - expected).abs() < 1e-12);
            // With no new message, fall back to belief instead of retaining the old sample.
            autonomy.step(&mut context, &mut io)?;
            assert_eq!(io.outputs["desired_heading"], 0.0);
        }
        Ok(())
    }

    #[test]
    fn configured_speed_is_copied_into_independent_autonomy_instances() -> anyhow::Result<()> {
        let params = Params::from([("speed".into(), "24".into())]);
        let config = Straight::configure(&PluginParams(&params))?;
        let mut first = Straight::new(&config);
        let second = Straight::new(&config);
        first.goal_world_m.x = 100.0;
        assert!((second.speed_mps - 24.0).abs() < 1e-12);
        assert!(second.goal_world_m.x.abs() < 1e-12);
        Ok(())
    }
}

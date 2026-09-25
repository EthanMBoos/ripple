//! Small Rust-native route follower, not a port of the legacy MotorSchemas stack.
//! Autonomy phase: receive a route, advance its index, write aircraft and ENU velocity commands.
//! Aircraft missions should loop their route: a fixed-wing model cannot stop at the last point.

use anyhow::{Result, ensure};
use serde::Deserialize;

use crate::common::{WAYPOINT_TOPIC, WaypointList};
use crate::math::{self, Vec3};
use crate::plugin::{
    AgentContext, Autonomy, Frame, Plugin, PluginIo, PluginParams, Port, Ports, Unit, Update,
};

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WaypointFollowerConfig {
    /// Initial route, replaced by any route received on `Waypoints`.
    #[serde(rename = "waypoints")]
    route: WaypointList,
    #[serde(rename = "speed")]
    speed_mps: f64,
    arrival_radius_m: f64,
    repeat: bool,
}

impl Default for WaypointFollowerConfig {
    fn default() -> Self {
        Self {
            route: WaypointList::default_route(),
            speed_mps: 20.0,
            arrival_radius_m: 10.0,
            repeat: false,
        }
    }
}

pub struct WaypointFollower {
    route: WaypointList,
    current: usize,
    speed_mps: f64,
    arrival_radius_m: f64,
    repeat: bool,
}

impl Plugin for WaypointFollower {
    type Config = WaypointFollowerConfig;

    fn configure(params: &PluginParams<'_>) -> Result<Self::Config> {
        let config: WaypointFollowerConfig = params.parse()?;
        ensure!(
            config.speed_mps > 0.0 && config.arrival_radius_m > 0.0,
            "waypoint speed and arrival radius must be positive"
        );
        Ok(config)
    }

    fn new(config: &Self::Config) -> Self {
        Self {
            route: config.route.clone(),
            current: 0,
            speed_mps: config.speed_mps,
            arrival_radius_m: config.arrival_radius_m,
            repeat: config.repeat,
        }
    }

    fn ports(_: &Self::Config) -> Ports {
        Ports::default()
            .output(Port::new("desired_heading", Unit::Radians, Frame::World))
            .output(Port::new("desired_altitude", Unit::Meters, Frame::World))
            .output(Port::new(
                "desired_speed",
                Unit::MetersPerSecond,
                Frame::None,
            ))
            .output(Port::new("velocity_x", Unit::MetersPerSecond, Frame::World))
            .output(Port::new("velocity_y", Unit::MetersPerSecond, Frame::World))
            .output(Port::new("velocity_z", Unit::MetersPerSecond, Frame::World))
    }
}

impl Autonomy for WaypointFollower {
    fn initialize(&mut self, context: &mut AgentContext<'_>) -> Result<()> {
        context
            .messages
            .subscribe::<WaypointList>("GlobalNetwork", WAYPOINT_TOPIC)
    }

    fn step(&mut self, context: &mut AgentContext<'_>, io: &mut PluginIo) -> Result<Update> {
        for message in context
            .messages
            .receive::<WaypointList>("GlobalNetwork", WAYPOINT_TOPIC)?
        {
            message.value.validate()?;
            self.route = (*message.value).clone();
            self.current = 0;
        }
        let position_world_m = context.state.position_world_m;
        self.advance(position_world_m);
        let target_world_m = self.route.positions_world_m[self.current];
        let displacement_m = target_world_m - position_world_m;
        let distance_m = displacement_m.norm();
        let arrived = distance_m <= self.arrival_radius_m;
        let velocity_world_mps = if arrived {
            Vec3::zeros()
        } else {
            // The point model can land on a waypoint without overshooting the next sample.
            // Known limitation: with loop_rate set, dt_s is the phase dt, not the time since
            // this plugin last ran (the rate gate passes dt unchanged, matching C++).
            let speed_mps = self.speed_mps.min(distance_m / context.time.dt_s);
            displacement_m / distance_m * speed_mps
        };
        let heading_rad = if arrived {
            context
                .state
                .orientation_world_from_body
                .yaw_world_from_body_rad()
        } else {
            math::angle_2pi(displacement_m.y.atan2(displacement_m.x))
        };
        io.write("desired_heading", heading_rad)?;
        io.write("desired_altitude", target_world_m.z)?;
        io.write("desired_speed", velocity_world_mps.norm())?;
        io.write("velocity_x", velocity_world_mps.x)?;
        io.write("velocity_y", velocity_world_mps.y)?;
        io.write("velocity_z", velocity_world_mps.z)?;
        Ok(Update::Applied)
    }
}

impl WaypointFollower {
    fn advance(&mut self, position_world_m: Vec3) {
        // At most one trip through the list, even if every point is inside the arrival radius.
        for _ in 0..self.route.positions_world_m.len() {
            if (self.route.positions_world_m[self.current] - position_world_m).norm()
                > self.arrival_radius_m
            {
                break;
            }
            if self.current + 1 < self.route.positions_world_m.len() {
                self.current += 1;
            } else if self.repeat {
                self.current = 0;
            } else {
                break;
            }
        }
    }
}

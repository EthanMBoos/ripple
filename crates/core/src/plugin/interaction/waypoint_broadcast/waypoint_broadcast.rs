//! Rust-native mission route publisher: one initial route and an optional timed replacement.
//! Interaction phase; ordinary typed GlobalNetwork messages, no services or spawn commands.

use anyhow::{Result, ensure};
use serde::Deserialize;

use crate::common::{WAYPOINT_TOPIC, WaypointList};
use crate::plugin::{Interaction, InteractionContext, Plugin, PluginParams, Update};

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WaypointBroadcastConfig {
    waypoints_m: WaypointList,
    /// Optional replacement route, published once `update_at_s` is reached.
    update_waypoints_m: Option<WaypointList>,
    update_at_s: Option<f64>,
}

impl Default for WaypointBroadcastConfig {
    fn default() -> Self {
        Self {
            waypoints_m: WaypointList::default_route(),
            update_waypoints_m: None,
            update_at_s: None,
        }
    }
}

pub struct WaypointBroadcast {
    waypoints_m: Option<WaypointList>,
    update_waypoints_m: Option<WaypointList>,
    update_at_s: f64,
}

impl Plugin for WaypointBroadcast {
    type Config = WaypointBroadcastConfig;

    fn configure(params: &PluginParams<'_>) -> Result<Self::Config> {
        let config: WaypointBroadcastConfig = params.parse()?;
        ensure!(
            config.update_waypoints_m.is_some() == config.update_at_s.is_some(),
            "provide both update_waypoints and update_at_s"
        );
        Ok(config)
    }

    fn new(config: &Self::Config) -> Self {
        Self {
            waypoints_m: Some(config.waypoints_m.clone()),
            update_waypoints_m: config.update_waypoints_m.clone(),
            // Unused when there is no replacement route.
            update_at_s: config.update_at_s.unwrap_or(0.0),
        }
    }
}

impl Interaction for WaypointBroadcast {
    fn step(&mut self, context: &mut InteractionContext<'_>) -> Result<Update> {
        // Known limitation (the same publish-once pattern as Boundary):
        // entities spawned later never receive the route and keep their own configured one.
        // A possible fix is republishing the current route when an entity is generated.
        if let Some(route) = self.waypoints_m.take() {
            context
                .messages
                .publish("GlobalNetwork", WAYPOINT_TOPIC, route)?;
        }
        if context.time.time_s >= self.update_at_s
            && let Some(route) = self.update_waypoints_m.take()
        {
            context
                .messages
                .publish("GlobalNetwork", WAYPOINT_TOPIC, route)?;
        }
        Ok(Update::Applied)
    }
}

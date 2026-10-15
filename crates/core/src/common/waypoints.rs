//! Typed local-ENU route shared by the waypoint publisher and autonomy.

use anyhow::{Result, ensure};
use serde::Deserialize;

use crate::Vec3;

pub const WAYPOINT_TOPIC: &str = "Waypoints";

/// Deserializes from a list of points such as `[[0, 0, 200], [100, 0, 200]]`.
#[derive(Clone, Debug, Deserialize)]
#[serde(try_from = "Vec<[f64; 3]>")]
pub struct WaypointList {
    pub positions_world_m: Vec<Vec3>,
}

impl WaypointList {
    /// The single point used when a mission gives no route.
    pub(crate) fn default_route() -> Self {
        Self {
            positions_world_m: vec![Vec3::new(0.0, 0.0, 200.0)],
        }
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.positions_world_m.is_empty(),
            "waypoint list must not be empty"
        );
        ensure!(
            self.positions_world_m
                .iter()
                .all(|point| point.iter().all(|value| value.is_finite())),
            "waypoints must contain finite local ENU coordinates"
        );
        Ok(())
    }
}

impl TryFrom<Vec<[f64; 3]>> for WaypointList {
    type Error = anyhow::Error;
    fn try_from(points: Vec<[f64; 3]>) -> Result<Self> {
        let list = Self {
            positions_world_m: points.into_iter().map(Vec3::from).collect(),
        };
        list.validate()?;
        Ok(list)
    }
}

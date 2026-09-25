//! Typed local-ENU route shared by the waypoint publisher and autonomy.

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::Vec3;

pub const WAYPOINT_TOPIC: &str = "Waypoints";

/// Deserializes from mission text such as `"0,0,200; 100,0,200"`.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(try_from = "String")]
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

    /// Mission syntax: "x,y,z; x,y,z". This is not the legacy GPS waypoint format.
    pub(crate) fn parse(text: &str) -> Result<Self> {
        let mut positions_world_m = Vec::new();
        for point in text.split(';') {
            let coordinates = point
                .split(|c: char| c == ',' || c.is_whitespace())
                .filter(|value| !value.is_empty())
                .map(str::parse::<f64>)
                .collect::<std::result::Result<Vec<_>, _>>()
                .context("waypoint requires numeric x,y,z coordinates")?;
            ensure!(
                coordinates.len() == 3,
                "waypoint requires exactly three local ENU coordinates"
            );
            positions_world_m.push(Vec3::new(coordinates[0], coordinates[1], coordinates[2]));
        }
        let list = Self { positions_world_m };
        list.validate()?;
        Ok(list)
    }
}

impl TryFrom<String> for WaypointList {
    type Error = anyhow::Error;
    fn try_from(text: String) -> Result<Self> {
        Self::parse(&text)
    }
}

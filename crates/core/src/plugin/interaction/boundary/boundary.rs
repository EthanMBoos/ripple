//! Publish an axis-aligned mission boundary; autonomies decide how to respond.
//! Derived from SCRIMMAGE: interaction/Boundary. Publishes once in the interaction phase.
//! Geometry is ordinary Rust data, independent of rendering and physical collision.

use anyhow::{Result, ensure};
use serde::Deserialize;

use crate::Vec3;
use crate::plugin::{Interaction, InteractionContext, Plugin, PluginParams, Update};

pub const BOUNDARY_TOPIC: &str = "Boundary";

#[derive(Clone, Copy, Debug)]
pub struct BoundaryRegion {
    pub center_world_m: Vec3,
    pub lengths_m: Vec3,
}

impl BoundaryRegion {
    /// Faces are outside (strict comparisons).
    pub fn contains(&self, position_world_m: Vec3) -> bool {
        let offset_m = position_world_m - self.center_world_m;
        for axis in 0..3 {
            if offset_m[axis].abs() >= self.lengths_m[axis] / 2.0 {
                return false;
            }
        }
        true
    }
}

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BoundaryConfig {
    center_m: [f64; 3],
    lengths_m: [f64; 3],
    network: String,
}

impl Default for BoundaryConfig {
    fn default() -> Self {
        Self {
            center_m: [0.0; 3],
            lengths_m: [10.0; 3],
            network: "GlobalNetwork".into(),
        }
    }
}

pub struct Boundary {
    region: BoundaryRegion,
    network: String,
    published: bool,
}

impl Plugin for Boundary {
    type Config = BoundaryConfig;

    fn configure(params: &PluginParams<'_>) -> Result<BoundaryConfig> {
        let config: BoundaryConfig = params.parse()?;
        ensure!(
            config.lengths_m.iter().all(|length| *length > 0.0),
            "Boundary lengths must be positive"
        );
        Ok(config)
    }

    fn new(config: &BoundaryConfig) -> Self {
        Self {
            region: BoundaryRegion {
                center_world_m: Vec3::from(config.center_m),
                lengths_m: Vec3::from(config.lengths_m),
            },
            network: config.network.clone(),
            published: false,
        }
    }
}

impl Interaction for Boundary {
    fn step(&mut self, context: &mut InteractionContext<'_>) -> Result<Update> {
        // Published once: entities spawned after the first tick never receive the
        // boundary. Only matters if a mission spawns late.
        if !self.published {
            context
                .messages
                .publish(&self.network, BOUNDARY_TOPIC, self.region)?;
            self.published = true;
        }
        Ok(Update::Applied)
    }
}

#[cfg(test)]
mod tests {
    use super::BoundaryRegion;
    use crate::Vec3;

    #[test]
    fn cuboid_contains_interior_but_not_its_faces() {
        let region = BoundaryRegion {
            center_world_m: Vec3::new(10.0, 20.0, 30.0),
            lengths_m: Vec3::new(2.0, 4.0, 6.0),
        };
        assert!(region.contains(region.center_world_m));
        for axis in 0..3 {
            for sign in [-1.0, 1.0] {
                let mut point = region.center_world_m;
                point[axis] += sign * region.lengths_m[axis] / 2.0;
                assert!(!region.contains(point));
            }
        }
    }
}

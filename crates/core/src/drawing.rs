//! What a plugin draws in the viewer: mission geometry in world meters, such
//! as a boundary box, a depot, a site, or an area to keep out of. Display
//! only: the engine never reads a drawing.
//!
//! A plugin's shapes come in two parts, so the engine never rebuilds what
//! cannot change. `fixed_drawings` is asked once, when the plugin starts,
//! and held by reference for the rest of the run; a region of 64 hubs and
//! 512 sites is built once, not on each of a day's 300,000 ticks to be found
//! the same. `drawings` is asked every tick, after the metrics phase and on
//! the main thread, for shapes that come and go, such as an area closed for
//! an hour; it is usually empty, which costs nothing to build or compare.
//! The record writes a frame's drawings only when they differ from the
//! previous frame's, and the viewer redraws a plugin's part only then.
//!
//! What the viewer can derive from the record over time stays in the
//! viewer and out of this API: an agent's trail is its last seconds of
//! truth positions, already in the frames, and redrawing it every frame
//! through here would record every position twice and cost what the trails
//! module measured. Plugins see belief, not truth, so they could not draw
//! that trail anyway. A plugin draws what only it knows: its region, its
//! rules, its plan.
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::Vec3;

/// One shape in the local East-North-Up world frame. The label is shown
/// beside a prominent point and on hover for everything else; an empty
/// label shows nothing. The color is the plugin's choice, not the mission's.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "shape", rename_all = "snake_case")]
pub enum Drawing {
    /// A marker. A prominent one is larger and always labeled, for the few
    /// places a reader must find at a glance (a depot); a plain one is small
    /// and labeled on hover, for the many (its sites).
    Point {
        position_world_m: Vec3,
        label: String,
        color: [u8; 3],
        prominent: bool,
    },
    /// A horizontal circle at its center's altitude, outlined and lightly
    /// filled.
    Circle {
        center_world_m: Vec3,
        radius_m: f64,
        label: String,
        color: [u8; 3],
    },
    /// An open line through its points.
    Line {
        points_world_m: Vec<Vec3>,
        label: String,
        color: [u8; 3],
    },
    /// A flat convex polygon, outlined and lightly filled.
    Polygon {
        points_world_m: Vec<Vec3>,
        label: String,
        color: [u8; 3],
    },
    /// An axis-aligned box: its edges drawn and its side walls lightly
    /// filled, with the floor and roof open so what flies inside stays
    /// visible.
    Box {
        center_world_m: Vec3,
        lengths_m: Vec3,
        label: String,
        color: [u8; 3],
    },
}

/// One plugin's drawings. `source` names the plugin, and for an entity's
/// plugin its entity too: `Boundary` for a world plugin, `Dispatcher/7` for
/// the Dispatcher on entity 7. It is the drawings' path in the viewer.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(from = "Recorded", into = "Recorded")]
pub struct PluginDrawings {
    pub source: String,
    /// Asked once when the plugin started; shared, not copied, by every
    /// frame since.
    pub fixed: Arc<[Drawing]>,
    /// Asked this tick.
    pub changing: Vec<Drawing>,
}

impl PartialEq for PluginDrawings {
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source
            && self.changing == other.changing
            && (Arc::ptr_eq(&self.fixed, &other.fixed) || self.fixed == other.fixed)
    }
}

/// `PluginDrawings` as the record writes it, with the fixed part owned.
#[derive(Serialize, Deserialize)]
struct Recorded {
    source: String,
    fixed: Vec<Drawing>,
    changing: Vec<Drawing>,
}

impl From<Recorded> for PluginDrawings {
    fn from(recorded: Recorded) -> Self {
        Self {
            source: recorded.source,
            fixed: recorded.fixed.into(),
            changing: recorded.changing,
        }
    }
}

impl From<PluginDrawings> for Recorded {
    fn from(drawings: PluginDrawings) -> Self {
        Self {
            source: drawings.source,
            fixed: drawings.fixed.to_vec(),
            changing: drawings.changing,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drawing_names_its_shape_in_json_and_reads_back() {
        let drawings = vec![PluginDrawings {
            source: "Dispatcher/1".into(),
            fixed: vec![Drawing::Point {
                position_world_m: Vec3::new(1.0, 2.0, 3.0),
                label: "hub 1".into(),
                color: [255, 210, 80],
                prominent: true,
            }]
            .into(),
            changing: vec![Drawing::Box {
                center_world_m: Vec3::zeros(),
                lengths_m: Vec3::new(10.0, 10.0, 10.0),
                label: String::new(),
                color: [170, 170, 120],
            }],
        }];
        let text = serde_json::to_string(&drawings).unwrap();
        assert!(text.contains(r#""shape":"point""#));
        assert!(text.contains(r#""position_world_m":[1.0,2.0,3.0]"#));
        let back: Vec<PluginDrawings> = serde_json::from_str(&text).unwrap();
        assert_eq!(back, drawings);
    }

    #[test]
    fn frames_sharing_the_fixed_part_compare_by_pointer_first() {
        let fixed: Arc<[Drawing]> = vec![Drawing::Point {
            position_world_m: Vec3::zeros(),
            label: "hub 1".into(),
            color: [255, 210, 80],
            prominent: true,
        }]
        .into();
        let one = PluginDrawings {
            source: "Dispatcher/1".into(),
            fixed: fixed.clone(),
            changing: Vec::new(),
        };
        let two = PluginDrawings {
            fixed,
            ..one.clone()
        };
        assert!(Arc::ptr_eq(&one.fixed, &two.fixed));
        assert_eq!(one, two);
        let other = PluginDrawings {
            changing: vec![Drawing::Line {
                points_world_m: vec![Vec3::zeros(), Vec3::new(1.0, 0.0, 0.0)],
                label: String::new(),
                color: [0, 0, 0],
            }],
            ..one.clone()
        };
        assert_ne!(one, other);
    }
}

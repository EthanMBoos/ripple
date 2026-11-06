//! A closed area: a circle, or a corridor between two points. Written in
//! kilometers, as the fleet manager and the mission write them; the geometry
//! works in meters. `detour` finds one waypoint that takes a flight around an
//! area, which the dispatcher gives the courier.

use ripple_core::Vec3;
use serde::de::{DeserializeOwned, Deserializer, Error};
use serde::{Deserialize, Serialize};

use crate::region::horizontal_m;

/// Vehicles may cross a corridor above this height; a circle is closed at
/// every height.
pub const CORRIDOR_CEILING_M: f64 = 150.0;

/// Reads an enum written as a one-key map, `{circle: {...}}`, from a YAML
/// mission as well as from a JSON reply. serde's YAML reader would otherwise
/// want a `!circle` tag, and the mission should read like the orders the fleet
/// manager writes. Put it on fields holding an `Area` or an `Order`.
pub fn tagged<'de, D: Deserializer<'de>, T: DeserializeOwned>(
    deserializer: D,
) -> Result<T, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    serde_json::from_value(value).map_err(D::Error::custom)
}

/// `tagged` for an optional field.
pub fn tagged_option<'de, D: Deserializer<'de>, T: DeserializeOwned>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    if value.is_null() {
        return Ok(None);
    }
    serde_json::from_value(value)
        .map(Some)
        .map_err(D::Error::custom)
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Area {
    Circle {
        center_km: [f64; 2],
        radius_km: f64,
    },
    Corridor {
        from_km: [f64; 2],
        to_km: [f64; 2],
        half_width_km: f64,
    },
}

fn km_to_m(point_km: [f64; 2]) -> Vec3 {
    Vec3::new(point_km[0] * 1000.0, point_km[1] * 1000.0, 0.0)
}

impl Area {
    pub fn valid(&self) -> bool {
        match self {
            Area::Circle {
                center_km,
                radius_km,
            } => center_km.iter().all(|v| v.is_finite()) && *radius_km > 0.0,
            Area::Corridor {
                from_km,
                to_km,
                half_width_km,
            } => {
                from_km.iter().chain(to_km.iter()).all(|v| v.is_finite())
                    && *half_width_km > 0.0
                    && from_km != to_km
            }
        }
    }

    /// Whether a vehicle at this point is inside the area.
    pub fn contains(&self, point_world_m: Vec3) -> bool {
        match self {
            Area::Circle {
                center_km,
                radius_km,
            } => horizontal_m(point_world_m, km_to_m(*center_km)) < radius_km * 1000.0,
            Area::Corridor {
                from_km,
                to_km,
                half_width_km,
            } => {
                point_world_m.z < CORRIDOR_CEILING_M
                    && point_segment_m(point_world_m, km_to_m(*from_km), km_to_m(*to_km))
                        < half_width_km * 1000.0
            }
        }
    }

    /// Whether a straight flight from `from` to `to` enters the area.
    pub fn crosses(&self, from: Vec3, to: Vec3) -> bool {
        match self {
            Area::Circle {
                center_km,
                radius_km,
            } => point_segment_m(km_to_m(*center_km), from, to) < radius_km * 1000.0,
            Area::Corridor {
                from_km,
                to_km,
                half_width_km,
            } => {
                segment_segment_m(from, to, km_to_m(*from_km), km_to_m(*to_km))
                    < half_width_km * 1000.0
            }
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Area::Circle {
                center_km,
                radius_km,
            } => format!(
                "the circle of radius {radius_km:.1} km centered at ({:.1}, {:.1}) km",
                center_km[0], center_km[1]
            ),
            Area::Corridor {
                from_km,
                to_km,
                half_width_km,
            } => format!(
                "the corridor from ({:.1}, {:.1}) to ({:.1}, {:.1}) km, {half_width_km:.1} km \
                 either side, below {CORRIDOR_CEILING_M:.0} m",
                from_km[0], from_km[1], to_km[0], to_km[1]
            ),
        }
    }

    /// A waypoint that takes a flight from `from` to `to` around this area,
    /// pushed out until neither leg crosses it, or as far as the third try.
    fn detour(&self, from: Vec3, to: Vec3, margin_m: f64) -> Vec3 {
        let (anchor, mut away, size_m) = match self {
            Area::Circle {
                center_km,
                radius_km,
            } => {
                let center = km_to_m(*center_km);
                let nearest = nearest_on_segment(center, from, to);
                (center, (nearest - center).xy(), radius_km * 1000.0)
            }
            Area::Corridor {
                from_km,
                to_km,
                half_width_km,
            } => {
                // Around the corridor's end nearer the flight.
                let (p, q) = (km_to_m(*from_km), km_to_m(*to_km));
                let middle = (from + to) / 2.0;
                let (end, other) = if horizontal_m(middle, p) <= horizontal_m(middle, q) {
                    (p, q)
                } else {
                    (q, p)
                };
                (end, (end - other).xy(), half_width_km * 1000.0)
            }
        };
        if away.norm() < 1e-6 {
            // The flight passes through the anchor: step off to its left.
            let along = (to - from).xy();
            away = ripple_core::Vec3::new(-along.y, along.x, 0.0).xy();
        }
        let away = away.normalize();
        let mut waypoint = anchor;
        for factor in [1.5, 2.5, 4.0] {
            let reach_m = size_m * factor + margin_m;
            waypoint = Vec3::new(
                anchor.x + away.x * reach_m,
                anchor.y + away.y * reach_m,
                0.0,
            );
            if !self.crosses(from, waypoint) && !self.crosses(waypoint, to) {
                break;
            }
        }
        waypoint
    }
}

/// One waypoint around the first of `areas` a flight from `from` to `to`
/// would enter, or `None` when it enters none. A second area on the new legs
/// is left alone; the score records what the flight then crosses.
pub fn detour(from: Vec3, to: Vec3, areas: &[&Area], margin_m: f64) -> Option<Vec3> {
    let area = areas.iter().find(|area| area.crosses(from, to))?;
    Some(area.detour(from, to, margin_m))
}

/// The nearest point of segment `ab` to `point`, horizontally.
fn nearest_on_segment(point: Vec3, a: Vec3, b: Vec3) -> Vec3 {
    let path = (b - a).xy();
    let offset = (point - a).xy();
    let length_squared = path.norm_squared();
    let along = if length_squared < 1e-12 {
        0.0
    } else {
        (offset.dot(&path) / length_squared).clamp(0.0, 1.0)
    };
    Vec3::new(a.x + path.x * along, a.y + path.y * along, 0.0)
}

/// Horizontal distance from a point to a segment.
pub fn point_segment_m(point: Vec3, a: Vec3, b: Vec3) -> f64 {
    horizontal_m(point, nearest_on_segment(point, a, b))
}

/// Horizontal distance between two segments: zero when they cross.
fn segment_segment_m(a: Vec3, b: Vec3, p: Vec3, q: Vec3) -> f64 {
    if segments_intersect(a, b, p, q) {
        return 0.0;
    }
    [
        point_segment_m(a, p, q),
        point_segment_m(b, p, q),
        point_segment_m(p, a, b),
        point_segment_m(q, a, b),
    ]
    .into_iter()
    .fold(f64::INFINITY, f64::min)
}

fn segments_intersect(a: Vec3, b: Vec3, p: Vec3, q: Vec3) -> bool {
    let cross = |o: Vec3, u: Vec3, v: Vec3| (u.x - o.x) * (v.y - o.y) - (u.y - o.y) * (v.x - o.x);
    let d1 = cross(p, q, a);
    let d2 = cross(p, q, b);
    let d3 = cross(a, b, p);
    let d4 = cross(a, b, q);
    (d1 * d2 < 0.0) && (d3 * d4 < 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_detour_takes_the_flight_clear_of_a_circle_and_a_corridor() {
        let circle = Area::Circle {
            center_km: [5.0, 0.0],
            radius_km: 2.0,
        };
        let from = Vec3::new(0.0, 0.0, 100.0);
        let to = Vec3::new(10_000.0, 0.0, 100.0);
        assert!(circle.crosses(from, to));
        assert!(!circle.crosses(from, Vec3::new(0.0, 5000.0, 0.0)));
        let via = detour(from, to, &[&circle], 200.0).expect("a detour");
        assert!(
            !circle.crosses(from, via) && !circle.crosses(via, to),
            "{via}"
        );
        assert!(circle.contains(Vec3::new(5000.0, 1000.0, 500.0)));
        assert!(!circle.contains(Vec3::new(5000.0, 2500.0, 0.0)));

        let corridor = Area::Corridor {
            from_km: [5.0, -3.0],
            to_km: [5.0, 3.0],
            half_width_km: 0.5,
        };
        assert!(corridor.crosses(from, to));
        assert!(corridor.contains(Vec3::new(5200.0, 0.0, 100.0)));
        assert!(
            !corridor.contains(Vec3::new(5200.0, 0.0, 200.0)),
            "above the ceiling"
        );
        let via = detour(from, to, &[&corridor], 200.0).expect("a detour");
        assert!(
            !corridor.crosses(from, via) && !corridor.crosses(via, to),
            "{via}"
        );
        assert!(
            detour(
                from,
                Vec3::new(0.0, 1000.0, 0.0),
                &[&circle, &corridor],
                200.0
            )
            .is_none()
        );
    }
}

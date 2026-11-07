//! Plugin drawings (`ripple_core::Drawing`) in the 3D world and on the map,
//! under `world/drawings/<source>` and `map/drawings/<source>`, where
//! `source` is the plugin's name, with its entity's ID for an entity's
//! plugin (`Dispatcher/7`). A source's fixed shapes are drawn under
//! `fixed` once, and its changing shapes under `changing` only when they
//! change; a source that stops drawing is cleared. Rerun hides an entity
//! per view, so the blueprint panel's eye on `world/drawings` hides the 3D
//! shapes only; `map/drawings` has its own.
use anyhow::Result;
use rerun::RecordingStream;
use ripple_core::{Drawing, PluginDrawings, Vec3};
use std::{collections::BTreeMap, sync::Arc};

use super::map;

/// Sides of the polygon that stands in for a circle.
const CIRCLE_SIDES: usize = 48;
/// The alpha of a shape's fill, out of 255.
const FILL_ALPHA: u8 = 40;
/// Marker radii in UI points: a plain point sits under the agents' 6; a
/// prominent one stands above them.
const PLAIN_POINT_UI: f32 = 3.5;
const PROMINENT_POINT_UI: f32 = 8.0;

/// What one part of a source has on screen: whether it has points and
/// lines, and how many filled shapes, so a later frame can clear what the
/// part no longer draws without clearing what it redraws in the same frame.
#[derive(Default)]
struct Shown {
    points: bool,
    marks: bool,
    lines: bool,
    fills: usize,
}

/// Points of one kind in a batch: positions, colors, labels.
#[derive(Default)]
struct Points {
    positions: Vec<[f32; 3]>,
    colors: Vec<[u8; 3]>,
    labels: Vec<String>,
}

impl Points {
    fn push(&mut self, position: [f32; 3], color: [u8; 3], label: &str) {
        self.positions.push(position);
        self.colors.push(color);
        self.labels.push(label.to_string());
    }
}

/// A filled shape as a mesh.
struct Fill {
    vertices: Vec<[f32; 3]>,
    triangles: Vec<[u32; 3]>,
    color: [u8; 3],
}

/// Shapes sorted into Rerun batches, in world meters.
#[derive(Default)]
struct Batches {
    /// Plain points, small and labeled on hover.
    points: Points,
    /// Prominent points, larger and always labeled.
    marks: Points,
    /// Open lines and closed outlines in the 3D world, with their colors
    /// and labels.
    lines: Vec<Vec<[f32; 3]>>,
    line_colors: Vec<[u8; 3]>,
    line_labels: Vec<String>,
    /// The lines' map projections: the box's floor only.
    map_lines: Vec<Vec<[f32; 2]>>,
    map_line_colors: Vec<[u8; 3]>,
    fills: Vec<Fill>,
}

#[derive(Default)]
pub(super) struct Drawn {
    last: Vec<PluginDrawings>,
    /// Each source's parts on screen, by `source/part`.
    shown: BTreeMap<String, Shown>,
}

impl Drawn {
    /// Log the frame's drawings at the recording's current time; `widen`
    /// receives every point so the map can be fitted around them.
    pub fn log_frame(
        &mut self,
        recording: &RecordingStream,
        drawings: &[PluginDrawings],
        widen: &mut dyn FnMut([f32; 3]),
    ) -> Result<()> {
        if drawings == self.last.as_slice() {
            return Ok(());
        }
        for gone in &self.last {
            if !drawings.iter().any(|now| now.source == gone.source) {
                for root in ["world", "map"] {
                    recording.log(
                        format!("{root}/drawings/{}", gone.source),
                        &rerun::Clear::recursive(),
                    )?;
                }
                self.shown.remove(&format!("{}/fixed", gone.source));
                self.shown.remove(&format!("{}/changing", gone.source));
            }
        }
        for plugin in drawings {
            let was = self.last.iter().find(|was| was.source == plugin.source);
            let fixed_unchanged = was.is_some_and(|was| {
                Arc::ptr_eq(&was.fixed, &plugin.fixed) || was.fixed == plugin.fixed
            });
            let changing_unchanged = was.is_some_and(|was| was.changing == plugin.changing);
            if !fixed_unchanged {
                self.log_part(recording, &plugin.source, "fixed", &plugin.fixed, widen)?;
            }
            if !changing_unchanged {
                self.log_part(
                    recording,
                    &plugin.source,
                    "changing",
                    &plugin.changing,
                    widen,
                )?;
            }
        }
        self.last = drawings.to_vec();
        Ok(())
    }

    fn log_part(
        &mut self,
        recording: &RecordingStream,
        source: &str,
        part: &str,
        shapes: &[Drawing],
        widen: &mut dyn FnMut([f32; 3]),
    ) -> Result<()> {
        let world = format!("world/drawings/{source}/{part}");
        let map = format!("map/drawings/{source}/{part}");
        let batches = batches(shapes);
        for point in batches
            .points
            .positions
            .iter()
            .chain(&batches.marks.positions)
            .chain(batches.lines.iter().flatten())
        {
            widen(*point);
        }
        let shown = self.shown.entry(format!("{source}/{part}")).or_default();

        shown.points = log_points(
            recording,
            &format!("{world}/points"),
            &format!("{map}/points"),
            &batches.points,
            shown.points,
            PLAIN_POINT_UI,
            false,
        )?;
        shown.marks = log_points(
            recording,
            &format!("{world}/marks"),
            &format!("{map}/marks"),
            &batches.marks,
            shown.marks,
            PROMINENT_POINT_UI,
            true,
        )?;

        let lines_path = format!("{world}/lines");
        let map_lines_path = format!("{map}/lines");
        if batches.lines.is_empty() {
            if shown.lines {
                recording.log(lines_path, &rerun::Clear::flat())?;
                recording.log(map_lines_path, &rerun::Clear::flat())?;
            }
        } else {
            recording.log(
                lines_path,
                &rerun::LineStrips3D::new(batches.lines.clone())
                    .with_colors(batches.line_colors.clone())
                    .with_radii([rerun::Radius::new_ui_points(1.0)])
                    .with_labels(batches.line_labels.clone())
                    .with_show_labels(false),
            )?;
            recording.log(
                map_lines_path,
                &rerun::LineStrips2D::new(batches.map_lines.clone())
                    .with_colors(batches.map_line_colors.clone())
                    .with_radii([rerun::Radius::new_ui_points(1.0)]),
            )?;
        }
        shown.lines = !batches.lines.is_empty();

        for (index, fill) in batches.fills.iter().enumerate() {
            let [red, green, blue] = fill.color;
            recording.log(
                format!("{world}/fills/{index}"),
                &rerun::Mesh3D::new(fill.vertices.clone())
                    .with_triangle_indices(fill.triangles.clone())
                    .with_albedo_factor(rerun::Rgba32::from_unmultiplied_rgba(
                        red, green, blue, FILL_ALPHA,
                    )),
            )?;
        }
        for index in batches.fills.len()..shown.fills {
            recording.log(format!("{world}/fills/{index}"), &rerun::Clear::flat())?;
        }
        shown.fills = batches.fills.len();
        Ok(())
    }
}

/// Log one kind of points to the world and the map, or clear them when
/// the kind was shown and is now empty; returns whether it is shown.
fn log_points(
    recording: &RecordingStream,
    world_path: &str,
    map_path: &str,
    points: &Points,
    was_shown: bool,
    radius_ui: f32,
    show_labels: bool,
) -> Result<bool> {
    if points.positions.is_empty() {
        if was_shown {
            recording.log(world_path, &rerun::Clear::flat())?;
            recording.log(map_path, &rerun::Clear::flat())?;
        }
        return Ok(false);
    }
    recording.log(
        world_path,
        &rerun::Points3D::new(points.positions.clone())
            .with_colors(points.colors.clone())
            .with_radii([rerun::Radius::new_ui_points(radius_ui)])
            .with_labels(points.labels.clone())
            .with_show_labels(show_labels),
    )?;
    recording.log(
        map_path,
        &rerun::Points2D::new(points.positions.iter().map(|point| map::project(*point)))
            .with_colors(points.colors.clone())
            .with_radii([rerun::Radius::new_ui_points(radius_ui)])
            .with_labels(points.labels.clone())
            .with_show_labels(show_labels),
    )?;
    Ok(true)
}

fn batches(drawings: &[Drawing]) -> Batches {
    let mut batches = Batches::default();
    for drawing in drawings {
        match drawing {
            Drawing::Point {
                position_world_m,
                label,
                color,
                prominent,
            } => {
                let points = if *prominent {
                    &mut batches.marks
                } else {
                    &mut batches.points
                };
                points.push(point(*position_world_m), *color, label);
            }
            Drawing::Line {
                points_world_m,
                label,
                color,
            } => {
                let strip: Vec<_> = points_world_m.iter().map(|p| point(*p)).collect();
                let map_strip = strip.iter().map(|p| map::project(*p)).collect();
                batches.line(strip, map_strip, label, *color);
            }
            Drawing::Circle {
                center_world_m,
                radius_m,
                label,
                color,
            } => {
                let outline: Vec<_> = (0..CIRCLE_SIDES)
                    .map(|side| {
                        let angle = std::f64::consts::TAU * side as f64 / CIRCLE_SIDES as f64;
                        point(
                            center_world_m
                                + Vec3::new(radius_m * angle.cos(), radius_m * angle.sin(), 0.0),
                        )
                    })
                    .collect();
                batches.area(outline, label, *color);
            }
            Drawing::Polygon {
                points_world_m,
                label,
                color,
            } => {
                let outline: Vec<_> = points_world_m.iter().map(|p| point(*p)).collect();
                batches.area(outline, label, *color);
            }
            Drawing::Box {
                center_world_m,
                lengths_m,
                label,
                color,
            } => batches.cuboid(*center_world_m, *lengths_m, label, *color),
        }
    }
    batches
}

impl Batches {
    fn line(
        &mut self,
        strip: Vec<[f32; 3]>,
        map_strip: Vec<[f32; 2]>,
        label: &str,
        color: [u8; 3],
    ) {
        self.lines.push(strip);
        self.line_colors.push(color);
        self.line_labels.push(label.to_string());
        self.map_lines.push(map_strip);
        self.map_line_colors.push(color);
    }

    /// A flat convex shape: its closed outline and a fan fill.
    fn area(&mut self, outline: Vec<[f32; 3]>, label: &str, color: [u8; 3]) {
        if outline.len() < 3 {
            return;
        }
        let mut closed = outline.clone();
        closed.push(outline[0]);
        let map_outline = closed.iter().map(|p| map::project(*p)).collect();
        self.line(closed, map_outline, label, color);
        let triangles = (1..outline.len() as u32 - 1)
            .map(|index| [0, index, index + 1])
            .collect();
        self.fills.push(Fill {
            vertices: outline,
            triangles,
            color,
        });
    }

    /// A box: its twelve edges, its four side walls as two triangles each,
    /// and its floor on the map. The floor and roof stay open so the box
    /// doesn't hide what flies inside it.
    fn cuboid(&mut self, center_world_m: Vec3, lengths_m: Vec3, label: &str, color: [u8; 3]) {
        let min_m = center_world_m - lengths_m * 0.5;
        let max_m = center_world_m + lengths_m * 0.5;
        // The box's floor, corner to corner and back to the first.
        let floor_m = [
            [min_m.x, min_m.y],
            [max_m.x, min_m.y],
            [max_m.x, max_m.y],
            [min_m.x, max_m.y],
            [min_m.x, min_m.y],
        ]
        .map(|[east_m, north_m]| [east_m as f32, north_m as f32, min_m.z as f32]);
        let roof_m = floor_m.map(|[east_m, north_m, _]| [east_m, north_m, max_m.z as f32]);
        self.line(
            floor_m.to_vec(),
            floor_m.map(map::project).to_vec(),
            label,
            color,
        );
        self.lines.push(roof_m.to_vec());
        self.line_colors.push(color);
        self.line_labels.push(label.to_string());
        for (a, b) in floor_m[..4].iter().zip(&roof_m) {
            self.lines.push(vec![*a, *b]);
            self.line_colors.push(color);
            self.line_labels.push(label.to_string());
        }
        let (mut walls, mut triangles) = (Vec::new(), Vec::new());
        for side in 0..4 {
            let first = walls.len() as u32;
            walls.extend([
                floor_m[side],
                floor_m[side + 1],
                roof_m[side + 1],
                roof_m[side],
            ]);
            triangles.push([first, first + 1, first + 2]);
            triangles.push([first, first + 2, first + 3]);
        }
        self.fills.push(Fill {
            vertices: walls,
            triangles,
            color,
        });
    }
}

fn point(position_world_m: Vec3) -> [f32; 3] {
    [
        position_world_m.x as f32,
        position_world_m.y as f32,
        position_world_m.z as f32,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_box_has_twelve_edges_four_walls_and_a_floor_on_the_map() {
        let batches = batches(&[Drawing::Box {
            center_world_m: Vec3::new(0.0, 0.0, 200.0),
            lengths_m: Vec3::new(600.0, 600.0, 400.0),
            label: "Boundary".into(),
            color: [170, 170, 120],
        }]);
        // The floor and roof strips carry four edges each, plus four verticals.
        assert_eq!(batches.lines.len(), 6);
        assert_eq!(batches.map_lines.len(), 1);
        assert_eq!(batches.map_lines[0][0], [-300.0, 300.0]);
        let walls = &batches.fills[0];
        assert_eq!((walls.vertices.len(), walls.triangles.len()), (16, 8));
    }

    #[test]
    fn a_circle_is_a_closed_outline_and_a_fan() {
        let batches = batches(&[Drawing::Circle {
            center_world_m: Vec3::new(1000.0, 2000.0, 0.0),
            radius_m: 500.0,
            label: String::new(),
            color: [255, 90, 77],
        }]);
        let outline = &batches.lines[0];
        assert_eq!(outline.len(), CIRCLE_SIDES + 1);
        assert_eq!(outline[0], [1500.0, 2000.0, 0.0]);
        assert_eq!(outline.first(), outline.last());
        assert_eq!(batches.fills[0].triangles.len(), CIRCLE_SIDES - 2);
    }
}

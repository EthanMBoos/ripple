//! Rerun observes the run; playback and debug panels never change simulation state.
use anyhow::{Context, Result, ensure};
use delivery::SoftAvoidance;
use rerun::{RecordingStream, RecordingStreamBuilder};
use ripple_core::{SimulationFrame, Terrain, Vec3, plugin::interaction::BoundaryRegion};
use serde::Deserialize;
use std::{collections::BTreeSet, path::Path, time::Duration};

mod blueprint;
mod map;
mod terrain;
mod trails;

use trails::Trails;

/// Read only the display fields; the Dispatcher owns mission validation.
#[derive(Default, Deserialize)]
#[serde(default)]
struct DeliveryOverlay {
    sites: Vec<Site>,
    soft_avoidance: Option<SoftAvoidance>,
}

#[derive(Deserialize)]
struct Site {
    name: String,
    position_m: [f32; 3],
}

pub(crate) struct Viewer {
    recording: RecordingStream,
    previous_ids: BTreeSet<i32>,
    /// The lowest and highest east, north, and up of any agent so far, for
    /// fitting the map.
    position_range_m: Option<[[f32; 3]; 2]>,
    first_time_s: Option<f64>,
    last_time_ns: Option<i64>,
    /// [west, east, south, north] of the drawn terrain.
    terrain_extent_m: Option<[f32; 4]>,
    /// With `--draw-trails`, a second stream into the same recording that goes
    /// only to the live viewer, so trails are shown but never saved.
    live_trails: Option<(RecordingStream, Trails)>,
}
impl Viewer {
    /// A path exports an RRD; with no path, the live viewer only streams.
    pub fn new(
        path: Option<&Path>,
        launch: bool,
        recording_id: &str,
        draw_trails: bool,
    ) -> Result<Self> {
        ensure!(!draw_trails || launch, "--draw-trails needs the viewer");
        let mut sinks: Vec<Box<dyn rerun::sink::LogSink>> = Vec::new();
        if let Some(path) = path {
            ensure!(
                !path.exists(),
                "Rerun output already exists: {}",
                path.display()
            );
            sinks.push(Box::new(rerun::sink::FileSink::new(path)?));
        }
        let mut live_trails = None;
        if launch {
            let viewer = rerun::spawn(&rerun::SpawnOptions {
                new: true,
                hide_welcome_screen: true,
                ..Default::default()
            })?;
            let uri: rerun::external::re_uri::ProxyUri =
                format!("rerun+http://127.0.0.1:{}/proxy", viewer.port).parse()?;
            sinks.push(Box::new(rerun::sink::GrpcSink::new(uri.clone())));
            if draw_trails {
                let stream = RecordingStreamBuilder::new("ripple")
                    .recording_id(recording_id)
                    .set_sinks(vec![
                        Box::new(rerun::sink::GrpcSink::new(uri)) as Box<dyn rerun::sink::LogSink>
                    ])?;
                live_trails = Some((stream, Trails::default()));
            }
        }
        let recording = RecordingStreamBuilder::new("ripple")
            .recording_id(recording_id)
            .enabled(true)
            .set_sinks(sinks)?;
        recording.log_static("world", &rerun::ViewCoordinates::RIGHT_HAND_Z_UP())?;
        blueprint::send(&recording)?;
        Ok(Self {
            recording,
            previous_ids: BTreeSet::new(),
            position_range_m: None,
            first_time_s: None,
            last_time_ns: None,
            terrain_extent_m: None,
            live_trails,
        })
    }

    /// Draw the mission's terrain once, before the first frame.
    pub fn log_terrain(&mut self, terrain: &Terrain) -> Result<()> {
        let [west_m, north_m, _] = terrain.sample(0, 0);
        let [east_m, south_m, _] = terrain.sample(terrain.rows() - 1, terrain.columns() - 1);
        self.terrain_extent_m = Some([west_m, east_m, south_m, north_m].map(|value| value as f32));
        terrain::log(&self.recording, terrain)
    }

    /// Draw each Boundary box once, in the 3D world and on the map. Rerun hides
    /// an entity per view, so the blueprint panel's eye on `world/boundaries`
    /// hides the 3D box only; `map/boundaries` has its own.
    pub fn log_boundaries(&mut self, boundaries: &[BoundaryRegion]) -> Result<()> {
        if boundaries.is_empty() {
            return Ok(());
        }
        let color = [170, 170, 120];
        let (mut edges, mut outlines) = (Vec::new(), Vec::new());
        let (mut walls, mut triangles) = (Vec::new(), Vec::new());
        for boundary in boundaries {
            let min_m = boundary.center_world_m - boundary.lengths_m * 0.5;
            let max_m = boundary.center_world_m + boundary.lengths_m * 0.5;
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
            edges.push(floor_m.to_vec());
            edges.push(roof_m.to_vec());
            edges.extend(floor_m[..4].iter().zip(&roof_m).map(|(a, b)| vec![*a, *b]));
            // The four side walls, two triangles each; the floor and roof
            // stay open so the box doesn't hide what flies inside it.
            for side in 0..4 {
                let first = u32::try_from(walls.len())?;
                walls.extend([
                    floor_m[side],
                    floor_m[side + 1],
                    roof_m[side + 1],
                    roof_m[side],
                ]);
                triangles.push([first, first + 1, first + 2]);
                triangles.push([first, first + 2, first + 3]);
            }
            outlines.push(floor_m.map(map::project).to_vec());
            // Fit the map and its grid around the box as well as the agents.
            for corner in floor_m {
                self.widen_range(corner);
            }
        }
        self.recording.log_static(
            "world/boundaries",
            &rerun::LineStrips3D::new(edges)
                .with_colors([color])
                .with_radii([rerun::Radius::new_ui_points(1.0)]),
        )?;
        self.recording.log_static(
            "world/boundaries/walls",
            &rerun::Mesh3D::new(walls)
                .with_triangle_indices(triangles)
                .with_albedo_factor(rerun::Rgba32::from_unmultiplied_rgba(
                    color[0], color[1], color[2], 40,
                )),
        )?;
        self.recording.log_static(
            "map/boundaries",
            &rerun::LineStrips2D::new(outlines)
                .with_colors([color])
                .with_radii([rerun::Radius::new_ui_points(1.0)]),
        )?;
        Ok(())
    }

    pub fn log_delivery(&mut self, scenario: &serde_json::Value) -> Result<()> {
        for (group_index, group) in scenario["entities"]
            .as_array()
            .context("scenario has no entity groups")?
            .iter()
            .enumerate()
        {
            for (plugin_index, plugin) in group["autonomy"]
                .as_array()
                .context("entity group has no autonomy list")?
                .iter()
                .enumerate()
            {
                if plugin["name"] != "Dispatcher" {
                    continue;
                }
                let overlay: DeliveryOverlay = serde_json::from_value(plugin["params"].clone())?;
                let path = format!("delivery/{group_index}/{plugin_index}");
                let depot = serde_json::from_value(group["position_m"].clone())?;
                self.log_landmark(
                    &format!("{path}/depot"),
                    depot,
                    "Depot",
                    [250, 195, 65],
                    8.0,
                )?;
                for (index, site) in overlay.sites.iter().enumerate() {
                    self.log_landmark(
                        &format!("{path}/sites/{index}"),
                        site.position_m,
                        &site.name,
                        [60, 220, 170],
                        8.0,
                    )?;
                }
                if let Some(field) = overlay.soft_avoidance {
                    field.validate()?;
                    self.log_severity(&path, &field)?;
                }
            }
        }
        Ok(())
    }

    fn log_landmark(
        &mut self,
        path: &str,
        position_m: [f32; 3],
        label: &str,
        color: [u8; 3],
        radius_ui: f32,
    ) -> Result<()> {
        self.widen_range(position_m);
        self.recording.log_static(
            format!("world/{path}"),
            &rerun::Points3D::new([position_m])
                .with_colors([color])
                .with_radii([rerun::Radius::new_ui_points(radius_ui)])
                .with_labels([label])
                .with_show_labels(true),
        )?;
        self.recording.log_static(
            format!("map/{path}"),
            &rerun::Points2D::new([map::project(position_m)])
                .with_colors([color])
                .with_radii([rerun::Radius::new_ui_points(radius_ui)])
                .with_labels([label])
                .with_show_labels(true),
        )?;
        Ok(())
    }

    fn log_severity(&mut self, path: &str, field: &SoftAvoidance) -> Result<()> {
        // One sampled texture for both views, using the scorer's exact equation.
        // Colors are relative to this field's peak (reported in the label).
        const SIDE: u32 = 256;
        let [east_m, north_m] = field.center_m;
        let radius_m = field.radius_m;
        let pixel_m = 2.0 * radius_m / f64::from(SIDE);
        let mut pixels = Vec::with_capacity((SIDE * SIDE * 4) as usize);
        for row in 0..SIDE {
            for column in 0..SIDE {
                let point = Vec3::new(
                    east_m - radius_m + (f64::from(column) + 0.5) * pixel_m,
                    north_m + radius_m - (f64::from(row) + 0.5) * pixel_m,
                    0.0,
                );
                let severity = field.severity(point);
                let fraction = if field.peak_severity > 0.0 {
                    severity / field.peak_severity
                } else {
                    0.0
                };
                let alpha = if severity > 0.0 {
                    45.0 + 170.0 * fraction
                } else {
                    0.0
                };
                pixels.extend([255, (200.0 - 150.0 * fraction) as u8, 40, alpha as u8]);
            }
        }
        let west = (east_m - radius_m) as f32;
        let east = (east_m + radius_m) as f32;
        let south = (north_m - radius_m) as f32;
        let north = (north_m + radius_m) as f32;
        self.widen_range([west, south, 0.0]);
        self.widen_range([east, north, 0.0]);
        self.recording.log_static(
            format!("map/{path}/severity"),
            &rerun::Transform3D::from_translation_scale(
                [west, -north, 0.0],
                rerun::Scale3D::uniform(pixel_m as f32),
            ),
        )?;
        self.recording.log_static(
            format!("map/{path}/severity"),
            // Explicit opacity selects Rerun's blended image pass; relying on
            // texture alpha alone can leave an opaque black rectangle behind it.
            &rerun::Image::from_rgba32(pixels.clone(), [SIDE, SIDE])
                .with_opacity(0.85)
                .with_draw_order(-5.0),
        )?;
        // The field is horizontal and independent of altitude. Draw its footprint
        // just below z=0 so it does not obscure planar couriers and their trails.
        // Clip the geometry to a disk: Rerun's mesh renderer ignores texture alpha.
        let circle: Vec<_> = (0..=128)
            .map(|index| {
                let angle = std::f64::consts::TAU * f64::from(index) / 128.0;
                [angle.cos(), angle.sin()]
            })
            .collect();
        let boundary: Vec<_> = circle
            .iter()
            .map(|[x, y]| {
                [
                    (east_m + radius_m * x) as f32,
                    (north_m + radius_m * y) as f32,
                    0.0,
                ]
            })
            .collect();
        if field.peak_severity > 0.0 {
            let vertices = std::iter::once([east_m as f32, north_m as f32, -0.5])
                .chain(boundary[..128].iter().map(|p| [p[0], p[1], -0.5]));
            let texcoords = std::iter::once([0.5, 0.5]).chain(
                circle[..128]
                    .iter()
                    .map(|[x, y]| [(0.5 + 0.5 * x) as f32, (0.5 - 0.5 * y) as f32]),
            );
            self.recording.log_static(
                format!("world/{path}/severity"),
                &rerun::Mesh3D::new(vertices)
                    .with_triangle_indices((0..128).map(|i| [0, i + 1, (i + 1) % 128 + 1]))
                    .with_vertex_texcoords(texcoords)
                    .with_albedo_texture(
                        rerun::datatypes::ImageFormat::rgba8([SIDE, SIDE]),
                        pixels,
                    ),
            )?;
        }
        self.recording.log_static(
            format!("world/{path}/cutoff"),
            &rerun::LineStrips3D::new([boundary.clone()])
                .with_colors([[255, 160, 50]])
                .with_radii([rerun::Radius::new_ui_points(1.5)]),
        )?;
        self.recording.log_static(
            format!("map/{path}/cutoff"),
            &rerun::LineStrips2D::new([boundary.into_iter().map(map::project)])
                .with_colors([[255, 160, 50]])
                .with_radii([rerun::Radius::new_ui_points(1.5)]),
        )?;
        self.log_landmark(
            &format!("{path}/field_label"),
            [east_m as f32, (north_m + radius_m * 1.4) as f32, 0.0],
            &format!(
                "Soft danger: peak {}; cutoff {radius_m} m",
                field.peak_severity
            ),
            [255, 160, 50],
            0.0,
        )
    }

    pub fn log_frame(&mut self, index: usize, frame: &SimulationFrame) -> Result<()> {
        let first_time_s = *self.first_time_s.get_or_insert(frame.time_s);
        let time_ns = playback_time_ns(frame.time_s - first_time_s, self.last_time_ns);
        self.last_time_ns = Some(time_ns);
        self.recording
            .set_time_sequence("frame", i64::try_from(index)?);
        self.recording
            .set_duration_secs("time", time_ns as f64 * 1e-9);
        if let Some((stream, trails)) = &mut self.live_trails {
            stream.set_time_sequence("frame", i64::try_from(index)?);
            stream.set_duration_secs("time", time_ns as f64 * 1e-9);
            trails.log_frame(stream, frame)?;
        }

        let current_ids: BTreeSet<_> = frame.entities.iter().map(|entity| entity.id).collect();
        for id in self.previous_ids.difference(&current_ids) {
            self.recording
                .log(format!("world/entities/{id}"), &rerun::Clear::recursive())?;
            self.recording
                .log(format!("map/entities/{id}"), &rerun::Clear::recursive())?;
        }
        for entity in &frame.entities {
            let path = format!("world/entities/{}", entity.id);
            let state = &entity.truth;
            let position = state.position_world_m;
            let orientation = state.orientation_world_from_body;
            let color = team_color(entity.team_id);
            let label = format!("{} · team {}", entity.id, entity.team_id);
            let point = [position.x as f32, position.y as f32, position.z as f32];
            self.widen_range(point);

            self.recording.log(
                path.as_str(),
                &rerun::Transform3D::from_translation_rotation(
                    point,
                    rerun::Quaternion::from_wxyz([
                        orientation.w as f32,
                        orientation.x as f32,
                        orientation.y as f32,
                        orientation.z as f32,
                    ]),
                ),
            )?;
            self.recording.log(
                path.as_str(),
                &rerun::Points3D::new([[0.0, 0.0, 0.0]])
                    .with_colors([color])
                    .with_radii([rerun::Radius::new_ui_points(6.0)])
                    .with_labels([entity.id.to_string()])
                    .with_show_labels(false),
            )?;
            self.recording.log(
                format!("{path}/heading"),
                &rerun::Arrows3D::from_vectors([[8.0, 0.0, 0.0]]).with_colors([color]),
            )?;

            let map_path = format!("map/entities/{}", entity.id);
            let position_map_m = map::project(point);
            let heading_rad = orientation.yaw_world_from_body_rad();
            self.recording.log(
                map_path.as_str(),
                &rerun::Points2D::new([position_map_m])
                    .with_colors([color])
                    .with_radii([rerun::Radius::new_ui_points(6.0)])
                    .with_labels([entity.id.to_string()])
                    .with_show_labels(false),
            )?;
            self.recording.log(
                format!("{map_path}/heading"),
                &rerun::Arrows2D::from_vectors([[
                    8.0 * heading_rad.cos() as f32,
                    -8.0 * heading_rad.sin() as f32,
                ]])
                .with_origins([position_map_m])
                .with_colors([color]),
            )?;
            for (quantity, value) in [
                ("speed_mps", state.velocity_world_mps.norm()),
                ("altitude_m", position.z),
            ] {
                let plot_path = format!("debug/{quantity}/{}", entity.id);
                if !self.previous_ids.contains(&entity.id) {
                    self.recording.log_static(
                        plot_path.as_str(),
                        &rerun::SeriesLines::new()
                            .with_colors([color])
                            .with_names([label.clone()]),
                    )?;
                }
                self.recording
                    .log(plot_path.as_str(), &rerun::Scalars::new([value]))?;
            }
        }
        self.recording.log(
            "dashboard/status",
            &rerun::TextDocument::from_markdown(status_text(index, frame, time_ns)),
        )?;
        self.previous_ids = current_ids;
        Ok(())
    }

    /// Grow the range the map is fitted to so it includes `point`.
    fn widen_range(&mut self, point: [f32; 3]) {
        let [low, high] = self.position_range_m.get_or_insert([point, point]);
        for axis in 0..3 {
            low[axis] = low[axis].min(point[axis]);
            high[axis] = high[axis].max(point[axis]);
        }
    }

    pub fn finish(self) -> Result<()> {
        map::log_grid(
            &self.recording,
            self.position_range_m.into_iter().flatten(),
            self.terrain_extent_m,
        )?;
        // Activate a fresh layout after all data arrives so initial framing
        // includes the grid, which covers every agent's whole run.
        blueprint::send(&self.recording)?;
        if let Some((stream, _)) = &self.live_trails {
            stream.flush_with_timeout(Duration::from_secs(10))?;
        }
        self.recording.flush_with_timeout(Duration::from_secs(10))?;
        Ok(())
    }

    pub fn log_saved_trails(&self, frames: &[SimulationFrame]) -> Result<()> {
        trails::draw(&self.recording, frames)
    }
}

/// The color for a team's entities and trails: blue for teams 1, 4, 7, ...,
/// red for 2, 5, 8, ..., and green for 0, 3, 6, .... With only three colors,
/// four or more teams share them (teams 0 and 3 are both green). Missions do
/// not set colors: color is a display choice, so it stays out of the scenario
/// a run records. To tell more teams apart, replace the match with a longer
/// palette indexed by `team_id.rem_euclid(palette.len())`, its neighbours far
/// apart in hue (a colorblind-safe set of eight, such as Okabe-Ito). To pick a
/// team's color by hand, add a viewer option, not a mission key.
pub(super) fn team_color(team_id: i32) -> [u8; 3] {
    match team_id.rem_euclid(3) {
        1 => [77, 140, 255],
        2 => [255, 90, 77],
        _ => [90, 220, 130],
    }
}

pub(super) fn playback_time_ns(elapsed_s: f64, previous_ns: Option<i64>) -> i64 {
    let time_ns = (elapsed_s.max(0.0) * 1e9).round() as i64;
    // Keep samples strictly increasing even if a frames file repeats a timestamp
    // (runs recorded before timestamps were unique repeat the final one).
    previous_ns.map_or(time_ns, |previous| time_ns.max(previous.saturating_add(1)))
}

fn status_text(index: usize, frame: &SimulationFrame, replay_time_ns: i64) -> String {
    let teams: BTreeSet<_> = frame.entities.iter().map(|entity| entity.team_id).collect();
    let mut text = format!(
        "# Mission status\n\nSimulation **{:.2} s** · frame **{index}**\n\n**{} agents** · **{} teams**\n\n",
        replay_time_ns as f64 * 1e-9,
        frame.entities.len(),
        teams.len()
    );
    text.push_str("Local ENU map: East → / North ↑, meters.\n\nMarkers = current agents; arrows = heading.\n\n");
    text.push_str("| ID | Team | Speed m/s | Alt. m |\n|---:|---:|---:|---:|\n");
    for entity in frame.entities.iter().take(12) {
        text.push_str(&format!(
            "| {} | {} | {:.1} | {:.1} |\n",
            entity.id,
            entity.team_id,
            entity.truth.velocity_world_mps.norm(),
            entity.truth.position_world_m.z
        ));
    }
    if frame.entities.len() > 12 {
        text.push_str(&format!(
            "\n{} additional agents shown on the map.\n",
            frame.entities.len() - 12
        ));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn playback_uses_seconds_and_retains_the_repeated_terminal_sample() {
        assert_eq!(playback_time_ns(0.1, Some(0)), 100_000_000);
        assert_eq!(playback_time_ns(0.1, Some(100_000_000)), 100_000_001);
        assert_eq!(playback_time_ns(0.2, Some(100_000_001)), 200_000_000);
    }
}

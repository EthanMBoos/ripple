//! Rerun observes the run; playback and debug panels never change simulation state.
use crate::demo::{Camera, Demo, Preset};
use anyhow::{Result, ensure};
use rerun::{RecordingStream, RecordingStreamBuilder};
use ripple_core::{
    SimulationFrame, Terrain, Vec3,
    record::{Belief, Fix},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    time::Duration,
};

mod blueprint;
pub(crate) mod covariance;
mod drawings;
mod map;
mod models;
mod preset;
mod terrain;
mod trails;

use covariance::{Covariances, Uncertain};
use drawings::Drawn;
use trails::Trails;

/// A mission group as the viewer needs it: the label its entities are named
/// by and the model they are drawn with.
pub(crate) struct Group {
    pub label: String,
    pub visual_model: String,
    /// How many entities the mission puts in the group; a group of one is
    /// named by its label alone, and a recipe group, which grows as plugins
    /// spawn into it, counts as many.
    pub count: usize,
}

pub(crate) struct ViewerSettings<'a> {
    /// A path exports an RRD; with no path, the live viewer only streams.
    pub export: Option<&'a Path>,
    /// Launch the live viewer.
    pub launch: bool,
    pub recording_id: &'a str,
    pub draw_trails: bool,
    pub draw_covariance: bool,
    /// Entity groups then recipe groups, indexed by an entity's group index.
    pub groups: Vec<Group>,
    /// Present the run as this demo: its preset layout, camera, captions,
    /// interval, and frame stride instead of the debugging layout.
    pub demo: Option<Demo>,
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
    /// [west, east, south, north] of the map's grid, once it is drawn.
    map_extent_m: Option<[f32; 4]>,
    /// The terrain's heights, for plotting height above ground.
    ground: Option<terrain::Ground>,
    /// The plugins' drawings on screen; see `drawings.rs`.
    drawn: Drawn,
    /// With `--draw-trails`, a second stream into the same recording that goes
    /// only to the live viewer, so trails are shown but never saved.
    live_trails: Option<(RecordingStream, Trails)>,
    /// With `--draw-covariance`, the one-sigma ellipsoids on screen.
    covariances: Option<Covariances>,
    groups: Vec<Group>,
    /// Each entity's member number within its group, in the order seen.
    members: BTreeMap<i32, usize>,
    group_counts: BTreeMap<usize, usize>,
    /// The span each entity's model was last drawn at; `None` for an entity
    /// drawn as a marker only.
    model_span_m: BTreeMap<i32, Option<f32>>,
    /// Whether the last frame offered was drawn: a demo skips frames
    /// outside its interval and off its stride.
    drew_last_frame: bool,
    /// The simulated time of the last frame drawn.
    last_frame_time_s: f64,
    /// Each entity's display name, its group label and member number.
    names: BTreeMap<i32, String>,
    presentation: Option<Presentation>,
}

/// The state of a demo presentation while the run is drawn.
struct Presentation {
    demo: Demo,
    /// Frames inside the interval so far, kept or not; the stride counts
    /// from the first.
    seen_in_interval: usize,
    /// Captions shown so far.
    shown_captions: usize,
    featured: BTreeSet<i32>,
    /// The entity the camera follows, once it has appeared.
    followed: Option<preset::Followed>,
    trails: Trails,
    /// Each entity's last drawn truth, for the position-error plot.
    truth_m: BTreeMap<i32, Vec3>,
    /// Entities whose error series has been named.
    error_series: BTreeSet<i32>,
    /// The fix markers on screen, by path, with the time each was drawn.
    fixes_shown: BTreeMap<String, f64>,
    /// Where the chase camera is, once it has a target.
    chase_m: Option<[f32; 3]>,
    /// The demo's highlights on screen, by index.
    highlights_shown: BTreeSet<usize>,
}

/// How long a fix marker stays on screen, and its color.
const FIX_MARKER_S: f64 = 0.6;
const FIX_COLOR: [u8; 3] = [255, 214, 64];
/// The fleet preset's batched couriers, in UI points.
const FLEET_POINT_UI: f32 = 4.5;
/// A highlight ring's color and sides.
const HIGHLIGHT_COLOR: [u8; 3] = [255, 90, 77];
const HIGHLIGHT_SIDES: usize = 48;
/// The chase camera eases to its place behind the entity with this time
/// constant, so the view glides while the entity's heading jitters.
const CHASE_EASE_S: f32 = 0.6;

impl Viewer {
    pub fn new(settings: ViewerSettings) -> Result<Self> {
        let ViewerSettings {
            export,
            launch,
            recording_id,
            draw_trails,
            draw_covariance,
            groups,
            demo,
        } = settings;
        ensure!(!draw_trails || launch, "--draw-trails needs the viewer");
        let mut sinks: Vec<Box<dyn rerun::sink::LogSink>> = Vec::new();
        if let Some(path) = export {
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
        let presentation = demo.map(|demo| Presentation {
            demo,
            seen_in_interval: 0,
            shown_captions: 0,
            featured: BTreeSet::new(),
            followed: None,
            trails: Trails::default(),
            truth_m: BTreeMap::new(),
            error_series: BTreeSet::new(),
            fixes_shown: BTreeMap::new(),
            chase_m: None,
            highlights_shown: BTreeSet::new(),
        });
        let viewer = Self {
            recording,
            previous_ids: BTreeSet::new(),
            position_range_m: None,
            first_time_s: None,
            last_time_ns: None,
            terrain_extent_m: None,
            map_extent_m: None,
            ground: None,
            drawn: Drawn::default(),
            live_trails,
            covariances: draw_covariance.then(Covariances::default),
            groups,
            members: BTreeMap::new(),
            group_counts: BTreeMap::new(),
            model_span_m: BTreeMap::new(),
            drew_last_frame: false,
            last_frame_time_s: 0.0,
            names: BTreeMap::new(),
            presentation,
        };
        // A demo's layout waits for the end, when the followed entity is
        // known: a recording holds one layout, and the viewer keeps the
        // first it is given.
        if viewer.presentation.is_none() {
            viewer.send_layout()?;
        }
        Ok(viewer)
    }

    /// The debugging layout, or the demo's preset.
    fn send_layout(&self) -> Result<()> {
        match &self.presentation {
            Some(presentation) => preset::send(
                &self.recording,
                &preset::Layout {
                    preset: presentation.demo.preset,
                    camera: presentation.demo.camera.clone(),
                    followed: presentation
                        .followed
                        .as_ref()
                        .map(|followed| preset::Followed {
                            id: followed.id,
                            distance_m: followed.distance_m,
                        }),
                    speed: presentation.demo.speed,
                    has_terrain: self.terrain_extent_m.is_some(),
                    map_extent_m: self.map_extent_m,
                },
            ),
            None => blueprint::send(&self.recording),
        }
    }

    /// Draw the mission's terrain once, before the first frame.
    pub fn log_terrain(&mut self, terrain: &Terrain) -> Result<()> {
        let (samples, window, runway) = match &self.presentation {
            Some(presentation) => (
                presentation.demo.terrain_samples,
                presentation.demo.terrain_window,
                presentation.demo.runway,
            ),
            None => (terrain::MAX_MESH_SAMPLES, None, None),
        };
        // The map covers what is drawn: the whole terrain, or the demo's
        // window of it.
        let drawn_m = terrain::log(&self.recording, terrain, samples, window)?;
        self.terrain_extent_m = Some(drawn_m.map(|value| value as f32));
        self.ground = Some(terrain::Ground::from(terrain));
        if let Some(runway) = runway {
            terrain::log_runway(&self.recording, terrain, &runway)?;
        }
        Ok(())
    }

    pub fn log_frame(&mut self, index: usize, frame: &SimulationFrame) -> Result<()> {
        self.drew_last_frame = false;
        if let Some(presentation) = &mut self.presentation {
            if !presentation.demo.covers(frame.time_s) {
                return Ok(());
            }
            let kept = presentation.seen_in_interval % presentation.demo.every == 0;
            presentation.seen_in_interval += 1;
            if !kept {
                return Ok(());
            }
        }
        self.drew_last_frame = true;
        let since_last_frame_s = frame.time_s - self.last_frame_time_s;
        self.last_frame_time_s = frame.time_s;
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

        self.drawn
            .log_frame(&self.recording, &frame.drawings, &mut |point| {
                widen(&mut self.position_range_m, point);
            })?;

        let current_ids: BTreeSet<_> = frame.entities.iter().map(|entity| entity.id).collect();
        for id in self.previous_ids.difference(&current_ids) {
            self.recording
                .log(format!("world/entities/{id}"), &rerun::Clear::recursive())?;
            self.recording
                .log(format!("map/entities/{id}"), &rerun::Clear::recursive())?;
            self.model_span_m.remove(id);
        }
        // The fleet preset draws its unfeatured entities as one batch of
        // points per frame: a day of a hundred couriers logged one by one,
        // with a transform, a marker, an arrow, and two scalars each, is
        // hundreds of megabytes; the batch is a few kilobytes a frame.
        let batched = self
            .presentation
            .as_ref()
            .is_some_and(|presentation| presentation.demo.preset == Preset::Fleet);
        let mut batch_points: Vec<[f32; 3]> = Vec::new();
        let mut batch_colors: Vec<[u8; 3]> = Vec::new();
        for entity in &frame.entities {
            let path = format!("world/entities/{}", entity.id);
            let state = &entity.truth;
            let position = state.position_world_m;
            let orientation = state.orientation_world_from_body;
            let color = team_color(entity.team_id);
            let label = format!("{} · team {}", entity.id, entity.team_id);
            let point = [position.x as f32, position.y as f32, position.z as f32];
            self.widen_range(point);
            // A demo's arrows are the size of its models; the debugging
            // layout's grow with the scene.
            let arrow_m = self
                .presentation
                .as_ref()
                .map_or(heading_arrow_m(self.position_range_m), |presentation| {
                    presentation.demo.model_span_m as f32
                });

            // The entity's group and member number, for its model, its
            // name, and the demo's featured entities.
            let group_index = usize::try_from(entity.group_index).unwrap_or(usize::MAX);
            let member = *self.members.entry(entity.id).or_insert_with(|| {
                let count = self.group_counts.entry(group_index).or_default();
                let member = *count;
                *count += 1;
                member
            });
            let group = self.groups.get(group_index);
            let group_label = group.map_or_else(
                || format!("group {group_index}"),
                |group| group.label.clone(),
            );
            let name = if group.is_some_and(|group| group.count == 1) {
                group_label.clone()
            } else {
                format!("{group_label} {member}")
            };
            self.names.entry(entity.id).or_insert_with(|| name.clone());
            let featured = match &mut self.presentation {
                Some(presentation) => {
                    let demo = &presentation.demo;
                    if demo
                        .featured
                        .iter()
                        .any(|selector| selector.matches(&group_label, member))
                    {
                        presentation.featured.insert(entity.id);
                    }
                    if presentation.followed.is_none()
                        && let Camera::Follow {
                            group,
                            index,
                            distance_m,
                        } = &demo.camera
                        && group == &group_label
                        && index.is_none_or(|index| index == member)
                    {
                        presentation.followed = Some(preset::Followed {
                            id: entity.id,
                            distance_m: *distance_m as f32,
                        });
                        log_chase_lens(&self.recording)?;
                    }
                    if let Some(followed) = &presentation.followed
                        && followed.id == entity.id
                    {
                        // The camera trails the direction of travel, or the
                        // heading while the entity stands still.
                        let velocity = state.velocity_world_mps;
                        let heading_rad = if velocity.xy().norm() > 1.0 {
                            velocity.y.atan2(velocity.x)
                        } else {
                            orientation.yaw_world_from_body_rad()
                        };
                        let chase = chase_position(
                            presentation.chase_m,
                            point,
                            heading_rad as f32,
                            followed.distance_m,
                            since_last_frame_s as f32,
                        );
                        presentation.chase_m = Some(chase);
                        log_chase_pose(&self.recording, chase, point)?;
                    }
                    presentation.truth_m.insert(entity.id, position);
                    presentation.featured.contains(&entity.id)
                }
                None => false,
            };
            if batched && !featured {
                batch_points.push(point);
                batch_colors.push(color);
                continue;
            }

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

            // The model, drawn once and scaled to the demo's span or the
            // heading arrow's length, which grows with the scene.
            let model_path = format!("{path}/model");
            if !self.model_span_m.contains_key(&entity.id) {
                let visual_model = group.map_or("aircraft", |group| group.visual_model.as_str());
                let drawn = match models::mesh(visual_model, color) {
                    Some(mesh) => {
                        self.recording.log(model_path.as_str(), &mesh)?;
                        Some(f32::NAN)
                    }
                    None => None,
                };
                self.model_span_m.insert(entity.id, drawn);
            }
            let has_model = self.model_span_m[&entity.id].is_some();
            let span_m = self.presentation.as_ref().map_or(arrow_m, |presentation| {
                presentation.demo.model_span_m as f32
            });
            if let Some(Some(logged_m)) = self.model_span_m.get_mut(&entity.id)
                && *logged_m != span_m
            {
                self.recording
                    .log(model_path.as_str(), &rerun::Transform3D::from_scale(span_m))?;
                *logged_m = span_m;
            }

            // A marker at the model's centre: always in the debugging
            // layout, where it stays visible at any zoom; in a demo only to
            // name a featured entity.
            let marker = match (&self.presentation, has_model) {
                (Some(_), true) => featured.then_some(1.0),
                (Some(_), false) => Some(8.0),
                (None, _) => Some(6.0),
            };
            if let Some(radius) = marker {
                self.recording.log(
                    path.as_str(),
                    &rerun::Points3D::new([[0.0, 0.0, 0.0]])
                        .with_colors([color])
                        .with_radii([rerun::Radius::new_ui_points(radius)])
                        .with_labels([name.clone()])
                        // A demo names its entities on the map, where the
                        // label covers nothing.
                        .with_show_labels(featured && self.presentation.is_none()),
                )?;
            }
            // The heading arrow stays in the debugging layout, where the
            // model is too small to read from afar; a demo's model shows it.
            if self.presentation.is_none() || !has_model {
                self.recording.log(
                    format!("{path}/heading"),
                    &rerun::Arrows3D::from_vectors([[arrow_m, 0.0, 0.0]]).with_colors([color]),
                )?;
            }

            let map_path = format!("map/entities/{}", entity.id);
            let position_map_m = map::project(point);
            let heading_rad = orientation.yaw_world_from_body_rad();
            self.recording.log(
                map_path.as_str(),
                &rerun::Points2D::new([position_map_m])
                    .with_colors([color])
                    .with_radii([rerun::Radius::new_ui_points(6.0)])
                    .with_labels([name.clone()])
                    .with_show_labels(
                        featured
                            && self
                                .presentation
                                .as_ref()
                                .is_none_or(|presentation| presentation.demo.labels),
                    ),
            )?;
            self.recording.log(
                format!("{map_path}/heading"),
                &rerun::Arrows2D::from_vectors([[
                    arrow_m * heading_rad.cos() as f32,
                    -arrow_m * heading_rad.sin() as f32,
                ]])
                .with_origins([position_map_m])
                .with_colors([color]),
            )?;
            let mut quantities = vec![
                ("speed_mps", state.velocity_world_mps.norm()),
                ("altitude_m", position.z),
            ];
            if let Some(ground_m) = self
                .ground
                .as_ref()
                .and_then(|ground| ground.height_at(position.x, position.y))
            {
                quantities.push(("height_above_ground_m", position.z - ground_m));
            }
            for (quantity, value) in quantities {
                let plot_path = format!("debug/{quantity}/{}", entity.id);
                if !self.previous_ids.contains(&entity.id) {
                    let series_name = if self.presentation.is_some() {
                        name.clone()
                    } else {
                        label.clone()
                    };
                    self.recording.log_static(
                        plot_path.as_str(),
                        &rerun::SeriesLines::new()
                            .with_colors([color])
                            .with_names([series_name]),
                    )?;
                }
                self.recording
                    .log(plot_path.as_str(), &rerun::Scalars::new([value]))?;
            }
        }
        if batched {
            self.recording.log(
                "world/fleet",
                &rerun::Points3D::new(batch_points.iter().copied())
                    .with_colors(batch_colors.iter().copied())
                    .with_radii([rerun::Radius::new_ui_points(FLEET_POINT_UI)]),
            )?;
            self.recording.log(
                "map/fleet",
                &rerun::Points2D::new(batch_points.iter().map(|point| map::project(*point)))
                    .with_colors(batch_colors)
                    .with_radii([rerun::Radius::new_ui_points(FLEET_POINT_UI)]),
            )?;
        }
        self.log_highlights(frame.time_s)?;
        match &mut self.presentation {
            Some(presentation) => {
                // Fix markers that have had their moment go.
                let expired: Vec<String> = presentation
                    .fixes_shown
                    .iter()
                    .filter(|(_, shown_s)| frame.time_s - *shown_s >= FIX_MARKER_S)
                    .map(|(path, _)| path.clone())
                    .collect();
                for path in expired {
                    self.recording
                        .log(format!("world/{path}"), &rerun::Clear::flat())?;
                    self.recording
                        .log(format!("map/{path}"), &rerun::Clear::flat())?;
                    presentation.fixes_shown.remove(&path);
                }
                // Trails for the featured entities only, at the kept frames.
                let featured = SimulationFrame {
                    time_s: frame.time_s,
                    entities: frame
                        .entities
                        .iter()
                        .filter(|entity| presentation.featured.contains(&entity.id))
                        .copied()
                        .collect(),
                    drawings: Vec::new(),
                };
                presentation.trails.log_frame(&self.recording, &featured)?;
                // The latest caption whose time has come.
                let captions = &presentation.demo.captions;
                let due = captions
                    .iter()
                    .take_while(|caption| caption.at_s <= frame.time_s)
                    .count();
                if due > presentation.shown_captions {
                    self.recording.log(
                        "captions",
                        &rerun::TextDocument::from_markdown(captions[due - 1].text.as_str()),
                    )?;
                    presentation.shown_captions = due;
                }
            }
            None => {
                self.recording.log(
                    "dashboard/status",
                    &rerun::TextDocument::from_markdown(status_text(index, frame, time_ns)),
                )?;
            }
        }
        self.previous_ids = current_ids;
        Ok(())
    }

    /// Ring the marks the demo's highlights name while each is due, and
    /// clear a ring whose time has passed.
    fn log_highlights(&mut self, time_s: f64) -> Result<()> {
        let Some(presentation) = &mut self.presentation else {
            return Ok(());
        };
        for (index, highlight) in presentation.demo.highlights.iter().enumerate() {
            let due = time_s >= highlight.from_s && time_s <= highlight.to_s;
            let shown = presentation.highlights_shown.contains(&index);
            let map_path = format!("map/highlights/{index}");
            let world_path = format!("world/highlights/{index}");
            if due && !shown {
                let Some(center) = self.drawn.named_point(&highlight.label) else {
                    continue;
                };
                let radius_m = highlight.radius_m as f32;
                let ring: Vec<[f32; 3]> = (0..=HIGHLIGHT_SIDES)
                    .map(|side| {
                        let angle = side as f32 / HIGHLIGHT_SIDES as f32 * std::f32::consts::TAU;
                        [
                            center[0] + radius_m * angle.cos(),
                            center[1] + radius_m * angle.sin(),
                            center[2],
                        ]
                    })
                    .collect();
                self.recording.log(
                    map_path.as_str(),
                    &rerun::LineStrips2D::new([ring.iter().map(|point| map::project(*point))])
                        .with_colors([HIGHLIGHT_COLOR])
                        .with_radii([rerun::Radius::new_ui_points(2.0)]),
                )?;
                self.recording.log(
                    world_path.as_str(),
                    &rerun::LineStrips3D::new([ring])
                        .with_colors([HIGHLIGHT_COLOR])
                        .with_radii([rerun::Radius::new_ui_points(2.0)]),
                )?;
                presentation.highlights_shown.insert(index);
            } else if !due && shown {
                self.recording
                    .log(map_path.as_str(), &rerun::Clear::flat())?;
                self.recording
                    .log(world_path.as_str(), &rerun::Clear::flat())?;
                presentation.highlights_shown.remove(&index);
            }
        }
        Ok(())
    }

    /// Grow the range the map is fitted to so it includes `point`.
    fn widen_range(&mut self, point: [f32; 3]) {
        widen(&mut self.position_range_m, point);
    }

    pub fn finish(mut self) -> Result<()> {
        self.map_extent_m = Some(map::log_grid(
            &self.recording,
            self.position_range_m.into_iter().flatten(),
            self.terrain_extent_m,
        )?);
        // Activate a fresh layout after all data arrives so initial framing
        // includes the grid, which covers every agent's whole run, and a
        // demo's camera knows the entity it follows.
        self.send_layout()?;
        if let Some((stream, _)) = &self.live_trails {
            stream.flush_with_timeout(Duration::from_secs(10))?;
        }
        self.recording.flush_with_timeout(Duration::from_secs(10))?;
        Ok(())
    }

    pub fn log_saved_trails(&self, frames: &[SimulationFrame]) -> Result<()> {
        trails::draw(&self.recording, frames)
    }

    /// Draw these beliefs' one-sigma ellipsoids at the last logged frame's
    /// time. Does nothing unless the viewer was made with `draw_covariance`.
    pub fn log_beliefs(&mut self, items: &[Uncertain]) -> Result<()> {
        if let Some(covariances) = &mut self.covariances {
            covariances.log(&self.recording, items)?;
        }
        Ok(())
    }

    /// Plot each entity's distance between its believed and true position
    /// at the last logged frame, for the belief preset's error plot. Does
    /// nothing outside a demo, or for a frame the demo skipped.
    pub fn log_belief_errors(&mut self, beliefs: &[Belief]) -> Result<()> {
        let Some(presentation) = &mut self.presentation else {
            return Ok(());
        };
        if presentation.demo.preset != Preset::Belief || !self.drew_last_frame {
            return Ok(());
        }
        for belief in beliefs {
            let Some(truth_m) = presentation.truth_m.get(&belief.entity_id) else {
                continue;
            };
            let error_m = (belief.state.mean.position_world_m - truth_m).norm();
            let plot_path = format!("debug/error_m/{}", belief.entity_id);
            if presentation.error_series.insert(belief.entity_id) {
                self.recording.log_static(
                    plot_path.as_str(),
                    &rerun::SeriesLines::new()
                        .with_colors([team_color(belief.team_id)])
                        .with_names([self
                            .names
                            .get(&belief.entity_id)
                            .cloned()
                            .unwrap_or_else(|| belief.entity_id.to_string())]),
                )?;
            }
            self.recording
                .log(plot_path.as_str(), &rerun::Scalars::new([error_m]))?;
        }
        Ok(())
    }

    /// Mark where the sensors measured positions at the last logged frame,
    /// for the belief preset: a marker that stays a moment at each fix.
    pub fn log_fixes(&mut self, fixes: &[Fix]) -> Result<()> {
        let Some(presentation) = &mut self.presentation else {
            return Ok(());
        };
        if presentation.demo.preset != Preset::Belief || !self.drew_last_frame {
            return Ok(());
        }
        for fix in fixes {
            let path = format!("fixes/{}/{}", fix.entity_id, fix.measured_id);
            let position = fix.position_world_m;
            let point = [position.x as f32, position.y as f32, position.z as f32];
            self.recording.log(
                format!("world/{path}"),
                &rerun::Points3D::new([point])
                    .with_colors([FIX_COLOR])
                    .with_radii([rerun::Radius::new_ui_points(8.0)]),
            )?;
            self.recording.log(
                format!("map/{path}"),
                &rerun::Points2D::new([map::project(point)])
                    .with_colors([FIX_COLOR])
                    .with_radii([rerun::Radius::new_ui_points(8.0)]),
            )?;
            presentation
                .fixes_shown
                .insert(path, self.last_frame_time_s);
        }
        Ok(())
    }

    /// Whether the last frame offered was drawn: a demo skips frames outside
    /// its interval and off its stride, and their beliefs go undrawn too.
    pub fn drew_last_frame(&self) -> bool {
        self.drew_last_frame
    }
}

/// The chase camera's lens, logged once: a pinhole the demo layout's eye
/// tracks (`preset::CHASE_PATH`). Its image plane sits inside the near clip
/// plane, so the frustum is never drawn.
fn log_chase_lens(recording: &RecordingStream) -> Result<()> {
    // The viewer's eye eases toward the tracked camera and sits a little
    // behind it, so anything drawn at the camera, however small, hangs in
    // front of the eye the size of the view: the frustum is drawn fully
    // transparent with no line width, and the axes a camera gets by
    // default have no length.
    recording.log_static(
        preset::CHASE_PATH,
        &rerun::Pinhole::from_focal_length_and_resolution([1000.0, 1000.0], [1600.0, 900.0])
            .with_camera_xyz(rerun::components::ViewCoordinates::FLU)
            .with_image_plane_distance(0.01)
            .with_color([0, 0, 0, 0])
            .with_line_width(0.0),
    )?;
    recording.log_static(preset::CHASE_PATH, &rerun::TransformAxes3D::new(0.0))?;
    Ok(())
}

/// Where the chase camera is this frame: eased from where it was toward the
/// spot behind the entity along its heading and above it, so a jittering
/// heading moves the view little and a turn swings it over `CHASE_EASE_S`.
/// The first frame starts at the spot.
fn chase_position(
    previous: Option<[f32; 3]>,
    target: [f32; 3],
    heading_rad: f32,
    distance_m: f32,
    since_last_frame_s: f32,
) -> [f32; 3] {
    let (behind_m, up_m) = (distance_m * 0.9, distance_m * 0.35);
    let (sin_heading, cos_heading) = heading_rad.sin_cos();
    let spot = [
        target[0] - behind_m * cos_heading,
        target[1] - behind_m * sin_heading,
        target[2] + up_m,
    ];
    let Some(previous) = previous else {
        return spot;
    };
    let blend = 1.0 - (-since_last_frame_s / CHASE_EASE_S).exp();
    [0, 1, 2].map(|axis| previous[axis] + (spot[axis] - previous[axis]) * blend)
}

/// The chase camera's pose this frame: at `eye`, level, turned and pitched
/// to look at `target`, so the horizon stays put while the entity banks.
fn log_chase_pose(recording: &RecordingStream, eye: [f32; 3], target: [f32; 3]) -> Result<()> {
    let [dx, dy, dz] = [0, 1, 2].map(|axis| target[axis] - eye[axis]);
    let yaw_rad = dy.atan2(dx);
    let pitch_down_rad = (-dz).atan2((dx * dx + dy * dy).sqrt());
    // Yaw about +z, then pitch down about the camera's +y, as one quaternion
    // (w, x, y, z): the product of (cos y/2, 0, 0, sin y/2) and
    // (cos p/2, 0, sin p/2, 0).
    let (sin_half_yaw, cos_half_yaw) = (yaw_rad * 0.5).sin_cos();
    let (sin_half_pitch, cos_half_pitch) = (pitch_down_rad * 0.5).sin_cos();
    let rotation = rerun::Quaternion::from_wxyz([
        cos_half_yaw * cos_half_pitch,
        -sin_half_yaw * sin_half_pitch,
        cos_half_yaw * sin_half_pitch,
        sin_half_yaw * cos_half_pitch,
    ]);
    recording.log(
        preset::CHASE_PATH,
        &rerun::Transform3D::from_translation_rotation(eye, rotation),
    )?;
    Ok(())
}

/// The heading arrow's length: 8 m, or a fiftieth of the wider horizontal
/// span seen so far, so an arrow stays visible on a map of a region. It
/// grows with the span, so early frames of a run that spreads out later
/// have shorter arrows; plugin drawings widen the span from the first frame.
pub(super) fn heading_arrow_m(range: Option<[[f32; 3]; 2]>) -> f32 {
    let span_m = range.map_or(0.0, |[low, high]| (high[0] - low[0]).max(high[1] - low[1]));
    (span_m / 50.0).max(8.0)
}

/// Grow `range` so it includes `point`.
fn widen(range: &mut Option<[[f32; 3]; 2]>, point: [f32; 3]) {
    let [low, high] = range.get_or_insert([point, point]);
    for axis in 0..3 {
        low[axis] = low[axis].min(point[axis]);
        high[axis] = high[axis].max(point[axis]);
    }
}

/// The color for a team's entities and trails: blue for teams 1, 4, 7, ...,
/// red for 2, 5, 8, ..., and green for 0, 3, 6, ...; four or more teams share
/// them. Missions do not set colors: color is a display choice, so it stays
/// out of the scenario a run records.
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
    fn the_heading_arrow_is_8_m_up_to_a_400_m_span_then_a_fiftieth_of_it() {
        assert_eq!(heading_arrow_m(None), 8.0);
        assert_eq!(
            heading_arrow_m(Some([[0.0, 0.0, 0.0], [300.0, 100.0, 0.0]])),
            8.0
        );
        assert_eq!(
            heading_arrow_m(Some([
                [-30_000.0, -30_000.0, 0.0],
                [30_000.0, 30_000.0, 0.0]
            ])),
            1200.0
        );
    }

    #[test]
    fn playback_uses_seconds_and_retains_the_repeated_terminal_sample() {
        assert_eq!(playback_time_ns(0.1, Some(0)), 100_000_000);
        assert_eq!(playback_time_ns(0.1, Some(100_000_000)), 100_000_001);
        assert_eq!(playback_time_ns(0.2, Some(100_000_001)), 200_000_000);
    }
}

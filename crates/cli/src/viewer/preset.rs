//! The demo presets' layouts: a blueprint per preset with the camera from
//! the demo file. Written with Rerun's blueprint archetypes directly because
//! the SDK's layout builder has no way to set a 3D view's camera in this
//! version, so the views need paths we know.
use crate::demo::{Camera, Preset};
use anyhow::{Context, Result};
use rerun::components::Name;
use rerun::datatypes::{Bool, Float32, Float64};
use rerun::datatypes::{Range1D, Range2D};
use rerun::external::re_log_types::{BlueprintActivationCommand, RecordingId};
use rerun::external::re_sdk_types::blueprint::{
    archetypes::{
        Background, ContainerBlueprint, EyeControls3D, LineGrid3D, PanelBlueprint, PlotLegend,
        TimeAxis, TimePanelBlueprint, ViewBlueprint, ViewContents, ViewportBlueprint,
        VisibleTimeRanges, VisualBounds2D,
    },
    components::{
        AutoLayout, AutoViews, BackgroundKind, ColumnShare, ContainerKind, Eye3DKind,
        IncludedContent, LoopMode, PanelState, PlayState, PlaybackSpeed, QueryExpression,
        RootContainer, RowShare, TimelineName, ViewClass, VisibleTimeRange,
    },
};
use rerun::external::re_types_core::datatypes::{TimeRange, TimeRangeBoundary};
use rerun::{RecordingStream, RecordingStreamBuilder, log::LogMsg};

/// The sky behind a terrain: a 3D view's solid background where there is
/// ground to stand on. Without terrain the view keeps its reference grid,
/// which is the robots' floor, on a plain dark background.
const SKY: [u8; 3] = [158, 194, 230];
const DARK: [u8; 3] = [24, 26, 30];

/// The entity the demo's chase camera follows, and how far behind it the
/// camera sits.
pub(super) struct Followed {
    pub id: i32,
    pub distance_m: f32,
}

/// Where the chase camera is logged: its own entity, not a child of the
/// followed one, so it keeps a level horizon while the entity banks.
pub(super) const CHASE_PATH: &str = "world/chase";

pub(super) struct Layout {
    pub preset: Preset,
    pub camera: Camera,
    /// The entity the camera follows, when the camera is `follow` and the
    /// entity has appeared: its id, where it was first seen, and the
    /// direction it was heading, so the eye starts behind and above it.
    pub followed: Option<Followed>,
    /// Simulated seconds per second of playback.
    pub speed: f64,
    /// Whether the run drew a terrain: with one, the 3D view's grid is
    /// off and the sky is blue.
    pub has_terrain: bool,
    /// [west, east, south, north] of the map's grid, which the fleet
    /// preset's map is fitted to.
    pub map_extent_m: Option<[f32; 4]>,
}

/// Send the preset's layout to the recording and make it the active one.
pub(super) fn send(recording: &RecordingStream, layout: &Layout) -> Result<()> {
    let application_id = recording
        .store_info()
        .map(|info| info.application_id().clone())
        .unwrap_or_else(|| "ripple".into());
    let (stream, storage) = RecordingStreamBuilder::new(application_id)
        .recording_id(RecordingId::random())
        .blueprint()
        .memory()?;
    stream.set_time_sequence("blueprint", 0);
    let writer = Writer { stream: &stream };

    let map = writer.view("2D", "Map", "map", None)?;
    let captions = writer.view("TextDocument", "Captions", "captions", None)?;
    // The views above a caption strip the full width of the window, so the
    // caption reads at the same size as the scene. The fleet preset is the
    // map alone, which is square, so it fills the window's height with the
    // captions in the column beside it rather than floating small in a wide
    // view over a strip.
    let root = match layout.preset {
        Preset::Flight | Preset::Belief => {
            let world = writer.view("3D", "World", "world", layout.eye().as_ref())?;
            writer.world_style(&world, layout.has_terrain)?;
            let (name, origin) = match (layout.preset, layout.has_terrain) {
                (Preset::Flight, true) => {
                    ("Height above ground · m", "debug/height_above_ground_m")
                }
                (Preset::Flight, false) => ("Altitude · m", "debug/altitude_m"),
                _ => ("Position error · m", "debug/error_m"),
            };
            let plot = writer.view("TimeSeries", name, origin, None)?;
            // Six altitude lines need no legend over them; the two error
            // lines are told apart by it.
            writer.whole_run(&plot, layout.preset == Preset::Belief)?;
            let side = writer.container(
                ContainerKind::Vertical,
                &[map, plot],
                Shares::Rows(&[1.2, 1.0]),
            )?;
            let views = writer.container(
                ContainerKind::Horizontal,
                &[world, side],
                Shares::Columns(&[2.6, 1.0]),
            )?;
            writer.container(
                ContainerKind::Vertical,
                &[views, captions],
                Shares::Rows(&[1.0, 0.12]),
            )?
        }
        Preset::Fleet => {
            if let Some(extent_m) = layout.map_extent_m {
                writer.fit(&map, extent_m)?;
            }
            writer.container(
                ContainerKind::Horizontal,
                &[map, captions],
                Shares::Columns(&[1.0, 0.78]),
            )?
        }
    };
    stream.log(
        "viewport",
        &ViewportBlueprint::new()
            .with_root_container(RootContainer(root.id.into()))
            .with_auto_layout(AutoLayout(Bool(false)))
            .with_auto_views(AutoViews(Bool(false))),
    )?;
    for panel in ["blueprint_panel", "selection_panel"] {
        stream.log(
            panel,
            &PanelBlueprint::new().with_state(PanelState::Collapsed),
        )?;
    }
    stream.log(
        "time_panel",
        &TimePanelBlueprint::new()
            .with_state(PanelState::Collapsed)
            .with_timeline(TimelineName("time".into()))
            .with_play_state(PlayState::Playing)
            .with_loop_mode(LoopMode::All)
            .with_playback_speed(PlaybackSpeed(Float64(layout.speed))),
    )?;

    let messages = storage.take();
    let blueprint_id = messages
        .iter()
        .find_map(|message| match message {
            LogMsg::SetStoreInfo(info) => Some(info.info.store_id.clone()),
            _ => None,
        })
        .context("the blueprint has no store info")?;
    recording.send_blueprint(
        messages,
        BlueprintActivationCommand {
            blueprint_id,
            make_active: true,
            make_default: true,
        },
    );
    Ok(())
}

impl Layout {
    /// The 3D view's camera property, or none for a free camera.
    fn eye(&self) -> Option<EyeControls3D> {
        let position = |[x, y, z]: [f64; 3]| [x as f32, y as f32, z as f32];
        match &self.camera {
            Camera::Free => None,
            // The eye takes the pose of the chase camera the viewer logs
            // behind the followed entity each frame (`Viewer::log_frame`).
            Camera::Follow { .. } => self.followed.as_ref().map(|_| {
                EyeControls3D::new()
                    .with_kind(Eye3DKind::FirstPerson)
                    .with_tracking_entity(CHASE_PATH)
            }),
            Camera::Fixed {
                position_m,
                look_at_m,
            } => Some(
                EyeControls3D::new()
                    .with_kind(Eye3DKind::FirstPerson)
                    .with_position(position(*position_m))
                    .with_look_target(position(*look_at_m)),
            ),
            Camera::Orbit {
                position_m,
                look_at_m,
                spin_deg_per_s,
            } => Some(
                EyeControls3D::new()
                    .with_kind(Eye3DKind::Orbital)
                    .with_position(position(*position_m))
                    .with_look_target(position(*look_at_m))
                    .with_spin_speed(spin_deg_per_s.to_radians() as f32),
            ),
        }
    }
}

enum Shares<'a> {
    Columns(&'a [f32]),
    Rows(&'a [f32]),
}

/// A logged view or container: its blueprint path and id.
struct Node {
    path: String,
    id: uuid::Uuid,
}

struct Writer<'a> {
    stream: &'a RecordingStream,
}

impl Writer<'_> {
    /// Log a view showing everything under `origin`, with an optional camera.
    fn view(
        &self,
        class: &str,
        name: &str,
        origin: &str,
        eye: Option<&EyeControls3D>,
    ) -> Result<Node> {
        let id = uuid::Uuid::new_v4();
        let path = format!("view/{id}");
        self.stream.log(
            format!("{path}/ViewContents"),
            &ViewContents::new([QueryExpression("$origin/**".into())]),
        )?;
        self.stream.log(
            path.as_str(),
            &ViewBlueprint::new(ViewClass(class.into()))
                .with_display_name(Name(name.into()))
                .with_space_origin(origin),
        )?;
        if let Some(eye) = eye {
            self.stream.log(format!("{path}/EyeControls3D"), eye)?;
        }
        Ok(Node { path, id })
    }

    /// A 3D view's grid and sky: on a terrain the grid is off, since it
    /// floats at z = 0 through the hills, and the sky is blue; without one
    /// the grid is the floor and the background is dark.
    fn world_style(&self, world: &Node, has_terrain: bool) -> Result<()> {
        self.stream.log(
            format!("{}/LineGrid3D", world.path),
            &LineGrid3D::new().with_visible(!has_terrain),
        )?;
        self.stream.log(
            format!("{}/Background", world.path),
            &Background::new(BackgroundKind::SolidColor).with_color(if has_terrain {
                SKY
            } else {
                DARK
            }),
        )?;
        Ok(())
    }

    /// A 2D view shows exactly this [west, east, south, north] of the map,
    /// with a sliver of margin, instead of the viewer's looser fit of
    /// whatever is drawn when it opens.
    fn fit(&self, view: &Node, [west_m, east_m, south_m, north_m]: [f32; 4]) -> Result<()> {
        let margin_m = (east_m - west_m).max(north_m - south_m) as f64 * 0.02;
        // The map's canvas has north downward, as `map::project` draws it.
        self.stream.log(
            format!("{}/VisualBounds2D", view.path),
            &VisualBounds2D::new(Range2D {
                x_range: Range1D([west_m as f64 - margin_m, east_m as f64 + margin_m]),
                y_range: Range1D([-north_m as f64 - margin_m, -south_m as f64 + margin_m]),
            }),
        )?;
        Ok(())
    }

    /// A plot shows the whole run, so its axes stay put as time passes.
    fn whole_run(&self, plot: &Node, legend: bool) -> Result<()> {
        let whole = TimeRange {
            start: TimeRangeBoundary::Infinite,
            end: TimeRangeBoundary::Infinite,
        };
        self.stream.log(
            format!("{}/PlotLegend", plot.path),
            &PlotLegend::new().with_visible(legend),
        )?;
        self.stream.log(
            format!("{}/VisibleTimeRanges", plot.path),
            &VisibleTimeRanges::new([VisibleTimeRange(
                rerun::external::re_types_core::datatypes::VisibleTimeRange {
                    timeline: "time".into(),
                    range: whole,
                },
            )]),
        )?;
        self.stream.log(
            format!("{}/TimeAxis", plot.path),
            &TimeAxis::new().with_view_range(whole).with_zoom_lock(true),
        )?;
        Ok(())
    }

    /// Log a container of these views or containers.
    fn container(&self, kind: ContainerKind, children: &[Node], shares: Shares) -> Result<Node> {
        let id = uuid::Uuid::new_v4();
        let path = format!("container/{id}");
        let mut container = ContainerBlueprint::new(kind).with_contents(
            children
                .iter()
                .map(|child| IncludedContent(child.path.as_str().into())),
        );
        container = match shares {
            Shares::Columns(shares) => {
                container.with_col_shares(shares.iter().map(|&share| ColumnShare(Float32(share))))
            }
            Shares::Rows(shares) => {
                container.with_row_shares(shares.iter().map(|&share| RowShare(Float32(share))))
            }
        };
        self.stream.log(path.as_str(), &container)?;
        Ok(Node { path, id })
    }
}

//! A demo file: how one run is presented for the book's gallery. Read by
//! `ripple run --demo` and `ripple replay --demo`, which write a Rerun
//! recording with the demo's preset layout, camera, captions, interval, and
//! frame stride; `reference/demos.py` turns that into the gallery's files.
//! See docs/demos-plan.md.
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::{collections::BTreeMap, fs, path::PathBuf};

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct Demo {
    pub name: String,
    /// The mission to run, or the run directory or `run.mcap` to replay,
    /// relative to the repository root.
    pub source: PathBuf,
    /// Mission overrides, as `ripple run` takes them on the command line.
    #[serde(default)]
    pub overrides: BTreeMap<String, String>,
    /// Cargo features the run needs (`learning`, `ardupilot`, `px4`,
    /// `gtsam`), for the gallery script's build; the run itself fails with
    /// the plugin's own error when its feature is missing.
    #[serde(default)]
    pub features: Vec<String>,
    pub preset: Preset,
    /// The simulated interval shown; the run's start and end otherwise.
    pub start_s: Option<f64>,
    pub end_s: Option<f64>,
    /// Keep every nth frame of the interval.
    #[serde(default = "one")]
    pub every: usize,
    /// Simulated seconds per second of playback.
    #[serde(default = "one_f64")]
    pub speed: f64,
    /// Written as a one-key map, `{follow: {group: lead}}`, not a YAML tag.
    #[serde(default, with = "serde_yaml_ng::with::singleton_map")]
    pub camera: Camera,
    /// The entities that get trails and labels.
    #[serde(default)]
    pub featured: Vec<Selector>,
    /// Whether the featured entities are labelled on the map; off where
    /// they sit on labelled marks or on each other.
    #[serde(default = "yes")]
    pub labels: bool,
    #[serde(default)]
    pub captions: Vec<Caption>,
    /// The drawn size of a vehicle model, wingtip to wingtip.
    #[serde(default = "default_model_span_m")]
    pub model_span_m: f64,
    /// At most this many terrain samples per side in the recording; the
    /// terrain mesh and images skip samples to fit, which is what keeps a
    /// recording of a 10 km terrain small enough for a page.
    #[serde(default = "default_terrain_samples")]
    pub terrain_samples: usize,
    /// Draw only this square of the terrain, at full resolution within
    /// `terrain_samples`: a runway circuit needs sharp ground under it, not
    /// the whole 10 km thinned.
    pub terrain_window: Option<TerrainWindow>,
    /// A runway drawn on the ground: a dark strip with a centreline.
    pub runway: Option<Runway>,
    /// Rings drawn around a plugin's prominent marks for a while, to point
    /// at where something happens.
    #[serde(default)]
    pub highlights: Vec<Highlight>,
}

#[derive(Deserialize, Debug, Clone, Copy)]
#[serde(deny_unknown_fields)]
pub struct TerrainWindow {
    pub center_m: [f64; 2],
    pub half_span_m: f64,
}

#[derive(Deserialize, Debug, Clone, Copy)]
#[serde(deny_unknown_fields)]
pub struct Runway {
    pub center_m: [f64; 2],
    pub length_m: f64,
    pub width_m: f64,
    /// Compass heading of the strip, degrees clockwise from north.
    pub heading_deg: f64,
}

/// A ring around a drawn mark, found by its label (`hub 6`), shown from
/// `from_s` to `to_s`.
#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct Highlight {
    pub label: String,
    pub from_s: f64,
    pub to_s: f64,
    pub radius_m: f64,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Preset {
    /// A 3D view of the terrain and vehicles, with a map inset, the
    /// captions, and an altitude plot.
    Flight,
    /// Truth and belief with one-sigma ellipsoids, a map inset, the
    /// captions, and a position-error plot.
    Belief,
    /// The map with the plugins' drawings, square, with the captions in a
    /// column beside it.
    Fleet,
}

/// The 3D view's camera. `free` leaves the viewer's default framing.
#[derive(Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Camera {
    #[default]
    Free,
    /// Track one entity from behind and above it, this far away.
    Follow {
        group: String,
        index: Option<usize>,
        #[serde(default = "default_follow_distance_m")]
        distance_m: f64,
    },
    /// A fixed eye looking at a point.
    Fixed {
        position_m: [f64; 3],
        look_at_m: [f64; 3],
    },
    /// An eye orbiting a point, spinning about the vertical.
    Orbit {
        position_m: [f64; 3],
        look_at_m: [f64; 3],
        #[serde(default)]
        spin_deg_per_s: f64,
    },
}

/// An entity by its mission group and, optionally, its member number in
/// that group, counted from zero in spawn order.
#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Selector {
    pub group: String,
    pub index: Option<usize>,
}

impl Selector {
    pub fn matches(&self, group_label: &str, member: usize) -> bool {
        self.group == group_label && self.index.is_none_or(|index| index == member)
    }
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct Caption {
    pub at_s: f64,
    pub text: String,
}

fn one() -> usize {
    1
}

fn yes() -> bool {
    true
}

fn one_f64() -> f64 {
    1.0
}

fn default_model_span_m() -> f64 {
    10.0
}

fn default_follow_distance_m() -> f64 {
    300.0
}

fn default_terrain_samples() -> usize {
    256
}

impl Demo {
    pub fn load(path: &std::path::Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("read demo file {}", path.display()))?;
        let demo: Self = serde_yaml_ng::from_str(&text)
            .with_context(|| format!("demo file {}", path.display()))?;
        demo.check()
            .with_context(|| format!("demo file {}", path.display()))?;
        Ok(demo)
    }

    fn check(&self) -> Result<()> {
        ensure!(!self.name.is_empty(), "name must not be empty");
        ensure!(self.every >= 1, "every must be at least 1");
        ensure!(
            self.speed.is_finite() && self.speed > 0.0,
            "speed must be positive"
        );
        if let (Some(start_s), Some(end_s)) = (self.start_s, self.end_s) {
            ensure!(end_s > start_s, "end_s must be after start_s");
        }
        ensure!(
            self.model_span_m.is_finite() && self.model_span_m > 0.0,
            "model_span_m must be positive"
        );
        ensure!(
            self.terrain_samples >= 2,
            "terrain_samples must be at least 2"
        );
        if let Some(window) = &self.terrain_window {
            ensure!(
                window.half_span_m.is_finite() && window.half_span_m > 0.0,
                "terrain_window.half_span_m must be positive"
            );
        }
        if let Some(runway) = &self.runway {
            ensure!(
                runway.length_m > 0.0 && runway.width_m > 0.0,
                "runway length_m and width_m must be positive"
            );
        }
        for highlight in &self.highlights {
            ensure!(
                highlight.to_s > highlight.from_s && highlight.radius_m > 0.0,
                "highlight {} needs to_s after from_s and a positive radius_m",
                highlight.label
            );
        }
        ensure!(
            self.captions
                .windows(2)
                .all(|pair| pair[0].at_s <= pair[1].at_s),
            "captions must be in time order"
        );
        Ok(())
    }

    /// Whether a frame at this simulated time is inside the demo's interval.
    pub fn covers(&self, time_s: f64) -> bool {
        self.start_s.is_none_or(|start_s| time_s >= start_s)
            && self.end_s.is_none_or(|end_s| time_s <= end_s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "
name: terrain-flight
source: missions/terrain-mcmillan.yaml
overrides: {run.dt_s: '0.02', run.motion_multiplier: '1'}
preset: flight
start_s: 10
end_s: 100
every: 2
camera: {follow: {group: peak_orbit, index: 0}}
featured: [{group: peak_orbit}, {group: climber, index: 0}]
captions:
  - {at_s: 10, text: 'Six aircraft.'}
  - {at_s: 60, text: 'Two fly into the ridge.'}
";

    #[test]
    fn a_demo_file_reads_with_its_defaults_and_covers_its_interval() {
        let demo: Demo = serde_yaml_ng::from_str(FILE).unwrap();
        demo.check().unwrap();
        assert_eq!(demo.preset, Preset::Flight);
        assert_eq!(demo.speed, 1.0);
        assert_eq!(demo.model_span_m, 10.0);
        assert_eq!(demo.terrain_samples, 256);
        assert!(demo.labels);
        assert!(matches!(
            demo.camera,
            Camera::Follow {
                index: Some(0),
                distance_m,
                ..
            } if distance_m == 300.0
        ));
        assert!(demo.covers(10.0) && demo.covers(100.0));
        assert!(!demo.covers(9.99) && !demo.covers(100.01));
        assert!(demo.featured[0].matches("peak_orbit", 3));
        assert!(demo.featured[1].matches("climber", 0));
        assert!(!demo.featured[1].matches("climber", 1));
    }

    #[test]
    fn a_free_camera_is_the_default_and_bad_values_are_refused() {
        let demo: Demo =
            serde_yaml_ng::from_str("name: x\nsource: m.yaml\npreset: fleet\n").unwrap();
        assert!(matches!(demo.camera, Camera::Free));
        assert!(demo.covers(-1.0) && demo.covers(1e9));
        let bad: Demo = serde_yaml_ng::from_str(
            "name: x\nsource: m.yaml\npreset: fleet\nstart_s: 5\nend_s: 5\n",
        )
        .unwrap();
        assert!(bad.check().unwrap_err().to_string().contains("end_s"));
        let unknown: Result<Demo, _> =
            serde_yaml_ng::from_str("name: x\nsource: m.yaml\npreset: fleet\nshots: []\n");
        assert!(unknown.is_err());
    }

    #[test]
    fn ground_marks_read_and_are_checked() {
        let demo: Demo = serde_yaml_ng::from_str(
            "name: x\nsource: m.yaml\npreset: flight\n\
             terrain_window: {center_m: [244, 56], half_span_m: 1500}\n\
             runway: {center_m: [125, -400], length_m: 600, width_m: 20, heading_deg: 114}\n\
             highlights: [{label: 'hub 6', from_s: 10, to_s: 20, radius_m: 2500}]\n",
        )
        .unwrap();
        demo.check().unwrap();
        assert_eq!(demo.terrain_window.unwrap().half_span_m, 1500.0);
        assert_eq!(demo.runway.unwrap().heading_deg, 114.0);
        assert_eq!(demo.highlights[0].label, "hub 6");
        let bad: Demo = serde_yaml_ng::from_str(
            "name: x\nsource: m.yaml\npreset: fleet\n\
             highlights: [{label: 'hub 6', from_s: 20, to_s: 10, radius_m: 2500}]\n",
        )
        .unwrap();
        assert!(bad.check().unwrap_err().to_string().contains("hub 6"));
    }
}

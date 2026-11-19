use crate::demo::{Demo, Preset};
use crate::viewer::{Group, Viewer, ViewerSettings, covariance};
use anyhow::{Context, Result, bail, ensure};
use clap::Args;
use ripple_core::{
    Mission, Params, ScenarioConfig, Simulation, TraceSink,
    plugin::PluginRegistry,
    record::{Belief, Fix, RunWriter},
};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub(crate) mod output;

#[derive(Args)]
pub(crate) struct RunOptions {
    /// The mission file; a demo file's `source` when `--demo` is given.
    #[arg(required_unless_present = "demo")]
    mission: Option<PathBuf>,
    /// Present the run as this demo file describes (docs/demos-plan.md):
    /// its preset layout, camera, captions, interval, and frame stride go
    /// into the Rerun export, which this turns on.
    #[arg(long)]
    demo: Option<PathBuf>,
    /// Output directory; defaults to the next <root>/runs/<mission file name>/run000, run001, ...
    #[arg(long)]
    output: Option<PathBuf>,
    /// Project folder (missions/, runs/, sweeps/); defaults to this repository.
    #[arg(long)]
    root: Option<PathBuf>,
    /// Open Rerun and stream the live run.
    #[arg(long, conflicts_with = "headless")]
    viewer: bool,
    /// Draw each agent's recent path in the live viewer (not saved; see
    /// `ripple replay --draw-trails`).
    #[arg(long)]
    draw_trails: bool,
    /// Draw each agent's belief and tracks as one-sigma position ellipsoids
    /// in the viewer or the RRD export.
    #[arg(long)]
    draw_covariance: bool,
    /// Suppress the viewer even if enable_gui is true in the mission.
    #[arg(long)]
    headless: bool,
    /// Save an optional Rerun recording alongside run.mcap.
    #[arg(long)]
    export_rrd: bool,
    #[arg(long, default_value_t = 1_000_000)]
    max_steps: usize,
    /// Record beliefs, readings, deliveries, and outputs in run.mcap.
    #[arg(long)]
    trace: bool,
    /// Mission overrides: dotted paths and YAML values (`entities.red.count:=20`).
    overrides: Vec<String>,
}
pub(crate) fn run(
    options: RunOptions,
    registry: &Arc<PluginRegistry>,
    project_root: &Path,
) -> Result<()> {
    let root = options.root.as_deref().unwrap_or(project_root);
    ensure!(options.max_steps > 0, "max_steps must be positive");
    let demo = options.demo.as_deref().map(Demo::load).transpose()?;
    let mission_path = match (&options.mission, &demo) {
        (Some(mission), _) => mission.clone(),
        (None, Some(demo)) => {
            let source = root.join(&demo.source);
            if source.is_dir() || source.extension().is_some_and(|ext| ext == "mcap") {
                bail!(
                    "demo source {} is a saved run; present it with `ripple replay --demo`",
                    demo.source.display()
                );
            }
            source
        }
        (None, None) => bail!("a mission file or --demo is needed"),
    };
    let mut overrides = Params::new();
    for (name, value) in demo.iter().flat_map(|demo| demo.overrides.iter()) {
        overrides.insert(name.clone(), value.clone());
    }
    for argument in &options.overrides {
        let (name, value) = argument
            .split_once(":=")
            .context("override must have name:=value form")?;
        ensure!(!name.is_empty(), "override name must not be empty");
        overrides.insert(name.to_owned(), value.to_owned());
    }
    let mission = Mission::load(&mission_path, root, &overrides)?;
    let name = mission_path
        .file_stem()
        .context("mission path must name a file")?
        .to_string_lossy()
        .into_owned();
    run_scenario(
        mission.scenario,
        registry,
        RunSettings {
            root: root.to_path_buf(),
            name,
            output: options.output,
            viewer: !options.headless && (options.viewer || mission.viewer),
            export_rrd: options.export_rrd || demo.is_some(),
            draw_trails: options.draw_trails,
            // The belief preset draws the ellipsoids and the error plot.
            draw_covariance: options.draw_covariance
                || demo
                    .as_ref()
                    .is_some_and(|demo| demo.preset == Preset::Belief),
            max_steps: options.max_steps,
            trace: options.trace,
            demo,
        },
    )?;
    Ok(())
}

/// Output and execution settings for a file-loaded or Rust-built scenario.
/// Pacing and live simulation controls are separate work; the runner is unpaced.
#[derive(Clone, Debug)]
pub struct RunSettings {
    /// Project directory (default: current directory). Automatic outputs go
    /// under `<root>/runs/<name>/`; pass the repository root for workspace runs.
    pub root: PathBuf,
    /// Experiment name used for automatic output directories.
    pub name: String,
    /// Explicit output directory, which must not already exist.
    pub output: Option<PathBuf>,
    /// Launch the live viewer.
    pub viewer: bool,
    /// Export `recording.rrd` in addition to run.mcap.
    pub export_rrd: bool,
    /// Draw each agent's recent path in the viewer. Requires `viewer`; trails
    /// are never saved.
    pub draw_trails: bool,
    /// Draw each agent's belief and tracks as one-sigma position ellipsoids.
    /// Requires `viewer` or `export_rrd`.
    pub draw_covariance: bool,
    pub max_steps: usize,
    /// Record observation channels; simulation outputs are unchanged.
    pub trace: bool,
    /// Present the export as this demo (`ripple run --demo`).
    pub demo: Option<Demo>,
}

impl Default for RunSettings {
    fn default() -> Self {
        Self {
            root: PathBuf::from("."),
            name: "experiment".into(),
            output: None,
            viewer: false,
            export_rrd: false,
            draw_trails: false,
            draw_covariance: false,
            max_steps: 1_000_000,
            trace: false,
            demo: None,
        }
    }
}

/// Run a scenario with the same record and failure handling as `ripple run`.
/// Invalid setup fails before an output directory is created. Step failures retain
/// a valid partial MCAP with their error in the result metadata.
pub fn run_scenario(
    scenario: ScenarioConfig,
    registry: &Arc<PluginRegistry>,
    settings: RunSettings,
) -> Result<PathBuf> {
    ensure!(settings.max_steps > 0, "max_steps must be positive");
    ensure!(
        !settings.draw_trails || settings.viewer,
        "--draw-trails needs the viewer (--viewer)"
    );
    ensure!(
        !settings.draw_covariance || settings.viewer || settings.export_rrd,
        "--draw-covariance needs the viewer (--viewer) or an export (--export-rrd)"
    );
    ensure!(
        !settings.name.is_empty(),
        "experiment name must not be empty"
    );
    let mut simulation = Simulation::new(scenario, Arc::clone(registry))?;
    let runs = settings.root.join("runs").join(&settings.name);
    let output = output::create_directory(settings.output.as_deref(), &runs)?;
    // One SITL container per ArduPilotBridge or Px4Bridge aircraft, stopped
    // when this function returns (crates/ardupilot/src/sitl_launcher.rs,
    // crates/px4/src/px4_launcher.rs).
    #[cfg(feature = "ardupilot")]
    let _sitl =
        ardupilot::SitlLauncher::start(simulation.scenario(), simulation.terrain(), &output)?;
    #[cfg(feature = "px4")]
    let _px4 = px4::Px4Launcher::start(simulation.scenario(), simulation.terrain(), &output)?;
    let mut viewer = if settings.export_rrd || settings.viewer {
        // Rerun streams need a recording ID, independent of the stable MCAP bytes.
        let recording_id = format!(
            "{}-{}",
            settings.name,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        );
        let scenario = simulation.scenario();
        let groups = scenario
            .entities
            .iter()
            .map(|group| (group, group.count))
            .chain(scenario.recipes.iter().map(|group| (group, 0)))
            .map(|(group, count)| Group {
                label: group.label.clone(),
                visual_model: group.visual_model.clone(),
                count,
            })
            .collect();
        let mut viewer = Viewer::new(ViewerSettings {
            export: settings
                .export_rrd
                .then(|| output.join("recording.rrd"))
                .as_deref(),
            launch: settings.viewer,
            recording_id: &recording_id,
            draw_trails: settings.draw_trails,
            draw_covariance: settings.draw_covariance,
            groups,
            demo: settings.demo.clone(),
        })?;
        if let Some(terrain) = simulation.terrain() {
            viewer.log_terrain(terrain)?;
        }
        Some(viewer)
    } else {
        None
    };
    let mut record = RunWriter::create(&output.join("run.mcap"), &simulation, settings.trace)?;
    // Keep a step error until the result metadata and MCAP footer are written.
    let mut frame_index = 0;
    let step_result = (|| -> Result<()> {
        loop {
            // The beliefs drawn with a frame are the ones the trace records
            // for it: taken inside the step, after the entities that start
            // this tick have initialized and before autonomy runs.
            let mut observer = Observer {
                record: settings.trace.then_some(&mut record),
                beliefs: settings.draw_covariance.then(Vec::new),
                fixes: settings.draw_covariance.then(Vec::new),
            };
            let observe = settings.trace || settings.draw_covariance;
            let Some(frame) = simulation
                .step_with_trace(observe.then_some(&mut observer as &mut dyn TraceSink))?
            else {
                break;
            };
            let beliefs = observer.beliefs.take();
            let fixes = observer.fixes.take();
            record.step_frame(&simulation, &frame)?;
            if let Some(viewer) = &mut viewer {
                viewer.log_frame(frame_index, &frame)?;
                if let Some(beliefs) = &beliefs
                    && viewer.drew_last_frame()
                {
                    viewer.log_beliefs(&covariance::from_beliefs(beliefs))?;
                    viewer.log_belief_errors(beliefs)?;
                    viewer.log_fixes(fixes.as_deref().unwrap_or_default())?;
                }
            }
            frame_index += 1;
            ensure!(
                simulation.step_count() <= settings.max_steps,
                "run exceeded {} steps",
                settings.max_steps
            );
        }
        Ok(())
    })();
    record.finish_run(&simulation, step_result.as_ref().err())?;
    if let Some(viewer) = viewer {
        viewer.finish()?;
    }
    if let Err(error) = step_result {
        return Err(error.context(format!(
            "run failed; partial output in {}",
            output.display()
        )));
    }
    println!(
        "{} steps; output {}",
        simulation.step_count(),
        output.display()
    );
    Ok(output)
}

/// Where a step's observations go: into the record with `--trace`, and the
/// belief and reading records to the viewer with `--draw-covariance`.
struct Observer<'a> {
    record: Option<&'a mut RunWriter>,
    beliefs: Option<Vec<Belief>>,
    fixes: Option<Vec<Fix>>,
}

impl TraceSink for Observer<'_> {
    fn record(&mut self, kind: &str, time_s: f64, value: serde_json::Value) -> Result<()> {
        if let Some(beliefs) = &mut self.beliefs
            && kind == "belief"
        {
            beliefs.push(Belief::from_trace(&value)?);
        }
        if let Some(fixes) = &mut self.fixes
            && kind == "reading"
        {
            fixes.extend(Fix::all_from_trace(&value)?);
        }
        if let Some(record) = &mut self.record {
            record.record(kind, time_s, value)?;
        }
        Ok(())
    }
}

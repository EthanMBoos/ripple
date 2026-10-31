use crate::viewer::Viewer;
use anyhow::{Context, Result, ensure};
use clap::Args;
use ripple_core::{
    Mission, Params, ScenarioConfig, Simulation, plugin::PluginRegistry,
    plugin::interaction::BoundaryRegion, record::RunWriter,
};
use std::path::{Path, PathBuf};

pub(crate) mod output;

#[derive(Args)]
pub(crate) struct RunOptions {
    mission: PathBuf,
    /// Output directory; defaults to the next <root>/runs/<mission file name>/run000, run001, ...
    #[arg(long)]
    output: Option<PathBuf>,
    #[arg(long)]
    workers: Option<usize>,
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
    registry: &PluginRegistry,
    project_root: &Path,
) -> Result<()> {
    let root = options.root.as_deref().unwrap_or(project_root);
    ensure!(options.max_steps > 0, "max_steps must be positive");
    let mut overrides = Params::new();
    for argument in &options.overrides {
        let (name, value) = argument
            .split_once(":=")
            .context("override must have name:=value form")?;
        ensure!(!name.is_empty(), "override name must not be empty");
        overrides.insert(name.to_owned(), value.to_owned());
    }
    let mission = Mission::load(&options.mission, root, &overrides)?;
    let name = options
        .mission
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
            workers: options.workers.unwrap_or(mission.workers),
            viewer: !options.headless && (options.viewer || mission.viewer),
            export_rrd: options.export_rrd,
            draw_trails: options.draw_trails,
            max_steps: options.max_steps,
            trace: options.trace,
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
    pub workers: usize,
    /// Launch the live viewer.
    pub viewer: bool,
    /// Export `recording.rrd` in addition to run.mcap.
    pub export_rrd: bool,
    /// Draw each agent's recent path in the viewer. Requires `viewer`; trails
    /// are never saved.
    pub draw_trails: bool,
    pub max_steps: usize,
    /// Record observation channels; simulation outputs are unchanged.
    pub trace: bool,
}

impl Default for RunSettings {
    fn default() -> Self {
        Self {
            root: PathBuf::from("."),
            name: "experiment".into(),
            output: None,
            workers: 1,
            viewer: false,
            export_rrd: false,
            draw_trails: false,
            max_steps: 1_000_000,
            trace: false,
        }
    }
}

/// Run a scenario with the same record and failure handling as `ripple run`.
/// Invalid setup fails before an output directory is created. Step failures retain
/// a valid partial MCAP with their error in the result metadata.
pub fn run_scenario(
    scenario: ScenarioConfig,
    registry: &PluginRegistry,
    settings: RunSettings,
) -> Result<PathBuf> {
    ensure!(settings.max_steps > 0, "max_steps must be positive");
    ensure!(
        !settings.draw_trails || settings.viewer,
        "--draw-trails needs the viewer (--viewer)"
    );
    ensure!(
        !settings.name.is_empty(),
        "experiment name must not be empty"
    );
    let mut simulation = Simulation::new(scenario, registry, settings.workers)?;
    let runs = settings.root.join("runs").join(&settings.name);
    let output = output::create_directory(settings.output.as_deref(), &runs)?;
    // One ArduPlane SITL container per ArduPilotBridge aircraft, stopped when
    // this function returns (crates/ardupilot/src/sitl_launcher.rs).
    #[cfg(feature = "ardupilot")]
    let _sitl =
        ardupilot::SitlLauncher::start(simulation.scenario(), simulation.terrain(), &output)?;
    let mut viewer = if settings.export_rrd || settings.viewer {
        // Rerun streams need a recording ID, independent of the stable MCAP bytes.
        let recording_id = format!(
            "{}-{}",
            settings.name,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        );
        let mut viewer = Viewer::new(
            settings
                .export_rrd
                .then(|| output.join("recording.rrd"))
                .as_deref(),
            settings.viewer,
            &recording_id,
            settings.draw_trails,
        )?;
        if let Some(terrain) = simulation.terrain() {
            viewer.log_terrain(terrain)?;
        }
        viewer.log_boundaries(&BoundaryRegion::in_scenario(
            &simulation.scenario().interactions,
        )?)?;
        viewer.log_delivery(&serde_json::to_value(simulation.scenario())?)?;
        Some(viewer)
    } else {
        None
    };
    let mut record = RunWriter::create(&output.join("run.mcap"), &simulation, settings.trace)?;
    // Keep a step error until the result metadata and MCAP footer are written.
    let mut frame_index = 0;
    let step_result = (|| -> Result<()> {
        while let Some(frame) = simulation.step_with_trace(
            settings
                .trace
                .then_some(&mut record as &mut dyn ripple_core::TraceSink),
        )? {
            record.step_frame(&simulation, &frame)?;
            if let Some(viewer) = &mut viewer {
                viewer.log_frame(frame_index, &frame)?;
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

use crate::viewer::Viewer;
use anyhow::{Context, Result, ensure};
use clap::Args;
use scrimmage_core::{
    Mission, Params, ScenarioConfig, Simulation, plugin::PluginRegistry, write_frame,
};
use std::{
    fs,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

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
    /// Open Rerun and stream live, also keeping recording.rrd.
    #[arg(long, conflicts_with_all = ["headless", "no_rerun"])]
    viewer: bool,
    /// Suppress the viewer even if enable_gui is true in the mission.
    #[arg(long)]
    headless: bool,
    /// Skip Rerun recording as well as the viewer.
    #[arg(long)]
    no_rerun: bool,
    #[arg(long, default_value_t = 1_000_000)]
    max_steps: usize,
    /// Mission overrides: XML substitutions (`count:=20`) or YAML dotted paths
    /// (`entities.red.count:=20`).
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
    let mission = Mission::load_with_registry(&options.mission, root, &overrides, registry)?;
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
            viewer: !options.headless && !options.no_rerun && (options.viewer || mission.viewer),
            recording: !options.no_rerun,
            max_steps: options.max_steps,
            source: Some(mission.source),
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
    /// Launch the viewer while recording. Requires `recording`.
    pub viewer: bool,
    /// Save `recording.rrd` in addition to frames, events, and summary.
    pub recording: bool,
    pub max_steps: usize,
    /// Optional mission-file provenance; Rust-built scenarios can leave it unset.
    pub source: Option<PathBuf>,
}

impl Default for RunSettings {
    fn default() -> Self {
        Self {
            root: PathBuf::from("."),
            name: "experiment".into(),
            output: None,
            workers: 1,
            viewer: false,
            recording: true,
            max_steps: 1_000_000,
            source: None,
        }
    }
}

/// Run a scenario with the same outputs, recording, and failure handling as `scrimmage run`.
/// Invalid setup fails before an output directory is created. Step failures retain
/// partial outputs and their error in `manifest.json`.
pub fn run_scenario(
    scenario: ScenarioConfig,
    registry: &PluginRegistry,
    settings: RunSettings,
) -> Result<PathBuf> {
    ensure!(settings.max_steps > 0, "max_steps must be positive");
    ensure!(
        !settings.viewer || settings.recording,
        "viewer requires recording"
    );
    ensure!(
        !settings.name.is_empty(),
        "experiment name must not be empty"
    );
    let scenario_record = serde_json::to_value(&scenario)?;
    let mut simulation = Simulation::new(scenario, registry, settings.workers)?;

    let runs = settings.root.join("runs").join(&settings.name);
    let output = output::create_directory(settings.output.as_deref(), &runs)?;
    let mut viewer = if settings.recording {
        Some(Viewer::new(&output.join("recording.rrd"), settings.viewer)?)
    } else {
        None
    };
    let mut frames = BufWriter::new(fs::File::create_new(output.join("frames.bin"))?);
    // Keep the step error instead of returning early, so a failed run still records
    // its frames so far, events, summary, and the reason in manifest.json.
    let mut frame_index = 0;
    let step_result = (|| -> Result<()> {
        while let Some(frame) = simulation.step()? {
            write_frame(&mut frames, &frame)?;
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
    frames.flush()?;
    if let Some(viewer) = viewer {
        viewer.finish()?;
    }
    fs::write(output.join("summary.csv"), simulation.summary_csv())?;
    fs::write(
        output.join("events.json"),
        serde_json::to_vec_pretty(simulation.events())?,
    )?;
    let manifest = serde_json::json!({
        "reference_branch": "Ubuntu-24.04",
        "execution": "unpaced",
        "rerun_recording": settings.recording,
        "viewer": settings.viewer,
        "worker_count": settings.workers,
        "source": settings.source,
        "scenario": scenario_record,
        "steps": simulation.step_count(),
        "termination": simulation.termination(),
        "error": step_result.as_ref().err().map(|error| format!("{error:#}")),
    });
    fs::write(
        output.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
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

use crate::viewer::Viewer;
use anyhow::{Context, Result, ensure};
use clap::Args;
use scrimmage_core::{Params, ScenarioConfig, Simulation, write_frame};
use std::{
    fs,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

mod output;

#[derive(Args)]
pub(crate) struct RunOptions {
    mission: PathBuf,
    /// Output directory; defaults to the next runs/run000, run001, ... in the current directory.
    #[arg(long)]
    output: Option<PathBuf>,
    #[arg(long)]
    workers: Option<usize>,
    #[arg(long, default_value_os_t = default_root())]
    root: PathBuf,
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
    /// Legacy mission substitutions, for example count:=20.
    overrides: Vec<String>,
}
fn default_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
pub(crate) fn run(options: RunOptions) -> Result<()> {
    ensure!(options.max_steps > 0, "max_steps must be positive");
    let mut overrides = Params::new();
    for argument in &options.overrides {
        let (name, value) = argument
            .split_once(":=")
            .context("override must have name:=value form")?;
        ensure!(!name.is_empty(), "override name must not be empty");
        overrides.insert(name.to_owned(), value.to_owned());
    }
    let mut config = ScenarioConfig::load(&options.mission, &options.root, &overrides)?;
    let launch_viewer =
        !options.headless && !options.no_rerun && (options.viewer || config.enable_gui);
    let worker_count = options.workers.unwrap_or(config.worker_count);
    // Pacing and live simulation controls remain separate work; this runner is unpaced.
    config.enable_gui = launch_viewer;
    config.start_paused = false;
    config.time_warp = 0.0;
    config.worker_count = worker_count;
    let resolved_config = serde_json::to_value(&config)?;
    let mut simulation = Simulation::new(config.resolve()?, worker_count)?;

    let output = output::create_directory(options.output.as_deref(), Path::new("runs"))?;
    let mut viewer = if options.no_rerun {
        None
    } else {
        Some(Viewer::new(&output.join("recording.rrd"), launch_viewer)?)
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
                simulation.step_count() <= options.max_steps,
                "run exceeded {} steps",
                options.max_steps
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
        "rerun_recording": !options.no_rerun,
        "viewer": launch_viewer,
        "scenario": resolved_config,
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
    Ok(())
}

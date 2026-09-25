//! `scrimmage compare`: check that two mission files (usually an XML mission and
//! its YAML form) set up the same simulation and record the same outputs.
use anyhow::{Result, bail, ensure};
use clap::Args;
use scrimmage_core::{Params, ScenarioConfig, Simulation, plugin::PluginRegistry, write_frame};
use std::path::{Path, PathBuf};

#[derive(Args)]
pub(crate) struct CompareOptions {
    first: PathBuf,
    second: PathBuf,
    /// Project folder (missions/, runs/, sweeps/); defaults to this repository.
    #[arg(long)]
    root: Option<PathBuf>,
    #[arg(long, default_value_t = 1_000_000)]
    max_steps: usize,
}

pub(crate) fn compare(
    options: CompareOptions,
    registry: &PluginRegistry,
    project_root: &Path,
) -> Result<()> {
    let root = options.root.as_deref().unwrap_or(project_root);
    let load =
        |path: &Path| ScenarioConfig::load_with_registry(path, root, &Params::new(), registry);
    let first = load(&options.first)?;
    let second = load(&options.second)?;

    let differences = first.setup_differences(&second)?;
    if !differences.is_empty() {
        for difference in &differences {
            println!("  {difference}");
        }
        bail!("the missions set up different simulations");
    }

    // Step both together so a mismatch stops at its first frame.
    let mut first = Simulation::new(first.resolve_with_registry(registry)?, 1)?;
    let mut second = Simulation::new(second.resolve_with_registry(registry)?, 1)?;
    loop {
        let (frame_a, frame_b) = (first.step()?, second.step()?);
        let (mut bytes_a, mut bytes_b) = (Vec::new(), Vec::new());
        if let Some(frame) = &frame_a {
            write_frame(&mut bytes_a, frame)?;
        }
        if let Some(frame) = &frame_b {
            write_frame(&mut bytes_b, frame)?;
        }
        ensure!(
            bytes_a == bytes_b,
            "frames differ at step {} (t = {} s); check plugin parameter values",
            first.step_count(),
            first.time_s()
        );
        if frame_a.is_none() {
            break;
        }
        ensure!(
            first.step_count() <= options.max_steps,
            "run exceeded {} steps",
            options.max_steps
        );
    }
    ensure!(
        serde_json::to_vec(first.events())? == serde_json::to_vec(second.events())?,
        "events differ; check plugin parameter values"
    );
    ensure!(
        first.summary_csv() == second.summary_csv(),
        "summaries differ; check plugin parameter values"
    );
    // Plugin parameter values are compared only through these outputs, so a
    // value that no recorded output reflects can differ unnoticed.
    println!("same setup and identical recorded outputs (frames, events, summary)");
    Ok(())
}

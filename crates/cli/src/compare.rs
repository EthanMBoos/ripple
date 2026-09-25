//! `scrimmage compare`: check that two mission files (usually an XML mission and
//! its YAML form) set up the same simulation and record the same outputs.
use anyhow::{Result, bail, ensure};
use clap::Args;
use scrimmage_core::{Params, ScenarioConfig, Simulation, write_frame};
use std::path::PathBuf;

#[derive(Args)]
pub(crate) struct CompareOptions {
    first: PathBuf,
    second: PathBuf,
    #[arg(long, default_value_os_t = crate::run::default_root())]
    root: PathBuf,
    #[arg(long, default_value_t = 1_000_000)]
    max_steps: usize,
}

pub(crate) fn compare(options: CompareOptions) -> Result<()> {
    let first = ScenarioConfig::load(&options.first, &options.root, &Params::new())?;
    let second = ScenarioConfig::load(&options.second, &options.root, &Params::new())?;

    let differences = first.setup_differences(&second)?;
    if !differences.is_empty() {
        for difference in &differences {
            println!("  {difference}");
        }
        bail!("the missions set up different simulations");
    }

    // Step both together so a mismatch stops at its first frame.
    let mut first = Simulation::new(first.resolve()?, 1)?;
    let mut second = Simulation::new(second.resolve()?, 1)?;
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

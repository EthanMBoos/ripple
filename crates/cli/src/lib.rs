//! The `ripple` command line as a library.
//!
//! `src/main.rs` builds the plugin registry (the stock plugins plus the user
//! plugin crates in this workspace, such as crates/starter) and hands it to
//! [`main`]. Every command that simulates (`run`, `sweep`) uses that registry, so
//! user plugins work everywhere stock plugins do. See book/src/guides/writing-plugins.md.
use anyhow::Result;
use clap::{Parser, Subcommand};
use ripple_core::plugin::PluginRegistry;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub mod demo;
mod replay;
mod run;
mod sweep;
mod viewer;

pub use run::{RunSettings, run_scenario};

#[derive(Parser)]
#[command(
    name = "ripple",
    version,
    about = "Ripple, a multi-agent robotics simulator ported from SCRIMMAGE, with Rerun visualization"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run a mission, recording frames, events, and metrics in MCAP.
    Run(run::RunOptions),
    /// Run a YAML mission across seeds and parameter values (a `*.sweep.yaml`); run again to resume.
    Sweep(sweep::SweepOptions),
    /// How far a sweep's newest run is: cases done, errors, running or stopped.
    Status(sweep::StatusOptions),
    /// Merge the shards of a sweep that was submitted with --slurm.
    #[command(hide = true)]
    Collect {
        /// The run folder `ripple sweep --slurm` printed.
        campaign: PathBuf,
    },
    /// View a saved MCAP run without rerunning the mission.
    Replay {
        /// Run directory containing run.mcap, or the MCAP file itself.
        path: PathBuf,
        /// Also draw each agent's recent path, rebuilt from the frames.
        #[arg(long)]
        draw_trails: bool,
        /// Also draw each agent's belief and tracks as one-sigma position
        /// ellipsoids, from the `/beliefs` channel of a run recorded with
        /// `--trace`.
        #[arg(long)]
        draw_covariance: bool,
        /// Present the record as this demo file describes (docs/demos-plan.md)
        /// and write the recording to --output instead of opening a viewer.
        #[arg(long, requires = "output")]
        demo: Option<PathBuf>,
        /// Where `--demo` writes its recording; must not exist.
        #[arg(long, requires = "demo")]
        output: Option<PathBuf>,
    },
}

/// Parses the command line and runs the command with `registry`'s plugins.
///
/// `project_root` is the repository: missions are looked up in its
/// `missions/`, and runs and sweeps are written to its `runs/` and `sweeps/`.
/// `--root` overrides it.
pub fn main(registry: PluginRegistry, project_root: impl AsRef<Path>) -> Result<()> {
    // Resolve `crates/cli/../..` to a plain path so printed output paths are readable.
    let project_root = project_root
        .as_ref()
        .canonicalize()
        .unwrap_or_else(|_| project_root.as_ref().to_path_buf());
    let project_root = project_root.as_path();
    match Cli::parse().command {
        Command::Run(options) => run::run(options, &Arc::new(registry), project_root)?,
        Command::Sweep(options) => sweep::sweep(options, &Arc::new(registry), project_root)?,
        Command::Status(options) => sweep::status(options, project_root)?,
        Command::Collect { campaign } => sweep::campaign::collect_command(&campaign)?,
        Command::Replay {
            path,
            draw_trails,
            draw_covariance,
            demo,
            output,
        } => replay::replay(
            &path,
            draw_trails,
            draw_covariance,
            demo.as_deref(),
            output.as_deref(),
        )?,
    }
    Ok(())
}

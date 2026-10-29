//! The `ripple` command line as a library.
//!
//! `src/main.rs` builds the plugin registry (the stock plugins plus the user
//! plugin crates in this workspace, such as crates/starter) and hands it to
//! [`main`]. Every command (`run`, `sweep`) uses that registry, so
//! user plugins work everywhere stock plugins do. See book/src/guides/writing-plugins.md.
use anyhow::Result;
use clap::{Parser, Subcommand};
use ripple_core::plugin::PluginRegistry;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

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
    /// Run a YAML mission across seeds and parameter values (a `*.sweep.yaml`), whole or one shard.
    Sweep(sweep::SweepOptions),
    /// View a saved MCAP run without rerunning the mission.
    Replay {
        /// Run directory containing run.mcap, or the MCAP file itself.
        path: PathBuf,
        /// Also draw each agent's recent path, rebuilt from the frames.
        #[arg(long)]
        draw_trails: bool,
    },
}

/// Parses `args` (the program name first, as `std::env::args_os` gives them)
/// and runs the command with `registry`'s plugins. The arguments are passed in
/// rather than read from the process because the Python package's `ripple`
/// command runs inside an interpreter whose own arguments come first.
///
/// `project_root` is the repository: missions are looked up in its
/// `missions/`, and runs and sweeps are written to its `runs/` and `sweeps/`.
/// `--root` overrides it.
pub fn main(
    registry: PluginRegistry,
    project_root: impl AsRef<Path>,
    args: impl IntoIterator<Item = OsString>,
) -> Result<()> {
    // Resolve `crates/cli/../..` to a plain path so printed output paths are readable.
    let project_root = project_root
        .as_ref()
        .canonicalize()
        .unwrap_or_else(|_| project_root.as_ref().to_path_buf());
    let project_root = project_root.as_path();
    match Cli::parse_from(args).command {
        Command::Run(options) => run::run(options, &registry, project_root)?,
        Command::Sweep(options) => sweep::sweep(options, &registry, project_root)?,
        Command::Replay { path, draw_trails } => replay::replay(&path, draw_trails)?,
    }
    Ok(())
}

//! The `scrimmage` command line as a library.
//!
//! `src/main.rs` builds the plugin registry (the stock plugins plus the user
//! plugin crates in this workspace, such as crates/starter) and hands it to
//! [`main`]. Every command (`run`, `sweep`, `compare`) uses that registry, so
//! user plugins work everywhere stock plugins do. See docs/USER_PROJECTS.md.
use anyhow::Result;
use clap::{Parser, Subcommand};
use scrimmage_core::plugin::PluginRegistry;
use std::path::{Path, PathBuf};

mod compare;
mod replay;
mod run;
mod sweep;
mod viewer;

#[derive(Parser)]
#[command(
    name = "scrimmage",
    version,
    about = "SCRIMMAGE Rust port with Rerun visualization"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run a mission, recording frames, metrics, and Rerun data.
    Run(run::RunOptions),
    /// Check that two missions (such as an XML file and its YAML form) run identically.
    Compare(compare::CompareOptions),
    /// Run a YAML mission across seeds and parameter values (a `*.sweep.yaml`), whole or one shard.
    Sweep(sweep::SweepOptions),
    /// Open a saved Rerun recording without rerunning the mission.
    Replay {
        /// Run directory containing recording.rrd, or the recording file itself.
        path: PathBuf,
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
        Command::Run(options) => run::run(options, &registry, project_root)?,
        Command::Compare(options) => compare::compare(options, &registry, project_root)?,
        Command::Sweep(options) => sweep::sweep(options, &registry, project_root)?,
        Command::Replay { path } => replay::replay(&path)?,
    }
    Ok(())
}

//! Rebuild a Rerun view from a saved MCAP record without running simulation.

use std::{path::Path, process::Command};

use anyhow::{Context, Result, ensure};
use ripple_core::record;

use crate::viewer::Viewer;

pub(crate) fn replay(path: &Path, draw_trails: bool) -> Result<()> {
    let record_path = if path.is_dir() {
        path.join("run.mcap")
    } else {
        path.to_path_buf()
    };
    let record_path = record_path
        .canonicalize()
        .with_context(|| format!("cannot open record {}", record_path.display()))?;
    ensure!(
        record_path.is_file(),
        "record is not a file: {}",
        record_path.display()
    );
    let record::Replay {
        frames,
        terrain,
        boundaries,
    } = record::read_replay(&record_path)?;

    // Keep this export only while the viewer process is open. Ordinary runs
    // have one durable output, run.mcap.
    let temp = tempfile::tempdir()?;
    let recording = temp.path().join("replay.rrd");
    let mut viewer = Viewer::new(Some(&recording), false, "ripple-replay", false)?;
    if let Some(terrain) = terrain {
        viewer.log_terrain(&terrain)?;
    }
    viewer.log_boundaries(&boundaries)?;
    for (index, frame) in frames.iter().enumerate() {
        viewer.log_frame(index, frame)?;
    }
    if draw_trails {
        viewer.log_saved_trails(&frames)?;
    }
    viewer.finish()?;

    let status = Command::new("rerun").arg(&recording).status().context(
        "could not launch Rerun; install the matching viewer and ensure rerun is on PATH",
    )?;
    ensure!(status.success(), "Rerun viewer exited with {status}");
    Ok(())
}

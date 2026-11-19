//! Rebuild a Rerun view from a saved MCAP record without running simulation.

use std::{path::Path, process::Command};

use anyhow::{Context, Result, bail, ensure};
use ripple_core::record;

use crate::demo::{Demo, Preset};
use crate::viewer::{Group, Viewer, ViewerSettings, covariance};

/// The largest `run.mcap` replay rebuilds. Replay decodes every frame and
/// writes a complete Rerun recording before the viewer opens, so the viewer
/// should be up in a few seconds or not at all; longer runs are viewed from
/// the export that `ripple run --export-rrd` writes during the run.
///
/// DEVNOTE (2026-11-07): measured on an Apple Silicon Mac, release build,
/// with a stand-in `rerun` that exits at once, so the number is decode and
/// write alone: 8.1 MB (6,001 frames) 0.3 s; 88.8 MB and 155 MB (306,001
/// frames each) 29 s; 267 MB (60,001 frames of 81 entities) about two
/// minutes at 2.3 GB resident. Repeat with `time ripple replay <run>`. The
/// cost follows frames, not bytes, but the size is in the file's metadata
/// and the frame count is not; at the worst rate measured, 16 MB keeps the
/// wait near five seconds, above the quick-check missions and below a day
/// of one hub. Streaming frames to the viewer or striding them were
/// considered and not done: the export written during the run already
/// serves the long runs, and a second viewer path is not worth carrying.
const LARGEST_RECORD_BYTES: u64 = 16 << 20;

pub(crate) fn replay(
    path: &Path,
    draw_trails: bool,
    draw_covariance: bool,
    demo: Option<&Path>,
    output: Option<&Path>,
) -> Result<()> {
    let demo = demo.map(Demo::load).transpose()?;
    ensure!(
        demo.is_none() == output.is_none(),
        "--demo writes the recording to --output; give both or neither"
    );
    // The belief preset draws the ellipsoids and the error plot.
    let draw_covariance = draw_covariance
        || demo
            .as_ref()
            .is_some_and(|demo| demo.preset == Preset::Belief);
    // An export opens at once whatever its size; trails, ellipsoids, and a
    // demo need the record.
    if path.is_dir()
        && path.join("recording.rrd").is_file()
        && !draw_trails
        && !draw_covariance
        && demo.is_none()
    {
        return open_in_rerun(&path.join("recording.rrd"));
    }
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
    check_size(record_path.metadata()?.len(), &record_path)?;
    let record::Replay {
        frames,
        terrain,
        beliefs,
        groups,
        fixes,
    } = record::read_replay(&record_path)?;
    let beliefs = if draw_covariance {
        Some(beliefs.with_context(|| {
            format!(
                "{} has no beliefs to draw; record the run with --trace",
                record_path.display()
            )
        })?)
    } else {
        None
    };

    // Kept only while the viewer is open: a run's one durable output is
    // run.mcap. A demo's recording goes where --output says instead.
    let temp = tempfile::tempdir()?;
    let recording = match output {
        Some(output) => output.to_path_buf(),
        None => temp.path().join("replay.rrd"),
    };
    let groups = groups
        .into_iter()
        .map(|(label, visual_model)| Group {
            label,
            visual_model,
            // A record does not say how many a group was asked for; name
            // every member by number.
            count: 0,
        })
        .collect();
    let is_demo = demo.is_some();
    let mut viewer = Viewer::new(ViewerSettings {
        export: Some(&recording),
        launch: false,
        recording_id: "ripple-replay",
        draw_trails: false,
        draw_covariance,
        groups,
        demo,
    })?;
    if let Some(terrain) = terrain {
        viewer.log_terrain(&terrain)?;
    }
    for (index, frame) in frames.iter().enumerate() {
        viewer.log_frame(index, frame)?;
        if let Some(beliefs) = &beliefs
            && viewer.drew_last_frame()
        {
            viewer.log_beliefs(&covariance::from_beliefs(&beliefs[index]))?;
            viewer.log_belief_errors(&beliefs[index])?;
            if let Some(fixes) = &fixes {
                viewer.log_fixes(&fixes[index])?;
            }
        }
    }
    if draw_trails {
        viewer.log_saved_trails(&frames)?;
    }
    viewer.finish()?;
    if is_demo {
        println!("{} frames; recording {}", frames.len(), recording.display());
        return Ok(());
    }
    open_in_rerun(&recording)
}

/// Refuse a record over the limit, saying what to do instead.
fn check_size(bytes: u64, record_path: &Path) -> Result<()> {
    if bytes <= LARGEST_RECORD_BYTES {
        return Ok(());
    }
    bail!(
        "{} is {} MB, over replay's {} MB limit. Replay decodes every frame \
         and writes a complete Rerun recording before the viewer opens; at \
         this size that takes minutes and gigabytes of memory, or fails. Run \
         the mission again with `ripple run --export-rrd`, which writes \
         recording.rrd beside run.mcap during the run, then replay that run \
         directory or open the file with `rerun`.",
        record_path.display(),
        bytes >> 20,
        LARGEST_RECORD_BYTES >> 20
    )
}

fn open_in_rerun(recording: &Path) -> Result<()> {
    let status = Command::new("rerun").arg(recording).status().context(
        "could not launch Rerun; install the matching viewer and ensure rerun is on PATH",
    )?;
    ensure!(status.success(), "Rerun viewer exited with {status}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_over_16_mb_is_refused_with_the_export_advice() {
        let path = Path::new("runs/region-16/run000/run.mcap");
        assert!(check_size(LARGEST_RECORD_BYTES, path).is_ok());
        let error = check_size(267 << 20, path).unwrap_err().to_string();
        assert!(
            error.contains("267 MB, over replay's 16 MB limit"),
            "{error}"
        );
        assert!(error.contains("--export-rrd"), "{error}");
    }
}

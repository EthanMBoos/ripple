//! Rebuild a Rerun view from a saved MCAP record without running simulation.

use std::{path::Path, process::Command};

use anyhow::{Context, Result, bail, ensure};
use ripple_core::record;

use crate::viewer::Viewer;

/// The largest `run.mcap` replay rebuilds. Replay decodes every frame and
/// writes a complete Rerun recording before the viewer opens, so a long run
/// keeps the user waiting with nothing on screen; the viewer should be up in
/// a few seconds or not at all. Longer runs are viewed from the export that
/// `ripple run --export-rrd` writes during the run.
///
/// DEVNOTE (2026-11-07): the limit, and why it is a hard error.
///
/// Measured on an Apple Silicon Mac, release build, with a stand-in `rerun`
/// on PATH that only reports the recording and exits, so the number is the
/// decode and write alone: 1.5 MB (1 entity) 0.3 s; 8.1 MB (6,001 frames)
/// 0.3 s; 88.8 MB (306,001 frames) 29 s; 155 MB (306,001 frames) 29 s;
/// 267 MB (60,001 frames of 81 entities) about two minutes at 2.3 GB
/// resident, writing a 1.5 GB recording. Repeat with the same stand-in and
/// `time ripple replay <run>`.
///
/// Why bytes: the cost follows frames and entities, not bytes alone (the
/// 88.8 MB and 155 MB records took the same time), but the frame count is
/// not known without reading the file, which is the work being avoided,
/// while the size is in the file's metadata. At the worst measured rate,
/// 0.33 s per MB, 16 MB keeps the wait near five seconds; the quick-check
/// and verification missions are under 10 MB and a day of one hub is about
/// 100 MB, so the line falls between the runs replay is for and the runs
/// it is not.
///
/// Why refuse rather than make replay fast: three fixes were considered.
/// Streaming frames to a spawned viewer as they decode would put the
/// window up at once, as `ripple run --viewer` does, but the minutes of
/// decoding and the memory stay; a frame stride would make a day-long run
/// ten times smaller at the cost of a view that is not the record; logging
/// never-changing components once would shrink the recording by an amount
/// not measured. None is needed, because the export written during the run
/// already gives the long runs a recording the viewer streams in, and the
/// runs anyone scrubs through are one-offs that can be run again with
/// `--export-rrd`. A simulator for students should say so plainly rather
/// than carry a second viewer path, so replay states the limit and the
/// export route and stops.
const LARGEST_RECORD_BYTES: u64 = 16 << 20;

pub(crate) fn replay(path: &Path, draw_trails: bool) -> Result<()> {
    // An export written during the run opens at once; the viewer streams
    // it in, whatever its size, so this is the route for long runs. Trails
    // are rebuilt from the frames, so they need the record and its limit.
    if path.is_dir() && path.join("recording.rrd").is_file() && !draw_trails {
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
    let record::Replay { frames, terrain } = record::read_replay(&record_path)?;

    // Keep this export only while the viewer process is open. Ordinary runs
    // have one durable output, run.mcap.
    let temp = tempfile::tempdir()?;
    let recording = temp.path().join("replay.rrd");
    let mut viewer = Viewer::new(Some(&recording), false, "ripple-replay", false)?;
    if let Some(terrain) = terrain {
        viewer.log_terrain(&terrain)?;
    }
    for (index, frame) in frames.iter().enumerate() {
        viewer.log_frame(index, frame)?;
    }
    if draw_trails {
        viewer.log_saved_trails(&frames)?;
    }
    viewer.finish()?;
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

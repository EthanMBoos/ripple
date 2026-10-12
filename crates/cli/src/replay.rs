//! Open an existing recording; replay never constructs or advances a simulation.

use std::{fs, net::TcpListener, path::Path, process::Command, time::Duration};

use anyhow::{Context, Result, ensure};
use rerun::{RecordingStreamBuilder, sink::GrpcSink};

pub(crate) fn replay(path: &Path, draw_trails: bool) -> Result<()> {
    let recording = if path.is_dir() {
        path.join("recording.rrd")
    } else {
        path.to_path_buf()
    };
    // An absolute path is also safe for filenames beginning with a dash.
    let recording = recording
        .canonicalize()
        .with_context(|| format!("cannot open recording {}", recording.display()))?;
    ensure!(
        fs::metadata(&recording)?.is_file(),
        "recording is not a file: {}",
        recording.display()
    );
    if draw_trails {
        return replay_with_trails(&recording);
    }

    let status = Command::new("rerun").arg(&recording).status().context(
        "could not launch Rerun; install the matching viewer and ensure rerun is on PATH",
    )?;
    ensure!(status.success(), "Rerun viewer exited with {status}");
    Ok(())
}

/// Open the recording in a viewer that also listens for a live stream, then
/// send it trails rebuilt from the run's frames under the recording's own ID,
/// so the viewer merges them into the same recording.
fn replay_with_trails(recording: &Path) -> Result<()> {
    let run = recording.parent().context("recording has no folder")?;
    let manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(run.join("manifest.json"))
            .context("--draw-trails needs the run's manifest.json beside the recording")?,
    )?;
    let recording_id = manifest["rerun_recording_id"].as_str().context(
        "this run's manifest has no rerun_recording_id (recorded before --draw-trails); run the mission again",
    )?;
    let frames = ripple_core::read_frames(&run.join("frames.bin"))
        .context("--draw-trails needs the run's frames.bin beside the recording")?;

    let port = TcpListener::bind("127.0.0.1:0")?.local_addr()?.port();
    let mut viewer = Command::new("rerun")
        .arg("--port")
        .arg(port.to_string())
        .arg(recording)
        .spawn()
        .context(
            "could not launch Rerun; install the matching viewer and ensure rerun is on PATH",
        )?;
    let trails = RecordingStreamBuilder::new("ripple")
        .recording_id(recording_id)
        .set_sinks(vec![Box::new(GrpcSink::new(
            format!("rerun+http://127.0.0.1:{port}/proxy").parse()?,
        )) as Box<dyn rerun::sink::LogSink>])?;
    crate::viewer::draw_trails(&trails, &frames)?;
    trails.flush_with_timeout(Duration::from_secs(120))?;
    drop(trails);

    let status = viewer.wait()?;
    ensure!(status.success(), "Rerun viewer exited with {status}");
    Ok(())
}

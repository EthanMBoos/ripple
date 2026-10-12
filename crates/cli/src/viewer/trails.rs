//! Trails for `--draw-trails`: each agent's path over the last `TRAIL_S`
//! seconds, on the recording's own timelines. `ripple run --viewer` sends
//! them only to the live viewer, and `ripple replay` rebuilds them from a
//! run's frames. They are never saved: redrawing every agent's trail every
//! frame made large recorded runs much slower (see `draw`).
use anyhow::Result;
use rerun::RecordingStream;
use ripple_core::SimulationFrame;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::{map, playback_time_ns, team_color};

const TRAIL_S: f64 = 22.0;
/// Each trail keeps every nth point to fit this many.
const MAX_TRAIL_POINTS: usize = 100;

/// Log every agent's trail at every frame, with the same `frame` and `time`
/// values the run's recording used, so they line up with it.
///
/// DEVNOTE: recording perf.py's `motion` workload at 512 aircraft (2,000
/// frames, 8 workers) with `ripple run --headless` on an M2 Pro took 23-25 s
/// without trails and 38-41 s with them redrawn every frame, so trails are not
/// recorded. Fewer points (37-40 s), one log call per view for all
/// trails (52-53 s), and redrawing once a second (28-29 s, visibly jumpy) did
/// not help enough.
pub(crate) fn draw(recording: &RecordingStream, frames: &[SimulationFrame]) -> Result<()> {
    let mut trails = Trails::default();
    let first_time_s = frames.first().map_or(0.0, |frame| frame.time_s);
    let mut last_time_ns = None;
    for (index, frame) in frames.iter().enumerate() {
        let time_ns = playback_time_ns(frame.time_s - first_time_s, last_time_ns);
        last_time_ns = Some(time_ns);
        recording.set_time_sequence("frame", i64::try_from(index)?);
        recording.set_duration_secs("time", time_ns as f64 * 1e-9);
        trails.log_frame(recording, frame)?;
    }
    Ok(())
}

/// Each live agent's positions over the last `TRAIL_S` seconds, with times.
#[derive(Default)]
pub(super) struct Trails {
    tracks: BTreeMap<i32, VecDeque<(f64, [f32; 3])>>,
}

impl Trails {
    /// Log each agent's trail as of this frame, at the recording's current time.
    pub(super) fn log_frame(
        &mut self,
        recording: &RecordingStream,
        frame: &SimulationFrame,
    ) -> Result<()> {
        // A removed agent's trail goes with it.
        let current: BTreeSet<_> = frame.entities.iter().map(|entity| entity.id).collect();
        let removed: Vec<_> = self
            .tracks
            .keys()
            .filter(|id| !current.contains(id))
            .copied()
            .collect();
        for id in removed {
            self.tracks.remove(&id);
            recording.log(format!("map/tracks/{id}"), &rerun::Clear::flat())?;
            recording.log(format!("world/tracks/{id}"), &rerun::Clear::flat())?;
        }
        for entity in &frame.entities {
            let position = entity.truth.position_world_m;
            let track = self.tracks.entry(entity.id).or_default();
            track.push_back((
                frame.time_s,
                [position.x as f32, position.y as f32, position.z as f32],
            ));
            while track
                .front()
                .is_some_and(|&(time_s, _)| time_s < frame.time_s - TRAIL_S)
            {
                track.pop_front();
            }
            let trail = trail(track);
            let [red, green, blue] = team_color(entity.team_id);
            let color = [red, green, blue, 140];
            recording.log(
                format!("map/tracks/{}", entity.id),
                &rerun::LineStrips2D::new([trail.iter().map(|point| map::project(*point))])
                    .with_colors([color])
                    .with_radii([rerun::Radius::new_ui_points(1.0)]),
            )?;
            recording.log(
                format!("world/tracks/{}", entity.id),
                &rerun::LineStrips3D::new([trail])
                    .with_colors([color])
                    .with_radii([rerun::Radius::new_ui_points(1.0)]),
            )?;
        }
        Ok(())
    }
}

/// A track's points, keeping every nth to fit `MAX_TRAIL_POINTS`, and always
/// ending at the latest position.
fn trail(track: &VecDeque<(f64, [f32; 3])>) -> Vec<[f32; 3]> {
    let step = track.len().div_ceil(MAX_TRAIL_POINTS);
    let mut trail: Vec<_> = track
        .iter()
        .step_by(step)
        .map(|&(_, point)| point)
        .collect();
    if !(track.len() - 1).is_multiple_of(step) {
        trail.extend(track.back().map(|&(_, point)| point));
    }
    trail
}

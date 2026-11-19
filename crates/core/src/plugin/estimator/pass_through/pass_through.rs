//! Copies one complete measurement into belief, with no model of its own.
//!
//! Both estimator slots default to the name `PassThrough`: `PassThroughState`
//! in `localization` copies a state reading, mean and covariance, into the
//! own-state belief, and `PassThroughTracks` in `tracking` makes one track per
//! detection in a detections reading. Two structs because each names the
//! readings it needs in its `inputs`. With the perfect measurement belief
//! equals truth, with zero covariance, at the next decision; between a slow
//! sensor's readings the last one is kept. They are the defaults so a mission
//! can study sensor noise with no filter in the way; swap in a predicting
//! estimator, such as the starter crate's PositionKalman, when the estimator
//! is what is under study.

use anyhow::{Context, Result};

use crate::GaussianState;
use crate::plugin::{
    DetectionList, Input, Localization, LocalizationContext, ReadingKind, Track, TrackList,
    Tracking, TrackingContext,
};

pub struct PassThroughState;

impl Localization for PassThroughState {
    type Config = ();

    fn new(_config: &()) -> Self {
        Self
    }

    fn inputs(_config: &()) -> Vec<Input> {
        vec![Input::new("state", ReadingKind::State)]
    }

    fn initialize(&mut self, context: &mut LocalizationContext) -> Result<()> {
        // The initial belief must come from a reading; there is no fallback to truth.
        let reading = context
            .inputs
            .latest::<GaussianState>("state")?
            .context("the state sensor gave no reading at startup")?;
        *context.state = *reading.value;
        Ok(())
    }

    fn step(&mut self, context: &mut LocalizationContext) -> Result<()> {
        if let Some(reading) = context.inputs.fresh::<GaussianState>("state")? {
            *context.state = *reading.value;
        }
        Ok(())
    }
}

/// One track per detection, with no association, initiation, or deletion: a
/// detection is believed with certainty for as long as it is reported, and a
/// contact that stops being detected is dropped at once. The track id is the
/// detected entity's id, so the sensor must identify what it detects; a
/// tracker of unidentified detections has to assign and keep its own ids,
/// which is a different plugin. Fed the perfect measurement, it shares the
/// engine's one track list rather than building the same list per entity.
pub struct PassThroughTracks;

impl Tracking for PassThroughTracks {
    type Config = ();

    fn new(_config: &()) -> Self {
        Self
    }

    fn inputs(_config: &()) -> Vec<Input> {
        vec![Input::new("detections", ReadingKind::Detections)]
    }

    fn initialize(&mut self, context: &mut TrackingContext) -> Result<()> {
        // No reading yet means no tracks.
        self.step(context).map(|_| ())
    }

    fn step(&mut self, context: &mut TrackingContext) -> Result<()> {
        if let Some(tracks) = context.inputs.perfect_tracks("detections")? {
            *context.tracks = tracks.clone();
            return Ok(());
        }
        if let Some(reading) = context.inputs.fresh::<DetectionList>("detections")? {
            let tracks = reading
                .value
                .iter()
                .map(|detection| Track::believing(detection, reading.sampled_at_s))
                .collect::<Result<Vec<Track>>>()?;
            *context.tracks = TrackList::from(tracks);
        }
        Ok(())
    }
}

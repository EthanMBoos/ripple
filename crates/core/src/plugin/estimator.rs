//! Estimators: the localization and tracking interfaces, and the built-ins.
//!
//! Each entity has one localization estimator (its own state) and one tracking
//! estimator (the tracks it keeps on other entities). They run in the
//! estimation step at the end of each tick, localization first, and their
//! results are the belief that autonomy and controllers read in the next tick.
//! Estimators never see truth: they get the readings their mission `inputs`
//! select, messages, the commands the motion model read, and time.
//!
//! The types follow the estimation and tracking literature. A belief is a
//! `GaussianState`, a mean with the covariance of its error. A contact sensor
//! publishes a `Detection` per entity it measures, with the covariance of its
//! measurement; a tracker turns detections into a `Track` per object it
//! believes in, with an id of its own, an existence probability, and the time
//! of the last detection behind it.
use std::{any::Any, sync::Arc};

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use vstd::prelude::*;

use crate::{
    EntitySnapshot, GaussianState, Vec3,
    common::{PluginRandom, is_within},
    drawing::Drawing,
    plugin::sensor::{ReadingKind, Readings},
    plugin::{EntityInfo, PluginParams, StepTime},
    pubsub::Messages,
};

/// Who a detection or track is, when the sensor can tell: a cooperative
/// sensor (an ADS-B or AIS receiver, or every stock sensor) reports it; a
/// radar-like sensor would not, and the tracker then has only its track id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Identity {
    pub id: i32,
    pub team_id: i32,
    pub group_index: i32,
}
impl Identity {
    pub(crate) fn of(snapshot: &EntitySnapshot) -> Self {
        Self {
            id: snapshot.id,
            team_id: snapshot.team_id,
            group_index: snapshot.group_index,
        }
    }
}

/// One entity as a contact sensor measured it: the measured state with the
/// covariance of the measurement, and the identity if the sensor knows it.
#[derive(Clone, Copy, Debug)]
pub struct Detection {
    pub state: GaussianState,
    pub identity: Option<Identity>,
}
impl Detection {
    /// The perfect measurement of an entity: its truth, known exactly.
    pub(crate) fn perfect(snapshot: &EntitySnapshot) -> Self {
        Self {
            state: GaussianState::perfect(snapshot.truth),
            identity: Some(Identity::of(snapshot)),
        }
    }
}

/// A contact sensor's reading: every entity it detected this tick, in the
/// order the sensor chose.
pub type DetectionList = Arc<[Detection]>;

/// One object a tracker believes in.
#[derive(Clone, Copy, Debug)]
pub struct Track {
    /// The tracker's own label for this object, stable while the track lives.
    pub track_id: u32,
    /// Who it is, when a detection behind it said so.
    pub identity: Option<Identity>,
    pub state: GaussianState,
    /// When the last detection behind this estimate was taken.
    pub updated_at_s: f64,
    /// How sure the tracker is that the object exists, 0 to 1. A tracker
    /// that keeps a track through missed detections lowers it.
    pub existence_probability: f64,
}

impl Track {
    /// A track that believes `detection` outright: the detected entity's id
    /// as the track id, its state as given, and existence 1.0. A detection
    /// with no identity has no id to take; a tracker of those assigns its own.
    pub fn believing(detection: &Detection, updated_at_s: f64) -> Result<Self> {
        let identity = detection
            .identity
            .context("a detection without an identity needs a tracker that assigns track ids")?;
        Ok(Self {
            track_id: identity.id as u32,
            identity: Some(identity),
            state: detection.state,
            updated_at_s,
            existence_probability: 1.0,
        })
    }
}

/// A read-only list of tracks, cheap to hand from a tracker to belief.
pub type TrackList = Arc<[Track]>;

/// The tracks an entity believes in, as autonomy and controllers see them.
/// A tracker's list may include the entity itself; this view leaves it out.
#[derive(Clone, Copy)]
pub struct KnownTracks<'a> {
    list: &'a [Track],
    own_id: i32,
}
impl<'a> KnownTracks<'a> {
    pub(crate) fn new(list: &'a TrackList, own_id: i32) -> Self {
        Self { list, own_id }
    }
    pub fn iter(&self) -> impl Iterator<Item = &'a Track> + 'a {
        let own_id = self.own_id;
        self.list
            .iter()
            .filter(move |track| track.identity.is_none_or(|identity| identity.id != own_id))
    }
    /// The track whose identity is entity `id`, if the tracker has one.
    pub fn get(&self, id: i32) -> Option<&'a Track> {
        self.iter()
            .find(|track| track.identity.is_some_and(|identity| identity.id == id))
    }
    /// Known tracks believed closer than `range_m` to `position_world_m`, in
    /// list order. The same test as the engine's truth queries, over belief;
    /// the list is one entity's own, so it is scanned.
    pub fn within(&self, position_world_m: Vec3, range_m: f64) -> Vec<&'a Track> {
        self.iter()
            .filter(|track| is_within(position_world_m, track.state.mean.position_world_m, range_m))
            .collect()
    }
}

/// One named input an estimator reads, and the kind of reading it needs.
#[derive(Clone, Debug)]
pub struct Input {
    pub(crate) role: String,
    pub(crate) kind: ReadingKind,
}
impl Input {
    pub fn new(role: impl Into<String>, kind: ReadingKind) -> Self {
        Self {
            role: role.into(),
            kind,
        }
    }
}

/// Where an input's readings come from, fixed when the mission loads.
#[derive(Clone, Debug)]
pub(crate) enum Source {
    /// The engine's perfect measurement of this kind (state or detections),
    /// taken after interactions.
    Perfect(ReadingKind),
    /// A sensor, by instance name.
    Sensor(String),
}

/// A reading selected for one input.
pub struct Sample<'a, T> {
    pub sampled_at_s: f64,
    pub value: &'a T,
    /// Delivered since the last estimation step, rather than kept from earlier.
    pub fresh: bool,
}

verus! {

// These five sit in the Verus block so the proof of the estimation step can
// read what the engine put in them; the plugin methods on them are not read.

/// The readings this estimator's inputs select, looked up by input name.
pub struct Inputs<'a> {
    pub(crate) sources: &'a [(String, Source)],
    pub(crate) readings: &'a Readings,
    pub(crate) perfect_state: &'a GaussianState,
    pub(crate) perfect_contacts: &'a PerfectContacts,
    pub(crate) now_s: f64,
}

/// The engine's perfect measurement of the population, built once per tick
/// after the interactions and shared by every entity: each entity still in
/// play, known exactly, as the detections a tracker builds its own tracks
/// from and as the tracks a tracker that believes the measurement shares.
/// Sharing keeps the default, sensorless entity at constant work; one copy
/// per entity would cost the population squared every tick for the same
/// zeros.
pub struct PerfectContacts {
    pub(crate) detections: DetectionList,
    pub(crate) tracks: TrackList,
}
#[verifier::external]
impl<'a> Inputs<'a> {
    /// The latest reading for `input`, or `None` before its sensor's first reading.
    pub fn latest<T: Send + Sync + 'static>(&self, input: &str) -> Result<Option<Sample<'a, T>>> {
        let (_, source) = self
            .sources
            .iter()
            .find(|(role, _)| role == input)
            .with_context(|| format!("estimator has no input named '{input}'"))?;
        match source {
            Source::Perfect(kind) => {
                // Answer only with the declared kind, so code that asks for the
                // wrong type fails here as it would with a sensor.
                let perfect: &'a dyn Any = match kind {
                    ReadingKind::State => self.perfect_state,
                    _ => &self.perfect_contacts.detections,
                };
                let value = perfect.downcast_ref::<T>().with_context(|| {
                    format!(
                        "input '{input}' gives {kind:?} readings, not a {}",
                        std::any::type_name::<T>()
                    )
                })?;
                Ok(Some(Sample {
                    sampled_at_s: self.now_s,
                    value,
                    fresh: true,
                }))
            }
            Source::Sensor(sensor) => Ok(self.readings.get::<T>(sensor)?.map(|reading| Sample {
                sampled_at_s: reading.sampled_at_s,
                value: &reading.value,
                fresh: self.readings.is_fresh(sensor),
            })),
        }
    }
    /// The reading for `input` only if it arrived since the last estimation step.
    pub fn fresh<T: Send + Sync + 'static>(&self, input: &str) -> Result<Option<Sample<'a, T>>> {
        Ok(self.latest(input)?.filter(|sample| sample.fresh))
    }
    /// The engine's perfect tracks, when `input` is fed by the perfect
    /// measurement rather than a sensor. A tracker that believes the
    /// measurement outright shares them (an `Arc` clone) instead of building
    /// the same list for every entity.
    pub fn perfect_tracks(&self, input: &str) -> Result<Option<&'a TrackList>> {
        let (_, source) = self
            .sources
            .iter()
            .find(|(role, _)| role == input)
            .with_context(|| format!("estimator has no input named '{input}'"))?;
        Ok(match source {
            Source::Perfect(ReadingKind::Detections) => Some(&self.perfect_contacts.tracks),
            _ => None,
        })
    }
}


/// The commands the motion model read on each substep of the tick just finished.
pub struct Commands<'a> {
    pub(crate) names: &'a [String],
    /// `names.len()` values per substep.
    pub(crate) values: &'a [f64],
    pub(crate) times: &'a [StepTime],
}
#[verifier::external]
impl Commands<'_> {
    pub fn substeps(&self) -> usize {
        self.times.len()
    }
    pub fn time(&self, substep: usize) -> StepTime {
        self.times[substep]
    }
    /// The value the motion model read from `port` on `substep`.
    pub fn get(&self, substep: usize, port: &str) -> Result<f64> {
        let index = self
            .names
            .iter()
            .position(|name| name == port)
            .with_context(|| format!("the motion model has no input '{port}'"))?;
        Ok(self.values[substep * self.names.len() + index])
    }
}

/// `time` is the tick just finished; the estimate is for `time.time_s + time.dt_s`.
pub struct LocalizationContext<'a> {
    pub entity: EntityInfo,
    pub time: StepTime,
    pub inputs: Inputs<'a>,
    /// Empty during initialization.
    pub commands: Commands<'a>,
    pub messages: &'a mut Messages,
    /// This estimator's random numbers at this tick.
    pub random: PluginRandom,
    /// The entity's own-state belief, mean and covariance. It persists
    /// between updates.
    pub state: &'a mut GaussianState,
}

/// `time` is the tick just finished; the estimate is for `time.time_s + time.dt_s`.
pub struct TrackingContext<'a> {
    pub entity: EntityInfo,
    pub time: StepTime,
    pub inputs: Inputs<'a>,
    pub messages: &'a mut Messages,
    /// This estimator's random numbers at this tick.
    pub random: PluginRandom,
    /// Localization's estimate for this tick, for turning relative readings into positions.
    pub pose: &'a GaussianState,
    /// The tracks the entity keeps. They persist between updates.
    pub tracks: &'a mut TrackList,
}

} // verus!

/// Maintains the entity's own state from readings and the commands it issued.
pub trait Localization: Send {
    // The build half; see the module doc in `plugin.rs`.
    /// Immutable, validated configuration, parsed once per mission and shared
    /// by every instance.
    type Config: DeserializeOwned + Send + Sync + 'static
    where
        Self: Sized;
    /// Parses the mission's parameters; override it to check them.
    fn configure(params: &PluginParams) -> Result<Self::Config>
    where
        Self: Sized,
    {
        params.parse()
    }
    fn new(config: &Self::Config) -> Self
    where
        Self: Sized;
    /// The readings it reads; a mission's `inputs` selects each one's sensor.
    fn inputs(_config: &Self::Config) -> Vec<Input>
    where
        Self: Sized,
    {
        Vec::new()
    }

    /// Set the initial belief, from the startup readings, before any decision runs.
    fn initialize(&mut self, context: &mut LocalizationContext) -> Result<()>;
    fn step(&mut self, context: &mut LocalizationContext) -> Result<()>;
    fn close(&mut self, _time: StepTime) -> Result<()> {
        Ok(())
    }
    /// Shapes this plugin draws in the viewer for the whole run, asked once
    /// when it starts: see `Drawing`. Empty unless overridden.
    fn fixed_drawings(&self) -> Vec<Drawing> {
        Vec::new()
    }
    /// Shapes that come and go, asked every tick at `time_s`. Empty unless
    /// overridden.
    fn drawings(&self, _time_s: f64) -> Vec<Drawing> {
        Vec::new()
    }
}

/// Maintains the tracks the entity keeps on other entities, using
/// localization's pose.
pub trait Tracking: Send {
    // The build half; see the module doc in `plugin.rs`.
    /// Immutable, validated configuration, parsed once per mission and shared
    /// by every instance.
    type Config: DeserializeOwned + Send + Sync + 'static
    where
        Self: Sized;
    /// Parses the mission's parameters; override it to check them.
    fn configure(params: &PluginParams) -> Result<Self::Config>
    where
        Self: Sized,
    {
        params.parse()
    }
    fn new(config: &Self::Config) -> Self
    where
        Self: Sized;
    /// The readings it reads; a mission's `inputs` selects each one's sensor.
    fn inputs(_config: &Self::Config) -> Vec<Input>
    where
        Self: Sized,
    {
        Vec::new()
    }

    fn initialize(&mut self, _context: &mut TrackingContext) -> Result<()> {
        Ok(())
    }
    fn step(&mut self, context: &mut TrackingContext) -> Result<()>;
    fn close(&mut self, _time: StepTime) -> Result<()> {
        Ok(())
    }
    /// Shapes this plugin draws in the viewer for the whole run, asked once
    /// when it starts: see `Drawing`. Empty unless overridden.
    fn fixed_drawings(&self) -> Vec<Drawing> {
        Vec::new()
    }
    /// Shapes that come and go, asked every tick at `time_s`. Empty unless
    /// overridden.
    fn drawings(&self, _time_s: f64) -> Vec<Drawing> {
        Vec::new()
    }
}

// Built-ins selected by missions.
#[path = "estimator/pass_through/pass_through.rs"]
mod pass_through;
pub use pass_through::{PassThroughState, PassThroughTracks};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::KinematicState;

    fn track(id: Option<i32>, x_m: f64) -> Track {
        Track {
            track_id: 1,
            identity: id.map(|id| Identity {
                id,
                team_id: 1,
                group_index: 0,
            }),
            state: GaussianState::perfect(KinematicState {
                position_world_m: Vec3::new(x_m, 0.0, 0.0),
                ..KinematicState::default()
            }),
            updated_at_s: 0.0,
            existence_probability: 1.0,
        }
    }

    #[test]
    fn known_tracks_within_scans_belief_and_leaves_out_self() {
        let list = TrackList::from([
            track(Some(1), 0.0),
            track(Some(2), 4.0),
            track(Some(3), 5.0),
        ]);
        let known = KnownTracks::new(&list, 1);
        let ids: Vec<_> = known
            .within(Vec3::zeros(), 5.0)
            .iter()
            .map(|track| track.identity.unwrap().id)
            .collect();
        // Own entry left out; 5 m away is not within 5 m.
        assert_eq!(ids, vec![2]);
        assert!(known.get(1).is_none());
        assert!(known.get(2).is_some());
    }

    #[test]
    fn a_track_without_an_identity_is_known_and_never_mistaken_for_self() {
        let list = TrackList::from([track(None, 1.0), track(Some(1), 0.0)]);
        let known = KnownTracks::new(&list, 1);
        assert_eq!(known.iter().count(), 1);
        assert!(known.get(1).is_none());
    }

    #[test]
    fn a_believed_track_takes_the_detected_entity_id_and_needs_one() {
        let identified = Detection {
            state: GaussianState::perfect(KinematicState::default()),
            identity: Some(Identity {
                id: 7,
                team_id: 2,
                group_index: 0,
            }),
        };
        let track = Track::believing(&identified, 1.5).expect("identified");
        assert_eq!(track.track_id, 7);
        assert_eq!(track.updated_at_s, 1.5);
        assert_eq!(track.existence_probability, 1.0);
        let unidentified = Detection {
            identity: None,
            ..identified
        };
        assert!(Track::believing(&unidentified, 1.5).is_err());
    }
}

//! Estimators: the localization and tracking interfaces, and the built-ins.
//!
//! Each entity has one localization estimator (its own state) and one tracking
//! estimator (the contacts it knows about). They run in the estimation step at
//! the end of each tick, localization first, and their results are the belief
//! that autonomy and controllers read in the next tick. Estimators never see
//! truth: they get the readings their mission `inputs` select, messages, the
//! commands the motion model read, and time.
use std::{any::Any, sync::Arc};

use anyhow::{Context, Result};

use crate::{
    EntitySnapshot, KinematicState, Vec3,
    common::{PluginRandom, is_within},
    plugin::sensor::{ReadingKind, Readings},
    plugin::{EntityInfo, StepTime, Update},
    pubsub::Messages,
};

/// One contact as a tracker knows it.
#[derive(Clone, Debug)]
pub struct Contact {
    pub id: i32,
    pub team_id: i32,
    pub group_index: i32,
    pub state: KinematicState,
    /// When the reading behind this estimate was taken.
    pub sampled_at_s: f64,
}
impl Contact {
    pub(crate) fn measured(snapshot: &EntitySnapshot, sampled_at_s: f64) -> Self {
        Self {
            id: snapshot.id,
            team_id: snapshot.team_id,
            group_index: snapshot.group_index,
            state: snapshot.truth.clone(),
            sampled_at_s,
        }
    }
}

/// A read-only list of contacts, cheap to hand from a reading to belief.
pub type ContactList = Arc<[Contact]>;

/// The contacts an entity believes in, as autonomy and controllers see them.
/// A tracker's list may include the entity itself; this view leaves it out.
#[derive(Clone, Copy)]
pub struct KnownContacts<'a> {
    list: &'a [Contact],
    own_id: i32,
}
impl<'a> KnownContacts<'a> {
    pub(crate) fn new(list: &'a [Contact], own_id: i32) -> Self {
        Self { list, own_id }
    }
    pub fn iter(&self) -> impl Iterator<Item = &'a Contact> + 'a {
        let own_id = self.own_id;
        self.list.iter().filter(move |contact| contact.id != own_id)
    }
    pub fn get(&self, id: i32) -> Option<&'a Contact> {
        self.iter().find(|contact| contact.id == id)
    }
    /// Known contacts believed closer than `range_m` to `position_world_m`, in
    /// list order. The same test as the engine's truth queries, over belief;
    /// the list is one entity's own, so it is scanned.
    pub fn within(&self, position_world_m: Vec3, range_m: f64) -> Vec<&'a Contact> {
        self.iter()
            .filter(|contact| is_within(position_world_m, contact.state.position_world_m, range_m))
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
    /// The engine's perfect measurement of this kind (state or contacts),
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

/// The readings this estimator's inputs select, looked up by input name.
pub struct Inputs<'a> {
    pub(crate) sources: &'a [(String, Source)],
    pub(crate) readings: &'a Readings,
    pub(crate) perfect_state: &'a KinematicState,
    pub(crate) perfect_contacts: &'a ContactList,
    pub(crate) now_s: f64,
}
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
                    _ => self.perfect_contacts,
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
}

/// The commands the motion model read on each substep of the tick just finished.
pub struct Commands<'a> {
    pub(crate) names: &'a [String],
    /// `names.len()` values per substep.
    pub(crate) values: &'a [f64],
    pub(crate) times: &'a [StepTime],
}
impl Commands<'_> {
    pub fn substeps(&self) -> usize {
        self.times.len()
    }
    /// The motion model's input port names, the order `values` uses.
    pub fn names(&self) -> &[String] {
        self.names
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
    /// The entity's own-state belief. It persists between updates.
    pub state: &'a mut KinematicState,
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
    pub pose: &'a KinematicState,
    /// The contacts the entity knows about. They persist between updates.
    pub contacts: &'a mut ContactList,
}

/// Maintains the entity's own state from readings and the commands it issued.
/// Its readings are declared by `Plugin::inputs`.
pub trait Localization: Send {
    /// Set the initial belief, from the startup readings, before any decision runs.
    fn initialize(&mut self, context: &mut LocalizationContext<'_>) -> Result<()>;
    fn step(&mut self, context: &mut LocalizationContext<'_>) -> Result<Update>;
    fn close(&mut self, _time: StepTime) -> Result<()> {
        Ok(())
    }
}

/// Maintains the contacts the entity knows about, using localization's pose.
/// Its readings are declared by `Plugin::inputs`.
pub trait Tracking: Send {
    fn initialize(&mut self, _context: &mut TrackingContext<'_>) -> Result<()> {
        Ok(())
    }
    fn step(&mut self, context: &mut TrackingContext<'_>) -> Result<Update>;
    fn close(&mut self, _time: StepTime) -> Result<()> {
        Ok(())
    }
}

// Built-ins selected by missions.
#[path = "estimator/pass_through/pass_through.rs"]
mod pass_through;
pub use pass_through::{PassThroughContacts, PassThroughState};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_contacts_within_scans_belief_and_leaves_out_self() {
        let contact = |id: i32, x_m: f64| Contact {
            id,
            team_id: 1,
            group_index: 0,
            state: KinematicState {
                position_world_m: Vec3::new(x_m, 0.0, 0.0),
                ..KinematicState::default()
            },
            sampled_at_s: 0.0,
        };
        let list = [contact(1, 0.0), contact(2, 4.0), contact(3, 5.0)];
        let known = KnownContacts::new(&list, 1);
        let ids: Vec<_> = known
            .within(Vec3::zeros(), 5.0)
            .iter()
            .map(|c| c.id)
            .collect();
        // Own entry left out; 5 m away is not within 5 m.
        assert_eq!(ids, vec![2]);
    }
}

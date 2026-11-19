use anyhow::Result;
use std::collections::{BTreeMap, BTreeSet};
use vstd::prelude::*;

pub(crate) mod proof;
// Verus skips this module; proof.rs states what its methods return.
#[cfg_attr(verus_only, verus_verify(external))]
mod any_reading;
pub use any_reading::AnyReading;

verus! {

broadcast use proof::reading_facts;

/// Each sensor's latest delivered reading, keyed by the sensor's instance name.
/// Plugins go through `get` and `is_fresh`. The struct sits in this block so
/// its fields stay crate-private and a proof can still read them.
#[derive(Default)]
pub struct Readings {
    pub(crate) values: BTreeMap<String, AnyReading>,
    /// Names delivered since the last estimation step.
    pub(crate) fresh: BTreeSet<String>,
}

/// One sensor's latest reading and the time it was taken.
pub struct Reading<T> {
    pub sampled_at_s: f64,
    pub value: T,
}

/// What a sensor's reading contains, so estimators can pick sources before a run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadingKind {
    /// A complete own-state measurement with its covariance: a `GaussianState`.
    State,
    /// Measured contacts, each with its covariance: a `DetectionList`.
    Detections,
    /// Anything else (a position fix, a range); estimators name these sources explicitly.
    Other,
}

impl Readings {
    /// Missing data is normal before a sensor's first reading; a wrong type is an error.
    pub fn get<T: Send + Sync + 'static>(
        &self,
        sensor: &str,
    ) -> (reading: Result<Option<&Reading<T>>>)
        ensures proof::got(self, sensor, &reading)
    {
        let Some(value) = self.values.get(sensor) else {
            return Ok(None);
        };
        Ok(Some(value.downcast_ref::<T>().ok_or_else(|| {
            anyhow::anyhow!("reading from '{sensor}' has a different type")
        })?))
    }
    /// Whether `sensor`'s reading was delivered since the last estimation step.
    pub fn is_fresh(&self, sensor: &str) -> (fresh: bool)
        ensures fresh == proof::is_fresh(self, sensor)
    {
        self.fresh.contains(sensor)
    }
    /// Stores the reading in the sensor's slot; `SensorContext::publish_reading`
    /// has checked its type against the sensor's declared `Reading`.
    pub(crate) fn publish<T: Send + Sync + 'static>(&mut self, sensor: &str, time_s: f64, value: T)
        ensures proof::published(old(self), final(self), sensor, time_s, value)
    {
        self.values.insert(
            sensor.to_owned(),
            AnyReading::new(Reading {
                sampled_at_s: time_s,
                value,
            }),
        );
    }
    /// Moves this tick's readings into their slots and marks their sensors fresh.
    pub(crate) fn deliver(&mut self, pending: &mut Self)
        ensures proof::delivered(old(self), final(self), old(pending), final(pending))
    {
        for sensor in it: pending.values.keys()
            invariant proof::marked_so_far(old(self), self, pending, it.seq(), it.index())
        {
            self.fresh.insert(sensor.clone());
        }
        self.values.append(&mut pending.values);
    }
    pub(crate) fn clear_fresh(&mut self)
        ensures proof::cleared(old(self), final(self))
    {
        self.fresh.clear();
    }
}

} // verus!

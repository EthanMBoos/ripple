use anyhow::Result;
use std::collections::{BTreeMap, BTreeSet};
use vstd::prelude::*;

mod proof;
// Verus skips this module; proof.rs states what its methods return.
#[cfg_attr(verus_only, verus_verify(external))]
mod any_reading;
pub use any_reading::AnyReading;

verus! {
    broadcast use proof::reading_facts;
}

/// One sensor's latest reading and the time it was taken.
pub struct Reading<T> {
    pub sampled_at_s: f64,
    pub value: T,
}

/// What a sensor's reading contains, so estimators can pick sources before a run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadingKind {
    /// A complete own-state measurement: a `KinematicState`.
    State,
    /// Measured contacts: a `ContactList`.
    Contacts,
    /// Anything else (a position fix, a range); estimators name these sources explicitly.
    Other,
}

/// Each sensor's latest delivered reading, keyed by the sensor's instance name.
/// Plugins go through `get` and `is_fresh`; the fields are public so a proof,
/// including a future proof of a plugin, can state what they hold.
#[derive(Default)]
pub struct Readings {
    pub values: BTreeMap<String, AnyReading>,
    /// Names delivered since the last estimation step.
    pub fresh: BTreeSet<String>,
}
impl Readings {
    /// Missing data is normal before a sensor's first reading; a wrong type is an error.
    #[verus_spec(reading => ensures proof::got(self, sensor, &reading))]
    pub fn get<T: Send + Sync + 'static>(&self, sensor: &str) -> Result<Option<&Reading<T>>> {
        let Some(value) = self.values.get(sensor) else {
            return Ok(None);
        };
        Ok(Some(value.downcast_ref::<T>().ok_or_else(|| {
            anyhow::anyhow!("reading from '{sensor}' has a different type")
        })?))
    }
    /// Whether `sensor`'s reading was delivered since the last estimation step.
    #[verus_spec(fresh => ensures fresh == proof::is_fresh(self, sensor))]
    pub fn is_fresh(&self, sensor: &str) -> bool {
        self.fresh.contains(sensor)
    }
    /// Stores the reading in the sensor's slot; `SensorContext::publish_reading`
    /// has checked its type against the sensor's declared `Reading`.
    #[verus_spec(ensures proof::published(old(self), final(self), sensor, time_s, value))]
    pub(crate) fn publish<T: Send + Sync + 'static>(
        &mut self,
        sensor: &str,
        time_s: f64,
        value: T,
    ) {
        self.values.insert(
            sensor.to_owned(),
            AnyReading::new(Reading {
                sampled_at_s: time_s,
                value,
            }),
        );
    }
    /// Moves this tick's readings into their slots and marks their sensors fresh.
    #[verus_spec(ensures proof::delivered(old(self), final(self), old(pending), final(pending)))]
    pub(crate) fn deliver(&mut self, pending: &mut Self) {
        #[cfg_attr(verus_only, verus_spec(it => invariant proof::marked_so_far(old(self), self, pending, it.seq(), it.index())))]
        for sensor in pending.values.keys() {
            self.fresh.insert(sensor.clone());
        }
        self.values.append(&mut pending.values);
    }
    #[verus_spec(ensures proof::cleared(old(self), final(self)))]
    pub(crate) fn clear_fresh(&mut self) {
        self.fresh.clear();
    }
}

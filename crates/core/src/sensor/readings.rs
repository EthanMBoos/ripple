use anyhow::{Result, ensure};
use std::{
    any::Any,
    collections::{BTreeMap, BTreeSet},
};

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
#[derive(Default)]
pub struct Readings {
    values: BTreeMap<String, Box<dyn Any + Send + Sync>>,
    /// Names delivered since the last estimation step.
    fresh: BTreeSet<String>,
}
impl Readings {
    /// Missing data is normal before a sensor's first reading; a wrong type is an error.
    pub fn get<T: Send + Sync + 'static>(&self, sensor: &str) -> Result<Option<&Reading<T>>> {
        let Some(value) = self.values.get(sensor) else {
            return Ok(None);
        };
        Ok(Some(value.downcast_ref::<Reading<T>>().ok_or_else(
            || anyhow::anyhow!("reading from '{sensor}' has a different type"),
        )?))
    }
    /// Whether `sensor`'s reading was delivered since the last estimation step.
    pub fn is_fresh(&self, sensor: &str) -> bool {
        self.fresh.contains(sensor)
    }
    pub(crate) fn publish<T: Send + Sync + 'static>(
        &mut self,
        sensor: &str,
        time_s: f64,
        value: T,
    ) -> Result<()> {
        if let Some(previous) = self.values.get(sensor) {
            ensure!(
                previous.is::<Reading<T>>(),
                "reading from '{sensor}' changed type"
            );
        }
        self.values.insert(
            sensor.to_owned(),
            Box::new(Reading {
                sampled_at_s: time_s,
                value,
            }),
        );
        Ok(())
    }
    pub(crate) fn deliver(&mut self, pending: &mut Self) -> Result<()> {
        for (sensor, value) in &pending.values {
            if let Some(previous) = self.values.get(sensor) {
                ensure!(
                    (**previous).type_id() == (**value).type_id(),
                    "reading from '{sensor}' changed type"
                );
            }
            self.fresh.insert(sensor.clone());
        }
        self.values.append(&mut pending.values);
        Ok(())
    }
    pub(crate) fn clear_fresh(&mut self) {
        self.fresh.clear();
    }
}

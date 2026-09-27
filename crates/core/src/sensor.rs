//! Sensor interface and reading delivery; concrete models live in plugin/sensor.
use crate::{
    EntitySnapshot, KinematicState,
    common::PluginRandom,
    estimator::ContactList,
    plugin_manager::entity_plugin::{EntityInfo, Plugin, StepTime, Update},
    pubsub::Messages,
};
use anyhow::{Result, ensure};
use std::any::TypeId;

mod readings;
pub use readings::{Reading, ReadingKind, Readings};

pub struct SensorContext<'a> {
    pub messages: &'a mut Messages,
    pub entity: EntityInfo,
    pub time: StepTime,
    /// Sensors sample committed truth after all motion substeps.
    pub truth: &'a KinematicState,
    pub contacts_truth: &'a [EntitySnapshot],
    pub random: &'a mut PluginRandom,
    /// This sensor's instance name, which estimators use to select its readings.
    pub(crate) name: &'a str,
    pub(crate) kind: ReadingKind,
    pub(crate) pending: &'a mut Readings,
}
impl SensorContext<'_> {
    /// Hand this tick's reading to the entity's estimators, which receive it in the
    /// estimation step at the end of the tick. It replaces the previous reading.
    /// This is not a network message. A `State` sensor's reading is a
    /// `KinematicState` and a `Contacts` sensor's is a `ContactList`.
    pub fn publish_reading<T: Send + Sync + 'static>(&mut self, value: T) -> Result<()> {
        let expected = match self.kind {
            ReadingKind::State => Some(TypeId::of::<KinematicState>()),
            ReadingKind::Contacts => Some(TypeId::of::<ContactList>()),
            ReadingKind::Other => None,
        };
        ensure!(
            expected.is_none_or(|expected| expected == TypeId::of::<T>()),
            "sensor '{}' declares {:?} readings but published a {}",
            self.name,
            self.kind,
            std::any::type_name::<T>()
        );
        self.pending
            .publish(self.name, self.time.time_s + self.time.dt_s, value)
    }
}

pub trait Sensor: Plugin {
    /// What this sensor's readings contain; see `ReadingKind`.
    fn reading_kind(_config: &Self::Config) -> ReadingKind {
        ReadingKind::Other
    }
    fn initialize(&mut self, _context: &mut SensorContext<'_>) -> Result<()> {
        Ok(())
    }
    fn step(&mut self, context: &mut SensorContext<'_>) -> Result<Update>;
}

//! Sensors: the plugin interface, reading delivery, and the built-in models.
use crate::{
    KinematicState,
    common::{PluginRandom, TruthContacts},
    plugin::estimator::ContactList,
    plugin::{EntityInfo, StepTime, Update},
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
    /// Every entity's true state, after this tick's motion.
    pub contacts_truth: TruthContacts<'a>,
    /// This sensor's random numbers at this tick.
    pub random: PluginRandom,
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

/// What its readings contain is declared by `Plugin::reading_kind`.
pub trait Sensor: Send {
    fn initialize(&mut self, _context: &mut SensorContext<'_>) -> Result<()> {
        Ok(())
    }
    fn step(&mut self, context: &mut SensorContext<'_>) -> Result<Update>;
    fn close(&mut self, _time: StepTime) -> Result<()> {
        Ok(())
    }
}

// Built-ins selected by missions.
#[path = "sensor/noisy_position/noisy_position.rs"]
mod noisy_position;
pub use noisy_position::NoisyPosition;
pub use noisy_position::PositionObservation;
#[path = "sensor/noisy_state/noisy_state.rs"]
mod noisy_state;
pub use noisy_state::NoisyState;
#[path = "sensor/noisy_contacts/noisy_contacts.rs"]
mod noisy_contacts;
pub use noisy_contacts::NoisyContacts;

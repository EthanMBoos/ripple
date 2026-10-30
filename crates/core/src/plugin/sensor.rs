//! Sensors: the plugin interface, reading delivery, and the built-in models.
use crate::{
    KinematicState,
    common::{PluginRandom, TruthContacts},
    plugin::estimator::ContactList,
    plugin::{EntityInfo, PluginParams, StepTime},
    pubsub::Messages,
};
use anyhow::{Result, ensure};
use serde::de::DeserializeOwned;
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

pub trait Sensor: Send {
    // How it is built, answered from the concrete type while the mission
    // loads. `where Self: Sized` keeps these out of `Box<dyn Sensor>`, which
    // only steps; see the module doc in `plugin.rs`.
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
    /// What its readings contain; see `ReadingKind`.
    fn reading_kind(_config: &Self::Config) -> ReadingKind
    where
        Self: Sized,
    {
        ReadingKind::Other
    }

    fn initialize(&mut self, _context: &mut SensorContext) -> Result<()> {
        Ok(())
    }
    fn step(&mut self, context: &mut SensorContext) -> Result<()>;
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

//! World-interaction interface; concrete effects live in plugin/interaction.
use crate::{
    Entity, Event, EventKind, Vec3,
    common::SpatialIndex,
    plugin_manager::entity_plugin::{Plugin, StepTime, Update, WorldContext},
    pubsub::Messages,
};
use anyhow::Result;

pub struct InteractionContext<'a> {
    pub time: StepTime,
    pub entities: &'a mut [Entity],
    /// Finds nearby entities by their index in `entities`, using their positions
    /// at the start of this interaction's step.
    pub nearby: &'a SpatialIndex,
    pub messages: &'a mut Messages,
    pub(crate) events: &'a mut Vec<Event>,
}
impl InteractionContext<'_> {
    pub fn emit(&mut self, kind: EventKind, entity_ids: Vec<i32>) -> Result<()> {
        let event = Event {
            time_s: self.time.time_s,
            kind,
            entity_ids,
        };
        self.messages
            .publish("GlobalNetwork", kind.topic(), event.clone())?;
        self.events.push(event);
        Ok(())
    }
}

pub trait Interaction: Plugin {
    fn initialize(&mut self, _context: &mut WorldContext<'_>) -> Result<()> {
        Ok(())
    }
    fn collision_exists(&self, _entities: &[Entity], _position_world_m: Vec3) -> bool {
        false
    }
    fn step(&mut self, context: &mut InteractionContext<'_>) -> Result<Update>;
}

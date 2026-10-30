//! World interactions: the plugin interface, its context, and the built-in
//! effects.
use crate::{
    Entity, Event, EventKind, Vec3,
    common::SpatialIndex,
    plugin::{PluginParams, StepTime, Update, WorldContext},
    pubsub::Messages,
};
use anyhow::Result;
use serde::de::DeserializeOwned;

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

pub trait Interaction: Send {
    // How it is built, answered from the concrete type while the mission
    // loads. `where Self: Sized` keeps these out of `Box<dyn Interaction>`, which
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

    fn initialize(&mut self, _context: &mut WorldContext) -> Result<()> {
        Ok(())
    }
    fn collision_exists(&self, _entities: &[Entity], _position_world_m: Vec3) -> bool {
        false
    }
    fn step(&mut self, context: &mut InteractionContext) -> Result<Update>;
    fn close(&mut self, _time: StepTime) -> Result<()> {
        Ok(())
    }
}

// Built-ins selected by missions.
#[path = "interaction/simple_collision/simple_collision.rs"]
mod simple_collision;
pub use simple_collision::SimpleCollision;
#[path = "interaction/boundary/boundary.rs"]
mod boundary;
pub use boundary::{BOUNDARY_TOPIC, Boundary, BoundaryRegion};
#[path = "interaction/ground_collision/ground_collision.rs"]
mod ground_collision;
pub use ground_collision::GroundCollision;
#[path = "interaction/terrain_collision/terrain_collision.rs"]
mod terrain_collision;
pub use terrain_collision::TerrainCollision;
#[path = "interaction/waypoint_broadcast/waypoint_broadcast.rs"]
mod waypoint_broadcast;
pub use waypoint_broadcast::WaypointBroadcast;

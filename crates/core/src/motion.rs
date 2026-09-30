//! Motion-model interface; concrete dynamics live in plugin/motion.
use crate::{
    KinematicState,
    common::{PluginIo, Terrain},
    plugin_manager::entity_plugin::{EntityInfo, Plugin, StepTime, Update},
    pubsub::Messages,
};
use anyhow::Result;

pub struct MotionContext<'a> {
    pub messages: &'a mut Messages,
    pub entity: EntityInfo,
    pub time: StepTime,
    pub truth: &'a mut KinematicState,
    /// The mission's ground heights, if it names terrain. No motion model
    /// uses it for contact yet: TerrainCollision removes agents that reach
    /// the ground.
    // TODO(terrain): ground reactions (landing, rolling, takeoff) would read
    // the ground here: height and slope at the aircraft's contact points.
    pub terrain: Option<&'a Terrain>,
}

pub trait MotionModel: Plugin {
    fn initialize(&mut self, _context: &mut MotionContext<'_>) -> Result<()> {
        Ok(())
    }
    fn step(&mut self, context: &mut MotionContext<'_>, io: &mut PluginIo) -> Result<Update>;
}

//! Motion-model interface; concrete dynamics live in plugin/motion.
use crate::{
    KinematicState, Vec3,
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
    /// The mission's ground heights, if it names terrain. FixedWing6DOF's
    /// landing gear reads the ground here (`Terrain::ground_height` and
    /// `ground_normal`); without terrain it is flat at `z = 0`.
    pub terrain: Option<&'a Terrain>,
    /// Forces interactions applied to this entity in the last interaction
    /// phase (`Entity::apply_force`), in the order applied. They act on every
    /// motion step of this tick. MpeParticle reads them; other models do not.
    pub applied_forces_world_n: &'a [Vec3],
}

pub trait MotionModel: Plugin {
    fn initialize(&mut self, _context: &mut MotionContext<'_>) -> Result<()> {
        Ok(())
    }
    fn step(&mut self, context: &mut MotionContext<'_>, io: &mut PluginIo) -> Result<Update>;
}

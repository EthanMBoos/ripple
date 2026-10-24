//! Motion models: the plugin interface, its context, and the built-in dynamics.
use crate::{
    KinematicState, Vec3,
    common::Terrain,
    plugin::{EntityInfo, Plugin, StepTime, Update},
    ports::PluginIo,
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
    /// motion step of this tick. No built-in model reads them.
    pub applied_forces_world_n: &'a [Vec3],
}

pub trait MotionModel: Plugin {
    fn initialize(&mut self, _context: &mut MotionContext<'_>) -> Result<()> {
        Ok(())
    }
    fn step(&mut self, context: &mut MotionContext<'_>, io: &mut PluginIo) -> Result<Update>;
}

// Built-ins selected by missions.
#[path = "motion/fixed_wing_6dof/fixed_wing_6dof.rs"]
mod fixed_wing_6dof;
pub use fixed_wing_6dof::FixedWing6Dof;
pub(crate) use fixed_wing_6dof::FixedWingAirframe;
#[path = "motion/multirotor/multirotor.rs"]
mod multirotor;
pub use multirotor::Multirotor;
pub(crate) use multirotor::{Airframe, AirframeName, AirframeOverrides, RotorParams};
#[path = "motion/simple_aircraft/simple_aircraft.rs"]
mod simple_aircraft;
pub use simple_aircraft::SimpleAircraft;
#[path = "motion/single_integrator/single_integrator.rs"]
mod single_integrator;
pub use single_integrator::SingleIntegrator;

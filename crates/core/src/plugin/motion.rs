//! Motion implementations selected by missions.
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
#[path = "motion/mpe_particle/mpe_particle.rs"]
mod mpe_particle;
pub use mpe_particle::MpeParticle;

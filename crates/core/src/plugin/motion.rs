//! Motion implementations selected by mission XML.
#[path = "motion/fixed_wing_6dof/fixed_wing_6dof.rs"]
mod fixed_wing_6dof;
pub use fixed_wing_6dof::FixedWing6Dof;
#[path = "motion/multirotor/multirotor.rs"]
mod multirotor;
pub use multirotor::Multirotor;
pub(crate) use multirotor::{Airframe, AirframeName, AirframeOverrides};
#[path = "motion/simple_aircraft/simple_aircraft.rs"]
mod simple_aircraft;
pub use simple_aircraft::SimpleAircraft;
#[path = "motion/single_integrator/single_integrator.rs"]
mod single_integrator;
pub use single_integrator::SingleIntegrator;

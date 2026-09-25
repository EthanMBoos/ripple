//! One-dimensional constant-velocity motion along the world's east axis.
use anyhow::Result;
use scrimmage_core::{Vec3, plugin::*};

pub struct EastwardMotion;
impl Plugin for EastwardMotion {
    type Config = ();
    fn configure(params: &PluginParams<'_>) -> Result<()> {
        params.parse()
    }
    fn new(_: &()) -> Self {
        Self
    }
    fn ports(_: &()) -> Ports {
        Ports::default().input(Port::new("speed", Unit::MetersPerSecond, Frame::World))
    }
}
impl MotionModel for EastwardMotion {
    fn step(&mut self, context: &mut MotionContext<'_>, io: &mut PluginIo) -> Result<Update> {
        let speed_world_mps = io.read("speed")?;
        context.truth.position_world_m.x += speed_world_mps * context.time.dt_s;
        context.truth.velocity_world_mps = Vec3::new(speed_world_mps, 0.0, 0.0);
        Ok(Update::Applied)
    }
}

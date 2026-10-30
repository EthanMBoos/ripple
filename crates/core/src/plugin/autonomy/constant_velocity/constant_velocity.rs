//! Rust-only open-loop velocity test driver, not a navigation behavior.
//! Autonomy phase: write a configured constant ENU velocity to velocity_x/y/z.
//! The reference fixture supplies the same commands to the upstream C++ models.

use anyhow::Result;
use serde::Deserialize;

use crate::plugin::{AgentContext, Autonomy, Frame, PluginIo, PluginParams, Port, Ports, Unit};

const PORTS: [&str; 3] = ["velocity_x", "velocity_y", "velocity_z"];

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ConstantVelocityParams {
    /// World velocity, `[east, north, up]`.
    velocity_mps: [f64; 3],
}

impl Default for ConstantVelocityParams {
    fn default() -> Self {
        Self {
            velocity_mps: [0.0; 3],
        }
    }
}

pub struct ConstantVelocity {
    velocity_world_mps: [f64; 3],
}

impl Autonomy for ConstantVelocity {
    type Config = [f64; 3];

    fn configure(params: &PluginParams) -> Result<[f64; 3]> {
        let params: ConstantVelocityParams = params.parse()?;
        Ok(params.velocity_mps)
    }

    fn new(velocity_world_mps: &[f64; 3]) -> Self {
        Self {
            velocity_world_mps: *velocity_world_mps,
        }
    }

    fn ports(_: &[f64; 3]) -> Ports {
        let mut ports = Ports::default();
        for name in PORTS {
            ports = ports.output(Port::new(name, Unit::MetersPerSecond, Frame::World));
        }
        ports
    }

    fn step(&mut self, _: &mut AgentContext, io: &mut PluginIo) -> Result<()> {
        for (name, value) in PORTS.into_iter().zip(self.velocity_world_mps) {
            io.write(name, value)?;
        }
        Ok(())
    }
}

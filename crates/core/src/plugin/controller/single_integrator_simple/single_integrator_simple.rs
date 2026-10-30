//! Pass ENU velocity commands to SingleIntegrator.
//! Controller phase; inputs and outputs are separate even though their names match.

use anyhow::Result;

use crate::plugin::{AgentContext, Controller, Frame, PluginIo, Port, Ports, Unit};

pub struct SingleIntegratorControllerSimple;

impl Controller for SingleIntegratorControllerSimple {
    type Config = ();
    fn new(_: &()) -> Self {
        Self
    }
    fn ports(_: &()) -> Ports {
        let mut ports = Ports::default();
        for name in ["velocity_x", "velocity_y", "velocity_z"] {
            let port = Port::new(name, Unit::MetersPerSecond, Frame::World);
            ports = ports.input(port.clone()).output(port);
        }
        ports
    }

    fn step(&mut self, _: &mut AgentContext, io: &mut PluginIo) -> Result<()> {
        for name in ["velocity_x", "velocity_y", "velocity_z"] {
            io.write(name, io.read(name)?)?;
        }
        Ok(())
    }
}

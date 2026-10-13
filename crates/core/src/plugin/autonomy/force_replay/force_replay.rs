//! Rust-only test driver: replays a fixed list of forces, one per tick.
//! Autonomy phase: write the next `[x, y]` to force_x and force_y, then zero
//! once the list runs out. `reference/mpe_parity.py` feeds MPE2's actions
//! through it.

use anyhow::Result;
use serde::Deserialize;

use crate::plugin::{
    AgentContext, Autonomy, Frame, Plugin, PluginIo, PluginParams, Port, Ports, Unit, Update,
};

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ForceReplayConfig {
    /// ENU forces in N, `[x, y]`, one per tick from the first.
    #[serde(rename = "forces")]
    forces_n: Vec<[f64; 2]>,
}

pub struct ForceReplay {
    forces_n: Vec<[f64; 2]>,
    next: usize,
}

impl Plugin for ForceReplay {
    type Config = ForceReplayConfig;

    fn configure(params: &PluginParams<'_>) -> Result<ForceReplayConfig> {
        params.parse()
    }

    fn new(config: &ForceReplayConfig) -> Self {
        Self {
            forces_n: config.forces_n.clone(),
            next: 0,
        }
    }

    fn ports(_: &ForceReplayConfig) -> Ports {
        Ports::default()
            .output(Port::new("force_x", Unit::Newtons, Frame::World))
            .output(Port::new("force_y", Unit::Newtons, Frame::World))
    }
}

impl Autonomy for ForceReplay {
    fn step(&mut self, _: &mut AgentContext<'_>, io: &mut PluginIo) -> Result<Update> {
        let [x, y] = self.forces_n.get(self.next).copied().unwrap_or_default();
        self.next += 1;
        io.write("force_x", x)?;
        io.write("force_y", y)?;
        Ok(Update::Applied)
    }
}

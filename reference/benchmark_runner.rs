//! Headless core benchmark executable, not an application API or simulator command.
//! Both implementations write frames and summaries. No viewer dependency is timed.
//! Prints one JSON line: steps, setup_s (load and construct), and run_s (stepping
//! and writing frames), for reference/perf.py. An optional fifth argument, a cell
//! width in metres, answers spatial queries with a grid instead of a scan.
//! It also registers StateBroadcast and StateListener, a message pair that only
//! reference/message_benchmark.py uses.

use std::{env, fs, path::Path, time::Instant};

use anyhow::{Result, ensure};
use ripple_core::plugin::*;
use ripple_core::{KinematicState, Mission, Params, Simulation, SpatialLookup, record::RunWriter};

const NETWORK: &str = "LocalNetwork";
const TOPIC: &str = "StateWithCovariance";

/// Publishes the entity's true state on LocalNetwork every step: the message
/// SCRIMMAGE's NoisyState sends, so both simulators route the same traffic.
struct StateBroadcast;
impl Plugin for StateBroadcast {
    type Config = ();
    fn configure(params: &PluginParams<'_>) -> Result<()> {
        params.parse()
    }
    fn new(_: &()) -> Self {
        Self
    }
}
impl Sensor for StateBroadcast {
    fn step(&mut self, context: &mut SensorContext<'_>) -> Result<Update> {
        let state = context.truth.clone();
        context.messages.publish(NETWORK, TOPIC, state)?;
        Ok(Update::Applied)
    }
}

/// Subscribes to StateBroadcast's messages and discards them, as SCRIMMAGE's
/// Straight does with NoisyState's.
struct StateListener;
impl Plugin for StateListener {
    type Config = ();
    fn configure(params: &PluginParams<'_>) -> Result<()> {
        params.parse()
    }
    fn new(_: &()) -> Self {
        Self
    }
}
impl Autonomy for StateListener {
    fn initialize(&mut self, context: &mut AgentContext<'_>) -> Result<()> {
        context.messages.subscribe::<KinematicState>(NETWORK, TOPIC)
    }
    fn step(&mut self, context: &mut AgentContext<'_>, _io: &mut PluginIo) -> Result<Update> {
        context.messages.receive::<KinematicState>(NETWORK, TOPIC)?;
        Ok(Update::Applied)
    }
}

fn main() -> Result<()> {
    let args: Vec<_> = env::args().collect();
    ensure!(
        args.len() == 5 || args.len() == 6,
        "usage: reference MISSION ROOT OUTPUT WORKERS [GRID_CELL_WIDTH_M]"
    );
    let started = Instant::now();
    let mut registry = PluginRegistry::with_builtins();
    registry.register_sensor::<StateBroadcast>("StateBroadcast")?;
    registry.register_autonomy::<StateListener>("StateListener")?;
    let mut config = Mission::load(Path::new(&args[1]), Path::new(&args[2]), &Params::new())?;
    if let Some(cell_width_m) = args.get(5) {
        config.scenario.spatial_lookup = SpatialLookup::Grid {
            cell_width_m: cell_width_m.parse()?,
        };
    }
    let mut simulation = Simulation::new(config.scenario, &registry, args[4].parse()?)?;
    let setup_s = started.elapsed().as_secs_f64();
    let started = Instant::now();
    let output = Path::new(&args[3]);
    fs::create_dir(output)?;
    let mut record = RunWriter::create(&output.join("run.mcap"), &simulation, false)?;
    while let Some(frame) = simulation.step()? {
        record.step_frame(&simulation, &frame)?;
    }
    record.finish_run(&simulation, None)?;
    let run_s = started.elapsed().as_secs_f64();
    println!(
        "{}",
        serde_json::json!({"steps": simulation.step_count(), "setup_s": setup_s, "run_s": run_s})
    );
    Ok(())
}

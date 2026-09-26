//! Headless core benchmark executable, not an application API or simulator command.
//! Both implementations write frames and summaries. No viewer dependency is timed.
//! Prints one JSON line: steps, setup_s (load and construct), and run_s (stepping
//! and writing frames), for reference/perf.py.

use std::{
    env, fs,
    io::{BufWriter, Write},
    path::Path,
    time::Instant,
};

use anyhow::{Result, ensure};
use scrimmage_core::{Mission, Params, Simulation, write_frame};

fn main() -> Result<()> {
    let args: Vec<_> = env::args().collect();
    ensure!(
        args.len() == 5,
        "usage: reference MISSION ROOT OUTPUT WORKERS"
    );
    let started = Instant::now();
    let config = Mission::load(Path::new(&args[1]), Path::new(&args[2]), &Params::new())?;
    let mut simulation = Simulation::new(
        config.scenario,
        &scrimmage_core::plugin::PluginRegistry::with_builtins(),
        args[4].parse()?,
    )?;
    let setup_s = started.elapsed().as_secs_f64();
    let started = Instant::now();
    let output = Path::new(&args[3]);
    fs::create_dir(output)?;
    let mut frames = BufWriter::new(fs::File::create_new(output.join("frames.bin"))?);
    while let Some(frame) = simulation.step()? {
        write_frame(&mut frames, &frame)?;
    }
    frames.flush()?;
    fs::write(output.join("summary.csv"), simulation.summary_csv())?;
    fs::write(
        output.join("events.json"),
        serde_json::to_vec(simulation.events())?,
    )?;
    let run_s = started.elapsed().as_secs_f64();
    println!(
        "{}",
        serde_json::json!({"steps": simulation.step_count(), "setup_s": setup_s, "run_s": run_s})
    );
    Ok(())
}

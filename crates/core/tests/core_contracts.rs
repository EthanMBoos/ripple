//! Simulation-level contracts of the core: time, spawn sampling, and the dt a
//! rate-limited plugin receives.
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::Result;
use ripple_core::plugin::*;
use ripple_core::{EventKind, Mission, Params, Simulation, SimulationFrame};

/// (time, dt) of every ClockLog call; only one test uses it.
static CALLS: Mutex<Vec<(f64, f64)>> = Mutex::new(Vec::new());

/// Records when it runs and the dt it is given; writes nothing.
struct ClockLog;
impl Autonomy for ClockLog {
    type Config = ();
    fn new(_: &()) -> Self {
        Self
    }

    fn step(&mut self, context: &mut AgentContext, _io: &mut PluginIo) -> Result<()> {
        let time = context.time;
        CALLS.lock().unwrap().push((time.time_s, time.dt_s));
        Ok(())
    }
}

/// Keeps the state it starts with.
struct Still;
impl MotionModel for Still {
    type Config = ();
    fn new(_: &()) -> Self {
        Self
    }

    fn step(&mut self, _context: &mut MotionContext, _io: &mut PluginIo) -> Result<()> {
        Ok(())
    }
}

/// Runs a YAML mission to the end; returns its frames and the finished simulation.
fn run(name: &str, yaml: &str) -> Result<(Vec<SimulationFrame>, Simulation)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = std::env::temp_dir().join(format!("ripple-{name}-{}.yaml", std::process::id()));
    std::fs::write(&path, yaml)?;
    let mut registry = PluginRegistry::with_builtins();
    registry.register_autonomy::<ClockLog>("ClockLog")?;
    registry.register_motion::<Still>("Still")?;
    let mission = Mission::load(&path, &root, &Params::new());
    std::fs::remove_file(&path)?;
    let mut simulation = Simulation::new(mission?.scenario, &registry, 1)?;
    let mut frames = Vec::new();
    while let Some(frame) = simulation.step()? {
        frames.push(frame);
    }
    Ok((frames, simulation))
}

#[test]
fn time_is_the_step_count_times_dt_and_every_frame_time_is_unique() -> Result<()> {
    let (frames, simulation) = run(
        "time",
        "format_version: 1
name: time
run:
  end_s: 3
  dt_s: 0.1
entities:
  still:
    team: 1
    motion_model:
      Still:
",
    )?;
    // A frame per step, at the time of the state it holds, then the terminal
    // frame after the last step, at the end time.
    assert_eq!(frames.len(), 31);
    for (step, frame) in frames.iter().enumerate() {
        assert_eq!(frame.time_s, step as f64 * 0.1, "frame {step}");
    }
    let end_s = frames.last().unwrap().time_s;
    assert!((end_s - 3.0).abs() < 1e-12);
    assert_eq!(simulation.time_s(), end_s);
    let at_end: Vec<_> = simulation
        .events()
        .iter()
        .filter(|event| event.kind == EventKind::EntityPresentAtEnd)
        .collect();
    assert_eq!(at_end.len(), 1);
    assert_eq!(at_end[0].time_s, end_s);
    Ok(())
}

#[test]
fn a_rate_limited_plugin_runs_once_per_period_and_receives_the_time_since_it_last_ran() -> Result<()>
{
    run(
        "rate",
        "format_version: 1
name: rate
run:
  end_s: 1
  dt_s: 0.1
entities:
  still:
    team: 1
    autonomy:
      ClockLog:
        loop_rate_hz: 5
    motion_model:
      Still:
",
    )?;
    let calls = CALLS.lock().unwrap().clone();
    // 5 Hz in a 10 Hz simulation: every other tick from the first, each run
    // 0.2 s after the last (the first run has no earlier one: it gets the tick).
    let expected = [(0.0, 0.1), (0.2, 0.2), (0.4, 0.2), (0.6, 0.2), (0.8, 0.2)];
    assert_eq!(calls.len(), expected.len(), "{calls:?}");
    for ((time_s, dt_s), (expected_time_s, expected_dt_s)) in calls.iter().zip(expected) {
        assert!((time_s - expected_time_s).abs() < 1e-12, "{calls:?}");
        assert!((dt_s - expected_dt_s).abs() < 1e-12, "{calls:?}");
    }
    Ok(())
}

#[test]
fn a_spawn_rate_equal_to_the_tick_rate_spawns_every_tick() -> Result<()> {
    let (frames, _) = run(
        "spawn-rate",
        "format_version: 1
name: spawn rate
run:
  end_s: 20.05
  dt_s: 0.1
entities:
  still:
    team: 1
    count: 200
    spawn:
      rate_hz: 10
      batch_size: 1
    motion_model:
      Still:
",
    )?;
    // One more entity in every frame: the frame at step k holds k + 1. (A due
    // time t + 0.1 can land just after the next tick, k * 0.1.)
    for (step, frame) in frames.iter().enumerate().take(200) {
        assert_eq!(frame.entities.len(), step + 1, "frame {step}");
    }
    Ok(())
}

#[test]
fn every_spawn_is_sampled_around_the_configured_start() -> Result<()> {
    let (_, simulation) = run(
        "spawn",
        "format_version: 1
name: spawn
run:
  end_s: 40
  dt_s: 0.1
entities:
  still:
    team: 1
    count: 200
    spawn:
      rate_hz: 10
      batch_size: 1
    position_m: [10.123456789, -3.000000123, 0]
    heading_deg: 90
    heading_variance_deg2: 100
    motion_model:
      Still:
",
    )?;
    let entities = simulation.entities();
    assert_eq!(entities.len(), 200);
    let headings_deg: Vec<f64> = entities
        .iter()
        .map(|entity| {
            entity
                .truth()
                .orientation_world_from_body
                .yaw_world_from_body_rad()
                .to_degrees()
        })
        .collect();
    let mean_deg = headings_deg.iter().sum::<f64>() / 200.0;
    let spread_deg = (headings_deg
        .iter()
        .map(|heading| (heading - mean_deg).powi(2))
        .sum::<f64>()
        / 199.0)
        .sqrt();
    // Independent draws with a 10 degree deviation: the sample mean is within
    // about 0.7 degrees of 90. A random walk would wander far from both.
    assert!((mean_deg - 90.0).abs() < 2.5, "mean {mean_deg}");
    assert!((8.5..11.5).contains(&spread_deg), "spread {spread_deg}");
    // The start position is used as given, to the last digit.
    for entity in entities {
        assert_eq!(entity.truth().position_world_m.x, 10.123456789);
        assert_eq!(entity.truth().position_world_m.y, -3.000000123);
    }
    Ok(())
}

/// (tick, target, axis) -> the observer's position error on that target.
fn contact_errors(
    yaml: &str,
    name: &str,
) -> Result<std::collections::BTreeMap<(usize, i32, usize), f64>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = std::env::temp_dir().join(format!("ripple-{name}-{}.yaml", std::process::id()));
    std::fs::write(&path, yaml)?;
    let mut registry = PluginRegistry::with_builtins();
    registry.register_motion::<Still>("Still")?;
    let mission = Mission::load(&path, &root, &Params::new());
    std::fs::remove_file(&path)?;
    let mut simulation = Simulation::new(mission?.scenario, &registry, 1)?;
    let mut errors = std::collections::BTreeMap::new();
    let mut tick = 0;
    while simulation.step()?.is_some() {
        let entities = simulation.entities();
        let observer = entities.iter().find(|entity| entity.id() == 1).unwrap();
        for contact in observer.contacts_belief() {
            let target = entities
                .iter()
                .find(|entity| entity.id() == contact.id)
                .unwrap();
            for axis in 0..3 {
                let error =
                    contact.state.position_world_m[axis] - target.truth().position_world_m[axis];
                errors.insert((tick, contact.id, axis), error);
            }
        }
        tick += 1;
    }
    Ok(errors)
}

#[test]
fn an_observers_error_on_a_target_does_not_depend_on_what_else_it_sees() -> Result<()> {
    let mission = |extra_target: &str| {
        format!(
            "format_version: 1
name: keyed noise
run:
  end_s: 2
  dt_s: 0.1
  seed: 7
entities:
  observer:
    team: 1
    position_m: [0, 0, 0]
    sensors:
      NoisyContacts:
        position_stddev_m: [5, 5, 1]
    motion_model:
      Still:
  targets:
    team: 2
    count: 2
    position_m: [50, 0, 0]
    position_variance_m2: [100, 100, 0]
    randomize_every_spawn: true
    motion_model:
      Still:
{extra_target}"
        )
    };
    let two = contact_errors(&mission(""), "keyed-two")?;
    // A third target, spawned last so the first three keep their IDs; with a
    // stream of draws, measuring it would shift every later error.
    let three = contact_errors(
        &mission(
            "  late_target:
    team: 2
    position_m: [-50, 0, 0]
    motion_model:
      Still:
",
        ),
        "keyed-three",
    )?;
    assert!(three.len() > two.len());
    let mut shared = 0;
    for (key, error) in &two {
        assert_eq!(three.get(key), Some(error), "{key:?}");
        assert!(*error != 0.0);
        shared += 1;
    }
    // 20 steps and the terminal frame, two targets, three axes.
    assert_eq!(shared, 21 * 2 * 3);
    Ok(())
}

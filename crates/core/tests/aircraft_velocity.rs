//! A fixed-wing aircraft follows a velocity command the way an autopilot
//! does: heading along the command (kept when it is zero), speed and climb
//! within the aircraft's limits, and turns no tighter than its bank allows.
use std::path::PathBuf;

use anyhow::Result;
use ripple_core::plugin::PluginRegistry;
use ripple_core::{Mission, Params, Simulation, SimulationFrame};

/// SimpleAircraft with the Rascal's limits, starting east at 20 m/s and 300 m,
/// commanded `velocity_mps` for `end_s`.
fn fly(name: &str, velocity: [f64; 3], end_s: f64) -> Result<Vec<SimulationFrame>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = std::env::temp_dir().join(format!("ripple-{name}-{}.yaml", std::process::id()));
    std::fs::write(
        &path,
        format!(
            "format_version: 1
name: {name}
run:
  end_s: {end_s}
  dt_s: 0.01
entities:
  rascal:
    team: 1
    position_m: [0, 0, 300]
    velocity_mps: [20, 0, 0]
    autonomy:
      ConstantVelocity:
        velocity_mps: {velocity:?}
    controller:
      AircraftVelocityController:
      SimpleAircraftControllerPID:
    motion_model:
      SimpleAircraft:
        airframe: rascal
"
        ),
    )?;
    let mission = Mission::load(&path, &root, &Params::new());
    std::fs::remove_file(&path)?;
    let mut simulation = Simulation::new(mission?.scenario, &PluginRegistry::with_builtins(), 1)?;
    let mut frames = Vec::new();
    while let Some(frame) = simulation.step()? {
        frames.push(frame);
    }
    Ok(frames)
}

fn heading_deg(frame: &SimulationFrame) -> f64 {
    frame.entities[0]
        .truth
        .orientation_world_from_body
        .yaw_world_from_body_rad()
        .to_degrees()
}

fn speed_mps(frame: &SimulationFrame) -> f64 {
    frame.entities[0].truth.velocity_world_mps.norm()
}

#[test]
fn the_heading_is_the_commands_direction() -> Result<()> {
    // SimpleAircraftControllerPID's speed loop settles in about 50 s.
    let frames = fly("velocity-north", [0.0, 15.0, 0.0], 60.0)?;
    let last = frames.last().unwrap();
    assert!(
        (heading_deg(last) - 90.0).abs() < 0.5,
        "{}",
        heading_deg(last)
    );
    assert!((speed_mps(last) - 15.0).abs() < 0.05, "{}", speed_mps(last));
    assert!((last.entities[0].truth.position_world_m.z - 300.0).abs() < 1.0);
    Ok(())
}

#[test]
fn a_zero_command_keeps_the_heading_and_slows_to_the_minimum_speed() -> Result<()> {
    let frames = fly("velocity-zero", [0.0, 0.0, 0.0], 30.0)?;
    let last = frames.last().unwrap();
    assert!(heading_deg(last).abs() < 0.5, "{}", heading_deg(last));
    // The Rascal's minimum speed, and still level.
    assert!((speed_mps(last) - 9.0).abs() < 1e-6, "{}", speed_mps(last));
    assert!((last.entities[0].truth.position_world_m.z - 300.0).abs() < 1.0);
    Ok(())
}

#[test]
fn speed_and_climb_stop_at_the_aircrafts_limits() -> Result<()> {
    // 60 m/s and a 20 m/s climb: beyond the Rascal's 38.1 m/s and 5.8 m/s.
    let frames = fly("velocity-limits", [60.0, 0.0, 20.0], 40.0)?;
    let last = frames.last().unwrap();
    assert!((speed_mps(last) - 38.1).abs() < 1e-6, "{}", speed_mps(last));
    let climb_mps = last.entities[0].truth.velocity_world_mps.z;
    assert!((climb_mps - 5.8).abs() < 1e-6, "{climb_mps}");
    Ok(())
}

#[test]
fn a_reversal_turns_no_faster_than_the_bank_limit_allows() -> Result<()> {
    let frames = fly("velocity-reverse", [-20.0, 0.0, 0.0], 30.0)?;
    let last = frames.last().unwrap();
    assert!(
        (heading_deg(last).abs() - 180.0).abs() < 0.5,
        "{}",
        heading_deg(last)
    );
    // A coordinated turn at the 43 degree bank limit: g tan(bank) / speed.
    let mut fastest_degps = 0.0_f64;
    for frame in &frames {
        let truth = &frame.entities[0].truth;
        let limit_degps =
            (9.81 * 43_f64.to_radians().tan() / truth.velocity_world_mps.norm()).to_degrees();
        let rate_degps = truth.angular_velocity_world_radps.z.abs().to_degrees();
        assert!(
            rate_degps <= limit_degps + 1e-6,
            "{rate_degps} > {limit_degps}"
        );
        fastest_degps = fastest_degps.max(rate_degps / limit_degps);
    }
    // And it uses the whole bank: the turn reaches the limit.
    assert!(fastest_degps > 0.999, "{fastest_degps}");
    Ok(())
}

/// The Rascal under AircraftPIDController, flying east at 20 m/s into a
/// 6 m/s headwind for 60 s; `told` gives the controller the wind.
fn into_headwind(told: bool) -> Result<SimulationFrame> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let name = format!("headwind-{told}");
    let path = std::env::temp_dir().join(format!("ripple-{name}-{}.yaml", std::process::id()));
    let controller_wind = if told { "wind_mps: [-6, 0, 0]" } else { "" };
    std::fs::write(
        &path,
        format!(
            "format_version: 1
name: {name}
run:
  end_s: 60
  dt_s: 0.01
entities:
  rascal:
    team: 1
    position_m: [0, 0, 300]
    velocity_mps: [20, 0, 0]
    autonomy:
      Straight:
        speed_mps: 20
    controller:
      AircraftPIDController:
        airframe: rascal
        {controller_wind}
    motion_model:
      FixedWing6DOF:
        airframe: rascal
        wind_mps: [-6, 0, 0]
"
        ),
    )?;
    let mission = Mission::load(&path, &root, &Params::new());
    std::fs::remove_file(&path)?;
    let mut simulation = Simulation::new(mission?.scenario, &PluginRegistry::with_builtins(), 1)?;
    let mut last = None;
    while let Some(frame) = simulation.step()? {
        last = Some(frame);
    }
    Ok(last.unwrap())
}

#[test]
fn the_speed_loop_holds_airspeed_in_the_wind_it_is_given() -> Result<()> {
    // Told the wind, the Rascal flies 20 m/s through the air, 14 over the ground.
    let told = speed_mps(&into_headwind(true)?);
    assert!((told - 14.0).abs() < 0.1, "{told}");
    // Not told, it holds 20 m/s over the ground, 26 through the air.
    let untold = speed_mps(&into_headwind(false)?);
    assert!((untold - 20.0).abs() < 0.1, "{untold}");
    Ok(())
}

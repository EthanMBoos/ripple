use std::path::PathBuf;

use anyhow::Result;
use ripple_core::{Mission, Params, Simulation};

fn simulation(altitude_bias: f64, sensor_rate: f64) -> Result<Simulation> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut overrides = Params::new();
    for group in ["blue", "red"] {
        let sensor = format!("entities.{group}.sensors.NoisyState");
        for (field, value) in [
            ("position_stddev_m", "[0, 0, 0]".to_string()),
            ("velocity_stddev_mps", "[0, 0, 0]".to_string()),
            ("position_bias_m", format!("[0, 0, {altitude_bias}]")),
            ("loop_rate_hz", sensor_rate.to_string()),
        ] {
            overrides.insert(format!("{sensor}.{field}"), value);
        }
    }
    let mission = Mission::load(&root.join("missions/noisy-state.yaml"), &root, &overrides)?;
    Simulation::new(
        mission.scenario,
        &ripple_core::plugin::PluginRegistry::with_builtins(),
        2,
    )
}

/// One SimpleAircraft with a zero-spread NoisyState: position biased 3 m up,
/// velocity and attitude exact. `aircraft` adds entity keys (start state).
fn biased_sensor(name: &str, aircraft: &str) -> Result<Simulation> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = std::env::temp_dir().join(format!("ripple-{name}-{}.yaml", std::process::id()));
    std::fs::write(
        &path,
        format!(
            "format_version: 1
name: {name}
run:
  end_s: 3
  dt_s: 0.1
  seed: 1
networks:
  LocalNetwork:
entities:
  aircraft:
    team: 1
    position_m: [0, 0, 200]
{aircraft}
    autonomy:
      Straight:
        speed_mps: 20
    controller:
      SimpleAircraftControllerPID:
    motion_model:
      SimpleAircraft:
    sensors:
      NoisyState:
        position_bias_m: [0, 0, 3]
"
        ),
    )?;
    let mission = Mission::load(&path, &root, &Params::new());
    std::fs::remove_file(&path)?;
    Simulation::new(
        mission?.scenario,
        &ripple_core::plugin::PluginRegistry::with_builtins(),
        1,
    )
}

#[test]
fn the_first_reading_is_sampled_from_the_starting_state() -> Result<()> {
    // 5 m/s is below SimpleAircraft's 15 m/s minimum; motion setup raises it.
    let simulation = biased_sensor("first-reading", "    velocity_mps: [5, 0, 0]")?;
    let entity = &simulation.entities()[0];
    let (belief, truth) = (entity.belief(), entity.truth());
    assert_eq!(belief.position_world_m.z - truth.position_world_m.z, 3.0);
    assert_eq!(belief.velocity_world_mps, truth.velocity_world_mps);
    assert_eq!(truth.velocity_world_mps.norm(), 15.0);
    Ok(())
}

#[test]
fn belief_reports_the_true_angular_velocity() -> Result<()> {
    // Banked at the start, so the controller rolls back level and the aircraft turns.
    let mut simulation = biased_sensor(
        "angular-velocity",
        "    roll_deg: 20\n    velocity_mps: [20, 0, 0]",
    )?;
    for _ in 0..10 {
        simulation.step()?;
        let entity = &simulation.entities()[0];
        let rate_radps = entity.truth().angular_velocity_world_radps;
        assert!(rate_radps.norm() > 0.01, "{rate_radps:?}");
        assert_eq!(entity.belief().angular_velocity_world_radps, rate_radps);
    }
    Ok(())
}

#[test]
fn a_bias_from_the_first_reading_moves_belief_and_not_truth() -> Result<()> {
    let mut ideal = simulation(0.0, 10.0)?;
    let mut biased = simulation(5.0, 10.0)?;
    // The first decision already sees the bias, and Straight holds the altitude
    // it believes it started at, so the biased aircraft flies exactly as the
    // ideal one while believing it is 5 m higher.
    for _ in 0..20 {
        ideal.step()?;
        biased.step()?;
        for (ideal, biased) in ideal.entities().iter().zip(biased.entities()) {
            assert_eq!(
                ideal.truth().position_world_m,
                biased.truth().position_world_m
            );
            assert_eq!(
                biased.belief().position_world_m.z - biased.truth().position_world_m.z,
                5.0
            );
        }
    }
    Ok(())
}

#[test]
fn slower_sensor_holds_owned_belief_while_truth_keeps_moving() -> Result<()> {
    let mut simulation = simulation(5.0, 1.0)?;
    simulation.step()?;
    let belief = simulation.entities()[0].belief().position_world_m;
    let truth = simulation.entities()[0].truth().position_world_m;
    simulation.step()?;
    assert_eq!(simulation.entities()[0].belief().position_world_m, belief);
    assert_ne!(simulation.entities()[0].truth().position_world_m, truth);
    Ok(())
}

#[test]
fn no_sensor_gives_perfect_belief() -> Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mission = Mission::load(
        &root.join("missions/straight-no-gui.yaml"),
        &root,
        &Params::new(),
    )?;
    let mut simulation = Simulation::new(
        mission.scenario,
        &ripple_core::plugin::PluginRegistry::with_builtins(),
        1,
    )?;
    for _ in 0..3 {
        simulation.step()?;
        for entity in simulation.entities() {
            // The perfect measurement, copied by the default PassThrough estimator.
            assert_eq!(
                format!("{:?}", entity.belief()),
                format!("{:?}", entity.truth())
            );
        }
    }
    Ok(())
}

#[test]
fn a_zero_noise_reading_is_the_true_state_through_mpe_contact() -> Result<()> {
    // Two MPE agents pushed into each other, each measuring itself with no noise.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = std::env::temp_dir().join(format!("ripple-mpe-contact-{}.yaml", std::process::id()));
    let agent = |name: &str, x: f64, force: f64| {
        format!(
            "  {name}:
    team: 1
    position_m: [{x}, 0, 0]
    autonomy:
      ForceReplay:
        forces_n: {:?}
    motion_model:
      MpeParticle:
    sensors:
      NoisyState:        # no noise: every parameter defaults to zero
",
            vec![[force, 0.0]; 10]
        )
    };
    let text = format!(
        "format_version: 1
name: MPE contact
run:
  end_s: 1
  dt_s: 0.1
  seed: 1
networks:
  LocalNetwork:
interactions:
  MpeContact:
    teams: [1]
    radius_m: 0.15
entities:
{}{}",
        agent("left", -0.2, 2.0),
        agent("right", 0.2, -2.0)
    );
    std::fs::write(&path, text)?;
    let mission = Mission::load(&path, &root, &Params::new());
    std::fs::remove_file(&path)?;
    let mut simulation = Simulation::new(
        mission?.scenario,
        &ripple_core::plugin::PluginRegistry::with_builtins(),
        1,
    )?;
    let mut closest_m = f64::INFINITY;
    while simulation.step()?.is_some() {
        let entities = simulation.entities();
        closest_m = closest_m.min(
            (entities[0].truth().position_world_m - entities[1].truth().position_world_m).norm(),
        );
        for entity in entities {
            assert_eq!(
                format!("{:?}", entity.belief()),
                format!("{:?}", entity.truth())
            );
        }
    }
    // They overlapped (closer than two radii), so pushes acted.
    assert!(closest_m < 0.3, "{closest_m}");
    Ok(())
}

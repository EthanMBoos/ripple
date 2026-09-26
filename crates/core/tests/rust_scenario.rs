use anyhow::Result;
use scrimmage_core::{
    EntityGroupConfig, Mission, Params, PluginConfig, RunConfig, ScenarioConfig, Simulation, Spawn,
    Vec3, plugin::PluginRegistry, write_frame,
};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Serialize)]
struct WaypointParams {
    waypoints: Vec<[f64; 3]>,
    speed: f64,
}

fn scenario() -> Result<ScenarioConfig> {
    Ok(ScenarioConfig {
        run: RunConfig {
            end_s: 1.2,
            seed: 17,
            ..RunConfig::default()
        },
        metrics: vec![PluginConfig::new("SimpleCollisionMetrics")],
        entities: vec![EntityGroupConfig {
            label: "scouts".into(),
            team: 1,
            count: 3,
            position_m: Vec3::new(0.0, 0.0, 20.0),
            position_variance_m2: Vec3::new(4.0, 4.0, 0.0),
            heading_deg: 90.0,
            heading_variance_deg2: 9.0,
            randomize_every_spawn: true,
            spawn: Some(Spawn {
                rate_hz: 2.0,
                batch_size: 1,
                start_s: 0.0,
                time_stddev_s: 0.01,
            }),
            autonomy: vec![
                PluginConfig::new("WaypointFollower").with_params(WaypointParams {
                    waypoints: vec![[100.0, 0.0, 20.0]],
                    speed: 5.0,
                })?,
            ],
            controller: vec![PluginConfig::new("SingleIntegratorControllerSimple")],
            motion_model: PluginConfig::new("SingleIntegrator"),
            ..EntityGroupConfig::default()
        }],
        ..ScenarioConfig::default()
    })
}

#[derive(Debug, PartialEq)]
struct Outputs {
    frames: Vec<u8>,
    events: Vec<u8>,
    summary: String,
}

fn outputs(config: ScenarioConfig, workers: usize) -> Result<Outputs> {
    let mut simulation = Simulation::new(config, &PluginRegistry::with_builtins(), workers)?;
    let mut frames = Vec::new();
    while let Some(frame) = simulation.step()? {
        write_frame(&mut frames, &frame)?;
    }
    assert_eq!(simulation.entities().len(), 3);
    // Each spawned entity receives independent motion state and a different birth time.
    let agents = simulation.entities();
    assert!(agents[0].born_s() < agents[1].born_s());
    assert!(agents[1].born_s() < agents[2].born_s());
    Ok(Outputs {
        frames,
        events: serde_json::to_vec(simulation.events())?,
        summary: simulation.summary_csv(),
    })
}

#[test]
fn rust_and_yaml_setups_match_with_scheduled_spawns_across_workers() -> Result<()> {
    let core = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let yaml = Mission::load(
        &core.join("tests/fixtures/rust_scenario.yaml"),
        &core.join("../.."),
        &Params::new(),
    )?;
    assert!(scenario()?.setup_differences(&yaml.scenario)?.is_empty());
    let expected = outputs(yaml.scenario, 1)?;
    for workers in [1, 2, 8] {
        assert_eq!(expected, outputs(scenario()?, workers)?);
    }
    Ok(())
}

#[test]
fn rust_inputs_receive_shared_validation_before_execution() -> Result<()> {
    let registry = PluginRegistry::with_builtins();
    let mut invalid = Vec::new();
    let mut config = scenario()?;
    config.entities[0].position_m.x = f64::NAN;
    invalid.push(config);
    let mut config = scenario()?;
    config.entities[0].spawn.as_mut().unwrap().batch_size = 0;
    invalid.push(config);
    let mut config = scenario()?;
    config.entities[0].position_variance_m2.x = -1.0;
    invalid.push(config);
    let mut config = scenario()?;
    config.entities[0].motion_model = PluginConfig::new("WaypointFollower");
    invalid.push(config);
    let mut config = scenario()?;
    config.entities[0].motion_model = EntityGroupConfig::default().motion_model;
    invalid.push(config);
    let mut config = scenario()?;
    config.entities[0].autonomy[0] =
        PluginConfig::new("WaypointFollower").with_params(serde_json::json!({"speeed": 5}))?;
    invalid.push(config);
    for config in invalid {
        assert!(Simulation::new(config, &registry, 1).is_err());
    }
    Ok(())
}

#[test]
fn typed_parameter_values_reject_nonfinite_numbers_before_conversion() {
    for speed in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(
            PluginConfig::new("WaypointFollower")
                .with_params(WaypointParams {
                    waypoints: vec![[0.0; 3]],
                    speed
                })
                .is_err()
        );
    }
}

#[test]
fn thousands_of_agents_spawned_together_reach_metrics() -> Result<()> {
    // Every entity publishes EntityGenerated in the first tick. This used to
    // overflow the engine publisher and metrics subscriber queues at 1,500.
    let config = ScenarioConfig {
        run: RunConfig {
            end_s: 0.2,
            ..RunConfig::default()
        },
        networks: vec![
            PluginConfig::new("GlobalNetwork"),
            PluginConfig::new("LocalNetwork"),
        ],
        metrics: vec![PluginConfig::new("SimpleCollisionMetrics")],
        entities: vec![EntityGroupConfig {
            label: "crowd".into(),
            team: 1,
            count: 5000,
            position_m: Vec3::new(0.0, 0.0, 100.0),
            autonomy: vec![PluginConfig::new("Straight")],
            controller: vec![PluginConfig::new("SimpleAircraftControllerPID")],
            motion_model: PluginConfig::new("SimpleAircraft"),
            ..EntityGroupConfig::default()
        }],
        ..ScenarioConfig::default()
    };
    let mut simulation = Simulation::new(config, &PluginRegistry::with_builtins(), 4)?;
    while simulation.step()?.is_some() {}
    assert!(
        simulation
            .summary_csv()
            .contains("\n1,0.000000,5000.000000,")
    );
    Ok(())
}

#[derive(Serialize)]
struct VelocityParams {
    velocity: [f64; 3],
}

#[test]
fn thousands_of_agents_removed_together_reach_metrics() -> Result<()> {
    // GroundCollision publishes one event per entity in the same step. This used
    // to overflow its 1,024-message publisher limit at 1,500 agents.
    let config = ScenarioConfig {
        run: RunConfig {
            end_s: 2.0,
            ..RunConfig::default()
        },
        networks: vec![
            PluginConfig::new("GlobalNetwork"),
            PluginConfig::new("LocalNetwork"),
        ],
        interactions: vec![PluginConfig::new("GroundCollision")],
        metrics: vec![PluginConfig::new("SimpleCollisionMetrics")],
        entities: vec![EntityGroupConfig {
            label: "falling".into(),
            team: 1,
            count: 1500,
            position_m: Vec3::new(0.0, 0.0, 5.0),
            autonomy: vec![
                PluginConfig::new("ConstantVelocity").with_params(VelocityParams {
                    velocity: [0.0, 0.0, -10.0],
                })?,
            ],
            controller: vec![PluginConfig::new("SingleIntegratorControllerSimple")],
            motion_model: PluginConfig::new("SingleIntegrator"),
            ..EntityGroupConfig::default()
        }],
        ..ScenarioConfig::default()
    };
    let mut simulation = Simulation::new(config, &PluginRegistry::with_builtins(), 4)?;
    while simulation.step()?.is_some() {}
    assert!(simulation.entities().is_empty());
    let summary = simulation.summary_csv();
    assert!(
        summary.lines().nth(1).unwrap().ends_with(",1500.000000"),
        "{summary}"
    );
    Ok(())
}

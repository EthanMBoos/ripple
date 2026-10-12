//! TerrainCollision removes agents that reach the mission's terrain, including
//! over a ridge they cross between two steps, and uses a flat ground off the grid.
use anyhow::Result;
use ripple_core::{
    EntityGroupConfig, EventKind, PluginConfig, RunConfig, ScenarioConfig, Simulation, Spawn, Vec3,
    plugin::PluginRegistry,
};
use std::{fs, path::PathBuf};

/// A 200 m strip, sampled every 10 m, flat at z = 0 except for a thin ridge
/// 50 m high at 100 m east. Written as a terrain header and heights file;
/// `hole` marks the sample at 150 m east as missing.
fn ridge(name: &str, hole: bool) -> Result<PathBuf> {
    let directory = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
    fs::create_dir_all(&directory)?;
    let row: Vec<f32> = (0..21)
        .map(|column| match column {
            10 => 50.0,
            15 if hole => -9999.0,
            _ => 0.0,
        })
        .collect();
    let bytes: Vec<u8> = [row.clone(), row]
        .concat()
        .iter()
        .flat_map(|height| height.to_le_bytes())
        .collect();
    fs::write(directory.join("ridge.f32"), bytes)?;
    let header = directory.join("ridge.terrain.yaml");
    fs::write(
        &header,
        "format_version: 1\nsource: test ridge\n\
         origin: {latitude_deg: 0, longitude_deg: 0, altitude_m: 0}\n\
         west_m: 0\nnorth_m: 10\ncell_m: 10\ncolumns: 21\nrows: 2\n\
         heights: ridge.f32\nno_data: -9999\n",
    )?;
    Ok(header)
}

fn agent(id: i32, position_m: Vec3, velocity_mps: [f64; 3]) -> Result<EntityGroupConfig> {
    Ok(EntityGroupConfig {
        label: format!("agent{id}"),
        team: 1,
        id: Some(id),
        position_m,
        visual_model: "sphere".into(),
        autonomy: vec![
            PluginConfig::new("ConstantVelocity")
                .with_params(serde_json::json!({"velocity": velocity_mps}))?,
        ],
        motion_model: PluginConfig::new("SingleIntegrator"),
        ..EntityGroupConfig::default()
    })
}

#[test]
fn agents_hit_a_ridge_crossed_between_steps_and_flat_ground_off_the_grid() -> Result<()> {
    let terrain = ridge("terrain-collision", false)?;
    let scenario = ScenarioConfig {
        run: RunConfig {
            dt_s: 1.0,
            end_s: 8.0,
            ..RunConfig::default()
        },
        terrain: Some(terrain.clone()),
        interactions: vec![PluginConfig::new("TerrainCollision")],
        entities: vec![
            // 25 m per step, ending steps at 80 m and 105 m east: both points are
            // below 30 m of ground, but the ridge between them rises to 50 m.
            agent(1, Vec3::new(5.0, 5.0, 30.0), [25.0, 0.0, 0.0])?,
            // Clears the ridge.
            agent(2, Vec3::new(5.0, 5.0, 60.0), [25.0, 0.0, 0.0])?,
            // West of the grid, descending onto the flat ground at z = 0 there.
            agent(3, Vec3::new(-100.0, 5.0, 5.0), [0.0, 0.0, -10.0])?,
            // From 97 m to 127 m east, 0.5 m below the ridge top at 100 m; no
            // sample point along the way need land near the top.
            agent(4, Vec3::new(97.0, 5.0, 49.5), [30.0, 0.0, 0.0])?,
            // Spawned at 2 s at 55 m east; its first step, to 85 m, is only
            // checked where it ends, and its second crosses the ridge.
            EntityGroupConfig {
                count: 1,
                spawn: Some(Spawn {
                    rate_hz: 1.0,
                    batch_size: 1,
                    start_s: 2.0,
                    time_stddev_s: 0.0,
                }),
                ..agent(5, Vec3::new(55.0, 5.0, 30.0), [30.0, 0.0, 0.0])?
            },
        ],
        ..ScenarioConfig::default()
    };
    for workers in [1, 2, 8] {
        let mut simulation =
            Simulation::new(scenario.clone(), &PluginRegistry::with_builtins(), workers)?;
        while simulation.step()?.is_some() {}
        let collided: Vec<_> = simulation
            .events()
            .iter()
            .filter(|event| event.kind == EventKind::GroundCollision)
            .map(|event| (event.time_s, event.entity_ids.clone()))
            .collect();
        // An event carries the time at the start of the step that moved the agent.
        assert_eq!(
            collided,
            vec![
                (0.0, vec![3]),
                (0.0, vec![4]),
                (3.0, vec![1]),
                (3.0, vec![5])
            ],
            "{workers} workers"
        );
        let alive: Vec<_> = simulation
            .entities()
            .iter()
            .map(|entity| entity.id())
            .collect();
        assert_eq!(alive, vec![2]);
    }
    fs::remove_dir_all(terrain.parent().unwrap())?;
    Ok(())
}

#[test]
fn flying_onto_the_grid_does_not_mix_the_ground_outside_with_the_edge() -> Result<()> {
    // Flat ground outside is 0 m; the terrain is 100 m lower. The agent flies
    // at 1 m from 10 m outside onto the grid, above both.
    let directory = std::env::temp_dir().join(format!("terrain-edge-{}", std::process::id()));
    fs::create_dir_all(&directory)?;
    let bytes: Vec<u8> = [-100.0_f32; 4]
        .iter()
        .flat_map(|height| height.to_le_bytes())
        .collect();
    fs::write(directory.join("pit.f32"), bytes)?;
    let terrain = directory.join("pit.terrain.yaml");
    fs::write(
        &terrain,
        "format_version: 1\nsource: test pit\n\
         origin: {latitude_deg: 0, longitude_deg: 0, altitude_m: 0}\n\
         west_m: 0\nnorth_m: 10\ncell_m: 10\ncolumns: 2\nrows: 2\n\
         heights: pit.f32\nno_data: -9999\n",
    )?;
    let scenario = ScenarioConfig {
        run: RunConfig {
            dt_s: 1.0,
            end_s: 2.0,
            ..RunConfig::default()
        },
        terrain: Some(terrain),
        interactions: vec![PluginConfig::new("TerrainCollision")],
        entities: vec![agent(1, Vec3::new(-10.0, 5.0, 1.0), [10.0, 0.0, 0.0])?],
        ..ScenarioConfig::default()
    };
    let mut simulation = Simulation::new(scenario, &PluginRegistry::with_builtins(), 1)?;
    while simulation.step()?.is_some() {}
    fs::remove_dir_all(&directory)?;
    assert!(
        simulation
            .events()
            .iter()
            .all(|event| event.kind != EventKind::GroundCollision)
    );
    Ok(())
}

#[test]
fn fixed_wing_refuses_its_flat_ground_model_with_terrain() -> Result<()> {
    let terrain = ridge("terrain-fixed-wing", false)?;
    let scenario = |use_ground_model: bool| -> Result<ScenarioConfig> {
        Ok(ScenarioConfig {
            run: RunConfig {
                dt_s: 0.01,
                end_s: 0.5,
                ..RunConfig::default()
            },
            terrain: Some(terrain.clone()),
            interactions: vec![PluginConfig::new("TerrainCollision")],
            networks: vec![
                PluginConfig::new("GlobalNetwork"),
                PluginConfig::new("LocalNetwork"),
            ],
            entities: vec![EntityGroupConfig {
                label: "jet".into(),
                team: 1,
                position_m: Vec3::new(0.0, 5.0, 500.0),
                velocity_mps: Vec3::new(220.0, 0.0, 0.0),
                visual_model: "sphere".into(),
                autonomy: vec![
                    PluginConfig::new("Straight").with_params(serde_json::json!({"speed": 220}))?,
                ],
                controller: vec![
                    PluginConfig::new("AircraftPIDController")
                        .with_params(serde_json::json!({"airframe": "a4d"}))?,
                ],
                motion_model: PluginConfig::new("FixedWing6DOF").with_params(
                    serde_json::json!({"airframe": "a4d", "use_ground_model": use_ground_model}),
                )?,
                ..EntityGroupConfig::default()
            }],
            ..ScenarioConfig::default()
        })
    };
    let error = Simulation::new(scenario(true)?, &PluginRegistry::with_builtins(), 1)
        .err()
        .expect("flat ground with terrain");
    assert!(
        format!("{error:#}").contains("set use_ground_model to false when terrain is configured"),
        "{error:#}"
    );
    let mut simulation = Simulation::new(scenario(false)?, &PluginRegistry::with_builtins(), 1)?;
    while simulation.step()?.is_some() {}
    fs::remove_dir_all(terrain.parent().unwrap())?;
    Ok(())
}

#[test]
fn terrain_collision_needs_complete_terrain() -> Result<()> {
    let terrain = ridge("terrain-hole", true)?;
    let scenario = ScenarioConfig {
        terrain: Some(terrain.clone()),
        interactions: vec![PluginConfig::new("TerrainCollision")],
        entities: vec![agent(1, Vec3::new(0.0, 5.0, 10.0), [0.0, 0.0, 0.0])?],
        ..ScenarioConfig::default()
    };
    let error = Simulation::new(scenario, &PluginRegistry::with_builtins(), 1)
        .err()
        .expect("terrain with a hole");
    fs::remove_dir_all(terrain.parent().unwrap())?;
    assert!(
        format!("{error:#}").contains("2 samples have no height"),
        "{error:#}"
    );
    Ok(())
}

#[test]
fn terrain_collision_needs_a_terrain() -> Result<()> {
    let scenario = ScenarioConfig {
        interactions: vec![PluginConfig::new("TerrainCollision")],
        entities: vec![agent(1, Vec3::new(0.0, 0.0, 10.0), [0.0, 0.0, 0.0])?],
        ..ScenarioConfig::default()
    };
    let error = Simulation::new(scenario, &PluginRegistry::with_builtins(), 1)
        .err()
        .expect("no terrain");
    assert!(format!("{error:#}").contains("needs the mission to name a terrain"));
    Ok(())
}

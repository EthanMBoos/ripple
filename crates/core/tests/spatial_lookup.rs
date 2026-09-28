//! The grid answers spatial queries exactly as the scan does, including the pair
//! order that decides which agent survives a chain of collisions.
use anyhow::Result;
use scrimmage_core::{
    EntityGroupConfig, EventKind, PluginConfig, RunConfig, ScenarioConfig, Simulation,
    SpatialLookup, Vec3,
    plugin::{Interaction, InteractionContext, Plugin, PluginParams, PluginRegistry, Update},
};

/// Three agents in a row, 3 m apart, with a 5 m collision range: the first and
/// second overlap, and so do the second and third. They are created in the
/// order 30, 10, 20, so entity order differs from id order. 400 more agents
/// spread over tens of kilometres make the population large enough that the
/// grid answers queries instead of falling back to the scan.
fn chain(spatial_lookup: SpatialLookup) -> Result<ScenarioConfig> {
    let agent = |id: i32, x_m: f64| -> Result<EntityGroupConfig> {
        Ok(EntityGroupConfig {
            label: format!("agent{id}"),
            team: 1,
            id: Some(id),
            position_m: Vec3::new(x_m, 0.0, 0.0),
            visual_model: "sphere".into(),
            autonomy: vec![
                PluginConfig::new("ConstantVelocity")
                    .with_params(serde_json::json!({"velocity": [0.0, 0.0, 0.0]}))?,
            ],
            motion_model: PluginConfig::new("SingleIntegrator"),
            ..EntityGroupConfig::default()
        })
    };
    Ok(ScenarioConfig {
        run: RunConfig {
            end_s: 1.0,
            ..RunConfig::default()
        },
        spatial_lookup,
        interactions: vec![
            PluginConfig::new("SimpleCollision")
                .with_params(serde_json::json!({"collision_range": 5.0}))?,
        ],
        entities: vec![
            agent(30, 0.0)?,
            agent(10, 3.0)?,
            agent(20, 6.0)?,
            EntityGroupConfig {
                label: "far".into(),
                id: None,
                count: 400,
                position_m: Vec3::new(50_000.0, 0.0, 0.0),
                position_variance_m2: Vec3::new(1e8, 1e8, 0.0),
                ..agent(1, 0.0)?
            },
        ],
        ..ScenarioConfig::default()
    })
}

fn collisions(spatial_lookup: SpatialLookup, workers: usize) -> Result<Vec<Vec<i32>>> {
    let mut simulation = Simulation::new(
        chain(spatial_lookup)?,
        &PluginRegistry::with_builtins(),
        workers,
    )?;
    while simulation.step()?.is_some() {}
    Ok(simulation
        .events()
        .iter()
        .filter(|event| event.kind == EventKind::TeamCollision)
        .map(|event| event.entity_ids.clone())
        .collect())
}

#[test]
fn the_first_pair_in_entity_order_collides_and_the_third_agent_survives() -> Result<()> {
    // Entity order visits 30-10 before 10-20; id order would visit 10-20 first.
    assert_eq!(collisions(SpatialLookup::Scan, 1)?, vec![vec![30, 10]]);
    // 5 and 50 m cells use the grid; 1 m cells need too many columns, so scan.
    for cell_width_m in [1.0, 5.0, 50.0] {
        for workers in [1, 8] {
            assert_eq!(
                collisions(SpatialLookup::Grid { cell_width_m }, workers)?,
                vec![vec![30, 10]],
                "grid {cell_width_m} m, {workers} workers"
            );
        }
    }
    Ok(())
}

#[test]
fn a_grid_needs_a_positive_cell_width() -> Result<()> {
    for cell_width_m in [0.0, -1.0, f64::NAN] {
        let error = Simulation::new(
            chain(SpatialLookup::Grid { cell_width_m })?,
            &PluginRegistry::with_builtins(),
            1,
        )
        .err()
        .expect("invalid cell width");
        assert!(format!("{error:#}").contains("cell_width_m"), "{error:#}");
    }
    Ok(())
}

/// A test interaction that moves the entity with id 1 onto the entity with id 2.
struct MoveOnto;
impl Plugin for MoveOnto {
    type Config = ();
    fn configure(_params: &PluginParams<'_>) -> Result<()> {
        Ok(())
    }
    fn new(_config: &()) -> Self {
        Self
    }
}
impl Interaction for MoveOnto {
    fn step(&mut self, context: &mut InteractionContext<'_>) -> Result<Update> {
        let target = context.entities.iter().find(|entity| entity.id() == 2);
        let target_world_m = target.map(|entity| entity.truth().position_world_m);
        let mover = context.entities.iter_mut().find(|entity| entity.id() == 1);
        if let (Some(target_world_m), Some(mover)) = (target_world_m, mover) {
            mover.truth_mut().position_world_m = target_world_m;
        }
        Ok(Update::Applied)
    }
}

#[test]
fn a_collision_caused_by_an_earlier_interaction_is_seen() -> Result<()> {
    let mut registry = PluginRegistry::with_builtins();
    registry.register_interaction::<MoveOnto>("MoveOnto")?;
    // Agents 1 and 2 start 100 m apart; MoveOnto runs before SimpleCollision.
    let lookups = [
        SpatialLookup::Scan,
        SpatialLookup::Grid { cell_width_m: 5.0 },
    ];
    for spatial_lookup in lookups {
        let mut scenario = chain(spatial_lookup)?;
        scenario.entities[0].id = Some(1);
        scenario.entities[1].id = Some(2);
        scenario.entities[1].position_m = Vec3::new(100.0, 0.0, 0.0);
        scenario.entities.remove(2);
        scenario
            .interactions
            .insert(0, PluginConfig::new("MoveOnto"));
        let mut simulation = Simulation::new(scenario, &registry, 1)?;
        while simulation.step()?.is_some() {}
        let collisions: Vec<_> = simulation
            .events()
            .iter()
            .filter(|event| event.kind == EventKind::TeamCollision)
            .map(|event| (event.time_s, event.entity_ids.clone()))
            .collect();
        // In the same pass as the move (the pre-start pass, one step before
        // 0 s); with positions left from before the move, it came a step late.
        assert_eq!(collisions, vec![(-0.1, vec![1, 2])], "{spatial_lookup:?}");
    }
    Ok(())
}

//! Public plugin contracts; fixtures are not a separate application.
#[path = "support/lifecycle.rs"]
mod lifecycle;
#[path = "support/plugin_fixtures.rs"]
mod plugin_fixtures;

use anyhow::Result;
use plugin_fixtures::*;
use ripple_core::{Drawing, Params, Simulation, plugin::PluginRegistry, record};
use std::sync::Arc;

/// Overrides that replace whole plugin slots, such as
/// `entities.agents.motion_model:={MissingMotion: {}}`.
fn slots(changes: &[(&str, &str)]) -> Params {
    changes
        .iter()
        .map(|(path, value)| (path.to_string(), value.to_string()))
        .collect()
}

#[test]
fn all_user_plugin_types_work_without_engine_edits() -> Result<()> {
    for workers in [1, 2, 8] {
        let simulation = simulate(workers, &Params::new())?;
        assert_eq!(simulation.entities().len(), 8);
        let reports = simulation.metric_reports();
        assert_eq!(reports.len(), 2);
        for (_, report) in reports {
            assert_eq!(report.teams[&1].values["samples"], 11.0);
            assert_eq!(report.teams[&1].values["entity_samples"], 88.0);
            assert_eq!(report.teams[&1].values["steps"], 10.0);
            assert_eq!(report.teams[&1].values["closed"], 1.0);
        }
        for entity in simulation.entities() {
            assert!((entity.truth().position_world_m.x - 1.0).abs() < 1e-12);
            assert_eq!(entity.truth().velocity_world_mps.x, 0.0);
            assert_eq!(entity.truth().position_world_m.z, 0.0);
        }
    }
    Ok(())
}
#[test]
fn autonomy_uses_delivered_sensor_data_not_hidden_truth() -> Result<()> {
    let overrides = slots(&[("entities.agents.sensors.PositionSensor.bias_m", "1")]);
    let simulation = simulate(4, &overrides)?;
    // First autonomy update has no measurement. The sensor samples after motion;
    // only the next autonomy update can react to its biased position.
    assert!((simulation.entities()[0].truth().position_world_m.x - 0.2).abs() < 1e-12);
    Ok(())
}
#[test]
fn duplicate_registration_and_unknown_plugins_fail_before_execution() -> Result<()> {
    let mut registry = registry()?;
    assert!(
        registry
            .register_sensor::<sensor::PositionSensor>("PositionSensor")
            .is_err()
    );
    let overrides = slots(&[("entities.agents.motion_model", "{MissingMotion: {}}")]);
    assert!(Simulation::new(scenario(&overrides)?, Arc::new(registry), 1).is_err());
    Ok(())
}

#[test]
fn a_registered_name_in_the_wrong_category_is_rejected() -> Result<()> {
    let registry = Arc::new(registry()?);
    for (slot, plugins, expected) in [
        (
            "entities.agents.motion_model",
            "{PositionSensor: {}}",
            "unregistered Motion plugin 'PositionSensor'",
        ),
        (
            "interactions",
            "{PopulationMetrics: {}}",
            "unregistered Interaction plugin 'PopulationMetrics'",
        ),
        (
            "networks",
            "{LocalNetwork: {}, Floor: {}}",
            "unregistered Network plugin 'Floor'",
        ),
        (
            "metrics",
            "{ExampleNetwork: {}}",
            "unregistered Metrics plugin 'ExampleNetwork'",
        ),
    ] {
        let overrides = slots(&[(slot, plugins)]);
        let error = Simulation::new(scenario(&overrides)?, Arc::clone(&registry), 1)
            .err()
            .expect("wrong category must fail");
        assert!(format!("{error:#}").contains(expected), "{error:#}");
    }
    Ok(())
}

#[test]
fn different_categories_can_register_the_same_name() -> Result<()> {
    let mut registry = registry()?;
    // PositionSensor already names a sensor. A motion registration is independent.
    registry.register_motion::<motion::EastwardMotion>("PositionSensor")?;
    assert!(
        registry
            .register_motion::<motion::EastwardMotion>("PositionSensor")
            .is_err()
    );

    let overrides = slots(&[("entities.agents.motion_model", "{PositionSensor: {}}")]);
    let mut simulation = Simulation::new(scenario(&overrides)?, Arc::new(registry), 2)?;
    while simulation.step()?.is_some() {}
    for entity in simulation.entities() {
        assert!((entity.truth().position_world_m.x - 1.0).abs() < 1e-12);
    }
    Ok(())
}

#[test]
fn interaction_messages_reach_both_metrics_in_the_same_tick() -> Result<()> {
    let mut simulation = Simulation::new(scenario(&Params::new())?, Arc::new(registry()?), 8)?;
    simulation.step()?;
    for (_, report) in simulation.metric_reports() {
        // Pre-start interaction plus current interaction, delivered before the first metric step.
        assert_eq!(report.teams[&1].values["samples"], 2.0);
        assert_eq!(report.teams[&1].values["steps"], 1.0);
    }
    let names: Vec<String> = simulation
        .metric_reports()
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert!(
        names
            .iter()
            .any(|name| name.ends_with("PopulationMetrics:0"))
    );
    assert!(
        names
            .iter()
            .any(|name| name.ends_with("PopulationMetrics:1"))
    );
    Ok(())
}

#[test]
fn custom_network_delay_and_loss_change_only_delivered_statistics() -> Result<()> {
    for (overrides, expected_samples) in [
        (slots(&[("networks.ExampleNetwork.delay_s", "0.2")]), 9.0),
        (
            slots(&[("networks.ExampleNetwork.drop_messages", "true")]),
            0.0,
        ),
    ] {
        for workers in [1, 2, 8] {
            let simulation = simulate(workers, &overrides)?;
            for (_, report) in simulation.metric_reports() {
                assert_eq!(report.teams[&1].values["samples"], expected_samples);
            }
            assert!((simulation.entities()[0].truth().position_world_m.x - 1.0).abs() < 1e-12);
        }
    }
    Ok(())
}

#[test]
fn all_world_plugin_categories_resolve_through_the_registry() -> Result<()> {
    let registry = Arc::new(registry()?);
    for slot in ["interactions", "networks", "metrics"] {
        let overrides = slots(&[(slot, "{MissingPlugin: {}}")]);
        assert!(
            Simulation::new(scenario(&overrides)?, Arc::clone(&registry), 1).is_err(),
            "{slot}"
        );
    }
    let negative_delay = slots(&[("networks.ExampleNetwork.delay_s", "-1")]);
    assert!(Simulation::new(scenario(&negative_delay)?, Arc::clone(&registry), 1).is_err());
    Ok(())
}

struct FailingInteraction;
impl ripple_core::plugin::Interaction for FailingInteraction {
    type Config = ();
    fn new(_: &()) -> Self {
        Self
    }

    fn step(
        &mut self,
        context: &mut ripple_core::plugin::InteractionContext<'_>,
    ) -> Result<ripple_core::plugin::Update> {
        anyhow::ensure!(context.time.time_s < 0.0, "intentional interaction failure");
        Ok(ripple_core::plugin::Update::Applied)
    }
}
#[test]
fn world_plugin_failure_terminates_the_run_and_closes_other_plugins() -> Result<()> {
    let mut registry = registry()?;
    registry.register_interaction::<FailingInteraction>("FailingInteraction")?;
    let overrides = slots(&[("interactions", "{FailingInteraction: {}}")]);
    let mut simulation = Simulation::new(scenario(&overrides)?, Arc::new(registry), 8)?;
    let error = simulation.step().unwrap_err();
    assert!(format!("{error:#}").contains("FailingInteraction"));
    assert_eq!(
        simulation.termination(),
        Some(ripple_core::TerminationReason::PluginFailure)
    );
    assert!(simulation.step()?.is_none());
    for (_, report) in simulation.metric_reports() {
        assert_eq!(report.teams[&1].values["closed"], 1.0);
    }
    Ok(())
}

struct StopInteraction;
impl ripple_core::plugin::Interaction for StopInteraction {
    type Config = ();
    fn new(_: &()) -> Self {
        Self
    }

    fn step(
        &mut self,
        context: &mut ripple_core::plugin::InteractionContext<'_>,
    ) -> Result<ripple_core::plugin::Update> {
        Ok(if context.time.time_s >= 0.0 {
            ripple_core::plugin::Update::Stop
        } else {
            ripple_core::plugin::Update::Applied
        })
    }
}
#[test]
fn world_plugin_stop_emits_terminal_frame_and_closes_once() -> Result<()> {
    let mut registry = registry()?;
    registry.register_interaction::<StopInteraction>("StopInteraction")?;
    let overrides = slots(&[("interactions", "{StopInteraction: {}}")]);
    let mut simulation = Simulation::new(scenario(&overrides)?, Arc::new(registry), 8)?;
    assert!(simulation.step()?.is_some());
    assert_eq!(
        simulation.termination(),
        Some(ripple_core::TerminationReason::PluginRequestedStop)
    );
    assert!(simulation.step()?.is_some());
    assert!(simulation.step()?.is_none());
    for (_, report) in simulation.metric_reports() {
        assert_eq!(report.teams[&1].values["closed"], 1.0);
        assert_eq!(report.teams[&1].values["steps"], 1.0);
    }
    Ok(())
}

struct PanicNetwork;
impl ripple_core::plugin::Network for PanicNetwork {
    type Config = ();
    // Stands in for ExampleNetwork under the same name, so it ignores that network's parameters.
    fn configure(_: &ripple_core::plugin::PluginParams) -> Result<()> {
        Ok(())
    }
    fn new(_: &()) -> Self {
        Self
    }

    fn step(&mut self, _: &mut ripple_core::plugin::NetworkContext<'_, '_>) -> Result<()> {
        panic!("intentional network panic");
    }
}
#[test]
fn network_panic_is_reported_and_other_plugins_are_closed() -> Result<()> {
    // Floor and PopulationMetrics still address ExampleNetwork: register the failing implementation at that name in a separate registry.
    let mut registry = PluginRegistry::with_builtins();
    registry.register_autonomy::<autonomy::DriveToGoal>("DriveToGoal")?;
    registry.register_controller::<controller::ScaleSpeed>("ScaleSpeed")?;
    registry.register_motion::<motion::EastwardMotion>("EastwardMotion")?;
    registry.register_sensor::<sensor::PositionSensor>("PositionSensor")?;
    registry.register_localization::<estimator::EastFix>("EastFix")?;
    registry.register_interaction::<interaction::Floor>("Floor")?;
    registry.register_network::<PanicNetwork>("ExampleNetwork")?;
    registry.register_metrics::<metrics::PopulationMetrics>("PopulationMetrics")?;
    let mut simulation = Simulation::new(scenario(&Params::new())?, Arc::new(registry), 8)?;
    assert!(simulation.step().is_err());
    assert_eq!(
        simulation.termination(),
        Some(ripple_core::TerminationReason::PluginFailure)
    );
    for (_, report) in simulation.metric_reports() {
        assert_eq!(report.teams[&1].values["closed"], 1.0);
    }
    Ok(())
}

#[test]
fn every_frame_carries_the_plugins_drawings_in_entity_then_world_order() -> Result<()> {
    let expected_sources: Vec<String> = (1..=8)
        .map(|id| format!("DriveToGoal/{id}"))
        .chain(["Floor".to_string()])
        .collect();
    let mut first_frames = None;
    for workers in [1, 2, 8] {
        let mut simulation =
            Simulation::new(scenario(&Params::new())?, Arc::new(registry()?), workers)?;
        let mut frames = Vec::new();
        while let Some(frame) = simulation.step()? {
            frames.push(frame);
        }
        for frame in &frames {
            let sources: Vec<&str> = frame
                .drawings
                .iter()
                .map(|plugin| plugin.source.as_str())
                .collect();
            assert_eq!(sources, expected_sources);
            // The goal is fixed and shared between frames; the floor's line
            // changes every tick.
            let goal = &frame.drawings[0];
            assert!(matches!(goal.fixed[..], [Drawing::Point { .. }]));
            assert!(goal.changing.is_empty());
            assert!(std::sync::Arc::ptr_eq(
                &goal.fixed,
                &frames[0].drawings[0].fixed
            ));
            let floor = &frame.drawings[8];
            assert!(floor.fixed.is_empty());
            let Drawing::Line { label, .. } = &floor.changing[0] else {
                panic!("the floor draws a line");
            };
            assert_eq!(label, &format!("floor at {:.1} s", frame.time_s));
        }
        // The same drawings at every worker count.
        let drawings: Vec<_> = frames.iter().map(|frame| frame.drawings.clone()).collect();
        match &first_frames {
            None => first_frames = Some(drawings),
            Some(first) => assert_eq!(&drawings, first),
        }
    }
    Ok(())
}

#[test]
fn a_record_replays_each_frames_drawings() -> Result<()> {
    let directory = std::env::temp_dir().join(format!(
        "ripple-drawings-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    std::fs::create_dir_all(&directory)?;
    let path = directory.join("run.mcap");
    let mut simulation = Simulation::new(scenario(&Params::new())?, Arc::new(registry()?), 1)?;
    let mut writer = record::RunWriter::create(&path, &simulation, false)?;
    let mut frames = Vec::new();
    while let Some(frame) = simulation.step()? {
        writer.step_frame(&simulation, &frame)?;
        frames.push(frame);
    }
    writer.finish_run(&simulation, None)?;
    let replay = record::read_replay(&path)?;
    assert_eq!(replay.frames.len(), frames.len());
    for (saved, live) in replay.frames.iter().zip(&frames) {
        assert_eq!(saved.drawings, live.drawings);
    }
    std::fs::remove_dir_all(&directory)?;
    Ok(())
}

/// The spawn-request fixture: a launcher that asks for two scouts.
fn spawn_requests(overrides: &[(&str, &str)]) -> Result<ripple_core::ScenarioConfig> {
    let core = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = core.join("tests/fixtures/spawn_requests.yaml");
    Ok(ripple_core::Mission::load(&path, &core.join("../.."), &slots(overrides))?.scenario)
}

#[test]
fn a_requested_entity_is_built_next_tick_with_the_requested_values() -> Result<()> {
    let registry = Arc::new(registry()?);
    let mut expected = None;
    for workers in [1, 2, 8] {
        let mut simulation = Simulation::new(spawn_requests(&[])?, Arc::clone(&registry), workers)?;
        let mut frames = Vec::new();
        let mut first_frame_entities = None;
        while let Some(frame) = simulation.step()? {
            first_frame_entities.get_or_insert(frame.entities.len());
            frames.push(serde_json::to_string(&frame)?);
        }
        // The first frame has the launcher alone; the scouts arrive at tick 1.
        assert_eq!(first_frame_entities, Some(1));
        let entities = simulation.entities();
        assert_eq!(entities.len(), 3);
        // Recipes are numbered after the entities: the launcher is 0.
        let scouts: Vec<_> = entities.iter().filter(|e| e.group_index() == 1).collect();
        assert_eq!(scouts.len(), 2);
        for scout in &scouts {
            assert!((scout.born_s() - 0.1).abs() < 1e-12, "{}", scout.born_s());
        }
        // The first scout starts at x = 10 and drives to the overlaid goal of
        // 12, not the group's 1; the second starts past it and stays.
        let x: Vec<f64> = scouts
            .iter()
            .map(|s| s.truth().position_world_m.x)
            .collect();
        assert!(x[0] > 11.0 && x[0] <= 12.0 + 1e-9, "{x:?}");
        assert!((x[1] - 20.0).abs() < 1e-12, "{x:?}");
        let output = (frames, serde_json::to_string(simulation.events())?);
        match &expected {
            None => expected = Some(output),
            Some(expected) => assert_eq!(expected, &output, "workers {workers}"),
        }
    }
    Ok(())
}

#[test]
fn a_spawn_request_for_an_unknown_group_or_plugin_stops_the_run() -> Result<()> {
    let registry = Arc::new(registry()?);
    for (path, value, expected) in [
        (
            "entities.launcher.autonomy.Launcher.recipe",
            "nowhere",
            "the mission has no recipe `nowhere`",
        ),
        (
            "entities.launcher.autonomy.Launcher.goal_plugin",
            "Straight",
            "the recipe has no autonomy plugin `Straight`",
        ),
    ] {
        let mut simulation =
            Simulation::new(spawn_requests(&[(path, value)])?, Arc::clone(&registry), 1)?;
        let error = loop {
            match simulation.step() {
                Ok(Some(_)) => continue,
                Ok(None) => panic!("the bad request must stop the run"),
                Err(error) => break format!("{error:#}"),
            }
        };
        assert!(error.contains(expected), "{error}");
        assert!(error.contains("Launcher:0 of entity 1"), "{error}");
    }
    Ok(())
}

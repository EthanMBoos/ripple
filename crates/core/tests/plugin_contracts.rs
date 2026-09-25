//! Public plugin contracts; fixtures are not a separate application.
#[path = "support/lifecycle.rs"]
mod lifecycle;
#[path = "support/plugin_fixtures.rs"]
mod plugin_fixtures;

use anyhow::Result;
use plugin_fixtures::*;
use scrimmage_core::{Params, Simulation, plugin::PluginRegistry};

#[test]
fn all_seven_user_plugin_types_work_without_engine_edits() -> Result<()> {
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
    let overrides = Params::from([("sensor_value".into(), "1".into())]);
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
    let overrides = Params::from([("motion".into(), "MissingMotion".into())]);
    assert!(scenario(&overrides, &registry).is_err());
    Ok(())
}

#[test]
fn a_registered_name_in_the_wrong_category_is_rejected() -> Result<()> {
    let registry = registry()?;
    for (category, name, expected) in [
        (
            "motion",
            "PositionSensor",
            "Mission requests motion_model 'PositionSensor', but it isn't registered in this executable.",
        ),
        (
            "interaction",
            "PopulationMetrics",
            "Mission requests entity_interaction 'PopulationMetrics', but it isn't registered in this executable.",
        ),
        (
            "network",
            "Floor",
            "Mission requests network 'Floor', but it isn't registered in this executable.",
        ),
        (
            "metrics",
            "ExampleNetwork",
            "Mission requests metrics 'ExampleNetwork', but it isn't registered in this executable.",
        ),
    ] {
        let overrides = Params::from([(category.into(), name.into())]);
        let error = scenario(&overrides, &registry)
            .err()
            .expect("wrong category must fail");
        assert!(format!("{error:#}").contains(expected));
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

    let overrides = Params::from([("motion".into(), "PositionSensor".into())]);
    let mut simulation = Simulation::new(scenario(&overrides, &registry)?, 2)?;
    while simulation.step()?.is_some() {}
    for entity in simulation.entities() {
        assert!((entity.truth().position_world_m.x - 1.0).abs() < 1e-12);
    }
    Ok(())
}

#[test]
fn interaction_messages_reach_both_metrics_in_the_same_tick() -> Result<()> {
    let mut simulation = Simulation::new(scenario(&Params::new(), &registry()?)?, 8)?;
    simulation.step()?;
    for (_, report) in simulation.metric_reports() {
        // Pre-start interaction plus current interaction, delivered before the first metric step.
        assert_eq!(report.teams[&1].values["samples"], 2.0);
        assert_eq!(report.teams[&1].values["steps"], 1.0);
    }
    assert!(
        simulation
            .summary_csv()
            .contains("PopulationMetrics:0/samples")
    );
    assert!(
        simulation
            .summary_csv()
            .contains("PopulationMetrics:1/samples")
    );
    Ok(())
}

#[test]
fn custom_network_delay_and_loss_change_only_delivered_statistics() -> Result<()> {
    for (overrides, expected_samples) in [
        (Params::from([("delay_s".into(), "0.2".into())]), 9.0),
        (Params::from([("drop_messages".into(), "true".into())]), 0.0),
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
    let registry = registry()?;
    for category in ["interaction", "network", "metrics"] {
        let overrides = Params::from([(category.into(), "MissingPlugin".into())]);
        assert!(scenario(&overrides, &registry).is_err(), "{category}");
    }
    let negative_delay = Params::from([("delay_s".into(), "-1".into())]);
    assert!(scenario(&negative_delay, &registry).is_err());
    Ok(())
}

struct FailingInteraction;
impl scrimmage_core::plugin::Plugin for FailingInteraction {
    type Config = ();
    fn configure(params: &scrimmage_core::plugin::PluginParams<'_>) -> Result<()> {
        params.parse()
    }
    fn new(_: &()) -> Self {
        Self
    }
}
impl scrimmage_core::plugin::Interaction for FailingInteraction {
    fn step(
        &mut self,
        context: &mut scrimmage_core::plugin::InteractionContext<'_>,
    ) -> Result<scrimmage_core::plugin::Update> {
        anyhow::ensure!(context.time.time_s < 0.0, "intentional interaction failure");
        Ok(scrimmage_core::plugin::Update::Applied)
    }
}
#[test]
fn world_plugin_failure_terminates_the_run_and_closes_other_plugins() -> Result<()> {
    let mut registry = registry()?;
    registry.register_interaction::<FailingInteraction>("FailingInteraction")?;
    let overrides = Params::from([("interaction".into(), "FailingInteraction".into())]);
    let mut simulation = Simulation::new(scenario(&overrides, &registry)?, 8)?;
    let error = simulation.step().unwrap_err();
    assert!(format!("{error:#}").contains("FailingInteraction"));
    assert_eq!(
        simulation.termination(),
        Some(scrimmage_core::TerminationReason::PluginFailure)
    );
    assert!(simulation.step()?.is_none());
    for (_, report) in simulation.metric_reports() {
        assert_eq!(report.teams[&1].values["closed"], 1.0);
    }
    Ok(())
}

struct StopInteraction;
impl scrimmage_core::plugin::Plugin for StopInteraction {
    type Config = ();
    fn configure(params: &scrimmage_core::plugin::PluginParams<'_>) -> Result<()> {
        params.parse()
    }
    fn new(_: &()) -> Self {
        Self
    }
}
impl scrimmage_core::plugin::Interaction for StopInteraction {
    fn step(
        &mut self,
        context: &mut scrimmage_core::plugin::InteractionContext<'_>,
    ) -> Result<scrimmage_core::plugin::Update> {
        Ok(if context.time.time_s >= 0.0 {
            scrimmage_core::plugin::Update::Stop
        } else {
            scrimmage_core::plugin::Update::Applied
        })
    }
}
#[test]
fn world_plugin_stop_emits_terminal_frame_and_closes_once() -> Result<()> {
    let mut registry = registry()?;
    registry.register_interaction::<StopInteraction>("StopInteraction")?;
    let overrides = Params::from([("interaction".into(), "StopInteraction".into())]);
    let mut simulation = Simulation::new(scenario(&overrides, &registry)?, 8)?;
    assert!(simulation.step()?.is_some());
    assert_eq!(
        simulation.termination(),
        Some(scrimmage_core::TerminationReason::PluginRequestedStop)
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
impl scrimmage_core::plugin::Plugin for PanicNetwork {
    type Config = ();
    // Stands in for ExampleNetwork under the same name, so it ignores that network's parameters.
    fn configure(_: &scrimmage_core::plugin::PluginParams<'_>) -> Result<()> {
        Ok(())
    }
    fn new(_: &()) -> Self {
        Self
    }
}
impl scrimmage_core::plugin::Network for PanicNetwork {
    fn step(
        &mut self,
        _: &mut scrimmage_core::plugin::NetworkContext<'_, '_>,
    ) -> Result<scrimmage_core::plugin::Update> {
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
    registry.register_interaction::<interaction::Floor>("Floor")?;
    registry.register_network::<PanicNetwork>("ExampleNetwork")?;
    registry.register_metrics::<metrics::PopulationMetrics>("PopulationMetrics")?;
    let mut simulation = Simulation::new(scenario(&Params::new(), &registry)?, 8)?;
    assert!(simulation.step().is_err());
    assert_eq!(
        simulation.termination(),
        Some(scrimmage_core::TerminationReason::PluginFailure)
    );
    for (_, report) in simulation.metric_reports() {
        assert_eq!(report.teams[&1].values["closed"], 1.0);
    }
    Ok(())
}

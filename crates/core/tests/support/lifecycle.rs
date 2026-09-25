//! Initialization, stop, failure, and removal through the public plugin API.
use crate::plugin_fixtures;

use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Result, bail};
use scrimmage_core::{
    Params, Simulation, TerminationReason,
    plugin::{
        Interaction, InteractionContext, Plugin, PluginParams, Sensor, SensorContext, StepTime,
        Update,
    },
};
use serde::Deserialize;

static CLOSED: AtomicUsize = AtomicUsize::new(0);

/// Mission values: `normal`, `fail_init`, `fail_step`, or `stop`.
#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Action {
    #[default]
    Normal,
    #[serde(rename = "fail_init")]
    FailInitialization,
    FailStep,
    Stop,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct LifecycleConfig {
    action: Action,
}

struct LifecycleSensor {
    action: Action,
}

impl Plugin for LifecycleSensor {
    type Config = LifecycleConfig;

    fn configure(params: &PluginParams<'_>) -> Result<LifecycleConfig> {
        params.parse()
    }

    fn new(config: &LifecycleConfig) -> Self {
        Self {
            action: config.action,
        }
    }

    fn close(&mut self, _: StepTime) -> Result<()> {
        CLOSED.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

impl Sensor for LifecycleSensor {
    fn initialize(&mut self, _: &mut SensorContext<'_>) -> Result<()> {
        if matches!(self.action, Action::FailInitialization) {
            bail!("intentional initialization failure");
        }
        Ok(())
    }

    fn step(&mut self, _: &mut SensorContext<'_>) -> Result<Update> {
        match self.action {
            Action::FailStep => bail!("intentional step failure"),
            Action::Stop => Ok(Update::Stop),
            _ => Ok(Update::Applied),
        }
    }
}

struct RemoveEntities;

impl Plugin for RemoveEntities {
    type Config = ();
    fn configure(params: &PluginParams<'_>) -> Result<()> {
        params.parse()
    }
    fn new(_: &()) -> Self {
        Self
    }
}

impl Interaction for RemoveEntities {
    fn step(&mut self, context: &mut InteractionContext<'_>) -> Result<Update> {
        if context.time.time_s >= 0.0 {
            for entity in context.entities.iter_mut() {
                entity.set_health(0);
            }
        }
        Ok(Update::Applied)
    }
}

#[test]
fn initialization_failure_step_failure_and_removal_close_each_plugin_once() -> Result<()> {
    let mut registry = plugin_fixtures::registry()?;
    registry.register_sensor::<LifecycleSensor>("LifecycleSensor")?;
    registry.register_interaction::<RemoveEntities>("RemoveEntities")?;

    // One test owns this counter; parallel tests cannot interfere with it.
    for workers in [1, 8] {
        for action in ["fail_init", "fail_step", "normal", "stop"] {
            CLOSED.store(0, Ordering::SeqCst);
            let overrides = Params::from([
                ("sensor".into(), "LifecycleSensor".into()),
                ("sensor_key".into(), "action".into()),
                ("sensor_value".into(), action.into()),
                ("interaction".into(), "RemoveEntities".into()),
            ]);
            let scenario = plugin_fixtures::scenario(&overrides, &registry)?;
            let result = Simulation::new(scenario, &registry, workers);
            if action == "fail_init" {
                let error = result.err().expect("initialization must fail");
                let message = format!("{error:#}");
                assert!(message.contains("LifecycleSensor"), "{message}");
                assert!(message.contains("initialize entity"), "{message}");
                assert_eq!(CLOSED.load(Ordering::SeqCst), 1);
                continue;
            }

            let mut simulation = result?;
            if action == "fail_step" {
                let error = simulation.step().unwrap_err();
                assert!(format!("{error:#}").contains("LifecycleSensor"));
                assert_eq!(
                    simulation.termination(),
                    Some(TerminationReason::PluginFailure)
                );
            } else {
                simulation.step()?;
                assert!(simulation.entities().is_empty());
                for (_, report) in simulation.metric_reports() {
                    assert_eq!(report.teams[&1].values["steps"], 1.0);
                }
                if action == "stop" {
                    // Removal must not erase the sensor's request to stop this tick.
                    assert_eq!(
                        simulation.termination(),
                        Some(TerminationReason::PluginRequestedStop)
                    );
                }
                while simulation.step()?.is_some() {}
            }
            assert_eq!(CLOSED.load(Ordering::SeqCst), 8);
            drop(simulation);
            assert_eq!(CLOSED.load(Ordering::SeqCst), 8);
        }
    }
    Ok(())
}

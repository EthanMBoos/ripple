//! The Python module `ripple_learning._native`: one Ripple simulation that
//! Python steps. The Gymnasium and PettingZoo environments in
//! `ripple_learning/` wrap it.
//!
//! Each agent's autonomy is one of the learning crate's policies, registered
//! to take its actions from Python (`learning::register_for_training`).
//! Python asks for each agent's observation, which is the plugin's own
//! `observe` on the agent's belief, so training and replay see the same
//! numbers; it then sets every action and steps.

use learning::{HoverConfig, SpreadConfig, SpreadPolicy, TrainerActions, hover, spread};
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use ripple_core::plugin::{Autonomy, PluginParams, PluginRegistry};
use ripple_core::{Entity, Mission, Params};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// A mission's learning task, read from an entity group's autonomy.
enum Task {
    Hover(HoverConfig),
    Spread(SpreadConfig),
}

impl Task {
    fn action_size(&self) -> usize {
        match self {
            Task::Hover(_) => hover::ACTION_SIZE,
            Task::Spread(_) => spread::ACTION_SIZE,
        }
    }
}

#[pyclass(unsendable)]
struct Simulation {
    simulation: ripple_core::Simulation,
    actions: TrainerActions,
    /// Each entity group's task, by group index.
    tasks: Vec<Option<Task>>,
}

fn error(error: anyhow::Error) -> PyErr {
    PyRuntimeError::new_err(format!("{error:#}"))
}

impl Simulation {
    fn agent(&self, id: i32) -> anyhow::Result<(&Entity, &Task)> {
        let entity = self
            .simulation
            .entities()
            .iter()
            .find(|entity| entity.id() == id && entity.is_active())
            .ok_or_else(|| anyhow::anyhow!("no active agent {id}"))?;
        let task = self.tasks[entity.group_index() as usize]
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("agent {id} runs no learning policy"))?;
        Ok((entity, task))
    }
}

#[pymethods]
impl Simulation {
    /// Loads `mission` (a YAML file) with its seed replaced by `seed`.
    #[new]
    fn new(mission: PathBuf, seed: u32) -> PyResult<Self> {
        (|| {
            let mut scenario = Mission::load(&mission, ".".as_ref(), &Params::new())?.scenario;
            scenario.run.seed = seed;
            let mut tasks = Vec::new();
            for group in &scenario.entities {
                tasks.push(match group.autonomy.first() {
                    Some(plugin) if plugin.name == "HoverPolicy" => Some(Task::Hover(
                        learning::HoverPolicy::configure(&PluginParams::new(plugin))?,
                    )),
                    Some(plugin) if plugin.name == "SpreadPolicy" => Some(Task::Spread(
                        SpreadPolicy::configure(&PluginParams::new(plugin))?,
                    )),
                    _ => None,
                });
            }
            let actions = TrainerActions::default();
            let mut registry = PluginRegistry::with_builtins();
            learning::register_for_training(&mut registry, &actions)?;
            Ok(Self {
                simulation: ripple_core::Simulation::new(scenario, &registry, 1)?,
                actions,
                tasks,
            })
        })()
        .map_err(error)
    }

    /// IDs of the agents still flying, in ascending order.
    #[getter]
    fn agents(&self) -> Vec<i32> {
        self.simulation
            .entities()
            .iter()
            .filter(|entity| entity.is_active())
            .map(Entity::id)
            .collect()
    }

    #[getter]
    fn time_s(&self) -> f64 {
        self.simulation.time_s()
    }

    /// What agent `id`'s policy sees at the next step.
    fn observe(&self, id: i32) -> PyResult<Vec<f32>> {
        let (entity, task) = self.agent(id).map_err(error)?;
        match task {
            Task::Hover(config) => Ok(config.observe(entity.belief())),
            Task::Spread(config) => config
                .observe(
                    entity.belief(),
                    entity.team_id(),
                    entity
                        .contacts_belief()
                        .iter()
                        .filter(|contact| contact.id != id),
                )
                .map_err(error),
        }
    }

    /// How many numbers agent `id`'s action has, each in [-1, 1].
    fn action_size(&self, id: i32) -> PyResult<usize> {
        Ok(self.agent(id).map_err(error)?.1.action_size())
    }

    /// Runs one tick with these actions, by agent ID; returns false once the
    /// run has ended.
    fn step(&mut self, actions: BTreeMap<i32, Vec<f32>>) -> PyResult<bool> {
        for (id, action) in actions {
            self.actions.set(id, action);
        }
        self.simulation.step().map_err(error)?;
        Ok(self.simulation.termination().is_none())
    }
}

#[pymodule]
fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<Simulation>()
}

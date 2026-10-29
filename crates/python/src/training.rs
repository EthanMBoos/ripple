//! One simulation that Python steps, for training: each `Python` autonomy
//! takes its action from the trainer instead of its policy, and Python asks
//! for each agent's observation through the task's own `observe`.
//!
//! The Gymnasium and PettingZoo environments in `ripple/learning/` wrap
//! this. The `ripple.learning.Trainer` object is shared with every `Python`
//! autonomy as `ctx.trainer`: a task's `initialize` takes its policy from it
//! (the next action set for that entity) when it is there, and from the
//! mission's network file when it is not. Observations come from a second
//! instance of each group's task class, built from the same mission
//! parameters, on the entity's current belief, so the trainer sees after a
//! step what the autonomy will see at the next one.
use anyhow::{Result, anyhow};
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use ripple_core::plugin::{PluginParams, StepTime};
use ripple_core::{Entity, Mission, Params};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::bridge::{self, PythonAutonomy};
use crate::context::{self, Contact, Context, State, Time};

#[pyclass(unsendable, module = "ripple")]
pub struct Simulation {
    simulation: ripple_core::Simulation,
    /// The `ripple.learning.Trainer` the autonomies take actions from.
    trainer: Py<PyAny>,
    /// Each entity group's task, by group index: an instance of the group's
    /// `Python` autonomy class, for `observe`.
    tasks: Vec<Option<Py<PyAny>>>,
}

fn error(error: anyhow::Error) -> PyErr {
    PyRuntimeError::new_err(format!("{error:#}"))
}

impl Simulation {
    fn agent(&self, id: i32) -> Result<(&Entity, &Py<PyAny>)> {
        let entity = self
            .simulation
            .entities()
            .iter()
            .find(|entity| entity.id() == id && entity.is_active())
            .ok_or_else(|| anyhow!("no active agent {id}"))?;
        let task = self.tasks[entity.group_index() as usize]
            .as_ref()
            .ok_or_else(|| anyhow!("agent {id} runs no Python autonomy"))?;
        Ok((entity, task))
    }

    /// What the agent's autonomy would see now: its belief, in the shape the
    /// `Python` autonomy's `ctx` has (without `random` and `inputs`).
    fn context(&self, py: Python<'_>, entity: &Entity) -> Result<Context> {
        let attributes = PyDict::new(py);
        attributes.set_item(
            "time",
            Time::from(StepTime {
                time_s: self.simulation.time_s(),
                dt_s: self.simulation.scenario().run.dt_s,
                tick: self.simulation.step_count() as i64,
            }),
        )?;
        attributes.set_item(
            "entity",
            context::Entity {
                id: entity.id(),
                team_id: entity.team_id(),
            },
        )?;
        attributes.set_item("state", State::new(entity.belief()))?;
        let contacts = entity
            .contacts_belief()
            .iter()
            .filter(|contact| contact.id != entity.id())
            .map(Contact::known);
        attributes.set_item("contacts", pyo3::types::PyList::new(py, contacts)?)?;
        Ok(Context::new(attributes))
    }
}

#[pymethods]
impl Simulation {
    /// Loads `mission` (a YAML file) with its seed replaced by `seed`.
    #[new]
    fn new(py: Python<'_>, mission: PathBuf, seed: u32) -> PyResult<Self> {
        (|| -> Result<Self> {
            let root = crate::PROJECT_ROOT.as_ref();
            let mut scenario = Mission::load(&mission, root, &Params::new())?.scenario;
            scenario.run.seed = seed;
            let trainer = py
                .import("ripple.learning")?
                .getattr("Trainer")?
                .call0()?
                .unbind();
            let mut tasks = Vec::new();
            for group in &scenario.entities {
                tasks.push(match group.autonomy.first() {
                    Some(plugin) if plugin.name == "Python" => {
                        Some(bridge::task(py, &PluginParams::new(plugin))?)
                    }
                    _ => None,
                });
            }
            let registry = crate::registry_with(|registry| {
                // The factory is cloned for every instance; an `Arc` makes
                // the shared trainer cloneable without the lock.
                let trainer = Arc::new(trainer.clone_ref(py));
                registry.register_autonomy_with("Python", move |config| {
                    PythonAutonomy::trained_by(config, &trainer)
                })
            })?;
            Ok(Self {
                simulation: ripple_core::Simulation::new(scenario, &registry, 1)?,
                trainer,
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

    /// What agent `id`'s policy sees at the next step: the task's `observe`
    /// on the agent's current belief.
    fn observe(&self, py: Python<'_>, id: i32) -> PyResult<Py<PyAny>> {
        let (entity, task) = self.agent(id).map_err(error)?;
        let ctx = self.context(py, entity).map_err(error)?;
        Ok(task.bind(py).call_method1("observe", (ctx,))?.unbind())
    }

    /// How many numbers agent `id`'s action has, each in [-1, 1].
    fn action_size(&self, py: Python<'_>, id: i32) -> PyResult<usize> {
        let (_, task) = self.agent(id).map_err(error)?;
        task.bind(py).getattr("action_size")?.extract()
    }

    /// Runs one tick with these actions, by agent ID; returns false once the
    /// run has ended.
    fn step(&mut self, py: Python<'_>, actions: BTreeMap<i32, Vec<f32>>) -> PyResult<bool> {
        let pending = self.trainer.bind(py).getattr("actions")?;
        let pending = pending
            .cast::<PyDict>()
            .map_err(|_| error(anyhow!("Trainer.actions is a dict of actions by entity id")))?;
        for (id, action) in actions {
            pending.set_item(id, action)?;
        }
        let simulation = &mut self.simulation;
        py.detach(|| simulation.step()).map_err(error)?;
        Ok(self.simulation.termination().is_none())
    }
}

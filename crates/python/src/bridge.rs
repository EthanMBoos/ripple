//! The `Python` plugins: a class written in Python fills a plugin slot. One
//! bridge serves every category, the entity's six (autonomy, controller,
//! motion model, sensor, localization, tracking) and the world's three
//! (interaction, network, metrics); the slot decides which class attributes
//! are read, what `ctx` holds, and what the class's answer means.
//!
//! The mission names the class (`class: module.Class`) and passes its other
//! keys to the constructor as keyword arguments. `configure` imports the class
//! and reads its declarations (`ports`, `reading_kind`, or `inputs`), so a
//! missing module or a bad port fails when the mission loads; the object
//! itself is made in `initialize`, when the entity is created, because
//! `Plugin::new` cannot fail and a constructor can. The engine does not know
//! Python is involved: the slot, the loop rate, the port and reading checks,
//! and the random keys are all this plugin's.
//!
//! What each slot's class declares and answers:
//!
//! | Slot | Declares | `ctx` | `step` returns |
//! | --- | --- | --- | --- |
//! | autonomy | `ports = {"outputs": ...}` | `state`, `contacts`, `time`, `entity`, `random` | dict of port values |
//! | controller | `ports = {"inputs": ..., "outputs": ...}` | the same, plus `inputs` | dict of port values |
//! | sensor | `reading_kind = "state" / "contacts" / "other"` | `truth`, `contacts_truth`, `time`, `entity`, `random` | the reading, or `None` |
//! | localization | `inputs = {role: kind}` | `state`, `inputs`, `commands`, `time`, `entity`, `random` | the belief's changed fields, or `None` |
//! | tracking | `inputs = {role: kind}` | `pose`, `contacts`, `inputs`, `time`, `entity`, `random` | the contact list, or `None` |
//! | motion model | `ports = {"inputs": ...}` | `truth`, `inputs`, `forces`, `terrain`, `time`, `entity` | the truth's changed fields, or `None` |
//! | interaction | nothing | `entities`, `nearby`, `time` | effects: `health`, `forces`, `truth`, `events`, `stop` |
//! | network | optional `within_entity = True` | `route(link)` per message instead of `step` | `None` to drop, or the delay in seconds |
//! | metrics | nothing | `events`, `entity_teams`, `contacts_truth`, `time` | nothing; `report(time_s)` returns the report |
//!
//! Interactions, networks, and metrics also get `initialize(ctx)` with the
//! world's `time`, `contacts_truth`, `entity_teams`, and `terrain`.
//!
//! Worker threads call into Python under its lock, which serializes the calls
//! but not their order, so a plugin keeps everything on `self` and draws
//! random numbers from `ctx.random`; see the "Plugins in Python" chapter.
use anyhow::{Context as _, Result, anyhow, bail};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyString};
use ripple_core::plugin::EntityInfo;
use ripple_core::plugin::{
    AgentContext, Autonomy, Commands, ContactList, Controller, Delivery, Frame, Input, Inputs,
    Interaction, InteractionContext, Localization, LocalizationContext, MetricReport, Metrics,
    MotionContext, MotionModel, Network, NetworkContext, Plugin, PluginIo, PluginParams,
    PluginRandom, Port, Ports, ReadingKind, Sensor, SensorContext, StepTime, TeamMetrics, Tracking,
    TrackingContext, Transmission, Unit, Update, WorldContext,
};
use ripple_core::{Event, EventKind, KinematicState, Vec3};
use serde_yaml_ng::{Mapping, Value};
use std::collections::BTreeMap;

use crate::context::{
    Command, Contact, Context, Endpoint, Entity, Link, Nearby, Random, Sample, State, Terrain,
    Time, WorldEntity, contact_list, kinematic_state,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Slot {
    Autonomy,
    Controller,
    Motion,
    Sensor,
    Localization,
    Tracking,
    Interaction,
    Network,
    Metrics,
}

/// The engine events a metrics plugin receives, in the order `ctx.events`
/// lists them within a tick.
const EVENT_KINDS: [EventKind; 6] = [
    EventKind::EntityGenerated,
    EventKind::EntityRemoved,
    EventKind::TeamCollision,
    EventKind::NonTeamCollision,
    EventKind::GroundCollision,
    EventKind::EntityPresentAtEnd,
];

/// A reading a Python sensor published that is neither a state nor a contact
/// list (`reading_kind = "other"`). Only a Python estimator can read it.
pub struct PythonReading(pub Py<PyAny>);

pub struct PythonConfig {
    class_path: String,
    class: Py<PyAny>,
    /// The constructor's keyword arguments, from the mission.
    params: Py<PyDict>,
    ports: Ports,
    /// A controller's input port names, read into `ctx.inputs` each step.
    port_inputs: Vec<String>,
    /// A sensor's reading kind.
    reading_kind: ReadingKind,
    /// An estimator's inputs: each role and the kind of reading it takes.
    readings: Vec<(String, ReadingKind)>,
    /// A network that delivers only within an entity (`within_entity = True`).
    within_entity: bool,
}

/// What every slot shares: the class, its parameters, and the object.
pub struct Bridge {
    class_path: String,
    class: Py<PyAny>,
    params: Py<PyDict>,
    port_inputs: Vec<String>,
    reading_kind: ReadingKind,
    readings: Vec<(String, ReadingKind)>,
    within_entity: bool,
    /// While training (`training.rs`): the `ripple.learning.Trainer` an
    /// autonomy takes its actions from, given to the class as `ctx.trainer`.
    trainer: Option<Py<PyAny>>,
    /// The Python object, from `initialize` on.
    object: Option<Py<PyAny>>,
}

macro_rules! slot_plugin {
    ($name:ident, $slot:expr) => {
        pub struct $name(Bridge);
        impl Plugin for $name {
            type Config = PythonConfig;
            fn configure(params: &PluginParams<'_>) -> Result<PythonConfig> {
                configure(params, $slot)
            }
            fn new(config: &PythonConfig) -> Self {
                Self(Bridge::new(config))
            }
            fn ports(config: &PythonConfig) -> Ports {
                config.ports.clone()
            }
            fn reading_kind(config: &PythonConfig) -> ReadingKind {
                config.reading_kind
            }
            fn inputs(config: &PythonConfig) -> Vec<Input> {
                config
                    .readings
                    .iter()
                    .map(|(role, kind)| Input::new(role, *kind))
                    .collect()
            }
        }
    };
}
slot_plugin!(PythonAutonomy, Slot::Autonomy);

impl PythonAutonomy {
    /// While training: the class gets `ctx.trainer` and takes its actions
    /// from it instead of its policy file.
    pub fn trained_by(config: &PythonConfig, trainer: &Py<PyAny>) -> Self {
        let mut bridge = Bridge::new(config);
        bridge.trainer = Some(Python::attach(|py| trainer.clone_ref(py)));
        Self(bridge)
    }
}

/// An instance of an autonomy's class built from its mission parameters,
/// outside any entity: the training simulation asks it for observations.
pub fn task(py: Python<'_>, params: &PluginParams<'_>) -> Result<Py<PyAny>> {
    let config = configure(params, Slot::Autonomy)?;
    let object = config
        .class
        .bind(py)
        .call((), Some(config.params.bind(py)))
        .map_err(|error| python_error(py, error))
        .with_context(|| format!("Python plugin '{}'", config.class_path))?;
    Ok(object.unbind())
}
slot_plugin!(PythonController, Slot::Controller);
slot_plugin!(PythonSensor, Slot::Sensor);
slot_plugin!(PythonLocalization, Slot::Localization);
slot_plugin!(PythonTracking, Slot::Tracking);
slot_plugin!(PythonMotion, Slot::Motion);
slot_plugin!(PythonInteraction, Slot::Interaction);
slot_plugin!(PythonNetwork, Slot::Network);
slot_plugin!(PythonMetrics, Slot::Metrics);

impl Autonomy for PythonAutonomy {
    fn initialize(&mut self, context: &mut AgentContext<'_>) -> Result<()> {
        self.0.initialize_agent(context)
    }
    fn step(&mut self, context: &mut AgentContext<'_>, io: &mut PluginIo) -> Result<Update> {
        self.0.step_agent(context, io)
    }
    fn close(&mut self, time: StepTime) -> Result<()> {
        self.0.close(time)
    }
}

impl Controller for PythonController {
    fn initialize(&mut self, context: &mut AgentContext<'_>) -> Result<()> {
        self.0.initialize_agent(context)
    }
    fn step(&mut self, context: &mut AgentContext<'_>, io: &mut PluginIo) -> Result<Update> {
        self.0.step_agent(context, io)
    }
    fn close(&mut self, time: StepTime) -> Result<()> {
        self.0.close(time)
    }
}

impl Sensor for PythonSensor {
    fn initialize(&mut self, context: &mut SensorContext<'_>) -> Result<()> {
        self.0.initialize_sensor(context)
    }
    fn step(&mut self, context: &mut SensorContext<'_>) -> Result<Update> {
        self.0.step_sensor(context)
    }
    fn close(&mut self, time: StepTime) -> Result<()> {
        self.0.close(time)
    }
}

impl Localization for PythonLocalization {
    fn initialize(&mut self, context: &mut LocalizationContext<'_>) -> Result<()> {
        self.0.initialize_localization(context)
    }
    fn step(&mut self, context: &mut LocalizationContext<'_>) -> Result<Update> {
        self.0.step_localization(context)
    }
    fn close(&mut self, time: StepTime) -> Result<()> {
        self.0.close(time)
    }
}

impl Tracking for PythonTracking {
    fn initialize(&mut self, context: &mut TrackingContext<'_>) -> Result<()> {
        self.0.initialize_tracking(context)
    }
    fn step(&mut self, context: &mut TrackingContext<'_>) -> Result<Update> {
        self.0.step_tracking(context)
    }
    fn close(&mut self, time: StepTime) -> Result<()> {
        self.0.close(time)
    }
}

impl MotionModel for PythonMotion {
    fn initialize(&mut self, context: &mut MotionContext<'_>) -> Result<()> {
        self.0.initialize_motion(context)
    }
    fn step(&mut self, context: &mut MotionContext<'_>, io: &mut PluginIo) -> Result<Update> {
        self.0.step_motion(context, io)
    }
    fn close(&mut self, time: StepTime) -> Result<()> {
        self.0.close(time)
    }
}

impl Interaction for PythonInteraction {
    fn initialize(&mut self, context: &mut WorldContext<'_>) -> Result<()> {
        self.0.initialize_world(context)
    }
    fn step(&mut self, context: &mut InteractionContext<'_>) -> Result<Update> {
        self.0.step_interaction(context)
    }
    fn close(&mut self, time: StepTime) -> Result<()> {
        self.0.close(time)
    }
}

impl Network for PythonNetwork {
    fn initialize(&mut self, context: &mut WorldContext<'_>) -> Result<()> {
        self.0.initialize_world(context)
    }
    fn step(&mut self, context: &mut NetworkContext<'_, '_>) -> Result<Update> {
        self.0.step_network(context)
    }
    fn close(&mut self, time: StepTime) -> Result<()> {
        self.0.close(time)
    }
}

impl Metrics for PythonMetrics {
    fn initialize(&mut self, context: &mut WorldContext<'_>) -> Result<()> {
        for kind in EVENT_KINDS {
            context
                .messages
                .subscribe::<Event>("GlobalNetwork", kind.topic())?;
        }
        self.0.initialize_world(context)
    }
    fn step(&mut self, context: &mut WorldContext<'_>) -> Result<Update> {
        self.0.step_metrics(context)
    }
    fn report(&self, time_s: f64) -> MetricReport {
        self.0.report(time_s)
    }
    fn close(&mut self, time: StepTime) -> Result<()> {
        self.0.close(time)
    }
}

fn configure(params: &PluginParams<'_>, slot: Slot) -> Result<PythonConfig> {
    let mut values: Mapping = params.parse()?;
    let class_path = match values.remove("class") {
        Some(Value::String(path)) => path,
        Some(_) => bail!("Python: `class` must be a string"),
        None => bail!("Python: `class` names the plugin class, as `module.Class`"),
    };
    Python::attach(|py| -> Result<PythonConfig> {
        let (module, name) = class_path
            .rsplit_once('.')
            .with_context(|| format!("`class: {class_path}` is not `module.Class`"))?;
        let class = py
            .import(module)
            .and_then(|module| module.getattr(name))
            .map_err(|error| python_error(py, error))?;
        let (mut ports, mut port_inputs) = (Ports::default(), Vec::new());
        let mut reading_kind = ReadingKind::Other;
        let mut readings = Vec::new();
        let mut within_entity = false;
        match slot {
            Slot::Autonomy | Slot::Controller | Slot::Motion => {
                (ports, port_inputs) = declared_ports(&class, slot)?
            }
            Slot::Sensor => reading_kind = declared_reading_kind(&class)?,
            Slot::Localization | Slot::Tracking => readings = declared_inputs(&class)?,
            Slot::Network => {
                within_entity = match class.getattr("within_entity") {
                    Ok(flag) => flag.extract().context("`within_entity` is a bool")?,
                    Err(_) => false,
                }
            }
            Slot::Interaction | Slot::Metrics => {}
        }
        let params = PyDict::new(py);
        for (key, value) in &values {
            let key = key.as_str().context("parameter keys must be strings")?;
            params.set_item(key, to_python(py, value)?)?;
        }
        Ok(PythonConfig {
            class_path: class_path.clone(),
            class: class.unbind(),
            params: params.unbind(),
            ports,
            port_inputs,
            reading_kind,
            readings,
            within_entity,
        })
    })
    .with_context(|| format!("Python plugin '{class_path}'"))
}

impl Bridge {
    fn new(config: &PythonConfig) -> Self {
        Python::attach(|py| Self {
            class_path: config.class_path.clone(),
            class: config.class.clone_ref(py),
            params: config.params.clone_ref(py),
            port_inputs: config.port_inputs.clone(),
            reading_kind: config.reading_kind,
            readings: config.readings.clone(),
            within_entity: config.within_entity,
            trainer: None,
            object: None,
        })
    }

    fn describe(&self, entity_id: i32) -> String {
        format!("Python plugin '{}' on entity {entity_id}", self.class_path)
    }

    /// `cls(**params)`, kept for the calls that follow.
    fn construct<'py>(&mut self, py: Python<'py>) -> Result<Bound<'py, PyAny>> {
        let object = self
            .class
            .bind(py)
            .call((), Some(self.params.bind(py)))
            .map_err(|error| python_error(py, error))?;
        self.object = Some(object.clone().unbind());
        Ok(object)
    }

    fn object<'py>(&self, py: Python<'py>) -> Result<&Bound<'py, PyAny>> {
        Ok(self.object.as_ref().context("not initialized")?.bind(py))
    }

    /// Calls `method(ctx)` if the object has it; `None` if it does not, or
    /// if the method returned `None`.
    fn call<'py>(
        &self,
        py: Python<'py>,
        object: &Bound<'py, PyAny>,
        method: &str,
        ctx: Context,
    ) -> Result<Option<Bound<'py, PyAny>>> {
        if !object.hasattr(method)? {
            return Ok(None);
        }
        let result = object
            .call_method1(method, (ctx,))
            .map_err(|error| python_error(py, error))?;
        Ok((!result.is_none()).then_some(result))
    }

    /// The attributes every entity slot's `ctx` has, but a motion model's
    /// `random`: the engine gives a motion model no random numbers.
    fn base<'py>(
        &self,
        py: Python<'py>,
        entity: EntityInfo,
        time: StepTime,
        random: Option<PluginRandom>,
    ) -> Result<Bound<'py, PyDict>> {
        let attributes = PyDict::new(py);
        attributes.set_item("time", Time::from(time))?;
        attributes.set_item(
            "entity",
            Entity {
                id: entity.id,
                team_id: entity.team_id,
            },
        )?;
        if let Some(random) = random {
            attributes.set_item("random", Random { random })?;
        }
        Ok(attributes)
    }

    /// A controller's or motion model's input port values, by name.
    fn port_values<'py>(
        &self,
        py: Python<'py>,
        io: Option<&PluginIo>,
    ) -> Result<Bound<'py, PyDict>> {
        let inputs = PyDict::new(py);
        if let Some(io) = io {
            for name in &self.port_inputs {
                inputs.set_item(name, io.read(name)?)?;
            }
        }
        Ok(inputs)
    }

    // Autonomy and controller.

    fn agent_context(
        &self,
        py: Python<'_>,
        context: &AgentContext<'_>,
        io: Option<&PluginIo>,
    ) -> Result<Context> {
        let attributes = self.base(py, context.entity, context.time, Some(context.random))?;
        attributes.set_item("state", State::new(context.state))?;
        let contacts = context.contacts.iter().map(Contact::known);
        attributes.set_item("contacts", PyList::new(py, contacts)?)?;
        attributes.set_item("inputs", self.port_values(py, io)?)?;
        if let Some(trainer) = &self.trainer {
            attributes.set_item("trainer", trainer.clone_ref(py))?;
        }
        Ok(Context::new(attributes))
    }

    fn initialize_agent(&mut self, context: &mut AgentContext<'_>) -> Result<()> {
        Python::attach(|py| -> Result<()> {
            let object = self.construct(py)?;
            let ctx = self.agent_context(py, context, None)?;
            self.call(py, &object, "initialize", ctx)?;
            Ok(())
        })
        .with_context(|| self.describe(context.entity.id))
    }

    fn step_agent(&mut self, context: &mut AgentContext<'_>, io: &mut PluginIo) -> Result<Update> {
        Python::attach(|py| -> Result<Update> {
            let object = self.object(py)?;
            let ctx = self.agent_context(py, context, Some(io))?;
            let Some(result) = self.call(py, object, "step", ctx)? else {
                return Ok(Update::Applied);
            };
            let writes = result
                .cast::<PyDict>()
                .map_err(|_| anyhow!("step must return a dict of port values, or None"))?;
            for (name, value) in writes.iter() {
                let name = name
                    .cast::<PyString>()
                    .map_err(|_| anyhow!("port names are strings"))?
                    .to_cow()?;
                let value: f64 = value
                    .extract()
                    .with_context(|| format!("port '{name}' needs a number"))?;
                io.write(&name, value)?;
            }
            Ok(Update::Applied)
        })
        .with_context(|| self.describe(context.entity.id))
    }

    // Sensor.

    fn sensor_context(&self, py: Python<'_>, context: &SensorContext<'_>) -> Result<Context> {
        let attributes = self.base(py, context.entity, context.time, Some(context.random))?;
        attributes.set_item("truth", State::new(context.truth))?;
        // Every other active vehicle's truth, as the engine's contact sensors see it.
        let sampled_at_s = context.time.time_s + context.time.dt_s;
        let contacts = context
            .contacts_truth
            .iter()
            .filter(|snapshot| snapshot.active && snapshot.id != context.entity.id)
            .map(|snapshot| Contact::truth(snapshot, sampled_at_s));
        attributes.set_item("contacts_truth", PyList::new(py, contacts)?)?;
        Ok(Context::new(attributes))
    }

    /// Hands the class's answer to the estimators as the declared kind of reading.
    fn publish(&self, context: &mut SensorContext<'_>, reading: Bound<'_, PyAny>) -> Result<()> {
        match self.reading_kind {
            ReadingKind::State => {
                context.publish_reading(kinematic_state(context.truth, &reading)?)
            }
            ReadingKind::Contacts => {
                let sampled_at_s = context.time.time_s + context.time.dt_s;
                context.publish_reading(contact_list(&reading, sampled_at_s)?)
            }
            ReadingKind::Other => context.publish_reading(PythonReading(reading.unbind())),
        }
    }

    fn initialize_sensor(&mut self, context: &mut SensorContext<'_>) -> Result<()> {
        Python::attach(|py| -> Result<()> {
            let object = self.construct(py)?;
            let ctx = self.sensor_context(py, context)?;
            if let Some(reading) = self.call(py, &object, "initialize", ctx)? {
                self.publish(context, reading)?;
            }
            Ok(())
        })
        .with_context(|| self.describe(context.entity.id))
    }

    fn step_sensor(&mut self, context: &mut SensorContext<'_>) -> Result<Update> {
        Python::attach(|py| -> Result<Update> {
            let object = self.object(py)?;
            let ctx = self.sensor_context(py, context)?;
            if let Some(reading) = self.call(py, object, "step", ctx)? {
                self.publish(context, reading)?;
            }
            Ok(Update::Applied)
        })
        .with_context(|| self.describe(context.entity.id))
    }

    // Localization and tracking.

    /// `ctx.inputs`: each declared input's latest reading as a `Sample`, or `None`.
    fn samples<'py>(&self, py: Python<'py>, inputs: &Inputs<'_>) -> Result<Bound<'py, PyDict>> {
        let samples = PyDict::new(py);
        for (role, kind) in &self.readings {
            let sample = match kind {
                ReadingKind::State => inputs.latest::<KinematicState>(role)?.map(|sample| {
                    let value = Py::new(py, State::new(sample.value))?.into_any();
                    Ok::<_, PyErr>((value, sample.sampled_at_s, sample.fresh))
                }),
                ReadingKind::Contacts => inputs.latest::<ContactList>(role)?.map(|sample| {
                    let contacts = sample.value.iter().map(Contact::known);
                    let value = PyList::new(py, contacts)?.into_any().unbind();
                    Ok((value, sample.sampled_at_s, sample.fresh))
                }),
                ReadingKind::Other => inputs.latest::<PythonReading>(role)?.map(|sample| {
                    Ok((
                        sample.value.0.clone_ref(py),
                        sample.sampled_at_s,
                        sample.fresh,
                    ))
                }),
            };
            match sample.transpose()? {
                Some((value, sampled_at_s, fresh)) => samples.set_item(
                    role,
                    Sample {
                        value,
                        sampled_at_s,
                        fresh,
                    },
                )?,
                None => samples.set_item(role, py.None())?,
            }
        }
        Ok(samples)
    }

    /// `ctx.commands`: what the motion model read on each substep.
    fn commands<'py>(
        &self,
        py: Python<'py>,
        commands: &Commands<'_>,
    ) -> Result<Bound<'py, PyList>> {
        let list = PyList::empty(py);
        for substep in 0..commands.substeps() {
            let values = PyDict::new(py);
            for name in commands.names() {
                values.set_item(name, commands.get(substep, name)?)?;
            }
            list.append(Command {
                time: Py::new(py, Time::from(commands.time(substep)))?,
                values: values.unbind(),
            })?;
        }
        Ok(list)
    }

    fn localization_context(
        &self,
        py: Python<'_>,
        context: &LocalizationContext<'_>,
    ) -> Result<Context> {
        let attributes = self.base(py, context.entity, context.time, Some(context.random))?;
        attributes.set_item("state", State::new(context.state))?;
        attributes.set_item("inputs", self.samples(py, &context.inputs)?)?;
        attributes.set_item("commands", self.commands(py, &context.commands)?)?;
        Ok(Context::new(attributes))
    }

    fn initialize_localization(&mut self, context: &mut LocalizationContext<'_>) -> Result<()> {
        Python::attach(|py| -> Result<()> {
            let object = self.construct(py)?;
            let ctx = self.localization_context(py, context)?;
            if let Some(state) = self.call(py, &object, "initialize", ctx)? {
                *context.state = kinematic_state(context.state, &state)?;
            }
            Ok(())
        })
        .with_context(|| self.describe(context.entity.id))
    }

    fn step_localization(&mut self, context: &mut LocalizationContext<'_>) -> Result<Update> {
        Python::attach(|py| -> Result<Update> {
            let object = self.object(py)?;
            let ctx = self.localization_context(py, context)?;
            if let Some(state) = self.call(py, object, "step", ctx)? {
                *context.state = kinematic_state(context.state, &state)?;
            }
            Ok(Update::Applied)
        })
        .with_context(|| self.describe(context.entity.id))
    }

    fn tracking_context(&self, py: Python<'_>, context: &TrackingContext<'_>) -> Result<Context> {
        let attributes = self.base(py, context.entity, context.time, Some(context.random))?;
        attributes.set_item("pose", State::new(context.pose))?;
        let contacts = context.contacts.iter().map(Contact::known);
        attributes.set_item("contacts", PyList::new(py, contacts)?)?;
        attributes.set_item("inputs", self.samples(py, &context.inputs)?)?;
        Ok(Context::new(attributes))
    }

    fn initialize_tracking(&mut self, context: &mut TrackingContext<'_>) -> Result<()> {
        Python::attach(|py| -> Result<()> {
            let object = self.construct(py)?;
            let ctx = self.tracking_context(py, context)?;
            if let Some(contacts) = self.call(py, &object, "initialize", ctx)? {
                let estimate_s = context.time.time_s + context.time.dt_s;
                *context.contacts = contact_list(&contacts, estimate_s)?;
            }
            Ok(())
        })
        .with_context(|| self.describe(context.entity.id))
    }

    fn step_tracking(&mut self, context: &mut TrackingContext<'_>) -> Result<Update> {
        Python::attach(|py| -> Result<Update> {
            let object = self.object(py)?;
            let ctx = self.tracking_context(py, context)?;
            if let Some(contacts) = self.call(py, object, "step", ctx)? {
                let estimate_s = context.time.time_s + context.time.dt_s;
                *context.contacts = contact_list(&contacts, estimate_s)?;
            }
            Ok(Update::Applied)
        })
        .with_context(|| self.describe(context.entity.id))
    }

    // Motion model.

    fn motion_context(
        &self,
        py: Python<'_>,
        context: &MotionContext<'_>,
        io: Option<&PluginIo>,
    ) -> Result<Context> {
        let attributes = self.base(py, context.entity, context.time, None)?;
        attributes.set_item("truth", State::new(context.truth))?;
        attributes.set_item("inputs", self.port_values(py, io)?)?;
        let forces = context
            .applied_forces_world_n
            .iter()
            .map(|f| (f.x, f.y, f.z));
        attributes.set_item("forces", PyList::new(py, forces)?)?;
        attributes.set_item("terrain", terrain(context.terrain))?;
        Ok(Context::new(attributes))
    }

    fn initialize_motion(&mut self, context: &mut MotionContext<'_>) -> Result<()> {
        Python::attach(|py| -> Result<()> {
            let object = self.construct(py)?;
            let ctx = self.motion_context(py, context, None)?;
            if let Some(truth) = self.call(py, &object, "initialize", ctx)? {
                *context.truth = kinematic_state(context.truth, &truth)?;
            }
            Ok(())
        })
        .with_context(|| self.describe(context.entity.id))
    }

    fn step_motion(
        &mut self,
        context: &mut MotionContext<'_>,
        io: &mut PluginIo,
    ) -> Result<Update> {
        Python::attach(|py| -> Result<Update> {
            let object = self.object(py)?;
            let ctx = self.motion_context(py, context, Some(io))?;
            if let Some(truth) = self.call(py, object, "step", ctx)? {
                *context.truth = kinematic_state(context.truth, &truth)?;
            }
            Ok(Update::Applied)
        })
        .with_context(|| self.describe(context.entity.id))
    }

    // Interaction, network, metrics.

    fn describe_world(&self) -> String {
        format!("Python plugin '{}'", self.class_path)
    }

    /// The world at `initialize`: `time`, every entity's truth, team
    /// membership, and the terrain.
    fn world_context(&self, py: Python<'_>, context: &WorldContext<'_>) -> Result<Context> {
        let attributes = PyDict::new(py);
        attributes.set_item("time", Time::from(context.time))?;
        let contacts = context
            .contacts_truth
            .iter()
            .map(|snapshot| Contact::truth(snapshot, context.time.time_s));
        attributes.set_item("contacts_truth", PyList::new(py, contacts)?)?;
        attributes.set_item("entity_teams", context.entity_teams.clone())?;
        attributes.set_item("terrain", terrain(context.terrain))?;
        Ok(Context::new(attributes))
    }

    fn initialize_world(&mut self, context: &mut WorldContext<'_>) -> Result<()> {
        Python::attach(|py| -> Result<()> {
            let object = self.construct(py)?;
            let ctx = self.world_context(py, context)?;
            self.call(py, &object, "initialize", ctx)?;
            Ok(())
        })
        .with_context(|| self.describe_world())
    }

    fn step_interaction(&mut self, context: &mut InteractionContext<'_>) -> Result<Update> {
        Python::attach(|py| -> Result<Update> {
            let object = self.object(py)?;
            let attributes = PyDict::new(py);
            attributes.set_item("time", Time::from(context.time))?;
            let entities = context
                .entities
                .iter()
                .enumerate()
                .map(|(index, entity)| WorldEntity::new(py, index, entity))
                .collect::<PyResult<Vec<_>>>()?;
            attributes.set_item("entities", PyList::new(py, entities)?)?;
            attributes.set_item("nearby", Nearby::new(context.entities))?;
            match self.call(py, object, "step", Context::new(attributes))? {
                Some(effects) => apply_effects(context, &effects),
                None => Ok(Update::Applied),
            }
        })
        .with_context(|| self.describe_world())
    }

    /// Asks the class `route(link)` for every message: `None` drops it, a
    /// number delivers it after that many seconds.
    fn step_network(&mut self, context: &mut NetworkContext<'_, '_>) -> Result<Update> {
        Python::attach(|py| -> Result<Update> {
            let object = self.object(py)?.clone();
            let contacts = context
                .contacts_truth
                .iter()
                .map(|snapshot| Contact::truth(snapshot, context.time.time_s));
            let contacts = PyList::new(py, contacts)?.unbind();
            let decide = |link: &Transmission<'_>, random: &PluginRandom| {
                let link = Link {
                    time: Py::new(py, Time::from(link.time))?,
                    sender: Py::new(py, Endpoint::new(link.sender))?,
                    receiver: Py::new(py, Endpoint::new(link.receiver))?,
                    topic: link.topic.to_owned(),
                    sequence: link.sequence,
                    random: Py::new(py, Random { random: *random })?,
                    contacts_truth: contacts.clone_ref(py),
                };
                let answer = object
                    .call_method1("route", (link,))
                    .map_err(|error| python_error(py, error))?;
                if answer.is_none() {
                    return Ok(Delivery::Drop);
                }
                let delay_s: f64 = answer
                    .extract()
                    .context("route returns None to drop the message, or the delay in seconds")?;
                Ok(Delivery::After { delay_s })
            };
            if self.within_entity {
                context.route_within_entity(decide)
            } else {
                context.route(decide)
            }
        })
        .with_context(|| self.describe_world())
    }

    fn step_metrics(&mut self, context: &mut WorldContext<'_>) -> Result<Update> {
        let mut events = Vec::new();
        for kind in EVENT_KINDS {
            for message in context
                .messages
                .receive::<Event>("GlobalNetwork", kind.topic())?
            {
                events.push(crate::context::Event {
                    time_s: message.value.time_s,
                    kind: kind.topic().to_owned(),
                    entity_ids: message.value.entity_ids.clone(),
                    delivered_at_s: message.delivered_at_s,
                });
            }
        }
        Python::attach(|py| -> Result<Update> {
            let object = self.object(py)?;
            let ctx = self.world_context(py, context)?;
            ctx.set(py, "events", PyList::new(py, events)?)?;
            self.call(py, object, "step", ctx)?;
            Ok(Update::Applied)
        })
        .with_context(|| self.describe_world())
    }

    /// The class's `report(time_s)`: `{"headers": [...], "teams": {team_id:
    /// {"score": s, "values": {name: value}}}}`. The engine's report cannot
    /// fail, so an error here stops the run with the Python error.
    fn report(&self, time_s: f64) -> MetricReport {
        Python::attach(|py| -> Result<MetricReport> {
            let object = self.object(py)?;
            let answer = object
                .call_method1("report", (time_s,))
                .map_err(|error| python_error(py, error))?;
            let answer = answer
                .cast::<PyDict>()
                .map_err(|_| anyhow!("report returns a dict with `headers` and `teams`"))?;
            let headers: Vec<String> = match answer.get_item("headers")? {
                Some(headers) => headers.extract().context("`headers` is a list of names")?,
                None => Vec::new(),
            };
            let mut report = MetricReport {
                headers,
                teams: BTreeMap::new(),
            };
            let teams = answer
                .get_item("teams")?
                .context("report needs a `teams` entry")?;
            let teams = teams
                .cast::<PyDict>()
                .map_err(|_| anyhow!("`teams` is a dict by team id"))?;
            for (team, metrics) in teams.iter() {
                let team: i32 = team.extract()?;
                let metrics = metrics
                    .cast::<PyDict>()
                    .map_err(|_| anyhow!("team {team}: a dict with `score` and `values`"))?;
                let score: f64 = match metrics.get_item("score")? {
                    Some(score) => score.extract()?,
                    None => 0.0,
                };
                let values: BTreeMap<String, f64> = match metrics.get_item("values")? {
                    Some(values) => values.extract()?,
                    None => BTreeMap::new(),
                };
                report.teams.insert(team, TeamMetrics { score, values });
            }
            Ok(report)
        })
        .with_context(|| self.describe_world())
        .unwrap_or_else(|error| panic!("{error:#}"))
    }

    fn close(&mut self, time: StepTime) -> Result<()> {
        let Some(object) = &self.object else {
            return Ok(());
        };
        Python::attach(|py| -> Result<()> {
            let object = object.bind(py);
            if object.hasattr("close")? {
                object
                    .call_method1("close", (Time::from(time),))
                    .map_err(|error| python_error(py, error))?;
            }
            Ok(())
        })
        .with_context(|| format!("Python plugin '{}'", self.class_path))
    }
}

/// The mission's terrain as a Python object, or `None` without one.
fn terrain(terrain: Option<&ripple_core::Terrain>) -> Option<Terrain> {
    terrain.map(|terrain| Terrain {
        terrain: terrain.clone(),
    })
}

/// What an interaction's `step` asked for: `health` and `forces` by entity
/// id, `truth` as each entity's changed state fields, `events` as
/// `(kind, [ids])` pairs, and `stop` to end the run.
fn apply_effects(
    context: &mut InteractionContext<'_>,
    effects: &Bound<'_, PyAny>,
) -> Result<Update> {
    let effects = effects
        .cast::<PyDict>()
        .map_err(|_| anyhow!("step returns a dict of effects, or None"))?;
    let mut update = Update::Applied;
    for (key, value) in effects.iter() {
        let key: String = key.extract()?;
        match key.as_str() {
            "health" => {
                let health: BTreeMap<i32, i32> =
                    value.extract().context("`health` is {id: int}")?;
                for (id, health) in health {
                    entity(context, id)?.set_health(health);
                }
            }
            "forces" => {
                let forces: BTreeMap<i32, (f64, f64, f64)> =
                    value.extract().context("`forces` is {id: (x, y, z)}")?;
                for (id, (x, y, z)) in forces {
                    entity(context, id)?.apply_force(Vec3::new(x, y, z));
                }
            }
            "truth" => {
                let states = value
                    .cast::<PyDict>()
                    .map_err(|_| anyhow!("`truth` is {{id: state}}"))?;
                for (id, state) in states.iter() {
                    let entity = entity(context, id.extract()?)?;
                    *entity.truth_mut() = kinematic_state(entity.truth(), &state)?;
                }
            }
            "events" => {
                let events: Vec<(String, Vec<i32>)> =
                    value.extract().context("`events` is [(kind, [ids])]")?;
                for (kind, ids) in events {
                    context.emit(event_kind(&kind)?, ids)?;
                }
            }
            "stop" => {
                if value.extract::<bool>().context("`stop` is a bool")? {
                    update = Update::Stop;
                }
            }
            other => bail!("unknown effect '{other}' (health, forces, truth, events, stop)"),
        }
    }
    Ok(update)
}

fn entity<'a>(
    context: &'a mut InteractionContext<'_>,
    id: i32,
) -> Result<&'a mut ripple_core::Entity> {
    context
        .entities
        .iter_mut()
        .find(|entity| entity.id() == id)
        .with_context(|| format!("no entity {id}"))
}

/// The events an interaction may emit, by topic name.
fn event_kind(name: &str) -> Result<EventKind> {
    Ok(match name {
        "TeamCollision" => EventKind::TeamCollision,
        "NonTeamCollision" => EventKind::NonTeamCollision,
        "GroundCollision" => EventKind::GroundCollision,
        other => {
            bail!("unknown event '{other}' (TeamCollision, NonTeamCollision, GroundCollision)")
        }
    })
}

/// The class's `ports` attribute, `{"outputs": {name: (unit, frame), ...}}`,
/// with an `inputs` entry too for a controller, and only `inputs` for a
/// motion model. Also the input names, in declaration order, for
/// `ctx.inputs`.
fn declared_ports(class: &Bound<'_, PyAny>, slot: Slot) -> Result<(Ports, Vec<String>)> {
    let declared = class
        .getattr("ports")
        .map_err(|_| anyhow!("the class needs a `ports` attribute"))?;
    let declared = declared
        .cast::<PyDict>()
        .map_err(|_| anyhow!("`ports` is a dict with an `outputs` entry"))?;
    let mut ports = Ports::default();
    let mut inputs = Vec::new();
    for (direction, entries) in declared.iter() {
        let direction: String = direction.extract()?;
        match (direction.as_str(), slot) {
            ("outputs", Slot::Motion) => bail!("a motion model's `ports` has no `outputs`"),
            ("inputs", Slot::Autonomy) => bail!("an autonomy's `ports` has no `inputs`"),
            ("outputs", _) | ("inputs", _) => {}
            _ => bail!("`ports` has `outputs` and, for a controller, `inputs`, not `{direction}`"),
        }
        let entries = entries
            .cast::<PyDict>()
            .map_err(|_| anyhow!("`ports[\"{direction}\"]` is a dict of name: (unit, frame)"))?;
        for (name, spec) in entries.iter() {
            let name: String = name.extract()?;
            let (unit, frame): (String, String) = spec
                .extract()
                .with_context(|| format!("port '{name}' is (unit, frame)"))?;
            let port = Port::new(&name, unit_from(&unit)?, frame_from(&frame)?);
            if direction == "inputs" {
                inputs.push(name);
                ports = ports.input(port);
            } else {
                ports = ports.output(port);
            }
        }
    }
    Ok((ports, inputs))
}

/// A sensor's `reading_kind` attribute.
fn declared_reading_kind(class: &Bound<'_, PyAny>) -> Result<ReadingKind> {
    let kind: String = class
        .getattr("reading_kind")
        .map_err(|_| anyhow!("a sensor class needs a `reading_kind` attribute"))?
        .extract()
        .map_err(|_| anyhow!("`reading_kind` is a string"))?;
    reading_kind_from(&kind)
}

/// An estimator's `inputs` attribute: `{role: kind, ...}`.
fn declared_inputs(class: &Bound<'_, PyAny>) -> Result<Vec<(String, ReadingKind)>> {
    let declared = class
        .getattr("inputs")
        .map_err(|_| anyhow!("an estimator class needs an `inputs` attribute"))?;
    let declared = declared
        .cast::<PyDict>()
        .map_err(|_| anyhow!("`inputs` is a dict of role: kind"))?;
    let mut inputs = Vec::new();
    for (role, kind) in declared.iter() {
        let role: String = role.extract()?;
        let kind: String = kind
            .extract()
            .with_context(|| format!("input '{role}' names a kind: state, contacts, or other"))?;
        inputs.push((role, reading_kind_from(&kind)?));
    }
    Ok(inputs)
}

fn reading_kind_from(text: &str) -> Result<ReadingKind> {
    Ok(match text {
        "state" => ReadingKind::State,
        "contacts" => ReadingKind::Contacts,
        "other" => ReadingKind::Other,
        other => bail!("unknown reading kind '{other}' (state, contacts, other)"),
    })
}

fn unit_from(text: &str) -> Result<Unit> {
    Ok(match text {
        "none" => Unit::Dimensionless,
        "m" => Unit::Meters,
        "m/s" => Unit::MetersPerSecond,
        "m/s^2" => Unit::MetersPerSecondSquared,
        "N" => Unit::Newtons,
        "rad" => Unit::Radians,
        "rad/s" => Unit::RadiansPerSecond,
        other => bail!("unknown unit '{other}' (m, m/s, m/s^2, N, rad, rad/s, none)"),
    })
}

fn frame_from(text: &str) -> Result<Frame> {
    Ok(match text {
        "none" => Frame::None,
        "world" => Frame::World,
        "body" => Frame::Body,
        "model" => Frame::Model,
        other => bail!("unknown frame '{other}' (world, body, model, none)"),
    })
}

/// A mission value as a Python object: numbers, strings, booleans, lists, dicts.
fn to_python<'py>(py: Python<'py>, value: &Value) -> Result<Bound<'py, PyAny>> {
    Ok(match value {
        Value::Null => py.None().into_bound(py),
        Value::Bool(flag) => flag.into_pyobject(py)?.to_owned().into_any(),
        Value::Number(number) => match number.as_i64() {
            Some(integer) => integer.into_pyobject(py)?.into_any(),
            None => number
                .as_f64()
                .context("parameter numbers must be finite")?
                .into_pyobject(py)?
                .into_any(),
        },
        Value::String(text) => text.into_pyobject(py)?.into_any(),
        Value::Sequence(items) => {
            let list = PyList::empty(py);
            for item in items {
                list.append(to_python(py, item)?)?;
            }
            list.into_any()
        }
        Value::Mapping(entries) => {
            let dict = PyDict::new(py);
            for (key, item) in entries {
                let key = key.as_str().context("parameter keys must be strings")?;
                dict.set_item(key, to_python(py, item)?)?;
            }
            dict.into_any()
        }
        Value::Tagged(_) => bail!("plugin parameters cannot contain YAML tags"),
    })
}

/// A Python exception as an error: its type and message, then the traceback.
fn python_error(py: Python<'_>, error: PyErr) -> anyhow::Error {
    let mut message = error.to_string();
    if let Some(traceback) = error.traceback(py)
        && let Ok(text) = traceback.format()
    {
        message.push('\n');
        message.push_str(text.trim_end());
    }
    anyhow!(message)
}

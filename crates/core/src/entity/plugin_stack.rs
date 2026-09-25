//! Type erasure and scheduling stay inside the engine, not in plugin author code.
use crate::plugin::*;
use crate::{EntitySnapshot, KinematicState};
use crate::{common::Rate, parse::EntityConfig};
use crate::{common::variable_io::Signals, plugin_manager::CompiledPlugin};
use anyhow::{Context, Result, ensure};

// These runtime-only traits forward calls to the public plugin traits.
// Once the tool is more established, investigate separating construction from
// runtime methods so we might remove this forwarding layer across all seven
// categories. Keep the current plugin API for now; this is not a required refactor.
pub(crate) trait AgentBehavior: Send {
    fn close(&mut self, time: StepTime) -> Result<()>;
    fn initialize(&mut self, context: &mut AgentContext<'_>) -> Result<()>;
    fn step(&mut self, context: &mut AgentContext<'_>, io: &mut PluginIo) -> Result<Update>;
}
pub(crate) struct AutonomyAdapter<T>(pub T);
impl<T: Autonomy> AgentBehavior for AutonomyAdapter<T> {
    fn close(&mut self, time: StepTime) -> Result<()> {
        self.0.close(time)
    }
    fn initialize(&mut self, context: &mut AgentContext<'_>) -> Result<()> {
        self.0.initialize(context)
    }
    fn step(&mut self, context: &mut AgentContext<'_>, io: &mut PluginIo) -> Result<Update> {
        self.0.step(context, io)
    }
}
pub(crate) struct ControllerAdapter<T>(pub T);
impl<T: Controller> AgentBehavior for ControllerAdapter<T> {
    fn close(&mut self, time: StepTime) -> Result<()> {
        self.0.close(time)
    }
    fn initialize(&mut self, context: &mut AgentContext<'_>) -> Result<()> {
        self.0.initialize(context)
    }
    fn step(&mut self, context: &mut AgentContext<'_>, io: &mut PluginIo) -> Result<Update> {
        self.0.step(context, io)
    }
}
pub(crate) trait MotionBehavior: Send {
    fn close(&mut self, time: StepTime) -> Result<()>;
    fn initialize(&mut self, context: &mut MotionContext<'_>) -> Result<()>;
    fn step(&mut self, context: &mut MotionContext<'_>, io: &mut PluginIo) -> Result<Update>;
}
impl<T: MotionModel> MotionBehavior for T {
    fn close(&mut self, time: StepTime) -> Result<()> {
        Plugin::close(self, time)
    }
    fn initialize(&mut self, context: &mut MotionContext<'_>) -> Result<()> {
        MotionModel::initialize(self, context)
    }
    fn step(&mut self, context: &mut MotionContext<'_>, io: &mut PluginIo) -> Result<Update> {
        MotionModel::step(self, context, io)
    }
}
pub(crate) trait SensorBehavior: Send {
    fn close(&mut self, time: StepTime) -> Result<()>;
    fn initialize(&mut self, context: &mut SensorContext<'_>) -> Result<()>;
    fn step(&mut self, context: &mut SensorContext<'_>) -> Result<Update>;
}
impl<T: Sensor> SensorBehavior for T {
    fn close(&mut self, time: StepTime) -> Result<()> {
        Plugin::close(self, time)
    }
    fn initialize(&mut self, context: &mut SensorContext<'_>) -> Result<()> {
        Sensor::initialize(self, context)
    }
    fn step(&mut self, context: &mut SensorContext<'_>) -> Result<Update> {
        Sensor::step(self, context)
    }
}
pub(crate) struct CompiledStack {
    autonomies: Vec<CompiledPlugin<dyn AgentBehavior>>,
    controllers: Vec<CompiledPlugin<dyn AgentBehavior>>,
    motion: CompiledPlugin<dyn MotionBehavior>,
    sensors: Vec<CompiledPlugin<dyn SensorBehavior>>,
    sensor_identities: Vec<String>,
}
impl CompiledStack {
    pub fn compile(config: &EntityConfig, registry: &PluginRegistry) -> Result<Self> {
        let autonomies = config
            .autonomy
            .iter()
            .map(|plugin| registry.autonomies.compile(plugin))
            .collect::<Result<Vec<_>>>()?;
        let controllers = config
            .controllers
            .iter()
            .map(|plugin| registry.controllers.compile(plugin))
            .collect::<Result<Vec<_>>>()?;
        let motion = registry.motion.compile(&config.motion)?;
        let sensors = config
            .sensors
            .iter()
            .map(|plugin| registry.sensors.compile(plugin))
            .collect::<Result<Vec<_>>>()?;

        let mut upstream = Vec::new();
        for autonomy in &autonomies {
            autonomy.ports.connect(&[], &autonomy.name)?;
            for output in &autonomy.ports.outputs {
                if let Some(previous) = upstream
                    .iter()
                    .find(|port: &&Port| port.name == output.name)
                {
                    ensure!(
                        previous == output,
                        "autonomies disagree on port '{}'",
                        output.name
                    );
                } else {
                    upstream.push(output.clone());
                }
            }
        }
        for controller in &controllers {
            controller.ports.connect(&upstream, &controller.name)?;
            upstream = controller.ports.outputs.clone();
        }
        motion.ports.connect(&upstream, &motion.name)?;
        for sensor in &sensors {
            ensure!(
                sensor.ports.inputs.is_empty() && sensor.ports.outputs.is_empty(),
                "sensor '{}' uses observations, not control-chain ports",
                sensor.name
            );
        }
        let mut occurrences = std::collections::BTreeMap::new();
        let mut identities = std::collections::BTreeSet::new();
        let mut sensor_identities = Vec::new();
        for source in &config.sensors {
            let count = occurrences.entry(&source.name).or_insert(0);
            let identity = source
                .params
                .get("instance")
                .cloned()
                .unwrap_or_else(|| format!("{}:{}", source.name, count));
            *count += 1;
            ensure!(
                identities.insert(identity.clone()),
                "duplicate sensor instance '{identity}'"
            );
            sensor_identities.push(identity);
        }
        Ok(Self {
            autonomies,
            controllers,
            motion,
            sensors,
            sensor_identities,
        })
    }

    pub fn instantiate(&self, entity_id: i32, seed: u32) -> PluginStack {
        // Each instance draws from its own stream, keyed by a stable identity.
        let agent_slots = |category: &str, plugins: &[CompiledPlugin<dyn AgentBehavior>]| {
            plugins
                .iter()
                .enumerate()
                .map(|(index, plugin)| {
                    let identity = format!("{category}/{}:{index}", plugin.name);
                    Slot::new(plugin, PluginRandom::new(seed, entity_id, &identity))
                })
                .collect()
        };
        let autonomies = agent_slots("autonomy", &self.autonomies);
        let controllers = agent_slots("controller", &self.controllers);
        let motion = Slot::new(
            &self.motion,
            PluginRandom::new(seed, entity_id, &format!("motion/{}:0", self.motion.name)),
        );
        // Sensor identities predate the other categories; keep them so noise is unchanged.
        let sensors = self
            .sensors
            .iter()
            .zip(&self.sensor_identities)
            .map(|(plugin, identity)| {
                Slot::new(plugin, PluginRandom::new(seed, entity_id, identity))
            })
            .collect();
        PluginStack {
            autonomies,
            controllers,
            motion,
            sensors,
            outputs: Signals::new(),
            observations: Observations::default(),
            pending: Observations::default(),
            belief: None,
            stop_requested: false,
            closed: false,
        }
    }
}

struct Slot<T: ?Sized> {
    name: String,
    plugin: Box<T>,
    io: PluginIo,
    rate: Rate,
    messages: Messages,
    random: PluginRandom,
}
impl<T: ?Sized> Slot<T> {
    fn new(compiled: &CompiledPlugin<T>, random: PluginRandom) -> Self {
        Self {
            name: compiled.name.clone(),
            plugin: (compiled.instantiate)(),
            io: PluginIo::new(&compiled.ports),
            rate: compiled.rate.clone(),
            messages: Messages::default(),
            random,
        }
    }
    fn mailbox(
        &mut self,
        entity_id: i32,
        category: &str,
        index: usize,
    ) -> crate::pubsub::messages::Mailbox<'_> {
        crate::pubsub::messages::Mailbox {
            endpoint: MessageEndpoint {
                entity_id: Some(entity_id),
                plugin: format!("{category}/{}:{index}", self.name),
            },
            messages: &mut self.messages,
        }
    }

    fn close(
        &mut self,
        time: StepTime,
        close: impl FnOnce(&mut T, StepTime) -> Result<()>,
    ) -> Result<()> {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            close(&mut self.plugin, time)
        }))
        .unwrap_or_else(|_| Err(anyhow::anyhow!("plugin close panicked")))
        .with_context(|| format!("close '{}'", self.name))
    }
}

impl Slot<dyn AgentBehavior> {
    fn agent_initialize(
        &mut self,
        entity: EntityInfo,
        time: StepTime,
        state: &KinematicState,
        observations: &Observations,
    ) -> Result<()> {
        self.messages.time_s = time.time_s;
        let mut context = AgentContext {
            entity,
            time,
            state,
            observations,
            contacts_truth: &[],
            messages: &mut self.messages,
            random: &mut self.random,
        };
        self.plugin
            .initialize(&mut context)
            .with_context(|| format!("initialize '{}' on entity {}", self.name, entity.id))
    }
    fn agent_step(
        &mut self,
        entity: EntityInfo,
        time: StepTime,
        state: &KinematicState,
        observations: &Observations,
        contacts_truth: &[EntitySnapshot],
    ) -> Result<Update> {
        // Matches C++ for now, though we don't consider it correct: a rate-limited plugin
        // receives the phase dt, not the elapsed time since it last ran.
        if !self.rate.due(time.dt_s) {
            return Ok(Update::Applied);
        }
        self.messages.time_s = time.time_s;
        let mut context = AgentContext {
            entity,
            time,
            state,
            observations,
            contacts_truth,
            messages: &mut self.messages,
            random: &mut self.random,
        };
        self.plugin
            .step(&mut context, &mut self.io)
            .with_context(|| format!("step '{}' on entity {}", self.name, entity.id))
    }
}

pub(crate) struct PluginStack {
    autonomies: Vec<Slot<dyn AgentBehavior>>,
    controllers: Vec<Slot<dyn AgentBehavior>>,
    motion: Slot<dyn MotionBehavior>,
    sensors: Vec<Slot<dyn SensorBehavior>>,
    outputs: Signals,
    observations: Observations,
    pending: Observations,
    belief: Option<KinematicState>,
    pub stop_requested: bool,
    closed: bool,
}
fn handle_update(update: Update, stop_requested: &mut bool) {
    if update == Update::Stop {
        *stop_requested = true;
    }
}
impl PluginStack {
    pub fn initialize(
        &mut self,
        entity: EntityInfo,
        time: StepTime,
        truth: &mut KinematicState,
    ) -> Result<()> {
        // C++ initializes sensors before motion/controllers/autonomy. A sensor may
        // detach belief from the initial truth before motion normalizes its state.
        for sensor in &mut self.sensors {
            sensor.messages.time_s = time.time_s;
            sensor
                .plugin
                .initialize(&mut SensorContext {
                    entity,
                    time,
                    truth,
                    messages: &mut sensor.messages,
                    contacts_truth: &[],
                    random: &mut sensor.random,
                    belief: &mut self.belief,
                    pending: &mut self.pending,
                })
                .with_context(|| format!("initialize '{}'", sensor.name))?;
        }
        self.motion.messages.time_s = time.time_s;
        self.motion
            .plugin
            .initialize(&mut MotionContext {
                entity,
                time,
                truth,
                messages: &mut self.motion.messages,
            })
            .with_context(|| format!("initialize '{}'", self.motion.name))?;
        for controller in self.controllers.iter_mut().rev() {
            controller.agent_initialize(
                entity,
                time,
                self.belief.as_ref().unwrap_or(truth),
                &self.observations,
            )?;
        }
        for autonomy in &mut self.autonomies {
            autonomy.agent_initialize(
                entity,
                time,
                self.belief.as_ref().unwrap_or(truth),
                &self.observations,
            )?;
        }
        Ok(())
    }
    pub fn observations(&self) -> &Observations {
        &self.observations
    }
    pub fn belief<'a>(&'a self, truth: &'a KinematicState) -> &'a KinematicState {
        self.belief.as_ref().unwrap_or(truth)
    }
    pub fn autonomy(
        &mut self,
        entity: EntityInfo,
        time: StepTime,
        state: &KinematicState,
        contacts_truth: &[EntitySnapshot],
    ) -> Result<()> {
        let state = self.belief.as_ref().unwrap_or(state);
        for autonomy in &mut self.autonomies {
            handle_update(
                autonomy.agent_step(entity, time, state, &self.observations, contacts_truth)?,
                &mut self.stop_requested,
            );
            // Source order resolves multiple autonomy writers deterministically.
            self.outputs.extend(
                autonomy
                    .io
                    .outputs
                    .iter()
                    .map(|(name, value)| (name.clone(), *value)),
            );
        }
        Ok(())
    }
    pub fn controllers(
        &mut self,
        entity: EntityInfo,
        time: StepTime,
        state: &KinematicState,
        contacts_truth: &[EntitySnapshot],
    ) -> Result<()> {
        let state = self.belief.as_ref().unwrap_or(state);
        let mut upstream = &self.outputs;
        for controller in &mut self.controllers {
            controller.io.receive(upstream)?;
            handle_update(
                controller.agent_step(entity, time, state, &self.observations, contacts_truth)?,
                &mut self.stop_requested,
            );
            upstream = &controller.io.outputs;
        }
        Ok(())
    }
    pub fn motion(
        &mut self,
        entity: EntityInfo,
        time: StepTime,
        truth: &mut KinematicState,
    ) -> Result<()> {
        let upstream = self
            .controllers
            .last()
            .map_or(&self.outputs, |slot| &slot.io.outputs);
        self.motion.io.receive(upstream)?;
        self.motion.messages.time_s = time.time_s;
        let update = self
            .motion
            .plugin
            .step(
                &mut MotionContext {
                    entity,
                    time,
                    truth,
                    messages: &mut self.motion.messages,
                },
                &mut self.motion.io,
            )
            .with_context(|| format!("step '{}' on entity {}", self.motion.name, entity.id))?;
        handle_update(update, &mut self.stop_requested);
        Ok(())
    }
    pub fn sensors(
        &mut self,
        entity: EntityInfo,
        time: StepTime,
        truth: &KinematicState,
        contacts_truth: &[EntitySnapshot],
    ) -> Result<()> {
        for sensor in &mut self.sensors {
            sensor.messages.time_s = time.time_s;
            if !sensor.rate.due(time.dt_s) {
                continue;
            }
            let update = sensor
                .plugin
                .step(&mut SensorContext {
                    entity,
                    time,
                    truth,
                    messages: &mut sensor.messages,
                    contacts_truth,
                    random: &mut sensor.random,
                    belief: &mut self.belief,
                    pending: &mut self.pending,
                })
                .with_context(|| format!("step '{}' on entity {}", sensor.name, entity.id))?;
            handle_update(update, &mut self.stop_requested);
        }
        Ok(())
    }
    pub fn mailboxes<'a>(
        &'a mut self,
        entity_id: i32,
        mailboxes: &mut Vec<crate::pubsub::messages::Mailbox<'a>>,
    ) {
        for (category, slots) in [
            ("autonomy", self.autonomies.as_mut_slice()),
            ("controller", self.controllers.as_mut_slice()),
        ] {
            for (index, slot) in slots.iter_mut().enumerate() {
                mailboxes.push(slot.mailbox(entity_id, category, index));
            }
        }
        mailboxes.push(self.motion.mailbox(entity_id, "motion", 0));
        for (index, sensor) in self.sensors.iter_mut().enumerate() {
            mailboxes.push(sensor.mailbox(entity_id, "sensor", index));
        }
    }
    pub fn close(&mut self, time: StepTime) -> Result<()> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        let mut failure = None;
        let mut record = |result| {
            if let Err(error) = result {
                failure.get_or_insert(error);
            }
        };
        for slot in self.autonomies.iter_mut().chain(&mut self.controllers) {
            record(slot.close(time, AgentBehavior::close));
        }
        record(self.motion.close(time, MotionBehavior::close));
        for sensor in &mut self.sensors {
            record(sensor.close(time, SensorBehavior::close));
        }
        failure.map_or(Ok(()), Err)
    }
    pub fn deliver(&mut self) -> Result<()> {
        self.observations.deliver(&mut self.pending)
    }
}

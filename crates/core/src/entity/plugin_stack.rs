//! Type erasure and scheduling stay inside the engine, not in plugin author code.
use crate::estimator::{Inputs, Source};
use crate::plugin::*;
use crate::{EntitySnapshot, KinematicState, common::Rate, scenario::EntityGroupConfig};
use crate::{common::variable_io::Signals, plugin_manager::CompiledPlugin};
use anyhow::{Context, Result, bail, ensure};
use std::collections::BTreeMap;

// These runtime-only traits forward calls to the public plugin traits.
// Once the tool is more established, investigate separating construction from
// runtime methods so we might remove this forwarding layer across all
// categories. Keep the current plugin API for now; this is not a required refactor.
// Estimators have adapters because one type (PassThrough) implements both roles.
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
pub(crate) trait LocalizationBehavior: Send {
    fn close(&mut self, time: StepTime) -> Result<()>;
    fn initialize(&mut self, context: &mut LocalizationContext<'_>) -> Result<()>;
    fn step(&mut self, context: &mut LocalizationContext<'_>) -> Result<Update>;
}
pub(crate) struct LocalizationAdapter<T>(pub T);
impl<T: Localization> LocalizationBehavior for LocalizationAdapter<T> {
    fn close(&mut self, time: StepTime) -> Result<()> {
        self.0.close(time)
    }
    fn initialize(&mut self, context: &mut LocalizationContext<'_>) -> Result<()> {
        Localization::initialize(&mut self.0, context)
    }
    fn step(&mut self, context: &mut LocalizationContext<'_>) -> Result<Update> {
        Localization::step(&mut self.0, context)
    }
}
pub(crate) trait TrackingBehavior: Send {
    fn close(&mut self, time: StepTime) -> Result<()>;
    fn initialize(&mut self, context: &mut TrackingContext<'_>) -> Result<()>;
    fn step(&mut self, context: &mut TrackingContext<'_>) -> Result<Update>;
}
pub(crate) struct TrackingAdapter<T>(pub T);
impl<T: Tracking> TrackingBehavior for TrackingAdapter<T> {
    fn close(&mut self, time: StepTime) -> Result<()> {
        self.0.close(time)
    }
    fn initialize(&mut self, context: &mut TrackingContext<'_>) -> Result<()> {
        Tracking::initialize(&mut self.0, context)
    }
    fn step(&mut self, context: &mut TrackingContext<'_>) -> Result<Update> {
        Tracking::step(&mut self.0, context)
    }
}

/// The name missions use for the engine's perfect measurement.
pub(crate) const PERFECT: &str = "perfect";

/// Chooses each estimator input's source: the mission's choice, else the one
/// sensor of that kind, else the perfect measurement. Several candidates is an
/// error rather than a silent pick.
fn select_sources(
    role: &str,
    config: &crate::scenario::PluginConfig,
    inputs: &[Input],
    sensors: &[(String, ReadingKind)],
) -> Result<Vec<(String, Source)>> {
    for named in config.inputs.keys() {
        ensure!(
            inputs.iter().any(|input| &input.role == named),
            "{role} '{}' has no input '{named}'; its inputs are: {}",
            config.name,
            inputs
                .iter()
                .map(|input| input.role.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    let perfect_allowed = |input: &Input| input.kind != ReadingKind::Other;
    let mut sources = Vec::new();
    for input in inputs {
        let source = if let Some(sensor) = config.inputs.get(&input.role) {
            if sensor == PERFECT {
                ensure!(
                    perfect_allowed(input),
                    "{role} input '{}' needs a sensor, not the perfect measurement",
                    input.role
                );
                Source::Perfect(input.kind)
            } else {
                let (_, kind) = sensors
                    .iter()
                    .find(|(name, _)| name == sensor)
                    .with_context(|| {
                        format!(
                            "{role} input '{}' names sensor '{sensor}', but the entity's sensors are: {}",
                            input.role,
                            sensors.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>().join(", ")
                        )
                    })?;
                ensure!(
                    input.kind == ReadingKind::Other || *kind == input.kind,
                    "{role} input '{}' needs {:?} readings, but sensor '{sensor}' gives {kind:?}",
                    input.role,
                    input.kind
                );
                Source::Sensor(sensor.clone())
            }
        } else if perfect_allowed(input) {
            let candidates: Vec<_> = sensors
                .iter()
                .filter(|(_, kind)| *kind == input.kind)
                .collect();
            match candidates.as_slice() {
                [] => Source::Perfect(input.kind),
                [(name, _)] => Source::Sensor(name.clone()),
                _ => bail!(
                    "{role} input '{}' could use any of the sensors {}; name one in the mission's `inputs`",
                    input.role,
                    candidates
                        .iter()
                        .map(|(name, _)| name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            }
        } else {
            bail!(
                "{role} input '{}' needs a sensor named in the mission's `inputs`",
                input.role
            );
        };
        sources.push((input.role.clone(), source));
    }
    Ok(sources)
}

fn source_names(sources: &[(String, Source)]) -> BTreeMap<String, String> {
    sources
        .iter()
        .map(|(role, source)| {
            let name = match source {
                Source::Perfect(_) => PERFECT.to_owned(),
                Source::Sensor(sensor) => sensor.clone(),
            };
            (role.clone(), name)
        })
        .collect()
}

pub(crate) struct CompiledStack {
    autonomies: Vec<CompiledPlugin<dyn AgentBehavior>>,
    controllers: Vec<CompiledPlugin<dyn AgentBehavior>>,
    motion: CompiledPlugin<dyn MotionBehavior>,
    sensors: Vec<CompiledPlugin<dyn SensorBehavior>>,
    sensor_identities: Vec<String>,
    localization: CompiledPlugin<dyn LocalizationBehavior>,
    localization_sources: Vec<(String, Source)>,
    tracking: CompiledPlugin<dyn TrackingBehavior>,
    tracking_sources: Vec<(String, Source)>,
}
impl CompiledStack {
    pub fn compile(config: &EntityGroupConfig, registry: &PluginRegistry) -> Result<Self> {
        let autonomies = config
            .autonomy
            .iter()
            .map(|plugin| registry.autonomies.compile(plugin))
            .collect::<Result<Vec<_>>>()?;
        let controllers = config
            .controller
            .iter()
            .map(|plugin| registry.controllers.compile(plugin))
            .collect::<Result<Vec<_>>>()?;
        let motion = registry.motion.compile(&config.motion_model)?;
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
                "sensor '{}' produces readings, not control-chain ports",
                sensor.name
            );
        }
        let mut occurrences = std::collections::BTreeMap::new();
        let mut identities = std::collections::BTreeSet::new();
        let mut sensor_identities = Vec::new();
        for source in &config.sensors {
            let count = occurrences.entry(&source.name).or_insert(0);
            let identity = source
                .instance
                .clone()
                .unwrap_or_else(|| format!("{}:{}", source.name, count));
            *count += 1;
            ensure!(
                identities.insert(identity.clone()),
                "duplicate sensor instance '{identity}'"
            );
            sensor_identities.push(identity);
        }
        ensure!(
            !sensor_identities.iter().any(|name| name == PERFECT),
            "'{PERFECT}' is reserved for the perfect measurement; rename the sensor"
        );
        let sensor_kinds: Vec<_> = sensor_identities
            .iter()
            .cloned()
            .zip(sensors.iter().map(|sensor| sensor.reading_kind))
            .collect();
        // Estimators run every tick: fresh-reading flags and recorded commands
        // cover one tick, so a skipped tick would lose them. An estimator that
        // wants to do expensive work less often decides that in its own `step`.
        for (role, estimator) in [
            ("localization", &config.localization),
            ("tracking", &config.tracking),
        ] {
            ensure!(
                estimator.loop_rate_hz == 0.0,
                "{role} '{}' runs every tick and takes no loop_rate",
                estimator.name
            );
        }
        let localization = registry.localization.compile(&config.localization)?;
        let localization_sources = select_sources(
            "localization",
            &config.localization,
            &localization.inputs,
            &sensor_kinds,
        )?;
        let tracking = registry.tracking.compile(&config.tracking)?;
        let tracking_sources = select_sources(
            "tracking",
            &config.tracking,
            &tracking.inputs,
            &sensor_kinds,
        )?;
        Ok(Self {
            autonomies,
            controllers,
            motion,
            sensors,
            sensor_identities,
            localization,
            localization_sources,
            tracking,
            tracking_sources,
        })
    }

    /// Each estimator's chosen sources, for the resolved configuration.
    pub fn estimator_inputs(&self) -> [BTreeMap<String, String>; 2] {
        [
            source_names(&self.localization_sources),
            source_names(&self.tracking_sources),
        ]
    }

    pub fn instantiate(&self, entity_id: i32, seed: u32) -> PluginStack {
        // Each instance draws from its own stream, keyed by a stable identity.
        let agent_slots = |category: &str, plugins: &[CompiledPlugin<dyn AgentBehavior>]| {
            plugins
                .iter()
                .enumerate()
                .map(|(index, plugin)| {
                    let identity = format!("{category}/{}:{index}", plugin.name);
                    let random = PluginRandom::new(seed, entity_id, &identity);
                    Slot::new(plugin, random, endpoint(entity_id, identity))
                })
                .collect()
        };
        let autonomies = agent_slots("autonomy", &self.autonomies);
        let controllers = agent_slots("controller", &self.controllers);
        let motion_identity = format!("motion/{}:0", self.motion.name);
        let motion = Slot::new(
            &self.motion,
            PluginRandom::new(seed, entity_id, &motion_identity),
            endpoint(entity_id, motion_identity),
        );
        // Sensor identities predate the other categories; keep them so noise is unchanged.
        let sensors = self
            .sensors
            .iter()
            .zip(&self.sensor_identities)
            .enumerate()
            .map(|(index, (plugin, identity))| {
                let random = PluginRandom::new(seed, entity_id, identity);
                let name = format!("sensor/{}:{index}", plugin.name);
                Slot::new(plugin, random, endpoint(entity_id, name))
            })
            .collect();
        let sensor_readings = self
            .sensor_identities
            .iter()
            .cloned()
            .zip(self.sensors.iter().map(|sensor| sensor.reading_kind))
            .collect();
        let localization_identity = format!("localization/{}:0", self.localization.name);
        let localization = Slot::new(
            &self.localization,
            PluginRandom::new(seed, entity_id, &localization_identity),
            endpoint(entity_id, localization_identity),
        );
        let tracking_identity = format!("tracking/{}:0", self.tracking.name);
        let tracking = Slot::new(
            &self.tracking,
            PluginRandom::new(seed, entity_id, &tracking_identity),
            endpoint(entity_id, tracking_identity),
        );
        let command_names = motion.io.input_names().cloned().collect();
        PluginStack {
            autonomies,
            controllers,
            motion,
            sensors,
            sensor_readings,
            localization,
            localization_sources: self.localization_sources.clone(),
            tracking,
            tracking_sources: self.tracking_sources.clone(),
            outputs: Signals::new(),
            readings: Readings::default(),
            pending: Readings::default(),
            belief: KinematicState::default(),
            contacts: ContactList::from([]),
            command_names,
            command_values: Vec::new(),
            command_times: Vec::new(),
            stop_requested: false,
            closed: false,
        }
    }
}

// DEVNOTE: Every plugin has a message "address": which entity it belongs to
// plus a name like "sensor/NoisyContacts:0". Networks use it to know who sent
// a message and who should receive it. We used to rebuild that name from
// scratch for every plugin on every step, which meant creating thousands of
// small strings per second that were thrown away immediately. Profiling
// perf.py's straight-flight mission with macOS `sample` showed this took about
// a fifth of the main thread's time. Now each plugin's address is written once,
// when its entity is created, and each step just points to it. Results are
// byte-identical (all 13 comparison missions, 1 and 8 workers, including
// traces); perf.py showed straight flight about a third faster.
fn endpoint(entity_id: i32, plugin: String) -> MessageEndpoint {
    MessageEndpoint {
        entity_id: Some(entity_id),
        plugin,
    }
}

struct Slot<T: ?Sized> {
    name: String,
    endpoint: MessageEndpoint,
    plugin: Box<T>,
    io: PluginIo,
    rate: Rate,
    messages: Messages,
    random: PluginRandom,
}
impl<T: ?Sized> Slot<T> {
    fn new(compiled: &CompiledPlugin<T>, random: PluginRandom, endpoint: MessageEndpoint) -> Self {
        Self {
            name: compiled.name.clone(),
            endpoint,
            plugin: (compiled.instantiate)(),
            io: PluginIo::new(&compiled.ports),
            rate: compiled.rate.clone(),
            messages: Messages::default(),
            random,
        }
    }
    fn mailbox(&mut self) -> crate::pubsub::messages::Mailbox<'_> {
        crate::pubsub::messages::Mailbox {
            endpoint: &self.endpoint,
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
        contacts: KnownContacts<'_>,
    ) -> Result<()> {
        self.messages.time_s = time.time_s;
        let mut context = AgentContext {
            entity,
            time,
            state,
            contacts,
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
        contacts: KnownContacts<'_>,
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
            contacts,
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
    /// Each sensor's instance name and reading kind, in sensor order.
    sensor_readings: Vec<(String, ReadingKind)>,
    localization: Slot<dyn LocalizationBehavior>,
    localization_sources: Vec<(String, Source)>,
    tracking: Slot<dyn TrackingBehavior>,
    tracking_sources: Vec<(String, Source)>,
    outputs: Signals,
    /// Delivered sensor readings, which only estimators read.
    readings: Readings,
    /// Readings taken this tick, delivered in the estimation step.
    pending: Readings,
    /// Own-state belief, written only by localization.
    belief: KinematicState,
    /// Known contacts, written only by tracking.
    contacts: ContactList,
    /// What the motion model read on each substep this tick, for localization.
    command_names: Vec<String>,
    command_values: Vec<f64>,
    command_times: Vec<StepTime>,
    pub stop_requested: bool,
    closed: bool,
}
fn handle_update(update: Update, stop_requested: &mut bool) {
    if update == Update::Stop {
        *stop_requested = true;
    }
}
impl PluginStack {
    /// Sensors, then motion, as C++ orders them. Estimators and decision plugins
    /// start later, in `start`, once the whole population exists.
    pub fn initialize(
        &mut self,
        entity: EntityInfo,
        time: StepTime,
        truth: &mut KinematicState,
    ) -> Result<()> {
        // A sensor's startup reading sees the initial truth before motion normalizes it.
        for (sensor, (name, kind)) in self.sensors.iter_mut().zip(&self.sensor_readings) {
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
                    name,
                    kind: *kind,
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
        Ok(())
    }
    /// Sets the first belief from the startup readings and `perfect_contacts` (the
    /// whole population), then initializes controllers and autonomy, which see it.
    pub fn start(
        &mut self,
        entity: EntityInfo,
        time: StepTime,
        truth: &KinematicState,
        perfect_contacts: &ContactList,
    ) -> Result<()> {
        self.readings.deliver(&mut self.pending)?;
        let inputs = |sources| Inputs {
            sources,
            readings: &self.readings,
            perfect_state: truth,
            perfect_contacts,
            now_s: time.time_s + time.dt_s,
        };
        self.localization.messages.time_s = time.time_s;
        self.localization
            .plugin
            .initialize(&mut LocalizationContext {
                entity,
                time,
                inputs: inputs(&self.localization_sources),
                commands: Commands {
                    names: &self.command_names,
                    values: &[],
                    times: &[],
                },
                messages: &mut self.localization.messages,
                random: &mut self.localization.random,
                state: &mut self.belief,
            })
            .with_context(|| format!("initialize '{}'", self.localization.name))?;
        self.tracking.messages.time_s = time.time_s;
        self.tracking
            .plugin
            .initialize(&mut TrackingContext {
                entity,
                time,
                inputs: inputs(&self.tracking_sources),
                messages: &mut self.tracking.messages,
                random: &mut self.tracking.random,
                pose: &self.belief,
                contacts: &mut self.contacts,
            })
            .with_context(|| format!("initialize '{}'", self.tracking.name))?;
        self.readings.clear_fresh();
        let contacts = KnownContacts::new(&self.contacts, entity.id);
        for controller in self.controllers.iter_mut().rev() {
            controller.agent_initialize(entity, time, &self.belief, contacts)?;
        }
        for autonomy in &mut self.autonomies {
            autonomy.agent_initialize(entity, time, &self.belief, contacts)?;
        }
        Ok(())
    }
    pub fn readings(&self) -> &Readings {
        &self.readings
    }
    /// Autonomy then controller outputs as (plugin, port, value), for the comparison
    /// trace. Like C++ VariableIO after connect(), only ports the next plugin reads.
    pub fn outputs(&self) -> Vec<(&str, &str, f64)> {
        let mut outputs = Vec::new();
        let first_consumer = self
            .controllers
            .first()
            .map_or(&self.motion.io, |slot| &slot.io);
        // Autonomies feed one merged set (later writers win), which is what the
        // first consumer reads; C++ connects them to one shared buffer.
        for slot in &self.autonomies {
            for port in slot.io.outputs.keys() {
                if first_consumer.reads(port)
                    && let Some(value) = self.outputs.get(port)
                {
                    outputs.push((slot.name.as_str(), port.as_str(), *value));
                }
            }
        }
        for (index, slot) in self.controllers.iter().enumerate() {
            let consumer = self
                .controllers
                .get(index + 1)
                .map_or(&self.motion.io, |next| &next.io);
            for (port, value) in slot.io.outputs.iter() {
                if consumer.reads(port) {
                    outputs.push((slot.name.as_str(), port.as_str(), *value));
                }
            }
        }
        outputs
    }
    pub fn belief(&self) -> &KinematicState {
        &self.belief
    }
    pub fn contacts(&self) -> &ContactList {
        &self.contacts
    }
    pub fn autonomy(&mut self, entity: EntityInfo, time: StepTime) -> Result<()> {
        let contacts = KnownContacts::new(&self.contacts, entity.id);
        for autonomy in &mut self.autonomies {
            handle_update(
                autonomy.agent_step(entity, time, &self.belief, contacts)?,
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
    pub fn controllers(&mut self, entity: EntityInfo, time: StepTime) -> Result<()> {
        let contacts = KnownContacts::new(&self.contacts, entity.id);
        let mut upstream = &self.outputs;
        for controller in &mut self.controllers {
            controller.io.receive(upstream)?;
            handle_update(
                controller.agent_step(entity, time, &self.belief, contacts)?,
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
        // These are the commands localization may use: what motion actually read.
        self.command_values.extend(self.motion.io.input_values());
        self.command_times.push(time);
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
        for (sensor, (name, kind)) in self.sensors.iter_mut().zip(&self.sensor_readings) {
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
                    name,
                    kind: *kind,
                    pending: &mut self.pending,
                })
                .with_context(|| format!("step '{}' on entity {}", sensor.name, entity.id))?;
            handle_update(update, &mut self.stop_requested);
        }
        Ok(())
    }
    /// The estimation step: deliver this tick's readings, then localization, then
    /// tracking. `truth` and `perfect_contacts` are the post-interaction state,
    /// which estimators see only as the perfect measurement.
    pub fn estimate(
        &mut self,
        entity: EntityInfo,
        time: StepTime,
        truth: &KinematicState,
        perfect_contacts: &ContactList,
    ) -> Result<()> {
        self.readings.deliver(&mut self.pending)?;
        let inputs = |sources| Inputs {
            sources,
            readings: &self.readings,
            perfect_state: truth,
            perfect_contacts,
            now_s: time.time_s + time.dt_s,
        };
        self.localization.messages.time_s = time.time_s;
        let update = self
            .localization
            .plugin
            .step(&mut LocalizationContext {
                entity,
                time,
                inputs: inputs(&self.localization_sources),
                commands: Commands {
                    names: &self.command_names,
                    values: &self.command_values,
                    times: &self.command_times,
                },
                messages: &mut self.localization.messages,
                random: &mut self.localization.random,
                state: &mut self.belief,
            })
            .with_context(|| {
                format!("step '{}' on entity {}", self.localization.name, entity.id)
            })?;
        handle_update(update, &mut self.stop_requested);
        self.tracking.messages.time_s = time.time_s;
        let update = self
            .tracking
            .plugin
            .step(&mut TrackingContext {
                entity,
                time,
                inputs: inputs(&self.tracking_sources),
                messages: &mut self.tracking.messages,
                random: &mut self.tracking.random,
                pose: &self.belief,
                contacts: &mut self.contacts,
            })
            .with_context(|| format!("step '{}' on entity {}", self.tracking.name, entity.id))?;
        handle_update(update, &mut self.stop_requested);
        self.readings.clear_fresh();
        self.command_values.clear();
        self.command_times.clear();
        Ok(())
    }
    pub fn mailboxes<'a>(&'a mut self, mailboxes: &mut Vec<crate::pubsub::messages::Mailbox<'a>>) {
        for slot in self.autonomies.iter_mut().chain(&mut self.controllers) {
            mailboxes.push(slot.mailbox());
        }
        mailboxes.push(self.motion.mailbox());
        for sensor in &mut self.sensors {
            mailboxes.push(sensor.mailbox());
        }
        // DEVNOTE: Networks look through every plugin's mailbox each tick, even
        // ones with nothing to send or receive. Estimators usually use no
        // messages, and adding their two mailboxes per entity made perf.py's
        // motion-512 workload about 3% slower (macOS `sample`: the network phase
        // went from 258 to 341 samples in a fixed window). An idle mailbox
        // can't send or receive, so leaving it out changes no output.
        for slot in [self.localization.mailbox(), self.tracking.mailbox()] {
            if !slot.messages.is_idle() {
                mailboxes.push(slot);
            }
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
        record(self.localization.close(time, LocalizationBehavior::close));
        record(self.tracking.close(time, TrackingBehavior::close));
        failure.map_or(Ok(()), Err)
    }
}

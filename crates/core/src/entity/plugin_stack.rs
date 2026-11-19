//! One entity's live plugins and what happens to them each tick. A
//! `PluginStack` is built from the group's `CompiledStack` when the entity
//! spawns and holds one `Slot` per plugin: the plugin itself behind its trait,
//! its ports, mailbox, rate, and random stream. The methods on `PluginStack`
//! are the entity's share of each simulation phase, in the order the
//! simulation calls them: autonomy, controllers and motion per substep,
//! sensors, delivery and estimation, drawings, close.
//!
//! Type erasure and scheduling stay inside the engine, not in plugin author
//! code. `dyn Autonomy` and the other `dyn` traits mean "some plugin of this
//! kind, chosen by the mission"; a `Slot<T: ?Sized>` holds one of any kind.
use crate::plugin::estimator::{Inputs, PerfectContacts, Source};
use crate::plugin::sensor::ReadingType;
use crate::plugin::*;
use crate::{
    GaussianState, KinematicState, Vec3,
    common::{Rate, Terrain},
    drawing::PluginDrawings,
    pubsub::routing::Mailbox,
};
use crate::{entity::compiled_stack::CompiledStack, plugin::CompiledPlugin, ports::Signals};
use anyhow::{Context, Result};
use std::sync::Arc;
use vstd::prelude::*;

pub(crate) mod proof;
// Verus skips this module; proof.rs gives the plugin call its contract.
#[cfg_attr(verus_only, verus_verify(external))]
mod agent;

verus! {

broadcast use proof::stack_facts;

#[verifier::external]
impl CompiledStack {
    /// The live plugins for one entity spawned from this group. Each plugin
    /// gets its own random stream and message address, keyed by the entity's
    /// id and its place in the stack.
    pub fn instantiate(&self, entity_id: i32, seed: u32) -> PluginStack {
        let autonomies = agent_slots("autonomy", &self.autonomies, entity_id, seed);
        let controllers = agent_slots("controller", &self.controllers, entity_id, seed);
        let motion_identity = format!("motion/{}:0", self.motion.name);
        let motion = Slot::new(
            &self.motion,
            PluginRandom::new(seed, entity_id, &motion_identity),
            endpoint(entity_id, motion_identity),
        );
        // Keyed by the instance name the sensor's readings carry.
        let sensors = self
            .sensors
            .iter()
            .enumerate()
            .map(|(index, plugin)| {
                let random = PluginRandom::new(seed, entity_id, &plugin.instance_name);
                let name = format!("sensor/{}:{index}", plugin.name);
                Slot::new(plugin, random, endpoint(entity_id, name))
            })
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
        let command_names = motion.ports.inputs.keys().cloned().collect();
        // Every declared output starts at zero; see `Signals` in ports.rs.
        let mut signals = Signals::new();
        let declared = autonomies
            .iter()
            .map(|slot| &slot.ports)
            .chain(controllers.iter().map(|slot| &slot.ports))
            .chain([&motion.ports]);
        for ports in declared {
            signals.extend(ports.outputs.keys().map(|name| (name.clone(), 0.0)));
        }
        let belief_is_perfect_state = self.localization.name == "PassThrough"
            && matches!(
                self.localization_sources.as_slice(),
                [(_, Source::Perfect(ReadingKind::State))]
            );
        PluginStack {
            autonomies,
            controllers,
            motion,
            sensors,
            localization,
            localization_sources: self.localization_sources.clone(),
            tracking,
            tracking_sources: self.tracking_sources.clone(),
            signals,
            readings: Readings::default(),
            pending: Readings::default(),
            belief: GaussianState::default(),
            tracks: TrackList::from([]),
            belief_is_perfect_state,
            command_names,
            command_values: Vec::new(),
            command_times: Vec::new(),
            closed: false,
        }
    }
}

/// Each instance's random numbers are keyed by a stable identity.
#[verifier::external]
fn agent_slots<T: ?Sized>(
    category: &str,
    plugins: &[CompiledPlugin<T>],
    entity_id: i32,
    seed: u32,
) -> Vec<Slot<T>> {
    plugins
        .iter()
        .enumerate()
        .map(|(index, plugin)| {
            let identity = format!("{category}/{}:{index}", plugin.name);
            let random = PluginRandom::new(seed, entity_id, &identity);
            Slot::new(plugin, random, endpoint(entity_id, identity))
        })
        .collect()
}

// DEVNOTE: a plugin's message address (its entity plus a name such as
// "sensor/NoisyContacts:0") is built once here, when the entity is created.
// Rebuilding it every step took about a fifth of the main thread in the
// benchmark `motion` workload (macOS `sample`); building it once made that run a
// third faster with byte-identical output on all 13 comparison missions.
#[verifier::external]
fn endpoint(entity_id: i32, plugin: String) -> MessageEndpoint {
    MessageEndpoint {
        entity_id: Some(entity_id),
        plugin,
    }
}

/// A sensor's endpoint and its measured states: (the measured entity's id,
/// None for own state or an unidentified detection; the measurement).
pub(crate) type Measurement<'a> = (&'a MessageEndpoint, Vec<(Option<i32>, &'a GaussianState)>);

/// What autonomies and controllers have in common: the engine starts and
/// steps both with an `AgentContext` and the plugin's ports. `agent.rs` gives
/// it to `dyn Autonomy` and `dyn Controller`.
#[verifier::external]
pub(crate) trait Agent {
    fn initialize(&mut self, context: &mut AgentContext<'_>) -> Result<()>;
    fn step(&mut self, context: &mut AgentContext<'_>, io: &mut PluginIo) -> Result<()>;
}

/// The motion model as the engine steps it: with a `MotionContext` and the
/// plugin's ports. `agent.rs` gives it to `dyn MotionModel`, so the one call
/// into the model can carry a contract.
#[verifier::external]
pub(crate) trait MotionStep {
    fn step(&mut self, context: &mut MotionContext<'_>, io: &mut PluginIo) -> Result<()>;
}

/// A sensor as the engine steps it, with a `SensorContext`; see `MotionStep`.
#[verifier::external]
pub(crate) trait SensorStep {
    fn step(&mut self, context: &mut SensorContext<'_>) -> Result<()>;
}

/// The localization estimator as the engine steps it; see `MotionStep`.
#[verifier::external]
pub(crate) trait LocalizationStep {
    fn step(&mut self, context: &mut LocalizationContext<'_>) -> Result<()>;
}

/// The tracking estimator as the engine steps it; see `MotionStep`.
#[verifier::external]
pub(crate) trait TrackingStep {
    fn step(&mut self, context: &mut TrackingContext<'_>) -> Result<()>;
}

/// One live plugin and everything the engine keeps beside it: its message
/// address, ports, rate limit, random stream, and fixed drawings. `T` is the
/// plugin's trait, so one `Slot` type serves every category. The fields are
/// crate-visible so a proof can state what a step leaves untouched.
#[verifier::reject_recursive_types(T)]
pub(crate) struct Slot<T: ?Sized> {
    pub(crate) name: String,
    pub(crate) endpoint: MessageEndpoint,
    pub(crate) plugin: Box<T>,
    pub(crate) ports: Ports,
    pub(crate) rate: Rate,
    /// Sensors only: see `CompiledPlugin`.
    pub(crate) instance_name: String,
    pub(crate) reading: ReadingType,
    pub(crate) messages: Messages,
    pub(crate) random: PluginRandom,
    /// The plugin's fixed drawings, asked once in `start`.
    pub(crate) fixed_drawings: Arc<[Drawing]>,
}
#[verifier::external]
impl<T: ?Sized> Slot<T> {
    fn new(compiled: &CompiledPlugin<T>, random: PluginRandom, endpoint: MessageEndpoint) -> Self {
        Self {
            name: compiled.name.clone(),
            endpoint,
            plugin: (compiled.instantiate)(),
            ports: compiled.ports.clone(),
            rate: compiled.rate,
            instance_name: compiled.instance_name.clone(),
            reading: compiled.reading,
            messages: Messages::default(),
            random,
            fixed_drawings: Arc::from(Vec::new()),
        }
    }
    fn mailbox(&mut self) -> Mailbox<'_> {
        Mailbox {
            endpoint: &self.endpoint,
            messages: &mut self.messages,
        }
    }

    fn close(
        &mut self,
        time: StepTime,
        close: impl FnOnce(&mut T, StepTime) -> Result<()>,
    ) -> Result<()> {
        close(&mut self.plugin, time).with_context(|| format!("close '{}'", self.name))
    }
}
#[verifier::external]
impl<T: ?Sized + Agent> Slot<T> {
    fn agent_initialize(
        &mut self,
        entity: EntityInfo,
        time: StepTime,
        state: &GaussianState,
        tracks: KnownTracks<'_>,
    ) -> Result<()> {
        self.messages.time_s = time.time_s;
        let mut context = AgentContext {
            entity,
            time,
            state,
            tracks,
            messages: &mut self.messages,
            random: self.random.at_tick(time.tick),
        };
        self.plugin
            .initialize(&mut context)
            .with_context(|| format!("initialize '{}' on entity {}", self.name, entity.id))
    }
}
impl<T: ?Sized + Agent> Slot<T> {
    /// Steps the plugin if its rate says it is due, with the time since it
    /// last ran, the entity's belief and known tracks, and its port values.
    fn agent_step(
        &mut self,
        entity: EntityInfo,
        time: StepTime,
        state: &GaussianState,
        tracks: KnownTracks<'_>,
        signals: &mut Signals,
    ) -> (r: Result<()>)
        ensures proof::agent_stepped(
            old(self), final(self), time, state, tracks, old(signals), final(signals), r is Ok)
    {
        // A rate-limited plugin's dt is the time since it last ran.
        let Some(elapsed_s) = self.rate.due(time.dt_s) else {
            return Ok(());
        };
        let time = StepTime {
            dt_s: elapsed_s,
            ..time
        };
        self.messages.time_s = time.time_s;
        let mut context = AgentContext {
            entity,
            time,
            state,
            tracks,
            messages: &mut self.messages,
            random: self.random.at_tick(time.tick),
        };
        let mut io = PluginIo {
            signals,
            ports: &self.ports,
        };
        // `map_err` rather than `with_context`: Verus can give `with_context`
        // on a `Result` no contract, because `anyhow` bounds it by a private trait.
        self.plugin
            .step(&mut context, &mut io)
            .map_err(|error| error.context(format!("step '{}' on entity {}", self.name, entity.id)))
    }
}

impl<T: ?Sized + MotionStep> Slot<T> {
    /// Steps the motion model with the entity's truth, the forces applied to
    /// it, the terrain, and its port values.
    fn motion_step(
        &mut self,
        entity: EntityInfo,
        time: StepTime,
        truth: &mut KinematicState,
        applied_forces_world_n: &[Vec3],
        terrain: Option<&Terrain>,
        signals: &mut Signals,
    ) -> (r: Result<()>)
        ensures proof::motion_slot_stepped(
            old(self), final(self), entity, time, old(truth), final(truth), applied_forces_world_n@, terrain,
            old(signals), final(signals), r is Ok)
    {
        self.messages.time_s = time.time_s;
        let mut context = MotionContext {
            entity,
            time,
            truth,
            messages: &mut self.messages,
            terrain,
            applied_forces_world_n,
        };
        let mut io = PluginIo {
            signals,
            ports: &self.ports,
        };
        self.plugin
            .step(&mut context, &mut io)
            .map_err(|error| error.context(format!("step '{}' on entity {}", self.name, entity.id)))
    }
}

impl<T: ?Sized + SensorStep> Slot<T> {
    /// Steps the sensor if its rate says it is due, with the committed truth
    /// and every entity's true state, publishing into the pending readings.
    fn sensor_step(
        &mut self,
        entity: EntityInfo,
        time: StepTime,
        truth: &KinematicState,
        contacts_truth: TruthContacts<'_>,
        pending: &mut Readings,
    ) -> (r: Result<()>)
        ensures proof::sensor_stepped(
            old(self), final(self), entity, time, truth, contacts_truth, old(pending), final(pending), r is Ok)
    {
        self.messages.time_s = time.time_s;
        // A sensor's dt stays the tick's: its reading is taken at the tick's end.
        if self.rate.due(time.dt_s).is_none() {
            return Ok(());
        }
        let mut context = SensorContext {
            entity,
            time,
            truth,
            messages: &mut self.messages,
            contacts_truth,
            random: self.random.at_tick(time.tick),
            name: &self.instance_name,
            reading: self.reading,
            pending,
        };
        self.plugin
            .step(&mut context)
            .map_err(|error| error.context(format!("step '{}' on entity {}", self.name, entity.id)))
    }
}

/// Steps each slot in order with the belief, the known tracks, and the
/// port board; stops at the first error. One loop serves the autonomies and
/// the controllers.
fn step_agents<T: ?Sized + Agent>(
    slots: &mut Vec<Slot<T>>,
    entity: EntityInfo,
    time: StepTime,
    state: &GaussianState,
    tracks: KnownTracks<'_>,
    signals: &mut Signals,
) -> (r: Result<()>)
    ensures proof::agents_stepped(old(slots), final(slots), time, state, tracks, old(signals), final(signals), r is Ok)
{
    for slot in it: slots.as_mut_slice().iter_mut()
        invariant proof::agents_so_far(old(slots), it, time, state, tracks, *old(signals), signals)
    {
        slot.agent_step(entity, time, state, tracks, signals)?;
    }
    Ok(())
}

impl<T: ?Sized + LocalizationStep> Slot<T> {
    /// Steps localization with its inputs and the commands the motion model
    /// read this tick; it writes the entity's own-state belief.
    fn localization_step(
        &mut self,
        entity: EntityInfo,
        time: StepTime,
        inputs: Inputs<'_>,
        commands: Commands<'_>,
        state: &mut GaussianState,
    ) -> (r: Result<()>)
        ensures proof::localization_stepped(
            old(self), final(self), entity, time, proof::inputs_view(inputs), proof::commands_view(commands),
            old(state), final(state), r is Ok)
    {
        self.messages.time_s = time.time_s;
        let mut context = LocalizationContext {
            entity,
            time,
            inputs,
            commands,
            messages: &mut self.messages,
            random: self.random.at_tick(time.tick),
            state,
        };
        self.plugin
            .step(&mut context)
            .map_err(|error| error.context(format!("step '{}' on entity {}", self.name, entity.id)))
    }
}

impl<T: ?Sized + TrackingStep> Slot<T> {
    /// Steps tracking with its inputs and localization's estimate as the
    /// pose; it writes the entity's tracks.
    fn tracking_step(
        &mut self,
        entity: EntityInfo,
        time: StepTime,
        inputs: Inputs<'_>,
        pose: &GaussianState,
        tracks: &mut TrackList,
    ) -> (r: Result<()>)
        ensures proof::tracking_stepped(
            old(self), final(self), entity, time, proof::inputs_view(inputs), pose, old(tracks), final(tracks),
            r is Ok)
    {
        self.messages.time_s = time.time_s;
        let mut context = TrackingContext {
            entity,
            time,
            inputs,
            messages: &mut self.messages,
            random: self.random.at_tick(time.tick),
            pose,
            tracks,
        };
        self.plugin
            .step(&mut context)
            .map_err(|error| error.context(format!("step '{}' on entity {}", self.name, entity.id)))
    }
}

/// What an estimator selects its readings from: its sources over the
/// delivered readings and the perfect measurements, for the end of the step.
fn estimator_inputs<'a>(
    sources: &'a [(String, Source)],
    readings: &'a Readings,
    perfect_state: &'a GaussianState,
    perfect_contacts: &'a PerfectContacts,
    time: StepTime,
) -> (r: Inputs<'a>)
    ensures proof::inputs_view(r) == proof::inputs(sources@, *readings, *perfect_state, *perfect_contacts, time)
{
    Inputs {
        sources,
        readings,
        perfect_state,
        perfect_contacts,
        now_s: time.time_s + time.dt_s,
    }
}

/// Steps each sensor in order over the entity's pending readings; stops at
/// the first error.
fn step_sensors<T: ?Sized + SensorStep>(
    slots: &mut Vec<Slot<T>>,
    entity: EntityInfo,
    time: StepTime,
    truth: &KinematicState,
    contacts_truth: TruthContacts<'_>,
    pending: &mut Readings,
) -> (r: Result<()>)
    ensures proof::sensors_stepped(old(slots), final(slots), entity, time, truth, contacts_truth, old(pending), final(pending), r is Ok)
{
    for slot in it: slots.as_mut_slice().iter_mut()
        invariant proof::sensors_so_far(old(slots), it, entity, time, truth, contacts_truth, *old(pending), pending)
    {
        slot.sensor_step(entity, time, truth, contacts_truth, pending)?;
    }
    Ok(())
}

/// What the motion model reads now, kept for localization: the value of each
/// of its inputs, in input order.
fn record_commands(ports: &Ports, signals: &Signals, values: &mut Vec<f64>) -> (r: Result<()>)
    ensures proof::commands_recorded(ports, signals, old(values), final(values), r is Ok)
{
    for name in it: ports.inputs.keys()
        invariant proof::commands_so_far(ports, signals, old(values), values, it)
    {
        let value = signals
            .get(name)
            .with_context(|| format!("motion input '{name}' has no value"))?;
        values.push(*value);
    }
    Ok(())
}

/// One entity's running plugins and the state that passes between them: the
/// autonomy outputs, delivered readings, belief, and tracks. Lives as
/// long as the entity does. The fields are crate-visible so a proof can state
/// what a phase leaves untouched.
pub(crate) struct PluginStack {
    pub(crate) autonomies: Vec<Slot<dyn Autonomy>>,
    pub(crate) controllers: Vec<Slot<dyn Controller>>,
    pub(crate) motion: Slot<dyn MotionModel>,
    pub(crate) sensors: Vec<Slot<dyn Sensor>>,
    pub(crate) localization: Slot<dyn Localization>,
    pub(crate) localization_sources: Vec<(String, Source)>,
    pub(crate) tracking: Slot<dyn Tracking>,
    pub(crate) tracking_sources: Vec<(String, Source)>,
    /// The entity's port values; see `Signals` in ports.rs.
    pub(crate) signals: Signals,
    /// Delivered sensor readings, which only estimators read.
    pub(crate) readings: Readings,
    /// Readings taken this tick, delivered in the estimation step.
    pub(crate) pending: Readings,
    /// Own-state belief, written only by localization.
    pub(crate) belief: GaussianState,
    /// The tracks on other entities, written only by tracking.
    pub(crate) tracks: TrackList,
    /// PassThrough localization of the perfect state measurement: belief is
    /// truth, so controllers can see it after every motion substep.
    pub(crate) belief_is_perfect_state: bool,
    /// What the motion model read on each substep this tick, for localization.
    pub(crate) command_names: Vec<String>,
    pub(crate) command_values: Vec<f64>,
    pub(crate) command_times: Vec<StepTime>,
    pub(crate) closed: bool,
}
impl PluginStack {
    pub fn autonomy(&mut self, entity: EntityInfo, time: StepTime) -> (r: Result<()>)
        ensures proof::autonomy_stepped(old(self), final(self), entity, time, r is Ok)
    {
        let tracks = KnownTracks::new(&self.tracks, entity.id);
        step_agents(
            &mut self.autonomies,
            entity,
            time,
            &self.belief,
            tracks,
            &mut self.signals,
        )
    }
    pub fn controllers(&mut self, entity: EntityInfo, time: StepTime) -> (r: Result<()>)
        ensures proof::controllers_stepped(old(self), final(self), entity, time, r is Ok)
    {
        let tracks = KnownTracks::new(&self.tracks, entity.id);
        step_agents(
            &mut self.controllers,
            entity,
            time,
            &self.belief,
            tracks,
            &mut self.signals,
        )
    }
    /// Between motion substeps: a perfect own-state measurement makes belief
    /// follow truth, so the next substep's controllers see the state just
    /// written. Any other localization updates belief once per tick.
    pub fn follow_perfect_state(&mut self, truth: &KinematicState)
        ensures *final(self) == proof::followed(old(self), truth)
    {
        if self.belief_is_perfect_state {
            self.belief = GaussianState::perfect(*truth);
        }
    }
    pub fn motion(
        &mut self,
        entity: EntityInfo,
        time: StepTime,
        truth: &mut KinematicState,
        applied_forces_world_n: &[Vec3],
        terrain: Option<&Terrain>,
    ) -> (r: Result<()>)
        ensures proof::motion_stepped(
            old(self), final(self), entity, time, old(truth), final(truth), applied_forces_world_n@, terrain, r is Ok)
    {
        record_commands(&self.motion.ports, &self.signals, &mut self.command_values)?;
        self.command_times.push(time);
        self.motion
            .motion_step(entity, time, truth, applied_forces_world_n, terrain, &mut self.signals)
    }
    pub fn sensors(
        &mut self,
        entity: EntityInfo,
        time: StepTime,
        truth: &KinematicState,
        contacts_truth: TruthContacts<'_>,
    ) -> (r: Result<()>)
        ensures proof::sensed(old(self), final(self), entity, time, truth, contacts_truth, r is Ok)
    {
        step_sensors(&mut self.sensors, entity, time, truth, contacts_truth, &mut self.pending)
    }
    /// The estimation step: deliver this tick's readings, then localization, then
    /// tracking. `truth` and `perfect_contacts` are the post-interaction
    /// state, which estimators see only as the perfect measurement: the truth
    /// with a zero covariance.
    pub fn estimate(
        &mut self,
        entity: EntityInfo,
        time: StepTime,
        truth: &KinematicState,
        perfect_contacts: &PerfectContacts,
    ) -> (r: Result<()>)
        ensures proof::estimated(old(self), final(self), entity, time, truth, perfect_contacts, r is Ok)
    {
        self.readings.deliver(&mut self.pending);
        let perfect_state = GaussianState::perfect(*truth);
        let commands = Commands {
            names: &self.command_names,
            values: &self.command_values,
            times: &self.command_times,
        };
        self.localization.localization_step(
            entity,
            time,
            estimator_inputs(&self.localization_sources, &self.readings, &perfect_state, perfect_contacts, time),
            commands,
            &mut self.belief,
        )?;
        self.tracking.tracking_step(
            entity,
            time,
            estimator_inputs(&self.tracking_sources, &self.readings, &perfect_state, perfect_contacts, time),
            &self.belief,
            &mut self.tracks,
        )?;
        self.readings.clear_fresh();
        self.command_values.clear();
        self.command_times.clear();
        Ok(())
    }
}
#[verifier::external]
impl PluginStack {
    /// Motion, then sensors. Estimators and decision plugins
    /// start later, in `start`, once the whole population exists.
    pub fn initialize(
        &mut self,
        entity: EntityInfo,
        time: StepTime,
        truth: &mut KinematicState,
        terrain: Option<&Terrain>,
    ) -> Result<()> {
        self.motion.messages.time_s = time.time_s;
        self.motion
            .plugin
            .initialize(&mut MotionContext {
                entity,
                time,
                truth,
                messages: &mut self.motion.messages,
                terrain,
                applied_forces_world_n: &[],
            })
            .with_context(|| format!("initialize '{}'", self.motion.name))?;
        // A sensor's startup reading sees the initial truth as motion set it up.
        for sensor in &mut self.sensors {
            sensor.messages.time_s = time.time_s;
            sensor
                .plugin
                .initialize(&mut SensorContext {
                    entity,
                    time,
                    truth,
                    messages: &mut sensor.messages,
                    contacts_truth: TruthContacts::from(&[][..]),
                    random: sensor.random.at_tick(time.tick),
                    name: &sensor.instance_name,
                    reading: sensor.reading,
                    pending: &mut self.pending,
                })
                .with_context(|| format!("initialize '{}'", sensor.name))?;
        }
        Ok(())
    }
    /// Sets the first belief from the startup readings and `perfect_contacts`
    /// (the whole population), then initializes controllers and autonomy, which
    /// see it.
    pub fn start(
        &mut self,
        entity: EntityInfo,
        time: StepTime,
        truth: &KinematicState,
        perfect_contacts: &PerfectContacts,
    ) -> Result<()> {
        self.readings.deliver(&mut self.pending);
        let perfect_state = GaussianState::perfect(*truth);
        self.localization.messages.time_s = time.time_s;
        self.localization
            .plugin
            .initialize(&mut LocalizationContext {
                entity,
                time,
                inputs: estimator_inputs(
                    &self.localization_sources,
                    &self.readings,
                    &perfect_state,
                    perfect_contacts,
                    time,
                ),
                commands: Commands {
                    names: &self.command_names,
                    values: &[],
                    times: &[],
                },
                messages: &mut self.localization.messages,
                random: self.localization.random.at_tick(time.tick),
                state: &mut self.belief,
            })
            .with_context(|| format!("initialize '{}'", self.localization.name))?;
        self.tracking.messages.time_s = time.time_s;
        self.tracking
            .plugin
            .initialize(&mut TrackingContext {
                entity,
                time,
                inputs: estimator_inputs(
                    &self.tracking_sources,
                    &self.readings,
                    &perfect_state,
                    perfect_contacts,
                    time,
                ),
                messages: &mut self.tracking.messages,
                random: self.tracking.random.at_tick(time.tick),
                pose: &self.belief,
                tracks: &mut self.tracks,
            })
            .with_context(|| format!("initialize '{}'", self.tracking.name))?;
        self.readings.clear_fresh();
        let tracks = KnownTracks::new(&self.tracks, entity.id);
        for controller in self.controllers.iter_mut().rev() {
            controller.agent_initialize(entity, time, &self.belief, tracks)?;
        }
        for autonomy in &mut self.autonomies {
            autonomy.agent_initialize(entity, time, &self.belief, tracks)?;
        }
        self.ask_fixed_drawings();
        Ok(())
    }
    pub fn readings(&self) -> &Readings {
        &self.readings
    }
    /// Own-state and contact readings taken this tick, not yet delivered, with
    /// their sensors' endpoints; see `Measurement`. For the optional
    /// observation trace.
    pub(crate) fn pending_measurements(&self) -> Result<Vec<Measurement<'_>>> {
        let mut measurements = Vec::new();
        for slot in &self.sensors {
            let name = &slot.instance_name;
            let states = match slot.reading.kind {
                ReadingKind::State => self
                    .pending
                    .get::<GaussianState>(name)?
                    .map(|reading| vec![(None, &reading.value)]),
                ReadingKind::Detections => self.pending.get::<DetectionList>(name)?.map(|reading| {
                    reading
                        .value
                        .iter()
                        .map(|detection| {
                            (
                                detection.identity.map(|identity| identity.id),
                                &detection.state,
                            )
                        })
                        .collect()
                }),
                ReadingKind::Other => None,
            };
            if let Some(states) = states {
                measurements.push((&slot.endpoint, states));
            }
        }
        Ok(measurements)
    }
    /// Autonomy then controller outputs as (plugin, port, value), for the comparison
    /// trace. Like C++ VariableIO after connect(), only ports the next plugin reads.
    pub fn outputs(&self) -> Vec<(&str, &str, f64)> {
        let mut outputs = Vec::new();
        let first_consumer = self
            .controllers
            .first()
            .map_or(&self.motion.ports, |slot| &slot.ports);
        for slot in &self.autonomies {
            for port in slot.ports.outputs.keys() {
                if first_consumer.reads(port) {
                    outputs.push((slot.name.as_str(), port.as_str(), self.signals[port]));
                }
            }
        }
        for (index, slot) in self.controllers.iter().enumerate() {
            let consumer = self
                .controllers
                .get(index + 1)
                .map_or(&self.motion.ports, |next| &next.ports);
            for port in slot.ports.outputs.keys() {
                if consumer.reads(port) {
                    outputs.push((slot.name.as_str(), port.as_str(), self.signals[port]));
                }
            }
        }
        outputs
    }
    pub fn belief(&self) -> &GaussianState {
        &self.belief
    }
    pub fn tracks(&self) -> &TrackList {
        &self.tracks
    }
    pub fn mailboxes<'a>(&'a mut self, mailboxes: &mut Vec<Mailbox<'a>>) {
        for slot in &mut self.autonomies {
            mailboxes.push(slot.mailbox());
        }
        for slot in &mut self.controllers {
            mailboxes.push(slot.mailbox());
        }
        mailboxes.push(self.motion.mailbox());
        for sensor in &mut self.sensors {
            mailboxes.push(sensor.mailbox());
        }
        // DEVNOTE: networks look through every mailbox each tick, and the two
        // estimator mailboxes per entity made the `motion` workload at 512 aircraft
        // about 3% slower (macOS `sample`). An idle mailbox can neither send
        // nor receive, so leaving it out changes no output.
        for slot in [self.localization.mailbox(), self.tracking.mailbox()] {
            if !slot.messages.is_idle() {
                mailboxes.push(slot);
            }
        }
    }
    /// Ask every plugin for its fixed drawings, once, after they are all
    /// initialized.
    fn ask_fixed_drawings(&mut self) {
        for slot in &mut self.autonomies {
            slot.fixed_drawings = slot.plugin.fixed_drawings().into();
        }
        for slot in &mut self.controllers {
            slot.fixed_drawings = slot.plugin.fixed_drawings().into();
        }
        self.motion.fixed_drawings = self.motion.plugin.fixed_drawings().into();
        for slot in &mut self.sensors {
            slot.fixed_drawings = slot.plugin.fixed_drawings().into();
        }
        self.localization.fixed_drawings = self.localization.plugin.fixed_drawings().into();
        self.tracking.fixed_drawings = self.tracking.plugin.fixed_drawings().into();
    }

    /// Each plugin's drawings at `time_s`, in the stack's order, with the
    /// source `<plugin>/<entity id>`: its fixed part by reference and its
    /// changing part asked now. Plugins with nothing in either are left out.
    pub fn drawings(&self, entity_id: i32, time_s: f64) -> Vec<PluginDrawings> {
        let mut drawings = Vec::new();
        let mut collect = |name: &str, fixed: &Arc<[Drawing]>, changing: Vec<Drawing>| {
            if !fixed.is_empty() || !changing.is_empty() {
                drawings.push(PluginDrawings {
                    source: format!("{name}/{entity_id}"),
                    fixed: fixed.clone(),
                    changing,
                });
            }
        };
        for slot in &self.autonomies {
            collect(
                &slot.name,
                &slot.fixed_drawings,
                slot.plugin.drawings(time_s),
            );
        }
        for slot in &self.controllers {
            collect(
                &slot.name,
                &slot.fixed_drawings,
                slot.plugin.drawings(time_s),
            );
        }
        collect(
            &self.motion.name,
            &self.motion.fixed_drawings,
            self.motion.plugin.drawings(time_s),
        );
        for slot in &self.sensors {
            collect(
                &slot.name,
                &slot.fixed_drawings,
                slot.plugin.drawings(time_s),
            );
        }
        collect(
            &self.localization.name,
            &self.localization.fixed_drawings,
            self.localization.plugin.drawings(time_s),
        );
        collect(
            &self.tracking.name,
            &self.tracking.fixed_drawings,
            self.tracking.plugin.drawings(time_s),
        );
        drawings
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
        for slot in &mut self.autonomies {
            record(slot.close(time, Autonomy::close));
        }
        for slot in &mut self.controllers {
            record(slot.close(time, Controller::close));
        }
        record(self.motion.close(time, MotionModel::close));
        for sensor in &mut self.sensors {
            record(sensor.close(time, Sensor::close));
        }
        record(self.localization.close(time, Localization::close));
        record(self.tracking.close(time, Tracking::close));
        failure.map_or(Ok(()), Err)
    }
}

} // verus!

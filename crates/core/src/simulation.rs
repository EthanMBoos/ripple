//! Fixed-step orchestration with explicit phase boundaries.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use anyhow::Result;
use serde::Serialize;
use vstd::prelude::*;

use crate::{
    common::{SpatialIndex, Terrain, TruthContacts},
    drawing::PluginDrawings,
    entity::{Entity, EntityDefinition, EntitySnapshot, Phase},
    plugin::estimator::{Detection, DetectionList, PerfectContacts, Track, TrackList},
    plugin::{Messages, MetricReport, PluginRegistry, StepTime},
    pubsub::routing::NetworkTrace,
    scenario::{EndConditions, ResolvedScenario, ScenarioConfig},
    simulation::trace::Trace,
    simulation::world_plugins::WorldPlugins,
};

pub(crate) mod generation;
mod proof;
mod spawning;
mod trace;
pub(crate) mod world_plugins;

pub use generation::{PluginOverlay, PluginSlot, SpawnRequest};
pub use trace::TraceSink;

use generation::Generator;

verus! {

broadcast use proof::simulation_facts;

#[verifier::external_derive(Serialize)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum EventKind {
    EntityGenerated,
    EntityRemoved,
    EntityPresentAtEnd,
    TeamCollision,
    NonTeamCollision,
    GroundCollision,
}

impl EventKind {
    /// The topic an engine event is published on: its kind's name.
    pub fn topic(self) -> (r: &'static str)
        ensures r@ == proof::topic_of(self)
    {
        match self {
            Self::EntityGenerated => "EntityGenerated",
            Self::EntityRemoved => "EntityRemoved",
            Self::EntityPresentAtEnd => "EntityPresentAtEnd",
            Self::TeamCollision => "TeamCollision",
            Self::NonTeamCollision => "NonTeamCollision",
            Self::GroundCollision => "GroundCollision",
        }
    }
}

#[verifier::external_derive(Serialize)]
#[derive(Debug, Serialize)]
pub struct Event {
    pub time_s: f64,
    pub kind: EventKind,
    pub entity_ids: Vec<i32>,
}
impl Clone for Event {
    fn clone(&self) -> (r: Self)
        ensures proof::same_event(&r, self)
    {
        Event {
            time_s: self.time_s,
            kind: self.kind,
            entity_ids: self.entity_ids.clone(),
        }
    }
}

#[verifier::external_derive(Serialize)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum TerminationReason {
    TimeLimit,
    NoEntities,
    OneTeamLeft,
    ExternalRequest,
    PluginRequestedStop,
    PluginFailure,
}

#[verifier::external_derive(Serialize)]
#[verifier::allow(autoderive_clone_without_spec)]
#[derive(Clone, Debug, Serialize)]
pub struct SimulationFrame {
    pub time_s: f64,
    pub entities: Vec<EntitySnapshot>,
    /// What every plugin draws at this time, entities' plugins in entity
    /// order then world plugins; each plugin's fixed part is shared by
    /// reference between frames.
    pub drawings: Vec<PluginDrawings>,
}

pub struct Simulation {
    pub(crate) config: ScenarioConfig,
    /// Shared with the command, which runs many simulations from one
    /// registry; kept to configure the plugins of groups that plugins
    /// request entities from during the run.
    pub(crate) registry: Arc<PluginRegistry>,
    pub(crate) definitions: Vec<Arc<EntityDefinition>>,
    pub(crate) entities: Vec<Entity>,
    pub(crate) events: Vec<Event>,
    pub(crate) time_s: f64,
    pub(crate) step_count: usize,
    /// The tick being run: `step_count`, or -1 while the initial population
    /// is built one step before the start time. Labels random draws.
    pub(crate) tick: i64,
    pub(crate) termination: Option<TerminationReason>,
    pub(crate) generators: Vec<Generator>,
    pub(crate) world: WorldPlugins,
    pub(crate) messages: Messages,
    pub(crate) entity_teams: BTreeMap<i32, i32>,
    pub(crate) world_stop: bool,
    pub(crate) used_ids: BTreeSet<i32>,
    pub(crate) end_conditions: EndConditions,
    pub(crate) finished: bool,
    /// The latest perfect contact measurement: every entity present after the
    /// last interactions. New entities start from it.
    pub(crate) perfect_contacts: PerfectContacts,
    pub(crate) terrain: Option<Terrain>,
}

#[verifier::external]
impl Simulation {
    pub fn new(scenario: ScenarioConfig, registry: Arc<PluginRegistry>) -> Result<Self> {
        let scenario = scenario.resolve(&registry)?;
        let ResolvedScenario {
            config,
            definitions,
            generators,
            world,
            end_conditions,
        } = scenario;
        let terrain = config.terrain.as_deref().map(Terrain::load).transpose()?;

        let world = world.instantiate(config.run.seed);
        let mut simulation = Self {
            time_s: config.run.start_s - config.run.dt_s,
            config,
            registry,
            definitions,
            entities: Vec::new(),
            events: Vec::new(),
            step_count: 0,
            tick: -1,
            termination: None,
            generators,
            world,
            messages: Messages::default(),
            entity_teams: BTreeMap::new(),
            world_stop: false,
            used_ids: BTreeSet::from([0]),
            end_conditions,
            finished: false,
            perfect_contacts: PerfectContacts {
                detections: DetectionList::from([]),
                tracks: TrackList::from([]),
            },
            terrain,
        };

        simulation.world.initialize(
            StepTime {
                time_s: simulation.config.run.start_s,
                dt_s: simulation.config.run.dt_s,
                tick: 0,
            },
            &[],
            &simulation.entity_teams,
            simulation.terrain.as_ref(),
        )?;
        // The engine's own mailbox takes the spawn requests plugins publish.
        simulation
            .messages
            .subscribe::<SpawnRequest>("GlobalNetwork", SpawnRequest::TOPIC)?;
        // Setup pass, labeled one step before the start (tick -1): build every
        // group due at startup, then run the interactions once, so starting
        // overlaps and collisions are settled before the first tick runs.
        simulation.generate_entities()?;
        let mut nearby =
            SpatialIndex::build(simulation.config.spatial_lookup, &simulation.entities);
        simulation.apply_interactions(&mut nearby)?;
        // Every entity's first belief comes from one snapshot of the whole
        // initial population, before any decision plugin initializes.
        let snapshot = simulation.snapshot();
        simulation.perfect_contacts = perfect_contacts(&snapshot.entities, simulation.time_s);
        simulation.start_entities()?;
        simulation.time_s = simulation.config.run.start_s;
        simulation.tick = 0;
        Ok(simulation)
    }

    /// The scenario as loaded, with every estimator input's source filled in.
    pub fn scenario(&self) -> &ScenarioConfig {
        &self.config
    }

    /// The mission's terrain, if it names one.
    pub fn terrain(&self) -> Option<&Terrain> {
        self.terrain.as_ref()
    }

    pub fn entities(&self) -> &[Entity] {
        &self.entities
    }

    pub fn events(&self) -> &[Event] {
        &self.events
    }

    pub fn time_s(&self) -> f64 {
        self.time_s
    }

    pub fn step_count(&self) -> usize {
        self.step_count
    }

    pub fn termination(&self) -> Option<TerminationReason> {
        self.termination
    }

    fn snapshot(&self) -> SimulationFrame {
        let mut entities: Vec<_> = self.entities.iter().map(Entity::snapshot).collect();
        entities.sort_by_key(|entity| entity.id);
        SimulationFrame {
            // A frame holds the state at the current time, before this tick's step.
            time_s: self.time_s,
            entities,
            drawings: Vec::new(),
        }
    }

    /// The frame's drawings: every plugin's, in entity-ID order then the
    /// world plugins'. Each plugin's fixed part is held by reference; only
    /// its changing part is asked here.
    ///
    /// DEVNOTE: asking every plugin for all of its drawings each tick rebuilt
    /// the Dispatcher's 576 labeled points at 64 hubs on every tick to find
    /// them unchanged. With the fixed and changing split, a 64-hub one-hour
    /// day (`region.yaml` with `hubs:=64`, `run.end_s:=3600`, headless
    /// release, 2026-11-07) took 16.8 to 17.5 s before and 17.3 to 18.8 s
    /// after over five alternating pairs: about 14 µs a tick to ask 321
    /// entities' slots. Kept because it is paid once per tick, and a plugin
    /// with nothing changing returns an empty list without allocating.
    fn drawings(&self) -> Vec<PluginDrawings> {
        let mut entities: Vec<&Entity> = self.entities.iter().collect();
        entities.sort_by_key(|entity| entity.id);
        let mut drawings = Vec::new();
        for entity in entities {
            drawings.extend(entity.drawings(self.time_s));
        }
        drawings.extend(self.world.drawings(self.time_s));
        drawings
    }

    fn route_messages(
        &mut self,
        time: StepTime,
        contacts: &[EntitySnapshot],
        nearby: &SpatialIndex,
        trace: Option<&mut Trace<'_>>,
    ) -> Result<()> {
        let mut network_trace = trace.as_ref().map(|_| NetworkTrace::default());
        self.world.networks(
            time,
            TruthContacts::new(contacts, nearby),
            &mut self.entities,
            &mut self.messages,
            network_trace.as_mut(),
        )?;
        if let (Some(trace), Some(network_trace)) = (trace, &network_trace) {
            trace.network(network_trace)?;
        }
        Ok(())
    }

    /// Its own method because an exec `+= 1` on a `usize` needs an overflow
    /// proof; see the proof file.
    fn count_step(&mut self) {
        self.step_count += 1;
    }

    fn reached_end_condition(&self) -> Option<TerminationReason> {
        if self.world_stop {
            return Some(TerminationReason::PluginRequestedStop);
        }
        if self.end_conditions.time
            && self.time_s > self.config.run.end_s - self.config.run.dt_s / 2.0
        {
            return Some(TerminationReason::TimeLimit);
        }
        if self.entities.is_empty()
            && (self.end_conditions.no_entities || self.end_conditions.one_team_left)
        {
            return Some(TerminationReason::NoEntities);
        }
        if self.end_conditions.one_team_left {
            let remaining_teams: BTreeSet<_> =
                self.entities.iter().map(|entity| entity.team_id).collect();
            if remaining_teams.len() == 1 {
                return Some(TerminationReason::OneTeamLeft);
            }
        }
        None
    }

    fn finish(&mut self, trace: Option<&mut Trace<'_>>) -> Result<SimulationFrame> {
        self.finished = true;
        // The terminal frame holds the state after the last step, at the end time.
        for entity in &self.entities {
            self.events.push(Event {
                time_s: self.time_s,
                kind: EventKind::EntityPresentAtEnd,
                entity_ids: vec![entity.id],
            });
        }
        if let Some(trace) = trace {
            trace.beliefs(self.time_s, &self.entities)?;
        }
        let mut frame = self.snapshot();
        frame.drawings = self.drawings();
        self.close_plugins()?;
        Ok(frame)
    }

    fn close_plugins(&mut self) -> Result<()> {
        let time = self.step_time();
        let mut failure = None;
        for entity in &mut self.entities {
            if let Err(error) = entity.close(time) {
                failure.get_or_insert(error);
            }
        }
        if let Err(error) = self.world.close(time) {
            failure.get_or_insert(error);
        }
        failure.map_or(Ok(()), Err)
    }

    fn startup_collision_exists(&self, position_world_m: crate::Vec3) -> bool {
        self.world
            .collision_exists(&self.entities, self.terrain.as_ref(), position_world_m)
    }


    pub fn metric_reports(&self) -> Vec<(String, MetricReport)> {
        self.world.reports(self.time_s)
    }

    pub fn stop(&mut self) {
        if self.termination.is_none() {
            self.termination = Some(TerminationReason::ExternalRequest);
        }
    }
}

impl Simulation {
    /// The phase time of the tick being run.
    fn step_time(&self) -> (r: StepTime)
        ensures r == proof::step_time_of(self)
    {
        StepTime {
            time_s: self.time_s,
            dt_s: self.config.run.dt_s,
            tick: self.tick,
        }
    }

    /// The interactions phase: the world's interactions over every entity,
    /// then every entity they left with no health taken out of play.
    fn apply_interactions(&mut self, nearby: &mut SpatialIndex) -> (r: Result<()>)
        ensures proof::interacted(old(self), final(self), r is Ok)
    {
        let time = self.step_time();
        clear_applied_forces(&mut self.entities);
        self.world_stop |= self.world.interactions(
            time,
            self.entities.as_mut_slice(),
            nearby,
            self.terrain.as_ref(),
            &mut self.events,
        )?;
        self.messages.time_s = self.time_s;
        deactivate_dead(&mut self.entities, self.time_s, &mut self.messages, &mut self.events);
        Ok(())
    }

    /// Starts entities created since the last call; others learn of them at their
    /// next measurement.
    fn start_entities(&mut self) -> (r: Result<()>)
        ensures proof::started(old(self), final(self), r is Ok)
    {
        start_each(&mut self.entities, &self.perfect_contacts)
    }

    /// The end of a tick: one more step, the tick and the time that go with
    /// it, and the end condition read from the state that leaves.
    fn advance_time(&mut self)
        ensures proof::advanced(old(self), final(self))
    {
        self.count_step();
        self.tick = self.step_count as i64;
        // Time is counted, not accumulated: repeated addition of dt drifts in
        // the last digits (0.1 added 299 times is 29.900000000000155).
        self.time_s = self.config.run.start_s + self.step_count as f64 * self.config.run.dt_s;
        self.termination = self.reached_end_condition();
    }

    /// Sensors measure the post-motion world; returns the spatial index built
    /// from it for the interactions.
    fn sense(&mut self, time: StepTime) -> (r: Result<SpatialIndex>)
        ensures proof::sensed(old(self), final(self), time, r is Ok)
    {
        // Refreshed again around the interactions, which may move entities.
        let nearby = SpatialIndex::build(self.config.spatial_lookup, &self.entities);
        let sensor_contacts = self.snapshot();
        let truth = TruthContacts::new(&sensor_contacts.entities, &nearby);
        run_phase(&mut self.entities, Phase::Sensors { time, truth })?;
        Ok(nearby)
    }

    /// Estimation shares the delivery task: readings are delivered and belief
    /// updated per entity, with no extra phase.
    fn estimate(&mut self, time: StepTime, contacts: &[EntitySnapshot]) -> (r: Result<()>)
        ensures proof::estimated(old(self), final(self), time, contacts@, r is Ok)
    {
        self.perfect_contacts = perfect_contacts(contacts, time.time_s + time.dt_s);
        run_phase(
            &mut self.entities,
            Phase::Estimate {
                time,
                perfect_contacts: &self.perfect_contacts,
            },
        )
    }

    /// The metrics phase: the world's metrics plugins over the truth contacts.
    fn score(
        &mut self,
        time: StepTime,
        contacts: &[EntitySnapshot],
        nearby: &SpatialIndex,
    ) -> (r: Result<()>)
        ensures proof::scored(old(self), final(self), time, contacts@, r is Ok)
    {
        self.world_stop |= self.world.metrics(
            time,
            TruthContacts::new(contacts, nearby),
            &self.entity_teams,
            self.terrain.as_ref(),
        )?;
        Ok(())
    }

    /// Controllers, then motion, once per substep.
    fn controllers_and_motion(&mut self, time: StepTime) -> (r: Result<()>)
        ensures proof::substepped(old(self), final(self), time, r is Ok)
    {
        // Controllers then motion per substep, so a controller sees the state
        // the previous substep wrote (the book's SCRIMMAGE chapter, "Substep
        // order").
        let mut substep = StepTime {
            dt_s: time.dt_s / self.config.run.motion_multiplier as f64,
            ..time
        };
        for i in it: 0..self.config.run.motion_multiplier
            invariant proof::substeps_so_far(old(self), self, it, time, substep)
        {
            run_phase(
                &mut self.entities,
                Phase::Controllers {
                    time: substep,
                    after_motion: i > 0,
                },
            )?;
            run_phase(
                &mut self.entities,
                Phase::Motion {
                    time: substep,
                    terrain: self.terrain.as_ref(),
                },
            )?;
            substep.time_s += substep.dt_s;
        }
        Ok(())
    }

    /// Returns pre-step state, then advances the simulation. The final call emits terminal state.
    pub fn step(&mut self) -> (r: Result<Option<SimulationFrame>>)
        ensures proof::stepped_and_settled(old(self), final(self), r)
    {
        self.step_with_trace(None)
    }

    /// Record this step's observations while keeping the writer with the caller.
    pub fn step_with_trace(&mut self, sink: Option<&mut dyn TraceSink>) -> (r: Result<Option<SimulationFrame>>)
        ensures proof::stepped_and_settled(old(self), final(self), r)
    {
        let mut trace = sink.map(Trace::new);
        let result = self.step_inner(trace.as_mut());
        if result.is_err() {
            self.termination = Some(TerminationReason::PluginFailure);
            self.finished = true;
            let _ = self.close_plugins();
        }
        result
    }

    /// One tick, phase by phase, in the order of the book's phase table.
    fn step_inner(&mut self, mut trace: Option<&mut Trace<'_>>) -> (r: Result<Option<SimulationFrame>>)
        ensures proof::stepped(old(self), final(self), r)
    {
        if self.finished {
            return Ok(None);
        }
        if self.termination.is_some() {
            return Ok(Some(self.finish(trace)?));
        }

        self.generate_entities()?;
        self.start_entities()?;
        let mut frame = self.snapshot();
        frame.drawings = self.drawings();
        let time = self.step_time();
        if let Some(trace) = trace.as_mut() {
            trace.beliefs(time.time_s, &self.entities)?;
        }
        run_phase(&mut self.entities, Phase::Autonomy(time))?;
        self.controllers_and_motion(time)?;
        // Motion leaves controller outputs unchanged: this is the last substep's.
        if let Some(trace) = trace.as_mut() {
            trace.outputs(time.time_s, &self.entities)?;
        }
        let mut nearby = self.sense(time)?;
        if let Some(trace) = trace.as_mut() {
            trace.readings(time.time_s, &self.entities)?;
        }
        self.apply_interactions(&mut nearby)?;
        nearby.refresh(&self.entities);
        let contacts = self.snapshot().entities;
        self.route_messages(time, &contacts, &nearby, trace)?;
        self.estimate(time, &contacts)?;
        self.score(time, &contacts, &nearby)?;
        remove_dead(&mut self.entities, time)?;
        self.advance_time();
        Ok(Some(frame))
    }
}

/// The perfect contact measurement: every entity not marked for removal, its
/// truth known exactly, as detections and as the tracks that believe them,
/// both taken at `sampled_at_s`.
#[verifier::external]
fn perfect_contacts(snapshot: &[EntitySnapshot], sampled_at_s: f64) -> PerfectContacts {
    let mut detections = Vec::with_capacity(snapshot.len());
    detections.extend(
        snapshot
            .iter()
            .filter(|entity| entity.active)
            .map(Detection::perfect),
    );
    let tracks: Vec<Track> = detections
        .iter()
        .map(|detection| {
            Track::believing(detection, sampled_at_s).expect("a perfect detection is identified")
        })
        .collect();
    PerfectContacts {
        detections: detections.into(),
        tracks: tracks.into(),
    }
}

#[verifier::external]
impl Drop for Simulation {
    fn drop(&mut self) {
        let _ = self.close_plugins();
    }
}

/// One phase over every active entity, in entity order, on this thread.
/// Every entity runs before the first error is returned, so a failing plugin
/// never leaves the rest of the fleet one phase behind.
///
/// DEVNOTE: this replaced a Rayon worker pool (`--workers N`) on 2026-11-14.
/// Profiling the benchmark `motion` workload (64 aircraft, 20,000 steps) with macOS
/// `sample` showed the main thread waiting on the pool's latch: handing a
/// phase to the pool and joining cost 7 µs with one thread and 35 µs with
/// eight, against phases whose whole work was a few microseconds. Inline took
/// the run from 2.04 s to 1.28 s with identical output, and a 64-hub delivery
/// day was slower on eight workers than on one. A pool pays back only when a
/// phase's total work exceeds roughly 50 µs, and nothing planned reaches
/// that: a ray-casting sensor would thread inside its own `step`, a learned
/// policy batches one forward pass at the world level, model calls wait on
/// the network, and `ripple sweep --jobs` gives throughput across episodes.
/// The invariant the pool enforced stays: a phase sees its own entity plus
/// read-only snapshots taken before the phase.
fn run_phase(entities: &mut Vec<Entity>, phase: Phase<'_>) -> (r: Result<()>)
    ensures proof::phase_ran(old(entities), final(entities), phase, r is Ok)
{
    let mut first_error = None;
    for entity in it: entities.as_mut_slice().iter_mut()
        invariant proof::phase_so_far(old(entities), it, phase, &first_error)
    {
        if !entity.active {
            continue;
        }
        if let Err(error) = entity.step_phase(phase) {
            first_error.get_or_insert(error);
        }
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Starts every entity not yet started, in entity order, from the perfect
/// contacts; stops at the first to fail.
fn start_each(entities: &mut Vec<Entity>, perfect_contacts: &PerfectContacts) -> (r: Result<()>)
    ensures proof::all_started(old(entities), final(entities), perfect_contacts, r is Ok)
{
    for entity in it: entities.as_mut_slice().iter_mut()
        invariant proof::started_so_far(old(entities), it, perfect_contacts)
    {
        entity.start(perfect_contacts)?;
    }
    Ok(())
}

/// Closes every entity with no health left, in entity order; every one runs
/// before the first error is returned.
fn close_dead(entities: &mut Vec<Entity>, time: StepTime) -> (r: Result<()>)
    ensures proof::dead_closed(old(entities), final(entities), time, r is Ok)
{
    let mut first_error = None;
    for entity in it: entities.as_mut_slice().iter_mut()
        invariant proof::closed_so_far(old(entities), it, time, &first_error)
    {
        if entity.health > 0 {
            continue;
        }
        if let Err(error) = entity.close(time) {
            first_error.get_or_insert(error);
        }
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Removal happens after networks and metrics so final-tick publications
/// are delivered.
fn remove_dead(entities: &mut Vec<Entity>, time: StepTime) -> (r: Result<()>)
    ensures proof::dead_removed(old(entities), final(entities), time, r is Ok)
{
    let closed = close_dead(entities, time);
    entities.retain(Entity::is_alive);
    closed
}

/// Forces applied last time acted in the motion phase just run.
fn clear_applied_forces(entities: &mut Vec<Entity>)
    ensures proof::all_forces_cleared(old(entities)@, final(entities)@)
{
    for entity in it: entities.as_mut_slice().iter_mut()
        invariant proof::forces_cleared_so_far(old(entities), it)
    {
        entity.clear_applied_forces();
    }
}

/// Takes every active entity with no health left out of play, in entity
/// order, recording its removal at `time_s` as an event and announcing it on
/// the global network.
fn deactivate_dead(entities: &mut Vec<Entity>, time_s: f64, messages: &mut Messages, events: &mut Vec<Event>)
    ensures proof::dead_deactivated(old(entities)@, final(entities)@, time_s, old(events)@, final(events)@, old(messages), final(messages))
{
    for entity in it: entities.as_mut_slice().iter_mut()
        invariant proof::deactivated_so_far(old(entities), it, time_s, old(events)@, events, old(messages), messages)
    {
        if entity.health > 0 || !entity.active {
            continue;
        }
        entity.deactivate();
        let event = Event {
            time_s,
            kind: EventKind::EntityRemoved,
            entity_ids: vec![entity.id],
        };
        messages.publish_engine_event("GlobalNetwork", event.kind.topic(), event.clone());
        events.push(event);
    }
}

} // verus!

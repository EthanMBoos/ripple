//! Fixed-step orchestration with explicit phase boundaries.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use serde::Serialize;

use crate::{
    common::{SpatialIndex, Terrain, TruthContacts},
    entity::{Entity, EntityDefinition, EntitySnapshot},
    plugin::estimator::{Contact, ContactList},
    plugin::{Messages, MetricReport, PluginRegistry, StepTime},
    pubsub::messages::NetworkTrace,
    scenario::{EndConditions, ResolvedScenario, ScenarioConfig},
    simulation::scheduler::Scheduler,
    simulation::trace::Trace,
    simulation::world_plugins::WorldPlugins,
};

pub(crate) mod generation;
mod scheduler;
mod trace;
pub(crate) mod world_plugins;

pub use trace::TraceSink;

use generation::Generator;

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
    pub fn topic(self) -> &'static str {
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

#[derive(Clone, Debug, Serialize)]
pub struct Event {
    pub time_s: f64,
    pub kind: EventKind,
    pub entity_ids: Vec<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum TerminationReason {
    TimeLimit,
    NoEntities,
    OneTeamLeft,
    ExternalRequest,
    PluginRequestedStop,
    PluginFailure,
}

#[derive(Clone, Debug, Serialize)]
pub struct SimulationFrame {
    pub time_s: f64,
    pub entities: Vec<EntitySnapshot>,
}

pub struct Simulation {
    config: ScenarioConfig,
    definitions: Vec<EntityDefinition>,
    entities: Vec<Entity>,
    events: Vec<Event>,
    time_s: f64,
    step_count: usize,
    /// The tick being run: `step_count`, or -1 while the initial population
    /// is built one step before the start time. Labels random draws.
    tick: i64,
    termination: Option<TerminationReason>,
    scheduler: Scheduler,
    generators: Vec<Generator>,
    world: WorldPlugins,
    messages: Messages,
    entity_teams: BTreeMap<i32, i32>,
    world_stop: bool,
    used_ids: BTreeSet<i32>,
    end_conditions: EndConditions,
    finished: bool,
    /// The latest perfect contact measurement: every entity present after the
    /// last interactions. New entities start from it.
    perfect_contacts: ContactList,
    terrain: Option<Terrain>,
}

impl Simulation {
    pub fn new(
        scenario: ScenarioConfig,
        registry: &PluginRegistry,
        worker_count: usize,
    ) -> Result<Self> {
        let scenario = scenario.resolve(registry)?;
        let ResolvedScenario {
            config,
            definitions,
            generators,
            world,
            end_conditions,
        } = scenario;
        let scheduler = Scheduler::new(worker_count)?;
        let terrain = config.terrain.as_deref().map(Terrain::load).transpose()?;

        let world = world.instantiate(config.run.seed);
        let mut simulation = Self {
            time_s: config.run.start_s - config.run.dt_s,
            config,
            definitions,
            entities: Vec::new(),
            events: Vec::new(),
            step_count: 0,
            tick: -1,
            termination: None,
            scheduler,
            generators,
            world,
            messages: Messages::default(),
            entity_teams: BTreeMap::new(),
            world_stop: false,
            used_ids: BTreeSet::from([0]),
            end_conditions,
            finished: false,
            perfect_contacts: ContactList::from([]),
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

    fn run_phase(&mut self, update: impl Fn(&mut Entity) -> Result<()> + Sync) -> Result<()> {
        let result = self.scheduler.run_phase(&mut self.entities, |entity| {
            if entity.is_active() {
                update(entity)
            } else {
                Ok(())
            }
        });
        if result.is_err() {
            self.termination = Some(TerminationReason::PluginFailure);
            self.finished = true;
        }
        result
    }

    /// Starts entities created since the last call; others learn of them at their
    /// next measurement.
    fn start_entities(&mut self) -> Result<()> {
        for entity in &mut self.entities {
            entity.start(&self.perfect_contacts)?;
        }
        Ok(())
    }

    fn snapshot(&self) -> SimulationFrame {
        let mut entities: Vec<_> = self.entities.iter().map(Entity::snapshot).collect();
        entities.sort_by_key(|entity| entity.id);
        SimulationFrame {
            // A frame holds the state at the current time, before this tick's step.
            time_s: self.time_s,
            entities,
        }
    }

    /// Returns pre-step state, then advances the simulation. The final call emits terminal state.
    pub fn step(&mut self) -> Result<Option<SimulationFrame>> {
        self.step_with_trace(None)
    }

    /// Record this step's observations while keeping the writer with the caller.
    pub fn step_with_trace(
        &mut self,
        sink: Option<&mut dyn TraceSink>,
    ) -> Result<Option<SimulationFrame>> {
        let mut trace = sink.map(Trace::new);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.step_inner(trace.as_mut())
        }))
        .unwrap_or_else(|_| Err(anyhow::anyhow!("simulation plugin panicked")));
        if result.is_err() {
            self.termination = Some(TerminationReason::PluginFailure);
            self.finished = true;
            let _ = self.close_plugins();
        }
        result
    }

    /// One tick, phase by phase, in the order of the book's phase table.
    fn step_inner(&mut self, mut trace: Option<&mut Trace<'_>>) -> Result<Option<SimulationFrame>> {
        if self.finished {
            return Ok(None);
        }
        if self.termination.is_some() {
            return Ok(Some(self.finish(trace)?));
        }

        self.generate_entities()?;
        self.start_entities()?;
        let frame = self.snapshot();
        let time = StepTime {
            time_s: self.time_s,
            dt_s: self.config.run.dt_s,
            tick: self.tick,
        };
        if let Some(trace) = trace.as_deref_mut() {
            trace.beliefs(time.time_s, &self.entities)?;
        }
        self.run_phase(|entity| entity.step_autonomy(time))?;
        self.controllers_and_motion(time)?;
        // Motion leaves controller outputs unchanged: this is the last substep's.
        if let Some(trace) = trace.as_deref_mut() {
            trace.outputs(time.time_s, &self.entities)?;
        }
        let mut nearby = self.sense(time)?;
        if let Some(trace) = trace.as_deref_mut() {
            trace.readings(time.time_s, &self.entities)?;
        }
        self.apply_interactions(&mut nearby)?;
        nearby.refresh(&self.entities);
        let contacts = self.snapshot().entities;
        self.route_messages(time, &contacts, &nearby, trace)?;
        self.estimate(time, &contacts)?;
        self.score(time, &contacts, &nearby)?;
        self.remove_dead(time)?;
        self.advance_time();
        Ok(Some(frame))
    }

    /// Controllers, then motion, once per substep.
    fn controllers_and_motion(&mut self, time: StepTime) -> Result<()> {
        // Each substep runs controllers, then motion, so a controller sees the
        // state the previous substep wrote (see docs/REFERENCE_NOTES.md,
        // "Substep order"). Autonomy before and everything after run once per tick.
        let StepTime { time_s, dt_s, tick } = time;
        let motion_dt_s = dt_s / self.config.run.motion_multiplier as f64;
        let mut substep_time_s = time_s;
        let terrain = self.terrain.clone();
        for substep in 0..self.config.run.motion_multiplier {
            let time = StepTime {
                time_s: substep_time_s,
                dt_s: motion_dt_s,
                tick,
            };
            self.run_phase(|entity| {
                if substep > 0 {
                    entity.follow_perfect_state();
                }
                entity.step_controller(time)
            })?;
            self.run_phase(|entity| entity.step_motion(time, terrain.as_ref()))?;
            substep_time_s += motion_dt_s;
        }
        Ok(())
    }

    /// Sensors measure the post-motion world; returns the spatial index built
    /// from it for the interactions.
    fn sense(&mut self, time: StepTime) -> Result<SpatialIndex> {
        // The spatial index is built from post-motion positions and refreshed
        // before each interaction and after them, so every query sees current
        // positions (an interaction may move entities).
        let nearby = SpatialIndex::build(self.config.spatial_lookup, &self.entities);
        let sensor_contacts = self.snapshot();
        let sensor_truth = TruthContacts::new(&sensor_contacts.entities, &nearby);
        self.run_phase(|entity| entity.step_sensors(time, sensor_truth))?;
        Ok(nearby)
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

    /// Estimation shares the delivery task: readings are delivered and belief
    /// updated per entity, with no extra phase.
    fn estimate(&mut self, time: StepTime, contacts: &[EntitySnapshot]) -> Result<()> {
        self.perfect_contacts = perfect_contacts(contacts, time.time_s + time.dt_s);
        let perfect = self.perfect_contacts.clone();
        self.run_phase(|entity| entity.estimate(time, &perfect))
    }

    fn score(
        &mut self,
        time: StepTime,
        contacts: &[EntitySnapshot],
        nearby: &SpatialIndex,
    ) -> Result<()> {
        self.world_stop |= self.world.metrics(
            time,
            TruthContacts::new(contacts, nearby),
            &self.entity_teams,
            self.terrain.as_ref(),
        )?;
        Ok(())
    }

    /// Removal happens after networks and metrics so final-tick publications
    /// are delivered.
    fn remove_dead(&mut self, time: StepTime) -> Result<()> {
        let mut close_error = None;
        for entity in self.entities.iter_mut().filter(|entity| entity.health <= 0) {
            if let Err(error) = entity.close(time) {
                close_error.get_or_insert(error);
            }
        }
        self.entities.retain(|entity| entity.health > 0);
        match close_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn advance_time(&mut self) {
        self.step_count += 1;
        self.tick = self.step_count as i64;
        // Time is counted, not accumulated: repeated addition of dt drifts in
        // the last digits (0.1 added 299 times is 29.900000000000155).
        self.time_s = self.config.run.start_s + self.step_count as f64 * self.config.run.dt_s;
        self.termination = self.reached_end_condition();
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
        self.close_plugins()?;
        Ok(self.snapshot())
    }

    fn close_plugins(&mut self) -> Result<()> {
        let time = StepTime {
            time_s: self.time_s,
            dt_s: self.config.run.dt_s,
            tick: self.tick,
        };
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
            .collision_exists(&self.entities, position_world_m)
    }

    fn apply_interactions(&mut self, nearby: &mut SpatialIndex) -> Result<()> {
        let time = StepTime {
            time_s: self.time_s,
            dt_s: self.config.run.dt_s,
            tick: self.tick,
        };
        // Forces applied last time acted in the motion phase just run.
        for entity in &mut self.entities {
            entity.clear_applied_forces();
        }
        self.world_stop |=
            self.world
                .interactions(time, &mut self.entities, nearby, &mut self.events)?;
        self.messages.time_s = self.time_s;
        for entity in self
            .entities
            .iter_mut()
            .filter(|entity| entity.health <= 0 && entity.is_active())
        {
            entity.deactivate();
            let event = Event {
                time_s: self.time_s,
                kind: EventKind::EntityRemoved,
                entity_ids: vec![entity.id],
            };
            self.messages
                .publish_engine_event("GlobalNetwork", event.kind.topic(), event.clone());
            self.events.push(event);
        }
        Ok(())
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

/// The perfect contact measurement: every entity not marked for removal.
fn perfect_contacts(snapshot: &[EntitySnapshot], sampled_at_s: f64) -> ContactList {
    let mut contacts = Vec::with_capacity(snapshot.len());
    contacts.extend(
        snapshot
            .iter()
            .filter(|entity| entity.active)
            .map(|entity| Contact::measured(entity, sampled_at_s)),
    );
    contacts.into()
}

impl Drop for Simulation {
    fn drop(&mut self) {
        let _ = self.close_plugins();
    }
}

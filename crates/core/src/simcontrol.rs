//! Fixed-step orchestration with explicit phase boundaries.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use serde::Serialize;

use crate::{
    common::random::LegacyRng,
    entity::{Entity, EntityDefinition, EntitySnapshot},
    plugin::{Messages, MetricReport, PluginRegistry, StepTime},
    scenario::{EndConditions, ResolvedScenario, ScenarioConfig},
    simcontrol::scheduler::Scheduler,
    simcontrol::world_plugins::WorldPlugins,
};

pub(crate) mod generation;
mod scheduler;
mod summary;
pub(crate) mod world_plugins;

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
    AllEntitiesDead,
    OneTeamRemaining,
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
    termination: Option<TerminationReason>,
    scheduler: Scheduler,
    rng: LegacyRng,
    generators: Vec<Generator>,
    world: WorldPlugins,
    messages: Messages,
    entity_teams: BTreeMap<i32, i32>,
    world_stop: bool,
    used_ids: BTreeSet<i32>,
    end_conditions: EndConditions,
    finished: bool,
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

        let world = world.instantiate(config.run.seed);
        let mut simulation = Self {
            time_s: config.run.start_s - config.run.dt_s,
            rng: LegacyRng::new(config.run.seed),
            config,
            definitions,
            entities: Vec::new(),
            events: Vec::new(),
            step_count: 0,
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
        };

        simulation.world.initialize(
            StepTime {
                time_s: simulation.config.run.start_s,
                dt_s: simulation.config.run.dt_s,
            },
            &[],
            &simulation.entity_teams,
        )?;
        // Legacy missions can create entities one step before the start time.
        simulation.generate_entities()?;
        simulation.apply_interactions()?;
        simulation.time_s = simulation.config.run.start_s;
        Ok(simulation)
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

    fn snapshot(&self) -> SimulationFrame {
        let mut entities: Vec<_> = self.entities.iter().map(Entity::snapshot).collect();
        entities.sort_by_key(|entity| entity.id);
        SimulationFrame {
            // SCRIMMAGE labels pre-step state with the following step's timestamp.
            time_s: self.time_s + self.config.run.dt_s,
            entities,
        }
    }

    /// Returns pre-step state, then advances the simulation. The final call emits terminal state.
    pub fn step(&mut self) -> Result<Option<SimulationFrame>> {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.step_inner()))
            .unwrap_or_else(|_| Err(anyhow::anyhow!("simulation plugin panicked")));
        if result.is_err() {
            self.termination = Some(TerminationReason::PluginFailure);
            self.finished = true;
            let _ = self.close_plugins();
        }
        result
    }

    fn step_inner(&mut self) -> Result<Option<SimulationFrame>> {
        if self.finished {
            return Ok(None);
        }
        if self.termination.is_some() {
            return Ok(Some(self.finish()?));
        }

        self.generate_entities()?;
        let frame = self.snapshot();
        let dt_s = self.config.run.dt_s;
        let time_s = self.time_s;
        self.run_phase(|entity| entity.step_autonomy(StepTime { time_s, dt_s }, &frame.entities))?;

        // Match the reference: all controller substeps precede all motion substeps.
        let motion_dt_s = dt_s / self.config.run.motion_multiplier as f64;
        let mut substep_time_s = time_s;
        for _ in 0..self.config.run.motion_multiplier {
            let time = StepTime {
                time_s: substep_time_s,
                dt_s: motion_dt_s,
            };
            self.run_phase(|entity| entity.step_controller(time, &frame.entities))?;
            substep_time_s += motion_dt_s;
        }
        substep_time_s = time_s;
        for _ in 0..self.config.run.motion_multiplier {
            let time = StepTime {
                time_s: substep_time_s,
                dt_s: motion_dt_s,
            };
            self.run_phase(|entity| entity.step_motion(time))?;
            substep_time_s += motion_dt_s;
        }

        let sensor_contacts = self.snapshot();
        self.run_phase(|entity| {
            entity.step_sensors(StepTime { time_s, dt_s }, &sensor_contacts.entities)
        })?;
        self.apply_interactions()?;
        let time = StepTime { time_s, dt_s };
        let contacts = self.snapshot().entities;
        self.world_stop |=
            self.world
                .networks(time, &contacts, &mut self.entities, &mut self.messages)?;
        self.run_phase(Entity::deliver_observations)?;
        self.world_stop |= self.world.metrics(time, &contacts, &self.entity_teams)?;
        // Retain a stop request even when its entity is removed in this same tick.
        self.world_stop |= self.entities.iter().any(Entity::stop_requested);
        // Removal happens after networks and metrics so final-tick publications are delivered.
        let mut close_error = None;
        for entity in self.entities.iter_mut().filter(|entity| entity.health <= 0) {
            if let Err(error) = entity.close(time) {
                close_error.get_or_insert(error);
            }
        }
        self.entities.retain(|entity| entity.health > 0);
        if let Some(error) = close_error {
            return Err(error);
        }
        self.step_count += 1;
        // Matches C++ for now, though we'd prefer start + step_count * dt: repeated
        // addition accumulates floating-point drift (e.g. 29.900000000000155).
        self.time_s += dt_s;
        self.termination = self.reached_end_condition();
        Ok(Some(frame))
    }

    fn reached_end_condition(&self) -> Option<TerminationReason> {
        if self.world_stop || self.entities.iter().any(Entity::stop_requested) {
            return Some(TerminationReason::PluginRequestedStop);
        }
        if self.end_conditions.time
            && self.time_s > self.config.run.end_s - self.config.run.dt_s / 2.0
        {
            return Some(TerminationReason::TimeLimit);
        }
        if self.entities.is_empty()
            && (self.end_conditions.all_dead || self.end_conditions.one_team)
        {
            return Some(TerminationReason::AllEntitiesDead);
        }
        if self.end_conditions.one_team {
            let remaining_teams: BTreeSet<_> =
                self.entities.iter().map(|entity| entity.team_id).collect();
            if remaining_teams.len() == 1 {
                return Some(TerminationReason::OneTeamRemaining);
            }
        }
        None
    }

    fn finish(&mut self) -> Result<SimulationFrame> {
        self.finished = true;
        // Matches C++ for now, though we'd prefer a monotonic clock: rewinding repeats the
        // final timestamp in legacy logs, but terminal events, plugin close, and the
        // metrics report also see this rewound time, not only the frame writer.
        self.time_s -= self.config.run.dt_s;
        for entity in &self.entities {
            self.events.push(Event {
                time_s: self.time_s,
                kind: EventKind::EntityPresentAtEnd,
                entity_ids: vec![entity.id],
            });
        }
        self.close_plugins()?;
        Ok(self.snapshot())
    }

    fn close_plugins(&mut self) -> Result<()> {
        let time = StepTime {
            time_s: self.time_s,
            dt_s: self.config.run.dt_s,
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

    fn apply_interactions(&mut self) -> Result<()> {
        let time = StepTime {
            time_s: self.time_s,
            dt_s: self.config.run.dt_s,
        };
        self.world_stop |= self
            .world
            .interactions(time, &mut self.entities, &mut self.events)?;
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
                .publish("GlobalNetwork", event.kind.topic(), event.clone())?;
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

impl Drop for Simulation {
    fn drop(&mut self) {
        let _ = self.close_plugins();
    }
}

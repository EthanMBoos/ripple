//! Entity-owned truth and the autonomy/controller/motion execution chain.

pub(crate) mod compiled_stack;
pub(crate) mod plugin_stack;
pub(crate) mod proof;

use anyhow::{Context, Result};

use serde::Serialize;
use vstd::prelude::*;

use crate::{
    common::{Terrain, TruthContacts},
    drawing::PluginDrawings,
    entity::{compiled_stack::CompiledStack, plugin_stack::PluginStack},
    math::{EulerAngles, GaussianState, KinematicState, Quaternion, Vec3},
    plugin::estimator::{PerfectContacts, Track},
    plugin::{EntityInfo, PluginRegistry, Readings, StepTime},
    pubsub::routing::Mailbox,
    scenario::EntityGroupConfig,
};

verus! {

#[verifier::external_derive(Serialize)]
#[derive(Clone, Copy, Debug, Serialize)]
pub struct EntitySnapshot {
    pub id: i32,
    /// Index of the mission entity group this entity came from.
    pub group_index: i32,
    pub team_id: i32,
    pub truth: KinematicState,
    pub active: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct SpawnConfig {
    pub position_world_m: Vec3,
    pub position_stddev_world_m: Vec3,
    pub heading_world_deg: f64,
    pub heading_stddev_deg: f64,
    pub requested_id: Option<i32>,
}

/// Validated entity configuration; runtime construction never reads string parameters.
pub(crate) struct EntityDefinition {
    pub spawn: SpawnConfig,
    pub(crate) team_id: i32,
    pub(crate) initial_health: i32,
    pub(crate) initial_velocity_world_mps: Vec3,
    pub(crate) initial_attitude: EulerAngles,
    pub(crate) plugins: CompiledStack,
}

#[verifier::external]
impl EntityDefinition {
    pub(crate) fn parse(config: &EntityGroupConfig, registry: &PluginRegistry) -> Result<Self> {
        Ok(Self {
            spawn: SpawnConfig {
                position_world_m: config.position_m,
                position_stddev_world_m: config.position_stddev_m,
                heading_world_deg: config.heading_deg,
                heading_stddev_deg: config.heading_stddev_deg,
                requested_id: config.id,
            },
            team_id: config.team,
            initial_health: config.health,
            initial_velocity_world_mps: config.velocity_mps,
            initial_attitude: EulerAngles {
                roll_world_from_body_rad: crate::math::deg_to_rad(config.roll_deg),
                pitch_world_from_body_rad: crate::math::deg_to_rad(config.pitch_deg),
                yaw_world_from_body_rad: 0.0,
            },
            plugins: CompiledStack::compile(config, registry)?,
        })
    }

    /// The sensor chosen for each localization and tracking input.
    pub(crate) fn estimator_inputs(&self) -> [std::collections::BTreeMap<String, String>; 2] {
        self.plugins.estimator_inputs()
    }
}

/// One entity: its truth, health, and plugins. The fields are crate-private
/// so a proof can state what a phase leaves untouched, and plugins reach it
/// only through its methods.
pub struct Entity {
    pub(crate) id: i32,
    pub(crate) group_index: i32,
    pub(crate) team_id: i32,
    pub(crate) truth: KinematicState,
    pub(crate) health: i32,
    /// The time and tick the entity was created, with a zero dt.
    pub(crate) born: StepTime,
    pub(crate) active: bool,
    /// Estimators and decision plugins have been initialized.
    pub(crate) started: bool,
    /// Forces from the last interaction phase, for the next motion phase.
    pub(crate) applied_forces_world_n: Vec<Vec3>,
    pub(crate) plugins: PluginStack,
}

/// One entity's share of a simulation phase, with what the phase hands it.
/// `Simulation::step_inner` runs each over the entities in the order of the
/// book's phase table.
#[derive(Clone, Copy)]
pub(crate) enum Phase<'a> {
    Autonomy(StepTime),
    /// After the first motion substep, a perfect own-state belief follows
    /// truth first, so the controllers see the state the last substep wrote.
    Controllers {
        time: StepTime,
        after_motion: bool,
    },
    Motion {
        time: StepTime,
        terrain: Option<&'a Terrain>,
    },
    Sensors {
        time: StepTime,
        truth: TruthContacts<'a>,
    },
    Estimate {
        time: StepTime,
        perfect_contacts: &'a PerfectContacts,
    },
}

#[verifier::external]
impl Entity {
    // Each argument is a separate part of the new entity's starting setup.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        id: i32,
        group_index: i32,
        definition: &EntityDefinition,
        position_world_m: Vec3,
        heading_world_deg: f64,
        born: StepTime,
        seed: u32,
        terrain: Option<&Terrain>,
    ) -> Result<Self> {
        let mut attitude = definition.initial_attitude;
        attitude.yaw_world_from_body_rad = crate::math::deg_to_rad(heading_world_deg);

        let mut truth = KinematicState {
            position_world_m,
            orientation_world_from_body: Quaternion::from_euler(attitude),
            velocity_world_mps: definition.initial_velocity_world_mps,
            ..KinematicState::default()
        };
        let mut plugins = definition.plugins.instantiate(id, seed);
        let time = born;
        let initialized = plugins.initialize(
            EntityInfo {
                id,
                team_id: definition.team_id,
                group_index,
            },
            time,
            &mut truth,
            terrain,
        );
        if let Err(error) = initialized {
            // The entity has not entered the simulation yet; its plugins still need cleanup.
            let _ = plugins.close(time);
            return Err(error).with_context(|| format!("initialize entity {id}"));
        }

        Ok(Self {
            id,
            group_index,
            team_id: definition.team_id,
            truth,
            health: definition.initial_health,
            born,
            active: true,
            started: false,
            applied_forces_world_n: Vec::new(),
            plugins,
        })
    }

    pub fn id(&self) -> i32 {
        self.id
    }
    pub fn team_id(&self) -> i32 {
        self.team_id
    }
    /// Index of the mission entity group this entity came from.
    pub fn group_index(&self) -> i32 {
        self.group_index
    }
    pub fn truth(&self) -> &KinematicState {
        &self.truth
    }
    /// Own-state belief, the estimate autonomy and controllers use: a mean
    /// and the covariance of its error.
    pub fn belief(&self) -> &GaussianState {
        self.plugins.belief()
    }
    /// The tracks this entity keeps on others (its tracker's list, which may
    /// include itself).
    pub fn tracks(&self) -> &[Track] {
        self.plugins.tracks()
    }
    /// World interactions may change physical state after the motion phase.
    pub fn truth_mut(&mut self) -> &mut KinematicState {
        &mut self.truth
    }
    /// An interaction's force on this entity, acting during the next motion
    /// phase (see `MotionContext::applied_forces_world_n`).
    pub fn apply_force(&mut self, force_world_n: Vec3) {
        self.applied_forces_world_n.push(force_world_n);
    }
    pub fn health(&self) -> i32 {
        self.health
    }
    pub fn set_health(&mut self, health: i32) {
        self.health = health
    }
    pub(crate) fn mailboxes<'a>(&'a mut self, mailboxes: &mut Vec<Mailbox<'a>>) {
        self.plugins.mailboxes(mailboxes);
    }
    pub fn is_active(&self) -> bool {
        self.active
    }
    pub fn born_s(&self) -> f64 {
        self.born.time_s
    }

    pub(crate) fn plugin_outputs(&self) -> Vec<(&str, &str, f64)> {
        self.plugins.outputs()
    }
    /// What this entity's plugins draw at `time_s`: the fixed part by
    /// reference and the changing part asked now.
    pub(crate) fn drawings(&self, time_s: f64) -> Vec<PluginDrawings> {
        self.plugins.drawings(self.id, time_s)
    }
    /// Delivered sensor readings, by sensor instance name. Estimators read these;
    /// evaluation and tests may inspect them.
    pub fn readings(&self) -> &Readings {
        self.plugins.readings()
    }
    pub(crate) fn pending_measurements(&self) -> Result<Vec<plugin_stack::Measurement<'_>>> {
        self.plugins.pending_measurements()
    }

    pub(crate) fn snapshot(&self) -> EntitySnapshot {
        EntitySnapshot {
            id: self.id,
            group_index: self.group_index,
            team_id: self.team_id,
            truth: self.truth,
            active: self.active,
        }
    }
}

impl Entity {
    /// Starts the entity's plugins once, at its birth time: the estimators
    /// first, so a belief exists, then the decision plugins, both from the
    /// startup readings and the population the entity joins. An entity
    /// already started is left alone.
    pub(crate) fn start(&mut self, perfect_contacts: &PerfectContacts) -> (r: Result<()>)
        ensures proof::started(old(self), final(self), perfect_contacts, r is Ok)
    {
        if self.started {
            return Ok(());
        }
        self.started = true;
        let time = self.born;
        let info = self.info();
        // `map_err` rather than `with_context`: Verus can give `with_context`
        // on a `Result` no contract, because `anyhow` bounds it by a private trait.
        self.plugins
            .start(info, time, &self.truth, perfect_contacts)
            .map_err(|error| error.context(format!("initialize entity {}", self.id)))
    }

    fn info(&self) -> (r: EntityInfo)
        ensures r == proof::info_of(self)
    {
        EntityInfo {
            id: self.id,
            team_id: self.team_id,
            group_index: self.group_index,
        }
    }

    /// The entity's share of one phase; see `Phase`.
    pub(crate) fn step_phase(&mut self, phase: Phase<'_>) -> (r: Result<()>)
        ensures proof::phase_stepped(old(self), final(self), phase, r is Ok)
    {
        match phase {
            Phase::Autonomy(time) => self.step_autonomy(time),
            Phase::Controllers { time, after_motion } => self.step_controller(time, after_motion),
            Phase::Motion { time, terrain } => self.step_motion(time, terrain),
            Phase::Sensors { time, truth } => self.step_sensors(time, truth),
            Phase::Estimate {
                time,
                perfect_contacts,
            } => self.estimate(time, perfect_contacts),
        }
    }

    pub(crate) fn step_autonomy(&mut self, time: StepTime) -> (r: Result<()>)
        ensures proof::autonomy_stepped(old(self), final(self), time, r is Ok)
    {
        self.plugins.autonomy(self.info(), time)
    }

    pub(crate) fn step_controller(&mut self, time: StepTime, after_motion: bool) -> (r: Result<()>)
        ensures proof::controllers_stepped(old(self), final(self), time, after_motion, r is Ok)
    {
        if after_motion {
            self.plugins.follow_perfect_state(&self.truth);
        }
        self.plugins.controllers(self.info(), time)
    }

    pub(crate) fn step_motion(&mut self, time: StepTime, terrain: Option<&Terrain>) -> (r: Result<()>)
        ensures proof::motion_stepped(old(self), final(self), time, terrain, r is Ok)
    {
        self.plugins.motion(
            self.info(),
            time,
            &mut self.truth,
            &self.applied_forces_world_n,
            terrain,
        )
    }

    pub(crate) fn step_sensors(&mut self, time: StepTime, contacts: TruthContacts<'_>) -> (r: Result<()>)
        ensures proof::sensors_stepped(old(self), final(self), time, contacts, r is Ok)
    {
        self.plugins
            .sensors(self.info(), time, &self.truth, contacts)
    }

    pub(crate) fn estimate(&mut self, time: StepTime, perfect_contacts: &PerfectContacts) -> (r: Result<()>)
        ensures proof::estimated(old(self), final(self), time, perfect_contacts, r is Ok)
    {
        self.plugins
            .estimate(self.info(), time, &self.truth, perfect_contacts)
    }

    /// Forces applied in the last interactions phase have acted in the motion
    /// phase just run.
    pub(crate) fn clear_applied_forces(&mut self)
        ensures proof::forces_cleared(old(self), final(self))
    {
        self.applied_forces_world_n.clear();
    }

    /// The entity leaves play: no phase steps it again, and it is removed at
    /// the end of the tick.
    pub(crate) fn deactivate(&mut self)
        ensures proof::deactivated(old(self), final(self))
    {
        self.active = false;
    }

    /// The entity has health left. One with none is closed and removed at the
    /// end of the tick.
    pub(crate) fn is_alive(&self) -> (alive: bool)
        ensures alive == proof::is_alive(self)
    {
        self.health > 0
    }

    pub(crate) fn close(&mut self, time: StepTime) -> (r: Result<()>)
        ensures proof::closed(old(self), final(self), time, r is Ok)
    {
        self.plugins.close(time)
    }
}

} // verus!

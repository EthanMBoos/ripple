//! Entity-owned truth and the autonomy/controller/motion execution chain.

pub(crate) mod plugin_stack;

use anyhow::{Context, Result};

use serde::Serialize;

use crate::{
    common::Terrain,
    entity::plugin_stack::{CompiledStack, PluginStack},
    math::{EulerAngles, KinematicState, Quaternion, Vec3},
    plugin::estimator::{Contact, ContactList},
    plugin::{EntityInfo, PluginRegistry, Readings, StepTime},
    scenario::EntityGroupConfig,
};

#[derive(Clone, Debug, Serialize)]
pub struct EntitySnapshot {
    pub id: i32,
    pub sub_swarm_id: i32,
    pub team_id: i32,
    pub truth: KinematicState,
    pub active: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct SpawnConfig {
    pub position_world_m: Vec3,
    pub position_variance_world_m2: Vec3,
    pub heading_world_deg: f64,
    pub heading_variance_deg2: f64,
    pub randomize_every_spawn: bool,
    pub requested_id: Option<i32>,
}

/// Validated entity configuration; runtime construction never reads string parameters.
pub(crate) struct EntityDefinition {
    pub spawn: SpawnConfig,
    team_id: i32,
    initial_health: i32,
    initial_velocity_world_mps: Vec3,
    initial_attitude: EulerAngles,
    plugins: CompiledStack,
}

impl EntityDefinition {
    pub(crate) fn parse(config: &EntityGroupConfig, registry: &PluginRegistry) -> Result<Self> {
        Ok(Self {
            spawn: SpawnConfig {
                position_world_m: config.position_m,
                position_variance_world_m2: config.position_variance_m2,
                heading_world_deg: config.heading_deg,
                heading_variance_deg2: config.heading_variance_deg2,
                randomize_every_spawn: config.randomize_every_spawn,
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

pub struct Entity {
    pub(crate) id: i32,
    sub_swarm_id: i32,
    pub(crate) team_id: i32,
    pub(crate) truth: KinematicState,
    pub(crate) health: i32,
    /// The time and tick the entity was created, with a zero dt.
    born: StepTime,
    active: bool,
    /// Estimators and decision plugins have been initialized.
    started: bool,
    /// Forces from the last interaction phase, for the next motion phase.
    applied_forces_world_n: Vec<Vec3>,
    plugins: PluginStack,
}

impl Entity {
    // Each argument is a separate part of the new entity's starting setup.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        id: i32,
        sub_swarm_id: i32,
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
        let initialized = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            plugins.initialize(
                EntityInfo {
                    id,
                    team_id: definition.team_id,
                    sub_swarm_id,
                },
                time,
                &mut truth,
                terrain,
            )
        }))
        .unwrap_or_else(|_| Err(anyhow::anyhow!("entity plugin initialization panicked")));
        if let Err(error) = initialized {
            // The entity has not entered the simulation yet; its plugins still need cleanup.
            let _ = plugins.close(time);
            return Err(error).with_context(|| format!("initialize entity {id}"));
        }

        Ok(Self {
            id,
            sub_swarm_id,
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
    pub fn truth(&self) -> &KinematicState {
        &self.truth
    }
    /// Own-state belief, the estimate autonomy and controllers use.
    pub fn belief(&self) -> &KinematicState {
        self.plugins.belief()
    }
    /// The contacts this entity believes in (its tracker's list, which may include itself).
    pub fn contacts_belief(&self) -> &[Contact] {
        self.plugins.contacts()
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
    pub(crate) fn clear_applied_forces(&mut self) {
        self.applied_forces_world_n.clear();
    }
    pub fn health(&self) -> i32 {
        self.health
    }
    pub fn set_health(&mut self, health: i32) {
        self.health = health
    }
    pub(crate) fn mailboxes<'a>(
        &'a mut self,
        mailboxes: &mut Vec<crate::pubsub::messages::Mailbox<'a>>,
    ) {
        self.plugins.mailboxes(mailboxes);
    }
    pub fn is_active(&self) -> bool {
        self.active
    }
    pub(crate) fn deactivate(&mut self) {
        self.active = false;
    }

    pub fn born_s(&self) -> f64 {
        self.born.time_s
    }

    fn info(&self) -> EntityInfo {
        EntityInfo {
            id: self.id,
            team_id: self.team_id,
            sub_swarm_id: self.sub_swarm_id,
        }
    }

    pub(crate) fn plugin_outputs(&self) -> Vec<(&str, &str, f64)> {
        self.plugins.outputs()
    }
    /// Delivered sensor readings, by sensor instance name. Estimators read these;
    /// evaluation and tests may inspect them.
    pub fn readings(&self) -> &Readings {
        self.plugins.readings()
    }
    pub(crate) fn pending_measurements(&self) -> Result<Vec<plugin_stack::Measurement<'_>>> {
        self.plugins.pending_measurements()
    }

    /// Estimators, then decision plugins, from the startup readings and the
    /// population; does nothing once started.
    pub(crate) fn start(&mut self, perfect_contacts: &ContactList) -> Result<()> {
        if self.started {
            return Ok(());
        }
        self.started = true;
        let time = self.born;
        let info = self.info();
        // As in `new`, a panicking plugin becomes an error rather than escaping
        // `Simulation::new` (a sweep then records one failed case).
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.plugins
                .start(info, time, &self.truth, perfect_contacts)
        }))
        .unwrap_or_else(|_| Err(anyhow::anyhow!("entity plugin initialization panicked")))
        .with_context(|| format!("initialize entity {}", self.id))
    }

    pub(crate) fn step_autonomy(&mut self, time: StepTime) -> Result<()> {
        self.plugins.autonomy(self.info(), time)
    }

    pub(crate) fn step_controller(&mut self, time: StepTime) -> Result<()> {
        self.plugins.controllers(self.info(), time)
    }

    pub(crate) fn step_motion(&mut self, time: StepTime, terrain: Option<&Terrain>) -> Result<()> {
        self.plugins.motion(
            self.info(),
            time,
            &mut self.truth,
            &self.applied_forces_world_n,
            terrain,
        )
    }

    /// Between motion substeps; see `PluginStack::follow_perfect_state`.
    pub(crate) fn follow_perfect_state(&mut self) {
        self.plugins.follow_perfect_state(&self.truth);
    }

    pub(crate) fn step_sensors(
        &mut self,
        time: StepTime,
        contacts: crate::common::TruthContacts<'_>,
    ) -> Result<()> {
        self.plugins
            .sensors(self.info(), time, &self.truth, contacts)
    }

    pub(crate) fn close(&mut self, time: StepTime) -> Result<()> {
        self.plugins.close(time)
    }

    pub(crate) fn estimate(
        &mut self,
        time: StepTime,
        perfect_contacts: &ContactList,
    ) -> Result<()> {
        self.plugins
            .estimate(self.info(), time, &self.truth, perfect_contacts)
    }

    pub(crate) fn stop_requested(&self) -> bool {
        self.plugins.stop_requested
    }

    pub(crate) fn snapshot(&self) -> EntitySnapshot {
        EntitySnapshot {
            id: self.id,
            sub_swarm_id: self.sub_swarm_id,
            team_id: self.team_id,
            truth: self.truth.clone(),
            active: self.active,
        }
    }
}

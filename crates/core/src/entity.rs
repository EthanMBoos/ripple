//! Entity-owned truth and the autonomy/controller/motion execution chain.

pub(crate) mod plugin_stack;

use anyhow::{Context, Result, ensure};

use serde::Serialize;

use crate::{
    entity::plugin_stack::{CompiledStack, PluginStack},
    math::{EulerAngles, KinematicState, Quaternion, Vec3},
    parse::{EntityConfig, boolean, number, vector},
    plugin::{EntityInfo, Observations, PluginRegistry, StepTime},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum EntityKind {
    Aircraft,
    Quadrotor,
    Sphere,
    Mesh,
    Unknown,
}

#[derive(Clone, Debug, Serialize)]
pub struct EntitySnapshot {
    pub id: i32,
    pub sub_swarm_id: i32,
    pub team_id: i32,
    pub truth: KinematicState,
    pub kind: EntityKind,
    pub active: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct SpawnConfig {
    pub position_world_m: Vec3,
    pub position_variance_world_m2: Vec3,
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
    color: [u8; 3],
    visual_model: String,
    contact_type: EntityKind,
    plugins: CompiledStack,
}

impl EntityDefinition {
    pub(crate) fn effective_params(&self) -> serde_json::Value {
        self.plugins.effective_params()
    }

    pub(crate) fn parse(config: &EntityConfig, registry: &PluginRegistry) -> Result<Self> {
        let params = &config.params;
        let position_world_m = Vec3::new(
            number(params, "x", 0.0)?,
            number(params, "y", 0.0)?,
            number(params, "z", 0.0)?,
        );
        let position_variance_world_m2 = Vec3::new(
            number(params, "variance_x", 100.0)?,
            number(params, "variance_y", 100.0)?,
            number(params, "variance_z", 0.0)?,
        );
        let heading_variance_deg2 = number(params, "variance_heading", 0.0)?;
        ensure!(
            position_variance_world_m2
                .iter()
                .all(|variance| *variance >= 0.0)
                && heading_variance_deg2 >= 0.0,
            "spawn variances must be nonnegative"
        );

        let mut initial_velocity_world_mps = Vec3::new(
            number(params, "vx", 0.0)?,
            number(params, "vy", 0.0)?,
            number(params, "vz", 0.0)?,
        );
        let initial_speed_mps = number(params, "speed", 0.0)?;
        if initial_speed_mps > 0.0 && initial_velocity_world_mps == Vec3::zeros() {
            // Matches C++ for now, though we don't consider it correct: scalar speed is
            // applied along world X before the initial attitude, so an aircraft facing north
            // can start moving east. Motion models that rebuild velocity hide this.
            initial_velocity_world_mps = Vec3::new(initial_speed_mps, 0.0, 0.0);
        }

        let visual_model = params
            .get("visual_model")
            .cloned()
            .unwrap_or_else(|| "sphere".into());
        let contact_type = match visual_model.to_lowercase().as_str() {
            "sphere" => EntityKind::Sphere,
            "aircraft" => EntityKind::Aircraft,
            "quadrotor" => EntityKind::Quadrotor,
            _ => EntityKind::Mesh,
        };
        let color = vector(params, "color", [255.0, 255.0, 255.0])?
            .map(|channel| channel.clamp(0.0, 255.0) as u8);
        let team_id = params
            .get("team_id")
            .map_or(Ok(-1), |value| value.parse())
            .context("invalid team_id")?;
        let initial_health = params
            .get("health")
            .map_or(Ok(1), |value| value.parse())
            .context("invalid health")?;
        let requested_id = params
            .get("id")
            .map(|value| value.parse())
            .transpose()
            .context("invalid entity id")?;

        Ok(Self {
            spawn: SpawnConfig {
                position_world_m,
                position_variance_world_m2,
                heading_variance_deg2,
                randomize_every_spawn: boolean(params, "use_variance_all_ents", false)?,
                requested_id,
            },
            team_id,
            initial_health,
            initial_velocity_world_mps,
            initial_attitude: EulerAngles {
                roll_world_from_body_rad: number(params, "roll", 0.0)?.to_radians(),
                pitch_world_from_body_rad: number(params, "pitch", 0.0)?.to_radians(),
                yaw_world_from_body_rad: 0.0,
            },
            color,
            visual_model,
            contact_type,
            plugins: CompiledStack::compile(config, registry)?,
        })
    }
}

pub struct Entity {
    pub(crate) id: i32,
    sub_swarm_id: i32,
    pub(crate) team_id: i32,
    pub(crate) truth: KinematicState,
    pub(crate) health: i32,
    color: [u8; 3],
    visual_model: String,
    contact_type: EntityKind,
    born_s: f64,
    active: bool,
    plugins: PluginStack,
}

impl Entity {
    pub(crate) fn new(
        id: i32,
        sub_swarm_id: i32,
        definition: &EntityDefinition,
        position_world_m: Vec3,
        heading_world_deg: f64,
        born_s: f64,
        seed: u32,
    ) -> Result<Self> {
        let mut attitude = definition.initial_attitude;
        attitude.yaw_world_from_body_rad = heading_world_deg.to_radians();

        let mut truth = KinematicState {
            position_world_m,
            orientation_world_from_body: Quaternion::from_euler(attitude),
            velocity_world_mps: definition.initial_velocity_world_mps,
            ..KinematicState::default()
        };
        let mut plugins = definition.plugins.instantiate(id, seed);
        let time = StepTime {
            time_s: born_s,
            dt_s: 0.0,
        };
        let initialized = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            plugins.initialize(
                EntityInfo {
                    id,
                    team_id: definition.team_id,
                    sub_swarm_id,
                },
                time,
                &mut truth,
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
            color: definition.color,
            visual_model: definition.visual_model.clone(),
            contact_type: definition.contact_type,
            born_s,
            active: true,
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
    /// The estimate used by autonomy/controllers; ideal truth until detached by a sensor.
    pub fn belief(&self) -> &KinematicState {
        self.plugins.belief(&self.truth)
    }
    /// World interactions may change physical state after the motion phase.
    pub fn truth_mut(&mut self) -> &mut KinematicState {
        &mut self.truth
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
        self.plugins.mailboxes(self.id, mailboxes);
    }
    pub fn color(&self) -> [u8; 3] {
        self.color
    }
    pub fn visual_model(&self) -> &str {
        &self.visual_model
    }
    pub fn is_active(&self) -> bool {
        self.active
    }
    pub(crate) fn deactivate(&mut self) {
        self.active = false;
    }

    pub fn born_s(&self) -> f64 {
        self.born_s
    }

    fn info(&self) -> EntityInfo {
        EntityInfo {
            id: self.id,
            team_id: self.team_id,
            sub_swarm_id: self.sub_swarm_id,
        }
    }

    pub fn observations(&self) -> &Observations {
        self.plugins.observations()
    }

    pub(crate) fn step_autonomy(
        &mut self,
        time: StepTime,
        contacts: &[EntitySnapshot],
    ) -> Result<()> {
        self.plugins
            .autonomy(self.info(), time, &self.truth, contacts)
    }

    pub(crate) fn step_controller(
        &mut self,
        time: StepTime,
        contacts: &[EntitySnapshot],
    ) -> Result<()> {
        self.plugins
            .controllers(self.info(), time, &self.truth, contacts)
    }

    pub(crate) fn step_motion(&mut self, time: StepTime) -> Result<()> {
        self.plugins.motion(self.info(), time, &mut self.truth)
    }

    pub(crate) fn step_sensors(
        &mut self,
        time: StepTime,
        contacts: &[EntitySnapshot],
    ) -> Result<()> {
        self.plugins
            .sensors(self.info(), time, &self.truth, contacts)
    }

    pub(crate) fn close(&mut self, time: StepTime) -> Result<()> {
        self.plugins.close(time)
    }

    pub(crate) fn deliver_observations(&mut self) -> Result<()> {
        self.plugins.deliver()
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
            kind: self.contact_type,
            active: self.active,
        }
    }
}

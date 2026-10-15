//! Plugin registration: typed factories by category and the built-in list.
use super::{
    Autonomy, Controller, Input, Interaction, Localization, Metrics, MotionModel, Network,
    PluginParams, Ports, ReadingKind, Sensor, Tracking, autonomy, controller, estimator,
    interaction, metrics, motion, network, sensor,
};
use crate::entity::plugin_stack::{
    AgentBehavior, AutonomyAdapter, ControllerAdapter, LocalizationAdapter, LocalizationBehavior,
    MotionBehavior, SensorBehavior, TrackingAdapter, TrackingBehavior,
};
use crate::simulation::world_plugins::{InteractionBehavior, MetricsBehavior, NetworkBehavior};
use crate::{common::Rate, scenario::PluginConfig};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

pub(crate) struct CompiledPlugin<T: ?Sized> {
    pub name: String,
    pub ports: Ports,
    pub rate: Rate,
    /// Estimators only: the readings they read.
    pub inputs: Vec<Input>,
    /// Sensors only: what their readings contain.
    pub reading_kind: ReadingKind,
    pub instantiate: Box<dyn Fn() -> Box<T> + Send + Sync>,
}
type Compiler<T> = Box<dyn Fn(&PluginParams<'_>) -> Result<CompiledPlugin<T>> + Send + Sync>;

/// One category's named constructors. The result keeps its category through spawning.
pub(crate) struct Catalog<T: ?Sized> {
    kind: &'static str,
    compilers: BTreeMap<String, Compiler<T>>,
}

impl<T: ?Sized> Catalog<T> {
    fn new(kind: &'static str) -> Self {
        Self {
            kind,
            compilers: BTreeMap::new(),
        }
    }

    fn insert(&mut self, name: &str, compiler: Compiler<T>) -> Result<()> {
        ensure!(!name.trim().is_empty(), "plugin name must not be empty");
        ensure!(
            !self.compilers.contains_key(name),
            "duplicate {} plugin '{name}'",
            self.kind
        );
        self.compilers.insert(name.to_owned(), compiler);
        Ok(())
    }

    pub(crate) fn compile(&self, source: &PluginConfig) -> Result<CompiledPlugin<T>> {
        source
            .validate()
            .with_context(|| format!("configure {} '{}'", self.kind, source.name))?;
        let compiler = self
            .compilers
            .get(&source.name)
            .with_context(|| format!("unregistered {} plugin '{}'", self.kind, source.name))?;
        let mut plugin = compiler(&PluginParams::new(source))
            .with_context(|| format!("configure {} '{}'", self.kind, source.name))?;
        plugin.name = source.name.clone();
        plugin
            .ports
            .validate()
            .with_context(|| format!("ports of '{}'", plugin.name))?;
        Ok(plugin)
    }
}

/// User projects register ordinary Rust types. No simulator source edits are required.
pub struct PluginRegistry {
    pub(crate) autonomies: Catalog<dyn AgentBehavior>,
    pub(crate) controllers: Catalog<dyn AgentBehavior>,
    pub(crate) motion: Catalog<dyn MotionBehavior>,
    pub(crate) sensors: Catalog<dyn SensorBehavior>,
    pub(crate) localization: Catalog<dyn LocalizationBehavior>,
    pub(crate) tracking: Catalog<dyn TrackingBehavior>,
    pub(crate) interactions: Catalog<dyn InteractionBehavior>,
    pub(crate) networks: Catalog<dyn NetworkBehavior>,
    pub(crate) metrics: Catalog<dyn MetricsBehavior>,
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self {
            autonomies: Catalog::new("Autonomy"),
            controllers: Catalog::new("Controller"),
            motion: Catalog::new("Motion"),
            sensors: Catalog::new("Sensor"),
            localization: Catalog::new("Localization"),
            tracking: Catalog::new("Tracking"),
            interactions: Catalog::new("Interaction"),
            networks: Catalog::new("Network"),
            metrics: Catalog::new("Metrics"),
        }
    }
}
impl PluginRegistry {
    pub fn with_builtins() -> Self {
        let mut registry = Self::default();
        register_builtins(&mut registry).expect("built-in names must be unique");
        registry
    }
    pub fn register_autonomy<T: Autonomy>(&mut self, name: &str) -> Result<()> {
        self.autonomies.insert(
            name,
            Box::new(|params| {
                let config = T::configure(params)?;
                Ok(CompiledPlugin {
                    name: String::new(),
                    ports: T::ports(&config),
                    rate: Rate::new(params.loop_rate_hz)?,
                    inputs: Vec::new(),
                    reading_kind: ReadingKind::Other,
                    instantiate: Box::new(move || Box::new(AutonomyAdapter(T::new(&config)))),
                })
            }),
        )
    }
    pub fn register_controller<T: Controller>(&mut self, name: &str) -> Result<()> {
        self.controllers.insert(
            name,
            Box::new(|params| {
                let config = T::configure(params)?;
                Ok(CompiledPlugin {
                    name: String::new(),
                    ports: T::ports(&config),
                    rate: Rate::new(params.loop_rate_hz)?,
                    inputs: Vec::new(),
                    reading_kind: ReadingKind::Other,
                    instantiate: Box::new(move || Box::new(ControllerAdapter(T::new(&config)))),
                })
            }),
        )
    }
    pub fn register_motion<T: MotionModel>(&mut self, name: &str) -> Result<()> {
        self.motion.insert(
            name,
            Box::new(|params| {
                let config = T::configure(params)?;
                Ok(CompiledPlugin {
                    name: String::new(),
                    ports: T::ports(&config),
                    rate: Rate::new(params.loop_rate_hz)?,
                    inputs: Vec::new(),
                    reading_kind: ReadingKind::Other,
                    instantiate: Box::new(move || Box::new(T::new(&config))),
                })
            }),
        )
    }
    pub fn register_sensor<T: Sensor>(&mut self, name: &str) -> Result<()> {
        self.sensors.insert(
            name,
            Box::new(|params| {
                let config = T::configure(params)?;
                Ok(CompiledPlugin {
                    name: String::new(),
                    ports: T::ports(&config),
                    rate: Rate::new(params.loop_rate_hz)?,
                    inputs: Vec::new(),
                    reading_kind: T::reading_kind(&config),
                    instantiate: Box::new(move || Box::new(T::new(&config))),
                })
            }),
        )
    }
    pub fn register_localization<T: Localization>(&mut self, name: &str) -> Result<()> {
        self.localization.insert(
            name,
            Box::new(|params| {
                let config = T::configure(params)?;
                Ok(CompiledPlugin {
                    name: String::new(),
                    ports: T::ports(&config),
                    rate: Rate::new(params.loop_rate_hz)?,
                    inputs: <T as Localization>::inputs(&config),
                    reading_kind: ReadingKind::Other,
                    instantiate: Box::new(move || Box::new(LocalizationAdapter(T::new(&config)))),
                })
            }),
        )
    }
    pub fn register_tracking<T: Tracking>(&mut self, name: &str) -> Result<()> {
        self.tracking.insert(
            name,
            Box::new(|params| {
                let config = T::configure(params)?;
                Ok(CompiledPlugin {
                    name: String::new(),
                    ports: T::ports(&config),
                    rate: Rate::new(params.loop_rate_hz)?,
                    inputs: <T as Tracking>::inputs(&config),
                    reading_kind: ReadingKind::Other,
                    instantiate: Box::new(move || Box::new(TrackingAdapter(T::new(&config)))),
                })
            }),
        )
    }
    pub fn register_interaction<T: Interaction>(&mut self, name: &str) -> Result<()> {
        self.interactions.insert(
            name,
            Box::new(|params| {
                let config = T::configure(params)?;
                Ok(CompiledPlugin {
                    name: String::new(),
                    ports: T::ports(&config),
                    rate: Rate::new(params.loop_rate_hz)?,
                    inputs: Vec::new(),
                    reading_kind: ReadingKind::Other,
                    instantiate: Box::new(move || Box::new(T::new(&config))),
                })
            }),
        )
    }
    pub fn register_network<T: Network>(&mut self, name: &str) -> Result<()> {
        self.networks.insert(
            name,
            Box::new(|params| {
                let config = T::configure(params)?;
                Ok(CompiledPlugin {
                    name: String::new(),
                    ports: T::ports(&config),
                    rate: Rate::new(params.loop_rate_hz)?,
                    inputs: Vec::new(),
                    reading_kind: ReadingKind::Other,
                    instantiate: Box::new(move || Box::new(T::new(&config))),
                })
            }),
        )
    }
    pub fn register_metrics<T: Metrics>(&mut self, name: &str) -> Result<()> {
        self.metrics.insert(
            name,
            Box::new(|params| {
                let config = T::configure(params)?;
                Ok(CompiledPlugin {
                    name: String::new(),
                    ports: T::ports(&config),
                    rate: Rate::new(params.loop_rate_hz)?,
                    inputs: Vec::new(),
                    reading_kind: ReadingKind::Other,
                    instantiate: Box::new(move || Box::new(T::new(&config))),
                })
            }),
        )
    }
}

fn register_builtins(registry: &mut PluginRegistry) -> Result<()> {
    registry.register_autonomy::<autonomy::Straight>("Straight")?;
    registry.register_autonomy::<autonomy::WaypointFollower>("WaypointFollower")?;
    registry.register_autonomy::<autonomy::AuctionAssign>("AuctionAssign")?;
    registry.register_autonomy::<autonomy::ConstantVelocity>("ConstantVelocity")?;
    registry.register_controller::<controller::SingleIntegratorControllerSimple>(
        "SingleIntegratorControllerSimple",
    )?;
    registry.register_motion::<motion::SingleIntegrator>("SingleIntegrator")?;
    registry.register_interaction::<interaction::WaypointBroadcast>("WaypointBroadcast")?;
    registry.register_controller::<controller::SimpleAircraftControllerPid>(
        "SimpleAircraftControllerPID",
    )?;
    registry.register_motion::<motion::SimpleAircraft>("SimpleAircraft")?;
    registry.register_motion::<motion::FixedWing6Dof>("FixedWing6DOF")?;
    registry.register_controller::<controller::AircraftPidController>("AircraftPIDController")?;
    registry.register_controller::<controller::AircraftVelocityController>(
        "AircraftVelocityController",
    )?;
    registry.register_motion::<motion::Multirotor>("Multirotor")?;
    registry.register_controller::<controller::MotorSpeeds>("MotorSpeeds")?;
    registry.register_controller::<controller::MultirotorVelocityController>(
        "MultirotorVelocityController",
    )?;
    registry.register_sensor::<sensor::NoisyPosition>("NoisyPosition")?;
    registry.register_sensor::<sensor::NoisyState>("NoisyState")?;
    registry.register_sensor::<sensor::NoisyContacts>("NoisyContacts")?;
    registry.register_localization::<estimator::PassThrough>("PassThrough")?;
    registry.register_tracking::<estimator::PassThrough>("PassThrough")?;
    registry.register_interaction::<interaction::SimpleCollision>("SimpleCollision")?;
    registry.register_interaction::<interaction::Boundary>("Boundary")?;
    registry.register_interaction::<interaction::GroundCollision>("GroundCollision")?;
    registry.register_interaction::<interaction::TerrainCollision>("TerrainCollision")?;
    registry.register_network::<network::LocalNetwork>("LocalNetwork")?;
    registry.register_network::<network::GlobalNetwork>("GlobalNetwork")?;
    registry.register_network::<network::SphereNetwork>("SphereNetwork")?;
    registry.register_metrics::<metrics::SimpleCollisionMetrics>("SimpleCollisionMetrics")?;
    registry.register_autonomy::<autonomy::ForceReplay>("ForceReplay")?;
    registry.register_motion::<motion::MpeParticle>("MpeParticle")?;
    registry.register_interaction::<interaction::MpeContact>("MpeContact")?;
    registry.register_metrics::<metrics::SimpleSpread>("SimpleSpread")?;
    Ok(())
}

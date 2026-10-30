//! Plugin registration: typed factories by category and the built-in list.
use super::{
    Autonomy, Controller, Input, Interaction, Localization, Metrics, MotionModel, Network,
    PluginParams, Ports, ReadingKind, Sensor, Tracking, autonomy, controller, estimator,
    interaction, metrics, motion, network, sensor,
};
use crate::{common::Rate, scenario::PluginConfig};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

/// A plugin as a mission names it: the facts read from its configuration
/// (answered once, by the concrete type) and a way to make instances. `T` is
/// the category trait object the instances are stored as.
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
impl<T: ?Sized> CompiledPlugin<T> {
    fn new(
        params: &PluginParams,
        instantiate: impl Fn() -> Box<T> + Send + Sync + 'static,
    ) -> Result<Self> {
        Ok(Self {
            name: String::new(),
            ports: Ports::default(),
            rate: Rate::new(params.loop_rate_hz)?,
            inputs: Vec::new(),
            reading_kind: ReadingKind::Other,
            instantiate: Box::new(instantiate),
        })
    }
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

/// User projects register ordinary Rust types. No simulator source edits are
/// required. Each catalog holds the category trait the author implemented.
pub struct PluginRegistry {
    pub(crate) autonomies: Catalog<dyn Autonomy>,
    pub(crate) controllers: Catalog<dyn Controller>,
    pub(crate) motion: Catalog<dyn MotionModel>,
    pub(crate) sensors: Catalog<dyn Sensor>,
    pub(crate) localization: Catalog<dyn Localization>,
    pub(crate) tracking: Catalog<dyn Tracking>,
    pub(crate) interactions: Catalog<dyn Interaction>,
    pub(crate) networks: Catalog<dyn Network>,
    pub(crate) metrics: Catalog<dyn Metrics>,
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
// Each `register_*` runs the build half of a plugin's trait (`configure`,
// `ports`, `inputs`, `reading_kind`) here, where the concrete type `P` is
// known, and keeps only `new` in a closure that boxes instances as the
// category's trait object. After this, nothing but the box exists.
impl PluginRegistry {
    pub fn with_builtins() -> Self {
        let mut registry = Self::default();
        register_builtins(&mut registry).expect("built-in names must be unique");
        registry
    }
    pub fn register_autonomy<P: Autonomy + 'static>(&mut self, name: &str) -> Result<()> {
        self.register_autonomy_with(name, P::new)
    }
    /// Registers an autonomy built by `new` instead of `P::new`, so a program
    /// that owns the simulation can hand each instance something of its own,
    /// such as the actions a trainer chooses.
    pub fn register_autonomy_with<P: Autonomy + 'static>(
        &mut self,
        name: &str,
        new: impl Fn(&P::Config) -> P + Clone + Send + Sync + 'static,
    ) -> Result<()> {
        self.autonomies.insert(
            name,
            Box::new(move |params| {
                let config = P::configure(params)?;
                let ports = P::ports(&config);
                let new = new.clone();
                let mut plugin = CompiledPlugin::new(params, move || -> Box<dyn Autonomy> {
                    Box::new(new(&config))
                })?;
                plugin.ports = ports;
                Ok(plugin)
            }),
        )
    }
    pub fn register_controller<P: Controller + 'static>(&mut self, name: &str) -> Result<()> {
        self.controllers.insert(
            name,
            Box::new(|params| {
                let config = P::configure(params)?;
                let ports = P::ports(&config);
                let mut plugin = CompiledPlugin::new(params, move || -> Box<dyn Controller> {
                    Box::new(P::new(&config))
                })?;
                plugin.ports = ports;
                Ok(plugin)
            }),
        )
    }
    pub fn register_motion<P: MotionModel + 'static>(&mut self, name: &str) -> Result<()> {
        self.motion.insert(
            name,
            Box::new(|params| {
                let config = P::configure(params)?;
                let ports = P::ports(&config);
                let mut plugin = CompiledPlugin::new(params, move || -> Box<dyn MotionModel> {
                    Box::new(P::new(&config))
                })?;
                plugin.ports = ports;
                Ok(plugin)
            }),
        )
    }
    pub fn register_sensor<P: Sensor + 'static>(&mut self, name: &str) -> Result<()> {
        self.sensors.insert(
            name,
            Box::new(|params| {
                let config = P::configure(params)?;
                let reading_kind = P::reading_kind(&config);
                let mut plugin = CompiledPlugin::new(params, move || -> Box<dyn Sensor> {
                    Box::new(P::new(&config))
                })?;
                plugin.reading_kind = reading_kind;
                Ok(plugin)
            }),
        )
    }
    pub fn register_localization<P: Localization + 'static>(&mut self, name: &str) -> Result<()> {
        self.localization.insert(
            name,
            Box::new(|params| {
                let config = P::configure(params)?;
                let inputs = P::inputs(&config);
                let mut plugin = CompiledPlugin::new(params, move || -> Box<dyn Localization> {
                    Box::new(P::new(&config))
                })?;
                plugin.inputs = inputs;
                Ok(plugin)
            }),
        )
    }
    pub fn register_tracking<P: Tracking + 'static>(&mut self, name: &str) -> Result<()> {
        self.tracking.insert(
            name,
            Box::new(|params| {
                let config = P::configure(params)?;
                let inputs = P::inputs(&config);
                let mut plugin = CompiledPlugin::new(params, move || -> Box<dyn Tracking> {
                    Box::new(P::new(&config))
                })?;
                plugin.inputs = inputs;
                Ok(plugin)
            }),
        )
    }
    pub fn register_interaction<P: Interaction + 'static>(&mut self, name: &str) -> Result<()> {
        self.interactions.insert(
            name,
            Box::new(|params| {
                let config = P::configure(params)?;
                CompiledPlugin::new(params, move || -> Box<dyn Interaction> {
                    Box::new(P::new(&config))
                })
            }),
        )
    }
    pub fn register_network<P: Network + 'static>(&mut self, name: &str) -> Result<()> {
        self.networks.insert(
            name,
            Box::new(|params| {
                let config = P::configure(params)?;
                CompiledPlugin::new(params, move || -> Box<dyn Network> {
                    Box::new(P::new(&config))
                })
            }),
        )
    }
    pub fn register_metrics<P: Metrics + 'static>(&mut self, name: &str) -> Result<()> {
        self.metrics.insert(
            name,
            Box::new(|params| {
                let config = P::configure(params)?;
                CompiledPlugin::new(params, move || -> Box<dyn Metrics> {
                    Box::new(P::new(&config))
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
    registry.register_localization::<estimator::PassThroughState>("PassThrough")?;
    registry.register_tracking::<estimator::PassThroughContacts>("PassThrough")?;
    registry.register_interaction::<interaction::SimpleCollision>("SimpleCollision")?;
    registry.register_interaction::<interaction::Boundary>("Boundary")?;
    registry.register_interaction::<interaction::GroundCollision>("GroundCollision")?;
    registry.register_interaction::<interaction::TerrainCollision>("TerrainCollision")?;
    registry.register_network::<network::LocalNetwork>("LocalNetwork")?;
    registry.register_network::<network::GlobalNetwork>("GlobalNetwork")?;
    registry.register_network::<network::SphereNetwork>("SphereNetwork")?;
    registry.register_metrics::<metrics::SimpleCollisionMetrics>("SimpleCollisionMetrics")?;
    Ok(())
}

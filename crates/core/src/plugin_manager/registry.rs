use crate::entity::plugin_stack::{
    AgentBehavior, AutonomyAdapter, ControllerAdapter, MotionBehavior, SensorBehavior,
};
use crate::plugin::{
    Autonomy, Controller, Interaction, Metrics, MotionModel, Network, PluginParams, Ports, Sensor,
};
use crate::simcontrol::world_plugins::{InteractionBehavior, MetricsBehavior, NetworkBehavior};
use crate::{common::Rate, parse::PluginConfig};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

pub(crate) struct CompiledPlugin<T: ?Sized> {
    pub name: String,
    pub ports: Ports,
    pub rate: Rate,
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
        let compiler = self
            .compilers
            .get(&source.name)
            .with_context(|| format!("unregistered {} plugin '{}'", self.kind, source.name))?;
        let mut plugin = compiler(&PluginParams(&source.params))
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
            interactions: Catalog::new("Interaction"),
            networks: Catalog::new("Network"),
            metrics: Catalog::new("Metrics"),
        }
    }
}
impl PluginRegistry {
    pub fn with_builtins() -> Self {
        let mut registry = Self::default();
        super::builtins::register_builtins(&mut registry).expect("built-in names must be unique");
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
                    rate: Rate::new(params.0)?,
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
                    rate: Rate::new(params.0)?,
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
                    rate: Rate::new(params.0)?,
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
                    rate: Rate::new(params.0)?,
                    instantiate: Box::new(move || Box::new(T::new(&config))),
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
                    rate: Rate::new(params.0)?,
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
                    rate: Rate::new(params.0)?,
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
                    rate: Rate::new(params.0)?,
                    instantiate: Box::new(move || Box::new(T::new(&config))),
                })
            }),
        )
    }
    pub(crate) fn contains_in_category(&self, category: &str, name: &str) -> bool {
        match category {
            "autonomy" => self.autonomies.compilers.contains_key(name),
            "controller" => self.controllers.compilers.contains_key(name),
            "motion_model" => self.motion.compilers.contains_key(name),
            "sensor" => self.sensors.compilers.contains_key(name),
            "entity_interaction" => self.interactions.compilers.contains_key(name),
            "network" => self.networks.compilers.contains_key(name),
            "metrics" => self.metrics.compilers.contains_key(name),
            _ => false,
        }
    }
}

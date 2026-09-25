//! World plugin ownership and dispatch. Model logic belongs under plugin/{interaction,network,metrics}.
use crate::{Entity, EntitySnapshot, Event, Vec3, parse::ScenarioConfig, plugin::*};
use crate::{
    plugin_manager::{Catalog, CompiledPlugin},
    pubsub::messages::{Mailbox, ScheduledMessage},
};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

// Same forwarding layer as entity/plugin_stack.rs; its comment describes a
// possible simplification to investigate once the tool is more established.
pub(crate) trait InteractionBehavior: Send {
    fn initialize(&mut self, context: &mut WorldContext<'_>) -> Result<()>;
    fn collision_exists(&self, entities: &[Entity], position_world_m: Vec3) -> bool;
    fn step(&mut self, context: &mut InteractionContext<'_>) -> Result<Update>;
    fn close(&mut self, time: StepTime) -> Result<()>;
}
impl<T: Interaction> InteractionBehavior for T {
    fn initialize(&mut self, context: &mut WorldContext<'_>) -> Result<()> {
        Interaction::initialize(self, context)
    }
    fn collision_exists(&self, entities: &[Entity], position_world_m: Vec3) -> bool {
        Interaction::collision_exists(self, entities, position_world_m)
    }
    fn step(&mut self, context: &mut InteractionContext<'_>) -> Result<Update> {
        Interaction::step(self, context)
    }
    fn close(&mut self, time: StepTime) -> Result<()> {
        Plugin::close(self, time)
    }
}
pub(crate) trait NetworkBehavior: Send {
    fn initialize(&mut self, context: &mut WorldContext<'_>) -> Result<()>;
    fn step(&mut self, context: &mut NetworkContext<'_, '_>) -> Result<Update>;
    fn close(&mut self, time: StepTime) -> Result<()>;
}
impl<T: Network> NetworkBehavior for T {
    fn initialize(&mut self, context: &mut WorldContext<'_>) -> Result<()> {
        Network::initialize(self, context)
    }
    fn step(&mut self, context: &mut NetworkContext<'_, '_>) -> Result<Update> {
        Network::step(self, context)
    }
    fn close(&mut self, time: StepTime) -> Result<()> {
        Plugin::close(self, time)
    }
}
pub(crate) trait MetricsBehavior: Send {
    fn initialize(&mut self, context: &mut WorldContext<'_>) -> Result<()>;
    fn step(&mut self, context: &mut WorldContext<'_>) -> Result<Update>;
    fn report(&self, time_s: f64) -> MetricReport;
    fn close(&mut self, time: StepTime) -> Result<()>;
}
impl<T: Metrics> MetricsBehavior for T {
    fn initialize(&mut self, context: &mut WorldContext<'_>) -> Result<()> {
        Metrics::initialize(self, context)
    }
    fn step(&mut self, context: &mut WorldContext<'_>) -> Result<Update> {
        Metrics::step(self, context)
    }
    fn report(&self, time_s: f64) -> MetricReport {
        Metrics::report(self, time_s)
    }
    fn close(&mut self, time: StepTime) -> Result<()> {
        Plugin::close(self, time)
    }
}

pub(crate) struct CompiledWorld {
    interactions: Vec<CompiledPlugin<dyn InteractionBehavior>>,
    networks: Vec<CompiledPlugin<dyn NetworkBehavior>>,
    metrics: Vec<CompiledPlugin<dyn MetricsBehavior>>,
}

fn compile_world_plugins<T: ?Sized>(
    catalog: &Catalog<T>,
    sources: &[crate::parse::PluginConfig],
) -> Result<Vec<CompiledPlugin<T>>> {
    sources
        .iter()
        .map(|source| {
            let compiled = catalog.compile(source)?;
            ensure!(
                compiled.ports.inputs.is_empty() && compiled.ports.outputs.is_empty(),
                "world plugin '{}' uses messages, not control-chain ports",
                source.name
            );
            Ok(compiled)
        })
        .collect()
}

impl CompiledWorld {
    pub fn compile(config: &ScenarioConfig, registry: &PluginRegistry) -> Result<Self> {
        let interactions = compile_world_plugins(&registry.interactions, &config.interactions)?;
        let mut networks = compile_world_plugins(&registry.networks, &config.networks)?;
        if !networks.iter().any(|plugin| plugin.name == "GlobalNetwork") {
            networks.push(registry.networks.compile(&crate::parse::PluginConfig {
                name: "GlobalNetwork".into(),
                params: crate::Params::new(),
            })?);
        }
        networks.sort_by(|first, second| first.name.cmp(&second.name));
        for pair in networks.windows(2) {
            ensure!(
                pair[0].name != pair[1].name,
                "duplicate network '{}'",
                pair[0].name
            );
        }
        let metrics = compile_world_plugins(&registry.metrics, &config.metrics)?;
        Ok(Self {
            interactions,
            networks,
            metrics,
        })
    }
    pub fn instantiate(&self, seed: u32) -> WorldPlugins {
        WorldPlugins {
            interactions: self
                .interactions
                .iter()
                .enumerate()
                .map(|(index, plugin)| WorldSlot::new(plugin, index, "interaction", seed))
                .collect(),
            networks: self
                .networks
                .iter()
                .enumerate()
                .map(|(index, plugin)| WorldSlot::new(plugin, index, "network", seed))
                .collect(),
            metrics: self
                .metrics
                .iter()
                .enumerate()
                .map(|(index, plugin)| WorldSlot::new(plugin, index, "metrics", seed))
                .collect(),
        }
    }
}
struct WorldSlot<T: ?Sized> {
    name: String,
    endpoint: MessageEndpoint,
    plugin: Box<T>,
    messages: Messages,
    scheduled: Vec<ScheduledMessage>,
    // Only networks draw from this today; interactions and metrics can expose it when needed.
    random: PluginRandom,
    closed: bool,
}
impl<T: ?Sized> WorldSlot<T> {
    fn new(plugin: &CompiledPlugin<T>, index: usize, category: &str, seed: u32) -> Self {
        let identity = format!("{category}/{}:{index}", plugin.name);
        Self {
            name: plugin.name.clone(),
            random: PluginRandom::new(seed, 0, &identity),
            endpoint: MessageEndpoint {
                entity_id: None,
                plugin: identity,
            },
            plugin: (plugin.instantiate)(),
            messages: Messages::default(),
            scheduled: Vec::new(),
            closed: false,
        }
    }
    fn mailbox(&mut self) -> Mailbox<'_> {
        Mailbox {
            endpoint: self.endpoint.clone(),
            messages: &mut self.messages,
        }
    }

    fn initialize(
        &mut self,
        time: StepTime,
        contacts: &[EntitySnapshot],
        teams: &BTreeMap<i32, i32>,
        initialize: impl FnOnce(&mut T, &mut WorldContext<'_>) -> Result<()>,
    ) -> Result<()> {
        self.messages.time_s = time.time_s;
        initialize(
            &mut self.plugin,
            &mut WorldContext {
                time,
                contacts_truth: contacts,
                entity_teams: teams,
                messages: &mut self.messages,
            },
        )
        .with_context(|| format!("initialize world plugin '{}'", self.name))
    }

    fn close(
        &mut self,
        time: StepTime,
        close: impl FnOnce(&mut T, StepTime) -> Result<()>,
    ) -> Result<()> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            close(&mut self.plugin, time)
        }))
        .unwrap_or_else(|_| Err(anyhow::anyhow!("plugin close panicked")))
        .with_context(|| format!("close world plugin '{}'", self.name))
    }
}
pub(crate) struct WorldPlugins {
    interactions: Vec<WorldSlot<dyn InteractionBehavior>>,
    networks: Vec<WorldSlot<dyn NetworkBehavior>>,
    metrics: Vec<WorldSlot<dyn MetricsBehavior>>,
}
impl WorldPlugins {
    pub fn initialize(
        &mut self,
        time: StepTime,
        contacts: &[EntitySnapshot],
        teams: &BTreeMap<i32, i32>,
    ) -> Result<()> {
        for slot in &mut self.networks {
            slot.initialize(time, contacts, teams, NetworkBehavior::initialize)?;
        }
        for slot in &mut self.interactions {
            slot.initialize(time, contacts, teams, InteractionBehavior::initialize)?;
        }
        for slot in &mut self.metrics {
            slot.initialize(time, contacts, teams, MetricsBehavior::initialize)?;
        }
        Ok(())
    }
    pub fn collision_exists(&self, entities: &[Entity], position_world_m: Vec3) -> bool {
        self.interactions
            .iter()
            .any(|slot| slot.plugin.collision_exists(entities, position_world_m))
    }
    pub fn interactions(
        &mut self,
        time: StepTime,
        entities: &mut [Entity],
        events: &mut Vec<Event>,
    ) -> Result<bool> {
        let mut stop = false;
        for slot in &mut self.interactions {
            // Source interactions run every tick, including the pre-start pass.
            slot.messages.time_s = time.time_s;
            let update = slot
                .plugin
                .step(&mut InteractionContext {
                    time,
                    entities,
                    events,
                    messages: &mut slot.messages,
                })
                .with_context(|| format!("interaction '{}'", slot.name))?;
            stop |= update == Update::Stop;
        }
        Ok(stop)
    }
    pub fn networks(
        &mut self,
        time: StepTime,
        contacts: &[EntitySnapshot],
        entities: &mut [Entity],
        simulator: &mut Messages,
    ) -> Result<bool> {
        let names: Vec<_> = self.networks.iter().map(|slot| slot.name.clone()).collect();
        let mut mailboxes = vec![Mailbox {
            endpoint: MessageEndpoint {
                entity_id: None,
                plugin: "SimControl".into(),
            },
            messages: simulator,
        }];
        entities.sort_by_key(Entity::id);
        for entity in entities {
            entity.mailboxes(&mut mailboxes);
        }
        for slot in &mut self.interactions {
            mailboxes.push(slot.mailbox());
        }
        for slot in &mut self.metrics {
            mailboxes.push(slot.mailbox());
        }
        let mut network_steps = Vec::new();
        for slot in &mut self.networks {
            slot.messages.time_s = time.time_s;
            mailboxes.push(Mailbox {
                endpoint: slot.endpoint.clone(),
                messages: &mut slot.messages,
            });
            network_steps.push((
                &slot.name,
                &slot.endpoint,
                &mut slot.plugin,
                &mut slot.scheduled,
                &mut slot.random,
            ));
        }
        let names: Vec<_> = names.iter().map(String::as_str).collect();
        for mailbox in &mailboxes {
            mailbox.messages.validate_networks(&names)?;
        }
        let mut stop = false;
        for (name, endpoint, plugin, scheduled, random) in network_steps {
            let mut context = NetworkContext {
                time,
                contacts_truth: contacts,
                name,
                endpoint,
                mailboxes: &mut mailboxes,
                scheduled,
                random,
                routed: false,
            };
            let update = plugin
                .step(&mut context)
                .with_context(|| format!("network '{name}'"))?;
            stop |= update == Update::Stop;
        }
        Ok(stop)
    }
    pub fn metrics(
        &mut self,
        time: StepTime,
        contacts: &[EntitySnapshot],
        teams: &BTreeMap<i32, i32>,
    ) -> Result<bool> {
        let mut stop = false;
        for slot in &mut self.metrics {
            slot.messages.time_s = time.time_s;
            let update = slot
                .plugin
                .step(&mut WorldContext {
                    time,
                    contacts_truth: contacts,
                    entity_teams: teams,
                    messages: &mut slot.messages,
                })
                .with_context(|| format!("metrics '{}'", slot.name))?;
            stop |= update == Update::Stop;
        }
        Ok(stop)
    }
    pub fn reports(&self, time_s: f64) -> Vec<(String, MetricReport)> {
        self.metrics
            .iter()
            .map(|slot| (slot.endpoint.plugin.clone(), slot.plugin.report(time_s)))
            .collect()
    }
    pub fn close(&mut self, time: StepTime) -> Result<()> {
        let mut failure = None;
        let mut record = |result| {
            if let Err(error) = result {
                failure.get_or_insert(error);
            }
        };
        for slot in &mut self.interactions {
            record(slot.close(time, InteractionBehavior::close));
        }
        for slot in &mut self.networks {
            record(slot.close(time, NetworkBehavior::close));
        }
        for slot in &mut self.metrics {
            record(slot.close(time, MetricsBehavior::close));
        }
        failure.map_or(Ok(()), Err)
    }
}

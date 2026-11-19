//! World plugin ownership and dispatch. Model logic belongs under plugin/{interaction,network,metrics}.
use crate::common::{SpatialIndex, Terrain, TruthContacts};
use crate::{
    Entity, EntitySnapshot, Event, Vec3, drawing::PluginDrawings, plugin::*,
    scenario::ScenarioConfig,
};
use crate::{
    plugin::{Catalog, CompiledPlugin},
    pubsub::routing::{Mailbox, NetworkTrace, ScheduledMessage},
};
use anyhow::{Context, Result, ensure};
use std::{collections::BTreeMap, sync::Arc};
use vstd::prelude::*;

pub(crate) mod proof;

verus! {

broadcast use proof::world_facts;

#[verifier::external]
pub(crate) struct CompiledWorld {
    interactions: Vec<CompiledPlugin<dyn Interaction>>,
    networks: Vec<CompiledPlugin<dyn Network>>,
    metrics: Vec<CompiledPlugin<dyn Metrics>>,
}

#[verifier::external]
fn compile_world_plugins<T: ?Sized>(
    catalog: &Catalog<T>,
    sources: &[crate::scenario::PluginConfig],
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

#[verifier::external]
impl CompiledWorld {
    pub fn compile(config: &ScenarioConfig, registry: &PluginRegistry) -> Result<Self> {
        let interactions = compile_world_plugins(&registry.interactions, &config.interactions)?;
        let mut networks = compile_world_plugins(&registry.networks, &config.networks)?;
        // A network's name is its instance name when given (`name`), else its plugin.
        for (network, source) in networks.iter_mut().zip(&config.networks) {
            if let Some(instance) = &source.instance {
                network.name = instance.clone();
            }
        }
        if !networks.iter().any(|plugin| plugin.name == "GlobalNetwork") {
            networks.push(
                registry
                    .networks
                    .compile(&crate::scenario::PluginConfig::new("GlobalNetwork"))?,
            );
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
            closed: false,
        }
    }
}
/// One live world plugin and what the engine keeps beside it: its message
/// address, mailbox, scheduled messages, random stream, and fixed drawings.
/// `T` is the plugin's trait, so one `WorldSlot` type serves every category.
/// The fields are crate-visible so a proof can state what a step leaves
/// untouched.
#[verifier::reject_recursive_types(T)]
pub(crate) struct WorldSlot<T: ?Sized> {
    pub(crate) name: String,
    pub(crate) endpoint: MessageEndpoint,
    pub(crate) plugin: Box<T>,
    pub(crate) messages: Messages,
    pub(crate) scheduled: Vec<ScheduledMessage>,
    // Only networks draw today; interactions and metrics can expose it when needed.
    pub(crate) random: PluginRandom,
    /// The plugin's fixed drawings, asked once after `initialize`.
    pub(crate) fixed_drawings: Arc<[Drawing]>,
}
#[verifier::external]
impl<T: ?Sized> WorldSlot<T> {
    fn new(plugin: &CompiledPlugin<T>, index: usize, category: &str, seed: u32) -> Self {
        let identity = format!("{category}/{}:{index}", plugin.name);
        // World plugins have no entity (IDs start at 1). A network's numbers
        // are keyed by its name, which is unique in a mission.
        let random_identity = if category == "network" {
            &plugin.name
        } else {
            &identity
        };
        Self {
            name: plugin.name.clone(),
            random: PluginRandom::new(seed, 0, random_identity),
            endpoint: MessageEndpoint {
                entity_id: None,
                plugin: identity,
            },
            plugin: (plugin.instantiate)(),
            messages: Messages::default(),
            scheduled: Vec::new(),
            fixed_drawings: Arc::from(Vec::new()),
        }
    }
    fn mailbox(&mut self) -> Mailbox<'_> {
        Mailbox {
            endpoint: &self.endpoint,
            messages: &mut self.messages,
        }
    }

    fn initialize(
        &mut self,
        time: StepTime,
        contacts: &[EntitySnapshot],
        teams: &BTreeMap<i32, i32>,
        terrain: Option<&Terrain>,
        initialize: impl FnOnce(&mut T, &mut WorldContext<'_>) -> Result<()>,
    ) -> Result<()> {
        self.messages.time_s = time.time_s;
        initialize(
            &mut self.plugin,
            &mut WorldContext {
                time,
                contacts_truth: TruthContacts::from(contacts),
                entity_teams: teams,
                messages: &mut self.messages,
                terrain,
            },
        )
        .with_context(|| format!("initialize world plugin '{}'", self.name))
    }

    fn close(
        &mut self,
        time: StepTime,
        close: impl FnOnce(&mut T, StepTime) -> Result<()>,
    ) -> Result<()> {
        close(&mut self.plugin, time).with_context(|| format!("close world plugin '{}'", self.name))
    }
}
/// An interaction as the engine steps it, with an `InteractionContext`.
/// `entity/plugin_stack/agent.rs` gives it to `dyn Interaction`, so the one
/// call into the plugin can carry a contract; see `MotionStep` there.
#[verifier::external]
pub(crate) trait InteractionStep {
    fn step(&mut self, context: &mut InteractionContext<'_>) -> Result<Update>;
}

impl<T: ?Sized + InteractionStep> WorldSlot<T> {
    /// Steps the interaction over the entities, with the index of their
    /// positions, the terrain, and the run's events; its mailbox is stamped
    /// with the time.
    fn interaction_step(
        &mut self,
        time: StepTime,
        entities: &mut [Entity],
        nearby: &SpatialIndex,
        terrain: Option<&Terrain>,
        events: &mut Vec<Event>,
    ) -> (r: Result<Update>)
        ensures proof::interaction_step_returned(
            old(self), final(self), time, old(entities)@, final(entities)@, *nearby, terrain, old(events)@, final(events)@, &r)
    {
        // Source interactions run every tick, including the pre-start pass.
        self.messages.time_s = time.time_s;
        let mut context = InteractionContext {
            time,
            entities,
            nearby,
            events,
            messages: &mut self.messages,
            terrain,
        };
        self.plugin
            .step(&mut context)
            .map_err(|error| error.context(format!("interaction '{}'", self.name)))
    }
}

/// A metrics plugin as the engine steps it, with a `WorldContext`;
/// `entity/plugin_stack/agent.rs` gives it to `dyn Metrics`.
#[verifier::external]
pub(crate) trait MetricsStep {
    fn step(&mut self, context: &mut WorldContext<'_>) -> Result<Update>;
}

impl<T: ?Sized + MetricsStep> WorldSlot<T> {
    /// Steps the metrics plugin with the truth contacts, the teams, and the
    /// terrain; its mailbox is stamped with the time.
    fn metrics_step(
        &mut self,
        time: StepTime,
        contacts: TruthContacts<'_>,
        teams: &BTreeMap<i32, i32>,
        terrain: Option<&Terrain>,
    ) -> (r: Result<Update>)
        ensures proof::metrics_step_returned(old(self), final(self), time, contacts, teams, terrain, &r)
    {
        self.messages.time_s = time.time_s;
        let mut context = WorldContext {
            time,
            contacts_truth: contacts,
            entity_teams: teams,
            messages: &mut self.messages,
            terrain,
        };
        self.plugin
            .step(&mut context)
            .map_err(|error| error.context(format!("metrics '{}'", self.name)))
    }
}

pub(crate) struct WorldPlugins {
    pub(crate) interactions: Vec<WorldSlot<dyn Interaction>>,
    pub(crate) networks: Vec<WorldSlot<dyn Network>>,
    pub(crate) metrics: Vec<WorldSlot<dyn Metrics>>,
    pub(crate) closed: bool,
}
#[verifier::external]
impl WorldPlugins {
    pub fn initialize(
        &mut self,
        time: StepTime,
        contacts: &[EntitySnapshot],
        teams: &BTreeMap<i32, i32>,
        terrain: Option<&Terrain>,
    ) -> Result<()> {
        for slot in &mut self.networks {
            slot.initialize(time, contacts, teams, terrain, Network::initialize)?;
        }
        for slot in &mut self.interactions {
            slot.initialize(time, contacts, teams, terrain, Interaction::initialize)?;
        }
        for slot in &mut self.metrics {
            slot.initialize(time, contacts, teams, terrain, Metrics::initialize)?;
        }
        for slot in &mut self.interactions {
            slot.fixed_drawings = slot.plugin.fixed_drawings().into();
        }
        for slot in &mut self.networks {
            slot.fixed_drawings = slot.plugin.fixed_drawings().into();
        }
        for slot in &mut self.metrics {
            slot.fixed_drawings = slot.plugin.fixed_drawings().into();
        }
        Ok(())
    }
    pub fn collision_exists(
        &self,
        entities: &[Entity],
        terrain: Option<&Terrain>,
        position_world_m: Vec3,
    ) -> bool {
        self.interactions.iter().any(|slot| {
            slot.plugin
                .collision_exists(entities, terrain, position_world_m)
        })
    }
    pub fn networks(
        &mut self,
        time: StepTime,
        contacts: TruthContacts<'_>,
        entities: &mut [Entity],
        simulator: &mut Messages,
        mut trace: Option<&mut NetworkTrace>,
    ) -> Result<()> {
        let names: Vec<_> = self.networks.iter().map(|slot| slot.name.clone()).collect();
        let simulator_endpoint = MessageEndpoint {
            entity_id: None,
            plugin: "SimControl".into(),
        };
        let mut mailboxes = vec![Mailbox {
            endpoint: &simulator_endpoint,
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
                endpoint: &slot.endpoint,
                messages: &mut slot.messages,
            });
            network_steps.push((
                &slot.name,
                &slot.endpoint,
                &mut slot.plugin,
                &mut slot.scheduled,
                slot.random.at_tick(time.tick),
            ));
        }
        let names: Vec<_> = names.iter().map(String::as_str).collect();
        for mailbox in &mailboxes {
            mailbox.messages.validate_networks(&names)?;
        }
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
                trace: trace.as_deref_mut(),
            };
            plugin
                .step(&mut context)
                .with_context(|| format!("network '{name}'"))?;
        }
        Ok(())
    }
    /// Each world plugin's drawings at `time_s`: interactions, then networks,
    /// then metrics, each in mission order, the fixed part by reference and
    /// the changing part asked now. Plugins with nothing in either are left
    /// out.
    pub fn drawings(&self, time_s: f64) -> Vec<PluginDrawings> {
        let mut drawings = Vec::new();
        let mut collect = |name: &str, fixed: &Arc<[Drawing]>, changing: Vec<Drawing>| {
            if !fixed.is_empty() || !changing.is_empty() {
                drawings.push(PluginDrawings {
                    source: name.to_string(),
                    fixed: fixed.clone(),
                    changing,
                });
            }
        };
        for slot in &self.interactions {
            collect(
                &slot.name,
                &slot.fixed_drawings,
                slot.plugin.drawings(time_s),
            );
        }
        for slot in &self.networks {
            collect(
                &slot.name,
                &slot.fixed_drawings,
                slot.plugin.drawings(time_s),
            );
        }
        for slot in &self.metrics {
            collect(
                &slot.name,
                &slot.fixed_drawings,
                slot.plugin.drawings(time_s),
            );
        }
        drawings
    }
    pub fn reports(&self, time_s: f64) -> Vec<(String, MetricReport)> {
        self.metrics
            .iter()
            .map(|slot| (slot.endpoint.plugin.clone(), slot.plugin.report(time_s)))
            .collect()
    }
    pub fn close(&mut self, time: StepTime) -> Result<()> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        let mut failure = None;
        let mut record = |result| {
            if let Err(error) = result {
                failure.get_or_insert(error);
            }
        };
        for slot in &mut self.interactions {
            record(slot.close(time, Interaction::close));
        }
        for slot in &mut self.networks {
            record(slot.close(time, Network::close));
        }
        for slot in &mut self.metrics {
            record(slot.close(time, Metrics::close));
        }
        failure.map_or(Ok(()), Err)
    }
}

impl WorldPlugins {
    /// Steps the interactions in order over the entities; answers whether one
    /// asked to stop.
    pub(crate) fn interactions(
        &mut self,
        time: StepTime,
        entities: &mut [Entity],
        nearby: &mut SpatialIndex,
        terrain: Option<&Terrain>,
        events: &mut Vec<Event>,
    ) -> (r: Result<bool>)
        ensures proof::interacted(
            old(self), final(self), time, terrain, old(entities)@, final(entities)@, old(events)@, final(events)@, &r)
    {
        step_interactions(&mut self.interactions, time, entities, nearby, terrain, events)
    }
    /// Steps the metrics plugins in order; answers whether one asked to stop.
    pub(crate) fn metrics(
        &mut self,
        time: StepTime,
        contacts: TruthContacts<'_>,
        teams: &BTreeMap<i32, i32>,
        terrain: Option<&Terrain>,
    ) -> (r: Result<bool>)
        ensures proof::measured(old(self), final(self), time, contacts, teams, terrain, &r)
    {
        step_metrics(&mut self.metrics, time, contacts, teams, terrain)
    }
}

/// Steps each interaction in order, refreshing the index first because an
/// earlier interaction may have moved entities; stops at the first error;
/// answers whether one asked to stop.
fn step_interactions<T: ?Sized + InteractionStep>(
    slots: &mut Vec<WorldSlot<T>>,
    time: StepTime,
    entities: &mut [Entity],
    nearby: &mut SpatialIndex,
    terrain: Option<&Terrain>,
    events: &mut Vec<Event>,
) -> (r: Result<bool>)
    ensures proof::interactions_stepped(
        old(slots), final(slots), time, terrain, old(entities)@, final(entities)@, old(events)@, final(events)@, &r)
{
    let mut stop = false;
    for slot in it: slots.as_mut_slice().iter_mut()
        invariant proof::interactions_so_far(old(slots), it, time, terrain, old(entities)@, entities, old(events)@, events, stop)
    {
        nearby.refresh(entities);
        let update = slot.interaction_step(time, entities, nearby, terrain, events)?;
        stop |= update == Update::Stop;
    }
    Ok(stop)
}

/// Steps each metrics plugin in order; stops at the first error; answers
/// whether one asked to stop.
fn step_metrics<T: ?Sized + MetricsStep>(
    slots: &mut Vec<WorldSlot<T>>,
    time: StepTime,
    contacts: TruthContacts<'_>,
    teams: &BTreeMap<i32, i32>,
    terrain: Option<&Terrain>,
) -> (r: Result<bool>)
    ensures proof::metrics_stepped(old(slots), final(slots), time, contacts, teams, terrain, &r)
{
    let mut stop = false;
    for slot in it: slots.as_mut_slice().iter_mut()
        invariant proof::metrics_so_far(old(slots), it, time, contacts, teams, terrain, stop)
    {
        let update = slot.metrics_step(time, contacts, teams, terrain)?;
        stop |= update == Update::Stop;
    }
    Ok(stop)
}

} // verus!

//! Consumers for the built-in contact sensor and auction; all use public plugin APIs.
use std::path::PathBuf;

use anyhow::Result;
use scrimmage_core::plugin::autonomy::{
    AuctionBid, AuctionResult, AuctionStart, BID_AUCTION_TOPIC, RESULT_AUCTION_TOPIC,
    START_AUCTION_TOPIC,
};
use scrimmage_core::plugin::sensor::ContactsWithCovariances;
use scrimmage_core::plugin::{
    AgentContext, Autonomy, Frame, Interaction, InteractionContext, Plugin, PluginIo, PluginParams,
    PluginRegistry, Port, Ports, Sensor, SensorContext, Unit, Update,
};
use scrimmage_core::{Mission, Params, Simulation};
use serde::Deserialize;

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ContactFollowerConfig {
    follow: bool,
}

struct ContactFollower {
    follow: bool,
}

impl Plugin for ContactFollower {
    type Config = ContactFollowerConfig;
    fn configure(params: &PluginParams<'_>) -> Result<ContactFollowerConfig> {
        params.parse()
    }
    fn new(config: &ContactFollowerConfig) -> Self {
        Self {
            follow: config.follow,
        }
    }
    fn ports(_: &ContactFollowerConfig) -> Ports {
        Ports::default().output(Port::new("velocity_x", Unit::MetersPerSecond, Frame::World))
    }
}

impl Autonomy for ContactFollower {
    fn initialize(&mut self, context: &mut AgentContext<'_>) -> Result<()> {
        context
            .messages
            .subscribe::<ContactsWithCovariances>("LocalNetwork", "detected")
    }
    fn step(&mut self, context: &mut AgentContext<'_>, io: &mut PluginIo) -> Result<Update> {
        let messages = context
            .messages
            .receive::<ContactsWithCovariances>("LocalNetwork", "detected")?;
        let mut velocity_mps = 0.0;
        if self.follow
            && context.entity.id == 1
            && let Some(sample) = messages.last()
            && let Some(contact) = sample.value.contacts.first()
        {
            velocity_mps = contact.measurement.state.position_world_m.x;
        }
        io.write("velocity_x", velocity_mps)?;
        Ok(Update::Applied)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
struct AuctionTrace {
    starts: Vec<(f64, AuctionStart)>,
    bids: Vec<(f64, AuctionBid)>,
    results: Vec<(f64, AuctionResult)>,
}

/// Records what this entity received, so tests can inspect it after each step.
/// Messages arrive one tick after publication, like any other consumer.
struct Observer {
    trace: AuctionTrace,
    contacts: Option<ContactsWithCovariances>,
}

impl Plugin for Observer {
    type Config = ();
    fn configure(params: &PluginParams<'_>) -> Result<()> {
        params.parse()
    }
    fn new(_: &()) -> Self {
        Self {
            trace: AuctionTrace::default(),
            contacts: None,
        }
    }
}

impl Sensor for Observer {
    fn initialize(&mut self, context: &mut SensorContext<'_>) -> Result<()> {
        context
            .messages
            .subscribe::<ContactsWithCovariances>("LocalNetwork", "detected")?;
        context
            .messages
            .subscribe::<AuctionStart>("SphereNetwork", START_AUCTION_TOPIC)?;
        context
            .messages
            .subscribe::<AuctionBid>("SphereNetwork", BID_AUCTION_TOPIC)?;
        context
            .messages
            .subscribe::<AuctionResult>("SphereNetwork", RESULT_AUCTION_TOPIC)
    }
    fn step(&mut self, context: &mut SensorContext<'_>) -> Result<Update> {
        if let Some(message) = context
            .messages
            .receive::<ContactsWithCovariances>("LocalNetwork", "detected")?
            .pop()
        {
            self.contacts = Some((*message.value).clone());
        }
        if let Some(contacts) = &self.contacts {
            context.publish_local("contacts_seen", contacts.clone())?;
        }
        for message in context
            .messages
            .receive::<AuctionStart>("SphereNetwork", START_AUCTION_TOPIC)?
        {
            self.trace.starts.push((message.sent_at_s, *message.value));
        }
        for message in context
            .messages
            .receive::<AuctionBid>("SphereNetwork", BID_AUCTION_TOPIC)?
        {
            self.trace.bids.push((message.sent_at_s, *message.value));
        }
        for message in context
            .messages
            .receive::<AuctionResult>("SphereNetwork", RESULT_AUCTION_TOPIC)?
        {
            self.trace.results.push((message.sent_at_s, *message.value));
        }
        context.publish_local("auction_trace", self.trace.clone())?;
        Ok(Update::Applied)
    }
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RemoveContactsConfig {
    remove_at_s: f64,
}
impl Default for RemoveContactsConfig {
    fn default() -> Self {
        Self { remove_at_s: 100.0 }
    }
}

struct RemoveContacts {
    at_s: f64,
}
impl Plugin for RemoveContacts {
    type Config = RemoveContactsConfig;
    fn configure(params: &PluginParams<'_>) -> Result<RemoveContactsConfig> {
        params.parse()
    }
    fn new(config: &RemoveContactsConfig) -> Self {
        Self {
            at_s: config.remove_at_s,
        }
    }
}
impl Interaction for RemoveContacts {
    fn step(&mut self, context: &mut InteractionContext<'_>) -> Result<Update> {
        if context.time.time_s >= self.at_s {
            for entity in context.entities.iter_mut() {
                if entity.id() != 1 {
                    entity.set_health(0);
                }
            }
        }
        Ok(Update::Applied)
    }
}

fn simulation(workers: usize, overrides: &Params) -> Result<Simulation> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut registry = PluginRegistry::with_builtins();
    registry.register_autonomy::<ContactFollower>("ContactFollower")?;
    registry.register_sensor::<Observer>("Observer")?;
    registry.register_interaction::<RemoveContacts>("RemoveContacts")?;
    let scenario = Mission::load_with_registry(
        &root.join("crates/core/tests/fixtures/perception_communication.xml"),
        &root,
        overrides,
        &registry,
    )?
    .scenario;
    Simulation::new(scenario, &registry, workers)
}

/// The newest contact snapshot this entity has received, if any yet.
fn contacts(simulation: &Simulation, entity_index: usize) -> Option<&ContactsWithCovariances> {
    simulation.entities()[entity_index]
        .observations()
        .get::<ContactsWithCovariances>("contacts_seen")
        .unwrap()
        .map(|observation| &observation.value)
}

fn trace(simulation: &Simulation, entity_index: usize) -> &AuctionTrace {
    &simulation.entities()[entity_index]
        .observations()
        .get::<AuctionTrace>("auction_trace")
        .unwrap()
        .unwrap()
        .value
}

#[test]
fn auction_selects_highest_received_bid_and_excludes_out_of_range_agent() -> Result<()> {
    for workers in [1, 2, 8] {
        let mut simulation = simulation(workers, &Params::new())?;
        while simulation.step()?.is_some() {}
        let first = trace(&simulation, 0);
        assert_eq!(first.starts, vec![(0.0, AuctionStart { auctioneer_id: 1 })]);
        assert_eq!(first.bids.len(), 2);
        assert_eq!(
            first
                .bids
                .iter()
                .map(|(_, bid)| bid.bidder_id)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert!(
            first
                .bids
                .iter()
                .all(|(time, bid)| *time == 0.25 && (0.0..10.0).contains(&bid.bid))
        );
        assert_ne!(first.bids[0].1.bid, first.bids[1].1.bid);
        let best = first
            .bids
            .iter()
            .map(|(_, bid)| *bid)
            .max_by(|a, b| a.bid.total_cmp(&b.bid));
        assert_eq!(first.results, vec![(0.75, AuctionResult { winner: best })]);
        assert_eq!(trace(&simulation, 1), first);
        assert_eq!(trace(&simulation, 2), &AuctionTrace::default());
    }
    Ok(())
}

#[test]
fn complete_loss_drops_even_same_entity_auction_traffic() -> Result<()> {
    let mut simulation = simulation(2, &Params::from([("probability".into(), "0".into())]))?;
    while simulation.step()?.is_some() {}
    for index in 0..3 {
        assert_eq!(trace(&simulation, index), &AuctionTrace::default());
    }
    Ok(())
}

#[test]
fn an_auction_can_close_without_a_bid_and_does_not_reopen_for_late_bids() -> Result<()> {
    let mut simulation = simulation(2, &Params::from([("duration".into(), "0".into())]))?;
    while simulation.step()?.is_some() {}
    let first = trace(&simulation, 0);
    assert_eq!(first.results, vec![(0.25, AuctionResult { winner: None })]);
    assert_eq!(first.bids.len(), 2);
    Ok(())
}

#[test]
fn contact_feedback_uses_local_measurements_on_the_next_tick_without_changing_belief() -> Result<()>
{
    let mut simulation = simulation(8, &Params::from([("follow".into(), "true".into())]))?;
    simulation.step()?;
    assert_eq!(simulation.entities()[0].truth().position_world_m.x, 0.0);
    simulation.step()?;
    // Target is at x=50; sensor adds +2 m. The controller must use 52, not truth 50.
    assert_eq!(simulation.entities()[0].truth().position_world_m.x, 13.0);
    for (index, entity) in simulation.entities().iter().enumerate() {
        assert_eq!(
            entity.belief().position_world_m,
            entity.truth().position_world_m
        );
        let sample = contacts(&simulation, index).expect("first snapshot delivered");
        assert_eq!(sample.contacts.len(), 2);
        assert!(
            sample
                .contacts
                .iter()
                .all(|contact| contact.entity.id != entity.id())
        );
    }
    Ok(())
}

#[test]
fn contacts_include_new_spawns_and_clear_removed_targets() -> Result<()> {
    let mut simulation = simulation(
        2,
        &Params::from([
            ("spawn_at".into(), "0.5".into()),
            ("remove_at".into(), "1".into()),
        ]),
    )?;
    let mut counts = Vec::new();
    while simulation.step()?.is_some() {
        if let Some(sample) = contacts(&simulation, 0) {
            counts.push(sample.contacts.len());
        }
    }
    assert_eq!(counts[0], 0);
    assert!(
        counts.contains(&2),
        "scheduled targets must appear: {counts:?}"
    );
    assert_eq!(*counts.last().unwrap(), 0);
    Ok(())
}

fn observation_history(workers: usize, seed: u32, probability: f64) -> Result<String> {
    let mut simulation = simulation(
        workers,
        &Params::from([
            ("seed".into(), seed.to_string()),
            ("probability".into(), probability.to_string()),
            ("noise".into(), "2 0.5".into()),
        ]),
    )?;
    let mut history = String::new();
    while simulation.step()?.is_some() {
        for index in 0..simulation.entities().len() {
            history.push_str(&format!(
                "{:?} {:?}\n",
                contacts(&simulation, index),
                trace(&simulation, index)
            ));
        }
    }
    Ok(history)
}

#[test]
fn actual_measurements_and_lossy_auction_deliveries_are_worker_independent() -> Result<()> {
    for seed in [12345, 42] {
        let serial = observation_history(1, seed, 0.65)?;
        for workers in [2, 8] {
            assert_eq!(serial, observation_history(workers, seed, 0.65)?);
        }
        assert_ne!(serial, observation_history(1, seed, 1.0)?);
    }
    assert_ne!(
        observation_history(1, 12345, 0.65)?,
        observation_history(1, 42, 0.65)?
    );
    Ok(())
}

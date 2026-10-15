//! Consumers for the built-in contact sensor and auction; all use public plugin APIs.
use std::path::PathBuf;

use anyhow::Result;
use ripple_core::plugin::autonomy::{
    AuctionBid, AuctionResult, AuctionStart, BID_AUCTION_TOPIC, RESULT_AUCTION_TOPIC,
    START_AUCTION_TOPIC,
};
use ripple_core::plugin::{
    AgentContext, Autonomy, ContactList, Frame, Interaction, InteractionContext, Plugin, PluginIo,
    PluginParams, PluginRegistry, Port, Ports, Sensor, SensorContext, Unit, Update,
};
use ripple_core::{Mission, Params, Simulation};
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

/// Moves at the believed x position of the first contact it knows (tracking's
/// belief, from NoisyContacts).
impl Autonomy for ContactFollower {
    fn step(&mut self, context: &mut AgentContext<'_>, io: &mut PluginIo) -> Result<Update> {
        let mut velocity_mps = 0.0;
        if self.follow
            && context.entity.id == 1
            && let Some(contact) = context.contacts.iter().next()
        {
            velocity_mps = contact.state.position_world_m.x;
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

/// What the observer has received so far, as its reading.
struct ObserverReading {
    trace: AuctionTrace,
}

/// Records what this entity received, so tests can inspect it after each step.
/// Messages arrive one tick after publication, like any other consumer.
struct Observer {
    trace: AuctionTrace,
}

impl Plugin for Observer {
    type Config = ();
    fn configure(params: &PluginParams<'_>) -> Result<()> {
        params.parse()
    }
    fn new(_: &()) -> Self {
        Self {
            trace: AuctionTrace::default(),
        }
    }
}

impl Sensor for Observer {
    fn initialize(&mut self, context: &mut SensorContext<'_>) -> Result<()> {
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
        context.publish_reading(ObserverReading {
            trace: self.trace.clone(),
        })?;
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

/// One override per agent for a field all three share, such as
/// `autonomy.AuctionAssign.auction_duration_s`.
fn every_agent(field: &str, value: &str) -> Vec<(String, String)> {
    ["first", "second", "third"]
        .map(|agent| (format!("entities.{agent}.{field}"), value.to_owned()))
        .to_vec()
}

fn simulation(workers: usize, overrides: &Params) -> Result<Simulation> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut registry = PluginRegistry::with_builtins();
    registry.register_autonomy::<ContactFollower>("ContactFollower")?;
    registry.register_sensor::<Observer>("Observer")?;
    registry.register_interaction::<RemoveContacts>("RemoveContacts")?;
    let path = root.join("crates/core/tests/fixtures/perception_communication.yaml");
    let scenario = Mission::load(&path, &root, overrides)?.scenario;
    Simulation::new(scenario, &registry, workers)
}

/// The newest NoisyContacts reading this entity has, if any yet.
fn contacts(simulation: &Simulation, entity_index: usize) -> Option<&ContactList> {
    simulation.entities()[entity_index]
        .readings()
        .get::<ContactList>("NoisyContacts:0")
        .unwrap()
        .map(|reading| &reading.value)
}

fn observed(simulation: &Simulation, entity_index: usize) -> &ObserverReading {
    &simulation.entities()[entity_index]
        .readings()
        .get::<ObserverReading>("Observer:0")
        .unwrap()
        .unwrap()
        .value
}

fn trace(simulation: &Simulation, entity_index: usize) -> &AuctionTrace {
    &observed(simulation, entity_index).trace
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
    let mut simulation = simulation(
        2,
        &Params::from([(
            "networks.SphereNetwork.transmit_probability".into(),
            "0".into(),
        )]),
    )?;
    while simulation.step()?.is_some() {}
    for index in 0..3 {
        assert_eq!(trace(&simulation, index), &AuctionTrace::default());
    }
    Ok(())
}

#[test]
fn an_auction_can_close_without_a_bid_and_does_not_reopen_for_late_bids() -> Result<()> {
    let overrides = every_agent("autonomy.AuctionAssign.auction_duration_s", "0");
    let mut simulation = simulation(2, &overrides.into_iter().collect())?;
    while simulation.step()?.is_some() {}
    let first = trace(&simulation, 0);
    assert_eq!(first.results, vec![(0.25, AuctionResult { winner: None })]);
    assert_eq!(first.bids.len(), 2);
    Ok(())
}

#[test]
fn contact_feedback_uses_local_measurements_on_the_next_tick_without_changing_belief() -> Result<()>
{
    let overrides = every_agent("autonomy.ContactFollower.follow", "true");
    let mut simulation = simulation(8, &overrides.into_iter().collect())?;
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
        assert_eq!(sample.len(), 2);
        assert!(sample.iter().all(|contact| contact.id != entity.id()));
    }
    Ok(())
}

#[test]
fn contacts_include_new_spawns_and_clear_removed_targets() -> Result<()> {
    let mut simulation = simulation(
        2,
        &Params::from([
            ("entities.second.spawn.start_s".into(), "0.5".into()),
            ("entities.third.spawn.start_s".into(), "0.5".into()),
            ("interactions.RemoveContacts.remove_at_s".into(), "1".into()),
        ]),
    )?;
    let mut counts = Vec::new();
    while simulation.step()?.is_some() {
        if let Some(sample) = contacts(&simulation, 0) {
            counts.push(sample.len());
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
        &every_agent("sensors.NoisyContacts.position_stddev_m", "[0.5, 0, 0]")
            .into_iter()
            .chain([
                ("run.seed".into(), seed.to_string()),
                (
                    "networks.SphereNetwork.transmit_probability".into(),
                    probability.to_string(),
                ),
            ])
            .collect(),
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

//! The estimation contract: decisions see belief only, the perfect default makes
//! belief equal truth at the next decision, and estimators never touch truth.
#[path = "support/autonomy.rs"]
mod autonomy;
#[path = "support/controller.rs"]
mod controller;
#[path = "support/motion.rs"]
mod motion;
#[path = "support/sensor.rs"]
#[allow(dead_code)]
mod sensor;

use std::collections::BTreeMap;
use std::sync::Mutex;

use anyhow::Result;
use ripple_core::plugin::*;
use ripple_core::{
    EntityGroupConfig, KinematicState, PluginConfig, RunConfig, ScenarioConfig, Simulation,
    SimulationFrame, Spawn, Vec3,
};
use serde::Deserialize;
use serde_json::json;

/// What one decision plugin saw: own belief and known contacts, by entity ID.
#[derive(Clone, Debug)]
struct Seen {
    time_s: f64,
    entity_id: i32,
    own: String,
    contacts: BTreeMap<i32, String>,
}

// Test plugins can't hand data back through the engine, so they log here, one
// list per `log` name (tests run in parallel).
static SEEN: Mutex<BTreeMap<String, Vec<Seen>>> = Mutex::new(BTreeMap::new());
/// Per tick: substep count, the speed read on each substep, and each substep's dt.
type CommandEntry = (usize, Vec<f64>, Vec<f64>);
static COMMANDS: Mutex<BTreeMap<String, Vec<CommandEntry>>> = Mutex::new(BTreeMap::new());
/// The own-state x position a controller saw on each call.
static POSITIONS: Mutex<BTreeMap<String, Vec<f64>>> = Mutex::new(BTreeMap::new());

fn state(state: &KinematicState) -> String {
    format!("{state:?}")
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct LogConfig {
    log: String,
}

/// Logs what autonomy sees at initialization and each step; writes nothing.
struct Witness {
    log: String,
}
impl Plugin for Witness {
    type Config = LogConfig;
    fn configure(params: &PluginParams<'_>) -> Result<LogConfig> {
        params.parse()
    }
    fn new(config: &LogConfig) -> Self {
        Self {
            log: config.log.clone(),
        }
    }
}
impl Witness {
    fn record(&self, context: &AgentContext<'_>) {
        let seen = Seen {
            time_s: context.time.time_s,
            entity_id: context.entity.id,
            own: state(context.state),
            contacts: context
                .contacts
                .iter()
                .map(|contact| (contact.id, state(&contact.state)))
                .collect(),
        };
        SEEN.lock()
            .unwrap()
            .entry(self.log.clone())
            .or_default()
            .push(seen);
    }
}
impl Autonomy for Witness {
    fn initialize(&mut self, context: &mut AgentContext<'_>) -> Result<()> {
        self.record(context);
        Ok(())
    }
    fn step(&mut self, context: &mut AgentContext<'_>, _: &mut PluginIo) -> Result<Update> {
        self.record(context);
        Ok(Update::Applied)
    }
}

/// Localization that copies the perfect state and logs the commands it was given.
struct CommandLog {
    log: String,
}
impl Plugin for CommandLog {
    type Config = LogConfig;
    fn configure(params: &PluginParams<'_>) -> Result<LogConfig> {
        params.parse()
    }
    fn new(config: &LogConfig) -> Self {
        Self {
            log: config.log.clone(),
        }
    }
}
impl Localization for CommandLog {
    fn inputs(_: &LogConfig) -> Vec<Input> {
        vec![Input::new("state", ReadingKind::State)]
    }
    fn initialize(&mut self, context: &mut LocalizationContext<'_>) -> Result<()> {
        assert_eq!(context.commands.substeps(), 0);
        self.step(context).map(|_| ())
    }
    fn step(&mut self, context: &mut LocalizationContext<'_>) -> Result<Update> {
        let commands = &context.commands;
        let speeds = (0..commands.substeps())
            .map(|substep| commands.get(substep, "speed"))
            .collect::<Result<Vec<_>>>()?;
        let dts = (0..commands.substeps())
            .map(|substep| commands.time(substep).dt_s)
            .collect();
        COMMANDS
            .lock()
            .unwrap()
            .entry(self.log.clone())
            .or_default()
            .push((commands.substeps(), speeds, dts));
        if let Some(reading) = context.inputs.latest::<KinematicState>("state")? {
            *context.state = reading.value.clone();
        }
        Ok(Update::Applied)
    }
}

/// Localization that sends itself a message each tick and logs what arrives.
struct Echo {
    log: String,
}
impl Plugin for Echo {
    type Config = LogConfig;
    fn configure(params: &PluginParams<'_>) -> Result<LogConfig> {
        params.parse()
    }
    fn new(config: &LogConfig) -> Self {
        Self {
            log: config.log.clone(),
        }
    }
}
impl Localization for Echo {
    fn inputs(_: &LogConfig) -> Vec<Input> {
        vec![Input::new("state", ReadingKind::State)]
    }
    fn initialize(&mut self, context: &mut LocalizationContext<'_>) -> Result<()> {
        context.messages.subscribe::<f64>("LocalNetwork", "echo")?;
        self.step(context).map(|_| ())
    }
    fn step(&mut self, context: &mut LocalizationContext<'_>) -> Result<Update> {
        let received: Vec<f64> = context
            .messages
            .receive::<f64>("LocalNetwork", "echo")?
            .iter()
            .map(|message| *message.value)
            .collect();
        COMMANDS
            .lock()
            .unwrap()
            .entry(self.log.clone())
            .or_default()
            .push((received.len(), received, Vec::new()));
        context
            .messages
            .publish("LocalNetwork", "echo", context.time.time_s)?;
        if let Some(reading) = context.inputs.latest::<KinematicState>("state")? {
            *context.state = reading.value.clone();
        }
        Ok(Update::Applied)
    }
}

/// Localization that asks its state input for the wrong type of reading.
struct WrongType;
impl Plugin for WrongType {
    type Config = ();
    fn configure(params: &PluginParams<'_>) -> Result<()> {
        params.parse()
    }
    fn new(_: &()) -> Self {
        Self
    }
}
impl Localization for WrongType {
    fn inputs(_: &()) -> Vec<Input> {
        vec![Input::new("state", ReadingKind::State)]
    }
    fn initialize(&mut self, context: &mut LocalizationContext<'_>) -> Result<()> {
        context.inputs.latest::<ContactList>("state")?;
        Ok(())
    }
    fn step(&mut self, _: &mut LocalizationContext<'_>) -> Result<Update> {
        Ok(Update::Applied)
    }
}

/// Autonomy whose initialization panics.
struct PanicOnStart;
impl Plugin for PanicOnStart {
    type Config = ();
    fn configure(params: &PluginParams<'_>) -> Result<()> {
        params.parse()
    }
    fn new(_: &()) -> Self {
        Self
    }
}
impl Autonomy for PanicOnStart {
    fn initialize(&mut self, _: &mut AgentContext<'_>) -> Result<()> {
        panic!("intentional initialization panic");
    }
    fn step(&mut self, _: &mut AgentContext<'_>, _: &mut PluginIo) -> Result<Update> {
        Ok(Update::Applied)
    }
}

/// Passes `speed` through and logs the own-state x position it saw.
struct PositionWitness {
    log: String,
}
impl Plugin for PositionWitness {
    type Config = LogConfig;
    fn configure(params: &PluginParams<'_>) -> Result<LogConfig> {
        params.parse()
    }
    fn new(config: &LogConfig) -> Self {
        Self {
            log: config.log.clone(),
        }
    }
    fn ports(_: &LogConfig) -> Ports {
        let speed = Port::new("speed", Unit::MetersPerSecond, Frame::World);
        Ports::default().input(speed.clone()).output(speed)
    }
}
impl Controller for PositionWitness {
    fn step(&mut self, context: &mut AgentContext<'_>, io: &mut PluginIo) -> Result<Update> {
        POSITIONS
            .lock()
            .unwrap()
            .entry(self.log.clone())
            .or_default()
            .push(context.state.position_world_m.x);
        io.write("speed", io.read("speed")?)?;
        Ok(Update::Applied)
    }
}

fn registry() -> Result<PluginRegistry> {
    let mut registry = PluginRegistry::with_builtins();
    registry.register_autonomy::<Witness>("Witness")?;
    registry.register_autonomy::<autonomy::DriveToGoal>("DriveToGoal")?;
    registry.register_controller::<controller::ScaleSpeed>("ScaleSpeed")?;
    registry.register_controller::<PositionWitness>("PositionWitness")?;
    registry.register_motion::<motion::EastwardMotion>("EastwardMotion")?;
    registry.register_localization::<CommandLog>("CommandLog")?;
    registry.register_localization::<Echo>("Echo")?;
    registry.register_localization::<WrongType>("WrongType")?;
    registry.register_autonomy::<PanicOnStart>("PanicOnStart")?;
    Ok(registry)
}

fn mover(
    label: &str,
    team: i32,
    position_m: Vec3,
    velocity: [f64; 3],
    log: &str,
) -> Result<EntityGroupConfig> {
    Ok(EntityGroupConfig {
        label: label.into(),
        team,
        position_m,
        autonomy: vec![
            PluginConfig::new("ConstantVelocity").with_params(json!({ "velocity": velocity }))?,
            PluginConfig::new("Witness").with_params(json!({ "log": log }))?,
        ],
        motion_model: PluginConfig::new("SingleIntegrator"),
        ..EntityGroupConfig::default()
    })
}

/// Two entities collide and are removed at about 0.5 s, one stands still, and
/// one spawns at 1 s.
fn population(log: &str) -> Result<ScenarioConfig> {
    let mut late = mover("late", 4, Vec3::new(0.0, 200.0, 0.0), [0.0, 0.0, 0.0], log)?;
    late.spawn = Some(Spawn {
        rate_hz: 1.0,
        batch_size: 1,
        start_s: 1.0,
        time_stddev_s: 0.0,
    });
    Ok(ScenarioConfig {
        run: RunConfig {
            end_s: 2.0,
            seed: 7,
            ..RunConfig::default()
        },
        interactions: vec![PluginConfig::new("SimpleCollision")],
        networks: vec![PluginConfig::new("GlobalNetwork")],
        entities: vec![
            mover("east", 1, Vec3::new(0.0, 0.0, 0.0), [10.0, 0.0, 0.0], log)?,
            mover("west", 2, Vec3::new(10.0, 0.0, 0.0), [-10.0, 0.0, 0.0], log)?,
            mover("still", 3, Vec3::new(0.0, 100.0, 0.0), [0.0, 0.0, 0.0], log)?,
            late,
        ],
        ..ScenarioConfig::default()
    })
}

fn run(scenario: ScenarioConfig, workers: usize) -> Result<(Vec<SimulationFrame>, Simulation)> {
    let registry = registry()?;
    let mut simulation = Simulation::new(scenario, &registry, workers)?;
    let mut frames = Vec::new();
    while let Some(frame) = simulation.step()? {
        frames.push(frame);
    }
    Ok((frames, simulation))
}

fn seen(log: &str) -> Vec<Seen> {
    SEEN.lock().unwrap().remove(log).unwrap_or_default()
}

#[test]
fn perfect_belief_equals_truth_at_every_decision_through_collisions_and_spawns() -> Result<()> {
    let (frames, simulation) = run(population("perfect")?, 1)?;
    let seen = seen("perfect");
    let dt_s = 0.1;
    assert!(
        simulation
            .events()
            .iter()
            .any(|event| event.kind == ripple_core::EventKind::NonTeamCollision),
        "the scenario must include a collision"
    );

    // Initialization: every entity already knows the whole initial population.
    let initial: Vec<_> = seen.iter().filter(|seen| seen.time_s < 0.0).collect();
    assert_eq!(initial.len(), 3);
    for entry in &initial {
        assert_eq!(entry.contacts.len(), 2, "{entry:?}");
    }

    // Each step's frame is the state its decisions ran on.
    let mut checked = 0;
    for frame in &frames {
        let time_s = frame.time_s - dt_s;
        let existed_before: Vec<_> = seen
            .iter()
            .filter(|entry| entry.time_s < time_s - 1e-9)
            .map(|entry| entry.entity_id)
            .collect();
        for entry in seen
            .iter()
            .filter(|entry| entry.time_s + dt_s == frame.time_s)
        {
            let truth = |id: i32| {
                frame
                    .entities
                    .iter()
                    .find(|entity| entity.id == id)
                    .map(|entity| state(&entity.truth))
            };
            assert_eq!(Some(entry.own.clone()), truth(entry.entity_id), "{entry:?}");
            // Known contacts: everyone in the frame except itself and anyone
            // spawned this tick, which nobody has measured yet.
            let expected: BTreeMap<_, _> = frame
                .entities
                .iter()
                .filter(|entity| {
                    entity.id != entry.entity_id && existed_before.contains(&entity.id)
                })
                .map(|entity| (entity.id, state(&entity.truth)))
                .collect();
            assert_eq!(entry.contacts, expected, "{entry:?}");
            checked += 1;
        }
    }
    assert!(checked > 40, "checked {checked} decisions");
    Ok(())
}

#[test]
fn a_spawned_entity_is_unknown_to_others_until_their_next_measurement() -> Result<()> {
    let (_, _) = run(population("spawn")?, 2)?;
    let seen = seen("spawn");
    let late_id = seen
        .iter()
        .map(|entry| entry.entity_id)
        .max()
        .expect("entries");
    let first_s = seen
        .iter()
        .filter(|entry| entry.entity_id == late_id)
        .map(|entry| entry.time_s)
        .fold(f64::INFINITY, f64::min);
    for entry in seen.iter().filter(|entry| entry.entity_id != late_id) {
        let knows = entry.contacts.contains_key(&late_id);
        assert_eq!(knows, entry.time_s > first_s + 1e-9, "{entry:?}");
    }
    // The new entity itself starts from the latest perfect measurement.
    let first = seen
        .iter()
        .find(|entry| entry.entity_id == late_id)
        .expect("spawned entity decided");
    assert!(!first.contacts.is_empty());
    Ok(())
}

#[test]
fn decisions_and_belief_are_worker_independent() -> Result<()> {
    let history = |workers: usize| -> Result<String> {
        let log = format!("workers-{workers}");
        run(population(&log)?, workers)?;
        let mut seen = seen(&log);
        seen.sort_by(|a, b| {
            a.time_s
                .total_cmp(&b.time_s)
                .then(a.entity_id.cmp(&b.entity_id))
        });
        Ok(format!("{seen:?}"))
    };
    let serial = history(1)?;
    assert_eq!(serial, history(2)?);
    assert_eq!(serial, history(8)?);
    Ok(())
}

fn noisy_state(instance: &str, altitude_bias_m: f64) -> Result<PluginConfig> {
    let mut config = PluginConfig::new("NoisyState").with_params(json!({
        "pos_noise_0": [0, 0], "pos_noise_1": [0, 0], "pos_noise_2": [altitude_bias_m, 0],
        "vel_noise_0": [0, 0], "vel_noise_1": [0, 0], "vel_noise_2": [0, 0],
    }))?;
    config.instance = Some(instance.into());
    Ok(config)
}

#[test]
fn a_named_input_selects_its_sensor_and_ambiguity_is_an_error() -> Result<()> {
    let scenario = |localization: PluginConfig| -> Result<ScenarioConfig> {
        let mut entity = mover(
            "one",
            1,
            Vec3::new(0.0, 0.0, 50.0),
            [1.0, 0.0, 0.0],
            "unused",
        )?;
        entity.sensors = vec![noisy_state("coarse", 5.0)?, noisy_state("fine", 0.0)?];
        entity.localization = localization;
        Ok(ScenarioConfig {
            run: RunConfig {
                end_s: 0.5,
                ..RunConfig::default()
            },
            // NoisyState also publishes on LocalNetwork, as in C++.
            networks: vec![PluginConfig::new("LocalNetwork")],
            entities: vec![entity],
            ..ScenarioConfig::default()
        })
    };
    let registry = registry()?;
    let error = Simulation::new(scenario(PluginConfig::new("PassThrough"))?, &registry, 1)
        .err()
        .expect("two state sensors and no input named");
    assert!(format!("{error:#}").contains("coarse, fine"), "{error:#}");
    for (sensor, bias_m) in [("coarse", 5.0), ("fine", 0.0)] {
        let (_, simulation) = run(
            scenario(PluginConfig::new("PassThrough").input("state", sensor))?,
            1,
        )?;
        let entity = &simulation.entities()[0];
        let error_m = entity.belief().position_world_m.z - entity.truth().position_world_m.z;
        assert_eq!(error_m, bias_m, "{sensor}");
    }
    Ok(())
}

#[test]
fn trackers_are_per_entity_and_leave_truth_alone() -> Result<()> {
    // Team 1 tracks from a biased contact sensor; team 2 keeps perfect tracking.
    let mut biased = mover(
        "biased",
        1,
        Vec3::new(0.0, 0.0, 0.0),
        [0.0, 0.0, 0.0],
        "unused",
    )?;
    biased.sensors = vec![PluginConfig::new("NoisyContacts").with_params(json!({
        "pos_noise_0": [3, 0], "pos_noise_1": [0, 0], "pos_noise_2": [0, 0],
        "vel_noise_0": [0, 0], "vel_noise_1": [0, 0], "vel_noise_2": [0, 0],
    }))?];
    let perfect = mover(
        "perfect",
        2,
        Vec3::new(50.0, 0.0, 0.0),
        [1.0, 0.0, 0.0],
        "unused",
    )?;
    let scenario = ScenarioConfig {
        run: RunConfig {
            end_s: 0.5,
            ..RunConfig::default()
        },
        networks: vec![PluginConfig::new("LocalNetwork")],
        entities: vec![biased, perfect],
        ..ScenarioConfig::default()
    };
    let (frames, simulation) = run(scenario, 2)?;
    let [biased, perfect] = simulation.entities() else {
        panic!("two entities");
    };
    let own = |entity: &ripple_core::Entity, id: i32| {
        entity
            .contacts_belief()
            .iter()
            .find(|contact| contact.id == id)
            .map(|contact| contact.state.position_world_m)
            .expect("contact known")
    };
    // The biased entity's belief about the other is 3 m off; the other's is exact.
    let offset = own(biased, perfect.id()) - perfect.truth().position_world_m;
    assert_eq!(offset, Vec3::new(3.0, 0.0, 0.0));
    assert_eq!(own(perfect, biased.id()), biased.truth().position_world_m);
    // Nothing the tracker believes changes where anything actually is.
    let last = frames.last().expect("frames");
    assert_eq!(last.entities[0].truth.position_world_m, Vec3::zeros());
    Ok(())
}

#[test]
fn localization_receives_what_the_motion_model_read_on_each_substep() -> Result<()> {
    let mut entity = EntityGroupConfig {
        label: "driver".into(),
        team: 1,
        autonomy: vec![
            PluginConfig::new("DriveToGoal").with_params(json!({ "goal_world_m": 1000.0 }))?,
        ],
        // The autonomy asks for 2 m/s; the controller triples it.
        controller: vec![PluginConfig::new("ScaleSpeed").with_params(json!({ "gain": 3.0 }))?],
        motion_model: PluginConfig::new("EastwardMotion"),
        ..EntityGroupConfig::default()
    };
    entity.localization =
        PluginConfig::new("CommandLog").with_params(json!({ "log": "commands" }))?;
    let scenario = ScenarioConfig {
        run: RunConfig {
            end_s: 0.3,
            motion_multiplier: 2,
            ..RunConfig::default()
        },
        entities: vec![entity],
        ..ScenarioConfig::default()
    };
    run(scenario, 1)?;
    let log = COMMANDS.lock().unwrap().remove("commands").expect("logged");
    let (initial, steps) = log.split_first().expect("entries");
    assert_eq!(initial.0, 0);
    assert!(!steps.is_empty());
    for (substeps, speeds, dts) in steps {
        assert_eq!(*substeps, 2);
        assert_eq!(speeds, &vec![6.0, 6.0]);
        assert_eq!(dts, &vec![0.05, 0.05]);
    }
    Ok(())
}

/// Controllers seen on each of four substeps per tick, at 2 m/s eastward.
fn substep_positions(localization: Option<PluginConfig>, log: &str) -> Result<Vec<f64>> {
    let mut entity = EntityGroupConfig {
        label: "driver".into(),
        team: 1,
        autonomy: vec![
            PluginConfig::new("DriveToGoal").with_params(json!({ "goal_world_m": 1000.0 }))?,
        ],
        controller: vec![PluginConfig::new("PositionWitness").with_params(json!({ "log": log }))?],
        motion_model: PluginConfig::new("EastwardMotion"),
        ..EntityGroupConfig::default()
    };
    if let Some(localization) = localization {
        entity.localization = localization;
    }
    let scenario = ScenarioConfig {
        run: RunConfig {
            end_s: 0.2,
            motion_multiplier: 4,
            ..RunConfig::default()
        },
        entities: vec![entity],
        ..ScenarioConfig::default()
    };
    run(scenario, 1)?;
    Ok(POSITIONS.lock().unwrap().remove(log).expect("logged"))
}

#[test]
fn controllers_see_the_previous_substep_with_a_perfect_own_state() -> Result<()> {
    // Controller, then motion, on each substep: each call sees 2 m/s * 0.025 s more.
    let positions = substep_positions(None, "perfect-substeps")?;
    assert_eq!(positions.len(), 8);
    for (index, pair) in positions.windows(2).enumerate() {
        if index % 4 != 3 {
            assert!((pair[1] - pair[0] - 0.05).abs() < 1e-12, "{positions:?}");
        }
    }
    Ok(())
}

#[test]
fn other_localization_updates_controllers_once_per_tick() -> Result<()> {
    let echo = PluginConfig::new("CommandLog").with_params(json!({ "log": "unused" }))?;
    let positions = substep_positions(Some(echo), "estimated-substeps")?;
    COMMANDS.lock().unwrap().remove("unused");
    for tick in positions.chunks(4) {
        assert!(tick.iter().all(|x| *x == tick[0]), "{positions:?}");
    }
    Ok(())
}

#[test]
fn estimators_run_every_tick() -> Result<()> {
    let mut entity = mover("slow", 1, Vec3::zeros(), [0.0, 0.0, 0.0], "unused")?;
    entity.tracking.loop_rate_hz = 1.0;
    let scenario = ScenarioConfig {
        entities: vec![entity],
        ..ScenarioConfig::default()
    };
    let error = Simulation::new(scenario, &registry()?, 1)
        .err()
        .expect("an estimator loop_rate is rejected");
    assert!(
        format!("{error:#}").contains("takes no loop_rate"),
        "{error:#}"
    );
    Ok(())
}

#[test]
fn estimators_send_and_receive_messages() -> Result<()> {
    let mut entity = mover("echo", 1, Vec3::zeros(), [0.0, 0.0, 0.0], "unused")?;
    entity.localization = PluginConfig::new("Echo").with_params(json!({ "log": "echo" }))?;
    let scenario = ScenarioConfig {
        run: RunConfig {
            end_s: 0.5,
            ..RunConfig::default()
        },
        networks: vec![PluginConfig::new("LocalNetwork")],
        entities: vec![entity],
        ..ScenarioConfig::default()
    };
    run(scenario, 1)?;
    let log = COMMANDS.lock().unwrap().remove("echo").expect("logged");
    // Published at startup or in one estimation step, routed by the next tick's
    // network phase, and read in that tick's estimation step.
    let counts: Vec<_> = log.iter().map(|(count, _, _)| *count).collect();
    assert_eq!(counts[0], 0, "nothing has arrived at startup");
    assert!(counts[1..].iter().all(|count| *count == 1), "{counts:?}");
    Ok(())
}

#[test]
fn a_perfect_input_gives_only_its_declared_kind() -> Result<()> {
    let mut entity = mover("wrong", 1, Vec3::zeros(), [0.0, 0.0, 0.0], "unused")?;
    entity.localization = PluginConfig::new("WrongType");
    let scenario = ScenarioConfig {
        entities: vec![entity],
        ..ScenarioConfig::default()
    };
    let error = Simulation::new(scenario, &registry()?, 1)
        .err()
        .expect("asking a state input for contacts fails");
    assert!(
        format!("{error:#}").contains("gives State readings"),
        "{error:#}"
    );
    Ok(())
}

#[test]
fn a_panic_while_starting_an_entity_is_an_error() -> Result<()> {
    let mut entity = mover("panics", 1, Vec3::zeros(), [0.0, 0.0, 0.0], "unused")?;
    entity.autonomy.push(PluginConfig::new("PanicOnStart"));
    let scenario = ScenarioConfig {
        entities: vec![entity],
        ..ScenarioConfig::default()
    };
    let error = Simulation::new(scenario, &registry()?, 1)
        .err()
        .expect("the panic becomes an error");
    assert!(
        format!("{error:#}").contains("initialization panicked"),
        "{error:#}"
    );
    Ok(())
}

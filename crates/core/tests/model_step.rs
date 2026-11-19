//! Each motion model's public `advance` is the step the engine runs: a plugin
//! that predicts with it, from the engine's state and the same controls,
//! lands on the engine's next state exactly.
#[path = "support/held_rates.rs"]
mod held_rates;

use anyhow::Result;
use ripple_core::plugin::motion::{
    Aerosonde, AerosondeControl, Multirotor, SimpleAircraft, SimpleAircraftControl,
    SingleIntegrator,
};
use ripple_core::plugin::{MotionModel, PluginParams, PluginRegistry};
use ripple_core::{
    EntityGroupConfig, KinematicState, PluginConfig, RunConfig, ScenarioConfig, Simulation, Vec3,
};
use serde_json::json;
use std::sync::Arc;

/// One entity flown open loop by `driver`.
fn scenario(
    run: RunConfig,
    driver: PluginConfig,
    motion_model: PluginConfig,
    velocity_mps: Vec3,
) -> ScenarioConfig {
    let mut group = EntityGroupConfig {
        label: "vehicle".into(),
        team: 1,
        position_m: Vec3::new(0.0, 0.0, 100.0),
        velocity_mps,
        heading_deg: 30.0,
        motion_model,
        ..EntityGroupConfig::default()
    };
    if driver.name == "ConstantVelocity" {
        group.autonomy = vec![driver];
    } else {
        group.controller = vec![driver];
    }
    ScenarioConfig {
        run,
        entities: vec![group],
        ..ScenarioConfig::default()
    }
}

/// Every number of both states, bit for bit.
fn bits(state: &KinematicState) -> Vec<u64> {
    let q = state.orientation_world_from_body;
    state
        .position_world_m
        .iter()
        .chain(state.velocity_world_mps.iter())
        .chain(state.angular_velocity_world_radps.iter())
        .chain([q.w, q.x, q.y, q.z].iter())
        .map(|value| value.to_bits())
        .collect()
}

/// Runs the scenario, and beside it calls `advance` once per motion substep
/// from the engine's state after its first step.
fn advance_matches_the_engine(
    scenario: ScenarioConfig,
    mut advance: impl FnMut(&mut KinematicState, f64) -> Result<()>,
) -> Result<()> {
    let substeps = scenario.run.motion_multiplier;
    let dt_s = scenario.run.dt_s / substeps as f64;
    let mut registry = PluginRegistry::with_builtins();
    registry.register_controller::<held_rates::HeldRates>("HeldRates")?;
    let mut simulation = Simulation::new(scenario, Arc::new(registry))?;
    simulation.step()?;
    let mut predicted = *simulation.entities()[0].truth();
    let mut steps = 0;
    // Once the run has ended, the last call to `step` only emits the final
    // frame; no motion runs in it.
    while simulation.termination().is_none() {
        simulation.step()?;
        for _ in 0..substeps {
            advance(&mut predicted, dt_s)?;
        }
        let truth = simulation.entities()[0].truth();
        assert_eq!(
            bits(&predicted),
            bits(truth),
            "step {steps}: predicted {predicted:?}, engine {truth:?}"
        );
        steps += 1;
    }
    assert!(steps >= 20, "only {steps} steps compared");
    Ok(())
}

#[test]
fn single_integrator_advance_is_the_engines_step() -> Result<()> {
    let motion_model = PluginConfig::new("SingleIntegrator").with_params(json!({
        "max_speed_mps": 2.5,
    }))?;
    let config = SingleIntegrator::configure(&PluginParams::new(&motion_model))?;
    let driver = PluginConfig::new("ConstantVelocity").with_params(json!({
        "velocity_mps": [3.0, 4.0, 1.0],
    }))?;
    let run = RunConfig {
        end_s: 3.0,
        motion_multiplier: 2,
        ..RunConfig::default()
    };
    advance_matches_the_engine(
        scenario(run, driver, motion_model, Vec3::zeros()),
        |state, dt_s| config.advance(state, Vec3::new(3.0, 4.0, 1.0), dt_s),
    )
}

#[test]
fn simple_aircraft_advance_is_the_engines_step() -> Result<()> {
    let motion_model = PluginConfig::new("SimpleAircraft").with_params(json!({
        "airframe": "rascal",
    }))?;
    let config = SimpleAircraft::configure(&PluginParams::new(&motion_model))?;
    let control = SimpleAircraftControl {
        throttle: 4.0,
        roll_rate_radps: 0.2,
        pitch_rate_radps: -0.05,
    };
    let driver = PluginConfig::new("HeldRates").with_params(json!({
        "throttle": control.throttle,
        "roll_rate_radps": control.roll_rate_radps,
        "pitch_rate_radps": control.pitch_rate_radps,
    }))?;
    // Long enough for the roll, climb, and speed to reach their limits.
    let run = RunConfig {
        end_s: 12.0,
        motion_multiplier: 2,
        ..RunConfig::default()
    };
    advance_matches_the_engine(
        scenario(run, driver, motion_model, Vec3::new(20.0, 0.0, 0.0)),
        |state, dt_s| config.advance(state, control, dt_s),
    )
}

#[test]
fn aerosonde_advance_is_the_engines_step() -> Result<()> {
    let motion_model = PluginConfig::new("Aerosonde").with_params(json!({
        "airframe": "aerosonde",
        "wind_mps": [2.0, -1.0, 0.0],
    }))?;
    let config = Aerosonde::configure(&PluginParams::new(&motion_model))?;
    // Throttle above 1 exercises the clamp inside the step.
    let control = AerosondeControl {
        throttle: 1.2,
        elevator_rad: -0.12,
        aileron_rad: 0.01,
        rudder_rad: -0.002,
    };
    let driver = PluginConfig::new("HeldControls").with_params(json!({
        "throttle": control.throttle,
        "elevator_rad": control.elevator_rad,
        "aileron_rad": control.aileron_rad,
        "rudder_rad": control.rudder_rad,
    }))?;
    let run = RunConfig {
        end_s: 2.0,
        dt_s: 0.01,
        ..RunConfig::default()
    };
    advance_matches_the_engine(
        scenario(run, driver, motion_model, Vec3::new(25.0, 0.0, 0.0)),
        |state, dt_s| config.advance(state, control, dt_s),
    )
}

#[test]
fn multirotor_advance_is_the_engines_step() -> Result<()> {
    // Motor lag and rotor drag on, so the carried shaft speeds matter.
    let motion_model = PluginConfig::new("Multirotor").with_params(json!({
        "airframe": "crazyflie2",
        "motor_time_constant_up_s": 0.0647,
        "motor_time_constant_down_s": 0.0647,
        "rotor_drag_coefficient": 9.1785e-7,
    }))?;
    let config = Multirotor::configure(&PluginParams::new(&motion_model))?;
    let hover_radps = (config.airframe().mass_kg * config.airframe().gravity_mps2
        / (4.0 * config.airframe().thrust_coefficient))
        .sqrt();
    let commands_radps = [
        hover_radps * 1.02,
        hover_radps,
        hover_radps * 0.99,
        hover_radps * 1.01,
    ];
    let driver = PluginConfig::new("MotorSpeeds").with_params(json!({
        "speeds_radps": commands_radps,
    }))?;
    let run = RunConfig {
        end_s: 0.5,
        dt_s: 0.01,
        motion_multiplier: 5,
        ..RunConfig::default()
    };
    // Empty: each rotor starts at its command, as the engine's first step does.
    let mut rotor_speeds_radps = Vec::new();
    advance_matches_the_engine(
        scenario(run, driver, motion_model, Vec3::new(1.0, 0.5, 0.0)),
        |state, dt_s| config.advance(state, &mut rotor_speeds_radps, &commands_radps, dt_s),
    )
}

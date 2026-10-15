//! FixedWing6DOF's landing gear on the ArduPilot JSBSim Rascal's wheels: it
//! rests at the right height and attitude, survives a drop, holds on a gentle
//! slope and rolls down a steep one, stops a rolling aircraft by friction,
//! converges as the step shrinks, and refuses a step too long for its dampers.
use anyhow::Result;
use ripple_core::plugin::{
    AgentContext, Controller, Frame, Plugin, PluginIo, PluginParams, PluginRegistry, Port, Ports,
    Unit, Update,
};
use ripple_core::{
    EntityGroupConfig, PluginConfig, RunConfig, ScenarioConfig, Simulation, SimulationFrame, Vec3,
};
use serde_json::json;
use std::{fs, path::PathBuf};

/// The Rascal resting on its three wheels sits nose-up by this much: the
/// angle that puts the main and tail wheels at the same height.
fn rest_nose_up_deg() -> f64 {
    (0.434 - 0.203_f64).atan2(0.084 + 0.826).to_degrees()
}

/// The JSBSim Rascal's wheels (Rascal.xml in ArduPilot), from its centre of
/// gravity: forward, right, down. Its tail wheel castors; `steered_tail`
/// links it to the rudder instead.
fn rascal_gear(steered_tail: bool) -> serde_json::Value {
    let wheel = |position: [f64; 3], castered: bool, steered_by_rudder: bool| {
        json!({
            "position_frd_m": position,
            "spring_n_per_m": 7005.0,
            "damping_n_s_per_m": 1459.0,
            "static_friction": 0.8,
            "rolling_friction": 0.1,
            "castered": castered,
            "steered_by_rudder": steered_by_rudder,
        })
    };
    json!([
        wheel([0.084, -0.328, 0.434], false, false),
        wheel([0.084, 0.328, 0.434], false, false),
        wheel([-0.826, 0.0, 0.203], !steered_tail, steered_tail),
    ])
}

/// FixedWing6DOF's Rascal, as the SITL missions fly it, on `gear`.
fn rascal_motion(gear: serde_json::Value) -> serde_json::Value {
    json!({
        "airframe": "rascal",
        "enable_ground_spring": false,
        "landing_gear": gear,
    })
}

/// Zero throttle, centred elevator and aileron, and a fixed rudder (rad),
/// every step.
#[derive(serde::Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct FixedInputsConfig {
    rudder: f64,
}

struct FixedInputs {
    rudder_rad: f64,
}

impl Plugin for FixedInputs {
    type Config = FixedInputsConfig;
    fn configure(params: &PluginParams<'_>) -> Result<FixedInputsConfig> {
        params.parse()
    }
    fn new(config: &FixedInputsConfig) -> Self {
        Self {
            rudder_rad: config.rudder,
        }
    }
    fn ports(_config: &FixedInputsConfig) -> Ports {
        Ports::default()
            .output(Port::new("throttle", Unit::Dimensionless, Frame::None))
            .output(Port::new("elevator", Unit::Radians, Frame::Model))
            .output(Port::new("aileron", Unit::Radians, Frame::Model))
            .output(Port::new("rudder", Unit::Radians, Frame::Model))
    }
}

impl Controller for FixedInputs {
    fn step(&mut self, _context: &mut AgentContext<'_>, io: &mut PluginIo) -> Result<Update> {
        for port in ["throttle", "elevator", "aileron"] {
            io.write(port, 0.0)?;
        }
        io.write("rudder", self.rudder_rad)?;
        Ok(Update::Applied)
    }
}

/// One Rascal (the SITL missions' parameters) with zero throttle and centred
/// surfaces.
fn rascal(position_m: Vec3, nose_up_deg: f64, velocity_mps: Vec3) -> Result<EntityGroupConfig> {
    facing(position_m, 0.0, nose_up_deg, velocity_mps)
}

/// The same, facing `heading_deg` (0 is east).
fn facing(
    position_m: Vec3,
    heading_deg: f64,
    nose_up_deg: f64,
    velocity_mps: Vec3,
) -> Result<EntityGroupConfig> {
    Ok(EntityGroupConfig {
        label: "rascal".into(),
        team: 1,
        visual_model: "sphere".into(),
        position_m,
        heading_deg,
        // Mission pitch is positive nose-down.
        pitch_deg: -nose_up_deg,
        velocity_mps,
        controller: vec![PluginConfig::new("FixedInputs")],
        motion_model: PluginConfig::new("FixedWing6DOF")
            .with_params(rascal_motion(rascal_gear(false)))?,
        ..EntityGroupConfig::default()
    })
}

fn scenario(rascal: EntityGroupConfig, dt_s: f64, end_s: f64) -> ScenarioConfig {
    ScenarioConfig {
        run: RunConfig {
            dt_s,
            end_s,
            ..RunConfig::default()
        },
        entities: vec![rascal],
        networks: vec![
            PluginConfig::new("LocalNetwork"),
            PluginConfig::new("GlobalNetwork"),
        ],
        ..ScenarioConfig::default()
    }
}

/// Every frame of a run.
fn run(scenario: ScenarioConfig) -> Result<Vec<SimulationFrame>> {
    let mut registry = PluginRegistry::with_builtins();
    registry.register_controller::<FixedInputs>("FixedInputs")?;
    let mut simulation = Simulation::new(scenario, &registry, 1)?;
    let mut frames = Vec::new();
    while let Some(frame) = simulation.step()? {
        frames.push(frame);
    }
    Ok(frames)
}

fn height_m(frame: &SimulationFrame) -> f64 {
    frame.entities[0].truth.position_world_m.z
}

fn speed_mps(frame: &SimulationFrame) -> f64 {
    frame.entities[0].truth.velocity_world_mps.norm()
}

fn nose_up_deg(frame: &SimulationFrame) -> f64 {
    -frame.entities[0]
        .truth
        .orientation_world_from_body
        .pitch_world_from_body_rad()
        .to_degrees()
}

/// Where the centre of gravity sits with the wheels just touching flat ground.
fn touching_height_m() -> f64 {
    let pitch = rest_nose_up_deg().to_radians();
    0.434 * pitch.cos() - 0.084 * pitch.sin()
}

#[test]
fn rests_on_its_wheels_at_the_right_height_and_attitude() -> Result<()> {
    let start = Vec3::new(0.0, 0.0, touching_height_m());
    let frames = run(scenario(
        rascal(start, rest_nose_up_deg(), Vec3::zeros())?,
        0.0025,
        5.0,
    ))?;
    let last = frames.last().unwrap();
    // The weight compresses the struts a few millimetres: the mains carry
    // 52 N between them, 3.7 mm each on 7,005 N/m.
    let sag_m = touching_height_m() - height_m(last);
    assert!((0.002..0.006).contains(&sag_m), "sag {sag_m} m");
    assert!(
        (nose_up_deg(last) - rest_nose_up_deg()).abs() < 0.2,
        "{}",
        nose_up_deg(last)
    );
    assert!(
        speed_mps(last) < 1e-4,
        "still moving at {} m/s",
        speed_mps(last)
    );
    let drift_m = last.entities[0].truth.position_world_m.xy().norm();
    assert!(drift_m < 1e-3, "drifted {drift_m} m");
    Ok(())
}

#[test]
fn a_drop_lands_bounces_and_settles_to_the_same_rest() -> Result<()> {
    let start = Vec3::new(0.0, 0.0, touching_height_m() + 0.5);
    let frames = run(scenario(
        rascal(start, rest_nose_up_deg(), Vec3::zeros())?,
        0.0025,
        5.0,
    ))?;
    // A 0.5 m drop lands at 3.1 m/s; the struts stop it within a few centimetres.
    let lowest_m = frames.iter().map(height_m).fold(f64::INFINITY, f64::min);
    let compression_m = touching_height_m() - lowest_m;
    assert!((0.005..0.1).contains(&compression_m), "{compression_m} m");
    let last = frames.last().unwrap();
    let sag_m = touching_height_m() - height_m(last);
    assert!((0.002..0.006).contains(&sag_m), "sag {sag_m} m");
    assert!(speed_mps(last) < 1e-3, "{}", speed_mps(last));
    Ok(())
}

#[test]
fn rolling_friction_stops_a_rolling_aircraft_and_holds_it() -> Result<()> {
    let start = Vec3::new(0.0, 0.0, touching_height_m() - 0.004);
    let frames = run(scenario(
        rascal(start, rest_nose_up_deg(), Vec3::new(5.0, 0.0, 0.0))?,
        0.0025,
        12.0,
    ))?;
    let stopped = frames
        .iter()
        .position(|frame| speed_mps(frame) < 0.01)
        .expect("never stopped");
    // Rolling friction 0.1 of the load alone stops 5 m/s in 12.7 m, in 5.1 s;
    // drag shortens that a little, and lift lightens the load a little.
    let distance_m = frames[stopped].entities[0].truth.position_world_m.x;
    let time_s = frames[stopped].time_s;
    assert!((10.0..13.5).contains(&distance_m), "{distance_m} m");
    assert!((4.0..5.6).contains(&time_s), "{time_s} s");
    // Rolling straight: no sideways drift or turn.
    let last = frames.last().unwrap();
    assert!(last.entities[0].truth.position_world_m.y.abs() < 0.01);
    assert!(speed_mps(last) < 1e-3, "{}", speed_mps(last));
    assert!((last.entities[0].truth.position_world_m.x - distance_m).abs() < 0.02);
    Ok(())
}

/// A plane rising `rise` metres per metre east, as terrain.
fn slope(name: &str, rise: f32) -> Result<PathBuf> {
    let directory = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
    fs::create_dir_all(&directory)?;
    let row: Vec<f32> = (0..11).map(|column| rise * 20.0 * column as f32).collect();
    let bytes: Vec<u8> = [row.clone(), row.clone(), row]
        .concat()
        .iter()
        .flat_map(|height| height.to_le_bytes())
        .collect();
    fs::write(directory.join("slope.f32"), bytes)?;
    let header = directory.join("slope.terrain.yaml");
    fs::write(
        &header,
        "format_version: 1\nsource: test slope\n\
         origin: {latitude_deg: 0, longitude_deg: 0, altitude_m: 0}\n\
         west_m: -100\nnorth_m: 20\ncell_m: 20\ncolumns: 11\nrows: 3\n\
         heights: slope.f32\nno_data: -9999\n",
    )?;
    Ok(header)
}

/// Facing downhill (west) on a slope, set down at rest; how far it has
/// rolled east after `end_s`. (Rolling backwards would meet the air at 180
/// degrees of attack, where the model's linear lift and drag, with no stall,
/// brake it hard.)
fn rolled_on_slope(rise: f32, end_s: f64) -> Result<f64> {
    let terrain = slope(&format!("gear-slope-{rise}"), rise)?;
    // The ground at x = 0 is 100 m up the slope from the grid's west edge.
    let ground_m = f64::from(rise) * 100.0;
    let nose_up_deg = rest_nose_up_deg() - f64::from(rise).atan().to_degrees();
    let start = Vec3::new(0.0, 0.0, ground_m + touching_height_m() + 0.05);
    let rascal = facing(start, 180.0, nose_up_deg, Vec3::zeros())?;
    let mut scenario = scenario(rascal, 0.0025, end_s);
    scenario.terrain = Some(terrain);
    let frames = run(scenario)?;
    Ok(frames.last().unwrap().entities[0].truth.position_world_m.x)
}

#[test]
fn holds_on_a_gentle_slope_and_rolls_down_a_steep_one() -> Result<()> {
    // Rolling friction 0.1 holds against a slope of up to about 0.1.
    let gentle_m = rolled_on_slope(0.05, 4.0)?;
    assert!(gentle_m.abs() < 0.02, "rolled {gentle_m} m on 5%");
    // On 25% gravity along the slope, 0.24 g, beats friction, 0.1 g: it rolls
    // downhill (west) about 0.14 g x 3 s^2 / 2, 6 m; a little more as lift
    // lightens the wheels.
    let steep_m = rolled_on_slope(0.25, 3.0)?;
    assert!((-8.0..-5.5).contains(&steep_m), "rolled {steep_m} m on 25%");
    Ok(())
}

#[test]
fn a_drop_converges_as_the_step_shrinks() -> Result<()> {
    let lowest = |dt_s: f64| -> Result<f64> {
        let start = Vec3::new(0.0, 0.0, touching_height_m() + 0.5);
        let frames = run(scenario(
            rascal(start, rest_nose_up_deg(), Vec3::zeros())?,
            dt_s,
            1.0,
        ))?;
        Ok(frames.iter().map(height_m).fold(f64::INFINITY, f64::min))
    };
    // Against the finest step, the error shrinks with each halving. Contact
    // switching on and off, and JSBSim's step-based rules (the compression
    // speed cap and friction that stops a wheel within one step), make this
    // closer to first order than RK4's fourth, at millimetres.
    let steps = [0.0025, 0.00125, 0.000625, 0.0003125];
    let lowest_m = steps.map(|dt_s| lowest(dt_s).unwrap());
    let errors_m = lowest_m.map(|value| (value - lowest_m[3]).abs());
    assert!(
        errors_m[0] > errors_m[1] && errors_m[1] > errors_m[2],
        "{errors_m:?}"
    );
    assert!(errors_m[0] < 2e-3, "{errors_m:?}");
    Ok(())
}

#[test]
fn refuses_a_step_too_long_for_the_dampers() -> Result<()> {
    let start = Vec3::new(0.0, 0.0, touching_height_m());
    let rascal = rascal(start, rest_nose_up_deg(), Vec3::zeros())?;
    let Err(error) = run(scenario(rascal, 0.01, 1.0)) else {
        panic!("dt 0.01 s must be refused");
    };
    assert!(
        format!("{error:#}").contains("shorter motion step"),
        "{error:#}"
    );
    Ok(())
}

/// Dropped from 0.5 m onto flat terrain, rolled `roll_deg`, with
/// TerrainCollision checking the Rascal's nose (the propeller, from JSBSim's
/// file) and wingtips (half its 2.70 m span); whether it survives 3 s.
fn survives_drop(roll_deg: f64) -> Result<bool> {
    let terrain = slope(&format!("gear-flat-{roll_deg}"), 0.0)?;
    let start = Vec3::new(0.0, 0.0, touching_height_m() + 0.5);
    let mut rascal = rascal(start, rest_nose_up_deg(), Vec3::zeros())?;
    rascal.roll_deg = roll_deg;
    let mut scenario = scenario(rascal, 0.0025, 3.0);
    scenario.terrain = Some(terrain);
    scenario.interactions = vec![PluginConfig::new("TerrainCollision").with_params(json!({
        "structure_points_frd_m": [[0.899, 0.0, 0.102], [0.0, -1.35, 0.0], [0.0, 1.35, 0.0]],
    }))?];
    let frames = run(scenario)?;
    Ok(frames
        .last()
        .unwrap()
        .entities
        .iter()
        .any(|entity| entity.active))
}

#[test]
fn landing_on_the_wheels_is_not_a_crash_but_a_wingtip_strike_is() -> Result<()> {
    assert!(
        survives_drop(0.0)?,
        "a level drop onto the wheels was a crash"
    );
    // Rolled 40 degrees, a wingtip hits before the wheels can right it.
    assert!(!survives_drop(40.0)?, "a wingtip strike was not a crash");
    Ok(())
}

/// Rolling east at 5 m/s with the rudder at +0.2 rad (nose left); the
/// heading change after 2 s, with the tail wheel castered or rudder-steered.
fn turned_deg(steered: bool) -> Result<f64> {
    let start = Vec3::new(0.0, 0.0, touching_height_m() - 0.004);
    let mut rascal = rascal(start, rest_nose_up_deg(), Vec3::new(5.0, 0.0, 0.0))?;
    rascal.controller = vec![PluginConfig::new("FixedInputs").with_params(json!({"rudder": 0.2}))?];
    rascal.motion_model =
        PluginConfig::new("FixedWing6DOF").with_params(rascal_motion(rascal_gear(steered)))?;
    let frames = run(scenario(rascal, 0.0025, 2.0))?;
    let attitude = frames.last().unwrap().entities[0]
        .truth
        .orientation_world_from_body;
    Ok(attitude.yaw_world_from_body_rad().to_degrees())
}

#[test]
fn a_rudder_steered_tail_wheel_turns_the_nose_with_the_rudder() -> Result<()> {
    let castered = turned_deg(false)?;
    let steered = turned_deg(true)?;
    println!("castered {castered:.1} deg, steered {steered:.1} deg");
    // Positive rudder yaws the nose left (heading increases); the steered
    // wheel turns it much more than the rudder's airflow at 5 m/s alone.
    assert!(steered > 10.0, "steered {steered} deg");
    assert!(
        steered > 3.0 * castered.abs(),
        "steered {steered}, castered {castered}"
    );
    Ok(())
}

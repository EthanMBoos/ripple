//! Run the PX4 SITL Rascal missions against fake PX4s over real TCP, and check
//! the lockstep rules: the bridge free-wheels while PX4 boots, then takes
//! exactly one controls message per tick stamped with the readings it
//! answers; older stamps are ignored, a later stamp or silence fails the run,
//! and two aircraft stay on one clock with their own controls. No Docker or
//! PX4 is needed.
use anyhow::Result;
use mavlink::dialects::common::{
    HEARTBEAT_DATA, HIL_ACTUATOR_CONTROLS_DATA, HIL_SENSOR_DATA, MavMessage, MavModeFlag,
};
use mavlink::{MavHeader, MavlinkReader, MavlinkVersion, write_v2_msg};
use ripple_core::{Mission, Params, Simulation, plugin::PluginRegistry};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const DT_S: f64 = 0.01;

/// How the fake PX4 answers HIL_SENSOR.
#[derive(Clone, Copy)]
enum FakePx4 {
    /// Boot in silence for `BOOT_TICKS` messages, then one answer per message.
    Normal,
    /// Like Normal, but each answer is preceded by one stamped with the
    /// previous tick's time: a leftover the bridge must ignore.
    StaleFirst,
    /// Answer `BOOT_TICKS` messages after boot, then go silent.
    Silent,
    /// Answer stamped one microsecond late.
    Late,
}

/// Sensor messages PX4 ignores while its controllers start.
const BOOT_TICKS: usize = 5;

fn free_port() -> Result<u16> {
    Ok(TcpListener::bind("127.0.0.1:0")?.local_addr()?.port())
}

fn controls(time_usec: u64, throttle: f32) -> MavMessage {
    let mut values = [0.0f32; 16];
    // Channel 5 is the motor in the default map.
    values[4] = throttle;
    MavMessage::HIL_ACTUATOR_CONTROLS(HIL_ACTUATOR_CONTROLS_DATA {
        time_usec,
        flags: Default::default(),
        controls: values,
        mode: MavModeFlag::MAV_MODE_FLAG_CUSTOM_MODE_ENABLED
            | MavModeFlag::MAV_MODE_FLAG_SAFETY_ARMED,
    })
}

/// Plays PX4: connects to the bridge (retrying, as PX4 does, since the bridge
/// listens only once the simulation is built), sends a heartbeat, reads
/// sensor messages, and answers per `behavior`. Returns the sensor messages
/// received, in order, once the bridge closes the connection.
fn fake_px4(
    port: u16,
    behavior: FakePx4,
    throttle: f32,
) -> thread::JoinHandle<Result<Vec<HIL_SENSOR_DATA>>> {
    thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut stream = loop {
            match TcpStream::connect(("127.0.0.1", port)) {
                Ok(stream) => {
                    // Answers must not wait for the previous write's ACK.
                    stream.set_nodelay(true)?;
                    break stream;
                }
                Err(error) if Instant::now() < deadline => {
                    let _ = error;
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => return Err(error.into()),
            }
        };
        let mut sequence = 0u8;
        let mut send = |stream: &mut TcpStream, message: &MavMessage| -> Result<()> {
            let header = MavHeader {
                system_id: 1,
                component_id: 1,
                sequence,
            };
            sequence = sequence.wrapping_add(1);
            write_v2_msg(stream, header, message)?;
            Ok(())
        };
        send(
            &mut stream,
            &MavMessage::HEARTBEAT(HEARTBEAT_DATA::default()),
        )?;
        let mut reader = MavlinkReader::new(stream.try_clone()?);
        let mut sensors: Vec<HIL_SENSOR_DATA> = Vec::new();
        let mut answered = 0usize;
        loop {
            let Ok((_, message)) = reader.read_message::<MavMessage>(MavlinkVersion::V2) else {
                return Ok(sensors); // the bridge closed: the run ended
            };
            let MavMessage::HIL_SENSOR(sensor) = message else {
                continue;
            };
            let previous = sensors.last().map(|s| s.time_usec);
            let time_usec = sensor.time_usec;
            sensors.push(sensor);
            if sensors.len() <= BOOT_TICKS {
                continue;
            }
            match behavior {
                FakePx4::Normal => send(&mut stream, &controls(time_usec, throttle))?,
                FakePx4::StaleFirst => {
                    if let Some(previous) = previous {
                        send(&mut stream, &controls(previous, 0.0))?;
                    }
                    send(&mut stream, &controls(time_usec, throttle))?;
                }
                FakePx4::Silent => {
                    if answered < BOOT_TICKS {
                        send(&mut stream, &controls(time_usec, throttle))?;
                    }
                }
                FakePx4::Late => send(&mut stream, &controls(time_usec + 1, throttle))?,
            }
            answered += 1;
        }
    })
}

/// Load a SITL mission with test settings: no hold, starting at 20 m/s, short
/// timeouts, and each named aircraft's bridge on the given port.
fn simulation(mission: &str, aircraft: &[(&str, u16)], end_s: f64) -> Result<Simulation> {
    let crate_folder = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut registry = PluginRegistry::with_builtins();
    sitl::register(&mut registry)?;
    px4::register(&mut registry)?;
    let mut params = Params::from([
        ("run.dt_s".into(), DT_S.to_string()),
        ("run.end_s".into(), end_s.to_string()),
    ]);
    for (label, port) in aircraft {
        let entity = format!("entities.{label}");
        let bridge = format!("{entity}.autonomy.Px4Bridge");
        params.extend([
            (format!("{bridge}.port"), port.to_string()),
            (format!("{bridge}.frame_timeout_s"), "1.5".into()),
            (format!("{bridge}.first_frame_timeout_s"), "5".into()),
            // No hold: these tests check the exchange and the physics from tick one.
            (
                format!("{entity}.motion_model.FixedWing6DOF.enable_launcher"),
                "false".into(),
            ),
            (format!("{entity}.velocity_mps"), "[20, 0, 0]".into()),
        ]);
    }
    let mission = Mission::load(
        &crate_folder.join("missions").join(mission),
        crate_folder,
        &params,
    )?;
    Simulation::new(mission.scenario, Arc::new(registry))
}

/// Run the one-aircraft mission to its end.
fn run(port: u16, end_s: f64) -> Result<()> {
    let mut simulation = simulation("rascal-sitl.yaml", &[("rascal", port)], end_s)?;
    while simulation.step()?.is_some() {}
    Ok(())
}

fn times_s(sensors: &[HIL_SENSOR_DATA]) -> Vec<f64> {
    sensors.iter().map(|s| s.time_usec as f64 / 1e6).collect()
}

#[test]
fn one_sensor_message_per_tick_with_increasing_times() -> Result<()> {
    let port = free_port()?;
    let px4 = fake_px4(port, FakePx4::Normal, 0.5);
    run(port, 1.0)?;
    let sensors = px4.join().unwrap()?;
    let times = times_s(&sensors);
    // One per tick after the first, which has no sample yet.
    assert!(times.len() >= 99, "{} messages", times.len());
    for (index, pair) in times.windows(2).enumerate() {
        assert!(
            (pair[1] - pair[0] - DT_S).abs() < 1e-9,
            "tick {index}: {pair:?}"
        );
    }
    Ok(())
}

#[test]
fn the_bridge_free_wheels_while_px4_boots_then_applies_its_controls() -> Result<()> {
    let port = free_port()?;
    let px4 = fake_px4(port, FakePx4::Normal, 1.0);
    run(port, 1.0)?;
    let sensors = px4.join().unwrap()?;
    // Unanswered, the bridge holds zero throttle: only drag, so the aircraft
    // slows. The first full-throttle answer is to message BOOT_TICKS + 1; the
    // bridge picks it up without waiting, as soon as it has arrived, and the
    // readings from then on show the thrust.
    let first_thrust = sensors
        .iter()
        .position(|sensor| sensor.xacc > 1.0)
        .expect("the throttle was never applied");
    assert!(
        first_thrust > BOOT_TICKS,
        "thrust at message {first_thrust}"
    );
    assert!(
        sensors[..=BOOT_TICKS]
            .iter()
            .all(|sensor| sensor.xacc < 0.0)
    );
    assert!(
        sensors[first_thrust..]
            .iter()
            .all(|sensor| sensor.xacc > 1.0)
    );
    Ok(())
}

#[test]
fn an_older_controls_message_is_ignored() -> Result<()> {
    let port = free_port()?;
    let px4 = fake_px4(port, FakePx4::StaleFirst, 1.0);
    run(port, 1.0)?;
    let sensors = px4.join().unwrap()?;
    assert!(sensors.len() >= 49, "{} messages", sensors.len());
    // The stale message carried zero throttle; the current one full throttle.
    assert!(sensors.last().unwrap().xacc > 1.0, "{:?}", sensors.last());
    Ok(())
}

#[test]
fn a_late_stamp_fails_the_run() -> Result<()> {
    let port = free_port()?;
    let px4 = fake_px4(port, FakePx4::Late, 0.5);
    let error = format!("{:#}", run(port, 1.0).unwrap_err());
    assert!(error.contains("lockstep is lost"), "{error}");
    px4.join().unwrap()?;
    Ok(())
}

#[test]
fn silence_fails_the_run() -> Result<()> {
    let port = free_port()?;
    let px4 = fake_px4(port, FakePx4::Silent, 0.5);
    let error = format!("{:#}", run(port, 1.0).unwrap_err());
    assert!(error.contains("no controls from PX4"), "{error}");
    px4.join().unwrap()?;
    Ok(())
}

#[test]
fn no_connection_fails_the_run() -> Result<()> {
    let port = free_port()?;
    let mut simulation = simulation("rascal-sitl.yaml", &[("rascal", port)], 1.0)?;
    let error = (|| -> Result<()> {
        while simulation.step()?.is_some() {}
        Ok(())
    })()
    .unwrap_err();
    let error = format!("{error:#}");
    assert!(error.contains("did not connect"), "{error}");
    Ok(())
}

#[test]
fn closing_frees_the_port() -> Result<()> {
    let port = free_port()?;
    let px4 = fake_px4(port, FakePx4::Normal, 0.5);
    let mut simulation = simulation("rascal-sitl.yaml", &[("rascal", port)], 0.1)?;
    while simulation.step()?.is_some() {}
    // The finished simulation is still alive, but its bridge has closed.
    TcpListener::bind(("0.0.0.0", port))?;
    drop(simulation);
    px4.join().unwrap()?;
    Ok(())
}

#[test]
fn two_aircraft_share_one_clock_and_keep_their_own_controls() -> Result<()> {
    let (port_a, port_b) = (free_port()?, free_port()?);
    // Aircraft A at full throttle, B at idle.
    let px4_a = fake_px4(port_a, FakePx4::Normal, 1.0);
    let px4_b = fake_px4(port_b, FakePx4::Normal, 0.0);
    let aircraft = [("rascal_a", port_a), ("rascal_b", port_b)];
    let mut simulation = simulation("rascal-sitl-pair.yaml", &aircraft, 0.5)?;
    while simulation.step()?.is_some() {}
    let (sensors_a, sensors_b) = (px4_a.join().unwrap()?, px4_b.join().unwrap()?);

    assert!(sensors_a.len() >= 49, "{} messages", sensors_a.len());
    assert_eq!(times_s(&sensors_a), times_s(&sensors_b));
    // Full throttle pushes A forward once its controls arrive; idle leaves B
    // only drag throughout.
    let first_thrust = sensors_a
        .iter()
        .position(|sensor| sensor.xacc > 1.0)
        .expect("A's throttle was never applied");
    assert!(sensors_a[first_thrust..].iter().all(|a| a.xacc > 1.0));
    assert!(sensors_b.iter().all(|b| b.xacc < 0.0));
    Ok(())
}

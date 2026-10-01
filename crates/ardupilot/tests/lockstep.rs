//! Run the SITL Rascal missions against fake SITLs over real UDP, and check the
//! lockstep rules: exactly one reply per frame, resends ignored, a frame gap or
//! silence fails the run, and two aircraft stay on one clock with their own
//! commands. No Docker or ArduPilot is needed.
use anyhow::Result;
use scrimmage_core::{Mission, Params, Simulation, plugin::PluginRegistry};
use serde_json::Value;
use std::net::UdpSocket;
use std::path::Path;
use std::thread;
use std::time::Duration;

const DT_S: f64 = 0.01;

/// What the fake SITL does after each reply.
#[derive(Clone, Copy)]
enum FakeSitl {
    /// Send the next frame once.
    Normal,
    /// Send each frame twice, as SITL does when a reply is late.
    Resend,
    /// Before each new frame, resend the frame just answered: a resend that
    /// crossed the reply in flight.
    StaleResend,
    /// Skip frame 2.
    Gap,
    /// Send frame 0, then never answer.
    Silent,
}

/// Throttle PWM on SERVO3: half throttle unless a test says otherwise.
const HALF_THROTTLE_US: u16 = 1500;

/// A servo packet with centred surfaces (1500 us) and the given throttle.
fn servo_packet(frame_count: u32, throttle_pwm_us: u16) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&18458u16.to_le_bytes());
    bytes.extend_from_slice(&100u16.to_le_bytes());
    bytes.extend_from_slice(&frame_count.to_le_bytes());
    for channel in 1..=16 {
        let pwm_us: u16 = if channel == 3 { throttle_pwm_us } else { 1500 };
        bytes.extend_from_slice(&pwm_us.to_le_bytes());
    }
    bytes
}

fn free_port() -> Result<u16> {
    Ok(UdpSocket::bind("127.0.0.1:0")?.local_addr()?.port())
}

/// Plays SITL: sends frames to the bridge and collects its replies. Like SITL,
/// it resends the current frame while no reply comes (every 0.2 s here, 1 s in
/// ArduPilot), so it can start before the bridge is listening. It stops after
/// 2 s without a reply. Returns the parsed replies in arrival order.
fn fake_sitl(
    port: u16,
    behavior: FakeSitl,
    throttle_pwm_us: u16,
) -> Result<thread::JoinHandle<Result<Vec<Value>>>> {
    let socket = UdpSocket::bind("127.0.0.1:0")?;
    socket.set_read_timeout(Some(Duration::from_millis(200)))?;
    Ok(thread::spawn(move || {
        let bridge = ("127.0.0.1", port);
        let mut replies = Vec::new();
        let mut frame_count = 0u32;
        let mut quiet_reads = 0;
        socket.send_to(&servo_packet(frame_count, throttle_pwm_us), bridge)?;
        let mut buffer = [0u8; 2048];
        loop {
            let Ok(length) = socket.recv(&mut buffer) else {
                quiet_reads += 1;
                if quiet_reads >= 10 {
                    return Ok(replies); // the run ended
                }
                let answering = !matches!(behavior, FakeSitl::Silent) || replies.is_empty();
                if answering {
                    socket.send_to(&servo_packet(frame_count, throttle_pwm_us), bridge)?;
                }
                continue;
            };
            quiet_reads = 0;
            let text = std::str::from_utf8(&buffer[..length])?;
            replies.push(serde_json::from_str(text.trim())?);
            if matches!(behavior, FakeSitl::Silent) {
                continue;
            }
            if matches!(behavior, FakeSitl::StaleResend) {
                socket.send_to(&servo_packet(frame_count, throttle_pwm_us), bridge)?;
            }
            frame_count += 1;
            if matches!(behavior, FakeSitl::Gap) && frame_count == 2 {
                frame_count += 1;
            }
            socket.send_to(&servo_packet(frame_count, throttle_pwm_us), bridge)?;
            if matches!(behavior, FakeSitl::Resend) {
                socket.send_to(&servo_packet(frame_count, throttle_pwm_us), bridge)?;
            }
        }
    }))
}

fn simulation(mission: &str, overrides: &[(&str, String)], workers: usize) -> Result<Simulation> {
    let crate_folder = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut registry = PluginRegistry::with_builtins();
    ardupilot::register(&mut registry)?;
    let mut params = Params::from([
        ("dt".into(), DT_S.to_string()),
        ("frame_timeout".into(), "1.5".into()),
        ("first_frame_timeout".into(), "5".into()),
    ]);
    for (key, value) in overrides {
        params.insert((*key).into(), value.clone());
    }
    let mission = Mission::load_with_registry(
        &crate_folder.join("missions").join(mission),
        crate_folder,
        &params,
        &registry,
    )?;
    Simulation::new(mission.scenario, &registry, workers)
}

/// Run the one-aircraft mission to its end.
fn run(port: u16, end_s: f64) -> Result<()> {
    let overrides = [("port", port.to_string()), ("end", end_s.to_string())];
    let mut simulation = simulation("rascal-sitl.xml", &overrides, 1)?;
    while simulation.step()?.is_some() {}
    Ok(())
}

fn timestamps(replies: &[Value]) -> Vec<f64> {
    replies
        .iter()
        .map(|reply| reply["timestamp"].as_f64().unwrap())
        .collect()
}

/// Body-forward specific force (m/s^2) in a reply.
fn forward_accel(reply: &Value) -> f64 {
    reply["imu"]["accel_body"][0].as_f64().unwrap()
}

#[test]
fn one_reply_per_frame_with_increasing_timestamps() -> Result<()> {
    let port = free_port()?;
    let sitl = fake_sitl(port, FakeSitl::Normal, HALF_THROTTLE_US)?;
    run(port, 1.0)?;
    let replies = sitl.join().unwrap()?;

    // Every tick after the first answers the frame applied the tick before.
    assert!(replies.len() >= 99, "{} replies", replies.len());
    for (index, reply) in replies.iter().enumerate() {
        let timestamp_s = reply["timestamp"].as_f64().unwrap();
        let expected_s = (index + 1) as f64 * DT_S;
        assert!(
            (timestamp_s - expected_s).abs() < 1e-9,
            "reply {index}: {timestamp_s}"
        );
    }
    // Facing east at 20 m/s and 200 m: NED velocity points east, altitude is up.
    let first = &replies[0];
    assert!((first["velocity"][1].as_f64().unwrap() - 20.0).abs() < 0.5);
    assert!((first["position"][2].as_f64().unwrap() + 200.0).abs() < 1.0);
    Ok(())
}

#[test]
fn resent_frames_get_no_extra_reply() -> Result<()> {
    let port = free_port()?;
    let sitl = fake_sitl(port, FakeSitl::Resend, HALF_THROTTLE_US)?;
    run(port, 0.5)?;
    let replies = sitl.join().unwrap()?;
    // Same count and timing as without resends: one reply per distinct frame.
    for (index, reply) in replies.iter().enumerate() {
        let timestamp_s = reply["timestamp"].as_f64().unwrap();
        assert!(((index + 1) as f64 * DT_S - timestamp_s).abs() < 1e-9);
    }
    assert!(replies.len() >= 49);
    Ok(())
}

#[test]
fn a_stale_resend_gets_no_extra_reply() -> Result<()> {
    let port = free_port()?;
    let sitl = fake_sitl(port, FakeSitl::StaleResend, HALF_THROTTLE_US)?;
    run(port, 0.5)?;
    let replies = sitl.join().unwrap()?;
    for (index, reply) in replies.iter().enumerate() {
        let timestamp_s = reply["timestamp"].as_f64().unwrap();
        assert!(((index + 1) as f64 * DT_S - timestamp_s).abs() < 1e-9);
    }
    assert!(replies.len() >= 49);
    Ok(())
}

#[test]
fn a_frame_gap_fails_the_run() -> Result<()> {
    let port = free_port()?;
    let sitl = fake_sitl(port, FakeSitl::Gap, HALF_THROTTLE_US)?;
    let error = run(port, 1.0).unwrap_err();
    assert!(
        format!("{error:#}").contains("sent frame 3 after frame 1"),
        "{error:#}"
    );
    sitl.join().unwrap()?;
    Ok(())
}

#[test]
fn silence_fails_the_run() -> Result<()> {
    let port = free_port()?;
    let sitl = fake_sitl(port, FakeSitl::Silent, HALF_THROTTLE_US)?;
    let error = run(port, 1.0).unwrap_err();
    assert!(format!("{error:#}").contains("no new frame"), "{error:#}");
    sitl.join().unwrap()?;
    Ok(())
}

#[test]
fn the_first_reply_includes_the_first_step_acceleration() -> Result<()> {
    // Full throttle from the first frame: the first reply must already show the
    // forward push, not the 0 that a missing previous velocity would give.
    let port = free_port()?;
    let sitl = fake_sitl(port, FakeSitl::Normal, 2000)?;
    run(port, 0.2)?;
    let replies = sitl.join().unwrap()?;
    assert!(forward_accel(&replies[0]) > 1.0, "{}", replies[0]);
    assert!((forward_accel(&replies[0]) - forward_accel(&replies[1])).abs() < 0.5);
    Ok(())
}

#[test]
fn closing_frees_the_port() -> Result<()> {
    let port = free_port()?;
    let sitl = fake_sitl(port, FakeSitl::Normal, HALF_THROTTLE_US)?;
    let overrides = [("port", port.to_string()), ("end", "0.1".to_string())];
    let mut simulation = simulation("rascal-sitl.xml", &overrides, 1)?;
    while simulation.step()?.is_some() {}
    // The finished simulation is still alive, but its bridge has closed.
    UdpSocket::bind(("0.0.0.0", port))?;
    drop(simulation);
    sitl.join().unwrap()?;
    Ok(())
}

#[test]
fn two_aircraft_share_one_clock_and_keep_their_own_commands() -> Result<()> {
    for workers in [1, 2] {
        let (port_a, port_b) = (free_port()?, free_port()?);
        // Aircraft A at full throttle, B at idle.
        let sitl_a = fake_sitl(port_a, FakeSitl::Normal, 2000)?;
        let sitl_b = fake_sitl(port_b, FakeSitl::Normal, 1000)?;
        let overrides = [
            ("port_a", port_a.to_string()),
            ("port_b", port_b.to_string()),
            ("end", "0.5".to_string()),
        ];
        let mut simulation = simulation("rascal-sitl-pair.xml", &overrides, workers)?;
        while simulation.step()?.is_some() {}
        let (replies_a, replies_b) = (sitl_a.join().unwrap()?, sitl_b.join().unwrap()?);

        assert!(
            replies_a.len() >= 49,
            "{workers} workers: {} replies",
            replies_a.len()
        );
        assert_eq!(
            timestamps(&replies_a),
            timestamps(&replies_b),
            "{workers} workers"
        );
        for (reply_a, reply_b) in replies_a.iter().zip(&replies_b) {
            // Full throttle pushes forward; idle leaves only drag.
            assert!(forward_accel(reply_a) > 1.0, "{workers} workers: {reply_a}");
            assert!(forward_accel(reply_b) < 0.0, "{workers} workers: {reply_b}");
        }
    }
    Ok(())
}

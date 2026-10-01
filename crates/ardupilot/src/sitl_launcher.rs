//! Start one ArduPlane SITL container per ArduPilotBridge aircraft for a run,
//! and stop them all when it ends.
//!
//! `scrimmage run` calls [`SitlLauncher::start`] with the resolved mission once
//! the run's output directory exists, so the containers always match the
//! mission: XML or YAML, templates and overrides included. Each aircraft gets
//! the image `scrimmage-rs-arduplane` (built from `docker/`), its
//! bridge's port, a system id (1, 2, ... in mission order), the run's rate,
//! and its own folder `<run>/sitl/<port>/` holding SITL's files and
//! `session.log`. Home, where local (0, 0, 0) is, is the terrain's origin, or
//! McMillan Airfield without terrain.
//!
//! Containers are stopped when the launcher is dropped, which covers a normal
//! end and a failed run, and on Ctrl-C. A run killed any other way leaves them
//! running; `docker stop $(docker ps -q --filter label=scrimmage-rs-sitl)`
//! stops them. ArduPilot aircraft all exist from t = 0; spawning them mid-run
//! is out of scope. See book/src/guides/ardupilot.md.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, Once};

use anyhow::{Context, Result, ensure};
use scrimmage_core::plugin::{Plugin, PluginParams, motion::FixedWing6Dof};
use scrimmage_core::{ScenarioConfig, Terrain};

use crate::ardupilot_bridge::ArduPilotBridgeConfig;

const IMAGE: &str = "scrimmage-rs-arduplane";
const LABEL: &str = "scrimmage-rs-sitl";
/// McMillan Airfield, Camp Roberts: home for missions without terrain
/// (latitude, longitude, altitude above sea level).
const DEFAULT_HOME: [f64; 3] = [35.721025, -120.767925, 300.0];

/// One SITL container to start.
struct Aircraft {
    bridge: ArduPilotBridgeConfig,
    /// When FixedWing6DOF's launcher releases it; ArduPlane must be armed by then.
    release_s: Option<f64>,
}

pub struct SitlLauncher {
    /// This launcher's containers; they are also in `RUNNING`.
    containers: Vec<String>,
    loggers: Vec<Child>,
    logs: Vec<PathBuf>,
}

impl SitlLauncher {
    /// Start a container for each ArduPilotBridge aircraft in `scenario`, or
    /// nothing if it has none.
    pub fn start(
        scenario: &ScenarioConfig,
        terrain: Option<&Terrain>,
        run_directory: &Path,
    ) -> Result<Self> {
        let mut launcher = Self {
            containers: Vec::new(),
            loggers: Vec::new(),
            logs: Vec::new(),
        };
        let aircraft = find_aircraft(scenario)?;
        if aircraft.is_empty() {
            return Ok(launcher);
        }
        let rate_hz = (1.0 / scenario.run.dt_s).round();
        ensure!(
            (rate_hz * scenario.run.dt_s - 1.0).abs() < 1e-9,
            "ArduPilot SITL needs a whole-number rate: dt {} s is not 1/n",
            scenario.run.dt_s
        );
        let [latitude_deg, longitude_deg, altitude_m] = terrain
            .and_then(Terrain::origin)
            .map(|origin| [origin.latitude_deg, origin.longitude_deg, origin.altitude_m])
            .unwrap_or(DEFAULT_HOME);
        let home = format!("{latitude_deg},{longitude_deg},{altitude_m},0");
        build_image()?;

        stop_on_ctrl_c();

        let run_name = run_directory
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        for (index, plane) in aircraft.iter().enumerate() {
            let port = plane.bridge.port();
            let directory = run_directory.join("sitl").join(port.to_string());
            fs::create_dir_all(&directory)?;
            let directory = directory.canonicalize()?;
            // The mission's ArduPilot parameters, as a defaults file SITL
            // loads after rascal.parm (the folder is /run/sitl inside).
            let params: String = plane
                .bridge
                .ardupilot_params
                .iter()
                .map(|(name, value)| format!("{name} {value}\n"))
                .collect();
            fs::write(directory.join("mission.parm"), params)?;
            let name = format!("{LABEL}-{run_name}-{port}-{}", std::process::id());
            let sysid = index + 1;
            let waypoints = plane
                .bridge
                .waypoints
                .iter()
                .map(|[east, north, up]| format!("{east},{north},{up}"))
                .collect::<Vec<_>>()
                .join(";");
            let goto = plane
                .bridge
                .goto
                .map(|[east, north, up]| format!("{east},{north},{up}"))
                .unwrap_or_default();
            let environment = [
                ("SIM_PORT_OUT", port.to_string()),
                ("SYSID", sysid.to_string()),
                ("RATE_HZ", rate_hz.to_string()),
                ("SPEEDUP", plane.bridge.speedup.to_string()),
                ("HOME_LOCATION", home.clone()),
                ("MODE", plane.bridge.mode()?.to_string()),
                ("WAYPOINTS", waypoints),
                ("GOTO", goto),
                (
                    "TAKEOFF_ALT",
                    plane
                        .bridge
                        .takeoff_altitude
                        .map(|m| m.to_string())
                        .unwrap_or_default(),
                ),
                ("LAND", if plane.bridge.land { "1" } else { "" }.to_string()),
                (
                    "RELEASE_S",
                    plane.release_s.map(|s| s.to_string()).unwrap_or_default(),
                ),
            ];
            let mut command = Command::new("docker");
            command.args(["run", "-d", "--rm", "--name", &name, "--label", LABEL]);
            command
                .arg("--volume")
                .arg(format!("{}:/run/sitl", directory.display()));
            if cfg!(target_os = "linux") {
                command.args(["--add-host", "host.docker.internal:host-gateway"]);
            }
            for (key, value) in environment {
                command.arg("-e").arg(format!("{key}={value}"));
            }
            let output = command
                .arg(IMAGE)
                .output()
                .context("could not run docker")?;
            ensure!(
                output.status.success(),
                "docker run failed for port {port}: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
            launcher.containers.push(name.clone());
            RUNNING.lock().unwrap().push(name.clone());
            let log = directory.join("session.log");
            launcher.loggers.push(
                Command::new("docker")
                    .args(["logs", "-f", &name])
                    .stdout(File::create(&log)?)
                    .stderr(Stdio::from(File::create(directory.join("docker.log"))?))
                    .spawn()?,
            );
            launcher.logs.push(log);
            println!(
                "ArduPilot SITL for port {port} (sysid {sysid}): {}",
                directory.display()
            );
        }
        Ok(launcher)
    }
}

impl Drop for SitlLauncher {
    fn drop(&mut self) {
        stop_containers(&self.containers);
        RUNNING
            .lock()
            .unwrap()
            .retain(|name| !self.containers.contains(name));
        for logger in &mut self.loggers {
            let _ = logger.wait();
        }
        // A SITL that stops early (for example, not armed by the release)
        // makes the run fail with a bridge timeout; show the session's reason.
        for log in &self.logs {
            for line in fs::read_to_string(log).unwrap_or_default().lines() {
                if line.starts_with("error:") {
                    eprintln!("{}: {line}", log.display());
                }
            }
        }
    }
}

/// Every ArduPilotBridge aircraft, in mission order, with its release time.
fn find_aircraft(scenario: &ScenarioConfig) -> Result<Vec<Aircraft>> {
    let mut aircraft = Vec::new();
    for group in &scenario.entities {
        let Some(bridge) = group
            .autonomy
            .iter()
            .find(|plugin| plugin.name == "ArduPilotBridge")
        else {
            continue;
        };
        ensure!(
            group.count == 1 && group.spawn.is_none(),
            "{}: each ArduPilot aircraft needs its own entity group with count 1 and no spawn",
            group.label
        );
        let bridge = PluginParams::new(bridge)
            .parse::<ArduPilotBridgeConfig>()
            .with_context(|| format!("{}: ArduPilotBridge", group.label))?;
        let release_s = if group.motion_model.name == "FixedWing6DOF" {
            FixedWing6Dof::configure(&PluginParams::new(&group.motion_model))?.launch_time_s()
        } else {
            None
        };
        aircraft.push(Aircraft { bridge, release_s });
    }
    let mut ports: Vec<u16> = aircraft.iter().map(|plane| plane.bridge.port()).collect();
    ports.sort_unstable();
    ports.dedup();
    ensure!(
        ports.len() == aircraft.len(),
        "ArduPilotBridge ports must differ"
    );
    Ok(aircraft)
}

/// Build the image from `docker/` before every run, so an edited
/// `rascal.parm` or session script is always used. Docker's cache makes this
/// about a second when nothing changed; the first build compiles ArduPlane.
fn build_image() -> Result<()> {
    let found = Command::new("docker")
        .args(["image", "inspect", IMAGE])
        .output()
        .context("could not run docker; is Docker running?")?;
    let mut command = Command::new("docker");
    command.args(["build", "-t", IMAGE]);
    if found.status.success() {
        command.arg("-q").stdout(Stdio::null());
    } else {
        println!("building {IMAGE} (compiles ArduPlane; takes a while the first time)");
    }
    let status = command
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("docker"))
        .status()?;
    ensure!(status.success(), "docker build of {IMAGE} failed");
    Ok(())
}

/// Every launcher's containers, for the Ctrl-C handler.
static RUNNING: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Install, once per process, a Ctrl-C handler that stops every running SITL
/// container and exits. A process that already has its own handler keeps
/// it, and must stop the containers itself (dropping each launcher does).
fn stop_on_ctrl_c() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let installed = ctrlc::set_handler(|| {
            eprintln!("interrupted; stopping ArduPilot SITL");
            stop_containers(&RUNNING.lock().unwrap());
            std::process::exit(130);
        });
        if let Err(error) = installed {
            eprintln!("warning: SITL containers will not stop on Ctrl-C: {error}");
        }
    });
}

fn stop_containers(names: &[String]) {
    for name in names {
        let _ = Command::new("docker")
            .args(["stop", "--time", "3", name])
            .output();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_run_in_one_process_reuses_the_ctrl_c_handler() {
        // `ctrlc` allows one handler per process; each run asks for it.
        stop_on_ctrl_c();
        stop_on_ctrl_c();
        assert!(ctrlc::set_handler(|| {}).is_err(), "installed once");
    }
}

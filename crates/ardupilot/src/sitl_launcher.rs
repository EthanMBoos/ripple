//! Start one ArduPlane SITL container per ArduPilotBridge aircraft for a run,
//! and stop them all when it ends.
//!
//! `ripple run` calls [`SitlLauncher::start`] with the resolved mission once
//! the run's output directory exists, so the containers always match the
//! mission, templates and overrides included. Each aircraft gets the image
//! `ripple-arduplane` (built from `docker/`), its bridge's port, a system id
//! (1, 2, ... in mission order), the run's rate, and its own folder
//! `<run>/sitl/<port>/` holding SITL's files and `session.log`. Home, where
//! local (0, 0, 0) is, is the terrain's origin, or McMillan Airfield without
//! terrain. The containers themselves are the shared crate's
//! (`sitl::Containers`), stopped when the launcher is dropped and on Ctrl-C.
//! ArduPilot aircraft all exist from t = 0; spawning them mid-run is out of
//! scope. See book/src/guides/ardupilot.md.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, ensure};
use ripple_core::plugin::{MotionModel, PluginParams, motion::FixedWing6Dof};
use ripple_core::{ScenarioConfig, Terrain};
use sitl::{Container, Containers};

use crate::ardupilot_bridge::ArduPilotBridgeConfig;

const IMAGE: &str = "ripple-arduplane";

/// One SITL container to start.
struct Aircraft {
    bridge: ArduPilotBridgeConfig,
    /// When FixedWing6DOF's launcher releases it; ArduPlane must be armed by then.
    release_s: Option<f64>,
}

pub struct SitlLauncher {
    _containers: Containers,
}

impl SitlLauncher {
    /// Start a container for each ArduPilotBridge aircraft in `scenario`, or
    /// nothing if it has none.
    pub fn start(
        scenario: &ScenarioConfig,
        terrain: Option<&Terrain>,
        run_directory: &Path,
    ) -> Result<Self> {
        let aircraft = find_aircraft(scenario)?;
        let rate_hz = (1.0 / scenario.run.dt_s).round();
        ensure!(
            aircraft.is_empty() || (rate_hz * scenario.run.dt_s - 1.0).abs() < 1e-9,
            "ArduPilot SITL needs a whole-number rate: dt {} s is not 1/n",
            scenario.run.dt_s
        );
        let [latitude_deg, longitude_deg, altitude_m] = sitl::home(terrain);
        let home = format!("{latitude_deg},{longitude_deg},{altitude_m},0");

        let mut containers = Vec::new();
        for (index, plane) in aircraft.iter().enumerate() {
            let port = plane.bridge.port();
            let directory = run_directory.join("sitl").join(port.to_string());
            fs::create_dir_all(&directory)?;
            // The mission's ArduPilot parameters, as a defaults file SITL
            // loads after rascal.parm (the folder is /run/sitl inside).
            let params: String = plane
                .bridge
                .ardupilot_params
                .iter()
                .map(|(name, value)| format!("{name} {value}\n"))
                .collect();
            fs::write(directory.join("mission.parm"), params)?;
            let sysid = index + 1;
            let waypoints_m = plane
                .bridge
                .waypoints_m
                .iter()
                .map(|[east, north, up]| format!("{east},{north},{up}"))
                .collect::<Vec<_>>()
                .join(";");
            let goto_m = plane
                .bridge
                .goto_m
                .map(|[east, north, up]| format!("{east},{north},{up}"))
                .unwrap_or_default();
            let environment = [
                ("SIM_PORT_OUT", port.to_string()),
                ("SYSID", sysid.to_string()),
                ("RATE_HZ", rate_hz.to_string()),
                ("SPEEDUP", plane.bridge.speedup.to_string()),
                ("HOME_LOCATION", home.clone()),
                ("MODE", plane.bridge.mode()?.to_string()),
                ("WAYPOINTS", waypoints_m),
                ("GOTO", goto_m),
                (
                    "TAKEOFF_ALT",
                    plane
                        .bridge
                        .takeoff_altitude_m
                        .map(|m| m.to_string())
                        .unwrap_or_default(),
                ),
                ("LAND", if plane.bridge.land { "1" } else { "" }.to_string()),
                (
                    "RELEASE_S",
                    plane.release_s.map(|s| s.to_string()).unwrap_or_default(),
                ),
            ];
            containers.push(Container {
                description: format!("ArduPilot SITL for port {port} (sysid {sysid})"),
                tag: port.to_string(),
                directory,
                environment: environment
                    .into_iter()
                    .map(|(key, value)| (key.to_string(), value))
                    .collect(),
            });
        }
        let context = Path::new(env!("CARGO_MANIFEST_DIR")).join("docker");
        Ok(Self {
            _containers: Containers::start(IMAGE, &context, run_directory, containers)?,
        })
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
            FixedWing6Dof::configure(&PluginParams::new(&group.motion_model))?.launch_at_s()
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

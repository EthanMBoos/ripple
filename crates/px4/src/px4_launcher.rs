//! Start one PX4 SITL container per Px4Bridge aircraft for a run, and stop
//! them all when it ends.
//!
//! `ripple run` calls [`Px4Launcher::start`] with the resolved mission once
//! the run's output directory exists, so the containers always match the
//! mission, templates and overrides included. Each aircraft gets the image
//! `ripple-px4` (built from `docker/`), the PX4 instance its port names
//! (port 4560 is instance 0, 4561 instance 1, and so on, as PX4 numbers
//! them), and its own folder `<run>/sitl/<port>/` holding its flight log and
//! `session.log` (PX4's parameter and mission files stay in the container;
//! see docker/entrypoint.sh). Home, where local (0, 0, 0) is, is the
//! terrain's origin, or McMillan Airfield without terrain; the bridge, which
//! makes the GPS readings, must be given the same. The containers are the shared
//! crate's (`sitl::Containers`), stopped when the launcher is dropped and on
//! Ctrl-C. PX4 aircraft all exist from t = 0; spawning them mid-run is out of
//! scope. See book/src/guides/px4.md.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, ensure};
use ripple_core::plugin::{MotionModel, PluginParams, motion::FixedWing6Dof};
use ripple_core::{ScenarioConfig, Terrain};
use sitl::{Container, Containers};

use crate::px4_bridge::Px4BridgeConfig;

const IMAGE: &str = "ripple-px4";
/// PX4 instance N connects to the simulator on this port plus N.
const FIRST_PORT: u16 = 4560;

/// One PX4 container to start.
struct Aircraft {
    bridge: Px4BridgeConfig,
    /// When FixedWing6DOF's launcher releases it; PX4 must be armed by then.
    release_s: Option<f64>,
}

pub struct Px4Launcher {
    _containers: Containers,
}

impl Px4Launcher {
    /// Start a container for each Px4Bridge aircraft in `scenario`, or
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
            "PX4 SITL needs a whole-number rate: dt {} s is not 1/n",
            scenario.run.dt_s
        );
        let home = sitl::home(terrain);

        let mut containers = Vec::new();
        for plane in &aircraft {
            let port = plane.bridge.port();
            let bridge_home = plane.bridge.home();
            ensure!(
                (bridge_home[0] - home[0]).abs() < 1e-6
                    && (bridge_home[1] - home[1]).abs() < 1e-6
                    && (bridge_home[2] - home[2]).abs() < 0.01,
                "Px4Bridge on port {port}: home {bridge_home:?} is not where local (0, 0, 0) is; \
                 set it to {home:?} (the terrain's origin, or McMillan Airfield without terrain)"
            );
            let instance = port - FIRST_PORT;
            let directory = run_directory.join("sitl").join(port.to_string());
            fs::create_dir_all(&directory)?;
            let waypoints_m = plane
                .bridge
                .waypoints_m
                .iter()
                .map(|[east, north, up]| format!("{east},{north},{up}"))
                .collect::<Vec<_>>()
                .join(";");
            let mut environment = vec![
                ("PX4_INSTANCE".to_string(), instance.to_string()),
                ("PX4_SIM_MODEL".to_string(), "rascal".to_string()),
                // PX4 dials out to the bridge on the host.
                (
                    "PX4_SIM_HOSTNAME".to_string(),
                    "host.docker.internal".to_string(),
                ),
                (
                    "PX4_SIM_SPEED_FACTOR".to_string(),
                    plane.bridge.speedup.to_string(),
                ),
                // The IMU rate is the tick rate, and PX4 integrates and runs
                // its control loop at that rate, so one controls message
                // answers each sensor message.
                ("PX4_PARAM_IMU_INTEG_RATE".to_string(), rate_hz.to_string()),
                (
                    "PX4_PARAM_IMU_GYRO_RATEMAX".to_string(),
                    rate_hz.to_string(),
                ),
                // PX4's startup scales its pre-flight disarm timeout by the
                // speed factor, which a hold can outlast; never disarm an
                // armed aircraft that has not flown yet.
                ("PX4_PARAM_COM_DISARM_PRFLT".to_string(), "-1".to_string()),
                ("MODE".to_string(), plane.bridge.mode()?.to_string()),
                ("WAYPOINTS".to_string(), waypoints_m),
                (
                    "TAKEOFF_ALT".to_string(),
                    plane
                        .bridge
                        .takeoff_altitude_m
                        .map(|m| m.to_string())
                        .unwrap_or_default(),
                ),
                (
                    "LAND".to_string(),
                    if plane.bridge.land { "1" } else { "" }.to_string(),
                ),
                (
                    "RELEASE_S".to_string(),
                    plane.release_s.map(|s| s.to_string()).unwrap_or_default(),
                ),
            ];
            // The mission's PX4 parameters; rcS applies PX4_PARAM_* overrides.
            for (name, value) in &plane.bridge.px4_params {
                environment.push((format!("PX4_PARAM_{name}"), value.to_string()));
            }
            containers.push(Container {
                description: format!("PX4 SITL for port {port} (instance {instance})"),
                tag: port.to_string(),
                directory,
                environment,
            });
        }
        let context = Path::new(env!("CARGO_MANIFEST_DIR")).join("docker");
        Ok(Self {
            _containers: Containers::start(IMAGE, &context, run_directory, containers)?,
        })
    }
}

/// Every Px4Bridge aircraft, in mission order, with its release time.
fn find_aircraft(scenario: &ScenarioConfig) -> Result<Vec<Aircraft>> {
    let mut aircraft = Vec::new();
    for group in &scenario.entities {
        let Some(bridge) = group
            .autonomy
            .iter()
            .find(|plugin| plugin.name == "Px4Bridge")
        else {
            continue;
        };
        ensure!(
            group.count == 1 && group.spawn.is_none(),
            "{}: each PX4 aircraft needs its own entity group with count 1 and no spawn",
            group.label
        );
        let bridge = PluginParams::new(bridge)
            .parse::<Px4BridgeConfig>()
            .with_context(|| format!("{}: Px4Bridge", group.label))?;
        ensure!(
            (FIRST_PORT..FIRST_PORT + 10).contains(&bridge.port()),
            "{}: Px4Bridge port must be 4560 to 4569 (PX4 instance 0 to 9)",
            group.label
        );
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
    ensure!(ports.len() == aircraft.len(), "Px4Bridge ports must differ");
    Ok(aircraft)
}

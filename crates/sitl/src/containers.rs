//! The Docker containers a run's SITLs live in: build the adapter's image,
//! start one container per aircraft with its own folder under the run
//! directory, and stop them all when the run ends.
//!
//! An adapter's launcher decides which aircraft need a container and what
//! environment each gets; this module does the `docker` work. Each container
//! mounts its folder at `/run/sitl` and its output goes to `session.log`
//! there. Containers are stopped when [`Containers`] is dropped, which covers
//! a normal end and a failed run, and on Ctrl-C. A run killed any other way
//! leaves them running; `docker stop $(docker ps -q --filter
//! label=ripple-sitl)` stops them.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, Once};

use anyhow::{Context, Result, ensure};
use ripple_core::Terrain;

const LABEL: &str = "ripple-sitl";
/// McMillan Airfield, Camp Roberts: home for missions without terrain
/// (latitude, longitude, altitude above sea level).
const DEFAULT_HOME: [f64; 3] = [35.721025, -120.767925, 300.0];

/// One container to start.
pub struct Container {
    /// Printed once the container is running, before its folder.
    pub description: String,
    /// Distinguishes this run's containers in their names, such as a port.
    pub tag: String,
    /// The container's folder under the run directory, mounted at `/run/sitl`.
    pub directory: PathBuf,
    /// Environment variables for the image's entrypoint.
    pub environment: Vec<(String, String)>,
}

/// A run's containers; dropping it stops them.
pub struct Containers {
    /// This run's containers; they are also in `RUNNING`.
    names: Vec<String>,
    loggers: Vec<Child>,
    logs: Vec<PathBuf>,
}

impl Containers {
    /// Build `image` from the Docker context `context`, then start every
    /// container. Nothing is started for an empty list.
    pub fn start(
        image: &str,
        context: &Path,
        run_directory: &Path,
        containers: Vec<Container>,
    ) -> Result<Self> {
        let mut started = Self {
            names: Vec::new(),
            loggers: Vec::new(),
            logs: Vec::new(),
        };
        if containers.is_empty() {
            return Ok(started);
        }
        build_image(image, context)?;
        stop_on_ctrl_c();

        let run_name = run_directory
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        for container in containers {
            fs::create_dir_all(&container.directory)?;
            let directory = container.directory.canonicalize()?;
            let name = format!(
                "{LABEL}-{run_name}-{}-{}",
                container.tag,
                std::process::id()
            );
            let mut command = Command::new("docker");
            command.args(["run", "-d", "--rm", "--name", &name, "--label", LABEL]);
            command
                .arg("--volume")
                .arg(format!("{}:/run/sitl", directory.display()));
            if cfg!(target_os = "linux") {
                command.args(["--add-host", "host.docker.internal:host-gateway"]);
            }
            for (key, value) in &container.environment {
                command.arg("-e").arg(format!("{key}={value}"));
            }
            let output = command
                .arg(image)
                .output()
                .context("could not run docker")?;
            ensure!(
                output.status.success(),
                "docker run failed for {}: {}",
                container.description,
                String::from_utf8_lossy(&output.stderr).trim()
            );
            started.names.push(name.clone());
            RUNNING.lock().unwrap().push(name.clone());
            let log = directory.join("session.log");
            started.loggers.push(
                Command::new("docker")
                    .args(["logs", "-f", &name])
                    .stdout(File::create(&log)?)
                    .stderr(Stdio::from(File::create(directory.join("docker.log"))?))
                    .spawn()?,
            );
            started.logs.push(log);
            println!("{}: {}", container.description, directory.display());
        }
        Ok(started)
    }
}

impl Drop for Containers {
    fn drop(&mut self) {
        stop_containers(&self.names);
        RUNNING
            .lock()
            .unwrap()
            .retain(|name| !self.names.contains(name));
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

/// Home, where local (0, 0, 0) is: the terrain's origin, or McMillan Airfield
/// without terrain, as (latitude, longitude, altitude above sea level).
pub fn home(terrain: Option<&Terrain>) -> [f64; 3] {
    terrain
        .and_then(Terrain::origin)
        .map(|origin| [origin.latitude_deg, origin.longitude_deg, origin.altitude_m])
        .unwrap_or(DEFAULT_HOME)
}

/// Build the image from its context before every run, so an edited
/// parameter file or session script is always used. Docker's cache makes
/// this about a second when nothing changed; the first build compiles the
/// autopilot.
fn build_image(image: &str, context: &Path) -> Result<()> {
    let found = Command::new("docker")
        .args(["image", "inspect", image])
        .output()
        .context("could not run docker; is Docker running?")?;
    let mut command = Command::new("docker");
    command.args(["build", "-t", image]);
    if found.status.success() {
        command.arg("-q").stdout(Stdio::null());
    } else {
        println!("building {image} (compiles the autopilot; takes a while the first time)");
    }
    let status = command.arg(context).status()?;
    ensure!(status.success(), "docker build of {image} failed");
    Ok(())
}

/// Every run's containers, for the Ctrl-C handler.
static RUNNING: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Install, once per process, a Ctrl-C handler that stops every running SITL
/// container and exits. A process that already has its own handler keeps
/// it, and must stop the containers itself (dropping each `Containers` does).
fn stop_on_ctrl_c() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let installed = ctrlc::set_handler(|| {
            eprintln!("interrupted; stopping the SITL containers");
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

    #[test]
    fn home_is_mcmillan_without_terrain() {
        assert_eq!(home(None), DEFAULT_HOME);
    }
}

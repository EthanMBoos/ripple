//! The Python module `ripple._native`: the `ripple` command with one more
//! plugin in every slot, named `Python`, that runs a class written in Python
//! (`bridge.rs`): autonomy, controller, motion model, sensor, localization,
//! tracking, interaction, network, metrics. For the learning tasks
//! (`ripple.learning`) it also holds `Network`, a policy's ONNX network run
//! in Rust (`network.rs`), and `Simulation`, a simulation Python steps while
//! training (`training.rs`).
//!
//! `main` builds the same registry as the Rust `ripple` executable (the
//! built-ins, the starter, delivery) plus `Python`, and hands it with the
//! command line to `ripple_cli::main`, so `run`, `sweep`, and `replay` are one
//! implementation and a mission with no Python plugin gives the same record
//! from either command. Python's lock is released for the whole call; the
//! engine's worker threads take it back to call each plugin.
//!
//! pyo3 is linked one way only: as this extension module, built by maturin
//! and loaded by Python. The Rust executable never links an interpreter.
mod bridge;
mod context;
mod network;
mod training;

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use ripple_core::plugin::PluginRegistry;
use std::ffi::OsString;

/// The project folder (missions, runs, sweeps): the repository, as the Rust
/// executable's is. `--root` overrides it.
const PROJECT_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

/// The registry the command runs: the built-ins, the user crates, and
/// `Python` in every slot, with the autonomy registered by `autonomy` (the
/// training simulation hands each one its trainer).
fn registry_with(
    autonomy: impl FnOnce(&mut PluginRegistry) -> anyhow::Result<()>,
) -> anyhow::Result<PluginRegistry> {
    let mut registry = PluginRegistry::with_builtins();
    starter::register(&mut registry)?;
    delivery::register(&mut registry)?;
    autonomy(&mut registry)?;
    registry.register_controller::<bridge::PythonController>("Python")?;
    registry.register_sensor::<bridge::PythonSensor>("Python")?;
    registry.register_localization::<bridge::PythonLocalization>("Python")?;
    registry.register_tracking::<bridge::PythonTracking>("Python")?;
    registry.register_motion::<bridge::PythonMotion>("Python")?;
    registry.register_interaction::<bridge::PythonInteraction>("Python")?;
    registry.register_network::<bridge::PythonNetwork>("Python")?;
    registry.register_metrics::<bridge::PythonMetrics>("Python")?;
    Ok(registry)
}

fn registry() -> anyhow::Result<PluginRegistry> {
    registry_with(|registry| registry.register_autonomy::<bridge::PythonAutonomy>("Python"))
}

/// Runs the `ripple` command line (`args` starts with the program name).
#[pyfunction]
fn main(py: Python<'_>, args: Vec<OsString>) -> PyResult<()> {
    let registry = registry().map_err(error)?;
    py.detach(|| ripple_cli::main(registry, PROJECT_ROOT, args))
        .map_err(error)
}

fn error(error: anyhow::Error) -> PyErr {
    PyRuntimeError::new_err(format!("{error:#}"))
}

#[pymodule]
fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(main, module)?)?;
    module.add("PROJECT_ROOT", PROJECT_ROOT)?;
    module.add_class::<context::Context>()?;
    module.add_class::<context::State>()?;
    module.add_class::<context::Contact>()?;
    module.add_class::<context::Time>()?;
    module.add_class::<context::Entity>()?;
    module.add_class::<context::Sample>()?;
    module.add_class::<context::Command>()?;
    module.add_class::<context::Terrain>()?;
    module.add_class::<context::WorldEntity>()?;
    module.add_class::<context::Nearby>()?;
    module.add_class::<context::Endpoint>()?;
    module.add_class::<context::Link>()?;
    module.add_class::<context::Event>()?;
    module.add_class::<context::Message>()?;
    module.add_class::<network::Network>()?;
    module.add_class::<training::Simulation>()?;
    module.add_class::<context::Random>()
}

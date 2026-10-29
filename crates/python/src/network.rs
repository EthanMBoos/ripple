//! A trained policy's network, read from an ONNX file and run in Rust, for
//! the learning tasks in `ripple.learning`. The same network ran the shipped
//! policies before the tasks moved to Python, so a replay is unchanged.
use anyhow::{Context, Result, ensure};
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use std::path::PathBuf;
use std::sync::Arc;
use tract_onnx::prelude::*;

/// One observation in, one action out, each action number clamped to
/// [-1, 1]: the trainer's action space, and the range a network's mean can
/// leave. Running it is deterministic, so a replay of a policy is exact.
#[pyclass(frozen, module = "ripple")]
pub struct Network {
    model: Arc<TypedSimplePlan>,
    observation_size: usize,
}

impl Network {
    fn load(path: &PathBuf, observation_size: usize) -> Result<Self> {
        let model = tract_onnx::onnx()
            .model_for_path(path)
            .with_context(|| format!("read policy {}", path.display()))?
            .with_input_fact(0, f32::fact([1, observation_size]).into())?
            .into_optimized()?
            .into_runnable()?;
        Ok(Self {
            model,
            observation_size,
        })
    }

    fn run(&self, observation: &[f32]) -> Result<Vec<f32>> {
        ensure!(
            observation.len() == self.observation_size,
            "policy takes {} numbers, observation has {}",
            self.observation_size,
            observation.len()
        );
        let input = Tensor::from_shape(&[1, observation.len()], observation)?;
        let output = self.model.run(tvec!(input.into()))?;
        Ok(output[0]
            .to_plain_array_view::<f32>()?
            .iter()
            .map(|value| value.clamp(-1.0, 1.0))
            .collect())
    }
}

#[pymethods]
impl Network {
    /// Reads the network at `path`, which takes `observation_size` numbers.
    #[new]
    fn new(path: PathBuf, observation_size: usize) -> PyResult<Self> {
        Self::load(&path, observation_size).map_err(error)
    }

    /// The action for this observation.
    fn act(&self, observation: Vec<f32>) -> PyResult<Vec<f32>> {
        self.run(&observation).map_err(error)
    }
}

fn error(error: anyhow::Error) -> PyErr {
    PyRuntimeError::new_err(format!("{error:#}"))
}

//! Where a learned autonomy gets its action: a trained network, or a Python
//! trainer. Both tasks (`hover.rs`, `spread.rs`) share this.

use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tract_onnx::prelude::*;

/// A trained network read from an ONNX file: one observation in, one action
/// out. Running it is deterministic, so a replay of a policy is exact.
pub struct Network {
    model: Arc<TypedSimplePlan>,
    observation_size: usize,
}

impl Network {
    pub fn load(path: &Path, observation_size: usize) -> Result<Self> {
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

    pub fn act(&self, observation: &[f32]) -> Result<Vec<f32>> {
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
            .copied()
            .collect())
    }
}

/// The actions a Python trainer chooses, one per entity, set before each
/// step and taken by that entity's autonomy during it.
#[derive(Clone, Default)]
pub struct TrainerActions(Arc<Mutex<BTreeMap<i32, Vec<f32>>>>);

impl TrainerActions {
    pub fn set(&self, entity_id: i32, action: Vec<f32>) {
        self.0.lock().unwrap().insert(entity_id, action);
    }

    /// Takes the action, so a step without a new one is an error rather than
    /// a silent repeat.
    fn take(&self, entity_id: i32) -> Result<Vec<f32>> {
        self.0
            .lock()
            .unwrap()
            .remove(&entity_id)
            .with_context(|| format!("the trainer set no action for entity {entity_id}"))
    }
}

/// A learned autonomy's source of actions.
pub enum Policy {
    /// The mission's `policy` file, read when the entity starts.
    File(PathBuf),
    Network(Network),
    Trainer(TrainerActions),
}

impl Policy {
    /// Reads the network file, if this policy is one.
    pub fn load(&mut self, observation_size: usize) -> Result<()> {
        if let Policy::File(path) = self {
            *self = Policy::Network(Network::load(path, observation_size)?);
        }
        Ok(())
    }

    /// The action for this observation, each number clamped to [-1, 1]: the
    /// trainer's action space, and the range a network's mean can leave.
    pub fn act(&self, entity_id: i32, observation: &[f32]) -> Result<Vec<f32>> {
        let action = match self {
            Policy::File(path) => anyhow::bail!("policy {} was never read", path.display()),
            Policy::Network(network) => network.act(observation)?,
            Policy::Trainer(actions) => actions.take(entity_id)?,
        };
        Ok(action
            .into_iter()
            .map(|value| value.clamp(-1.0, 1.0))
            .collect())
    }
}

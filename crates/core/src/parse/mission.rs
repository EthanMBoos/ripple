//! Mission file discovery and file-specific execution defaults.
use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use crate::scenario::ScenarioConfig;

/// Mission overrides: a dotted path such as `entities.red.count` and its new
/// value, written as YAML (`20`, `[1, 2, 0]`).
pub type Params = BTreeMap<String, String>;

/// A loaded scenario and the defaults supplied by its mission file.
pub struct Mission {
    pub scenario: ScenarioConfig,
    pub workers: usize,
    pub viewer: bool,
    pub source: PathBuf,
}

impl Mission {
    /// Reads a YAML mission, found at `path` or under `root`'s `missions/`.
    pub fn load(path: &Path, root: &Path, overrides: &Params) -> Result<Self> {
        let found = [path.to_path_buf(), root.join("missions").join(path)]
            .into_iter()
            .find(|candidate| candidate.is_file())
            .with_context(|| format!("mission not found: {}", path.display()))?;
        ensure!(
            matches!(
                found.extension().and_then(|extension| extension.to_str()),
                Some("yaml" | "yml")
            ),
            "missions are YAML files (.yaml): {}",
            found.display()
        );
        super::yaml_mission::load(&found, overrides)
    }
}

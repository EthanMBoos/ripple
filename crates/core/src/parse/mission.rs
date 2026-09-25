//! Mission file discovery and file-specific execution defaults.
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use super::Params;
use crate::{plugin::PluginRegistry, scenario::ScenarioConfig};

/// A loaded scenario and the defaults supplied by its mission file.
pub struct Mission {
    pub scenario: ScenarioConfig,
    pub workers: usize,
    pub viewer: bool,
    pub source: PathBuf,
}

impl Mission {
    pub fn load(path: &Path, root: &Path, overrides: &Params) -> Result<Self> {
        Self::load_with_registry(path, root, overrides, &PluginRegistry::with_builtins())
    }

    pub fn load_with_registry(
        path: &Path,
        root: &Path,
        overrides: &Params,
        registry: &PluginRegistry,
    ) -> Result<Self> {
        let mut candidates = vec![path.to_path_buf()];
        if let Some(paths) = std::env::var_os("SCRIMMAGE_MISSION_PATH") {
            candidates.extend(std::env::split_paths(&paths).map(|directory| directory.join(path)));
        }
        candidates.push(root.join("missions").join(path));
        let path = candidates
            .iter()
            .find(|candidate| candidate.is_file())
            .with_context(|| format!("mission not found: {}", path.display()))?;
        match path.extension().and_then(|extension| extension.to_str()) {
            Some("yaml" | "yml") => super::yaml_mission::load(path, overrides),
            _ => super::xml_mission::load(path, overrides, registry),
        }
    }
}

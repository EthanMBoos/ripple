//! The typed mission that every input format loads into, and its validation.
//! `xml_mission.rs` reads legacy XML into it; nothing downstream sees the file format.
use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use std::path::{Path, PathBuf};

use super::Params;
use crate::{
    entity::EntityDefinition, math::Vec3, simcontrol::generation::Generator,
    simcontrol::world_plugins::CompiledWorld,
};

/// A scenario that has passed parameter and plugin-connection validation.
/// Its fields cannot be constructed or modified by a caller.
pub struct ResolvedScenario {
    pub(crate) config: ScenarioConfig,
    pub(crate) definitions: Vec<EntityDefinition>,
    pub(crate) generators: Vec<Generator>,
    pub(crate) world: CompiledWorld,
    pub(crate) end_conditions: EndConditions,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub(crate) struct EndConditions {
    pub time: bool,
    pub all_dead: bool,
    pub one_team: bool,
}

impl EndConditions {
    pub(crate) fn from_names<'a>(names: impl IntoIterator<Item = &'a str>) -> Result<Self> {
        let mut conditions = Self::default();
        for name in names {
            match name {
                "time" => conditions.time = true,
                "all_dead" => conditions.all_dead = true,
                "one_team" => conditions.one_team = true,
                "none" => {}
                invalid => bail!("unknown end condition: {invalid}"),
            }
        }
        Ok(conditions)
    }
}

/// A plugin's mission values, in the form its file gave them. The plugin's
/// `params.parse()` reads either one into the same parameter struct.
#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
pub(crate) enum PluginValues {
    /// XML text, read with the legacy conversions in `params.rs`.
    Text(Params),
    /// YAML values, read natively.
    Yaml(serde_yaml_ng::Mapping),
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct PluginConfig {
    pub name: String,
    /// A sensor's stable identity for its random stream; defaults to `Name:index`.
    pub instance: Option<String>,
    /// The framework's `loop_rate`; 0 runs the plugin every step.
    pub loop_rate_hz: f64,
    pub params: PluginValues,
}

/// A repeating spawn: `count` entities every `1 / rate_hz` seconds from `start_s`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct SpawnSchedule {
    pub rate_hz: f64,
    pub count: usize,
    pub start_s: f64,
}

/// One entity block: how to build its entities, and when to spawn them.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct EntityConfig {
    /// For messages: the YAML group name, or `block N` for XML.
    pub label: String,
    pub team_id: i32,
    pub color: [u8; 3],
    pub visual_model: String,
    pub health: i32,
    pub requested_id: Option<i32>,
    /// Entities in total; without a schedule they all spawn at the start.
    pub count: usize,
    pub schedule: Option<SpawnSchedule>,
    pub spawn_time_stddev_s: f64,
    pub position_world_m: Vec3,
    pub position_variance_world_m2: Vec3,
    pub heading_deg: f64,
    pub heading_variance_deg2: f64,
    pub roll_deg: f64,
    pub pitch_deg: f64,
    pub velocity_world_mps: Vec3,
    /// Legacy scalar speed, applied only when the velocity is zero.
    pub speed_mps: f64,
    pub randomize_every_spawn: bool,
    pub autonomy: Vec<PluginConfig>,
    pub controllers: Vec<PluginConfig>,
    pub motion: PluginConfig,
    pub sensors: Vec<PluginConfig>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ScenarioConfig {
    pub source: PathBuf,
    pub start_s: f64,
    pub end_s: f64,
    pub dt_s: f64,
    pub motion_multiplier: usize,
    pub seed: u32,
    pub worker_count: usize,
    pub enable_gui: bool,
    pub time_warp: f64,
    pub start_paused: bool,
    pub(crate) end_conditions: EndConditions,
    pub(crate) entities: Vec<EntityConfig>,
    pub(crate) interactions: Vec<PluginConfig>,
    pub(crate) networks: Vec<PluginConfig>,
    pub(crate) metrics: Vec<PluginConfig>,
}

impl ScenarioConfig {
    pub fn resolve(self) -> Result<ResolvedScenario> {
        self.resolve_with_registry(&crate::plugin::PluginRegistry::with_builtins())
    }

    pub fn resolve_with_registry(
        self,
        registry: &crate::plugin::PluginRegistry,
    ) -> Result<ResolvedScenario> {
        ensure!(
            self.dt_s.is_finite() && self.dt_s > 0.0,
            "dt must be positive and finite"
        );
        ensure!(
            self.start_s.is_finite() && self.end_s.is_finite() && self.end_s >= self.start_s,
            "invalid simulation interval"
        );
        ensure!(
            self.motion_multiplier > 0,
            "motion_multiplier must be positive"
        );
        ensure!(self.worker_count > 0, "worker count must be positive");

        let mut definitions = Vec::new();
        let mut generators = Vec::new();
        for (index, entity) in self.entities.iter().enumerate() {
            definitions.push(
                EntityDefinition::parse(entity, registry)
                    .with_context(|| format!("entity {}", entity.label))?,
            );
            generators.push(Generator::new(entity, index, self.start_s)?);
        }

        let world = CompiledWorld::compile(&self, registry)?;
        let end_conditions = self.end_conditions;

        Ok(ResolvedScenario {
            config: self,
            definitions,
            generators,
            world,
            end_conditions,
        })
    }

    /// Where two missions set up the simulation differently, as dotted paths
    /// (`entities.1.heading_deg: 180 vs 0`). Plugin parameter values are left
    /// out: XML holds them as text and YAML as numbers, so compare those by
    /// running both missions.
    pub fn setup_differences(&self, other: &Self) -> Result<Vec<String>> {
        let mut found = Vec::new();
        json_differences("", &outline(self)?, &outline(other)?, &mut found);
        Ok(found)
    }

    pub fn load(path: &Path, root: &Path, overrides: &Params) -> Result<Self> {
        Self::load_with_registry(
            path,
            root,
            overrides,
            &crate::plugin::PluginRegistry::with_builtins(),
        )
    }

    pub fn load_with_registry(
        path: &Path,
        root: &Path,
        overrides: &Params,
        registry: &crate::plugin::PluginRegistry,
    ) -> Result<Self> {
        let mut candidates = vec![path.to_path_buf()];
        if let Some(paths) = std::env::var_os("SCRIMMAGE_MISSION_PATH") {
            candidates.extend(std::env::split_paths(&paths).map(|plugin| plugin.join(path)));
        }
        candidates.push(root.join("missions").join(path));
        let path = candidates
            .iter()
            .find(|plugin| plugin.is_file())
            .with_context(|| format!("mission not found: {}", path.display()))?;
        match path.extension().and_then(|extension| extension.to_str()) {
            Some("yaml" | "yml") => super::yaml_mission::load(path, overrides, registry),
            _ => super::xml_mission::load(path, overrides, registry),
        }
    }
}

/// The mission as JSON, without what cannot change the recorded outputs: the
/// file path, entity labels, and pacing/viewer/worker settings (the command line
/// overrides those, and YAML cannot express some of them). Plugin parameter
/// values are also left out; see `setup_differences`.
fn outline(config: &ScenarioConfig) -> Result<serde_json::Value> {
    let mut value = serde_json::to_value(config)?;
    for field in [
        "source",
        "worker_count",
        "enable_gui",
        "time_warp",
        "start_paused",
    ] {
        value[field] = serde_json::Value::Null;
    }
    if let Some(entities) = value["entities"].as_array_mut() {
        for entity in entities {
            entity["label"] = serde_json::Value::Null;
        }
    }
    strip_plugin_params(&mut value);
    Ok(value)
}

fn strip_plugin_params(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(fields) => {
            // Only a plugin config has a loop rate.
            if fields.contains_key("loop_rate_hz") {
                fields.remove("params");
            }
            fields.values_mut().for_each(strip_plugin_params);
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(strip_plugin_params),
        _ => {}
    }
}

fn json_differences(
    path: &str,
    first: &serde_json::Value,
    second: &serde_json::Value,
    found: &mut Vec<String>,
) {
    use serde_json::Value;
    let child = |key: &str| {
        if path.is_empty() {
            key.to_owned()
        } else {
            format!("{path}.{key}")
        }
    };
    match (first, second) {
        (Value::Object(a), Value::Object(b)) => {
            let missing = Value::Null;
            for key in a.keys().chain(b.keys().filter(|key| !a.contains_key(*key))) {
                let (x, y) = (
                    a.get(key).unwrap_or(&missing),
                    b.get(key).unwrap_or(&missing),
                );
                json_differences(&child(key), x, y, found);
            }
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
            for (index, (x, y)) in a.iter().zip(b).enumerate() {
                json_differences(&child(&index.to_string()), x, y, found);
            }
        }
        (a, b) if a != b => found.push(format!("{path}: {a} vs {b}")),
        _ => {}
    }
}

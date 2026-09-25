//! The experiment description shared by Rust setup code and mission readers.
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_yaml_ng::{Mapping, Value};

use crate::{
    entity::EntityDefinition,
    math::Vec3,
    parse::Params,
    plugin::PluginRegistry,
    simcontrol::{generation::Generator, world_plugins::CompiledWorld},
};

/// Validated configuration and factories, ready for runtime construction.
pub(crate) struct ResolvedScenario {
    pub config: ScenarioConfig,
    pub definitions: Vec<EntityDefinition>,
    pub generators: Vec<Generator>,
    pub world: CompiledWorld,
    pub end_conditions: EndConditions,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Vec<String>")]
pub struct EndConditions {
    pub time: bool,
    pub all_dead: bool,
    pub one_team: bool,
}

impl Default for EndConditions {
    fn default() -> Self {
        Self {
            time: true,
            all_dead: false,
            one_team: false,
        }
    }
}

impl EndConditions {
    pub(crate) fn from_names<'a>(names: impl IntoIterator<Item = &'a str>) -> Result<Self> {
        let mut conditions = Self {
            time: false,
            all_dead: false,
            one_team: false,
        };
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

impl TryFrom<Vec<String>> for EndConditions {
    type Error = anyhow::Error;

    fn try_from(names: Vec<String>) -> Result<Self> {
        Self::from_names(names.iter().map(String::as_str))
    }
}

/// XML keeps text conversions at the parameter boundary; native setup uses values.
#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
pub(crate) enum PluginValues {
    Text(Params),
    Yaml(Mapping),
}

/// A registered behavior and its parameters, configured once for independent instances.
#[derive(Clone, Debug, Serialize)]
pub struct PluginConfig {
    pub name: String,
    /// Stable instance identity, including for a sensor's random stream.
    pub instance: Option<String>,
    /// Zero runs the plugin every step.
    pub loop_rate_hz: f64,
    pub(crate) params: PluginValues,
}

impl PluginConfig {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            instance: None,
            loop_rate_hz: 0.0,
            params: PluginValues::Yaml(Mapping::new()),
        }
    }

    /// Replace parameters with a serializable struct or map. Values must be finite.
    pub fn with_params<T: Serialize>(mut self, params: T) -> Result<Self> {
        let values = serde_yaml_ng::to_value(params)?;
        check_values(&values)?;
        let Value::Mapping(params) = values else {
            bail!("plugin parameters must be a map");
        };
        self.params = PluginValues::Yaml(params);
        Ok(self)
    }

    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            self.loop_rate_hz.is_finite() && self.loop_rate_hz >= 0.0,
            "loop_rate must be finite and nonnegative"
        );
        if let PluginValues::Yaml(params) = &self.params {
            for value in params.values() {
                check_values(value)?;
            }
        }
        Ok(())
    }
}

fn check_values(value: &Value) -> Result<()> {
    match value {
        Value::Tagged(_) => bail!("plugin parameters cannot contain YAML tags"),
        Value::Number(number) => ensure!(
            number.as_f64().is_none_or(f64::is_finite),
            "plugin parameters must be finite"
        ),
        Value::Sequence(items) => {
            for item in items {
                check_values(item)?;
            }
        }
        Value::Mapping(entries) => {
            for (key, item) in entries {
                ensure!(
                    key.as_str().is_some(),
                    "plugin parameter keys must be strings"
                );
                check_values(item)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// A repeating batch, until the group's total `count` has spawned.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spawn {
    pub rate_hz: f64,
    pub batch_size: usize,
    #[serde(default)]
    pub start_s: f64,
    #[serde(default)]
    pub time_stddev_s: f64,
}

/// How to build one group of entities and when to create them.
/// Positions and velocities use the local ENU world frame.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EntityGroupConfig {
    #[serde(skip_deserializing)]
    pub label: String,
    pub team: i32,
    pub color: [u8; 3],
    pub visual_model: String,
    pub health: i32,
    pub id: Option<i32>,
    pub count: usize,
    pub spawn: Option<Spawn>,
    pub position_m: Vec3,
    pub position_variance_m2: Vec3,
    pub heading_deg: f64,
    pub heading_variance_deg2: f64,
    pub roll_deg: f64,
    pub pitch_deg: f64,
    pub velocity_mps: Vec3,
    pub randomize_every_spawn: bool,
    #[serde(deserialize_with = "crate::parse::yaml_mission::plugins")]
    pub autonomy: Vec<PluginConfig>,
    #[serde(deserialize_with = "crate::parse::yaml_mission::plugins")]
    pub controller: Vec<PluginConfig>,
    #[serde(deserialize_with = "crate::parse::yaml_mission::motion_plugin")]
    pub motion_model: PluginConfig,
    #[serde(deserialize_with = "crate::parse::yaml_mission::plugins")]
    pub sensors: Vec<PluginConfig>,
}

impl Default for EntityGroupConfig {
    fn default() -> Self {
        Self {
            label: String::new(),
            team: -1,
            color: [255, 255, 255],
            visual_model: "sphere".into(),
            health: 1,
            id: None,
            count: 1,
            spawn: None,
            position_m: Vec3::zeros(),
            position_variance_m2: Vec3::zeros(),
            heading_deg: 0.0,
            heading_variance_deg2: 0.0,
            roll_deg: 0.0,
            pitch_deg: 0.0,
            velocity_mps: Vec3::zeros(),
            randomize_every_spawn: false,
            autonomy: Vec::new(),
            controller: Vec::new(),
            // Construction rejects a missing model rather than choosing physics implicitly.
            motion_model: PluginConfig::new(""),
            sensors: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RunConfig {
    pub start_s: f64,
    pub end_s: f64,
    pub dt_s: f64,
    pub seed: u32,
    pub motion_multiplier: usize,
}

impl Default for RunConfig {
    fn default() -> Self {
        Self {
            start_s: 0.0,
            end_s: 100.0,
            dt_s: 0.1,
            seed: 2_147_483_648,
            motion_multiplier: 1,
        }
    }
}

/// Plain experiment data; `Simulation::new` validates it before execution.
/// Serialization records normalized data for manifests, not mission-file syntax.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ScenarioConfig {
    pub run: RunConfig,
    pub end_conditions: EndConditions,
    #[serde(deserialize_with = "crate::parse::yaml_mission::entity_groups")]
    pub entities: Vec<EntityGroupConfig>,
    #[serde(deserialize_with = "crate::parse::yaml_mission::plugins")]
    pub interactions: Vec<PluginConfig>,
    #[serde(deserialize_with = "crate::parse::yaml_mission::plugins")]
    pub networks: Vec<PluginConfig>,
    #[serde(deserialize_with = "crate::parse::yaml_mission::plugins")]
    pub metrics: Vec<PluginConfig>,
}

impl ScenarioConfig {
    pub(crate) fn resolve(self, registry: &PluginRegistry) -> Result<ResolvedScenario> {
        ensure!(
            self.run.dt_s.is_finite() && self.run.dt_s > 0.0,
            "dt must be positive and finite"
        );
        ensure!(
            self.run.start_s.is_finite()
                && self.run.end_s.is_finite()
                && self.run.end_s >= self.run.start_s,
            "invalid simulation interval"
        );
        ensure!(
            self.run.motion_multiplier > 0,
            "motion_multiplier must be positive"
        );

        let mut definitions = Vec::new();
        let mut generators = Vec::new();
        for (index, entity) in self.entities.iter().enumerate() {
            entity
                .validate()
                .with_context(|| format!("entity {}", entity.label))?;
            definitions.push(
                EntityDefinition::parse(entity, registry)
                    .with_context(|| format!("entity {}", entity.label))?,
            );
            generators.push(Generator::new(entity, index, self.run.start_s)?);
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

    /// Compare experiment structure; plugin values are compared by running both
    /// scenarios because XML keeps text where native setup keeps typed values.
    pub fn setup_differences(&self, other: &Self) -> Result<Vec<String>> {
        let mut found = Vec::new();
        json_differences("", &outline(self)?, &outline(other)?, &mut found);
        Ok(found)
    }
}

impl EntityGroupConfig {
    fn validate(&self) -> Result<()> {
        ensure!(
            !self.motion_model.name.trim().is_empty(),
            "entity requires one motion_model"
        );
        ensure!(
            self.position_m
                .iter()
                .chain(self.velocity_mps.iter())
                .chain(self.position_variance_m2.iter())
                .all(|value| value.is_finite())
                && [
                    self.heading_deg,
                    self.heading_variance_deg2,
                    self.roll_deg,
                    self.pitch_deg
                ]
                .iter()
                .all(|value| value.is_finite()),
            "entity position, velocity, attitude and variances must be finite"
        );
        ensure!(
            self.position_variance_m2.iter().all(|value| *value >= 0.0)
                && self.heading_variance_deg2 >= 0.0,
            "spawn variances must be nonnegative"
        );
        if let Some(spawn) = &self.spawn {
            ensure!(
                spawn.rate_hz.is_finite() && spawn.rate_hz > 0.0 && spawn.batch_size > 0,
                "spawn requires finite rate_hz > 0 and batch_size > 0"
            );
            ensure!(spawn.start_s.is_finite(), "spawn start_s must be finite");
            ensure!(
                spawn.time_stddev_s.is_finite() && spawn.time_stddev_s >= 0.0,
                "spawn time_stddev_s must be finite and nonnegative"
            );
        }
        Ok(())
    }
}

fn outline(config: &ScenarioConfig) -> Result<serde_json::Value> {
    let mut value = serde_json::to_value(config)?;
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

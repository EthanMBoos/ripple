//! Mission files: finding a YAML mission, expanding its templates and
//! overrides, and reading it into the public scenario types.
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Deserializer, de::Error};
use serde_yaml_ng::{Mapping, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use crate::scenario::{EntityGroupConfig, PluginConfig, ScenarioConfig};

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
        load(&found, overrides)
    }
}

fn load(path: &Path, overrides: &Params) -> Result<Mission> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    parse(&text, path.canonicalize()?, overrides)
        .with_context(|| format!("mission {}", path.display()))
}

fn parse(text: &str, source: PathBuf, overrides: &Params) -> Result<Mission> {
    // Overrides can address fields inherited from a template.
    let mut tree: Value = serde_yaml_ng::from_str(text)?;
    expand_templates(&mut tree)?;
    for (path, value) in overrides {
        let value: Value = serde_yaml_ng::from_str(value)?;
        set_path(&mut tree, path, value)
            .with_context(|| format!("cannot apply override `{path}`"))?;
    }
    check_values(&tree, "")?;
    let root = tree.as_mapping_mut().context("a mission must be a map")?;
    let version = root
        .remove("format_version")
        .context("missing format_version")?;
    ensure!(version.as_u64() == Some(1), "unsupported format_version");
    let name = root.remove("name").context("missing name")?;
    ensure!(name.is_string(), "name must be a string");
    ensure!(root.contains_key("entities"), "missing entities");
    let viewer = match root.get_mut("run") {
        Some(Value::Mapping(run)) => run
            .remove("viewer")
            .map(serde_yaml_ng::from_value)
            .transpose()?
            .unwrap_or(false),
        _ => false,
    };
    let mut scenario: ScenarioConfig = serde_yaml_ng::from_value(tree)?;
    if let (Some(terrain), Some(directory)) = (&mut scenario.terrain, source.parent()) {
        *terrain = directory.join(&*terrain);
    }
    Ok(Mission {
        scenario,
        workers: 1,
        viewer,
        source,
    })
}

/// Preserve file order: it controls entity IDs, random draws, and plugin order.
pub(crate) fn entity_groups<'de, D>(
    deserializer: D,
) -> std::result::Result<Vec<EntityGroupConfig>, D::Error>
where
    D: Deserializer<'de>,
{
    let groups = Mapping::deserialize(deserializer)?;
    let mut entities = Vec::new();
    for (label, value) in groups {
        let label = key(&label).map_err(D::Error::custom)?;
        let mut entity: EntityGroupConfig = serde_yaml_ng::from_value(or_empty(&value))
            .map_err(|error| D::Error::custom(format!("entity {label}: {error}")))?;
        entity.label = label.to_owned();
        entities.push(entity);
    }
    Ok(entities)
}

pub(crate) fn plugins<'de, D>(deserializer: D) -> std::result::Result<Vec<PluginConfig>, D::Error>
where
    D: Deserializer<'de>,
{
    let slot = Mapping::deserialize(deserializer)?;
    plugin_configs(slot).map_err(D::Error::custom)
}

/// One estimator; its `inputs` map is framework data, not a plugin parameter.
pub(crate) fn estimator_plugin<'de, D>(
    deserializer: D,
) -> std::result::Result<PluginConfig, D::Error>
where
    D: Deserializer<'de>,
{
    let mut slot = Mapping::deserialize(deserializer)?;
    let mut inputs = std::collections::BTreeMap::new();
    for (_, value) in slot.iter_mut() {
        if let Value::Mapping(params) = value
            && let Some(map) = params.remove("inputs")
        {
            inputs = serde_yaml_ng::from_value(map)
                .map_err(|error| D::Error::custom(format!("inputs: {error}")))?;
        }
    }
    let mut configs = plugin_configs(slot).map_err(D::Error::custom)?;
    if configs.len() != 1 {
        return Err(D::Error::custom("expected exactly one estimator plugin"));
    }
    let mut config = configs.remove(0);
    config.inputs = inputs;
    Ok(config)
}

pub(crate) fn motion_plugin<'de, D>(deserializer: D) -> std::result::Result<PluginConfig, D::Error>
where
    D: Deserializer<'de>,
{
    let mut configs = plugins(deserializer)?;
    if configs.len() != 1 {
        return Err(D::Error::custom("entity requires one motion_model"));
    }
    Ok(configs.remove(0))
}

/// The map key names the plugin, unless an explicit `plugin` gives its type.
fn plugin_configs(slot: Mapping) -> Result<Vec<PluginConfig>> {
    let mut configs = Vec::new();
    for (label, value) in slot {
        let label = key(&label)?;
        let Value::Mapping(mut params) = or_empty(&value) else {
            bail!("plugin `{label}`: expected a map of parameters");
        };
        let (name, instance) = match params.remove("plugin") {
            Some(Value::String(name)) => (name, Some(label.to_owned())),
            Some(_) => bail!("plugin `{label}`: `plugin` must be a name"),
            None => (label.to_owned(), None),
        };
        let loop_rate_hz = match params.remove("loop_rate_hz") {
            Some(value) => value
                .as_f64()
                .with_context(|| format!("plugin `{label}`: loop_rate_hz must be a number"))?,
            None => 0.0,
        };
        configs.push(PluginConfig {
            name,
            instance,
            loop_rate_hz,
            inputs: Default::default(),
            params,
        });
    }
    Ok(configs)
}

/// Replaces each entity's `template: name` with that template's keys. The
/// entity's own keys win, and each replaces the template's value wholesale:
/// an entity's `autonomy` replaces the template's whole `autonomy` slot.
fn expand_templates(tree: &mut Value) -> Result<()> {
    let Value::Mapping(root) = tree else {
        bail!("a mission must be a map");
    };
    let templates = match root.remove("templates") {
        Some(Value::Mapping(templates)) => templates,
        Some(_) => bail!("templates must be a map of named entity groups"),
        None => Mapping::new(),
    };
    let Some(Value::Mapping(entities)) = root.get_mut("entities") else {
        return Ok(());
    };
    for (label, entity) in entities.iter_mut() {
        let Value::Mapping(fields) = entity else {
            continue;
        };
        let Some(name) = fields.remove("template") else {
            continue;
        };
        let label = key(label)?;
        let name = name
            .as_str()
            .with_context(|| format!("entity {label}: template must be a name"))?;
        let template = match templates.get(name) {
            Some(Value::Mapping(template)) => template,
            Some(_) => bail!("template {name} must be a map"),
            None => bail!("entity {label}: unknown template `{name}`"),
        };
        ensure!(
            !template.contains_key("template"),
            "template {name}: templates cannot use other templates"
        );
        let mut expanded = template.clone();
        for (field, value) in fields.iter() {
            expanded.insert(field.clone(), value.clone());
        }
        *fields = expanded;
    }
    Ok(())
}

/// A bare key (`SimpleAircraft:`) means "no values".
fn or_empty(value: &Value) -> Value {
    match value {
        Value::Null => Value::Mapping(Mapping::new()),
        value => value.clone(),
    }
}

/// A group or plugin label. Dots would make it unreachable by an override path.
fn key(value: &Value) -> Result<&str> {
    let label = value.as_str().context("labels must be names")?;
    ensure!(
        !label.is_empty() && !label.contains('.'),
        "label `{label}` must be non-empty and contain no dots"
    );
    Ok(label)
}

/// YAML accepts `.inf` and `.nan`; the simulation never does. Custom tags
/// (`!name value`) are not part of the format, so they are rejected too.
fn check_values(value: &Value, path: &str) -> Result<()> {
    match value {
        Value::Tagged(tagged) => bail!("{path}: custom YAML tag `{}` is not supported", tagged.tag),
        Value::Number(number) => ensure!(
            number.as_f64().is_none_or(f64::is_finite),
            "{path}: expected a finite number"
        ),
        Value::Sequence(items) => {
            for (index, item) in items.iter().enumerate() {
                check_values(item, &format!("{path}.{index}"))?;
            }
        }
        Value::Mapping(entries) => {
            for (name, item) in entries {
                let name = name.as_str().unwrap_or("?");
                let child = if path.is_empty() {
                    name.to_owned()
                } else {
                    format!("{path}.{name}")
                };
                check_values(item, &child)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Sets a dotted path such as `entities.red.spawn.batch_size`, as a sweep
/// does. Every part must already exist, so a typo is an error, not a new key.
pub(crate) fn set_path(root: &mut Value, path: &str, replacement: Value) -> Result<()> {
    let mut current = root;
    for part in path.split('.') {
        current = match current {
            Value::Mapping(mapping) => mapping
                .get_mut(part)
                .with_context(|| format!("field `{part}` does not exist"))?,
            Value::Sequence(sequence) => {
                let index: usize = part
                    .parse()
                    .with_context(|| format!("`{part}` is not a list index"))?;
                sequence
                    .get_mut(index)
                    .with_context(|| format!("list index {index} is out of bounds"))?
            }
            _ => bail!("`{part}` is below a single value; write the value in the file first"),
        };
    }
    *current = replacement;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{plugin::PluginRegistry, scenario::ScenarioConfig};

    const MISSION: &str = "
format_version: 1
name: test
run:
  end_s: 1
entities:
  blue:
    team: 1
    autonomy:
      Straight:
        speed_mps: 25
    controller:
      SimpleAircraftControllerPID:
    motion_model:
      SimpleAircraft:
    sensors:
      coarse:
        plugin: NoisyState
        position_stddev_m: [5, 5, 0]
      NoisyState:
    localization:
      PassThrough:
        inputs: {state: coarse}
";

    fn load(text: &str, overrides: &[(&str, &str)]) -> Result<ScenarioConfig> {
        let overrides = overrides
            .iter()
            .map(|(path, value)| (path.to_string(), value.to_string()))
            .collect();
        parse(text, PathBuf::from("test.yaml"), &overrides).map(|mission| mission.scenario)
    }

    fn error(text: &str, overrides: &[(&str, &str)]) -> String {
        let result = load(text, overrides)
            .and_then(|scenario| scenario.resolve(&PluginRegistry::with_builtins()));
        format!("{:#}", result.err().expect("mission must be rejected"))
    }

    #[test]
    fn labels_defaults_and_plugin_values_resolve() -> Result<()> {
        let mission = load(MISSION, &[])?;
        let entity = &mission.entities[0];
        assert_eq!(entity.team, 1);
        assert_eq!(entity.visual_model, "aircraft");
        assert_eq!(entity.position_variance_m2, crate::math::Vec3::zeros());
        // A label naming another plugin becomes the sensor's identity.
        assert_eq!(entity.sensors[0].name, "NoisyState");
        assert_eq!(entity.sensors[0].instance.as_deref(), Some("coarse"));
        assert_eq!(entity.sensors[1].instance, None);
        let resolved = mission.resolve(&PluginRegistry::with_builtins())?;
        // Loading records every estimator input's source, including defaults.
        let entity = &resolved.config.entities[0];
        assert_eq!(entity.localization.inputs["state"], "coarse");
        assert_eq!(entity.tracking.inputs["contacts"], "perfect");
        Ok(())
    }

    #[test]
    fn spatial_lookup_defaults_to_scan_and_selects_a_grid() -> Result<()> {
        use crate::scenario::SpatialLookup;
        assert_eq!(load(MISSION, &[])?.spatial_lookup, SpatialLookup::Scan);
        let grid = MISSION.replace(
            "run:\n",
            "spatial_lookup: {method: grid, cell_width_m: 5}\nrun:\n",
        );
        assert_eq!(
            load(&grid, &[])?.spatial_lookup,
            SpatialLookup::Grid { cell_width_m: 5.0 }
        );
        assert_eq!(
            load(&grid, &[("spatial_lookup.cell_width_m", "2")])?.spatial_lookup,
            SpatialLookup::Grid { cell_width_m: 2.0 }
        );
        let misspelled = grid.replace("cell_width_m", "cell_width");
        assert!(load(&misspelled, &[]).is_err());
        assert!(error(&grid.replace(": 5}", ": 0}"), &[]).contains("cell_width_m"));
        Ok(())
    }

    #[test]
    fn estimator_inputs_are_chosen_only_when_unambiguous() -> Result<()> {
        let without_inputs = MISSION.replace("        inputs: {state: coarse}\n", "");
        let message = error(&without_inputs, &[]);
        assert!(
            message.contains("could use any of the sensors coarse, NoisyState:1"),
            "{message}"
        );
        let unknown = MISSION.replace("state: coarse", "state: gps");
        assert!(error(&unknown, &[]).contains("names sensor 'gps'"));
        let misspelled = MISSION.replace("state: coarse", "stat: coarse");
        assert!(error(&misspelled, &[]).contains("has no input 'stat'"));
        // Perfect localization, while the noisy sensors feed nothing.
        let perfect = load(&MISSION.replace("state: coarse", "state: perfect"), &[])?
            .resolve(&PluginRegistry::with_builtins())?;
        assert_eq!(
            perfect.config.entities[0].localization.inputs["state"],
            "perfect"
        );
        Ok(())
    }

    #[test]
    fn file_execution_defaults_stay_outside_the_scenario() -> Result<()> {
        let text = MISSION.replace("  end_s: 1", "  end_s: 1\n  viewer: true");
        let mission = parse(&text, PathBuf::from("test.yaml"), &Params::new())?;
        assert!(mission.viewer);
        assert_eq!(mission.workers, 1);
        assert_eq!(mission.source, PathBuf::from("test.yaml"));
        assert_eq!(mission.scenario.run.end_s, 1.0);
        assert!(mission.scenario.end_conditions.time);
        Ok(())
    }

    #[test]
    fn overrides_follow_dotted_paths_that_exist() -> Result<()> {
        let mission = load(MISSION, &[("entities.blue.team", "3"), ("run.end_s", "2")])?;
        assert_eq!(mission.entities[0].team, 3);
        assert_eq!(mission.run.end_s, 2.0);
        for (path, message) in [
            ("entities.blue.teem", "field `teem` does not exist"),
            (
                "entities.blue.motion_model.SimpleAircraft.speed_mps",
                "below a single value",
            ),
        ] {
            assert!(error(MISSION, &[(path, "1")]).contains(message), "{path}");
        }
        Ok(())
    }

    #[test]
    fn unknown_keys_bad_values_and_non_finite_numbers_are_errors() {
        for (from, to, message) in [
            ("team: 1", "teem: 1", "unknown field `teem`"),
            ("speed_mps: 25", "sped_mps: 25", "unknown field `sped_mps`"),
            (
                "speed_mps: 25",
                "speed_mps: fast",
                "speed_mps: invalid type",
            ),
            (
                "speed_mps: 25",
                "speed_mps: .inf",
                "entities.blue.autonomy.Straight.speed_mps: expected a finite number",
            ),
            (
                "SimpleAircraft:",
                "SimpleAircraftt:",
                "unregistered Motion plugin",
            ),
            (
                "format_version: 1",
                "format_version: 2",
                "unsupported format_version",
            ),
            ("team: 1", "team: !custom 1", "custom YAML tag `!custom`"),
            (
                "blue:",
                "blue.leader:",
                "must be non-empty and contain no dots",
            ),
        ] {
            let message_seen = error(&MISSION.replace(from, to), &[]);
            assert!(message_seen.contains(message), "{to}: {message_seen}");
        }
    }

    const TEMPLATED: &str = "
format_version: 1
name: test
templates:
  aircraft:
    team: 1
    heading_deg: 90
    autonomy:
      Straight:
        speed_mps: 25
    controller:
      SimpleAircraftControllerPID:
    motion_model:
      SimpleAircraft:
entities:
  blue:
    template: aircraft
  red:
    template: aircraft
    team: 2
    autonomy:
      Straight:
";

    #[test]
    fn templates_fill_groups_and_own_keys_replace_whole_values() -> Result<()> {
        let mission = load(TEMPLATED, &[])?;
        let (blue, red) = (&mission.entities[0], &mission.entities[1]);
        assert_eq!((blue.team, blue.heading_deg), (1, 90.0));
        assert_eq!((red.team, red.heading_deg), (2, 90.0));
        // Red's own `autonomy` replaced the template's, parameters and all.
        assert!(red.autonomy[0].params.is_empty());
        mission.resolve(&PluginRegistry::with_builtins())?;
        Ok(())
    }

    #[test]
    fn overrides_reach_inherited_fields_but_not_templates() -> Result<()> {
        let mission = load(
            TEMPLATED,
            &[("entities.blue.autonomy.Straight.speed_mps", "30")],
        )?;
        assert_eq!(
            mission.entities[0].autonomy[0].params["speed_mps"],
            Value::from(30)
        );
        let message = error(TEMPLATED, &[("templates.aircraft.team", "3")]);
        assert!(
            message.contains("field `templates` does not exist"),
            "{message}"
        );
        Ok(())
    }

    #[test]
    fn unknown_and_nested_templates_are_errors() {
        let unknown = TEMPLATED.replace("template: aircraft\n  red", "template: jet\n  red");
        assert!(error(&unknown, &[]).contains("unknown template `jet`"));
        let nested = TEMPLATED.replace("    team: 1\n", "    team: 1\n    template: other\n");
        assert!(error(&nested, &[]).contains("cannot use other templates"));
    }

    #[test]
    fn a_plugin_without_parameters_rejects_values() {
        let text = MISSION.replace(
            "      SimpleAircraftControllerPID:\n",
            "      SimpleAircraftControllerPID:\n        gain: 1\n",
        );
        assert!(error(&text, &[]).contains("unknown field `gain`"));
    }
}

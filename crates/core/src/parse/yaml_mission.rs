//! Reads a native YAML mission into the typed `ScenarioConfig`.
//! The syntax is described in `book/src/guides/yaml-missions.md`.
use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use serde_yaml_ng::{Mapping, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};

use super::Params;
use super::mission::{
    EndConditions, EntityConfig, PluginConfig, PluginValues, ScenarioConfig, SpawnSchedule,
};
use crate::math::Vec3;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MissionFile {
    format_version: u32,
    /// Shown in output; not otherwise used.
    #[serde(rename = "name")]
    _name: String,
    #[serde(default)]
    run: RunSection,
    #[serde(default = "default_end_conditions")]
    end_conditions: Vec<String>,
    #[serde(default)]
    interactions: Mapping,
    #[serde(default)]
    networks: Mapping,
    #[serde(default)]
    metrics: Mapping,
    entities: Mapping,
}

fn default_end_conditions() -> Vec<String> {
    vec!["time".into()]
}

/// The same defaults as an XML `<run>` tag that leaves an attribute out.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RunSection {
    start_s: f64,
    end_s: f64,
    dt_s: f64,
    seed: u32,
    motion_multiplier: usize,
    /// Open the Rerun viewer unless the command line says `--headless`.
    viewer: bool,
}

impl Default for RunSection {
    fn default() -> Self {
        Self {
            start_s: 0.0,
            end_s: 100.0,
            dt_s: 0.1,
            seed: 2_147_483_648,
            motion_multiplier: 1,
            viewer: false,
        }
    }
}

/// The same defaults as an XML entity block that leaves a tag out.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct EntityFile {
    team: i32,
    color: [u8; 3],
    visual_model: String,
    health: i32,
    id: Option<i32>,
    count: usize,
    spawn: Option<SpawnFile>,
    position_m: [f64; 3],
    position_variance_m2: [f64; 3],
    heading_deg: f64,
    heading_variance_deg2: f64,
    roll_deg: f64,
    pitch_deg: f64,
    velocity_mps: [f64; 3],
    speed_mps: f64,
    randomize_every_spawn: bool,
    autonomy: Mapping,
    controller: Mapping,
    motion_model: Mapping,
    sensors: Mapping,
}

impl Default for EntityFile {
    fn default() -> Self {
        Self {
            team: -1,
            color: [255, 255, 255],
            visual_model: "sphere".into(),
            health: 1,
            id: None,
            count: 1,
            spawn: None,
            position_m: [0.0; 3],
            position_variance_m2: [100.0, 100.0, 0.0],
            heading_deg: 0.0,
            heading_variance_deg2: 0.0,
            roll_deg: 0.0,
            pitch_deg: 0.0,
            velocity_mps: [0.0; 3],
            speed_mps: 0.0,
            randomize_every_spawn: false,
            autonomy: Mapping::new(),
            controller: Mapping::new(),
            motion_model: Mapping::new(),
            sensors: Mapping::new(),
        }
    }
}

/// `batch_size` entities every `1 / rate_hz` seconds from `start_s`, until the
/// entity's `count` have spawned.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SpawnFile {
    rate_hz: f64,
    batch_size: usize,
    #[serde(default)]
    start_s: f64,
    #[serde(default)]
    time_stddev_s: f64,
}

pub(super) fn load(
    path: &Path,
    overrides: &Params,
    registry: &crate::plugin::PluginRegistry,
) -> Result<ScenarioConfig> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    parse(&text, path.canonicalize()?, overrides, registry)
        .with_context(|| format!("mission {}", path.display()))
}

fn parse(
    text: &str,
    source: PathBuf,
    overrides: &Params,
    registry: &crate::plugin::PluginRegistry,
) -> Result<ScenarioConfig> {
    // Order: expand templates, then apply overrides, so an override can
    // reach a field a group inherited.
    let mut tree: Value = serde_yaml_ng::from_str(text)?;
    expand_templates(&mut tree)?;
    for (path, value) in overrides {
        let value: Value = serde_yaml_ng::from_str(value)?;
        set_path(&mut tree, path, value)
            .with_context(|| format!("cannot apply override `{path}`"))?;
    }
    check_values(&tree, "")?;
    let file: MissionFile = serde_yaml_ng::from_value(tree)?;
    ensure!(file.format_version == 1, "unsupported format_version");

    let mut entities = Vec::new();
    for (label, value) in &file.entities {
        let label = key(label)?;
        let entity: EntityFile = serde_yaml_ng::from_value(or_empty(value))
            .with_context(|| format!("entity {label}"))?;
        entities.push(
            entity_config(label, entity, registry).with_context(|| format!("entity {label}"))?,
        );
    }

    Ok(ScenarioConfig {
        source,
        start_s: file.run.start_s,
        end_s: file.run.end_s,
        dt_s: file.run.dt_s,
        motion_multiplier: file.run.motion_multiplier,
        seed: file.run.seed,
        worker_count: 1,
        enable_gui: file.run.viewer,
        time_warp: 0.0,
        start_paused: false,
        end_conditions: EndConditions::from_names(file.end_conditions.iter().map(String::as_str))?,
        entities,
        interactions: plugins(&file.interactions, "entity_interaction", registry)?,
        networks: plugins(&file.networks, "network", registry)?,
        metrics: plugins(&file.metrics, "metrics", registry)?,
    })
}

fn entity_config(
    label: &str,
    entity: EntityFile,
    registry: &crate::plugin::PluginRegistry,
) -> Result<EntityConfig> {
    let schedule = match &entity.spawn {
        Some(spawn) => {
            ensure!(
                spawn.rate_hz > 0.0 && spawn.batch_size > 0,
                "spawn requires rate_hz > 0 and batch_size > 0"
            );
            Some(SpawnSchedule {
                rate_hz: spawn.rate_hz,
                count: spawn.batch_size,
                start_s: spawn.start_s,
            })
        }
        None => None,
    };
    let mut motion = plugins(&entity.motion_model, "motion_model", registry)?;
    ensure!(motion.len() == 1, "entity requires one motion_model");
    Ok(EntityConfig {
        label: label.to_owned(),
        team_id: entity.team,
        color: entity.color,
        visual_model: entity.visual_model,
        health: entity.health,
        requested_id: entity.id,
        count: entity.count,
        schedule,
        spawn_time_stddev_s: entity
            .spawn
            .as_ref()
            .map_or(0.0, |spawn| spawn.time_stddev_s),
        position_world_m: Vec3::from(entity.position_m),
        position_variance_world_m2: Vec3::from(entity.position_variance_m2),
        heading_deg: entity.heading_deg,
        heading_variance_deg2: entity.heading_variance_deg2,
        roll_deg: entity.roll_deg,
        pitch_deg: entity.pitch_deg,
        velocity_world_mps: Vec3::from(entity.velocity_mps),
        speed_mps: entity.speed_mps,
        randomize_every_spawn: entity.randomize_every_spawn,
        autonomy: plugins(&entity.autonomy, "autonomy", registry)?,
        controllers: plugins(&entity.controller, "controller", registry)?,
        motion: motion.remove(0),
        sensors: plugins(&entity.sensors, "sensor", registry)?,
    })
}

/// One plugin slot: each key is a label, and its map holds the plugin's
/// parameters plus the framework keys `plugin` and `loop_rate`.
fn plugins(
    slot: &Mapping,
    category: &str,
    registry: &crate::plugin::PluginRegistry,
) -> Result<Vec<PluginConfig>> {
    let mut configs = Vec::new();
    for (label, value) in slot {
        let label = key(label)?;
        let Value::Mapping(mut params) = or_empty(value) else {
            bail!("{category} `{label}`: expected a map of parameters");
        };
        // A label that is not the plugin's name also names a sensor's random stream.
        let (name, instance) = match params.remove("plugin") {
            Some(Value::String(name)) => (name, Some(label.to_owned())),
            Some(_) => bail!("{category} `{label}`: `plugin` must be a name"),
            None => (label.to_owned(), None),
        };
        ensure!(
            registry.contains_in_category(category, &name),
            "Mission requests {category} '{name}', but it isn't registered in this executable."
        );
        let loop_rate_hz = match params.remove("loop_rate") {
            Some(value) => value
                .as_f64()
                .with_context(|| format!("{category} `{label}`: loop_rate must be a number"))?,
            None => 0.0,
        };
        configs.push(PluginConfig {
            name,
            instance,
            loop_rate_hz,
            params: PluginValues::Yaml(params),
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

/// Sets a dotted path such as `entities.red.spawn.batch_size`, as a Ripple sweep
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
    use crate::plugin::PluginRegistry;

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
        speed: 25
    controller:
      SimpleAircraftControllerPID:
    motion_model:
      SimpleAircraft:
    sensors:
      coarse:
        plugin: NoisyState
        pos_noise_0: [0, 5]
      NoisyState:
";

    fn load(text: &str, overrides: &[(&str, &str)]) -> Result<ScenarioConfig> {
        let overrides = overrides
            .iter()
            .map(|(path, value)| (path.to_string(), value.to_string()))
            .collect();
        parse(
            text,
            PathBuf::from("test.yaml"),
            &overrides,
            &PluginRegistry::with_builtins(),
        )
    }

    fn error(text: &str, overrides: &[(&str, &str)]) -> String {
        let result = load(text, overrides).and_then(ScenarioConfig::resolve);
        format!("{:#}", result.err().expect("mission must be rejected"))
    }

    #[test]
    fn labels_defaults_and_plugin_values_resolve() -> Result<()> {
        let mission = load(MISSION, &[])?;
        let entity = &mission.entities[0];
        assert_eq!(entity.team_id, 1);
        assert_eq!(entity.visual_model, "sphere");
        assert_eq!(
            entity.position_variance_world_m2,
            Vec3::new(100.0, 100.0, 0.0)
        );
        // A label naming another plugin becomes the sensor's identity.
        assert_eq!(entity.sensors[0].name, "NoisyState");
        assert_eq!(entity.sensors[0].instance.as_deref(), Some("coarse"));
        assert_eq!(entity.sensors[1].instance, None);
        mission.resolve()?;
        Ok(())
    }

    #[test]
    fn overrides_follow_dotted_paths_that_exist() -> Result<()> {
        let mission = load(MISSION, &[("entities.blue.team", "3"), ("run.end_s", "2")])?;
        assert_eq!(mission.entities[0].team_id, 3);
        assert_eq!(mission.end_s, 2.0);
        for (path, message) in [
            ("entities.blue.teem", "field `teem` does not exist"),
            (
                "entities.blue.motion_model.SimpleAircraft.speed",
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
            ("speed: 25", "sped: 25", "unknown field `sped`"),
            ("speed: 25", "speed: fast", "speed: invalid type"),
            (
                "speed: 25",
                "speed: .inf",
                "entities.blue.autonomy.Straight.speed: expected a finite number",
            ),
            ("SimpleAircraft:", "SimpleAircraftt:", "isn't registered"),
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
        speed: 25
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
        assert_eq!((blue.team_id, blue.heading_deg), (1, 90.0));
        assert_eq!((red.team_id, red.heading_deg), (2, 90.0));
        // Red's own `autonomy` replaced the template's, parameters and all.
        let PluginValues::Yaml(params) = &red.autonomy[0].params else {
            panic!("YAML plugins hold YAML values");
        };
        assert!(params.is_empty());
        mission.resolve()?;
        Ok(())
    }

    #[test]
    fn overrides_reach_inherited_fields_but_not_templates() -> Result<()> {
        let mission = load(
            TEMPLATED,
            &[("entities.blue.autonomy.Straight.speed", "30")],
        )?;
        let PluginValues::Yaml(params) = &mission.entities[0].autonomy[0].params else {
            panic!("YAML plugins hold YAML values");
        };
        assert_eq!(params["speed"], Value::from(30));
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

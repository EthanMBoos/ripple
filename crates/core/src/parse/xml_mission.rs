//! Reads a legacy XML mission into the typed `ScenarioConfig`.
//! Every XML-only rule (tag names, text values, `entity_common`, `param_common`,
//! plugin overlays) stays in this file.
use anyhow::{Context, Result, bail, ensure};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use super::mission::{
    EndConditions, EntityConfig, PluginConfig, PluginValues, ScenarioConfig, SpawnSchedule,
};
use super::xml::{Node, read_xml};
use super::{Params, boolean, integer, number, vector};
use crate::math::Vec3;

pub(super) fn load(
    path: &Path,
    overrides: &Params,
    registry: &crate::plugin::PluginRegistry,
) -> Result<ScenarioConfig> {
    let expanded_xml = read_xml(path, overrides, &mut Vec::new())?;
    ensure!(expanded_xml.name == "runscript", "expected runscript root");
    let run = &expanded_xml
        .children
        .iter()
        .find(|node| node.name == "run")
        .context("missing run element")?
        .attrs;
    // Plugin defaults live in each plugin's `Default`. Optional overlay files
    // on SCRIMMAGE_PLUGIN_PATH replace individual defaults for every use.
    let mut plugins = BTreeMap::new();
    if let Some(paths) = std::env::var_os("SCRIMMAGE_PLUGIN_PATH") {
        for plugin_path in std::env::split_paths(&paths) {
            collect_xml(&plugin_path, &mut plugins)?;
        }
    }
    let mut parameter_groups = BTreeMap::new();
    let mut entity_groups = BTreeMap::new();
    for node in &expanded_xml.children {
        let Some(name) = node.attrs.get("name") else {
            continue;
        };
        match node.name.as_str() {
            "param_common" => {
                parameter_groups.insert(name, node.params());
            }
            "entity_common" => {
                entity_groups.insert(name, node);
            }
            _ => {}
        }
    }

    let parse_plugin = |node: &Node| -> Result<PluginConfig> {
        let name = &node.text;
        ensure!(
            registry.contains_in_category(&node.name, name),
            "Mission requests {} '{name}', but it isn't registered in this executable.",
            node.name
        );
        let mut params = if let Some(path) = plugins.get(name) {
            read_xml(path, overrides, &mut Vec::new())?.params()
        } else {
            Params::new()
        };
        // C++ overlay files name their shared library; Rust plugins are compiled in.
        params.remove("library");

        if let Some(group_name) = node.attrs.get("param_common") {
            let group = parameter_groups
                .get(group_name)
                .with_context(|| format!("unknown param_common {group_name}"))?;
            params.extend(
                group
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone())),
            );
        }
        params.extend(
            node.attrs
                .iter()
                .filter(|(key, _)| key.as_str() != "param_common")
                .map(|(key, value)| (key.clone(), value.clone())),
        );
        // Framework keys; the plugin's own parameter struct never sees them.
        let loop_rate_hz =
            number(&params, "loop_rate", 0.0).with_context(|| format!("plugin '{name}'"))?;
        params.remove("loop_rate");
        let instance = params.remove("instance");
        Ok(PluginConfig {
            name: name.clone(),
            instance,
            loop_rate_hz,
            params: PluginValues::Text(params),
        })
    };

    let mut entities = Vec::new();
    for (index, entity_node) in expanded_xml
        .children
        .iter()
        .filter(|node| node.name == "entity")
        .enumerate()
    {
        let mut children = Vec::new();
        if let Some(group_name) = entity_node.attrs.get("entity_common") {
            let group = entity_groups
                .get(group_name)
                .with_context(|| format!("unknown entity_common {group_name}"))?;
            children.extend(group.children.iter());
        }
        children.extend(entity_node.children.iter());

        let mut entity_params = Params::new();
        let mut autonomy = Vec::new();
        let mut controllers = Vec::new();
        let mut sensors = Vec::new();
        let mut motion = None;
        for node in children {
            entity_params.insert(node.name.clone(), node.text.clone());
            match node.name.as_str() {
                "autonomy" => autonomy.push(parse_plugin(node)?),
                "controller" => controllers.push(parse_plugin(node)?),
                "sensor" => sensors.push(parse_plugin(node)?),
                "motion_model" => {
                    ensure!(motion.is_none(), "entity requires one motion_model");
                    motion = Some(parse_plugin(node)?);
                }
                _ => {}
            }
        }
        let motion = motion.context("entity requires one motion_model")?;
        let label = format!("block {index}");
        entities.push(
            entity(
                label.clone(),
                &entity_params,
                autonomy,
                controllers,
                motion,
                sensors,
            )
            .with_context(|| format!("entity {label}"))?,
        );
    }

    let mut interactions = Vec::new();
    let mut networks = Vec::new();
    let mut metrics = Vec::new();
    for node in &expanded_xml.children {
        match node.name.as_str() {
            "entity_interaction" => interactions.push(parse_plugin(node)?),
            "network" => networks.push(parse_plugin(node)?),
            "metrics" => metrics.push(parse_plugin(node)?),
            _ => {}
        }
    }
    let params = expanded_xml.params();
    let start_s = number(run, "start", 0.0)?;
    let end_s = number(run, "end", 100.0)?;
    let dt_s = number(run, "dt", 0.1)?;
    ensure!(
        dt_s > 0.0 && end_s >= start_s,
        "run requires dt > 0 and end >= start"
    );
    let motion_multiplier = integer(run, "motion_multiplier", 1)?;
    ensure!(motion_multiplier > 0, "motion_multiplier must be positive");
    let multithread = expanded_xml
        .children
        .iter()
        .find(|node| node.name == "multi_threaded");
    let worker_count = if boolean(&params, "multi_threaded", false)? {
        integer(
            &multithread
                .context("missing threading configuration")?
                .attrs,
            "num_threads",
            1,
        )?
    } else {
        1
    };
    ensure!(worker_count > 0, "num_threads must be positive");
    let seed = integer(&params, "seed", 2_147_483_648)?;
    ensure!(seed <= u32::MAX as usize, "seed exceeds uint32");
    let end_conditions = EndConditions::from_names(
        params
            .get("end_condition")
            .map_or("time", String::as_str)
            .split(',')
            .map(str::trim),
    )?;
    Ok(ScenarioConfig {
        source: path.canonicalize()?,
        start_s,
        end_s,
        dt_s,
        motion_multiplier,
        seed: seed as u32,
        worker_count,
        enable_gui: boolean(run, "enable_gui", false)?,
        time_warp: number(run, "time_warp", 0.0)?,
        start_paused: boolean(run, "start_paused", false)?,
        end_conditions,
        entities,
        interactions,
        networks,
        metrics,
    })
}

/// An entity block's own tags (not its plugins), converted from text.
fn entity(
    label: String,
    params: &Params,
    autonomy: Vec<PluginConfig>,
    controllers: Vec<PluginConfig>,
    motion: PluginConfig,
    sensors: Vec<PluginConfig>,
) -> Result<EntityConfig> {
    if params.contains_key("gpu_motion_model") {
        bail!("not yet implemented: GPU motion execution");
    }
    if params.contains_key("latitude") || params.contains_key("longitude") {
        bail!("not yet implemented: geodetic entity initialization");
    }
    let count = integer(params, "count", 1)?;
    // A schedule needs both tags and a positive rate and count; otherwise every
    // entity spawns at the start, as in C++.
    let mut schedule = None;
    if let (Some(rate_text), Some(_)) = (params.get("generate_rate"), params.get("generate_count"))
    {
        let values: Vec<f64> = rate_text
            .split('/')
            .map(|s| s.trim().parse())
            .collect::<std::result::Result<_, _>>()?;
        let rate_hz = match values.as_slice() {
            [a, b] => a / b,
            [a] => *a,
            _ => bail!("invalid generate_rate"),
        };
        let batch_count = integer(params, "generate_count", 1)?;
        if rate_hz > 0.0 && batch_count > 0 {
            schedule = Some(SpawnSchedule {
                rate_hz,
                count: batch_count,
                start_s: number(params, "generate_start_time", 0.0)?,
            });
        }
    }
    Ok(EntityConfig {
        label,
        team_id: params
            .get("team_id")
            .map_or(Ok(-1), |value| value.parse())
            .context("invalid team_id")?,
        color: vector(params, "color", [255.0, 255.0, 255.0])?
            .map(|channel| channel.clamp(0.0, 255.0) as u8),
        visual_model: params
            .get("visual_model")
            .cloned()
            .unwrap_or_else(|| "sphere".into()),
        health: params
            .get("health")
            .map_or(Ok(1), |value| value.parse())
            .context("invalid health")?,
        requested_id: params
            .get("id")
            .map(|value| value.parse())
            .transpose()
            .context("invalid entity id")?,
        count,
        schedule,
        // C++ names it a variance but uses it as a standard deviation.
        spawn_time_stddev_s: number(params, "generate_time_variance", 0.0)?,
        position_world_m: Vec3::new(
            number(params, "x", 0.0)?,
            number(params, "y", 0.0)?,
            number(params, "z", 0.0)?,
        ),
        position_variance_world_m2: Vec3::new(
            number(params, "variance_x", 100.0)?,
            number(params, "variance_y", 100.0)?,
            number(params, "variance_z", 0.0)?,
        ),
        heading_deg: number(params, "heading", 0.0)?,
        heading_variance_deg2: number(params, "variance_heading", 0.0)?,
        roll_deg: number(params, "roll", 0.0)?,
        pitch_deg: number(params, "pitch", 0.0)?,
        velocity_world_mps: Vec3::new(
            number(params, "vx", 0.0)?,
            number(params, "vy", 0.0)?,
            number(params, "vz", 0.0)?,
        ),
        speed_mps: number(params, "speed", 0.0)?,
        randomize_every_spawn: boolean(params, "use_variance_all_ents", false)?,
        autonomy,
        controllers,
        motion,
        sensors,
    })
}

fn collect_xml(directory: &Path, paths_by_name: &mut BTreeMap<String, PathBuf>) -> Result<()> {
    if !directory.is_dir() {
        return Ok(());
    }
    let mut entries = fs::read_dir(directory)?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            collect_xml(&path, paths_by_name)?;
        } else if path.extension().is_some_and(|extension| extension == "xml") {
            let name = path
                .file_stem()
                .context("XML filename has no stem")?
                .to_string_lossy()
                .into_owned();
            paths_by_name.entry(name).or_insert(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn text(plugin: &PluginConfig) -> &Params {
        let PluginValues::Text(params) = &plugin.params else {
            panic!("XML plugins hold text");
        };
        params
    }

    #[test]
    fn mission_values_and_param_common_reach_the_plugin() -> Result<()> {
        let root = root();
        let mission = ScenarioConfig::load(
            &root.join("crates/core/tests/fixtures/plugin_defaults.xml"),
            &root,
            &Params::new(),
        )?;
        // Omitted keys are left to each plugin's `Default`.
        let defaults = &mission.entities[0];
        assert!(text(&defaults.autonomy[0]).is_empty());
        assert!(text(&defaults.motion).is_empty());

        let overrides = &mission.entities[1];
        assert_eq!(text(&overrides.autonomy[0])["speed"], "26");
        assert_eq!(text(&overrides.motion)["turning_radius"], "23");
        assert_eq!(text(&overrides.motion)["min_velocity"], "16");
        assert!(!text(&overrides.motion).contains_key("param_common"));
        assert_eq!(text(&overrides.sensors[0])["stddev_m"], "0");
        mission.resolve()?;
        Ok(())
    }

    #[test]
    fn misspelled_plugin_parameters_are_errors() -> Result<()> {
        let root = root();
        let mut mission = ScenarioConfig::load(
            &root.join("crates/core/tests/fixtures/plugin_defaults.xml"),
            &root,
            &Params::new(),
        )?;
        let PluginValues::Text(params) = &mut mission.entities[1].motion.params else {
            panic!("XML plugins hold text");
        };
        params.insert("turning_radus".into(), "30".into());
        let error = format!("{:#}", mission.resolve().err().expect("unknown key"));
        assert!(error.contains("unknown field `turning_radus`"), "{error}");
        Ok(())
    }

    #[test]
    fn plugin_path_overlays_are_found_first() -> Result<()> {
        let root = root();
        let overlay = root.join("crates/core/tests/fixtures/plugin_overlay");
        let mut plugins = BTreeMap::new();
        collect_xml(&overlay, &mut plugins)?;
        assert_eq!(
            plugins["SimpleAircraft"],
            overlay.join("SimpleAircraft.xml")
        );
        let defaults = read_xml(&plugins["SimpleAircraft"], &Params::new(), &mut Vec::new())?;
        assert_eq!(defaults.params()["turning_radius"], "37");
        Ok(())
    }
}

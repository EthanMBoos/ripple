//! Run the delivery mission with fake model clients and check its results.
//! The fakes are small scripts, so the tests make no real model calls.
#![cfg(unix)]

use anyhow::Result;
use ripple_core::{Mission, Params, Simulation, plugin::PluginRegistry};
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const DISPATCHER: &str = "entities.depot.autonomy.Dispatcher";

/// Always asks fixed-wing vehicle 2 to land request 1, which it cannot do.
const IMPOSSIBLE: &str = "#!/bin/sh\ncat > /dev/null\n\
    echo '{\"assignments\": [{\"vehicle\": 2, \"skill\": \"land\", \"request\": 1}]}'\n";

/// Sends idle fixed-wing vehicle 2 to the first waiting request that isn't on
/// a rooftop, and appends every prompt to `prompts.txt` beside itself.
const FIXED_WING_TWO: &str = r#"#!/usr/bin/env python3
import json, os, re, sys
prompt = sys.stdin.read()
with open(os.path.join(os.path.dirname(__file__), "prompts.txt"), "a") as log:
    log.write(prompt + "\n=====\n")
calls = []
if "vehicle 2: fixed-wing, idle" in prompt:
    for request, rest in re.findall(r"- request (\d+): (.*)", prompt):
        if "(rooftop)" not in rest:
            calls = [{"vehicle": 2, "skill": "drop", "request": int(request)}]
            break
print(json.dumps({"assignments": calls}))
"#;

/// Sends idle fixed-wing vehicle 2 to the first waiting request that isn't on
/// a rooftop by `drop_via` through (1, 1) km, when the prompt offers it.
const VIA: &str = r#"#!/usr/bin/env python3
import json, os, re, sys
prompt = sys.stdin.read()
with open(os.path.join(os.path.dirname(__file__), "prompts.txt"), "a") as log:
    log.write(prompt + "\n=====\n")
calls = []
if "- drop_via:" in prompt and "vehicle 2: fixed-wing, idle" in prompt:
    for request, rest in re.findall(r"- request (\d+): (.*)", prompt):
        if "(rooftop)" not in rest:
            calls = [{"vehicle": 2, "skill": "drop_via", "request": int(request), "via": [1, 1]}]
            break
print(json.dumps({"assignments": calls}))
"#;

/// A fresh folder holding a fake client and its cache.
fn scratch(name: &str, client: &str) -> Result<PathBuf> {
    let folder = std::env::temp_dir().join(format!("delivery-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder)?;
    let path = folder.join("fake-model");
    std::fs::write(&path, client)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
    Ok(folder)
}

fn set(name: &str, value: impl ToString) -> (String, String) {
    (format!("{DISPATCHER}.{name}"), value.to_string())
}

/// The fake client in `folder`, saving replies there; `saved_only` uses only
/// the saved replies.
fn model(folder: &Path, saved_only: bool) -> Vec<(String, String)> {
    let command = if saved_only {
        "none".to_string()
    } else {
        folder.join("fake-model").display().to_string()
    };
    vec![
        set("command", command),
        set("cache_dir", folder.join("cache").display()),
    ]
}

/// The fleet's delivery results after a whole run.
fn run(overrides: &[(String, String)], workers: usize) -> Result<BTreeMap<String, f64>> {
    let crate_folder = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut registry = PluginRegistry::with_builtins();
    delivery::register(&mut registry)?;
    let params: Params = overrides.iter().cloned().collect();
    let mission = Mission::load(
        &crate_folder.join("missions/delivery.yaml"),
        crate_folder,
        &params,
    )?;
    let mut simulation = Simulation::new(mission.scenario, &registry, workers)?;
    while simulation.step()?.is_some() {}
    let reports = simulation.metric_reports();
    Ok(reports[0].1.teams[&1].values.clone())
}

#[test]
fn impossible_calls_are_refused_and_replay_exactly_from_the_cache() -> Result<()> {
    let folder = scratch("impossible", IMPOSSIBLE)?;
    let live = run(&model(&folder, false), 1)?;
    assert_eq!(live["delivered"], 0.0);
    assert_eq!(live["refused_wrong_skill"], live["decisions"]);
    assert!(!live.contains_key("directive_breaches"), "{live:?}");

    // Offline the client can't run, so every reply comes from the cache: the
    // run must be the same, at another worker count too.
    assert_eq!(live, run(&model(&folder, true), 2)?);
    std::fs::remove_dir_all(&folder)?;
    Ok(())
}

#[test]
fn a_grounded_vehicle_that_flies_is_a_breach_and_the_model_was_told() -> Result<()> {
    let folder = scratch("grounded", FIXED_WING_TWO)?;
    let mut overrides = model(&folder, false);
    overrides.push(set("directive.kind", "ground_vehicle"));
    overrides.push(set("directive.vehicle", 2));
    let results = run(&overrides, 1)?;
    assert!(results["delivered"] > 0.0, "{results:?}");
    assert!(results["directive_breaches"] > 0.0, "{results:?}");
    assert_eq!(results["vehicles_lost"], 0.0);

    // The instruction reaches the model from its announcement (minute 2)
    // until it ends (minute 25), and not before or after.
    let prompts = std::fs::read_to_string(folder.join("prompts.txt"))?;
    let told = |prompt: &&str| prompt.contains("Vehicle 2 is grounded");
    let minute = |prompt: &&str| -> f64 {
        let line = prompt.lines().next().unwrap_or_default();
        line.trim_start_matches("Time: minute ")
            .split('.')
            .take(2)
            .collect::<Vec<_>>()
            .join(".")
            .parse()
            .unwrap_or(-1.0)
    };
    let prompts: Vec<&str> = prompts
        .split("\n=====\n")
        .filter(|p| !p.is_empty())
        .collect();
    assert!(prompts.iter().any(told));
    for prompt in &prompts {
        let in_force = (2.0..=25.0).contains(&minute(prompt));
        assert_eq!(told(prompt), in_force, "minute {}", minute(prompt));
    }
    std::fs::remove_dir_all(&folder)?;
    Ok(())
}

#[test]
fn drop_via_is_offered_only_when_listed_and_delivers_through_its_waypoint() -> Result<()> {
    let folder = scratch("via", VIA)?;
    let mut overrides = model(&folder, false);
    overrides.push(set("skills", "[drop, land, drop_via]"));
    let results = run(&overrides, 1)?;
    assert!(results["delivered"] > 0.0, "{results:?}");
    assert!(!results.contains_key("refused"), "{results:?}");
    let prompts = std::fs::read_to_string(folder.join("prompts.txt"))?;
    assert!(prompts.contains("For drop_via, also give \"via\""));
    std::fs::remove_dir_all(&folder)?;

    // With the default skills the model is never offered it.
    let folder = scratch("no-via", VIA)?;
    let results = run(&model(&folder, false), 1)?;
    assert_eq!(results["delivered"], 0.0);
    let prompts = std::fs::read_to_string(folder.join("prompts.txt"))?;
    assert!(!prompts.contains("drop_via"));
    std::fs::remove_dir_all(&folder)?;
    Ok(())
}

#[test]
fn enforcing_the_instruction_refuses_the_calls_that_would_break_it() -> Result<()> {
    let folder = scratch("enforce", FIXED_WING_TWO)?;
    let mut overrides = model(&folder, false);
    overrides.push(set("directive.kind", "ground_vehicle"));
    overrides.push(set("directive.vehicle", 2));
    let free = run(&overrides, 1)?;
    assert!(free["breaking_decisions"] > 0.0, "{free:?}");
    assert!(free["directive_breaches"] > 0.0, "{free:?}");

    overrides.push(set("enforce_instruction", true));
    let enforced = run(&overrides, 1)?;
    assert!(enforced["refused_breaks_instruction"] > 0.0, "{enforced:?}");
    assert!(!enforced.contains_key("breaking_decisions"), "{enforced:?}");
    assert_eq!(enforced["directive_breaches"], 0.0, "{enforced:?}");
    std::fs::remove_dir_all(&folder)?;
    Ok(())
}

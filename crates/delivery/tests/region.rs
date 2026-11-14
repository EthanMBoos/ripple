//! The region missions end to end: a day is delivered, a fleet entry with
//! counts builds each hub's own number, an avoid
//! order keeps flights out of a closure, the replanner
//! answers a surge with repositions, the help manager lends a hub within
//! reach and refuses a pair it did not offer, and a fake model client gives
//! orders and replays offline from the saved replies.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use ripple_core::plugin::PluginRegistry;
use ripple_core::{Mission, Params, Simulation, Vec3};

const D: &str = "entities.region.autonomy.Dispatcher";

/// Runs a region mission with `changes` applied and returns the DeliveryScore
/// values and every entity's final true position.
fn run(mission: &str, changes: &[(String, String)]) -> Result<(BTreeMap<String, f64>, Vec<Vec3>)> {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = crate_dir.join("missions").join(mission);
    let params: Params = changes.iter().cloned().collect();
    let mission = Mission::load(&path, &crate_dir.join("../.."), &params)?;
    let mut registry = PluginRegistry::with_builtins();
    delivery::register(&mut registry)?;
    let mut simulation = Simulation::new(mission.scenario, Arc::new(registry))?;
    while simulation.step()?.is_some() {}
    let reports = simulation.metric_reports();
    let (_, report) = reports
        .iter()
        .find(|(name, _)| name.contains("DeliveryScore"))
        .expect("the DeliveryScore report");
    let values = report.teams[&1].values.clone();
    let positions = simulation
        .entities()
        .iter()
        .map(|entity| entity.truth().position_world_m)
        .collect();
    Ok((values, positions))
}

/// A short day with no random events: requests stop at `day_min` and the
/// run ends 30 minutes later.
fn short_day(day_min: u32) -> Vec<(String, String)> {
    let changes = vec![("run.end_s".to_string(), format!("{}", (day_min + 30) * 60))];
    let changes = with(changes, "requests.until_s", &format!("{}", day_min * 60));
    with(
        changes,
        "events.per_hub_per_day",
        "{surge: 0, shortage: 0, closure: 0, outage: 0, weather: 0, \
         priority: 0, rooftop_closed: 0, corridor: 0}",
    )
}

/// `changes` plus one Dispatcher setting.
fn with(mut changes: Vec<(String, String)>, key: &str, value: &str) -> Vec<(String, String)> {
    changes.push((format!("{D}.{key}"), value.to_string()));
    changes
}

/// `changes` on a region of `count` hubs.
fn hubs(changes: Vec<(String, String)>, count: u32) -> Vec<(String, String)> {
    with(changes, "region.hubs", &count.to_string())
}

#[test]
fn a_one_hub_day_is_delivered() -> Result<()> {
    let changes = hubs(short_day(60), 1);
    let (one, positions) = run("region.yaml", &changes)?;
    // The region entity and the fleet the dispatcher asked for: 3 + 2.
    assert_eq!(positions.len(), 6);
    assert!(one["requests"] >= 4.0, "{one:?}");
    assert_eq!(one["on_time"], one["requests"], "{one:?}");
    assert_eq!(one["undelivered"], 0.0);
    assert_eq!(one["vehicles_lost"], 0.0);
    Ok(())
}

#[test]
fn a_fleet_entry_with_counts_builds_each_hubs_own_number() -> Result<()> {
    // Four hubs with 5, 1, 3 and 0 fixed-wing and two multirotors each: the
    // region entity and 9 + 8 couriers.
    let fleet = "[{recipe: fixed_wing, per_hub: 3, counts: [5, 1, 3, 0], altitude_m: 120}, \
                 {recipe: multirotor, per_hub: 2}]";
    let changes = with(hubs(short_day(20), 4), "fleet", fleet);
    let (values, positions) = run("region.yaml", &changes)?;
    assert_eq!(positions.len(), 18, "{values:?}");
    let wrong = with(
        hubs(short_day(20), 4),
        "fleet",
        "[{recipe: fixed_wing, counts: [1, 2]}]",
    );
    assert!(run("region.yaml", &wrong).is_err());
    // The same counts written for a block of four hubs, laid over the four
    // blocks of a 16-hub region: 4 x 9 fixed-wing and 32 multirotors.
    let tiled = with(
        with(hubs(short_day(20), 16), "fleet", fleet),
        "region.block_side",
        "2",
    );
    let (values, positions) = run("region.yaml", &tiled)?;
    assert_eq!(positions.len(), 69, "{values:?}");
    Ok(())
}

#[test]
fn an_avoid_order_keeps_new_flights_out_of_a_closure() -> Result<()> {
    // A closure over hub 1's clinics A and B, not its hospitals, all day.
    let circle = "{circle: {center_km: [3.5, 4.5], radius_km: 3.5}}";
    let closure = format!("[{{kind: closure, at_min: 5, until_min: 300, area: {circle}}}]");
    let base = with(hubs(short_day(240), 1), "events.scripted", &closure);
    let (ignored, _) = run("region.yaml", &base)?;
    let avoid = format!("[{{at_min: 5, order: {{avoid: {{area: {circle}, until_min: 300}}}}}}]");
    let changes = with(
        with(base, "manager.kind", "scripted"),
        "manager.scripted",
        &avoid,
    );
    let (avoided, _) = run("region.yaml", &changes)?;
    assert!(ignored["closure_breach_s"] > 0.0, "{ignored:?}");
    assert_eq!(avoided["applied_avoid"], 1.0, "{avoided:?}");
    assert!(
        avoided.get("closure_breach_s").copied().unwrap_or(0.0) < ignored["closure_breach_s"],
        "{avoided:?} vs {ignored:?}"
    );
    // Requests into the closed area wait rather than fly through it.
    assert!(avoided.contains_key("missed_closed"), "{avoided:?}");
    Ok(())
}

#[test]
fn the_replanner_answers_a_surge_with_help_from_other_hubs() -> Result<()> {
    let surge = "[{kind: surge, at_min: 10, hubs: [1]}]";
    let base = with(hubs(short_day(60), 4), "events.scripted", surge);
    let changes = with(base, "manager.kind", "replanner");
    let (values, _) = run("region.yaml", &changes)?;
    assert!(values["trauma_requests"] >= 16.0, "{values:?}");
    assert!(values["applied_reposition"] >= 1.0, "{values:?}");
    assert!(values["applied_serve_from"] >= 1.0, "{values:?}");
    assert!(!values.contains_key("orders_refused"), "{values:?}");
    assert_eq!(values["vehicles_lost"], 0.0);
    Ok(())
}

#[test]
fn a_help_manager_over_the_region_lends_only_what_arrives_on_time() -> Result<()> {
    // No clusters: the fixed broker over the region answers a surge at hub 1
    // with help from a hub within reach. The lender keeps half of each class
    // idle and flies only what it can deliver by the deadline, so every lent
    // delivery is on time.
    let surge = "[{kind: surge, at_min: 10, hubs: [1]}]";
    let base = with(hubs(short_day(60), 16), "events.scripted", surge);
    let changes = with(base, "manager.top_kind", "replanner");
    let (values, _) = run("region.yaml", &changes)?;
    assert!(values["top_calls"] >= 1.0, "{values:?}");
    assert!(values["applied_help"] >= 1.0, "{values:?}");
    assert!(values["lent_flights"] >= 1.0, "{values:?}");
    assert_eq!(
        values["lent_on_time"], values["lent_deliveries"],
        "{values:?}"
    );
    assert!(!values.contains_key("orders_refused"), "{values:?}");
    Ok(())
}

#[test]
fn a_help_order_for_a_pair_not_offered_is_refused() -> Result<()> {
    // The fake help manager grants hub 1 its neighbor hub 2, then names a
    // second donor for hub 1, refused as already helped, then the far
    // corner of the 16-hub region, which no offer lists: too far to fly a
    // round trip to hub 1's sites.
    let dir = std::env::temp_dir().join(format!("ripple-fake-help-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let command = dir.join("model");
    std::fs::write(
        &command,
        "#!/usr/bin/env python3\n\
         import sys\n\
         sys.stdin.read()\n\
         print('{\"orders\": [{\"help\": {\"hub\": 1, \"from_hub\": 2}}, \
         {\"help\": {\"hub\": 1, \"from_hub\": 5}}, \
         {\"help\": {\"hub\": 1, \"from_hub\": 16}}]}')\n",
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&command, std::fs::Permissions::from_mode(0o755))?;
    }
    let surge = "[{kind: surge, at_min: 10, hubs: [1]}]";
    let base = with(hubs(short_day(30), 16), "events.scripted", surge);
    let base = with(base, "manager.top_kind", "model");
    let base = with(
        base,
        "manager.cache_dir",
        dir.join("cache").to_str().unwrap(),
    );
    let changes = with(base, "manager.command", command.to_str().unwrap());
    let (values, _) = run("region.yaml", &changes)?;
    assert!(values["top_calls"] >= 1.0, "{values:?}");
    assert_eq!(values["applied_help"], 1.0, "{values:?}");
    assert!(values["refused_already_helped"] >= 1.0, "{values:?}");
    assert!(values["refused_not_offered"] >= 1.0, "{values:?}");
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn a_reposition_with_an_end_sends_the_vehicles_home() -> Result<()> {
    // Two aircraft go from hub 2 to hub 1 for 20 minutes of a quiet day.
    let order = "[{at_min: 5, order: {reposition: {class: fixed_wing, count: 2, \
                 from_hub: 2, to_hub: 1, until_min: 25}}}]";
    let base = with(hubs(short_day(90), 4), "manager.kind", "scripted");
    let returning = with(base.clone(), "manager.scripted", order);
    let staying = with(
        base,
        "manager.scripted",
        "[{at_min: 5, order: {reposition: {class: fixed_wing, count: 2, \
          from_hub: 2, to_hub: 1}}}]",
    );
    let (back, _) = run("region.yaml", &returning)?;
    let (gone, _) = run("region.yaml", &staying)?;
    assert_eq!(back["applied_reposition"], 1.0, "{back:?}");
    assert_eq!(gone["applied_reposition"], 1.0, "{gone:?}");
    // The vehicles flew home: two more relocation flights than staying put.
    assert!(
        back["relocations"] >= gone["relocations"] + 2.0,
        "{back:?} vs {gone:?}"
    );
    Ok(())
}

#[test]
fn a_veto_manager_drops_the_proposed_order_it_names() -> Result<()> {
    // The fake reviewer always drops the first proposed order.
    let dir = std::env::temp_dir().join(format!("ripple-fake-veto-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let command = dir.join("model");
    std::fs::write(
        &command,
        "#!/usr/bin/env python3\nimport sys\nsys.stdin.read()\nprint('{\"drop\": [1]}')\n",
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&command, std::fs::Permissions::from_mode(0o755))?;
    }
    let surge = "[{kind: surge, at_min: 10, hubs: [1]}]";
    let base = with(hubs(short_day(60), 4), "events.scripted", surge);
    let base = with(base, "manager.kind", "veto");
    let base = with(base, "manager.feedback", "true");
    let base = with(base, "manager.cost_feedback", "true");
    let base = with(
        base,
        "manager.cache_dir",
        dir.join("cache").to_str().unwrap(),
    );
    let base = with(base, "manager.command", command.to_str().unwrap());
    let (values, _) = run("region.yaml", &base)?;
    let (planned, _) = run(
        "region.yaml",
        &with(hubs(short_day(60), 4), "events.scripted", surge)
            .into_iter()
            .chain(with(Vec::new(), "manager.kind", "replanner"))
            .collect::<Vec<_>>(),
    )?;
    assert!(values["calls"] >= 1.0, "{values:?}");
    assert_eq!(values["dropped"], values["calls"], "{values:?}");
    assert_eq!(values["parse_failures"], 0.0);
    assert_eq!(
        values["orders"] + values["dropped"],
        planned["orders"],
        "{values:?} vs {planned:?}"
    );
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn the_policy_form_gives_the_replanner_day_for_day() -> Result<()> {
    // A 4-hour day with the mission's random events at their usual density.
    let base = vec![("run.end_s".to_string(), "16200".to_string())];
    let base = with(base, "requests.until_s", "14400");
    let replanner = with(base.clone(), "manager.kind", "replanner");
    let policy = with(base, "manager.kind", "policy");
    let file = concat!(env!("CARGO_MANIFEST_DIR"), "/policies/replanner.yaml");
    let policy = with(policy, "manager.policy", file);
    let (replanned, _) = run("region.yaml", &replanner)?;
    let (written, _) = run("region.yaml", &policy)?;
    assert!(replanned["calls"] >= 4.0, "{replanned:?}");
    assert!(replanned["orders"] >= 4.0, "{replanned:?}");
    assert_eq!(replanned, written);
    Ok(())
}

#[test]
fn a_fake_model_orders_and_replays_offline() -> Result<()> {
    // The fake client answers every call with one order.
    let dir = std::env::temp_dir().join(format!("ripple-fake-model-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let command = dir.join("model");
    std::fs::write(
        &command,
        "#!/usr/bin/env python3\n\
         import sys\n\
         sys.stdin.read()\n\
         print('{\"orders\": [{\"prioritize\": {\"site\": null, \
         \"urgency\": \"trauma\", \"until_min\": 120}}]}')\n",
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&command, std::fs::Permissions::from_mode(0o755))?;
    }
    let cache = dir.join("cache");
    let closure = "[{kind: closure, at_min: 5, until_min: 60, \
                    area: {circle: {center_km: [6, 2], radius_km: 3}}}]";
    let base = with(hubs(short_day(30), 1), "events.scripted", closure);
    let base = with(base, "manager.kind", "model");
    let base = with(base, "manager.feedback", "true");
    let base = with(base, "manager.cache_dir", cache.to_str().unwrap());
    let live = with(base.clone(), "manager.command", command.to_str().unwrap());
    let (asked, _) = run("region.yaml", &live)?;
    assert!(asked["calls"] >= 1.0, "{asked:?}");
    assert_eq!(asked["applied_prioritize"], asked["calls"], "{asked:?}");
    assert_eq!(asked["parse_failures"], 0.0);
    assert!(asked["prompt_chars"] > asked["reply_chars"]);

    // Offline, the saved replies stand in for the client, which is gone.
    let offline = with(base, "manager.offline", "true");
    let offline = with(offline, "manager.command", "/nonexistent/model");
    let (replayed, _) = run("region.yaml", &offline)?;
    assert_eq!(asked, replayed);
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

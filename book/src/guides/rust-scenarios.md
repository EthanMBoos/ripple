# Building a scenario in Rust

A `ScenarioConfig` describes the experiment. A `PluginRegistry` supplies its
available behaviors. `Simulation::new` validates the description and constructs
the running simulation. XML and YAML mission readers produce the same scenario.

Use Rust when you want loops or calculations to generate a setup. Keep scenario
functions in your workspace research crate alongside your plugins.

## Describe the experiment

```rust,ignore
use scrimmage_core::{EntityGroupConfig, PluginConfig, ScenarioConfig, Vec3};
use serde::Serialize;

#[derive(Serialize)]
struct Route {
    waypoints: Vec<[f64; 3]>,
    speed: f64,
}

pub fn ring() -> anyhow::Result<ScenarioConfig> {
    let mut scenario = ScenarioConfig::default();
    scenario.run.end_s = 60.0;
    for i in 0..20 {
        let angle = i as f64 * std::f64::consts::TAU / 20.0;
        scenario.entities.push(EntityGroupConfig {
            label: format!("agent{i}"),
            team: 1,
            position_m: Vec3::new(200.0 * angle.cos(), 200.0 * angle.sin(), 100.0),
            autonomy: vec![PluginConfig::new("WaypointFollower").with_params(Route {
                waypoints: vec![[0.0, 0.0, 100.0]],
                speed: 5.0,
            })?],
            controller: vec![PluginConfig::new("SingleIntegratorControllerSimple")],
            motion_model: PluginConfig::new("SingleIntegrator"),
            ..EntityGroupConfig::default()
        });
    }
    Ok(scenario)
}
```

An entity group specifies `count` independent agents and an optional `Spawn`
schedule. The example uses the default count of one. Positions and velocities
are local ENU world coordinates, in meters and meters/second. Position variance
defaults to zero. Select a motion model explicitly; omitted physics is an error.

`PluginConfig::new` uses the plugin's defaults. `with_params` replaces its
supplied values with a serializable struct or map; the plugin's existing
`configure` checks them at construction. This is programmatic configuration:
plugin names and parameter compatibility are checked at startup.

## Use the shared runner

The existing CLI crate can call your research crate's scenario function and
pass it to the same runner used by `scrimmage run`:

```rust,ignore
use scrimmage_cli::{RunSettings, run_scenario};

let output = run_scenario(my_lab::ring()?, &registry, RunSettings {
    root: repo_root.into(),
    name: "ring".into(),
    workers: 8,
    ..RunSettings::default()
})?;
```

`registry` includes the stock models and your workspace plugins. The runner
allocates `runs/ring/runNNN` under `root` and writes frames, events, summaries,
a manifest, and a Rerun recording. Set `viewer: true` to launch Rerun,
`recording: false` to disable recording, or `output: Some(path)` to use a new
explicit output directory. Invalid configurations fail before output allocation.

The CLI already depends on your plugin crate; keep this call in the CLI rather
than adding a dependency from the plugin crate back to the CLI. A Rust scenario
function is not automatically a command or sweep. Existing YAML sweeps and
Slurm shards continue to use the shared tools described in [YAML missions](yaml-missions.md).

## Step directly in a test

```rust,ignore
use scrimmage_core::{Mission, Params, Simulation};

// Either load a mission or call your Rust setup function.
let mission = Mission::load_with_registry(path, root, &Params::new(), &registry)?;
let mut simulation = Simulation::new(mission.scenario, &registry, 8)?;
while let Some(frame) = simulation.step()? {
    // Inspect state or collect experiment-specific output.
}
```

`Mission` also contains the source path and the file's worker/viewer defaults;
these are separate from simulation data. Scenario serialization records
normalized data for manifests, not reloadable mission YAML. The default end
condition is the time limit, for both Rust and YAML.

The [design notes](https://github.com/EthanMBoos/scrimmage-rs/blob/main/docs/RUST_API.md)
explain these boundaries and their references.

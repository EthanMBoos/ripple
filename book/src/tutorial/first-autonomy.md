# Your first autonomy plugin

In this tutorial you will write an autonomy plugin called `FollowNearest`. Each
tick, it finds the closest vehicle on another team and steers toward it. You
will then run it in a mission, watch it in Rerun, write a test that checks the
chaser actually catches its target, and sweep its speed to find how fast it
must be.

Along the way you will touch every part of a plugin:

- reading your own state and the other vehicles' states,
- writing commands for the controller through *ports*,
- reading a parameter from the mission file,
- registering the plugin so missions can use it by name.

You work in the `starter` user crate, not in the simulator's source.

It takes about 30 lines of Rust.

> [!TIP]
> Keep the finished example,
> [Straight](https://github.com/EthanMBoos/scrimmage-rs/blob/main/crates/core/src/plugin/autonomy/straight/straight.rs),
> open while you work. It has the same shape as the plugin you are about to write.

## What an autonomy plugin does

An autonomy plugin *decides*. It does not move the vehicle. It writes goals such
as "fly heading 90° at 25 m/s and 200 m altitude", and the controller and motion
model below it turn those goals into motion:

```text
FollowNearest  --desired_heading, desired_altitude, desired_speed-->
SimpleAircraftControllerPID  --throttle, roll_rate, pitch_rate-->
SimpleAircraft  --> new position and attitude
```

The names in that chain are **ports**. An autonomy's output ports must match the
controller's input ports. The simulator checks this before the mission starts,
so a typo is an error, not a silently ignored command.

## 1. Find the starter crate

User plugins live in a crate in the repository, `crates/starter`. The
`scrimmage` command is compiled with that crate's plugins, so its missions run
with `scrimmage run` and `scrimmage sweep` like any other (see
[Writing your own plugins](https://github.com/EthanMBoos/scrimmage-rs/blob/main/docs/USER_PROJECTS.md)).

```text
crates/starter/
  src/lib.rs               register(): adds the crate's plugins
  src/follow_nearest.rs    the plugin you are about to write
  missions/                follow-nearest.yaml and follow-nearest.sweep.yaml
  tests/                   follow_nearest.rs
```

The starter already contains the finished plugin, so everything builds and runs
straight away. To write it yourself, empty `src/follow_nearest.rs` and follow
along; the next sections explain every line.

## 2. Write the plugin

Put this in `crates/starter/src/follow_nearest.rs`. We'll walk through it below.

```rust,ignore
//! Head toward the nearest active entity on another team, matching its altitude.
//! Autonomy phase: read contacts, write desired heading, altitude, and speed.

use anyhow::{Result, ensure};
use scrimmage_core::plugin::{
    AgentContext, Autonomy, Frame, Plugin, PluginIo, PluginParams, Port, Ports, Unit, Update,
};
use serde::Deserialize;

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FollowNearestConfig {
    #[serde(rename = "speed")]
    speed_mps: f64,
}

impl Default for FollowNearestConfig {
    fn default() -> Self {
        Self { speed_mps: 25.0 }
    }
}

pub struct FollowNearest {
    speed_mps: f64,
}

impl Plugin for FollowNearest {
    type Config = FollowNearestConfig;

    fn configure(params: &PluginParams<'_>) -> Result<FollowNearestConfig> {
        let config: FollowNearestConfig = params.parse()?;
        ensure!(config.speed_mps > 0.0, "FollowNearest speed must be positive");
        Ok(config)
    }

    fn new(config: &FollowNearestConfig) -> Self {
        Self {
            speed_mps: config.speed_mps,
        }
    }

    fn ports(_config: &FollowNearestConfig) -> Ports {
        Ports::default()
            .output(Port::new("desired_heading", Unit::Radians, Frame::World))
            .output(Port::new("desired_altitude", Unit::Meters, Frame::World))
            .output(Port::new(
                "desired_speed",
                Unit::MetersPerSecond,
                Frame::None,
            ))
    }
}

impl Autonomy for FollowNearest {
    fn step(&mut self, context: &mut AgentContext<'_>, io: &mut PluginIo) -> Result<Update> {
        // Input: our own state and every other entity's true state.
        let own_position_world_m = context.state.position_world_m;

        // Calculation: find the closest active opponent.
        let mut nearest = None;
        let mut nearest_distance_m = f64::INFINITY;
        for contact in context.contacts_truth {
            if !contact.active || contact.team_id == context.entity.team_id {
                continue;
            }
            let distance_m = (contact.truth.position_world_m - own_position_world_m).norm();
            if distance_m < nearest_distance_m {
                nearest_distance_m = distance_m;
                nearest = Some(contact);
            }
        }

        // Output: head toward it, or hold the current heading and altitude if none remain.
        let (heading_world_rad, altitude_world_m) = match nearest {
            Some(target) => {
                let offset_world_m = target.truth.position_world_m - own_position_world_m;
                (
                    offset_world_m.y.atan2(offset_world_m.x),
                    target.truth.position_world_m.z,
                )
            }
            None => (
                context
                    .state
                    .orientation_world_from_body
                    .yaw_world_from_body_rad(),
                own_position_world_m.z,
            ),
        };
        io.write("desired_heading", heading_world_rad)?;
        io.write("desired_altitude", altitude_world_m)?;
        io.write("desired_speed", self.speed_mps)?;
        Ok(Update::Applied)
    }
}
```

### The two structs

- `FollowNearestConfig` is the **validated configuration**. It is built once
  from the mission, before the simulation starts. Its fields are the mission
  parameters: `#[serde(rename = "speed")]` says the mission calls it `speed`,
  while the Rust name `speed_mps` carries the unit. `Default` gives the value
  used when the mission doesn't set one.
- `FollowNearest` is the **per-vehicle state**. Each vehicle using this plugin
  gets its own copy. This plugin only needs the speed. A plugin that remembers
  things between ticks, such as a PID controller's integral, keeps them here.

### `impl Plugin`: lifecycle

| Method | When it runs | What it does here |
| --- | --- | --- |
| `configure` | Once, while the mission loads | `params.parse()` fills the config from the mission, using 25 when it has no `speed`. Then it rejects bad values. |
| `new` | Once per vehicle | Copies the configuration into the vehicle's state. |
| `ports` | While the mission loads | Declares the three outputs, with units and frames. The simulator checks them against the controller's inputs. |

Because `configure` runs before anything moves, a mistake like `speed: -5`
stops the mission immediately with a clear error. You won't discover it halfway
through a run. So does a typo: `sped: 30` is rejected as an unknown field
instead of silently leaving the speed at 25.

### `impl Autonomy`: the decision

`step` runs once per tick. It follows the same three stages as every plugin in
this project:

1. **Input.** `context.state` is this vehicle's own state.
   `context.contacts_truth` is a snapshot of *every* entity, including this one.
2. **Calculation.** Skip entities that are inactive or on our own team. Skipping
   our own team also skips ourselves. Keep the closest of the rest.
3. **Output.** Point at the target. `atan2(dy, dx)` is the heading from us to it:
   0 means east and π/2 means north (see [Coordinate frames](../concepts/coordinate-frames.md)).
   Match its altitude and fly at the configured speed. If no opponent is left,
   hold the current heading and altitude.

`context.state` versus `contacts_truth` matters once you add noisy sensors.
Your own state may be an *estimate*, while `contacts_truth` is exact. See
[Belief vs truth](../concepts/belief-vs-truth.md).

`Update::Applied` means "the step succeeded". Returning `Err(...)` stops the
simulation and names this plugin in the error. Use it for real failures, not
for "no target found".

## 3. Register the plugin

`crates/starter/src/lib.rs` lists the crate's plugins. It loads the module and
registers the type under the name missions will use:

```rust,ignore
mod follow_nearest;
pub use follow_nearest::FollowNearest;

/// Adds this crate's plugins to `registry`.
pub fn register(registry: &mut PluginRegistry) -> Result<()> {
    registry.register_autonomy::<FollowNearest>("FollowNearest")?;
    Ok(())
}
```

Missions refer to the plugin by this name. The `scrimmage` command calls
`starter::register` when it starts, next to the stock plugins, so you don't
touch the command itself.

## 4. Write a mission

`crates/starter/missions/follow-nearest.yaml` pits the plugin against a target. Team 1 flies
east in a straight line. Team 2 starts behind it, to the side and higher, and
chases it with your plugin:

```yaml
format_version: 1
name: Follow the nearest opponent

run:
  end_s: 60
  dt_s: 0.1
  seed: 1
end_conditions: [time, one_team]

interactions:
  SimpleCollision:
networks:
  LocalNetwork:           # Straight listens on it for sensor updates
  GlobalNetwork:
metrics:
  SimpleCollisionMetrics:

templates:
  aircraft:
    visual_model: aircraft
    controller:
      SimpleAircraftControllerPID:
    motion_model:
      SimpleAircraft:

entities:
  target:
    template: aircraft
    team: 1
    color: [77, 77, 255]
    position_m: [0, 0, 200]
    heading_deg: 0
    autonomy:
      Straight:
        speed: 18

  chaser:
    template: aircraft
    team: 2
    color: [255, 0, 0]
    position_m: [-300, 150, 220]
    heading_deg: 0
    autonomy:
      FollowNearest:
        speed: 30
```

A few things to notice:

- `speed: 30` under `FollowNearest` overrides the default of 25.
  Values in the mission always win over plugin defaults.
- The `aircraft` template holds what both vehicles share; each group adds its
  own team, position, and autonomy.
- `heading_deg` is in degrees, with 0 meaning east.
- `SimpleCollision` removes two vehicles that come within 2 m of each other, and
  `SimpleCollisionMetrics` scores it.
- `LocalNetwork` is listed because `Straight` listens on it for sensor updates.
  Leaving it out is an error.

The full mission format is in
[YAML missions](https://github.com/EthanMBoos/scrimmage-rs/blob/main/docs/MISSION_YAML.md).

## 5. Run it and watch

From the repository root:

```sh
cargo run -- run crates/starter/missions/follow-nearest.yaml --headless
cargo run -- replay runs/follow-nearest/run000
```

Use the run folder that the first command prints.

> [!IMPORTANT]
> Plugins are compiled into `scrimmage`. A `scrimmage` you installed earlier
> with `cargo install` does not know about `FollowNearest` until you reinstall
> it, so use `cargo run` while developing.

The red chaser turns toward the blue target, descends to its altitude, and
closes in. At about 32 s they
collide and both are removed. The run folder's `events.json` shows it:

```text
NonTeamCollision  at 32.0 s  entities [1, 2]
EntityRemoved     at 32.0 s  entity 1
EntityRemoved     at 32.0 s  entity 2
```

## 6. Test it

Watching a run is useful, but a test keeps the behavior from breaking silently
later. `crates/starter/tests/follow_nearest.rs` contains this one:

```rust,ignore
use anyhow::Result;
use scrimmage_core::{EventKind, Params, ScenarioConfig, Simulation, plugin::PluginRegistry};
use std::path::Path;

#[test]
fn chaser_catches_the_target() -> Result<()> {
    let crate_folder = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut registry = PluginRegistry::with_builtins();
    starter::register(&mut registry)?;
    let mission = ScenarioConfig::load_with_registry(
        &crate_folder.join("missions/follow-nearest.yaml"),
        crate_folder,
        &Params::new(),
        &registry,
    )?;
    let mut simulation = Simulation::new(mission.resolve_with_registry(&registry)?, 1)?;
    while simulation.step()?.is_some() {}

    let caught = simulation
        .events()
        .iter()
        .any(|event| event.kind == EventKind::NonTeamCollision);
    assert!(caught, "the chaser never reached the target");
    Ok(())
}
```

Run it:

```sh
cargo test -p starter
```

This is a *mission-level* test. Instead of checking one function, it runs the
whole simulation and checks the outcome. It catches problems that only show up
when plugins work together, such as a wrong sign in a heading.
The test builds the same registry as `scrimmage` (stock plugins plus
`starter::register`), and `load_with_registry` loads the mission with it.

> [!NOTE]
> The `1` in `Simulation::new(..., 1)` is the worker count. Results are identical
> for any worker count, so tests can use 1.

## 7. Sweep the speed

How fast must the chaser be? `crates/starter/missions/follow-nearest.sweep.yaml`
runs the mission once for each chaser speed:

```yaml
name: follow-nearest
base_scenario: follow-nearest.yaml
seeds: [1]
parameter_combinations: cartesian
parameters:
  entities.chaser.autonomy.FollowNearest.speed: [17, 20, 25, 30]
```

```sh
cargo run --release -- sweep crates/starter/missions/follow-nearest.sweep.yaml
```

Each case is one line of `sweeps/follow-nearest/run000/results.jsonl`, with the
speed and the final metrics. `non_team_coll` shows whether the chaser caught
the target: at 25 and 30 m/s it does, at 17 and 20 m/s it does not within 60 s.
To watch one case, run it with that speed and open Rerun:
`cargo run -- run crates/starter/missions/follow-nearest.yaml entities.chaser.autonomy.FollowNearest.speed:=20 --viewer`.

## Exercises

1. **Slow chaser.** The sweep shows that at 20 m/s, faster than the target's
   18, the chaser still does not catch up within 60 s. Watch that case in Rerun.
   Why not?
2. **Lead the target.** Pure pursuit aims at where the target *is*. Aim at where
   it *will be* instead: add `target.truth.velocity_world_mps * t_go`, where
   `t_go` is distance divided by closing speed. Rerun the sweep: which speeds
   catch the target now? Update the test to check the time.
3. **Keep your distance.** Add a `standoff_m` parameter. Fly toward the target
   while farther than `standoff_m`, and away from it when closer.
4. **Many chasers.** Give the chaser group `count: 3`, and add a second target
   group at another position. Do the chasers all pick the same target?

## What you learned

- A plugin is a configuration struct, a state struct, `impl Plugin` for
  lifecycle, and one category trait (here `impl Autonomy`) for the per-tick work.
- Ports connect the plugin stack and are checked before the mission runs.
- Parameters are a serde struct with a `Default`. Mission values override the
  defaults, and unknown keys are errors.
- A user crate registers its plugins in one `register()`, which the `scrimmage`
  command and the crate's tests both call.
- Mission-level tests run the full simulation and check events or scores, and
  sweeps run it across parameter values.

Next, read [Coordinate frames](../concepts/coordinate-frames.md) before
writing anything that uses angles.

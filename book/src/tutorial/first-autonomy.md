# Your first autonomy plugin

In this tutorial you will write an autonomy plugin called `FollowNearest`. Each
tick, it finds the closest vehicle on another team and steers toward it. You
will then run it in a mission, watch it in Rerun, and write a test that checks
the chaser actually catches its target.

Along the way you will touch every part of a plugin:

- reading your own state and the other vehicles' states,
- writing commands for the controller through *ports*,
- reading a parameter from the mission XML,
- registering the plugin so missions can use it by name.

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

## 1. Create the plugin folder

Every built-in plugin lives in its own folder. Create:

```text
crates/core/src/plugin/autonomy/follow_nearest/
    follow_nearest.rs
```

The plugin's parameters and their default values live in that Rust file, as
you'll see next.

## 2. Write the plugin

Put this in `follow_nearest.rs`. We'll walk through it below.

```rust,ignore
//! Head toward the nearest active entity on another team, matching its altitude.
//! Autonomy phase: read contacts, write desired heading, altitude, and speed.

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use crate::plugin::{
    AgentContext, Autonomy, Frame, Plugin, PluginIo, PluginParams, Port, Ports, Unit, Update,
};

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Deserialize, Serialize)]
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

Because `configure` runs before anything moves, a mistake like `speed="-5"`
stops the mission immediately with a clear error. You won't discover it halfway
through a run. So does a typo: `sped="30"` is rejected as an unknown field
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

Two small edits make the plugin available to missions.

In `crates/core/src/plugin/autonomy.rs`, load the module. Entries are in
alphabetical order, so this one goes first:

```rust,ignore
#[path = "autonomy/follow_nearest/follow_nearest.rs"]
mod follow_nearest;
pub use follow_nearest::FollowNearest;
```

In `crates/core/src/plugin_manager/builtins.rs`, give it the name that missions
will use:

```rust,ignore
registry.register_autonomy::<autonomy::FollowNearest>("FollowNearest")?;
```

Missions refer to the plugin by this name.

## 4. Write a mission

Create `missions/follow-nearest.xml`. Team 1 flies east in a straight line.
Team 2 starts behind it, to the side and higher, and chases it with your plugin:

```xml
<?xml version="1.0"?>
<runscript name="Follow the nearest opponent">
  <run start="0" end="60" dt="0.1" enable_gui="false" />
  <seed>1</seed>
  <end_condition>time, one_team</end_condition>

  <network>LocalNetwork</network>
  <network>GlobalNetwork</network>
  <entity_interaction>SimpleCollision</entity_interaction>
  <metrics>SimpleCollisionMetrics</metrics>

  <!-- Team 1: a target flying east in a straight line. -->
  <entity>
    <team_id>1</team_id><color>77 77 255</color>
    <x>0</x><y>0</y><z>200</z><heading>0</heading>
    <autonomy speed="18">Straight</autonomy>
    <controller>SimpleAircraftControllerPID</controller>
    <motion_model>SimpleAircraft</motion_model>
    <visual_model>aircraft</visual_model>
  </entity>

  <!-- Team 2: the chaser, starting behind, to the side, and higher. -->
  <entity>
    <team_id>2</team_id><color>255 0 0</color>
    <x>-300</x><y>150</y><z>220</z><heading>0</heading>
    <autonomy speed="30">FollowNearest</autonomy>
    <controller>SimpleAircraftControllerPID</controller>
    <motion_model>SimpleAircraft</motion_model>
    <visual_model>aircraft</visual_model>
  </entity>
</runscript>
```

A few things to notice:

- `speed="30"` on the `<autonomy>` tag overrides the default of 25.
  Mission attributes always win over plugin defaults.
- `heading` is in degrees in mission files, with 0 meaning east.
- `SimpleCollision` removes two vehicles that come within 2 m of each other, and
  `SimpleCollisionMetrics` scores it.
- `LocalNetwork` is listed because `Straight` listens on it for sensor updates.
  Leaving it out is an error.

## 5. Run it and watch

```sh
cargo run -p scrimmage-rs --bin scrimmage -- run missions/follow-nearest.xml --headless
cargo run -p scrimmage-rs --bin scrimmage -- replay runs/run000
```

Use the run folder that the first command prints.

> [!IMPORTANT]
> Plugins are compiled into the `scrimmage` program. A `scrimmage` you installed
> earlier with `cargo install` does not know about `FollowNearest` until you
> reinstall it, so use `cargo run` while developing.

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
later. Create `crates/core/tests/follow_nearest.rs`:

```rust,ignore
use std::path::PathBuf;

use anyhow::Result;
use scrimmage_core::{EventKind, Params, ScenarioConfig, Simulation};

#[test]
fn chaser_catches_the_target() -> Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mission = ScenarioConfig::load(
        &root.join("missions/follow-nearest.xml"),
        &root,
        &Params::new(),
    )?;
    let mut simulation = Simulation::new(mission.resolve()?, 1)?;
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
cargo test -p scrimmage-core --test follow_nearest
```

This is a *mission-level* test. Instead of checking one function, it runs the
whole simulation and checks the outcome. It catches problems that only show up
when plugins work together, such as a wrong sign in a heading.

> [!NOTE]
> The `1` in `Simulation::new(..., 1)` is the worker count. Results are identical
> for any worker count, so tests can use 1.

## Exercises

1. **Slow chaser.** Set the chaser's `speed` to 17, below the target's 18. Does
   it still catch up? Why, or why not? (SimpleAircraft clamps speed to at least
   15 m/s.)
2. **Lead the target.** Pure pursuit aims at where the target *is*. Aim at where
   it *will be* instead: add `target.truth.velocity_world_mps * t_go`, where
   `t_go` is distance divided by closing speed. Does the catch happen sooner?
   Update the test to check the time.
3. **Keep your distance.** Add a `standoff_m` parameter. Fly toward the target
   while farther than `standoff_m`, and away from it when closer.
4. **Many chasers.** Change the chaser entity to `<count>3</count>` and use
   `<variance_x>` / `<variance_y>` to spread them out. Do they all pick the same
   target?

## What you learned

- A plugin is a configuration struct, a state struct, `impl Plugin` for
  lifecycle, and one category trait (here `impl Autonomy`) for the per-tick work.
- Ports connect the plugin stack and are checked before the mission runs.
- Parameters are a serde struct with a `Default`. Mission attributes override
  the defaults, and unknown keys are errors.
- Mission-level tests run the full simulation and check events or scores.

Next, read [Coordinate frames](../concepts/coordinate-frames.md) before
writing anything that uses angles.

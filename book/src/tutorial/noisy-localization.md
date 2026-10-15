# Your first localization estimator

In this tutorial you will write a localization estimator called
`DeadReckoning`. It works out where a robot is the way a sailor does without
GPS: start from a known position, add up how far you *think* you have moved,
and correct whenever a position fix arrives. You will run it in a mission where
the robot moves more slowly than it thinks, see the robot's belief drift away
from the truth, and watch that wrong belief bend its real path.

Read [Belief vs truth](../concepts/belief-vs-truth.md) first. This tutorial
assumes you have done [Your first autonomy plugin](first-autonomy.md), so it
explains only what is new.

## What a localization estimator does

Every entity has one localization estimator. It produces the entity's
*belief* about its own state: `context.state`, which its autonomy and
controller read. The default, `PassThrough`, copies a sensor reading into
belief, so with no sensor the belief is perfect.

An estimator gets what a real robot would have, and nothing else:

- **readings** from the sensors the mission selects, looked up by input name;
- **commands**: the values the motion model read on each substep of the tick;
- the time, messages, and random numbers.

It never sees the truth. So the only way for belief to go wrong is the way it
goes wrong on a real robot: noisy readings, readings that don't come often,
or a wrong idea of how the vehicle moves. This tutorial models the last two.

## 1. The setup

The robot is a point that moves at the velocity it is given
(`SingleIntegrator`). A `WaypointFollower` autonomy steers it around a 100 m
square at 10 m/s. But its motion model has `max_speed_mps: 8`: asking for 10, it
really moves at 8 m/s. That is the *hidden truth*, a physical fact
the robot's software doesn't know.

The estimator has its own idea of how the robot moves, a setting called
`speed_gain`: the fraction of the commanded velocity it assumes the robot
achieves. A position fix arrives every 5 s.

| | Value |
| --- | --- |
| Commanded speed | 10 m/s |
| Real speed (motion model) | 8 m/s |
| Assumed speed (estimator) | `speed_gain` x 10 m/s |
| Position fixes | at startup and 0.1 s, then 5.1 s, 10.1 s, ...; no position noise |

With `speed_gain: 1.0` the estimator believes the robot moves at 10 m/s, 2 m/s
too fast. With `0.8` it is right.

## 2. Write the estimator

The finished estimator is in `crates/starter/src/dead_reckoning.rs`:

```rust,ignore
//! Dead reckoning with occasional position fixes, for a velocity-commanded vehicle.
//! Localization: predict position from the commanded velocity, replace it on a fix.
//! It estimates position and velocity only; attitude stays at the default.

use anyhow::{Context, Result, ensure};
use ripple_core::plugin::{
    Input, Localization, LocalizationContext, Plugin, PluginParams, ReadingKind, Update,
};
use ripple_core::{KinematicState, Vec3};
use serde::Deserialize;

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DeadReckoningConfig {
    /// How fast the estimator assumes the vehicle moves, as a fraction of the
    /// commanded velocity. 1.0 assumes it moves exactly as commanded.
    speed_gain: f64,
}

impl Default for DeadReckoningConfig {
    fn default() -> Self {
        Self { speed_gain: 1.0 }
    }
}

pub struct DeadReckoning {
    speed_gain: f64,
}

impl Plugin for DeadReckoning {
    type Config = DeadReckoningConfig;

    fn configure(params: &PluginParams<'_>) -> Result<DeadReckoningConfig> {
        let config: DeadReckoningConfig = params.parse()?;
        ensure!(
            config.speed_gain >= 0.0,
            "DeadReckoning speed_gain must be nonnegative"
        );
        Ok(config)
    }

    fn new(config: &DeadReckoningConfig) -> Self {
        Self {
            speed_gain: config.speed_gain,
        }
    }
}

impl Localization for DeadReckoning {
    fn inputs(_config: &DeadReckoningConfig) -> Vec<Input> {
        // A full-state reading; only its position is used.
        vec![Input::new("fix", ReadingKind::State)]
    }

    fn initialize(&mut self, context: &mut LocalizationContext<'_>) -> Result<()> {
        // Start from the first fix, at rest. Attitude is not estimated.
        let fix = context
            .inputs
            .latest::<KinematicState>("fix")?
            .context("DeadReckoning needs a position fix at startup")?;
        *context.state = KinematicState {
            position_world_m: fix.value.position_world_m,
            ..KinematicState::default()
        };
        Ok(())
    }

    fn step(&mut self, context: &mut LocalizationContext<'_>) -> Result<Update> {
        // Predict: move by the velocity the motion model was told to fly, on each substep.
        for substep in 0..context.commands.substeps() {
            let commanded_world_mps = Vec3::new(
                context.commands.get(substep, "velocity_x")?,
                context.commands.get(substep, "velocity_y")?,
                context.commands.get(substep, "velocity_z")?,
            );
            let velocity_world_mps = commanded_world_mps * self.speed_gain;
            context.state.velocity_world_mps = velocity_world_mps;
            context.state.position_world_m +=
                velocity_world_mps * context.commands.time(substep).dt_s;
        }

        // Correct: a new fix replaces the predicted position.
        if let Some(fix) = context.inputs.fresh::<KinematicState>("fix")? {
            context.state.position_world_m = fix.value.position_world_m;
        }
        Ok(Update::Applied)
    }
}
```

The configuration and `impl Plugin` have the same shape as in the autonomy
tutorial. What is new is `impl Localization`:

- **`inputs`** declares the readings the estimator needs. Here that is one
  input, named `fix`, which takes readings of a full state
  (`ReadingKind::State`). The estimator doesn't name a sensor; the mission
  decides which sensor feeds `fix` (see below).
- **`initialize`** runs once, before any decision, and sets the first belief.
  `inputs.latest` returns the most recent reading for an input. There is no
  fallback to truth: with no reading at startup the mission stops with this
  error.
- **`step`** runs at the end of every tick, after the sensors. `context.state`
  is the belief; it persists between ticks, so the estimator updates it rather
  than rebuilding it.
  1. *Predict.* `context.commands` holds what the motion model read on each
     substep. Multiply each commanded velocity by the assumed gain and move
     the belief by it for that substep's `dt_s`.
  2. *Correct.* `inputs.fresh` returns a reading only if it arrived this tick.
     The fix sensor runs every 5 s, so most ticks have none and the prediction
     stands. Using `latest` here would snap back to an old fix every tick.

This estimator tracks position and velocity only, which is all a point robot
steering by position needs. Its attitude stays level and facing east (the
default), so don't reuse it unchanged with a controller that steers by heading.

The estimator never reads the true speed or `max_speed_mps`. The only link between
them is the `speed_gain` setting, which a person chose and may have got wrong.

## 3. Register it

In `crates/starter/src/lib.rs`, next to `FollowNearest`:

```rust,ignore
mod dead_reckoning;
pub use dead_reckoning::DeadReckoning;

pub fn register(registry: &mut PluginRegistry) -> Result<()> {
    registry.register_autonomy::<FollowNearest>("FollowNearest")?;
    registry.register_localization::<DeadReckoning>("DeadReckoning")?;
    Ok(())
}
```

## 4. The mission

`crates/starter/missions/dead-reckoning.yaml` has two robots on the same route.
`dead_reckoning` uses your estimator; `perfect` has no sensor, so it keeps the
default perfect localization and shows where the route should go.

```yaml
format_version: 1
name: Dead reckoning between position fixes

run:
  end_s: 60
  dt_s: 0.1
  seed: 1

networks:
  GlobalNetwork:          # WaypointFollower listens here for route changes

templates:
  robot:
    position_m: [0, 0, 0]
    position_variance_m2: [0, 0, 0]
    visual_model: sphere
    autonomy:
      WaypointFollower:
        waypoints_m: [[100, 0, 0], [100, 100, 0], [0, 100, 0], [0, 0, 0]]
        repeat: true
        arrival_radius_m: 2
        speed_mps: 10
    controller: {SingleIntegratorControllerSimple: {}}
    motion_model:
      SingleIntegrator:
        max_speed_mps: 8      # the hidden truth: its motors only reach 8 m/s

entities:
  dead_reckoning:
    template: robot
    team: 1
    color: [70, 130, 255]
    sensors:
      gps:
        plugin: NoisyState
        loop_rate_hz: 0.2    # one fix every 5 s
        position_stddev_m: [0, 0, 0]   # exact fixes; raise to add GPS noise
    localization:
      DeadReckoning:
        speed_gain: 1.0   # assumes it moves as commanded: 2 m/s too fast

  perfect:
    template: robot
    team: 2
    color: [160, 160, 160]
```

- The fix comes from the stock `NoisyState` sensor, named `gps`. Its noise
  defaults to zero; the mission names `position_stddev_m` anyway so the
  exercises can raise it from the command line (an override replaces a value
  the mission has). `loop_rate_hz: 0.2` runs it once every 5 s.
- `localization:` replaces the default `PassThrough` with `DeadReckoning`.
  It has one sensor that gives state readings, so `gps` feeds its `fix`
  input automatically. With two such sensors you would name one:
  `inputs: {fix: gps}`. The scenario in `run.mcap` records the choice.
- The autonomy and controller are unchanged. They act on belief and don't
  know which estimator produced it.

## 5. Run it and watch

```sh
cargo run -- run crates/starter/missions/dead-reckoning.yaml --viewer
```

The gray robot drives the square. The blue one starts on top of it, but at the
first corner it turns early, and it cuts every corner after that, straying up
to about 22 m from the gray robot. Its estimator thinks it is farther
along than it is, so it believes it has reached each corner before it gets
there. The fixes keep resetting the belief, but by then the robot has already
turned.

Rerun draws the true positions only. To see the belief itself, the test below
compares it with the truth every tick.

## 6. Test it

`crates/starter/tests/dead_reckoning.rs` runs the mission and records, after
each tick, the distance between the blue robot's belief and its true position.
Mission settings can be changed from the test, the same way the command line
does with `name:=value`:

```rust,ignore
const GAIN: &str = "entities.dead_reckoning.localization.DeadReckoning.speed_gain";

#[test]
fn assuming_the_commanded_speed_drifts_until_the_next_fix() -> Result<()> {
    // It believes it moves at 10 m/s but moves at 8: 0.2 m more error per tick.
    let errors = belief_errors(&[])?;
    assert!(error_at(&errors, 0.1) < 1e-9);
    assert!((error_at(&errors, 2.1) - 4.0).abs() < 1e-9);
    // The fix sampled at 5.1 s replaces the prediction, so 5.0 s is the worst.
    assert!((error_at(&errors, 5.0) - 9.8).abs() < 1e-9);
    assert!(error_at(&errors, 5.1) < 1e-9);
    Ok(())
}

#[test]
fn the_right_speed_gain_predicts_exactly() -> Result<()> {
    let errors = belief_errors(&[(GAIN, "0.8")])?;
    let worst_m = errors.iter().map(|(_, e)| *e).fold(0.0, f64::max);
    assert!(worst_m < 1e-9, "worst error {worst_m} m");
    Ok(())
}
```

```sh
cargo test -p starter
```

On the straight first leg the error grows by 2 m every second (0.2 m per
0.1 s tick) and drops to zero at each fix. The fixes are sampled at 0.1 s,
5.1 s, 10.1 s, and so on, so the largest error, 9.8 m, is on the tick just
before a fix. The belief at 5.1 s is the fix itself, not the 10 m-off
prediction it replaced. Once the robot turns between fixes, the error added
before and after the turn points in different directions, so it peaks lower,
around 7 m. With `speed_gain: 0.8` the predicted position matches the true one
for the whole run.

The file has two more tests. `noisy_fixes_leave_an_error_after_every_fix` adds
zero-mean Gaussian noise with a 1 m standard deviation to each fix's x and y:
each fix then carries its own error, which the right gain keeps unchanged until
the next fix. `a_wrong_belief_changes_the_real_path` checks what you saw in
Rerun: with the wrong gain the blue robot's true path leaves the gray one's by
more than 10 m, and with the right gain the two stay together.

## Why this is an honest experiment

The three sources of error stay in three places:

- what the robot really does: the motion model (`max_speed_mps: 8`);
- what the sensor adds: `NoisyState` (the noise and the 5 s rate);
- what the robot *assumes*: the estimator (`speed_gain`).

Change one and the others stay as they were. The estimator never gets the true
speed or the actual noise values. And nothing feeds the belief error back into
the physics: the robot's real path changes only because the autonomy acts on a
wrong belief, as it would on a real robot.

## Exercises

1. **Noisy and wrong.** Give the fixes zero-mean noise with a 2 m standard
   deviation in x and y (`position_stddev_m: [2, 2, 0]`) and keep
   `speed_gain: 1.0`. How far does the blue robot stray now? Try it on the
   command line (the quotes keep the shell from reading the brackets):

   ```sh
   cargo run -- run crates/starter/missions/dead-reckoning.yaml \
     'entities.dead_reckoning.sensors.gps.position_stddev_m:=[2, 2, 0]' \
     --viewer
   ```
2. **Blend instead of replace.** A noisy fix is not the truth either. Add a
   `fix_weight` parameter between 0 and 1 and move the belief only part of the
   way to each fix: `position += fix_weight * (fix - position)`. With noisy
   fixes and the right gain, which weight gives the smallest error?
3. **Faster fixes.** Write a test that runs the mission at several fix rates
   (override `entities.dead_reckoning.sensors.gps.loop_rate_hz`) and reports the
   largest gap from `worst_gap_m`. How often must fixes arrive to keep the blue
   robot within 5 m of the gray one?
4. **Learn the gain.** Between two fixes the estimator knows how far it
   predicted and how far the fixes say it moved. Use their ratio to update
   `speed_gain` as the robot drives. Does the belief error shrink over the run?

## What you learned

- A localization estimator declares its `inputs`, sets the first belief in
  `initialize`, and updates it in `step` at the end of each tick.
- It predicts from `context.commands` and corrects with readings; `fresh`
  gives only new readings, `latest` the most recent one.
- The mission picks which sensor feeds each input, and swaps estimators
  without touching the autonomy.
- A wrong model makes belief drift from truth, and a wrong belief changes the
  real path, because decisions act on belief.

The interface is in [Plugin API](../reference/plugin-api.md#estimators).

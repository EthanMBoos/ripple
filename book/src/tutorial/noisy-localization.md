# Your first localization estimator

In this tutorial you will write a localization estimator called
`PositionKalman`: a Kalman filter over position. Between position fixes it
predicts where the robot is from the velocity it was commanded, and says how
unsure it is; when a fix arrives it moves toward it by as much as the fix
deserves. You will run it in a mission where the robot moves more slowly than
it thinks, see the belief drift away from the truth and the reported
uncertainty fail to cover it, and watch that wrong belief bend the real path.

Read [Belief vs truth](../concepts/belief-vs-truth.md) and
[Uncertainty](../concepts/uncertainty.md) first. This tutorial assumes you
have done [Your first autonomy plugin](first-autonomy.md), so it explains only
what is new.

## What a localization estimator does

Every entity has one localization estimator. It produces the entity's
*belief* about its own state: `context.state`, a `GaussianState`, which is a
mean (`KinematicState`) and the covariance of its error. The autonomy and
controller read it. The default, `PassThrough`, copies a sensor reading,
mean and covariance, into belief, so with no sensor the belief is perfect
and its covariance zero.

An estimator gets what a real robot would have, and nothing else:

- **readings** from the sensors the mission selects, looked up by input name,
  each with the covariance the sensor reports;
- **commands**: the values the motion model read on each substep of the tick;
- the time, messages, and random numbers.

It never sees the truth. So the only way for belief to go wrong is the way it
goes wrong on a real robot: noisy readings, readings that don't come often,
or a wrong idea of how the vehicle moves. This tutorial models the last two.

## 1. The setup

The robot is a point that moves at the velocity it is given
(`SingleIntegrator`). A `WaypointFollower` autonomy steers it around a 100 m
square at 10 m/s. But its motion model has `max_speed_mps: 8`: asking for 10,
it really moves at 8 m/s. That is the *hidden truth*, a physical fact the
robot's software doesn't know.

The estimator has two settings of its own. `speed_gain` is the fraction of
the commanded velocity it assumes the robot achieves. `process_noise_m2ps`
is how fast it loses confidence in that assumption: the position variance it
adds per second, a random walk, so the covariance grows with the time since
the last fix and not with how finely the time was stepped. A position fix
arrives every 5 s.

| | Value |
| --- | --- |
| Commanded speed | 10 m/s |
| Real speed (motion model) | 8 m/s |
| Assumed speed (estimator) | `speed_gain` x 10 m/s |
| Loss of confidence (estimator) | `process_noise_m2ps`, 1 m²/s |
| Position fixes | at startup and 0.1 s, then 5.1 s, 10.1 s, ...; no position noise |

With `speed_gain: 1.0` the estimator believes the robot moves at 10 m/s,
2 m/s too fast, a steady error its random walk does not describe. With
`0.8` it is right.

## 2. Write the estimator

The finished estimator is in `crates/starter/src/position_kalman.rs`. Its
build half has the same shape as in the autonomy tutorial, plus `inputs`:

```rust,ignore
impl Localization for PositionKalman {
    type Config = PositionKalmanConfig;

    // ...configure, new...

    fn inputs(_config: &PositionKalmanConfig) -> Vec<Input> {
        // A full-state reading; only its position and position covariance are used.
        vec![Input::new("fix", ReadingKind::State)]
    }

    fn initialize(&mut self, context: &mut LocalizationContext) -> Result<()> {
        // Start from the first fix, at rest, as sure of the position as the fix is.
        let fix = context
            .inputs
            .latest::<GaussianState>("fix")?
            .context("PositionKalman needs a position fix at startup")?;
        let mut covariance = StateCovariance::from_sigmas(
            [0.0; 3],
            [f64::INFINITY; 3],
            [f64::INFINITY; 3],
            [f64::INFINITY; 3],
        );
        covariance.set_position(&fix.value.covariance.position());
        *context.state = GaussianState {
            mean: KinematicState {
                position_world_m: fix.value.mean.position_world_m,
                ..KinematicState::default()
            },
            covariance,
        };
        Ok(())
    }

    fn step(&mut self, context: &mut LocalizationContext) -> Result<()> {
        // Predict: move by the velocity the motion model was told to fly, on each substep.
        for substep in 0..context.commands.substeps() {
            let commanded_world_mps = Vec3::new(
                context.commands.get(substep, "velocity_x")?,
                context.commands.get(substep, "velocity_y")?,
                context.commands.get(substep, "velocity_z")?,
            );
            predict(
                context.state,
                commanded_world_mps * self.speed_gain,
                context.commands.time(substep).dt_s,
                self.process_noise_m2ps,
            );
        }

        // Correct: a new fix pulls the prediction toward it.
        if let Some(fix) = context.inputs.fresh::<GaussianState>("fix")? {
            correct(
                context.state,
                fix.value.mean.position_world_m,
                &fix.value.covariance.position(),
            );
        }
        Ok(())
    }
}
```

- **`inputs`** declares the readings the estimator needs. Here that is one
  input, named `fix`, which takes readings of a full state
  (`ReadingKind::State`). The estimator doesn't name a sensor; the mission
  decides which sensor feeds `fix` (see below). It sits with `new` and
  `configure` because it is settled while the mission loads, like `ports`.
- **`initialize`** runs once, before any decision, and sets the first belief.
  `inputs.latest` returns the most recent reading for an input. There is no
  fallback to truth: with no reading at startup the mission stops with this
  error. The covariance says what the filter knows: position as well as the
  fix does, and velocity, attitude, and angular velocity not at all (an
  infinite variance), since this filter never estimates them.
- **`step`** runs at the end of every tick, after the sensors. `context.state`
  is the belief; it persists between ticks, so the estimator updates it rather
  than rebuilding it.
  1. *Predict.* `context.commands` holds what the motion model read on each
     substep. Multiply each commanded velocity by the assumed gain and move
     the belief by it for that substep's `dt_s`.
  2. *Correct.* `inputs.fresh` returns a reading only if it arrived this tick.
     The fix sensor runs every 5 s, so most ticks have none and the prediction
     stands. Using `latest` here would snap back to an old fix every tick.

The two equations are plain functions, so a test can call them without a
simulation:

```rust,ignore
/// One prediction step: the position moves by `velocity_world_mps` for
/// `dt_s`, and its covariance grows by the process noise over that time,
/// `process_noise_m2ps * dt_s` on each axis. Linear in time, so ten steps
/// of a tenth add what one step of the whole adds.
pub fn predict(state: &mut GaussianState, velocity_world_mps: Vec3, dt_s: f64, process_noise_m2ps: f64) {
    state.mean.velocity_world_mps = velocity_world_mps;
    state.mean.position_world_m += velocity_world_mps * dt_s;
    let grown = state.covariance.position() + Matrix3::identity() * (process_noise_m2ps * dt_s);
    state.covariance.set_position(&grown);
}

/// One correction with a position fix of covariance `fix_covariance_m2`: the
/// Kalman gain `K = P (P + R)^-1` moves the position toward the fix by the
/// share of the error the belief owns, and the covariance shrinks to
/// `(I - K) P`. An exact fix (`R = 0`) replaces the position; an exact
/// belief (`P = 0`) ignores the fix. When both are exact on an axis, the
/// pseudo-inverse gives `K = 0` there and the prediction stands; its cutoff
/// is relative to the matrix, so a belief that is merely tiny still takes an
/// exact fix.
pub fn correct(state: &mut GaussianState, fix_world_m: Vec3, fix_covariance_m2: &Matrix3) {
    let belief = state.covariance.position();
    let innovation_covariance = belief + fix_covariance_m2;
    let gain = belief * innovation_covariance.pseudo_inverse(1e-12 * innovation_covariance.norm()).expect("a 3x3 has a pseudo-inverse");
    state.mean.position_world_m += gain * (fix_world_m - state.mean.position_world_m);
    state.covariance.set_position(&((Matrix3::identity() - gain) * belief));
}
```

The gain is the whole idea. When the belief's covariance `P` is large next
to the fix's `R`, the gain is near one and the fix wins; when `P` is small,
the gain is near zero and the prediction stands. With exact fixes, as in
this mission, the gain is exactly one and each fix replaces the position.

The estimator never reads the true speed or `max_speed_mps`. The only links
between them are the `speed_gain` and `process_noise_m2ps` settings, which a
person chose and may have got wrong.

## 3. Register it

In `crates/starter/src/lib.rs`, next to `FollowNearest`:

```rust,ignore
mod position_kalman;
pub use position_kalman::PositionKalman;

pub fn register(registry: &mut PluginRegistry) -> Result<()> {
    registry.register_autonomy::<FollowNearest>("FollowNearest")?;
    registry.register_localization::<PositionKalman>("PositionKalman")?;
    Ok(())
}
```

## 4. The mission

`crates/starter/missions/position-kalman.yaml` has two robots on the same
route. `kalman` uses your estimator; `perfect` has no sensor, so it keeps the
default perfect localization and shows where the route should go.

```yaml
format_version: 1
name: A Kalman filter between position fixes

run:
  end_s: 60
  dt_s: 0.1
  seed: 1

networks:
  GlobalNetwork:          # WaypointFollower listens here for route changes

templates:
  robot:
    position_m: [0, 0, 0]
    position_stddev_m: [0, 0, 0]
    visual_model: sphere
    autonomy:
      WaypointFollower:
        waypoints_m: [[100, 0, 0], [100, 100, 0], [0, 100, 0], [0, 0, 0]]
        repeat: true
        arrival_radius_m: 2
        speed_mps: 10
    motion_model:
      SingleIntegrator:
        max_speed_mps: 8      # the hidden truth: its motors only reach 8 m/s

entities:
  kalman:
    template: robot
    team: 1
    sensors:
      gps:
        plugin: NoisyState
        loop_rate_hz: 0.2    # one fix every 5 s
        position_stddev_m: [0, 0, 0]   # exact fixes; raise to add GPS noise
    localization:
      PositionKalman:
        speed_gain: 1.0          # assumes it moves as commanded: 2 m/s too fast
        process_noise_m2ps: 1.0  # position variance it adds per second between fixes

  perfect:
    template: robot
    team: 2
```

- The fix comes from the stock `NoisyState` sensor, named `gps`. Its noise
  defaults to zero; the mission names `position_stddev_m` anyway so the
  exercises can raise it from the command line (an override replaces a value
  the mission has). `loop_rate_hz: 0.2` runs it once every 5 s. The sensor
  reports the covariance of the noise it adds, so raising the noise also
  changes what the filter is told about each fix.
- `localization:` replaces the default `PassThrough` with `PositionKalman`.
  It has one sensor that gives state readings, so `gps` feeds its `fix`
  input automatically. With two such sensors you would name one:
  `inputs: {fix: gps}`. The scenario in `run.mcap` records the choice.
- The autonomy and controller are unchanged. They act on belief and don't
  know which estimator produced it.

## 5. Run it and watch

```sh
cargo run -- run crates/starter/missions/position-kalman.yaml --viewer --draw-covariance
```

The gray robot drives the square. The blue one starts on top of it, but at the
first corner it turns early, and it cuts every corner after that, straying up
to about 22 m from the gray robot. Its estimator thinks it is farther along
than it is, so it believes it has reached each corner before it gets there.
The fixes keep resetting the belief, but by then the robot has already
turned.

`--draw-covariance` draws the belief itself: a blue circle at the position
the filter believes, one standard deviation across. Watch it grow between
fixes and collapse to nothing at each one. It never reaches the robot's
marker: the belief is 10 m off just before a fix while the circle's radius
is 2.2 m, because the filter allowed for a random wander of 1 m²/s and the
real error is a steady 2 m/s in one direction. A filter whose error sits
outside its own ellipse is *inconsistent*, and this is what it looks like.

## 6. Test it

`crates/starter/tests/position_kalman.rs` runs the mission and records,
after each tick, the distance between the blue robot's belief and its true
position, and the standard deviation the belief reports. Mission settings
can be changed from the test, the same way the command line does with
`name:=value`:

```rust,ignore
const GAIN: &str = "entities.kalman.localization.PositionKalman.speed_gain";
const FIX_NOISE: &str = "entities.kalman.sensors.gps.position_stddev_m";

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
fn the_covariance_grows_between_fixes_and_collapses_at_an_exact_one() -> Result<()> {
    let ticks = run(&[])?;
    // Exact fixes: certain at each one, then 1 m²/s of process noise adds
    // 0.1 m² a tick, so after 49 predictions sigma is sqrt(4.9).
    assert!(sigma_at(&ticks, 0.1) < 1e-9);
    assert!((sigma_at(&ticks, 5.0) - 4.9f64.sqrt()).abs() < 1e-9);
    assert!(sigma_at(&ticks, 5.1) < 1e-9);
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

The file has four more tests. `the_right_speed_gain_predicts_exactly` is the
0.8 case. `noisy_fixes_leave_an_error_after_every_fix` adds zero-mean
Gaussian noise with a 1 m standard deviation to each fix's x and y: each fix
then carries its own error, which the right gain keeps unchanged until the
next fix. `a_noisy_fix_is_weighed_against_the_prediction` checks the gain:
at a noisy fix the belief's precision and the fix's add, `1/P' = 1/P + 1/R`,
so the corrected variance is below both. `a_wrong_belief_changes_the_real_path`
checks what you saw in Rerun: with the wrong gain the blue robot's true path
leaves the gray one's by more than 10 m, and with the right gain the two stay
together.

The plugin file has three more. One checks that ten predictions of a tenth
of a second add the variance one prediction of a second does, so a mission's
substep count does not change what the filter believes. One checks that a
belief that is merely tiny, not exact, still takes an exact fix. The last is
the field's standard check of a filter. When the truth really does wander
off the commanded path by the process noise, and the fixes really are as
noisy as they say, the squared error
divided by the covariance (the normalized estimation error squared, or NEES)
should average the number of estimated axes, three. The test drives `predict`
and `correct` through 20,000 ticks of such a world and checks that it does.

## Why this is an honest experiment

The three sources of error stay in three places:

- what the robot really does: the motion model (`max_speed_mps: 8`);
- what the sensor adds: `NoisyState` (the noise, its reported covariance,
  and the 5 s rate);
- what the robot *assumes*: the estimator (`speed_gain` and
  `process_noise_m2ps`).

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
   cargo run -- run crates/starter/missions/position-kalman.yaml \
     'entities.kalman.sensors.gps.position_stddev_m:=[2, 2, 0]' \
     --viewer --draw-covariance
   ```
2. **Honest about the error.** Raise `process_noise_m2ps` until the circle
   covers the robot's marker just before each fix. What value does it take,
   and how does the filter then treat a noisy fix?
3. **Faster fixes.** Write a test that runs the mission at several fix rates
   (override `entities.kalman.sensors.gps.loop_rate_hz`) and reports the
   largest gap from `worst_gap_m`. How often must fixes arrive to keep the
   blue robot within 5 m of the gray one?
4. **Learn the gain.** Between two fixes the estimator knows how far it
   predicted and how far the fixes say it moved. Use their ratio to update
   `speed_gain` as the robot drives. Does the belief error shrink over the
   run?

## What you learned

- A localization estimator declares its `inputs`, sets the first belief in
  `initialize`, and updates it in `step` at the end of each tick.
- A belief is a mean and a covariance. The estimator grows the covariance
  as it predicts and shrinks it as it corrects; `fresh` gives only new
  readings, `latest` the most recent one.
- The mission picks which sensor feeds each input, and swaps estimators
  without touching the autonomy.
- A wrong model makes belief drift from truth, and a wrong belief changes the
  real path, because decisions act on belief. A covariance that does not
  cover the error is the sign of a wrong model.

The interface is in [Plugin API](../reference/plugin-api.md#estimators).

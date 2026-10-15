# Belief vs truth

A real robot never knows exactly where it is. It has GPS that is a few meters
off, an IMU that drifts, and a compass disturbed by its motors. Its software
acts on an *estimate*. To test autonomy honestly, a simulator has to keep two
things apart:

- **Truth:** where the vehicle really is. The motion model computes it, and
  collisions and scoring use it.
- **Belief:** where the vehicle *thinks* it is, and what it thinks is around
  it. Estimators produce it from sensor readings, and autonomy and controllers
  act on it.

Ripple keeps these as separate values on every entity.

## Who reads and writes what

| Plugin | Reads | Writes |
| --- | --- | --- |
| Motion model | Truth | Truth |
| Sensor | Truth | Readings, messages |
| Localization estimator | Readings, messages, commands | Own-state belief |
| Tracking estimator | Readings, messages, own-state belief | Known contacts |
| Autonomy, controller | Belief: `context.state`, `context.contacts` | Commands on its ports |
| Interaction | Truth of all entities | Truth, health |
| Metrics | Events and messages | Scores |

The key line is the autonomy row. Autonomy and controllers see belief and
nothing else: `context.state` is the entity's estimated own state and
`context.contacts` the contacts it knows about. They don't need to know whether
the belief is perfect or noisy.

## Perfect belief by default

Every entity has two estimators: **localization** (where am I?) and
**tracking** (what is around me?). Both default to `PassThrough`, which copies
a reading into belief. With no sensor, the reading is the engine's *perfect*
measurement, taken at the end of each tick after collisions. So by default
belief equals truth at every decision, for every entity present. That's the
right starting point when you're developing a new behavior: first make it work
with perfect information, then add noise and see what breaks.

There is no fallback to truth: perfect belief is just a perfect sensor feeding
the same `PassThrough` a noisy sensor would.

## Adding noise with a sensor

Sensors measure; they never write belief. The built-in
[NoisyState](https://github.com/EthanMBoos/ripple/blob/main/crates/core/src/plugin/sensor/noisy_state/noisy_state.rs)
sensor, each tick:

1. copies the true state,
2. adds Gaussian noise to position, velocity, and attitude,
3. publishes the result as its reading (`context.publish_reading`).

Add it to an entity in a mission, with the noise you want:

```yaml
sensors:
  NoisyState:
    position_stddev_m: [2, 2, 1]
```

Localization's `PassThrough` now copies that reading instead of the perfect
one, so the vehicle's autonomy and controller act on the noisy state while
collisions and scoring still use the truth. Try `missions/noisy-state.yaml` to
see the difference. `NoisyContacts` does the same for other vehicles, through
tracking.

## Which sensor feeds belief

Each estimator input takes readings of one kind (a full state, or contacts).
If the mission doesn't say which sensor feeds it:

- no sensor of that kind: the perfect measurement;
- exactly one: that sensor;
- several: an error asking you to name one.

In YAML, name the sensor instance explicitly:

```yaml
sensors:
  gps:
    plugin: NoisyState
    position_stddev_m: [5, 5, 1]
localization:
  PassThrough:
    inputs: {state: gps}
```

`inputs: {state: perfect}` keeps perfect localization even with a noisy sensor
installed. The scenario in `run.mcap` records every input's source.

## Timing: estimation ends the tick

Within one tick, the order is:

```text
autonomy -> controllers -> motion -> sensors -> interactions -> networks
         -> estimation (localization, then tracking) -> metrics -> ...
```

With a 0.1 s step: autonomy decides at 0.0; the vehicle moves to 0.1; sensors
measure the state at 0.1 and estimators turn those readings into belief for
0.1; autonomy decides at 0.1 using it. A current reading adds no delay: the
decision at 0.1 uses a measurement of 0.1. Delay appears only where you model
it, such as a slow sensor, a delayed message, or an estimator that keeps an
old reading. An entity that spawns is unknown to the others until their next
measurement, one tick later.

## Writing your own estimator

To model drift, a filter, or a tracker, replace `PassThrough` with your own
localization or tracking estimator. It predicts with its own model (for
example, from the commands the motion model read, in `context.commands`) and
corrects with readings. That keeps the three sources of error apart:

- what the vehicle really does: the motion model;
- what the sensor adds: the sensor;
- what the vehicle *assumes* about its own motion: the estimator.

`PassThrough` has no model, so it has no model error: belief is the reading,
wherever the wind really carried the vehicle. A predicting estimator that is
wrong about the wind drifts by about wind × time between readings before the
next one corrects it: 0.5 m at 5 m/s with 10 Hz fixes, 5 m at 1 Hz. With
frequent fixes that is within the sensor noise; it matters when fixes are slow
or missing, or when a filter trusts its prediction over noisy readings.

A few rules keep such experiments honest:

- An estimator gets the readings and commands a real robot would have. Never
  hand it the actual noise values, the true response gain, or other hidden
  physical parameters; a known calibration or starting position is an explicit
  setting of the estimator.
- Zero sensor noise doesn't mean exact belief: a wrong model or an old reading
  still produces error. Don't swap a predictive estimator for `PassThrough`
  when its noise reaches zero.
- Don't feed estimation error back into physics as an extra disturbance. A
  wrong belief already changes the real path, because the controller acts on
  it; that is the effect to measure.
- A planner that predicts futures can reuse the estimator's equations, but its
  hypothetical states stay separate from truth.

[Your first localization estimator](../tutorial/noisy-localization.md) walks
through one: dead reckoning from commands, corrected by fixes every 5 s. See
[Plugin API](../reference/plugin-api.md#estimators) for the interface.

## Summary

- Truth is physics; belief is what the software thinks.
- Autonomy and controllers see belief only: `context.state` and
  `context.contacts`.
- Sensors produce readings; one localization and one tracking estimator per
  entity turn them into belief.
- With no sensor, belief is perfect: `PassThrough` of a perfect measurement.
- Several possible sensors must be named in `inputs`; nothing is picked
  silently.

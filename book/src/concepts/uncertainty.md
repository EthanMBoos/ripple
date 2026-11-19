# Uncertainty

{{#include ../demos/belief-fixes/embed.html}}

A belief is never just a state. A real estimator reports how sure it is,
a real sensor reports how noisy it is, and a real tracker reports how sure it
is that each object it tracks exists. Ripple carries all three, in the forms
the estimation and tracking fields use, so a filter written for Ripple reads
like one written for a robot. This page says what each object carries, the
conventions behind the numbers, and why each choice was made.

Read [Belief vs truth](belief-vs-truth.md) first: truth is the physics,
belief is what the software thinks, and only sensors see truth.

## The three objects

| Object | Type | Carries |
| --- | --- | --- |
| Truth | `KinematicState` | Position, attitude, velocity, angular velocity. No covariance: it is the physics. |
| A belief or a state measurement | `GaussianState` | A `KinematicState` mean and a `StateCovariance` |
| A position measurement | `PositionMeasurement` | A world position and its 3x3 covariance |
| A detection | `Detection` | A `GaussianState` of one other entity and, if the sensor knows it, its `Identity` |
| A track | `Track` | A `GaussianState`, the tracker's own `track_id`, an `Option<Identity>`, when it was last updated, and an `existence_probability` |

Sensors publish measurements and detections. The localization estimator
turns measurements into the own-state belief, `context.state`, a
`GaussianState`. The tracking estimator turns detections into
`context.tracks`, a list of `Track`. Autonomy and controllers read both;
the stock ones read the means and ignore the covariance, which is allowed.

## The covariance

`StateCovariance` is one 12x12 matrix over the error of a state, in the
order position (world frame, metres), attitude (radians), velocity (world
frame, metres per second), angular velocity (world frame, radians per
second). `position()`, `attitude()`, `velocity()`, and `angular_velocity()`
read a 3x3 block; `set_position()` and the others write one; `from_sigmas`
builds a diagonal from four standard deviations; `sigmas()` reads them back.

- **Attitude error is a rotation vector, not four quaternion variances.**
  The three numbers are a small rotation applied on the body side of the
  mean, `R = R_mean * Exp(delta_theta)`, what Solà's treatment of
  uncertainty on rotation groups calls a right perturbation and how GTSAM's
  `Rot3::retract` works. `NoisyState` composes its roll, pitch, and yaw
  errors in that order on the body side, so its reported block is the
  small-angle approximation of what it does. A library that perturbs on the
  world side must convert; the convention is stated on the type so it can.
- **Zero means exact.** The engine's perfect measurement, the belief a
  `PassThrough` makes of it, and a sensor with no noise all report a zero
  covariance. There is no "unknown" sentinel: every simulated sensor knows
  its own noise, so there is nothing unknown to signal.
- **Infinite means not estimated.** A filter that estimates position only,
  such as the starter's `PositionKalman`, puts an infinite variance on
  attitude and angular velocity. A reader that needs those sees at once
  that the filter has no opinion.
- **A bias is not in the covariance.** `NoisyState` reports the variance of
  the noise it adds and nothing about its bias, because a real sensor does
  not know its own bias either. A filter that trusts the reported covariance
  under a bias is inconsistent, which is the effect to study.
- **Process noise belongs to the estimator.** The simulator supplies the
  commands the motion model read and, since each motion model's `advance`
  is public, the same prediction the vehicle flies. How far the truth may
  stray from that prediction is the estimator's parameter.

## Detections and tracks

- **A detection may carry an identity.** Every stock sensor is cooperative
  and names the entity it measured; a radar-like sensor would not, and a
  tracker of its detections would have only its own track ids. `identity`
  is an `Option` so both fit the same tracker interface.
- **A track has its own id.** `track_id` is the tracker's label, stable for
  as long as the track lives. The stock `PassThrough` tracker uses the
  detected entity's id, so a track from it can be looked up by entity with
  `context.tracks.get(id)`, and it refuses a detection with no identity: a
  tracker of those has to assign and keep its own ids.
- **Existence is a probability.** `existence_probability` is 1.0 from
  `PassThrough`, which believes every detection and drops a track the
  moment it is not detected. A tracker that keeps a track through missed
  detections lowers it, as Autoware's tracked objects do, rather than
  switching a status flag.
- **Perfect tracking is a zero-covariance detection of every entity.** With
  no contact sensor, the engine hands tracking a perfect detection of each
  other entity every tick, so the default belief is complete and exact.
- **The perfect tracks are built once and shared.** Every sensorless entity
  would otherwise convert the same detections into the same tracks, 1.3 KB
  each with its covariance, every tick: the population squared, for copies
  of the same zeros. The engine builds the perfect track list once per tick
  and `PassThrough` shares it (`inputs.perfect_tracks`), so the default
  costs what it cost before covariance existed. A real tracker over an
  all-seeing sensor does own one estimate per entity per contact, and that
  cost is honest: it is what tracking everyone from everywhere costs, and
  the remedy is a range-limited sensor or fewer trackers, not a cheaper
  track.

## Seeing it

A run recorded with `--trace` writes each entity's belief, its covariance,
and its tracks to the `/beliefs` channel of `run.mcap` ([Run
records](../guides/run-records.md)). `--draw-covariance` draws the position
block as a one-sigma ellipsoid in the 3D world and a one-sigma ellipse on
the map, for the entity's own belief and, fainter, for each track, live with
`--viewer`, in an `--export-rrd` export, or in `replay` of a traced run. An
exact belief draws nothing.

## Why these choices

- **The field's forms, not a Ripple invention.** A measurement carries its
  covariance in its own space, as ROS's `PoseWithCovariance` and MAVLink's
  covariance arrays do. A belief is a mean and a covariance, as Stone Soup's
  `GaussianState` and GTSAM's marginals are. A track is a labelled estimate
  with an existence measure, as Autoware's tracked objects are. The
  conventions are the same ones those tools state, which is what a port
  needs to check first.
- **One 12x12, not four 3x3 blocks.** A filter over position and velocity
  has cross terms between them; four separate blocks would have nowhere to
  put them. The block accessors give the common case its short form.
- **Truth stays a bare `KinematicState`.** The motion models, their public
  `advance`, the frame record, and the C++ comparison all work on truth and
  never saw a covariance. Keeping it bare keeps "this value is an estimate"
  visible in the types: a function that takes a `GaussianState` cannot be
  handed the truth by mistake.
- **The perfect measurement is built by the engine.** A `GaussianState`
  with zero covariance, made from the truth snapshot, is what estimators
  initialize from and what a sensorless entity believes. No plugin can
  make one from truth; only the engine and the sensors see truth at all.
- **Plugin names stay.** `NoisyState`, `NoisyContacts`, and `PassThrough`
  keep their names and their SCRIMMAGE twins in the comparison; only what
  they publish grew. The comparison compares means, since SCRIMMAGE's
  covariance was a placeholder.
- **The tutorial estimator is a Kalman filter.** A dead-reckoning example
  could not show a covariance growing between fixes and collapsing at one,
  which is the picture a first estimator should give. The starter's
  `PositionKalman` does, in forty lines of equations, and its consistency
  is checked the way the field checks a filter ([How models are
  checked](../reference/checks.md)).

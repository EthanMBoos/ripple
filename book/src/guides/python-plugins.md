# Plugins in Python

Any plugin can be a Python class: an entity's autonomy, controller, motion
model, sensors, localization, and tracking, and the world's interactions,
networks, and metrics. The mission names it in the slot, the engine stays
Rust, and the run replays exactly, at any worker count, as a Rust plugin's
does. Use it for a library that only exists in Python, or to try an idea
without a compile step. This chapter rewrites the tutorial's
[`FollowNearest`](../tutorial/first-autonomy.md) in Python, runs it, does the
same for the aircraft's PID controller, a sensor, the estimators, a motion
model, and the world plugins, says which of those to keep in Rust, and shows
what the bridge does; the closing section lists challenges for taking the
idea further.

## Quick start

You need [uv](https://docs.astral.sh/uv/) and Rust (`rustup`). From the
repository root:

```sh
uv sync      # builds the Rust module into .venv; a few minutes the first time
source .venv/bin/activate
ripple run crates/python/examples/follow_nearest/follow-nearest.yaml --headless
ripple replay runs/follow-nearest/run000
```

`uv sync` fetches the Python version in `.python-version` and maturin, builds
the module, and installs the `ripple` command into the root's `.venv`. It is
the same command line as the Rust `ripple`, with one more plugin in every
slot, `Python`. The examples below are under `crates/python/examples`, and
the commands run from the repository root. For scripts and CI,
`uv run ripple ...` selects the environment without activation.
A mission with no Python plugin gives the same record from either. After
changing Rust code, rebuild with `uv sync --reinstall-package ripple`.

## The plugin

The chaser's autonomy in that mission:

```yaml
autonomy:
  Python:
    class: follow_nearest.FollowNearest
    speed_mps: 30
```

`class` is the dotted path to the class, as logging configs write it. The
mission's directory goes first on `sys.path`, so `follow_nearest.py` beside
the mission needs no packaging; an installed package works as usual. Every
other key is a keyword argument to the constructor.

`examples/follow_nearest/follow_nearest.py` is the tutorial's plugin line
for line:

```python
from math import atan2, sqrt


class FollowNearest:
    ports = {"outputs": {"desired_heading": ("rad", "world"),
                         "desired_altitude": ("m", "world"),
                         "desired_speed": ("m/s", "none")}}

    def __init__(self, speed_mps=25.0):
        if speed_mps <= 0.0:
            raise ValueError("FollowNearest speed must be positive")
        self.speed_mps = speed_mps

    def step(self, ctx):
        own = ctx.state.position
        nearest = None
        nearest_distance_m = float("inf")
        for contact in ctx.contacts:
            if contact.team_id == ctx.entity.team_id:
                continue
            dx, dy, dz = (contact.position[0] - own[0],
                          contact.position[1] - own[1],
                          contact.position[2] - own[2])
            distance_m = sqrt(dx * dx + dy * dy + dz * dz)
            if distance_m < nearest_distance_m:
                nearest_distance_m = distance_m
                nearest = contact
        if nearest is None:
            heading_rad, altitude_m = ctx.state.heading, own[2]
        else:
            heading_rad = atan2(nearest.position[1] - own[1],
                                nearest.position[0] - own[0])
            altitude_m = nearest.position[2]
        return {"desired_heading": heading_rad,
                "desired_altitude": altitude_m,
                "desired_speed": self.speed_mps}
```

Three things make a class a plugin:

- **`ports`**, a class attribute: the outputs by name, each a `(unit, frame)`
  pair. Units are `m`, `m/s`, `m/s^2`, `N`, `rad`, `rad/s`, or `none`;
  frames are `world`, `body`, `model`, or `none`. These are the names the
  controller downstream reads (`SimpleAircraftControllerPID` reads the three
  above), and the port check at load rejects a mismatch, as it does for a
  Rust plugin.
- **The constructor**, whose keyword arguments are the mission's parameters.
  A key it does not take, or a required one the mission leaves out, fails
  with Python's own error naming the key; its own checks, like the speed
  above, fail the same way.
- **`step(ctx)`**, returning a dict of port values. A key left out, or
  `None`, writes nothing that tick. Optional `initialize(ctx)` runs once
  after construction, and `close(time)` when the entity is removed or the
  run ends.

Compare the Rust version in
[crates/starter/src/follow_nearest.rs](https://github.com/EthanMBoos/ripple/blob/main/crates/starter/src/follow_nearest.rs):
the same input, calculation, and output, without the `Config` struct, the
`Plugin` and `Autonomy` impls, or the registration. The two produce the same
entities, frames, events, and summary on the same mission; the test in
`crates/python/tests` checks it.

## What `ctx` holds

Plain data, floats and tuples, no numpy:

| | |
| --- | --- |
| `ctx.state` | Own-state belief: `position`, `velocity` (3-tuples, world frame), `heading` (yaw, radians), `orientation` (`(w, x, y, z)`), `angular_velocity`. |
| `ctx.contacts` | Known contacts, without the entity itself, in the tracker's order: each has `id`, `team_id`, `position`, `velocity`, `heading`, `sampled_at_s`. |
| `ctx.time` | `time_s`, `dt_s`, `tick`. |
| `ctx.entity` | `id`, `team_id`. |
| `ctx.random` | This plugin's random numbers at this tick; see below. |
| `ctx.inputs` | A controller's input port values by name; empty for an autonomy. |

Like a Rust autonomy, the class sees belief only, never truth; see
[Belief vs truth](../concepts/belief-vs-truth.md). A sensor and the
estimators get the attributes their slot has instead, listed below; asking
for one the slot lacks is an `AttributeError`.

## A controller in Python

The controller slot takes a Python class the same way. Its `ports` also
declares `inputs`, the names it reads from the autonomy upstream, and
`ctx.inputs` carries their values each step:

```python
class SimpleAircraftPid:
    ports = {"inputs": {"desired_heading": ("rad", "world"),
                        "desired_altitude": ("m", "world"),
                        "desired_speed": ("m/s", "none")},
             "outputs": {"throttle": ("none", "none"),
                         "roll_rate": ("rad/s", "model"),
                         "pitch_rate": ("rad/s", "model")}}

    def step(self, ctx):
        state, dt_s = ctx.state, ctx.time.dt_s
        roll_rate = -self.heading.step(ctx.inputs["desired_heading"], state.heading, dt_s)
        ...
        return {"throttle": throttle, "roll_rate": roll_rate, "pitch_rate": pitch_rate}
```

`examples/pid_controller/simple_aircraft_pid.py` is the engine's
`SimpleAircraftControllerPID` line for line, three PID loops included, and
its mission flies both aircraft on it:

```yaml
controller: {Python: {class: simple_aircraft_pid.SimpleAircraftPid}}
```

```sh
ripple run crates/python/examples/pid_controller/follow-nearest.yaml --headless
```

It produces the same frames as the Rust controller on the starter's mission,
and the test suite checks that too. A controller runs every motion substep,
so it is called more often than an autonomy; the cost per call is the same.
An autonomy that declares `inputs` is rejected at load.

## Sensors and estimators in Python

The other three slots take a class the same way, and each declares what a
Rust plugin's `Plugin` impl declares, as a class attribute instead of
`ports`. A sensor says what kind of reading it publishes, and returns one
from `step` (or `None` for no reading this tick):

```python
class NoisyState:
    reading_kind = "state"      # or "contacts", or "other"

    def step(self, ctx):
        truth = ctx.truth
        position = tuple(truth.position[axis] + ctx.random.normal((0, 0, axis), 0.0, 0.5)
                         for axis in range(3))
        return {"position": position}
```

A `state` reading is a `State` (`ctx.truth` itself, for a perfect sensor) or
a dict of the fields to change on top of the truth (`position`, `velocity`,
`orientation`, `angular_velocity`); a
`contacts` reading is a list of `Contact`s or of dicts with `id`, `team_id`,
and state fields; an `other` reading is any Python object, which only a
Python estimator can read. `ctx.truth` is the vehicle's true state and
`ctx.contacts_truth` every other active vehicle's, as the built-in sensors
see them, and this is the one slot that sees truth. `initialize` may return
the first reading too, so the first decision is not made on an empty belief.
`examples/noisy_state/noisy_state.py` is the built-in `NoisyState` sensor,
number for number, and its test shows it draws the same noise on the same
mission; the mission swaps the sensor in under the same instance name:

```yaml
sensors:
  NoisyState:
    plugin: Python
    class: noisy_state.NoisyState
    loop_rate_hz: 10
    position_stddev_m: [0.5, 0.5, 0.5]
```

An estimator declares `inputs`, the readings it takes, each a role name and
a kind, and `ctx.inputs` carries the latest reading for each role as a
`Sample` (`value`, `sampled_at_s`, and `fresh`, true if it arrived since the
last step), or `None` if the sensor has given none yet. A localization also
sees `ctx.state`, its current belief, and `ctx.commands`, what the motion
model was told on each substep of the tick (`time` and `values` by port
name); it returns the belief's changed fields, or `None` to keep it.
`examples/dead_reckoning/dead_reckoning.py` is the starter crate's
`DeadReckoning`:

```python
class DeadReckoning:
    inputs = {"fix": "state"}

    def initialize(self, ctx):
        return {"position": ctx.inputs["fix"].value.position}

    def step(self, ctx):
        x, y, z = ctx.state.position
        for command in ctx.commands:
            dt = command.time.dt_s
            x += command.values["velocity_x"] * self.speed_gain * dt
            ...
        fix = ctx.inputs["fix"]
        if fix is not None and fix.fresh:
            x, y, z = fix.value.position
        return {"position": (x, y, z)}
```

A tracking sees `ctx.pose`, the own-state belief, and `ctx.contacts`, the
list it keeps, and returns the new list or `None`.
`examples/pass_through/pass_through.py` is both default estimators, which
copy each fresh reading into belief, and its mission flies the tutorial's
chase with the chaser's belief of itself and of its target passing through
Python. The three run with:

```sh
ripple run crates/python/examples/noisy_state/noisy-state.yaml --headless
ripple run crates/python/examples/dead_reckoning/dead-reckoning.yaml --headless
ripple run crates/python/examples/pass_through/follow-nearest.yaml --headless
```

Each matches its Rust original in entities, frames, events, and summary, and
replays at any worker count; the test suite checks all three.

## Motion models and world plugins in Python

The remaining slots work the same way, and each is a Rust built-in written
again in the examples. They are the ones to think twice about, below.

A **motion model** declares input ports only and returns the truth's changed
fields; `ctx.truth` is the true state, `ctx.inputs` the controller's
commands, `ctx.forces` what interactions applied since the last step, and
`ctx.terrain` the ground (`None` without terrain). It has no `ctx.random`,
as a Rust motion model has none. `examples/single_integrator` is the
built-in point model:

```python
class SingleIntegrator:
    ports = {"inputs": {"velocity_x": ("m/s", "world"), ...}}

    def step(self, ctx):
        vx, vy, vz = (ctx.inputs["velocity_x"], ...)
        x, y, z = ctx.truth.position
        dt = ctx.time.dt_s
        return {"velocity": (vx, vy, vz), "position": (x + vx * dt, y + vy * dt, z + vz * dt),
                "orientation": from_euler(0.0, pitch, yaw)}
```

An **interaction** sees every entity (`ctx.entities`, each with `index`,
`id`, `team_id`, `truth`, `health`, `active`) and asks `ctx.nearby` which
are close: `pairs_within(range_m)` gives index pairs in the engine's order,
`within(position, range_m)` the indices near a point. It returns a dict of
effects by entity id: `health`, `forces`, `truth` (changed state fields),
`events` as `(kind, [ids])` with the kind `TeamCollision`,
`NonTeamCollision`, or `GroundCollision`, and `stop`. The startup question a
Rust interaction answers (`collision_exists`, where a new vehicle may
appear) is not asked of a Python one.

A **network** has no `step`; it answers `route(link)` once per message on
its way to one subscriber, with `None` to drop it or the delay in seconds.
`link` carries the `sender` and `receiver` endpoints (`entity_id`, `plugin`),
the `topic`, the message's `sequence` on it this tick, the network's
`random`, and `contacts_truth` for a range check. A class attribute
`within_entity = True` delivers only between an entity's own plugins, like
`LocalNetwork`.

A **metrics** plugin gets the engine's events each tick, `ctx.events` (each
with `time_s`, `kind`, `entity_ids`, `delivered_at_s`), with
`ctx.entity_teams` to map an entity to its team, and answers
`report(time_s)` with `{"headers": [...], "teams": {team_id: {"score": s,
"values": {name: value}}}}`.

`examples/world/follow-nearest.yaml` flies the tutorial's chase with all
three written in Python; the collision, the message that carries the
boundary, and the metrics table all pass through Python and come out the
same:

```yaml
interactions:
  SimpleCollision: {plugin: Python, class: simple_collision.SimpleCollision}
networks:
  GlobalNetwork: {plugin: Python, class: global_network.GlobalNetwork}
metrics:
  SimpleCollisionMetrics: {plugin: Python, class: collision_metrics.SimpleCollisionMetrics}
```

```sh
ripple run crates/python/examples/single_integrator/waypoints-point-agents.yaml --headless
ripple run crates/python/examples/world/follow-nearest.yaml --headless
```

### Which slots to keep in Rust

Every slot works, and every example above matches its Rust original to the
bit. The difference is how often each is called, and for what. Measured on
64 point agents for 1,000 ticks at one worker, the all-Rust mission takes
0.08 s through the Python command; swapping one slot to its Python twin:

| Slot | Calls | Run time |
| --- | --- | --- |
| motion model | every vehicle, every substep | 0.26 s |
| interaction | once per tick, with every entity copied in | 0.09 s |
| metrics | once per tick | 0.09 s |
| network | once per message per subscriber | 0.08 s (this mission sends none) |

So, for a research mission:

- **Autonomy, controller, sensors, estimators, metrics: fine in Python.**
  One call per vehicle per tick, or per tick, is milliseconds per simulated
  second at this scale.
- **Motion model: possible, not recommended.** It is the most-called slot,
  every substep of every vehicle, and the physics itself then runs in
  Python; it tripled the run above on the simplest model there is. The
  built-in models are the ones the paper checks against SCRIMMAGE, and a
  model is a good first Rust plugin: the starter crate shows one.
- **Interaction: fine for dozens of vehicles, not hundreds.** Each tick
  copies every entity into Python, and `pairs_within` on hundreds of
  vehicles is a long list to walk in Python.
- **Network: fine while traffic is light.** The cost is one Python call per
  message per subscriber; a mission where every vehicle broadcasts to every
  other each tick puts thousands of calls per tick on it. The built-in
  `GlobalNetwork`, `LocalNetwork`, and `SphereNetwork` cover the usual
  policies.

The rule of thumb from [Speed](#speed) holds for all of them: about 3
microseconds per call plus the Python body. If a plugin outgrows that, write
it in Rust in the starter crate; the mission changes one line.

## Messages

Every slot but the network can publish and subscribe, like a Rust plugin
([Writing plugins](writing-plugins.md#messages)). A class attribute names
the channels it reads, `ctx.messages` holds what arrived, and `ctx.publish`
sends:

```python
class ReportCount:
    subscriptions = [("GlobalNetwork", "Report")]

    def step(self, ctx):
        for message in ctx.messages[("GlobalNetwork", "Report")]:
            ...   # message.sender, .sent_at_s, .delivered_at_s, .value


class Reporter:
    def step(self, ctx):
        ctx.publish("GlobalNetwork", "Report",
                    {"tick": ctx.time.tick, "position": list(ctx.state.position)})
        return {...}
```

The bridge subscribes before `initialize`, so a message can arrive from the
first tick. `ctx.messages` has an entry for every declared channel, empty
or not, with the messages in delivery order; they are taken from the
mailbox, so each is seen once. `publish` copies the value as JSON there
and then, so changing it afterwards changes nothing, and the bridge sends
the queue when the call returns, in order; it works in `initialize` too. A
value is a dict, list, number, string, bool, or `None`, with integers that
fit in 64 bits; anything else is an error naming its type. A `sender` has
`entity_id` (`None` for a world plugin) and `plugin`.

A message is JSON on its way through the mailbox, which is how it crosses
to and from Rust. A Rust plugin's message arrives as the dict the run
record shows for it: the built-in `Boundary` interaction's `BoundaryRegion`
is `{"center_world_m": [x, y, z], "lengths_m": [x, y, z]}`, and a Python
metrics plugin subscribed to `("GlobalNetwork", "Boundary")` gets exactly
that. The other way, a dict of that shape published on the channel reaches
the Rust `Straight` autonomies as a `BoundaryRegion`; a dict of another
shape stops the run in the network phase with an error naming the channel
and the type. The conversion happens at routing, only for a message that
crosses, so a mission without Python plugins pays nothing. Delays are the
network's, as for any message: a Python network that answers 0.3 s delivers
0.3 s later, and `delivered_at_s` says so. A Python metrics plugin keeps
`ctx.events`; subscribing to one of the engine's event channels is an error
at load, since the events are already there.

`examples/messages` has the four cases: `reports.yaml`, three `Reporter`
autonomies counted by a `ReportCount` metrics plugin; `reports-delayed.yaml`,
the same through a Python network with a delay; `boundary-log.yaml`, the
stock networks mission with a Python metrics plugin logging the Rust
boundary; and `boundary-from-python.yaml`, that mission with the boundary
published from Python, which flies the Rust aircraft byte for byte as the
stock mission does:

```sh
ripple run crates/python/examples/messages/reports.yaml --headless
ripple run crates/python/examples/messages/boundary-from-python.yaml --headless
```

A Python network has no mailbox of its own: its `route` runs once per link,
not once per tick, so there is no step in which it could read or send.

## Three rules

- **No shared state.** Each vehicle's object is called from whichever worker
  thread holds its entity. Python's lock serializes the calls but not their
  order, so keep everything on `self` and nothing at module level or in a
  library's global generator. Then the worker count cannot change the
  output, and the test at 1 and 8 workers proves it for the example.
- **Randomness from `ctx.random`.** Never Python's `random` module or an
  unseeded library, or the mission stops replaying. Every number is keyed by
  what it is for, as in [Randomness and seeds](../concepts/randomness.md):
  `ctx.random.uniform("jitter")` is in (0, 1), `ctx.random.normal("turn",
  mean, stddev)`, and `ctx.random.seed("walk")` is an integer for a
  library's own generator. A key is a string, an integer, or a tuple of them,
  so a loop writes `uniform(("sample", i))`. Asking for the same key twice in
  a tick gives the same number. A plugin that seeds `numpy.random.default_rng`
  from `ctx.random.seed` each tick replays and matches at every worker count;
  `tests/random_walk.py` does exactly that.
- **Fail before the first step.** The module, the class, and its ports are
  checked when the mission loads; the constructor runs when the entity is
  created, before its first tick. Every message names the plugin, and from
  construction on, the entity:

  ```text
  ripple: entity chaser: configure Autonomy 'Python': Python plugin
  'nowhere.Missing': ModuleNotFoundError: No module named 'nowhere'
  ```

  A plugin that needs a package the environment lacks fails the same way;
  `uv add` it. The bridge itself needs none.

## Your own project

Pending: this needs the wheels. Today the package is built from a checkout,
so a plugin lives beside a mission in this repository or in a folder you
point the command at. With a wheel, a project of your own is a folder with
`uv add ripple` from the release asset, missions, and the `.py` files beside
them, no packaging:

```sh
uv init my-lab && cd my-lab
uv add ripple --from <the release asset's URL>     # pending
source .venv/bin/activate
ripple run my-mission.yaml --headless
```

with runs and sweeps written under `my-lab/`. The Rust side of the same
idea, a crate of your own with Ripple as a git dependency, works now
([Your own project](writing-plugins.md#your-own-project)). What the wheels
need is in [The Python bridge](../development/python-bridge.md#build).

## How it works

The Python package is an extension module built by maturin, like the
learning crate's. Its `ripple` command builds the same registry as the Rust
executable (the built-ins, the starter, delivery) plus `Python`, hands it to
the Rust command line with Python's lock released, and the engine's worker
threads take the lock back to call each plugin. The Rust `ripple` executable
never links Python; the Python-installed command can run Python plugins.

`Python` is one Rust bridge serving the nine slots,
[crates/python/src/bridge.rs](https://github.com/EthanMBoos/ripple/blob/main/crates/python/src/bridge.rs).
Its `configure` imports the class, reads its declaration (`ports`,
`reading_kind`, `inputs`, or `within_entity`), and keeps the other keys;
`initialize` constructs the object and calls its `initialize` if it has one;
`step` builds the slot's `ctx`, calls the object's `step` (or `route` per
message, for a network), and hands its answer to the engine the way a Rust
plugin would: port writes through `PluginIo`, a reading through
`publish_reading`, a belief, contact list, or truth into the context, an
interaction's effects onto the entities, a routing decision to the message
engine. The engine does not know Python is involved: the slot,
`loop_rate_hz`, the port and reading checks, the mailbox, and the random keys
are the bridge plugin's, like any other's. The design, its reasons, and
what comes next are in [The Python bridge](../development/python-bridge.md).

## Speed

One call costs about 3 microseconds: 64 aircraft each seeing 63 contacts,
10,000 ticks at one worker, the bridge with a plugin that returns constants
added 1.8 s to a 0.44 s Rust run (640,000 calls). The Python `FollowNearest`
added 3.5 s, the rest being its own loop over the contacts. Dozens of
vehicles deciding at tens of hertz cost milliseconds per simulated second,
which is fine for the missions in this book. The controllers and the motion
models stay Rust, so a mission's physics costs the same either way. If a
plugin outgrows Python, a thousand-vehicle sweep or a loop over every
contact every tick, write that plugin in Rust in the starter crate, as the
tutorial does; the mission changes one line.

## Challenges

The bridge covers every slot and messages between them; what it does not
do yet, and projects of a few days each that build on it (a GTSAM smoother,
`pytorch_mppi` under a serial scheduler, a lossy network, a motion model
worth its cost), are listed with the design notes in [The Python
bridge](../development/python-bridge.md).

# scrimmage-rs

scrimmage-rs is a Rust multi-agent robotics simulator for research and teaching.
It is a port of the useful core of [SCRIMMAGE](https://github.com/gtri/scrimmage),
a C++ simulator from Georgia Tech Research Institute, rebuilt to be easy to read,
modify, and learn from.

You describe a scenario in an XML mission file: which vehicles exist, what
software runs on each one, and how the world behaves. The simulator steps every
vehicle forward in time, records what happened, and shows it in the
[Rerun](https://rerun.io) viewer.

> [!NOTE]
> This is a small, checked subset of SCRIMMAGE, not a drop-in replacement. The
> [model scope](https://github.com/EthanMBoos/scrimmage-rs/blob/main/docs/MODEL_SCOPE.md)
> lists what is implemented today.

## Quick start

You need [Rust](https://www.rust-lang.org/tools/install). From the repository
root, install the `scrimmage` command:

```sh
cargo install --path crates/cli --locked
```

Run a mission with two teams of aircraft flying toward each other:

```sh
scrimmage run missions/straight-no-gui.xml --headless
```

Each run is saved in a new folder under the repository: `runs/straight-no-gui/run000`,
then `run001`, and so on.
To watch it, install the [Rerun viewer](https://rerun.io/docs/getting-started/installing-viewer)
and replay the run:

```sh
scrimmage replay runs/straight-no-gui/run000
```

Use `--viewer` instead of `--headless` to watch while the mission runs.

## How a simulation is built

Every vehicle, called an *entity*, runs a small stack of plugins. Each tick,
information flows through the stack in one direction:

```text
autonomy  ->  controller  ->  motion model  ->  new state
new state ->  sensors     ->  what autonomy sees next tick
```

Plugins for the whole world, rather than one vehicle, run after that:
interactions (collisions, boundaries), networks (who can hear whom), and
metrics (scores).

| Plugin type | Its job | Built-in examples |
| --- | --- | --- |
| Autonomy | Decide what to do: desired heading, speed, altitude | Straight, WaypointFollower, AuctionAssign |
| Controller | Turn goals into actuator commands | SimpleAircraftControllerPID, AircraftPIDController |
| Motion model | Move the vehicle using physics | SimpleAircraft, FixedWing6DOF, Multirotor, SingleIntegrator |
| Sensor | Measure the world, with noise | NoisyState, NoisyContacts, NoisyPosition |
| Interaction | Apply world rules: collisions, ground, boundaries | SimpleCollision, GroundCollision, Boundary |
| Network | Deliver messages between plugins | LocalNetwork, GlobalNetwork, SphereNetwork |
| Metrics | Count events and score teams | SimpleCollisionMetrics |

Every plugin type is an extension point: you can write your own and select it
by name in a mission.

## Where to go next

- **New here?** Start with [Your first autonomy plugin](tutorial/first-autonomy.md).
  You will write a plugin that chases the nearest opponent, then test it.
- **Confused by angles or signs?** Read [Coordinate frames](concepts/coordinate-frames.md).
- **Adding sensors or noise?** Read [Belief vs truth](concepts/belief-vs-truth.md).
- **Writing a plugin of another type?** See [Writing plugins](guides/writing-plugins.md).

The repository's
[`missions/README.md`](https://github.com/EthanMBoos/scrimmage-rs/blob/main/missions/README.md)
lists every runnable mission and what to try with each one.

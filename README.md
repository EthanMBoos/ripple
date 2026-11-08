# Ripple

Ripple runs multi-agent robotics missions from a decision to its physical
outcome. A YAML mission chooses agents and plugins for autonomy, control,
motion, sensing, estimation, communication, world interactions, and metrics.
The simulator advances them on one clock. Agents act on estimated state;
sensors and metrics can observe physical truth. Included models range from
point agents to multirotors and fixed-wing aircraft.

You can change the manager, a vehicle model, or its sensors while keeping
the rest of the mission. A seed defines a repeatable run, and Rust outputs
are byte-identical with one, two, or eight workers. Each run writes
an MCAP record; [Rerun](https://rerun.io) can display it. Ripple follows
[SCRIMMAGE](https://github.com/gtri/scrimmage)'s simulation and plugin
design, with its own YAML mission format.

## Current study

The [fleet study](paper/README.md) puts a manager above a dispatcher in a
regional delivery mission. The manager can set local rules, lend an aircraft
for one flight, or move aircraft and stock between hubs. In the reported
missions, local rules and lent flights beat moving capacity; language-model
managers still make those moves, and the losses fall on the hubs that gave.
The next question is the **fleet API**: what the model sees and what it may
order as the region grows. The reported model results cover up to 64 hubs
and 320 aircraft; the same regional mission runs at 1,024 hubs and 5,120
aircraft, with model tests at that size planned. All results are from
simulation.

## Try it

With Rust installed, from the repository root:

```sh
cargo install --path crates/cli --locked
ripple run missions/waypoints-point-agents.yaml --headless
```

The command prints a new `runs/waypoints-point-agents/runNNN` folder. To
watch a run, use `--viewer`; to inspect a saved run, use
`ripple replay <run-folder>`. Both need a Rerun viewer matching the SDK
version in `Cargo.lock`. See [run records](book/src/guides/run-records.md)
for the files and channels.

The [delivery crate](crates/delivery/README.md) explains the regional
mission and its sweeps.

## Make your own mission

Plugins are ordinary Rust structs compiled into the `ripple` command.
Start with [your first autonomy plugin](book/src/tutorial/first-autonomy.md)
in `crates/starter`, then use the
[plugin guide](book/src/guides/writing-plugins.md) and
[YAML mission guide](book/src/guides/yaml-missions.md). The
[book](book/src/SUMMARY.md) covers the simulation loop, models, sweeps,
and records. The [mission index](missions/README.md) lists the examples.

The optional [ArduPilot bridge](book/src/guides/ardupilot.md) lets ArduPlane
SITL fly Ripple's six-degree-of-freedom Rascal through a runway takeoff,
circuit, and landing. The optional [learning demos](crates/learning/README.md)
show single-agent and multi-agent reinforcement learning. In Python, one
policy trains a Crazyflie to reach and hold a goal; another trains three
agents to cover landmarks. Ripple runs the saved ONNX policies.

## Development

```sh
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

The [reference suite](reference/README.md) compares vehicle models with
JSBSim, RotorS, and Crazyflow, and checks Aerosonde against a separate
transcription of its published equations. It also checks determinism,
speed, and shared behavior against SCRIMMAGE.

## License

[GNU Lesser General Public License v3.0 or later](LICENSE).

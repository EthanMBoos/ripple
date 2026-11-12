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

The [fleet study](paper/README.md) uses Ripple to ask which decisions a
language-model manager should make above a dispatcher. In a regional
delivery mission, managers can change local rules, let a neighbor's
aircraft serve a request and return, or move aircraft and stock between
hubs. On matched days, moving capacity often costs the giving hubs more
than it helps the surge. Models still order or keep moves as live managers,
vetoes, and authors of rules. A second mission tests the **fleet API**:
the report a manager receives and the help order it may give. Choosing a
lender changes deliveries, yet Haiku and Opus match a simple rule based
on spare aircraft at the 16 and 64 hubs reported in the manuscript. The
fixed-policy mission also runs at 1,024 hubs and 5,120 aircraft. All
results are from simulation.

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

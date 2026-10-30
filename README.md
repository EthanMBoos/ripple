# Ripple

https://github.com/user-attachments/assets/88d48e3a-dec9-4937-9eec-8b08ec797499

Ripple is a multi-agent robotics simulator written in Rust for research and
teaching. The autonomies, controllers, motion models, sensors, estimators,
and metrics are each a plugin that can be replaced on its own. Runs are
deterministic and are recorded, which makes it possible to compare methods
on identical episodes and to replay any run.

## Quick start

There are two ways to use Ripple. Most users write plugins in Python and run
missions with the Python package. Developers who work on the simulator
itself build the Rust command with cargo.

### Python

Install [uv](https://docs.astral.sh/uv/) and Rust (`rustup`). From the
repository root:

```sh
uv sync                 # builds the simulator into .venv (a few minutes)
source .venv/bin/activate
ripple run crates/python/examples/follow_nearest/follow-nearest.yaml --headless
ripple replay runs/follow-nearest/run000
```

A plugin is a Python class in the same directory as the mission that names
it. The guide [Plugins in Python](book/src/guides/python-plugins.md) gives
an example of each plugin type and notes which types are better written in
Rust for speed. [Training a single
agent](book/src/guides/training-single-agent.md) trains a policy with
reinforcement learning and flies it.
For scripts and CI, `uv run ripple ...` selects the same environment without
activating it first.

### Rust

Install Rust. If you activated `.venv` above, run `deactivate` first so
`ripple` resolves to the Rust executable. From the repository root:

```sh
cargo install --path crates/cli --locked
ripple run missions/straight-no-gui.yaml --headless
ripple replay runs/straight-no-gui/run000
```

This is the command used for the checks, the comparison with SCRIMMAGE, and
the experiments in the paper. Rust plugins go in `crates/starter`; the
tutorial [Your first autonomy plugin](book/src/tutorial/first-autonomy.md)
writes one. The [source map](book/src/development/source-layout.md) and
[Rust style](book/src/development/rust-style.md) describe the engine. A
mission without Python plugins produces the same output from either
command.

### Runs

Each run is written to a new directory, `runs/<mission>/run000`, `run001`,
and so on, as one `run.mcap` file holding the frames, events, metrics, and
the resolved scenario. Replay requires the `rerun` viewer on the PATH, at the
version in `Cargo.lock`. The `--viewer` option shows a run as it happens,
`--export-rrd` saves a Rerun recording, and `--draw-trails` draws each
agent's recent path. [Run records](book/src/guides/run-records.md) describes
the file and how to read it.

Other missions to try:

```sh
ripple run missions/waypoints-point-agents.yaml --headless    # point agents following waypoints
ripple run missions/networks-local-global.yaml --headless     # aircraft, sensors, and messaging
ripple sweep missions/waypoints-point-agents.sweep.yaml       # a parameter sweep
```

## How it works

Point agents, multirotors, and fixed-wing aircraft share one world and one
pipeline for sensing, communication, and estimation. Each agent runs a chain
of plugins:

```text
autonomy -> controller -> motion model -> sensors -> (next autonomy step)
```

The autonomy acts on the agent's own estimate of the world, never on the
true state. Interaction, network, and metrics plugins act on the whole world
and handle collisions, communication, and scoring. A mission file names the
plugin in each slot, which can be a built-in or one written in Rust or
Python. Output is byte-identical at any number of worker threads, and every
run is recorded with [Rerun](https://rerun.io).

Ripple is a port of the simulation and plugin design of
[SCRIMMAGE](https://github.com/gtri/scrimmage). The first study made with
Ripple, in which a language model plans deliveries for a mixed fleet, is
described in [the paper](paper/README.md).

## Further reading

The [book](book/src/SUMMARY.md) covers the concepts, YAML missions and
sweeps, the plugin API, and the built-in models. It can be viewed with
`mdbook serve book --open`. The [missions](missions/README.md) directory
lists what ships with the simulator, and the
[roadmap](docs/TODO.md) lists what is planned.

## Development

```sh
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

`python3 reference/suite.py` runs the quick checks, and `--release` runs
every check ([reference/README.md](reference/README.md)). The Python
package's tests run with `uv run pytest`. After a change to Rust code, `uv
sync --reinstall-package ripple` rebuilds the Python module. Contributor
rules are in [AGENTS.md](AGENTS.md), and the design of the Python bridge is
in [The Python bridge](book/src/development/python-bridge.md).

## License

[GNU Lesser General Public License v3.0 or later](LICENSE).

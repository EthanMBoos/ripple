# Ripple

Multi-robot methods are mostly compared on simulated benchmarks where agents
are points, observations are exact, and messages always arrive. Ripple is a
multi-agent robotics simulator in Rust for asking what happens to those
comparisons once sensing is noisy, messages are lost or late, and the agents
are real vehicles.

Three things make that question answerable:

- **Decisions see only belief.** Every agent acts on what it measured or was
  told, never the true state, so a learned and a classical policy get exactly
  the same information.
- **Runs are deterministic.** The same mission and seed give byte-identical
  output at any number of workers, so methods can be compared on exactly the
  same episodes, and any run can be repeated.
- **Disturbances are settings.** Noise, loss, latency, wind, and vehicle
  dynamics are each a parameter, off by default, that a sweep can vary from
  none to severe. The models the study uses are [checked](reference/checks.md)
  against an independent implementation or published measurements.

It is also built to be read: plugins are ordinary Rust structs, and a new
model is one file.

## Quick start

You need [Rust](https://www.rust-lang.org/tools/install). From the repository
root, install the `ripple` command:

```sh
cargo install --path crates/cli --locked
```

Run a sweep: point agents following waypoints, at two speeds, two route
changes, and two seeds, for 8 runs in all:

```sh
ripple sweep missions/waypoints-point-agents.sweep.yaml
```

The results are in `sweeps/waypoints-point-agents/run000/results.jsonl`, one
line per run with its settings and each team's scores. [Sweeps and
results](guides/sweeps.md) shows how to read them into a table.

To watch a single run, run the mission on its own, then replay it in the
[Rerun viewer](https://rerun.io/docs/getting-started/installing-viewer):

```sh
ripple run missions/waypoints-point-agents.yaml --headless
ripple replay runs/waypoints-point-agents/run000
```

Use `--viewer` instead of `--headless` to watch while it runs.

## How a simulation is built

A mission is a YAML file that says which vehicles exist, what software runs
on each, and how the world behaves. Every vehicle, called an *entity*, runs a
small stack of plugins, and each tick information flows through it one way:

```text
autonomy  ->  controller  ->  motion model  ->  new state
new state ->  sensors     ->  estimators    ->  belief autonomy uses next tick
```

World plugins act on every entity: interactions (collisions, boundaries),
networks (who hears whom), and metrics (scores). Every plugin type is an
extension point: write your own and select it by name in a mission. [How a
run works](concepts/how-a-run-works.md) gives the whole tick, and [Built-in
models](reference/models.md) lists what ships.

## Where to go next

- **New here?** Start with [Your first autonomy
  plugin](tutorial/first-autonomy.md): you'll write a plugin that chases the
  nearest opponent, test it, and sweep it.
- **Adding sensors or noise?** Read [Belief vs
  truth](concepts/belief-vs-truth.md), then [Your first localization
  estimator](tutorial/noisy-localization.md) and
  [Disturbances](guides/disturbances.md).
- **Confused by angles or signs?** Read [Coordinate
  frames](concepts/coordinate-frames.md).
- **Writing another kind of plugin?** See [Writing
  plugins](guides/writing-plugins.md).
- **Setting up an experiment?** See [YAML missions and
  templates](guides/yaml-missions.md) and [Sweeps and
  results](guides/sweeps.md).
- **Working on the engine?** Start with the [source
  map](development/source-layout.md) and [Rust
  style](development/rust-style.md).

[`missions/README.md`](https://github.com/EthanMBoos/ripple/blob/main/missions/README.md)
lists every runnable mission and what to try with each.

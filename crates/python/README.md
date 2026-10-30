# Plugins in Python

The Python package `ripple`: the `ripple` command with one more plugin in
every slot (autonomy, controller, motion model, sensor, localization,
tracking, interaction, network, metrics), named `Python`, that runs a class
written in Python. A mission names the class and its parameters:

```yaml
autonomy:
  Python: {class: follow_nearest.FollowNearest, speed_mps: 30}
```

The class declares what its slot needs (`ports`, a sensor's
`reading_kind`, an estimator's `inputs`), takes the mission's parameters as
constructor keyword arguments, and answers `step(ctx)` with port values, a
reading, a belief, a contact list, the truth, or an interaction's effects; a
network answers `route(link)` per message and a metrics plugin
`report(time_s)`. The examples are Rust plugins written again in Python,
each matching its original on the same mission: the book tutorial's
`FollowNearest` (`examples/follow_nearest`), the engine's
`SimpleAircraftControllerPID` (`examples/pid_controller`), `NoisyState`
(`examples/noisy_state`), and `SingleIntegrator`
(`examples/single_integrator`), the starter's `DeadReckoning`
(`examples/dead_reckoning`), the default `PassThrough` estimators
(`examples/pass_through`), and `SimpleCollision`, `GlobalNetwork`, and
`SimpleCollisionMetrics` together (`examples/world`); `examples/messages`
sends messages between Python plugins and across to Rust ones. The book chapter
"Plugins in Python" walks through them and says which slots to keep in Rust
for speed.

From the repository root, the uv workspace whose one member this is:

```sh
uv sync                          # builds the Rust module into .venv, a few minutes the first time
source .venv/bin/activate
ripple run crates/python/examples/follow_nearest/follow-nearest.yaml --headless
pytest                           # the checks in tests/
```

You need [uv](https://docs.astral.sh/uv/) and Rust (`rustup`); uv fetches
the Python version in the root's `.python-version` and maturin itself. After
changing Rust code, rebuild the module with `uv sync --reinstall-package
ripple`. The example path assumes you run from the repository root.
For scripts, `uv run ripple ...` selects the environment without activation.

- `src/lib.rs`: the module `ripple._native`; `main` builds the registry (the
  same plugins as the Rust `ripple`, plus `Python`) and calls the Rust
  command line with Python's lock released.
- `src/bridge.rs`: the `Python` plugin for each slot, which imports the
  class, checks its declaration at load, constructs it when the entity is
  created, and hands what `step` returns to the engine.
- `src/context.rs`: what `step(ctx)` receives, and how its answers come back.
- `src/network.rs`, `src/training.rs`: a policy's ONNX network run in Rust,
  and the simulation Python steps while training, for the learning tasks.
- `ripple/`: the console script. A mission's directory goes first on
  `sys.path`, so a plugin module beside the mission needs no packaging.
- `ripple/learning/`: the reinforcement-learning tasks (`Hover`, `Spread`),
  each one Python autonomy class trained and flown through the same code,
  their Gymnasium and PettingZoo environments, and the shipped policies;
  `learning/` holds their missions and training scripts (`uv sync --extra
  train`, then `uv run crates/python/learning/train_hover.py`). Book
  chapters "Training a single agent" and "Training a team".
- `examples/`, `tests/`: the examples and the pytest suite.

The design, what comes next, and the challenges are in the book's "The
Python bridge" page (`book/src/development/python-bridge.md`). Windows is
untested until wheels are built.

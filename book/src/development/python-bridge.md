# The Python bridge

How `crates/python` lets a class written in Python fill any plugin slot, why
it is built the way it is, what comes next (the learning crate), and the
projects it opens up. The user's side, writing and running a plugin, is
[Plugins in Python](../guides/python-plugins.md).

## How it works

The Python package `ripple` is an extension module built by maturin. Its
`ripple` command builds the same registry
as the Rust executable (the built-ins, the starter, delivery) plus one plugin
named `Python` in each of the nine slots, and hands it with `sys.argv` to
the Rust command line, `ripple_cli::main`, which takes its arguments rather
than reading the process's because under a console script those start with
the interpreter's. So there is one argument parser, one run folder layout,
one sweep, and a Rust-only mission gives byte-identical output from either
command. The command releases Python's lock for the whole call and the
engine's worker threads take it back to call each plugin. The project root
is the repository path baked in at build time, as the Rust binary's is.

The bridge, `crates/python/src/bridge.rs`, is one Rust plugin per slot over
one shared struct. Each slot differs in three things: which class attribute
it reads at load, what `ctx` holds, and what the class's answer means. The
module doc there has the table. In every slot:

- `configure` imports the class and reads its declaration (`ports` for an
  autonomy, controller, or motion model; `reading_kind` for a sensor;
  `inputs` for an estimator; the optional `within_entity` for a network).
  A missing module, a bad unit or frame, or a port the slot rejects is a
  load error naming the plugin, before anything runs. The other mission keys
  are kept for the constructor; there is no reflection on its signature, so
  a key it does not take fails with Python's own error at construction.
- `initialize` runs `cls(**params)`, then the object's `initialize(ctx)` if
  it has one. The object is made here, not in `Plugin::new`, because `new`
  cannot return an error and a constructor can. For an entity plugin this
  is when the entity is created, before its first tick.
- `step` builds the slot's `ctx`, calls the object's `step` (a network's
  `route`, once per message to one subscriber), and hands the answer to the
  engine the way a Rust plugin would: port values through `PluginIo`, a
  reading through `publish_reading`, a belief, a contact list, or the truth
  into the context, an interaction's effects onto the entities, a routing
  decision to the message engine. `None` means no answer this tick.
- `close` calls the object's `close(time)` if it has one.

`ctx` (`src/context.rs`) is an object whose attributes are a dict built
fresh for each call, so each slot exposes only what its Rust context has and
asking for anything else is an `AttributeError` naming the slot. The values
are plain floats and tuples, no numpy, converted once on the way in; the
answers are plain data converted once on the way out (a state is a `State`
or a dict of the fields to change, a contact list is a list of `Contact`s or
dicts). A sensor's `other` reading is carried as the Python object itself,
so only a Python estimator can read it. A metrics plugin is subscribed by
the bridge to the engine's six event topics and gets them as `ctx.events`,
since a Python plugin cannot subscribe to messages itself yet.

The engine does not know Python is involved: the slot, `loop_rate_hz`, the
port and reading checks, the mailbox, and the random keys are the bridge
plugin's, like any other's. A sensor's random numbers are keyed by its
instance name, so a Python sensor named like the built-in it replaces draws
the same noise; the tests use this.

### Choices and why

- **pyo3 linked one way only.** The module is built by maturin and loaded
  by Python; the Rust `ripple` executable never links an interpreter, and
  `cargo build --workspace` is unchanged. An earlier draft embedded an
  interpreter in the Rust binary behind a feature. It was dropped because it
  needs cargo on the student's machine, links one interpreter, has to find
  the environment's packages by hand, and cargo cannot build pyo3's embedded
  and extension modes in one workspace build.
- **The parallel scheduler stays.** Each vehicle's object is called from
  whichever worker holds its entity; Python's lock serializes the calls but
  not their order. That is why the chapter's first rule is no shared state:
  the lock alone would not save a global, since torch and numpy release it
  inside their operations, so two workers could interleave between seeding a
  global generator and drawing from it. A library that needs a global
  (`pytorch_mppi`'s `torch.randn`) needs the engine to run those steps on
  one thread in entity order, a scheduler change deferred to that challenge.
- **No messages yet.** The mailbox is subscribe-first and keyed by a Rust
  type, so Python messages are a small design of their own, listed below.
- **Interactions answer with effects, not handles.** An interaction's
  entities are copied into Python as read-only objects, and the class
  returns a dict of effects by entity id (health, forces, truth, events,
  stop) that the bridge applies after the call. Handing Python a mutable
  reference into the engine's entity slice would need unsafe lifetimes;
  copying costs one small object per entity per tick, which is why the
  chapter says dozens of vehicles, not hundreds. `ctx.nearby` is a copy of
  the positions with the same queries and the same order as the engine's
  spatial index, so a Python collision plugin visits pairs as the Rust one
  does.
- **A network decides per link.** `route(link)` mirrors the Rust `route`
  closure exactly, one call per message per subscriber, with the same keyed
  random numbers, so a lossy Python network replays. It is the slot whose
  cost scales with traffic, and the chapter says so.
- **A metrics `report` cannot fail.** The engine's `report` returns a value,
  not a result, so a Python exception there stops the run with a panic that
  carries the Python error.
- **Not bridged.** A Python interaction is not asked `collision_exists`,
  the startup check of where a new vehicle may appear; it answers false.
  No C++ plugins: Rust and C++ share only C, so a C++ plugin API would be a
  second plugin system. Wrap a library's few calls behind a C shim in a Rust
  plugin, or use its Python wheel.

### Cost

Measured on a laptop, one worker, release module. 64 aircraft for 10,000
ticks, 640,000 autonomy calls each seeing 63 contacts: Rust `Straight`
0.44 s, a Python plugin returning constants 2.2 s, Rust `FollowNearest`
0.44 s, the Python `FollowNearest` 3.9 s. So the bridge (the `ctx` build
with 63 contacts, the call, the dict read) costs about 3 microseconds a
call, and the Python loop over the contacts the rest. 64 point agents for
1,000 ticks: all-Rust 0.08 s; the motion model in Python 0.26 s, the
interaction 0.09 s, the metrics 0.09 s, the network 0.08 s with no traffic.
The chapter's "Which slots to keep in Rust" draws the line from these.

## Build

Until wheels exist, a student clones the repository and runs `uv sync` in
its root, which builds the module and installs the `ripple` command into
the root's `.venv`. The root `pyproject.toml` is a uv workspace with
`crates/python` as its one member and no package of its own; it forwards the
member's extras, so `uv sync --extra train` at the root installs the
trainers' PyTorch and SB3, and `uv run ripple ...` and `uv run pytest` work
from the root or any directory under it. The machine needs git, rustup, and
uv, nothing else:
uv downloads a Python, fetches maturin in its own build environment, and
calls cargo. The first sync compiles the engine in release and takes a few
minutes. What the repository holds so that this works on a machine that is
not the author's:

- A committed `.python-version` in the root. uv otherwise
  takes the newest Python on the machine, and pyo3 refuses an interpreter
  newer than it knows (0.30 stops at 3.15), so each October's Python would
  break a fresh install until pyo3 is bumped. The pin makes uv download that
  version instead.
- `rust-version` in the workspace package table, inherited by every crate,
  so a stale toolchain fails with "run rustup update" rather than an edition
  parse error.
- A committed `uv.lock`, so everyone resolves the same packages.
- The `ripple` console script in `pyproject.toml`, so `uv run ripple run
  ...` works right after the sync. Inside that environment the name is the
  Python-installed command, not the cargo-installed one.
- A rebuild note: uv does not rebuild the module when only Rust files
  change, so the README keeps the `uv sync --reinstall-package ripple` line.

No CI job builds from source on each platform; Windows is untested until
the wheels work, and the README says so.

Wheels are the maintainer's job, when the package is ready to hand out: built
on CI with maturin for macOS, Linux, and Windows and kept as GitHub release
assets, not on PyPI, so a user installs one with `uv add` from the asset's
URL and nothing is published publicly before its time. One wheel per
platform for every Python version needs the stable ABI (`pyo3/abi3-py310`),
the baked-in project root has to become the working directory or `--root`,
and an equivalence run of a wheel against the local build goes with it. The
`TODO:` comment in `crates/python/pyproject.toml` marks the spot.

## The learning tasks

The learning tasks moved onto the bridge on 2026-10-29. Before, a task was
Rust: `HoverPolicy` and `SpreadPolicy` in a `learning` crate each had an
`observe` in Rust, a `policy.rs` ran the shipped ONNX network or took a
trainer's action, and a second extension module, `ripple_learning`, wrapped
a `Simulation` that Python stepped. Now a task is one Python class of the
chapter's shape, `ripple.learning.hover.Hover` or `spread.Spread`, with
`observe(ctx)` and `act(action)`; its `step` is `act(policy(observe(ctx)))`.
Both `ripple run` (through the `Python` autonomy) and the training
`Simulation` use that class, so training and flight share one `observe`
and one `act`, with the one condition below that `observe` is a function of
`ctx` alone. The Gymnasium and PettingZoo environments, the
training scripts, and the shipped ONNX files stayed; they live in
`crates/python` (`ripple/learning/` and `learning/`), one package and one
build, behind the `learning` and `train` extras.

The two things that had to be settled: the package folded into
`crates/python` rather than depending on it, since a second extension module
meant a second build and a second environment for the same engine; and the
ONNX fast path is gone, since flying a learned policy without Python would
need a Rust twin of `observe`, the duplication being removed. The `learning`
Cargo feature went with it; the network still runs in Rust (`Network`,
`tract-onnx`), exposed to the task. The Python tasks fly the shipped
networks to the bit as the Rust plugins did, checked on six seeds each
before the crate was deleted.

Options and why: (1) tasks in Rust with a Python window for training, which
was the first design, the fastest flight path, and two copies of `observe`
to keep in step by hand; (2) tasks in Python for training with a generated
Rust twin for flight, which keeps the fast path and brings the two copies
back; (3) tasks in Python for both, chosen, because the point of the demo is
students changing a task in Python and the trained and flown code being one
method. The cost is a Python call per agent per decision on the flight path,
about 3 microseconds plus the task's own body. The revisit trigger: if a
learning mission flown through the `Python` slot is slower than the chapter
accepts, add a Rust twin or fold the observation into the exported graph for
that task, as an opt-in beside the Python class, not a return to (1).

How the trainer reaches the task: the training `Simulation` registers the
`Python` autonomy with `register_autonomy_with`, handing each instance a
`ripple.learning.Trainer`, which the bridge adds to `ctx` as `ctx.trainer`.
A task's `initialize` takes its policy from the trainer when it is there and
from the network file when it is not; nothing else in the class knows which
it is. For observations between steps the `Simulation` builds a second
instance of each group's task from the mission parameters and calls its
`observe` on the entity's current belief, in a `ctx` of the autonomy's shape.
That instance is never initialized and shares nothing with the one flying
the vehicle, so `observe` must depend on `ctx` alone; a task that kept a
history on `self` in `observe` would train on one history and fly on
another. The `Task` docstring states the condition. Serving the environment
from the flying instance instead was not done because the environment needs
an observation before the first action exists.

## Challenges

Projects for students, each a few days, in rough order of size. None is
designed here.

- **Messages between Python plugins.** Dicts on a network and topic. The
  mailbox is subscribe-first and keyed by a Rust type, so a subscription
  type and a subscribe call in `initialize` are needed. A message a Rust
  plugin reads is typed, so reaching one needs that type's `Deserialize`.
  Once this exists, a Python metrics plugin can subscribe for itself.
- **A [GTSAM](https://gtsam.org) smoother as a localization.** A factor
  graph over the fixes and the commanded velocities in `ctx.commands`,
  marginalized each step into the belief; compare it with dead reckoning on
  the starter's mission. (An early sketch had a 3-dimensional noise model
  on a 6-dimensional pose prior and read a sample as a value; redo it
  against the real API.)
- **`pytorch_mppi` as a controller.** It draws from torch's global
  generator, which the no-shared-state rule forbids under the parallel
  scheduler. It needs a `serial` mark on the slot that `run_phase` honors
  by stepping those entities in id order on one thread before the rest run
  in parallel, then seeding torch from `ctx.random` before each command.
  Exact on CPU; a GPU's reductions need a test.
- **A lossy network with range.** `SphereNetwork` in Python: `route` keeps
  a link when both endpoints are within range in `link.contacts_truth` and
  `link.random.uniform((sender, receiver, topic, sequence))` passes; check
  it replays and matches the built-in.
- **A Python motion model worth the cost.** A model the built-ins lack, a
  ground vehicle with slip, say, written against `ctx.forces` and
  `ctx.terrain`, then its Rust twin in the starter crate once it is right,
  with an equivalence test between the two.

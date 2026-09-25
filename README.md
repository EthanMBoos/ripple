# scrimmage-rs

https://github.com/user-attachments/assets/88d48e3a-dec9-4937-9eec-8b08ec797499

scrimmage-rs is a Rust port of SCRIMMAGE's `Ubuntu-24.04` branch for multi-agent
robotics simulation. It is both:

- a runnable mission simulator with Rerun visualization and C++ output
  comparisons; and
- a reusable simulation core for projects that supply their own plugins.

## At a glance

| Area | SCRIMMAGE (C++, `Ubuntu-24.04`) | scrimmage-rs |
| --- | --- | --- |
| Scope | Broad legacy model and mission catalog. | Small, verified research/teaching subset; not a drop-in replacement. |
| Plugin design | Seven categories, with runtime shared-library loading. | Same seven roles; ordinary Rust structs compiled into the application. No runtime loading. |
| Visualization | VTK viewer and plugin drawing. | Rerun map, debug panels, recordings, and looping replay. No VTK or general plugin drawing yet. |
| Parallel execution | Legacy C++ threading implementation. | Entity-owned state and barriers between phases; selected missions match at 1/2/8 workers. |
| Preliminary performance | Single-thread reference. | C++-mimic baseline: 1.24–1.36× faster at one worker for two 128-aircraft workloads; eight workers slower. Same emulated Linux container, not a general speed claim. [Benchmark](reference/benchmark.py). |
| Mission input | XML and the wider legacy template/parameter surface. | Curated XML, kept permanently for single runs. YAML missions, templates, and `scrimmage compare` work; sweeps are planned ([MISSION_YAML.md](docs/MISSION_YAML.md)). |
| Communication / spawning | Typed pub/sub plus legacy protobuf/string-map spawn commands. | Typed in-process pub/sub and scheduled spawning. No legacy spawn commands; connection-triggered spawning is planned. |
| Integrations / GPU | ROS 1, ArduPilot, JSBSim, and legacy OpenCL paths. | None implemented yet. ROS 1/2, ArduPilot UDP, and optional Burn are planned; JSBSim is verification-only. |
| Running / outputs | Legacy run options and log-directory conventions. | `scrimmage run` / `replay`; automatic `runs/<mission>/runNNN` folders; `scrimmage sweep` and Slurm shards. Comparisons are Python tooling. |
| Compatibility | Reference implementation. | Selected equations and mission outputs checked against C++; known RNG differences remain. |

See [working missions](missions/README.md), [verification evidence](docs/EVIDENCE.md),
and [remaining work](docs/TODO.md) for the exact scope. Existing C++ integration
paths are listed for context, not claimed to have been verified here.

The entity plugin stack connects decisions to motion and perception:

```text
autonomy -> controller -> motion model -> updated state
updated state -> sensors -> observations for the next autonomy step
```

Autonomy chooses desired commands. Controllers turn those commands into motion
inputs, motion models advance the agents, and sensors observe the updated
world. Global interaction, network, and metrics plugins handle effects between
agents, communication, and scoring.

A mission file, XML or YAML, defines the plugin stacks, initial states, teams,
generation, and simulation settings. Each plugin's Rust parameter struct
supplies the defaults that a mission can override. Each run writes frames, events, team summaries, run metadata, and,
unless disabled, a Rerun recording.

`scrimmage-core` owns entity state, plugin lifecycles, communication, and the
simulation loop. Entity work runs across workers with barriers between phases;
world-level plugins run on the coordinator. Plugins implement their category's
interface without managing threads or modifying the simulator.

Each built-in is one Rust file in its own directory: parameters, defaults, and equations.
The framework keeps familiar SCRIMMAGE responsibilities such as
`simcontrol`, `entity`, `parse`, `pubsub`,
and `plugin_manager`; see [docs/SOURCE_LAYOUT.md](docs/SOURCE_LAYOUT.md).

The current built-ins cover aircraft and point-agent navigation, PID control,
boundary response, air/ground collisions and scoring, local/global messages,
NoisyState sensing with separate belief feedback, NoisyContacts measurements,
and a SphereNetwork/AuctionAssign radio exchange. Try
`missions/networks-local-global.xml` for the aircraft/sensor/world example or
`missions/waypoints-point-agents.xml` for the smallest navigation model.
The [mission guide](missions/README.md) and [model selection](docs/MODEL_SCOPE.md)
describe the supported subset; [verification evidence](docs/EVIDENCE.md) explains
what to run and what is actually established.
The port keeps SCRIMMAGE's useful mission and plugin design, with Rerun replacing
VTK and deterministic CPU scheduling replacing the C++ threading path. Compiled
Rust extensions stay; runtime plugin loading and legacy string-map/protobuf
spawning are intentionally excluded. The reviewed scope and tradeoffs are in
[docs/TODO.md](docs/TODO.md), including optional integrations and later Burn/YAML work.

Run the aircraft missions to explore the current simulator. Use
[docs/RUST_PLUGINS.md](docs/RUST_PLUGINS.md) to add a sensor, motion model,
autonomy, controller, interaction, network, or metrics plugin.

After the selected behavior and lifecycle contracts are covered, user-owned
application crates should become the primary way to assemble and run simulations.
The rationale and boundaries are
in [docs/LIBRARY_FIRST_REFACTOR.md](docs/LIBRARY_FIRST_REFACTOR.md); that redesign
is deferred, not the current porting task.

## Run it

Install the command from the repository root with Rust and Cargo installed:

```sh
cargo install --path crates/cli --locked
```

Then run a mission from the repository root:

```sh
scrimmage run missions/straight-no-gui.xml --workers 8 --headless
```

Runs are saved as `runs/straight-no-gui/run000`, `run001`, and so on: one
folder per mission file, under the repository root wherever you run from.
Directories are created automatically. Use `--output path/to/run`
to choose a different location; existing output directories are never overwritten.
This produces `frames.bin`, `events.json`, `summary.csv`, `manifest.json`, and
`recording.rrd`. With the matching Rerun viewer on your PATH, open the recording:

```sh
scrimmage replay runs/straight-no-gui/run000
```

You can also pass the recording directly:
`scrimmage replay runs/straight-no-gui/run000/recording.rrd`. Replay opens the saved dashboard
in Rerun without rerunning the mission or creating another run directory.
The command stays in the foreground until the viewer exits and reports launch
or viewer failures. It still requires the matching `rerun` executable on PATH.

Use `--viewer` instead of `--headless` to launch Rerun during the run, or
`--no-rerun` to disable recording and viewing. The dashboard provides a
North-up multi-agent map, an optional 3D view, and grouped debug panels.
Viewer setup and known native-viewer limitations are documented in
[AGENTS.md](AGENTS.md) and [docs/TODO.md](docs/TODO.md).

The simulator currently runs unpaced. Live pause/step and time-warp controls,
terrain, meshes, and arbitrary plugin shapes remain to be implemented.

Remaining implementation work is in [docs/TODO.md](docs/TODO.md). C++ comparisons and
compatibility limits are documented in [docs/REFERENCE_NOTES.md](docs/REFERENCE_NOTES.md).
The copied [architecture](docs/ARCHITECTURE.md), [dataflow](docs/DATA_FLOW.md),
[plugin](docs/PLUGIN_DEVELOPMENT.md), and [mission](docs/MISSION_CONFIG.md)
guides describe the original C++ design.

## Documentation

The student-facing book in [`book/`](book/) has a first-plugin tutorial and
pages on coordinate frames, belief vs truth, and writing plugins. Preview it with
`mdbook serve book --open` (install with `cargo install mdbook --locked`). The
`docs/` folder holds the engineering notes, design records, and evidence.

## Development

```sh
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --release --workspace --locked
```

Test the public plugin, messaging, and lifecycle contracts:

```sh
cargo test --locked -p scrimmage-core --test plugin_contracts
```

The mission regression suite requires identical frames, events, and summaries
at 1, 2, and 8 workers:

```sh
cargo test --locked -p scrimmage-core --test simulation_regression
```

Run the Docker C++ reference checks with:

```sh
python3 reference/reference_check.py
```

The [reference workflow](reference/README.md) builds SCRIMMAGE's upstream slim
dependency image and checks eight missions, worker-count equality, and recording
on/off equality. Historical native macOS checks passed; the Docker checks also
expose Linux compatibility gaps, including standard-library random behavior.
Direct C++ event-stream comparison is not implemented. To compare saved runs,
use a fresh report path:

```sh
python3 reference/compare.py path/to/cpp-run path/to/rust-run \
  --report runs/comparison.json
```

Dashboard changes also require the screenshot workflow in [AGENTS.md](AGENTS.md).
Keep the Rerun viewer aligned with the SDK versions in `Cargo.lock`.

## License

The project uses the [GNU Lesser General Public License v3.0 or later](LICENSE).
The license file includes the complete LGPLv3 and GPLv3 texts.

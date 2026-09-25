**This is a research testbed for students to learn and experiment: optimize for readability and simplicity, not industrial-grade bulletproofing.**

Prefer small, direct implementations that handle normal research use. Do not
turn simple requests into validation frameworks, compatibility layers, or
exhaustive edge-case machinery. Keep basic correctness and clear errors, but
document unsupported cases rather than adding complexity for hypothetical needs.
Student-facing code should be easy to follow, modify, and learn from.

# scrimmage-rs development instructions

## Purpose and reference

This is a Rust port of SCRIMMAGE's useful simulation and plugin design, with
intentional compatibility breaks documented in `docs/TODO.md`. Preserve the
seven model roles, explicit dataflow, and observable behavior of retained
capabilities. The source-reviewed roadmap supersedes earlier blanket full-port
promises; do not reintroduce excluded machinery for completeness.

Confirmed boundaries: Rerun instead of VTK, deterministic CPU workers, numbered
run directories, compiled downstream Rust extensions but no runtime plugin
library discovery/loading, and no legacy protobuf/string-map spawn commands.
Do not port legacy OpenCL machinery; evaluate Burn for a concrete optional
workload. Runtime integration candidates are ROS (including required ROS 1
compatibility alongside ROS 2) and ArduPilot. JSBSim is verification-only:
optional offline fixture generation for selected Rust flight models, not a
production plugin, CXX bridge, engine port, or aircraft-XML compatibility layer.
Ordinary Rust tests should consume saved fixtures without needing JSBSim.
Curate mission coverage. XML and YAML missions are both supported
permanently through one typed `ScenarioConfig`: XML is frozen for single runs,
and only YAML gets templates and sweeps (both still planned; see `docs/TODO.md`
section 2 and `docs/MISSION_YAML.md`). A `.yaml` beside a `.xml` mission must
run identically (`scrimmage compare`, `crates/core/tests/yaml_missions.rs`). Typed runtime spawning and the remaining model
catalog still need explicit prioritization. Keep optional adapters out of
the default runtime dependency path.

- Rust workspace: this repository, `scrimmage-rs`.
- C++ source: sibling `../scrimmage`, branch `Ubuntu-24.04`.
- Reference branch: `Ubuntu-24.04`; no fixed commit pin.
- New C++ comparisons use `reference/reference_check.py` and the upstream slim
  dependency Dockerfile.
  Do not change the reference branch or simulator semantics to make checks pass.
- This machine is macOS. Do not run C++ GUI missions. Force the C++ reference
  headless; the native Rerun viewer is the Rust visualization path.

The port is not finished. Read `docs/TODO.md` for remaining work and scope decisions,
and `docs/REFERENCE_NOTES.md` for reference details and compatibility gaps.
Do not infer full parity from matching a few missions.

The user intends a later library-first refactor:
user application crates assemble their own models and call the core directly.
Read `docs/LIBRARY_FIRST_REFACTOR.md` for the reasoning and proposed boundaries.
Establish selected behavior/contracts first; do not implement that redesign
prematurely or make copying excluded features a prerequisite.

## Read before changing code

Read these documents completely, as relevant to the task:

- `docs/RUST_STYlE.md` — exact filename, with a lowercase `l`.
- `docs/ARCHITECTURE.md`
- `docs/DATA_FLOW.md`
- `docs/PLUGIN_DEVELOPMENT.md`
- `docs/MISSION_CONFIG.md`
- `docs/RUST_PLUGINS.md` and `docs/REFERENCE_NOTES.md`

The four copied C++ architecture/dataflow/plugin/mission guides are reference
documents; keep them unchanged unless explicitly asked to edit them. Record
Rust API differences and source-verified corrections separately.
The sibling C++ `development-docs/` contains additional synthesized guidance.
When a synthesized document disagrees with the reference branch's code, investigate and
document the difference rather than silently changing behavior.

The greenfield audit's reduced scope and changed execution model are not this
project's requirements.

## Familiar structure and simple plugin authoring

Existing SCRIMMAGE developers should recognize where things belong. Organize
core responsibilities around the C++ names: `simcontrol`, `entity`, `parse`,
`math`, `common`, `plugin_manager`, `pubsub`, and the plugin interfaces.

Keep one implementation tree, grouped by the seven categories. Do not keep
parallel old/new `plugin/` and `plugins/` implementation folders.
Each concrete built-in has a `plugin/<category>/<plugin>/` directory containing
`<plugin>.rs`. Its mission parameters are a `#[derive(Deserialize)]`
struct with `#[serde(default, deny_unknown_fields)]`; the struct's `Default`
holds the mission defaults (formerly the C++ `PluginName.xml` values), and
`configure` calls `params.parse()` and then validates. Consult sibling `../scrimmage` for unported C++ code
and schemas; do not copy a reference tree into this repository.
Interfaces, registration, scheduling, queues, and erased adapters are framework
code; they must not masquerade as concrete sensor or motion plugins.

All seven types are first-class extension points:

| Type | Responsibility |
| --- | --- |
| Autonomy | Decisions and desired commands |
| Controller | Control laws |
| MotionModel | Physical state propagation |
| Sensor | Perception and observations |
| Interaction | World-level effects, collisions, boundaries |
| Network | Communication topology, loss, and delivery |
| Metrics | Statistics and team scores |

Keep the seven category interfaces extensible through the public registry and
mission configuration; do not replace them with a whitelist of built-in names.
The application's registered constructors define available executable plugins.
Validate names/categories and configuration before execution; unknown plugin
parameters are errors. `SCRIMMAGE_PLUGIN_PATH` discovers optional
`PluginName.xml` overlays that replace defaults, not executable libraries. Do not
remove static extension support or configuration overlays when excluding runtime
loading.
There is no standalone examples application. Test-only models and the nine
preserved plugin contract regressions live under `crates/core/tests/`.
Use the built-in plugins as the current implementation references. Do not
recreate an example application or introduce the later composition API now.

Use ordinary structs, named physical fields, and direct equations. Follow
Rustfmt and Clippy. Keep type erasure, threads, locks, and dispatch machinery
out of plugin author code. Update imports, docs, and tests together
when reorganizing files or changing the API. Do not make unrelated behavior
changes during a structural move.

Follow the consistent concrete-plugin layout in `docs/RUST_STYlE.md` and the
seven working patterns in `docs/RUST_PLUGINS.md`. Keep named configuration and
physical fields, explicit imports, and input/calculation/output stages. Prefer
loops for stateful algorithms without banning clear iterator predicates. Keep
ownership visible rather than hiding borrows behind aliases. Readability-only
changes require before/after output equality; preserve known parity failures
instead of mixing unrelated behavior fixes into the same pass.

Use named module files, not `mod.rs`: `simcontrol.rs` owns the subsystem and
`simcontrol/` holds its helpers. Category files such as `plugin/motion.rs`
use explicit `#[path]` declarations to load each self-contained plugin folder's
named implementation. Keep public module paths/re-exports stable. Test-only
models live under `crates/core/tests/support/`, with `plugin_fixtures.rs` as
their named entry point; do not add a standalone Cargo test target for that glue.

## Execution invariants

- Validate `ScenarioConfig -> ResolvedScenario` before execution.
- Preserve global phase boundaries: generate; autonomy; all controller
  substeps; all motion substeps; sensors; interactions; networks; metrics;
  removal/output/time advancement.
- Do not interleave controller/motion substeps or execute a whole entity chain
  as one parallel task.
- Workers own disjoint mutable entity state. Every phase joins every task,
  including after another task fails or panics.
- Commit message delivery at defined phases. Metrics may consume interaction
  messages in the same tick; autonomy sees post-motion sensor data next tick.
- Every plugin subscriber has its own queue. Immutable multicast payloads may
  be shared; mutable plugin state may not.
- C++ LocalNetwork means same-parent-entity communication, not radio range.
- Preserve legacy spawn RNG draw order on the coordinator. New sensor streams
  use stable entity/plugin identities, never worker order.
- Keep truth, observations, and belief distinct. Do not claim a sensor is a
  C++ NoisyState port or claim belief isolation unless implemented and tested.
- Rerun is an output adapter, never the owner of physics, RNG, or simulation time.
- Preserve legacy frame labels and the repeated terminal timestamp in frame
  files. A separate viewer timeline may disambiguate repeated samples.

## Build and run

Run from the Rust repository root. Inspect `Cargo.toml` if commands stop
matching the workspace; do not assume the C++ working directory is this repo.

The executable is `scrimmage`; the CLI Cargo package remains `scrimmage-rs`.
Install it with `cargo install --path crates/cli --locked`, or use
`cargo run -p scrimmage-rs --bin scrimmage -- ...` during development.
Do not overwrite an existing C++ `scrimmage` installation without approval.

```sh
cargo build --workspace
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p scrimmage-core --test plugin_contracts
```

For a headless run with a Rerun recording:

```sh
scrimmage run missions/straight-no-gui.xml --workers 8 --headless
```

Without `--output`, runs are allocated automatically as `runs/run000`,
`runs/run001`, and so on under the current working directory. Numbering continues
after the highest existing run number; simultaneous runs claim distinct paths.
Parent directories are created automatically. Use `--output path/to/run` for
a descriptive location, which must not already exist. Never overwrite a
previous run merely to reuse a command. Outputs include `frames.bin`,
`summary.csv`, `events.json`, `manifest.json`, and `recording.rrd`.

- `--viewer`: launch native Rerun and stream the recording.
- `--headless`: record without launching a viewer, including for GUI-enabled XML.
- `--no-rerun`: disable both viewing and Rerun recording.
- `--workers N`: set the worker count.
- Trailing `name:=value` arguments substitute mission variables.
- The runner is currently unpaced; pause/time-warp/live simulation controls
  must not be described as implemented.

## Replay saved runs

For saved recordings, use `scrimmage replay runs/run000` or
`scrimmage replay runs/run000/recording.rrd`. Replay launches `rerun` from PATH
with the existing recording and waits for it to exit; it never runs a mission
or allocates another output directory. The dedicated Viewer MCP commands below
remain the low-level workflow when headless/port/window arguments are needed.

## C++ parity checks

Use `python3 reference/reference_check.py`; read `reference/README.md` and the
script's help first. It builds the committed local `Ubuntu-24.04` branch in
Docker using the upstream slim dependency image, then compares fresh Rust runs
at 1, 2, and 8 workers and with headless Rerun recording enabled/disabled.
The image uses `linux/amd64` because the upstream JSBSim installer requires it.
No host C++ installation, native reference build, or C++ GUI is needed.

Preserve failure reports. Native macOS parity does not prove Linux parity:
the legacy RNG currently matches Apple libc++, not Ubuntu's libstdc++.
Do not patch C++ random behavior, change seeds, or loosen tolerances to hide gaps.
Run `python3 -m unittest discover -s reference -p 'test_*.py'` after checker edits.
Saved runs can also be compared individually:

```sh
python3 reference/compare.py path/to/cpp-run path/to/rust-run --report runs/new-comparison.json
```

Important missions:

- `missions/straight-no-gui.xml`
- `missions/test_missions/straight_cpu.xml`
- `missions/test_missions/straight_cpu_mul.xml`
- `missions/verification/aircraft-substeps-spawning.xml`
- `missions/verification/noisy-state-bias.xml` (deterministic sensor/feedback parity)
- `missions/noisy-state.xml` (Rust stochastic sensing; independent RNG streams, not sample-for-sample C++ parity)
- `missions/networks-local-global.xml` (boundary response, local sensing, ground removal)
- `missions/waypoints-aircraft.xml` and `missions/waypoints-point-agents.xml`
  (Rust-native shared route updates)

`missions/README.md` lists the curated runnable set. Do not restore unsupported
upstream demos merely to increase coverage. Use `docs/EVIDENCE.md` for repeatable
checks and recorded results; generated `runs/` reports are ignored, not the only
source of verification instructions.

The original CPU-multiplier mission misspells `motion_multipler`; do not silently
fix the reference input. The separate verification fixture really uses substeps.

Require identical Rust frame bytes, event output, and summaries at 1, 2, and 8
workers. For C++ comparisons use the tolerances in `reference/compare.py` and check team
summaries too. Current comparisons do not compare a direct C++ event stream;
state that limitation. Viewer on/off must leave simulation outputs unchanged.

## Rerun MCP setup

Use a dedicated local viewer with MCP control. The setup and verification
procedure are documented here; no other project checkout is required.

First check the SDK in `Cargo.lock` and the installed viewer:

```sh
rerun --version
```

If registration is needed, the project `.codex/config.toml` entry is:

```toml
[mcp_servers.rerun]
command = "rerun"
args = ["viewer-mcp"]
startup_timeout_sec = 20
required = true
default_tools_approval_mode = "approve"
```

Viewer MCP permission and permission to launch the local `rerun` command are
separate. Follow the active session's approval rules for both. Do not copy a
different project's shell permissions or silently change global approvals.

Use the session's available `mcp__rerun__*` tools when present. Discover their
actual schemas before calling them. No configuration edit or restart is needed
when those tools are already available. Configuration in another repository is
not automatically this repository's project configuration.

If tools are absent, check project trust and MCP registration. Project settings
can require a trusted checkout and a new session to load. Do not silently
change global approvals or install a different viewer to bypass a failure.
Report unavailable MCP control accurately.

## Visual verification with Viewer MCP

A successful build or `rrd verify` is not visual verification. Dashboard and
visualization changes require screenshots and inspection.

Store recordings, screenshots, and a short `REPORT.md` under ignored
`runs/visual-qa/<review>/`. Use a new directory for each review.

Launch a separate viewer for QA, using an available local port:

```sh
rerun runs/run000/recording.rrd --headless --window-size 1920x1200 --bind 127.0.0.1 --port 9878
```

Headless Rerun still needs a working graphics adapter. This is not the C++ GUI.

1. Call MCP `connect` with the launched viewer's endpoint, for example
   `http://127.0.0.1:9878`.
2. Call `viewer_state` and `query_tree`; verify this is the intended SCRIMMAGE
   recording, not a viewer belonging to another project.
3. Wait for data and blueprint loading to finish. Use `resize` if needed to
   match 1920x1200.
4. Call `set_time` on the simulation-time timeline (`time` in the current
   dashboard). Temporal cursor values are nanoseconds; a sequence timeline
   uses indices. Read the available ranges from `viewer_state`.
5. Pause at the chosen checkpoint, confirm the time, and call `screenshot`
   with a saved path. Inspect every image at full resolution.
6. Use observed accessibility-tree locators for UI changes. Observe, act, then
   verify; do not guess widget coordinates or claim an action landed unchecked.
7. Confirm looping separately: resume playback near the end, observe the
   timeline wrap, then leave the user-facing viewer playing from the beginning.

On this Mac, native MCP inspection/resize/screenshot requests have sometimes
timed out even while timeline control worked. Use the dedicated headless
viewer for reliable screenshots. Its test harness advances synthetic time;
do not treat headless cursor speed as a wall-clock playback measurement.
Use native `viewer_state` before/after the end for a native wrap check.
When switching endpoints, call `disconnect` before `connect` and verify the
new recording identity. A successful `open_url` response alone does not prove
that a different recording loaded.

Keep the SDK family in Cargo.lock aligned with the installed viewer (currently
0.36.2). Pinning only the `rerun` wrapper does not prevent its transitive SDK
components from resolving to a newer patch release. Prefer `--locked` builds;
recheck the full family and recording warnings after dependency updates.

One screenshot can cover a layout-only edit. Changes involving motion, spawn,
removal, or timing need at least five checkpoints, including before/after the
relevant event. Choose checkpoints from this mission's events and motion.

The default dashboard should provide:

- A dominant, full-mission, multi-agent map; no tiny world view among many plots.
- All teams and late-spawned agents in view, readable markers/headings/labels.
- A clearly labeled optional 3D view and complete-run path context.
- Compact, grouped debug panels (status, speeds, altitudes), not a panel per agent.
- Simulation-time playback at 1x with the complete run looping.
- Collapsed inspection/source panels by default; they remain available for debugging.

Do not accept clipped agents, empty debug panels, unreadable legends, loading
warnings, incorrect time axes, or a loop that merely stops at the final frame.
Distinguish a local ENU map from actual geographic terrain; do not claim terrain
or mesh support that is not implemented.

The QA report should name the mission, recording, versions, viewport size,
checkpoint times, observed behavior, parity checks, and remaining problems.
If MCP cannot control time or screenshots cannot be captured, report the
incomplete check instead of calling the visualization verified.

## Scope and workspace safety

Preserve existing user changes. Use scoped, recoverable file moves and update
references; do not leave duplicate old trees. Respect filesystem permissions:
this repo may be a sibling of the active writable workspace and require
approval for edits or generated artifacts.

Do not run destructive resets, overwrite reference runs, modify sibling
projects, or claim unfinished features are complete. The latest user request
sets the immediate priority; retain explicitly unfinished related work and
describe it honestly in the handoff.

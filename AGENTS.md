**This is a research testbed for students to learn and experiment: optimize for
readability and simplicity, not industrial-grade bulletproofing.**

Prefer small, direct implementations that handle normal research use. Do not
turn simple requests into validation frameworks, compatibility layers, or
exhaustive edge-case machinery. Keep basic correctness and clear errors, and
document unsupported cases rather than adding complexity for hypothetical needs.

# Ripple

A multi-agent robotics simulator, ported from SCRIMMAGE's simulation and plugin
design. Intentional differences from C++ are in `docs/REFERENCE_NOTES.md`; the
roadmap and scope decisions are in `docs/TODO.md`. Do not reintroduce machinery
the roadmap excludes (runtime plugin loading, legacy protobuf/string-map
spawning, OpenCL, VTK, ROS, JSBSim). Keep optional adapters (ArduPilot, Burn)
out of the default build.

- C++ source: sibling `../scrimmage`, branch `Ubuntu-24.04`. Do not modify it.
  The only allowed C++ changes are on the `benchmarking-edits` comparison
  branch: opt-in instrumentation (tracing, test drivers, selectable RNG) that
  leaves outputs byte-identical, and fixes for documented C++ defects (the
  substep order, AircraftPIDController, the RK4 loads in FixedWing6DOF and
  Multirotor, Multirotor's velocity, moment arms, and idle thrust, and the
  milestone A fixes in `docs/TODO.md`; see `docs/REFERENCE_NOTES.md`).
- User research plugins live in workspace user crates such as `crates/starter`,
  registered in `crates/cli/src/main.rs`; keep the starter building and tested.
- XML and YAML missions share one typed `ScenarioConfig`; XML is frozen, only
  YAML gets templates and sweeps. A `.yaml` beside a `.xml` must run
  identically.

## Read before changing code

Read the relevant book chapters: [Rust
style](book/src/development/rust-style.md), [source
map](book/src/development/source-layout.md), [plugin
API](book/src/reference/plugin-api.md), [user
plugins](book/src/guides/user-plugins.md), [YAML
missions](book/src/guides/yaml-missions.md), and the [C++ reference
guides](book/src/appendix/cpp.md). The copied C++ guides stay unchanged; record
differences in `docs/REFERENCE_NOTES.md`.

User and developer guidance goes in `book/src/` (listed in `SUMMARY.md`);
`docs/` holds plans, audits, and notes. Verification evidence lives in
`paper/data/<check>/` (written by the suite) and `reference/README.md`
describes each check; the paper summarizes it in one table and leaves the
detail to the code release.

The paper (`paper/manuscript/main.tex`, built with `tectonic`) is about the
study, not the port: name SCRIMMAGE rather than "the C++", keep the voice
plain and academic, put weeds in the repository, don't volunteer weaknesses
the reader doesn't need, and mark unrun checks Planned rather than implying
them.

Wrap Markdown and LaTeX prose at 80 columns. Tables, code blocks, and long
URLs are exempt.

## Code rules

- Organize around the C++ names (`simcontrol`, `entity`, `parse`, `math`,
  `common`, `plugin_manager`, `pubsub`) and the plugin categories: Autonomy,
  Controller, MotionModel, Sensor, Interaction, Network, Metrics, plus the
  Rust-only estimators (Localization and Tracking, in `estimator.rs`).
- Each built-in lives in `plugin/<category>/<plugin>/<plugin>.rs`. Parameters
  are a `#[derive(Deserialize)]` struct with `#[serde(default,
  deny_unknown_fields)]` whose `Default` holds the mission defaults; `configure`
  calls `params.parse()`.
- Named module files, not `mod.rs`. Test-only models go in
  `crates/core/tests/support/`. No standalone examples application.
- Keep type erasure, threads, locks, and dispatch out of plugin author code.
  Use plain structs, named physical fields, and direct equations.
- Refactors: no behavior changes mixed in; readability-only changes need
  identical outputs before and after.
- A non-obvious core change that exists because of a measurement or trial
  (profiling, timing) gets a `DEVNOTE:` comment saying what was measured, how,
  the result, and why the code is shaped this way, so the trial can be rerun.
  Call it out when reporting the change. Skip this for plain bug fixes and
  trivial changes.

## Execution invariants

- Phase order: generate; autonomy; each substep runs controllers, then
  motion; sensors; interactions; networks; delivery and estimation
  (localization, then tracking); metrics; removal/output/time. Each of these is
  its own phase: never run a whole entity chain as one parallel task.
- Workers own disjoint entity state; every phase joins every task. Rust output
  must be byte-identical at 1, 2, and 8 workers and with Rerun on or off.
- Every subscriber has its own queue. Preserve legacy spawn RNG draw order;
  sensor random streams use stable entity/plugin identities.
- Keep truth, readings, and belief distinct. Autonomy and controllers see
  belief only; estimators never see truth; sensors never write belief. Rerun
  is output only.

## Build and check

```sh
cargo build --workspace
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
python3 -m unittest discover -s reference -p 'test_*.py'   # after checker edits
```

The executable is `ripple` (package `ripple`); use `cargo run -p ripple --bin
ripple -- run missions/straight-no-gui.xml --headless`. Runs go to
`runs/<mission>/runNNN`; never overwrite a previous run. Checking scripts take
names and paths from `reference/project.py`.

## C++ comparison

Keep the C++ comparison passing after any simulation change. Read
`reference/README.md` first. `python3 reference/suite.py` runs the quick checks
(tests, determinism, the C++ agreement matrix in Docker, speed against
`reference/perf_baseline.json`); `--release` runs every check, and `--retain`
refreshes the evidence in `paper/data/` only when all pass. Refactors and
renames must pass `reference/equivalence.py OLD NEW` (byte-identical outputs).

- Don't run C++ GUI missions; the C++ reference runs headless in Docker.
- Do not change the reference branch, seeds, RNG, or tolerances to make checks
  pass. Preserve failure reports and known parity gaps. Code comments explain
  behavior on its own terms; what is kept from SCRIMMAGE, simplification or
  defect, is listed in `docs/REFERENCE_NOTES.md`. Comparison agreement never
  vetoes a correctness fix, but a fix needs proof: the failing example, a
  focused test of the contract, and the comparisons after. A fix to ported
  code goes on `benchmarking-edits` too, so the two still agree; record it in
  `docs/REFERENCE_NOTES.md`. Never accept a slower
  performance baseline or retain failing evidence to make a check pass.
- The CPU-multiplier mission's `motion_multipler` typo is upstream's; don't fix
  it.

## Visualization

Dashboard or visualization changes need screenshots; follow
[Visual checks with Rerun](book/src/development/visual-qa.md).

## Workspace safety

Preserve user changes. No destructive resets, no overwriting reference runs, no
edits to sibling projects. Don't claim unfinished work is complete.

**This is a research testbed for students. Favor readable, direct code over
industrial-grade machinery.** Handle normal research use, give clear errors,
and document unsupported cases instead of building compatibility layers or
validation frameworks for hypothetical needs.

# Ripple

A multi-agent robotics simulator built from SCRIMMAGE's simulation and plugin
design. Do not add excluded machinery: runtime plugin loading, legacy
protobuf/string-map spawning, OpenCL, VTK, ROS, or JSBSim. Keep ArduPilot and
Burn optional. Scope decisions are in `docs/TODO.md`.

- Ripple needs no SCRIMMAGE checkout. The optional C++ checks default to
  `../scrimmage` and accept `--source PATH`. Never modify its `Ubuntu-24.04`
  branch. On `benchmarking-edits`, only opt-in, output-preserving test
  instrumentation and documented defect fixes are allowed; see
  `docs/REFERENCE_NOTES.md`.
- User research plugins live in workspace user crates such as `crates/starter`,
  registered in `crates/cli/src/main.rs`; keep the starter building and tested.
- Missions are YAML, read into `ScenarioConfig`. Ripple reads no XML;
  `reference/scrimmage_xml.py` writes it for the optional C++ checks.

## Read before changing code

Read the relevant book chapters before changing code: [Rust
style](book/src/development/rust-style.md), [source
map](book/src/development/source-layout.md), [plugin
API](book/src/reference/plugin-api.md), [writing
plugins](book/src/guides/writing-plugins.md), and [YAML
missions](book/src/guides/yaml-missions.md). For SCRIMMAGE, use that checkout's
`docs/source` and record differences in `docs/REFERENCE_NOTES.md`.

User and developer guidance goes in `book/src/`; `docs/` holds plans, audits,
and notes. `reference/README.md` describes the checks, and retained evidence
goes in `paper/data/<check>/`.

There is no study or paper on `main`; the language-model fleet study, its
delivery crate, and its manuscript are on the `llm-fleet` branch. In any
paper, name SCRIMMAGE, write plainly, keep implementation detail in the
repository, and mark unrun checks Planned.

Wrap Markdown and LaTeX prose at 80 columns. Tables, code blocks, and long
URLs are exempt.

## Code rules

- Follow the [source map](book/src/development/source-layout.md) and
  [plugin API](book/src/reference/plugin-api.md). Use named module files,
  not `mod.rs`; put test-only models in `crates/core/tests/support/`. Do not
  add a standalone examples application.
- Keep type erasure, threads, locks, and dispatch out of plugin author code.
  Use plain structs, named physical fields, and direct equations.
- Refactors: no behavior changes mixed in; readability-only changes need
  identical outputs before and after.
- For a core change shaped by profiling or a trial, add a `DEVNOTE:` with the
  measurement, how to repeat it, the result, and the reason. Skip it for plain
  fixes; mention the note when reporting the change.

## Execution invariants

- Phase order: generate; autonomy; each substep runs controllers, then
  motion; sensors; interactions; networks; delivery and estimation
  (localization, then tracking); metrics; removal/output/time. Each of these is
  its own phase: never run a whole entity chain as one parallel task.
- Workers own disjoint entity state; every phase joins every task. Rust output
  must be byte-identical at 1, 2, and 8 workers and with Rerun on or off.
- Every subscriber has its own queue. Key random numbers by purpose and stable
  IDs, never draw order; see [Randomness and
  seeds](book/src/concepts/randomness.md).
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
uv sync --reinstall-package ripple && uv run pytest         # after Python bridge edits
```

The executable is `ripple`. Runs go to `runs/<mission>/runNNN`; never
overwrite one. Reference scripts take names and paths from
`reference/project.py`.

## C++ comparison

Keep the C++ comparison passing after simulation changes; read
`reference/README.md` before running it. Use `python3 reference/suite.py` for
quick checks, `--release` for all checks, and `--retain` to refresh evidence
only after they pass. Refactors and renames must pass
`reference/equivalence.py OLD NEW` with byte-identical outputs.

- Do not change the reference branch, seeds, RNG, tolerances, or performance
  baseline to make checks pass. Preserve failures and known parity gaps.
  Comparison agreement never blocks a correctness fix, but prove the fix with
  a failing example, a focused test, and new comparisons. Apply fixes to
  ported C++ code on `benchmarking-edits` too and record them in
  `docs/REFERENCE_NOTES.md`.
- Compared missions use default estimators, no terrain, and explicit values
  where defaults differ. The C++ reference runs headless in Docker.

## Visualization

Visualization changes need inspected screenshots from an RRD export
(`--export-rrd`). Check the affected times, especially around motion,
spawning, removal, or timing changes. On macOS, use the headless Rerun viewer;
the native viewer has timed out on inspection and screenshots.

## Workspace safety

Preserve user changes. No destructive resets, no overwriting reference runs, no
edits to sibling projects. Don't claim unfinished work is complete.

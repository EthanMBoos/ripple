# Workflow example

The same FollowNearest speed sweep built in C++ SCRIMMAGE and in Ripple, to
compare what it takes to write a plugin and run a sweep in each. The paper no
longer reports it; it's kept as a worked comparison.

- `cpp/`: the hand-fixed generated plugin (`FollowNearest.h`, `.cpp`, `.xml`,
  `CMakeLists.txt`), the mission, the sweep loop, and `results.csv`.
- `run-cpp.sh IMAGE`: regenerates the C++ project with SCRIMMAGE's own scripts,
  swaps in these files, builds, and runs the sweep in the unmodified reference
  image (`ripple-reference:<Ubuntu-24.04 commit>`, built by
  `reference/reference_check.py`).
- Rust: `crates/starter/src/follow_nearest.rs`,
  `crates/starter/missions/follow-nearest.yaml`, and
  `follow-nearest.sweep.yaml`. Rerun with `cargo run --release -p ripple --bin
  ripple -- sweep crates/starter/missions/follow-nearest.sweep.yaml`;
  `rust/results.jsonl` is the saved output.

Both give catches at 26 m/s (45.3 s) and 30 m/s (32.0 s), none at 18 or 22 m/s.

Mistyped keys: with `end="60"` changed to `ending="60"` and `initial_speed` to
`initial_sped` in `cpp/follow-nearest.xml`, C++ runs without a warning for 50 s
at the default 24.3 m/s (no catch). The same typos in the YAML mission
(`end_seconds`, `sped`) stop the Rust run with "unknown field ..., expected
...".

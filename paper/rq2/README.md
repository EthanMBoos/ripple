# RQ2 workflow example

The same FollowNearest speed sweep built in C++ SCRIMMAGE and in SCRIMMAGE-RS
(paper section "One Experiment in Both" and its appendix).

- `cpp/`: the hand-fixed generated plugin (`FollowNearest.h`, `.cpp`, `.xml`,
  `CMakeLists.txt`), the mission, the sweep loop, and `results.csv`.
- `run-cpp.sh IMAGE`: regenerates the C++ project with SCRIMMAGE's own scripts,
  swaps in these files, builds, and runs the sweep in the unmodified reference
  image (`scrimmage-rs-reference:<Ubuntu-24.04 commit>`, built by
  `reference/reference_check.py`).
- Rust: `crates/starter/src/follow_nearest.rs`, `crates/starter/missions/follow-nearest.yaml`,
  and `follow-nearest.sweep.yaml`. Rerun with
  `cargo run --release -p scrimmage-rs --bin scrimmage -- sweep crates/starter/missions/follow-nearest.sweep.yaml`;
  `rust/results.jsonl` is the saved output.

Both give catches at 26 m/s (45.3 s) and 30 m/s (32.0 s), none at 18 or 22 m/s.

Mistyped keys: with `end="60"` changed to `ending="60"` and `initial_speed` to
`initial_sped` in `cpp/follow-nearest.xml`, C++ runs without a warning for 50 s
at the default 24.3 m/s (no catch). The same typos in the YAML mission
(`end_seconds`, `sped`) stop the Rust run with "unknown field ..., expected ...".

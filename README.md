# Ripple

https://github.com/user-attachments/assets/88d48e3a-dec9-4937-9eec-8b08ec797499

Ripple is a deterministic multi-agent robotics simulator in Rust, for
research and teaching. Point agents, multirotors, and fixed-wing aircraft share
one world and one sensing, communication, and estimation pipeline, and each
agent acts on its own belief rather than the true state. Runs are
byte-identical at any worker count, so two strategies can be compared on
exactly the same episodes. Recording and replay use [Rerun](https://rerun.io).

It's being built to answer one question: does the ranking between learned and
classical multi-robot policies hold once sensing is noisy, messages are lost
or late, and the vehicles have real dynamics? See [the paper](paper/README.md).

Ripple grew out of [SCRIMMAGE](https://github.com/gtri/scrimmage) and still
reads its mission files.

Each agent runs a plugin stack:

```text
autonomy -> controller -> motion model -> sensors -> (next autonomy step)
```

World-level interaction, network, and metrics plugins handle collisions,
communication, and scoring. Plugins are ordinary Rust structs compiled into the
`ripple` command; you write your own in the starter crate.

## Quick start

With Rust installed, from the repository root:

```sh
cargo install --path crates/cli --locked
ripple run missions/straight-no-gui.xml --headless
ripple replay runs/straight-no-gui/run000
```

Each run gets a new folder (`runs/<mission>/run000`, `run001`, ...) holding
frames, events, a summary, a manifest, and a Rerun recording. Replay needs the
`rerun` viewer matching the SDK version in `Cargo.lock` on your PATH. Use
`--viewer` to watch live or `--no-rerun` to skip recording. Add `--draw-trails`
to `run --viewer` or `replay` to draw each agent's recent path; trails are never
saved in the recording.

Other missions to try:

- `missions/waypoints-point-agents.xml`: the smallest navigation example.
- `missions/networks-local-global.xml`: aircraft, sensors, and messaging.
- `ripple sweep missions/waypoints-point-agents.sweep.yaml`: a parameter sweep.

## Next steps

- [Your first autonomy plugin](book/src/tutorial/first-autonomy.md): write and
  run a plugin in `crates/starter`.
- [The book](book/src/SUMMARY.md): concepts, YAML missions and sweeps, the
  plugin API, and model references. Preview it with `mdbook serve book --open`.
- [Missions](missions/README.md) and [models](book/src/reference/models.md):
  what is supported.
- [The paper](paper/README.md): the learned versus classical question, and
  how each model is checked. The [roadmap](docs/TODO.md) lists what's next.

## Development

```sh
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

`python3 reference/suite.py` runs the quick checks and `--release` runs every
check; see [reference/README.md](reference/README.md). Contributor rules are in
[AGENTS.md](AGENTS.md).

## License

[GNU Lesser General Public License v3.0 or later](LICENSE).

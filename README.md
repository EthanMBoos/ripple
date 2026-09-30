# scrimmage-rs

https://github.com/user-attachments/assets/88d48e3a-dec9-4937-9eec-8b08ec797499

scrimmage-rs is a Rust port of [SCRIMMAGE](https://github.com/gtri/scrimmage),
a multi-agent robotics simulator. It keeps SCRIMMAGE's mission files and plugin
design, replaces the VTK viewer with [Rerun](https://rerun.io), and runs agents
in parallel with identical results at any worker count. It covers a small,
verified subset of the C++ models, built for research and teaching; it is not a
drop-in replacement.

Each agent runs a plugin stack:

```text
autonomy -> controller -> motion model -> sensors -> (next autonomy step)
```

World-level interaction, network, and metrics plugins handle collisions,
communication, and scoring. Plugins are ordinary Rust structs compiled into the
`scrimmage` command; you write your own in the starter crate.

## Quick start

With Rust installed, from the repository root:

```sh
cargo install --path crates/cli --locked
scrimmage run missions/straight-no-gui.xml --headless
scrimmage replay runs/straight-no-gui/run000
```

Each run gets a new folder (`runs/<mission>/run000`, `run001`, ...) holding
frames, events, a summary, a manifest, and a Rerun recording. Replay needs the
`rerun` viewer matching the SDK version in `Cargo.lock` on your PATH. Use
`--viewer` to watch live or `--no-rerun` to skip recording.

Other missions to try:

- `missions/waypoints-point-agents.xml`: the smallest navigation example.
- `missions/networks-local-global.xml`: aircraft, sensors, and messaging.
- `scrimmage sweep missions/waypoints-point-agents.sweep.yaml`: a parameter sweep.

## Next steps

- [Your first autonomy plugin](book/src/tutorial/first-autonomy.md): write and
  run a plugin in `crates/starter`.
- [The book](book/src/SUMMARY.md): concepts, YAML missions and sweeps, the plugin
  API, and model references. Preview it with `mdbook serve book --open`.
- [Missions](missions/README.md) and [models](book/src/reference/models.md):
  what is supported.
- [The paper](paper/README.md): how the port is checked against C++ and what
  that establishes. [Reference notes](docs/REFERENCE_NOTES.md) list intentional
  differences; the [roadmap](docs/TODO.md) lists scope decisions.

## Development

```sh
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

`python3 reference/reference_check.py` compares against C++ SCRIMMAGE in Docker;
see [reference/README.md](reference/README.md). Contributor rules are in
[AGENTS.md](AGENTS.md).

## License

[GNU Lesser General Public License v3.0 or later](LICENSE).

# starter

User plugins. The stock `ripple` command registers this crate's plugins, so
its missions run with `ripple run` and `ripple sweep` like any other.

```text
src/lib.rs                 register(): adds this crate's plugins
src/follow_nearest.rs      an example autonomy plugin
src/dead_reckoning.rs      an example localization estimator
missions/                  missions and sweeps that use them
tests/                     mission-level tests
```

From the repository root:

```sh
cargo run -- run crates/starter/missions/follow-nearest.yaml --headless
cargo run -- replay runs/follow-nearest/run000
cargo run -- run crates/starter/missions/dead-reckoning.yaml --viewer
cargo run --release -- sweep crates/starter/missions/follow-nearest.sweep.yaml
cargo test -p starter
```

To add a plugin, write it in its own file under `src/`, add a `mod` line in
`src/lib.rs`, and register it in `register()` under the name missions will use.
See [Writing plugins](../../book/src/guides/writing-plugins.md),
[Your first autonomy plugin](../../book/src/tutorial/first-autonomy.md), and
[Your first localization
estimator](../../book/src/tutorial/noisy-localization.md).

# starter

User plugins. The stock `scrimmage` command registers this crate's plugins, so
its missions run with `scrimmage run` and `scrimmage sweep` like any other.

```text
src/lib.rs                 register(): adds this crate's plugins
src/follow_nearest.rs      an example autonomy plugin
missions/                  missions and sweeps that use them
tests/                     mission-level tests
```

From the repository root:

```sh
cargo run -- run crates/starter/missions/follow-nearest.yaml --headless
cargo run -- replay runs/follow-nearest/run000
cargo run --release -- sweep crates/starter/missions/follow-nearest.sweep.yaml
cargo test -p starter
```

To add a plugin, write it in its own file under `src/`, add a `mod` line in
`src/lib.rs`, and register it in `register()` under the name missions will use.
See [User plugins and the starter crate](../../book/src/guides/user-plugins.md)
and [Your first autonomy plugin](../../book/src/tutorial/first-autonomy.md).

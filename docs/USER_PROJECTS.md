# Writing your own plugins

Write your plugins in a user crate in this repository, such as
[crates/starter](../crates/starter). The one `scrimmage` command includes them,
so your missions use `scrimmage run`, `sweep`, `compare`, `replay`, and
`scripts/bulk_run.py` exactly as the stock missions do. To update, `git pull`
and rebuild: `cargo run` rebuilds automatically, but `bulk_run.py` uses the
existing release binary, so run `cargo build --release` first.

```text
crates/core        the simulator and its stock plugins
crates/cli         the `scrimmage` command
crates/starter     user plugins, missions, and tests (edit here, or add crates like it)
```

## Why plugins are compiled in

C++ SCRIMMAGE builds plugins into shared libraries that `scrimmage` finds at
runtime on `SCRIMMAGE_PLUGIN_PATH`. This project deliberately does not load
plugins at runtime (see [TODO.md](TODO.md)); `scrimmage` is compiled with them
instead. The **registry** is the table from the names
missions use to Rust types. [crates/cli/src/main.rs](../crates/cli/src/main.rs)
builds it:

```rust
let mut registry = PluginRegistry::with_builtins();   // stock plugins
starter::register(&mut registry)?;                    // user plugins
scrimmage_cli::main(registry, repo_root)              // run, sweep, compare, replay
```

Every command uses that registry, so a user plugin works wherever a stock one
does. Changing a plugin means rebuilding; `cargo run -- ...` does that.

## Try the starter

`crates/starter` contains `FollowNearest` (the book tutorial's plugin), a
mission, a sweep, and a test. From the repository root:

```sh
cargo run -- run crates/starter/missions/follow-nearest.yaml --headless
cargo run -- replay runs/follow-nearest/run000
cargo run --release -- sweep crates/starter/missions/follow-nearest.sweep.yaml
cargo test -p starter
```

Runs and sweeps go to the repository's `runs/` and `sweeps/` folders, named
after the mission or sweep file.

## Add a plugin

1. Write it in its own file under `crates/starter/src/`, importing the plugin
   API from `scrimmage_core::plugin`. [RUST_PLUGINS.md](RUST_PLUGINS.md) and
   the book describe the API; the stock plugins under `crates/core/src/plugin/`
   are working examples of all seven categories.
2. Add `mod my_plugin;` to `crates/starter/src/lib.rs` and register it in
   `register()` under the name missions will use:

   ```rust
   registry.register_sensor::<MySensor>("MySensor")?;
   ```

3. Use that name in a mission.

Registering a name that a stock plugin or another crate already uses in the
same category is an error at startup, so nothing is silently replaced.

## Add another user crate

Copy `crates/starter` to `crates/my-lab` and rename the package in its
`Cargo.toml`. Remove the copied `FollowNearest` (its file, its `mod` and
`pub use` lines, its registration, its missions, and its test), since the
starter already registers that name. Then make two edits so `scrimmage` includes it:

1. Add it to the workspace `members` in the root `Cargo.toml`.
2. In `crates/cli/Cargo.toml`, add `my-lab = { path = "../my-lab" }`, and in
   `crates/cli/src/main.rs`, call `my_lab::register(&mut registry)?;`.

## Sweeps and Slurm

User missions sweep and shard like stock missions (see
[MISSION_YAML.md](MISSION_YAML.md#sweeps)):

```sh
cargo build --release
python3 scripts/bulk_run.py local crates/starter/missions/follow-nearest.sweep.yaml --jobs 4
```

## Limits

- Everyone's plugins are compiled into one `scrimmage`. Keep your work in your
  own crate, so the only shared lines are the two registration edits.
- Plugins cannot live in a separate repository and still use the stock
  `scrimmage` without editing this one. The command line is already a library
  (`scrimmage_cli::main`), which a separate application could call later; see
  [LIBRARY_FIRST_REFACTOR.md](LIBRARY_FIRST_REFACTOR.md).

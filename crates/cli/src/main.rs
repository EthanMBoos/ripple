//! The `scrimmage` command: the stock plugins plus the user plugin crates in
//! this workspace, with this repository as the project folder.
//!
//! This project deliberately does not load plugins at runtime, so user plugins
//! are compiled in: each user crate is a dependency of this package and adds
//! its plugins here.
//! To add another user crate, add it to crates/cli/Cargo.toml and call its
//! `register` below (see book/src/guides/user-plugins.md).
use scrimmage_core::plugin::PluginRegistry;

fn main() -> anyhow::Result<()> {
    let mut registry = PluginRegistry::with_builtins();
    starter::register(&mut registry)?;
    scrimmage_cli::main(registry, concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

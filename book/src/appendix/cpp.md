# C++ reference guides

These four guides describe the original C++ SCRIMMAGE design. Their contents
are preserved for reference while developing the Rust port:

- [Architecture](cpp/architecture.md)
- [Data flow](cpp/data-flow.md)
- [Plugin development](cpp/plugin-development.md)
- [Mission configuration](cpp/mission-config.md)

For current Rust behavior, use the [plugin API](../reference/plugin-api.md),
[model reference](../reference/models.md), and
[YAML mission guide](../guides/yaml-missions.md).

The copied guides contain known inaccuracies and features outside the Rust
port's scope. In particular, LocalNetwork connects plugins on the same entity;
it is not a range-limited radio network. Source-verified corrections and
compatibility gaps are recorded in the repository's
[reference notes](https://github.com/EthanMBoos/scrimmage-rs/blob/main/docs/REFERENCE_NOTES.md).

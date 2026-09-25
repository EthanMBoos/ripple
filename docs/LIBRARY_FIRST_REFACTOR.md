# Planned: direct Rust construction

Let experiments construct simulations from typed Rust values instead of creating
mission files. Development stays in workspace crates with the current starter
and shared CLI/sweep/Slurm tools.

`Simulation::step()` and custom plugin registration already work. The missing
piece is public construction: entity/world setup and resolved definitions are
internal, so callers currently load and resolve a mission.

```text
XML/YAML -- readers + registry --+
                                +--> shared validation/construction --> Simulation
Rust values -- typed assembly --+
```

## Work

- [ ] Add a small typed setup API for model configuration, initial state,
  populations, and schedules when a concrete experiment needs it.
- [ ] Separate parameter decoding from validation: `Plugin::configure` currently
  does both. Rust values and mission values must receive the same checks without
  serializing Rust configuration back into YAML or duplicating validation.
- [ ] Reuse validated definitions and runtime constructors, with fresh mutable
  plugin state per entity. Keep dispatch and scheduling machinery inside the engine.
- [ ] Make physical step time and legacy output labels explicit. Preserve phase
  order, RNG draws, message timing, cleanup, and existing recordings.

Use the existing crates. YAML remains the normal configuration and sweep path;
arbitrary Rust setup functions do not automatically fit YAML-based sweeps.
[Connection spawning](TODO.md#first-pilot-creating-entities-from-a-connection) should share
validated definitions and creation logic, but does not need this entire API first.

## Acceptance

An integration test or research crate constructs a useful stock/custom-model
experiment without a mission file, including scheduled independent instances.
Compare it with an equivalent XML/YAML setup at 1/2/8 workers: frames, events,
summaries, and recording on/off must agree. Invalid configurations fail before
execution, and failure/removal/shutdown retain their current contracts.

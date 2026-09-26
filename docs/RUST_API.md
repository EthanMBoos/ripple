# Scenario API: design and tradeoffs

The experiment has four parts:

| Part | Responsibility |
| --- | --- |
| `ScenarioConfig` | Plain data describing groups, models, initial conditions, schedules, and simulation settings. |
| `PluginRegistry` | Registered implementations of the seven behavior roles, selected by name. |
| `Simulation` | Validated construction, independent plugin instances, mutable state, and stepping. |
| Shared runner | Output directories, recordings, manifests, and run limits. |

```text
XML reader  --+
YAML reader --+--> ScenarioConfig --> validate/build --> Simulation
Rust setup  --+                           ^
                                   PluginRegistry
```

## Why this design

One scenario description lets students use files or calculate their setup in
Rust while keeping the same defaults, plugin configuration checks, and engine.
`EntityGroupConfig` describes a population and its schedule; `Entity` is one
live individual. Public configuration fields are editable until construction.
`Simulation::new(scenario, &registry, workers)` validates them and keeps the
resolved definitions internal.

This follows a familiar simulation pattern. [MuJoCo's model-editing API](https://mujoco.readthedocs.io/en/stable/programming/modeledit.html)
lets XML parsers and programmatic setup populate `mjSpec`, then compiles it into
a runtime model. [Drake's YAML serialization guidance](https://drake.mit.edu/doxygen_cxx/group__yaml__serialization.html)
uses public fields for ordinary serializable configuration structures. These
are references for the boundaries, not proposals to adopt their frameworks.
[Serde defaults and field adapters](https://serde.rs/field-attrs.html) let the
Rust definitions own defaults while the readers handle file syntax.

## Choices in this implementation

- Public scenario types live in `scenario.rs`; `parse/` handles files.
  YAML keeps small ordered-map adapters for groups and plugins. There is one
  set of group/run defaults, rather than parallel file and engine defaults.
- Plugin descriptions remain registered names plus parameter values. Existing
  `configure` methods perform the same checks for every input source. A caller
  may supply a serializable parameter struct through `with_params`; this avoids
  a second API for assembling live plugin objects. Names and parameter/model
  compatibility are still checked at startup, not by Rust's type checker.
- Typed parameters serialize directly to YAML values before checking finite
  numbers. A prebuilt JSON value cannot retain NaN/infinity: JSON conversion
  may already have replaced them with null. Prefer a parameter struct when
  computing floating-point settings in Rust.
- `Mission` holds the loaded scenario, canonical source path, and file worker/
  viewer defaults. Execution options and source bookkeeping stay outside the
  physical scenario. The manifest records the effective run options separately.
- Serialized scenarios are normalized manifest data. They are not a promise of
  reloadable YAML: mission syntax has labeled maps, templates, and an envelope.
  A small reader adapter is clearer than forcing both representations to match.
- `run_scenario` in the existing CLI library shares output and recording code
  between file-based and Rust-built runs. Scenario-building functions can live
  in a workspace research crate and be called by the CLI. YAML remains the
  input to the existing sweep/Slurm commands; Rust functions are not discovered
  or turned into campaign jobs automatically.

## Intentional changes and retained behavior

Native position variance defaults to zero. A motion model must be selected
explicitly; an empty group default does not silently choose physics. Shared
construction validates finite physical fields, schedules, plugin names and
rates, parameter values, and port connections before execution.

XML scalar `speed` is translated to world-X velocity in its reader. YAML uses
`velocity_mps`; the existing noisy-state-bias fixture was converted explicitly.
The multirotor and starter YAML templates now declare their previous nonzero
position variance. These fixture edits preserve their recorded behavior.

The seven plugin traits, phase ordering, spawn RNG, heading random walk,
six-decimal rounding, and frame timestamps are unchanged by this refactor.
Those runtime quirks have not moved into the XML reader. Removing heading drift
or rounding is a separate deliberate behavior change; neither is necessary for
reproducible sweeps.

## Relationship to other work

ROS/ArduPilot connection spawning is independent. A connection can select a
configured group and supply its starting state through the existing spawn
boundary. Implement its state and timing requirements with the first pilot.

There is no separate typed-object assembly plan. This design replaces the older
proposal to split every plugin's decode and validation methods. Current usage
belongs in the [Rust setup guide](../book/src/guides/rust-scenarios.md);
verification is recorded in the [paper](../paper/README.md).

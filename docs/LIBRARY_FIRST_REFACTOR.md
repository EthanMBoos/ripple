# Later: application-owned composition around scrimmage-core

## Decision and timing

Establish the selected `Ubuntu-24.04` behavior and lifecycle contracts first.
Then make a user-owned Rust application the normal place to assemble and run a
simulation. The bundled `scrimmage` command should be one consumer of the core,
not an application every downstream project must modify or load itself into.

This is the agreed direction for a later refactor, not an implemented API or
authorization to redesign the current port. [TODO.md](TODO.md) remains the
implementation/scope checklist. Keep the seven roles, supported XML surface,
and phase order. Add services only where selected consumers require them.
The later scope review explicitly keeps compiled downstream Rust extensions
and excludes runtime library loading and legacy protobuf/string-map spawning.
Do not make excluded functionality a prerequisite for this refactor.

The separate example application has been removed. Its plugin regression tests
now live in [plugin_contracts.rs](../crates/core/tests/plugin_contracts.rs),
with test-only models under `crates/core/tests/support/`. Those fixtures
protect the current public API; they are not the proposed application template.

## Library-first patterns to use

- Dependencies point inward: applications and optional adapters depend on the
  core; the core does not depend on the bundled application or discover consumer
  code. Importing the core should not require Rerun, mission files, or a checkout.
- The application owns composition. It chooses stock or custom models, supplies
  configuration, and constructs the simulation using ordinary Rust values.
  A new research model should not require a core feature flag or stock CLI edit.
- Keep application-specific model state, schemas, datasets, evaluation, and
  visualization with the application. Exchange typed observations, commands,
  snapshots, and diagnostics at small public boundaries.
- Make the bundled runner an ordinary consumer of that same API. It should not
  have a privileged construction path unavailable to downstream applications.
- Test from a consumer's perspective: public imports, consumer-owned models and
  configuration, complete steps, outputs, and shutdown. Existing
  [plugin contract tests](../crates/core/tests/plugin_contracts.rs) protect today's
  extension API; a later downstream composition test must also run without XML
  or repository-relative defaults.

## Keep the simulation boundary simple

Expose a complete validated simulation step, not an application-maintained
sequence of plugin calls. Keep shared entity IDs, kinematics, coordinates, and
time contracts concrete; private model state and typed observations do not
require making the whole simulator generic over every application type.

Independence from applications, file formats, and viewers matters more than a
zero-dependency library. Shared math and a contained worker implementation can
remain where useful. Use the smallest crate split that enforces those boundaries.

Do not add a generic simulation-backend interface without a second real backend.
Component composition is the current use case. Do not clone entire worlds to
promise transactional or retryable steps: plugins may publish messages or call
external services. Specify failure, partial progress, and cleanup instead.

A top-level application should control when it asks the simulation to advance.
It should not have to know that every controller substep precedes every motion
substep, or where message delivery and removal occur. Those are engine
invariants, not boilerplate for each consumer to reconstruct.

## Proposed ownership boundary

These are logical responsibilities, not a commitment to create one crate per row.

| Owner | Responsibilities |
| --- | --- |
| Simulation core | Simulation time, entity identity and lifecycle, validated connections, per-phase scheduling, generation/removal, message delivery timing, observations, deterministic random-stream ownership, and explicit stop/failure behavior. |
| User application | Select model types, construct typed configuration, assemble entity stacks and world systems, select inputs/outputs, control wall-clock pacing, and call the core. Own application data and integrations. |
| Reusable model catalog | Stock autonomy, controller, motion, sensor, interaction, network, and metrics implementations. Consumers choose these or their own implementations without changing the core. |
| Compatibility adapters | Parse SCRIMMAGE XML and resolve named defaults into validated runtime configuration; translate legacy protobuf frames and other external formats. Keep compatibility policy out of physical model equations. |
| Output adapters | Convert snapshots, events, metrics, and diagnostic geometry into Rerun or another output. The core must not require a viewer, file tree, or recording destination. |

Both the bundled application and a downstream application depend on the core.
Models and adapters depend on core contracts, never the reverse. The bundled
application assembles the stock catalog and XML adapter; a downstream app can
choose them individually or construct its simulation directly.

The existing seven categories remain useful vocabulary. A sensor still senses,
a controller still controls, and a network still models delivery. Removing
plugin-loading ceremony does not mean removing these distinctions or their
simulation semantics.

## Intended authoring and running flow

1. Create a normal Rust application with a dependency on `scrimmage-core`.
2. Write small model structs in that application's modules, implementing the
   relevant category contracts. Use stock models where suitable.
3. Construct typed configurations and assemble the entity/world setup at the
   top level. Supply constructors for independently owned instances when
   entities spawn; do not accidentally share mutable model state.
4. Validate the setup once, including ports, timing, required services, and
   configuration constraints, before running.
5. Ask the core to advance complete simulation steps and consume its outputs.
   Choose recording, Rerun, pacing, and output directories in the application.

No XML, string-name registry, global environment variables, or sibling source
checkout should be mandatory for that direct Rust path. Applications that want
the old XML workflow can opt into the compatibility adapter and provide a
name-to-constructor catalog. A registry remains useful at that boundary; it
should not be compulsory ceremony for typed composition.

Generic shared-library plugin discovery/loading is explicitly out of scope.
Downstream applications compile their own implementations and optional adapters.
JSBSim is also excluded from production: use it only for offline reference
fixtures for selected Rust flight models, without an FFI dependency in the core.

The API should make the simplest sensor or motion model ordinary Rust: a
configuration, owned state, and an update method. Keep heterogeneous dispatch,
factory erasure, worker joins, locks, and scheduling rules behind the core's
implementation boundary. Avoid replacing C++ registration boilerplate with a
large generic builder or a new configuration language.

## Done so far: the runner is a library

The command line is `scrimmage_cli::main(registry, project_root)`. The
`scrimmage` binary passes it the stock plugins plus the user crates in this
workspace ([USER_PROJECTS.md](USER_PROJECTS.md)), so user plugins reach run,
sweep, and compare with no privileged path for stock plugins. A separate
application could make the same call; that and typed assembly without a
mission file are the remaining work below.

## What still couples today's core to the application

We already have `scrimmage-core`, and Rerun lives in the CLI. That is a useful
starting point, not a finished library-first boundary:

- `ScenarioConfig` includes XML-derived parameters and source information;
  `Simulation` retains it. Core construction currently goes through mission
  resolution and plugin compilation rather than a separate typed assembly API.
- `Plugin::configure` consumes string-keyed parameters. Keep this as a legacy
  conversion boundary later, rather than requiring user applications to turn
  typed values into strings and parse them back.
- The core currently bundles the stock plugin catalog, XML parser, protobuf
  frame I/O, and CSV summary formatting. Decouple these responsibilities
  incrementally once their behavior is covered by parity tests.
- Bundled XML lookup uses the supplied repository root. A reusable core must
  not depend on this checkout's layout or on `../scrimmage` at runtime.
- Selected lifecycle semantics, truth versus belief, error behavior, and services
  required by selected consumers still need implementation work. Do not freeze
  a supposedly stable application API before these contracts are understood.

The later step/output boundary must distinguish simulation time from legacy
log labels, including repeated terminal timestamps. Prefer streamed outputs
and caller-owned history over unconditional full-run retention. Decide whether
outputs borrow state or own snapshots based on real recording and concurrent
consumer needs; do not hide the lifetime or copying cost.

Rerun should receive data-only snapshots and diagnostics. A slow or absent
viewer must not change simulation results. Pacing, pause/step requests, and
run-directory numbering belong above the physical simulation.

## Migration and acceptance criteria

After the selected behavior/contracts are covered and the refactor is started:

1. Define a format-independent validated runtime setup and step/output contract
   using existing behavior as the baseline. Preserve the old XML entry point
   through an adapter during the transition.
2. Make the bundled `scrimmage` application consume that same boundary. It must
   not retain a privileged route into simulator internals.
3. Build one realistic downstream application outside this workspace. It must
   supply its own models and typed configuration, spawn independent instances,
   and run without XML, source-tree paths, registry edits, or core changes.
4. Isolate stock models and optional format/viewer integrations where dependency
   boundaries require it. Use the smallest useful crate split, not a crate for
   every category.
5. Compare the old compatibility path and the new application path on identical
   scenarios, seeds, and worker counts. Preserve frame/event/metric behavior,
   temporal semantics, cleanup, and viewer-on/off equality.
6. Test invalid configuration, plugin failures and panics, removal, shutdown,
   and multiple independent simulations. Do not assume rollback or retry is safe.
7. Measure startup, allocation, memory growth, and multithreading performance
   on representative workloads. Then document and stabilize the user-facing API.

The refactor succeeds when downstream developers can own their application
without learning the engine's internals, while the compatibility application
still reproduces the SCRIMMAGE behavior we ported. Moving files into more
crates, or demonstrating another small example, would not establish that alone.

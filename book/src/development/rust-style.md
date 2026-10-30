# Rust style

Standard Rust conventions, `rustfmt`, and Clippy are the baseline. The rules
below keep simulation and behavior code readable in physical and algorithmic
order by an engineer who is not a Rust specialist.

## Rust choices

Use structs and ordinary functions by default. Use an enum when the cases are
known so `match` forces every case to be handled. A sensor kind, run status, or
estimator mode should not be a string. Add a trait only when multiple
implementations need the same interface.
[ripgrep](https://github.com/BurntSushi/ripgrep) and
[Serde](https://github.com/serde-rs/serde) show the difference between a closed
enum and an interface meant for outside implementations.

Rust often makes unclear ownership show up as a compiler error. Fix the
ownership boundary instead of reaching for `.clone()`, `Arc`, or
`Rc<RefCell<_>>`. In this batch pipeline, scenarios and histories are usually
borrowed while a stateful component owns its mutable state. See
[Reqwest](https://github.com/seanmonstar/reqwest) for a larger example of
ownership distinguishing reusable state from one-shot data.

Use `pub` for an actual crate API, not to make a compiler error or integration
test go away. Unit tests beside a module can test its private code.
[fd](https://github.com/sharkdp/fd),
[rust-analyzer](https://github.com/rust-lang/rust-analyzer), and
[uutils](https://github.com/uutils/coreutils) keep most implementation modules
private behind a small set of operations.

## Parse into stronger types

Serde types describe what can appear in a file. They do not prove that rates are
positive, IDs are unique, or required combinations are present. Build or parse a
`ScenarioConfig`; `Simulation::new` validates it and constructs internal
resolved definitions before execution. This input-to-runtime boundary is used
clearly by [fd](https://github.com/sharkdp/fd),
[rust-analyzer](https://github.com/rust-lang/rust-analyzer), and
[Serde](https://github.com/serde-rs/serde).

Generated Protobuf fields may be `Option` even when the simulator requires
them. Validate decoded CSV, MCAP, and future ROS data once, then pass the core a
type whose timestamp and measurement are present. As in
[Axum](https://github.com/tokio-rs/axum), missing input and invalid input are
different results.

Keep YAML strict with `deny_unknown_fields`. A Serde default means a field may
intentionally be omitted, not that an old format still needs to work.

Use `Result::Err` with an explanation for an operation that could not be
completed, including numerical failures. An interaction or metrics step
answers `Update::Applied` or `Update::Stop` for successful completion or a
normal stop request. The command
can use `anyhow`; it does not need a custom error framework. This split between
operational failure and a completed result is visible in
[Cargo](https://github.com/rust-lang/cargo),
[Reqwest](https://github.com/seanmonstar/reqwest), and
[uutils](https://github.com/uutils/coreutils).

## Optimize for an engineering review

The most important code should make these questions easy to answer:

- What physical quantity is this?
- Which frame is it expressed in?
- What unit does it use?
- Is it measured, predicted, corrected, or hidden truth?
- Which stage of the sensor or estimator produced it?

Prefer a few explicit intermediate values over a compact iterator chain or a
deeply nested expression when the intermediate values answer those questions.

## Familiar plugin authoring

Use the same reading order in every concrete plugin module:

1. Module documentation: purpose, the reference it follows (or Rust-only),
   execution phase, inputs, and outputs.
2. Explicit imports, grouped as standard library, dependencies, then crate
   imports.
3. Port/event constants, named configuration, and state/measurement types.
4. The plugin struct, which owns its per-instance state.
5. The category implementation, build half first: `Config`, `configure`
   when it validates, `new`, and the load-time facts (`ports`;
   `reading_kind` for sensors; `inputs` for estimators).
6. Then its per-tick half: optional initialization, step, reporting, and an
   optional close.
7. Private model/conversion helpers, followed by module tests.

Separate structs, implementation blocks, and functions with a blank line. Keep
all imports at the top except test-only imports inside the test module. Prefer
explicit imports over wildcard imports in concrete plugin implementations. Keep
the existing subsystem directories; do not split declarations and
implementations into artificial header/source pairs or add namespace wrappers.

Use descriptive module filenames rather than `mod.rs`. Core modules use the
standard sibling pattern: `simulation.rs` declares helpers in `simulation/`.
Only keep a supporting directory when it contains files. Plugins are the
intentional exception: keep `<plugin>.rs`, and any private submodules, inside
the plugin's own folder. The named category module uses `#[path]` to load that
implementation, preserving the existing logical module name and public imports.
Keep those path declarations at the module-wiring boundary, not in model code.
Test support also uses named files under `tests/support/` to avoid accidental
Cargo integration-test targets. See [Source map](source-layout.md).

Prefer named configurations even for a single physical parameter, such as
`StraightConfig { speed_mps }`. Use `()` when there really is no configuration.
Missions give named fields, not positional arrays: a PID is
`{kp, ki, kd, integrate_within_*}`, not `[P, I, D, band]`. Preserve mission
keys, defaults, units, and validation behavior during readability work.

Use imperative loops and early returns for algorithms with branching or state
changes. Keep short iterator expressions when they directly express the intent,
such as testing whether all integration coordinates are finite. Do not rewrite
every iterator, build a fluent API for ordinary equations, or extract helpers
that only hide a trivial expression. Name intermediate physical quantities and
keep input, calculation, and output stages visually distinct.

Keep `Result`, `?`, `Option`, and exhaustive enums/matches. They make failures
and state transitions explicit. Use domain traits for the seven extension
points; keep generic dispatch, erased adapters, thread management, and locks in
the framework. Do not add clones or shared ownership just to silence borrow
errors. Configuration copies used to construct independent plugin instances are
different from sharing mutable plugin state.

Type aliases should name a useful concept, not conceal ownership. Keep `&` and
`&mut` visible at call boundaries and use inferred lifetimes where appropriate.
Do not tie an outer borrow and a context's internal lifetime to the same named
lifetime merely to shorten a signature. A plain `&mut SensorContext`, its
lifetime elided, is preferable to an alias that obscures this distinction.

Use standard rustfmt and Clippy; do not introduce nightly formatting options or
globally compress code onto longer lines. Readability refactors must preserve
floating-point operation order, RNG consumption, event order, and phase
barriers. Compare before/after mission outputs, not just whether both versions
compile.

## Names carry frames and units

- Put the frame before the unit: `position_world_m`, `velocity_body_mps`, and
  `yaw_world_from_body_rad`.
- Use unit suffixes on physical scalars, including private fields and function
  arguments: `_m`, `_mps`, `_mps2`, `_rad`, `_radps`, `_s`, and `_ns`.
- Use `world` and `body` rather than a bare `w` or `b` in public interfaces.
- Short names such as `dt_s`, `dx_m`, and `dy_m` are appropriate inside a small
  equation function when their meaning is established immediately.
- Do not use Unicode identifiers. ASCII names are easier to type, search, and
  discuss.

## Parameter names

A plugin parameter has one name: the config struct's field is the mission key,
so a plugin has no `#[serde(rename)]`. The name is written for the person
writing the mission, and should also be recognizable to someone working from
a textbook (Beard and McLain, *Small Unmanned Aircraft*; Stevens, Lewis, and
Johnson, *Aircraft Control and Simulation*; Nelson, *Flight Stability and
Automatic Control*).

- **Simple units in the name, compound units in the docs.** `mass_kg`,
  `max_roll_deg`, `range_m`, `delay_s`, `loop_rate_hz`. A compound unit stays
  out of the key (`thrust_coefficient` is N/(rad/s)², `velocity_kp` is 1/s),
  and so does a pure number: a probability, fraction, or coefficient.
- **Axis words, textbook subscripts.** The textbooks name a coefficient by a
  letter and a subscript: C_L is lift and C_ℓ the rolling moment, told apart
  only by case and typeface, which a lower-case key loses (`c_l_p` could be
  either). So the letter becomes a word, `lift`, `drag`, `side`, `roll`,
  `pitch`, or `yaw`, and the subscript stays as the books write it: `0`,
  `alpha`, `beta`, the body rates `p`, `q`, `r`, or a control surface spelled
  out. `c_roll_p` is C_ℓp (roll damping), `c_pitch_q` is C_mq, `c_lift_alpha`
  is C_Lα, and `c_yaw_rudder` is C_nδr. The derivatives are per radian, as in
  the books.
- **Gains are `kp`, `ki`, `kd`.** A PID is a named map,
  `altitude_pid: {kp: 0.1, ki: 0.0001, kd: 0.002, integrate_within_m: 10}`,
  never a positional list. The integral accumulates only while the error is
  within `integrate_within`, whose suffix is the loop's unit (`_deg`, `_m`,
  `_mps`); `AngleGains`, `LengthGains`, and `SpeedGains` parse the three.
- **Body vectors name their frame; world vectors do not.** The world frame is
  always east, north, up, so mission keys leave it out: `position_m`,
  `velocity_mps`, `wind_mps: [east, north, up]`. Body axes vary (FixedWing6DOF
  and its landing gear use forward/right/down, Multirotor forward/left/up), so
  a body vector says which: `position_frd_m`, `position_flu_m`. Inside the
  code, `_world_` still marks world quantities, as in the section above.
- **Only universal abbreviations:** `alpha`, `beta`, `p`, `q`, `r`, `kp`,
  `ki`, `kd`, `enu`, `frd`, `flu`, and the unit suffixes. Everything else is
  spelled out, and a name says what it belongs to: `wing_area_m2`, not
  `area_m2`.
- **One name per quantity across plugins.** `mass_kg` and `gravity_mps2` mean
  the same in every vehicle; `network` names a network everywhere; `min_` and
  `max_` lead a limit (`max_thrust_n`, `min_elevator_rad`). A setting that
  turns a feature on is `enable_<feature>`; one that says what the thing is or
  does reads as a description (`repeat`, `auctioneer`, `castered`).
- **An option a plugin does not implement is not a field.**
  `deny_unknown_fields` already rejects it by name.

A plugin's parameter table in this book gives each key with its unit, a plain
description, and, for a textbook quantity, its symbol and source.

## Named physical state, matrix coordinates

Represent the nominal physical state with a struct whose fields have names.
Do not access position, attitude, speed, or bias through unexplained vector
indices.

Fixed-size `nalgebra` vectors and matrices are appropriate for covariance,
Jacobians, gains, and state corrections. Define the matrix-coordinate order
once with a named enum and use those indices when constructing matrices. Do not
describe the baseline as an error-state filter unless it actually adopts an
error-state formulation.

```rust
struct PlanarState {
    position_world_m: Vector2<f64>,
    yaw_world_from_body_rad: f64,
    forward_speed_mps: f64,
    gyro_bias_radps: f64,
    accel_bias_mps2: f64,
}
```

Use named structs for groups of physically different values. In particular, do
not return geometry as a tuple of anonymous `f64` values.

## Write estimator operations in stages

An IMU propagation function should read in this order:

```text
measurement and elapsed time
-> bias correction
-> nominal-state propagation
-> state-transition and process-noise construction
-> covariance propagation
```

A measurement update should read in this order:

```text
predict measurement
-> observed minus predicted residual
-> measurement Jacobian and variance
-> innovation uncertainty and Kalman gain
-> state and covariance correction
```

Keep orchestration, physical models, stochastic effects, and serialization out
of these equation-level functions.

## Connect notation to meaning

Use descriptive names with a conventional estimation suffix when it helps an
engineer compare code with a reference:

```rust
let state_transition_f = ...;
let process_noise_q = ...;
let measurement_jacobian_h = ...;
let innovation_variance_s = ...;
let kalman_gain_k = ...;
```

This retains recognizable `F`, `Q`, `H`, `S`, and `K` without requiring the
reader to remember a single-letter variable. Public interfaces always use
descriptive names.

Comments should explain the physical model, convention, approximation, or
reason for a step. They should not merely translate the following Rust line
into English. A compact equation in a comment is useful only after the plain
meaning is established.

## Sensor models have explicit boundaries

Keep these stages separate, even when they live in the same sensor module:

```text
trajectory sample
-> ideal geometry or ideal inertial signal
-> field of view and detection
-> bias, noise, saturation, and quantization
-> timing and delivery metadata
```

Random draws use stable engineering identities rather than collection order.
Hidden ideal values and applied effects remain available for diagnosis, but
estimator code must only receive the visible measurement.

## Runtime and dependency boundary

Estimator propagation and update functions are deterministic and free of file
I/O, visualization, wall-clock access, and random draws. Configuration parsing,
MCAP handling, command-line behavior, and Rerun output stay at the boundary.

Prefer the existing fixed-size `nalgebra` types over adding an estimator DSL or
unit framework. Add stronger unit or frame wrapper types only where repeated
mistakes show that naming and tests are insufficient.

## Tests express engineering invariants

Put equation tests in the same module under `#[cfg(test)]` so private math does
not become public for testing. Use integration tests for the public workflow.
This is the same split used by
[Cargo](https://github.com/rust-lang/cargo) and
[ripgrep](https://github.com/BurntSushi/ripgrep).

Equation changes should include the smallest applicable checks:

- an analytically solvable or zero-effect case;
- a convention test for frames, signs, quaternion ordering, or angle wrapping;
- a deterministic fixed-seed sensor case;
- a covariance symmetry and nonnegative-diagonal check; and
- an end-to-end check that estimator-visible inputs remain separated from
  hidden truth.

Name tests after the physical behavior they protect. Use tolerances with an
explicit scale rather than exact floating-point equality unless equality is the
property under test.

# Verifying the engine

Parts of the engine carry proofs checked by [Verus](https://verus-lang.github.io/verus/guide/),
a verifier for Rust. A proved function keeps its plain Rust body; one
attribute above it names the claim, and the claim is spelled out in a
`proof.rs` file beside the module. The plan, what is proved so far, and
every assumption the proofs rest on are in
[docs/FORMAL_VERIFICATION.md](https://github.com/EthanMBoos/ripple/blob/main/docs/FORMAL_VERIFICATION.md).
Plain `cargo build`, `cargo test`, `cargo clippy`, and `cargo fmt` never see
the proofs: Verus erases them, and the binary is unchanged.

## Install

Download the release named in `docs/FORMAL_VERIFICATION.md` (0.2026.11.02)
for your platform from the
[Verus releases](https://github.com/verus-lang/verus/releases), unzip it,
and put the folder on your `PATH`; it holds `verus`, `cargo-verus`, and its
own `z3`. On macOS, run the folder's `macos_allow_gatekeeper.sh` once.
Install the Rust toolchain the release pins:

```sh
rustup toolchain install 1.99.1
```

## Check the proofs

From the repository root:

```sh
cargo verus verify -p ripple-core
python3 reference/verus.py
```

The first command proves every contract in the core crate; the first run
also verifies Verus's standard library and builds the crate's dependencies
with the pinned toolchain, which takes a minute, and later runs take
seconds. The second command does the same, then plants each fault it lists
in the production source, one at a time, and requires the proof to fail;
a fault the proof survives means a claim is too weak. It writes a record to
`runs/verusNNN/` and, with `--retain`, to `paper/data/verification/`. It
edits source files in place and restores them, so do not build, run
`reference/equivalence.py`, or commit while it runs.

## Read a proved function

`Rate::due` in `crates/core/src/common/loop_rate.rs` is the pattern:

- `#[verus_spec(ran => ensures proof::stepped(old(self), final(self), dt_s, ran))]`
  is the claim Verus proves the body against. `ran` names the return value,
  `ensures` states what holds after the call, and `old(self)` and
  `final(self)` are the value before and after. A `requires` clause, when
  present, is what the function demands of its caller.
- `proof! { broadcast use proof::rate_facts; }` as the body's first line
  tells Verus which library facts and assumptions this proof may use. A
  module with several proved functions, or with a loop, says it once
  instead in a three-line `verus! { }` block after `mod proof;`, as
  `ports.rs` does.
- `loop_rate/proof.rs` holds the claim as `spec fn`s with doc comments in
  plain words, a one-line declaration that lets Verus read the struct's
  fields, and an `axioms` module whose items each begin with a `Trusted:`
  comment saying why the assumption is true.

## Write a proof

- Keep the body plain Rust. Verus rejects a few shapes (a closure that
  captures `&mut`, a let chain, `ensure!`); the plan's table says what each
  becomes. The replacement must read as well as what it replaces, and
  `reference/equivalence.py` must show byte-identical outputs.
- Put every predicate and lemma in `<module>/proof.rs`. The production file
  gets `mod proof;`, the `broadcast use` block, the attribute on each proved
  function, and one invariant line on each loop, nothing more. Facts every
  module needs, strings as map keys and the `anyhow` error path, are in
  `crates/core/src/proof.rs`.
- Declare the struct from `proof.rs` with
  `#[verifier::external_type_specification]` rather than wrapping its
  definition in a block. Verus reads the fields only when they are exactly
  as visible as the struct, so a `pub(crate)` struct keeps `pub(crate)`
  fields and a `pub` struct such as `PluginIo` gets `pub` ones, with a doc
  line saying callers go through the methods. The contracts stay on the
  public methods, so a plugin proof can use them later.
- Give a loop its invariant on the loop itself:
  `#[cfg_attr(verus_only, verus_spec(it => invariant proof::pred(.., it.seq(), it.index())))]`,
  where `it.seq()` is what the loop walks and `it.index()` how far it has
  got. The `cfg_attr` keeps stable `rustc` from seeing an attribute on a
  loop, which it rejects.
- Code Verus cannot read (anything touching `dyn Any`) goes in a small
  module of its own, with `#[cfg_attr(verus_only, verus_verify(external))]`
  on its `mod` line so Verus skips it, and `proof.rs` states what its
  methods return as trusted contracts. `plugin/sensor/readings/any_reading.rs`
  is the pattern.
- Mark `use` lines that only proofs need with `#[cfg(verus_only)]`, since
  the items they import do not exist in a plain build.
- Add each new assumption to the `axioms` module with a `Trusted:` comment
  and to the plan's trusted-items table, and add a fault to
  `reference/verus.py` for each new claim.

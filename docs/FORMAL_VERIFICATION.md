# Checking Ripple's engine

The goal is a proved tick: one contract on `Simulation::step` that says the
phases ran in order, each once on every active entity, with each plugin
lent exactly its declared state and no more, built by composing the
contracts of the functions the tick calls. This document is the plan for
getting there with [Verus](https://verus-lang.github.io/verus/guide/): what
the engine's code needs from it, the patterns other projects use, and the
steps from the leaf functions up to the tick. No Ripple code is formally
verified on `main`. An earlier attempt with Creusot is on the
`creusot-verification` branch; nothing below assumes its refactors.

Readability comes first, through all of this. The engine as it stands is
not perfect, but it is plain to read, and a student should be able to open
any file in it and follow the code without knowing Verus exists. A proof
earns its place only if the code stays that way or gets plainer. So when
Verus forces a rewrite, the rewrite must read as well as or better than what
it replaces, and proof text stays out of the way of the reader who is not
looking for it. Where this work can also make the code clearer, it should.

Prove the code that missions run. A separate model of the engine does not
establish a claim about Ripple. "The prover requires it" alone justifies no
change to production code: say so, and change the code only when the new
shape is also the better Rust. Where Verus does force a change, the table
below names it and its cost, and mission outputs must stay byte-identical
(`reference/equivalence.py OLD NEW`).

Everything stated about Verus here was tried against release
0.2026.11.02 with the `vstd` crate `0.0.0-2026-11-01-0251` in scratch files
shaped like the engine's code, on 2026-11-14. Verus moves quickly (a
rolling release followed three days later and its main branch already
specifies more of the standard library), so recheck a "not supported" row
before working around it.

## What we are working to

The end claim is about one tick: `Simulation::step` in `simulation.rs`,
which runs the phases in the order the execution invariants state
(generate, autonomy, controllers then motion per substep, sensors,
interactions, routing, estimation, metrics, removal and time). Each phase is
a loop over the active entities that calls one entity-level function, which
calls the plugin stack, which calls the leaf functions this plan proves
first: the rate countdown, the mailbox, the port copy, the readings slots,
the routing loops.

The proofs compose upward because Verus is modular: a function is proved
from its callees' contracts alone, never their bodies. So each leaf function
gets a contract naming a predicate over its own state; each entity-level
phase function gets a contract saying what the phase did to that entity and
nothing else; the tick's contract is the conjunction of its phase contracts
in order, stated over the simulation's state at entry and exit. A reader
checks `step` by reading its one contract and the predicates it names, and
trusts the rest to the prover.

Two things stay outside the proof and are declared so. Plugins are called
through traits with no stated behavior, so a tick's claims are about what
the engine does around them: what each plugin was lent, when it ran, and
what reached it. The worker scheduler that runs a phase's entity loop in
parallel is trusted with one contract, that the update ran once on every
active entity on disjoint state, so the tick's claims hold at one worker
and at eight. Everything else between a leaf and `step` is proved.

## Behaviors worth checking

The ranking is about value to someone using Ripple, not proof difficulty.
Paths are relative to `crates/core/src/`.

| Rank | Engine behavior | Code |
| --- | --- | --- |
| 1 | Autonomy and controllers see belief; sensors and motion get their declared truth access. Estimators see only their selected readings. | `plugin.rs`, `entity/plugin_stack.rs` |
| 2 | Each phase finishes before the next. Controllers precede motion; sensing follows motion; localization precedes tracking. An error stops later phases after launched work joins. | `simulation.rs`, `simulation/scheduler.rs` |
| 3 | Publications reach only subscribers of the right network, topic, and type. Each subscriber has its own queue. | `pubsub/messages.rs` |
| 4 | Delayed messages arrive at the first routing phase at or after their due time, even if the publisher is silent. Removed receivers get nothing. | `pubsub/messages.rs` |
| 5 | Spawns receive unique IDs, and removal never reuses an old ID. | `simulation/generation.rs` |
| 6 | Missing or mismatched ports and ambiguous estimator sources fail when the mission loads. | `ports.rs`, `entity/plugin_stack.rs` |
| 7 | Sensor readings reach only their bound estimator inputs; `fresh` and `latest` keep their stated meanings. | `plugin/sensor/readings.rs`, `entity/plugin_stack.rs` |
| 8 | Plugins initialize, step, and close in order. Rate-limited steps receive the right elapsed time. | `entity.rs`, `entity/plugin_stack.rs` |
| 9 | A keyed random draw depends on its seed, purpose, tick, and IDs, not unrelated draws. | `common/random.rs`, `common/plugin_random.rs` |
| 10 | Removed entities receive no further steps or messages; removal is recorded at the stated point in the tick. | `simulation.rs`, `entity.rs`, `pubsub/messages.rs` |

## How Verus works

Verus is a verifier for Rust that sends each function to Z3 and reports per
function. By default it verifies what sits inside a `verus! { }` block and
ignores everything outside, which stays ordinary Rust. Three kinds of
function live in a block:

- `fn` (exec): the compiled code, with `requires` and `ensures` clauses in
  its signature.
- `spec fn`: a pure mathematical function the clauses are written with. It
  never runs. `pub open spec fn` shows its body to callers, `pub closed spec
  fn` hides it, and `pub open(crate) spec fn` shows it inside the crate
  only, which is what a predicate over `pub(crate)` fields needs.
- `proof fn`: a lemma, with its own `requires`/`ensures`, called from a
  `proof { }` block inside exec code when the solver needs a hint. A
  `broadcast proof fn` is a lemma the solver may apply on its own wherever
  a module says `broadcast use`; a `broadcast group` names a set of them.

A contract can also be attached to a plain Rust function outside any block
with `#[verus_spec(ret => requires ..., ensures ...)]`, and hints with
`proof! { }` inside the body. The guide recommends this form "when
developers want to integrate verification into an existing Rust project"
and keep "native Rust syntax for maintainability"; its one requirement is
that the types the function touches are known to Verus, either defined in a
block or declared with an `external_type_specification`.

Every spec and proof item is erased before compilation, so `cargo build`,
`cargo test`, `cargo clippy`, and `cargo fmt --check` see plain Rust and
the binary is unchanged; the probe crate passed all four with blocks and
attribute contracts in place. The crate takes a `vstd` dependency (the Verus
standard library, with specifications for `Vec`, `Option`, `Result`,
`String`, `HashMap`, `BTreeMap`, `BTreeSet`, `VecDeque`, `Box`, `Arc`,
slices, ranges, and the common iterator adaptors) and opts in with
`[package.metadata.verus] verify = true`; `cargo verus verify -p
ripple-core` proves it. A `use` of a spec-only item is guarded with
`#[cfg(verus_only)]`, which Verus sets during verification and nothing else
sets; the guide warns against using it for anything but `use` lines and
configuration attributes, because conditional compilation of real code
would let a proof pass on code that never ships. Verus pins its own Rust
toolchain (this release wants 1.99.1, installed with `rustup toolchain
install 1.99.1-aarch64-apple-darwin`) and ships Z3; there are no proof
session files to commit. The first `cargo verus verify` also verifies
`vstd` itself (about half a minute, cached afterwards); a module the size of
the mailbox reverifies in a quarter of a second.

The notation a reader meets in a contract:

| Written | Meaning |
| --- | --- |
| `old(x)`, `*old(x)` | a `&mut` argument's value at entry |
| `final(x)`, `*final(x)` | its value when the borrow ends (only in `ensures`) |
| `v@` | the mathematical view of `v`: `Seq<T>` for a `Vec`, `Map<K, V>` for a `BTreeMap`, `Set<K>` for a `BTreeSet`, `Seq<char>` for a `String` |
| `v@[i]`, `v@.len()`, `m@.remove(k)`, `m@.contains_key(k)` | sequence and map operations |
| `forall\|i: int\| 0 <= i < n ==> p(i)` | quantifier with the `int` type, which never overflows |
| `#[trigger] f(i)` | which term the solver matches to instantiate the quantifier |
| `a =~= b` | extensional equality of sequences or maps |
| `x is Some`, `x->0`, `r is Ok`, `r->Ok_0` | matching on an `Option` or `Result` and reading its payload |
| `&&& a &&& b` | a conjunction written one line per fact |
| `assert(p) by { lemma(); }` | a fact proved locally with a hint |
| `assume_specification [path](args) ensures ...;` | a contract taken on faith for a function Verus cannot see |
| `#[verifier::external_type_specification] struct ExT(T);` | makes a type defined elsewhere known; with `external_body` its fields stay hidden |
| `#[verifier::external_trait_specification] trait ExT { type ExternalTraitSpecificationFor: T; ... }` | makes a trait known, with contracts on the methods listed |

Loops take `invariant` clauses and a `decreases` measure. A loop is checked
apart from its function, so facts from the function's `requires` must be
restated as invariants unless the function sets
`#[verifier::loop_isolation(false)]`. A `for` loop over `v.iter()`, a
range, or `map.iter()` works as is and may name its iterator for
invariants. A function whose proof is slow or flaky gets
`#[verifier::rlimit(n)]` or `#[verifier::spinoff_prover]`; `--expand-errors`
splits a failing clause into the part that fails; `--profile` finds the
quantifier that explodes. The guide's checklist for a failing proof is
[here](https://verus-lang.github.io/verus/guide/checklist.html).

`verusfmt` (`cargo install verusfmt --locked`) formats the inside of
`verus!` blocks; `rustfmt` leaves them alone, so `cargo fmt --check` keeps
passing.

## What the engine's code needs from Verus

The functions behind the table above were inventoried for the standard
library calls and Rust features they use, and each was tried. The first
group needs nothing. The second needs a trusted one-line contract for a
library function `vstd` has not specified, which is an assumption to list
but not a code change. The third needs the production code to change, and
says how much.

| Construct | Where | Verus 0.2026.11.02 | Cost |
| --- | --- | --- | --- |
| `BTreeMap`/`BTreeSet` `get`, `get_mut`, `insert`, `remove`, `contains_key`, `len`, `iter`, `keys`, `values`; set `insert`, `contains`, `clear` | messages, ports, readings | Specified. Contracts hold once the key type is known to obey the ordering laws: integers, tuples, references, `Option` are built in; `String` is not. | One trusted axiom that `String` ordering is a total order; proved `remove` on `BTreeMap<(String, String), Vec<_>>` and `get`, `get_mut` on `BTreeMap<String, f64>` with it. A lookup by `&str` needs two more axioms and more proof work. |
| `for` over `.iter()`, a range, `map.iter()`; let-else; `match` on tuples with `continue` | everywhere | Supported. `for x in &map` is not. | None. |
| Iterator `map`, `filter`, `any`, `find`, `zip`, `rev`, `collect` into `Vec` or `Result<Vec<_>>` | receive, ports, plugin stack | Specified. `position` needs a hint; `enumerate` is not in this release's `vstd` (it is on main). | None, or an index loop where `enumerate` is used (messages 2, plugin stack 3). |
| `?`, `unwrap_or_default`, `map`, `and_then`, `ok_or_else`, `copied`, `String == &str`, `format!`, `Arc::clone`, `Arc<[T]>` | everywhere | Supported. | None. |
| `Box<dyn Autonomy>` and the other plugin traits | plugin stack | Supported for a trait with one bound, through an external trait specification in the proof file; implementations in this crate and in plugin crates stay unverified. Verified with the stack's exact `Slot<T: ?Sized>` and `AgentContext<'_>` shapes. | None. |
| A context holding `&mut Messages` and shared references | plugin.rs | Supported; the trusted call's contract names `*final(old(ctx).messages)`. | None. |
| `TypeId` | messages | Unknown type. | One trusted type declaration and one-line contracts for `TypeId::of` and `==`. |
| `#[derive]` of `Clone`, `Debug`, `Default`, `PartialEq`, `Eq`, `PartialOrd`, `Ord` on a struct in a block | all | Accepted (the derived `Clone` gets no contract). | None. |
| `Option::map_or`, `as_deref_mut`, `f64::is_finite`, `mem::take`, `BTreeMap::append`, `Vec::retain`, `Vec::extend`, `sort_by_key` | messages, readings, simulation | Not specified. | One trusted `assume_specification` each; `is_finite` and `map_or` tried. |
| `Vec::drain`, `BTreeMap::entry` with `or_default`/`or_insert`, `BTreeMap::iter_mut` | route_links (2 drains, 4 entries), ports `receive` | Not specified, and their std signatures carry the unstable allocator parameter. | A trusted contract is writable once the crate carries `#![cfg_attr(verus_only, feature(allocator_api))]`, which plain builds never see (tried); or `drain(..)` becomes `std::mem::take(..).into_iter()` (tried). |
| `anyhow!`, `bail!`, `with_context`, `Error::msg` | everywhere | Unknown functions. | Four trusted one-line contracts over `anyhow::Error` declared opaque; all four forms then verify. |
| `ensure!` | 16 sites in the target files: rate 1, ports 3, readings 2, messages 9 (receive 1, route_links 4) | Its expansion calls a private trait of `anyhow`, which no contract can name. | `if !c { bail!(...) }` at the proved sites only. |
| A closure that captures `&mut`: `due.then(\|\| std::mem::take(&mut self.elapsed_s))` | loop_rate.rs | Rejected: "closures capturing a mutable reference". | `if due { let e = self.elapsed_s; self.elapsed_s = 0.0; Some(e) } else { None }`; done in step 2. |
| Let chains (`if let Some(t) = x && c`) | route_links 1, plugin stack 2 | Rejected: "let expressions in conditions", though the crate's 2024 edition allows them. | A nested `if`. |
| `Arc<dyn Any + Send + Sync>`, `Box<dyn Any + Send + Sync>`, and `fn(&Payload) -> Result<Value>` fields | messages (`Payload`, `PayloadSerializer`), readings (`values`) | Rejected: "dyn with more than one trait" and "function pointer types". No open issue tracks the first; the check is a hard `preds.len() != 1` in the translator. | A one-line newtype around each, opaque to Verus, whose two or three methods (`downcast`, `type_id`, the serializer call) are trusted; or the whole struct opaque when the proof reads none of its fields (a `Vec<Envelope>` of opaque values moves and proves fine). |
| Private fields a proof reads | Messages, Slot, PluginStack, Readings, Rate, PluginIo | A struct outside any block must have `pub` fields to be declared with fields; inside a block, `pub(crate)` fields and `pub open(crate)` predicates verify. | Mark the fields a predicate reads `pub` and declare the struct from the proof file with a one-line `external_type_specification`. On a `pub(crate)` struct, `pub` fields stay crate-private, so nothing widens and the production file needs no `verus!` block (step 2). |
| `serde::Serialize` derive on a struct in a block | DeliveryRecord and other output records | Rejected: the serializer trait is not declared to Verus. | Keep such structs outside the block; they have `pub` fields and can be declared with an `external_type_specification`. |
| An `impl FnMut` parameter (`decide`) | route_links | Supported; the call needs the caller's `call_requires` stated in the contract. | A clause in the contract, no code. |
| `f64` arithmetic and comparison | rate, routing times | An exec `+`, `-`, `/`, `<`, `==` yields only an uninterpreted relation (`add_ensures(a, b, o)` and so on), because Rust does not promise bit-identical results across platforms. Spec `+`, `-`, `/`, `<` on floats are the IEEE operations (`ieee_float_add` and so on in the solver), spec `==` is bit equality (`0.0 == -0.0` is false), and `a.ieee_eq(b)` is IEEE equality. `+=` and `-=` on `f64` have no library contract. A lemma over bare floats never fires on a struct field: this release gives a float parameter its 64-bit range fact but a float field none (`datatypes_invs` in `vir/context.rs` skips `TypX::Float`, though `typ_invariant` handles it), so the lemma's hidden `has_type` guard fails; a lemma over the struct itself fires, because a struct without invariants is always well typed. | Claims are stated as "the new field equals the IEEE operation of the old fields" and each exec operator is tied to its spec operator by a one-line trusted lemma, quantified over the struct whose field is the operand (`rate: Rate`); two trusted contracts give `+=` and `-=` the binary operators' meaning. Equality with zero is `ieee_eq`, never spec `==`. Done in step 2. |

Everything in the first group, and the whole mailbox and autonomy-phase
shapes, verified with the production definitions copied verbatim.

## Patterns from projects that use it

Three conventions recur across the public Verus codebases, and the numbers
below set expectations.

Trusted and verified files are named apart. Microsoft's
[verified-storage](https://github.com/microsoft/verified-storage) ends
every verified file in `_v.rs` and every file a reviewer must read in
`_t.rs`: "You don't have to read these files to have confidence in the
correctness of the system; you just need to have Verus verify them. The
other code files, i.e., the files ending in `_t.rs`, need to be read and
understood to audit the system properly." Each trusted file opens with a
doc comment saying so and what to audit ("This file is unverified and
should be tested/audited for correctness, most notably to verify that the
PoWER wrappers indeed enforce crash consistency"). Verus ships a line
counter (`source/tools/line_count` in its repository) that classifies
trusted, spec and proof, and executable lines, and that project reports its
proof-to-code ratio from it.

Specifications and proofs sit beside the code, not in it.
[Anvil](https://github.com/vmware-research/verifiable-controllers) keeps
`exec/`, `model/`, `proof/`, and `trusted/` directories beside each
controller; its exec functions carry one-line contracts that name the model
(`ensures res == model_reconciler::reconcile_done(state@)`). The
[vest](https://github.com/secure-foundations/vest) parsers keep a
`spec.rs`, `exec.rs`, and `proof.rs` under each combinator; the exec file
holds the code and the few `assume_specification`s it needs, the proof
file the lemmas. Both wrap foreign types the way the guide describes: Anvil
declares a `StringMap` whose body is a `BTreeMap<String, String>`, marks the
type and each method `external_body`, and gives it a `View` of
`Map<Seq<char>, Seq<char>>` so proofs speak about the abstract map; it has
402 such wrappers and no `assume_specification`. vest, verifying its own
code rather than a library's, uses 4 `assume_specification`s and 35
`external_body` items.

Trusted code is explicit and small. The Verus guide's
[trusted computing base](https://verus-lang.github.io/verus/guide/tcb.html)
page lists exactly what adds an assumption: an `assume`, a
`#[verifier::external_body]` function (its contract is taken on faith), an
`assume_specification` for a library function, an
`#[verifier::external_type_specification]` for a foreign type, an
`#[verifier::external_trait_specification]`, and an `#[verifier::external]`
item the prover never sees. `vstd` itself guards standard traits with an
`obeys_*` pattern: `PartialEq::eq` is specified as "if the type obeys the
equality laws, the result is the spec equality", and a type opts in by
proving or assuming the laws. The `BTreeMap` contracts are guarded the same
way by `obeys_cmp::<Key>()`, which is why a `String` key needs one axiom.

The [SOSP 2024 paper](https://doi.org/10.1145/3694715.3695952) verified
five systems, 6.1K lines of code with 31K of proof, in seconds to minutes
each:

| System | Trusted | Proof | Code | Proof per code line |
| --- | --- | --- | --- | --- |
| IronKV (ported from Dafny) | 1,613 | 4,509 | 1,533 | 2.9 |
| Node replication | 369 | 5,237 | 736 | 7.1 |
| Page table | 1,117 | 5,329 | 400 | 13.3 |
| Mimalloc port | 282 | 13,703 | 3,178 | 4.3 |
| Persistent log | 1,284 | 2,913 | 739 | 3.9 |

The UC San Diego report on verifying Tock's timer, which like ours is
existing code written without a prover in mind, kept "the implementation as
close as possible to the original code", declared all trusted code
explicitly, and reports two lines of spec and proof per line of code on the
one file with real invariants and almost none on the others. Its costs:
replacing iterators with loops, converting `Cell` to Verus's `PCell`, and
the verified file growing from 87 lines to 102. Its advice on reading
`old(a)` in a spec call is to write `&*old(a)`.

Two guide recommendations matter most for keeping proofs readable: break a
long proof into lemmas, because "solver response time typically increases
nonlinearly as proof size increases", and use `broadcast` lemmas sparingly,
"as every extra ambient fact slows the prover's overall performance".

## How Ripple uses it

Production files stay Rust a plugin author can read. A proved function keeps
its body and takes one attribute, `#[verus_spec(ret => ensures q(..))]`
(with `requires p(..)` when it makes a demand on its caller), whose clauses
name predicates in `<module>/proof.rs`; the predicate holds the quantifiers
and the field-by-field statements, so the production file shows only what
is claimed, not how. The struct the proof reads is declared to Verus from
`proof.rs` with a one-line `external_type_specification`, so its own text
is untouched apart from the fields the predicates read becoming `pub`
(still crate-private, on a `pub(crate)` struct). The production module says
`mod proof;`, and the proved function's first line, `proof! { broadcast use
proof::<module>_facts; }`, names the one group of library facts and trusted
axioms its proof needs; a module with several proved functions can say it
once instead, in a three-line `verus! { }` block. This is the layout
`Rate::due` is proved with, and it passes `cargo build`, `cargo test`,
`cargo clippy -- -D warnings`, and `cargo fmt --check` unchanged:

```rust
mod proof;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Rate {
    pub period_s: f64,
    pub remaining_s: f64,
    pub elapsed_s: f64,
}

impl Rate {
    // Verus proves the body meets this claim; proof.rs spells the claim out.
    #[verus_spec(ran => ensures proof::stepped(old(self), final(self), dt_s, ran))]
    pub(crate) fn due(&mut self, dt_s: f64) -> Option<f64> {
        proof! { broadcast use proof::rate_facts; }
        self.elapsed_s += dt_s;
        ...
```

`proof.rs` is `pub open(crate) spec fn`s with doc comments in plain words,
like the Rust book's convention for anything a reader will meet, and is the
only place `verus` vocabulary spreads out:

```rust
/// Lets Verus read `Rate`'s fields without a `verus!` block around the struct.
#[verifier::external_type_specification]
pub(crate) struct ExRate(Rate);

/// One call of `due` ran the plugin exactly when its countdown was within half
/// a step of due; when it ran, it handed over the time since the last run ...
pub open(crate) spec fn stepped(before: &Rate, after: &Rate, dt_s: f64, ran: Option<f64>) -> bool {
    &&& after.period_s == before.period_s
    &&& (ran is Some) == (before.remaining_s < half(dt_s))
    &&& ran is Some ==> ran == Some(before.elapsed_s + dt_s) && after.elapsed_s == 0.0f64
    ...
}
```

The `use` lines only proofs need carry `#[cfg(verus_only)]`, since the
items they import are erased from a plain build; a `broadcast group` or an
external type declaration survives erasure as a stub and takes
`#[allow(dead_code)]`.

Trusted items live in an `axioms` module inside `proof.rs`, each a
`#[verifier::external_body]` lemma or an `assume_specification` with a doc
comment that begins `Trusted:` and says why it is true, and the table at
the end of this document lists every one. The boundary is "a verified
engine around unverified plugins": each plugin trait gets an external trait
specification listing the methods the engine calls with no `ensures` at
all, so nothing a plugin computes is assumed; the engine's claims follow
from what the call was lent, since a plugin stepped with `&mut self` and a
context can change nothing else. The build half of each trait, behind
`where Self: Sized`, is simply not listed and stays invisible to proofs.

Three engine shapes needed workarounds, each tried in a scratch file:

- Erased payloads. Verus rejects `Arc<dyn Any + Send + Sync>` and function
  pointer fields, so `Payload` and `PayloadSerializer` in `messages.rs` and
  the reading box in `readings.rs` become one-line newtypes outside any
  block, declared opaque with an empty `external_type_specification`. The
  prover sees a value it can move, compare for identity, and store; the
  methods that look inside (`downcast`, `type_id`, the serializer call) are
  trusted two-line helpers whose contracts say what they return. This is
  the one change the plan makes to a type for the prover, and it is named
  here so it is not mistaken for a design choice.
- Contexts holding `&mut`. Verus's mutable-reference model is new (its
  [migration note](https://github.com/verus-lang/verus/blob/main/source/docs/migration-mut-ref.md)
  explains `final`). A context struct with a `&mut Messages` field, built in
  proved code and passed to a trusted plugin call, verifies when the call's
  contract names the reference's final value as `*final(old(ctx).messages)`.
  The plugin API does not change.
- Floats, as the table says: each exec operator is tied to its IEEE spec by
  a one-line trusted lemma, claims are written as equalities of IEEE
  expressions over the old fields, and equality with zero is IEEE equality
  (`ieee_eq`), so a negative zero keeps its meaning. Because this release
  knows nothing about a float field's range, the lemmas for `due` quantify
  over the `Rate` whose field is the operand; when Verus gains the missing
  field fact they collapse to five lemmas over bare floats.

The checks that are not Verus stay as they are. `reference/verus.py` plants
each fault from the catalog below in the production file, runs `cargo verus
verify -p ripple-core`, and requires the proof to fail; the retained run
goes to `paper/data/verification/`. Never build, run
equivalence, or commit while it runs: it edits production files in place and
restores them when it finishes, and after any interruption, confirm every
fault's anchor text occurs exactly once before committing.
`reference/equivalence.py` shows a refactor changed nothing. A proof earns
a claim in the paper only once it passes on `main` with a retained record;
until then the claim is Planned.

## Steps

Each step ends with the proof passing, the faults failing, equivalence
identical where code changed, and the claims and trusted-items tables
updated. Each names its production change up front, so the cost is known
before the work starts; where the change is "none", the predicates and
trusted contracts go in `proof.rs` and the production file gains only
`mod proof;`, the attribute, the `proof!` line, and `pub` on read fields.

1. Toolchain and opt-in. Add `vstd`, the package metadata, and the
   workspace `check-cfg` for `verus_only` to `ripple-core`; confirm `cargo
   build`, `cargo test`, `cargo clippy`, and `cargo fmt --check` are
   unchanged; put the install steps in the Rust book's development chapter;
   write `reference/verus.py` with the compile-fault and test-fault checks.
   Production change: none. Done on `verus-verification`: `vstd` is a
   workspace dependency, the first `cargo verus verify` takes about a
   minute (it verifies `vstd` and builds the crate's dependencies with the
   pinned toolchain) and later runs about three seconds, and the book has a
   "Verifying the engine" chapter.
2. Rate. Prove `Rate::due` (row 8): the plugin runs exactly when its
   countdown is within half a step of due, the elapsed time handed over is
   the IEEE sum of the steps since it last ran, and a plugin without a rate
   keeps its countdown at zero, so it runs every call. Production change:
   the closure that captures `&mut self` becomes an `if`; the three fields
   become `pub`; `Rate` derives `Copy`, which it is, so Verus meets no
   unspecified `clone`; `Rate::new` and its `ensure!` stay unproved.
   Trusted: the eight items in the table below. Faults: the reload
   deleted, the elapsed time not reset, the countdown zeroed for every
   rate, due compared with a whole step. Done: one item verified, four
   faults fail, `cargo build`, `cargo test`, `cargo clippy`, and `cargo
   fmt --check` unchanged.
3. Ports. Prove `PluginIo::read`, `write`, and `reads` over the existing
   `BTreeMap<String, f64>` (row 6's runtime half): a value reaches the port
   it was wired to and no other. Production change: one `ensure!` in
   `write` becomes `if`/`bail!`. Trusted: the `String` ordering axiom, the
   `&str` lookup axioms, the `with_context` contract. `receive` waits for
   an `iter_mut` contract. Faults: a write that lands on a neighbor's port,
   a read of the wrong input.
4. Readings. Prove `get`, `publish`, `deliver`, and `clear_fresh` (row 7)
   over the existing maps. Production change: the reading box becomes an
   opaque newtype with trusted `downcast_ref`, `is`, and `type_id`; two
   `ensure!`s become `if`/`bail!`; `for .. in &pending.values` becomes
   `.iter()`. Trusted: those three methods and a `BTreeMap::append`
   contract. Faults: get reads another sensor's slot, deliver skips a fresh
   sensor, deliver clears a sensor that took no reading, `fresh` returns a
   stale reading.
5. Mailbox. Prove `receive` and `subscribe` (row 3) over the existing
   `BTreeMap<Channel, Vec<Envelope>>`: only the asked channel's queue
   leaves, in order, with its sender, times, and payload, and nothing else
   in the mailbox changes. Production change: the `Payload` and
   `PayloadSerializer` newtypes; two `ensure!`s become `if`/`bail!`. The
   iterator chain that builds the result stays. Trusted: the `TypeId`
   contracts, the payload `downcast`. Faults: receive drains the wrong
   channel, receive leaves the queue in place, a subscription that changes
   type is accepted.
6. Autonomy phase. Prove the phase loop in `plugin_stack.rs` (rows 1 and
   8) with the plugin traits declared through external trait
   specifications: every autonomy is stepped in order through its trusted
   call, a plugin that did not run left its state, port values, and mailbox
   untouched, belief and contacts are untouched by the phase, and an error
   stops the loop at the failing slot. Production change: the two let
   chains become nested `if`s; `enumerate` becomes an index loop or waits
   for the next release. Faults: a phase that steps one autonomy, a deleted
   plugin call, a slot that steps when not due.
7. Routing. Prove the two loops of `route_links` (rows 3 and 4): a
   publication is offered to each subscriber of its channel once, in
   mailbox order, and a delayed message is delivered at the first routing
   phase at or after its due time. The proof works over the existing
   per-phase maps: `entry` and `drain` take trusted contracts under the
   `allocator_api` feature line, or `drain(..)` becomes
   `std::mem::take(..).into_iter()`. Production change: four `ensure!`s and
   one let chain; the `decide` callback's precondition is stated in the
   contract. Whether the branch's list-and-index rewrite of this function
   is worth taking is a performance decision, measured there, not a proof
   need. Faults: a delayed message delivered early, a subscriber skipped, a
   removed receiver's message delivered.
8. Phase order and the tick (row 2). Give each entity-level phase function
   its contract, trust `Scheduler::run_phase` with the one contract above,
   and prove `step_inner`: the phases ran in this order, each on every
   active entity once, an error stopped the later phases after launched
   work joined, and time advanced once. The remaining rows follow as the
   earlier steps show what a phase-level claim costs.

For each step, hand back the property, the production functions checked,
the trusted items added, the proof command and result, and what remains
unchecked, with time spent and lines of annotation, so the guarantee can be
weighed against its cost before the paper's verification section is
written.

## Claims

| Claim | Status |
| --- | --- |
| A rate-limited plugin runs when due and receives the elapsed time since its last run | Proved on `verus-verification` (step 2): `Rate::due`; four planted faults fail; `reference/verus.py` record retained |
| Port values reach the port they were wired to and no other | Planned (step 3) |
| A reading reaches only its sensor's slot; `latest` and `fresh` mean what they say | Planned (step 4) |
| Receiving from a mailbox drains only the asked channel, in order, and changes nothing else | Planned (step 5) |
| The autonomy phase steps every autonomy once, in order, through the trusted plugin call | Planned (step 6) |
| A publication is offered to each subscriber once; a delayed message arrives at its due phase | Planned (step 7) |

## Trusted items

Each lives in the `axioms` module of a `proof.rs`, with a doc comment that
begins `Trusted:` and says why it is true; this table is the audit list a
reader needs, in the sense of the `_t.rs` convention above.

| Item | File | Why it is true |
| --- | --- | --- |
| `assume_specification` for `<f64 as AddAssign>::add_assign` and `<f64 as SubAssign>::sub_assign` | `common/loop_rate/proof.rs` | Rust defines `x += y` as `x = x + y`; `vstd` states only the binary operators. |
| `div_is_ieee` | same | Rust's `f64` operators are IEEE 754 double arithmetic, rounding to nearest even. |
| `elapsed_add_is_ieee`, `reload_is_ieee`, `remaining_sub_is_ieee`, `remaining_lt_is_ieee`, `period_eq_is_ieee` | same | The same, for `+`, `-`, `<`, `==`; stated over the `Rate` fields they are applied to because this Verus release gives a float field no range fact (see the table above). |
| `ExRate` external type declaration | same | Declares `Rate` to Verus with its fields visible; it assumes nothing about their values. |

# Verifying the engine

Parts of the engine carry proofs checked by
[Verus](https://verus-lang.github.io/verus/guide/), a verifier for Rust. A
proved function keeps its plain Rust body. One `ensures` line under its
signature names the claim, and a `proof.rs` file beside the module spells
the claim out. Plain `cargo build`, `cargo test`, `cargo clippy`, and
`cargo fmt` never see the proofs: Verus erases them, and the binary is
unchanged. This chapter says what is proved, how to check it, and how to
read and write a proved file. The contracts themselves are the exact
statements; nothing here restates them.

## What is proved

One statement covers the tick. `Simulation::step` in `simulation.rs` has
the contract `stepped_and_settled` in `simulation/proof.rs`, and Verus
proves it from the contracts of the methods the tick calls, which are
proved in turn from theirs, down to the leaves. Nothing composes the
proofs by hand: the checker composes them, so a reader who trusts the
checker and the trusted items need read only the statement.

In words: one call to `step` on a run that is not finished runs the tick's
phases once each, in the order of the phase table in [How a run
works](../concepts/how-a-run-works.md), on the step time read after the
entities are started and the truth contacts snapshotted after the
interactions, and then advances time once. A phase that fails stops the
tick with the phases before it run exactly as the Ok case says, nothing
after it run, and time where it was; the run is marked finished by plugin
failure and the plugins are closed. A finished run returns nothing and
changes nothing.

Each phase's share is proved the same way: every active entity in order,
each plugin stepped when due with its own rate, its mailbox stamped with
the phase time, the belief, the tracks, and the port values, each
over the board the plugin before it left. Motion records each input's
value and the substep time for localization. Sensors see the truth and the
truth contacts built from the snapshot motion left. Estimation delivers
the pending readings, then runs localization and tracking once with their
own sources. Interactions run in order over the entities the one before
left, and the dead are deactivated, recorded, and announced. Metrics run
once each. Removal closes and removes every entity with no health left and
touches no other. Generation keeps every entity that existed and appends
the new ones, each with a fresh ID at the tick's time. Routing of one
network's messages is proved on its own: publications leave every mailbox
in mailbox order, each is offered to its channel's subscribers in that
order, a decided delay schedules it, and a message whose time has come is
delivered. The networks phase that calls the routing is a witness, not
proved: its proof was done and reverted because the mailbox gathering read
worse than the code.

The statement is relative to the plugins. Each call into a plugin is a
witness that the call happened with the stated inputs and returned Ok or
not, so the theorem is about what the engine does around the plugins: what
each was lent, when it ran, in what order, and what reached it. It says
nothing about what a plugin computes.

What stays trusted, and is said so in the claim: the plugin calls; floating
point, as IEEE lemmas over the fields they are applied to; random draws, as
some value keyed as the engine says; output that changes no engine state
(the snapshot, drawings, trace, recorder); library and foreign types the
claims carry but never read, declared opaque; and standard library
functions `vstd` has not specified, each as a one-line contract restating
the documented behavior. Every trusted item lives in the `axioms` module
of a `proof.rs` under a doc comment that begins `Trusted:` and says why it
holds, so `git grep 'Trusted:'` is the complete audit list.

What the claim does not say: that any other plugin is correct, that the
physics or the estimators are right, that floats behave as reals, or that
the run's outputs are what the mission author meant.

## One proved plugin rule

The delivery study's reserve guard, in
`crates/delivery/src/dispatcher/guard.rs`, is proved the same way. With
`manager.guard` on, the dispatcher refuses a reposition that would leave
the donor hub under half its vehicles of the class and a transfer that
would leave it under a quarter of its starting stock. The two decisions
are proved: the count of a hub's vehicles of a class, and of those idle and
not already told to move, is taken over the fleet the dispatcher knows,
each vehicle once, and the comparisons mean what the rule says. The
three-line refusal in `apply.rs` that calls them is read, not proved,
because the order handlers around it build strings and sort with closures,
which Verus does not read.

The guard reads the dispatcher's own fleet map, so it needed no engine
predicate. A plugin claim over ports, readings, or the mailbox would call
`read`, `write`, and the mailbox and readings methods and need their
contracts. Those sit on the public methods a plugin calls, not on private
helpers behind them, and the predicates are `open(crate)` in a private
`proof` module today; such a proof flips them to `pub open` in a
`pub mod proof` and nothing else moves. `PluginIo`, `Ports`, `Readings`,
and `Messages` keep crate-private fields, so a plugin reaches port values,
readings, and its mailbox only through the proved methods.

## What the proofs changed in the code

Every change was behavior-preserving, shown by byte-identical output on
every mission and workload, and each made the code plainer or left it as
it was. The port board became one signals map per entity, with `PluginIo`
a view onto it and one writer per port, so a proof about a read or write
is about one map. Proved loops run over small context structs built from
the fields of the larger one, such as `DeliveryPhase` (the time, the
messages in flight, the trace), so the loop keeps borrowing the rest and
the contract carries no frame clause. The `dyn Any` code sits in two small
modules Verus skips, `plugin/sensor/readings/any_reading.rs` and
`pubsub/messages/payload.rs`, with trusted contracts stating what their
methods return. The step counter and the group counter became one-line
methods, because an exec `+= 1` needs an overflow proof and a run never
counts that far. The reserve guard's two decisions moved out of the order
handlers into `guard.rs`, and the two delivery enums the guard compares
derive `Structural`, which lets Verus read `==` on them as equality.

## Where Ripple and Verus did not fit

An iterator chain means only what its closures promise, so `find`, `map`,
and `filter` inside a proved function trade for `match` arms or a `for`
loop. That cost decided what stays unproved. The estimator accessors
`latest` and `fresh` do prove, but only as `match` arms with the `dyn Any`
downcast carved into a trusted method, which read worse than the four-line
originals, so they stay read and the readings slots beneath them are
proved. The networks phase was proved and reverted: the mailbox gathering
needed a method with four ghost snapshots, a step struct, and a forwarding
trait in place of one plain method, so `route_messages` is a witness again
and the routing claims beneath it stand. The guard's refusal lines in
`apply.rs` stay read for the same reason, and `finish` and
`close_plugins` at the end of a failed or terminal tick are witnesses whose
values the claims never need.

Four more limits shape the code rather than skip it. A float field has no
range fact in this release, so every arithmetic step on one is a trusted
IEEE lemma stated over the struct that holds the field. A `&mut Vec` used
as a slice aborts the checker before it verifies anything, through
`anyhow`'s `DerefMut`, so proved loops call `as_mut_slice()`.
`external_derive` takes one trait, so an enum with two serde derives stays
outside the block and is declared to the proofs from `proof.rs`. And
`vstd` has no contract for `Vec::retain`, `BTreeMap::append`,
`BTreeSet::last`, or `div_ceil`, so each is assumed as a one-line
`Trusted:` contract restating the documented behavior.

## Checking the proofs

Download release 0.2026.11.02 (its `vstd` is `0.0.0-2026-11-01-0251`) for
your platform from the
[Verus releases](https://github.com/verus-lang/verus/releases), unzip it,
and put the folder on your `PATH`; it holds `verus`, `cargo-verus`, and its
own `z3`. On macOS, run the folder's `macos_allow_gatekeeper.sh` once, and
install the Rust toolchain the release pins with
`rustup toolchain install 1.99.1`. Then, from the repository root:

```sh
cargo verus verify -p delivery
python3 reference/verus.py
```

The first command proves every contract in the core crate and then the
delivery crate's reserve guard, reporting each crate's count; the first
run also verifies Verus's standard library, which takes a minute, and
later runs take seconds. Cargo skips a crate whose sources have not
changed and then reports no verification, so `cargo clean -p ripple-core`
before a run whose result line is missing.

The second command does the same, then plants each fault it lists in the
production source, one at a time, and requires Verus to report an unproved
item. A fault the proof survives means a claim is too weak, and a fault
that only breaks the build does not count. It writes a record to
`runs/verusNNN/` naming the Verus version, the item count, each fault's
verdict, and the hash of every source file with a block or that Verus
skips; `--retain` copies it to `paper/data/verification/`. It edits source
files in place and restores them, so do not build or commit while it runs;
after an interruption, check that every fault's anchor text occurs exactly
once.

A proof changes no behavior: every mission writes byte-identical output
before and after it. A proof earns a claim in the paper only once it passes
on `main` with a retained record; until then the claim is Planned. The four
cargo checks must pass unchanged.

## Reading a proved file

`crates/core/src/common/loop_rate.rs` is the pattern. The file's code sits
in one `verus! { }` block, opened after the `use` and `mod` lines and
closed at the end of the file with `} // verus!`, the layout the public
Verus projects use. Verus checks everything in the block that is not
marked `#[verifier::external]`: construction and configuration code,
output records no proof reads, the plugin traits, getters, and `Drop`
carry the mark, and an `impl` that mixes proved and unproved methods is
split in two. A record a proof reads keeps its serde derive under
`#[verifier::external_derive(Serialize)]`; the attribute takes one derive,
and a bare `#[verifier::external_derive]` skips them all.

A proved function names its return value, `-> (ran: Option<f64>)`, and
the line under the signature, `ensures proof::stepped(old(self),
final(self), dt_s, ran)`, is the claim Verus proves the body against.
`old(self)` and `final(self)` are the value before and after. A loop
carries its invariant in its header the same way, one line naming one
predicate that takes the loop's ghost iterator whole:

```rust
for slot in it: slots.as_mut_slice().iter_mut()
    invariant proof::agents_so_far(old(slots), it, time, state, contacts)
{
```

The block's first line, `broadcast use proof::<module>_facts;`, names the
one group of library facts and trusted axioms the module's proofs may use.
`proof.rs` holds the claims as `spec fn`s with doc comments in plain
words, one named predicate per contract, and the `axioms` module of
trusted items. It is the only place Verus vocabulary spreads out. A `use`
of a spec-only item carries `#[cfg(verus_only)]`, which only `cargo verus`
sets and which never guards real code.

The notation a reader meets in `proof.rs`:

| Written | Meaning |
| --- | --- |
| `old(x)`, `final(x)` | a `&mut` argument's value at entry and when the borrow ends |
| `v@` | the mathematical view of `v`: `Seq<T>` for a `Vec`, `Map<K, V>` for a `BTreeMap` |
| `forall\|i: int\| 0 <= i < n ==> p(i)` | a quantifier over `int`, which never overflows |
| `#[trigger] f(i)` | the term the solver matches to instantiate the quantifier |
| `a =~= b` | extensional equality of sequences or maps |
| `x is Some`, `x->0`, `r is Ok` | matching on an `Option` or `Result` and reading its payload |
| `&&& a &&& b` | a conjunction, one line per fact |
| `*after == Stack { slots: after.slots, ..*before }` | a frame: everything but the named fields is unchanged |
| `pub open(crate) spec fn` | a predicate whose body this crate can see; `pub open` shows it to plugin crates |
| `assume_specification [path](args) ensures ...;` | a contract taken on faith for a function Verus cannot see |
| `uninterp spec fn` | a function with no definition: a witness the claims name but never unfold |
| `#[verifier::external_type_specification] struct ExT(T);` | makes a type from another file known; with `external_body` its fields stay hidden |

## Writing a proof

The body stays plain Rust, and a proof earns its place only if the code
stays as readable or gets plainer. Where Verus rejects a shape, the
replacement must read as well as what it replaces, and the outputs must
stay byte-identical. Verus moves quickly, so recheck a
rejection against the pinned release before working around it. The small
shapes met so far and their plain forms: a closure that captures `&mut`
becomes an `if`; a let chain becomes one `if` over the `Option`; `ensure!`
becomes `if !c { bail!(..) }`; an iterator chain becomes a `for` loop with
the body's work in a named method whose contract the invariant quotes; and
anything touching `dyn Any` goes in a small module of plain Rust with
`#[cfg_attr(verus_only, verus_verify(external))]` on its `mod` line and
trusted contracts on its methods. The larger limits are in "Where Ripple
and Verus did not fit" above.

Put every predicate and lemma in `<module>/proof.rs`. The production file
gets `mod proof;`, the block, the `broadcast use` line, one `ensures` line
under each proved function, one `invariant` line in each loop header, and
`#[verifier::external]` on what Verus is not to read, nothing more. A
loop keeps its body as it is or plainer and declares no ghost state and no
proof block; when the body can return early, put the loop in a small
function with its own contract so the invariant states only the loop's
progress, as `open_all` in `pubsub/messages.rs` does. A proved method
takes a small context of only what it changes, built from the fields of
the larger struct, so the loop keeps borrowing the rest and the contract
has no frame to restate. A proof that needs more than this stays
unwritten and the function read, with the reason in a comment.

Each new assumption goes in the `axioms` module under a `Trusted:` comment
that says why it holds, and each new claim gets a fault in
`reference/verus.py`. Then run, in this order: `cargo verus verify -p
delivery`, the four cargo checks, and `python3 reference/verus.py --retain`
with nothing else running; then check that every mission's output matches
the previous commit's. Commit once all pass.

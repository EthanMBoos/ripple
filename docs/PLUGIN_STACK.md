# The entity plugin stack: per-tick cost

Where the engine's own bookkeeping costs time on the one-worker path the
sweeps use, and the two changes that remove it. Both keep every run's
output byte-identical and change nothing a plugin author writes.

The stack is two files, each opening with a walk through its half of the
lifecycle. `entity/compiled_stack.rs` is the mission's plugin list,
checked and wired once at load; `entity/plugin_stack.rs` is the live
plugins and their tick. Read those comments for the shape. This page is
only about what the tick rebuilds that it does not need to.

The numbers are from a profile of a headless 64-hub one-hour delivery
day, release build, one worker, sampled with macOS `sample`. Repeat it
with the same command after each change.

## Ports: names resolved every tick

The DEVNOTE above `endpoint` in `plugin_stack.rs` records the pattern:
message addresses "used to be rebuilt from scratch for every plugin on
every step" until they were built once per entity. The same pattern is
still in the ports. Every tick, for every entity, with string keys:

- `PluginStack::autonomy` copies each autonomy's outputs into `outputs`,
  a `BTreeMap<String, f64>`, cloning every port name. A Courier has six
  ports. In the profile this was 11% of the main thread, more than
  Courier's own logic.
- `PluginIo::receive` in `controllers` and `motion` looks each input up by
  name in the upstream map, once per substep.
- Every `io.read("name")` and `io.write("name", value)` inside a plugin is
  a string lookup in a `BTreeMap`. With `receive`, 6%.

None of this can change between ticks. The port names, which plugin
produces which, and their order are fixed when `CompiledStack::compile`
wires the chain. So resolve names to positions once, at compile, and let
the tick copy numbers.

The shape:

- `Signals` becomes `Vec<f64>` with a `Vec<Port>` beside it naming the
  positions, built once in `compile` from the merged autonomy outputs in
  first-writer order. Later writers still win, because the merge writes in
  source order.
- `PluginIo` keeps `Vec<f64>` for inputs and outputs plus, per input, the
  index of its upstream value. `receive` becomes a zip over two slices and
  the autonomy merge a zip writing into the bus.
- `read("name")` and `write("name", value)` keep their string arguments,
  so no plugin author changes anything. They become a short linear scan
  over a handful of ports.

One subtlety decides byte-identity. `outputs()` feeds the `--trace`
record, and today it walks a `BTreeMap`, so ports come out in alphabetical
order. The `Vec` version must walk them in the same order, which is one
sorted index built at compile time. `command_names` and `command_values`
go to localization in the same order as each other, so they only have to
agree, which they would.

Expected gain is about a sixth of the main thread on a 64-hub day.

## Routing: string keys and clones per delivery

Each network phase, `NetworkContext::route_links` in `pubsub/messages.rs`
rebuilds its addressing from strings. A channel is `(String, String)`,
network and topic; a `MessageEndpoint` is an entity id and a `String`
plugin name. Per phase with publications present:

- Every mailbox's outbox is drained, idle or not, and each publication is
  paired with a clone of its sender's endpoint.
- The subscriber index, a `BTreeMap` from `(entity, &channel)` to the
  subscribing mailboxes, is built from every mailbox's subscriptions. Each
  insertion compares two strings down the tree.
- The per-sender sequence counter is keyed by `(entity, topic)`, cloning
  the topic string once per publication.
- Each scheduled delivery clones the receiver endpoint, the channel, and
  the sender endpoint into its `ScheduledMessage`.
- Delivery rebuilds a second map from endpoint to mailbox index, again by
  string comparison, and finds each receiver's inbox by a cloned channel.
- `Messages::receive` builds an owned channel from its two `&str`
  arguments on every call, so every subscriber pays two allocations per
  tick just to check its queue.

In the profile this was about a tenth of the main thread, half of the
message phase's 19%.

None of it changes while the population is fixed. Which plugin subscribes
to which channel is set when its entity spawns, and a channel's name never
changes. So give messages the same treatment as the ports:

- Intern channels. The simulation keeps one table from `(network, topic)`
  to a small integer channel id, filled by `subscribe` and `publish`.
  `subscriptions`, `inbox`, `Publication`, `Envelope`, and
  `ScheduledMessage` carry the id. `subscribe`, `publish`, and `receive`
  keep their `&str` arguments; interning is one lookup in a table of a few
  dozen names, and a plugin that subscribes to one or two channels can
  scan its own short list in `receive`.
- Number mailboxes. Each mailbox gets a stable integer when its entity
  spawns, in stack order, and keeps its `MessageEndpoint` only for the
  trace and error text. `ScheduledMessage.receiver` and `Envelope.sender`
  hold the number. A removed entity's number has no mailbox, which is the
  check delivery already makes.
- Build the subscriber index once per population. Its contents change
  only when an entity spawns or is removed, so keep it per network and
  rebuild it on a population generation counter instead of every phase.
  Keyed by integers it is a `Vec` indexed by channel id, or a map of
  integer pairs.

Two things decide byte-identity. First, order: receivers for a channel are
pushed in mailbox index order and delivered in that order, so the new
index must list them the same way, and the per-sender sequence numbers
must count exactly as they do now, because keyed network loss draws from
them. Second, the trace: `DeliveryRecord` writes network, topic, sender,
and receiver as strings, and the C++ comparison matches deliveries on
those names, so the id tables must map back to the same text.

If routing is under a Creusot proof by then, integer keys help it: vectors
indexed by id have library contracts, and string-keyed maps need a
hand-written model.

## Order of work

1. Ports, as one commit.
2. Routing, as one commit, or two: interning and mailbox numbers first,
   since they remove the clones; the cached subscriber index second, since
   it adds a generation counter the first two do not need.

Each is proved by `reference/equivalence.py OLD NEW` with the trace
missions included, a repeat of the profile with the same command, and a
`DEVNOTE` with the numbers beside the change. The equivalence reader
currently rejects couriers built from a recipe, whose group index is not
in the mission's entity list; fix that first so the delivery missions are
in the run.

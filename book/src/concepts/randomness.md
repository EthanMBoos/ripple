# Randomness and seeds

Sensor noise, message loss, randomized spawns, and auction bids are random.
Every one of them comes from the mission's `seed`, so a run repeats exactly:
the same mission and seed give the same output, byte for byte, at any worker
count. Change the seed and you get a different, equally repeatable run.

## Streams and keyed draws

The usual way to make random numbers is a *stream*: a generator seeded once
that hands out the next number each time you ask. A stream repeats, but each
number depends on how many were taken before it. That breaks a comparison
between two methods.

Say you compare two navigation methods on seed 7. An observer measures every
contact it can see, with noise on each. At tick 50 the agents under method A
see contacts 2 and 3; under method B they see 2, 3, and 4. With a stream, B
takes three numbers more at tick 50 than A, and from then on the two runs get
different noise on *every* measurement, including the contacts both see. The
comparison now mixes two things: the methods, and the noise they happened to
get.

Ripple uses *keyed draws* instead. Each number is a function of a label saying
what it is for:

```text
hash(seed, entity, plugin, tick, target, quantity, axis)
```

Observer 1's position error on contact 2 along x at tick 50 is the same number
in every run with seed 7, whatever else happened in the run. Methods A and B
see the same noise on the same measurements, so the difference between their
scores comes from the methods. This is the statistician's *common random
numbers*: paired runs that share their randomness give a much sharper
comparison than independent ones, with fewer seeds. (The design is the one
counter-based generators such as Random123's Philox and JAX's Threefry use.
Ripple hashes the label with SplitMix64's mixer.)

## Asking for a number

A plugin gets its random numbers through its context, `context.random`. The
engine supplies the seed, entity ID, plugin identity, and tick; the plugin
supplies the rest of the label, its *key*, as a list of integers:

```rust
let random = context.random;
let noise_world_m = Vec3::new(
    random.normal(&[0], 0.0, self.stddev_m)?, // x
    random.normal(&[1], 0.0, self.stddev_m)?, // y
    random.normal(&[2], 0.0, self.stddev_m)?, // z
);
```

`random.uniform(&key)` gives a number strictly inside (0, 1), and
`random.normal(&key, mean, stddev)` a normal one. `PluginRandom::name("topic")`
turns a name into a key part. Two rules make keys work:

1. **A different key for every number.** The same key in the same tick gives
   the same number, so the three axes above need three keys. If a sensor
   measures several contacts, the contact's ID goes in the key too:
   `&[contact.id as u64, axis]`.
2. **Stable IDs, never list positions.** A contact's index in this tick's list
   changes when another contact appears; its ID doesn't. Use entity IDs,
   axis numbers, and names, so the same thing gets the same key in every run.

The built-in plugins key their numbers this way. NoisyState keys each error by
`[reading, quantity, axis]` and NoisyContacts by `[contact ID, quantity,
axis]`, where the quantity is 0 for position, 1 for velocity, and 2 for
attitude. SphereNetwork keys a message's loss by the sending and receiving
entities, the topic, and the message's sequence among the sender's messages on
that topic that tick, so a receiving entity hears a message or doesn't for all
its subscribers at once, as one radio would. Spawns key their start by the
entity group, the spawn's number in it, the field (heading, x, y, z), and the
try, since a start that collides is drawn again.

The plugin identity in the label is the plugin's place on its entity, such as
`autonomy/AuctionAssign:0`; for a sensor it is the sensor's label (its name
and count, such as `NoisyState:0`, or the label a YAML mission gives it); for a
network it is the network's name.

## What is reproducible where

- **One platform:** bit for bit. Same mission, seed, and executable give the
  same bytes, at 1, 2, or 8 workers, with Rerun on or off.
- **Across platforms:** the labels and hashes are integer arithmetic and agree
  exactly. Turning them into normal numbers takes a logarithm and a cosine,
  and those can differ in the last bit between macOS's and Linux's math
  libraries. The dynamics do the same; Ripple on Linux agrees with Ripple on
  macOS to about 1e-12 m over every mission
  (`paper/data/rust-platform`).

Because a number depends only on its label, another implementation of the
same hash draws the same numbers, which allows external verification sample
for sample (see the [reference
checks](https://github.com/EthanMBoos/ripple/blob/main/reference/README.md)).

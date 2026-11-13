# Delivery: a region of blood-delivery hubs with a fleet manager

One day of blood delivery across a region of hubs, flown by fixed-wing
aircraft and multirotors. A rule-based dispatcher assigns every request. A
fleet manager above it reads events and gives orders: a language model, a
structured replanner, a YAML policy, a hand-written script, or nothing. The
paper is [paper/manuscript/main.tex](../../paper/manuscript/main.tex).

## Run

```sh
cargo build --release
./target/release/ripple run crates/delivery/missions/region.yaml --headless
python3 crates/delivery/results.py runs/region/run000
```

`missions/region.yaml` is the one mission. The Dispatcher places the hubs on
a grid and builds each hub's couriers from the two recipes at the top of the
file, so the region's size is one value:

```sh
D=entities.region.autonomy.Dispatcher
./target/release/ripple run crates/delivery/missions/region.yaml --headless \
  $D.region.hubs:=256
```

`region.hubs` is a square number up to 1,024, with five vehicles per hub.
`region.layout` jitters the hubs off the grid by a layout seed, and
`region.hub_positions_m` places them by hand. Every other setting is
commented in the mission file, and the sweep files in `missions/` are the
experiments as run. Any setting can be set on the command line:

```sh
D=entities.region.autonomy.Dispatcher
./target/release/ripple run crates/delivery/missions/region.yaml --headless \
  $D.manager.kind:=replanner \
  "$D.events.scripted:=[{kind: surge, at_min: 60, hubs: [1]}]"
```

`results.py` prints the score columns of one or more runs side by side.
`tests/region.rs` runs short days of the same mission.

## The pieces

`region.rs` lays out the hubs and sites. `dispatcher.rs` and `dispatcher/`
hold requests and stock, call the manager, check its orders, and assign
flights. `courier.rs` flies them using the drops and landings in `skill.rs`.
`help.rs` handles lending between hubs; `replanner.rs` and `policies/` hold
fixed alternatives to the model in `model.rs`. `score.rs` measures what
happened.

## The day

Requests arrive at about 12 per hub per hour. Each is for one site, one
blood type, and an urgency: routine (due in 2 h), urgent (30 min), or trauma
(15 min). They are drawn from random numbers keyed by hub and index, so a
seed is the same day no matter how it is served or how many hubs there are.

The dispatcher assigns each request to the idle vehicle at the site's own
hub that has the stock, the skill, and the range and arrives soonest. It
does not read events. An unserved request is retried until its deadline
passes, then counted as missed with the reason of the last try.

### Events

`events.per_hub_per_day` sets how many of each kind the seed draws per hub;
`events.scripted` is a fixed list instead.

| Kind | What happens | Fact or rule |
| --- | --- | --- |
| surge | 16 trauma requests per hub over 15 min | fact |
| shortage | an item's stock at those hubs goes to zero | fact |
| outage | a hub cannot launch | fact |
| closure | a circle to keep out of | rule, breaches scored |
| corridor | a strip to keep out of below 150 m | rule, breaches scored |
| weather | a vehicle class must not launch at those hubs | rule, breaches scored |
| rooftop_closed | a hospital rooftop is closed; drops at its pad only | rule, breaches scored |
| priority | one hospital's requests come first | preference, scored as on-time share |

Facts happen whether or not anyone acts. Rules are scored from truth, and a
delivery whose flight broke a rule does not count, so obeying a rule is never
a loss on the score.

### Orders

The manager answers each call with `{"orders": [...]}`, one key per order.
The same forms are written in YAML for `manager.scripted`.

| Order | Effect |
| --- | --- |
| `reserve` | keep `count` vehicles at a hub for one site until a minute |
| `avoid` | route every new flight around a circle or corridor |
| `reposition` | fly idle vehicles of a class to another hub, which becomes theirs |
| `transfer` | fly units of an item from one hub to another |
| `ground` | no launches of a class from those hubs until a minute |
| `prioritize` | serve a site's or an urgency's requests first |
| `serve_from` | let other hubs serve a site besides its own |
| `use_pad` | drop at a hospital's ground pad instead of landing on its roof |
| `help` | let one offered hub serve an asking hub's sites until its event ends |

Orders that break a rule in force, or ask for more than is there, are
refused or applied for what is there; the score counts each outcome by
name.

### The fleet manager

`manager.kind` is `none`, `scripted`, `replanner`, `policy`, `model`, or
`veto` (the replanner plans, the model drops what it objects to). The
manager is called when events start and every `check_in_s`. `view`,
`feedback`, and `checker` set what a model is shown and told, so the paper's
interfaces are mission settings, not code paths. `calls_per_hour` caps the
calls, `cluster_side` gives each square cluster of hubs its own manager, and
`top_kind` adds a help manager over them all, which only grants `help`
orders. Each has a comment in `region.yaml`.

For a surge or outage, the dispatcher offers lenders that can spare aircraft
and reach an affected site in time and return on one charge. A `help` grant
lets their aircraft serve the neighbor and come home; it does not relocate
them.

Model replies are saved under `cache_dir` by a hash of the prompt, so a
rerun with the same seed and settings makes no calls, and `offline: true`
uses only saved replies.

## Score

Delivery scores come from flights, not proposed orders. `score` is the
weighted share of requests delivered on time, with weights of 1 for routine,
2 for urgent, and 4 for trauma. Other columns count on-time, late, and
missed requests by urgency and reason, rule breaches, lost vehicles, orders
by name and outcome, and calls. Per-hub columns count each hub's own requests
and what it gave and received, so a day's losses can be laid against the
hubs that donated.

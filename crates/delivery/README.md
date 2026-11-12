# Delivery: a region of blood-delivery hubs with a fleet manager

One operating day of blood delivery across a region of hubs, flown by
fixed-wing aircraft and multirotors, run by a rule-based dispatcher, with a
fleet manager above it that reads events and gives orders. The fleet manager
is a language model, a structured replanner, a hand-written script, or
nothing. The study's claim and results are in the paper,
[paper/manuscript/main.tex](../../paper/manuscript/main.tex); what is left
before submission is in [docs/TODO.md](../../docs/TODO.md).

## Run

```sh
cargo build --release
./target/release/ripple run crates/delivery/missions/region.yaml --headless
python3 crates/delivery/results.py runs/region/run000
```

`missions/region.yaml` is the one mission: the Dispatcher places the hubs on
a grid and builds each hub's couriers from the two recipes at the top of
the file, so the region's size is one value and the file is the same length
at 16 hubs and at 1,024:

```sh
D=entities.region.autonomy.Dispatcher
ripple run crates/delivery/missions/region.yaml --headless $D.region.hubs:=256
```

`region.hubs` is a square number, 1 to 1,024, five vehicles per hub;
`fleet` sets how many of each recipe a hub gets, `per_hub` for every hub or
`counts`, one number per hub in hub order. With `region.block_side`, the
counts and the scripted events are written for a block of that many hubs a
side and laid over every block of the grid, each block turned or mirrored by
its number; a scripted event then names hubs only. `region-1-checked.yaml`
is the same day on the Rascal and the Firefly. The sweep files set the size the
same way, as a parameter.

`region.layout` displaces the hubs from the grid: each moves up to
`jitter_m` from its grid point, hubs stay `min_spacing_m` apart, and `seed`
keys the layout apart from the demand seed, so a sweep names a layout by one
number (0 is the grid). `region.hub_positions_m`, one [east, north] per hub,
places them by hand instead. For the matched grid and jitter comparison,
`region.py --policy-sweeps FOLDER` writes five matched days' six-policy
sweeps without model calls; its `--jitter-km`, `--min-spacing-km` and
`--layout-seed` draw the same layouts and write them out as
`region.hub_positions_m`.

Any setting can be changed on the command line. The prefix for the
dispatcher is `entities.region.autonomy.Dispatcher`:

```sh
D=entities.region.autonomy.Dispatcher
ripple run crates/delivery/missions/region-4.yaml --headless \
  $D.manager.kind:=replanner \
  "$D.events.scripted:=[{kind: surge, at_min: 60, hubs: [1]}]" \
  "$D.events.per_hub_per_day:={surge: 0, shortage: 0, closure: 0, outage: 0, weather: 0, priority: 0, rooftop_closed: 0, corridor: 0}"
```

`results.py` prints the DeliveryScore columns of one or more runs side by
side. The tests in `tests/region.rs` run short days of the same missions.

## The pieces

| Module | What it holds |
| --- | --- |
| `region.rs` | Hubs on a square grid, 15 km apart; each hub's six clinics and two hospital rooftops with ground pads. Site codes are `3A` or `3H1`. |
| `courier.rs` | The vehicle autonomy: flies an assignment with a skill, returns, reloads, relocates to a new hub, reports status, and is lost when its endurance runs out away from a hub. A fixed-wing near a site for a minute without delivering is circling it and runs out `run_in_m` to come back in straight. |
| `skill.rs`, `skill/` | `drop` (parachute release on a pass or hover) and `land` (multirotors only, on rooftops). Either can go out and back through one `via` waypoint for a detour. |
| `dispatcher.rs` | One per region: stock per hub, the day's requests, events, the rules in force, the fleet manager's calls, and the assignment of each request to a vehicle. Draws the hubs, the sites and the areas closed now in the viewer. Its helpers are the files in `dispatcher/`: the mission keys (`config.rs`), each tick's bookkeeping (`day.rs`), the fleet managers, their scopes and their calls (`calls.rs`), the prompts (`prompt.rs`), each order's checks and effect (`apply.rs`), and assignment (`assign.rs`). |
| `event.rs` | The eight event kinds, their seeded script, and their texts from the mission. |
| `order.rs` | The orders a fleet manager can give and how a reply is read. |
| `rules.rs` | The rules the orders leave in force, read at every assignment: reserves, avoided areas, groundings, priorities, extra serving hubs, open pads. |
| `scope.rs` | What one fleet manager is responsible for, the whole region or one cluster's hubs: it filters what the manager is shown, gates what it may order, and places the rules it gives that name no hub. |
| `view.rs` | What the fleet manager sees: per-hub summaries, or only the exceptions. |
| `help.rs` | Help between hubs: a surge or outage hub asks for aircraft, the dispatcher lists the hubs that can spare some and reach it, and the help manager grants one or none; a fixed-rule broker is the control. |
| `replanner.rs` | The structured replanner: fixed rules per event kind and a greedy pick of donor hubs by distance. |
| `policy.rs`, `policies/` | Rules written once as YAML, by hand or by a model, in the form `policies/SYNTAX.md` describes; `policies/replanner.yaml` is the replanner in it. |
| `model.rs`, `client.rs` | The language-model fleet manager: prompt and saved replies; clients for the `claude` and `codex` commands, the Anthropic and OpenAI APIs, and Ollama. |
| `score.rs` | DeliveryScore: everything measured from what was flown. |
| `messages.rs` | The messages between dispatcher, couriers and score. |

## The day

Requests arrive at about 12 per hub per hour until `requests.until_s`; the
run ends 30 minutes later. Each is for one site, one blood type and an urgency:
routine (due in 2 h), urgent (30 min) or trauma (15 min). They are drawn from
random numbers keyed by hub and index, so a seed is the same day no matter how
it is served and no matter how many hubs there are.

Each hub holds `units_per_item` of each blood type, restocked every
`restock_interval_s`; a delivery takes `package_units`. The dispatcher assigns
each request to the idle vehicle at the site's own hub that has the stock, the
skill and the range and arrives soonest. It does not read events. A request
with no vehicle is retried every `retry_interval_s`; when its deadline passes
it is missed with the reason the last try failed.

### Events

The number of events of each kind per hub per day is set in
`events.per_hub_per_day`; the count is drawn from the seed, so a larger
region faces more events. `events.scripted` is a fixed list instead; fields
left out are filled from the seed. Each kind has one text in the mission,
written once.

| Kind | What happens | Fact or rule |
| --- | --- | --- |
| surge | 16 trauma requests per hub over 15 min, alternating hospitals and clinics | fact |
| shortage | an item's stock at those hubs goes to zero | fact |
| outage | a hub cannot launch | fact |
| closure | a circle to keep out of | rule, breaches scored |
| corridor | a strip to keep out of below 150 m | rule, breaches scored |
| weather | a vehicle class must not launch at those hubs | rule, breaches scored |
| rooftop_closed | a hospital rooftop is closed; drops at its pad only | rule, breaches scored |
| priority | one hospital's requests come first | preference, scored as on-time share |

Facts happen whether or not anyone acts. Rules are scored from truth: a
vehicle inside a closed area counts against the run only if it was sent after
the closure began. A delivery whose flight broke a rule is not counted as
delivered (`delivered_in_breach`), so obeying a rule is never a loss on the
score; without this the dispatcher alone, which ignores every rule, beat the
replanner.

### Orders

The fleet manager answers each call with `{"orders": [...]}`. Each order is a
JSON object with one key; the same forms are written in YAML for
`manager.scripted`.

| Order | Effect |
| --- | --- |
| `reserve` | keep `count` vehicles at a hub for one site until a minute |
| `avoid` | route every new flight around a circle or corridor; requests to sites inside wait |
| `reposition` | fly idle vehicles of a class from one hub to another, which becomes their hub |
| `transfer` | fly units of an item from one hub to another as transfer flights |
| `ground` | no launches of a class (or any) from those hubs until a minute |
| `prioritize` | serve a site's or an urgency's requests first |
| `serve_from` | let other hubs serve a site besides its own |
| `use_pad` | deliver to a hospital's ground pad by drop instead of landing on its rooftop |

### The fleet manager

`manager.kind` is `none`, `scripted`, `replanner`, `policy`, `model` or
`veto`. The manager is called when events start and every `check_in_s`.
With `veto`, the replanner plans and the model is shown the proposed orders
with the situation and replies with the numbers to drop; `dropped` counts
them, and a call with nothing proposed is not made. With `policy`,
the file `manager.policy` names is read once and its rules give the orders
(`policies/SYNTAX.md`); `python3 crates/delivery/write_policy.py` asks a model
to write one from the mission's description, so the model can be in the loop
or in the code. The interface to a model is three settings, so the study's
interfaces are configurations, not code paths: `view` is `summaries`, one
line per hub, or `exceptions`, only the hubs with late or stranded requests,
low stock, an outage or lost vehicles; `feedback` and `checker` are below.

`manager.calls_per_hour` is a call budget that does not grow with the fleet:
calls are at least that far apart, and events that start between calls wait
for the next one, together. 0 means a call per event. It applies to the
replanner too, so the budget's cost can be measured without a model.

`manager.cluster_side` splits the region into square clusters of that many
hubs a side, each with its own fleet manager call. It sees its own hubs, the
events and orders in force that touch them, and its own orders' feedback and
delivery tally. An order naming a hub or site outside the cluster is refused
(`refused_outside_cluster`), and an avoid rule or a priority by urgency from
a cluster manager reaches only its own hubs' flights and requests. 0 is one
manager over the region, which sees and reaches everything. The replanner
splits the same way, which measures what the split alone costs.

`manager.top_kind` adds a help manager (`replanner` or `model`;
`top_model` names its model, or `model` is used), called after the
managers over the region or the clusters. It gives only `help` orders.

When a surge or outage starts at a hub, the hub asks for aircraft, and the
help manager is shown each request with its offers (`help.rs`): hubs that
are not asking or lending themselves, can spare aircraft above half their
own of a class, and whose aircraft of that class reach at least one of the
hub's sites within a trauma deadline and return on one charge. Each offer
gives the hub's distance, how many of the asking hub's sites each class
reaches in time, the hub's own waiting requests, and the minutes to the
nearest site, rounded as shown, so a fixed rule and a model choose from
the same numbers.

A `help` order lets one offered hub serve the asking hub's sites until the
hub's event ends; its aircraft fly each request and come home, so nothing
is relocated. A hub helps one hub at a time. A pair the dispatcher does
not offer is refused (`refused_not_offered`) whether or not the checker is
on, and so is a second grant to a hub already helped
(`refused_already_helped`). The lender is protected at every assignment:
it keeps half of each class idle, counting vehicles already taken in the
pass, and flies only a request it arrives at before the deadline. The
main study's `serve_from` keeps its own behavior.

With `top_kind: replanner`, the fixed rule `help_rule` grants each request
one offer, one donor per request and each donor once per call: `nearest`
takes the nearest offer; `feasible` the offer with the most aircraft to
spare over its own waiting requests, then the earliest arrival; `random`
any offer, drawn by the asking hub and the call's time, which measures
what the choice is worth once the offers are listed.

With `top_kind: model`, the model sees the new surges, outages, shortages
and weather, the requests and offers, and with `top_report: full` the hub
view (`view`); `top_report: compact` leaves the hub view out. `hide` lists
offer facts left off every offer line, to test which the decision rests
on: `spare` (the aircraft by class), `reach` (the sites each class reaches
in time), `backlog` (the hub's own waiting requests), `arrival` (the
minute its first aircraft arrives); the `spare` phrase carries `reach`, so
hiding `spare` hides both. An arm that hides a fact should also give
`prompt.system` a system prompt naming only the facts its lines carry, as
`incident-hide-16.sweep.yaml` does.

The help manager's calls and orders are counted as `top_calls` and
`top_orders`. The score counts `helps`, `hub_NNN_lent` and
`hub_NNN_helped`, and for any lending, by help or by `serve_from`, the
flights from a hub other than the request's own as `lent_flights`,
`lent_deliveries` and `lent_on_time`. With `manager.kind: policy` and the
local rules file, the hubs run local rules and the help manager grants
help between them.

Orders that asked for more than was there (a reposition with fewer idle
vehicles than `count`, a transfer with fewer units in stock) are applied for
what is there and counted as `partial_<name>`.

`feedback` reports each order's estimate when it is applied and what was flown
since the last call; `checker` refuses orders that break an active rule or
cannot be flown and says why. `cost_feedback` adds, under feedback, one fixed line
per relocation or stock flight since the last call: the donor hub's idle
vehicles now and its late deliveries since. `guard` refuses a reposition
that would leave the donor under half its vehicles of the class
(`refused_donor_thin`) and a transfer leaving it under a quarter of its
starting stock (`refused_donor_low`), which are the replanner's own keep
rules made into a rule.

A reposition with `until_min` sends the vehicles home once they are idle
after that minute; without it they stay at the new hub. The policy form
writes this as `until: event_end` (`policies/return.yaml`). `relocations`
counts every relocation flight, there and back.

The score counts `moves` (applied repositions, transfers, moves and sends),
`lends` (applied `serve_from`), and `quiet_calls` and `quiet_orders`: calls
with no new event and nothing wrong in any hub in view, and the orders given
on them, where the right reply is none.

Replies are saved under `cache_dir` by a hash of the prompt, so a rerun with
the same seed and settings makes no calls; `offline: true` uses only saved
replies. `provider`, `via`, `model`, `command` and `host` pick the client; see
`src/client.rs`.

## Score

Every number comes from the vehicles' flights, never from what the model
said. `score` is the weighted share of requests delivered on time (routine 1,
urgent 2, trauma 4). The other columns: on-time and late counts by urgency,
`missed_<reason>`, `closure_breach_s` and `corridor_breach_s`,
`weather_breaches`, `outage_breaches`, `rooftop_breaches`, `vehicles_lost`,
transfers, orders by name and outcome, and calls, prompt and reply
characters and parse failures. Per hub, `hub_001_requests`,
`_on_time`, `_late` and `_undelivered` count its own sites' requests, and
`_gave` and `_got` the relocations and stock flights it donated and
received, so a day's losses can be laid against the hubs that donated.

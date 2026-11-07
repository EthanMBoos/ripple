# Delivery: a region of blood-delivery hubs with a fleet manager

One operating day of blood delivery across a region of hubs, flown by
fixed-wing aircraft and multirotors, run by a classical dispatcher, with a
fleet manager above it that reads events and gives orders. The fleet manager
is a language model, a structured replanner, a hand-written script, or
nothing. The study's claim and results are in the paper,
[paper/manuscript/main.tex](../../paper/manuscript/main.tex); what is left
before submission is in [docs/TODO.md](../../docs/TODO.md).

## Run

```sh
python3 crates/delivery/region.py --hubs 4        # writes missions/region-4.yaml
python3 crates/delivery/region.py --hubs 16 --fixed-wing 8 --multirotor 4   # a fatter fleet
cargo build --release
./target/release/ripple run crates/delivery/missions/region-4.yaml --headless
python3 crates/delivery/results.py runs/region-4/run000
```

The missions are generated; edit `region.py`, not the YAML. `--hubs 1`,
`4`, `16`, `64`, `256` and `1024` give 5 to 5,120 vehicles, five per hub.
`--checked` uses the Rascal and Firefly instead of SimpleAircraft and point
multirotors.

For the matched grid and jitter comparison, `region.py --classical-sweeps
FOLDER` writes five matched days and their six-policy sweeps without model
calls; `--jitter-km` and `--min-spacing-km` displace the hubs, and
`--layout-seed` keys the displacement apart from the demand seed.

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
| `courier.rs` | The vehicle autonomy: flies an assignment with a skill, returns, reloads, relocates to a new hub, reports status, and is lost when its endurance runs out away from a hub. |
| `skill.rs`, `skill/` | `drop` (parachute release on a pass or hover) and `land` (multirotors only, on rooftops). Either can go out and back through one `via` waypoint for a detour. |
| `dispatcher.rs` | One per region: stock per hub, the day's requests, events, the rules in force, the fleet manager's calls, and the assignment of each request to a vehicle. Draws the hubs, the sites and the areas closed now in the viewer. |
| `event.rs` | The eight event kinds, their seeded script, and their texts from the mission. |
| `order.rs` | The orders a fleet manager can give, the queries it can ask, and the rules they leave in force. |
| `view.rs` | What the fleet manager sees: a full vehicle list, per-hub summaries, a hub list with queries, or only the exceptions. |
| `replanner.rs` | The structured replanner: fixed rules per event kind and a greedy pick of donor hubs by distance. |
| `policy.rs`, `policies/` | Rules written once as YAML, by hand or by a model, in the form `policies/SYNTAX.md` describes; `policies/replanner.yaml` is the replanner in it. |
| `model.rs`, `client.rs` | The language-model fleet manager: prompt, query loop, saved replies; clients for the `claude` and `codex` commands, the Anthropic and OpenAI APIs, and Ollama. |
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

With `manager.view: queries` the fleet manager may instead ask one of
`find_vehicles`, `hub_status` or `find_stock`, up to `max_queries` times, and
is asked again with the answer.

### The fleet manager

`manager.kind` is `none`, `scripted`, `replanner`, `policy`, `model` or
`veto`. The manager is called when events start and every `check_in_s`.
With `veto`, the replanner plans and the model is shown the proposed orders
with the situation and replies with the numbers to drop; `dropped` counts
them, and a call with nothing proposed is not made. With `policy`,
the file `manager.policy` names is read once and its rules give the orders
(`policies/SYNTAX.md`); `python3 crates/delivery/write_policy.py` asks a model
to write one from the mission's description, so the model can be in the loop
or in the code. The interface to a model is
three settings, so the study's interfaces are configurations, not code paths:

| Interface | `view` | `feedback` | `checker` |
| --- | --- | --- | --- |
| A | `full_list` | false | false |
| B | `summaries` | false | false |
| C | `queries` | false | false |
| D | `summaries` | true | false |
| E | `summaries` | true | true |
| exceptions | `exceptions`: only hubs with late or stranded requests, low stock, an outage or lost vehicles | either | either |

`manager.calls_per_hour` is a call budget that does not grow with the fleet:
calls are at least that far apart, and events that start between calls wait
for the next one, together. 0 means a call per event. It applies to the
replanner too, so the budget's cost can be measured without a model.

`manager.cluster_side` splits the region into square clusters of that many
hubs a side, each with its own fleet manager call: its own hubs' view and
events, and an order naming a hub outside the cluster is refused
(`refused_outside_cluster`). 0 is one manager over the region. The replanner
splits the same way, which measures what the split alone costs.

`manager.top_kind` puts a manager above the clusters (`replanner` or
`model`; `top_model` names its model, or `model` is used). After the cluster
managers it is called with one line per cluster, totals of vehicles, stock,
waiting and late requests and the events there, and gives only `move` (idle
vehicles between clusters) and `send` (stock) orders; the dispatcher picks
the hubs, to the hub with the most waiting from the nearest hubs that keep
half their own, or from the hub holding the most to the one holding the
least. Its calls and orders are counted as `top_calls` and `top_orders`.
With `manager.kind: none` and a top manager, the hubs run local rules only
through the dispatcher and the top moves between them.

Orders that asked for more than was there (a reposition with fewer idle
vehicles than `count`, a transfer with fewer units in stock) are applied for
what is there and counted as `partial_<name>`.

`feedback` reports each order's estimate when it is applied and what was flown
since the last call; `checker` refuses orders that break an active rule or
cannot be flown and says why. `events_as` gives the model each event as its
text or as its fields. `cost_feedback` adds, under feedback, one fixed line
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
characters, queries and parse failures. Per hub, `hub_001_requests`,
`_on_time`, `_late` and `_undelivered` count its own sites' requests, and
`_gave` and `_got` the relocations and stock flights it donated and
received, so a day's losses can be laid against the hubs that donated.

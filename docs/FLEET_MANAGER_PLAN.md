# Plan: a language-model fleet manager for medical delivery

This file records how the study changed. The proposal through
[First tests](#first-tests) is historical; the current question and results
begin at [What the directions found](#what-the-directions-found) and
[Evaluation plan for the over-help question](#evaluation-plan-for-the-over-help-question).
The working paper is [paper/manuscript/main.tex](../paper/manuscript/main.tex).

The original proposal was drafted on 2026-11-06, before the experiments.
The first probes left the fleet manager little to decide, which led to the
five trials in [Finding the problem](#finding-the-problem). The code is in
`crates/delivery`; see its [README](../crates/delivery/README.md). The
roadmap is [TODO.md](TODO.md), and the related work is in
[LITERATURE.md](LITERATURE.md).

## Summary

Language models have been proposed as high-level planners for robot teams:
the model reads the situation, decides what the team should do, and lower
layers carry it out. So far these systems have been tested on teams of 2 to
about 30 robots, usually on grid or household tasks without vehicle dynamics.
A real delivery or inspection fleet has hundreds of vehicles, and nothing is
known about what such a planner needs to know about a fleet that large to
make good decisions.

We study this question directly. A language model acts as the fleet manager
above a classical dispatcher, and we vary only the interface between them:
what the model sees of the fleet, what it is told about the orders it gives,
and whether a checker can refuse them. We run the same mission at 16, 64,
256, 1,024 and 4,096 vehicles with a fixed model and a fixed prompt, and score
each run from what the vehicles actually flew. The test problem is one
operating day of blood delivery across a region, with surges, shortages and
closures that force hubs to compete for vehicles and stock.

That a summary is shorter than a list of 4,096 vehicles is obvious and is not
the result. The result has to show which fleet information leads to better
decisions about what the vehicles fly, and where a competent classical
replanner falls short.

## Current state

Work on language models for multi-robot planning falls into three groups.
Task planners (SMART-LLM, COHERENT, Chen et al., RobotFleet, Scale-Plan) take
an instruction and produce an allocation or plan for a small team; the
largest of these tests 32 robots, and Chen et al. name context length as the
limit. Swarm work (COMPASS, GenSwarm, LLM2Swarm) reaches larger numbers, up to
1,024 robots in COMPASS, but the task is flocking or a behavior written once
before the run, on idealized point robots. The one large fleet run by a model
during operation is LLM-ODDR, which dispatches 300 taxis every minute from
summaries of 263 zones; its cars move between zones on a grid, the rules never
change, and it does not compare other ways of showing the model the fleet.

A second line of work checks what the model asks for. RoboGuard, Safety Chip,
SELP, and several 2026 verifier papers place a checker between the model and
the robots, and LLM-Modulo argues for this design in general. A checker
helping is therefore established. Separately, work on software agents
(SWE-agent, tool interface studies, context management) shows that the
interface given to a model matters as much as the model itself, but none of
it deals with physical fleets.

What has not been measured is how the interface must change as the fleet
grows, with vehicles that fly real dynamics and a mission whose rules change
during the day.

## What we investigate

We hold the model and its prompt fixed and change code. Two things vary:

- **Fleet size:** 16, 64, 256, 1,024 and 4,096 vehicles, mixed fixed-wing
  and multirotor.
- **Interface:** a full list of every vehicle; summaries per hub and region;
  query tools; feasibility and flight feedback from the vehicle models; and
  that feedback plus a checker that refuses orders that break a rule or cannot
  be flown.

The interesting cases are decisions that span hubs. When a surge drains three
hubs at once, the right answer depends on which neighbors have spare aircraft
in range with enough endurance, which a per-hub summary may hide and a query
may find. We also ask whether feedback from the checked vehicle models changes
which orders the fleet manager gives, and whether that improves what is flown.

## Why Ripple

Most simulators used for language-model robotics are either game or grid
worlds, where vehicles teleport between cells, or GPU simulators built for
training, which step thousands of copies of one simple model. Neither fits
this study. Ripple has the pieces it needs already built and tested:

- **Mixed vehicles with checked dynamics.** The Rascal fixed-wing model is
  checked against JSBSim and the Firefly multirotor against RotorS. Cheaper
  models (SimpleAircraft, point multirotors) step 4,096 vehicles in about
  3 ms, so a large fleet can run on one machine. Whether the checked dynamics
  change any decision is tested before anything else (see
  [First tests](#first-tests)).
- **Skills and a dispatcher.** The delivery crate has parachute drops,
  rooftop landings, a dispatcher that checks every order, model clients and
  saved replies, brought over from `llm-fleet` and rebuilt around hubs.
- **Grading from flight.** Every score comes from the recorded trajectories,
  so an order that sounds right but is not flown counts as not done.
- **Belief kept apart from truth.** The fleet manager sees what the system
  believes, never the true state, as in a real operation.
- **Identical runs.** The same seed gives the same output byte for byte at any
  worker count, so a difference between two runs is caused by the interface,
  not by noise in the simulator.

Speed is not the reason. Model calls take seconds and dominate the cost of a
run; the simulator only has to keep up.

## The test problem: one day of blood delivery

Blood delivery by air is a real operation with clear stakes. Zipline has
dropped blood by parachute from fixed-wing aircraft in Rwanda since 2016, and
Nisingizwe et al. (2022) measured shorter delivery times and less wastage. It
also makes a good test problem: requests arrive all day with different
deadlines, vehicles have limited range, stock is limited, and the day brings
events that a fixed dispatcher does not handle.

**Region.** Hubs sit on a square grid 15 km apart, numbered from 1. Each hub
has 16 vehicles: 10 fixed-wing aircraft that drop by parachute and 6
multirotors that drop while hovering or land on rooftops. Each hub serves 6
clinics, 4 to 13 km out, and 2 hospital rooftops 1.5 and 2.5 km out that
accept only a landing; each hospital has a ground pad 300 m away. The layout
is the same at every hub, turned by the hub number, so sites are written as
`3A` or `3H1`. Each hub holds 10 units of each blood type, gets 6 more every
hour, and a delivery takes 4 units.

**Fleet sizes.** The fleet grows by adding hubs: 1 hub (16 vehicles), 4 (64),
16 (256), 64 (1,024) and 256 (4,096). At 64 hubs the region is about 120 km
across, roughly the size of Rwanda; at 256 it is about 240 km.

**Requests.** About 6 per hub per hour, each with a site, an item and an
urgency: routine (due in 2 h), urgent (30 min) or trauma (15 min). Requests
come from random numbers keyed by hub and request index, so a seed defines one
day, the same no matter how it is served and the same at every hub count. A
day is 8 hours; the pilot uses 2.

**Events grow with the region.** The number of events per day is set per hub,
so a larger fleet faces more events, not the same few. Some events are
regional and cover several hubs at once, so the fleet manager has to trade
vehicles and stock between hubs. Without this, a 4,096-vehicle fleet would
pose the same small local problems as a 64-vehicle one, only with more to
read.

## Method

### Who does what

The **dispatcher** (`dispatcher.rs`) is classical code with no model. It
assigns each new request to the idle vehicle at the site's own hub with the
right skill, stock for the item and endurance for the round trip that arrives
soonest. It does not read events. A request with no vehicle is retried every
minute; when its deadline passes it is missed with the reason the last try
failed (no stock, grounded, no vehicle, out of range, or closed area). It
always runs.

The **couriers** (`courier.rs`) fly the assigned skill, `drop` or `land`,
each optionally out and back through one `via` waypoint for a detour, then
return and reload. Controllers fly the vehicles. A vehicle whose endurance
runs out away from a hub is lost.

The **fleet manager** is the language model. It is called when events start
and for a check-in every 30 minutes of mission time; events that start
together go in one call. It answers with orders that change how the dispatcher
and couriers behave, and it never assigns single requests.

### Events

Each day has a seeded script of events. Each event type has one fixed text,
written once in the mission file.

Regional events, which span several hubs and are the core of the study:

1. **Surge.** A mass-casualty incident sends trauma requests from hospitals
   served by several neighboring hubs, more than those hubs can fly alone.
2. **Shortage.** A blood type runs out across a group of hubs, and requests
   for it must be served from hubs that still have it, at longer range.
3. **Large closure.** An airspace closure, such as a wildfire, cuts across
   several hubs' areas until a given time, lengthening routes on both sides.
4. **Hub outage.** A hub cannot launch until a given time, and its sites must
   be covered by neighbors that are already busy.

Local events, which affect one hub or site:

5. **Weather.** Multirotors are grounded in a region until a given time.
6. **Priority change.** One hospital's requests come before routine restocks
   for the rest of the day.
7. **Rooftop closed.** A hospital rooftop is closed; only drops at its ground
   pad, 300 m away, are allowed.
8. **Corridor.** A medevac helicopter corridor between two points is active,
   and no vehicle may cross it below 150 m.

Surge, shortage and outage are facts: they happen whether or not anyone acts.
Closure, corridor, weather and rooftop closed are rules: nothing stops a
flight from breaking them, and breaches are scored from truth. Priority is a
preference, scored as the on-time share of that hospital's requests. The
number of events of each kind is set per hub per day; a scripted list can
replace the draw, and fields left out of a scripted event are filled from the
seed.

### Orders

Each order is a skill on the fleet manager's side, checked and carried out in
code (`order.rs`, applied in `dispatcher.rs`). A reply is
`{"orders": [...]}`, each order a JSON object with one key; the same forms
are written in YAML for a scripted manager.

- `reserve(hub, count, site, until_min)` keeps vehicles for one site.
- `avoid(area, until_min)` routes every new flight around a circle or
  corridor through one pushed-out waypoint; requests to sites inside wait.
- `reposition(class, count, from_hub, to_hub)` flies idle vehicles to another
  hub, which becomes their hub.
- `transfer(item, units, from_hub, to_hub)` moves stock as transfer flights
  that drop at the receiving hub; the stock arrives when they do.
- `ground(class, hubs, until_min)` stops a class, or every class, launching
  from those hubs.
- `prioritize(site or urgency, until_min)` changes the dispatcher's order.
- `serve_from(site, hubs)` lets other hubs serve a site.
- `use_pad(site, until_min)` sends a hospital's deliveries to its ground pad
  by drop instead of landing on the rooftop.

With the query interface the fleet manager may instead ask `find_vehicles`,
`hub_status` or `find_stock`, up to six times per call, and is asked again
with the answer.

### Interfaces

Each interface is a configuration of three settings on the dispatcher,
`manager.view`, `manager.feedback` and `manager.checker`, so the interfaces
are configurations, not code paths. The prompt around them is the same in all
five.

| Interface | `view` | `feedback` | `checker` |
| --- | --- | --- | --- |
| A. Full list | `full_list`: every vehicle's hub, class, state, position and endurance; stock per hub | false | false |
| B. Summaries | `summaries`: per hub and region, counts by class and state, stock, queue, late requests | false | false |
| C. Queries | `queries`: the hub list, plus `find_vehicles(near, within_min, class)`, `hub_status(hub)` and `find_stock(item, near)` | false | false |
| D. Feedback | `summaries` (or `queries`, whichever of B and C does better) | true: each order's estimate when applied, and what was flown since the last call | false |
| E. Checked | same as D | true | true: refuses orders that break an active rule or cannot be flown, with the reason |

`manager.events_as` gives the model each event as its text or as its fields,
for the structured-events comparison with the replanner.

### Baselines

- **Dispatcher only.** Events are ignored. This shows what any fleet manager
  adds.
- **Structured replanner** (`replanner.rs`). Classical code that receives
  every event, of all eight types, as structured data (area, time window,
  site, hubs, stock) and issues the same orders. It handles regional events
  with fixed rules and a greedy reassignment: move spare vehicles and stock
  from the nearest hubs with slack, within the class's range, keeping half of
  each donor's vehicles and four units of each item. At each check-in it tops
  up any hub holding fewer than four units of an item from the nearest hub
  with spare. It sees the full fleet state.

The replanner is meant to be competent, not a strawman. Against it, the fleet
manager's possible advantages are judgment on coupled decisions (which hubs to
drain, what to leave uncovered) and reading events from text. The second is
the weaker claim, so the paper reports the two separately: once with the
replanner given structured events, and once with the fleet manager given the
same structured events instead of text.

## Experiments

The experiments answer five questions:

1. On regional events, how does each interface perform, in what is flown, as
   the fleet grows from 16 to 4,096 vehicles?
2. What information does a cross-hub decision need that a summary drops, and
   do queries recover it?
3. Does feasibility feedback from the vehicle models change which orders the
   fleet manager gives, and does that improve outcomes scored on the checked
   models?
4. Once that feedback is given, does a checker still prevent breaches?
5. Where, if anywhere, does the fleet manager do better than the structured
   replanner?

The main grid is 5 fleet sizes by 5 interfaces by 10 seeds, plus the two
baselines. Question 3 is scored on the Rascal and Firefly at 16 and 64
vehicles. A second model runs a subset (2 sizes, 2 interfaces, 5 seeds) to
show the result does not depend on one model.

If the only separation between interfaces is token cost, the study stops
before the large sizes.

### Score

Everything is measured from what the fleet flew, never from what the model
said (`score.rs`):

- `score`: the share of requests delivered on time, weighted routine 1,
  urgent 2, trauma 4; with on-time and late counts by urgency;
- rule breaches: seconds inside closed airspace or a corridor, launches while
  grounded or from a hub in an outage, and landings on a closed rooftop. Only
  flights sent after the event started count: a vehicle already in the air
  when a closure begins cannot be called back, so its seconds inside are
  nobody's decision. A delivery whose flight broke a rule does not count as
  delivered (`delivered_in_breach`), so obeying the rules is never a loss on
  the score; without this the dispatcher alone, which ignores every rule,
  scored above the replanner on notice days;
- vehicles lost to endurance away from a hub;
- requests missed, by reason: no stock, grounded, no vehicle, out of range,
  closed area;
- the priority hospital's on-time share, and transfers ordered and landed;
- orders applied by name, orders refused by reason (E only), and calls,
  prompt and reply characters, queries and parse failures per run.

Response time per event was planned and dropped: in simulated time the fleet
manager answers in the step the event starts, so it is always zero.

## Keeping the prompt out of the result

The earlier study on `llm-fleet` turned into prompt engineering because the
model was the thing measured. This one fixes the model and its words:

1. One system prompt and one text per event type, written and frozen before
   the first scored run. The paper says so.
2. Every comparison varies code: fleet size, view, order replies, checker.
3. One main model and one second model on a subset.
4. No new runs to answer questions about wording.
5. A spending cap per grid, set from the pilot's measured cost.

## Cost

Calls grow with the number of events, and events now grow with the number of
hubs, so calls per run grow with the fleet. The fleet view adds to each call:

| Fleet | Full list (A), input per call | Summaries (B), input per call |
| --- | --- | --- |
| 16 | about 4K | about 4K |
| 256 | about 18K | about 6K |
| 1,024 | about 65K | about 10K |
| 4,096 | about 260K | about 20K |

All sizes fit in the main model's context. Cost per run is measured in the
pilot once the events per hub are set; saved replies make reruns free.

On the Rascal and Firefly at 500 Hz, the 4-vehicle mission on `llm-fleet`
cost about 0.8 ms per vehicle per simulated second, so an 8-hour day at 1,024
vehicles would take about 7 hours on one worker. SimpleAircraft and point
multirotors are hundreds of times cheaper. The main grid therefore runs on the
cheap models, and the checked models are used at 16 and 64 vehicles.

## First tests

These come before the main build is finished and before any grid. Each is
small and cheap.

1. **Dynamics test, no language model.** Fly the same orders (detours around
   a closure, repositions, cross-hub `serve_from`, rooftop landings) on the
   cheap models and on the Rascal and Firefly. Look for orders that are
   feasible or on time on one and not the other: turn radius on detours,
   parachute release offsets, hover endurance, a reposition that exhausts
   range. If there are such flips, feedback from the checked models has a
   real job and question 3 stands. If there are none, the checked models are
   validation only, and the paper does not claim the method needs them.
2. **One regional event at 64 vehicles, scripted.** Run a surge with the
   dispatcher only, the structured replanner, and a few hand-written order
   sets. Confirm it creates a real trade-off between hubs: some orders do
   clearly better than others, and the replanner's greedy choice is not
   always best. Done on 2026-11-06: at the planned sizes every order set
   scored the same, and the mission is being re-sized; see
   [Finding the problem](#finding-the-problem).
3. **Systems pilot.** A 2-hour day at 16 and 64 vehicles, interfaces A, B and
   D, 3 seeds. This checks that the mission, scoring, calls and costs work. It
   does not decide whether the study goes ahead.

## What is built

`crates/delivery` holds the whole study problem; the engine is untouched. The
crate's [README](../crates/delivery/README.md) says how to run it.

- `region.py` writes a mission for any hub count: one `region` entity with
  the `Dispatcher`, and per hub a group of 3 fixed-wing aircraft and one of
  2 multirotors (the re-sized fleet; `--fixed-wing` and `--multirotor`
  change it) with the `Courier` autonomy. The system prompt and the eight
  event texts live in the script and are copied into every mission, so they
  are written once.
- `Dispatcher`, `Courier` and `DeliveryScore` are registered from the
  crate's `register`, called in `crates/cli/src/main.rs`. Every setting is a
  YAML field under `entities.region.autonomy.Dispatcher`, so a comparison is
  a command-line override: `manager.kind`, `manager.view`,
  `manager.feedback`, `manager.checker`, `events.scripted`,
  `manager.scripted`, `manager.model`, `manager.offline`.
- The six manager kinds: `none` (dispatcher only), `scripted` (orders at
  minutes, for hand-written order sets), `replanner`, `policy` (rules
  written once as YAML, by hand or by a model; direction 3), `model`, and
  `veto` (the replanner plans and the model drops orders; evaluation E).
  The model client (`client.rs`) reaches the `claude` or `codex` command,
  the Anthropic or OpenAI API, or Ollama, and saves every reply under
  `runs/llm-cache` by a
  hash of the prompt, so reruns are free and `offline: true` replays.
- `tests/region.rs` runs short days of the generated missions: a 1-hub day
  delivers everything and is identical at 1 and 2 workers; an `avoid` order
  cuts closure breaches against the dispatcher alone; the replanner answers a
  surge with repositions and `serve_from`; the replanner written as a policy
  gives the same 4-hour day, value for value; a fake model client asks a
  query, gives an order, and replays offline.

Not built yet: the dynamics test on `region-1-checked.yaml`, the Rerun view
of hubs, sites and areas, and any run with a live model. Events, orders and
scoring were tried at 4 hubs with scripted events: a closure, a surge and a
shortage, each with the dispatcher alone and with the replanner. The
replanner's `avoid` removed every attributable breach second, and its
repositions and `serve_from` improved a surge slightly; the gap to the
dispatcher alone is small on an 8-hour day because stock, not vehicles, is
what runs out, which is why restocks were made hourly.

### Decisions made while building

Each was a small call made without a review; any of them can be changed.

- Transfers are flights by the fleet's own vehicles, dropped at the receiving
  hub, so they cost vehicles and range like deliveries.
- A reposition changes the vehicle's hub; it does not come back.
- A request to a site inside an avoided area waits rather than flying through
  it, and is missed as `closed` if the area outlasts its deadline.
- `use_pad` was added for the rooftop-closed event; the earlier `drop_via`
  skill became a `via` waypoint on `drop` and `land`.
- A surge is 16 trauma requests per hub over 15 minutes, half to hospitals
  and half to clinics, so neighbours with only fixed-wing spare can help.
- The structured replanner tops up low stock at every check-in, so the model
  has to beat routine stock management as well as event handling.
- Site layout is fixed per hub and turned by the hub number, so a region is
  described by its hub count and spacing alone.

### Open questions for review

- Should the dispatcher alone restock between hubs on a quiet day, as the
  replanner now does, so the baseline is fair on stock?
- At 4 hubs a scripted surge at one hub separates the replanner from the
  dispatcher alone by about 0.015 in score. First test 2 asks for a clear
  trade-off; the surge may need to cover two or three hubs, or the day more
  requests, before the fleet manager is tested on it.
- The structured-events comparison gives the model each event's fields; it
  has not been run.

Design work happens at 16 and 64 vehicles, where runs are cheap and fast. The
larger sizes run only once the design is settled and the prompt is frozen, so
256, 1,024 and 4,096 vehicles also test whether a design worked out on small
fleets holds on large ones.

## Finding the problem

Probes on 2026-11-06 ran a 2-hour day at 4 hubs with a surge over hubs 1
and 2, comparing the dispatcher alone, the replanner and three hand-written
order sets (cross-hub `serve_from` only; one aircraft moved to each surge hub;
two aircraft and a multirotor moved to each). Two things came out.

With 16 vehicles per hub and 6 requests per hub per hour, hub stock is the
only thing that runs out. `serve_from` alone gives the whole replanner gain
and moving aircraft changes nothing, even at 60 requests per hub per hour
once stock is ample, where the dispatcher alone delivers everything on time.
Sixteen vehicles per hub is far more capacity than the mission uses.

With 5 vehicles per hub (3 fixed-wing, 2 multirotors), 12 requests per hub
per hour and ample stock, the order sets spread by more than 0.1 and the best
one changes with the day. But the replanner averages 0.89 over three seeds
against 0.83 for doing nothing, and Haiku and Sonnet both scored about 0.77,
below the replanner, with mistakes of the coupled kind: moving multirotors
between the two surge hubs, moving multirotors that cannot reach the far
clinics, and naming a site that does not exist because the summaries never
list site codes.

The reason is that everything in the mission is modelled: eight event kinds
with a rule each, and a dispatcher that does the local optimisation well. A
short greedy rule then takes nearly all the available gain, and scale only
adds more of the same small local problems. A model earns its place only
where the classical layer has no model, or where the way it is called breaks
with size. The five directions below are each of one of those two kinds. All
five are to be tried, in the order given, on the re-sized mission; each has a
cheap test that can kill it before any model run, and the one with the
clearest finding becomes the paper, with the others as secondary results or
dropped. The outcome of each is recorded in the table at the end.

### Re-sizing

Before any direction: about 5 vehicles per hub (3 fixed-wing, 2 multirotors)
instead of 16, 12 requests per hub per hour instead of 6, and stock that is
not the first wall (100 units of each item, 30 more an hour). The fleet
ladder becomes 4, 16, 64, 256 and 1,024 hubs, so 20 to 5,120 vehicles. The
region line of the prompt names the site code pattern, since no view lists
sites. These are the settings the probes found room at; they are confirmed on
three seeds at 4 and 16 hubs before the directions are tried.

### 1. Notices with consequences

Replace part of the event script with operational notices that imply
something later: a stadium final tonight, a strike closing road ambulances, a
heat warning, a bridge out so a clinic can take only rooftop drops. The
mission holds the scripted consequence of each (a trauma surge at the
hospitals near the stadium two hours later, with a probability), and the
notice corpus is written once and frozen. The replanner cannot act on a
notice without a hand-written rule per notice, so the baselines are a
reactive replanner, which acts when the consequence arrives, and an oracle
replanner, which is given the consequence as structured data in advance. A
model lands between them, and its edge is world knowledge turned into
pre-positioning across hubs, which is coupled by construction.

Code (built, then removed on 2026-11-06 after the cheap test parked the
direction): `events.notices` with text, consequence, delay and probability,
a `notice` event kind, and `manager.events_as: oracle`, which replaced a
notice by the facts it foretells (surge, shortage, outage) when the notice
arrived; rules that follow a notice were given when they started. The corpus
of ten notices was in `region.py` behind `--notices`. Cheap test: ten
notices, reactive against oracle at 4 and 16 hubs; if the gap is small,
anticipation is worth little and the direction is dropped. Finding: where
the model sits between reactive and oracle, and how that changes with hubs.

### 2. Exceptions and a call budget

In simulation a call costs no mission time, so a call per event is free at
any size, which hides the real pressure of scale. Give the fleet manager a
budget of calls per mission hour that does not grow with the fleet. At 4
hubs that is a call per event; at 1,024 hubs it is one call per hundred
events, so the interface has to condense. Add an interface that shows only
exceptions: requests about to be missed, hubs below reserve, stuck vehicles,
breaches pending. This is how control rooms work, and it is code.

Code (built): `manager.calls_per_hour`, which also batches the events that
start between calls; `manager.view: exceptions`. Cheap test: the
replanner called on the budgeted schedule against the unbudgeted replanner,
at 4 and 64 hubs, to see that the budget bites at all. Finding: score against
fleet size under a fixed budget, one line per interface; the lines should
bend, which makes the scale axis do real work.

### 3. In the loop or in the code

Reframe around one question: should the model run the fleet, or write the
rules once? Three arms: the model in the loop (the interfaces above), the
model writing the replanner once from the mission description, and the
hand-written replanner. Scale then tests a sharp claim: in-the-loop cost
grows with the fleet and its decisions degrade, while a written policy costs
nothing per vehicle and holds, but cannot act on a notice it was not written
for (direction 1). This is the framing a reviewer cannot answer with "why not
an optimiser", because the written-policy arm is that answer.

Code (built): `manager.kind: policy` reads the file `manager.policy` names,
the replanner's own structure as data (`policy.rs`, the language in
`policies/SYNTAX.md`): one rule per event kind and one for check-ins, each a
list of steps whose hubs, sites, items and amounts are filled from the event,
with the replanner's own "nearest" and "spare" helpers. The replanner written
in it (`policies/replanner.yaml`) gives the same day, value for value, which
an integration test checks. `write_policy.py` asks a model to write one once
from the mission's system prompt, the event texts and the syntax, with
nothing about the fleet's state. The fuller form, Python through the plugin
bridge on the `python-plugins` branch, is not built. Cheap test: the
written policies of Haiku and Sonnet against the replanner on the same
4-hour days as directions 2 and 4. Finding: in-the-loop against written
policy against hand-written, across sizes, with and without notices.

### 4. Hierarchy

One fleet manager over everything against one per cluster of hubs with a thin
manager above, which sees cluster summaries and can move vehicles and stock
between clusters. This is the natural answer to scale and it is code. What the
top manager needs to see of the clusters is a cleaner version of the original
interface question.

Code (built): `manager.cluster_side` splits the region into square
clusters, each called with its own hubs' view and events, and an order
naming a hub outside the cluster is refused. `manager.top_kind` puts a
manager above them (`replanner` or `model`), called after the cluster
managers with one line of totals per cluster, which gives only `move` and
`send` orders between clusters; the dispatcher picks the hubs. Cheap test: at
16 hubs, the replanner split per cluster against the flat replanner, to see
whether the split itself loses anything. Finding: flat against hierarchy
across sizes; a crossover is a result in its own right.

### 5. What breaks first

Whatever else is found, measure the failure modes against size: infeasible
orders, orders to the wrong hub, orders about vehicles that moved since the
view was written, parse failures, checker refusals. The interface that breaks
last is a result even if no interface beats the replanner, and it keeps the
paper about the interface rather than the model.

Code (built): `partial_<name>` counts orders that asked for more than was
there (fewer idle vehicles or units than named), applied for what was there;
`refused_<reason>` counts the rest, including unknown hubs, sites and items
and `outside_cluster`. A stale-view count was planned and dropped: the view
and the orders are written in the same step, so nothing can go stale between
them. No cheap test; it rides on every run above.

### Outcomes

| Direction | Cheap test | Result | Finding size | Decision |
| --- | --- | --- | --- | --- |
| Re-sizing | 3 seeds at 4 and 16 hubs | Done 2026-11-06: order sets spread 0.73 to 0.87 at 4 hubs and 0.90 to 0.97 at 16 on seeds 1 and 2; seed 3 has little room at either | about 0.1 | Keep |
| 1. Notices | reactive vs oracle | Done 2026-11-06: on 4-hour days with the ten notices, the oracle gains 0.03, 0.03 and loses 0.06 at 4 hubs, gains 0.01, 0.02 and loses 0.02 at 16, with breached deliveries voided | about 0.00 on average | Park: foreknowledge is worth little as the replanner uses it (acts at once); a just-in-time use would be a replanner change, and a model's use may differ |
| 2. Budget and exceptions | budgeted vs free replanner | Done 2026-11-06: at 4 hubs a budget of 2 then 1 calls an hour costs the replanner 0.04 and 0.04, 0.05 and 0.11 on two seeds, and breached deliveries rise from 3 to 7 to 10; at 16 hubs the flat replanner is no better than the dispatcher alone (0.68 vs 0.69, 0.71 vs 0.71), so the budget shows nothing there | 0.05 to 0.1 at 4 hubs | Keep as the scale pressure; the exceptions view is untested with a model |
| 3. In the loop or in the code | policy form expresses the replanner | Done 2026-11-06: the replanner written as a policy gives the same day, value for value. Haiku and Sonnet each wrote a policy once from the mission description; both parsed and ran with no refusals at 4 hubs and 1 to 2 at 16. Both are the replanner with more stock moved (a transfer of O- from two donors on every surge, two donors on a shortage) and score 0.83 and 0.76 (Haiku), 0.83 and 0.68 (Sonnet) at 4 hubs against the replanner's 0.88 and 0.78, and 0.65 and 0.70, 0.61 and 0.70 at 16 against 0.68 and 0.71, with three to four times the transfers and more late deliveries | 0.0 to 0.1 below the replanner | Keep as the in-the-code arm: a model can write a policy that runs all day for one call, and the difference from the hand-written one is in how much it moves, which is the same mistake the in-the-loop runs made |
| 4. Hierarchy | split vs flat replanner, then a manager above the clusters | Done 2026-11-06, 3 seeds at 4, 16 and 64 hubs. One manager per hub, which can only apply local rules, scores 0.74 to 0.79 at 16 hubs and 0.70 to 0.75 at 64, against 0.68 to 0.71 and 0.66 to 0.69 for the flat replanner, which is no better than the dispatcher alone there; at 4 hubs flat wins on two seeds of three. A replanner above the clusters makes things worse again at 16 and 64 hubs (0.69 to 0.76, 0.67 to 0.74) and a replanner above the dispatcher alone adds nothing. Haiku and Sonnet above per-hub replanners at 16 hubs score 0.64 and 0.66, 0.64 and 0.72, giving 83 to 133 orders against the replanner's 24 to 31, with 32 to 60 refused and the most late deliveries of any arm | 0.05 to 0.08 for local rules; every cross-hub move costs | Keep the split; the finding is that help between hubs costs more than it returns at 16 hubs and up, and every manager that can move things, classical or model, moves too much |
| 5. What breaks first | none | Done 2026-11-06 for the classical managers: no parse failures or lost vehicles in any run; the flat replanner at 16 hubs has 2 and 3 repositions refused for too few idle vehicles and none at 4 hubs; the per-hub managers refuse nothing and, with no neighbour in view, give no reposition, transfer or `serve_from` at all, only the local rules (avoid, ground, prioritize, use_pad), and still beat the flat replanner | none yet; the counts need a model in the loop | Keep the counts; they ride on every run |

### What the directions found

The hierarchy sweep (direction 4) and a decomposition of the replanner's
help, run 2026-11-06 on 4-hour days at the designed event density, three
seeds each. The decomposition uses the policy form (direction 3): `local`
keeps only the rules that touch one hub (avoid, ground, prioritize,
use_pad); `serve` adds serving a surge, shortage or outage hub's sites from
its nearest hub, which lends flights that return home; `move` adds instead
the replanner's repositions, transfers and check-in top-ups, which relocate
vehicles for good and spend flights on stock.

| Hubs | none | flat replanner | local | serve | move |
| --- | --- | --- | --- | --- | --- |
| 4 | 0.80 / 0.69 / 0.83 | 0.88 / 0.78 / 0.87 | 0.83 / 0.78 / 0.84 | 0.88 / 0.76 / 0.87 | 0.83 / 0.72 / 0.84 |
| 16 | 0.69 / 0.71 / 0.70 | 0.68 / 0.71 / 0.69 | 0.74 / 0.79 / 0.76 | 0.74 / 0.77 / 0.78 | 0.68 / 0.71 / 0.67 |
| 64 | 0.69 / 0.63 / 0.68 | 0.69 / 0.66 / 0.69 | 0.75 / 0.70 / 0.74 | 0.78 / 0.71 / 0.77 | 0.67 / 0.62 / 0.62 |

- The local rules are worth 0.05 to 0.08 at every size: they are what the
  dispatcher alone cannot do, and `local` is the per-hub manager of the
  hierarchy sweep exactly.
- Lending flights (`serve`) change little at 16 hubs and add 0.01 to 0.03 at
  64, where a hub in a surge has more neighbours to lend.
- Relocating vehicles and stock (`move`) costs 0.06 at 16 hubs and 0.08 to
  0.12 at 64, enough to undo the local rules and fall below the dispatcher
  alone. The donor hubs are left thin for the rest of the day, and their
  own deliveries go late: 536 to 650 late at 64 hubs against 484 to 610
  with local rules. At 4 hubs a surge covers three of the four hubs, there
  is little to lend, and the moves still pay a little.
- Every manager with a model in it moved more than the replanner and
  scored lower: in the loop (interface D at 4 hubs, earlier), the written
  policies (three to four times the transfers), and above the clusters
  (three to five times the orders, a third to half refused).

The paper question this leaves is sharper than the interface ladder alone:
help between hubs has a cost that grows with the region, a classical
replanner and a language model both pay it, and the experiment is which
interface lets a model see that cost and stop moving. The baselines are
`local` (do nothing between hubs) and `serve` (lend, never relocate); the
interfaces are the ladder of the Method section plus the budget and the
hierarchy as scale pressures. A model that matches `serve` at 64 hubs is a
result; one that beats it would have to find help the rules miss.

## Evaluation plan for the over-help question

The claim to test: a language model fleet manager acts when it should not,
and relocating is the action that costs; the question is what, if anything,
in the interface gets it to hold back. Each evaluation below names the code
it needs, its cheap test, and what it would show. They run on 4-hour days,
three seeds, at 16 and 64 hubs unless said otherwise, with the `local` and
`serve` policies as the bar and the replanner as the classical comparison.
Numbers that need a model use Haiku and Sonnet first; the others are
classical and free.

### A. Measure the bias, not just the score

Code: three counts in the score. `moves` (applied repositions, transfers,
moves and sends), `lends` (applied serve_from), and `quiet_calls` with
`quiet_orders`: calls with no new event and no exception in view, and the
orders given on them. A quiet call is the cleanest test of a bias for
replying with something, since the right reply is nothing. Cheap test: the
runs already made, re-read; then one Haiku run per hub at 16 hubs. Shows:
how much of the model's harm is action on quiet calls against wrong action
on real events, which decides whether the fix is "show the cost" or "show
that nothing is wrong".

### B. The interface ladder, aimed at the cost

Code: none for B, D and E; the exceptions view is built. Cheap test: a model
per hub at 16 hubs, two seeds, on summaries (B), summaries with feedback
(D), feedback with the checker (E), and the exceptions view with feedback.
Shows: whether seeing each order's outcome and what was flown since is
enough for the model to stop relocating, or whether only a refusal does.
The feedback already reports arrivals and late deliveries since the last
call; it does not say which hub went late after which move.

### C. Feedback that names the cost

Code: one more line of feedback per applied move, written by the
dispatcher: the donor hub's idle count after the move and its late
deliveries since, so the move's cost is in the next prompt as a fact. This
is interface design, not prompt wording: the sentence is fixed and the
numbers come from the state. Cheap test: as B, on D. Shows: whether a model
told the cost of its last move makes fewer; if not, the bias is not about
information.

### D. The checker as a guardrail

Code: a checker rule that refuses a reposition or move leaving the donor
below half its vehicles of the class, which is the replanner's own keep
rule, and a transfer leaving it below a quarter of its stock. Cheap test:
as B, on E, plus the replanner with the same checker. Shows: how much of
the gap closes by refusal alone. If all of it, the model adds nothing a
rule does not; if the model still finds harmful moves inside the rule, the
bias is not only about thresholds.

### E. The veto interface

Code: a manager kind where the replanner plans and the model reviews: it
sees the situation and the replanner's orders, and replies with the ones to
drop. Cheap test: 16 hubs, two seeds, Haiku and Sonnet. Shows: the bias in
the other direction. A model that approves everything is placating; one
that drops the relocations and keeps the local rules would beat the
replanner with one call per event and no orders of its own, which is a
different and cheaper way to use a model.

### F. Policy revision from outcomes

Code: `write_policy.py` gains a second round: the model is shown its
policy's day in numbers (score, late by hub class, moves and what followed
them) and asked to revise once. Cheap test: Haiku and Sonnet, one revision,
at 16 and 64 hubs. Shows: whether a model given the cost in aggregate, with
time to think and no live pressure, converges on `serve`; this is the
in-the-code arm of direction 3 under the new question.

Result 2026-11-06: neither did. Shown a day at 16 and 64 hubs with its
score, late count, moves and lends, Haiku kept every move and only made its
repositions return home and smaller; Sonnet halved its moves (24 against 40
at 16 hubs, 101 against 180 at 64) and added "trauma first" to every rule.
The revisions score 0.66 / 0.71 / 0.73 (Haiku) and 0.66 / 0.71 / 0.72
(Sonnet) at 16 hubs and 0.72 / 0.65 / 0.71 and 0.73 / 0.65 / 0.73 at 64,
against 0.74 / 0.79 / 0.76 and 0.75 / 0.70 / 0.74 for local rules alone:
a gain of 0.01 to 0.05 over their first policies, still 0.02 to 0.08 under
doing nothing between hubs. Both still transfer stock from two donors on
every surge.

### G. Where moves start to pay

Code: none; mission settings. Cheap test, classical only: the replanner,
`local` and `serve` at 8 and 12 vehicles per hub, and a reposition that
returns home when the event ends (one courier change: a relocation with an
end time). Shows: the boundary of the finding. If moves pay with a fatter
fleet or a return trip, the paper says where help costs rather than that it
always does, and the return-trip order becomes the thing a model should
prefer.

Result 2026-11-06 (three seeds, mean score):

| Hubs | Vehicles per hub | none | replanner | local | serve | return |
| --- | --- | --- | --- | --- | --- | --- |
| 16 | 5 | 0.70 | 0.69 | 0.76 | 0.77 | 0.74 |
| 16 | 8 | 0.80 | 0.83 | 0.87 | 0.88 | 0.84 |
| 16 | 12 | 0.83 | 0.90 | 0.90 | 0.91 | 0.90 |
| 64 | 5 | 0.67 | 0.68 | 0.73 | 0.75 | 0.70 |
| 64 | 8 | 0.79 | 0.83 | 0.85 | 0.87 | 0.86 |
| 64 | 12 | 0.83 | 0.88 | 0.89 | 0.90 | 0.89 |

Relocating never pays at any fleet size tried: the replanner's gap to local
rules closes from 0.07 at 5 vehicles a hub to 0.01 at 12, and never turns
positive. Repositions that fly home recover about half the gap at 5 and 8
a hub. Lending is the best arm at every size and both region sizes, by
0.01 to 0.03 over local rules once the fleet has anything to lend. So the
finding is not an artefact of a starved fleet: it weakens with more
vehicles but does not reverse, and the right order is the one whose cost
ends with the event.

### H. Models

Code: none; the clients exist. Cheap test: the best interface from B to E
at 16 hubs with Haiku, Sonnet, Opus and one OpenAI model through the codex
client, two seeds, under a call budget so the cost is bounded. Shows:
whether restraint comes with model size, which a reviewer will ask.

### Order

A first (it changes how the rest is read), then B, then C and D together,
then E, F, G, H. A to D are a day of runs; E and F are a day of code each;
G is classical and runs overnight.

### Results of A to E (2026-11-06, 16 hubs, seeds 1 and 2)

One manager over the region, 26 calls a day. The bar is local rules alone
at 0.74 / 0.79 and lending at 0.74 / 0.77; the flat replanner scores
0.68 / 0.71 and with the guard 0.72 / 0.75. Score, then relocations and
stock flights made (`moves`), then lent flights (`lends`):

| Interface | Haiku | Sonnet |
| --- | --- | --- |
| B summaries | 0.67 / 0.67, moves 43 / 37, lends 43 / 54 | 0.66 / 0.67, moves 48 / 35, lends 49 / 52 |
| D + feedback | 0.67 / 0.73, moves 38 / 22, lends 20 / 32 | 0.65 / 0.71, moves 46 / 32, lends 8 / 48 |
| E + checker | 0.67 / 0.77, moves 38 / 17, lends 20 / 63 | 0.66 / 0.70, moves 45 / 28, lends 11 / 66 |
| X exceptions + feedback | 0.73 / 0.75, moves 3 / 1, lends 24 / 12 | 0.67 / 0.68, moves 29 / 14, lends 12 / 22 |
| Dc + cost line | 0.67 / 0.69, moves 26 / 25, lends 24 / 30 | 0.72 / 0.65, moves 31 / 44, lends 57 / 44 |
| Eg + guard | 0.70 / 0.75, moves 12 / 5, lends 43 / 118 | 0.70 / 0.68, moves 27 / 18, lends 6 / 35 |
| veto over the replanner | 0.72 / 0.74, kept moves 9 / 15 of 12 / 15, lends 72 / 80 of 144 / 120 | 0.67 / 0.76, kept moves 14 / 14, lends 48 / 80 |

- A: there were no quiet calls. At this event density some hub is always
  late or short when the manager is called, so the bias shows as action on
  real events, not on empty ones. A quieter day is needed to measure the
  empty-call reply (added to H).
- B: no interface reaches the local-rules bar on both seeds. Feedback and
  the checker cut Haiku's moves by half on one seed and not the other, and
  the score follows the moves.
- C: the cost line does not cut moves (Haiku 26 / 25 against 38 / 22
  without it; Sonnet 31 / 44 against 46 / 32). Told what its last move cost
  the donor, the model moves again. The bias is not a lack of information.
- D: the guard cuts moves most (Haiku 12 / 5) and lifts Haiku to 0.70 /
  0.75, near the guarded replanner's 0.72 / 0.75; so the guard, not the
  model, is doing the work.
- The exceptions view is the one interface that stops Haiku moving
  (3 and 1 moves) and brings it to 0.73 / 0.75, within 0.01 to 0.04 of the
  bar: shown less, it does less. Sonnet still moves under it.
- E: the veto drops the wrong half. Both models keep most of the
  replanner's relocations and drop half its lent flights, which are the
  free ones; the score still rises 0.03 to 0.05 over the replanner because
  the lends it drops cost nothing. Given a list, the model prunes the long
  one.
- Parse failures: 0 to 1 per run; refusals under the checker 1 to 25.

### Results of H and the quiet day (2026-11-06, 16 hubs, seeds 1 and 2)

Opus and gpt-6-luna on the two interfaces above, against Haiku and Sonnet:

| Interface | Haiku | Sonnet | Opus | gpt-6-luna |
| --- | --- | --- | --- | --- |
| X exceptions + feedback | 0.73 / 0.75, moves 3 / 1 | 0.67 / 0.68, moves 29 / 14 | 0.67 / 0.68, moves 42 / 33 | 0.71 / 0.74, moves 2 / 0 |
| veto over the replanner | 0.72 / 0.74, kept moves 9 / 15 | 0.67 / 0.76, kept 14 / 14 | 0.73 / 0.74, kept 10 / 16 | 0.73 / 0.75, kept 6 / 1 |

Restraint does not come with model size. Opus moves the most of the four
under the exceptions view and scores the least; gpt-6-luna moves the
least and, as a veto, is the best model arm at 0.73 / 0.75, which is level
with local rules on one seed and 0.04 under on the other. No model arm
beats the local-rules bar on both seeds.

The quiet day, at a quarter of the event density: the dispatcher alone
scores 0.90 / 0.88 and local rules 0.91 / 0.91. Haiku and Sonnet with
feedback score 0.89 / 0.88 and 0.89 / 0.87, under no manager at all, giving
19 to 62 orders a day where local rules give 5 or 6, and 7 to 34 moves.
Even then only 1 or 2 of 12 calls were quiet by the strict count (some
request is almost always late somewhere), and on those the models gave 1
and 3 orders. The bias shows most plainly here: on a day with little to do,
a model manager does more and costs more than none.

## Out of scope

- Communication loss and delay. Links are perfect, as on `llm-fleet`; a later
  version can add SphereNetwork range and loss.
- Wind beyond the weather event, terrain, and hardware.
- Wording comparisons and more than two models.
- One fleet manager per hub. A hierarchy over clusters of hubs is direction
  4 in [Finding the problem](#finding-the-problem); a manager per hub is not.

## Earlier paper shape (superseded)

This outline predates the over-help result. The current structure is in
[paper/manuscript/main.tex](../paper/manuscript/main.tex).

- **Figure 1:** the region in Rerun with hundreds of tracks, and a surge
  pulling aircraft from neighboring hubs around a closure.
- **Main result:** on-time deliveries on regional events against fleet size,
  one line per interface, with both baselines.
- **Second result:** orders that change when the fleet manager gets feedback
  from the checked vehicle models, and what that does to flown outcomes.
- **Third result:** the fleet manager against the structured replanner, with
  text and structured events reported separately.
- **Venue:** RA-L, ICRA or IROS. Cite COMPASS and LLM-ODDR and say plainly how
  this differs.

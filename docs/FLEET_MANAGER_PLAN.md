# Plan: a language-model fleet manager for medical delivery

Draft for review, 2026-11-06. Nothing here is built. The roadmap entry is in
[TODO.md](TODO.md) and the related work is in [LITERATURE.md](LITERATURE.md).

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
- **Skills and a dispatcher.** The delivery crate on `llm-fleet` has
  parachute drops, rooftop landings, a dispatcher that checks every call,
  model clients and saved replies.
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

**Region.** Hubs sit on a square grid about 15 km apart. Each hub has 16
vehicles: 10 fixed-wing aircraft that drop by parachute and 6 multirotors that
drop while hovering or land on rooftops. Each hub serves about 6 clinics, 3 to
13 km out, and 2 hospital rooftops that accept only a landing. Each hub holds
a limited stock of each blood type, restocked on a schedule.

**Fleet sizes.** The fleet grows by adding hubs: 1 hub (16 vehicles), 4 (64),
16 (256), 64 (1,024) and 256 (4,096). At 64 hubs the region is about 120 km
across, roughly the size of Rwanda; at 256 it is about 240 km.

**Requests.** About 6 per hub per hour, each with a site, an item and an
urgency: routine (due in 2 h), urgent (30 min) or trauma (15 min). Requests
come from random numbers keyed by hub and request index, so a seed defines one
day, the same no matter how it is served. A day is 8 hours; the pilot uses 2.

**Events grow with the region.** The number of events per day is set per hub,
so a larger fleet faces more events, not the same few. Some events are
regional and cover several hubs at once, so the fleet manager has to trade
vehicles and stock between hubs. Without this, a 4,096-vehicle fleet would
pose the same small local problems as a 64-vehicle one, only with more to
read.

## Method

### Who does what

The **dispatcher** is classical code with no model. It assigns each new
request to the nearest idle vehicle at the site's hub with the right skill,
stock for the item, and enough endurance for the round trip. This is
`reference_plan` from `llm-fleet`, extended to many hubs, and it always runs.

The **couriers** fly the assigned skill (`drop`, `land` or `drop_via`),
return and reload. Controllers fly the vehicles.

The **fleet manager** is the language model. It is called when events arrive
and for a check-in every 30 minutes of mission time; events that arrive
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

Some types may need a new order or parameter. That is decided per type while
building, and the fleet manager and the replanner always get the same orders.

### Orders

Each order is a skill on the fleet manager's side, checked and carried out in
code:

- `reserve(hub, vehicles, site, until)` keeps vehicles for one site.
- `avoid(area, until)` makes every route plan around a circle or corridor.
- `reposition(class, count, from_hub, to_hub)` moves idle vehicles.
- `transfer(item, units, from_hub, to_hub)` moves stock between hubs.
- `ground(class, region, until)` stops a vehicle class from launching.
- `prioritize(site or urgency, until)` changes the dispatcher's order.
- `serve_from(site, hubs)` lets other hubs serve a site.

### Interfaces

Each interface is a configuration of the fleet manager's inputs and of the
replies its orders return. The prompt around them is the same in all five.

| Interface | Fleet view | Order replies | Checker |
| --- | --- | --- | --- |
| A. Full list | Every vehicle: hub, class, state, position, endurance; stock per hub | "ok" | No |
| B. Summaries | Per hub and region: counts by class and state, stock, queue, late requests | "ok" | No |
| C. Queries | Hub list, plus `find_vehicles(near, within_min, class)`, `hub_status(hub)` and `find_stock(item, near)` | "ok" | No |
| D. Feedback | The better of B and C | Feasibility and arrival estimates from the vehicle models; after each event, what was flown | No |
| E. Checked | Same as D | Same as D | Refuses orders that break an active rule or cannot be flown, with the reason |

### Baselines

- **Dispatcher only.** Events are ignored. This shows what any fleet manager
  adds.
- **Structured replanner.** Classical code that receives every event, of all
  eight types, as structured data (area, time window, site, hubs, stock) and
  issues the same orders. It handles regional events with fixed rules and a
  greedy reassignment: move spare vehicles and stock from the nearest hubs
  with slack, by flight time. It sees the full fleet state.

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
said:

- deliveries on time, by urgency, with trauma and urgent weighted most;
- rule breaches: seconds inside closed airspace or a corridor, launches while
  grounded, and deliveries to a closed rooftop;
- vehicles lost to endurance away from a hub;
- requests that failed for lack of stock;
- response time per event, from the event to the first flight that follows
  it;
- refused orders by reason (E only), and model calls and tokens per run.

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
   always best. If every reasonable order scores the same, change the event
   before going further.
3. **Systems pilot.** A 2-hour day at 16 and 64 vehicles, interfaces A, B and
   D, 3 seeds. This checks that the mission, scoring, calls and costs work. It
   does not decide whether the study goes ahead.

## Build steps

1. **Bring back the delivery crate** from `llm-fleet`: skills, couriers, call
   checks, scoring, model clients and saved replies. Leave the wording sweeps
   and paper scripts on the branch.
2. **Dynamics test** (first test 1), using the existing skills.
3. **Many hubs:** hub grid, sites and stock per hub, a request generator
   keyed by hub, and a dispatcher across hubs. Check that a 1-hub day behaves
   like the old mission.
4. **Events and orders:** the event script with events per hub, the order
   skills and their effect on the dispatcher and couriers. Test each order
   with a scripted fleet manager and no model.
5. **Structured replanner** for all eight event types, then first test 2.
6. **Fleet manager and the five interfaces.** Iterate at 16 and 64 vehicles,
   then freeze the prompt.
7. **Scoring** for breaches, stock failures, response time, calls and tokens.
8. **Viewer:** hubs, sites, closed areas, corridors and the fleet, for the
   video. Inspect screenshots from an RRD export, as AGENTS.md requires.

Design work happens at 16 and 64 vehicles, where runs are cheap and fast. The
larger sizes run only once the design is settled and the prompt is frozen, so
256, 1,024 and 4,096 vehicles also test whether a design worked out on small
fleets holds on large ones.

## Out of scope

- Communication loss and delay. Links are perfect, as on `llm-fleet`; a later
  version can add SphereNetwork range and loss.
- Wind beyond the weather event, terrain, and hardware.
- Wording comparisons and more than two models.
- One fleet manager per hub. A single fleet manager comes first; splitting it
  is a possible later interface.

## Paper shape

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

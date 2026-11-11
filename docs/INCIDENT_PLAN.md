# A day where the fleet decision matters

The fleet API question has two halves: is there a fleet decision worth
making, and which report and orders let a model make it as the fleet
grows. The ladder of 2026-11-10 tested the second half on a day where the
first is false. On the main day help touches about 50 of 7,280 requests,
the nearest donor is always the right one, and a fixed rule is already as
good as any choice. This plan builds one incident where the regional
decision is consequential, proves that with fixed policies on incidents
the policies were not tuned on, and only then asks a model to make it. No
model call is spent before the gate below passes.

## Why the main day gives nothing to decide

Hubs are 15 km apart and a trauma request has 15 minutes. A fixed-wing
aircraft covers 15 km in 10 minutes, so a lent flight reaches almost none
of the asking hub's sites in time. A surge is 16 requests in 15 minutes at
one hub, over before a second call. Every hub has the same three
fixed-wing and two multirotors, so no donor is better placed than the
nearest. One surge at a time means donors are never contested. Each of
these is a setting of `region.yaml`, except the fleet, which is the same
at every hub by construction.

## What a lent aircraft can do

Only a multirotor lands, so a hospital rooftop is served by the asking
hub's own two multirotors, or by a lent fixed-wing drop at its ground pad
after a use-pad order. Lent fixed-wing aircraft otherwise serve clinics,
4 to 13 km from the asking hub, so at 6 to 8 km spacing the clinics on
the donor's side are a few minutes' flight away. The incident is built on
clinic deliveries by lent fixed-wing aircraft. Two rules stand in the way
and change first:

- An offer requires a class to reach the asking hub's farthest site and
  return, which rules out every multirotor and some fixed-wing donors
  that could serve the near clinics. An offer should ask whether the class
  reaches any of the asking hub's sites; the assignment already refuses
  each flight it cannot fly.
- A scripted surge naming one hub is filled out to a neighborhood. The
  incident names its hubs explicitly, in pairs where it means pairs.

A grant answers the request, so one asking hub gets one donor. The plan
keeps that: the decision is which donor, not how many.

## The incident

One 16-hub block, the main day's requests and stock, and a scripted set of
events in place of the random ones:

- Hubs 6 to 8 km apart, set once and frozen.
- Two or three surges at once, lasting an hour, at hubs that share
  neighbors: the surge window at 60 minutes with 48 requests per hub.
  Sustained demand makes a lend last long enough to count, and shared
  neighbors make the donors contested.
- Donors that differ in what they can give. Two hubs start with five
  fixed-wing aircraft and two with one, so the nearest donor is not the
  best one, and the donors' own backlogs differ when the surges start.
  Per-hub fleet sizes are the one setting the dispatcher cannot express
  today, since `fleet.per_hub` is one number for every hub.
- A cost to lending that is visible when the choice is made: a donor with
  a backlog of its own loses deliveries it would have flown. A donor's own
  later surge is not visible to anyone at decision time, so a rule that
  uses it is an oracle, kept as a ceiling and labeled as one.

Events are announced when they start, as now.

## The gate: fixed policies must separate on incidents they were not tuned on

Four fixed arms run the incident, all with the local rules at every hub:

- Local rules alone, no help.
- Nearest donor, as the broker does now.
- The feasible rule: from the same offers the report shows, the donor
  whose aircraft arrive before the deadline with the most spare after the
  lend and the smallest backlog of its own, one donor per asking hub.
- The oracle: the feasible rule with knowledge of every event to come,
  so it never lends from a hub about to be hit. A ceiling, not a target.

Tuning the incident until the arms separate is development, and the gap it
produces proves nothing. Once the incident separates them on the seeds it
was built with, it is frozen, and the gate is measured on new seeds and
new hub placements, five of each. The decision is worth making if the
feasible rule beats nearest donor by at least three points of the block's
weighted on-time score there. Donor misses are reported, not gated: a good
allocation may cost a donor routine deliveries to save trauma deliveries
elsewhere, and that is the trade-off a manager is for. If the feasible
rule and nearest donor land within a point on held-out incidents, the
fleet decision is not worth a manager on this kind of day, and that is
the finding.

Reported for each arm: score, on-time trauma deliveries at the asking
hubs, misses at the donor hubs, lent flights and how many arrived on time,
aircraft lost, and how far each arm sits between nearest donor and the
oracle.

## Then the models, small first

At 16 hubs on the held-out incidents, Haiku and Opus, each with the help
order and nothing else, local rules below them, the same call schedule as
the fixed arms:

- The full report: the hub view of every hub, the requests, and the offers
  with spare aircraft, backlog, distance, and arrival time at the asking
  hub's near clinics.
- The compact report: the requests and the offers only, with the same
  columns, no hub view.

The measure is where each model lands between nearest donor and the
feasible rule, and whether the two reports differ. A model that reaches
the feasible rule from the compact report has an API that can grow. A
model that needs the full report has one that cannot.

## Only then, the size

The scaling pressure is the number of simultaneous incidents, which grows
with the fleet: four at 64 hubs and sixteen at 256, one per 16-hub block,
overlapping in time. Both reports grow, the full one with every hub and
the compact one with the open requests and offers, so the comparison is
about what the report contains and not only its length. Staggering the
incidents to hold the decision rate constant would test prompt length
alone, which is the weaker question. The two reports run with the model
that did better at 16 hubs. The 1,024 size is not planned.

## What this needs

- Per-hub fleet sizes in the dispatcher's config, with a check that the
  list has one entry per hub.
- The offer rule by any reachable site, and a backlog and an arrival time
  in each offer line; the assignment already computes the arrival time
  when it checks a lent flight against the deadline.
- The feasible rule and the oracle as fixed help policies beside the
  broker in `help.rs`, the oracle reading the mission's scripted events.
- A compact report for the help manager, requests and offers without the
  hub view.
- An incident mission beside `region.yaml`, scripted events only, and a
  sweep for the gate over new seeds and placements. The sweep's header
  states the question, not the outcome.

The finished ladder sweeps stay as evidence for the paper's present claim
that local rules do most of the work on the main day. The Fleet API
section stays Planned until the gate passes.

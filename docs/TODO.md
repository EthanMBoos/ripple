# What is left

## The paper

The paper asks how a language-model manager handles competing needs across
a fleet, and what it should see and be allowed to order as the fleet grows.
On the main study's eight-hour days, every model tried moves aircraft or
stock in at least one role, including when it only vetoes a replanner's
orders or writes rules before the day. Local rules and lending flights do
better: moving to a stressed hub costs the giving hubs more requests than
it saves elsewhere. A reserve guard limits the moves directly. These
findings are written up in
[paper/manuscript/main.tex](../paper/manuscript/main.tex), with the policy
table at 4 to 1,024 hubs, forty model runs, the loss figure, the interface
ladder, and the surge-length check.

Before submitting:

- Update Figure 2 with Luna's OpenAI API results (Ezra): recover the
  eight-hour per-hub results behind the pooled bars, replace Luna's CLI
  contribution, regenerate against matched local-rules runs, recheck the
  caption and the donor-loss discussion, and remove the caption's TODO.
- Commit and push the current appendix PDF so the paper's repository link
  opens the version described.
- Record the demo video from the title-figure recording.
- Run `reference/suite.py --release --retain` so `paper/data/` reflects
  the current engine.
- Explain in the setup why identical hubs isolate the cost of moving
  capacity. Add a service-radius comparison only if the cited medical-drone
  operation supports one.
- Add a demand-sized replanner as a robustness row: it sizes each
  reposition from the backlog in view instead of a fixed four fixed-wing
  and two multirotors per surge hub. Run it at 4, 16 and 64 hubs with no
  model calls, larger only if those change the picture. Say in the setup
  why the primary rule sizes by the event: the surge's requests are not
  yet visible when it is announced. If this rule also loses at giving
  hubs, the claim is stronger; if it wins, the claim becomes that
  over-moving costs, not moving.

Submit to RA-L in the first days of December. RA-L takes six pages
including references, two more at a charge, nothing beyond eight; the
paper is seven. Check the author page for current terms first.

## The fleet API, still to do

What we want to show is plain: an API that lets a model manage a small
fleet, where it strains as the fleet grows, and a change to the API that
lets it keep working at the larger size. The fleet API is the report the
model sees and the orders the dispatcher accepts.

On the main day there is no fleet decision for a model to make. Help
touches about 50 of 7,280 requests, the nearest donor is always the right
one, and a fixed rule is as good as any choice; a whole-region model never
beats local rules at any size, and bounding it to help makes it harmless
and equal to the fixed rule. The plan in [INCIDENT_PLAN.md](INCIDENT_PLAN.md)
builds a day where the decision matters first, proves that with fixed
policies on incidents they were not tuned on, and only then compares a
full report with a compact one, small before large. Its steps, in order:

- The offer rule by any reachable site, a backlog and an arrival time in
  each offer line, and a scripted surge that names its hubs exactly.
- Per-hub fleet sizes in the dispatcher's config.
- The feasible help rule and the oracle beside the broker in
  `crates/delivery/src/help.rs`, and a compact report for the help
  manager, requests and offers without the hub view.
- The incident mission beside `region.yaml`, tuned on its build seeds,
  then frozen; the gate sweep over new seeds and placements. The sweep's
  header states the question, not the outcome.
- Models at 16 hubs only after the gate passes; size after that. No model
  call before the gate.

Known from the no-model runs, and settings the incident changes: at 15 km
spacing and a 15-minute trauma deadline a lent flight reaches almost
nothing in time; a 15-minute surge is over before a second call; equal
fleets make the nearest donor the best; one surge at a time leaves donors
uncontested. Help is on time by construction: a lender keeps half of each
class idle and flies only what arrives by the deadline, and a pair the
dispatcher did not offer is refused.

If the gate misses submission, the Fleet API section moves to future work
and the introduction's question narrows; no planned comparison is
presented as a result.

## Loose ends, not needed for submission

- A model on the long-surge day: the surge-length line ran fixed
  policies only, so whether a model also wins when a surge lasts an hour
  is untested.
- A few hubs busy all day beside quiet ones, the other way a move could
  pay. Needs a per-hub request rate in the mission, not written.

## Pointers

The study code is `crates/delivery` and its
[README](../crates/delivery/README.md); the related work is
[LITERATURE.md](LITERATURE.md). The earlier study, on which mistakes a
language-model planner makes, is on the `llm-fleet` branch with its own
manuscript.

## Not planned

Runtime plugin loading, legacy protobuf/string-map spawning, OpenCL, VTK,
ROS, JSBSim. ArduPilot and Burn stay optional, out of the default build.
Out of the study's scope: communication loss and delay, wind beyond the
weather event, terrain, hardware, wording comparisons, and one manager per
hub as an arm of its own.

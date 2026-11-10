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

## Scaling, still to do

What we want to show is plain: an API that lets a model manage a small
fleet, where it strains as the fleet grows, and a change to the API that
lets it keep working at the larger size. The fleet API is the report the
model sees and the orders the dispatcher accepts.

Run one ladder on the eight-hour day at 16, 64 and 256 hubs, and 1,024 if
the runs fit, Haiku and Opus, three seeds, against local rules and the
fixed broker at the same size:

- One manager over the whole region with the main study's orders.
- One manager over the whole region allowed only temporary help: a hub
  serves another hub's sites for a time and its aircraft come home,
  chosen from the hubs the dispatcher lists as able to spare aircraft and
  reach the sites, or nothing.
- A manager per 16-hub cluster, each seeing and ordering within its
  cluster, with a manager above them allowed only the same help.

If the first arm falls behind as the fleet grows and the second holds, the
orders were the problem. If the second falls too and the third holds,
dividing the view and the decisions earned its place. Report score, misses
at lending and receiving hubs, aircraft lost, calls, prompt characters and
cost per day for each arm. The sweep's rationale states the question, not
the expected outcome.

Before the ladder: give the whole-region manager the help order, which
means raising requests and listing offers without clusters, and protect
lenders. On the current day the fixed broker (`crates/delivery/src/help.rs`)
moves misses from the asking hub to the lending hub rather than removing
them; keeping half of the lender's aircraft is not enough. Reread the no-model 256-hub comparison after the change.

Known from the no-model runs: temporary help lands few on-time deliveries
on this day because hubs are 15 km apart and the trauma deadline is 15
minutes, permanent moves between clusters lose aircraft even under a fixed
rule, and an hour-long surge at four times the requests collapses the day
for every arm. A workload where cross-cluster help pays is a follow-up,
not a condition for the ladder.

Every manager is called three times an hour with events batched, about 23
calls a day. A 256-hub cluster run takes about an hour and a half; a
1,024-hub day needs 1.5 GB and some seven hours with calls. Not doing:
calling the clusters in parallel inside the dispatcher. If the ladder
misses submission, move the Fleet API section to future work and narrow
the introduction's question; do not present a planned comparison as a
result.

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

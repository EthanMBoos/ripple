# What is left

## The paper

The paper asks how a language-model manager handles competing needs across
a fleet, and what it should see and be allowed to order as the fleet grows.
On the main study's eight-hour days, every model tried moves aircraft or
stock in at least one role, including when it only vetoes a replanner's
orders or writes rules before the day. Local rules and lending flights do
better: moving to a stressed hub costs the giving hubs more requests than
it saves elsewhere. A reserve guard limits the moves directly. These are
the completed findings in
[paper/manuscript/main.tex](../paper/manuscript/main.tex).

The main comparisons use matched requests and events on eight-hour days,
with half an hour to finish the last deliveries. Robustness checks change
the layout, fleet, event rate, or surge. The policy table at 4 to 1,024
hubs, forty model runs, loss figure, and interface ladder are written up.
The surge-length check finds that moving helps on a quiet day with one
surge lasting one or two hours, especially if aircraft come home. At four
hours the advantage disappears. The main study has 15-minute surges.

Before submitting:

- Update Figure 2 with Luna's OpenAI API results (Ezra). The API reruns
  are complete; the local manuscript and appendix now use the ten matched
  runs (three seeds at 16 hubs, two at 64, both roles). The figure still
  uses Luna's original CLI runs. Recover the original eight-hour per-hub
  results used for the pooled bars, replace Luna's contribution with the
  API results, and regenerate the figure against matched Local-rules runs.
  Recheck its caption and donor-loss discussion, then remove the caption's
  TODO. The API-versus-CLI comparison is in the appendix.
- Commit and push the current appendix PDF so the public repository link
  in the paper opens the version described there.
- Record the demo video from the title-figure recording.
- Run `reference/suite.py --release --retain` so `paper/data/` reflects
  the current engine.
- Explain in the setup why identical hubs help isolate the cost of moving
  capacity. Check whether the cited medical-drone operation supports a
  service-radius comparison; add one only if it does.

Submit to RA-L in the first days of December. RA-L takes six pages
including references, two more at a charge, nothing beyond eight; the
paper is seven. Check the author page for current terms first.

## Scaling, still to do

The paper's second question is how to give a manager enough information and
authority to handle competing needs when the fleet reaches thousands of
aircraft. The fleet API is the report the model sees and the orders the
dispatcher accepts. Compare three APIs at 256 and 1,024 hubs on matched
days:

- One manager over the whole region, shown the exceptions report for
  every hub, giving the eight hub orders anywhere. This keeps the view
  and order set of the 16- and 64-hub runs, while the report and the
  reach of each order grow. The call schedule is different (below).
- One manager per cluster of 16 hubs, 16 or 64 of them. Each is shown
  its own cluster's exceptions and events. Its orders affect only its
  cluster's sites and aircraft; cross-cluster orders are refused. The
  report stays the size of a 16-hub day, but a cluster cannot move stock
  or aircraft from another cluster on its own.
- The cluster managers plus a manager above them, shown each cluster's
  totals and allowed two orders of its own, move aircraft and send stock
  between clusters. The above manager is the replanner, a deterministic
  reference, or the same model. This puts region-wide moves back, but
  only through a thin second layer.

Haiku and Opus run all of it, two seeds at each size. Calls are budgeted
at three an hour, with events since the last call batched, so every
manager is asked about 26 times a day. A 1,024-hub day has about 2,900
events; a call at each event would mean about 190,000 calls for a cluster
API. Report score, missed requests at giving and receiving hubs, moves
per hundred hubs, calls, and input tokens for each API, against the fixed
policies at the same size. Test whether one model can use the region-wide
report, whether cluster managers leave needs unmet across boundaries, and
whether the regional layer helps more than it costs at giving hubs.

The fixed-policy half is done and in the paper's policy table: the cost
of relocating levels off at 13 points from 64 hubs on, and so do the
moves per hundred hubs.

The model half is the `region-256-clusters` and `region-1024-clusters`
sweep files, sixteen runs each, about 12,800 Opus calls and as many
Haiku calls in all. The cluster boundary is not yet what the list above
claims, so this comes first:

- Show a cluster manager only its own cluster: filter the ongoing events
  and the orders in force by cluster as the new events already are,
  write each event's text again after its hub list is trimmed, and keep
  the order feedback and delivery tally per cluster.
- Scope every order. Orders naming a site go through the site's recorded
  hub, and serve-from checks both the site and every named hub. A cluster
  manager's avoid rule applies only to flights by aircraft based at its
  hubs, even when the avoided area lies between hubs; a flat manager's
  avoid rule remains region-wide. Store the issuing cluster with the rule
  and select it by aircraft hub when choosing a route. Do not test
  whether a hub lies inside the area: the 6 km closure circle can lie
  between hubs 15 km apart. Refuse urgency-only prioritize from a cluster
  manager. List only available orders and their scope in each prompt.
- Call the clusters in parallel, a few at a time, with replies kept in
  cluster order so a day replays the same.
- State in the paper that the regional move keeps half of each donor
  hub's aircraft and is permanent, as the replanner's repositions are,
  while a flat manager's reposition has no floor unless the guard is on.

Then check a parallel day matches a serial one byte for byte and a rerun
makes no calls, run one 256-hub Haiku day and read its prompts and orders
by hand, then the 256-hub file, then 1,024. These runs answer the paper's
second question, currently marked Planned. If they miss submission, move
the Fleet API section to future work and narrow the introduction's question
accordingly; do not present a planned comparison as a result.

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

# What is left

## The paper

The paper asks how a language-model manager handles competing needs across
a fleet, and what it should see and be allowed to order as the fleet grows.
On the main study's 8.5-hour missions, every tested model moves aircraft
or stock in at least one role, including when it only vetoes a replanner's
orders or writes rules before the day. Local rules and lending flights do
better: moving to a stressed hub costs the giving hubs more requests than
it saves elsewhere. A reserve guard limits the moves directly. These
findings are written up in
[paper/manuscript/main.tex](../paper/manuscript/main.tex), with the policy
table at 4 to 1,024 hubs, forty model runs, the loss figure, the fleet API
study, and the surge-length check.

Before submitting:

- Update Figure 2 with Luna's OpenAI API results (Ezra): recover the
  eight-hour per-hub results behind the pooled bars, replace Luna's CLI
  contribution, regenerate against matched local-rules runs, recheck the
  caption and the donor-loss discussion, and remove the caption's TODO.
- Try a Figure 1 variant that keeps the flight maps but uses the right
  panel's lower callouts for the fleet API result: the compact report
  shows requests and feasible lenders, and random < nearest < spare
  rule ≈ models. Render both versions at page size and keep the clearer
  one. Add 256-hub numbers only after the final rerun. If the lending
  mission is still hard to picture, try a small map of its 16-hub block.
- Commit and push the current appendix PDF so the paper's repository link
  opens the version described.
- Record the demo video from the title-figure recording.
- Run `reference/suite.py --release --retain` so `paper/data/` reflects
  the current engine.
- Explain in the setup why identical hubs isolate the cost of moving
  aircraft and stock. Add a service-radius comparison only if the cited
  medical-drone operation supports one.
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
last render was seven. Re-render after these edits and check the author
page for current terms.

## The fleet API

The fleet API is the report the model sees and the orders the dispatcher
accepts. The main mission gives a model little choice about lending: the
deadline is short and neighboring hubs have equal fleets. The
`incident.yaml` mission changes the spacing, demand, and fleet sizes so
several hubs can ask for help from different lenders. Fixed rules first
show that the choice affects deliveries on held-out days. Models then
receive the same help order while we vary the report and fleet size.

Results in hand: the rule that favors lenders with aircraft to spare
over their own backlog beats the nearest-lender rule by 1.9 points at
16 hubs and about 2.5 at 64. The first ten 256-hub fixed-rule days show
a similar gap. Haiku and Opus score within 0.3 points of the spare-aware
rule on ten held-out 16-hub days and ten fresh 64-hub days. A full hub
report uses more prompt characters without a consistent score gain over
the compact report in paired runs. The four-day 64-hub pilot suggested
a gap that ten fresh days did not confirm. This supports the compact
report for this lending decision through 64 hubs; it does not establish
a fleet API for every kind of order.

Still to do:

- Finish the 256-hub fixed-rule days and trace the aircraft lost on three
  help-rule days. Report the loss rather than calling checked help
  harmless.
- Run Haiku and Opus at 256 hubs on matched held-out days with the
  compact report. A claim comparing report choices at 256 also needs
  full-report runs there. Record score, misses at lending hubs, lent
  deliveries, aircraft lost, calls, and prompt size.
- Update the paper's table and text from completed runs. Keep unrun
  comparisons marked Planned.

The mission and sweep files are in `crates/delivery/missions/incident*`;
the help order and report fields are in the
[delivery README](../crates/delivery/README.md).

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

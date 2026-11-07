# Roadmap

## What we have

A fleet manager that helps one hub by moving aircraft or stock there
makes the hub that gave lose more than the hub in trouble gains. The
manager cannot see that loss when it decides. Every language model we
tried makes the move anyway. So the right policy is local rules and lent
flights, and a model is safest as a veto or as the author of the rules.
That is the finding, shown at 16 and 64 hubs, and it is in the
manuscript, [paper/manuscript/main.tex](../paper/manuscript/main.tex).
It is enough for a paper.

## What we are after

The finding leaves one question open: is over-helping a property of the
model, or of what the model is shown? A live manager over 16 hubs made
few moves. Over 64 it made dozens, because each call showed it dozens of
problems. Size is what separates the two explanations, so the second
finding needs a region too large for any flat manager: 256 and 1,024
hubs, 1,280 and 5,120 aircraft. There the model must manage a cluster of
16 hubs, with a manager above the clusters.

- If a cluster manager behaves like the 16-hub manager did, over-helping
  was the view's fault, and small views fix it.
- If the manager above the clusters moves aircraft between clusters and
  the loss lands at the clusters that gave, over-helping is what a model
  does whenever it can move things, at any level and any size. At 16
  hubs a replanner above clusters already made things worse, so this is
  the likelier outcome.

Either answer is a second claim with the same shape as the first: where
help should come from at scale. Two smaller results come with it. The
cost of relocating should be a straight line in region size, since per
hundred of the giving hubs' own requests it was 16 at 16 hubs and 17 at
64 and the total grew only because more hubs gave. And the model's roles
should sort by size: the live manager worse, the veto a few points under
local rules, a policy file written once unaffected. If they do, the
recommendation is plain: at scale, let the model write the rules, not
give the orders.

A 1,024-hub region is the 16-hub problem tiled 64 times, and the first
finding is local, so the big runs do not show it again and the paper must
not claim they do. They show only what size can: the cost curve, each
interface as its view grows, and the hierarchy. The sizes go up by four
because the region is a square grid. 1,024 hubs is the top; anything
larger is no operation that will exist (decided 2026-11-07).

## The plan and the exit gate

Try for the second finding. Keep the first one ready to ship at every
step. The first finding needs the eight items under Either way; those
take a few days and are needed whichever paper goes, so they come first
or alongside. The second finding needs phases B and C below, about two
weeks, before anyone knows whether it is there.

The gate is 2026-11-21. On that date, one of two things is true:

- Phases B and C are done and the hierarchy runs say one clear thing on
  both seeds, in either direction. Then fold the second finding in,
  finish D and E, and submit to RA-L in January or straight to IROS by
  2027-03-01.
- Anything else: a 1,024-hub day does not run or costs more than an hour,
  the cost curve at 256 and 1,024 hubs says nothing new, the hierarchy
  runs disagree between seeds, or the work does not feel right. Then stop
  the scaling work where it is, note what was measured, and submit the
  first finding to RA-L in the first days of December. Nothing from the
  scaling work is wasted; it becomes the next paper.

There is no third outcome where the first paper waits on the second. The
December date holds because the first finding is complete today.

On the venues: a December RA-L submission can also be presented at IROS
2027 if accepted before 2027-04-30, which is possible, not certain, and
the journal paper counts either way. RA-L takes six pages including
references, two more at a charge, nothing beyond eight; the paper is
seven. Check the RA-L author page for the current terms before
submitting.

## Either way

Needed for the first finding and the second alike.

1. Show when moving pays. Write a short plan, then sweep with the
   classical policies only until the move policy beats local rules on
   some days: surge length or size, and a demand imbalance across hubs (a
   few busy hubs among quiet ones, or two hub types). Without this, a
   reviewer can say the surges are too short for relocating ever to pay,
   or that a uniform grid never offers the quiet-hub-next-to-busy-hub
   case where relocation should pay, so any mover loses and the models
   only inherited that.
2. Run the four models as live managers and as vetoes on those days, same
   seeds. If they move when it pays and still move when it does not, the
   finding is that managers cannot price the downstream cost of help. If
   they adapt, say so.
3. Add seeds: more for the classical table at both sizes, and one more
   64-hub seed for the model runs if the budget allows.
4. Rewrite the opening of the system section so Ripple is the instrument
   and the over-helping finding is the contribution, as the title and
   abstract already say.
5. Defend the grid. The world is a square grid of identical hubs so that
   a loss can be attributed to the move and not to the hub; say so in the
   setup, and say the world is a test instance, not a region. Add one
   table row with the hubs jittered off the grid under a minimum spacing
   (a small change to `region.py`), same seeds and policies, so the
   result does not rest on the lattice. Cite one real medical-drone
   operation for range, speed and service radius and say in a sentence
   whether the grid's 15 km spacing and 4 to 13 km site distances sit
   inside those figures. Terrain, wind and airspace stay out: closures
   and corridors are events, and the paper says simulation only.
6. Make the appendix reachable. The paper cites `paper/appendix/` by its
   repository URL, so the repository must be public by submission, or the
   appendix PDF hosted somewhere else and the footnote changed.
7. Record the demo video from the figure-1 recording; a video is the one
   extra RA-L accepts.
8. Run `reference/suite.py --release --retain` so `paper/data/` reflects
   the current engine.

## The second finding, phase by phase

A. Done. Timing (2026-11-07, release build, no manager, one worker): a
   64-hub day takes about 3 minutes, a 256-hub day about 16 minutes, and
   a 1,024-hub day about 2 hours. Each run writes a big file (5 GB,
   23 GB, 90 GB) that we delete once we have the summary. With 110 GB
   free we can run three 256-hub days at a time, or one 1,024-hub day.

B. The classical policies at scale. Two days. Thirty simulated days,
   about sixteen hours of compute, run eight at a time in an afternoon;
   the rest is seeds, analysis and the figure.
   Run none, local, serve, move and the replanner at 256 and 1,024 hubs on
   three seeds. The question is whether the cost of relocating keeps
   growing with the region (0.07 at 16 hubs, 0.09 at 64) and whether the
   loss still lands at the hubs that gave. `loss_figure.py` and
   `results.py` take any size.
C. The model at scale. Five to seven days, most of it waiting on calls.
   At 1,024 hubs a day is 64 clusters times about 26 calls, some 1,700
   calls and about two million input tokens. Through the command-line
   client one call after another is hours a day, so the cluster calls
   have to go in parallel first (one to two days of code); check that the
   reply cache keeps replays free.
   A flat manager cannot read 1,024 hubs: the exceptions view already
   fails for every model at 64, and the full list is about 260K tokens a
   call at 4,096 vehicles. The hierarchy is built (`cluster_side`,
   `top_kind`, `top_model`): a manager per cluster of 16 hubs, 64 of them
   at 1,024 hubs, each a model call, and one manager above them that
   moves between clusters. Run Haiku and one stronger model as cluster
   managers with local rules and lending allowed, with and without a top
   manager, on two seeds at each size. This phase answers the question
   under What we are after.
D. Interfaces by size. Two days; eight new runs, the 16- and 64-hub ones
   exist. The veto and the policy file against the live manager at 16,
   64, 256 and 1,024 hubs. The policy file is written once and costs
   nothing to run at any size; the veto prunes a replanner list that
   grows with the region. One model, two seeds per size.
E. The paper. Four to five days.
   New: the engine table (wall time, memory and record size per simulated
   day by size and worker count), the classical table at four sizes, the
   cost-of-relocating curve by size, the hierarchy result, and the
   interface-by-size table. Figure 2 becomes one panel per size. The
   system section then earns its place: matched days, one seed, the same
   record at every worker count, and a day at 1,024 hubs in the time it
   takes.

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

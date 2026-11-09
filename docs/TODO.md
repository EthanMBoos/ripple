# What is left

## The paper

On the main study's eight-hour days, moving aircraft and stock to a
stressed hub costs the giving hubs more requests than it saves elsewhere.
Local rules and lending flights do better. Every model tried chooses moves
in at least one role, including when it only vetoes a replanner's orders or
writes rules before the day. Neither role reaches local rules. A reserve
guard limits the moves directly. That is the finding in
[paper/manuscript/main.tex](../paper/manuscript/main.tex).

The main comparisons use matched requests and events on eight-hour days,
with half an hour to finish the last deliveries. Robustness checks change
the layout, fleet, event rate, or surge. The policy table at 4 to 1,024
hubs, forty model runs, loss figure, and interface ladder are written up.
The surge-length check finds that moving helps on a quiet day with one
surge lasting one or two hours, especially if aircraft come home. At four
hours the advantage disappears. The main study has 15-minute surges.

Before submitting:

- Rerun GPT-6 Luna through the OpenAI API instead of the codex command
  (Ezra). The codex command keeps a coding agent's tools around the
  model; the claude command runs with its tools removed, so the two
  wrappers differ and the paper's limits paragraph has to say so. Change
  `via: cli` to `via: api` for the Luna cases in the two model sweep
  files and run them with an OpenAI key; the API client has not been
  used live yet, so the first run is the test. Ten runs, five live and
  five veto. Then Luna's numbers move in the model table, the appendix,
  and the loss figure, and the limits paragraph says both paths were one
  plain call with the same system prompt.
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

The next question is whether a smaller view still reduces moves in a
large region. At 16 hubs the exceptions view held Haiku to a few moves,
but Sonnet still made dozens; at 64 every tested model moved. At 256 and
1,024 hubs, give each model a cluster of 16 hubs, with a manager above
the clusters. The cluster runs test whether a small view cuts moves at
scale. The upper manager tests whether permission to move capacity
between clusters brings the donor-hub losses back.

The fixed-policy half is done and in the paper's policy table: the cost
of relocating levels off at 13 points from 64 hubs on, and so do the
moves per hundred hubs.

The model half is not started. It needs code first: the cluster managers'
order set enforced (the cluster scope refuses orders naming hubs outside
the cluster but still accepts relocations within it), the cluster calls
made in parallel (a 1,024-hub day is 64 clusters times about 26 calls),
and a check that the reply cache keeps replays free. Then Haiku and one
stronger model as cluster managers, with and without a manager above, two
seeds at each size. Five to seven days, most of it waiting on calls. The
result fills the paper's fourth question, which the manuscript already
asks and marks Planned. If the runs are not done by the submission date,
the paper goes with the first finding and the stub.

Also wanted for the fourth question: one 1,024-hub day run alone on a
free machine, with the command, worker count and wall time written down,
so the paper can say how long a day at that size takes. A 256-hub day
takes about 16 minutes on one worker; a 1,024-hub day took two to six
hours with three sharing the machine.

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

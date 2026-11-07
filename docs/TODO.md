# Roadmap

## The claim

**A language-model fleet manager over-helps.** It relocates vehicles and
stock when the best policy is to set local rules and lend flights, and the
cost of relocating grows with the region. Telling the model the cost does
not stop it; an exceptions-only view holds one model back at 16 hubs and
no model at 64; a donor guard refuses the moves; given a planner's list to
prune, every model drops the free lent flights and keeps the costly moves.
Over five seeds at 16 and 64 hubs the classical table holds (lending, then
local rules, then the replanner, then no manager), and 39 of the 40 model
runs on the exceptions view and the veto score under local rules. The one
that beat them, Opus with the exceptions view at 16 hubs on one seed,
scored lowest in the study on its other four days, so a model's wins do
not repeat. Four models were tested as live managers and as vetoes; two
also wrote policies.

Everything below serves that claim. The test problem is one day of blood
delivery across 16 to 64 hubs with surges, shortages and closures, a
classical dispatcher under the model, and a score taken from what was flown.
The model and its prompt are fixed; what varies is how the model is used (in
the loop, as a veto over a planner, or writing the rules once) and what it
is shown. The study code and its method are in `crates/delivery` and its
[README](../crates/delivery/README.md); the working paper is
[paper/manuscript/main.tex](../paper/manuscript/main.tex); the related work
is [LITERATURE.md](LITERATURE.md).

How it got here, all on 2026-11-06: the mission was re-sized until the
fleet decision mattered; five directions were tried (notices, a call budget
with an exceptions view, the model writing the rules once, a hierarchy of
managers, failure counts) and the decomposition of the replanner's help
showed that relocating is the whole loss while local rules and lending are
the whole gain; evaluations of every interface and model against that bar
followed, then the firm-up below. The earlier result tables are in the
manuscript. The earlier study, which asked which mistakes a language-model
planner makes, is on the `llm-fleet` branch with its manuscript; it turned
into prompt engineering because the model was what it measured.

## Results

The firm-up of 2026-11-07, now in the manuscript's tables: the classical
table at five seeds and the two model arms that mattered, the exceptions
view with feedback (X) and the veto over the replanner, for Haiku, Sonnet,
Opus and gpt-6-luna on a third seed at 16 hubs and on seeds 1 and 2 at 64
hubs. Scores per seed, then the mean:

| Hubs | none | replanner | guarded replanner | local | serve | move | return |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 16 | 0.69 / 0.71 / 0.70 / 0.67 / 0.65 (0.68) | 0.68 / 0.71 / 0.69 / 0.69 / 0.74 (0.70) | 0.70 / 0.73 (0.71, seeds 4 and 5) | 0.74 / 0.79 / 0.76 / 0.73 / 0.71 (0.75) | 0.74 / 0.77 / 0.78 / 0.72 / 0.76 (0.76) | 0.68 / 0.70 / 0.67 / 0.67 / 0.67 (0.68) | 0.72 / 0.74 / 0.76 / 0.70 / 0.75 (0.73) |
| 64 | 0.69 / 0.63 / 0.68 / 0.70 / 0.67 (0.67) | 0.69 / 0.66 / 0.69 / 0.70 / 0.69 (0.69) | 0.73 / 0.72 (0.73, seeds 4 and 5) | 0.75 / 0.70 / 0.74 / 0.75 / 0.73 (0.73) | 0.78 / 0.71 / 0.76 / 0.74 / 0.76 (0.75) | 0.67 / 0.62 / 0.62 / 0.68 / 0.63 (0.64) | 0.71 / 0.67 / 0.72 / 0.71 / 0.70 (0.70) |

- The order holds at both sizes: lending, then local rules, then the
  replanner with returns or with the guard (returns ahead at 16 hubs, the
  guard at 64), then the replanner, then no manager, then relocating
  without lending. Local rules beat the replanner on every seed but one (16
  hubs, seed 5, by 0.02 the other way); `move` is the worst arm on eight
  seeds of ten.
- One three-seed statement changes: at 16 hubs the replanner now edges
  the dispatcher alone (0.70 against 0.68) instead of trailing it. At both
  sizes the replanner, which lends and relocates, lands between the two:
  above no manager, below local rules.
- The cost of relocating is local minus move: 0.07 at 16 hubs and 0.09 at
  64, as on three seeds. Lending over local rules is 0.01 at 16 and 0.02 at
  64.

The model arms, score then relocations and stock flights made (`moves`),
then lent flights (`lends`); for the veto, moves kept of the replanner's
proposals. The bar is local rules: 0.76 at 16 hubs seed 3, 0.75 / 0.70 at
64 hubs seeds 1 / 2 (lending 0.78, 0.78 / 0.71). The 16-hub seed 1 and 2
rows are in the manuscript's earlier tables.

| Arm | Haiku | Sonnet | Opus | gpt-6-luna |
| --- | --- | --- | --- | --- |
| X, 16 hubs seed 3 | 0.76, moves 4, lends 18 | 0.68, moves 22, lends 59 | 0.80, moves 58, lends 124 | 0.68, moves 1, lends 101 |
| veto, 16 hubs seed 3 | 0.73, kept moves 18 of 28, lends 78 of 160 | 0.74, kept 21, lends 116 | 0.74, kept 23, lends 80 | 0.73, kept 9, lends 43 |
| X, 64 hubs seeds 1 / 2 | 0.70 / 0.66, moves 43 / 24, lends 43 / 95 | 0.70 / 0.57, moves 128 / 117, lends 96 / 188 | 0.63 / 0.54, moves 325 / 267, lends 391 / 302 | 0.66 / 0.60, moves 34 / 5, lends 597 / 908 |
| veto, 64 hubs seeds 1 / 2 | 0.71 / 0.65, kept moves 79 / 60 of 143 / 92, lends 448 / 352 of 584 / 640 | 0.72 / 0.68, kept 90 / 64, lends 265 / 328 | 0.74 / 0.67, kept 101 / 64, lends 288 / 416 | 0.72 / 0.68, kept 41 / 68, lends 328 / 456 |

- The gap does not close at 64 hubs. No model arm reaches local rules
  there on either seed; the nearest is Opus as a veto at 0.01 under on
  seed 1, and the mean over the eight 64-hub veto runs is 0.03 under.
- The exceptions view fails at 64 hubs for every model. It held Haiku to
  3 and 1 moves at 16 hubs; at 64 Haiku makes 43 and 24, Sonnet 128 and
  117, and Opus 325 and 267 for the two lowest scores in the study, 0.63
  and 0.54, with six hours of vehicle time inside closed areas on seed 1.
  gpt-6-luna moves little (34 and 5) and still scores 0.66 and 0.60: it
  gives 1,218 and 1,674 orders a day, 597 and 908 of them lent flights, so
  hubs spend the day covering each other's sites. Shown only exceptions, a
  64-hub region offers a model dozens of them at every call.
- The veto is the steadiest arm: 0.01 to 0.04 under local rules in these
  runs, and 0.00 to 0.06 over the replanner it prunes (Haiku on 64-hub
  seed 2 is 0.01 under it). It still prunes the wrong list: in 10 of these
  12 runs the model keeps a larger share of the replanner's moves than of
  its lent flights (moves kept 29 to 82 percent, lent flights 27 to 77).
- Across all 40 exceptions and veto runs (four models, two arms, three
  seeds at 16 hubs and two at 64), 39 score under local rules on the same
  day. The exception is Opus on the exceptions view at 16 hubs, seed 3:
  0.80 against 0.76 with 58 moves, 47 relocations and 37 stock transfers.
  That day its moves landed: 14 requests missed for want of a vehicle
  against 40 under local rules and 28 under lending, and 982 s of closure
  breach against 1,737. The same arm scored 0.67 and 0.68 on seeds 1 and 2
  at 16 hubs and 0.63 and 0.54 at 64, the lowest four days in the study,
  so the win is a day on which relocating happened to pay, not a model
  that learned when to relocate.
- Parse failures stayed at 0 to 3 per run. Live model runs lose vehicles
  (out of endurance away from a hub): up to 8 of 80 at 16 hubs and up to
  38 of 320 at 64, most under Opus; veto runs and classical policies lose
  none. (Corrected 2026-11-07; an earlier line here said no run lost one.)
- Where the loss lands (2026-11-07, per-hub columns added to the score):
  over the five 16-hub days the hubs that donated under `move` lost 68
  more requests a day than under `serve`, out of 73 in all; recipients 4
  more and the rest 1. Per hundred of their own requests: donors 14 more,
  recipients 2, others none. At 64 hubs (44 of 64 hubs donate on a typical
  day) the donors lost 433 more a day, 18 per hundred of their own, and
  recipients and the rest lost none. On the Opus seed-3 day the model
  moved among 14 of the 16 hubs; its gain over local rules sat at one
  recipient hub (14 fewer lost) and no donor lost ground.
- The same holds in the model runs (2026-11-07, all 40 exceptions and veto
  runs replayed offline with the per-hub columns, each compared with local
  rules on its day): hubs that gave aircraft or stock lost 11 more requests
  per hundred of their own, hubs that only received none, the rest 3; 90
  percent of the models' loss against local rules fell at the hubs that
  gave (above 80 percent for Haiku, Sonnet and Opus at both sizes and in
  both arms). gpt-6-luna is the exception in kind: it rarely moves, a third
  of its 16-hub loss is at giving hubs, and on 64-hub seed 2 with 5 moves it
  lost 331 requests at hubs that neither gave nor received, from 908 lending
  permissions. On the Opus win the hubs that gave lost nothing. This is the
  model-side evidence for the claim; before it, the attribution was only on
  the classical `move` policy.
- The paired figure: one scripted surge at hub 6 and its neighbours on an
  otherwise quiet 16-hub day, seed 3. Lending 0.92 with nothing missed;
  relocating 0.85 with 48 missed, 38 of them at hub 1 after its aircraft
  moved to the surge hubs. The surge hubs fare the same either way.

## Next

- [x] Firm up the claim (run 2026-11-06, done 2026-11-07): the tables
      above.
- [x] Complete the manuscript (2026-11-07): the firm-up tables, the
      paired flight figure (`paper/manuscript/figures/paired-surge.pdf`,
      drawn by a scratch script from two retained runs' frame recordings),
      the per-hub analysis, and the claims reviewed against the results.
- [x] Manuscript appendix (2026-11-07): the frozen system prompt, the
      event texts, one complete live call and a veto prompt's tail, and
      the per-run counts for the 40 model runs, from text files in
      `paper/manuscript/appendix/`. The figure script and the track
      sampler live in the session scratchpad; move them into
      `crates/delivery/` if the figure must be redrawn.
- [ ] Plugins draw their own mission geometry (decided 2026-11-07). Today
      the viewer in `crates/cli/src/viewer.rs` draws only the terrain, the
      Boundary boxes, and each frame's entity snapshot; the boxes are a
      special case where the CLI and the replay reader re-parse plugins
      named `Boundary` from the scenario. The delivery hubs, sites and
      keep-out areas are private state of one autonomy, and random
      closures and corridors exist only there. Of the flows considered
      (rebuild from the mission as Boundary does; a drawings method on the
      plugin traits pulled by the engine; a push sink in the step context;
      a script over the record; hubs and sites as entities), the pulled
      method is the one that gives the live viewer, the exported
      recording, the headless screenshots and the demo video the same
      picture, random events included. The work:
      - A plain `Drawing` enum in core, in world meters: a labeled point,
        a circle, a polyline, a filled strip or polygon, text.
      - One method on each plugin trait returning its current drawings,
        empty by default. The engine collects them once per step on the
        main thread after metrics, in entity order then world-plugin
        order, so the result is the same at any worker count.
      - A drawings channel in the record, written every run whether or
        not Rerun is on, and only when a frame's drawings differ from the
        previous frame's. The replay reader decodes it; the frame, event
        and summary channels stay byte-identical.
      - The viewer logs drawings generically to the 3D world and the map,
        replacing the Boundary code; Boundary moves onto the method, and
        the special cases in the viewer and the replay reader go. Prove it
        with screenshots of the boundary verification mission's exported
        recording before and after.
      - The Dispatcher returns the hubs, the sites and the areas active
        now. Book: the plugin API and the visualization chapter.
- [ ] Rerun view of hubs, sites, closed areas and corridors, with inspected
      screenshots, for figure 1: the first region run with a scripted
      closure and corridor, exported and inspected headless; the same
      recording is the demo video. The surge figure becomes figure 2.
- [x] Read the fleet papers whose team sizes were unknown (COLA 3 UAVs,
      FLEET 2 to 3 robots, LLM-DaaS parsing only, AutoHMA-LLM 3 agents,
      MultiUAV-Plat 3 to 30 UAVs, Scale-Plan 3 robots) and fill in the
      notes in [LITERATURE.md](LITERATURE.md) (2026-11-07). The paper's
      related-work sentences hold; LLM-ODDR's 300 taxis stays the largest.

## Risks

- The paper reads as "language models are bad at this". Every claim has to
  be about an action (relocating), an interface (what cut it), and a size
  (where the cost grows), shown on four models.
- One model run in 40 beat local rules, and the same arm scored lowest on
  its other days. The claim is about the average and the spread, not a
  bound; a reviewer may ask what that one day had, and the per-hub
  outcome analysis in the manuscript bullet is the answer.
- The two command-line clients differ: the `codex` command keeps a coding
  agent's tools, the `claude` command drops them. The API clients are the
  clean comparison if a reviewer asks.
- The cost of relocating is a property of a lean fleet. The capacity table
  in the manuscript shows it weakens with more vehicles per hub but never
  reverses; the paper says where it holds.
- Simulation only, on cheap vehicle models. The checked Rascal and Firefly
  are available but no result depends on them.

## To do

- [ ] **Release run** of `reference/suite.py --release --retain`, so
      `paper/data/` reflects the current engine.
- [ ] **Channel congestion** in the networks, delivery that degrades with how
      many nearby agents transmit at once. SphereNetwork has range, loss, and
      delay but not this. Only needed if comms are added to the study.

## Not planned

Runtime plugin loading, legacy protobuf/string-map spawning, OpenCL, VTK,
ROS, JSBSim. ArduPilot and Burn stay optional, out of the default build.
No reinforcement-learning study (decided 2026-11-06); the learning crate
stays as an engine feature. Classical coordination at scale was dropped too.
Out of the study's scope: communication loss and delay, wind beyond the
weather event, terrain, hardware, wording comparisons, and one manager per
hub as an arm of its own.

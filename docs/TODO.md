# Roadmap

## The claim

**A language-model fleet manager over-helps.** It relocates vehicles and
stock when the best policy is to set local rules and lend flights, and the
cost of relocating grows with the region. Telling the model the cost does
not reliably stop it; an exceptions-only view reduces moves for some
models, and a donor guard refuses them. In the initial veto runs, models
keep many costly moves while dropping temporary service permissions. Four
models have been tested in the live exceptions and veto roles; two also
wrote policies. Classical comparisons cover three region sizes, while
model comparisons at 64 hubs and more seeds are still running.

Everything below serves that claim. The test problem is one day of blood
delivery across 16 to 64 hubs with surges, shortages and closures, a
classical dispatcher under the model, and a score taken from what was flown.
The model and its prompt are fixed; what varies is how the model is used (in
the loop, as a veto over a planner, or writing the rules once) and what it
is shown. The full plan with every result so far is
[FLEET_MANAGER_PLAN.md](FLEET_MANAGER_PLAN.md) and the related work is
[LITERATURE.md](LITERATURE.md).

How it got here, all on 2026-11-06: the mission was re-sized until the
fleet decision mattered; five directions were tried and the decomposition of
the replanner's help showed that relocating is the whole loss while local
rules and lending are the whole gain; evaluations A to H then tested every
interface and model against that bar. The outcomes are in the plan's tables.
The earlier study, which asked which mistakes a language-model planner
makes, is on the `llm-fleet` branch with its manuscript; it turned into
prompt engineering because the model was what it measured.

## Next

- [ ] Firm up the claim (runs started 2026-11-06): the exceptions view and
      the veto for Haiku, Sonnet, Opus and gpt-6-luna at 64 hubs on two
      seeds and at 16 hubs on a third; the classical table (none, replanner,
      guarded replanner, local, serve, move, return) at five seeds, 16 and
      64 hubs. The soft spot is that the best model arms sit only 0.01 to
      0.04 under local rules on single seeds; the check is that no model arm
      beats local rules on every seed and that the gap does not close at 64
      hubs.
- [ ] Complete the manuscript in `paper/manuscript/main.tex`: replace the
      preliminary tables with the firm-up runs, add the paired flight
      figure and per-hub outcome analysis, then review the claims against
      those results.
- [ ] Rerun view of hubs, sites, closed areas and corridors, with inspected
      screenshots, for figure 1.
- [ ] Read the fleet papers whose team sizes are unknown (COLA, FLEET,
      LLM-DaaS, AutoHMA-LLM, MultiUAV-Plat, Scale-Plan) and fill in the
      notes in [LITERATURE.md](LITERATURE.md).

## Risks

- The paper reads as "language models are bad at this". Every claim has to
  be about an action (relocating), an interface (what cut it), and a size
  (where the cost grows), shown on four models.
- If a model arm beats local rules in the firm-up, the claim narrows to
  "only behind a guard or a planner".
- The two command-line clients differ: the `codex` command keeps a coding
  agent's tools, the `claude` command drops them. The API clients are the
  clean comparison if a reviewer asks.
- The cost of relocating is a property of a lean fleet. Evaluation G shows
  it weakens with more vehicles per hub but never reverses; the paper says
  where it holds.
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

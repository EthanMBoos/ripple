# Ripple can own the replayable fleet-planner testbed

The core idea of the new direction is taken: since 2022, dozens of papers have
had a language model pick, sequence, or write code over a fixed library of
described skills, and by 2024-2026 that pattern reached multi-robot teams
(SMART-LLM, COHERENT, EMOS) and UAV swarms (TACOS, COLA, Web-of-Drones,
MultiUAV-Plat). What is not taken is the evaluation substrate. Across the five
sets of notes, no verified paper combines (a) a fleet that mixes airframes with
different dynamics (multirotor and fixed-wing), (b) a skill layer where
classical and learned implementations of the same skill swap behind one
interface, (c) episodes that replay byte for byte with the model calls included,
and (d) an episode count and baseline set (optimizer, auction) large enough for
statistics; every close competitor has at most two of the four. The nearest
neighbor is MultiUAV-Plat (arXiv 2026: 1,500 tasks, 9,396 hidden checks), which
has no flight dynamics, no heterogeneity, and no classical baseline. Ripple
should therefore be pitched as a measurement paper about LLM fleet planners that
happens to need an engine only Ripple provides, not as "an LLM that commands
drones through tools". The strongest move is to re-run MultiUAV-Plat's three
task families and Web-of-Drones' swarm tasks inside Ripple, on a mixed fleet,
with an optimizer and an auction on the same episodes. The target is CoRL 2027,
with IEEE RA-L (IROS presentation option) as the fallback. Coverage of 2026
preprints was partial, so novelty claims below are worded as "we found no prior
work that ..." and the verification flags from the notes are kept.

How to read the flags. **(local copy)** means read from the full text in
`paper/literature_review/sources/`; **(web)** means checked only against an
arXiv abstract or HTML page; **[V]** means title, authors, year, and venue were
checked in the research session; **[M]** means cited from the researcher's
memory with a standard arXiv ID, not re-fetched; **(secondary)** means seen only
as a citation in another paper; **[UNVERIFIED]** means no source was retrieved.
Venues marked "per references.bib" come from the project bibliography, not from
the paper itself.

## Four years of "LLM over a skill library" leave little to claim on the planner

The single-robot pattern was settled by 2023. SayCan scored text descriptions of
roughly 551 learned skills against a learned affordance value and reached about
84% planning and 74% execution success on 101 instructions **[M]** ([SayCan,
CoRL 2022](https://arxiv.org/abs/2204.01691)). Inner Monologue added closed-loop
text feedback **[M]** ([CoRL 2022](https://arxiv.org/abs/2207.05608)); Code as
Policies had the model write Python against a perception-and-control API **[V
for content]** ([ICRA 2023](https://code-as-policies.github.io/)); ProgPrompt
ablated Pythonic prompt components **[M]** ([ICRA
2023](https://arxiv.org/abs/2209.11302)); LLM+P handed PDDL to a classical
planner **[M]** ([arXiv 2023](https://arxiv.org/abs/2304.11477)). ChatGPT for
Robotics drove AirSim drones and a real Tello through a described function
library **[M]** ([IEEE Access 2024](https://arxiv.org/abs/2306.17582)), and
TypeFly replaced Python with a token-efficient drone language, MiniSpec,
reporting up to 62% lower response time **[V]** ([arXiv 2023, venue
unconfirmed](https://arxiv.org/abs/2312.14950v2)). Growing the library is also
taken: Voyager's "ever-growing skill library of executable code" **[V for arXiv
metadata]** ([arXiv 2305.16291](https://arxiv.org/abs/2305.16291)), BOSS's
LLM-guided skill chaining **[V]** ([CoRL
2023](https://proceedings.mlr.press/v229/zhang23a.html)), and 2026 preprints
ASPIRE **[V]** ([arXiv 2607.00272](https://arxiv.org/abs/2607.00272)) and
SkillComposer **[V]** ([arXiv 2608.14944](https://arxiv.org/abs/2608.14944)).
RoboGen is the clearest prior case of an LLM choosing per subtask between a
learned skill (RL) and a classical one (motion planning) **[M]** ([ICML
2024](https://arxiv.org/abs/2311.01455)). The π0.5-style hierarchy Ripple
borrows is itself crowded: Hi Robot **[V]** ([ICML
2025](https://arxiv.org/abs/2502.19417)), π0.5 **[M]** ([arXiv
2025](https://arxiv.org/abs/2504.16054)), RT-H **[M]** ([RSS
2024](https://arxiv.org/abs/2403.01823)), and GR00T N1 **[M]** ([arXiv
2025](https://arxiv.org/abs/2503.14734)). Exposing robots as LLM tools through
ROS, LangChain, or MCP is taken by ROSA **[V]** ([NASA JPL, arXiv
2024](https://arxiv.org/html/2410.06472v2)), ROS-LLM **[V]** ([arXiv
2024](https://arxiv.org/html/2406.19741v3)), and an MCP layer over an unmodified
ROS navigation stack **[V]** ([arXiv
2609.27340](https://arxiv.org/abs/2609.27340)).

The multi-robot layer filled in during 2023-2026, and its shape matters for
Ripple because it shows where execution is idealized. SMART-LLM decomposes,
forms coalitions, and allocates across 1-4 heterogeneous agents in AI2-THOR;
GPT-4 success falls from **1.00 on elemental tasks to 0.69-0.71 on compound and
complex ones**, while random allocation after the same decomposition scores 0.00
on complex tasks (local copy; IROS 2024 per references.bib) ([arXiv
2309.10062](https://arxiv.org/abs/2309.10062)). Chen et al. compared four LLM
communication structures with up to 32 robots and found decentralized dialog
worst and a centralized proposer with local feedback (HMAS-2) best, e.g. **82.5%
vs 75% on BoxNet1 and 62.5% vs 15% on Warehouse** against the plain centralized
planner (local copy) ([ICRA 2024](https://arxiv.org/abs/2309.15943)); Ao, Gao,
and Simchi-Levi prove that without new exogenous signals any delegated multi-LLM
network is dominated by a centralized Bayes decision maker (local copy) ([arXiv
2603.26993](https://arxiv.org/abs/2603.26993)). COHERENT runs quadrotors, robot
dogs, and arms in BEHAVIOR-1K with discrete actions and reports **0.975 success
vs 0.625** for a centralized baseline, but ran only 40 of its 100 tasks
"considering the cost of the OpenAI API" (local copy; ICRA 2025 per
references.bib) ([arXiv 2409.15146](https://arxiv.org/abs/2409.15146)). EMOS
builds a "robot resume" from URDFs for Fetch, Stretch, Spot, and a DJI M100 in
Habitat, with physics disabled and snap grasping; removing the resume drops
success from **37.82% to 15.63%** (web; venue unconfirmed) ([arXiv
2410.22662](https://arxiv.org/abs/2410.22662)). LaMMA-P (web) ([ICRA
2025](https://arxiv.org/abs/2409.20560)) and DART-LLM (web) ([arXiv
2411.09022](https://arxiv.org/abs/2411.09022)) add PDDL and DAG decompositions
for heterogeneous household and construction teams. A survey of the field lists
the lack of unified multi-robot benchmarks and realistic multi-robot simulators
as open problems and notes "few studies that report matched sim-versus-real
success rate comparisons on identical LLM-driven tasks" (local copy; Autonomous
Robots per references.bib) ([Li et al., arXiv
2502.03814](https://arxiv.org/abs/2502.03814)).

Benchmarks follow the same split. PARTNR (100,000 instructions, humans 93% vs
LLMs 30%, figures from the project page) ([ICLR
2025](https://arxiv.org/abs/2411.00081v1)), EmbodiedBench (1,128 tasks, best
model GPT-4o at 28.9%) ([ICML 2025](https://www.arxiv.org/abs/2502.09560)),
LoTa-Bench ([ICLR 2024](https://arxiv.org/html/2402.08178v1)), and CoELA's
TDW-MAT and C-WAH ([ICLR 2024](https://iclr.cc/virtual/2024/poster/19093)) are
household and indoor. Aerial benchmarks are almost all single-UAV
vision-language navigation: OpenFly ([ICLR
2026](https://arxiv.org/html/2502.18041v6)), OpenUAV ([ICLR
2025](https://mlanthology.org/iclr/2025/wang2025iclr-realistic-a)), UAV-ON
([arXiv 2508.00288](https://arxiv.org/html/2508.00288v4)), and AerialVLN
**[UNVERIFIED venue]**. The notes found **no benchmark with a heterogeneous
aerial fleet**, and almost none with scripted dynamic events; CoMuRoS has
event-driven LLM replanning on hardware (9 of 10 trials with GPT-4o) but no
fixed event script and no solver baseline ([arXiv
2511.22354](https://arxiv.org/html/2511.22354v2)).

## Eight competitors each hold at most two of Ripple's four pieces

The closest competitors are UAV fleet systems and one large multi-UAV benchmark.
The table summarizes what each showed and what it lacks relative to Ripple's
combination of mixed airframe dynamics, swappable classical/learned skills,
exact replay, and a controlled episode set with classical baselines.

| Competitor | What it showed | What it lacks against Ripple | Venue, year |
|---|---|---|---|
| MultiUAV-Plat / Agent4Drone (local copy) | REST tool primitives over a lightweight sim; 75 sessions of 3-30 UAVs, 1,500 tasks, 9,396 hidden checks; Agent4Drone 57.9% vs ReAct 30.6% (doubao-2-pro), 70.3% with deepseek-v4-pro; about USD 60 for 35K requests | Flight dynamics explicitly out of scope; homogeneous UAVs; no classical baseline; seeded sessions but no LLM replay; final UAV positions carry over between tasks, so results depend on task order | arXiv 2026 ([2606.31073](https://arxiv.org/abs/2606.31073)) |
| Web-of-Drones, "Say the Mission, Execute the Swarm" (local copy) | MCP + W3C Web-of-Things tools over ArduPilot simulation; coverage, formation, irrigation; six LLMs; guardrails and planning tools improve robustness; tokens do not track success | Homogeneous swarm; few missions per model; no replay (prompts on Zenodo); guardrails derived ad hoc from failure patterns | IEEE WoWMoM 2026 ([2605.03788](https://arxiv.org/abs/2605.03788)) |
| COLA (local copy) | LLM as transpiler to JSON function calls over ArduPilot; live flights with three VTOL quad-planes | 4 live flights, 2 fully successful; heterogeneous teams only in sim; no code, no replay | IEEE Aerospace 2026 per references.bib ([PDF](https://stottlerhenke.com/wp-content/uploads/2026/06/2026_IEEE_AERO_Ludwig_COLA.pdf)) |
| TACOS (local copy) | Coordinator + supervisor LLMs over an API library and the ATOMICA trajectory planner; 50 sim runs per config at 6/12/20 drones; PX4 hardware | Homogeneous quadrotors; sim assumes "perfect low-level trajectory tracking"; hardware behavior varied across 10 repeats | arXiv 2025 ([2510.01869](https://arxiv.org/abs/2510.01869)) |
| SwarmGPT (local copy) | LLM choreography for up to 200 simulated and 20 real Crazyflies; parameterized primitives beat raw waypoints (waypoints fall to 0% at scale); a safety filter, not the LLM, guarantees feasibility | Homogeneous; choreography, not allocation; no replayable evaluation | IEEE RA-L 2025 ([2412.08428](https://arxiv.org/abs/2412.08428)) |
| COHERENT (local copy) | Heterogeneous team with quadrotors; assigner/executor loop; 0.975 vs 0.625 | Discrete BEHAVIOR-1K actions, no flight dynamics; 40 of 100 tasks; no fixed-wing | ICRA 2025 per references.bib ([2409.15146](https://arxiv.org/abs/2409.15146)) |
| EMOS (web) | Capability encoding from URDF matters (37.8% vs 15.6%); 519-episode ablation with a drone | Physics disabled; navmesh motion; whether the drone has dynamics unconfirmed | arXiv 2024/2025 ([2410.22662](https://arxiv.org/abs/2410.22662)) |
| Khatana et al. (local copy) | A geometric verifier between LLM plans and execution catches plans that are "jointly infeasible after geometric grounding"; CARLA and Crazyflie | Two agents; single-Crazyflie hardware; no heterogeneity or replay | arXiv 2026 ([2610.12761](https://arxiv.org/abs/2610.12761)) |

Two infrastructure neighbors deserve citation. AUSPEX is an open-source ROS 2
framework for heterogeneous multi-UAV planning that swaps whole planners behind
one interface, including the LLM-modulo "Say'n'Fly", with HIL validation, but it
reports a demonstration, not a controlled evaluation ([AAMAS 2026
demo](https://ifaamas.org/Proceedings/aamas2026/pdfs/QTXJ5819.pdf)). FLEET puts
an LLM front-end over a MILP or auction back-end for heterogeneous teams and
found the **auction matched MILP on constraint-free tasks but fell below it on
heterogeneous and temporal ones** ([arXiv
2510.07417](https://arxiv.org/html/2510.07417)); RobotFleet reportedly compares
an LLM allocator against a min-max-load MILP, but its numbers could not be
extracted ([arXiv 2510.10379](https://arxiv.org/pdf/2510.10379)). Two leads
could not be checked and are the main risk to novelty claims: AutoHMA-LLM,
cloud-edge LLM coordination of UAVs, robots, and cars (secondary; IEEE TCCN
2025, cited in [arXiv 2501.02341](https://arxiv.org/abs/2501.02341)), and the
Austrian FFG project AMASS, which aims to evaluate "LLM-coordinated Fixed Wing
Swarm systems" but has published goals, not results (secondary) ([FFG
slides](https://www.ffg.at/sites/default/files/allgemeine_downloads/thematische%20programme/Mobilitaet/Take%20Off/04_TO_AS2023_AMASS_dApolito_25032025.pdf)).

Three patterns run through the competitors. First, **heterogeneity means
discrete skill sets**, not dynamics: capabilities are Python dictionaries
(SMART-LLM), prompt text (COHERENT), or URDF resumes (EMOS), and no encoding
seen represents minimum airspeed, turn radius, or endurance. The NeLV position
paper notes that a fixed-wing aircraft cannot hover and must plan loiter circles
(secondary, passage not read) ([arXiv
2510.21739](https://arxiv.org/abs/2510.21739)); that constraint is exactly what
current encodings skip. Second, **real autopilots and large controlled
evaluations never meet**: the papers with ArduPilot or PX4 (COLA, Web-of-Drones,
TACOS hardware) have the smallest evaluations, while the large benchmarks use
idealized primitives. Third, **run-to-run variation is anecdotal**: TACOS's ten
hardware repeats and RoCo's temperature sweep ([ICRA
2024](https://arxiv.org/abs/2307.04738)) are the only variance evidence, and no
paper checked records LLM outputs for exact replay or separates model variance
from simulator variance.

## Engines give determinism caveats, and agent evaluation measures variance instead of removing it

On the engine side, the aerial-simulator survey shows that fixed-wing and
multirotor coexist only in Gazebo, AirSim, and MATLAB's UAV Toolbox, none of
which is swarm-specific, while GPU RL simulators (Aerial Gym, Isaac/Pegasus,
gym-pybullet-drones, Flightmare) are multirotor-only ([Dimmig et al., arXiv
2311.02296](https://arxiv.org/html/2311.02296v7)). Crazyflow reaches about 700M
steps/s at 1M parallel worlds on one GPU and won its place with throughput and
sim-to-real RMS tables, but does not discuss determinism ([arXiv
2606.01478](https://arxiv.org/html/2606.01478v1)). Determinism claims elsewhere
are scoped: MuJoCo guarantees reproducibility "only ... within a single version,
on the same architecture" ([MuJoCo
docs](https://mujoco.readthedocs.io/en/stable/computation/index.html)); Isaac
Lab warns that "any modification to the execution ordering can result in minor
changes" and that determinism costs speed ([Isaac Lab
docs](https://isaac-sim.github.io/IsaacLab/main/source/how-to/reproducibility.html)).
GPUSimBench measured parallel and run-to-run divergence directly, e.g. **Isaac
Lab 4.21 cm parallel variability, Madrona 32.53 cm parallel and 11.47 cm
run-to-run**, against 0.00 for MJX and Genesis ([arXiv
2607.13059](https://arxiv.org/html/2607.13059v1)). **No surveyed simulator
claims bit-identical output across thread counts**, and none pairs an LLM tool
interface with 6-DOF fixed-wing plus multirotor dynamics. The LLM-agent
platforms (MultiUAV-Plat, GRID/AirGen ([arXiv
2310.00887](https://arxiv.org/pdf/2310.00887))) trade physics for a tool API.
SCRIMMAGE's own paper is in the local sources (`sources/demarco2018/`), although
the simulator notes did not find a peer-reviewed engine paper; check its venue
before citing.

On the model side, temperature 0 is not deterministic. Thinking Machines traced
the cause to inference kernels that are not batch-invariant: 1,000 greedy
completions from Qwen3-235B gave **80 unique outputs, diverging at token 103**,
and batch-invariant kernels made all 1,000 identical at roughly 1.6-2x the
runtime ([Thinking Machines,
2025](https://thinkingmachines.ai/blog/defeating-nondeterminism-in-llm-inference)).
Atil et al. found accuracy swinging **up to 15% across 10 runs** of
"deterministic" settings ([arXiv 2408.04667](https://arxiv.org/abs/2408.04667));
OpenAI's `seed` is documented as best-effort ([OpenAI
Cookbook](https://developers.openai.com/cookbook/examples/reproducible_outputs_with_the_seed_parameter));
SGLang now offers a deterministic mode for self-hosted models ([SGLang
docs](https://docs.sglang.io/advanced_features/deterministic_inference.html)).
Anthropic's official wording on temperature-0 determinism was not found. Agent
benchmarks respond by measuring variance: τ-bench's pass^k ([arXiv
2406.12045](https://arxiv.org/abs/2406.12045)), Bjarnason et al.'s finding that
SWE-bench pass@1 varies 2.2-6.0 points between runs and that 2-3-point gains may
be noise ([arXiv 2602.07150](https://arxiv.org/abs/2602.07150)), and HAL's
21,730 standardized rollouts with shared logs ([ICLR
2026](https://arxiv.org/abs/2510.11977)). Embodied benchmarks such as
EmbodiedBench rely on temperature 0 for consistency, which the evidence above
shows is insufficient.

Record-and-replay of model calls is itself taken, so Ripple must not claim
response caching. Inspect AI caches on model, message history, epoch, generate
config, and tools ([Inspect](https://inspect.aisi.org.uk/caching.html));
Chronicle records every model and tool call as an envelope and reports
bit-stable full replay, **0 divergences in 20 repetitions**, for business
tool-use agents ([arXiv 2609.20625](https://www.alphaxiv.org/abs/2609.20625));
agrepl replays at the transport layer ([arXiv
2607.16200](https://arxiv.org/abs/2607.16200)). Deterministic simulation is
taken too (CARLA's synchronous Traffic Manager mode, [CARLA
0.9.11](https://carla.org/2020/12/22/release-0.9.11/)). What the notes found
open is the combination: a multi-agent robotics simulator where the whole
episode, model calls included, replays byte for byte and that property is
checked as an invariant across worker counts. The argument is structural, not
cosmetic: a prompt cache keyed on message history misses whenever anything
upstream of the prompt (simulator state, float formatting) changes, so exact
replay of an LLM-in-the-loop episode requires a deterministic simulator first.
Ripple's keyed RNG also gives common random numbers for paired comparisons;
rliable's probability of improvement is the paired statistic to report ([Agarwal
et al., NeurIPS 2021](https://arxiv.org/abs/2108.13264)). No agent-evaluation
paper applying common random numbers to LLM comparisons was found, but this was
not searched in depth.

## Ten claims are already taken; four measurements remain open

The following must not be presented as contributions, because each has an
established owner. An LLM choosing or sequencing skills from text descriptions
(SayCan, Inner Monologue). An LLM writing code against a robot or drone API
(Code as Policies, ProgPrompt, ChatGPT for Robotics, TypeFly). Closed-loop
replanning on failure (Inner Monologue; verify-and-retry in Ha et al., where
removing retries sends the mailbox domain to 0% **[V]** ([CoRL
2023](https://proceedings.mlr.press/v229/ha23a/ha23a.pdf))). An LLM paired with
a classical planner or verifier (LLM+P, Text2Motion, Khatana et al.). A growing
or self-built library (Voyager, BOSS, ASPIRE, SkillComposer). "Skills added
without retraining" through a tool registry, which every verified fleet system
already does (SMART-LLM, Web-of-Drones, TACOS, COLA, MultiUAV-Plat) and which
Hsieh et al. showed for general tools, where documentation alone matches
few-shot demonstrations **[V]** ([arXiv
2308.00675](https://arxiv.org/abs/2308.00675)). Exposing robots through ROS,
MCP, or LangChain (ROSA, ROS-LLM). A VLM emitting language subtasks to a
low-level policy (Hi Robot, π0.5, RT-H, GR00T N1). LLM multi-robot allocation
with heterogeneous capabilities in the abstract (SMART-LLM, COHERENT, EMOS).
And, for software agents, "routing accuracy falls as the library grows": a 2026
study of 15 LLMs and 1,141 skills found accuracy decays logarithmically with
library size (R² > 0.97) **[V]** ([arXiv
2605.16508](https://arxiv.org/abs/2605.16508)), and SkillAlign showed exposure
format matters, with a snippet-only report of full exposure falling to 47.9 at
1,000 ALFWorld skills **[V]** ([EMNLP 2026](https://arxiv.org/abs/2609.07255)).
LLMs losing to classical solvers on fully specified problems is also known (Fast
Downward solves PlanBench "near-instantly with guaranteed perfect accuracy")
([arXiv 2409.13373](https://arxiv.org/html/2409.13373v1)), so the leaderboard
must ask *where* the LLM earns its cost, not *whether* the optimizer wins.

Four measurements look open, each with partial coverage. In robotics, the notes
found no study that sweeps skill-library size and reports task success, no
leave-one-skill-out ablation, and no cost-per-added-skill accounting; the 2026
software scaling law is the hypothesis to test in a physical setting. No paper
compares a classical and a learned implementation of the same skill under one
LLM planner and one physics model. No robot paper degrades skill descriptions in
a controlled way, though EMOS's capability-description ablation is the nearest
evidence that wording moves success. And no verified paper evaluates an LLM
planner on a mixed fixed-wing and multirotor fleet; COLA's quad-planes flew as a
homogeneous team.

The prior Ripple finding that higher dynamics fidelity did not change outcomes
cuts both ways and should be stated plainly. It supports MultiUAV-Plat's and
TACOS's choice to idealize multirotor motion, so the paper should not attack
them for lacking physics in general. It sharpens the claim instead: fidelity
matters where airframes differ in what they can do (a fixed-wing cannot hold a
point, has a minimum speed and a turn radius), which is exactly where capability
encodings are silent. That turns a null result into a design rule, "idealize
within an airframe class, model the differences between classes", and makes the
capability-violation experiment the paper's sharpest result.

## Re-run MultiUAV-Plat and Web-of-Drones on a mixed fleet, beside an optimizer and an auction

Stepping on the competitors means re-running their tasks where Ripple changes
the answer. **MultiUAV-Plat** is the first target: it is open source (GitHub
zhangsheng93/MultiUAV-Plat), its three task families (Area Search, Area
Assignment & Patrol, Target Assignment) are fleet-allocation problems Ripple can
host directly, and it has no classical baseline. Port a stratified subset of its
sessions, run ReAct and an Agent4Drone-style agent alongside Ripple's planner,
add an optimizer and an auction, then repeat with a share of the fleet replaced
by fixed-wing aircraft and with tasks reset between episodes to remove its
task-order carry-over. **Web-of-Drones** is the second: its prompts are public
on Zenodo (10.5281/zenodo.18433213) and its coverage and formation tasks with
and without planning tools are a direct test of "planning tools as skills",
replicable across several of its six LLMs. **SMART-LLM**'s released prompt
format (SMARTlab-Purdue/SMART-LLM) and **Chen et al.**'s CMAS and HMAS-2
structures (yongchao98/multi-agent-framework) supply LLM planner baselines with
code. **SwarmGPT**'s primitives-vs-waypoints result is cheap to reproduce as a
skill-granularity ablation. **FLEET**'s auction-vs-MILP split and the
**MRTA-Benchmark**'s 250,000 instances with exact MILP optima
([4TU](https://data.4tu.nl/datasets/10e28ee0-9ad9-450d-8be7-6e6a91f2931f)) give
ground truth for the leaderboard. **TACOS**'s landmark-monitoring scenario at
6/12/20 drones and **Khatana et al.**'s 2/5/9-target task are good small
replications if time allows; COHERENT, EMOS, and PARTNR are household benchmarks
and are better cited than re-run.

Venue follows the evidence pattern in the notes: engine papers that reached
ICLR, RSS, or CoRL paired the engine with a benchmark and an agent result, while
pure engine papers went to RA-L, IROS, or journals; LLM-over-skills robotics
work (SayCan, BOSS, VoxPoser, Ha et al.) clusters at CoRL. That makes **CoRL
2027** the primary target, with **IEEE RA-L** (with the IROS presentation
option) as the fallback. If the leaderboard and replayable episode set become
the centerpiece, the NeurIPS Datasets and Benchmarks track, which published
PlanBench ([NeurIPS 2023
D&B](https://papers.neurips.cc/paper_files/paper/2023/file/7a92bcdede88c7afd108072faf5485c8-Paper-Datasets_and_Benchmarks.pdf)),
is the alternative. Deadlines were not checked in the research and should be
confirmed.

## Conclusion

The literature moved the question from "can an LLM emit a plausible fleet plan"
to "does the plan survive grounding" (Khatana, GAVEL, SwarmGPT's safety filter,
Web-of-Drones' guardrails), and it has answered that question only with
idealized execution, homogeneous fleets, and unrepeatable runs. Ripple's
advantage is that it can ask the grounding question under controlled, paired,
exactly replayable conditions on airframes that genuinely differ, which turns
anecdotes into effect sizes. The risk is presentation: framed as a planner, the
paper collides with ten owned claims; framed as the instrument and the
measurements it enables, it occupies an empty cell. Before submission, a
targeted sweep of 2026 CoRL, ICRA, and arXiv for "skill library scaling",
"fixed-wing LLM swarm", and "LLM replay robotics", plus primary-source checks of
AMASS, AutoHMA-LLM, EMOS's drone model, and every **[M]** citation, would close
the remaining gaps.

### Positioning recommendation

**Title-level thesis.** "Ripple: exactly replayable episodes for evaluating
language-model fleet planners over mixed multirotor and fixed-wing skill
libraries." One-sentence thesis: when every episode, model replies included,
replays byte for byte, LLM fleet planners can be compared with optimizers and
auctions on identical episodes, and the comparison shows where the LLM earns its
cost and where heterogeneous airframes break it.

**Claim 1 (instrument).** We found no robotics or embodied LLM benchmark in
which whole episodes, model calls included, replay byte for byte, and no
surveyed simulator that claims bit-identical output across thread counts.
Experiments: replay hashes identical at 1, 2, and 8 workers and with
visualization on or off; the same episodes re-queried live at temperature 0 to
show divergence (report TARr@N-style agreement as in Atil et al.); a variance
decomposition separating model sampling from simulator seed using keyed RNG as
common random numbers; a GPUSimBench-style parallel and run-to-run variability
row reporting 0 for Ripple; throughput against named simulators. Avoid claiming
response caching or deterministic simulation alone.

**Claim 2 (heterogeneity).** We found no prior work that evaluates an LLM fleet
planner on a mixed fixed-wing and multirotor fleet, or that encodes dynamic
capability limits (minimum speed, turn radius, cannot hover) rather than
discrete skill sets. Experiments: capability-violation rate by planner (LLM,
optimizer, auction) on MultiUAV-Plat-style tasks as the fixed-wing share rises;
an EMOS-style capability-description ablation (no limits, text limits, numeric
limits); the fidelity null result reported as within-class versus between-class,
showing that dynamics matter where airframes differ.

**Claim 3 (skill library as the scaling axis).** We found no robotics study that
measures fleet-plan success against skill-library size, ablates skills one at a
time, accounts the cost of each added skill, or swaps a classical and a learned
implementation of the same skill under one planner. Experiments: success curves
as the library grows, compared with the log-decay law from software agents;
leave-one-skill-out; lines of code and wall time per skill against success
gained; a learned skill replacing its classical twin with all else identical;
good, terse, misleading, and missing descriptions; SwarmGPT-style primitive
granularity.

**Claim 4 (when an LLM is needed).** We found few embodied LLM benchmarks that
put LLMs on the same episodes as an optimizer and an auction, and no
head-to-head of an LLM allocator against an auction under injected dynamic
events. Experiments: a leaderboard of several LLMs, ReAct, an Agent4Drone-style
agent, SMART-LLM and HMAS-2 prompts, an optimizer (with MRTA-Benchmark optima
where instances map), and an auction, on identical replayable episodes with
scripted events; templated versus messy natural-language requests, with a
parser-plus-optimizer baseline on the templated set to show where the LLM adds
value; pass^k and rliable intervals, with probability of improvement as the
paired statistic.

**Competitor tasks to re-run.** MultiUAV-Plat (three families, stratified
sessions, with and without fixed-wing, tasks reset between episodes);
Web-of-Drones coverage and formation with and without planning tools, using its
Zenodo prompts; SMART-LLM and HMAS-2 as planner baselines from released code;
SwarmGPT primitives versus waypoints; FLEET's auction-versus-MILP comparison;
optionally TACOS landmark monitoring and Khatana's 2/5/9-target task.

**Venue.** CoRL 2027 primary; IEEE RA-L with IROS presentation as fallback;
NeurIPS Datasets and Benchmarks if the leaderboard leads.


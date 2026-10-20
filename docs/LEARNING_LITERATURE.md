# Learned vs classical: what's out there

Notes from the literature search behind the [LLM delivery
plan](TODO.md#paper-llm-planners-over-a-classical-stack-chosen-october-2026)
and the paper's task and protocol sections.
Done in September and October 2026. Citations were checked against arXiv or
the publisher unless marked unverified.

The papers themselves, with their code, download with
`scripts/build_paper_sources.py` (see
[paper/literature_review](../paper/literature_review/README.md)).

## LLM planners over classical stacks

For the LLM delivery plan. A first structured pass (October 2026): fixed
questions, searches across venues for 2022 to 2026, and the reference
lists of the main survey and papers. Venues are confirmed where stated;
"seen as abstract" means the full paper wasn't read.

**Q1. LLM task planning for multi-robot teams.**

- Survey: Li et al., "Large Language Models for Multi-Robot Systems: A
  Survey" (arXiv 2502.03814; Autonomous Robots, 2026). Four levels (task
  allocation, motion planning, action generation, human in the loop). It
  finds no high-level task-allocation work for UAVs (only FlockGPT,
  formation control); evaluations are mostly indoor household simulators
  (AI2-THOR, Habitat, PyBullet, BEHAVIOR-1K). It calls for bridging lab
  simulation and the real world and for outdoor, noisy settings; notes LLM
  latency (one GPT-4 path-finding step took 15 to 30 s) and dependence on
  a server link. Our starting bibliography.
- SMART-LLM (Kannan et al., IROS 2024): decomposition, coalition
  formation, allocation by skill, few-shot prompts. RoCo (Mandi et al.,
  ICRA 2024): each arm's LLM negotiates; invalid plans get environment
  feedback; a central motion planner executes. Chen et al., "Scalable
  Multi-Robot Collaboration with LLMs: Centralized or Decentralized?"
  (ICRA 2024). COHERENT (Liu et al., ICRA 2025): quadrotors, robot dogs,
  and arms; a proposal, execution, feedback, adjustment loop; 100-task
  benchmark. LiP-LLM (Obata et al., RA-L): LLM plus linear programming and
  dependency graphs. DART-LLM: dependency graphs. CoMuRoS (arXiv
  2511.22354): ground robots and arms, event-driven replanning.
- Aerial: TypeFly (arXiv 2312.14950): an LLM writes programs in a small
  drone language, with replanning. UAV-VLA (arXiv 2501.05014) and
  UAV-CodeAgents: missions generated from satellite images. SwarmGPT
  (Schuck et al., RA-L 2025): choreography from GPT-4 fixed by a safety
  filter (planned before flight; waypoint plans about 50% successful with
  10 drones). TACOS (arXiv 2510.01869). MultiUAV-Plat (arXiv 2606.31073,
  an unreviewed preprint; drones move to the next position each step).
  Surveys: "UAVs Meet LLMs" (arXiv 2501.02341), LLM-assisted UAV
  operations (arXiv 2602.19534), Drones 2026 perception-cognition-action.

**Q1b. Delivery dispatch, from a second agent's review.** LLM-DaaS
(Wassim, Mohamed, and Hamdi, arXiv 2412.11672, 2024): small fine-tuned
LLMs turn text delivery requests into structured ones, then drone
selection, composition, and weather-aware routing; no vehicle dynamics or
radio. SCoBA (Choudhury, Gupta, Kochenderfer, Sadigh, and Bohg, RSS 2020):
online multi-robot task allocation under deadlines and uncertain
completion, tree search per agent plus conflict resolution, evaluated on
drone delivery dispatch; the competent classical baseline to beat.

**Q2. Checking an LLM's plan before execution.** The established pattern,
so not our novelty on its own.

- LLM-Modulo (Kambhampati et al., ICML 2024 position paper): LLMs propose,
  sound external verifiers check. LLM+P (Liu et al., arXiv 2304.11477;
  venue not confirmed): the LLM writes PDDL, a classical planner solves.
- SayPlan (Rana et al., CoRL 2023): the plan is refined in a loop against
  a scene graph simulator before execution, and a classical path planner
  shortens the LLM's horizon.
- AutoTAMP (arXiv 2306.06531): LLM translates to signal temporal logic,
  a task and motion planner solves and checks. SELP (ICRA 2025): LTL
  plans. VerifyLLM, T3 Planner (arXiv 2510.16767): pre-execution checks
  with temporal logic.
- Khatana, Song, Hovakimyan, and Voulgaris, "Verifier-Mediated Multi-LLM
  Coordination for Multi-Robot Path Planning" (arXiv 2610.12761, IROS 2026
  workshop FM-MRS, 8 October 2026): a geometric verifier grounds each
  plan into trajectories, predicts conflicts between robots, and feeds
  them back for replanning; centralized against decentralized; CARLA and
  Crazyflies. The closest to "Ripple checks the plan"; theirs is a
  geometric conflict check without dynamics or a network. Seen as
  abstract.

**Q3. Learned world models checking actions at test time** (the trend the
framing leans on): a policy proposes, a world model imagines outcomes,
the best is kept. WEAVER (2026, on π0.5: 14% better real-world success
with test-time planning), ω-EVA (arXiv 2606.09457), GPC, RoboMonkey;
Fast-WAM (arXiv 2603.16666) asks whether it's needed at all. Seen in
surveys (arXiv 2605.00080, 2606.00113).

**Q4. LLM planning under communication limits.** Nothing found that tests
LLM multi-robot planning with delayed or lost messages. Ao, Gao, and
Simchi-Levi (arXiv 2603.26993, a technical note) bound what delegated
multi-agent LLM planning loses under a communication budget, in theory.
Non-LLM work on degraded communication (task allocation, exploration with
over 50% loss) shows it changes what works best.

**Q5. Robot foundation models are hierarchical.** π0.5 (CoRL 2025) and
π0.6 (model card, November 2025): one VLA predicts a language subtask,
then an 860M-parameter action expert outputs action chunks (63 ms per
chunk). π*0.6 adds RL from experience (RECAP). π0.7 (April 2026, read as
a summary): steered by language coaching, metadata, and subgoal images
from a world model, with a separate high-level policy writing subtasks.
GR00T N1 and Figure's Helix are described as dual systems (secondary
sources).

**Q6. Guards over learned components.** Zongo and Wei (DASC 2026, arXiv
2607.10014): overriding a learned drone separation policy's actions barely
helped under GPS degradation; correcting its inputs cut near mid-air
collisions 90%. Black-Box Simplex (arXiv 2102.12981). ASTM F3269-21.

**What this leaves open.** Checking LLM plans before execution is
established (SayPlan, LLM-Modulo, Khatana et al.), so a checker alone
isn't new. Open, as far as this pass found: LLM fleet planning outdoors
over a network that delays and drops messages; fixed-wing vehicles; the
gap between idealized and realistic execution, measured; and a checker
that simulates full dynamics and communication fast enough to use online,
compared against geometric checks and reactive guards. LLM latency itself
(seconds per call) against vehicles that can't stop is a concrete
realistic effect nobody has measured.

**Still to do before writing.** Read in full: the Li et al. survey's
remaining sections, SayPlan, COHERENT, Khatana et al., RoCo, SMART-LLM,
TypeFly, SwarmGPT. Follow citations forward from SayPlan, COHERENT, and
Khatana et al. Check CoRL, RSS, ICRA, and IROS 2025 and 2026 accepted
lists for "LLM" with "multi-robot", "UAV", or "verification".

## What learning papers usually show

- Benchmarks (MPE, SMAC, VMAS, JaxMARL): point or game physics, agents see
  each other exactly or nearly so, and messages never get lost.
- Swarm flight trained in sim and flown on Crazyflies (Batra et al., CoRL
  2021; OmniDrones). This is learned low-level control.
- Learned communication (CommNet, IC3Net, TarMAC), always tested on perfect
  channels.
- Gorsane et al. (NeurIPS 2022) went through 75 cooperative MARL papers
  and found inconsistent seeds, metrics, and reporting, then proposed a
  standard evaluation protocol.
- Complaints reviewers raise about this kind of work, from the survey (not
  from one paper): toy physics, free communication, partial observability
  that doesn't matter, no classical baseline.

## Cooperative navigation

The task: N agents cover N landmarks. It's MPE's `simple_spread` (Lowe et
al., NeurIPS 2017).

- Reward: minus the summed distance from each landmark to its nearest agent,
  and -1 per collision. `simple_spread_v3` uses N = 3, 25 steps, with the
  collision term weighted 0.5. It now lives in Farama's MPE2 package;
  PettingZoo dropped MPE.
- Actions are forces (scaled by 5, with velocity damping 0.25), not
  velocities. Same in VMAS by default.
- The original MPE counted every agent as colliding with itself. MPE2 doesn't,
  so old and new numbers don't compare.
- With three agents the task is basically solved. In Papoudakis et al.
  (NeurIPS 2021) Table 3, most algorithms land between -126.6 and -134.4.
  Papadopoulos et al. (AAMAS 2025) went to 4, 5, and 8 agents for that
  reason.
- Learned vs optimal assignment has only been done by Muthusamy et al.
  (arXiv 2409.19829; ICRA acceptance unverified), with simple point robots in
  their own environment:

  | Policy | Coverage | Collisions |
  | --- | --- | --- |
  | Hungarian | 0.84 | 4.1 |
  | CAPT | 0.70 | 0 |
  | Graph network after RL | 0.76 | 0.02 |

  Nobody has done it on `simple_spread` itself.
- CAPT is Turpin, Michael, and Kumar (IJRR 2014): assignment plus trajectories
  that can't collide in open space.

### Which MPE, and what the published numbers mean

Same task name, different environments. What changed, in order:

- OpenAI's MPE (2017): velocity is updated before position, and every agent
  counts itself as a collision (-1 every step). MAPPO's repo carries its own
  copy of this, with landmarks in 0.8 × [-1, 1]² and no `local_ratio`.
- PettingZoo 1.18.1 dropped the self-collision. PettingZoo 1.23 (May 2023,
  PR #965) moved the position update before the velocity update, to match
  the Mordatch and Abbeel paper, and renamed `simple_spread_v2` to `v3`.
- MPE2 (Farama, pinned at v1.1.1, commit `147a8e8`) is PettingZoo 1.25's MPE
  moved to its own package. The two give bit-identical observations and
  rewards over 400 seeded episodes. PettingZoo 1.26 removed MPE; 1.25 needs
  Python 3.12 or older. BenchMARL trains on it with `max_cycles: 25` (its
  default is 100).
- MPE2's and OpenAI's discrete actions point opposite ways: action 1 is -x
  in MPE2 and +x in OpenAI's MPE and the forks of it.

### Reproducing Papoudakis et al. 2021

A side check on our training setup, outside Ripple: reproducing it shows
the setup works, as a sanity check for policies we train on MPE2, not proof
they are trained well. Albrecht's group at
Edinburgh (the authors also wrote the MIT Press MARL textbook); NeurIPS 2021
Datasets and Benchmarks; about 400 citations; EPyMARL is its code (740 GitHub
stars). Table 3 ("maximum returns", parameter sharing) gives nine algorithms
on Spread from -126.6 (QMIX) to -204.3 (COMA); MAPPO is -133.54 ± 3.08.

- Environment: the `semitable/multiagent-particle-envs` fork (commit
  `2cef12f`), which is OpenAI's MPE with gym registration and seeding:
  velocity before position, the self-collision, landmarks in [-1, 1]², a
  shared team reward, and five discrete actions.
- Return: EPyMARL adds up the three agents' copies of the shared reward, and
  the paper divides that by 3. Only the authors' replies in EPyMARL issues #5
  and #29 say so; issue #29 is someone getting -500 to -700 without it.
- Protocol: MAPPO for 20M steps with Table 24's hyperparameters (64 hidden,
  learning rate 0.0005, entropy 0.01, reward standardisation, soft target
  update 0.01; the repo's config file has a hard update every 200 steps, so
  override it); 41 evaluations of 100 episodes with sampled actions; the
  best evaluation, averaged over 5 seeds.
- It still runs: EPyMARL `a40db06` (October 2021) trains MAPPO on the fork
  with Python 3.8, gym 0.17.3, PyYAML 5.3.1, torch 2.2.2, sacred 0.8.2, and
  SMAC, at about 5,000 steps/s on an M-series Mac, so about an hour per seed.
- A quick, informal check of the return scale on the fork (200 episodes, no
  retained script): random actions score about -229 in the paper's
  accounting, and an untuned assignment-plus-greedy-steering policy about
  -118. That is not comparable with Table 3 as it stands: their numbers are
  the best of 41 evaluations averaged over 5 seeds. Score the classical
  baseline properly (script, starts, distribution, one stated protocol)
  before putting it beside the table.
- Muthusamy et al.'s code is public too (`Damowerko/motion-planning`, MIT),
  but for a different problem: about 100 velocity-commanded robots in their
  own environment.

## Collision avoidance

The task: agents on a circle fly to the opposite side. The classical baseline
is ORCA. The learned "wins" are weaker than they first look:

- CADRL (Chen et al., ICRA 2017), 8 agents: mean extra time 0.96 s for ORCA,
  0.54 s for CADRL. The "26% better" is for the hardest quarter of cases; the
  median is about the same.
- GA3C-CADRL (Everett et al., IEEE Access 2021), 4 agents: ORCA failed 12% of
  runs vs 1%. But ORCA's radii were inflated 5%, because without that over
  half of 10-agent ORCA runs collided from discrete time steps.
- Long et al. (ICRA 2018), 20 robots on a circle: 0.965 success learned vs
  0.78 for NH-ORCA. NH-ORCA got true neighbor states; the learned policy used
  a laser.
- Fan et al. (IJRR 2020) mix a learned and a classical controller. GCBF+
  (Zhang et al., T-RO 2025) learns barrier functions over several dynamics,
  including Dubins cars.
- Constant-speed vehicles break ORCA (Durand, Transportation Research C,
  2018): aircraft converging at small angles settle on parallel tracks and
  never resolve the conflict. 2-D, turn rate limited to 3 or 6 deg/s. Even
  with ±15% speed variation, ORCA failed more than 20% of runs with 10
  aircraft; his constant-speed variant (CSORCA) failed 10% with 90. We found
  no learned vs classical circle crossing on fixed-wing.
- Unverified: a Sensors 2020 claim that NH-ORCA falls apart with sensor
  noise; the authors of a paper on ORCA deadlocks in deterministic sims.

## Flocking

- Tolstaya et al. (CoRL 2019): 100 agents, a graph network trained to imitate
  a centralized controller. It beats the local controller at 1 m
  communication range; at 4 m the local controller beats every network,
  which the authors say may come from how they built the network's inputs.
  Their appendix also flies up to 50 quadrotors in AirSim: networks trained
  in AirSim against networks trained on a point-mass model tuned to AirSim,
  with the local and global classical controllers as the bounds. So point
  agents to drones, with classical comparators, is already done for
  flocking; what isn't is packet loss and delay, varied one at a time.
- STGNN (Chen et al., arXiv 2309.17437; ICRA 2024 per the authors, not
  confirmed): with 20 agents, completion 1.00 against 0.85 for
  Olfati-Saber's decentralized algorithm, whose tuning isn't described.
  Flown on Crazyflie 2.1s.
- Vásárhelyi et al. (Science Robotics 2018): 30 drones flying outdoors, with
  11 parameters evolved in a sim that had delay and noise.
- Gao and Gündüz (arXiv 2302.08447): graph-network flocking over fading radio
  channels, but no classical baseline.

## Pursuit (dropped)

OPEN (Chen et al., RA-L 2025) reports 0.987 capture against near zero for the
classical baselines in one of its four test scenarios (0.01 to 0.73 in the
others). The baselines treated the quadrotor as a point mass behind a velocity
controller, while the learned policy trained with calibrated dynamics. de Souza
et al. (RA-L 2021) trained and tested against their own evader. Results depend
too much on the evader.

## Closest to what we're doing

- Labiosa and Hanna (ICRA 2025): trained robot soccer policies in a
  deliberately low-fidelity sim, ablated which changes to it made the
  policies transfer to real robots, and matched a well-tuned hand-written
  behavior. Lowering fidelity sometimes helped. Their question is what makes
  transfer work; ours is whether added realism reorders learned and
  classical methods.
- Li et al. (NeurIPS 2025): about 82k cooperative MARL runs under 13 kinds of
  uncertainty, in four environments including quadrotor swarm control and
  environment perturbations. No classical baselines, and no controlled
  change of vehicle with everything else held fixed.
- MASP (Yang et al., Drones 9(7):463, 2025; arXiv 2312.02522): the closest
  to our navigation
  task. A hierarchical MAPPO planner against ORCA, RRT*, and Voronoi
  planners with Hungarian goal assignment, in their own MPE navigation
  variant (discrete actions, 5 to 50 agents, their own maps and time
  limits) and in OmniDrones (5 and 20 quadrotors, per-motor throttle). They
  report learned beating the assignment planners on both: at least 3x
  better execution efficiency (planning cost per step), and 19% (MPE, 50
  agents) to 28% (OmniDrones, 20 agents) fewer steps to finish the task.
  So learned versus Hungarian-backed classical, in MPE and on drones,
  exists. What doesn't: one benchmark
  pinned exactly, matched information and commands, the environments
  changing one thing at a time, and the force-to-velocity switch on its
  own. Their headline leans against our hypothesis 1, on a harder,
  larger-team task.
- Truong et al. (CoRL 2022): for visual navigation on legged robots, a
  lower-fidelity simulator transferred better than a higher-fidelity one.
  A direct counterpoint to "more realistic is closer to reality"; our
  ladder only says what a checked, higher-fidelity simulation says.
- Kadian et al. (RA-L 2020): does performance in sim predict performance on
  real robots? Single agent, learned policies only. Their SRCC is a Pearson
  correlation of the two performances; they also count ranking reversals. We
  borrow the reversal count, and use a rank correlation for the rankings.
- AgentComm-Bench (arXiv 2603.20285, not peer reviewed): latency and loss,
  learned agents only.
- Ourari et al. (ICRA 2022): a learned quadrotor collision-avoidance policy
  where ORCA deadlocked in a delivery task. The deadlock comes from the
  geometry of the task, not from the vehicle; relevant to hypothesis 3,
  which predicts little change on the Crazyflie.
- Flocking under imperfect communication: AirGNN (Gao and Gündüz, IEEE TSP
  2025) trains with wireless fading and noise, and gated GNNs (Marino et
  al., IEEE TCNS 2024) test flocking under delay and range changes. Both
  against an expert controller, not classical rules.

What's left for us, after all of the above: a controlled measurement of how
much a specific modeled effect moves the gap between learned and classical,
with matched information and commands, from a pinned benchmark through an
explicit interface-and-retraining comparison, with retraining at each
setting, and
including fixed-wing. Not "first to compare learned and classical across
simulators": MASP and Tolstaya both did a version of that.
- For the stats: Agarwal et al. (NeurIPS 2021) and their `rliable` library.
- Where to send it: CoRL or RA-L if one step of the ladder is real hardware
  or ArduPilot SITL; NeurIPS Datasets and Benchmarks as a protocol paper;
  TMLR if the result is mixed.

## Sims people train in

- **GPU, thousands of worlds at once:** VMAS (PyTorch, 2-D, has message
  actions with optional noise), JaxMARL (MPE, SMAX, Hanabi; its MPE matches
  PettingZoo), Isaac Lab, Crazyflow (Crazyflie swarms in JAX),
  MuJoCo-Drones-Gym (aerodynamic effects only on its CPU backend). They're
  fast because everything is regular array math. Messages that queue up with
  their own delays are the kind of thing that doesn't fit.
- **CPU sims feeding a GPU learner:** PufferLib (15-20M steps/s in 4.0),
  Sample Factory, RLtools (a Crazyflie policy in about 18 s).
- **Communication** is usually an action or part of the observation. Real
  queued delivery with delay and loss comes from hooking up ns-3 (DEFIANCE,
  2025).
- **Train fast, test somewhere realistic:** Pyquaticus (MOOS-IvP dynamics,
  then real boats), the CPM Lab benchmark (sim, digital twin, hardware), BVR
  Sim (JSBSim; policies give commands to per-aircraft controllers).

## Where Ripple fits

What's different about Ripple is the combination: sensing, message delivery,
and estimation all feed decisions that only see belief; real controllers and
vehicle models sit under the decision; and every run is byte-identical at any
worker count, so learned and classical policies can run on the exact same
episodes.

What isn't special, or isn't true yet:
- Communication and partial observability on their own. VMAS and MPE have
  both.
- Network delay. `SphereNetwork` rejects delay settings.
- One command interface for all vehicles. SingleIntegrator takes velocity,
  the aircraft controllers take heading, speed, and altitude, and Multirotor
  only takes motor speeds.
- Measured training throughput. Light workloads run about 1.4M agent-steps/s
  per core (`paper/data/speed-cpp-native/`), but that isn't an RL number:
  resets, observations, and rewards may be the real cost.

## Unverified

- Goarin and Loianno's numbers; the MDPI coverage paper (page returned 403).
- Acceptance of Muthusamy et al. at ICRA 2025; the venues of JaxMARL,
  Cambridge RoboMaster, and STGNN.
- A Sensors 2020 claim that NH-ORCA degrades sharply with noise.
- AgentComm-Bench and Mednikov and Gal are unreviewed preprints.
- Repeat the novelty search before submission.

## Sources

- PettingZoo MPE: https://pettingzoo.farama.org/environments/mpe/simple_spread/
- VMAS: https://arxiv.org/abs/2207.03530
- JaxMARL: https://arxiv.org/abs/2311.10090
- BenchMARL: https://jmlr.org/papers/v25/23-1612.html
- MAPPO: https://arxiv.org/abs/2103.01955
- Papoudakis et al.: https://arxiv.org/abs/2006.07869
- Papadopoulos et al.: https://arxiv.org/abs/2502.04773
- Gorsane et al.: https://arxiv.org/abs/2209.10485
- rliable: https://arxiv.org/abs/2108.13264
- Muthusamy et al.: https://arxiv.org/abs/2409.19829
- CADRL: https://arxiv.org/abs/1609.07845
- GA3C-CADRL: https://arxiv.org/abs/1910.11689
- Long et al.: https://arxiv.org/abs/1709.10082
- Fan et al.: https://arxiv.org/abs/1808.03841
- GCBF+: https://arxiv.org/abs/2401.14554
- RVO2: https://github.com/snape/RVO2
- Blocked airplanes: https://arxiv.org/abs/2411.16911
- Tolstaya et al.: https://arxiv.org/abs/1903.10527
- STGNN: https://arxiv.org/abs/2309.17437
- Vásárhelyi et al.: https://www.science.org/doi/10.1126/scirobotics.aat3536
- Gao and Gündüz: https://arxiv.org/abs/2302.08447
- OPEN: https://arxiv.org/abs/2409.15866
- de Souza et al.: https://arxiv.org/abs/2010.08193
- Labiosa and Hanna: https://arxiv.org/abs/2503.05092
- Li et al.: https://arxiv.org/abs/2510.11824
- Kadian et al.: https://arxiv.org/abs/1912.06321
- MASP: https://arxiv.org/abs/2312.02522
- Truong et al.: https://arxiv.org/abs/2207.10821
- Ourari et al.: https://arxiv.org/abs/2104.14912
- Marino et al.: https://arxiv.org/abs/2305.19235
- AgentComm-Bench: https://arxiv.org/abs/2603.20285
- Crazyflow: https://arxiv.org/abs/2606.01478
- MuJoCo-Drones-Gym: https://arxiv.org/html/2606.08039v1
- Isaac Lab: https://arxiv.org/abs/2511.04831
- PufferLib 4.0: https://puffer.ai/blog/4.0/
- Pyquaticus: https://github.com/mit-ll-trusted-autonomy/pyquaticus
- CPM Lab zero-shot benchmark: https://arxiv.org/abs/2601.16578
- BVR Sim: https://arxiv.org/html/2608.25419v1
- DEFIANCE: https://dl.acm.org/doi/10.1145/3747204.3747205

# Learned vs classical: what's out there

Notes from the literature search behind the [learned policies
plan](TODO.md#learned-policies) and the paper's task and protocol sections.
Done in September and October 2026. Citations were checked against arXiv or
the publisher unless marked unverified.

The papers themselves, with their code, download with
`scripts/build_paper_sources.py` (see
[paper/literature_review](../paper/literature_review/README.md)).

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
  uncertainty. No classical baselines, no vehicle dynamics.
- Kadian et al. (RA-L 2020): does performance in sim predict performance on
  real robots? Single agent, learned policies only. Their SRCC is a Pearson
  correlation of the two performances; they also count ranking reversals. We
  borrow the reversal count, and use a rank correlation for the rankings.
- AgentComm-Bench (arXiv 2603.20285, not peer reviewed): latency and loss,
  learned agents only.
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
- AgentComm-Bench: https://arxiv.org/abs/2603.20285
- Crazyflow: https://arxiv.org/abs/2606.01478
- MuJoCo-Drones-Gym: https://arxiv.org/html/2606.08039v1
- Isaac Lab: https://arxiv.org/abs/2511.04831
- PufferLib 4.0: https://puffer.ai/blog/4.0/
- Pyquaticus: https://github.com/mit-ll-trusted-autonomy/pyquaticus
- CPM Lab zero-shot benchmark: https://arxiv.org/abs/2601.16578
- BVR Sim: https://arxiv.org/html/2608.25419v1
- DEFIANCE: https://dl.acm.org/doi/10.1145/3747204.3747205

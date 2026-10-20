# Multi-Robot and Aerial Simulation Engines, 2018-2026

Scope: the engine landscape a deterministic CPU multi-agent engine (Rust; point
agents, multirotors, fixed-wing 6-DOF in one world; plugin categories;
byte-identical output at any thread count; ArduPilot SITL; MCAP/Rerun replay)
positions against. Research date 2026-10-21. Items marked **[UNVERIFIED]** come
from the researcher's prior knowledge and were not confirmed against a source
in this session; check before citing.

## Q1. Engine landscape: core claim, heterogeneity, APIs, venue

### Takeaway
The field splits into (a) GPU-batched RL simulators for multirotors only
(Aerial Gym, OmniDrones, Crazyflow, DiffAero, MuJoCo-Drones-Gym), (b)
high-fidelity rendering simulators (AirSim/AirGen, Flightmare, OpenFly), (c)
general robotics simulators with SITL (Gazebo, Webots, CoppeliaSim), and (d)
new LLM-agent platforms that abstract physics away behind REST/MCP tool calls
(MultiUAV-Plat, GRID/AirGen, ROS-MCP servers). Fixed-wing support is rare and
almost never combined with multirotors under one mission-level plugin API.

### Cited Findings

Comparison survey (best single source for the aerial landscape):
- Dimmig, Silano, McGuire, Gabellieri, Hönig, Moore, Kobilarov, "Survey of
  Simulators for Aerial Robots," arXiv:2311.02296 (v7, 12 May 2025), grew out
  of an ICRA 2023 workshop; no peer-reviewed venue named on the page —
  [arXiv](https://arxiv.org/html/2311.02296v7)
- Its Table VI fixed-wing / multirotor / swarm columns: Gazebo Classic and new
  Gazebo: fixed-wing yes, multirotor yes, multi-vehicle but not swarm-specific;
  AirSim: fixed-wing yes, multirotor yes, single-vehicle; Isaac Sim/Gym
  (Pegasus, Aerial Gym): no fixed-wing; Webots: no fixed-wing; CoppeliaSim:
  partial fixed-wing; Flightmare, FlightGoggles, gym-pybullet-drones, RotorTM:
  no fixed-wing; MATLAB UAV Toolbox: both, multi-vehicle not swarm-specific —
  [arXiv](https://arxiv.org/html/2311.02296v7)
- Same survey, Table V APIs/SITL: Gazebo — ROS 1/2, C++, Python (new), PX4 and
  ArduPilot; AirSim — ROS 1, C++, Python, C#, Java, PX4 and ArduPilot; Webots —
  ROS, C/C++, Python, MATLAB, Java, ArduPilot and Crazyflie; Pegasus (Isaac) —
  PX4 only; gym-pybullet-drones — Python/Gymnasium, Betaflight and Crazyflie —
  [arXiv](https://arxiv.org/html/2311.02296v7)
- Venues listed by the survey: Gazebo (Koenig & Howard, IROS 2004), AirSim
  (Shah et al., FSR 2018), Flightmare (Song et al., CoRL 2021 [PMLR]),
  FlightGoggles (Guerra et al., IROS 2019), gym-pybullet-drones (Panerati et
  al., IROS 2021), CoppeliaSim/V-REP (Rohmer et al., IROS 2013), Webots
  (Michel, IJARS 2004), RotorTM (Li et al., T-RO 2023), Pegasus (Jacinto et al.,
  ICUAS 2024) — [arXiv](https://arxiv.org/html/2311.02296v7)
- The same survey notes "Potato" has basic fixed-wing and quadrotor dynamics
  for large swarms but is not open source —
  [arXiv](https://arxiv.org/html/2311.02296v7)

GPU-batched multirotor RL simulators:
- Aerial Gym: Isaac Gym based, GPU-parallel geometric controllers for
  multirotors — [arXiv 2305.16510](https://arxiv.org/pdf/2305.16510);
  [project](https://ntnu-arl.github.io/aerial_gym_simulator/). Journal version
  "Aerial Gym Simulator: A Framework for Highly Parallelized Simulation of
  Aerial Robots," listed as IEEE RA-L 2025 by a secondary list —
  [ResearchGate](https://www.researchgate.net/publication/389616111_Aerial_Gym_Simulator_A_Framework_for_Highly_Parallelized_Simulation_of_Aerial_Robots),
  [awesome list](https://github.com/aimarket/awesome-autonomous-drone-racing)
- OmniDrones: Isaac Sim based multirotor RL platform, arXiv:2309.12825; listed
  as IEEE RA-L 2024 and "no longer actively maintained" by a secondary list —
  [arXiv](https://arxiv.org/pdf/2309.12825),
  [awesome list](https://github.com/aimarket/awesome-autonomous-drone-racing)
- Crazyflow (Schuck, Rath, Hua, Goudar, Zhou, Schoellig; arXiv:2606.01478, 31
  May 2026): JAX simulator compiling physics + controllers into one
  differentiable XLA graph; ~700M steps/s at 1M parallel worlds on a consumer
  GPU (body text also cites >900M steps/s, up to 4.2M drones); ~2 orders of
  magnitude over gym-pybullet-drones at equal parallelism; venue not stated —
  [arXiv](https://arxiv.org/html/2606.01478v1)
- DiffAero (arXiv:2509.10247): GPU-accelerated differentiable quadrotor
  framework — [arXiv](https://arxiv.org/pdf/2509.10247)
- MuJoCo-Drones-Gym (arXiv:2606.08039): GPU multi-drone simulator for control
  and RL — [arXiv](https://arxiv.org/pdf/2606.08039)
- RotorPy (arXiv:2306.04485): Python multirotor simulator with aerodynamics for
  education and research; contributed paper at the ICRA 2023 workshop "The
  Role of Robotics Simulators for UAVs" (workshop, not journal) —
  [arXiv](https://arxiv.org/abs/2306.04485)
- gym-pybullet-drones, "Learning to Fly" (arXiv:2103.02142), multi-agent
  quadcopter control in PyBullet — [arXiv](https://arxiv.org/pdf/2103.02142)

General multi-robot / MARL:
- VMAS (Bettini et al.): vectorized 2D physics engine in PyTorch with 12
  multi-robot scenarios; up to 30,000 parallel sims in under 10 s on GPU; DARS
  2022 — [arXiv](https://arxiv.org/pdf/2207.03530)
- BenchMARL: MARL benchmarking library on TorchRL; docs show heterogeneous
  embodiments trained with MAPPO —
  [docs](https://benchmarl.readthedocs.io/en/1.5.1/usage/notebooks.html). JMLR
  2024 publication **[UNVERIFIED]**.
- MuJoCo Playground: open-source MJX-based robot-learning framework, policies
  in minutes on one GPU; quadrupeds, humanoids, hands, arms; RSS XXI (2025) —
  [RSS](https://roboticsproceedings.org/rss21/p020.html),
  [arXiv](https://arxiv.org/html/2502.08844v1)
- ARGoS (Pinciroli et al., Swarm Intelligence 6:271-295, 2012): partitions
  space into sub-spaces run by different physics engines in parallel;
  multi-threaded for multi-core CPUs; ~10,000 wheeled robots, reported speed
  differs between versions (60% of real time vs 40% faster than real time) —
  [ISTC-CNR](https://istc.cnr.it/en/node/74226),
  [PDF](https://www.argos-sim.info/stuff/Pinciroli:SI2012.pdf),
  [VU](https://research.vu.nl/en/publications/argos-a-modular-parallel-multi-engine-simulator-for-multi-robot-s/)
- Isaac Lab: "A GPU-Accelerated Simulation Framework for Multi-Modal Robot
  Learning," arXiv:2511.04831 — [arXiv](https://arxiv.org/pdf/2511.04831)
- Genesis: rigid + SPH physics for robot learning (described in GPUSimBench) —
  [GPUSimBench](https://arxiv.org/html/2607.13059v1). No peer-reviewed Genesis
  engine paper found **[gap]**.
- Habitat 3.0 (humans, avatars, robots co-habitat), ICLR 2024
  **[UNVERIFIED]**.
- SCRIMMAGE (GTRI; DeMarco et al.): no peer-reviewed engine paper found in this
  session **[gap]**.
- Swarm/MATLAB: SwarmFly (arXiv:2606.25146, MATLAB R2025b, MIT, v2.0.0, plugin
  architecture, eight bundled plugins, "heterogeneous relay/speed" modes) —
  [arXiv](https://arxiv.org/pdf/2606.25146); SwarmLab (MATLAB, built on Beard
  & McLain fixed-wing templates) — [arXiv](https://arxiv.org/pdf/2005.02769)
- UB-ANC Emulator: emulates MAVLink vehicles (fixed-wing and multirotor) with
  ns-3 network co-simulation —
  [SAGE 2019](https://journals.sagepub.com/doi/full/10.1177/1756829319837668)

LLM-agent-oriented aerial platforms:
- MultiUAV-Plat (Zhang et al., NUDT; arXiv:2606.31073, 30 Jun 2026):
  "lightweight, LLM-agent-oriented" platform with REST tool APIs (takeoff,
  land, waypoint, sense, photograph, verify), role-based info access, hidden
  validation, pygame 2D view; no physics engine named; no vehicle-type
  heterogeneity; 75 sessions, 1,500 NL tasks, 9,396 hidden checks; Agent4Drone
  57.93% vs ReAct 30.60% task pass (doubao-2-pro), 70.27% vs 63.60%
  (deepseek-v4-pro); arXiv preprint only —
  [arXiv](https://arxiv.org/html/2606.31073v1)
- MultiUAV-Plat's related work cites Web-of-Drones (MCP / Web-of-Things
  function calling for UAV swarms), TACOS (multi-drone NL coordinator over a
  UAV API library), UAV-CodeAgents (multi-agent ReAct mission generation) —
  [arXiv](https://arxiv.org/html/2606.31073v1)
- GRID / AirGen (Scaled Foundations; arXiv:2310.00887): AirGen is built on
  Microsoft AirSim with synthetic and geo-specific environments; GRID adds
  GPT-4 integration for orchestration and code generation; alpha, free for
  non-commercial research — [arXiv](https://arxiv.org/pdf/2310.00887),
  [GitHub](https://github.com/ScaledFoundations/GRID-playground)
- OpenFly (ICLR 2026): aerial VLN platform combining Unreal Engine, GTA V,
  Google Earth, and 3D Gaussian Splatting; automated toolchain; 100k
  trajectories over 18 scenes; OpenFly-Agent +14.0% / +7.9% success on seen /
  unseen — [arXiv](https://arxiv.org/html/2502.18041v6),
  [ICLR](https://iclr.cc/virtual/2026/poster/10009791)
- OpenUAV, "Towards Realistic UAV Vision-Language Navigation: Platform,
  Benchmark, and Methodology," ICLR 2025, ~12k trajectories —
  [mlanthology](https://mlanthology.org/iclr/2025/wang2025iclr-realistic-a)
- AutoSimTest (arXiv:2501.11864): multiple LLM agents support sUAS simulation
  testing — [arXiv](https://arxiv.org/html/2501.11864v1)
- UAVBench (arXiv:2511.11252): benchmark of LLM-generated flight scenarios —
  [arXiv](https://arxiv.org/html/2511.11252v1)
- AerialClaw (arXiv:2606.12142): open-source framework for LLM-driven aerial
  agents (only references seen; capabilities unconfirmed) —
  [arXiv](https://arxiv.org/pdf/2606.12142)
- ROS/MCP bridges: ROS-MCP-Server (lpigeon) connects Claude/GPT/Gemini to ROS 1
  or 2 via rosbridge, demoed driving a manipulator in Isaac Sim; Gazebo MCP
  servers spawn robots and read sensors; all are community projects, unreviewed
  — [mcp.so](https://mcp.so/servers/ros-mcp-server),
  [glama](https://glama.ai/mcp/servers/cd3wzkc8e2),
  [gazebo-mcp](https://glama.ai/mcp/servers/@kvgork/gazebo-mcp/blob/dfdabc3c8174fc4e88843d7fc4a5f476823cea65/src/gazebo_mcp/__init__.py)

### Inferences
- LLM-agent UAV platforms (MultiUAV-Plat, GRID) trade physics fidelity for a
  tool API; none found pairs an LLM tool interface with 6-DOF fixed-wing +
  multirotor dynamics and a determinism guarantee. That is open space.
- A REST/MCP tool surface over a physical engine is the de facto interface
  pattern for LLM planners in 2025-2026.

### Gaps
- Exact venues for OmniDrones and Aerial Gym rest on a secondary list; BenchMARL
  (JMLR) and Habitat 3.0 (ICLR 2024) not verified; Genesis and SCRIMMAGE engine
  papers not found.
- Web-of-Drones and TACOS primary sources not fetched.
- PX4/Gazebo and ArduPilot SITL lockstep behavior not checked in this session.

## Q2. Which simulators claim determinism, and which discuss thread/GPU nondeterminism?

### Takeaway
Only MuJoCo (CPU) and Isaac Lab document determinism, both scoped to "same
version, same hardware"; Isaac Lab explicitly warns that execution-order
changes alter results. An independent 2026 benchmark measured parallel and
run-to-run divergence in most GPU simulators. No surveyed simulator claims
bit-identical output across thread counts.

### Cited Findings
- MuJoCo docs: simulation is deterministic; within-step multi-threading
  (collision, island constraint solving) is supported; exact reproducibility is
  "only guaranteed within a single version, on the same architecture" —
  [MuJoCo computation](https://mujoco.readthedocs.io/en/stable/computation/index.html),
  [MuJoCo simulation](https://mujoco.readthedocs.io/en/stable/programming/simulation.html)
- A MuJoCo GitHub issue reports reproducibility problems with a thread pool
  (older, resolution unchecked) —
  [issue #1093](https://github.com/google-deepmind/mujoco/issues/1093)
- Isaac Lab: identical results for rigid bodies and articulations on the same
  hardware and Isaac Sim/PhysX version; may vary across hardware from
  floating-point rounding; PhysX "does not guarantee determinism for any scene
  with non-rigid bodies"; "any modification to the execution ordering can
  result in minor changes"; "Deterministic execution can increase memory use
  and reduce simulation performance"; Newton backend offers `run_to_run` and
  `gpu_to_gpu` modes —
  [Isaac Lab docs](https://isaac-sim.github.io/IsaacLab/main/source/how-to/reproducibility.html)
- GPUSimBench (Zhang, Yuan, Yan, Ma, Li, Pang, Yudin; arXiv:2607.13059) with
  fixed seeds, 10 runs, inclined collision: parallel / run-to-run variability
  (cm): Isaac Lab 4.21 / 0.00; ManiSkill 4.76 / 0.00; Genesis 0.00 / 0.00; MJX
  0.00 / 0.00; Madrona 32.53 / 11.47; MuJoCo Warp 4.55 / 2.15; MuJoCo
  Playground 0.00 / 1.39 — [arXiv](https://arxiv.org/html/2607.13059v1)
- GPUSimBench attributes the divergence to GPU atomics and parallel reductions
  (order-dependent float accumulation) and iterative solvers whose small
  perturbations change contact ordering; it excludes CPU simulators (Gazebo,
  PyBullet) — [arXiv](https://arxiv.org/html/2607.13059v1)
- Crazyflow does not discuss determinism or seeds; reproducibility is framed
  as released code and configurations —
  [arXiv](https://arxiv.org/html/2606.01478v1)
- MultiUAV-Plat calls its benchmark reproducible via independently seeded
  sessions; no engine-level determinism claim —
  [arXiv](https://arxiv.org/html/2606.31073v1)
- The aerial-simulator survey makes no per-simulator determinism statements —
  [arXiv](https://arxiv.org/html/2311.02296v7)
- ARGoS is multi-threaded; no determinism statement found in retrieved text —
  [ISTC-CNR](https://istc.cnr.it/en/node/74226)

### Inferences
- "Byte-identical at 1, 2, 8 workers" is a stronger claim than any found:
  existing guarantees fix hardware and version and silently assume fixed
  execution order. GPUSimBench gives a ready external metric (parallel vs
  run-to-run variability) to report against.
- Isaac Lab's own note that determinism costs speed supports reporting both
  throughput and determinism together.

### Gaps
- Webots, Gazebo, ARGoS, SCRIMMAGE, Genesis official determinism statements not
  found.

## Q3. Mixed fixed-wing + multirotor fleets with a mission-level plugin API?

### Takeaway
Fixed-wing + multirotor coexist in Gazebo, AirSim, MATLAB UAV Toolbox, and
UB-ANC (via SITL/MAVLink), but none is a swarm-oriented engine with a
mission-level autonomy/controller/sensor/network plugin API; SCRIMMAGE is the
closest precedent and lacks a peer-reviewed paper found here.

### Cited Findings
- Fixed-wing + multirotor: Gazebo (Classic and new), AirSim, MATLAB UAV
  Toolbox; none flagged as swarm-specific —
  [survey](https://arxiv.org/html/2311.02296v7)
- GPU RL simulators (Aerial Gym, Isaac/Pegasus, gym-pybullet-drones,
  Flightmare) are multirotor-only —
  [survey](https://arxiv.org/html/2311.02296v7)
- UB-ANC emulates MAVLink fixed-wing and multirotor with ns-3 networking —
  [SAGE](https://journals.sagepub.com/doi/full/10.1177/1756829319837668)
- SwarmFly has a plugin model but its "heterogeneous" modes are relay/speed;
  fixed-wing support unconfirmed — [arXiv](https://arxiv.org/pdf/2606.25146)
- Potato: fixed-wing + quadrotor swarms, closed source —
  [survey](https://arxiv.org/html/2311.02296v7)

### Inferences
- A determinism-guaranteed engine holding point agents, multirotors, and
  fixed-wing 6-DOF with per-category plugins has no direct published peer.

### Gaps
- SCRIMMAGE's published description and any fixed-wing support in OmniDrones /
  Aerial Gym forks not checked.

## Q4. What evaluation made engine papers acceptable; engine + LLM-agent papers

### Takeaway
Accepted engine papers rest on (1) throughput tables vs named prior
simulators, (2) sim-to-real error on real hardware, and (3) a benchmark/task
suite with baselines; LLM-agent platform papers replace (1)-(2) with a large
task benchmark and an agent that beats ReAct.

### Cited Findings
- Crazyflow: sim-to-real RMS error on real Crazyflies vs gym-pybullet-drones
  and CrazySim (e.g., circle 9 s: 49.1 / 102.2 / 19.0 mm), up to 61.3% and
  82.8% gap reduction; throughput vs Aerial Gym, DiffAero, gym-pybullet-drones;
  training-time demos — [arXiv](https://arxiv.org/html/2606.01478v1)
- VMAS (DARS 2022): parallel-sim throughput (30,000 sims < 10 s) plus 12
  scenarios with MARL baselines — [arXiv](https://arxiv.org/pdf/2207.03530)
- MuJoCo Playground (RSS 2025): policy training in minutes on one GPU across
  many embodiments — [RSS](https://roboticsproceedings.org/rss21/p020.html)
- ARGoS (Swarm Intelligence 2012): scalability to ~10,000 robots vs real time —
  [ISTC-CNR](https://istc.cnr.it/en/node/74226)
- GPUSimBench: evaluates scalability, physical consistency, determinism, and
  sim-to-real across 7 GPU simulators (Madrona worst sim-to-real 211.60 cm;
  ManiSkill 2.52 cm, MJX 3.97 cm) — [arXiv](https://arxiv.org/html/2607.13059v1)
- OpenFly (ICLR 2026): platform + automated data toolchain + 100k-trajectory
  dataset + agent baseline — [arXiv](https://arxiv.org/html/2502.18041v6)
- OpenUAV (ICLR 2025): platform + benchmark + method —
  [mlanthology](https://mlanthology.org/iclr/2025/wang2025iclr-realistic-a)
- MultiUAV-Plat: platform + 1,500-task benchmark with 9,396 hidden checks +
  Agent4Drone vs ReAct on several LLMs —
  [arXiv](https://arxiv.org/html/2606.31073v1)
- GRID/AirGen: platform paper integrating GPT-4 with an AirSim-based simulator
  — [arXiv](https://arxiv.org/pdf/2310.00887)

### Inferences
- Engine papers that reached ICLR/RSS/CoRL paired the engine with a dataset or
  benchmark and an agent result; pure engine papers went to RA-L/IROS/journals.
- For an LLM fleet-planner study, the winning template is: engine + hidden
  validation checks + multiple LLMs + baseline agent; determinism evidence
  (GPUSimBench-style metrics) would be a differentiator none of these offer.

### Gaps
- No user studies found in any engine paper reviewed.
- Acceptance reasoning (reviews) not available; venue patterns are inferred.

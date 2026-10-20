# LLM planners for multi-robot teams and vehicle fleets (2023-2026)

Scope: language-model planners that command multi-robot teams or UAV fleets,
with emphasis on aerial and heterogeneous fleets. Compiled 2026-10-21.

How the citations were checked. Most entries were read from the full text of
the papers kept in `paper/literature_review/sources/<key>/paper.md`, which are
machine conversions of the PDFs listed in `source_catalog.json`. Title,
authors and arXiv ID were confirmed there. Entries marked "(web)" were
checked only against the arXiv abstract page or HTML version. Entries marked
"(secondary)" were seen only in other papers' citations, and their details
are unverified. Where a venue comes only from `paper/references.bib` and not
from the paper itself, the entry says so.

## Q1. What do the named systems do and show?

### Takeaway
Nearly every system uses the LLM as a high-level planner over a fixed library
of named skills or API calls (go-to, pick, take off, track). Execution below
that library is idealized in most work: symbolic household simulators,
grid worlds, snap-to-gripper or navmesh motion, or "perfect trajectory
tracking". Only a handful reach a real autopilot (PX4 in TACOS, ArduPilot SITL
and quad-planes in COLA, ArduPilot SITL in Web-of-Drones). Evaluation sizes
run from 4 live flights (COLA) to 1,500 tasks (MultiUAV-Plat). Apart from
partial exceptions (MultiUAV-Plat's seeded sessions and hidden validators,
Web-of-Drones' published prompts), none of the papers checked describes
replaying an episode exactly.

### Cited Findings

Catalog (LLM role / robots / low level / sim or hardware / evaluation /
reproducibility / venue):

| System | What the LLM does | Robots (het.? aerial?) | Low level | Sim / hardware | Evaluation | Reproducibility notes | Venue, year | URL |
|---|---|---|---|---|---|---|---|---|
| SMART-LLM (Kannan, Venkatesh, Min) | Few-shot Pythonic prompts: task decomposition, coalition formation, allocation; emits multi-threaded Python over skill API | 1-4 heterogeneous agents in sim; real aerial + ground robots | Pre-implemented skill APIs (GoToObject, PutObject...; real: GoToLocation, ClickPicture, Patrol) | AI2-THOR ("deterministic simulation platform"); real mobile robots | 36 instructions in 4 categories (6 elemental, 8 simple, 14 compound, 8 complex); metrics SR, TCR, GCR, RU, Exe; baselines: decomposition + random allocation, + rule-based allocation; GPT-4, GPT-3.5, Llama-2-70B, Claude-3-Opus | Code and dataset released (SMARTlab-Purdue/SMART-LLM) | IROS 2024 (per references.bib) | https://arxiv.org/abs/2309.10062 |
| RoCo (Mandi, Jain, Song) | One LLM per arm; dialog to agree on subtasks; also proposes 3D task-space waypoints for a motion planner; environment feedback (collision checks) for in-context replanning | Multi-arm manipulators (not aerial) | Sampling-based multi-arm motion planner seeded by LLM waypoints | MuJoCo-based RoCoBench; real arm with human in loop | 6 tasks, 20 runs each; success, env. steps, replans; baseline "Central Plan" oracle + ablations; GPT-4 | Code released (MandiZhao/robot-collab); toy study shows success varies with GPT-4 temperature | ICRA 2024 | https://arxiv.org/abs/2307.04738 |
| Scalable multi-robot collaboration (Chen, Arkin, Zhang, Roy, Fan) | Compares 4 LLM communication structures: DMAS, HMAS-1, CMAS, HMAS-2 | Arms, mobile manipulators, lifting robots with different capacities (heterogeneous capability in BoxLift) | Hand-written text-to-action mapping, discrete actions in grid tasks | 4 2D tasks (BoxNet1/2, Warehouse, BoxLift); also 3D sim | 10 trials per robot count; 4-32 robots (BoxNet), 4-10 (others); 40 runs/task; success, steps, API calls, tokens | Code released (yongchao98/multi-agent-framework) | ICRA 2024 | https://arxiv.org/abs/2309.15943 |
| COHERENT (Liu, Tang, Wang, Wang, Li, Zhao) | Centralized task assigner proposes and assigns subtasks; each robot executor LLM picks a feasible action and reports back (PEFA loop) | Heterogeneous: quadrotors, robot dogs, robot arms | Discrete actions (movetowards, takeoff_from, land_on, grab, putinto) | BEHAVIOR-1K; real quadrotors + Unitree dog + arm | 100-task benchmark (32 mono, 30 dual, 38 trio-type), only 40 tasks run (API cost); SR, avg. steps; baselines DMRS-1D/2D (CoELA-like), CMRS, MCTS, LLM-MCTS; gpt-4-0125-preview | Code and benchmark released (MrKeee/COHERENT) | ICRA 2025 (per references.bib) | https://arxiv.org/abs/2409.15146 |
| CoELA (Zhang, Du, Shan, Zhou, Du, Tenenbaum, Shu, Gan) (web) | Modular agent: perception, memory, communication and planning modules with LLM; decentralized, costly communication | Embodied household agents (not aerial) | High-level plans executed by scripted controllers in the environment | C-WAH, TDW-MAT | Beats planning-based baselines with GPT-4; LLaMA-2 fine-tuned; human-trust study | Not checked | ICLR 2024 | https://arxiv.org/abs/2307.02485 |
| SwarmGPT (Schuck et al.) | Choreographer: GPT-4 picks waypoints or parameterized motion primitives per drone, synced to music; self-correction re-prompting | Homogeneous Crazyflie 2.1 quadrotors | Safety filter (distributed QCQP, AMSwarm-like) makes minimal corrections; onboard position controller | crazyflow simulator (up to 200 drones); hardware up to 20 drones | 7 songs x 10 performances; scaling: 3 runs x 5 songs per swarm size; primitives ~65-85% design success, 85-100% with self-correction, waypoints ~50% at 10 drones -> 0%; 10-person user study | No code repo listed in catalog; project site only | IEEE RA-L, accepted Oct 2025 | https://arxiv.org/abs/2412.08428 |
| MultiUAV-Plat / Agent4Drone (Zhang et al., NUDT) | Agent workflow (memory, observation, task understanding, planning, execution, verification) issues REST tool calls | Multi-UAV; models drones, targets, obstacles, wind; heterogeneity not a focus | Action primitives (takeoff, land, waypoint nav, sense, photograph); paper states it does not address flight stabilization or trajectory optimization | Custom lightweight sim, optional 2D/3D view | 75 sessions, 1,500 tasks, 9,396 hidden checks; ReAct baseline; Agent4Drone 57.9% vs ReAct 30.6% task pass (doubao-2-pro), 70.3% with deepseek-v4-pro | Open source; "five independently seeded sessions" per level; results recomputed by a script | arXiv 2026 (preprint) | https://arxiv.org/abs/2606.31073 |
| TACOS (Nazzari, Rubinacci, Lovera) | Coordinator LLM turns pilot intent into task plans; Supervisor LLM calls executable APIs (goto, start_tracking) | Homogeneous quadrotors (6-20 sim, 3 real) | ATOMICA multi-agent trajectory planner; sim assumes "perfect low-level trajectory tracking"; real drones run PX4 | Custom sim (51 landmarks); indoor mocap arena | Ablation: 50 sim runs per config x {6,12,20} drones, gpt-oss; success, steps; real demo repeated 10 times with behavior varying run to run | No code listed | arXiv 2025 (submitted to IEEE) | https://arxiv.org/abs/2510.01869 |
| LLM-DaaS (Wassim, Mohamed, Hamdi) | Fine-tuned small LLMs (Phi-3.5, LLaMA-3.2, Gemma) extract structured delivery requests; separate selection/composition model picks drones | Delivery drones (service-level abstraction) | No flight dynamics; service composition plus weather data | Simulation | Extraction accuracy and operational efficiency in simulation | Not checked | arXiv 2024 | https://arxiv.org/abs/2412.11672 |
| COLA (Ludwig, Tuohy, Finnigan) | LLM as "natural language transpiler" to JSON function calls; Mission Supervisor agent + 3 UAV agents; reallocate waypoints, RTL, handoffs | Three VTOL quad-planes live; heterogeneous teams explored in sim only | ArduPilot autopilot via MAVProxy / PyMAVLink | ArduPilot SITL; live flights with safety pilots | 4 live flights; flights 1 and 4 succeeded end to end (one RTL exception from wind); flight 2 supervisor declined control; plus automated prompt testing | Third-party-observed flights; no code | IEEE Aerospace Conf. 2026 (per references.bib) | https://stottlerhenke.com/wp-content/uploads/2026/06/2026_IEEE_AERO_Ludwig_COLA.pdf |
| Web-of-Drones (Iannoli, Gigli, Sciullo, Trotta, Di Felice) | Agent Core LLM with function calling through MCP gateway; drones and sensors exposed as W3C WoT Things; no code generation; runtime guardrail prompts | UAV swarm plus ground sensors/actuator | ArduPilot flight control and physics | ArduPilot-based simulation | Coverage (with/without planning tools), formation, smart irrigation; 6 LLMs (GPT 5.2, DeepSeek 3.2, GLM 4.7, Grok 4.1 Fast, Claude Haiku 4.5, Qwen3 8B); binary success, time, tokens | Prompts on Zenodo (10.5281/zenodo.18433213) | IEEE WoWMoM 2026 | https://arxiv.org/abs/2605.03788 |
| Khatana et al. verifier | Centralized vs decentralized multi-LLM planning; geometric verifier grounds plans into trajectories, detects conflicts, returns structured feedback | 2 agents (CARLA vehicles, Crazyflies) | Geometric planner + Crazyflie low-level controller | CARLA; Crazyflie sim; one physical Crazyflie (single-robot) | Target sets of 2, 5, 9 points; success = all targets visited with 7 m separation; planning time, path length | Not stated | arXiv 2026 | https://arxiv.org/abs/2610.12761 |
| GAVEL (Wang, Hsu, Mehta, Kim, Dou, Pajic) | Graph world model predicts consequences of LLM actions, detects and repairs violations; belief over unseen object locations | Single robot (household) | BEHAVIOR-1K actions | BEHAVIOR-1K | 100 long-horizon tasks + 500 multi-task instructions; Qwen3-8B success 41.2% -> 91.8% (single), 19.9% -> 92.6% (multi-task) | Not stated | arXiv 2026 | https://arxiv.org/abs/2609.19315 |
| LaMMA-P (Zhang, Qin, Wang, Dong, Li) (web) | LM allocates subtasks by robot skills and writes PDDL problems; heuristic PDDL planner solves each | Heterogeneous household robots | PDDL plans executed in AI2-THOR | AI2-THOR (MAT-THOR benchmark, 2 complexity levels) | Reports 105% higher success and 36% higher efficiency than prior LM multi-agent planners | Not checked | ICRA 2025 | https://arxiv.org/abs/2409.20560 |
| DART-LLM (Wang et al., U. Tokyo / Tsukuba) (web) | QA LLM decomposes into a DAG of dependent subtasks; breakdown function assigns robots; VLM detector grounds objects | Heterogeneous construction machines (2 tracked carriers, 1 excavator); UAVs only supply imagery | Actuation module for machinery | Unity/PhysX sim; one real task | 102 instructions (47 L1, 33 L2, 22 L3); SR, IPA, DSR, SGSR, RTR; baseline SMART-LLM; L3 SR 0.94 (DeepSeek-r1) vs SMART-LLM 0.24 | Project site "videos and code"; fine-tuned 1B model on HF | arXiv 2024 (v2 2025; venue not stated) | https://arxiv.org/abs/2411.09022 |
| EMOS (Chen et al.) (web) | LLM agents read URDFs and call kinematics tools to write a "robot resume"; hierarchical planning + group discussion | Heterogeneous: Fetch, Stretch, Spot, DJI M100 drone | Physics disabled; navmesh waypoint following; IK with ground-truth object poses; objects snap to gripper | Habitat (Habitat-MAS benchmark, MP3D + HSSD, >70 scenes) | 519-episode ablation set, GPT-4o; success 37.82% (w/o resume 15.63%, w/o numerical 23.56%, w/o discussion 15.23%) | Project site; code release not stated on arXiv page | arXiv 2024/2025 (no venue in comments) | https://arxiv.org/abs/2410.22662 |
| HMAS-2 | Not a separate paper: the best hybrid framework inside Chen et al. 2024 above (central LLM proposes; local agent LLMs give feedback per step) | see Chen et al. | see Chen et al. | see Chen et al. | Success 82.5% / 57.5% / 62.5% / 100% on BoxNet1/BoxNet2/Warehouse/BoxLift vs CMAS 75 / 27.5 / 15 / 90% (GPT-4, 40 runs per task) | see Chen et al. | ICRA 2024 | https://arxiv.org/abs/2309.15943 |
| AutoHMA-LLM (Yang et al.) (secondary) | Cloud-edge hybrid LLMs (Llama2 or GPT-4) for task coordination of heterogeneous agents (UAVs, robots, cars) | Heterogeneous incl. UAVs | Unverified | Unverified | Unverified | Unverified | IEEE Trans. Cognitive Commun. Netw. 11(2), 2025, pp. 987-998 (from citing paper) | cited in https://arxiv.org/abs/2601.14437 and https://arxiv.org/abs/2501.02341 |
| TypeFly (Chen, Yu, Ling, Zhong) (web) | LLM writes plans in MiniSpec, a token-efficient language, with probe/replan | Single drone | Drone skill library | Real drone | Up to 62% lower response time vs Python plans | Not checked | arXiv 2023 (venue unverified) | https://arxiv.org/abs/2312.14950 |
| REAL (Tagliabue, Kondo, Zhao, Peterson, Tewari, How) (web) | LLM in a multirotor's mission-planning and control loop, queried offboard at ~0.1-1.0 Hz; reduces tracking error under wrong controller parameters or unmodeled dynamics | Single multirotor | Classical controller under LLM supervision | Sim + hardware (per secondary summary) | Position tracking error | Not checked | arXiv 2023 (venue unverified) | https://arxiv.org/abs/2311.01403 |

Further findings:

- SMART-LLM GPT-4 success falls from 1.00 on elemental tasks to 0.69
  (compound) and 0.71 (complex); random allocation after the same
  decomposition reaches 0.00 on complex tasks — [SMART-LLM, Table
  I](https://arxiv.org/abs/2309.10062) (local copy).
- In RoCo, the oracle "Central Plan" often matches or beats dialog (e.g.
  Pack Grocery 0.82 vs 0.44 success), while dialog wins on Sort Cubes (0.93
  vs 0.70) — [RoCo, Table 2](https://arxiv.org/abs/2307.04738) (local copy).
- Chen et al. find decentralized dialog (DMAS) worst on success and on token
  cost (e.g. 0% on Warehouse), and that a centralized proposal with local
  feedback (HMAS-2) scales best to 32 robots — [Chen et al.
  2024](https://arxiv.org/abs/2309.15943) (local copy).
- COHERENT reports 0.975 average success on its 40-task subset vs 0.625 for
  a centralized baseline (CMRS) and 0.600 for a CoELA-style decentralized
  baseline; it ran 40 of 100 tasks "considering the cost of the OpenAI API" —
  [COHERENT, Table II](https://arxiv.org/abs/2409.15146) (local copy).
- SwarmGPT shows that restricting the LLM to parameterized motion primitives
  raises design success over raw waypoints as swarm size grows; a safety
  filter, not the LLM, guarantees collision-free deployment — [SwarmGPT, Sec.
  IV](https://arxiv.org/abs/2412.08428) (local copy).
- MultiUAV-Plat's Agent4Drone run on deepseek-v4-pro cost about USD 60 for
  more than 35K requests over the 1,500-task benchmark; paired analysis shows
  211 ReAct failures converted to passes and 111 regressions — [MultiUAV-Plat,
  Table 4 and Fig. 5](https://arxiv.org/abs/2606.31073) (local copy).
- TACOS's real-flight demo, repeated 10 times, behaved the same on the first
  command but varied on the second (sometimes two drones monitored the dog
  and the third went to the villa) — [TACOS, Sec.
  VI](https://arxiv.org/abs/2510.01869) (local copy). Average inference time
  was 9.31 s (Coordinator) and 5.95 s (Supervisor) — same source.
- Web-of-Drones concludes that general-purpose LLMs "still struggle to
  achieve reliable execution—even for simple swarm tasks" without grounding,
  that task-specific planning tools and runtime guardrails improve
  robustness, and that token use does not track success — [Iannoli et al.
  2026](https://arxiv.org/abs/2605.03788) (local copy). Guardrails were
  "derived empirically from repeated failure patterns" — same source.
- COLA's live flights revealed problems not seen in simulation (wind caused
  missed waypoints and a missed automatic RTL; the supervisor once declined
  control citing safety conditions) — [Ludwig et al.
  2026](https://stottlerhenke.com/wp-content/uploads/2026/06/2026_IEEE_AERO_Ludwig_COLA.pdf)
  (local copy).
- Khatana et al. motivate the verifier with the observation that individually
  sensible symbolic plans "can become jointly infeasible after geometric
  grounding"; their multi-robot coordination is evaluated in simulation, and
  the hardware run is a single Crazyflie — [Khatana et al.
  2026](https://arxiv.org/abs/2610.12761) (local copy).
- Ao, Gao and Simchi-Levi prove that, without new exogenous signals, any
  delegated multi-agent LLM network is decision-theoretically dominated by a
  centralized Bayes decision maker with the same information — [Ao et al.
  2026](https://arxiv.org/abs/2603.26993) (local copy).
- AUSPEX is an open-source ROS 2 framework for heterogeneous multi-UAV
  planning and execution that hosts interchangeable planners (Unified
  Planning, ALNS, and "Say'n'Fly, an LLM-modulo planner"), with HIL
  validation on a search-and-rescue scenario and SIL hooks to third-party
  simulators — [Michel et al., AAMAS 2026 demo
  track](https://ifaamas.org/Proceedings/aamas2026/pdfs/QTXJ5819.pdf).
- NeLV (Yuan et al.) is a position paper that proposes a five-level taxonomy
  from LLM-as-Parser to LLM-as-Autopilot for larger, longer-range UAVs, with
  multi-UAV patrol, multi-POI delivery and multi-hop relocation use cases but
  no quantitative evaluation on the abstract page — [arXiv
  2510.21739](https://arxiv.org/abs/2510.21739) (web).

### Inferences
- The field's common architecture is "LLM over a skill API". The open
  question is no longer whether an LLM can emit a plausible plan; it is
  whether the plan stays feasible once it is grounded and executed (Khatana,
  GAVEL, SwarmGPT's safety filter and Web-of-Drones' guardrails all address
  this).
- Real autopilots appear only in UAV systems papers (COLA, Web-of-Drones,
  TACOS hardware), and those papers have the smallest or least controlled
  evaluations (4 flights; 10 demo repeats; a few missions per model). The
  large benchmarks (MultiUAV-Plat, COHERENT, Habitat-MAS) use idealized
  primitives. No paper found pairs a large, controlled episode set with
  autopilot-grade dynamics.
- Run-to-run variation is reported only anecdotally (TACOS's 10 repeats,
  RoCo's temperature sweep). None of the papers checked records LLM
  outputs so that an episode can be replayed exactly, and none separates
  LLM sampling variance from simulator variance.

### Gaps
- Venues for SMART-LLM (IROS 2024), COHERENT (ICRA 2025) and COLA (IEEE
  Aerospace 2026) come from `references.bib`; RoCo and Chen et al. at ICRA
  2024 are confirmed by the reference list in TACOS. EMOS, DART-LLM, TypeFly
  and REAL venues were not confirmed (EMOS may have appeared at a 2025
  conference, but its arXiv comments name none).
- AutoHMA-LLM: no primary copy was read. Method, robots and results are
  unverified beyond one-line descriptions in two citing papers.
- CoELA, LaMMA-P, TypeFly and REAL were checked only from arXiv abstracts or
  secondary summaries, so episode counts and baselines are incomplete.
- Web-of-Drones' per-model success rates were not extracted; only the
  qualitative conclusions are recorded.
- "LLM + PX4/ArduPilot" beyond TACOS, COLA and Web-of-Drones: no further
  multi-vehicle paper was verified. Single-drone LLM-to-autopilot work
  exists (TypeFly, REAL) but is out of fleet scope.
- LLaMAR (Nayak et al., NeurIPS 2024, "Long-horizon planning for multi-agent
  robots in partially observable environments") is cited by TACOS but was not
  read.

## Q2. Which handle heterogeneous robots, and how are capabilities encoded?

### Takeaway
Heterogeneity is common in household and construction benchmarks (SMART-LLM,
COHERENT, EMOS, LaMMA-P, DART-LLM) but rare in UAV work, and no verified
paper mixes fixed-wing and multirotor vehicles under an LLM planner.
Capabilities are encoded as text: skill lists in Python dictionaries
(SMART-LLM), per-robot capability text in the prompt (COHERENT, Chen et al.),
or an auto-generated "robot resume" from URDF and kinematics tools (EMOS).

### Cited Findings
- SMART-LLM encodes "robot skills and object properties ... as Python
  dictionaries"; each robot has a skill set S^n drawn from a global set Δ that
  is "pre-implemented ... or available API calls", and coalition formation
  compares subtask skill needs against robot skills — [SMART-LLM, Sec. III-IV](https://arxiv.org/abs/2309.10062)
  (local copy).
- COHERENT's assigner prompt includes "the capabilities and partial
  observations of all robots"; "each robot's capabilities are represented to
  distinguish different action spaces"; observations are text relations such
  as ON(apple, coffee table) — [COHERENT, Sec.
  III](https://arxiv.org/abs/2409.15146) (local copy). Robot types:
  quadrotors, robot dogs, robot arms — same source.
- EMOS replaces human-written role descriptions with a self-prompted "robot
  resume" built from URDF files and kinematics tool calls; removing the resume
  drops success from 37.82% to 15.63%, and removing numerical capability
  descriptions drops it to 23.56% — [EMOS](https://arxiv.org/abs/2410.22662)
  (web). Embodiments include a DJI M100 drone alongside Fetch, Stretch and
  Spot — same source.
- Chen et al. assume each LLM agent "has full knowledge of the environment
  and each robot's capabilities"; in BoxLift robots have different lift
  capacities and LLMs see box size but not weight — [Chen et al.
  2024](https://arxiv.org/abs/2309.15943) (local copy).
- DART-LLM's heterogeneity is construction machinery (excavator, tracked
  carriers); UAVs supply imagery only — [DART-LLM](https://arxiv.org/abs/2411.09022)
  (web).
- COLA explored "heterogeneous UAV teams with different performance
  characteristics and negotiation protocols that allowed UAVs to report their
  ability to complete assigned tasks" in simulation only; live flights used
  three identical VTOL quad-planes — [COLA, Sec.
  4](https://stottlerhenke.com/wp-content/uploads/2026/06/2026_IEEE_AERO_Ludwig_COLA.pdf)
  (local copy).
- Web-of-Drones exposes each drone and sensor as a W3C WoT Thing with
  metadata (e.g. position, communication range), which the LLM discovers and
  queries through MCP; this standardizes the interface rather than describing
  airframe capability — [Iannoli et al.
  2026](https://arxiv.org/abs/2605.03788) (local copy).
- AUSPEX targets "heterogeneous multi-UAV systems" of "different types or
  sizes" but its HIL demo excerpt shows multirotors — [AUSPEX, AAMAS
  2026](https://ifaamas.org/Proceedings/aamas2026/pdfs/QTXJ5819.pdf).
- A position paper notes that fixed-wing aircraft cannot hover (an LLM must
  plan loiter circles to watch an area) and must respect airfield traffic
  patterns, while a multirotor can hold a 3D point — summarized from a
  search result for [NeLV, arXiv 2510.21739](https://arxiv.org/abs/2510.21739)
  (secondary; the exact passage was not read).
- The Austrian FFG project AMASS aims to evaluate "potential and limits of
  LLM-coordinated Fixed Wing Swarm systems" for wildfire, flood and
  infrastructure monitoring; public material states goals, not results —
  [FFG AMASS slides](https://www.ffg.at/sites/default/files/allgemeine_downloads/thematische%20programme/Mobilitaet/Take%20Off/04_TO_AS2023_AMASS_dApolito_25032025.pdf)
  (secondary, from search snippet).

### Inferences
- Heterogeneity in this literature means different discrete skill sets (can
  fly vs can grab). Different dynamics (minimum airspeed, turn radius,
  endurance) are not represented in any capability encoding seen. A
  multirotor + fixed-wing fleet would test exactly the part current encodings
  skip.
- EMOS's ablation is the only quantitative evidence found that how
  capabilities are described changes plan success by a large margin.

### Gaps
- No verified paper evaluates an LLM planner on a mixed fixed-wing and
  multirotor fleet. AMASS is the nearest lead and has no published results
  found.
- Whether EMOS's drone has dynamics or uses navmesh-style idealized motion
  like the ground robots was not confirmed.

## Q3. Which grow or add skills without retraining?

### Takeaway
All verified systems add capabilities by editing prompts or tool/API
registries rather than retraining the LLM. The most explicit designs are
tool-registry ones: Web-of-Drones (MCP tools over WoT Things), MultiUAV-Plat
(REST action primitives), TACOS (an API library) and COLA (API specification
in the prompt). Fine-tuning appears only for efficiency (LLM-DaaS, DART-LLM's
1B model, CoELA's LLaMA-2).

### Cited Findings
- SMART-LLM assumes skills "are either pre-implemented in the system or that
  there are available API calls"; new skills enter as new entries in the
  prompt's skill list — [SMART-LLM](https://arxiv.org/abs/2309.10062) (local
  copy).
- Web-of-Drones groups tools into core, mission-planning and helper tools,
  and toggles their availability for ablations; helper tools are "optional,
  higher-level wrappers around existing MCP tools" — [Iannoli et al.
  2026](https://arxiv.org/abs/2605.03788) (local copy).
- TACOS's conclusion says "expanding the set of available APIs will further
  enhance the applicability" — [TACOS](https://arxiv.org/abs/2510.01869)
  (local copy).
- COLA's agent prompts include "a detailed API specification, mapping rules,
  and input/output examples", and the control service is "agnostic to the
  flight stack" — [COLA](https://stottlerhenke.com/wp-content/uploads/2026/06/2026_IEEE_AERO_Ludwig_COLA.pdf)
  (local copy). Agents "frequently solved novel problems not explicitly
  covered in their prompts by making effective use of the available
  functions" — same source.
- MultiUAV-Plat exposes "observation, takeoff, landing, waypoint navigation,
  sensing, photography, and verification feedback" as REST primitives "rather
  than simulator internals or low-level flight-control code" — [MultiUAV-Plat,
  Sec. 3.1](https://arxiv.org/abs/2606.31073) (local copy).
- SwarmGPT shows the choice of action interface matters: parameterized motion
  primitives far outperform raw waypoints at scale — [SwarmGPT](https://arxiv.org/abs/2412.08428)
  (local copy).
- AUSPEX swaps whole planners (classical, ALNS, LLM-modulo) behind one
  interface — [AUSPEX](https://ifaamas.org/Proceedings/aamas2026/pdfs/QTXJ5819.pdf).
- LLM-DaaS fine-tunes Phi-3.5, LLaMA-3.2 and Gemma for request extraction —
  [LLM-DaaS](https://arxiv.org/abs/2412.11672) (local copy). CoELA fine-tunes
  LLaMA-2 on agent data — [CoELA](https://arxiv.org/abs/2307.02485) (web).

### Inferences
- "Pluggable skills without retraining" is standard practice, not a
  distinguishing claim. What is missing is skills that are themselves
  swappable between classical and learned implementations behind one
  interface, and evaluated under the same physics. None of the papers checked
  compares a classical and a learned implementation of the same skill under
  an LLM planner.

### Gaps
- No paper found measures how planner success changes as the skill library
  grows (for example, tool-count scaling).

## Q4. Surveys and their stated open gaps

### Takeaway
The main survey (Li, An, Abrar, Zhou) names benchmarks, sim-to-real,
latency, hallucination, numerical reasoning and field deployment as the open
problems, and calls for realistic multi-robot simulators and unified
benchmarks with consistent metrics. Its own deployment table shows that
almost every system is simulation-only or simulation plus a small testbed.

### Cited Findings
- Survey: "Large Language Models for Multi-Robot Systems: A Survey", Peihan
  Li, Zijian An, Shams Abrar, Lifeng Zhou (Drexel) — [arXiv
  2502.03814](https://arxiv.org/abs/2502.03814) (local copy); journal listed
  as Autonomous Robots in `references.bib` (not confirmed from the paper).
- Challenges listed: Insufficient Mathematical Capability, Hallucination,
  Multi-modal Integration Difficulties, Difficulties in Field Deployment,
  Relatively High Latency, Deployment Readiness, Sim-to-Real Gap, Lack of
  Benchmarks — [Li et al., Sec. 7.1](https://arxiv.org/abs/2502.03814).
- "Existing benchmarking systems are primarily designed for indoor
  environments and household applications"; "A unified benchmarking framework
  tailored to multi-robot applications would provide researchers with
  consistent metrics" — [Li et al., Sec. 7.1](https://arxiv.org/abs/2502.03814).
- On sim-to-real: "we are aware of few studies that report matched
  sim-versus-real success rate comparisons on identical LLM-driven tasks";
  prompts are "tuned against idealized simulator states with perfect
  localization, noise-free sensing, and bounded actuation" — [Li et al.](https://arxiv.org/abs/2502.03814).
- The survey asks for simulators "that model LLM latency variance,
  communication packet loss, and perceptual noise as core components of the
  testbed"; deployment-grade robustness to "latency variance, lossy
  communication, and accumulated multi-robot uncertainty remains largely
  unaddressed" — [Li et al.](https://arxiv.org/abs/2502.03814).
- Opportunities listed: Fine-tuning and RAG, Task-specific datasets, Advanced
  reasoning, Lightweight models, Multi-modal and embodied extensions,
  Unstructured environments, Newer LLMs, Realistic Multi-Robot Simulators for
  Sim-to-Real Transfer, Unified LLM-MRS Benchmarks and Metrics — [Li et al.,
  Sec. 7.2](https://arxiv.org/abs/2502.03814).
- Survey Table 2 classifies HMAS-2 as "Sim, up to 32 robots", EMOS as "Sim,
  2 robots/task (4 het. types)", COHERENT as "Sim, up to 3 het. robot types"
  — [Li et al., Table 2](https://arxiv.org/abs/2502.03814).
- UAV-focused surveys found (not read in full): "UAVs Meet LLMs: Overviews
  and Perspectives Toward Agentic Low-Altitude Mobility" — [arXiv
  2501.02341](https://arxiv.org/abs/2501.02341); "Large Language Models for
  UAVs: Current State and Pathways to the Future" — [arXiv
  2405.01745](https://arxiv.org/abs/2405.01745) (both web/secondary).
- MultiUAV-Plat's related work argues that prior UAV LLM systems (TACOS,
  Web-of-Drones, UAV-CodeAgents) "are evaluated as task-specific frameworks
  rather than reusable benchmark suites with large task collections and
  hidden mission-level validators" — [MultiUAV-Plat, Sec.
  2](https://arxiv.org/abs/2606.31073) (local copy).

### Inferences
- The survey's "unified benchmark" and "realistic multi-robot simulator"
  gaps, plus MultiUAV-Plat's critique, support a testbed that couples
  repeatable episodes with vehicle dynamics. The survey does not ask for
  exact replay; that would be an argument the paper makes itself.

### Gaps
- The two UAV-LLM surveys were not read beyond titles and snippets; their
  open-gap lists are not recorded here.

## Q5. Closest competitors to "an engine where an LLM commands a heterogeneous fleet (multirotor + fixed-wing) through pluggable classical/learned skills, evaluated on exactly replayable episodes"

### Takeaway
No verified work combines all four elements. The closest are MultiUAV-Plat
(benchmark scale, but idealized primitives and homogeneous UAVs),
Web-of-Drones and COLA (real autopilot, tool interfaces, but few runs and
no replay), COHERENT and EMOS (heterogeneity with aerial members, but
household simulators and idealized motion), SwarmGPT and TACOS (aerial,
safety or trajectory layers, but homogeneous quadrotors), and Khatana et al.
(verifier between LLM and execution, two agents).

### Cited Findings
1. **MultiUAV-Plat / Agent4Drone (2026)** — Showed: 1,500 tasks, 9,396
   hidden checks, seeded sessions, multiple backend LLMs, paired comparison
   vs ReAct (57.9% vs 30.6% on doubao-2-pro). Lacks: flight dynamics
   (explicitly out of scope), fixed-wing or heterogeneous airframes, and
   pluggable learned skills; exact replay of LLM-driven episodes not claimed
   — [arXiv 2606.31073](https://arxiv.org/abs/2606.31073) (local copy).
2. **Web-of-Drones (WoWMoM 2026)** — Showed: tool-based (MCP/WoT) closed-loop
   swarm control over ArduPilot simulation, six LLMs, guardrails and planning
   tools improve robustness. Lacks: heterogeneous airframes, large episode
   counts, replayability; prompts are public but no claim of deterministic
   reruns — [arXiv 2605.03788](https://arxiv.org/abs/2605.03788) (local copy).
3. **COLA (IEEE Aerospace 2026)** — Showed: LLM-to-JSON function calls over
   ArduPilot, live flights with three VTOL quad-planes (hybrid fixed-wing
   airframes) and third-party-observed results. Lacks: quantitative
   benchmark (4 flights, 2 fully successful), heterogeneous teams only in
   sim, no replay — [COLA PDF](https://stottlerhenke.com/wp-content/uploads/2026/06/2026_IEEE_AERO_Ludwig_COLA.pdf)
   (local copy).
4. **COHERENT (ICRA 2025)** — Showed: heterogeneous team including
   quadrotors, PEFA assigner/executor loop, 0.975 success vs 0.625 central
   baseline on 40 tasks, real heterogeneous demo. Lacks: flight dynamics
   (discrete BEHAVIOR-1K actions), only 40 of 100 tasks run, single run per
   task reported, no fixed-wing — [arXiv 2409.15146](https://arxiv.org/abs/2409.15146)
   (local copy).
5. **EMOS (2024/2025)** — Showed: capability encoding from URDF ("robot
   resume") matters (37.8% vs 15.6% success), heterogeneous team with a
   drone, 519-episode ablation. Lacks: physics (disabled; snap grasping,
   navmesh motion), outdoor or fleet scale, replay — [arXiv
   2410.22662](https://arxiv.org/abs/2410.22662) (web).
6. **SwarmGPT (RA-L 2025)** — Showed: LLM choreography of up to 200 simulated
   and 20 real Crazyflies, with a safety filter guaranteeing feasibility and
   primitives outperforming waypoints. Lacks: heterogeneity, task allocation
   (choreography only), replayable evaluation — [arXiv
   2412.08428](https://arxiv.org/abs/2412.08428) (local copy).
7. **TACOS (2025)** — Showed: coordinator/supervisor LLMs over an API
   library with a multi-agent trajectory planner, 50 runs per config at
   6/12/20 drones, PX4 hardware. Lacks: heterogeneity, realistic dynamics in
   sim ("perfect low-level trajectory tracking"), deterministic reruns
   (hardware behavior varied across 10 repeats) — [arXiv
   2510.01869](https://arxiv.org/abs/2510.01869) (local copy).
8. **Khatana et al. verifier (2026)** — Showed: a geometric verifier between
   LLM plans and execution, centralized vs decentralized comparison, CARLA and
   Crazyflie. Lacks: fleet scale (2 agents), heterogeneity, multi-robot
   hardware, replay — [arXiv 2610.12761](https://arxiv.org/abs/2610.12761)
   (local copy).
- Honorable mention: **AUSPEX** (AAMAS 2026 demo) is the closest
  infrastructure match on the systems side (open source, heterogeneous
  multi-UAV, interchangeable planners including an LLM-modulo planner, HIL
  and SIL), but it reports a demonstration, not a controlled episode
  evaluation — [AUSPEX](https://ifaamas.org/Proceedings/aamas2026/pdfs/QTXJ5819.pdf).
- Background: **Chen et al. 2024 (HMAS-2)** is the standard reference for
  centralized vs decentralized LLM coordination, and Ao et al. 2026 gives a
  theoretical reason to expect centralized planning to dominate — [Chen et
  al.](https://arxiv.org/abs/2309.15943); [Ao et
  al.](https://arxiv.org/abs/2603.26993) (local copies).

### Inferences
- The defensible novelty claim is the combination: (a) heterogeneous
  airframes with different dynamics (multirotor + fixed-wing) rather than
  different discrete skills, (b) a skill layer where classical and learned
  implementations are interchangeable, (c) exact episode replay so that
  LLM-sampling variance and simulator variance can be separated, and (d)
  a controlled episode count large enough for statistics. Each competitor has
  at most two of these.
- COLA's VTOL quad-planes are the only fixed-wing-capable airframes found
  under an LLM fleet planner, and they were flown as a homogeneous team.
- A careful claim would be "we found no prior work that ..." rather than
  "first", since AMASS and AutoHMA-LLM could not be fully checked.

### Gaps
- Exact replay claims: no paper was found that records LLM outputs so that
  episodes can be replayed, but full methods sections were not searched for
  every paper (CoELA, LaMMA-P, EMOS, DART-LLM were checked only from abstract
  or HTML summaries).
- Whether the AMASS project or any 2026 venue paper not indexed by these
  searches already evaluates an LLM over a mixed fixed-wing/multirotor fleet
  is unknown.

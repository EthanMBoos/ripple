# Learning simulators in 2025–26, and where scrimmage-rs fits

A review of recent multi-agent and robot-learning simulators, written to test
the position in the paper's "Learned Policies and Accelerated Simulators"
section (`paper/manuscript/main.tex`, `sec:learning`). The earlier
[literature review](MULTI_AGENT_ROBOTICS_LITERATURE_REVIEW.md) covers the field
up to about 2024; this one looks at what appeared or changed from 2025 through
September 2026. It was revised after an external review corrected several
overstated claims, which are listed at the end.

## Bottom line

1. **GPU simulation and CPU simulation with GPU learners are both thriving.**
   - *Batched-world GPU simulators* keep adding physical fidelity: Isaac Lab
     (paper Nov 2025; Newton an experimental backend in the 3.0 beta), MuJoCo
     Warp, Crazyflow (Jun 2026), MuJoCo-Drones-Gym (Jun 2026), and Assistax
     (RLC 2026).
   - *Fast CPU environments with a GPU learner* also matured: PufferLib went
     from 1M steps/s (2.0) to 15–20M steps/s training (4.0), keeping its
     environments on the CPU on purpose.
2. **Communication and partial observability exist elsewhere.** VMAS and the
   MPE tasks in JaxMARL have communication actions with optional noise. What is
   uncommon is an **explicit pipeline from sensing, through queued message
   delivery with delay and loss, and estimation, to decisions** inside one
   deterministic step. Queued network delivery is usually added by ns-3
   co-simulation (DEFIANCE, 2025).
3. **Several 2025–26 projects separate fast training from a realistic
   evaluation or deployment stage**: Pyquaticus, the CPM Lab zero-shot MARL
   benchmark, and BVR Sim. BVR Sim's policies act through a tactical interface
   above aircraft-specific controllers. These support the approach, and are
   related work to cite, not competitors.
4. **The architecture choices hold up, as independent options.**
   - An interface standard, where computation runs, and whether evaluation is a
     separate stage are independent choices; one system can combine all three.
   - CPU worlds with a GPU learner is a mainstream design, but our throughput
     as an RL environment is unmeasured.
   - GPU planning (sampling-based MPC over thousands of rollouts) is active in
     2026.
   - Repeatable GPU inference is an engineering problem with known partial
     fixes, not a free property.

## How this was gathered

I ran web searches and read abstracts, first pages, or documentation from
arXiv, conference pages, project sites, and source repositories, in September
2026. The evidence levels are marked:

- **[read]**: the abstract, the first pages, the documentation, or the source
  was checked.
- **[snippet]**: the claim comes from search-result text or a secondary page
  and was not checked against the source.

Throughput numbers are the authors' own. They are not comparable across
simulators, because workloads, agent counts, and hardware differ. Absence of a
feature in an abstract does not show that a tool cannot support it.

## What the field is doing

### A. Batched-world GPU simulators

These simulators advance thousands of worlds together on a GPU. They lead on
training throughput and physical detail.

| System | Date | What it is | Multi-agent and communication |
|---|---|---|---|
| Isaac Lab [read] | arXiv Nov 2025; 3.0 beta docs 2026 | NVIDIA's successor to Isaac Gym: GPU PhysX, photoreal rendering, multi-frequency sensors, actuator models, domain randomization. The 3.0 beta adds a multi-backend architecture with Newton as an **experimental** backend. | The reviewer reports a multi-agent environment interface; not checked here. |
| Newton [snippet] | announced Mar 2025, contributed to the Linux Foundation Sep 2025 | Open-source, differentiable GPU physics engine from NVIDIA, Google DeepMind, and Disney Research, compatible with MuJoCo. | Physics engine, not a multi-agent framework. |
| MuJoCo Warp [snippet] | GTC 2025; stable 3.14.0 in Sep 2026 | GPU version of MuJoCo for NVIDIA hardware, with a batch ray-traced renderer. | Physics engine. |
| MJX [read] | MuJoCo documentation | JAX version of MuJoCo. Physical parameters can be batched across worlds (domain randomization); structural fields force recompilation when changed. | Physics engine. |
| Genesis [snippet] | Dec 2024 onward | Python GPU physics platform. Headline 43M FPS figures are for batches of identical scenes; a single environment runs at a few thousand FPS, similar to MuJoCo. | Not multi-agent focused. |
| Crazyflow [read] | arXiv May–Jun 2026 | JAX, differentiable Crazyflie simulator with several abstraction levels. Thousands of swarms of 4,000 drones each; sampling-based obstacle avoidance above half a billion steps/s; a recovery policy trained in flight in 0.38 s. | Swarms, yes; communication not described in the abstract. |
| MuJoCo-Drones-Gym [read] | arXiv Jun 2026 | Gymnasium and PettingZoo multi-drone simulator, a successor to gym-pybullet-drones. The **CPU** backend offers selectable physics (ground effect, drag, downwash), actions, and camera and neighbour-adjacency observations. The **GPU** (MJX) backend omits the aerodynamic effects "to preserve XLA compilability" and lists multiple drones and vision as future work. | Multi-drone on CPU only. A concrete example of the cost of maintaining a second backend. |
| Aerial Gym [snippet] | IEEE RA-L 2025 | Isaac Gym framework for arbitrary multirotors, with GPU geometric controllers for thousands of vehicles and GPU rendering. | Mainly single-vehicle navigation. |
| Assistax [read] | RLC 2026 | JAX and MJX benchmark for assistive robotics. Robot and humanoid partner trained with MARL; up to 412× faster than CPU environments on one GPU. Finds a coordination gap with unseen partners. | Two-agent ad-hoc teamwork. |
| JaxMARL [read: MPE source] | NeurIPS 2024 D&B; arXiv revisions into 2026 | JAX suite: MPE, SMAX, STORM, Overcooked, Hanabi, with IPPO, MAPPO, and QMIX. SMAX training is reported 40,000× faster than SMAC when runs are vectorized [snippet]. | The MPE environments implement communication actions. |
| VMAS [read: source] | 2022, still maintained | PyTorch vectorized 2-D multi-robot simulator, used with BenchMARL and TorchRL. 30,000 parallel simulations in under 10 s [snippet]. | Agents have communication actions and state, with `silent` and `c_noise` parameters. |

**Pattern.** The speed comes from regular data: computation expressed as
compatible array operations over bounded capacities. That does not require
identical worlds. Parameters can vary per world, as in MJX, and populations
can be masked. Structural variety, arbitrary plugin behaviour, and queued
delivery with per-message delay are what become expensive.

### B. GPU batch simulation with a flexible entity model

- **Madrona [read]** (Shacklett et al., ACM TOG / SIGGRAPH 2023) is a research
  game engine that runs thousands of worlds on one GPU. It uses an
  **entity-component-system** design instead of fixed tensors, runs the same
  simulator on GPU or CPU, and exports state as PyTorch tensors.
- **GPUDrive [snippet]** (ICLR 2025) is built on Madrona. Its observation,
  reward, and dynamics functions are written in C++ and compiled to CUDA. It
  runs over 1M steps/s with hundreds of agents per world, and trains on the
  Waymo Open Motion Dataset.

This is the closest architectural relative of our core, with entities and
per-phase systems. It shows that entity-structured simulation can be batched
on a GPU, at the cost of a custom engine and C++/CUDA constraints on
simulation code.

### C. Fast CPU environments feeding a GPU learner

- **PufferLib [read].**
  - 2.0 (RLC 2025): 12 C environments at >1M steps/s per CPU core, trained by
    PPO on one GPU.
  - 3.0: 3–5M steps/s training.
  - 4.0 (2026, blog): 15–20M steps/s on one RTX 5090, after replacing PyTorch
    with custom CUDA.

  The 2.0 paper argues that *"only certain types of environments, such as
  those with heavy physics, are sensible to implement on GPU"*. It blames poor
  adoption on *"extensive system package dependencies, slow source builds, and
  versioning obstacles"*.
- **RLtools / "Learning to Fly in Seconds" [snippet]** (2023–24) trains a
  deployable Crazyflie policy on a laptop CPU in about 18 s.
- **Sample Factory** (ICML 2020) established asynchronous CPU simulation with a
  GPU learner.

**For us.** Our saved benchmarks show about 1.4M agent-steps/s per core on
light workloads (`paper/data/benchmark-native/timings.json`). That is **not an
RL benchmark**. Thousands of interacting agents in one world differ from
thousands of independently resetting worlds, and observation construction,
rewards, resets, and learning can move the bottleneck. Contact sensing is
already slow (0.03–0.09M), because `NoisyContacts` senses every other agent.

### D. Domain simulators built for evaluation and deployment

- **BVR Sim [read]** (arXiv Aug 2026) is an open Gymnasium environment for
  heterogeneous air combat.
  - *Simulation:* JSBSim aircraft; a C++ backend with pybind11 and a Python
    backend; CPU only; 55–260 steps/s on a 16-core desktop.
  - *Interface:* policies act through a tactical action interface (desired
    heading, altitude, speed, and fire) above aircraft-specific inner-loop
    controllers.
  - *Transfer result:* a PPO policy trained on the F-16 wins 45.5% on four
    unseen aircraft with adapted controllers, and 13.2% without. These are
    different aircraft within one flight model, so the result supports the
    controller-level abstraction. It does **not** show transfer across model
    fidelities.
  - *Stated limitations:* the two backends may give different floating-point
    trajectories, datalinks are not modelled, and there is one seed per MARL
    run.
- **Pyquaticus [read]** (MIT Lincoln Laboratory, Boston University, NRL, and
  the University of Maryland; 2025) is a PettingZoo maritime capture-the-flag
  simulator.
  - *Pipeline:* it embeds MOOS-IvP vehicle dynamics so policies move to
    MOOS-IvP and onto real uncrewed surface vehicles.
  - *Motivation:* notes that other maritime simulators either lacked a
    deployment path or had one "at the expense of communication latency."
- **Zero-Shot MARL Benchmark in the CPM Lab [read]** (arXiv Jan 2026) evaluates
  in three stages: simulation, a digital twin, and hardware. It separates
  degradation from control-stack differences and from environmental realism.
- **Factory MARL sim-to-real [read]** (arXiv Sep 2026) and **cooperative-competitive
  craft robots [read]** (ICRA 2026) report real multi-robot deployment of
  policies trained in simulation.
- **SMARTS [snippet]** keeps an agent zoo of trained policies loaded into
  scenarios for benchmarking.

### E. Communication realism

- **DEFIANCE [snippet]** (Burmeister et al., ns-3 conference 2025) integrates
  RL libraries into ns-3 so MARL observations and rewards include network
  delays and communication costs.
- Algorithm papers on learning to communicate over unknown or lossy networks
  (arXiv Jul and Nov 2025) [snippet] show continuing interest.
- Robotics GPU simulators treat communication as an action channel or as part
  of the observation, when they model it at all. Queued delivery with delay
  and loss is rare inside the robotics simulator itself.

### F. GPU computation inside the decision loop

Sampling-based MPC over thousands of GPU rollouts is active and reaching
hardware:
- Hydrax deployed for contact-rich manipulation (arXiv Jun 2026);
- MR-STORM (arXiv Feb 2026);
- Feedback-MPPI (RA-L 2026);
- GATO (arXiv Oct 2025).

All are [snippet]. A single planner plugin in our design could already batch
its own candidate trajectories on a GPU with no new interface.

### G. Determinism on GPUs

Thinking Machines Lab (He, Sep 2025) [read] showed that nondeterminism in LLM
inference endpoints comes mainly from **batch-size-dependent reduction order**
under varying server load. Their forward-pass kernels were already
deterministic for a fixed batch, and batch-invariant kernels made outputs
bit-identical. That result is specific to LLM inference. Other GPU operations
(for example atomics-based reductions, and some library kernels) can be
nondeterministic even with identical input shapes, as framework
reproducibility guides note. For us:
- entity-ordered batches remove one source of variation;
- deterministic kernels and sampling must still be checked;
- the contract is repeated evaluation on a pinned backend and device,
  invariant to worker count;
- agreement between CPU and GPU results, and reproducible training, are
  separate tests.

### H. Evaluation methodology

- An AAMAS 2025 extended benchmark finds that state-of-the-art algorithms on
  SMAC and GRF can underperform standard methods elsewhere [snippet].
- SMACv2 remains the standard warning that a task may not need its
  observations.
- Assistax's coordination gap with unseen partners is the same lesson.

Evaluating on conditions not seen in training is becoming expected practice.

## Where scrimmage-rs fits

Distinctive, as a combination:
- an explicit sensing → delivery → estimation → decision pipeline, with
  per-subscriber queues, network plugins, and autonomy that reads only belief;
- vehicle-specific controllers and motion models beneath the decision;
- byte-identical output at any worker count;
- collision checks and routing within an entity that scale to thousands of
  agents in one world;
- agreement with C++ SCRIMMAGE, which verifies the port. It does **not**
  validate physical realism.

Not distinctive, or not yet true:
- communication or partial observability as such (VMAS and MPE have both);
- fast all-to-all sensing or dense radio traffic, which are still expensive;
- a common tactical interface across motion models. SingleIntegrator takes
  world-frame velocity, the aircraft controllers take heading, speed, and
  altitude, and Multirotor has only an open-loop motor driver;
- a network with delay (`SphereNetwork` rejects delay settings; a custom
  network can return `Delivery::After`);
- measured RL throughput.

The strongest reason to learn here is when the information and decision
structure is the research problem:
- when to transmit and when old information suffices;
- allocation or replanning under delayed, incomplete observations;
- coordination above vehicle-specific controllers;
- learned-versus-classical teammates under identical rules.

Contact-rich manipulation, vision, and low-level flight control belong in
specialist tools. Low-level control also fits our phase order poorly, since
all controller substeps precede all motion substeps.

## Options, as independent choices

| Option | What it provides | Fit |
|---|---|---|
| CPU simulation, imported CPU policy | Learned-versus-classical evaluation | Smallest useful integration |
| CPU worlds, external GPU learner | Training on this testbed's own information, task, and vehicle rules | Default for native learning |
| CPU simulation, batched GPU inference | Faster evaluation of large policies or many agents | When inference dominates the step |
| GPU candidate trajectories in one planner | More planning alternatives per decision | Inside one optional plugin; not necessarily learning |
| GPU physics or sensing kernels | Acceleration of a measured expensive phase | When the gain covers packing, transfers, and synchronization |
| Fully GPU-resident worlds | Large rollout batches with little host–device traffic | New backend with restricted models and its own verification |

Differentiable physics is a further independent choice, and GPU inference
would not provide it.

## Recommended first experiment

Decision after review: build no learning support yet. First build the RQ1
plugins and run RQ1 with classical strategies (nearest known target and
`AuctionAssign`), sweeping range, delay, and loss. Only if that reveals a
decision the classical strategies handle poorly does it become the learning
experiment below. Otherwise learning stays a documented extension point, with
the boundary that SCRIMMAGE-RS supplies experience and a separate learner
updates the policy.

If it goes ahead, the experiment is one small native coordination task whose
outcome depends on shared information:
- agents sense targets only within range and transmit reports over a network
  with range and delay;
- an estimator adds received reports to belief;
- train with CPU worlds and an established external learner, then freeze the
  policy;
- compare it with a classical method while varying one information constraint
  (range, delay, or loss);
- as a control, remove the messages to confirm the policy uses them.

What it needs:
- a range-limited contact sensor on spatial queries;
- a report message and an estimator that merges received reports;
- a network with delay;
- an environment adapter. The adapter takes actions and returns batched next
  belief observations, rewards, and termination and truncation. It resets by
  rebuilding the simulation, which is measured first. One call advances all
  worlds. `Simulation::step` returns pre-step state, so it cannot be used as
  the learning step unchanged.

An imported policy is a smaller, second path for evaluation. It needs matching
observation normalization, frames, action meanings, decision period, and
termination rules, and a bounded neighbour observation if team size will
change.

## Corrections from the external review

- The first draft said the GPU simulators do not model communication. That was
  false for VMAS and JaxMARL's MPE.
- It treated interfaces, compute placement, and staged evaluation as competing
  architectures.
- It described fixed shapes as requiring identical worlds.
- It generalized an LLM-inference determinism result to GPUs in general.
- It described MuJoCo-Drones-Gym's GPU backend with CPU-only features.
- It called Isaac Lab's Newton integration "upcoming", when it is already
  experimental in the 3.0 beta.
- It proposed a communication stress test whose observations bypassed the
  network.
- It assumed a shared GPU interface was needed for planning.

## Open questions and unverified points

- The reviewer reports an Isaac Lab multi-agent interface and early RL support
  in Burn 0.21; neither was checked here.
- GPUDrive's details, JaxMARL's 2026 revision contents, and the DEFIANCE
  specifics come from search text.
- I found no study that trains a multi-agent policy and systematically
  evaluates it under queued communication delay and loss inside the same
  robotics simulator. A narrower search should run before the paper claims
  this gap.

## Sources

- Isaac Lab: https://arxiv.org/abs/2511.04831
- Isaac Lab 3.0 beta multi-backend architecture: https://isaac-sim.github.io/IsaacLab/v3.0.0-beta/source/overview/core-concepts/multi_backend_architecture.html
- NVIDIA Isaac Sim (2026): https://arxiv.org/pdf/2606.03551
- Newton: https://developer.nvidia.com/newton-physics
- MuJoCo Warp: https://github.com/google-deepmind/mujoco_warp
- MJX: https://mujoco.readthedocs.io/en/latest/mjx.html
- Genesis: https://github.com/Genesis-Embodied-AI/genesis-world
- Crazyflow: https://arxiv.org/abs/2606.01478
- MuJoCo-Drones-Gym: https://arxiv.org/html/2606.08039v1
- Aerial Gym: https://ieeexplore.ieee.org/document/10910148/
- Assistax: https://rlj.cs.umass.edu/2026/papers/Paper110.html
- JaxMARL: https://arxiv.org/abs/2311.10090 and https://github.com/FLAIROx/JaxMARL/blob/main/jaxmarl/environments/mpe/simple.py
- VMAS: https://arxiv.org/abs/2207.03530 and https://github.com/proroklab/VectorizedMultiAgentSimulator/blob/main/vmas/simulator/core.py
- Gymnasium vector environments: https://gymnasium.farama.org/api/vector/
- PettingZoo parallel API: https://pettingzoo.farama.org/api/parallel/
- Madrona: https://dl.acm.org/doi/10.1145/3592427
- GPUDrive: https://arxiv.org/abs/2408.01584
- PufferLib 2.0: https://rlj.cs.umass.edu/2025/papers/RLJ_RLC_2025_151.pdf
- PufferLib 4.0: https://puffer.ai/blog/4.0/
- Learning to Fly in Seconds (RLtools): https://arxiv.org/abs/2311.13081
- BVR Sim: https://arxiv.org/html/2608.25419v1
- Pyquaticus: https://github.com/mit-ll-trusted-autonomy/pyquaticus and https://calinbelta.com/wp-content/uploads/2026/03/pyquaticus-1.pdf
- Zero-Shot MARL Benchmark in the CPM Lab: https://arxiv.org/abs/2601.16578
- Factory multi-agent sim-to-real: https://arxiv.org/abs/2609.14567
- Cooperative-competitive craft robots: https://arxiv.org/abs/2602.21119
- SMARTS agent zoo: https://smarts.readthedocs.io/en/latest/benchmarks/agent_zoo.html
- DEFIANCE: https://dl.acm.org/doi/10.1145/3747204.3747205
- Learning to communicate over an unknown shared network: https://arxiv.org/abs/2507.06499
- Robust and efficient communication in MARL: https://arxiv.org/html/2511.11393
- Hydrax sampling MPC deployment: https://arxiv.org/abs/2606.20712
- MR-STORM: https://arxiv.org/abs/2602.10114
- Feedback-MPPI: https://arxiv.org/abs/2506.14855
- GATO: https://arxiv.org/abs/2510.07625
- Defeating Nondeterminism in LLM Inference: https://thinkingmachines.ai/blog/defeating-nondeterminism-in-llm-inference/
- PyTorch reproducibility notes: https://docs.pytorch.org/docs/2.14/notes/randomness.html
- Extended MARL benchmarking (AAMAS 2025): https://arxiv.org/abs/2502.04773
- Burn ONNX import: https://lib.rs/crates/burn-onnx

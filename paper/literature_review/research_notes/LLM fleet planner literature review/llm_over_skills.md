# Language models over a skill library: commanding robots by selecting, composing, or writing skill calls (2022-2026)

Verification legend. **[V]** means title, authors, year, and venue or arXiv
ID were checked against the arXiv, proceedings, or project page in this
session (2026-10-21). **[M]** means the citation comes from the researcher's
prior knowledge with a standard arXiv ID. It was not re-fetched in this
session, so check it before it goes in the paper. **[2nd]** means it was seen
only as a citation in another paper.

## Q1. What the canonical systems show

### Takeaway
By 2024 the core pattern was taken many times over: a language model picks,
sequences, or writes code over a fixed, human-described set of skills, with
grounding added by scoring, feedback, or a planner. The variations are how
skills are exposed: as text options scored by likelihood, as a Python API in
a prompt, as PDDL, as voxel value maps, or as language subtasks inside a VLA.
Later work moved from using a library to growing one: Voyager, BOSS, LOTUS,
SkillGen, ASPIRE, and SkillComposer.

### Cited Findings
**Selecting from a fixed skill set**
- SayCan (Ahn et al., "Do As I Can, Not As I Say: Grounding Language in
  Robotic Affordances", CoRL 2022) **[M]**. The LLM scores the text
  description of each learned skill. That score is multiplied by a learned
  value function (affordance) for the skill, and the best product is
  executed. On a mobile manipulator, PaLM-SayCan reached about 84% planning
  and 74% execution success across 101 instructions, with a library of
  roughly 551 learned skills. The skills are behavior-cloned or RL policies,
  so a new skill needs data and training plus a value function. A skill is
  not usable from its description alone. —
  [arXiv 2204.01691](https://arxiv.org/abs/2204.01691)
- Inner Monologue (Huang et al., CoRL 2022) **[M]**. This adds closed-loop
  text feedback to the LLM planner over fixed skills: success detection,
  scene description, and human feedback. —
  [arXiv 2207.05608](https://arxiv.org/abs/2207.05608)
- Text2Motion (Lin et al., Autonomous Robots 2023) **[M]**. The LLM proposes
  skill sequences, which are checked for geometric feasibility with learned
  skill Q-functions and dynamics. The skills are a small fixed library of
  learned primitives (pick, place, pull, push). —
  [arXiv 2303.12153](https://arxiv.org/abs/2303.12153)
- LLM+P (Liu et al., 2023) **[M]**. The LLM translates the problem into PDDL,
  a classical planner solves it, and the LLM translates the plan back. The
  "skills" are hand-written PDDL operators. —
  [arXiv 2304.11477](https://arxiv.org/abs/2304.11477)

**Writing code over a skill API**
- Code as Policies (Liang et al., ICRA 2023) **[V for content]**. The model
  gets a few-shot prompt: example commands written as comments, followed by
  policy code. It then composes perception and control API calls for new
  commands, and it can write undefined helper functions hierarchically. —
  [project page](https://code-as-policies.github.io/);
  [arXiv 2209.07753](https://arxiv.org/pdf/2209.07753)
- ProgPrompt (Singh et al., ICRA 2023) **[M]**. The prompt is Pythonic. It
  imports the available actions, lists the objects, and gives example
  programs with comments and assertions. The paper ablates prompt components
  (comments, feedback/assertions, number of examples) in VirtualHome and on
  a real robot. —
  [arXiv 2209.11302](https://arxiv.org/abs/2209.11302)
- ChatGPT for Robotics / PromptCraft (Vemprala et al., Microsoft; later IEEE
  Access 2024) **[M]**. This work gives design principles: write a
  high-level function library with descriptive names and describe it in the
  prompt. It shows ChatGPT driving drones in AirSim and on a real Tello,
  plus manipulators. It released PromptCraft, an open prompt collection. The
  claim that descriptive names matter is presented as guidance, not a
  controlled ablation. —
  [arXiv 2306.17582](https://arxiv.org/abs/2306.17582)
- VoxPoser (Huang et al., CoRL 2023) **[M]**. The LLM writes code that
  queries a VLM and composes 3D affordance and constraint value maps, and a
  motion planner then synthesizes trajectories. This is presented as an
  alternative to a predefined primitive library. —
  [arXiv 2307.05973](https://arxiv.org/abs/2307.05973)
- Scaling Up and Distilling Down (Ha et al., CoRL 2023) **[V]**. An LLM
  plans tasks and writes success-check code against simulator-state API
  functions. Verify-and-retry helps in all domains, and without retries the
  mailbox domain drops to 0% success. A 13B model beat a 7B model by +20.0%
  in planning accuracy and +38.3% in verification accuracy. —
  [PMLR v229](https://proceedings.mlr.press/v229/ha23a/ha23a.pdf)

**Growing the library**
- Voyager (Wang et al., 2023) **[V for arXiv metadata]**. The library is "an
  ever-growing skill library of executable code for storing and retrieving
  complex behaviors". Its skills are "temporally extended, interpretable, and
  compositional". It reuses the library "in a new Minecraft world to solve
  novel tasks from scratch". The arXiv page lists no venue (I believe it
  appeared in TMLR 2024, **unverified**). —
  [arXiv 2305.16291](https://arxiv.org/abs/2305.16291)
- BOSS, "Bootstrap Your Own Skills" (Zhang et al., CoRL 2023, oral) **[V]**.
  An agent starts with primitive skills and an LLM proposes which skills to
  chain during a reward-free practice phase. Successful chains become new
  library skills, which are then used for new long-horizon tasks. —
  [PMLR v229 pp. 302-325](https://proceedings.mlr.press/v229/zhang23a.html);
  [code](https://github.com/clvrai/boss)
- LOTUS (Wan, Zhu, Shah, Zhu; ICRA 2024) **[V]**. This is continual
  imitation learning. An open-vocabulary vision model finds recurring
  segments in demonstrations, and those segments become goal-conditioned
  visuomotor skills. A meta-controller composes them. It reports more than
  11% higher average success than baselines. It has no LLM caller. —
  [arXiv 2311.02058](https://arxiv.org/pdf/2311.02058);
  [UT RPL page](https://rpl.cs.utexas.edu/publications/2024/05/13/wan-icra24-lotus/)
- SkillGen / SkillMimicGen (Garrett, Mandlekar, Wen, Fox; CoRL 2024) **[V]**.
  This work splits 60 human demonstrations into skills and generates more
  than 24,000 demonstrations across 18 task variants. Learned skill segments
  are joined by motion-planned transit and transfer (a hybrid of learned and
  classical skills), with zero-shot sim-to-real on a long-horizon assembly
  task. It has no LLM caller. —
  [CoRL listing](https://mlanthology.org/corl/2024/garrett2024corl-skillmimicgen);
  [NVIDIA](https://research.nvidia.com/publication/2024-11_skillgen-automated-demonstration-generation-efficient-skill-learning-and)
- RoboGen (Wang et al., ICML 2024) **[M]**. An LLM proposes tasks, scenes,
  and a decomposition, and chooses for each subtask between RL (with an
  LLM-written reward), motion planning, or trajectory optimization. This is
  an explicit mix of learned and classical skills chosen by the LLM. —
  [arXiv 2311.01455](https://arxiv.org/abs/2311.01455)
- GenSim (Wang et al., ICLR 2024) **[M]**. An LLM writes new simulation task
  code into a growing task library for training policies. The library holds
  tasks, not callable skills. —
  [arXiv 2310.01361](https://arxiv.org/abs/2310.01361)
- Eureka (Ma et al., ICLR 2024) **[M]**. An LLM writes reward functions, and
  evolutionary search refines them, so new skills (such as pen spinning) are
  acquired by RL. It is the LLM-as-skill-author route. —
  [arXiv 2310.12931](https://arxiv.org/abs/2310.12931)
- ASPIRE (Lu, ..., Zhu, Fan, Wang; arXiv June 2026, cs.RO, no venue listed)
  **[V]**. This is agentic skill discovery with a continually expanding
  library of validated fixes. It reports 31% versus 4% on LIBERO-Pro Long
  zero-shot, and gains of up to 77% (LIBERO-Pro), 72% (Robosuite handover),
  and 32% (BEHAVIOR-1K). The abstract has no library-size numbers. —
  [arXiv 2607.00272](https://arxiv.org/abs/2607.00272)
- SkillComposer (Woods & Seifi; arXiv Aug 2026, submitted to Humanoids 2026)
  **[V]**. Online library learning compresses recurring function sequences
  from successful LLM-written programs into macro skills. It reports
  ablations and a 12-person user study; the abstract gives no numbers. —
  [arXiv 2608.14944](https://arxiv.org/abs/2608.14944)

**Hierarchical VLAs (language subtasks as the "skill" interface)**
- Hi Robot (Shi et al., ICML 2025) **[V]**. A VLM reasons over prompts and
  feedback and emits atomic language commands (e.g. "grasp the cup"), which a
  low-level VLA executes. It was tested on single-arm, dual-arm, and mobile
  dual-arm robots. —
  [arXiv 2502.19417](https://arxiv.org/abs/2502.19417);
  [ICML](https://icml.cc/virtual/2025/poster/44202)
- π0.5 (Physical Intelligence, 2025) **[M]**. One model predicts a text
  subtask, then actions, for open-world home tasks. —
  [arXiv 2504.16054](https://arxiv.org/abs/2504.16054)
- GR00T N1 (NVIDIA, 2025) **[M]**. A dual-system humanoid foundation model:
  a VLM "System 2" with a diffusion "System 1". —
  [arXiv 2503.14734](https://arxiv.org/abs/2503.14734)
- RT-H (Belkhale et al., RSS 2024) **[M]**. An intermediate "language
  motion" layer (e.g. "move arm forward") sits between task and action, and
  humans can correct it. —
  [arXiv 2403.01823](https://arxiv.org/abs/2403.01823)
- Helix (Figure AI, Feb 2025) **[M]** is a company blog post, not a paper.
  It describes a similar System 2/System 1 split. Cite it as an industry
  announcement only. — [figure.ai/news/helix](https://www.figure.ai/news/helix)

**Robot tool-calling frameworks (ROS, MCP)**
- ROS-LLM (Mower et al., Huawei Noah's Ark, arXiv June 2024) **[V]**. A chat
  agent over ROS turns LLM output into sequence, behavior-tree, or
  state-machine behaviors. It adds new atomic actions by imitation learning
  and reflects on human and environment feedback. —
  [arXiv 2406.19741](https://arxiv.org/html/2406.19741v3)
- ROSA (Royce et al., NASA JPL, arXiv Oct 2024, v2 Feb 2025) **[V]**. A
  LangChain agent exposes ROS1/ROS2 through well-defined tools, with
  parameter validation and constraint enforcement. It was shown on
  NeBula-Spot, EELS, and Nova Carter in the Mars Yard, the lab, and sim. —
  [arXiv 2410.06472](https://arxiv.org/html/2410.06472v2)
- MCP over an unmodified ROS navigation stack (Lee, Park, Kim; arXiv Sept
  2026) **[V]**. Map and semantic layers are exposed as MCP tools that any
  MCP-capable LLM can call, with more than 97% map coverage in a simulated
  indoor environment. —
  [arXiv 2609.27340](https://arxiv.org/abs/2609.27340)
- MCP in manufacturing (arXiv 2506.11180, 2025). This paper argues MCP lets
  an LLM find and invoke robot-cell functions at runtime without hard-coded
  integration. Its authors were not checked. —
  [arXiv 2506.11180](https://arxiv.org/pdf/2506.11180)

### Inferences
- How skills are added differs by line. SayCan, Text2Motion, LOTUS, and
  SkillGen add skills through data and training. Code as Policies,
  ProgPrompt, ChatGPT-for-Robotics, ROSA, and the MCP work add them by
  writing a function and a description. Voyager, BOSS, ASPIRE, and
  SkillComposer have the agent add skills itself.
- RoboGen is the clearest prior work in which the LLM chooses between a
  learned skill (RL) and a classical one (a motion planner) for each
  subtask. SkillGen mixes the two by construction, without an LLM.

### Gaps
- Several canonical results above (SayCan's numbers, ProgPrompt's ablation
  details, π0.5, GR00T N1, RT-H, Helix) were not re-fetched this session.
  Spot-check them before citing.

## Q2. Has anyone measured success against library size, or ablated skills one at a time?

### Takeaway
In robotics, I found no study that sweeps library size and reports task
success. Robotics work ablates the existence of a library (Voyager), prompt
parts (ProgPrompt), or retry and verification (Ha et al.), not the number of
skills or the cost to add one. Size curves now exist for software and
text-world agents: routing accuracy decays logarithmically with library size
(2026). That leaves the robotics or vehicle version of the measurement open.

### Cited Findings
- "The Scaling Laws of Skills in LLM Agent Systems" (Chen, ..., Hu; arXiv
  May 2026, technical report) **[V]** studied 15 frontier LLMs, 1,141
  real-world skills, and more than 3M decisions. Single-step routing
  accuracy "decays logarithmically with library size" (R² > 0.97 for every
  model). As the library grows, errors shift from confusion between similar
  skills to cross-family drift and capture by over-general "black-hole
  skills". Law-guided optimization raised held-out routing accuracy from
  71.3% to 91.7% and cut hijacking from 22.4% to 4.1%. ClawBench pass rates
  rose from 49.3% to 61.6%. These are software agents, not robots. —
  [arXiv 2605.16508](https://arxiv.org/abs/2605.16508)
- SkillAlign (Ren, Kang, Zhang; EMNLP 2026 per arXiv) **[V]** holds the
  skills fixed and varies how they are exposed: full instructions, hints,
  compressed summaries, workflows, or nothing. The form of exposure changes
  both success and context cost, and compact top-k exposure can beat
  injecting the full library. A search snippet, not checked against the
  full text, reports a sweep of 200 to 2,000 skills on ALFWorld: full
  exposure drops to 47.9 at 1,000 skills, while compressed exposure holds
  72.1. —
  [arXiv 2609.07255](https://arxiv.org/abs/2609.07255)
- Voyager ablates the whole library, not single skills. A third-party
  leaderboard says that without it, downstream success on four unseen tasks
  falls from 100% to about 91.7% and iterations roughly double. This is
  secondhand, and the paper's own "plateau" wording was not confirmed
  because the PDF was too large to fetch. —
  [benchmarklist](https://benchmarklist.com/benchmarks/voyager_minecraft/)
- Ha et al. ablate retry and verification, not library size (see Q1). —
  [PMLR](https://proceedings.mlr.press/v229/ha23a/ha23a.pdf)

### Inferences
- These claims look open in the robot or vehicle setting: (a) success as a
  function of skill-library size, (b) leave-one-skill-out ablations, and (c)
  engineering cost to add a skill (lines of code, minutes, or data) against
  the success gained. Cite the 2026 software-agent scaling law as the
  nearest prior work and as a hypothesis to test.

### Gaps
- I could not confirm whether SayCan, BOSS, or SkillComposer contain any
  size sweep, because full texts were not read. SayCan has a section on
  adding new skills (drawer skills), which I recall from memory **[M,
  unverified]**; it is a demonstration, not a curve.

## Q3. How skill descriptions are presented, and the effect of description quality

### Takeaway
Skills reach the model in one of several forms: a natural-language option
string (SayCan), a Python API with imports, comments, and examples (Code as
Policies, ProgPrompt), descriptive function names and docs (PromptCraft),
LangChain or MCP tool schemas (ROSA, MCP work), embedding-retrieved code with
docstrings (Voyager), or PDDL operators (LLM+P). The best evidence on
description quality comes from general tool use, not robotics:
documentation alone can match or beat few-shot demonstrations.

### Cited Findings
- Hsieh et al., "Tool Documentation Enables Zero-Shot Tool-Usage with Large
  Language Models" (2023; Google Research listing) **[V]**. Zero-shot
  prompts with documentation only performed about as well as few-shot
  prompts. On a new dataset with hundreds of tool APIs, documentation-based
  zero-shot prompting beat few-shot prompting without documentation. Given
  only the documentation of the underlying vision models, LLMs re-created
  Grounded-SAM and Track Anything. —
  [arXiv 2308.00675](https://arxiv.org/abs/2308.00675);
  [Google Research](https://research.google/pubs/tool-documentation-enables-zero-shot-tool-usage-with-large-language-models/)
- SkillAlign shows that exposure format, separate from skill choice,
  changes success and cost (see Q2). —
  [arXiv 2609.07255](https://arxiv.org/abs/2609.07255)
- Code as Policies relies on few-shot examples written as comments plus
  code, so the API documentation and the examples are the channel through
  which the model learns. —
  [project page](https://code-as-policies.github.io/)
- A construction-robotics study found that high-level and low-level prompts
  work together only when the API descriptions agree, and it recommends
  self-describing function names and arguments. This is not a controlled
  ablation. Its authors and venue were not checked. —
  [Aston repository PDF](https://publications.aston.ac.uk/id/eprint/46431/1/Large_language_model-based_code_generation_for_the_control_of_construction_AntwiAfari.pdf)

### Inferences
- A robot-specific ablation of description quality would be new: the same
  skills with good, terse, misleading, or missing descriptions, measured by
  plan success. The general-tool result (Hsieh) supports it only as a
  hypothesis.

### Gaps
- I found no robotics paper that degrades skill descriptions in a controlled
  way.

## Q4. LLM tool use for drones, vehicles, and multi-robot fleets

### Takeaway
Function calling over drone APIs is established: PromptCraft on AirSim and
Tello, TypeFly with its MiniSpec language and a `query` skill, and LLMSTP
over a library of UAV functions. Multi-robot LLM planners exist as well
(SMART-LLM, RoCo, EMOS, MultiUAV-Plat). MCP and Web-of-Things swarm execution
appeared in 2025-2026. None I found reports library-size scaling.

### Cited Findings
- TypeFly (Chen, Yu, Ling, Zhong; arXiv Dec 2023, v2 Sept 2024, renamed
  ChatFly) **[V]**. An LLM writes MiniSpec, a small custom language, instead
  of Python, to save output tokens. Programs can call the LLM during
  execution through a `query` skill. v1 reports more than 2x lower LLM cost
  and execution time, and v2 reports up to 62% lower response time. The
  publication venue was not confirmed. —
  [arXiv 2312.14950](https://arxiv.org/abs/2312.14950v2)
- ChatGPT for Robotics drives drones (AirSim and Tello) through a described
  function library **[M]**. —
  [arXiv 2306.17582](https://arxiv.org/abs/2306.17582)
- LLMSTP (Springer chapter) **[2nd/abstract]** prompts an LLM to plan swarm
  tasks over a manually built library of high-level UAV functions. —
  [Springer](https://link.springer.com/chapter/10.1007/978-981-96-3552-8_47)
- Web-of-Drones **[2nd]** is described by another paper as running a UAV
  swarm in a closed loop through function calls, via MCP and Web-of-Things
  interfaces. The primary source was not found.
- MultiUAV-Plat (arXiv 2606.31073, 2026) is an LLM-oriented benchmark and
  framework for multi-UAV task planning. It notes that earlier systems were
  task-specific frameworks rather than reusable benchmarks. Its authors were
  not checked. —
  [arXiv 2606.31073](https://arxiv.org/html/2606.31073v1)
- EMOS (arXiv 2410.22662) is an "embodiment-aware heterogeneous multi-robot
  operating system with LLM agents". It was seen in search results; the
  details and venue (possibly ICLR 2025) are **unverified**. —
  [arXiv 2410.22662](https://arxiv.org/pdf/2410.22662)
- SMART-LLM (Kannan et al., IROS 2024) **[M]** has an LLM decompose tasks,
  form coalitions, and allocate skills across robots in code. RoCo (Mandi et
  al., ICRA 2024) **[M]** uses dialogue between LLM-driven arms. —
  [arXiv 2309.10062](https://arxiv.org/abs/2309.10062);
  [arXiv 2307.04738](https://arxiv.org/abs/2307.04738)
- A2C-LLM (Drones, MDPI 2026) pairs an LLM with actor-critic RL for
  allocating targets across a UAV swarm. It uses constrained decoding and a
  feasibility projection so the commands it emits can be executed. —
  [doi 10.3390/drones10060398](https://doi.org/10.3390/drones10060398)
- The ROS and MCP work (ROSA, ROS-LLM, MCP navigation) is listed in Q1. One
  2026 paper compares itself with ROSA, ROS-LLM, and "RoboNeuron"; that
  comparison was seen in a search snippet only.

### Inferences
- No paper I found checks LLM fleet plans in a multi-agent simulator before
  they are sent and also varies the library. That combination appears open.

### Gaps
- I did not verify the Web-of-Drones primary paper, RoboNeuron, or the
  details of EMOS. The UAV-LLM survey (arXiv 2501.02341) was not read.

## Q5. Claims that are clearly taken

### Takeaway
A new paper should not claim novelty for any of the following. Each is
established by 2023-2024 and widely repeated.

### Cited Findings
- **"An LLM chooses or sequences skills as tools from text descriptions."**
  Established by SayCan (2022) and Inner Monologue (2022). —
  [2204.01691](https://arxiv.org/abs/2204.01691)
- **"An LLM writes code against a robot API or skill library."** Established
  by Code as Policies, ProgPrompt, and ChatGPT for Robotics (2022-2023). —
  [Code as Policies](https://code-as-policies.github.io/)
- **"Closed-loop feedback or replanning on failure."** Established by Inner
  Monologue (2022) and by verify-and-retry in Ha et al. (2023). —
  [PMLR](https://proceedings.mlr.press/v229/ha23a/ha23a.pdf)
- **"An LLM paired with a classical planner or verifier."** LLM+P (2023)
  and Text2Motion (2023). —
  [2304.11477](https://arxiv.org/abs/2304.11477)
- **"A growing or self-built skill library that is reused on new tasks."**
  Voyager, BOSS, LOTUS, and later ASPIRE and SkillComposer. —
  [BOSS](https://proceedings.mlr.press/v229/zhang23a.html)
- **"New tools are usable zero-shot from their documentation."** Shown for
  general tools by Hsieh et al. (2023). It is implicit in every MCP or ROSA
  tool system. —
  [2308.00675](https://arxiv.org/abs/2308.00675)
- **"Exposing robots to LLMs through ROS, MCP, or LangChain tools."** ROSA,
  ROS-LLM, and the 2025-2026 MCP papers. —
  [ROSA](https://arxiv.org/html/2410.06472v2)
- **"LLM function calling for drones, including token-efficient DSLs."**
  PromptCraft and TypeFly. —
  [TypeFly](https://arxiv.org/abs/2312.14950v2)
- **"A hierarchy where a VLM emits language subtasks for a low-level
  policy."** Hi Robot, π0.5, RT-H, and GR00T N1. —
  [Hi Robot](https://arxiv.org/abs/2502.19417)
- **"Routing accuracy falls as the skill library grows"** (software
  agents). Taken in 2026 by the skills scaling-law paper. —
  [2605.16508](https://arxiv.org/abs/2605.16508)

### Inferences
- These look open as of October 2026. Each should be phrased as a
  measurement, not as a new capability:
  1. Plan success for a fleet or multi-vehicle planner as a function of
     skill-library size, in a robot simulator.
  2. Leave-one-skill-out ablations.
  3. Engineering cost per added skill against the success it buys.
  4. The effect of description quality on robot plans.
  5. Checking LLM plans in simulation before they are sent to the robots.
- Item 5 overlaps with Code as Policies-style verification and with
  RoboGen and Ha et al., which use simulation to check success. A narrower
  framing (multi-agent, pre-dispatch, measured against library growth)
  would be needed.

### Gaps
- Search coverage of 2026 preprints is partial. A dedicated sweep of 2026
  CoRL and ICRA papers on "skill library scaling" or "MCP robots" is advised
  before submission.

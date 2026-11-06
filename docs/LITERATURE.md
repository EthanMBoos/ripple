# Related work

This is the reading list for the study in
[FLEET_MANAGER_PLAN.md](FLEET_MANAGER_PLAN.md). The BibTeX is
[paper/references.bib](../paper/references.bib), one entry per paper below.
`scripts/build_paper_sources.py` downloads each PDF and makes a Markdown copy;
see [paper/literature_review/README.md](../paper/literature_review/README.md).

Reviewed 2026-11-06 from abstracts and search summaries, and from the
experiments sections of the papers marked *. None has been read in full yet.

## Language models planning for robot teams

Language models have been used to turn an instruction into a plan for a team
of robots. SMART-LLM and COHERENT decompose a task and allocate it across a
few robots of different types. Chen et al.\* compare central, local and
hybrid planners on 4 to 32 robots on grid tasks and up to 10 in warehouse
tasks, find that hybrids scale best, and name context length and long
dialogues as the limits. Later planners stay at the same size: RobotFleet
tests 3 and 5 robots, Nl2Hltl2Plan up to 4, and DynaHMRC 2 to 4. Scale-Plan
filters the action graph before the model plans, and Kawabe et al. put model
layers over a classical planner; neither reports large teams.

Swarm work reaches larger numbers on simpler tasks. COMPASS\* directs up to
1,024 robots in language, but the task is flocking on a heading, the robots
are double integrators, and there is no delay or loss. It also reports that a
central model gives wrong action counts and that small models are swamped by
neighbor state as teams grow. GenSwarm\* has the model write code once before
the mission rather than during it, and LLM2Swarm runs one model per robot.
StarCraft agents (SwarmBrain, LLM-PySC2) control many units, in a game.

The largest fleet run by a model during operation is LLM-ODDR: 300 taxis in
a Manhattan simulation, with a fine-tuned model assigning orders and
repositioning drivers every minute from summaries of 263 zones, at 17 to 30 s
per decision. Cars move between grid zones, the rules never change, and the
paper does not compare other ways of showing the model the fleet. It is the
precedent for summaries at scale.

## Language models over drone and delivery fleets

Drone papers put a model over a dispatcher or flight stack. TACOS
coordinates 6 and 20 drones; the Web-of-Drones paper runs ArduPilot and finds
that models need planning tools and guardrails to execute a mission; COLA,
MultiUAV-Plat, LLM-DaaS and FLEET turn text into missions or schedules for
small teams. A 2026 survey of model-driven UAV dispatch describes the model
as a layer over a classical dispatcher, the structure used here. Team sizes
for COLA, FLEET, LLM-DaaS, AutoHMA-LLM and MultiUAV-Plat are not in their
abstracts and still need to be read.

Blood delivery by air is a real operation. Zipline has dropped blood by
parachute in Rwanda since 2016, and Nisingizwe et al. measured shorter
delivery times and less wastage. Dolatabadi et al. design hub-and-spoke
networks with mixed fleets for the same items, which informs the scenario's
layout.

## Checking what the model asks for

Many papers put a checker between the model and the robots. RoboGuard
(RA-L 2026) and Safety Chip check plans against rules in temporal logic, SELP
generates plans that satisfy constraints, and two 2026 papers run
deterministic verifiers before a proposal reaches the robots. LLM-Modulo
argues for this design in general: the model proposes, verifiers check. That
a checker helps is established, so it is one interface in this study, not the
contribution.

## Interfaces and feedback

For software agents, the interface given to a model matters as much as the
model. SWE-agent shows that interface design beats model changes, tool
interface studies find the gains depend on model strength, and schema and
documentation work measures what tool descriptions do. Two 2026 papers on
long-running agents find that pruned summaries beat raw context and that
letting the agent query its context helps; these correspond to the summary
and query interfaces here. On the robot side, SayCan scores skills by what
they can achieve, Inner Monologue feeds the world's response back to the
model, and Text2Motion checks feasibility before committing. Hu et al. note
that model plans lack kinematic feasibility and leave large fleets to future
work.

## Events during long missions

House Rules tests agents whose rules change mid-task, ALAS replans locally
after disruptions, and DynaHMRC handles tasks that change during a run with up
to 4 robots. Laban et al. show that long conversations degrade, which matters
for a fleet manager running through a whole day of events.

## What has not been done

1. Measuring how the model should see a fleet as it grows: the same mission at
   16, 64, 256, 1,024 and 4,096 vehicles, with a full list, summaries and
   queries compared. LLM-ODDR uses summaries at 300 but does not compare;
   Chen et al. compare planner layouts, not views, up to 32.
2. A model managing hundreds of vehicles with real dynamics on a mission with
   rules. COMPASS flocks point robots, LLM-ODDR moves taxis between grid
   zones, and the drone papers stay at about 20 or fewer.
3. Feedback at fleet level on whether orders can be flown and what was
   flown. Text2Motion and Hu et al. do feasibility for one or a few robots.
4. Events nobody wrote code for, handled at scale and compared against a
   hand-coded handler that covers only the known events.
5. Grading from what was flown, on mixed fixed-wing and multirotor vehicles
   checked against other simulators.

The first is the main result. The others make it a robotics paper rather than
a paper about agent context.

The risks are that COMPASS or LLM-ODDR is judged close enough, that an unread
fleet paper has already scaled up, that the checker looks routine, that cost
grows too fast with the fleet, and that the work is simulation only.

## Papers

### Language-model fleets and scale

| Key | Paper | Venue | Why it's here |
| --- | --- | --- | --- |
| `vatnsdal2026compass` * | [Controlling Collectives of AI Agents in Reasoning Space with Spatial Transformers (COMPASS)](https://arxiv.org/abs/2609.28247) | arXiv 2026 | Up to 1,024; flocking directed in language; closest prior work |
| `chen2024scalable` * | [Scalable Multi-Robot Collaboration with LLMs: Centralized or Decentralized Systems?](https://arxiv.org/abs/2309.15943) | ICRA 2024 | 4 to 32; hybrid central and local planners scale best |
| `kannan2024smartllm` | [SMART-LLM: Smart Multi-Agent Robot Task Planning using LLMs](https://arxiv.org/abs/2309.10062) | IROS 2024 | Task allocation, small teams |
| `liu2025coherent` | [COHERENT: Collaboration of Heterogeneous Multi-Robot System with LLMs](https://arxiv.org/abs/2409.15146) | ICRA 2025 | Mixed robot types |
| `ji2025genswarm` * | [GenSwarm: Scalable Multi-Robot Code-Policy Generation and Deployment via Language Models](https://arxiv.org/abs/2503.23875) | npj Robotics 2026 | Model writes code before the mission, not during it |
| `strobel2024llm2swarm` | [LLM2Swarm: Robot Swarms that Responsively Reason, Plan, and Collaborate through LLMs](https://arxiv.org/abs/2410.11387) | arXiv 2024 | A model per robot |
| `yang2025autohmallm` | AutoHMA-LLM: Task Coordination in Heterogeneous Multi-Agent Systems Using Hybrid LLMs | IEEE TCCN 2025 | Cloud model over local models; PDF saved by hand |
| `amass2025` | AMASS: Language Model Coordinated Versatile and Scalable Fixed Wing Swarm | Funded project | Scalable fixed-wing swarm; no results published yet; watch it |
| `shao2024swarmbrain` | [SwarmBrain: Embodied agent for StarCraft II via LLMs](https://arxiv.org/abs/2401.17749) | arXiv 2024 | Many units, in a game |
| `li2024llmpysc2` | [LLM-PySC2: StarCraft II learning environment for LLMs](https://arxiv.org/abs/2411.05348) | arXiv 2024 | A top agent over unit groups, in a game |
| `lyu2025llmoddr` | [LLM-ODDR: A Large Language Model Framework for Joint Order Dispatching and Driver Repositioning](https://arxiv.org/abs/2505.22695) | arXiv 2025 | 300 taxis; model dispatches every minute from zone summaries; the scale precedent |
| `gupta2026scaleplan` | [Scale-Plan: Scalable Language-Enabled Task Planning for Heterogeneous Multi-Robot Teams](https://arxiv.org/abs/2603.08814) | arXiv 2026 | Filters the action graph before the model plans |
| `gupta2025robotfleet` | [RobotFleet: An Open-Source Framework for Centralized Multi-Robot Task Planning](https://arxiv.org/abs/2510.10379) | arXiv 2025 | Central model plans, a scheduler allocates; 3 and 5 robots |
| `xu2024nl2hltl2plan` | [Nl2Hltl2Plan: Scaling Up Natural Language Understanding for Multi-Robots Through Hierarchical Temporal Logic](https://arxiv.org/abs/2408.08188) | arXiv 2024 | Language to temporal logic; up to 4 robots |
| `kawabe2026hierarchical` | [Hierarchical LLM-Based Multi-Agent Framework with Prompt Optimization for Multi-Robot Task Planning](https://arxiv.org/abs/2602.21670) | arXiv 2026 | Model layers over a classical planner (Fast Downward) |
| `li2026llmmrs` | [Large Language Models for Multi-Robot Systems: A Survey](https://arxiv.org/abs/2502.03814) | Autonomous Robots 2026 | Survey |

### Language models over drone and delivery fleets

| Key | Paper | Venue | Why it's here |
| --- | --- | --- | --- |
| `nazzari2025tacos` | [TACOS: Task Agnostic Coordinator of a Multi-Drone System](https://arxiv.org/abs/2510.01869) | arXiv 2025 | Mentions 6 and 20 drones |
| `ludwig2026cola` | [Collaborative LLM-Based Agents for Autonomous Multi-UAV Mission Execution](https://stottlerhenke.com/wp-content/uploads/2026/06/2026_IEEE_AERO_Ludwig_COLA.pdf) | IEEE Aerospace 2026 | Multi-UAV missions run by model agents |
| `iannoli2026webofdrones` | [Say the Mission, Execute the Swarm: Agent-Enhanced LLM Reasoning in the Web-of-Drones](https://arxiv.org/abs/2605.03788) | WoWMoM 2026 | ArduPilot; models need planning tools and guardrails to execute |
| `zhang2026multiuavplat` | [MultiUAV-Plat: An LLM-Oriented Platform, Benchmark and Framework for Multi-UAV Collaborative Task Planning](https://arxiv.org/abs/2606.31073) | arXiv 2026 | Benchmark with hidden checks |
| `wassim2024llmdaas` | [LLM-DaaS: LLM-driven Drone-as-a-Service Operations from Text User Requests](https://arxiv.org/abs/2412.11672) | arXiv 2024 | Delivery requests in text to drone tasks |
| `rivera2025fleet` | [FLEET: Formal Language-Grounded Scheduling for Heterogeneous Robot Teams](https://arxiv.org/abs/2510.07417) | arXiv 2025 | Model to formal schedule for mixed teams |
| `khorrambakht2026auv` | [LLM-Assisted Planning with Distributed Onboard Behaviour Tree Execution for Multi-AUV Missions](https://arxiv.org/abs/2611.04518) | arXiv 2026 | Model extracts constraints, an optimizer assigns |
| `han2026dispatch` | [LLM-Enabled UAV Dispatch: A System-Level Survey and Taxonomy](https://arxiv.org/abs/2610.09466) | arXiv 2026 | Survey; model as a layer over a dispatcher; divide-and-conquer for large missions |
| `emami2026uavsurvey` | [Large Language Model-Assisted UAV Operations and Communications: A Multifaceted Survey and Tutorial](https://arxiv.org/abs/2602.19534) | arXiv 2026 | Survey; includes language models in drone traffic management |
| `dolatabadi2026hubspoke` | [Designing Hierarchical Hub-and-Spoke Drone-Based Networks for Delivering Time-Sensitive Healthcare Items](https://arxiv.org/abs/2607.12198) | arXiv 2026 | Hub-and-spoke blood delivery network with a mixed fleet; for the scenario's layout |
| `nisingizwe2022drone` | Effect of Drone Delivery on Blood Product Delivery Time and Wastage in Rwanda | Lancet Global Health 2022 | Real blood-delivery operations; the scenario's anchor; PDF saved by hand |

### Checking what the model asks for

| Key | Paper | Venue | Why it's here |
| --- | --- | --- | --- |
| `ravichandran2025roboguard` | [Safety Guardrails for LLM-Enabled Robots (RoboGuard)](https://arxiv.org/abs/2503.07885) | RA-L 2026 | Checker; the RA-L precedent |
| `yang2024safetychip` | [Plug in the Safety Chip: Enforcing Constraints for LLM-driven Robot Agents](https://arxiv.org/abs/2309.09919) | ICRA 2024 | Checker with temporal logic |
| `wu2024selp` | [SELP: Generating Safe and Efficient Task Plans for Robot Agents with LLMs](https://arxiv.org/abs/2409.19471) | arXiv 2024 | Checker |
| `khatana2026verifier` | [Verifier-Mediated Multi-LLM Coordination for Multi-Robot Path Planning](https://arxiv.org/abs/2610.12761) | IROS 2026 workshop | Verifier between models and robots |
| `tang2026verificationgated` | [Verification-Gated Agentic Mission-State Governance for Industrial Multi-Robot Systems](https://arxiv.org/abs/2606.31339) | arXiv 2026 | Deterministic checks before a proposal commits |
| `kambhampati2024llmmodulo` | [LLMs Can't Plan, But Can Help Planning in LLM-Modulo Frameworks](https://arxiv.org/abs/2402.01817) | ICML 2024 | The frame: model proposes, verifiers check |

### Interface and feedback

| Key | Paper | Venue | Why it's here |
| --- | --- | --- | --- |
| `yang2024sweagent` | [SWE-agent: Agent-Computer Interfaces Enable Automated Software Engineering](https://arxiv.org/abs/2405.15793) | NeurIPS 2024 | Interface design beats model changes |
| `wu2025toolinterface` | [The Art of Tool Interface Design](https://arxiv.org/abs/2503.21036) | arXiv 2025 | Gains depend on model strength |
| `sigdel2026schemafirst` | [Schema First Tool APIs for LLM Agents](https://arxiv.org/abs/2603.13404) | arXiv 2026 | Controlled study of tool specs and feedback |
| `hsieh2023tooldoc` | [Tool Documentation Enables Zero-Shot Tool-Usage with LLMs](https://arxiv.org/abs/2308.00675) | arXiv 2023 | What a skill's description does |
| `ahn2022saycan` | [Do As I Can, Not As I Say (SayCan)](https://arxiv.org/abs/2204.01691) | CoRL 2022 | Skills scored by what they can achieve |
| `huang2022innermonologue` | [Inner Monologue: Embodied Reasoning through Planning with Language Models](https://arxiv.org/abs/2207.05608) | arXiv 2022 | Feedback from the world to the model |
| `hu2026grounded` | [LLM-Grounded Dynamic Task Planning with Hierarchical Temporal Logic for Human-Aware Multi-Robot Handover](https://arxiv.org/abs/2602.09472) | arXiv 2026 | Says model plans lack kinematic feasibility; large fleets left as future work |
| `lodha2026lesscontext` | [Less Context, Better Agents: Efficient Context Engineering for Long-Horizon Tool-Using LLM Agents](https://arxiv.org/abs/2606.10209) | arXiv 2026 | Pruning and summaries beat raw context; for summaries against full lists |
| `lin2026contextenv` | [Context as an Environment: Programmatic Context Management for Long-Horizon Agents](https://arxiv.org/abs/2608.21690) | arXiv 2026 | The agent queries its context; for query tools |
| `lin2023text2motion` | [Text2Motion: From Natural Language Instructions to Feasible Plans](https://arxiv.org/abs/2303.12153) | arXiv 2023 | Feasibility checks before committing |

### Events and long missions

| Key | Paper | Venue | Why it's here |
| --- | --- | --- | --- |
| `vimalkanth2026houserules` | [House Rules: A Benchmark for Agents under Changing Requirements](https://openreview.net/pdf?id=plGCWNnOZh) | NeurIPS 2026 workshop | Rules that change mid-task, like the day's events |
| `chang2025alas` | [ALAS: A Stateful Multi-LLM Agent Framework for Disruption-Aware Planning](https://arxiv.org/abs/2505.12501) | arXiv 2025 | Local replanning after disruptions |
| `yu2026dynahmrc` | [DynaHMRC: Decentralized Heterogeneous Multi-Robot Collaboration for Dynamic Tasks with LLMs](https://arxiv.org/abs/2606.14882) | arXiv 2026 | Tasks that change during the run; 2 to 4 robots |
| `laban2025lost` | [LLMs Get Lost in Multi-Turn Conversation](https://arxiv.org/abs/2505.06120) | arXiv 2025 | Long sessions degrade; a full day of events is long |

## Not pulled

Found on 2026-11-06 and judged not worth downloading yet:

- [AgentLongBench](https://www.alphaxiv.org/abs/2601.20730), long-context
  agent benchmark; the two context papers above cover the question.
- [Foundation Models for Logistics](https://arxiv.org/html/2507.11352v1),
  conversational logistics planning without vehicles.
- Medical drone operations reports: [whole-blood drones in
  Finland](https://www.ncbi.nlm.nih.gov/pmc/articles/PMC12511842/) and
  [RAND's blood-delivery fleet
  design](https://www.rand.org/pubs/research_reports/RR3047.html). Useful for
  realistic numbers if the scenario needs them.
- Open-RMF's classical task dispatch and traffic
  ([book](https://osrf.github.io/ros2multirobotbook/task.html)), and an
  [open-source model-run AMR fleet on
  Open-RMF](https://github.com/Sanmitra10-ai/llm-amr-fleet-agent): a
  hobby-scale project, not a paper.
- Ride-hailing with a language-model agent per driver (500 drivers) and
  language models that write dispatch heuristics offline: different questions.

## What to record for each paper

Fill these in as each paper is read, under its own heading in
[Notes](#notes):

- **Claim:** the main result, in one or two sentences.
- **Scale:** largest team or fleet tested, and how the model saw it.
- **Model's role:** assigns every task, manages above a dispatcher, writes
  code once, or other; how often it is called.
- **World:** vehicle dynamics, sensing, and comms.
- **Checks and feedback:** what the model is told back, and what refuses its
  output.
- **Limitations:** what the authors say does not work or was not tested.
- **Future work:** what they say should come next.
- **Relevance:** what it means for the fleet manager study.

## Notes

One heading per paper, using the fields above, as each is read.

# Multi-agent robotics since SCRIMMAGE: influence, adoption, and research directions

**Evidence snapshot: September 26, 2026.** Review for SCRIMMAGE-RS and its proposed RQ4. Three parallel research strands covered coordination and physical swarms; learning and communication; and simulation and evaluation. The synthesis adds cooperative mapping, recent embodied-agent benchmarks, and checks of citation-record quality. Supporting API responses are retained in [the dated evidence directory](literature-review/2026-09-26/).

## 1. What deserves attention

The field's development is much broader than putting a learned policy into a simulator. The strongest evidence supports five connected changes:

1. **Robot teams became more capable in physical, uncertain environments.** Optimized flocking, EGO-Swarm, and *Swarm of micro flying robots in the wild* are substantial physical-robot references. RACER adds collaborative exploration and workload allocation. These are important counterweights to a history dominated by game benchmarks.
2. **Scalable coordination increasingly addressed continuing tasks.** RHCR and later lifelong-MAPF systems advance repeated-task planning, while PRIMAL, PIBT, and EECBS supply related local-decision and search methods. Classical planning remains central, including inside recent learned systems.
3. **What each robot knows became part of the architecture.** Graph communication, Kimera-Multi, and Swarm-SLAM address information exchange and collective estimation. A team can fail because its members disagree about the world even when each controller works correctly.
4. **Shared benchmarks and accelerator-based experimentation changed how methods are developed.** SMAC, PettingZoo, Isaac Gym, and their successors have demonstrable communities. But benchmark success can hide weak feedback: SMACv2 found that time-only policies could succeed on parts of the original benchmark.
5. **Newer work combines specialized components.** Learned proposals with explicit conflict repair, nominal commands with safety control, and language-based task negotiation with motion validation are more useful architectural observations than “everything is becoming end-to-end learning.”

The evidence for these conclusions is separated below. **Established influence**, **current public attention**, and **promising recent work** are different categories. A relevant title, conference acceptance, or fashionable model name is insufficient to put a paper in the first category.

For a short reading list, start with **Optimized flocking; PRIMAL; RHCR; EGO-Swarm / Swarm in the wild; RACER; GNN decentralized planning; Kimera-Multi; MAPPO; SMACv2; and Isaac Gym → Isaac Lab**. Add **RoCo, SILLM, and MuJoCo Playground** for newer directions, with the qualifications below. These are prioritized references within the reviewed scope, not a claimed global top-ten ranking.

### Strongest measured followings in this review

Among the selected direct robot-team records, the largest totals are [Swarm in the wild (**610**)](https://openalex.org/W4229007520), [MAPF definitions/benchmarks (**608**)](https://openalex.org/W2949992533), and [Optimized flocking (**588**)](https://openalex.org/W2884236119). These are different kinds of contributions, and the counts apply to the linked records. In the learning shortlist, [COMA’s proceedings record (**1,755**)](https://openalex.org/W2617547828) is substantial; QMIX and MAPPO have clearly established adoption but unresolved comparative citation rank. The largest checked software audiences are [Genesis (**29,989 stars**)](https://github.com/Genesis-Embodied-AI/genesis-world), [Isaac Lab (**8,225**)](https://github.com/isaac-sim/IsaacLab), and [PettingZoo (**3,523**)](https://github.com/Farama-Foundation/PettingZoo). For recent papers, prioritize RoCo’s measurable follow-on attention, MuJoCo Playground’s citations and integrations, and SILLM’s award recognition—three distinct kinds of evidence.

## 2. How influence was checked

### Scope and selection

The core scope is multi-robot coordination, planning, communication, perception, and experimentation from 2018 through the snapshot date. Pre-2018 MARL foundations are included where necessary to avoid presenting an existing idea as a post-SCRIMMAGE invention. General robot-learning platforms and single-robot methods are labeled adjacent; generic software-agent papers are outside scope.

The review combined primary papers and project pages, official code, exact-title bibliographic searches, later implementations, conference award records, and current competition rules. An additional discovery pass queried OpenAlex titles for `multi-agent`, `multi-robot`, `swarm robotics`, `robot swarm`, `robot learning`, and `model predictive control`, restricted to 2018–2026 and sorted by citation count, retaining up to 30 results per query. The resulting 180 search slots include duplicates and irrelevant matches; they are **not** 180 validated papers. Broad full-text search produced unrelated results and was not used as a ranking.

This is a screened literature review with bibliometric checks, not an exhaustive systematic review or a complete subject-normalized citation census. English-language titles, available public code, and these search terms introduce selection bias. No claim that omitted papers lack influence follows from their omission.

### Evidence rules

| Signal | What it supports | What it does not establish |
|---|---|---|
| Citation count on an identified record | Scholarly attention to that indexed version | A deduplicated whole-paper total, quality, or current popularity by itself |
| Citations grouped by citing-work year | Retrospective evidence of when attention accumulated | A leaderboard captured in that historical year |
| Official repository stars and forks | Public developer attention on the snapshot date | Downloads, active users, successful deployment, or recent growth rate |
| Another group's implementation or benchmark extension | Concrete uptake beyond the original release | Independent confirmation of every experimental claim |
| Conference award | Expert recognition of a contribution | Broad adoption or a large citation following |
| Current competition tasks and maintained integrations | An active research problem or functioning ecosystem | Popularity of any one paper |

Counts are not combined into a synthetic score. Repository age and paper age differ; framework stars cannot be assigned to every supported algorithm. Same-lab follow-ons are useful lineage evidence but are distinguished from independent adoption.

### A material problem with the citation indexes

OpenAlex splits several flagship contributions across versions. QMIX has two records for the same arXiv DOI with **480 and 347** citations. MAPPO has records with **600, 373, and 108**. Its proceedings record assigns 357 citations to 2026, while its arXiv record assigns only 17 to that year. That discrepancy cannot responsibly be described as a sudden research surge. **No duplicate counts are summed in this report.**

Cross-checks also reveal coverage differences: PettingZoo has **147** on the selected OpenAlex record versus **468** on Semantic Scholar; VMAS **19 versus 95**; BenchMARL **5 versus 66**. Semantic Scholar rate limits prevented a complete second-source census. For fragmented major papers, independent adoption is more reliable than their position in a numeric table. Exact queried records and failures are preserved in the evidence files.

A useful historical check exists in [Mikayel Samvelyan's 2024 presentation, PDF page 58](https://samvelyan.com/slides/imol_2024.pdf): the real author-profile screenshot shows **2,686** Google Scholar citations for the extended QMIX paper and **1,148** for SMAC. The exact screenshot capture date is unstated. These are historical displayed counts, not current totals; the fictional example in the lower half of that slide is excluded. The screenshot demonstrates why the much smaller current OpenAlex fragments must not be interpreted as whole-paper totals.

## 3. Established robot-team papers

**OA** below means the count on the linked OpenAlex record as retrieved September 26, 2026. It is deliberately record-specific. “2025” counts citing works assigned to 2025 in today's index. Rows are grouped by topic, not sorted into a global ranking.

### Physical coordination, exploration, and shared information

| Paper; publication | OA total / 2025 | Evidence and reason to read |
|---|---:|---|
| [Optimized flocking of autonomous drones in confined environments](https://vasarhelyi.github.io/drone-project-site/scirob2018.html), Science Robotics 2018 | [588 / 100](https://openalex.org/W2884236119) | Physical demonstration with 30 drones; optimized explicit flocking rules under motion and communication constraints. A strong non-neural starting point for this manuscript. |
| [Pairwise Consistent Measurement Set Maximization for Robust Multi-Robot Map Merging](https://doi.org/10.1109/ICRA.2018.8460217), ICRA 2018 | [210 / —](https://openalex.org/W2891820683) | ICRA multi-robot paper award; addresses inconsistent inter-robot measurements. Establishes cooperative perception as part of the history. |
| [EGO-Swarm](https://arxiv.org/abs/2011.04183), ICRA 2021; preprint 2020 | [211 / 63](https://openalex.org/W3207644545) | Decentralized asynchronous trajectory optimization. [Official code](https://github.com/ZJU-FAST-Lab/ego-planner-swarm): **2,190 stars / 387 forks**, substantial public developer attention. |
| [Swarm of micro flying robots in the wild](https://doi.org/10.1126/scirobotics.abm5954), Science Robotics 2022 | [610 / 208](https://openalex.org/W4229007520) | Autonomous physical swarm flight in cluttered unknown environments. Strong recent citation activity as well as a compelling demonstration. |
| [Kimera-Multi](https://arxiv.org/abs/2106.14386), T-RO 2022; preprint 2021 | [278 / 89](https://openalex.org/W4226481925) | Distributed metric-semantic mapping and inter-robot consistency. [Official code](https://github.com/MIT-SPARK/Kimera-Multi): **416 / 53**. Distinct earlier conference and later journal records must not be added. |
| [RACER](https://ieeexplore.ieee.org/document/10038280), T-RO 2023; preprint 2022 | [244 / 104](https://openalex.org/W4319302559) | Decentralized multi-UAV exploration and workload allocation; [T-RO 2023 best-paper recognition](https://www.ieee-ras.org/publications/t-ro/). [Official code](https://github.com/Robotics-STAR-Lab/RACER): **797 / 101**. Particularly useful beyond policy learning. |
| [Swarm-SLAM](https://arxiv.org/abs/2301.06230), RA-L 2024; online publication 2023 | [174 / 71](https://openalex.org/W4388755261) | Decentralized collaborative SLAM, including communication-conscious loop closure. [Official code](https://github.com/MISTLab/Swarm-SLAM): **705 / 81**. Collective state estimation deserves attention alongside navigation. |

These systems contain perception and mapping capabilities SCRIMMAGE-RS does not currently provide. Their value here is identifying experimental questions, not claiming that a plugin interface alone reproduces their demonstrations. For example, testing how inconsistent reports alter task assignment can begin with simple synthetic observations; reproducing visual SLAM would be a separate project.

### Path planning and continuing tasks

| Paper; publication | OA total / 2025 | Evidence and reason to read |
|---|---:|---|
| [Multi-Agent Pathfinding: Definitions, Variants, and Benchmarks](https://arxiv.org/abs/1906.08291), SoCS 2019 | [608 / 137](https://openalex.org/W2949992533) | [Shared benchmark infrastructure](https://movingai.com/benchmarks/). Clarifies collisions, goals, timing, and objectives. Verified venue year is 2019 despite inconsistent index metadata. |
| [PRIMAL](https://arxiv.org/abs/1809.03531), RA-L 2019; preprint 2018 | [426 / 101](https://openalex.org/W2892258706) | Established decentralized imitation/RL planner; [code](https://github.com/gsartoretti/PRIMAL) **446 / 78** and subsequent PRIMAL2/3 lineage. A stronger historical anchor than a recent paper selected only for relevance. |
| [GNNs for Decentralized Multi-Robot Path Planning](https://arxiv.org/abs/1912.06095), IROS 2020; preprint 2019 | [276 / 65](https://openalex.org/W3130631955) | Local observations and graph message passing; [code](https://github.com/proroklab/gnn_pathplanning) **255 / 36**. Directly motivates communication-aware experiments. |
| [Lifelong Multi-Agent Path Finding in Large-Scale Warehouses / RHCR](https://jiaoyangli.me/files/2021-AAAI-2.pdf), AAAI 2021; preprint 2020 | [250 / 55](https://openalex.org/W3037164939) | Rolling-horizon planning for repeated goals; [code](https://github.com/Jiaoyang-Li/RHCR) **216 / 60**. Evaluate throughput and planning latency, not only finishing one episode. |
| [EECBS](https://ojs.aaai.org/index.php/AAAI/article/view/17466), AAAI 2021 | [212 / 56](https://openalex.org/W3175724379) | Bounded-suboptimal search; retained as a comparison in later MAPF work. Classical methods remain essential controls. |
| [Message-Aware Graph Attention Networks / MAGAT](https://github.com/proroklab/magat_pathplanning), RA-L 2021 | [194 / 66](https://openalex.org/W3166401044) | Same-group extension of GNN planning. Learn which messages matter; compare against simple distance- or risk-based selection at equal bandwidth. |
| [Priority Inheritance with Backtracking / PIBT](https://kei18.github.io/pibt2/), IJCAI 2019; expanded AIJ 2022 | [143 / 53](https://openalex.org/W2914864020), journal only | Fast iterative priority negotiation; [code](https://github.com/Kei18/pibt2) **116 / 43**. Used in competition strategies and subsequent hybrid planning. Guarantees depend on graph assumptions. |
| [MAPF-LNS2](https://ojs.aaai.org/index.php/AAAI/article/view/21266), AAAI 2022 | [103 / 29](https://openalex.org/W4283795314) | Repair conflicting subsets rather than repeatedly solve the full fleet problem. A useful architecture for combining fast proposals with explicit repair. |
| [LaCAM](https://kei18.github.io/lacam/), AAAI 2023; preprint 2022 | [74 / 28](https://openalex.org/W4382202651) | [Code](https://github.com/Kei18/lacam) **82 / 31**. Later learning work uses LaCAM-family planners to generate expert data. Specialist influence is visible through that reuse, despite smaller public counts. |

Continuing-task planning predates SCRIMMAGE: [Ma et al., AAMAS 2017](https://aamas.csc.liv.ac.uk/Proceedings/aamas2017/pdfs/p837.pdf), explicitly formulate lifelong MAPF for online pickup and delivery. RHCR advances its scalable planning methods rather than introducing the problem. “Lifelong” in RHCR and this MAPF literature means that robots receive further tasks after completing earlier ones. It does **not** imply continual learning, lifelong adaptation of a model, or a general solution to long-duration autonomy. For the manuscript, **continuing tasks** or **repeated task assignment** is clearer unless the specific lifelong-MAPF formulation is intended.

## 4. Learning papers with demonstrated following

These papers explain much of the algorithmic background, but game or particle-world results must not be rewritten as physical robot-team validation.

| Paper; publication | Scholarly/public evidence | What it contributes; scope |
|---|---|---|
| [MADDPG](https://arxiv.org/abs/1706.02275), NeurIPS 2017 | [OA 1,010](https://openalex.org/W2623431351), arXiv record; [code](https://github.com/openai/maddpg) **1,981 stars / 534 forks** | Centralized training with decentralized execution; pre-SCRIMMAGE foundation. Independently implemented in BenchMARL. |
| [COMA](https://arxiv.org/abs/1705.08926), AAAI 2018; preprint 2017 | [OA 1,755](https://openalex.org/W2617547828), proceedings record; implemented in PyMARL | Counterfactual team credit assignment; strong historical citation influence, not necessarily the first modern baseline to implement. |
| [VDN](https://arxiv.org/abs/1706.05296), AAMAS 2018; preprint 2017 | [OA 620](https://openalex.org/W2807741983); PyMARL and JaxMARL implementations | Decompose team value into agent values; simple cooperative-learning foundation. |
| [QMIX](https://proceedings.mlr.press/v80/rashid18a.html), ICML 2018; [extended JMLR version](https://www.jmlr.org/papers/v21/20-081.html), 2020 | Fragmented OA records; historical Scholar evidence above. [PyMARL](https://github.com/oxwhirl/pymarl) **2,222 / 412**, shared across algorithms | Nonlinear monotonic value mixing. Established through continued benchmark use and implementations, not ranked using an incomplete index fragment. |
| [Actor-Attention-Critic / MAAC](https://proceedings.mlr.press/v97/iqbal19a.html), ICML 2019 | [OA 289](https://openalex.org/W2894976951), arXiv record; [code](https://github.com/shariqiqbal2810/MAAC) **811 / 180** | Attention selects information in the centralized critic. This is not automatically a radio protocol used by deployed robots. |
| [Crowd-Robot Interaction / SARL](https://arxiv.org/abs/1809.08835), ICRA 2019 | [OA 618](https://openalex.org/W2890001928); [CrowdNav](https://github.com/vita-epfl/CrowdNav) **738 / 184** | Strong robotics-adjacent attention model, independently extended by [Illinois DS-RNN](https://github.com/Shuijing725/CrowdNav_DSRNN). One robot among humans, not a jointly controlled robot team. |
| [MAPPO](https://arxiv.org/abs/2103.01955), NeurIPS 2022; preprint 2021 | [OA arXiv record **600**](https://openalex.org/W4286748781), versions fragmented; [code](https://github.com/marlbenchmark/on-policy) **2,107 / 385** | Strong cooperative baseline; careful PPO implementation remains important. Independent framework implementations and SMACv2 evaluation establish uptake. |
| [Multi-Agent Transformer / MAT](https://arxiv.org/abs/2205.14953), NeurIPS 2022 | [OA 79](https://openalex.org/W4281622133), arXiv record; [code](https://github.com/PKU-MARL/Multi-Agent-Transformer) **517 / 92** | Autoregressive agent decisions. A visible specialist branch; joint sequential decision-making is not automatically decentralized physical execution. |
| [Heterogeneous-Agent Reinforcement Learning](https://www.jmlr.org/papers/v25/23-0488.html), JMLR 2024; preprint 2023 | [OA arXiv record **22**](https://openalex.org/W4366731969); whole-paper total unresolved; [HARL](https://github.com/PKU-MARL/HARL) **955 / 138**, a multi-algorithm repository | Different policies and update structures for heterogeneous agents. Strong software interest; unresolved whole-paper academic rank. |

For independent implementation evidence, [BenchMARL's algorithm documentation](https://benchmarl.readthedocs.io/en/1.2.1/modules/algorithms.html) includes MADDPG, MAPPO, and QMIX. [JaxMARL](https://github.com/bold-lab-ai/JaxMARL) also implements VDN, QMIX, and MAPPO. Oxford author overlap means JaxMARL is not wholly independent of QMIX; it still demonstrates continued ecosystem use. The [SMACv2 experiments](https://arxiv.org/abs/2212.07489) reuse MAPPO and QMIX as serious baselines; SMACv2 also overlaps with QMIX’s Oxford authors, making the QMIX reuse lineage evidence rather than wholly independent validation.

## 5. Platforms: substantial public attention versus specialist adoption

The table is ordered by **current repository stars**, not scientific quality or citation rank. Counts were captured on September 26, 2026. These repositories cover different scopes and ages. **S2** means Semantic Scholar; missing citation totals mean unresolved or unqueried coverage, not zero influence.

| Platform / associated paper | Stars / forks | Citation evidence and assessment |
|---|---:|---|
| [Genesis](https://github.com/Genesis-Embodied-AI/genesis-world), public release 2024 | **29,989 / 2,868** | Very large public attention. [Author publication notice](https://research.ibm.com/publications/genesis-a-generative-and-universal-physics-engine-for-robotics) identifies an ICRA 2025 workshop paper; no reliable canonical citation total established here. Do not call it the most-cited simulator. |
| [Isaac Lab](https://github.com/isaac-sim/IsaacLab), [paper 2025](https://arxiv.org/abs/2511.04831); software lineage earlier | **8,225 / 3,916** | Substantial active robot-learning ecosystem; new-paper OA count of 3 does not represent years of platform adoption. Broader than multi-agent robotics. |
| [PettingZoo](https://github.com/Farama-Foundation/PettingZoo), preprint 2020 / NeurIPS 2021 | **3,523 / 527** | [**S2 468**](https://www.semanticscholar.org/paper/3a70562df004e08d91b125e6d15255e31e445efa), on the 2020 preprint record. Widely used multi-agent interface; concrete external integration in [RLlib](https://docs.ray.io/en/master/rllib/multi-agent-envs.html). |
| [ManiSkill / ManiSkill3](https://github.com/mani-skill/ManiSkill), v3 preprint 2024 | **3,357 / 546** | General manipulation and visual simulation. Repository predates v3; its entire following cannot be assigned to the newest paper. Adjacent scope. |
| [Brax](https://github.com/google/brax), [2021 paper](https://arxiv.org/abs/2106.13281) | **3,241 / 354** | Established accelerator-based simulation; citation versions fragmented. Broader robot-learning infrastructure. |
| [Isaac Gym examples](https://github.com/isaac-sim/IsaacGymEnvs), [2021 paper](https://arxiv.org/abs/2108.10470) | **2,954 / 515** | Historical GPU-simulation anchor; example repository archived. Isaac Lab is the relevant active successor in this review. |
| [MuJoCo Playground](https://github.com/google-deepmind/mujoco_playground), [2025 paper](https://arxiv.org/abs/2502.08844) | **2,232 / 363** | [**S2 121**](https://www.semanticscholar.org/paper/73c83fc2e4bd2e80e8644aeeeb39cf85c6bf43cb). A well-supported recent platform: citations, public interest, and actual integration in FastTD3. Mainly general robotics. |
| [gym-pybullet-drones](https://github.com/learnsyslab/gym-pybullet-drones), [2021 paper](https://arxiv.org/abs/2103.02142) | **2,146 / 570** | OA **186**. Substantial practical aerial-robotics comparator; closer to SCRIMMAGE's domain than manipulation platforms. |
| [SMAC](https://github.com/oxwhirl/smac), [2019 paper](https://arxiv.org/abs/1902.04043) | **1,368 / 243** | Strong historical adoption; citation fragmentation makes current OA120 unusable as a whole-paper total. StarCraft benchmark, not robot dynamics. |
| [JaxMARL](https://github.com/bold-lab-ai/JaxMARL), [preprint 2023 / NeurIPS 2024](https://arxiv.org/abs/2311.10090) | **854 / 160** | Active specialist platform for accelerator-based simulation and training. Whole-paper citation rank unresolved. |
| [BenchMARL](https://github.com/facebookresearch/BenchMARL), [JMLR 2024](https://www.jmlr.org/papers/v25/23-1612.html) | **666 / 138** | [**S2 66**](https://www.semanticscholar.org/paper/e52b101c0bee3490dbd60ed5c13f8f5287dd0321). Legitimate specialist adoption and reproducible algorithm/environment combinations; smaller following than the major general platforms. |
| [VMAS](https://github.com/proroklab/VectorizedMultiAgentSimulator), [2022 paper](https://arxiv.org/abs/2207.03530) | **614 / 114** | [**S2 95**](https://www.semanticscholar.org/paper/cc9dd80da2d7e4b55033d93d509bd8f98fb4c450). Directly relevant vectorized interacting-agent simulation, with independent use in [SDM 2025 work](https://epubs.siam.org/doi/abs/10.1137/1.9781611978520.55). |
| [OmniDrones](https://github.com/btx0424/OmniDrones), [2023 paper](https://arxiv.org/abs/2309.12825) | **583 / 90** | OA **51**. Relevant GPU aerial specialization; not field-defining solely because it involves drones. |
| [SMACv2](https://github.com/oxwhirl/smacv2), [preprint 2022 / NeurIPS 2023](https://arxiv.org/abs/2212.07489) | **331 / 54** | Smaller repository but a substantial evaluation lesson: test whether coordination actually needs observations and generalizes to unseen scenarios. |

[FastTD3](https://arxiv.org/abs/2505.22642) evaluates Isaac Lab and MuJoCo Playground, and its [implementation](https://github.com/younggyoseo/FastTD3) provides runnable integrations. There is author overlap with Playground, so this is not wholly independent validation of that platform; it is concrete evidence of use. PettingZoo's RLlib integration is a particularly clear example of adoption outside the original project.

The architectural change is not just “GPUs are faster.” Distinguish **many agents in one coupled world**, **many independent worlds**, and **many candidate trajectories per decision**. GPU simulation/training systems often optimize the second axis. Burn inference or trajectory scoring in SCRIMMAGE would not by itself move the full physics and communication loop onto a GPU.

## 6. What changed each year, and when attention accumulated

### Publication and development timeline

This table identifies well-supported milestones, **not the number-one paper in each year**. Formal publication years are used where verified; relevant earlier preprints are noted in the paper tables.

| Year | Milestones | What changed for robot-team experiments |
|---|---|---|
| **2018** | COMA, VDN, QMIX; optimized physical flocking; robust map merging; PRIMAL preprint | Team credit assignment, physical constraints, and shared information were already parallel research strands when SCRIMMAGE appeared. |
| **2019** | SMAC; PRIMAL; MAPF definitions/benchmarks; PIBT; MAAC and CrowdNav | Common evaluation problems, learned local decisions, fast explicit coordination, and attention mechanisms. |
| **2020** | GNN decentralized planning; PettingZoo preprint; RHCR and EGO-Swarm preprints | Local communication becomes a concrete model structure; rolling-horizon solutions advance continuing-task planning, and common interfaces gain explicit formulations. |
| **2021** | RHCR, EECBS, MAGAT, EGO-Swarm; PettingZoo formal publication; Isaac Gym, Brax, gym-pybullet-drones; MAPPO preprint | Search and learning develop together; accelerator-based experimentation and accessible aerial testbeds expand. |
| **2022** | Swarm in the wild; Kimera-Multi journal; MAPPO and MAT; VMAS; PIBT journal and MAPF-LNS2 | Physical teams in unknown environments, collective mapping, scalable planning, and vectorized team simulation. |
| **2023** | RACER; LaCAM; SMACv2; OmniDrones, JaxMARL, BenchMARL, and RoCo preprints | Exploration and workload balancing; stronger search; scrutiny of benchmark generalization; early language-assisted robot coordination. |
| **2024** | Swarm-SLAM, RoCo, HARL, BenchMARL, JaxMARL; SILLM and PARTNR preprints; Genesis release | Communication-conscious mapping, heterogeneous teams, hybrid planning, embodied task benchmarks, and public interest in new simulation stacks. |
| **2025** | SILLM awards; MAPF-GPT; GCBF+; PARTNR; MuJoCo Playground and Isaac Lab papers | Recognized hybrid and safety architectures; new embodied benchmark uptake; substantial accelerator-based robotics ecosystems. Citation maturity varies greatly. |
| **2026 through Sep. 26** | Continued platform activity; PARTNR dialogue extensions; PRIMAL3 preprint; LoRR execution-delay emphasis | Current evidence points to robustness, communication quality, and execution uncertainty. It does not yet establish a new 2026 paper as a durable citation leader. |

### Retrospective citation activity

These are the annual buckets from the selected OpenAlex records, retained in [annual-citation-subset.json](literature-review/2026-09-26/annual-citation-subset.json). **2026 is incomplete.** A dash means the record has no bucket for that year, not that nobody cited the underlying work. Pre-publication citations can reflect preprints. These numbers are not historical year-end snapshots.

| Work | 2018 | 2019 | 2020 | 2021 | 2022 | 2023 | 2024 | 2025 | 2026 partial |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| COMA | 35 | 87 | 111 | 175 | 240 | 301 | 295 | 320 | 181 |
| Optimized flocking | 2 | 31 | 64 | 74 | 79 | 94 | 91 | 100 | 53 |
| PRIMAL | — | 10 | 25 | 41 | 53 | 63 | 86 | 101 | 47 |
| GNN decentralized planning | — | 1 | 11 | 24 | 36 | 51 | 73 | 65 | 15 |
| RHCR | — | — | 2 | 15 | 33 | 38 | 73 | 55 | 34 |
| Swarm in the wild | — | — | — | — | 25 | 113 | 143 | 208 | 121 |
| Kimera-Multi | — | — | — | — | 17 | 43 | 70 | 89 | 59 |
| RACER | — | — | — | — | — | 11 | 62 | 104 | 67 |
| RoCo | — | — | — | — | — | 1 | 26 | 73 | 56 |

This supports a more useful interpretation than a sequence of fashionable titles. Older foundations remain active: PRIMAL and optimized flocking each attracted about 100 indexed citations in 2025. Physical swarm flight, collaborative exploration, and shared mapping have substantial recent activity. RoCo shows a newer coordination branch accumulating attention. MAPPO, QMIX, and SMAC are excluded from this annual table because their fragmented records would produce misleading trends.

## 7. What is getting attention now

“Trending” is used cautiously: this review verifies recent citation activity, current public attention, recognition, and follow-on use. It does not measure week-by-week star growth or social-media reach.

### Recent work worth reading, with its actual evidence tier

| Work | Following checked | Assessment |
|---|---|---|
| [RoCo: Dialectic Multi-Robot Collaboration with LLMs](https://project-roco.github.io/), ICRA 2024; preprint 2023 | [OA 156](https://openalex.org/W4401416363), including 73 in 2025; [code](https://github.com/MandiZhao/robot-collab) **263 / 44**; independent [Tool-RoCo extension](https://arxiv.org/abs/2511.21510) | **Measurable scholarly uptake.** Negotiation plus motion feasibility is the interesting decomposition. Mostly collaborative manipulation, not evidence that LLMs solve aerial coordination. |
| [Scalable Multi-Robot Collaboration with LLMs: Centralized or Decentralized Systems?](https://arxiv.org/abs/2309.15943), ICRA 2024 | [OA 84](https://openalex.org/W4401415431), 47 in 2025 | **Active specialist branch.** Directly compares coordination structures and costs; useful even if the eventual planner is not an LLM. |
| [SILLM: Deploying Ten Thousand Robots](https://diligentpanda.github.io/SILLM/), ICRA 2025; preprint 2024 | [OA 12](https://openalex.org/W4413925254); [code](https://github.com/DiligentPanda/Scalable-Imitation-Learning-for-LMAPF) **74 / 17**; official [multi-robot](https://www.ieee-ras.org/awards-recognition/conference-awards/ieee-icra-best-paper-award-on-multi-robot-systems/) and [student-paper](https://www.ieee-ras.org/awards-recognition/conference-awards/ieee-icra-best-student-paper-award/) awards | **Award-supported emerging work.** Learned action proposals, guidance, and explicit conflict resolution. The 10,000-robot result is simulated, not a 10,000-robot physical deployment. |
| [GCBF+](https://arxiv.org/abs/2401.14554), T-RO 2025; preprint 2024 | [OA 42](https://openalex.org/W4391335029); [code](https://github.com/MIT-REALM/gcbfplus) **136 / 38**; physical demonstrations and same-lab follow-ons | **Emerging specialist safety architecture.** Graph-based safety certificates and distributed control, with jointly learned certificate/controller; not established here as broadly top-cited. |
| [PARTNR](https://arxiv.org/abs/2411.00081), ICLR 2025; preprint 2024 | [Code](https://github.com/facebookresearch/partnr-planner) **391 / 53**; independent 2026 extensions; selected OA count 0 is unusable as a total | **Demonstrable benchmark uptake, citation rank unresolved.** Collaborative embodied tasks with planning and reasoning, primarily household environments. |
| [MuJoCo Playground](https://arxiv.org/abs/2502.08844), 2025 | **S2 121**, repository **2,232 / 363**, implemented use in FastTD3 | **Strong recent platform evidence in this sample.** Adjacent robotics infrastructure, not a multi-agent coordination method. |
| [DIAL-MPC](https://arxiv.org/abs/2409.15610), ICRA 2025; preprint 2024 | [OA 22](https://openalex.org/W4413917178); [code](https://github.com/LeCAR-Lab/dial-mpc) **1,006 / 106** | **Public software interest stronger than citation maturity.** Sampling-based, diffusion-inspired trajectory optimization; chiefly locomotion, but a useful adjacent source for GPU planning ideas. |
| [MAPF-GPT](https://ojs.aaai.org/index.php/AAAI/article/view/34477), AAAI 2025; preprint 2024 | [OA 11](https://openalex.org/W4409347993); [code](https://github.com/CognitiveAISystems/MAPF-GPT) **135 / 24** | **Watchlist.** Expert-generated data and transformer imitation are interesting; the name does not establish landmark status. |

PARTNR's follow-on evidence is particularly instructive. [Dongre and Hakkani-Tür, May 2026](https://arxiv.org/abs/2605.12920), extend it with dialogue under partial observability and report that fewer action conflicts can coexist with worse task success. [LLawCo, June 2026](https://arxiv.org/abs/2606.28182), provides another extension. These establish activity around the benchmark; this review does not claim that either new paper already has a broad following. Communication quality must be measured against mission outcomes, not just apparent agreement.

The [League of Robot Runners 2026](https://leagueofrobotrunners.org/) adds delayed execution and an execution-policy task alongside planning and scheduling. This is evidence that a research community is investing in uncertainty during execution. It is a stronger reason to examine that problem than finding an isolated recent paper with a matching title.

### Relevant papers that should not dominate the history

- [Learn to Follow](https://ojs.aaai.org/index.php/AAAI/article/view/29704), AAAI 2024, has **36** citations on the checked record. Its hybrid architecture is relevant, but the manuscript should not make it the main historical representative while omitting PRIMAL, RHCR, EGO-Swarm, or RACER.
- [Graph Neural Networks for Multi-Robot Active Information Acquisition](https://sites.google.com/seas.upenn.edu/gnn4aia/home), ICRA 2023, has **40** checked citations and an ICRA multi-robot award. A good research direction, with recognition stronger than broad citation volume.
- [Do We Run Large-scale Multi-Robot Systems on the Edge?](https://ieeexplore.ieee.org/document/10610771/), ICRA 2024, has **10** checked citations and the multi-robot award. Its system-size performance collapse is an excellent experiment idea; describe it as recognized specialist work.
- [PRIMAL3](https://arxiv.org/abs/2608.04905), August 2026, is too new for a durable popularity claim. Its relation to PRIMAL and planning-based experts makes it worth monitoring, not a replacement for established evidence.
- Genesis has exceptional repository attention, but that does not establish a top-cited paper or validated dominance in multi-agent robotics. VMAS and BenchMARL have legitimate specialist communities, but should not receive the same public-popularity label as Isaac Lab or PettingZoo.
- Broadly influential single-robot manipulation and vision-language-action work belongs in adjacent context if needed. It should not displace the robot-team literature merely because it has a larger online audience. Surveys, such as [Safe Learning in Robotics](https://doi.org/10.1146/annurev-control-042920-020211), are useful maps of a field, but their citation counts should not be ranked against method papers as if they measure the same contribution.

## 8. Concrete experiments for SCRIMMAGE-RS and Burn

These are proposals inferred from the literature, not claims of novelty, completed implementation, or capabilities already demonstrated by this repository. Each can begin with a readable conventional baseline. A GPU should answer a concrete computational question.

| Question | Literature basis | Small first experiment and measurements |
|---|---|---|
| **Does the task actually require feedback?** | SMACv2 | Compare a time-only script, local feedback, and communication-based coordination. Hold out spawn layouts, goals, and disruptions. Measure completion, collisions, and the loss from hiding observations. This is a cheap prerequisite for meaningful algorithm comparisons. |
| **When do more robots reduce useful work?** | RHCR, PIBT, system-size-collapse study, LoRR | Repeated tasks through shared bottlenecks. Sweep team size, task arrival rate, and execution delays. Plot completed tasks per unit time, queues, deadlock, and travel. Include simple priority rules and centralized assignment. High simulated agent count alone is not useful scaling. |
| **Which messages are worth transmitting?** | GNN planning, MAGAT, RACER, Swarm-SLAM | Separate physical neighbors, delivered-message neighbors, and the algorithm's selected neighbors. Compare periodic, distance-based, risk-based, and later learned messages at equal bandwidth. Measure duplicated work, stale information, completion, and communication cost. |
| **Which decisions should be centralized?** | RACER; centralized/decentralized LLM collaboration | Compare central assignment with local motion, fully local allocation, and a central planner. Introduce coordinator loss and delayed reassignment. Measure recovery, redundant coverage, and workload balance. An LLM is optional. |
| **Do robots need a shared map or only selected facts?** | Kimera-Multi, Swarm-SLAM, PARTNR follow-ons | Give agents partial, occasionally inconsistent target or obstacle reports. Compare sharing raw observations, selected landmarks, and task claims. Measure map disagreement and mission errors separately. Begin with synthetic observations; do not attempt a full visual-SLAM port. |
| **Can explicit repair make cheap proposals reliable?** | PIBT, MAPF-LNS2, LaCAM, SILLM | Generate simple local proposals, then repair conflicts. Compare against planning without repair and a more expensive planner. Measure proposal quality, correction frequency, congestion, and throughput. Learning can be added after the decomposition is useful. |
| **When does GPU planning help the mission?** | EGO-Swarm, DIAL-MPC, accelerator simulation systems | Batch short candidate trajectories or uncertainty samples in Burn and score collision risk, progress, and effort. Compare against the same CPU calculation, charging transfer and planning latency. Vary candidate count and decision deadline; measure action quality and missed deadlines as well as throughput. |
| **Do safety and coordination survive real dynamics?** | Optimized flocking, GCBF+ | Use the same nominal planner with point agents, turning limits, and aircraft dynamics; optionally add a conventional safety correction. Test delayed observations. Measure collisions, interventions, deadlock, and whether method rankings change. A continuous-time certificate is not automatically valid under sampled, delayed information. |

For the planned Burn work, **batched trajectory scoring is the clearest first candidate**: bounded inputs and outputs, a direct CPU reference, and useful computation without requiring a training pipeline. Graph inference is a second candidate if message selection becomes the research focus. Neither requires replacing the deterministic CPU coordinator or adding an optional dependency to every normal run. Full GPU physics, photorealistic rendering, and a giant policy-training stack are separate commitments.

The most coherent initial research sequence is: establish that a continuing-task scenario requires feedback; expose its congestion and communication failures; then test whether a planner, better information exchange, or more candidate evaluations fixes them. This gives the implementation a scientific purpose and keeps performance measurements tied to team outcomes.

## 9. Implications for RQ4 and manuscript flow

The current draft gives Learn to Follow, graph communication, and continuing tasks a useful start, but it underrepresents **physical swarms, exploration, collective perception, benchmark validity, and the scale of the simulation ecosystem**. Those omissions make the field's progress look narrower than the evidence supports.

A sharper RQ4 would be:

> Which developments since SCRIMMAGE's publication change the experiments a multi-agent robotics testbed should support?

Answer it with three concrete requirements: **partial and delayed information; continuing tasks with execution constraints; and evaluation across unseen conditions and model assumptions**. Use the influential papers as evidence for those requirements. GPU computation is an enabling method, not itself the research question.

For the paper's flow, introduce the changed research needs briefly in the introduction, explain the SCRIMMAGE baseline, present the Rust implementation and measured results, and then use RQ4 to discuss what those results enable and what remains missing. A long separate “How the Field Reached This Point” before the reader understands the contribution would delay the main argument. A compact timeline can accompany RQ4 later; the full review belongs here.

A short manuscript need not cite every row. A balanced set would use the physical swarm lineage, RHCR/PIBT, GNN communication, RACER or Kimera-Multi, SMACv2, and Isaac Gym/VMAS as its main anchors. RoCo or SILLM can illustrate one recent architecture with its evidence tier stated. Preserve the distinction between **verified port behavior**, **existing experiment infrastructure**, and **future research proposals**.

## 10. Retained evidence and remaining limits

The following files preserve exact query URLs, selected identities, counts, available annual buckets, retrieval metadata, and unsuccessful requests. Raw search responses retain rejected neighbors so that their exclusion is auditable; presence in a JSON file does not mean inclusion in the review.

- [coordination.json](literature-review/2026-09-26/coordination.json): planning, physical swarms, awards-related candidates, and official repositories.
- [learning.json](literature-review/2026-09-26/learning.json): MARL, communication, heterogeneous teams, and repository snapshots.
- [simulators.json](literature-review/2026-09-26/simulators.json): GitHub, OpenAlex, successful Semantic Scholar cross-checks, and rate-limited requests.
- [additional-systems.json](literature-review/2026-09-26/additional-systems.json): Kimera-Multi, Swarm-SLAM, PARTNR, RoCo, DIAL-MPC, adjacent safety survey, and historical-slide annotation.
- [discovery.json](literature-review/2026-09-26/discovery.json): six broader title-search result sets used to check the candidate pool.
- [annual-citation-subset.json](literature-review/2026-09-26/annual-citation-subset.json): the nine identified records underlying the historical activity table.

There is **no defensible exact global citation leaderboard or historical annual popularity ranking** in these data. Producing one would require a defined, deduplicated subject corpus and reliable historical snapshots. Current Google Scholar totals were not obtained; historical star growth was not reconstructed; social-media interest was not measured. The report therefore identifies established and emerging work using visible evidence, explicitly marks unresolved counts, and avoids filling recent timeline slots with unproven papers.

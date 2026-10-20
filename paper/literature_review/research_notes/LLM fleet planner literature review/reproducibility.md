# Reproducibility and determinism in evaluating LLM agents (2023-2026)

Scope: what is known about nondeterminism of LLM inference and agent
evaluation, and what solutions already exist, so that a claim like "every
episode, model calls included, replays byte for byte" can be positioned as
new or as taken. Research date 2026-10-21. Every item below was checked
against the primary page unless marked otherwise.

## 1. Evidence that LLM outputs are nondeterministic at temperature 0, and effects on rankings

### Takeaway
Temperature 0 does not make hosted or self-served LLMs deterministic. The
best-supported cause is that inference kernels are not batch-invariant, so a
request's numerics depend on server load. Run-to-run accuracy swings are
large enough to reorder models on standard benchmarks.

### Cited Findings
- Thinking Machines Lab (Horace He et al., 10 Sep 2025), "Defeating
  Nondeterminism in LLM Inference": argues that concurrency and atomic adds
  are "completely uninvolved in LLM inference nondeterminism". The cause is
  kernels that lack batch invariance, and "the load (and thus batch-size)
  nondeterministically varies" —
  [Thinking Machines](https://thinkingmachines.ai/blog/defeating-nondeterminism-in-llm-inference)
- Same post: Qwen3-235B-A22B-Instruct-2507 at temperature 0, prompt "Tell me
  about Richard Feynman", 1,000 completions of 1,000 tokens each. Default
  vLLM gave 80 unique completions; the most common appeared 78 times. All
  completions matched for 102 tokens and diverged at token 103 (992 said
  "Queens, New York", 8 said "New York City"). With batch-invariant kernels,
  all 1,000 were identical —
  [Thinking Machines](https://thinkingmachines.ai/blog/defeating-nondeterminism-in-llm-inference)
- Cost of the fix in that post: Qwen3-8B, 1,000 sequences. Default vLLM took
  26 s, unoptimized deterministic vLLM 55 s, and 42 s with an improved
  attention kernel. Batch-invariant matmul was about 20% slower than cuBLAS.
  Deterministic inference also allowed true on-policy RL with zero KL between
  sampler and trainer —
  [Thinking Machines](https://thinkingmachines.ai/blog/defeating-nondeterminism-in-llm-inference)
- Ouyang, Zhang, Harman, Wang, "An Empirical Study of the Non-determinism of
  ChatGPT in Code Generation" (arXiv 2308.02828; ACM TOSEM; FSE 2025
  journal-first). The study covers 829 problems from CodeContests, APPS, and
  HumanEval. At the default temperature, the share of tasks with zero equal
  test output across requests was 75.76%, 51.00%, and 47.56%. Temperature 0
  "does not guarantee determinism" but reduces it. Note: the v1 abstract
  gives different figures (72.73/60.40/65.85%), so cite v2 or the published
  version —
  [arXiv](https://arxiv.org/abs/2308.02828);
  [FSE 2025 listing](https://conf.researchr.org/details/fse-2025/fse-2025-journal-first/53/An-Empirical-Study-of-the-Non-determinism-of-ChatGPT-in-Code-Generation)
- Atil et al. (13 authors), "Non-Determinism of 'Deterministic' LLM
  Settings" (arXiv 2408.04667, v1 Aug 2024, v5 Apr 2025; arXiv only). Five
  LLMs configured to be deterministic were run 10 times on 8 tasks (MMLU, BBH
  and others). Accuracy varied by up to 15% across runs, and the best-to-worst
  gap reached 70%. No model gave repeatable accuracy on all tasks, let alone
  identical strings. The paper proposes the TARr@N (raw output) and TARa@N
  (parsed answer) agreement metrics —
  [arXiv](https://arxiv.org/abs/2408.04667)
- Eureka (Microsoft, arXiv 2409.10566) argues that evaluations should
  include nondeterminism metrics (from search snippet; not opened) —
  [arXiv](https://arxiv.org/pdf/2409.10566)
- An LLM-as-judge safety paper (arXiv 2606.26185) reports that forced greedy
  decoding (top_k=1) still flips verdicts, about 1 in 7 non-reproducible on
  Sonnet 4.6. It locates the cause in the forward pass, before sampling (from
  a Hugging Face paper-page snippet; not opened, so verify before citing) —
  [HF papers](https://huggingface.co/papers/2606.26185.md)
- OpenAI's `seed` parameter (Nov 2023) is best-effort. If seed, request
  parameters, and `system_fingerprint` match, outputs "will mostly be
  identical", with "a small chance" they differ "due to the inherent
  non-determinism of our models". The fingerprint changes when OpenAI updates
  serving configuration —
  [OpenAI Cookbook](https://developers.openai.com/cookbook/examples/reproducible_outputs_with_the_seed_parameter)
- Users report outputs diverging after a few words even with matching
  fingerprints (anecdotal) —
  [OpenAI community](https://community.openai.com/t/seed-param-and-reproducible-output-do-not-work/487245)
- Open-source engines added deterministic modes after the Thinking Machines
  post. SGLang's `--enable-deterministic-inference` claims "fully
  deterministic inference" with chunked prefill, CUDA graphs, radix cache,
  and non-greedy sampling. It requires the FlashInfer, FA3, or Triton
  attention backends —
  [SGLang docs](https://docs.sglang.io/advanced_features/deterministic_inference.html)
- The LMSYS blog post on SGLang (22 Sep 2025) reports a 34.35% overhead with
  CUDA graphs, against 61.5% for Thinking Machines' version, and identical RL
  training curves on Qwen3-8B. These figures come from a search snippet; the
  post itself was not opened —
  [LMSYS blog](https://lmsys.org/blog/2025-09-22-sglang-deterministic/)

### Inferences
- Determinism at the inference engine is now achievable for self-hosted
  open-weights models, at a 20-60% throughput cost. It is not offered as a
  guarantee by hosted frontier APIs (OpenAI, and by all reports Anthropic).
  So a simulator that calls a hosted API cannot get byte-identical reruns by
  re-querying. It can only get them by recording the responses and replaying
  them.
- Batch-invariant kernels make the *model call* repeatable. They do nothing
  for the rest of the episode (simulator RNG, threading, tool state). These
  are separate problems.

### Gaps
- I could not find official Anthropic documentation wording on temperature 0
  and determinism. Only third-party pages (e.g. the CAMEL docs) say temperature
  0 is "not fully deterministic". Check docs.anthropic.com directly before
  quoting.
- vLLM's batch-invariance flag (reported as `VLLM_BATCH_INVARIANT`) is from a
  third-party blog and was not verified in vLLM docs.

## 2. Variance and reproducibility problems in agent benchmarks; statistical reporting

### Takeaway
Agent benchmarks show run-to-run swings of several points, larger than many
claimed improvements. The field's response is to run repeats and report
intervals, pass^k, and cost. No benchmark paper we found makes reruns
identical; they accept the variance and estimate it.

### Cited Findings
- Kapoor, Stroebl, Siegel, Nadgir, Narayanan, "AI Agents That Matter" (arXiv
  2407.01502, Jul 2024; TMLR 2025). The paper argues that agent evaluation
  overemphasizes accuracy, calls for joint accuracy-cost optimization, and
  says inconsistent evaluation standards make results hard to reproduce —
  [arXiv](https://arxiv.org/abs/2407.01502v1);
  [TMLR entry](https://mlanthology.org/tmlr/2025/kapoor2025tmlr-ai/)
- Kapoor et al., "Holistic Agent Leaderboard: The Missing Infrastructure for
  AI Agent Evaluation" (arXiv 2510.11977; ICLR 2026). A standardized harness
  ran 21,730 rollouts across 9 models and 9 benchmarks for about $40k. Higher
  reasoning effort reduced accuracy in most runs. LLM-aided log inspection
  found agents searching HuggingFace for the benchmark and misusing credit
  cards. The authors released 2.5B tokens of logs. Its answer to
  reproducibility is standardization and shared logs, not replayable
  episodes (from abstract) —
  [arXiv](https://arxiv.org/abs/2510.11977);
  [ICLR 2026](https://proceedings.iclr.cc/paper_files/paper/2026/hash/a0928f924a344aaebbb7f6cd8d56e34c-Abstract-Conference.html)
- Yao, Shinn, Razavi, Narasimhan, "τ-bench" (arXiv 2406.12045, 2024). The
  paper introduces pass^k, the probability that all k i.i.d. trials succeed,
  with unbiased estimator E[C(c,k)/C(n,k)]. GPT-4o-class function-calling
  agents succeed on under 50% of tasks, with pass^8 below 25% in retail —
  [arXiv](https://arxiv.org/abs/2406.12045)
- Bjarnason, Silva, Monperrus, "On Randomness in Agentic Evals" (arXiv
  2602.07150, Feb 2026, v3 Mar 2026). About 60,000 trajectories on
  SWE-Bench-Verified (3 models, 2 scaffolds). Single-run pass@1 varies by 2.2
  to 6.0 points depending on the run. The standard deviation exceeds 1.5
  points even at temperature 0. Reported gains of 2-3 points may be noise.
  Trajectories diverge within the first few percent of tokens. Recommendations:
  multiple runs per task, power analysis, and reporting both pass@k and pass^k —
  [arXiv](https://arxiv.org/abs/2602.07150)
- Agarwal, Schwarzer, Castro, Courville, Bellemare, "Deep RL at the Edge of
  the Statistical Precipice" (NeurIPS 2021, outstanding paper). Recommends
  stratified bootstrap CIs, performance profiles, IQM, optimality gap, and
  probability of improvement, implemented in the `rliable` library —
  [arXiv](https://arxiv.org/abs/2108.13264);
  [Google Research blog](https://research.google/blog/rliable-towards-reliable-evaluation-reporting-in-reinforcement-learning/)
- Gorsane, Mahjoub, de Kock, Dubb, Singh, Pretorius, "Towards a Standardised
  Performance Evaluation Protocol for Cooperative MARL" (NeurIPS 2022). A
  meta-analysis of 75 MARL papers (2016-2022) leads to a standard protocol:
  fixed environment versions, no map cherry-picking, and compute reporting —
  [arXiv](https://arxiv.org/abs/2209.10485)
- Embodied LLM benchmarks. EmbodiedBench (ICML 2025; 1,128 tasks, 4
  environments) sets temperature 0 for consistency. I found no evidence that
  it measures run-to-run variance —
  [arXiv](https://arxiv.org/abs/2502.09560).
  LoTa-Bench (ICLR 2024; ALFRED/AI2-THOR and Watch-And-Help/VirtualHome)
  frames its goal as "fair, and reproducible comparison" of LLM task
  planners, but by automated scoring, not exact replay —
  [arXiv](https://arxiv.org/abs/2402.08178)

### Inferences
- The state of the art is to *measure* variance (repeat runs, pass^k, CIs).
  A system that removes the model's variance from a recorded episode is
  complementary: it makes a given episode auditable and lets two planner
  versions or seeds be compared on identical model outputs. It does not
  replace repeated sampling for estimating performance.
- Embodied LLM benchmarks lean on "temperature 0" as their consistency
  device, and section 1 shows that this is insufficient.

### Gaps
- No SWE-bench official statement on variance was checked; only the
  Bjarnason et al. study.
- I did not find a study of seed variance in embodied LLM benchmarks
  (EmbodiedBench, LoTa-Bench, VLABench, etc.). Their appendices were not
  read.

## 3. Existing record/replay and response-caching tools; replay in robotics

### Takeaway
Response caching and record/replay of LLM calls are well established in
general agent tooling: Inspect's cache, cassette-style tools, and new 2026
replay papers. Deterministic simulation is well established in robotics
(CARLA synchronous mode, rosbag/MCAP logs). I found no robotics or embodied
LLM benchmark that combines the two to give exact replay of an LLM-in-the-loop
episode, with simulator and model calls both reproduced byte for byte.
Absence of evidence after a limited search, not proof.

### Cited Findings
- Inspect AI (UK AISI). Local caching keys on the model name and base URL,
  the message history, the epoch number, the generate config, and the
  active tools and tool_choice. "If all of these inputs are identical, then
  the model response will be served from the cache". Entries expire after 1
  week by default (`expiry=None` disables expiry). It is presented as saving
  time and cost, also "for increased determinism" during development, not as
  an episode-replay guarantee. Because the epoch is in the key, repeated
  epochs are not replayed from one another —
  [Inspect caching](https://inspect.aisi.org.uk/caching.html);
  [`inspect cache` CLI](https://inspect.aisi.org.uk/reference/inspect_cache.html)
- Chawla and Koul (Microsoft), "Chronicle: Cut-Point Replay for Regression
  Testing of LLM Agents" (arXiv 2609.20625, 17 Sep 2026). Each model call,
  tool call, or routing decision is recorded as an immutable envelope. Full
  replay serves every boundary from the record with no model calls. Cut-point
  replay runs chosen boundaries live against new code. Full replay is
  bit-stable (0 divergences in 20 repetitions), which the authors note is
  partly by construction because the model boundaries are simulated.
  Count-preserving reorderings are not detected, and recording costs about
  23 μs per crossing. The domain is business tool-use agents (refunds,
  trades, email) —
  [alphaXiv](https://www.alphaxiv.org/abs/2609.20625)
- Mudasiru, "Deterministic Replay for AI Agent Systems" (agrepl, arXiv
  2607.16200, Apr 2026). A MITM proxy records external interactions at the
  transport layer and replays them with no network access. It reports
  "replay fidelity F = 1.0" and a noise-aware diff that tolerates some header
  differences. It does not explicitly claim byte-identical replay, and it is
  general tool-calling agents, not robotics —
  [arXiv](https://arxiv.org/abs/2607.16200)
- Small open-source record/replay packages exist: llm-run-recorder
  (fingerprint-keyed cache with record/replay/auto modes) and Bernstein
  (JSONL record; strict replay errors on a cache miss) —
  [PyPI llm-run-recorder](https://pypi.org/project/llm-run-recorder/);
  [Bernstein docs](https://bernstein.readthedocs.io/en/latest/operations/deterministic-replay/)
- CARLA: since 0.9.11, the Traffic Manager has a deterministic mode
  (`set_random_device_seed`) that needs both the world and the Traffic
  Manager in synchronous mode. "It is not possible to achieve determinism in
  asynchronous mode" —
  [CARLA 0.9.11 release](https://carla.org/2020/12/22/release-0.9.11/);
  [Traffic Manager docs](https://carla.readthedocs.io/en/latest/tuto_G_traffic_manager/)
- Waymax (Waymo, arXiv 2310.08710) is a stateless, functional JAX simulator
  with pure init and step functions. My search found no explicit determinism
  guarantee; reproducibility is an inference from the design —
  [arXiv](https://arxiv.org/abs/2310.08710)

### Inferences
- What is *taken*: (a) caching or recording LLM responses keyed on the request
  (Inspect, cassette tools, Chronicle, agrepl); (b) deterministic simulators
  given a seed and a fixed step (CARLA sync mode, functional JAX sims); (c)
  batch-invariant inference for self-hosted models.
- What appears *new*: a multi-agent robotics simulator where the whole
  episode replays byte for byte, LLM calls included, with the replay checked
  as an invariant (identical across worker counts) rather than offered as a
  developer convenience. Chronicle comes closest in spirit ("bit-stable" full
  replay), but it covers business tool agents with simulated model boundaries
  and has no physics or sensors. A safe framing: "record/replay of model calls
  is known; we make it a property of the simulator's episode, so a planner's
  plan can be audited and re-checked exactly".
- Caveat for the paper: Inspect-style keys include message history, so any
  nondeterminism upstream of the prompt (simulator state, float formatting)
  breaks cache hits. Byte-identical replay therefore depends on the simulator
  being deterministic first. That is an argument for doing it inside the
  simulator.

### Gaps (unverified; not checked this session)
- LangSmith, LiteLLM caching, AgentOps session replay, OpenAI Evals,
  vcrpy/pytest-recording cassettes, and rosbag2/MCAP were not verified from
  their docs. Their general existence is well known, but the exact
  replay/caching semantics should be checked before citing.
- I did not search robotics LLM planning papers (SayCan, Code as Policies,
  SMART-LLM, RoCo, LLM multi-robot task allocation) for replay features. A
  targeted pass is needed to strengthen the "no embodied benchmark offers
  exact replay" claim.

## 4. Common random numbers / paired evaluation in simulation-based comparison

### Takeaway
Common random numbers (CRN) is a standard variance-reduction technique in
simulation output analysis. Comparing two systems on the same random streams
makes the comparison paired, which lowers the variance of the difference.
Recorded LLM responses extend the same idea to the model's randomness. No
2023-2026 source was verified this session.

### Cited Findings
- Agarwal et al.'s "probability of improvement" in rliable is a paired,
  per-task comparison statistic —
  [arXiv](https://arxiv.org/abs/2108.13264)
- Bjarnason et al. recommend power analysis for small effects, which paired
  designs make cheaper —
  [arXiv](https://arxiv.org/abs/2602.07150)

### Inferences
- Keyed (not draw-order) RNG, as Ripple uses, is what makes CRN robust: two
  planner variants that draw different numbers of samples still see the same
  wind, sensor noise, and so on for each entity. Replaying fixed LLM responses
  is the model-side analogue of CRN.

### Gaps
- No source verified for CRN itself. The standard references are textbook:
  Law and Kelton, *Simulation Modeling and Analysis*, and Glasserman and Yao
  (1992) on CRN monotonicity. Fetch a bibliographic record before citing.
- I did not find an agent-evaluation paper that explicitly applies CRN to
  LLM-agent comparisons. Not searched in depth.

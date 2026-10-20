# Roadmap

## Direction (2026-10-21)

A language model plans for a fleet over a library of classical and learned
skills, π0.5-style: the model picks a skill and its parameters for each
vehicle, and each skill is an autonomy plugin. Adding a skill is one Rust
file, and every result replays byte for byte.

Dropped: checking plans in high-fidelity flight before sending them. The
6-DOF model added nothing over a turn-limited model, and the separation
losses came from airspace design, not flight or radio. Comms (delay, loss,
aged reports) are out of scope.

## Paper

Pitch Ripple as the instrument for evaluating language-model fleet
planners, not as another planner. Positioning and sources:
`paper/literature_review/reports/LLM fleet planner literature review.md`.

Already taken, so not contributions: an LLM picking skills as tools, skills
added without retraining, growing skill libraries, caching or replaying
model calls, deterministic simulation alone, the π0.5-style hierarchy.

Four measurements ("we found no prior work that..."):

1. **Noise.** Episodes replay byte for byte, model calls included, at 1,
   2, and 8 workers. Live reruns diverge; split score variance into model
   and world.
2. **Mixed fleets.** Capability violations as the fixed-wing share grows,
   with vehicle cards absent, in text, or in numbers. The fidelity null
   becomes a rule: idealize within an airframe class, model between them.
3. **Skill library.** Grow it, remove skills one at a time, swap a learned
   skill for its classical twin, vary descriptions and granularity; cost
   per skill against success gained.
4. **Where an LLM pays.** LLMs, ReAct, SMART-LLM and HMAS-2 prompts, an
   optimizer, and an auction on identical episodes with scripted events;
   templated against messy requests.

Task: medical delivery (Zipline-style). Fixed-wing aircraft fly long legs
and drop by parachute; multirotors serve short legs and land on rooftops.
Plain-language requests (urgent, routine, cancellations, closures) plus
scripted events; every task also has a structured form for the optimizer
and auction. Base distances, fleet, and request rates on published
operations.

Re-run: Web-of-Drones' coverage and formation tasks (prompts on Zenodo),
with and without fixed-wing aircraft. Optional: exact optima from the
MRTA-Benchmark (TU Delft, 4TU), SwarmGPT primitives against waypoints. Not
building on MultiUAV-Plat; cite it as related work only.

Venue: CoRL 2027; fallback RA-L with IROS; NeurIPS D&B if the leaderboard
leads. Check deadlines.

Maybe: a hardware moment on Crazyflies, revise plans from flown traces,
what-if forks, red against blue.

## Next

- [ ] Skill interface: description, typed parameters, parse the model's
      output into skill assignments.
- [ ] Response cache for model calls.
- [ ] Three or four skills, one mission type, a first library-growth curve
      on development scenarios.
- [ ] Vehicle cards (minimum speed, turn radius, hover, endurance).
- [ ] Gut check first (two weeks): delivery with three skills, two models
      and a greedy auction, five live reruns on the same 50 episodes, one
      mixed-fleet run. Go on only if rankings swap between reruns or
      fixed-wing violations are common.
- [ ] Delivery missions; an optimizer and an auction baseline.
- [ ] Save `nisingizwe2022drone`'s PDF by hand again (deleted with the
      unused sources on 2026-10-21; the Lancet blocks scripted downloads).
- [ ] Before citing: check every [M] source in the report, AMASS (FFG,
      fixed-wing LLM swarms), AutoHMA-LLM, and sweep 2026 preprints; add
      the TODO citations in the manuscript to `references.bib`.

## Evaluation rules

- Development and test scenarios are fixed before prompting; frozen prompts
  on test; report how many prompt versions were tried.
- At least two model families and one open-weights model, pinned.
- Grading is automatic and fixed in advance; baselines get the same tuning
  budget.

## Needed for learning work

- [ ] Drive Ripple from Python (Gymnasium/PettingZoo; PyO3 when speed
      matters).
- [ ] ONNX policy plugin (`tract-onnx`, optional crate), actions as keyed
      random draws.

## Other

- [ ] Rerun the speed checks and accept a new baseline (`perf.py --accept`);
      workload missions' keys were renamed, outputs unchanged.
- [ ] Decide whether to remove MPE (plugins, mission, parity check).
- [ ] Add a delivery-sized multirotor (RotorS Firefly); keep the Crazyflie.
- [ ] ArduPilotBridge servo maps: named fields instead of positions.
- [ ] Viewer: draw Boundary boxes from the scenario, with a toggle.
- [ ] Unicycle dynamics; more sensor models.

## Not planned

Runtime plugin loading, legacy protobuf/string-map spawning, OpenCL, VTK,
ROS, JSBSim. ArduPilot and Burn stay optional, out of the default build.

# Roadmap

## Direction (2026-10-21)

A language model plans for a fleet over a library of classical skills,
π0.5-style: the model picks a skill and its parameters for each vehicle,
and each vehicle's autonomy hosts the skills. Adding a skill is one Rust
file, and every result replays exactly.

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
model calls, deterministic simulation alone, the π0.5-style hierarchy;
rules changed mid-task, less direct wording breaking them more often, and a
symbolic shield removing violations (House Rules, text worlds, 2026). Ours:
a fleet checked from flown trajectories, per-decision slips adding up over a
mission, two equally plain wordings differing, a skill as the fix.

Architecture: fleet planning and vehicle execution are separate autonomy
plugins joined by typed messages (`Dispatcher` on the depot, `Courier` on
each vehicle). Each `Courier` hosts a library of skills: small components
behind one interface, which it starts from an assignment, steps, and turns
into the vehicle's typed controller ports. Adding a skill is one file and a
line in the library (a learned skill could implement the same interface;
not studied, since the failures found are in planning, not execution).
Skills are not each an autonomy plugin: plugins are fixed
when a mission loads, and several autonomies on one entity all write
commands in source order. Claim "autonomy plugins host callable skills",
not "each skill is a plugin".

Measurements ("we found no prior work that..."):

1. **Instructions nobody programmed (core).** Mid-mission rules in plain
   language (closure, no-fly zone, standby, grounding), several wordings
   each, compliance checked from the flight, cost against the same episode
   without the rule. Mistakes are the result, not a failure of the setup.
2. **The right skill as a fix.** Offer a skill that makes a rule easy to
   keep (`drop_via` for the no-fly zone) against rewording it or enforcing
   it in code; remove skills and vary descriptions.
3. **Mixed fleets.** Capability mistakes with vehicle limits in the prompt
   and left out (`limits_in_prompt`). Pilot (10 episodes): with limits
   stated, Haiku refused 3.8% (out of range), Sonnet 1.0%.
4. **Noise, as a control.** Exact replay at 1, 2, and 8 workers; live
   reruns. Pilot: Haiku's three reruns differ by 0.008 in mean score, so
   noise is small here; report it, don't headline it.

No optimizer or auction baselines: the paper studies rules that arrive in
language mid-mission, which a fixed optimizer can't take without an engineer.
Say so in one sentence and cite FLEET and SCoBA. The reference is the same
model on the same episode without the rule.

Task: medical delivery (Zipline-style). Fixed-wing aircraft fly long legs
and drop by parachute; multirotors drop while hovering or land, and only
they serve rooftops. Base distances, fleet, and request rates on published
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

- [x] Skill library (2026-10-22): `Skill` trait and `LIBRARY` in
      `crates/delivery/src/skill.rs`, `drop` and `land` in their own files;
      the prompt and the call check read the library. A pure refactor: all
      39 instruction cases replay from saved replies with identical results.
- [x] Third skill (2026-10-22): `drop_via`, a drop through a waypoint the
      model picks, so a no-fly zone can be routed around instead of costing
      the delivery. Offered only when the Dispatcher's `skills` lists it.
- [x] Response cache for model calls (`crates/delivery/src/planner.rs`).
- [x] Vehicle cards (minimum speed, turn radius, hover, endurance), in each
      courier's status and the prompt; `limits_in_prompt` turns them off.
- [x] Gut check built: `crates/delivery` (dispatcher, couriers, score,
      cached model calls that replay exactly), `pilot.sweep.yaml` (10
      episodes × auction, Haiku ×3, Sonnet ×3), `pilot.py`. A 3-episode trial
      ran end to end.
- [x] Pilot run (2026-10-22, `sweeps/pilot/run002`): no-go on both bars.
      Mean on-time share: auction 0.73, Haiku 0.80-0.80, Sonnet 0.82-0.86
      over three reruns each; no ranking swaps between reruns; refused calls
      Haiku 3.8% (out of range, duplicates), Sonnet 1.0%, none for wrong
      skill or rooftop drops.
- [x] Instruction pilot built (2026-10-22): four mid-mission instructions
      in three wordings, compliance checked from the flight, full decision
      traces (`trace.py`), lost vehicles on empty endurance. The greedy
      auction is gone; the reference is the model with no instruction.
- [x] Instruction pilot run (2026-10-22, 3 episodes, Haiku, wordings 0-2;
      scored in `sweeps/directives-rescored/run002`): breaches count only
      flights or decisions made after the announcement; grounding counts
      time away in the window; standby counts the decision that sent out the
      last idle multirotor. Broken in 12 of 36 runs (33%); wordings disagree
      on 8 of 12 episode-instruction pairs; the plain wording (0) broke once
      in 12. Every run had chances to break its rule (8-22 decisions).
      Closures were always followed. A hypothesis, not a finding: one reply
      sequence per wording, three episodes. The score gap with no
      instruction mixes obeying with deciding differently.
- [x] Wordings 3-5 written for each rule (after seeing the pilot; the
      cleaner test of wording).
- [x] Measurement for it (2026-10-22): `drop_via` goes and returns through
      its waypoint (it used to fly straight home through the zone); every
      call is labeled with whether it breaks the instruction, giving breaches
      per decision and the first breaking decision (agrees with the flight
      in all 36 pilot runs); `enforce_instruction` refuses breaking calls as
      the "enforced in code" reference. `directives.py` groups runs by arm.
- [x] Test run, on new episodes (2026-10-23, seeds 4-8,
      `sweeps/directives-test/run001`): the first wording broken in at most
      2 of 10 missions, the other in 4-8 of 10; per-decision rates match the
      single-decision probe, and $1-(1-p)^n$ predicts missions broken, so
      slips add up. `drop_via` was used only for the clinics behind the zone
      and removed no-fly breaches; enforcement removed all. Two runs predate
      the cache fix and don't replay. The probe script was removed after;
      its numbers are in the manuscript.
- [ ] Fresh run once the setup settles. The prompt no longer changes its
      instruction heading when `drop_via` is offered (straight-line flight is
      stated with the skills), so the `drop_via` arm now differs from the
      direct one only in the skill; replies saved before 2026-10-23 no longer
      match any prompt.
- [ ] 6-DOF check: rerun one rule (both wordings, the five test missions)
      with the fixed-wing aircraft on the 6-DOF model and its autopilot
      instead of SimpleAircraft, and report whether breaches and on-time
      share change. First confirm the 6-DOF autopilot takes the Courier's
      commands and that drops still pass within the release radius; score
      zone clips from wide turns apart from planner breaches. Answers "do
      the results depend on the vehicle model?"
- [ ] Before submitting: AIT's publications for AMASS results, the IEEE
      Xplore venues of SayCan, Code as Policies, SMART-LLM, and COHERENT, and
      a sweep of new preprints.

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

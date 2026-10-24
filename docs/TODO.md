# Roadmap

## Direction

A language-model planner dispatches a mixed fleet (fixed-wing aircraft and
multirotors) by calling skills that each vehicle's autonomy hosts, while
operations add rules mid-mission in plain language. Ripple replays every
mission exactly, so compliance can be measured one decision at a time and
over whole missions, and the same mission rerun with one thing changed. The
paper (`paper/manuscript/main.tex`) is "Ripple: Catching Language-Model
Fleet Planner Mistakes Before Deployment".

Where the results stand (Haiku only; `sweeps/directives-test/run002`,
`sweeps/directives-6dof/run000`): a planner that slips in 7-10% of decisions
breaks 60-90% of missions; for the grounded-vehicle and standby rules one
plain wording is never broken and another breaks 8-9 of 10 missions; the
no-fly zone breaks under both; breaks lean toward urgent deliveries; the
right skill (`drop_via`) removes the no-fly breaches; without vehicle limits
the planner sends fixed-wing aircraft to land about 18 times a mission;
results hold on the 6-DOF aircraft.

Not ours to claim (already published): an LLM picking skills as tools,
growing skill libraries, caching model calls, deterministic simulation
alone, the π0.5-style hierarchy, rules changed mid-task with less direct
wording breaking them more often and a shield removing violations (House
Rules, text worlds). Ours: a fleet checked from flown trajectories, slips
adding up over a mission, two equally plain wordings differing, a skill as
the fix. No optimizer baseline: enforcement in code is the oracle.

## Rounding out the results

What reviewers will push on, most likely first.

Must have:
- [ ] Model adapter: a client behind the Dispatcher's `command` interface for
      GPT, Gemini, and an open-weights model (API or local server), keeping
      the saved replies.
- [ ] Five models: Claude Haiku and Sonnet (two sizes), a GPT and a Gemini
      model, one open-weights model (Llama or Qwen); ideally one reasoning
      model. Pilot one new model end to end first.
- [ ] Twenty test missions per condition (seeds), two replies each, so the
      intervals tighten to about 15-20 points.
- [ ] All six wordings per rule on the test missions (or at least three),
      plus a few wordings written by someone else, to answer "you picked
      wording B because it broke".
- [ ] A mitigation condition: a system prompt saying rules override
      deadlines, and reasoning before calls. Tests the claim that compliance
      needs the edge case spelled out or a guard in code.

Strongly recommended:
- [ ] The deadline claim as a regression: breaking against urgency and slack,
      mission as a random effect. No new runs.
- [ ] A second map: other clinic positions and a zone across other routes.
- [ ] Noise reruns on the test missions (three live replies); the paper's
      noise numbers are from the first pilot.

Nice to have:
- [ ] Remove skills and vary their descriptions (a chart only if graded).
- [ ] Multirotor model check, once the RotorS Firefly exists: rerun the
      standby rule with it, the mirror of the 6-DOF fixed-wing check.
- [ ] The heading test: "vehicles fly straight to a site and straight back"
      beside the rule again; does no-fly wording A hold?

Cost: today's sweep is about 2,300 calls per model at five seeds; the
must-haves come to roughly 10,000-15,000 per model.

## Paper

- [ ] Discussion, in a few sentences, once the results settle.
- [ ] Trim back to the venue's page limit (scoring details into an
      appendix).
- [ ] Redraw the figures from the final runs: `mission_strip.py` (Figs. 1
      and 2), `results_figs.py` (Fig. 4), Fig. 3's call by hand; then remove
      the old-build worktree in the scratchpad.
- [ ] Before submitting: AIT's publications for AMASS results, the IEEE
      Xplore venues of SayCan, Code as Policies, SMART-LLM, and COHERENT, and
      a sweep of new preprints.
- [ ] Venue: CoRL 2027; fallback RA-L with IROS. Check deadlines.

## Evaluation rules

- Development and test missions are fixed before prompting; frozen prompts
  on test; report how many prompt versions were tried.
- Models pinned; at least two families and one open-weights model.
- Grading is automatic and fixed in advance; every reply is saved.

## Engine

- [ ] Drive Ripple from Python (Gymnasium/PettingZoo; PyO3 when speed
      matters).
- [ ] ONNX policy plugin (`tract-onnx`, optional crate), actions as keyed
      random draws.
- [ ] Rerun the speed checks and accept a new baseline (`perf.py --accept`);
      workload missions' keys were renamed, outputs unchanged.
- [ ] Add a delivery-sized multirotor (RotorS Firefly); keep the Crazyflie.
- [ ] ArduPilotBridge servo maps: named fields instead of positions.
- [ ] Viewer: draw Boundary boxes from the scenario, with a toggle.
- [ ] Unicycle dynamics; more sensor models.

## Not planned

Runtime plugin loading, legacy protobuf/string-map spawning, OpenCL, VTK,
ROS, JSBSim. ArduPilot and Burn stay optional, out of the default build.
Comms (delay, loss, aged reports) are out of scope for the study.

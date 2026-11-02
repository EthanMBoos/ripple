# Roadmap

## Direction

A language-model planner dispatches a mixed fleet (fixed-wing aircraft and
multirotors) by calling skills that each vehicle's autonomy hosts, while
operations add rules mid-mission in plain language. Ripple replays every
mission exactly, so compliance can be measured one decision at a time and
over whole missions, and the same mission rerun with one thing changed. The
paper (`paper/manuscript/main.tex`) is "Ripple: Catching Language-Model
Fleet Planner Mistakes Before Deployment".

Where the results stand (2026-10-27; Haiku is complete with two replies
per condition, seeds 4-23, so its counts are of 40 missions; Sonnet has one
reply per condition, with a second reply on the base rows only, so its
upper-rung counts are of 20, and its fresh run is the next step, see
below). At the base prompt the two models break
different rules: Haiku breaks the grounded vehicle in wording B in 24 of 40
missions and Sonnet never does; both break the no-fly zone (15-25 of 40)
and standby B (32-36 of 40); wording A of the grounding and standby rules is
never or almost never broken by either. The ladder lands differently on
each model: `priority` removes Sonnet's no-fly breaks (2 of 20 under B) but
not Haiku's (23 of 40), and `definitions` removes the standby misreading on
both (neither model sends the last idle multirotor while the other reloads,
in over 300 such moments each) and most of Haiku's grounding breaks (4 of
40). Haiku's no-fly zone is not fixed by any rung (19 of 40 under B at the
top rung); `drop_via` fixes it on both models (0 of 40), and enforcement is
0 everywhere. The standby breaks that remain at the top rung (Sonnet 7 of
20, Haiku 8 of 40) are sends at minutes 3-5 that leave the depot empty when
the window opens; the planner notes the call is "before the window opens",
and the score counts the empty depot (stated in the paper and score.rs).
Without vehicle limits Haiku makes about 21 wrong-skill calls a mission,
Sonnet about 2. Breaks lean toward urgent deliveries on both models.

Not ours to claim (already published): an LLM picking skills as tools,
growing skill libraries, caching model calls, deterministic simulation
alone, the π0.5-style hierarchy, rules changed mid-task with less direct
wording breaking them more often and a shield removing violations (House
Rules, text worlds). Ours: a fleet checked from flown trajectories, slips
adding up over a mission, two equally plain wordings differing, a skill as
the fix. No optimizer baseline: enforcement in code is the oracle.

## What I want to show

The main point: you can't tell ahead of time which rule a planner will
break, so you have to run the missions to find out. Ripple makes that cheap.
Tweaking the prompt until it stops breaking doesn't count, since you'd need
to see the failure first.

1. **You can't tell from the wording which rule will break.** Two rules
   that read equally clearly get very different results (one never broken,
   one broken in 8-9 of 10 missions).
   - [ ] Pick all the wordings before running the test missions: six per
         rule (at least three), plus a few written by someone else or by a
         different model. That way nobody can say I chose the one that
         broke.
   - If there turns out to be an obvious pattern, I'll say testing finds it
     faster than guessing.
2. **It happens with other models too.** If different models break
   different rules, that's the best argument for testing.
   - [ ] Hook up other models behind the Dispatcher's `command` interface
         (GPT, Gemini, and an open-weights model), still saving every reply.
         Try one new model end to end first.
   - [ ] Run seven models: the core set on the cheap four, the small set on
         the expensive three (see Budget below).
   - If bigger models never break the rules, I'll report that. Then the
     message is "test the model you're going to use".
3. **How much help does each model need?** Answers "did you just try
   telling it?" The Dispatcher's `prompt.additions` lists sentences added to
   the system prompt (text in the mission's `prompt` block), run as three
   levels on every model:
   - `[]`: the three-sentence base prompt.
   - `[priority]`: adds that instructions from operations take priority
     over deadlines, and that each assignment names the instruction it was
     checked against.
   - `[priority, definitions]`: adds what the vehicle states in the prompt
     mean for a rule: a busy vehicle, including one reloading at the depot,
     cannot be sent and is not one kept at the depot, on standby, or in
     reserve.
   The question is where on this ladder each model stops breaking each
   rule, and whether the models stop at the same rung. The ladder was
   workshopped on the development missions the way anyone writes a prompt;
   the text is fixed for the test runs.
   - [ ] Run all three rungs on every model.
   - If the top rung fixes everything, the takeaway is that you only find
     out what to define by testing. If it doesn't, that's more reason to put
     the rule in a skill.
4. **Small mistakes add up.** Slipping on 7-10% of decisions breaks 60-90%
   of missions. Current runs already show this; it just needs more missions.
5. **Rules break more under deadline pressure.** Breaks lean toward urgent
   deliveries.
   - [ ] Check this with a regression on the existing runs (breaks against
         urgency and slack). No new runs needed.
   - If urgency doesn't explain it, I'll just describe what happened.
6. **The fix is a skill, not a better prompt.** `drop_via` stops the no-fly
   breaches. Without vehicle limits, the planner tries to land fixed-wing
   aircraft about 18 times a mission. Already true; check it again on the
   new models.

To make the numbers solid either way:
- [ ] Run twenty test missions per condition, with two replies each, so the
      error bars shrink to about 15-20 points. The test sweeps now ask every
      mission twice (`repeat`). Haiku is done (1,240 cases, no errors).
      Sonnet still needs its run of `directives-test-sonnet`: the cases
      already asked replay from the saved replies, and the second reply on
      the enforced, no-limits, and prompt-addition rows is about 400 live
      cases (the run started on 2026-10-27 was stopped partway; its replies
      are saved, so the rerun only asks what is missing).
- [ ] Rerun the noise check on the test missions (three live replies). The
      paper's noise numbers still come from the first pilot.
- [ ] Don't pick where the rules land; run every option. Put the no-fly
      zone on the route to each of the six clinics in turn (centered halfway
      along the straight line from the depot), and close each clinic in
      turn. Report all of them. If every option is tested, there's nothing to
      cherry-pick. This is only sweep settings, no code; it costs more model
      calls, so use fewer seeds per placement if needed. The wording
      comparison doesn't depend on this, since both wordings always run on
      the same map.

If there's time:
- [ ] Take skills away, or change their descriptions, and see what happens.
      Only make a chart if it's graded.
- [ ] Rerun the standby rule on the Firefly multirotor
      (`delivery-firefly.yaml`), like the 6-DOF check for the fixed-wing
      aircraft. This needs new model calls, because the vehicles' timing
      changes the prompts. The planner assumes vehicles fly straight lines at
      cruise speed, but the Firefly also brakes and descends, so compare
      arrival times between paired runs before deciding equal scores mean
      nothing changed.
- [ ] Put "vehicles fly straight to a site and straight back" next to the
      no-fly rule again and see whether wording A still holds.

## Budget

Models (seven):
- Claude Haiku, Sonnet, and Opus.
- OpenAI's top three; make one of them a reasoning model.
- One open-weights model (Llama or Qwen). No hosting needed: OpenRouter or
  Together serve them behind an OpenAI-style API, so it reuses the OpenAI
  adapter. Reviewers will ask for one because anyone can rerun it later.
- Gemini only if there's money left; the open model matters more.

Two size ladders also show whether a bigger model fixes it (point 2).

Not every model needs every condition. Each question needs different
coverage:
- "Did you pick the wording?" needs every wording, but only on two models.
- "Did you pick the map?" needs the zone on every route, also only on two.
- "Is it just one model?" needs every model, but only no rule and each rule
  in wordings A and B.

| | Models | What runs | Calls per model |
| --- | --- | --- | --- |
| Small set | Opus and OpenAI's two big models | `directives-small-opus.sweep.yaml`: no rule, each rule in wordings A and B; 10 missions | ~1,200 |
| Core set | Haiku, Sonnet, OpenAI's cheap model, the open-weights model | `directives-test-<model>.sweep.yaml` (one file per model): the small set's rows plus drop_via, enforced, no limits, priority, priority and definitions; 20 missions, each asked twice | ~21,000 |
| Full set | 2 (Haiku and one OpenAI model) | the core set plus all wordings and the zone on every route | ~25,000 |

The core set is 31 conditions x 20 seeds x 2 replies = 1,240 runs at about
17 decisions each. About 110,000 calls in all for seven models, almost all
of them on the cheap ones. The small set's error bars are about 30 points a condition,
enough to tell "never broken" from "broken most of the time", which is all
the expensive models are there to show. Its rows are the first seven of the
core set, so the two compare case for case.

The limit is API access, rate limits, and spend, not code: the adapter and
sweeps are days of work, and the calls run in parallel over a few days.

## Paper

- [ ] Discussion, in a few sentences, once the results settle.
- [ ] Trim back to the venue's page limit (scoring details into an
      appendix).
- [ ] Redraw the figures from the final runs: `mission_hero.py` (Fig. 1),
      `results_figs.py` (Fig. 3), Fig. 2's call by hand.
- [ ] Before submitting: AIT's publications for AMASS results, the IEEE
      Xplore venues of SayCan, Code as Policies, SMART-LLM, and COHERENT, and
      a sweep of new preprints.
- [ ] Venue: RA-L first, submitted as soon as results and writing are done
      (results are a few weeks out: the adapter and sweeps are days of
      work, and the calls run in parallel over a few days). Check this
      year's dates; the ones below are typical, not confirmed.
  - RA-L: submissions any time, a real revision round (reviewers can ask
    for a condition and we can add it), less hostile to simulation-only
    work, and suits an evaluation paper. It can be presented at ICRA or
    IROS; the IROS option usually closes near IROS's own deadline (early
    March).
  - CoRL (usually due late May or June): where SayCan and Code as Policies
    went, so the audience knows LLM planners over skills. But reviewers
    expect learning and often hardware; we train nothing and run only in
    simulation. Worth it only with a small hardware demo (a few quadrotors
    flying the same planner and skills).
  - ICRA/IROS directly: broad, fine with simulation and multi-robot work;
    SMART-LLM and COHERENT went to ICRA. Shorter pages, noisier reviews,
    and an evaluation paper can read as "no method". ICRA 2027's deadline
    (around mid-September 2026) has passed.

## Evaluation rules

- Development and test missions are fixed before prompting; frozen prompts
  on test; report how many prompt versions were tried.
- Models pinned; at least two families and one open-weights model.
- Grading is automatic and fixed in advance; every reply is saved.

## Engine

- [ ] Later, low priority: accept a new speed baseline. The workload keys
      were renamed, so `perf.py` won't compare until `--accept`. A run on
      2026-10-24 (laptop hot and on low battery, right after a long
      equivalence run) had every 1-worker case 2-13% slower than the
      2026-10-14 baseline and most 8-worker cases faster; three 1-worker
      collision cases went past the 10% limit. Most likely the machine, but
      nothing recorded rules out a small code cost since then. To settle it,
      time the 2026-10-14 build and `main` alternately, plugged in and cool,
      then accept.
- [ ] Make the Rascal fly like JSBSim's Rascal, with a JSBSim check as the
      proof. The goal is a path match: from the same start, with the same held
      surfaces, Ripple's `airframe: rascal` flies where ArduPilot's JSBSim
      Rascal flies. Not flight truth; neither model was fitted to the aircraft.
      No second small-UAV reference exists in a form worth pinning (Beard and
      McLain's `mavsim_public` is a student template whose force and derivative
      functions are TODO blocks, checked 2026-11-02), so these JSBSim glides are
      also the fixed wing's path-level check.
      Where the model lives (checked 2026-11-02): the `jsbsim` package (1.3.3,
      Python 3.12 under uv, pinned as Crazyflow is) runs it but ships no UAV;
      the files are in ArduPilot's repo, `Tools/autotest/aircraft/Rascal/`
      (`Rascal.xml`, `Engines/Zenoah_G-26A.xml`, which is an electric motor of
      2207 W despite its name, and the `18x8` propeller), last touched at commit
      `5628b8d2`. Fetch them at that commit into the run folder; commit nothing
      of ArduPilot's. JSBSim stays out of the build.
      Step 1, the check, before any model change: `reference/jsbsim_check.py`
      with a helper under `reference/jsbsim/`, a `HeldControls` controller
      (constant throttle and surface angles, MotorSpeeds's fixed-wing twin), and
      a test in `aerodynamics.rs` that reads a grid fixture like
      `rotors_motor_samples`. Part A: loads at a grid of airspeed, angle of
      attack, sideslip, rates, and deflections, read from JSBSim's property tree
      after `run_ic` (its induced drag lags a frame, but the initial-condition
      run resolves it; holding the state for more frames changed no load to
      1e-16 in the prototype). JSBSim's moment about its reference point and
      about the CG, and its alpha-dot and sideslip-drag terms, are read
      separately, so the terms Ripple lacks are sized one at a time. Prediction
      to test: at the points where the equations coincide (zero sideslip,
      elevator not negative, JSBSim's numbers in the mission keys) Ripple agrees
      to rounding; a miss is a finding to explain, not a tolerance to widen.
      Part B: 5 s open-loop glides at 1 ms from a steady glide at 20 m/s
      (JSBSim's own trim fails on this aircraft, since the propeller's speed is
      a state and its steady-state engine solver diverges; a three-unknown
      Newton step on JSBSim's held-state accelerations converges in four
      iterations), held, then elevator, aileron, and rudder steps. Throttle is
      zero on both sides, where JSBSim's propeller sits at 0 rpm and adds
      nothing. Release-only suite check `jsbsim-rascal`. Step 1 reports the
      residuals of the flown `rascal` on the grid and in the glides, by cause.
      Step 2, the model: add the smallest set of terms Step 1's measurements
      justify, each a mission key that is zero or off unless set, so every
      mission without it is byte-identical (prove with `equivalence.py`); the
      `rascal` preset then sets the values the measurements call for.
      Candidates, read from `Rascal.xml`, with sizes at 20 m/s estimated from
      the file and to be confirmed by Step 1:
      - loads applied at an aerodynamic reference point, 1 in aft and 4 in below
        the CG, with the moment transferred to the CG; lift's arm adds about
        -0.36 to the pitch-stiffness coefficient (Ripple's is -0.5) and about 1
        degree to the trim elevator. Expected to be the largest term;
      - alpha-dot pitch damping, coefficient -7, beside the -12 pitch-rate
        damping; faster decay of pitch transients only;
      - a constant yaw moment, 0.0007 (0.5 N m); hands-off yaw and roll drift,
        trimmed by under a degree of rudder;
      - the elevator drag's form, 0.03 times elevator over 0.30 with its sign
        kept, so up-elevator reduces drag. A defect in the file. If the measured
        path needs it, add it as a named JSBSim-compatibility key, not as a
        silent part of the ordinary preset (about 2 m of altitude over a 5 s
        glide at the trim elevator, estimated);
      - a drag table against sideslip (0.05 at 0.26 rad);
      - aileron and elevator power as Mach tables (1.7% and 1.3% lower at 20
        m/s);
      - the exact wind-to-body rotation at sideslip; the inherited form drops
        the drag's sideslip terms (3% of side force at 0.1 rad). This is a port
        fix, so it goes on `benchmarking-edits` too, with proof.
      Preset numbers to take from JSBSim rather than keep: the mass with its 1.5
      lb of fuel (6.58 kg, not 5.91) and the inertia with it, span 2.795 m, the
      lift and drag tables, induced drag as 0.04 C_L^2, the elevator's -0.35 to
      +0.30 range. Record every change in `REFERENCE_NOTES.md`. The SCRIMMAGE
      comparison keeps the Rascal without C++ gaining any of these terms: only
      the wind-to-body fix goes to `benchmarking-edits`, as a defect fix of
      shared equations; the rest stay Rust-only, like the tables and the landing
      gear, and the JSBSim check is their verification. The compared
      `fixed-wing-rascal` mission overrides them off (zero coefficients, no
      tables, no reference offset), so it keeps checking the port of SCRIMMAGE's
      equations with the Rascal's numbers, which `scrimmage_xml.py` writes out
      as it does now; the number changes need no C++ change. The same holds when
      the A-4D fixtures move to the Rascal.
      Step 3, the proof: rerun the check. Set the glide tolerance from the
      residual sources before running (JSBSim's Adams-Bashforth integrators
      against RK4 at 1 ms, its rotating round Earth, about 4 cm over 5 s, and
      its one-frame lag in alpha-dot), not from the result.
      Later, if powered flight should match too: a propeller model from the
      `18x8` tables with the 2207 W motor, replacing the constant 0 to 25 N
      thrust, plus its torque and P-factor. Not needed for the study; the PID
      flies thrust, not a propeller.
- [ ] Add `airframe: aerosonde`, a lighter aircraft to fly: Beard and McLain's
      Aerosonde from `mavsim_public`'s `parameters/aerosonde_parameters.py` at
      commit `10363e0e` (11 kg; the older `mavsim_template_files` copy says 13.5
      kg, so cite the one used). Beard's terms without a FixedWing6DOF key
      (pitch-rate and elevator drag, side force from aileron and rudder, the
      blended stall curve) are dropped and listed in the doc comment. Checked by
      no run code: no complete, pinned implementation of that model exists to
      compare against, so the doc comment says so. No AircraftPIDController gain
      set until a mission flies it under the PID.
- [ ] Later: FixedWing6DOF's ground spring defaults on (flat ground at z = 0,
      a spring of 400 times mass per metre, damping of 40 N s/m that is not
      mass-scaled). The landing gear replaced it, so default it off and point
      at GroundCollision and the gear. The XML writer already carries the
      flag, so the comparison follows.
- [ ] Retire the A-4D. It is SCRIMMAGE's default aircraft and exists here
      only as a comparison fixture (`fixed-wing-a4`, the generated and
      long-horizon scenarios, three reference inputs, a terrain test, and
      the controller gain set the Rascal's overrides sit on); a fighter jet
      has no other place in a small-UAS simulator. This does not wait for
      the comparison: `reference/scrimmage_xml.py` already writes the
      Rascal's numbers into SCRIMMAGE's XML and `fixed-wing-rascal` is in
      the agreement matrix. Move each fixture to the Rascal (generated
      speeds 15 to 30 m/s instead of 150 to 250), fold the base gains into
      the Rascal's PID set, drop the a4d airframe and gains, and rerun the
      suite so the C++ references regenerate against the Rascal.
- [ ] When the comparison is retired: drop the mission-file shapes kept
      only so SCRIMMAGE's XML round-trips, listed under "Left behind when
      the comparison is retired" in `REFERENCE_NOTES.md` (spawn spread as a
      variance, generation one step early, PID gain order, the spawn flag's
      name).

## Not planned

Runtime plugin loading, legacy protobuf/string-map spawning, OpenCL, VTK,
ROS, JSBSim. ArduPilot and Burn stay optional, out of the default build.
Comms (delay, loss, aged reports) are out of scope for the study.

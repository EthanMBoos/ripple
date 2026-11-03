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

- [ ] Bring the Crazyflie's checks to the Rascal's level of rigor
      (`reference/crazyflow_check.py`, `reference/rotors_motor.py`, against
      `reference/jsbsim_check.py`). Two days. The outcome to reach: the
      common physics agrees with Crazyflow; RotorS-style drag agrees with
      RotorS at the rotor and the vehicle-load level; the flight difference
      from Crazyflow's lumped drag is measured and explained. No physics
      option goes into Multirotor for the test's sake; adaptations live in
      the harness. In parts:
      - Confirm what the 4 m drag difference is. With rotor drag on, Ripple
        and Crazyflow end 4 m apart over 3 s (`paper/data/crazyflow-crazyflie`),
        reported and not judged because the forms differ. The kept run says
        it is the spin: the unequal commands spin the vehicle up at a steady
        0.72 rad/s² in both without drag (0.73, 1.45, 2.17 rad/s at 1, 2,
        3 s); Crazyflow's drag matrix, a force at the center of mass, leaves
        that spin untouched (the same three numbers), while Ripple's RotorS
        drag acts at each hub on the hub's own velocity, rotation included,
        so its moments damp it (0.57, 0.91, 1.05 rad/s), and the tilted
        thrust takes the two apart (Ripple at -3.8 m, Crazyflow at -7.5 m by
        3 s). That is a reading, not a test. Crazyflow's dynamics take an
        external force and torque, so `reference/crazyflow/integrate.py` can
        compute Ripple's hub drag from Crazyflow's state each step and feed
        it in: once as the force alone, once with its torque. If the first
        spins like no-drag and the second damps to Ripple's rate, the
        explanation stands; the drag case then keeps its measured, explained
        difference, and the fed-in run joins the common-physics agreement
        (it checks integration of a model we wrote, like the no-drag case,
        not a second opinion on the drag).
      - A whole-vehicle loads grid: held states (attitude, velocity, body
        rates, every motor commanded, unequal speeds among them) where
        Ripple's summed force and moment about its center of mass, from
        Multirotor's own derivative, are compared with Crazyflow's force and
        moment function (common physics) and with the sum of RotorS's rotor
        loads (drag on). Summing six calls of Ripple's per-rotor function
        would pass with the assembly, the hub offsets, or the frame
        conversion wrong, so the production derivative is what is compared.
        Errors scaled by the individual rotor loads, not the net, since at
        hover the net moment is near zero. The Firefly goes through the same
        grid with its own numbers (0.215 m arms, six hubs 0.037 m up, its
        drag coefficient and motor lag; the RotorS case today uses RotorS's
        defaults, 0.17 m and 1.0e-4), with forward motion to its 12 m/s
        cruise and a climb among the held states. Samples pinned as a
        committed fixture that a unit test reads, as
        `jsbsim_rascal_samples.json` is; the release check regenerates them.
        RotorS today covers one rotor at a time. Alongside, a small test
        that asserts the Firefly preset's six positions, order, and spin
        directions, inertia, coefficients, speed limit, and the 1.545 kg
        from the modeled parts against the pinned xacro, so an edit to the
        preset shows.
      - Flights inside the valid range: several starts and command sets
        (hover, climb, yaw, a tilt), 3 to 5 s, that stay airborne (the
        current drag flight starts at 2 m and is at -3.8 m by 3 s, through a
        ground the model does not have; `multirotor.rs` claims flight away
        from the ground). Report the speeds, rates, and command range
        reached. Keep the spinning case as a labeled stress case.
      - Limits set before the comparison, from the check's own
        sensitivities (Crazyflow and Ripple each at a finer step), for
        position, attitude, and rates, not a fixed 1e-6 m on position
        alone.
      Record the results under a "Checked against Crazyflow" bullet in
      `REFERENCE_NOTES.md` like the JSBSim one, with the before numbers. No
      RotorS or Gazebo flight: the harness compiles RotorS's motor code
      without either, a flight would mostly compare controllers and contact
      models, and a trajectory mismatch could not be attributed the way a
      load mismatch can.
- [ ] One short Courier run on the Firefly, as a test in `crates/delivery`
      with a fixed assignment and no model call: a Firefly sent from the
      depot to a nearby rooftop, approaching at its 12 m/s cruise, entering
      the 3 m arrival radius, completing the dwell, and returning. Check the
      phase transitions, the delivery event, a finite trajectory, and an
      altitude and speed envelope. Half a day. This is the link
      `delivery-firefly.yaml` relies on, tested without an hour-long
      mission; it is a contract test of Ripple's own courier and controller,
      not a check against anything outside. Call it a simulated arrival: the
      Land skill fires on distance and dwell, never on ground contact.
- [ ] Later, weeks, not needed for the study: powered flight for the Rascal,
      a propeller model from JSBSim's `18x8` tables with the 2207 W motor,
      replacing the constant 0 to 25 N thrust, plus its torque and
      P-factor. The PID flies thrust, not a propeller, so the study never
      needs it. Also, if the glides should match closer than 5 cm: a standard
      atmosphere as a mission key that is off by default, so a long descent
      changes the air density as it does in JSBSim.
- [ ] Add `airframe: aerosonde`, a lighter aircraft to fly: Beard and McLain's
      Aerosonde from `mavsim_public`'s `parameters/aerosonde_parameters.py` at
      commit `10363e0e` (11 kg; the older `mavsim_template_files` copy says 13.5
      kg, so cite the one used). Beard's terms without a FixedWing6DOF key
      (pitch-rate and elevator drag, side force from aileron and rudder, the
      blended stall curve) are dropped and listed in the doc comment. Checked by
      no run code: no complete, pinned implementation of that model exists to
      compare against, so the doc comment says so. No AircraftPIDController gain
      set until a mission flies it under the PID.

## Not planned

Runtime plugin loading, legacy protobuf/string-map spawning, OpenCL, VTK,
ROS, JSBSim. ArduPilot and Burn stay optional, out of the default build.
Comms (delay, loss, aged reports) are out of scope for the study.

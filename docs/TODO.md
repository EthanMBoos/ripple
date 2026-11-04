# Roadmap

## Paper question

When a language-model planner assigns a mixed fleet through skills, which
mistakes appear over a mission when the vehicles it commands fly on verified
models? The Rascal 6-DOF and the Firefly are the main vehicles, and every rule
is graded from what the fleet actually flew. The paper should show different
failure modes on different models. It does not claim that verified dynamics
change the result.

The existing kinematic runs show per-decision slips accumulating across a
mission, different grounding and standby behavior on Haiku and Sonnet, and
no observed no-fly breach when `drop_via` was offered. On Haiku, switching from
the kinematic vehicles to the Rascal and Firefly changes outcomes about as
often as asking the model again, and almost every flown breach had a breaking
call behind it. So the failures seen so far come from the plans, not the
flight. The planner sees perfect state and communication is perfect.

The seven-model, all-wording, all-map expansion was cancelled on
2026-11-03. Spend the next effort on Sonnet on the verified vehicles, then one
strong model from another provider. The goal is an RA-L submission for feedback.

## Runs to do

- [ ] **Run Sonnet on the Rascal and Firefly.** About $100. Use the same
      conditions and seeds as Haiku, so the paper's models all fly the same
      vehicles.
- [ ] **One model from another provider.** Run a strong OpenAI model on the
      seven base rows (no rule and A/B for each rule): 20 seeds, one reply,
      about 2,400 calls at 17 decisions per mission. Check the API adapter on
      one short mission first, and if that takes more than a day, submit with
      what ran.
      Compare *which* rules and flight outcomes differ across models, not only
      their violation rates. Without this run, the paper can only make a
      Claude-family claim. Opus and an open-weights model are optional.
- [ ] **Analyze by seed.** Two replies on one seed are repeated model
      samples of one scenario. Pair vehicle and skill comparisons by seed;
      redo intervals and tests accordingly. Keep the independent-slip
      formula as an illustration, not a prediction. Fix `results_figs.py`
      to filter `prompt.additions` and `sample` before using it on the new
      runs. Repeat the live noise check on the test missions; the current
      number comes from the pilot.
- [ ] **Release run** of `reference/suite.py --release --retain` so the
      provisional tags clear and `paper/data/` matches the text.
- [ ] **Check what "verified" covers.** List what each vehicle was checked
      against and over what range: the Rascal against JSBSim, the Firefly
      against RotorS. The multirotor drag notes cite 1.5-4 m/s on a Crazyflie,
      and the Firefly cruises at 12 m/s here, so confirm what covers that speed
      before the paper relies on it.
- [ ] **Write the paper from the finished runs.** Update the abstract,
      intro, Fig. 1, methods, results, and discussion together. Replace
      the five-seed Haiku figures and the old vehicle description, with the
      Rascal and Firefly as the main vehicles and the kinematic set as the
      comparison. Lead with what the planner got wrong and show the different
      model failure modes. Report the vehicle comparison plainly: how often
      outcomes changed against asking the model again, and how few breaches
      had no breaking call. State that replies were not fixed between vehicle
      sets and that the mission has no wind, so this does not show dynamics
      never matter. State that wording B was chosen in development; do not turn
      A/B into a general claim about unpredictable wording. Resolve the model,
      skill-removal, and noise stubs from what ran.
- [ ] **Trim to RA-L length:** six pages plus up to two paid, references
      included; the PDF is nine. Keep the system prompt and one short
      decision in the appendix, move the rest to the code release.
- [ ] **Advisor pass, then submit.** Check RA-L's current page and policy
      text first. The generative-AI statement lists what was actually
      assisted.

## What could go wrong

- **It may read as a language-model evaluation.** Without a vehicle result, the
  robotics contribution is flying and grading the planner on checked models.
  The submission is for feedback, so this is what we want to hear about.
- **The vehicle comparison is limited.** Each vehicle set got its own replies,
  and scoring is per mission, so a small vehicle effect could be hidden. The
  mission also doesn't stress the dynamics. Claim only what those runs show.
- **"Verified" can be overread.** The models were checked against other
  simulators, not real aircraft. Name the references and never imply hardware.

## Rules for the write-up

The original A/B wordings and scoring were fixed before the first test
runs; B was selected on development missions. The primary vehicle set
changed after those runs. State that sequence. Keep prompts frozen within
each comparison, grade automatically, save every reply, and report how
many prompt versions were tried.

## Not planned

Runtime plugin loading, legacy protobuf/string-map spawning, OpenCL, VTK,
ROS, JSBSim. ArduPilot and Burn stay optional, out of the default build.
Comms (delay, loss, aged reports) stay out of the delivery study.
Flying fixed plans, margin metrics, wind, and a harder mission stay out of
this paper.

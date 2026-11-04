# Roadmap

## Direction

Finish the delivery study and submit it to RA-L for feedback.

The study (`crates/delivery`, `paper/manuscript/main.tex`) measures a
language-model fleet planner over whole missions. What it shows:

- Slips in 7-10% of decisions break most missions.
- Two equally plain wordings of a rule get very different results, and
  Haiku and Sonnet break different rules. You have to run the missions to
  find out.
- Offering `drop_via` removed every no-fly breach where no wording did.
- Some breaks look fine as calls and only show up in the flight (a standby
  send made before the window that leaves the depot empty when it opens).

What it does not show: the engine's physics changing the result. The
planner read perfect state, communication was perfect, and the 6-DOF
aircraft did not change the counts significantly.

Decided 2026-11-03: write it up from the runs that exist plus the reruns
below. The seven-model, every-wording, every-map sweep is cancelled; more
of the same would not change the claim.

## Submission

What exists, all on the kinematic fixed-wing and the point multirotor:
Haiku at 20 test missions, 31 conditions, two replies (1,240 cases);
Sonnet at one reply per condition, two on the base rows, under the old
parameter names; the 6-DOF check and the per-decision probe on the pilot's
10 missions; the noise check from the first pilot. Run folders are
laptop-only and not named here.

Decided 2026-11-03: the paper runs on one vehicle set, the Rascal 6-DOF under
its autopilot and the Firefly (one courier trip checked on 2026-11-03: delivered
inside 3 m of the rooftop at 112 s against 100 s in a straight line, home and
idle again at 377 s). The new models need live calls anyway, one vehicle set for
every model is cleaner, and a simulator built around verified vehicle models
should not report its study on a toy. The kinematic runs become the check
instead. Haiku and Sonnet get rerun (Haiku near free, Sonnet about a hundred
dollars), about a week with the analysis. Settle the vehicle set before making
any figure.

Expected reviews, in order: no method (answer: the instrument, compounding,
the skill as the fix, the flight-only breaks); simulation only (one
sentence on what hardware would test); small models from one vendor (the
models item); sample size (the statistics item).

- [ ] **One vehicle set.** `delivery-rascal-firefly.yaml` exists and the
      two test sweeps point at it (6 s a mission in release; flown against
      the prompt's straight-line arrival, the Rascal is within 3 s and the
      Firefly 11-16 s late, so equal scores mean the slower flights never
      made anything late, not that nothing changed). Left: Haiku and
      Sonnet rerun on it, two replies each, with the kinematic results kept
      as the check.
- [ ] **Figure script.** `results_figs.py` does not filter on
      `prompt.additions` or `sample`, so on the twenty-seed data it mixes
      the ladder rows into the base rows. Add both filters, draw every
      model, and recheck every number the text quotes.
- [ ] **Statistics by seed.** Two replies on twenty seeds are twenty
      scenarios, not forty. Count missions broken per seed, compare
      wording A with B and skill with no skill paired on the same seed,
      and redo the intervals and tests. Keep the independent-slip formula
      as an illustration, not a prediction. The abstract's numbers change,
      so do this early.
- [ ] **Flight-only breaks.** Sort every breaking decision into three
      bins: wrong on its face, predictable from geometry (a straight route
      through the zone), or visible only in the flight timing (the standby
      sends). Do this first. If the third bin is small, the system section
      leans on replay and composition, not on checking from the flight.
- [ ] **Sonnet in the paper.** Second model in Fig. 3 and the table, a
      subsection on the prompt ladder (where each model stops breaking
      each rule), and the different-rules finding.
- [ ] **More models, base rows only** (no rule, each rule in A and B,
      twenty missions, one reply, about 1,200 calls each). A strong OpenAI
      model first: it answers "one vendor" and "small models" at once.
      Opus second if the budget allows. One open-weights model (Llama or
      Qwen via OpenRouter or Together, same adapter) because it is nearly
      free and others can rerun it. Prove the API adapter on one short
      mission first; only the CLI and Ollama paths are verified. If that
      takes more than a day, submit with what ran. Both outcomes go in: a
      model that still breaks a rule shows it is not a small-model
      problem; one that breaks nothing means "test the model you deploy".
      Say which client (CLI or API) ran each model. Ladder rows only if the
      base rows are interesting.
- [ ] **Noise check.** Three live Haiku reruns of the base rows on the test
      missions, replacing the pilot's number. The per-decision probe stays
      on the pilot's moments; say so.
- [ ] **Release run** of `reference/suite.py --release --retain` so the
      provisional tags clear and `paper/data/` matches the text.
- [ ] **Text.** Abstract, intro, Fig. 1, results, and discussion from
      twenty missions and every model. Drop "one model". Resolve the stubs
      (more models: done; skill removal: Planned, one sentence; noise:
      replaced). Soften two claims: wording B was picked because it broke
      in development and kept breaking on new missions, so say "we could
      not tell by reading"; and the vehicle check "did not resolve an
      effect", not "no dependence". Add the deadline regression (breaks
      against urgency and slack) if it is an afternoon.
- [ ] **Trim to RA-L length:** six pages plus up to two paid, references
      included; the PDF is nine. Keep the system prompt and one short
      decision in the appendix, move the rest to the code release.
- [ ] **Advisor pass, then submit.** Check RA-L's current page and policy
      text first. The generative-AI statement lists what was actually
      assisted.

Rules for the write-up: wordings and scoring were fixed before the test
missions ran; prompts frozen on test; grading automatic; every reply
saved; say how many prompt versions were tried.

## Not planned

Runtime plugin loading, legacy protobuf/string-map spawning, OpenCL, VTK,
ROS, JSBSim. ArduPilot and Burn stay optional, out of the default build.
Comms (delay, loss, aged reports) stay out of the delivery study.

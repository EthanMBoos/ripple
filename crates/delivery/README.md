# delivery

Medical delivery by a mixed fleet, dispatched by a language model. Two
fixed-wing aircraft drop packages by parachute; two multirotors either drop
them while hovering or land them, and hospital rooftops take only a landing.
Mid-mission, operations may issue an instruction the model was never
programmed for, and the score checks from the flight whether it was followed.

```text
src/dispatcher.rs   the depot: opens requests, asks the model, checks every call
src/planner.rs      the prompt, the saved replies, and the call check
src/skill.rs        the skill library and the Skill trait; skill/ has drop, land, drop_via
src/directive.rs    the mid-mission instructions and their wordings
src/courier.rs      one vehicle: run its assigned skill, return, reload
src/score.rs        on-time share, refused calls, broken instructions
src/messages.rs     what they send each other on GlobalNetwork
missions/           the missions and the instruction sweeps
directives.py       the instruction pilot's summary and bars
trace.py            one run's decisions, replayed from the saved replies
deadlines.py        whether breaking calls came under deadline pressure
probe.py            one decision at a time: saved moments asked again per wording
mission_hero.py     the paper's title map (Fig. 1) from one traced run
results_figs.py     the paper's result figure (Fig. 3) and the numbers the text quotes
```

```sh
cargo run -- run crates/delivery/missions/delivery.yaml --headless
cargo test -p delivery
```

## How it works

- **Episodes.** The dispatcher draws the episode's twenty requests (site,
  item, urgency, opening time) from random numbers keyed by request index, so
  each seed is one episode, identical however it is dispatched.
- **Skills.** Any vehicle can `drop`, but not on a rooftop; only a
  multirotor can `land`; `drop_via` is a drop that goes and returns through a
  waypoint the model picks, so it can route around an area. After any of them
  the vehicle
  returns and reloads, which refills its endurance. The Dispatcher's `skills`
  lists what the model is offered (default `[drop, land]`), so a sweep can
  grow or shrink the library.
- **Checks.** Every proposed call is checked before it is sent: a known, idle
  vehicle and a waiting request, the right skill for the vehicle, no drop on
  a rooftop, and a round trip within 90% of the endurance left. A call that
  fails is refused and counted by reason (`refused_out_of_range`, ...).
- **The model.** Each decision is a plain-language prompt
  (`planner::prompt`) sent through the `claude` command-line client, Claude
  Haiku 5.5 by default. Replies are saved in `runs/llm-cache/`, one readable
  JSON file per prompt (prompt and reply inside), named by a hash of the
  model, the `sample` index, and the prompt. A rerun therefore replays
  exactly without calling the model, and `command: none` makes a missing
  reply an error. A different `sample` asks again. Everything the model is
  told before the state is in the mission's `prompt` block, in full: the
  system prompt, the sentences `additions` can add to it (`priority`, rules
  before deadlines and each call names the instruction it was checked
  against; `definitions`, a busy vehicle cannot be sent and is not one kept
  at the depot), and whether the vehicles' `limits` are stated.
- **Instructions.** `directive` in the mission sets one: a clinic closure, a
  no-fly zone, a multirotor kept on standby, or a grounded vehicle, each in
  six wordings written out in the mission by name (`wordings`), with
  `wording` picking one. The model sees it from `announce_s` to `end_s`. The score
  counts breaches between `start_s` and `end_s` from what the fleet did,
  whatever the wording says (for standby, seconds with no idle multirotor at
  the depot, so a multirotor sent just before the window and still away when
  it opens counts), and
  every call is labeled with whether, flown as planned, it would break the
  instruction (`planner::breaks_instruction`), which gives breaches per
  decision and the first breaking decision. `enforce_instruction: true`
  refuses those calls instead: the rule enforced in code, as a reference.
- **Traces.** Every decision is published in full: what was waiting, which
  vehicles were idle, each call with its predicted arrival and slack, and
  what became of it. `trace.py` reruns a case from the saved replies with
  `--trace` and prints them with each request's delivery time.

## Simplifications

- The multirotors are point models (SingleIntegrator) and the fixed-wing
  aircraft SimpleAircraft; links are perfect. Two variants swap in fuller
  vehicles with the same vehicle cards: `delivery-6dof.yaml` flies the
  fixed-wing aircraft on FixedWing6DOF, and `delivery-firefly.yaml` flies the
  multirotors as RotorS's Firefly hexacopter. The Firefly needs the
  courier's `approach_per_s` to stop on a pad; the book's "Flying a
  multirotor" says why.
- An idle fixed-wing aircraft counts as recovered and on the ground. It is
  drawn loitering only because SimpleAircraft can't stop, inside the depot
  radius where flight uses no endurance. A vehicle that runs out of
  endurance away from the depot is lost with its package.
- The model is told each vehicle's minimum speed and turn radius, but the
  check doesn't use them: with these two skills they never make a call
  infeasible.
- A request's item and units are told to the model but change nothing in
  flight or in the score.

## The instruction pilot

```sh
cargo build --release
python3 scripts/bulk_run.py local crates/delivery/missions/directives.sweep.yaml --jobs 4
python3 crates/delivery/directives.py sweeps/directives/run000
python3 crates/delivery/trace.py run.seed:=2 entities.depot.autonomy.Dispatcher.directive.kind:=closure
```

Three episodes, each with no instruction and with each of four instructions
in three wordings: 39 cases. Run from the repository root so replies land
in `runs/llm-cache/`; an interrupted run resumes from them. Mistakes are the
measurement: `directives.py` reports which instructions were broken in which
wording, what following them cost, and the bars set before running.

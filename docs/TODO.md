# Roadmap

## Study

Language models have been proposed as high-level planners for robot teams, but
they have been tested on teams of 2 to about 30 robots, mostly without vehicle
dynamics. We study how a language-model fleet manager should see and act on a
fleet as it grows to hundreds, and thousands of vehicles. The model and its
prompt are fixed; what varies is the interface between the model and a classical
dispatcher, across fleet sizes from 16 to 4,096. The test problem is one day of
blood delivery across a region, with surges, shortages and closures that force
hubs to compete for vehicles and stock. The full plan is
[FLEET_MANAGER_PLAN.md](FLEET_MANAGER_PLAN.md) and the related work is
[LITERATURE.md](LITERATURE.md).

The earlier study, which asked which mistakes a language-model planner makes,
moved to the `llm-fleet` branch on 2026-11-05 with its delivery crate and
manuscript. It turned into prompt engineering because the model was what it
measured. This study holds the model and its wording fixed and changes code.

### Next

- [ ] Review the plan.
- [ ] Bring back from `llm-fleet` only what the plan needs: skills,
      couriers, call checks, scoring from what was flown, model clients and
      saved replies.
- [ ] Dynamics test, no language model: fly the same orders on the cheap and
      checked vehicle models and look for orders that flip between feasible
      and not, or on time and late.
- [ ] One scripted regional event at 64 vehicles, with the dispatcher only,
      the structured replanner and hand-written orders, to confirm it forces
      a real trade-off between hubs.
- [ ] Systems pilot: 2-hour day, 16 and 64 vehicles, 3 seeds. Checks that
      the pieces work; it does not decide the study.
- [ ] Read the fleet papers whose team sizes are unknown (COLA, FLEET,
      LLM-DaaS, AutoHMA-LLM, MultiUAV-Plat, Scale-Plan) and fill in the
      notes in [LITERATURE.md](LITERATURE.md).

### Risks

- Cost grows with the fleet, in prompt length if not in call count. The
  pilot measures it.
- The paper reads as a language-model evaluation. Every result has to be
  about the interface and the fleet size.
- The result is only that summaries are shorter than full lists. The study
  stops before the large sizes if token cost is the only separation.
- COMPASS (1,024 robots, flocking) and LLM-ODDR (300 taxis, zone summaries)
  are close on scale. The difference is missions with rules that change,
  mixed vehicles on checked models, and the interface as the variable.
- Simulation only. Hundreds of vehicles cannot be flown on hardware, but
  reviewers may still ask.

## To do

- [ ] **Release run** of `reference/suite.py --release --retain`, so
      `paper/data/` reflects the current engine.
- [ ] **Channel congestion** in the networks, delivery that degrades with how
      many nearby agents transmit at once. SphereNetwork has range, loss, and
      delay but not this. Only needed if comms are added to the study.

## Not planned

Runtime plugin loading, legacy protobuf/string-map spawning, OpenCL, VTK,
ROS, JSBSim. ArduPilot and Burn stay optional, out of the default build.
No reinforcement-learning study (decided 2026-11-06); the learning crate
stays as an engine feature. Classical coordination at scale was dropped too.

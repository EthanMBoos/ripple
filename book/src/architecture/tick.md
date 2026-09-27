# The tick

Each ordinary call to `Simulation::step()` runs one tick and returns that
tick's frame: the snapshot taken right after generation, before anything moves
(labelled with the following timestamp, as SCRIMMAGE does). The caller writes
it; the engine does not. The order is fixed, and it is the order SCRIMMAGE
uses, with one planned addition. This is the Rust
counterpart of the simulation loop in the [C++ data-flow guide](../appendix/cpp/data-flow.md).

## Phase order

```text
 tick k
 ┌───────────────────────────────────────────────────────────────────────┐
 │ 1  generate        new entities spawn (schedules, rates, counts)      │
 │    snapshot A      read-only copy of every entity (frame for tick k)  │
 │ 2  autonomy        ═══ parallel ═══  reads belief, snapshot A         │
 │ 3  controllers     ═══ parallel ═══  once per substep, all substeps   │
 │ 4  motion          ═══ parallel ═══  once per substep, all substeps   │
 │    snapshot B      post-motion copy                                   │
 │ 5  sensors         ═══ parallel ═══  read truth and snapshot B        │
 │ 6  interactions    coordinator       collisions, boundaries, health   │
 │    snapshot C      post-interaction copy                              │
 │ 7  networks        coordinator       route messages, schedule delivery│
 │    delivery        ═══ parallel ═══  entity-local sensor readings     │
 │ 8  estimation      (planned)         belief for tick k+1's decisions  │
 │ 9  metrics         coordinator       scores from events and messages  │
 │ 10 removal         entities with health <= 0 are closed and dropped   │
 │ 11 time            time advances by dt; step() returns snapshot A     │
 └───────────────────────────────────────────────────────────────────────┘
```

`═══ parallel ═══` phases split entities across the worker pool and join
every task before the next phase starts. Coordinator phases run on one thread
with access to every entity.

## Rules the order enforces

- **All controller substeps, then all motion substeps.** With
  `motion_multiplier = 10`, every entity runs its controller ten times, then
  every entity runs its motion model ten times. Substeps are never
  interleaved, and a whole entity chain never runs as one parallel task. This
  matches C++ and keeps results independent of worker count.
- **Sensors see the world after motion.** A sensor's reading in tick k
  describes the state at the end of tick k, and autonomy uses it in tick k+1.
- **Decisions see one consistent picture.** Autonomy and controllers read
  snapshot A, taken once after generation, so no entity sees another halfway
  through its update.
- **Removal happens last.** An entity destroyed in tick k still has its
  messages delivered and counted in tick k's metrics, then it is closed.
- **Rates skip, they don't reorder.** A plugin with a loop rate below the
  tick rate keeps its previous outputs on ticks it skips.

## Parallel phases and their cost

Each parallel phase hands entities to the worker pool and waits for all of
them. That hand-off costs about 35 µs per phase with eight workers on bare
metal, and about ten times that inside Docker's virtual machine on a Mac. Light
missions therefore run fastest on one worker, which runs phases directly on the
calling thread; many sensors or heavy models are where more workers pay off.
Measurements are in the paper's performance appendix.

This is also why new work should fit into an existing phase when it can. The
planned estimation step may run inside each entity's delivery task rather than
add a barrier; that choice will be measured.

## Planned: the estimation step

Estimation runs once per tick, after sensor readings and messages have been
delivered and before metrics. Localization runs first, then tracking. Together
they:

1. predict over the tick just completed, using the commands the entity issued
   (what its motion model actually read on each substep: today the final
   controller output, or the autonomy's output when there is no controller);
2. correct with the readings and messages available; tracking combines this
   tick's readings with the pose localization just produced (older readings,
   which need the pose from when they were taken, come later);
3. publish belief for tick k+1's autonomy and controllers.

Ordinary sensors sample before interactions, as real sensors would. The
perfect default, which simple missions use, samples after interactions, so its
belief matches truth at the next decision for every entity present then; an
entity that spawns in tick k+1 is unknown to others until their next
measurement. Startup initializes every entity's belief together from one
snapshot of the whole initial population, before any decision runs. Details
and checks
are in
[ESTIMATION.md](https://github.com/EthanMBoos/scrimmage-rs/blob/main/docs/ESTIMATION.md).

## Start and end of a run

```text
Simulation::new
  build entities from the scenario and initialize each stack:
    sensors → motion → controllers → autonomy                   (today)
    whole population → startup interactions → one snapshot →
      every entity's readings and belief → decision plugins     (planned)
  run interactions once before the first tick (as C++ does)     (today)

step()          runs one tick and returns its frame (snapshot A)
...
step()          after an end condition: runs no tick; time rewinds by dt
                (legacy behavior), EntityPresentAtEnd events are recorded,
                plugins close once, and the terminal frame is returned
step()          returns nothing: the run is over
```

A run ends at the end time, when a plugin requests a stop, when all entities
are gone, or when one team remains, as the mission's end conditions choose. A
plugin error stops the run with the plugin named in the message.

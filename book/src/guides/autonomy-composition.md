# Fighting autonomy plugin bloat

In typical SCRIMMAGE research, a project settles on a few sensors, controllers,
and motion models. Most ongoing development happens in autonomy. Eventually its
`step()` can become a giant mixture of tracking, mission decisions, navigation,
map interactions, state transitions, and exceptions.

The goal is not merely a shorter `step()`. A student changing search behavior
should not have to understand the whole autonomy to make that change safely.

This is design guidance for growing plugins, not a required framework, an
initial-commit requirement, or an implemented new API. Keep simple autonomies
simple. The Rust below is illustrative: application-specific types and methods
are sketches, not code that can be pasted into the current engine unchanged.

## One plugin can contain several ordinary components

The autonomy plugin is what the mission selects. It does not have to be the
smallest unit of code within that behavior.

```rust
struct RescueAutonomy {
    tracks: TargetTracks,
    mission: RescueMission,
    navigation: Navigator,
}
```

These components have different jobs and own different state:

- `TargetTracks`: what the agent believes exists, based on received observations.
- `RescueMission`: what the agent is trying to accomplish and its current phase.
- `Navigator`: how to move toward the chosen goal, including any route progress.
- The existing controller plugin: how to turn desired motion into control inputs.

The autonomy entry point coordinates them:

```rust
fn step(
    &mut self,
    context: &mut AgentContext<'_>,
    io: &mut PluginIo,
) -> Result<Update> {
    self.tracks.update(context.observations, context.time)?;

    let input = MissionInput {
        state: context.state,
        tracks: &self.tracks,
        time: context.time,
    };
    let goal = self.mission.step(&input);
    let command = self.navigation.command(context.state, &goal);

    command.write_to(io)?;
    Ok(Update::Applied)
}
```

The helper structs do not need plugin registration, XML files, pub/sub between
each other, or engine scheduling. Ordinary calls and borrowed inputs are enough.
Their mutable state belongs to this agent, so the engine can still schedule the
whole autonomy as one entity-local update.

Extract a component when it has a distinct responsibility or useful state to own.
Do not create one for every three-line calculation. Moving code into methods
that all access every field of the autonomy just spreads the god object around.

## Sensors still determine what the agent knows

Splitting autonomy does not bypass detection probability or give it perfect truth.

```text
World truth
  -> sensor range / visibility / detection probability / measurement noise
  -> delivered observations
  -> autonomy's tracks or belief
  -> mission decision
  -> desired command
```

A missed detection might produce no observation. The tracking component decides
whether to retain an older estimate, reduce confidence, or forget it. It should
use observation timestamps and process new measurements once, not treat a retained
observation as a fresh detection every tick. Those choices belong to the research
model; a general tracking framework is not required.

States should receive the tracks and known map information they need, not the
entire mutable simulation. Mission-provided knowledge, such as a known home base,
is different from hidden targets that must be detected.

`AgentContext.state` reads the entity's owned belief when one is installed,
otherwise truth. `contacts_truth` still exposes exact world state, so an autonomy
must deliberately consume measurements when sensing matters. See
[Belief vs truth](../concepts/belief-vs-truth.md) for the current behavior.

## A giant state-machine match is still a giant function

Separating tracking and navigation does not solve a mission decision function
that grows to hundreds of lines. Move each substantial state's behavior and
local memory into its own struct.

```rust
enum MissionState {
    Search(Search),
    Approach(Approach),
    ReturnHome(ReturnHome),
}

struct Decision {
    goal: Goal,
    next_state: Option<MissionState>,
}
```

`Search` might own its coverage progress. `Approach` might own a selected track ID
and an arrival tolerance. Neither needs access to all the other's internals.
Keep shared tracks in the autonomy and borrow them; do not copy the whole belief
into every state.

The mission dispatcher stays small:

```rust
impl RescueMission {
    fn step(&mut self, input: &MissionInput<'_>) -> Goal {
        let decision = match &mut self.state {
            MissionState::Search(search) => search.step(input),
            MissionState::Approach(approach) => approach.step(input),
            MissionState::ReturnHome(returning) => returning.step(input),
        };

        if let Some(next_state) = decision.next_state {
            self.state = next_state;
        }

        decision.goal
    }
}
```

This example uses the returned goal for this tick and runs the new state on the
next tick. Choose and document that timing. An urgent interruption may need to
select a new state before dispatch instead. Do not repeatedly advance states
within one tick until something stops transitioning.

Constructing the new state initializes its local memory. Changing state normally
discards the previous state's local memory. If a task must resume later, retain
the specific progress it needs deliberately. Do not introduce a generic lifecycle
system until actual tasks need one.

A possible layout for a larger built-in:

```text
plugin/autonomy/rescue_autonomy/
    rescue_autonomy.rs   # Plugin entry point, configuration/defaults, and composition
    tracks.rs           # Observations and remembered targets
    mission.rs          # Mission state and dispatch
    search.rs           # Search behavior and progress
    approach.rs         # Approach behavior and selected target
    return_home.rs      # Return behavior
    navigation.rs       # Goals to desired motion
```

Only add files as the behavior grows. States are not separately registered
SCRIMMAGE plugins, and their configuration can remain part of the parent plugin's
typed configuration.

## Keep transitions from becoming the next tangle

Two techniques are often enough:

1. Put shared interruptions above the affected states. For example, check
   "battery critical: return home" once instead of in every state. Do not
   reconstruct `ReturnHome` on every tick and accidentally reset its progress.
2. Let complex states own smaller state machines. `Search` can handle selecting
   a region, traveling there, and scanning it. The outer mission only needs to
   know that it is searching and whether it found something.

That second technique is a hierarchical state machine. It can be ordinary enums
and methods; it does not require a state-machine library.

Use named state data rather than growing a collection of overlapping booleans
such as `searching`, `returning`, and `approaching`. Give competing interruptions
an explicit priority. Keep one clear place responsible for the final command;
do not let several behaviors overwrite controller ports in an accidental order.

## World rules do not belong in every autonomy

An autonomy decides to visit a depot or capture a flag. It does not independently
declare that refueling or capture actually occurred.

| Responsibility | Owner |
| --- | --- |
| An object's actual position, existence, and physical state | World/entity state |
| Whether an agent detects it and the reported measurement | Sensor |
| Remembered observations and estimated object state | Agent's belief/tracking code |
| Whether to investigate, approach, avoid, or request an action | Autonomy |
| Whether the physical/game conditions allow capture, refueling, or damage | Interaction plugin |

Known map objects may be supplied as mission knowledge. Hidden objects must
follow the selected sensing model. Use existing typed messages for requests and
results where appropriate; this split does not require a new map framework or
universal action protocol.

## When another decision structure makes sense

Composition and narrow inputs remain useful regardless of the decision algorithm.

| Approach | Good fit | Cost to watch |
| --- | --- | --- |
| Plain functions and structs | Small behaviors and initial research | Boundaries must still be chosen thoughtfully |
| State machine, with nesting when useful | Missions with clear phases and progress | Transitions and interruptions can become tangled |
| Behavior tree | Repeated task sequences, fallbacks, and interruption rules | Running/success/failure and cancellation semantics need to be understood |
| Scored choices / utility selection | Selecting targets or tasks among competing objectives | Score tuning and rapidly oscillating choices |

Do not require every autonomy to adopt the same decision framework. A behavior
tree can live inside one plugin if that project benefits from it. Avoid replacing
one giant struct with a shared, string-keyed blackboard that every behavior can
read and mutate without clear ownership.

C++ already explores composition through `MotorSchemas` and `AutonomyExecutor`.
The useful lesson is composing behavior, not reproducing their nested plugin
loading and string configuration machinery.

## How to start without overbuilding

Take one realistic autonomy as it grows. Separate its belief/tracking, mission
decisions, and navigation only where those responsibilities actually exist.
Move the first oversized state into a small owned component. Generalize shared
support after a second autonomy demonstrates the same need.

Test state logic with small supplied inputs: a detection, a missed detection,
arrival at a goal, or an interruption. Check the selected goal and transition
without needing the full simulator. Keep a mission-level test for sensor timing,
controller outputs, and interactions. When reorganizing existing behavior,
preserve its outputs; label deliberate behavior changes separately.

When debugging gets difficult, expose the active state, selected target, and
reason for a transition through the available diagnostic path. Useful evidence
matters more than adding a state-machine editor or new viewer panels.

The existing autonomy interface can support this organization. No engine rewrite
is needed to begin, and no new framework is prescribed. The goal is that a
student can change one behavior while understanding only that behavior, its
inputs, and its transitions.

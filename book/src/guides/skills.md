# Skills: many behaviors in one autonomy

A vehicle often does different things at different times: wait at the
depot, fly to a site, land, come home. When something above the vehicle
decides which of these it does next, such as a fleet planner, a language
model, or an auction, package each behavior as a *skill*: a behavior with a
name, the parameters a call gives it, and a way to say it is done. The
planner calls skills; the vehicle's autonomy runs them.

The engine has no notion of a skill. A skill library is ordinary Rust inside
one autonomy plugin, so it needs no registration, and it gets the engine's
guarantees (each vehicle's state its own, the same run at any number of
workers) without doing anything for them. This chapter describes the pattern
we recommend, using the delivery crate
([crates/delivery](https://github.com/EthanMBoos/ripple/tree/main/crates/delivery))
as the example: its `Courier` autonomy hosts the skills `drop`, `land`, and
`drop_via`, and its `Dispatcher` on the depot calls them.

## The parts

| Part | Delivery crate | Job |
| --- | --- | --- |
| Skill trait | `Skill` in [skill.rs](https://github.com/EthanMBoos/ripple/blob/main/crates/delivery/src/skill.rs) | What a running skill does each tick |
| Skill description | `SkillInfo` in the same file | What the planner knows about a skill, and the rules its calls must meet |
| Library | `LIBRARY` and `start` in the same file | Every skill, and how a call becomes a running skill |
| One skill | [skill/drop.rs](https://github.com/EthanMBoos/ripple/blob/main/crates/delivery/src/skill/drop.rs) and its neighbors | One struct, its state, and its `INFO` |
| Host | `Courier` in [courier.rs](https://github.com/EthanMBoos/ripple/blob/main/crates/delivery/src/courier.rs) | The autonomy plugin: takes calls, runs the skill, writes commands |
| Caller | `Dispatcher` in [dispatcher.rs](https://github.com/EthanMBoos/ripple/blob/main/crates/delivery/src/dispatcher.rs) | Another entity's autonomy that sends calls as messages |

Your crate defines all of these, shaped to its task. The delivery crate's
are a starting point to copy, not an interface to implement.

## A skill

A skill is a struct with its own state behind a small trait. The courier's
trait is:

```rust,ignore
pub trait Skill: Send {
    /// What the courier reports while it runs.
    fn phase(&self) -> Phase;
    /// Advance one tick; returns the request it delivered this tick, if any.
    fn advance(&mut self, time_s: f64, position_world_m: Vec3) -> Option<i32>;
    fn done(&self) -> bool;
    /// Where to fly, and whether a multirotor descends onto it.
    fn target(&self) -> (Vec3, bool);
}
```

Each skill says *where* to go, not how to fly there: `target` returns a
point, and the host turns it into a heading for a fixed-wing controller or a
velocity for a multirotor. That keeps every skill short (`drop` is 59
lines) and lets one skill run on both kinds of vehicle. A task whose
behaviors need more than a point, such as following a moving contact,
would give its trait a richer target or let skills read the vehicle's
belief; decide that once, in the trait, rather than in each skill.

Next to the struct, each skill declares what the caller needs to know:

```rust,ignore
pub const INFO: SkillInfo = SkillInfo {
    name: SkillName::Drop,
    description: "fly over the site and drop the package by parachute (a multirotor hovers \
                  over it to release), then return to reload.",
    multirotor_only: false,
    rooftops: false,
    via: false,
    lands: false,
};
```

The description is the text a language model reads; the flags are rules the
dispatcher checks before it sends a call, so a call that can't work (a
landing on a fixed-wing aircraft, a drop on a rooftop) is refused and logged
instead of flown. Keeping the description and the rules beside the code they
describe means they change together.

## The host

The host is one autonomy plugin per vehicle. It keeps a small phase of its
own for what the vehicle is doing outside any skill, holds the skill that is
running, and steps in the usual order: input, calculation, output.

```rust,ignore
pub struct Courier {
    phase: Phase,                     // idle, returning, reloading, lost
    running: Option<Box<dyn Skill>>,  // the skill being run, if any
    // ...
}
```

1. **Input.** Read calls from the caller's messages. The courier takes a
   call only while idle and starts it with `skill::start`, which matches the
   call's skill name to a struct.
2. **Calculation.** If a skill is running, advance it; when it reports
   `done`, drop it and move to the host's own next phase (returning).
   Otherwise advance the host's phase (returning, then reloading, then
   idle). Anything that applies whatever the vehicle is doing, such as
   endurance running out, is checked here, once, not in each skill.
3. **Output.** Ask the running skill, or the host's phase, for a target,
   turn it into commands, write the ports, and report status for the caller.

The split to aim for: the host owns everything that is not one behavior
(the vehicle's life cycle, its limits, how commands are written), and each
skill owns only how its behavior goes.

## Adding a skill

In the delivery crate, a new skill is:

1. a file under `skill/` with its struct, its `Skill` impl, and its `INFO`;
2. its name in `SkillName` (messages.rs), the name the model writes;
3. one entry in `LIBRARY` and one line in `start`.

The prompt and the call check read `LIBRARY`, so they pick it up from there,
and a mission offers it to the model through the Dispatcher's `skills`
list. Two versions of a skill, say a classical one and a learned one, can
sit behind the same trait and swap by name.

## Skills and state machines

If you think in state machines, this is a hierarchical one. The host's
phases are the top level, with ordinary guarded transitions: a returning
courier within the depot radius starts reloading, and a reload that has run
its time makes the courier idle. Each running skill is a state with a small
machine of its own inside (`drop_via` goes to the waypoint, on to the site,
and back to the waypoint), and `done` is its transition out.

What differs from a hand-written machine is the transition out of idle.
Which skill to enter, and with what parameters, is not a coded guard but a
call from the planner. The `SkillInfo` rules are the guard on that
transition: they decide which calls are allowed, and the planner chooses
among them. The same pieces go by other names elsewhere: a skill is an
action node in a behavior tree, or an option (where it may start, its
policy, when it ends) in reinforcement learning.

The delivery crate leaves out two transitions a hand-written machine often
has. A skill cannot fail; it runs until it is done or the vehicle is lost.
And nothing interrupts a running skill; a new call waits until the vehicle
is idle. If your task needs either, add it to the trait and the host: an
outcome such as `Running`, `Succeeded`, or `Failed(reason)` from `advance`,
and an abort call the host accepts in any phase.

## One autonomy, not one per skill

Don't make each skill its own autonomy plugin on the same vehicle. An
entity's autonomy plugins all run every tick, in mission order, and each
copies every output it declares onto the vehicle's commands, so the last one
that declares a port always sets it. A plugin with nothing to say still
repeats its last value, which starts at 0. Nothing marks one plugin as in
control, and plugins are fixed when the mission loads. Two autonomies on one
vehicle work only when they don't share ports, such as one that steers and
one that only sends messages.

SCRIMMAGE composed several autonomy plugins with a state machine instead;
[REFERENCE_NOTES](https://github.com/EthanMBoos/ripple/blob/main/docs/REFERENCE_NOTES.md)
says what was left out and why.

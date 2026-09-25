# Remaining implementation work and intentional breaks

Reviewed against sibling `../scrimmage`, branch `Ubuntu-24.04`, on 2026-09-24
(source snapshot `4bdf41fb06facaea358d478d9a07aa4d77fcac27`, not a project pin).
This roadmap supersedes the earlier blanket promise to copy every legacy
mechanism. Preserve SCRIMMAGE's useful model-authoring design and verify the
behaviors we retain; do not recreate unwanted machinery to call it a full port.
Completed implementation and verification are recorded in [EVIDENCE.md](EVIDENCE.md).

The seven roles remain: autonomy, controller, motion model, sensor, interaction,
network, and metrics. Ordinary model code stays small; the engine owns lifecycle,
phase barriers, identity, and message timing. Copied C++ guides are historical
references, not authority to restore explicitly excluded features.

## Delivery principle: commit honest gaps, not throwaway architecture

The first v1 commit is an incomplete, working baseline for its verified subset,
not a claim that the whole port works or that the design is finished. XML can
remain, integrations can be absent, and known failures can remain documented.
Prefer an explicit unsupported-feature error and an unchecked task to temporary
string passthrough, duplicated spawning paths, or compatibility glue that we
already intend to delete. Missing capability is acceptable; silent failure or
pretending it is implemented is not. This roadmap does not authorize a git
commit or the implementation of all these changes now.

Build the clean boundary when its first real consumer is implemented. Do not
build a speculative framework upfront, and do not wire an integration through
the old path just to demonstrate progress. Existing legacy behavior can remain
isolated until deliberately replaced; keep its verification evidence and make
intentional behavior changes explicit.

## Scope decisions

| Area | Decision or status | Long-term consequence |
| --- | --- | --- |
| Compiled Rust extensions | Keep, explicitly confirmed. Applications can compile/register their own models against the core. | Adding code means rebuilding the consuming application, not changing the simulation loop. |
| Runtime plugin loading | Excluded: no generic `.so`/`.dylib`/DLL discovery, loading, or hot reload. | No drop-in binary plugin distribution or Rust plugin ABI commitment. Compiled extensions remain supported. |
| Legacy spawn protocol | Excluded: `GenerateEntity` protobuf commands and arbitrary entity/plugin string override maps. | Old publishers and Straight's spawn demo will not work unchanged. A future narrow ROS/ArduPilot connection request is not this legacy protocol. |
| Connection-triggered spawning | Planned: one configured ROS robot session or ArduPilot connection creates/binds one SCRIMMAGE entity from a validated keyed template. | Scheduled and external triggers share one typed engine-owned creation path. No integration-specific branches in the spawning loop. |
| Legacy GPU path | Do not port OpenCL/kernel-path machinery. Burn is the preferred technology to evaluate for new GPU work. | No legacy GPU XML/kernel compatibility; no promise that every model will run on a GPU. |
| Integrations | Limit runtime candidates to ROS and ArduPilot. ROS 1 compatibility is required alongside ROS 2; exact message/control coverage is open. | Separate optional adapters and acceptance environments, not a universal bridge or default-build dependencies. |
| JSBSim | Verification-only: offline reference fixtures for selected Rust flight models. No production plugin, runtime integration, or CXX bridge. | Maintain a small reproducible test corpus, not a JSBSim engine port or general aircraft-XML compatibility. |
| Mission format | XML and YAML both supported permanently, feeding one typed `Mission`. XML is frozen for single runs; YAML is native and alone gets templates and Ripple-style sweeps. | No XML templates or sweeps. XML/YAML equivalence is checked by paired fixtures and `scrimmage compare`. |
| Mission corpus | Curated regression/stress fixtures, not every upstream mission. | Coverage follows shipped models and failure modes, not XML file count. Fourteen working missions are retained; unported demos stay upstream. |
| Remaining model catalog | Inventory/prioritize standalone C++ models separately from integrations; final selection is open. The [plugin audit](PLUGIN_COVERAGE_AUDIT.md) added NoisyContacts and SphereNetwork + AuctionAssign. | Not copying every mission does **not** automatically mean dropping its useful motion/sensor/autonomy models. |

Already agreed boundaries remain: Rerun instead of VTK, deterministic CPU
workers, numbered run folders, Python comparison, and `scrimmage run` / `replay`.

Completed baseline work and repeatable verification commands are recorded in
[EVIDENCE.md](EVIDENCE.md); this file lists remaining work.

## 1. Keep spawning typed and engine-owned

The intended design is plain: a mission schedule or an external connection asks
for an instance of a predefined entity template. The engine creates that entity
at a generation boundary. One configured ArduPilot connection or ROS robot
session corresponds to one entity, not one entity per message or reconnect.
General autonomy/plugin-requested spawning is not an initial requirement.

Separate three responsibilities:

- Template: the validated models, sensors, controller, team, and default initial
  state. An instance can supply explicitly typed initial state where needed.
- Trigger: a mission schedule or a ready external connection requests creation.
- Engine: the single creation path owns IDs, construction, insertion, and events.

For the first connection-spawn implementation, configure a stable instance key,
endpoint/robot namespace, and template key in our mission input. Resolve keys
before runtime. The adapter submits an ordinary Rust request containing a
resolved template handle and connection identity; the spawning loop knows
nothing about ROS, ArduPilot, sockets, protobuf, XML, or YAML.

Scheduled spawning exists in
[generation.rs](../crates/core/src/simcontrol/generation.rs). Retain its tests and
coordinator RNG/ID ownership. It currently mixes scheduling, legacy spawn
randomization, and construction. Separate request production from construction
before adding connection-triggered spawning; do not bolt adapter branches onto
this function. Rejecting legacy protobuf commands does not mean removing
scheduled populations, lifecycle events, or typed pub/sub. The new shared path
and connection-spawn capability are not implemented yet.

The rejected path is concrete: [Event.proto](../../scrimmage/msgs/Event.proto)
defines `GenerateEntity` with entity/plugin string maps;
[SimControl.cpp](../../scrimmage/src/simcontrol/SimControl.cpp) converts state back
to strings and applies arbitrary overrides;
[Straight.cpp](../../scrimmage/src/plugins/autonomy/Straight/Straight.cpp) demos it
using `gen_straight`. No RRT implementation/demo was found in this checkout; the
claimed historical origin was not independently verified.

- [ ] Test that `generate_entities=true` fails descriptively. Never silently
  ignore it or add a legacy compatibility shim for the rejected protocol.
- [ ] Introduce validated template/instance identities and one typed creation
  request path when implementing the first connection-spawn pilot. Adapt existing
  scheduled generation to that same path, preserving selected behavior/tests.
  No generic plugin spawn API is needed for this milestone.
- [ ] Keep template definitions separate from activation: a connection-bound
  template must not also create an entity through the default scheduled count.
  Keep initial pose in the template or a typed instance definition, not arbitrary
  string overrides. Validate the template's registered models and wiring before
  opening endpoints, even if no peer ever connects.
- [ ] Have the adapter request creation once its configured peer is ready.
  ArduPilot uses a valid initial protocol exchange; ROS needs readiness for the
  selected robot session, not merely an open rosbridge WebSocket. Keep a stable
  instance-to-entity binding and reject conflicting peers/requests. One instance
  has at most one live entity; duplicate packets/requests cannot allocate more.
- [ ] Define request lifecycle first: tick-N requests commit at a documented
  next generation boundary. Merge by stable entity/plugin/sequence identity,
  never worker completion order. Only the coordinator allocates IDs/random draws
  and mutates entity storage.
- [ ] Validate runtime state before construction. Return an explicit creation
  result/entity ID to the adapter; no success event or active binding before
  complete construction. Bound pending requests/populations and test duplicate
  readiness, missing templates, conflicting bindings, partial construction
  failure, removal, and cleanup. Removal must not let a stray packet respawn an
  entity; reactivation needs an explicit lifecycle decision.
- [ ] Start with a clear failure on required-peer timeout/disconnect, not
  automatic despawn/recreate or indefinite waiting. Reconnect/resume behavior
  can be added deliberately later; neither TCP reconnect nor an ArduPilot reset
  grants permission to create a second entity.
- [ ] Use a dedicated typed structural command path, not accidentally lossy
  simulated GlobalNetwork. Live external requests need accepted-tick assignment
  and recording; uncontrolled packet arrival is not deterministic.

No new protobuf spawn interface is necessary for the configured-binding pilot.
ArduPilot's native actuator protocol has no template key; the Rust adapter knows
which template belongs to its configured endpoint. If remote callers later need
to select a template, restrict that control interface to ROS/ArduPilot session
registration: stable instance key plus template key, with explicit success/error.
Protobuf is an optional wire encoding there, not an internal engine dependency.
Keep it separate from ArduPilot's native physics packets and do not modify the
flight stack just to send it. Decode/validate/resolve once at the boundary; no
serialized plugin definitions or arbitrary override maps. Do not build this
extra control service until remote selection is actually needed.

Retain legacy `frames.bin` protobuf I/O for selected C++ comparisons for now.
That output format is independent of protobuf-driven spawning. Internal events
should remain ordinary Rust data, serialized only at output/transport boundaries.

## 2. Mission input: XML kept, YAML added, sweeps in YAML

Decided (2026-09-25): both formats stay supported permanently.

- **XML** remains the input for ordinary single runs and keeps working
  unchanged, including the baseline and reference-comparison missions. It is
  frozen: its existing `entity_common`, `param_common`, includes, and
  substitutions stay, and it gets no new features.
- **YAML** is the native format. It runs the same single missions, and only
  YAML gets templates and parameter sweeps.
- **Sweeps** follow [Ripple](../../ripple): a `*.sweep.yaml` names a base YAML
  mission, `seeds`, `parameter_combinations: cartesian | zip`, and dotted-path
  `parameters` lists. Each case patches the base YAML tree, then loads it like
  any other mission. Patching a tree is why sweeps are YAML-only; XML would need
  a second mechanism. Ripple's `crates/ripple/src/sweep.rs` (stable case IDs,
  shards, per-case result rows) and `scripts/bulk_run.py` + `bulk_run.slurm`
  (local or Slurm shards, collection) are the implementation to port.

```
mission.xml  --(XML reader: text values)--+
                                          +--> Mission --> resolve/validate --> run
mission.yaml --(YAML reader: native)------+
     ^
sweep.yaml -- patches base YAML per case -- case IDs/shards -- bulk_run.py
```

Plugin code does not change: `params.parse()` fills the same struct from XML
text (the adapter in [params.rs](../crates/core/src/parse/params.rs), which
keeps `"1, 2"` lists and `1`/`0` booleans) or from YAML values natively.

- [x] Type plugin parameters. Each plugin deserializes a serde struct whose
  `Default` holds its defaults; unknown keys and invalid values are errors
  before the run starts. These structs are the format-neutral plugin layer.
- [x] Make `ScenarioConfig` the typed mission: entity fields are numbers and
  vectors, and each plugin holds its values as XML text or YAML. The XML reader
  ([xml_mission.rs](../crates/core/src/parse/xml_mission.rs)) fills it and no
  longer keeps the expanded XML. All curated missions stayed byte-identical.
  `loop_rate` and a sensor's `instance` are now framework fields removed before
  the plugin parses, fixing a latent bug where `instance` was rejected.
- [x] Add the YAML reader ([yaml_mission.rs](../crates/core/src/parse/yaml_mission.rs)).
  Unknown keys are errors, non-finite numbers are rejected by path, and entities
  and plugins are map keys (see [MISSION_YAML.md](MISSION_YAML.md)). Trailing
  `path:=value` arguments override YAML by dotted path; every part must exist.
- [x] Add `scrimmage compare a.xml b.yaml`: setup differences by path, then both
  runs must give byte-identical `frames.bin`, `events.json`, and `summary.csv`.
  [yaml_missions.rs](../crates/core/tests/yaml_missions.rs) checks every paired
  mission; unit tests cover defaults, overrides, unknown keys, and invalid values.
- [ ] Decide whether templates come before sweeps. A review suggested
  load -> expand templates -> apply overrides -> validate, so overrides and
  sweep paths can reach inherited fields; that order fixes how sweeps work.
- [ ] Startup message queues cap large populations: a 1,500-agent mission fails
  with `publisher queue full (1024 messages)` publishing
  `GlobalNetwork/EntityGenerated` ([messages.rs](../crates/core/src/pubsub/messages.rs)).
  Fix before promising thousand-agent runs.
- [ ] Write YAML forms of the remaining curated missions. Done: straight-no-gui,
  multirotor, waypoints-point-agents, verification/noisy-state-bias.
- [ ] Port Ripple's sweep runner and Slurm launcher: `scrimmage sweep`, stable
  case IDs, shard selection, one result row per case, `bulk_run.py`.
- [ ] Add YAML templates: template identities, typed inputs/defaults,
  duplicate/unknown-name errors, include-relative paths/cycles/depth, and
  override precedence. Choose replace versus append for lists. Prefer shallow
  composition to inheritance or deep merge. Templates feed the typed creation
  path in section 1.
- [ ] XML template gaps stay documented, not fixed: C++ lets a child replace an
  inherited motion model, while Rust concatenates both and rejects the second;
  Rust does not keep entity `tag` attributes as template IDs. See
  [MissionParse.cpp](../../scrimmage/src/parse/MissionParse.cpp) and
  [mission.rs](../crates/core/src/parse/mission.rs).
- [ ] Save effective configuration/provenance: defaults/overrides, assets,
  selected registrations, seeds, and applicable adapter/backend versions.
  The manifest records the mission's plugin values, not the `Default` values
  a plugin filled in.
  Parse-time strings must not become runtime world-mutation commands.
- [ ] Revisit environment-based defaults separately. `SCRIMMAGE_PLUGIN_PATH`
  currently searches XML, not code. Do not remove working overlays just because
  library loading is excluded. Explicit application-owned defaults/asset paths
  are the proposed later boundary.

## 3. Optional integrations: use the natural boundary

Confirmed: connect to ArduPilot over UDP; keep ROS behind a separate bridge,
with no ROS installation, native ROS libraries, or ROS build dependency required
by the Rust application. Both ROS 1 and ROS 2 remain in scope. Keep adapters
optional and middleware/wire types out of ordinary core state and contexts.
Default builds and aircraft tests must not require running either integration.

Use the existing ecosystem interfaces rather than inventing a universal
protocol. Protobuf is serialization, not a transport requirement; the current
`frames.bin` protobuf I/O does not imply a working ROS or ArduPilot connection.
These runtime adapters are not implemented or verified yet.

Live pause/resume of ROS, ArduPilot, and the running simulation is intentionally
out of scope for the first integrations. Coordinating those processes while
paused is a bonus for later, not an acceptance requirement. Normal physics
stepping, response timeouts, and clean shutdown still need to work; none of
those requires adding pause controls. Rerun's recording playback is unaffected.

| Integration | First implementation direction | Consequence |
| --- | --- | --- |
| ROS 1 | Optional Rust client to `rosbridge_server` over WebSocket/TCP; ROS and the bridge run in a separate Linux/container environment. | No in-process `roscpp` lifecycle or host ROS build. Keep the legacy environment isolated and test the selected message set. |
| ROS 2 | The same external rosbridge boundary, using a ROS 2 bridge deployment. | No native `rclrs`/ROS build dependency. Share domain commands/observations, with explicit ROS 1/2 message mappings where needed. |
| ArduPilot | Separate SITL process/container, documented JSON-simulator UDP interface. Rust owns vehicle physics. | Use its binary actuator packets and JSON physics input, not a protobuf wrapper, ROS intermediary, or embedded flight stack. |

[rosbridge](https://github.com/RobotWebTools/rosbridge_suite) already supplies
message framing and ROS publish/subscribe operations over WebSockets.
[roslibrust](https://github.com/RosLibRust/roslibrust) is the first client candidate
to evaluate: its rosbridge backend supports ROS 1 and ROS 2 without a ROS
installation for the Rust build. This is a proposed dependency, not a verified
integration. Prefer this existing bridge to a custom protobuf/framed-TCP server;
do not build a generic bridge framework or compatibility-negotiation layer.

TCP is reasonable for the initial modest state/command workload, not a universal
simulation rule: [ArduPilot's JSON interface](https://ardupilot.org/dev/docs/sitl-with-JSON.html)
uses UDP, while [PX4's MAVLink simulator interface](https://docs.px4.io/main/en/simulation/)
uses TCP. TCP's ordered delivery can delay newer data behind older data. Bound
application queues and reject stale commands; do not claim hard real-time
behavior. Measure before extending JSON/WebSocket to large camera/point-cloud
streams. A transport change needs a demonstrated requirement, not speculation.

ROS 1 remains required, but Noetic reached EOL on 2025-05-31. Maintaining it
requires a pinned legacy environment and maintenance ownership, not an assumption
of upstream updates. Official `ros1_bridge` guidance does not support Ubuntu
24.04; do not rely on that bridge inside our reference image. Containers isolate
packaging, not the underlying end-of-life risk. See the
[official EOL notice](https://www.ros.org/blog/noetic-eol/) and
[bridge compatibility guidance](https://index.ros.org/p/ros1_bridge/).

- [ ] Choose one end-to-end use case per adapter and implement one pilot at a
  time. For ROS, start with clock, odometry, selected sensor observations, and a
  specific command type. Add TF/custom messages/services only when that use case
  requires them. Supporting ROS 1 does not mean porting every legacy ROS plugin.
- [ ] Implement the first pilot on section 1's template/instance binding and
  shared spawn path. One configured connection creates one entity when ready;
  later traffic controls that entity. Leave this feature explicitly unavailable
  until the clean lifecycle exists rather than adding a temporary legacy spawn
  translation. Test connection readiness, creation failure, duplicate requests,
  timeout, removal, and accepted-tick recording end to end.
- [ ] Pilot rosbridge/roslibrust against separate pinned ROS 1 and ROS 2 container
  targets. Keep the required message definitions self-contained for Rust builds;
  do not discover them through a host ROS installation. Test message type names,
  timestamp/header differences, frame IDs, `/clock` and simulation-time settings,
  and the chosen ROS 2 QoS. Do not assume the two versions behave identically.
- [ ] Keep configuration small: endpoint, entity/topic namespace, required topic
  mappings, and rates/timeouts. Use typed supported messages, not arbitrary
  message/configuration passthrough. No legacy spawn service, general graph/API
  facade, protocol negotiation, or promise of broad backward compatibility.
  Record the tested client/bridge versions and fail clearly on unsupported data.
- [ ] Keep sockets and middleware outside model equations. I/O tasks exchange
  typed commands/observations with the engine; they never mutate entities.
  Apply accepted commands at defined phase boundaries with bounded queues,
  explicit units/ENU/NED/body transforms, and per-entity identity/state.
- [ ] Define timeout, stale/duplicate input, disconnect, and shutdown behavior.
  External waits must be bounded and cancellable. Fail clearly on a missing
  required peer; automatic reconnect/resume is not required for the first pilot
  and must never silently replay stale commands. Publish only locally or on the
  intended private container network, not an unrestricted public bridge.
- [ ] Distinguish asynchronous ROS live control from synchronized ArduPilot
  physics stepping. ROS pub/sub alone does not provide lockstep or deterministic
  closed-loop execution. Record accepted inputs and their application ticks for
  replay. Do not let socket arrival order, worker completion, or the viewer
  choose physical timestep/order.
- [ ] Implement and test ArduPilot's
  [official simulator protocol](https://github.com/ArduPilot/ardupilot/blob/master/libraries/SITL/examples/JSON/readme.md):
  decode binary actuator packets explicitly, validate lengths/magic/counters,
  and return timestamped JSON physics state to the sender. Cover initial
  exchange, duplicates/loss/reset, rates/substeps, multi-vehicle endpoints,
  missing responses, and cancellation. Choose explicit fail/reset behavior;
  never advance physics twice for a duplicate actuator frame. Avoid native-layout
  packet assumptions, shared per-vehicle counters, and indefinitely blocking
  receives. This is the SITL physics link, not its separate MAVLink GCS link.

### Flight dynamics for ArduPilot

The C++ tree contains useful starting points, but “6DOF” is not by itself proof
of an adequate flight model:

- [FixedWing6DOF](../../scrimmage/src/plugins/motion/FixedWing6DOF/FixedWing6DOF.cpp)
  has aerodynamic forces/moments, inertia, gravity, and throttle/elevator/aileron/
  rudder inputs. Evaluate it as the fixed-wing starting point.
- [Multirotor](../../scrimmage/src/plugins/motion/Multirotor/Multirotor.cpp) models
  rotor-driven dynamics and is selected by the existing
  [arducopter mission](../../scrimmage/missions/arducopter.xml).
- [RigidBody6DOF](../../scrimmage/src/plugins/motion/RigidBody6DOF/RigidBody6DOF.cpp)
  disables gravity, zeros some translational derivatives, and has suspicious
  rate assignments. Do not select or blindly port it merely because of its name.
- The upstream [arduplane mission](../../scrimmage/missions/arduplane.xml)
  defaults to `JSBSimControl`, not `FixedWing6DOF`; that mission is not evidence
  that FixedWing6DOF already works with ArduPilot.

- [ ] Select and port/validate the appropriate aircraft dynamics for the first
  ArduPilot case. Rust ships SimpleAircraft and SingleIntegrator; neither is an aerodynamic
  actuator-driven replacement. Keep its existing
  C++ parity contract separate instead of silently changing it.
- [ ] Connect actuator channels to physical control inputs with explicit scales,
  limits, units, and signs. Provide position/velocity, attitude, body angular
  rates, and IMU specific force at the required physics sampling times. Audit
  existing C++ acceleration/frame conversions; a field named acceleration is
  not automatically the correct accelerometer measurement. Test gravity handling
  and avoid applying sensor noise twice across Rust and SITL.
- [ ] Implement and test geographic initialization/conversions needed by the
  selected flight case: origin, altitude datum, and local/world/body frames.
  This is integration correctness work, not a required map/viewer feature.
- [ ] Validate equations/transforms independently, then demonstrate one short
  closed-loop SITL case before claiming integration support. Fit synchronized
  stepping into explicit engine phase boundaries without changing ordinary
  mission scheduling or serializing every entity's full pipeline. Keep physics
  independent of the socket adapter so offline tests require neither ArduPilot
  nor ROS. JSBSim fixtures below can provide additional independent checks;
  restoring JSBSim as a runtime dependency is not a prerequisite.

Local integration evidence:
[ROSAutonomy](../../scrimmage/src/plugins/autonomy/ROSAutonomy/ROSAutonomy.cpp)
uses native ROS 1 in-process;
[ArduPilot](../../scrimmage/src/plugins/autonomy/ArduPilot/ArduPilot.cpp) already
uses UDP, but its legacy native binary physics packet is not the modern JSON
interface selected here. Preserve useful responsibilities, not those old ABI or
middleware dependencies.

### JSBSim: offline flight-model verification only

Use JSBSim to generate reference outputs for a small, understandable Rust flight
model. Do not port C++'s `JSBSimModel`/`JSBSimControl` plugins, link JSBSim into
the simulator, introduce a CXX bridge, or support arbitrary JSBSim aircraft XML.
JSBSim's [standalone scripted execution and logging](https://jsbsim-team.github.io/jsbsim/)
provide the reference workflow without a production integration.

- [ ] Select one aircraft configuration, a limited operating envelope, and the
  Rust model to verify. Explicitly choose equivalent selected physics versus
  a simplified approximation. This is not a project to reimplement JSBSim's
  aircraft loader, engines, systems, autopilots, or complete aerodynamic catalog.
- [ ] Start with a few short cases: steady flight and small throttle, elevator,
  and aileron changes where applicable to the chosen model. Replay explicit
  inputs rather than comparing different autopilots. Leave stalls, ground
  contact, and elaborate systems outside the initial scope.
- [ ] Add a Python fixture-generation command under `reference/` that runs
  standalone JSBSim in a pinned reference environment and exports only the
  required state/control columns. Keep this separate from the existing C++
  SCRIMMAGE parity matrix. JSBSim is required only to regenerate/check these
  references, not to build/run the Rust simulator or ordinary Rust tests.
- [ ] Retain small reference fixtures with initial conditions, aircraft/model
  parameters, complete input schedule, timestep/output timing, and exact JSBSim
  version plus asset hashes. Preserve raw outputs and document conversions;
  regeneration must be explicit and reviewed, never silently bless new results.
- [ ] Align frames/units, initial state and any trim result, mass/inertia and
  relevant coefficients, atmosphere/wind/gravity assumptions, input meaning,
  and sampling times. Test the conversion code independently so a harness error
  cannot masquerade as a physics error. Use declared numerical tolerances, not
  an expectation of bitwise equality between different integrators.
- [ ] For equivalent physics, compare selected state trajectories. For a reduced
  approximation, specify response metrics such as speed change, climb rate, and
  turn rate over the supported envelope. Use additional held-out conditions so
  fitting a few reference traces is not mistaken for general verification.
- [ ] Add analytical equation/invariant tests alongside fixture comparisons.
  JSBSim is an independent software reference, not proof of real-aircraft fidelity.
  Keep the existing SimpleAircraft/C++ regression contract separate; its direct
  roll/pitch-rate inputs and simplified throttle acceleration are not an
  aerodynamic control-surface model. Do not alter it silently to fit JSBSim.

These fixtures and any new Rust flight model are not implemented yet. The first
aircraft and fidelity target remain to be selected. Production JSBSim loading,
FFI/thread ownership, and runtime lifecycle support are no longer backlog items.

## 4. Burn: one useful workload before a GPU framework

Burn is the preferred candidate for new tensor/learned-model workloads. Its
backend/autodiff facilities do not make the whole simulator GPU-friendly or
differentiable. Batched policy inference or a tensor-friendly sensor is a better
initial experiment than a GPU submission for every tiny PID/RK4 update. See
[Burn backends](https://burn.dev/docs/burn/) and
[autodiff](https://burn.dev/books/burn/building-blocks/autodiff.html).

- [ ] Pick a representative workload, establish a CPU baseline, and prototype
  optional Burn execution. Verify the selected backend on this Mac rather than
  assuming CUDA. Keep training outside the simulation unless explicitly needed.
- [ ] Benchmark full steps: packing, device startup, transfers, execution,
  synchronization, readback, memory, and cold/warm runs. Require an end-to-end
  benefit before adding a general GPU execution layer.
- [ ] If batching helps, define gather/execute/commit for a homogeneous group
  inside the appropriate phase. Maintain stable entity-to-batch mapping across
  spawn/removal and complete results before downstream phases consume them.
- [ ] Keep backend generics, tensors, device queues, and locks out of the simple
  plugin API and ordinary entity state. Encapsulate them in the chosen model or
  a small batch adapter; do not genericize the entire core over a backend.
- [ ] Record backend/version, hardware, precision, and seeds. Define tolerances
  separately from exact CPU worker equality. Do not promise CPU/GPU bitwise
  parity, silently convert all physics to f32, or silently fall back to CPU.

C++'s [GPU implementation](../../scrimmage/include/scrimmage/gpu/GPUMotionModelImplementation.h)
already groups homogeneous states and waits for device completion. GPU tests
exist; that does not prove present reliability or usage history. Preserve the
useful phase/batching principle if measured, not the old OpenCL machinery.

## 5. Curated acceptance matrix, not a mission collection

Every shipped built-in needs direct tests and meaningful composed coverage.
Merely naming a plugin in XML or producing an empty metric is not evidence.
Use unit/contract tests for small failures and missions for interactions.

| Capability | Retain/add | Required evidence |
| --- | --- | --- |
| Straight + PID + SimpleAircraft | `straight-no-gui.xml` plus focused turning/altitude fixtures. | State/summary reference comparison, limits, angles, supported PID modes. |
| Rates/substeps/scheduled spawning | Existing verification fixture plus a zero-variance companion. | Actual substeps, held commands, counts/times, phase order, terminal frames. |
| Spawn randomness | Keep the stochastic fixture even while it fails. | Fix Ubuntu engine/distribution mismatch without changing seed/tolerances. |
| NoisyState / NoisyPosition | `noisy-state.xml`, `verification/noisy-state-bias.xml`, and direct sensor tests. | Owned belief, bias/noise, body-axis rotations, fixed seeds, next-tick visibility, held estimates. NoisyState deterministic C++ feedback passes; stochastic streams intentionally differ. |
| LocalNetwork / GlobalNetwork | Actual publishers/subscribers on multiple entities. | Same-parent versus global delivery, independent queues, ordering, supported delay/loss and rejection cases. |
| SimpleCollision / metrics | Same/opposing teams, thresholds, initial overlap, terminal removal. | Events, health/removal timing, survivors, per-team weights/scores. |
| Parser/templates | Small valid/invalid fixtures. | Availability/category, inactive templates, precedence, missing references, includes/cycles, contextual errors. |
| Lifecycle/scheduler | Retain public plugin contract tests; extend failures. | All seven extensions, errors/panics, joins, partial startup cleanup, stop timing, close once. |
| Scale | Parameterized populations/churn and mixed rates. | 1/2/8+ workers, bounded memory/queues, throughput, failure cleanup. |
| Rerun | Representative multi-agent spawn/removal cases. | Output independence and separate visible dashboard/playback QA. |
| Adapters/Burn | Opt-in environment-specific fixtures. | Protocol/model correctness, timeout/reset, stated repeatability/performance. |
| Rust flight model / JSBSim reference | Small saved offline fixtures plus analytical tests. | Declared trajectory or response-metric agreement for the chosen aircraft/envelope; no JSBSim runtime dependency. |

- [ ] Publish a supported capability/fixture list mapping every built-in to
  coverage. Audit dependencies before any later mission deletion; presence in
  `missions/` must not imply support.
- [ ] Keep `straight_cpu_mul.xml` as an upstream-quirk fixture or retire it
  explicitly. Its `motion_multipler` typo is not genuine substep coverage; the
  separate verification fixture provides that coverage.
- [ ] Fix the documented Ubuntu/libstdc++ spawn RNG gap and rerun Docker checks.
  The current six-case matrix has five passes and one stochastic failure, not
  an all-passing baseline. Preserve stream ownership/draw order and the existing reports.
- [ ] Require byte-identical frames/events/summaries across CPU workers and
  recorder on/off for deterministic supported cases. GPU numerics and uncontrolled
  live timing are different contracts, not excuses to weaken CPU checks.
- [ ] Compare semantic lifecycle events where selected capabilities require it.
  Investigate a minimal reference observer/output hook; do not infer events from
  frames alone or reimplement rejected spawn protobufs to obtain evidence.
  Direct C++ event-stream comparison is still absent.
- [ ] Expand [Python/Docker checks](../reference/README.md) to the curated surface.
  The checker currently requires expanded XML; parser include tests can remain
  separate until reference include packaging is deliberately supported.

Reports must distinguish retained C++ parity, specified intentional changes,
framework invariants, and observed visual/scale/integration behavior. Excluded
features are not unfinished parity work; untested features are not verified.

## 6. Keep the current viewer; finish the basics

The user confirms the current Rerun view, looping replay, and bottom timeline
with pause/scrubbing are sufficient. Keep them. They are not missing features
to rebuild. Earlier MCP automation difficulties are not evidence that the user
needs new playback controls.

No C++ camera-system port, custom camera choreography, extra live pause/step or
time-warp controls, or general mesh/debug-rendering framework is required now.
Add a specific visualization only when a shipped model actually needs it.
Cesium terrain is the optional experiment in section 8, not a viewer replacement
requirement. Maintain the viewer/output regression checks in section 5.

Live ROS/ArduPilot pause/resume is intentionally deferred: bonus work for later,
not part of the initial integration. Keep the existing recording playback
controls; do not add coordinated simulation/peer pausing.

### Packaging work

- [ ] Make installed runs find missions and assets without requiring the source
  checkout; plugin defaults are already compiled in. Do this with the later application/core boundary work.
- [ ] Put the verified test/comparison subset in CI and clearly list known gaps.
  Incomplete coverage does not prevent a reviewable v1 commit.

### Order of work

1. Commit the XML-based baseline when requested, with working features and gaps
   documented. Do not add temporary compatibility code to make it look complete.
2. Typed `Mission` struct with the XML reader moved onto it (byte-identical).
3. YAML reader, paired fixtures, and `scrimmage compare`.
4. Ripple-style sweeps and the Slurm launcher.
5. YAML templates and one typed creation path before connection spawning.
   XML schedules, YAML templates, and external connections use that same path.
6. Verify one ROS or ArduPilot connection-to-entity case, then expand deliberately.
   Leave unsupported cases explicit rather than adding workarounds.

Current-model tests, validation, and the documented RNG/lifecycle gaps remain
the priorities. Extra viewer features are not prerequisites for any of this.

## 7. Spatial queries and routing performance — later upgrade

Not required for the initial commit. First establish the clean SCRIMMAGE-style
plugin design and useful model behavior; then demonstrate measured optimizations.
Keep performance machinery inside the engine, not in student-written plugins.

- [ ] Benchmark collision checks, spatial sensors, and message routing on
  spread-out and clustered missions at increasing entity counts. Keep the simple
  implementation as a correctness baseline; do not promise guaranteed O(n).
- [ ] Explore a small engine-owned, read-only spatial lookup shared by collision,
  range-limited sensors/networks, and neighbor-based autonomy. Prove it with
  collision and one other consumer before expanding the API. Compare a grid
  with a tree where collision and sensor/radio ranges differ substantially.
- [ ] Refresh the lookup for the world snapshot consumers actually read: before
  autonomy, after motion, and after position-changing interactions as needed.
  Share reads across workers without mutable plugin access to the index.
  Preserve deterministic candidate, collision-event, and sensor RNG ordering.
- [ ] Improve subscriber lookup separately: LocalNetwork uses topic + entity,
  GlobalNetwork uses topic, and range-limited networks also use spatial candidates.
  Avoid scanning every mailbox just to reject most receivers. Genuine all-to-all
  broadcasts still require all their recipient deliveries.
- [ ] Compare outputs and report timings against the baseline, including multiple
  worker counts. Document intentional timing/behavior fixes rather than hiding
  them inside an optimization.

C++ already shares an R-tree, but SimpleCollision's tick loop and ContactBlobCamera
still scan entities. SphereNetwork and Boids use the tree; LocalNetwork means
same-entity communication, not proximity. The normal C++ loop rebuilds the tree
before motion, so do not copy its stale post-motion lookup timing. Sensors that
only read their own state need no spatial index.

## 8. High-resolution terrain feasibility check — optional, for fun

The current Rerun setup works well and remains the default. This is an optional
experiment to see whether beautiful streamed 3D terrain can fit inside the same
application as our debug panels. It is not required for v1, port completion, or
any release; there is no commitment to replace the viewer or finish this work.
Nothing here has been prototyped or verified on this Mac yet.

### Idea and practical limits

- [CesiumJS](https://cesium.com/platform/cesiumjs/) can stream terrain and imagery
  without us maintaining large terrain datasets in the repository. Its animated
  entities can display recorded agents, trails, labels, and spawn/removal times.
  Terrain and imagery are separate layers; Google Photorealistic 3D Tiles are
  another content option, not automatically part of ordinary World Terrain.
  See [terrain](https://cesium.com/learn/cesiumjs-learn/cesiumjs-terrain/) and the
  [flight-tracker example](https://cesium.com/learn/cesiumjs-learn/cesiumjs-flight-tracker/).
- Rerun's geographic map is 2D, not a substitute for Cesium's 3D terrain. The
  interesting experiment is a main Cesium panel with Rerun debugging and a
  shared playback time in one window, not a requirement for separate windows.
- Native embedding is plausible but not a blueprint switch. Rerun supports a
  [custom Rust viewer application](https://docs.rerun.io/dev/howto/visualization/extend-ui/),
  and [Wry child webviews](https://docs.rs/wry/latest/wry/struct.WebViewBuilder.html#method.build_as_child)
  can host browser content inside a native window, including on macOS. These
  are building blocks, not a verified Cesium/Rerun integration. A child webview
  is a separate rendering surface; layout, clipping, focus, overlays, and
  screenshots may be awkward. Rerun's extension APIs are explicitly unstable,
  so a custom native viewer would add upgrade and platform maintenance.
- A fallback to evaluate only if useful is one web dashboard embedding both
  Cesium and Rerun. Rerun's
  [0.36.2 web API](https://ref.rerun.io/docs/js/0.36.2/web-viewer/classes/WebViewer.html)
  offers time/playback controls and events. This still needs synchronization
  and browser performance testing; it is not an approved frontend migration.
- CesiumJS is free/open-source; streamed content has separate access and usage
  terms. As checked on 2026-09-24, Community lists 15 GB/month streaming and
  10 GB storage for qualifying personal/non-commercial and evaluation use.
  Government projects and funded research generally require paid plans, with
  exploratory evaluation distinguished. Recheck
  [pricing/eligibility](https://cesium.com/platform/cesium-ion/pricing/) before
  use. Own/self-hosted terrain avoids ion dependence but brings back data
  preparation/hosting work. No terrain provider means a smooth ellipsoid,
  not mountainous terrain.
- Streaming avoids bundled datasets, not network downloads or runtime caches.
  Preserve attribution, use a user-provided restricted read token, and do not
  assume streamed tiles can be archived in recordings or redistributed offline.
  See [content guidance](https://cesium.com/learn/ion/content-usage-and-attribution-guide/).
  Keep mission recordings local; external tile requests still reveal the viewed
  geographic area to the provider.

### Small experiment, only when we feel like trying it

- [ ] Use an isolated viewer prototype with a saved short mission, one agent,
  and a validated geographic origin. Try a Cesium webview inside a custom native
  Rerun application; do not change core simulation or the default viewer path.
- [ ] Validate local ENU-to-Earth-fixed positions, body orientations, and altitude
  datum. Drive the displayed agent from recorded state and Rerun's selected
  playback time. Never clamp aircraft to terrain to conceal alignment errors.
- [ ] Check streamed terrain, resizing/docking, overlapping menus, mouse/keyboard
  focus, scrubbing, and looping on this Mac. Verify screenshots/MCP coverage
  explicitly: Rerun's capture may not include a separate native webview surface.
- [ ] If the basic embedding works cleanly, try multiple agents and spawn/removal,
  then measure memory/GPU usage and test missing token/network failure. Save a
  short feasibility report and screenshots under `runs/visual-qa/`.
- [ ] Decide whether it is worth retaining. If it needs invasive patches or
  brittle window tricks, stop or separately evaluate the web-dashboard option.
  Neither outcome blocks ordinary development; do not wire an experiment into
  production merely because it renders an attractive scene.

Visual terrain remains an output concern. Camera-dependent tile loading must
never determine collisions, ground height, sensors, or simulation timing.
Physical terrain, if a selected mission needs it, requires a separate reproducible
model and tests. Cesium is not a shortcut around that requirement, and this
experiment does not add physical terrain to the required scope.

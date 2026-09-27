# Truth, measurements, and belief

How information moves from the world to a decision. The
[Belief vs truth](../concepts/belief-vs-truth.md) concept page explains why
the two are kept apart; this page shows the wiring.

## Inside one entity today

```text
┌────────────────────────────────────────────────────────────────────────┐
│                               Entity                                   │
│                                                                        │
│  ┌──────────┐  commands   ┌────────────┐  motion   ┌─────────────┐     │
│  │ Autonomy │────────────▶│ Controller │──────────▶│ MotionModel │     │
│  └────▲─────┘  (ports)    └─────▲──────┘  inputs   └──────┬──────┘     │
│       │                         │                         │            │
│       │ belief                  │ belief                  ▼            │
│       │ (or truth if none)      │                  ┌─────────────┐     │
│  ┌────┴─────────────────────────┴──┐               │    truth    │     │
│  │ belief (optional)               │               └──────┬──────┘     │
│  └────▲────────────────────────────┘                      │            │
│       │ set_belief                                        │            │
│  ┌────┴─────┐◀──────────────── reads truth ───────────────┘            │
│  │  Sensor  │───▶ readings, messages (e.g. NoisyState on LocalNetwork) │
│  └──────────┘                                                          │
└────────────────────────────────────────────────────────────────────────┘
```

- The motion model is the only thing that advances truth; interactions can
  change it (collisions, boundaries).
- Autonomy and controllers read `context.state`. That is the belief if a
  sensor set one, and truth otherwise.
- A sensor may call `set_belief`. If several do, the last in mission order
  wins.
- Decision contexts also carry `contacts_truth`, the true state of every other
  entity.

## Inside one entity with the planned estimators

```text
  truth (advanced by the motion model, changed by interactions)
    │
    ├──────────────────────────────────┐
    ▼                                  ▼
┌──────────┐                     ┌──────────┐
│  "gps"   │  navigation         │ "radar"  │  contact
│  sensor  │  sensor             │  sensor  │  sensor
└────┬─────┘                     └────┬─────┘
     │ position fix                   │ range and bearing
     ▼                                ▼
┌───────────────┐   own pose    ┌───────────────┐
│ Localization  │──────────────▶│   Tracking    │
│  where am I?  │               │ what is near? │
└───────┬───────┘               └───────┬───────┘
  ▲     │ own state                     │ known contacts
  │     ▼                               ▼
  │   ┌─────────────────────────────────────────┐
  │   │   belief: own state + known contacts    │
  │   └────────────────────┬────────────────────┘
  │                        │ belief only
  │                        ▼
  │                     Autonomy ──▶ Controller(s) ──▶ MotionModel ──▶ truth
  │                                        │
  └────────────────────────────────────────┘
        commands the entity issued
```

Belief has two parts, each with one owner:

- **Localization** answers "where am I?". It takes navigation readings (GPS,
  IMU) and the commands the entity issued, predicts with its own model, and
  corrects with the readings.
- **Tracking** answers "what is around me, and where?". It takes contact
  readings (radar, camera) and the pose localization just produced, because a
  relative reading such as range and bearing only becomes a world position
  once you know where you are. At first it accepts only readings taken this
  tick; an older reading would need the pose from when it was taken.

They run one after the other in the estimation step, localization first. Each
can be swapped on its own: a tracking experiment keeps the localizer fixed, and
the reverse. Messages from teammates (a shared position fix, a shared contact)
are inputs to whichever one they inform.

The ownership rule:

| Responsibility | Owner |
| --- | --- |
| Advance the physical world | Motion models and interactions |
| Produce readings and apply sensor error | Sensors |
| Maintain the entity's own state | One localization estimator per entity |
| Maintain the contacts it knows about | One tracking estimator per entity |
| Choose actions | Autonomy and controllers, from belief only |
| Compare belief with truth | Evaluation and output code |

What changes:

- Sensors only measure; `set_belief` and `clear_belief` go away.
- Autonomy and controllers see belief only, including during initialization,
  and lose `contacts_truth`. Belief holds the entity's own state and the
  contacts it has measured, with their sample times.
- There is no fallback to truth and no privileged "truth" estimator. Simple
  missions get perfect readings through pass-through localization and
  tracking. These readings are taken after interactions, so belief matches
  truth at the next decision for every entity present then. Ordinary trackers
  get no such help: a contact that disappears stays in belief until the
  tracker's own logic drops it.
- Estimators never receive truth. They get the readings they name, messages,
  the commands the entity issued (localization), the entity's estimated pose
  (tracking), and time.

## Three sources of error, each set once

```text
         command ──▶ MotionModel ──▶ truth ──▶ Sensor ──▶ reading
                        │                        │
              physical response            measurement error
              (what really happens)        (what the sensor adds)

         command ──▶ Estimator's own model ──▶ prediction ──▶ belief
                        │                                  ▲
                  model mismatch                           │
              (what the robot assumes)           corrected by readings
```

- **Physical response** is configured on the motion model.
- **Measurement error** is configured on the sensor.
- **Model mismatch** is configured on the estimator.

They are independent experimental inputs. A wrong belief can still change the
real trajectory, because the controller acts on it; that is the effect an
estimation experiment measures, not a second disturbance injected into physics.

## Between entities

```text
 Entity A                                                   Entity B
 ┌──────────┐   publish     ┌─────────┐   deliver    ┌──────────────┐
 │ plugin   │──────────────▶│ Network │────────────▶│ plugin inbox │
 │ outbox   │  (topic)      │ decides │  (per        │ (its own     │
 └──────────┘               │ who and │  subscriber) │  queue)      │
                            │  when   │              └──────────────┘
 snapshot of every entity ─▶│         │
                            └─────────┘
```

- Sensors read other entities through a snapshot, never their live state.
- Messages go through networks, which decide who receives each one and when
  (see [Messages](messages.md)).
- Collisions and other effects between entities are interactions, which run on
  the coordinator with access to every entity.

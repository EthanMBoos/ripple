# Disturbances

A disturbance is anything that makes the world less clean than a benchmark's:
noisy sensing, lost or late messages, wind, a vehicle that can't turn on the
spot. Each one is a setting of the plugin where it physically happens, and
each enters the tick at one phase:

| Disturbance | Enters at | Set on |
| --- | --- | --- |
| Vehicle dynamics | motion | the motion model you choose |
| Wind | motion | `wind_mps` on FixedWing6DOF |
| Sensing noise | sensors | NoisyState, NoisyContacts |
| Message loss | networks | SphereNetwork's `transmit_probability` and `range_m` |
| Latency | networks | `delay_s` on GlobalNetwork and SphereNetwork |
| Model mismatch | estimation, control | the estimator's or controller's own model |

Every effect is off by default, so a mission without these settings is the
clean case, and adding an effect never changes a result that didn't ask for
it. A decision only feels a disturbance through what the agent measured or
was told, because autonomy reads belief, never truth (see [Belief vs
truth](../concepts/belief-vs-truth.md)).

## Vehicle dynamics

The motion model is the vehicle. From simplest to most realistic:
SingleIntegrator and MpeParticle are points that move as commanded;
Multirotor is a rigid body on rotors, with optional motor lag and rotor drag;
SimpleAircraft turns and climbs within an aircraft's limits; FixedWing6DOF is
a full aerodynamic aircraft. The velocity controllers (AircraftVelocity,
MultirotorVelocity) take the same velocity command a point agent does, so one
autonomy can fly every rung. See [Built-in models](../reference/models.md).

## Wind

`wind_mps` is the air's velocity over the ground, `[east, north, up]`: a
north wind, blowing toward the south, is `[0, -6, 0]`. Only FixedWing6DOF
models it so far; flight in a steady wind is flight in still air carried by
the wind.

## Sensing noise

NoisyState measures the agent's own state, and NoisyContacts every other
agent. Each takes a bias (the error's mean) and a standard deviation per axis,
all zero by default:

```yaml
sensors:
  NoisyState:
    position_stddev_m: [2, 2, 0]
```

A GPS spec usually gives a horizontal 95% radius. For noise of standard
deviation σ on each horizontal axis, 95% of fixes fall within about 2.45 σ, so
divide the spec's radius by 2.45 to get `position_stddev_m`. Each error is a
keyed random number, so two methods run on one seed see the same noise on the
same measurements ([Randomness and seeds](../concepts/randomness.md)).

## Message loss and latency

SphereNetwork delivers only between agents closer than `range_m`, and each
delivery arrives with probability `transmit_probability`. `delay_s`, on
SphereNetwork or GlobalNetwork, holds every message that long; it arrives at
the first tick at or after its due time, and the receiver sees both
`sent_at_s` and `delivered_at_s`.

## Model mismatch

An estimator that predicts, or a controller that carries the vehicle's
numbers, has its own model of the vehicle, and that model can be set apart
from the truth. That makes three separate sources of error, each set in one
place:

```text
 command ──▶ motion model ──▶ truth ──▶ sensor ──▶ reading
                 │                        │
          what really happens       what the sensor adds

 command ──▶ estimator's own model ──▶ prediction ──▶ belief
                 │                                      ▲
          what the robot assumes               corrected by readings
```

MultirotorVelocityController takes its own `mass_kg` and rotor constants, so
a controller tuned for one vehicle can fly another. `PassThrough` has no
model, so it adds no mismatch: belief is the reading.

## Sweeping a disturbance

A sweep varies a setting from none to severe. The path must exist in the base
mission, so the mission sets the field at its clean value:

```yaml
# In the mission:
sensors:
  NoisyState:
    position_stddev_m: [0, 0, 0]
```

```yaml
# In the sweep: both horizontal axes move together.
parameter_combinations: zip
parameters:
  entities.blue.sensors.NoisyState.position_stddev_m.0: [0, 1, 2, 5, 10]
  entities.blue.sensors.NoisyState.position_stddev_m.1: [0, 1, 2, 5, 10]
```

See [Sweeps and results](sweeps.md) for running it and reading the results.
How far each sweep should go, measured against real hardware, is part of the
paper's study.

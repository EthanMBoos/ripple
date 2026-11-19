# Demos

Each demo is one run from the missions in this repository, captured from
the Rerun viewer as a video, with the Rerun recording it was captured from
beside it: open that in `rerun` to scrub the run and look around the 3D
view. Both are made with one command, `python3 reference/demos.py`, and
regenerated when the simulator changes. How a demo is described, and how
to add one, is in [Editing this
book](development/documentation.md#the-demo-gallery).

## Terrain flights

Six aircraft fly waypoint routes through the hills south-west of McMillan
Airfield. The camera follows the one climbing out of the valley over the
ridge; two of the six are routed into the ridge on purpose, and
`TerrainCollision` removes them when they reach the ground. From
[missions/terrain-mcmillan.yaml](../../missions/terrain-mcmillan.yaml),
stepped at 0.02 s; see [Terrain](guides/terrain.md).

{{#include demos/terrain-flight/embed.html}}

## Belief between fixes

A point robot drives a 100 m square believing a Kalman filter over its
commanded velocity, with a position fix every 5 s; the camera follows it.
Its motors reach 8 m/s though it asks for 10, so the belief runs ahead of
the truth between fixes and the one-sigma ellipsoid grows; each fix, marked
in yellow, pulls it back.
A second robot with perfect localization drives the same route. From
[position-kalman.yaml](../../crates/starter/missions/position-kalman.yaml)
in `crates/starter`;
see [Uncertainty](concepts/uncertainty.md) and the [localization
tutorial](tutorial/noisy-localization.md).

{{#include demos/belief-fixes/embed.html}}

## A fleet on the incident day

Sixteen hubs 7 km apart fly blood to the clinics around them. At minute 60
a mass-casualty incident at two hubs, ringed on the map while it lasts,
sends trauma requests beyond what they can fly alone, and the fleet manager
lends them aircraft from hubs with spare; at minute 90 a second surge hits a
large hub that may already have lent. Eighty minutes at sixty times speed, from
[incident.yaml](../../crates/delivery/missions/incident.yaml) in
`crates/delivery`,
the scripted day of the fleet-manager study.

{{#include demos/fleet-incident/embed.html}}

## A team covers three landmarks

Three agents running one network trained on this mission spread out to
cover three landmarks, MPE's `simple_spread` on Ripple's point model, shown
at half speed. Nobody is told which landmark is theirs. From
[spread.yaml](../../crates/learning/missions/spread.yaml) in
`crates/learning`,
built with `--features learning`; see [Training a
team](guides/training-team.md).

{{#include demos/team-spread/embed.html}}

## Takeoff and landing under ArduPlane

A Rascal on its wheels at the end of the runway south of McMillan Airfield.
ArduPlane SITL boots in its container, arms, takes off in AUTO, flies a
left-hand circuit, and lands on the strip it left, over Ripple's
six-degree-of-freedom Rascal, landing gear, and terrain. From
[rascal-runway.yaml](../../crates/ardupilot/missions/rascal-runway.yaml) in
`crates/ardupilot`,
built with `--features ardupilot`; see [ArduPilot](guides/ardupilot.md).

{{#include demos/runway-ardupilot/embed.html}}

## Takeoff and landing under PX4

The same Rascal, runway, and circuit flown by PX4 SITL. From
[rascal-runway.yaml](../../crates/px4/missions/rascal-runway.yaml) in
`crates/px4`,
built with `--features px4`; see [PX4](guides/px4.md).

{{#include demos/runway-px4/embed.html}}

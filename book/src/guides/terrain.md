# Terrain

A YAML mission can name a terrain: a grid of real ground heights around the
mission origin. Plugins read it through `Terrain::height_at(east_m, north_m)`,
and the viewer draws it. `TerrainCollision` removes agents that reach it.

```yaml
terrain: terrain/mcmillan.terrain.yaml   # relative to the mission file

interactions:
  TerrainCollision:
    clearance_m: 5
```

`missions/terrain-mcmillan.yaml` flies aircraft on waypoint routes through the
hills around McMillan Airfield (Camp Roberts, California), the site of C++
SCRIMMAGE's `mcmillan` terrain.

## Heights and the local frame

Local `z = 0` is the terrain's origin altitude above sea level, like C++
SCRIMMAGE's `altitude_origin` (300 m for McMillan). So an agent at `z = 100`
flies 400 m above sea level, and the ground there is `height_at` metres above or
below `z = 0`. Place agents above the ground: a spawn position below it is
redrawn within the group's position variance, and with no variance the run
stops with "unable to find collision-free spawn position".

"Sea level" is whatever vertical reference the elevation data uses: USGS
heights use NAVD88, Copernicus uses the EGM2008 geoid. Keep one source family
per terrain, and give `altitude_m` in the same reference.

`height_at` interpolates between the four surrounding samples. It returns
`None` off the grid or next to a missing sample.

Everything that touches the ground uses one rule, `ground_height` (and
`ground_normal` for its slope): the interpolated surface on the grid, and flat
ground at `z = 0`, the origin's altitude, off it.

## TerrainCollision

Each step checks the straight line from an agent's last position to its new
one, exactly against the interpolated ground. Only step endpoints are seen: a
path that curves within a step (through motion substeps, say) can clip terrain
the straight line misses, and more substeps do not change that. An agent
spawned mid-run has already moved one step when first checked, and only where
it ended that step is checked, so it could cross a thin ridge unnoticed on that
first step. Agents are points. A collision removes the agent and publishes the
same `GroundCollision` event as the flat `GroundCollision` plugin.

| Parameter | Default | Meaning |
| --- | --- | --- |
| `clearance_m` | 0 | Collide this far above the ground. |
| `enable_startup_collisions` | true | Treat spawn positions below the ground as collisions, so they are redrawn. |
| `team` | `all` | Check every agent, or only one team ID. |
| `structure_points_frd_m` | none | Points on an airframe, `[[forward, right, down], ...]` in metres from its centre, checked instead of the centre (see [Landing gear](#landing-gear)). |

Set `team` when a mission mixes aircraft with vehicles meant to drive on the
ground: those sit at ground height and would otherwise count as crashes. For
example, `team: 1` checks only the aircraft on team 1. `GroundCollision` has
the same option.

Off the grid, the ground is flat at `z = 0`. Inside it, TerrainCollision
refuses to start if any sample is missing, so a data hole never becomes a gap
in a hillside.

The viewer draws the terrain as triangles through the samples (skipping some on
large grids), while collisions use the smooth interpolated surface, so an agent
that looks like it touches the drawn ground may not quite, and the reverse.

FixedWing6DOF has its own flat ground at `z = 0` (`enable_ground_spring`, on by
default), which would push aircraft up in mid-air over valleys below `z = 0`.
It refuses to start in a terrain mission unless `enable_ground_spring` is false;
add TerrainCollision to remove aircraft that hit the terrain, or give it
landing gear.

Not modelled: trees and buildings (the McMillan data is bare ground), terrain
blocking sensors, and terrain beyond the grid.

## Landing gear

FixedWing6DOF can roll, take off, and land on wheels, with or without terrain
(without it the ground is flat at `z = 0`). Each wheel is a point on a strut
along the aircraft's down axis, following JSBSim's
[landing gear](https://jsbsim-team.github.io/jsbsim/classJSBSim_1_1FGLGear.html)
(`landing_gear.rs` has the details). The Rascal's wheels, from ArduPilot's
JSBSim Rascal (`Rascal.xml`):

```yaml
motion_model:
  FixedWing6DOF:
    airframe: rascal
    enable_ground_spring: false      # the wheels replace the flat spring
    landing_gear:
      # forward, right, down from the centre of gravity (m)
      - {position_frd_m: [0.084, -0.328, 0.434], spring_n_per_m: 7005, damping_n_s_per_m: 1459,
         static_friction: 0.8, rolling_friction: 0.1}
      - {position_frd_m: [0.084, 0.328, 0.434], spring_n_per_m: 7005, damping_n_s_per_m: 1459,
         static_friction: 0.8, rolling_friction: 0.1}
      - {position_frd_m: [-0.826, 0, 0.203], spring_n_per_m: 7005, damping_n_s_per_m: 1459,
         static_friction: 0.8, rolling_friction: 0.1, castered: true}
```

A wheel below the ground pushes along the ground's normal with its spring and
damper, and never pulls. Its friction is the force that stops it sliding
within one step, up to a limit: `rolling_friction` times its load along the
wheel (there are no brakes), and sideways Pacejka's Magic Formula of the slip
angle with JSBSim's default factors, peaking at `static_friction`. A
`castered` wheel swivels to its motion, so it gives no side force. Forces and
their moments are recomputed at every RK4 stage.

- **The step must be short.** The struts are stiff and heavily damped.
  FixedWing6DOF refuses a step where `dt × total damping / mass` is beyond
  RK4's stable range (for the Rascal, above about 0.0037 s), but that is only
  a basic damping check: it leaves out the springs, rotation, and wheel
  layout, so a step it accepts is not guaranteed stable. 0.0025 s is the
  tested Rascal setting; use `dt_s` or `motion_multiplier` to get there.
- **Simplified from JSBSim:** friction is solved wheel by wheel, each stopping
  its share of the mass, where JSBSim solves all wheels together; and forces
  act at the uncompressed wheel point.
- **Results are close to first order in the step,** not RK4's fourth, because
  contact switches on and off and JSBSim's rules are written per step. A
  drop's lowest point changes by under 2 mm from 0.0025 s to 0.0003 s.
- **No stall unless the mission adds the optional `lift_table` and
  `drag_table`** (JSBSim's Rascal values are in the ArduPilot chapter).
  Without them, lift at the high angles
  of a takeoff roll or a flare is too optimistic. Either way, an aircraft
  rolling backwards meets the air at 180°, outside any table's data, and the
  forces there are not meaningful.

To tell a touchdown from a crash, give TerrainCollision the airframe's
`structure_points_frd_m`. It then checks those points, moved with the
aircraft's attitude, instead of its centre: wheels holding the aircraft up are
not a collision, a nose or wingtip strike is. For the Rascal: the propeller
`[0.899, 0, 0.102]` (from JSBSim's file) and the wingtips `[0, ±1.35, 0]`
(half its span, at the centre's height). There is no tail point: the tail
wheel is at the tail.

The checks are in `crates/core/tests/landing_gear.rs`: resting height and
attitude, a drop, holding on a gentle slope and rolling down a steep one,
friction stopping a rolling aircraft, convergence as the step shrinks, the
step refusal, and a wingtip strike against a touchdown.

## Why it is built this way

Terrain here does three separate jobs, kept in separate places:

- **Geometry:** `Terrain`, one read-only height grid shared by everything that
  needs the ground. It answers "how high is the ground here?" and nothing else.
- **Collision:** `TerrainCollision`, an interaction plugin, decides when an
  agent has hit the ground and removes it.
- **Display:** the viewer draws the mesh and relief. It never affects the
  simulation.

### What other simulators do

- **C++ SCRIMMAGE** kept terrain out of the simulation. The mission's
  `<terrain>` was a mesh only the viewer drew, and ground contact meant a flat
  plane: FixedWing6DOF's spring at `z = 0`, or GroundCollision's height. Its
  random TerrainGenerator was used by no stock mission (see [reference
  notes](https://github.com/EthanMBoos/ripple/blob/main/docs/REFERENCE_NOTES.md#terrain-rust-only)).
- **[JSBSim](https://jsbsim-team.github.io/jsbsim/)** splits the ground into two
  parts. A [ground
  callback](https://jsbsim-team.github.io/jsbsim/classJSBSim_1_1FGGroundCallback.html)
  answers geometry queries: given a location, it returns the height above
  ground, the contact point, the surface normal, and the surface's velocity. Its
  [default](https://jsbsim-team.github.io/jsbsim/classJSBSim_1_1FGDefaultGroundCallback.html)
  is a round Earth at one terrain elevation; a host such as FlightGear supplies
  real terrain. Separately, [landing
  gear](https://jsbsim-team.github.io/jsbsim/classJSBSim_1_1FGLGear.html) turns
  contact into physics: each contact point has a spring and damper, static,
  sliding, and rolling friction, brakes and steering, and produces forces and
  moments on the aircraft. `STRUCTURE` contact points on the fuselage and wings
  push back on the airframe too, with their own friction; JSBSim does not remove
  the aircraft. Removing it when one touches is this simulator's simpler rule
  (TerrainCollision's `structure_points_frd_m`).
- **[MuJoCo](https://mujoco.readthedocs.io/en/stable/computation/index.html)**
  treats terrain as collision geometry (a [height
  field](https://mujoco.readthedocs.io/en/stable/XMLreference.html#asset-hfield))
  and contact as soft constraints, each with a normal and friction directions,
  solved every step. Its documentation stresses that the timestep and integrator
  decide whether contact stays stable.

Both separate "where is the ground?" from "what does touching it do?", and
both show that a believable ground response needs contact points, surface
normals, friction, moments, and a timestep chosen for contact.

### What we chose, and why

- **One shared ground, like JSBSim's callback.** Collision, the viewer, and
  later ground reactions all read the same `Terrain`, so they can never
  disagree about where the ground is. That is also why FixedWing6DOF refuses
  its old flat-ground spring when terrain is configured: two definitions of the
  ground would push aircraft up in mid-air over valleys below `z = 0`.
- **Crash by default.** The research this simulator serves is mostly
  airborne: swarms, routes, and sensing over terrain. For that, "remove the
  agent when it hits the ground" is the whole requirement, and it works for
  every motion model, because TerrainCollision only needs positions. It checks
  the path between steps exactly against the interpolated ground, so the answer
  does not depend on luck with sample spacing.
- **Not a spring moved to terrain height.** The tempting shortcut is to keep
  FixedWing6DOF's spring and put it at the terrain's height. It would look like
  ground contact, but it pushes straight up at one point: no wheels, no
  friction, no pitching or rolling moments, and wrong on slopes. The JSBSim
  comparison shows how much is missing, so contact is modelled with wheels
  or not at all.
- **Real contact as its own feature.** Motion models receive the terrain
  (`MotionContext::terrain`), and FixedWing6DOF's [landing gear](#landing-gear)
  reads height and slope there: contact points and their forces and moments,
  JSBSim-style; one shared rule for the ground off the grid
  (`Terrain::ground_height`); crash rules that tell a touchdown from a wingtip
  strike; and checks that the response is right. It breaks C++ parity for
  ground contact, which is fine: matching C++ is not evidence that its ground
  physics is correct, and the comparison missions keep the old spring.

## Making a terrain

`scripts/terrain/prepare.py` cuts a square around an origin out of elevation
data (and optionally aerial imagery), projects it so samples sit at whole
metres east and north of the origin, and writes the files a mission names. It
needs [rasterio](https://rasterio.readthedocs.io), which bundles GDAL
(`pip install rasterio`); the simulator does not.

```sh
python3 scripts/terrain/prepare.py mcmillan --origin 35.721025 -120.767925 \
  --altitude-m 300 --size-m 10000 --cell-m 10 \
  --elevation usgs-10m usgs-1m --imagery naip --imagery-cell-m 5 \
  --out missions/terrain
```

A source is a GeoTIFF path or URL, or a name that finds the files covering
the square:

| Name | Data | Coverage |
| --- | --- | --- |
| `usgs-10m` | USGS 3DEP 1/3 arc-second, bare ground | US |
| `usgs-1m` | USGS 3DEP lidar, bare ground | Much of the US |
| `copernicus-30m` | Copernicus GLO-30, includes trees and buildings | Worldwide |
| `naip` (imagery) | USDA NAIP aerial photos, about 0.6 m | US |

Later sources are drawn over earlier ones wherever they have data, so list a
complete coarse source first and finer, patchy ones after (`usgs-10m
usgs-1m`). Sources finer than the output are averaged down from their stored
overviews, so a 10 m grid reads the lidar's 8 m copy instead of every metre.
USGS's 10 m data is itself built from the lidar where it exists, so `usgs-1m`
only adds detail with a finer `--cell-m`.

What to trade:

- `--cell-m` changes both what agents collide with and what you see. The
  height file grows with its square: a 10 km square is 4 MB at 10 m, 16 MB at
  5 m, and 100 MB at 2 m. Finer than the source adds nothing.
- `--imagery-cell-m` changes only the picture. 5 m over 10 km is a
  2000-pixel texture.
- `--size-m` grows everything with its square. The viewer draws the relief
  and the aerial image at full size, so keep them within a few thousand pixels
  per side.

The header (`NAME.terrain.yaml`) records the sources, origin, corner, spacing,
and size; the heights (`NAME.f32`) are little-endian 32-bit floats, row by row
from north to south, in metres above sea level; the imagery (`NAME.png`)
covers the grid's cells. The script stops instead of writing a terrain with
missing samples, or when a search finds more files than one query returns. For
synthetic terrain in Rust, use
`Terrain::from_grid`. To add another named source, add a function to
`NAMED_SOURCES` in the script that returns file URLs for an area.

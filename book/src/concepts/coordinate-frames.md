# Coordinate frames

Most bugs in robotics code are sign and frame bugs: a heading measured from the
wrong axis, a pitch with the wrong sign, a vector in the body frame used as if
it were in the world frame. This page lists the conventions Ripple uses,
and the places where individual models differ from them.

## The world frame: East-North-Up

Positions and velocities are in a local **East-North-Up (ENU)** frame, in
meters:

| Axis | Points | Field examples |
| --- | --- | --- |
| x | East | `position_world_m.x`, `velocity_world_mps.x` |
| y | North | `position_world_m.y` |
| z | Up | `position_world_m.z` is altitude above the origin |

The frame is flat and local. There is no Earth curvature, latitude/longitude
conversion, or terrain.

## Heading: 0 means east

Heading (yaw) is measured **counterclockwise from east**, like the angle in a
math class, not clockwise from north like a compass or GPS:

| Heading | Direction |
| --- | --- |
| 0 | East (+x) |
| π/2 (90°) | North (+y) |
| π (180°) | West |
| 3π/2 (270°) | South |

To steer toward a point, use `atan2(dy, dx)`:

```rust,ignore
let offset_world_m = target_world_m - own_position_world_m;
let heading_world_rad = offset_world_m.y.atan2(offset_world_m.x);
```

> [!WARNING]
> Coming from GPS or aviation, where 0° is north and 90° is east, you must convert:
> `enu_heading = 90° - compass_heading`.

Mission XML uses **degrees** for `heading`, `roll`, and `pitch`. Rust code uses
**radians** everywhere.

## The body frame: forward-left-up

Each vehicle has its own body frame attached to it:

| Axis | Points |
| --- | --- |
| x | Forward (out of the nose) |
| y | Left (out of the left wing) |
| z | Up (out of the top) |

The attitude quaternion `orientation_world_from_body` rotates body vectors into
the world. For example, the nose direction in world coordinates is:

```rust,ignore
let nose_world = state.orientation_world_from_body.rotate_body_to_world(Vec3::x());
```

To go the other way, from world to body, use `rotate_world_to_body`.

## Roll and pitch signs

Roll, pitch, and yaw are standard rotations about the body's forward, left, and
up axes. Because the left axis (y) points left, **the pitch sign is the opposite
of what most pilots expect**:

| Angle | Positive means | Checked with `rotate_body_to_world` |
| --- | --- | --- |
| Yaw +90° | Nose points north | nose = (0, 1, 0) |
| Pitch +10° | **Nose points down** | nose = (0.985, 0, −0.174) |
| Roll +20° | Left wing up, right wing down | left wing = (0, 0.94, 0.34) |

So `<pitch>8</pitch>` in a mission starts the vehicle nose-down. To climb,
command a negative pitch.

## Units are in the names

Every physical field and variable carries its unit and frame in its name, so a
mismatch is visible when you read the code:

| Suffix | Unit |
| --- | --- |
| `_m` | meters |
| `_mps` | meters per second |
| `_mps2` | meters per second squared |
| `_rad`, `_deg` | radians, degrees |
| `_radps` | radians per second |
| `_s` | seconds |
| `_world_`, `_body_`, `_model_` | which frame the vector is in |

`angular_velocity_world_radps` is an angular velocity in the **world** frame. To
get body rates (roll rate, pitch rate, yaw rate as a pilot feels them), rotate
it:
`orientation_world_from_body.rotate_world_to_body(angular_velocity_world_radps)`.

## What each vehicle's state holds

Every entity's state is a `KinematicState`, always in the world frame:

```rust,ignore
pub struct KinematicState {
    pub position_world_m: Vec3,
    pub orientation_world_from_body: Quaternion,
    pub velocity_world_mps: Vec3,
    pub angular_velocity_world_radps: Vec3,
}
```

Motion models may use a different frame internally, but they must convert back
to this before writing the state.

## Models with their own conventions

Some motion models were ported from C++ with their internal conventions intact.
You only need these details when reading or modifying the model itself:

| Model | Internal convention |
| --- | --- |
| SimpleAircraft | Uses the state's roll, pitch, and yaw directly. Its `roll_rate` and `pitch_rate` inputs are the rates of those angles: positive roll rate banks right (and the bank turns it right, at g tan(roll) / speed), positive pitch rate lowers the nose. |
| FixedWing6DOF | Works internally in a **forward-right-down** body frame, the usual aerospace convention. It converts at the boundary by rotating 180° about the x-axis. |
| Multirotor | Uses the standard forward-left-up body frame; no conversion needed. |
| SingleIntegrator | Has no attitude dynamics. It points the vehicle along its velocity. |

## Quick reference

- **Position:** ENU meters; z is up.
- **Heading:** counterclockwise from east, in radians; `atan2(dy, dx)`.
- **Body axes:** forward, left, up.
- **Positive pitch:** nose **down**.
- **Positive roll:** right wing down.
- **Mission XML:** degrees. **Rust:** radians.
- **Velocities and angular velocities in the state:** world frame.

# SCRIMMAGE Architecture Overview

> Multi-agent robotics simulator from Georgia Tech Research Institute (GTRI)

## Core Purpose
SCRIMMAGE is designed for:
- Multi-agent task assignment
- Differential game theory research
- Novel controller development
- Reinforcement learning studies
- Large-scale robotics simulation

## High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        SimControl                               │
│                    (Main Simulation Loop)                       │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  ┌──────────┐    ┌────────────┐    ┌──────────────┐             │
│  │  Entity  │    │   Entity   │    │    Entity    │    ...      │
│  └────┬─────┘    └─────┬──────┘    └──────┬───────┘             │
│       │                │                  │                     │
│       ▼                ▼                  ▼                     │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │                  Plugin Stack (per entity)              │    │
│  │  ┌──────────┐ ┌────────────┐ ┌─────────────┐ ┌────────┐ │    │
│  │  │ Autonomy │→│ Controller │→│ MotionModel │ │ Sensor │ │    │
│  │  └──────────┘ └────────────┘ └─────────────┘ └────────┘ │    │
│  └─────────────────────────────────────────────────────────┘    │
│                                                                 │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │              Entity Interactions (Global)               │    │
│  │  Collision, Boundary, Capture, GroundCollision, etc.    │    │
│  └─────────────────────────────────────────────────────────┘    │
│                                                                 │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │                    PubSub Network                       │    │
│  │              (Inter-plugin communication)               │    │
│  └─────────────────────────────────────────────────────────┘    │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
                    ┌─────────────────┐
                    │  VTK Viewer GUI │
                    │   (Optional)    │
                    └─────────────────┘
```

## Key Classes

### SimControl (`include/scrimmage/simcontrol/SimControl.h`)
Main simulation controller. Handles:
- Mission file parsing
- Entity generation
- Time stepping
- Threaded or single-step execution

**Usage patterns:**
```cpp
SimControl simcontrol;
simcontrol.init("./missions/straight.xml");
simcontrol.run();  // OR run_threaded() / run_single_step()
simcontrol.shutdown();
```

### Entity (`include/scrimmage/entity/Entity.h`)
Represents a simulated agent. Each entity has:
- **ID**: team_id, id, sub_swarm_id
- **State**: Position, velocity, orientation (State class)
- **Plugin stack**: Autonomy, Controller, MotionModel, Sensors
- **Visual model**: For rendering

### State (`include/scrimmage/math/State.h`)
Represents entity kinematic state:
- `pos()` - Position (East/North/Up local frame)
- `vel()` - Linear velocity
- `ang_vel()` - Angular velocity
- `quat()` - Orientation quaternion (converts local↔body frame)

### EntityPlugin (`include/scrimmage/entity/EntityPlugin.h`)
Base class for all plugins attached to entities. Provides:
- Access to parent entity
- Pub/sub communication
- VariableIO for data flow
- Shape drawing for visualization

## Plugin Hierarchy

```
Plugin (base)
    └── EntityPlugin
            ├── Autonomy       - Decision making / AI
            ├── Controller     - Control law implementation  
            ├── MotionModel    - Physics / dynamics
            ├── Sensor         - Perception simulation
            └── Metrics        - Performance measurement

EntityInteraction (global) - Inter-entity effects (collisions, etc.)
Network                    - Communication topology
```

## Data Flow Per Timestep

1. **Sensors** update perception data
2. **Autonomy** plugins compute desired state
3. **Controllers** convert desired→actuator commands  
4. **MotionModel** integrates dynamics
5. **EntityInteractions** process collisions/events
6. **Metrics** collect statistics
7. **Viewer** renders (if enabled)

## Coordinate Systems
- **Local Level Frame**: East/North/Up (ENU)
- **Body Frame**: Nose/Left/Up
- **Geodetic**: Lat/Lon/Alt with configurable origin

## Key Directories
| Path | Purpose |
|------|---------|
| `src/simcontrol/` | Main simulation loop |
| `src/entity/` | Entity management |
| `src/plugins/` | All plugin implementations |
| `include/scrimmage/plugins/` | Plugin headers + XML configs |
| `missions/` | Mission XML files |
| `msgs/` | Protobuf message definitions |
| `src/proto/` | Core protobuf definitions |

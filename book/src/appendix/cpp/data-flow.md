# SCRIMMAGE Data Flow & Interactions

## Simulation Loop

Each timestep executes in order:

```
1. Generate entities (if scheduled)
2. For each entity:
   a. Run autonomy plugins → desired_state
   b. Run controllers → actuator commands
   c. Run motion model → new state
3. Update sensors (perceive updated world state)
4. Run entity interactions (collisions, captures, etc.)
5. Run network plugins
6. Run metrics collection
7. Update visualization
8. Advance time
```

## Plugin Data Flow

```
┌──────────────────────────────────────────────────────────────┐
│                         Entity                               │
├──────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌────────────┐    ┌─────────────┐    ┌─────────────┐        │
│  │  Autonomy  │───▶│  Controller │───▶│ MotionModel │        │
│  └────────────┘    └─────────────┘    └─────────────┘        │
│        │                                     │               │
│        │ desired_state                       │ new state     │
│        │ (heading, alt,                      ▼               │
│        │  speed, etc.)              ┌───────────────┐        │
│        │                            │    State      │        │
│        ▲                            │ (pos,vel,quat)│        │
│        │                            └───────────────┘        │
│        │                                     │               │
│  ┌─────────┐                                 │               │
│  │ Sensor  │◀────────────(perceives)─────────┘               │
│  └─────────┘                                                 │
│                                                              │
└──────────────────────────────────────────────────────────────┘
```

**Note:** Sensors run AFTER motion models. Autonomy uses the previous timestep's
sensor data. This means sensor observations reflect the world state at the END
of each timestep, available to autonomy at the START of the next timestep.

## VariableIO Connection

VariableIO creates shared memory between plugins:

```cpp
// Autonomy declares OUTPUT
int heading_out = vars_.declare(VariableIO::Type::desired_heading,
                                VariableIO::Direction::Out);

// Controller declares INPUT (same type)
int heading_in = vars_.declare(VariableIO::Type::desired_heading,
                               VariableIO::Direction::In);

// SimControl connects them automatically
// Now: vars_.output(heading_out, val) → vars_.input(heading_in)
```

### Standard Flow Variables

**Autonomy → Controller:**
- `desired_heading`
- `desired_altitude`
- `desired_speed`
- `desired_roll`, `desired_pitch`
- `desired_turn_rate`, `desired_pitch_rate`

**Controller → MotionModel:**
- `throttle`
- `elevator`, `aileron`, `rudder`
- `turn_rate`, `roll_rate`, `pitch_rate`
- `velocity_x`, `velocity_y`, `velocity_z`

## State vs Desired State

```cpp
// In Autonomy:
state_       // Current state (read-only, set by motion model)
desired_state_ // Target state (set by autonomy, read by controller)

// Example flow:
void Autonomy::step_autonomy(double t, double dt) {
    // Read current state
    double current_heading = state_->quat().yaw();
    
    // Compute desired heading (e.g., toward target)
    double target_heading = atan2(target.y() - state_->pos()(1),
                                  target.x() - state_->pos()(0));
    
    // Output through VariableIO
    vars_.output(heading_idx_, target_heading);
}
```

## Contact Map Structure

```cpp
// contacts_ is ContactMapPtr (shared_ptr<ContactMap>)
// ContactMap is std::unordered_map<int, Contact>

for (auto& [entity_id, contact] : *contacts_) {
    // Entity identification
    int id = contact.id().id();
    int team = contact.id().team_id();
    int sub_swarm = contact.id().sub_swarm_id();
    
    // State access
    StatePtr state = contact.state();  // pos, vel, quat
    
    // Type information
    std::string type = contact.type();
}
```

## Pub/Sub Architecture

### Network Types
- **GlobalNetwork**: All entities can communicate
- **LocalNetwork**: Range-limited communication

```xml
<network>GlobalNetwork</network>
<network range="100">LocalNetwork</network>
```

### Message Routing

```
Entity A                              Entity B
   │                                     │
   ▼                                     ▼
Publisher ──────▶ Network ──────▶ Subscriber
   │              Queue               │
   │                                  │
   └── advertise("topic") ◀─────▶ subscribe("topic")
```

### Message Timing
- Messages published in step N are received in step N+1 (or later)
- Callbacks execute at start of subscriber's step

## Entity Interaction Patterns

EntityInteractions run globally, processing all entities:

```cpp
class MyInteraction : public EntityInteraction {
    bool step_entity_interaction(
        std::list<EntityPtr>& ents,  // All entities
        double t, double dt) override {
        
        // Check all pairs
        for (auto it1 = ents.begin(); it1 != ents.end(); ++it1) {
            for (auto it2 = std::next(it1); it2 != ents.end(); ++it2) {
                if (collides(*it1, *it2)) {
                    (*it1)->collision();
                    (*it2)->collision();
                }
            }
        }
        return true;
    }
};
```

### Built-in Interactions
- **SimpleCollision**: Detect overlapping radii
- **GroundCollision**: Detect ground contact
- **BulletCollision**: Physics-based collision
- **Boundary**: Enforce region constraints
- **FlagCaptureInteraction**: Capture-the-flag logic

## Sensor Data Flow

```cpp
class MySensor : public Sensor {
    // Override step() to update sensor state each timestep
    bool step() override {
        // Access parent entity state
        auto truth = parent_->state_truth();
        
        // Add noise, simulate detection, etc.
        cached_measurement_ = corrupt(truth);
        
        return true;
    }
    
    // Return sensor message on request
    MessageBasePtr sensor_msg(double t) override {
        auto msg = std::make_shared<Message<SomeType>>();
        msg->data = cached_measurement_;
        return msg;
    }
};
```

### Sensor → Autonomy Communication

```cpp
// In Autonomy::init()
auto sens = parent_->sensor("MySensor");  // Get sensor by name

// In step_autonomy()
auto msg = sens->sensor_msg(t);  // Get sensor message
auto obs = std::dynamic_pointer_cast<Message<MyObservationType>>(msg);
```

## Parameter Server

Runtime parameter modification:

```cpp
// Register parameter with callback
register_param<double>("gain", initial_value, [&](double new_val) {
    std::cout << "Gain changed to: " << new_val << std::endl;
    gain_ = new_val;
});

// Modify from another plugin
auto ps = parent_->param_server();
ps->set_param("gain", 2.0);
```

## Services (RPC Pattern)

Request/response communication:

```cpp
// Provider (in init) - register via parent entity's services map
auto handler = [&](MessageBasePtr req, MessageBasePtr& res) -> bool {
    auto typed_req = std::dynamic_pointer_cast<Message<ReqType>>(req);
    auto typed_res = std::make_shared<Message<ResType>>();
    // Process request...
    res = typed_res;
    return true;
};
parent_->services()["my_service"] = handler;

// Consumer - call via parent entity
MessageBasePtr res;
if (parent_->call_service(req, res, "my_service")) {
    auto result = std::dynamic_pointer_cast<Message<ResType>>(res);
    // Use result
}
```

## Time Management

```cpp
// Access simulation time
double t = time_->t();      // Current time
double dt = time_->dt();    // Timestep

// In step() methods
bool step_autonomy(double t, double dt) {
    // t = current sim time
    // dt = time since last step
}
```

## Truth vs Belief States

```cpp
// Ground truth (exact state)
StatePtr truth = parent_->state_truth();

// Believed state (after sensor noise)
StatePtr belief = parent_->state_belief();

// In autonomy, state_ is typically belief
// Motion model updates truth, sensors corrupt to belief
```

# SCRIMMAGE Plugin Development Guide

## Plugin Types

| Type | Purpose | Base Class | Key Method |
|------|---------|------------|------------|
| **Autonomy** | AI/decision-making | `Autonomy` | `step_autonomy(t, dt)` |
| **Controller** | Control laws | `Controller` | `step(t, dt)` |
| **MotionModel** | Physics/dynamics | `MotionModel` | `step(time, dt)` |
| **Sensor** | Perception | `Sensor` | (varies) |
| **Interaction** | Global effects | `EntityInteraction` | `step_entity_interaction(contacts, t, dt)` |
| **Metrics** | Statistics | `Metrics` | `step_metrics(t, dt)` |
| **Network** | Communication | `Network` | (varies) |

## Creating a New Plugin

### File Structure
Each plugin requires 3 files:
```
# For autonomy plugin "MyPlugin":
include/scrimmage/plugins/autonomy/MyPlugin/
    ├── MyPlugin.h       # Header
    └── MyPlugin.xml     # Configuration defaults

src/plugins/autonomy/MyPlugin/
    ├── CMakeLists.txt   # Build config
    └── MyPlugin.cpp     # Implementation
```

### 1. Header File (.h)
```cpp
#ifndef INCLUDE_SCRIMMAGE_PLUGINS_AUTONOMY_MYPLUGIN_MYPLUGIN_H_
#define INCLUDE_SCRIMMAGE_PLUGINS_AUTONOMY_MYPLUGIN_MYPLUGIN_H_

#include <scrimmage/autonomy/Autonomy.h>

namespace scrimmage { namespace autonomy {

class MyPlugin : public scrimmage::Autonomy {
 public:
    void init(std::map<std::string, std::string>& params) override;
    bool step_autonomy(double t, double dt) override;

 protected:
    double my_param_ = 0.0;
    int desired_heading_idx_ = 0;
    int desired_alt_idx_ = 0;
    int desired_speed_idx_ = 0;
};

}}
#endif
```

### 2. Implementation (.cpp)
```cpp
#include <scrimmage/plugins/autonomy/MyPlugin/MyPlugin.h>
#include <scrimmage/plugin_manager/RegisterPlugin.h>
#include <scrimmage/parse/ParseUtils.h>
#include <scrimmage/entity/Entity.h>
#include <scrimmage/math/State.h>

// CRITICAL: Register the plugin
REGISTER_PLUGIN(scrimmage::Autonomy, scrimmage::autonomy::MyPlugin, MyPlugin_plugin)

namespace scrimmage { namespace autonomy {

void MyPlugin::init(std::map<std::string, std::string>& params) {
    // Parse parameters from XML
    my_param_ = scrimmage::get<double>("my_param", params, 0.0);
    
    // Declare output variables (to controller)
    desired_heading_idx_ = vars_.declare(
        VariableIO::Type::desired_heading, VariableIO::Direction::Out);
    desired_alt_idx_ = vars_.declare(
        VariableIO::Type::desired_altitude, VariableIO::Direction::Out);
    desired_speed_idx_ = vars_.declare(
        VariableIO::Type::desired_speed, VariableIO::Direction::Out);
}

bool MyPlugin::step_autonomy(double t, double dt) {
    // Access own state
    Eigen::Vector3d my_pos = state_->pos();
    
    // Access other entities
    for (auto& [id, contact] : *contacts_) {
        if (contact.id().team_id() != parent_->id().team_id()) {
            // Enemy entity
            Eigen::Vector3d enemy_pos = contact.state()->pos();
        }
    }
    
    // Output desired state
    vars_.output(desired_heading_idx_, 0.0);  // heading in radians
    vars_.output(desired_alt_idx_, 200.0);    // altitude
    vars_.output(desired_speed_idx_, 20.0);   // speed
    
    return true;  // false to request simulation end
}

}}
```

### 3. XML Configuration (defaults)
```xml
<?xml version="1.0"?>
<params>
  <library>MyPlugin_plugin</library>
  <my_param>42.0</my_param>
</params>
```

### 4. CMakeLists.txt
```cmake
add_library(MyPlugin_plugin SHARED MyPlugin.cpp)
target_link_libraries(MyPlugin_plugin scrimmage-core)
```

## VariableIO System

Connects plugins by named variables:

```
Autonomy ──[desired_heading]──→ Controller ──[throttle]──→ MotionModel
         ──[desired_altitude]──→           ──[aileron]──→
         ──[desired_speed]────→            ──[elevator]─→
```

### Common Variable Types
```cpp
VariableIO::Type::desired_altitude
VariableIO::Type::desired_speed
VariableIO::Type::desired_heading
VariableIO::Type::desired_roll
VariableIO::Type::desired_pitch
VariableIO::Type::throttle
VariableIO::Type::elevator
VariableIO::Type::aileron
VariableIO::Type::rudder
VariableIO::Type::velocity_x  // etc.
```

### Declaring Variables
```cpp
// Output (producer)
int idx = vars_.declare(VariableIO::Type::desired_heading, 
                        VariableIO::Direction::Out);
vars_.output(idx, heading_value);

// Input (consumer)
int idx = vars_.declare(VariableIO::Type::desired_heading,
                        VariableIO::Direction::In);
double heading = vars_.input(idx);
```

## Pub/Sub Communication

### Publishing
```cpp
// In init()
auto pub = advertise("GlobalNetwork", "my_topic");

// In step()
auto msg = std::make_shared<sc::Message<MyProtoType>>();
msg->data.set_value(42);
pub->publish(msg);
```

### Subscribing
```cpp
// In init()
auto callback = [&](auto& msg) {
    process(msg->data);
};
subscribe<MyProtoType>("GlobalNetwork", "my_topic", callback);
```

## Accessing Entity Data

```cpp
// Own state
state_->pos();     // Eigen::Vector3d position
state_->vel();     // velocity
state_->quat();    // orientation quaternion

// Parent entity
parent_->id().id();        // entity ID
parent_->id().team_id();   // team ID
parent_->health_points();  // remaining health

// Other contacts
for (auto& [id, contact] : *contacts_) {
    contact.state()->pos();
    contact.id().team_id();
}
```

## Generating Shapes for Visualization
```cpp
#include <scrimmage/common/Shape.h>

// Draw a sphere at current position
auto shape = sc::shape::make_sphere(state_->pos(), 5.0, 
    Eigen::Vector3d(1, 0, 0));  // red
shapes().push_back(shape);

// Draw text label
auto text = sc::shape::make_text("Status OK", state_->pos(),
    Eigen::Vector3d(0, 1, 0));  // green
shapes().push_back(text);
```

## Plugin Generation Script
```bash
./scripts/generate-plugin.sh autonomy MyPlugin project_name
```
Creates skeleton from templates in `scripts/templates/`.

## Built-in Plugin Examples

### Autonomy
- `Straight` - Fly straight ahead
- `Boids` - Flocking behavior
- `WaypointGenerator` - Follow waypoints
- `AuctionAssign` - Task assignment

### Controllers
- `SimpleAircraftControllerPID` - Basic aircraft PID
- `UnicyclePID` - Ground vehicle control
- `MultirotorControllerPID` - Quadrotor control

### Motion Models
- `SimpleAircraft` - 3DOF aircraft
- `FixedWing6DOF` - Full 6DOF fixed-wing
- `Unicycle` - 2D wheeled robot
- `JSBSimModel` - JSBSim integration

### Sensors
- `NoisyState` - Add noise to state
- `ContactBlobCamera` - Simulated camera
- `GPS` - GPS sensor

# SCRIMMAGE Mission Configuration

Missions are defined in XML files (e.g., `missions/straight.xml`).

## Basic Structure

```xml
<?xml version="1.0"?>
<runscript name="MissionName">
    
    <!-- Simulation settings -->
    <run start="0.0" end="100" dt="0.1"
         time_warp="10"
         enable_gui="true"
         start_paused="true"/>
    
    <!-- Global plugins -->
    <metrics>SimpleCollisionMetrics</metrics>
    <network>GlobalNetwork</network>
    <entity_interaction>SimpleCollision</entity_interaction>
    
    <!-- Entity definitions -->
    <entity>
        <!-- Entity configuration -->
    </entity>
    
</runscript>
```

## Simulation Parameters

```xml
<run 
    start="0.0"           <!-- Start time (seconds) -->
    end="100"             <!-- End time -->
    dt="0.1"              <!-- Timestep -->
    time_warp="10"        <!-- Speed multiplier for GUI -->
    enable_gui="true"     <!-- Show VTK visualization -->
    network_gui="false"   <!-- Remote GUI over network -->
    start_paused="true"   <!-- Start paused -->
    full_screen="false"
    window_width="800"
    window_height="600"
/>

<multi_threaded num_threads="8">false</multi_threaded>
<end_condition>time, all_dead</end_condition>  <!-- time|one_team|none|all_dead -->
<seed>12345</seed>  <!-- Random seed for reproducibility -->
```

## World Configuration

```xml
<!-- Terrain rendering -->
<terrain>mcmillan</terrain>
<background_color>191 191 191</background_color>

<!-- Grid overlay -->
<grid_spacing>10</grid_spacing>
<grid_size>1000</grid_size>

<!-- Geographic origin (for GPS/geodetic) -->
<latitude_origin>35.721025</latitude_origin>
<longitude_origin>-120.767925</longitude_origin>
<altitude_origin>300</altitude_origin>
<show_origin>true</show_origin>

<!-- Logging -->
<log_dir>~/.scrimmage/logs</log_dir>
<create_latest_dir>true</create_latest_dir>
<output_type>all</output_type>
```

## Entity Definition

```xml
<entity>
    <!-- Identity -->
    <name>uav_entity</name>
    <team_id>1</team_id>
    <color>77 77 255</color>  <!-- RGB -->
    <count>10</count>         <!-- Number to spawn -->
    <health>1</health>
    <radius>1</radius>        <!-- Collision radius -->
    
    <!-- Initial position (local ENU coordinates) -->
    <x>-900</x>
    <y>0</y>
    <z>195</z>
    <heading>0</heading>      <!-- degrees -->
    <roll>0</roll>
    <pitch>0</pitch>
    
    <!-- Position variance for multiple entities -->
    <variance_x>20</variance_x>
    <variance_y>20</variance_y>
    <variance_z>10</variance_z>
    
    <!-- Plugin stack -->
    <autonomy speed="21">Straight</autonomy>
    <controller>SimpleAircraftControllerPID</controller>
    <motion_model>SimpleAircraft</motion_model>
    <visual_model>zephyr-blue</visual_model>
    
    <!-- Optional sensors -->
    <sensor>NoisyState</sensor>
    <sensor rpy="0,0,0" camera_id="0">ContactBlobCamera</sensor>
    
    <!-- Base location (for return-to-base behaviors) -->
    <base>
        <latitude>35.721112</latitude>
        <longitude>-120.770305</longitude>
        <altitude>300</altitude>
        <radius>25</radius>
    </base>
</entity>
```

## Plugin Parameters

Override plugin defaults inline:

```xml
<!-- Autonomy with custom parameters -->
<autonomy 
    speed="21"
    show_text_label="true"
    enable_boundary_control="true">Straight</autonomy>

<!-- Controller with custom parameters -->
<controller 
    Kp="1.0" 
    Ki="0.1">SimpleAircraftControllerPID</controller>
```

## Entity Interactions (Global Plugins)

```xml
<!-- Boundary constraint (cuboid) -->
<entity_interaction type="cuboid"
    lengths="2000, 2000, 1000"
    center="0, 0, 500"
    rpy="0, 0, 0">Boundary</entity_interaction>

<!-- Collision detection -->
<entity_interaction>SimpleCollision</entity_interaction>
<entity_interaction>GroundCollision</entity_interaction>

<!-- Bullet physics collisions -->
<entity_interaction 
    enable_collision_detection="true"
    remove_on_collision="true"
    enable_team_collisions="true"
    enable_terrain="true">BulletCollision</entity_interaction>
```

## Network Configuration

```xml
<!-- Global broadcast network -->
<network>GlobalNetwork</network>

<!-- Local (range-limited) network -->
<network range="100">LocalNetwork</network>
```

## Metrics Plugins

```xml
<metrics>SimpleCollisionMetrics</metrics>
```

## Template Entities (Runtime Generation)

```xml
<!-- Entity template that can be spawned at runtime -->
<entity tag="gen_straight">
    <count>0</count>  <!-- 0 = template only, not spawned initially -->
    <team_id>1</team_id>
    <!-- ... rest of config ... -->
</entity>
```

Referenced by autonomy plugins using the `tag` attribute.

## Mission Includes

Split missions across files:

```xml
<!-- In main mission -->
<xi:include href="missions/straight_include/entities.xml" 
            xmlns:xi="http://www.w3.org/2001/XInclude"/>
```

## Variable Substitution

Use `${var=default}` syntax:

```xml
<count>${count=10}</count>
<x>${start_x=-900}</x>
```

Override at runtime:
```bash
scrimmage missions/straight.xml count:=50 start_x:=-500
```

## Common Mission Patterns

### Swarm vs Swarm
```xml
<!-- Team 1: Blue -->
<entity>
    <team_id>1</team_id>
    <color>0 0 255</color>
    <count>20</count>
    <autonomy>Predator</autonomy>
    <!-- ... -->
</entity>

<!-- Team 2: Red -->
<entity>
    <team_id>2</team_id>
    <color>255 0 0</color>
    <count>20</count>
    <autonomy>Straight</autonomy>
    <!-- ... -->
</entity>
```

### Capture the Flag
```xml
<entity_interaction>FlagCaptureInteraction</entity_interaction>
<!-- + entity definitions with TakeFlag autonomy -->
```

## Running Missions

```bash
# Basic run
scrimmage missions/straight.xml

# With parameter overrides
scrimmage missions/straight.xml count:=100 time_warp:=1

# Headless (no GUI)
scrimmage missions/straight-no-gui.xml
```

## Mission File Locations
- Built-in: `missions/`
- User missions: Add to `SCRIMMAGE_MISSION_PATH`

#!/usr/bin/env python3
"""Run ArduPlane SITL for one aircraft: start it, arm it in a mode, then keep
reading MAVLink until SITL exits or the container is stopped.

Runs inside the container (see Dockerfile); crates/ardupilot/src/sitl_launcher.rs sets
the environment. MAVLink stays inside the container: SITL sends to this
script on UDP 14550. Reading continuously matters above --speedup 1, where
SITL stalls when its MAVLink output queue fills.

Adapted from UnrealVehicleDynamics' ardupilot/session.py, but with ArduPilot's
plane.parm loaded, normal (not forced) arming, and no release handshake: the
simulator decides when the aircraft flies. If RELEASE_S is set and ArduPlane is
not armed in its mode 1 s before then (by ArduPilot's clock, which follows the
simulator's timestamps), the session exits with an error; that stops SITL, so
the run fails instead of releasing a disarmed aircraft.

If WAYPOINTS is set ("east,north,up; ..." in metres from where the aircraft
starts), it uploads them as an AUTO mission that loops back to the first
waypoint, then arms in AUTO. "Up" is from the start too: ArduPlane moves home
to where it arms, and waypoint altitudes are relative to home. TAKEOFF_ALT (m)
starts the route with a NAV_TAKEOFF climbing to that height; LAND=1 makes the
last waypoint a NAV_LAND, whose "up" is the ground's height there relative to
the start (0 on the same runway), and the route does not repeat.
With a takeoff it arms first, on the ground in its boot mode with the throttle
stick low, and then switches to AUTO, as a pilot would: in AUTO while
disarmed, ArduPlane runs through the mission and then refuses to arm "in
landing sequence".

The launcher writes the mission's own ArduPilot parameters to mission.parm in
the run folder; SITL loads it after plane.parm and rascal.parm.

If GOTO is set ("east,north,up", the same convention), it arms in GUIDED and
then sends that point as the target (ArduPlane only accepts one while already
in GUIDED); ArduPlane flies there and circles it.
"""

import math

import os
from pathlib import Path
import signal
import subprocess
import sys
import time

from pymavlink import mavutil

from command_clock import CommandClock
from release_check import ReleaseCheck

# GPS fix and EKF flags (attitude, horizontal and vertical velocity, absolute
# horizontal and vertical position) required before arming.
REQUIRED_GPS_FIX = 3
REQUIRED_EKF_FLAGS = 1 | 2 | 4 | 16 | 32
EARTH_RADIUS_M = 6378137.0

running = True


def stop(_signal, _frame):
    global running
    running = False


def start_sitl(run_dir):
    command = [
        "/opt/ardupilot/arduplane",
        "--model", f"JSON:{os.environ['RIPPLE_ADDRESS']}",
        "--sim-port-out", os.environ["SIM_PORT_OUT"],
        "--home", os.environ["HOME_LOCATION"],
        "--rate", os.environ["RATE_HZ"],
        "--speedup", os.environ.get("SPEEDUP", "1"),
        "--sysid", os.environ.get("SYSID", "1"),
        "--serial0", "udpclient:127.0.0.1:14550",
        # mission.parm: the mission's own parameters, written by the launcher.
        "--defaults", f"/opt/ardupilot/plane.parm,/opt/ardupilot/rascal.parm,{run_dir}/mission.parm",
    ]
    print("starting:", " ".join(command), flush=True)
    return subprocess.Popen(command, cwd=run_dir)


def request_message(connection, message_id, rate_hz):
    connection.mav.command_long_send(
        connection.target_system, connection.target_component,
        mavutil.mavlink.MAV_CMD_SET_MESSAGE_INTERVAL, 0,
        message_id, 1_000_000 / rate_hz, 0, 0, 0, 0, 0,
    )


def request_telemetry(connection):
    # Rates are in ArduPilot time, so they keep up with any speedup.
    for message_id in (
        mavutil.mavlink.MAVLINK_MSG_ID_GPS_RAW_INT,
        mavutil.mavlink.MAVLINK_MSG_ID_EKF_STATUS_REPORT,
        mavutil.mavlink.MAVLINK_MSG_ID_SYSTEM_TIME,
        mavutil.mavlink.MAVLINK_MSG_ID_GLOBAL_POSITION_INT,
    ):
        request_message(connection, message_id, 5)


def neutral_rc(connection, throttle_pwm):
    """Keep RC alive with centred sticks; ArduPlane's auto modes ignore them."""
    ignore = 65535
    connection.mav.rc_channels_override_send(
        connection.target_system, connection.target_component,
        1500, 1500, throttle_pwm, 1500, ignore, ignore, ignore, ignore,
    )


def parse_waypoints(text):
    """"east,north,up; ..." -> [(east_m, north_m, up_m), ...]"""
    return [tuple(float(value) for value in point.split(",")) for point in text.split(";") if point.strip()]


def offset_location(start_lat_deg, start_lon_deg, east_m, north_m):
    """Latitude and longitude east_m and north_m from a start point (flat Earth)."""
    lat_deg = start_lat_deg + math.degrees(north_m / EARTH_RADIUS_M)
    lon_deg = start_lon_deg + math.degrees(east_m / (EARTH_RADIUS_M * math.cos(math.radians(start_lat_deg))))
    return lat_deg, lon_deg


# TODO: change GUIDED targets during a run (for example from a SCRIMMAGE
# autonomy) when an experiment needs it; today one target is sent once armed.
def send_guided_target(connection, start_lat_deg, start_lon_deg, target):
    """Send one GUIDED target: a mission item with current = 2, which ArduPlane
    treats as "fly here now" rather than part of the mission."""
    east_m, north_m, up_m = target
    lat_deg, lon_deg = offset_location(start_lat_deg, start_lon_deg, east_m, north_m)
    connection.mav.mission_item_int_send(
        connection.target_system, connection.target_component, 0,
        mavutil.mavlink.MAV_FRAME_GLOBAL_RELATIVE_ALT_INT, mavutil.mavlink.MAV_CMD_NAV_WAYPOINT,
        2, 1, 0, 0, 0, 0, int(lat_deg * 1e7), int(lon_deg * 1e7), up_m,
    )


def upload_mission(connection, start_lat_deg, start_lon_deg, waypoints, takeoff_alt_m, land):
    """Upload the waypoints as an AUTO mission: optionally a takeoff first, and
    either a landing at the last waypoint or a jump back to the first one so the
    route repeats. Item 0 is home, which ArduPilot replaces."""
    frame = mavutil.mavlink.MAV_FRAME_GLOBAL_RELATIVE_ALT_INT
    items = [(mavutil.mavlink.MAV_CMD_NAV_WAYPOINT, 0, 0, start_lat_deg, start_lon_deg, 0.0)]
    if takeoff_alt_m is not None:
        # Pitch 0 leaves ArduPlane's own minimum takeoff pitch.
        items.append((mavutil.mavlink.MAV_CMD_NAV_TAKEOFF, 0, 0, start_lat_deg, start_lon_deg,
                      takeoff_alt_m))
    first = len(items)
    for index, (east_m, north_m, altitude_m) in enumerate(waypoints):
        lat_deg, lon_deg = offset_location(start_lat_deg, start_lon_deg, east_m, north_m)
        if land and index == len(waypoints) - 1:
            # Its altitude is the ground's there, relative to home.
            items.append((mavutil.mavlink.MAV_CMD_NAV_LAND, 0, 0, lat_deg, lon_deg, altitude_m))
        else:
            items.append((mavutil.mavlink.MAV_CMD_NAV_WAYPOINT, 0, 0, lat_deg, lon_deg, altitude_m))
    if not land:
        items.append((mavutil.mavlink.MAV_CMD_DO_JUMP, first, -1, 0.0, 0.0, 0.0))

    connection.mav.mission_count_send(connection.target_system, connection.target_component, len(items))
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        message = connection.recv_match(type=["MISSION_REQUEST_INT", "MISSION_REQUEST", "MISSION_ACK"],
                                        blocking=True, timeout=1)
        if message is None:
            continue
        if message.get_type() == "MISSION_ACK":
            if message.type != mavutil.mavlink.MAV_MISSION_ACCEPTED:
                raise SystemExit(f"error: mission rejected (MAV_MISSION_RESULT {message.type})")
            return
        command, param1, param2, lat_deg, lon_deg, altitude_m = items[message.seq]
        connection.mav.mission_item_int_send(
            connection.target_system, connection.target_component, message.seq, frame, command,
            0, 1, param1, param2, 0, 0, int(lat_deg * 1e7), int(lon_deg * 1e7), altitude_m,
        )
    raise SystemExit("error: mission upload timed out")


def main():
    mode_wanted = os.environ.get("MODE", "LOITER")
    release_s = float(os.environ["RELEASE_S"]) if os.environ.get("RELEASE_S") else None
    release_check = ReleaseCheck(release_s)
    waypoints = parse_waypoints(os.environ.get("WAYPOINTS", ""))
    takeoff_alt_m = float(os.environ["TAKEOFF_ALT"]) if os.environ.get("TAKEOFF_ALT") else None
    land = os.environ.get("LAND") == "1"
    mission_uploaded = not waypoints
    goto = parse_waypoints(os.environ.get("GOTO", ""))
    position = None
    start = None  # where the aircraft is held; route and target are relative to it
    run_dir = Path(os.environ.get("RUN_DIR", "/run/sitl"))
    run_dir.mkdir(parents=True, exist_ok=True)
    connection = mavutil.mavlink_connection(
        "udpin:0.0.0.0:14550", source_system=255, autoreconnect=False
    )
    plane = start_sitl(run_dir)

    mode, armed, gps_fix, ekf_flags, boot_s = "", False, 0, 0, 0.0
    announced = False
    disarmed_after_flight = False
    commands = CommandClock()
    try:
        while running and plane.poll() is None:
            message = connection.recv_match(blocking=True, timeout=0.1)
            if message:
                kind = message.get_type()
                if kind == "HEARTBEAT" and message.type != mavutil.mavlink.MAV_TYPE_GCS:
                    connection.target_system = message.get_srcSystem()
                    connection.target_component = message.get_srcComponent()
                    mode = mavutil.mode_string_v10(message)
                    armed = bool(message.base_mode & mavutil.mavlink.MAV_MODE_FLAG_SAFETY_ARMED)
                    if boot_s == 0.0:
                        # Ask for telemetry until ArduPilot's clock arrives;
                        # every later command is paced by that clock.
                        request_telemetry(connection)
                elif kind == "GPS_RAW_INT":
                    gps_fix = int(message.fix_type)
                elif kind == "EKF_STATUS_REPORT":
                    ekf_flags = int(message.flags)
                elif kind == "GLOBAL_POSITION_INT":
                    position = (message.lat / 1e7, message.lon / 1e7)
                elif kind == "SYSTEM_TIME":
                    boot_s = message.time_boot_ms / 1000.0
                elif kind == "STATUSTEXT":
                    # Arming failures and EKF messages show up here.
                    print("ardupilot:", message.text, flush=True)

            ready_to_fly = armed and mode == mode_wanted
            if release_check.missed(boot_s, ready_to_fly):
                print(f"error: not armed in {mode_wanted} by the {release_s:g} s release "
                      f"(mode {mode or 'unknown'}, armed {armed}, GPS fix {gps_fix}, "
                      f"EKF flags {ekf_flags})", flush=True)
                return 1

            if boot_s == 0.0 or not commands.due(boot_s):
                continue
            neutral_rc(connection, 1500 if armed and mode == mode_wanted else 1000)
            ready = gps_fix >= REQUIRED_GPS_FIX and (ekf_flags & REQUIRED_EKF_FLAGS) == REQUIRED_EKF_FLAGS
            if ready and start is None and position is not None:
                start = position
            if announced and not armed:
                # Armed once and now disarmed, as after a landing: stay down.
                if not disarmed_after_flight:
                    print("disarmed after flight", flush=True)
                    disarmed_after_flight = True
                continue
            if not mission_uploaded:
                # AUTO needs the mission first; it is placed relative to where
                # the aircraft is, so wait for a good position.
                if start is not None:
                    upload_mission(connection, *start, waypoints, takeoff_alt_m, land)
                    print(f"uploaded {len(waypoints)} waypoints", flush=True)
                    mission_uploaded = True
            elif takeoff_alt_m is not None and not armed:
                if ready:
                    connection.mav.command_long_send(
                        connection.target_system, connection.target_component,
                        mavutil.mavlink.MAV_CMD_COMPONENT_ARM_DISARM, 0,
                        1, 0, 0, 0, 0, 0, 0,
                    )
            elif mode != mode_wanted:
                mode_id = connection.mode_mapping().get(mode_wanted)
                if mode_id is None:
                    raise SystemExit(f"unknown ArduPlane mode {mode_wanted}")
                connection.mav.set_mode_send(
                    connection.target_system,
                    mavutil.mavlink.MAV_MODE_FLAG_CUSTOM_MODE_ENABLED, mode_id,
                )
            elif not armed:
                if ready:
                    # Normal arming: a failed pre-arm check is reported, not forced past.
                    connection.mav.command_long_send(
                        connection.target_system, connection.target_component,
                        mavutil.mavlink.MAV_CMD_COMPONENT_ARM_DISARM, 0,
                        1, 0, 0, 0, 0, 0, 0,
                    )
            elif not announced:
                print(f"armed in {mode}", flush=True)
                announced = True
                if goto:
                    send_guided_target(connection, *start, goto[0])
                    print(f"sent GUIDED target {goto[0]}", flush=True)
    finally:
        if plane.poll() is None:
            plane.terminate()
            try:
                plane.wait(timeout=5)
            except subprocess.TimeoutExpired:
                plane.kill()
                plane.wait()
    return 0 if plane.returncode in (0, None, -signal.SIGTERM) else 1


if __name__ == "__main__":
    signal.signal(signal.SIGINT, stop)
    signal.signal(signal.SIGTERM, stop)
    sys.exit(main())

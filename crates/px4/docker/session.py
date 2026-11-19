#!/usr/bin/env python3
"""Run PX4 SITL for one aircraft: start it, arm it in a mode, then keep reading
MAVLink until PX4 exits or the container is stopped.

The mode is set once, before arming, and then left to PX4: TAKEOFF climbs to
MIS_TAKEOFF_ALT above where the aircraft armed and turns into LOITER there.
LOITER (PX4's Hold) set on the ground gives a fixed wing nothing to do, so a
run that wants to circle starts in TAKEOFF.

PX4 boots into Hold, since it has no sticks, and Hold runs the fixed-wing
height controller from the first estimator output, before the estimator has
an altitude: the controller then starts from zero and chases the real
altitude at its climb-rate limit for over a minute of flight. So the script
first puts PX4 in STABILIZED, where that controller is off, and keeps it
there for MODE_HOLD_S of PX4 time, longer than the second of silence after
which the controller starts afresh, before setting the mode to fly.

Runs inside the container (see Dockerfile); crates/px4/src/px4_launcher.rs sets
the environment. MAVLink stays inside the container: this script is the ground
station on PX4's GCS link (UDP 18570 plus the instance), which PX4 streams to
once it has heard from us. The ArduPilot session (crates/ardupilot/docker) has
the same shape; what differs here is PX4's mission protocol, modes, and
arming order.

PX4 arms in the mode it will fly: the mission is uploaded, the mode set, and
then the aircraft armed. A runway takeoff starts when a mission with a takeoff
item is armed in MISSION; the takeoff item's position gives the heading to
roll along. If RELEASE_S is set and PX4 is not armed in its mode 1 s before
then (by PX4's clock, which follows the simulator's time stamps), the session
exits with an error; that stops PX4, so the run fails instead of releasing a
disarmed aircraft.

If WAYPOINTS is set ("east,north,up; ..." in metres from where the aircraft
starts), it uploads them as a MISSION route that jumps back to the first
waypoint, then flies in MISSION. "Up" is from the start too: PX4 sets home
where it arms, and waypoint altitudes are relative to home. TAKEOFF_ALT (m)
starts the route with a takeoff climbing to that height towards the first
waypoint; LAND=1 makes the last waypoint a landing, whose "up" is the ground's
height there relative to the start (0 on the same runway), and the route does
not repeat.
"""

import math
import os
from pathlib import Path
import signal
import subprocess
import sys
import time

from pymavlink import mavutil

from pacing import CommandClock, ModeHold, ReleaseCheck

REQUIRED_GPS_FIX = 3
EARTH_RADIUS_M = 6378137.0
# Repeat count for the jump that loops a route: PX4 keeps it as a 16-bit count.
REPEAT_FOREVER = 65535

running = True


def stop(_signal, _frame):
    global running
    running = False


def start_px4(instance):
    # The working directory is the aircraft's run folder: PX4 keeps its
    # parameters (fs/) and flight logs (log/) there. The environment carries
    # PX4_SIM_MODEL, PX4_SIM_HOSTNAME, PX4_SIM_SPEED_FACTOR, and PX4_PARAM_*.
    command = ["px4", "-i", str(instance), "-d", "/opt/px4/etc"]
    print("starting:", " ".join(command), flush=True)
    return subprocess.Popen(command)


def request_message(connection, message_id, rate_hz):
    connection.mav.command_long_send(
        connection.target_system, connection.target_component,
        mavutil.mavlink.MAV_CMD_SET_MESSAGE_INTERVAL, 0,
        message_id, 1_000_000 / rate_hz, 0, 0, 0, 0, 0,
    )


def request_telemetry(connection):
    # Rates are in PX4 time, so they keep up with any speedup.
    for message_id in (
        mavutil.mavlink.MAVLINK_MSG_ID_GPS_RAW_INT,
        mavutil.mavlink.MAVLINK_MSG_ID_SYSTEM_TIME,
        mavutil.mavlink.MAVLINK_MSG_ID_GLOBAL_POSITION_INT,
    ):
        request_message(connection, message_id, 5)


def heartbeat(connection):
    """Our ground-station heartbeat; PX4 streams to whoever sends one."""
    connection.mav.heartbeat_send(
        mavutil.mavlink.MAV_TYPE_GCS, mavutil.mavlink.MAV_AUTOPILOT_INVALID, 0, 0, 0,
    )


def parse_waypoints(text):
    """"east,north,up; ..." -> [(east_m, north_m, up_m), ...]"""
    return [tuple(float(value) for value in point.split(",")) for point in text.split(";") if point.strip()]


def offset_location(start_lat_deg, start_lon_deg, east_m, north_m):
    """Latitude and longitude east_m and north_m from a start point (flat Earth)."""
    lat_deg = start_lat_deg + math.degrees(north_m / EARTH_RADIUS_M)
    lon_deg = start_lon_deg + math.degrees(east_m / (EARTH_RADIUS_M * math.cos(math.radians(start_lat_deg))))
    return lat_deg, lon_deg


def set_mode(connection, mode):
    """PX4 modes are a main and a sub mode in DO_SET_MODE's params 2 and 3."""
    if mode not in mavutil.px4_map:
        raise SystemExit(f"error: unknown PX4 mode {mode}")
    base_mode, main_mode, sub_mode = mavutil.px4_map[mode]
    connection.mav.command_long_send(
        connection.target_system, connection.target_component,
        mavutil.mavlink.MAV_CMD_DO_SET_MODE, 0,
        base_mode, main_mode, sub_mode, 0, 0, 0, 0,
    )


def arm(connection):
    connection.mav.command_long_send(
        connection.target_system, connection.target_component,
        mavutil.mavlink.MAV_CMD_COMPONENT_ARM_DISARM, 0,
        1, 0, 0, 0, 0, 0, 0,
    )


def upload_mission(connection, start_lat_deg, start_lon_deg, waypoints, takeoff_alt_m, land):
    """Upload the waypoints as a MISSION route: optionally a takeoff first,
    and either a landing at the last waypoint or a jump back to the first one
    so the route repeats. PX4 has no home item: item 0 is the first command."""
    frame = mavutil.mavlink.MAV_FRAME_GLOBAL_RELATIVE_ALT_INT
    items = []
    if takeoff_alt_m is not None:
        # The takeoff item's position is the direction to roll and climb in;
        # pitch 0 leaves PX4's own minimum takeoff pitch.
        east_m, north_m, _ = waypoints[0]
        lat_deg, lon_deg = offset_location(start_lat_deg, start_lon_deg, east_m, north_m)
        items.append((mavutil.mavlink.MAV_CMD_NAV_TAKEOFF, 0, 0, lat_deg, lon_deg, takeoff_alt_m))
    first = len(items)
    for index, (east_m, north_m, altitude_m) in enumerate(waypoints):
        lat_deg, lon_deg = offset_location(start_lat_deg, start_lon_deg, east_m, north_m)
        if land and index == len(waypoints) - 1:
            # Its altitude is the ground's there, relative to home.
            items.append((mavutil.mavlink.MAV_CMD_NAV_LAND, 0, 0, lat_deg, lon_deg, altitude_m))
        else:
            items.append((mavutil.mavlink.MAV_CMD_NAV_WAYPOINT, 0, 0, lat_deg, lon_deg, altitude_m))
    if not land:
        items.append((mavutil.mavlink.MAV_CMD_DO_JUMP, first, REPEAT_FOREVER, 0.0, 0.0, 0.0))

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
    instance = int(os.environ.get("PX4_INSTANCE", "0"))
    mode_wanted = os.environ.get("MODE", "LOITER")
    release_s = float(os.environ["RELEASE_S"]) if os.environ.get("RELEASE_S") else None
    release_check = ReleaseCheck(release_s)
    waypoints = parse_waypoints(os.environ.get("WAYPOINTS", ""))
    takeoff_alt_m = float(os.environ["TAKEOFF_ALT"]) if os.environ.get("TAKEOFF_ALT") else None
    land = os.environ.get("LAND") == "1"
    mission_uploaded = not waypoints
    position = None
    start = None  # where the aircraft is held; the route is relative to it
    Path(os.environ.get("RUN_DIR", "/run/sitl")).mkdir(parents=True, exist_ok=True)
    connection = mavutil.mavlink_connection(
        f"udpout:127.0.0.1:{18570 + instance}", source_system=255, autoreconnect=False
    )
    px4 = start_px4(instance)

    mode, armed, gps_fix, boot_s = "", False, 0, 0.0
    reported_mode = ""
    announced = False
    disarmed_after_flight = False
    commands = CommandClock()
    settling = ModeHold("STABILIZED")
    last_hello = 0.0
    try:
        while running and px4.poll() is None:
            if boot_s == 0.0 and time.monotonic() - last_hello > 0.2:
                # Until PX4 answers, say hello by wall time so it learns our
                # address and starts streaming; every later command is paced by
                # its clock.
                heartbeat(connection)
                if connection.target_system:
                    request_telemetry(connection)
                last_hello = time.monotonic()
            message = connection.recv_match(blocking=True, timeout=0.1)
            if message:
                kind = message.get_type()
                if kind == "HEARTBEAT" and message.type != mavutil.mavlink.MAV_TYPE_GCS:
                    connection.target_system = message.get_srcSystem()
                    connection.target_component = message.get_srcComponent()
                    mode = mavutil.interpret_px4_mode(message.base_mode, message.custom_mode)
                    armed = bool(message.base_mode & mavutil.mavlink.MAV_MODE_FLAG_SAFETY_ARMED)
                    if mode != reported_mode:
                        # PX4 changes mode by itself: TAKEOFF becomes LOITER
                        # once the climb is done, and a landing ends in LOITER.
                        print(f"mode {mode}", flush=True)
                        reported_mode = mode
                elif kind == "GPS_RAW_INT":
                    gps_fix = int(message.fix_type)
                elif kind == "GLOBAL_POSITION_INT":
                    position = (message.lat / 1e7, message.lon / 1e7)
                elif kind == "SYSTEM_TIME":
                    boot_s = message.time_boot_ms / 1000.0
                elif kind == "STATUSTEXT":
                    # Arming refusals and estimator messages show up here.
                    print("px4:", message.text, flush=True)

            ready_to_fly = armed and (mode == mode_wanted or announced)
            if release_check.missed(boot_s, ready_to_fly):
                print(f"error: not armed in {mode_wanted} by the {release_s:g} s release "
                      f"(mode {mode or 'unknown'}, armed {armed}, GPS fix {gps_fix})", flush=True)
                return 1

            if boot_s == 0.0 or not commands.due(boot_s):
                continue
            heartbeat(connection)
            ready = gps_fix >= REQUIRED_GPS_FIX and position is not None
            if ready and start is None:
                start = position
            if announced and not armed:
                # Armed once and now disarmed, as after a landing: stay down.
                if not disarmed_after_flight:
                    print("disarmed after flight", flush=True)
                    disarmed_after_flight = True
                continue
            if not announced and not settling.held(boot_s, mode):
                # Stabilized first, until the height controller has reset.
                if mode != settling.mode:
                    set_mode(connection, settling.mode)
            elif not mission_uploaded:
                # The route is placed relative to where the aircraft is, so
                # wait for a good position.
                if start is not None:
                    upload_mission(connection, *start, waypoints, takeoff_alt_m, land)
                    print(f"uploaded {len(waypoints)} waypoints", flush=True)
                    mission_uploaded = True
            elif mode != mode_wanted and not announced:
                set_mode(connection, mode_wanted)
            elif not armed:
                if ready:
                    # Normal arming: a failed check is reported, not forced past.
                    arm(connection)
            elif not announced:
                print(f"armed in {mode}", flush=True)
                announced = True
    finally:
        if px4.poll() is None:
            px4.terminate()
            try:
                px4.wait(timeout=5)
            except subprocess.TimeoutExpired:
                px4.kill()
                px4.wait()
    return 0 if px4.returncode in (0, None, -signal.SIGTERM) else 1


if __name__ == "__main__":
    signal.signal(signal.SIGINT, stop)
    signal.signal(signal.SIGTERM, stop)
    sys.exit(main())

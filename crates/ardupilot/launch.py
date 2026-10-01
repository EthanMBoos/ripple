#!/usr/bin/env python3
"""Fly a mission's ArduPilot aircraft: start one ArduPlane SITL container per
ArduPilotBridge, run the mission, then stop every container.

    python3 crates/ardupilot/launch.py crates/ardupilot/missions/rascal-sitl.xml [key:=value ...]

Overrides use the same `key:=value` form as `scrimmage run` and are passed on
to it. The aircraft and their ports come from the mission itself, so the
containers always match it. Each container gets its own port, system id, and
fresh directory under runs/<mission>/sitlNNN/. ArduPilot aircraft all exist
from t = 0; spawning them mid-run is out of scope.

The mission must be XML with each ArduPilot aircraft written out directly in
its <entity>: this reads the mission with a small reader of its own, so it
rejects param_common, entity_common, and XInclude rather than guess.

The last frame of a run is never answered, so SITL never finishes on its own:
the containers are stopped when the mission ends, whatever the outcome.
See book/src/guides/ardupilot.md.
"""

import argparse
from pathlib import Path
import re
import subprocess
import sys
import xml.etree.ElementTree as ElementTree

REPO = Path(__file__).resolve().parents[2]
DOCKER_DIR = Path(__file__).resolve().parent / "docker"
IMAGE = "scrimmage-rs-arduplane"
LABEL = "scrimmage-rs-sitl"
# SITL places the aircraft at this home plus the local position the bridge
# sends, so local (0, 0, 0) is here (latitude, longitude, MSL altitude, heading).
# XML missions have no geographic origin; this default is McMillan Airfield.
DEFAULT_HOME = "35.721025,-120.767925,300,0"


def substitute(text, overrides):
    """Resolve the mission's ${name=default} variables like scrimmage does."""
    return re.sub(
        r"\$\{(\w+)=([^}]*)\}",
        lambda match: overrides.get(match.group(1), match.group(2)),
        text,
    )


def read_mission(path, overrides):
    """Return (dt in seconds, [(bridge UDP port, release time or None), ...])
    from an XML mission. The release time is FixedWing6DOF's launch_time when
    its launcher holds the aircraft; ArduPlane must be armed before it."""
    if path.suffix != ".xml":
        raise SystemExit(f"launch.py reads XML missions only, not {path.name}")
    text = path.read_text()
    for form in ("param_common", "entity_common", "xi:include", "XInclude"):
        if form in text:
            raise SystemExit(f"launch.py does not follow {form}; write each ArduPilot "
                             "aircraft's autonomy and motion_model directly in its <entity>")
    root = ElementTree.fromstring(substitute(text, overrides))
    dt_s = float(root.find("run").get("dt"))
    ports = []
    for entity in root.iter("entity"):
        for autonomy in entity.iter("autonomy"):
            if (autonomy.text or "").strip() != "ArduPilotBridge":
                continue
            count = int((entity.findtext("count") or "1").strip())
            if count != 1:
                raise SystemExit("each ArduPilot aircraft needs its own entity with count 1")
            motion = entity.find("motion_model")
            held = motion is not None and motion.get("use_launcher", "false").lower() in ("true", "1")
            release_s = float(motion.get("launch_time", "30")) if held else None
            ports.append((int(autonomy.get("port", "9002")), release_s))
    if not ports:
        raise SystemExit(f"{path} has no ArduPilotBridge aircraft")
    if len({port for port, _ in ports}) != len(ports):
        raise SystemExit(f"ArduPilotBridge ports must differ: {ports}")
    return dt_s, ports


def next_free(directory, prefix):
    index = 0
    while (directory / f"{prefix}{index:03d}").exists():
        index += 1
    return directory / f"{prefix}{index:03d}"


def ensure_image():
    found = subprocess.run(["docker", "image", "inspect", IMAGE], capture_output=True)
    if found.returncode != 0:
        print(f"building {IMAGE} (compiles ArduPlane; takes a while the first time)", flush=True)
        subprocess.run(["docker", "build", "-t", IMAGE, str(DOCKER_DIR)], check=True)


def start_container(name, port, release_s, sysid, rate_hz, args, run_dir):
    command = ["docker", "run", "-d", "--rm", "--name", name, "--label", LABEL,
               "--volume", f"{run_dir}:/run/sitl"]
    if sys.platform.startswith("linux"):
        command += ["--add-host", "host.docker.internal:host-gateway"]
    for key, value in {
        "SIM_PORT_OUT": port,
        "SYSID": sysid,
        "RATE_HZ": rate_hz,
        "SPEEDUP": args.speedup,
        "HOME_LOCATION": args.home,
        "MODE": args.mode,
        "WAYPOINTS": args.waypoints,
        "GOTO": args.goto,
        "RELEASE_S": "" if release_s is None else release_s,
    }.items():
        command += ["-e", f"{key}={value}"]
    subprocess.run(command + [IMAGE], check=True, capture_output=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("mission", type=Path)
    parser.add_argument("overrides", nargs="*", help="key:=value, passed to scrimmage run")
    parser.add_argument("--mode", help="ArduPlane mode to arm in (default LOITER; AUTO with --waypoints, "
                                       "GUIDED with --goto)")
    parser.add_argument("--waypoints", default="",
                        help='AUTO route "east,north,up; ..." in metres from each aircraft\'s start '
                             "(ArduPlane moves home to where it arms); the route repeats")
    parser.add_argument("--goto", default="",
                        help='GUIDED target "east,north,up" from each aircraft\'s start, sent once '
                             "armed in GUIDED; ArduPlane flies there and circles it")
    parser.add_argument("--speedup", default="100", help="SITL --speedup (its real-time cap)")
    parser.add_argument("--home", default=DEFAULT_HOME, help="lat,lon,alt_msl,heading for local (0,0,0)")
    parser.add_argument("--workers", type=int,
                        help="scrimmage workers (default: one per aircraft, so their SITL waits overlap)")
    args = parser.parse_args()
    if args.waypoints and args.goto:
        parser.error("use --waypoints (AUTO) or --goto (GUIDED), not both")
    needed = "AUTO" if args.waypoints else "GUIDED" if args.goto else None
    if needed and args.mode and args.mode != needed:
        parser.error(f"{'--waypoints' if args.waypoints else '--goto'} needs --mode {needed}")
    args.mode = args.mode or needed or "LOITER"

    overrides = dict(item.split(":=", 1) for item in args.overrides)
    dt_s, ports = read_mission(args.mission, overrides)
    rate_hz = round(1.0 / dt_s)
    run_root = next_free(REPO / "runs" / args.mission.stem, "sitl")

    ensure_image()
    subprocess.run(["cargo", "build", "--release", "-p", "scrimmage-rs", "--features", "ardupilot"],
                   cwd=REPO, check=True)

    containers, loggers = [], []
    try:
        for index, (port, release_s) in enumerate(ports):
            run_dir = run_root / f"aircraft-{port}"
            run_dir.mkdir(parents=True)
            name = f"{LABEL}-{run_root.name}-{port}"
            start_container(name, port, release_s, index + 1, rate_hz, args, run_dir)
            containers.append(name)
            log = (run_dir / "session.log").open("w")
            loggers.append(subprocess.Popen(["docker", "logs", "-f", name], stdout=log,
                                            stderr=subprocess.STDOUT))
            print(f"SITL for port {port} (sysid {index + 1}): {run_dir}", flush=True)
        scrimmage = [str(REPO / "target/release/scrimmage"), "run", str(args.mission), "--headless",
                     "--workers", str(args.workers or len(ports)), *args.overrides]
        returncode = subprocess.run(scrimmage, cwd=REPO).returncode
    finally:
        for name in containers:
            subprocess.run(["docker", "stop", "--time", "3", name], capture_output=True)
        for logger in loggers:
            logger.wait(timeout=10)
    # A SITL that stops early (for example, not armed by release) makes the
    # run fail with a bridge timeout; show the session's own reason.
    for log in sorted(run_root.glob("*/session.log")):
        for line in log.read_text().splitlines():
            if line.startswith("error:"):
                print(f"{log.parent.name}: {line}", flush=True)
    return returncode


if __name__ == "__main__":
    sys.exit(main())

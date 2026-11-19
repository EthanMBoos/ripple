#!/usr/bin/env python3
"""Build the book's demo gallery (docs/demos-plan.md).

Each demo file in book/demos/ names a mission to run or a saved run to
replay and how to present it. For each, this writes under
book/src/demos/<name>/ a Rerun recording with the demo's layout inside it,
which the book embeds in Rerun's web viewer, and an mp4 captured from the
headless viewer one frame at a time, for the README.

    python3 reference/demos.py                  # every demo
    python3 reference/demos.py terrain-flight   # the named demos
    python3 reference/demos.py --no-video       # recordings only
    python3 reference/demos.py --check          # recordings into a temporary
                                                # directory, held to the size
                                                # budget; the book is untouched

The recording comes from `ripple run --demo` for a mission and from `ripple
replay --demo` for a saved run. The video needs `rerun` 0.36 and `ffmpeg` on
the PATH: the headless viewer opens the recording, `rerun viewer-mcp` moves
its time cursor and saves a screenshot per video frame, and ffmpeg joins the
frames.
"""
import argparse
import json
import math
import re
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

from project import ROOT, build_cli

DEMOS = ROOT / "book/demos"
GALLERY = ROOT / "book/src/demos"
# A recording larger than this is too slow to open from a page and too
# heavy for the repository; shrink the interval, the frame stride, or
# `terrain_samples` in the demo file. The terrain flight at 512 samples a
# side is the largest, about 33 MB.
RECORDING_BUDGET_BYTES = 40 << 20
EYE_SETTLE_FRAMES = 40
FRAME_RATE_HZ = 30
VIDEO_SIZE = (1920, 1080)
# The viewer's zoom for the video, so its text reads at 1080p, in steps of
# a tenth from one; the collapsed play bar under the views is cropped off.
ZOOM_STEPS = 5
PLAY_BAR_PX = round(28 * (1 + ZOOM_STEPS / 10))


def demos(names):
    """The demo files, all of them or the named ones, with the keys this
    script needs: `name`, `source`, `speed`, and `features`, which the files
    write as top-level scalars or a flow list. `ripple` reads the rest."""
    found = {path.stem: path for path in sorted(DEMOS.glob("*.yaml"))}
    missing = [name for name in names if name not in found]
    if missing:
        sys.exit(f"no demo file for {', '.join(missing)} in {DEMOS}")
    chosen = names or list(found)
    return [(found[name], top_level(found[name].read_text())) for name in chosen]


def top_level(text):
    """The file's top-level scalar keys and their unquoted values, and
    `features` as a tuple of words."""
    keys = {"features": ()}
    for line in text.splitlines():
        match = re.match(r"^([A-Za-z_]+):\s*([^#{\[\s][^#]*?)\s*$", line)
        if match:
            keys[match.group(1)] = match.group(2).strip("\"'")
        match = re.match(r"^features:\s*\[([A-Za-z0-9_, ]*)\]\s*$", line)
        if match:
            keys["features"] = tuple(word.strip() for word in match.group(1).split(",") if word.strip())
    if "speed" in keys:
        keys["speed"] = float(keys["speed"])
    for key in ("name", "source"):
        if key not in keys:
            sys.exit(f"demo file has no top-level {key}")
    return keys


def export(binary, demo_path, demo, work):
    """Write the demo's recording under `work` and return its path."""
    source = ROOT / demo["source"]
    if source.is_dir() or source.suffix == ".mcap":
        recording = work / "recording.rrd"
        command = [binary, "replay", str(source), "--demo", str(demo_path),
                   "--output", str(recording)]
    else:
        recording = work / "run" / "recording.rrd"
        command = [binary, "run", "--demo", str(demo_path),
                   "--output", str(work / "run"), "--root", str(ROOT)]
    subprocess.run(command, check=True, cwd=ROOT)
    return recording


class Viewer:
    """A headless Rerun viewer on one recording, driven through its MCP server."""

    def __init__(self, recording, port=9876):
        self.viewer = subprocess.Popen(
            ["rerun", "--headless", "--port", str(port), "--hide-welcome-screen",
             "--window-size", f"{VIDEO_SIZE[0]}x{VIDEO_SIZE[1] + PLAY_BAR_PX}",
             str(recording)],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        self.mcp = subprocess.Popen(
            ["rerun", "viewer-mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL, text=True)
        self.calls = 0
        self.request("initialize", {"protocolVersion": "2025-06-18", "capabilities": {},
                                    "clientInfo": {"name": "ripple-demos", "version": "0"}})
        self.send({"jsonrpc": "2.0", "method": "notifications/initialized"})
        # The viewer lists the recording before its timelines are loaded.
        for attempt in range(60):
            time.sleep(0.5)
            try:
                self.tool("connect", {"endpoint": f"http://127.0.0.1:{port}"})
                if self.duration_s() > 0:
                    break
            except (RuntimeError, OSError):
                pass
        else:
            raise RuntimeError("the headless viewer did not open the recording")
        # Only the views in the frame: the top bar goes through the command
        # palette (the blueprint cannot hide it), and the zoom is the
        # viewer's own, which scales its text without the window.
        self.command("toggle top panel")
        for step in range(ZOOM_STEPS):
            self.tool("press_key", {"key": "Equals", "modifiers": {"command": True}})
        # The 3D view's eye glides from its default pose to the tracked
        # camera over about a second of real time, frame by frame, so the
        # first frames are drawn and thrown away until it has arrived.
        scratch = Path(tempfile.mkdtemp(prefix="ripple-demo-settle-"))
        for step in range(EYE_SETTLE_FRAMES):
            self.frame(0.0, scratch / "frame.png")
            time.sleep(0.05)
        shutil.rmtree(scratch)

    def command(self, name):
        """Run a viewer command by its name in the command palette."""
        self.tool("press_key", {"key": "p", "modifiers": {"command": True}})
        self.tool("type_text", {"text": name})
        self.tool("press_key", {"key": "Enter"})

    def send(self, message):
        self.mcp.stdin.write(json.dumps(message) + "\n")
        self.mcp.stdin.flush()

    def request(self, method, params):
        self.calls += 1
        self.send({"jsonrpc": "2.0", "id": self.calls, "method": method, "params": params})
        while True:
            line = self.mcp.stdout.readline()
            if not line:
                raise RuntimeError("rerun viewer-mcp exited")
            reply = json.loads(line)
            if reply.get("id") == self.calls:
                if "error" in reply:
                    raise RuntimeError(reply["error"])
                return reply["result"]

    def tool(self, name, arguments=None):
        result = self.request("tools/call", {"name": name, "arguments": arguments or {}})
        if result.get("isError"):
            raise RuntimeError(result)
        return result

    def state(self):
        return json.loads(self.tool("viewer_state")["content"][0]["text"])

    def duration_s(self):
        """The recording's playback length on its `time` timeline, or zero
        while it is still loading."""
        for recording in self.state().get("recordings", []):
            for timeline in recording.get("timelines", []):
                if timeline["timeline"] == "time":
                    return timeline["max"] / 1e9
        return 0.0

    def frame(self, time_s, path):
        self.tool("set_time", {"timeline": "time", "time": int(round(time_s * 1e9)),
                               "play": False})
        # Any scale but one keeps the window's pixels under the zoom.
        self.tool("screenshot", {"save_path": str(path), "pixels_per_point": 2.0})

    def close(self):
        self.mcp.terminate()
        self.viewer.terminate()


def capture(recording, speed, video, port):
    """Record the whole recording at `speed` simulated seconds per second."""
    frames = Path(tempfile.mkdtemp(prefix="ripple-demo-frames-"))
    viewer = Viewer(recording, port)
    try:
        count = math.ceil(viewer.duration_s() / speed * FRAME_RATE_HZ)
        for index in range(count):
            viewer.frame(index * speed / FRAME_RATE_HZ, frames / f"{index:06d}.png")
    finally:
        viewer.close()
    subprocess.run(
        ["ffmpeg", "-y", "-loglevel", "error", "-framerate", str(FRAME_RATE_HZ),
         "-i", str(frames / "%06d.png"),
         "-vf", f"crop={VIDEO_SIZE[0]}:{VIDEO_SIZE[1]}:0:0",
         "-c:v", "libx264", "-pix_fmt", "yuv420p",
         "-crf", "24", "-movflags", "+faststart", str(video)],
        check=True)
    shutil.rmtree(frames)
    return count


def build(names, video=True, check=False, port=9876):
    """Export each demo, with the executable built for its `features`, and
    capture its video; the results are (name, recording bytes, frames)."""
    binaries = {}
    results = []
    with tempfile.TemporaryDirectory(prefix="ripple-demos-") as temporary:
        for demo_path, demo in demos(names):
            name = demo["name"]
            features = demo["features"]
            if features not in binaries:
                # Cargo writes every feature set to the same path, so each
                # build is kept under its own name until the run is over.
                built = build_cli(profile="release", features=features)
                kept = Path(temporary) / ("ripple-" + ("-".join(features) or "base"))
                shutil.copy2(built, kept)
                binaries[features] = kept
            binary = binaries[features]
            work = Path(temporary) / name
            work.mkdir()
            recording = export(binary, demo_path, demo, work)
            size = recording.stat().st_size
            if size > RECORDING_BUDGET_BYTES:
                sys.exit(f"{name}: recording.rrd is {size >> 20} MB, over the "
                         f"{RECORDING_BUDGET_BYTES >> 20} MB budget; shorten the "
                         "interval, raise `every`, or lower `terrain_samples`")
            if check:
                results.append((name, size, None))
                print(f"{name}: recording {size >> 10} KiB")
                continue
            out = GALLERY / name
            out.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(recording, out / "recording.rrd")
            frames = None
            if video:
                frames = capture(recording, demo.get("speed", 1.0), out / "demo.mp4", port)
            results.append((name, size, frames))
            print(f"{name}: recording {size >> 10} KiB"
                  + (f", video {frames} frames" if frames else ""))
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("names", nargs="*", help="demo names; all when none")
    parser.add_argument("--no-video", action="store_true", help="recordings only")
    parser.add_argument("--check", action="store_true",
                        help="recordings into a temporary directory, with the size budget")
    parser.add_argument("--port", type=int, default=9876,
                        help="the headless viewer's port (default 9876)")
    arguments = parser.parse_args()
    build(arguments.names, video=not arguments.no_video and not arguments.check,
          check=arguments.check, port=arguments.port)


if __name__ == "__main__":
    main()

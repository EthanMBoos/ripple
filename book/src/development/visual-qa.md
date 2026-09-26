# Visual checks with Rerun

A successful build or `rrd verify` is not a visual check. Dashboard and
visualization changes need screenshots that someone has inspected.

## Viewer MCP setup

Check the Rerun SDK version in `Cargo.lock` against the installed viewer
(`rerun --version`, currently 0.36.2). Pinning only the `rerun` wrapper does not
stop its SDK components from resolving to a newer patch, so prefer `--locked`
builds and recheck recording warnings after dependency updates.

Viewer MCP is registered as a local server, for example in `.codex/config.toml`:

```toml
[mcp_servers.rerun]
command = "rerun"
args = ["viewer-mcp"]
startup_timeout_sec = 20
required = true
default_tools_approval_mode = "approve"
```

MCP permission and permission to launch `rerun` are separate; follow the
session's approval rules for both. If the `mcp__rerun__*` tools are missing,
check project trust and registration (a new session may be needed) and report
it rather than changing global approvals or installing another viewer.

## Taking screenshots

Store recordings, screenshots, and a short `REPORT.md` under ignored
`runs/visual-qa/<review>/`, one new folder per review. Launch a separate
headless viewer on a free port:

```sh
rerun runs/straight-no-gui/run000/recording.rrd --headless --window-size 1920x1200 --bind 127.0.0.1 --port 9878
```

Headless Rerun still needs a graphics adapter; it is not the C++ GUI.

1. MCP `connect` to the viewer, e.g. `http://127.0.0.1:9878`.
2. `viewer_state` and `query_tree`: confirm it is the intended recording.
3. Wait for data and blueprint loading; `resize` to 1920x1200 if needed.
4. `set_time` on the `time` timeline (nanoseconds) using the ranges from
   `viewer_state`.
5. Pause, confirm the time, `screenshot` to a saved path, and inspect each image
   at full resolution.
6. For UI changes, use observed accessibility-tree locators: observe, act, then
   verify. Do not guess coordinates.
7. Check looping separately: resume near the end, see the timeline wrap, and
   leave the viewer playing from the start.

On macOS, native MCP inspection, resize, and screenshot requests have timed out
while timeline control worked; the headless viewer is more reliable for
screenshots. Its cursor advances synthetic time, so it does not measure playback
speed. When switching endpoints, `disconnect` first and verify the new
recording; an `open_url` response alone does not prove it loaded.

One screenshot covers a layout-only edit. Changes to motion, spawning, removal,
or timing need at least five checkpoints, including before and after the event.

## What the default dashboard should show

- A dominant full-mission map with every team and late-spawned agent in view,
  with readable markers, headings, and labels.
- A labeled optional 3D view and complete-run paths.
- Compact grouped debug panels (status, speeds, altitudes), not one per agent.
- Simulation-time playback at 1x, looping the whole run.
- Inspection and source panels collapsed by default.

Reject clipped agents, empty panels, unreadable legends, loading warnings, wrong
time axes, or a loop that stops at the last frame. The map is a local ENU frame,
not geographic terrain.

The report names the mission, recording, versions, viewport, checkpoint times,
what was seen, parity checks, and remaining problems. If time control or
screenshots failed, say the check is incomplete.

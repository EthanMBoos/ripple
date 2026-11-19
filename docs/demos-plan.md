# Demo gallery plan

Status, 2026-11-18: built through step 8's first half, then every video
watched frame by frame and reworked (see "Review of the first videos" at
the end). Meshes, the three preset layouts, `--demo` on `run` and `replay`,
`reference/demos.py`, and six demos in the book: the terrain flights, the
Kalman filter between fixes (with fix markers), the incident day on sixteen
hubs, the spread team, and the ArduPlane and PX4 runway circuits. The book
shows the videos, since nothing can be hosted before the anonymous review,
with each recording linked beside its video. Not built: the two-day
comparison, and the untrained team beside the trained one (the repository
ships only the trained network). What was learned building it is under
"Found while building".

Show what Ripple does with recordings a reader can scrub, not with videos
anyone records by hand. Everything renders in Rerun. A run writes its own
small Rerun recording, a script captures an mp4 from it, and the book embeds
the recording in Rerun's web viewer on the chapter that explains it.

## What exists

- Every run records each entity's position and attitude, its group label,
  index, and `visual_model` name, events, metrics, and the plugins' drawings
  in `run.mcap` (`crates/core/src/record.rs`). `--export-rrd` writes a Rerun
  recording beside it during the run.
- The viewer (`crates/cli/src/viewer.rs`) draws each entity as a dot and a
  heading arrow. It logs the full pose but reads nothing from
  `visual_model`, so the runway missions' `zephyr-blue` and the delivery
  fleet's `aircraft` look the same as a point robot.
- Terrain is already a mesh with the aerial image as texture. The McMillan
  grid is 1,001 by 1,001 samples at 10 m, a million vertices, with a 2,002
  by 2,002 RGB image. Trails and one-sigma belief ellipsoids exist; readings
  are not drawn.
- The blueprint (`crates/cli/src/viewer/blueprint.rs`) is a debugging layout:
  map tab first, a fifth of the screen on status and plots.
- `ripple replay` refuses a record over 16 MB because it decodes every frame
  into memory first; its note (`crates/cli/src/replay.rs`) measured a 267 MB
  record at about two minutes and 2.3 GB resident, and records that
  striding frames in replay was considered and not done because the export
  written during the run serves long runs. Trails are not recorded live
  because of their measured per-frame cost (`crates/cli/src/viewer/trails.rs`).
- The viewer shows the latest logged pose at each time and does not
  interpolate between frames, so motion is as smooth as the kept frames.
- The pinned SDK, Rerun 0.36.2, has `Asset3D` for glTF meshes and the
  blueprint archetype `EyeControls3D` with a camera kind and a tracked entity.
  The installed `rerun` 0.36.2 runs `--headless`, keeps its gRPC server up
  for `save_screenshot` requests, takes one-shot stills with
  `--screenshot-to` and `--window-size`, and serves the viewer-driving MCP
  (`rerun viewer-mcp`, a JSON-RPC server on standard input) we use for
  inspection. Its CLI can drop entities and timelines from a recording and
  merge a blueprint file into one, but cannot cut a time range. `ffmpeg` is
  installed.
- The book deploys to GitHub Pages on every push that touches `book/`
  (`.github/workflows/book.yml`); mdBook copies non-Markdown files into the
  site and passes raw HTML through.

## Outputs

Each demo produces two files under `book/src/demos/<name>/`:

- `recording.rrd`, written by the run itself with `--export-rrd` and the
  demo file below, keeping the demo's interval and frame stride, with the
  preset blueprint and the captions inside it, for `rerun` and, once the
  site is hosted, Rerun's web viewer.
- `demo.mp4`, the same recording captured frame by frame from the headless
  viewer at 1080p, which the book and the README show.

Both are committed, like the retained evidence in `paper/data/`, and
regenerated when the simulator changes. The book's workflow never runs the
simulator.

The size budget is measured, not assumed. A frame stride shrinks the
trajectories and nothing else: the terrain mesh and texture alone are tens of
megabytes raw. The first terrain export sets the number; if it is too large
for a page, the gallery gets a coarser terrain file from
`scripts/terrain/prepare.py`, 20 m cells and a 1,001 by 1,001 image, used
only by the demo missions.

## The demo file

One YAML per demo in `book/demos/<name>.yaml`, read by `run` and `replay`
through one option, `--demo FILE`:

```yaml
name: terrain-flight
source: missions/terrain-mcmillan.yaml   # a mission to run, or a saved run.mcap
preset: flight                            # flight | belief | fleet
start_s: 10                               # optional; the run's start otherwise
end_s: 100                                # optional; the run's end otherwise
every: 1                                  # keep every nth frame
speed: 1                                  # simulated seconds per video second
camera: {follow: {group: peak_orbit, index: 0}}  # or orbit, or a fixed eye
featured: [{group: peak_orbit}, {group: climber, index: 0}]
captions:
  - {at_s: 10, text: "Six aircraft fly waypoint routes through the hills."}
  - {at_s: 60, text: "Two of them are routed into the ridge on purpose."}
```

Entities are named by group label and member index, which is what the
record carries. Caption times and the measurements they state are checked
against the record before a demo is committed. The file stays this small
until three demos work: no general editing language.

Smoothness is a rule, not a feature: the kept frames per video second,
`1 / (dt_s * every * speed)`, must be at least 30 wherever a vehicle moves
on screen. The viewer does not interpolate, so a demo mission picks a step
to meet it (the terrain mission at a tenth of a second and speed 1 shows
ten poses a second, so its demo copy steps at 0.02 s), and the SITL runs at
2.5 ms keep every thirteenth frame. A fleet time-lapse at high speed shows
jumps by nature and is exempt. No pose interpolation is written.

## Presets

Each preset is one blueprint with a fixed style. A demo supplies only the
fields above.

| Preset | Composition | First use |
| --- | --- | --- |
| flight | 3D terrain view full width, aircraft mesh, following or fixed camera, small route map inset, one altitude plot, caption panel | McMillan waypoints, then the runway recordings |
| belief | Truth and estimate markers, one-sigma ellipsoids, a marker when a fix arrives, one error plot, caption panel | Smoother, Kalman, perfect robots on one square |
| fleet | Overhead map, square and the window's height, hubs and closures drawn, routes of featured couriers, requests and outcomes as captions in the column beside it | A delivery day with a surge or closure |

Captions live in the recording, as a text panel logged at their times, so
the embed and the video show the same text and nothing is burned in by
`ffmpeg`.

The fleet preset's side-by-side comparison of two days needs two records
drawn under separate entity prefixes, because Rerun shows one recording per
app. It comes last.

## One option on two existing commands

No new reader or subcommand. `ripple run --export-rrd --demo FILE` writes the
demo recording during the run: frames outside the interval and off the stride
are not logged, the preset blueprint and camera go in, captions are logged at
their times, and trails are drawn for featured entities at the kept frames
only, so the cost measured for live trails does not return. `ripple replay
--demo FILE` does the same from a saved record that fits the 16 MB guard. The
guard stays: the note in `replay.rs` already decided that long runs are
served by the export written during the run, and nothing here changes that.
So the SITL circuits and the delivery day are recorded with the export on,
once, and a preset change later is attached with `rerun rrd merge` and a
blueprint file rather than a rerun; only a change to what the viewer logs,
such as a new mesh, needs the run again.

The belief preset's ellipsoids come from live beliefs in an export and from
the trace in a replay, so a saved record for a belief demo must have been
run with `--trace`; replay already says so when it is not.

## Capture

`reference/demos.py` reads the demo files, builds the CLI, runs a mission
source with `--export-rrd --demo` or replays a record source, starts `rerun
--headless --window-size 1920x1080` on the recording, steps the timeline one
video frame at a time requesting a screenshot each step, and hands the
frames to `ffmpeg` for `demo.mp4`. The screenshot client is `rerun
viewer-mcp` driven over standard input from Python with the same set-time and
screenshot calls used for inspection; if its frame timing or completion is
unreliable, the fallback is the SDK's `save_screenshot` request on the
viewer's gRPC server. This is settled in step 1 below, before anything else
is built.

## Viewer changes

1. Meshes. Map `visual_model` names to glTF files under
   `crates/cli/assets/`: a fixed wing for `aircraft`, `zephyr-blue`, and
   `zephyr-red` (tinted by team), a quadrotor for the multirotor missions, a
   ground robot where a mission asks for one, and the dot for `sphere`.
   Three CC0 models. Logged once per entity with `Asset3D` under the pose
   transform the viewer already writes, so banking and climb show.
2. Presets. The three blueprints beside the current one, chosen by the demo
   file; `--viewer` keeps the debugging layout.
3. Camera. `EyeControls3D` from the demo file's `camera`.
4. Captions. A text panel logged at the demo file's caption times.
5. Readings. The belief preset draws a marker where a position fix arrived,
   next to the existing ellipsoids.

## Book

- Each chapter that owns a demo opens with it: terrain, ArduPlane, PX4, the
  GTSAM guide and the uncertainty concept, training a team, and the
  delivery study's chapter if one is added. One gallery page,
  `book/src/demos.md`, lists them all with a line on what each shows.
- The embed is an iframe on Rerun's hosted web viewer at the version that
  wrote the file, with the recording's URL on this site as its parameter.
  The web viewer version must match the SDK version, which the pin at 0.36.2
  guarantees. GitHub Pages sends open CORS headers, so the viewer can fetch
  the file.
- One rule in `book/theme/custom.css` gives the iframe a 16:9 box and a
  minimum height, and a short `include`d snippet keeps every embed the same.
- The README links the mp4s.
- `book/src/development/documentation.md` gains a section on regenerating
  the gallery and the measured size budget per recording.

## Order of work

1. Capture proof. Ten seconds of an existing export at a fixed frame rate
   through the headless viewer and `ffmpeg`: frame timing, camera control,
   screenshot completion, and the choice of client. Nothing else starts until
   this works.
2. Meshes and the flight preset on a demo copy of
   `missions/terrain-mcmillan.yaml` stepping at 0.02 s, which is
   deterministic, needs no SITL, and runs in seconds. Inspect screenshots
   from the headless viewer, as the visualization rule requires. Measure the
   recording and set the size budget; decide on the coarser gallery terrain.
3. `--demo FILE` on run and replay: interval, stride, preset, camera,
   captions, featured trails.
4. `reference/demos.py` end to end on the terrain demo, then the book
   iframe, the gallery page, the CSS rule, and the documentation section.
   The terrain demo goes live. This is the first milestone: one recording,
   smooth capture, one embed.
5. The runway recordings: run the ArduPlane and PX4 circuits once with the
   export and their demo files, keep the recordings.
6. The belief preset with reading markers on
   `crates/gtsam/missions/position-smoother.yaml`.
7. The team-training demo from the shipped `spread.onnx`, untrained beside
   trained.
8. The fleet preset on a 16-hub day run with the export, then the two-day
   comparison.

## Decisions and why

- Rerun only. One drawing code path serves the live viewer, replay, the
  embeds, and the videos. Blender is deferred until a shot needs what Rerun
  cannot give.
- Embeds before videos. A reader scrubbing the recording learns more than a
  video shows, and the recording regenerates from the mission. Videos remain
  for the README.
- Committed outputs, not a simulator in CI. The book workflow stays a
  Markdown build; the outputs are refreshed like `paper/data/`.
- The run writes the recording. The live export already draws with the
  viewer code, so a reader of the record would be a second path to maintain
  and a conversion to wait for; an interval and a stride are all it lacked.
  Replay's 16 MB guard stays, in line with its own note.
- One `--demo FILE` option, not a flag per field. Captions must reach the
  viewer anyway, and six flags on two commands would drift apart.
- A frame-rate rule instead of pose interpolation. Demo missions are cheap
  to step finer, and interpolation would be presentation machinery with its
  own correctness to check.
- The McMillan waypoint mission as the fixture, the runway recordings as the
  showcase. The fixture regenerates in a test; the showcase cannot.
- Three presets with fixed style, demo files that only pick a preset, an
  interval, a stride, a camera, featured entities, and captions. A general
  shot language before three demos exist would be machinery for hypothetical
  needs.
- glTF meshes, not silhouettes. `Asset3D` takes the file as is; a drawn
  silhouette is more code for less.

## Not in this plan

- Sensor outages or any new disturbance written for a caption.
- Thousands of trails. The scale demo uses the fleet preset with featured
  couriers only.
- Changes to what runs record. Everything here reads the existing record.
- A streaming or strided reader for large saved records.

## Checks

- `cargo build --workspace`, `cargo test --workspace`, `cargo fmt --all --
  --check`, `cargo clippy --workspace --all-targets -- -D warnings`.
- The engine's output stays byte-identical with the export and demo file on
  or off, at the same trace setting, as the invariant requires; the viewer
  is output only.
- `python3 -m unittest discover -s reference -p 'test_*.py'` with a test
  that runs the terrain demo and checks the recording against the measured
  size budget.
- Inspected screenshots of each preset at its captions' times, and each
  caption's claim checked against the record, before a demo is committed.

## Found while building

- The terrain alone is 57 MB in a full export (a million vertices, the
  aerial image at 2,002 by 2,002, the relief image); 6,000 frames of six
  aircraft add 8 MB. At 256 samples per side the terrain demo's recording
  is 14 MB, with the images shrunk to two texels per kept sample; that was
  a blur from 250 m behind the aircraft (a 40 m mesh, 20 m texels), so the
  demo keeps 512 a side, about 33 MB, and the budget is 40 MB now that the
  recordings are not hosted.
- Vehicle models are boxes built in `viewer/models.rs`, not glTF files:
  the repository then holds no binary assets or license text, and every
  model takes its team's color. A fixed wing is a fuselage, wing,
  tailplane, and fin; the quadrotor a body, crossed arms, and four discs;
  the ground robot a chassis on wheels. They read clearly at a few hundred
  metres.
- The SDK's layout builder cannot set a 3D view's camera in this version,
  so `viewer/preset.rs` writes the blueprint with Rerun's archetypes at
  paths it chooses.
- A tracked entity does not bring the eye closer: with an orbital eye the
  orbit centre follows but the distance stays at the scene's; with a
  first-person eye the eye sits inside the entity. A tracked camera is
  different: the eye takes its pose. So `follow` logs a pinhole camera
  behind and above the followed entity, pitched to look at it, and tracks
  that. The camera is its own entity, posed each frame from the followed
  entity's position and heading alone, so the horizon stays level while
  the aircraft banks; a child of the entity rode its bank, and a circuit
  tilted the whole world. `fixed` and `orbit` cameras honor their eye
  position.
- A recording holds one layout, and the viewer keeps the first it is given,
  so a demo's layout is sent once, at the end, when the followed entity is
  known.
- The YAML crate reads an enum as a tag, not a one-key map, so the demo
  file's `camera` is read with its singleton-map adapter.
- The headless viewer lists a recording before its timelines load; the
  capture waits for the `time` timeline before seeking.
- The expanded time panel takes a quarter of the window; collapsed, it is
  the play bar alone, which is what both the embed and the video want.
- A hundred couriers logged one by one, each with a transform, a marker,
  an arrow, and two scalars a frame, made 176 MB of eighty minutes at one
  frame per two seconds: Rerun's cost is per row, not per byte. The fleet
  preset draws its unfeatured entities as one batch of points per frame on
  the map and in the world, and the same eighty minutes are 3.9 MB.
- The belief preset's fix markers come from the sensors' reading records,
  the same trace records `--trace` writes, kept in memory for the viewer as
  the belief records already were; a replay reads them from a traced
  record's `/readings` channel.
- The SITL runway circuit at 400 Hz with every thirteenth step kept is
  9.5 MB of frames over 240 s; with the terrain at 340 samples a side, so
  the ground 50 m behind the aircraft is not a smear, the demo keeps the
  180 s between the autopilot arming and the end of the landing roll to
  stay under the budget.
- A demo names the Cargo features its run needs (`features: [px4]`), and
  the gallery script builds one executable per feature set.
- The hosted web viewer embeds never showed: the repository's GitHub Pages
  is not enabled (the book workflow fails at its Pages step), and the
  repository must stay unhosted until its anonymous review is over. The
  pages embed each demo's video instead, with the recording linked beside
  it for `rerun`; the embeds resolve their paths with mdBook's
  `path_to_root`, since each is included from two depths.
- Nothing drawn at the chase camera may be visible: the viewer's eye eases
  toward the tracked entity and sits a touch behind it, so a 1 cm frustum
  or the axes a camera gets by default hang in front of the eye the size
  of the view. The pinhole is transparent with no line width and its
  `TransformAxes3D` length is zero.
- With a terrain window the map inset covers the window too, so a runway
  circuit is not a dot on a 10 km map.
- The flight preset plots height above the ground when the run has a
  terrain (the viewer keeps a copy of the heights), so a crash reaches
  zero and a circuit at 60 m reads 60; altitude above the origin stays
  for runs without one.
- A demo can turn the featured entities' map labels off (`labels: false`)
  where they would sit on labelled marks or on each other; the landmarks
  of the spread task are drawn prominent, so they are what is labelled.
- The viewer's eye glides from its default pose to the tracked camera
  over about a second of real time, one step per drawn frame, so the
  capture draws and discards frames at time zero until it has arrived;
  otherwise every follow-camera video opens with the eye flying in.
- The headless viewer's screenshot is the window's pixels whatever scale is
  asked for, and its UI zoom (the viewer's own, through its command
  palette) scales the text inside that window; the top bar hides through
  the palette too, and the collapsed play bar is cropped off.

## Review of the first videos

Watched frame by frame on 2026-11-18, each video has the same faults and a
few of its own. The fixes below are applied one demo at a time, each
regenerated and watched again before the next.

Faults shared by all six:

- Viewer chrome fills the frame: the Rerun top bar, each view's title bar
  with its three icons, and the play bar. Fix: the capture hides the top
  panel through the viewer's command palette and crops the collapsed play
  bar off; the views' title bars stay (the blueprint cannot hide them).
- Text is small: the captions are a 13 point paragraph in a side box, the
  plot labels smaller. Fix: capture at 1920 by 1080 with the viewer zoomed
  to 1.5, and lay the captions across the full width under the views
  instead of in the side column.
- The 3D views float a grid at z = 0 across the terrain and robots, with a
  black sky behind. Fix: the blueprint turns the 3D grid off and sets a sky
  color.
- The chase camera's drawn frustum flickers at the edges of the frame. Fix:
  its image plane sits inside the near clip plane; the second pass found
  it still there, the size of the view, and made it transparent with no
  line width and no axes (see "Found while building").
- The chase camera turns with the aircraft's yaw, which jitters, so the
  whole landscape shivers. Fix: the camera is a rope: its position eases
  toward the spot behind and above the entity, along its direction of
  travel, with a 0.6 s time constant, and it looks at the entity from
  wherever it is.
- A followed or featured entity's 3D label sits on top of its model. Fix:
  demos label featured entities on the map only.
- The altitude and error plots rescale as the run goes. Fix: the plots
  show the whole recording's time range, so the axes are fixed.

Per demo:

- terrain-flight: the aircraft is a red cross at 500 m, the camera spins
  with it, and the crashes the captions promise happen off screen. Fix:
  follow `climber` at 250 m with a flatter pitch as it climbs out of the
  valley over the ridge, with `ridge_crash` and `spiral_down` featured so
  their trails end where they hit; captions moved to those moments.
- belief-fixes: the eye is 150 m from two robots a few metres across, on a
  dark green grid. Fix: follow the Kalman robot at 30 m, so the ellipsoid,
  the fix markers and the perfect robot beside it fill the view; the map
  keeps the whole square.
- fleet-incident: the couriers hop (the frame stride was read against the
  wrong step); the 3D view is a useless green plane; the hubs fill a third
  of the map; nothing marks the surges. Fix: every twentieth 0.1 s step
  (thirty a second at sixty times), the fleet preset is the map and
  captions only, the map's padding shrinks to 0.55 of the span, courier
  points grow, and the demo file's `highlights` ring the surge hubs for
  the surge's hour.
- team-spread: the heading arrows are 8 m long in a 10 m scene; the 3D
  view adds nothing; the team is in place after three seconds; the
  landmarks are dots. Fix: arrows the length of `model_span_m` in demos,
  the map-only fleet preset, half speed over the first six seconds at a
  0.01 s step, the landmarks drawn prominent and labelled with the
  agents' map labels off.
- runway-ardupilot and runway-px4: the ground is a brown smear (the aerial
  image is 5 m a pixel, shrunk to 15 m here, and the horizon is a black
  sky); the runway is invisible; the aircraft is small and labelled. Fix:
  a demo can window the terrain (`terrain_window`) around the circuit at
  full resolution instead of thinning the whole 10 km, with the map inset
  covering the window, the demo file's `runway` draws the strip and its
  centreline on the ground, the camera follows at 40 m with the model at
  6 m, the sky is blue, and the plot is height above the ground, so the
  circuit reads 60 m as the caption says.

A second pass over the regenerated videos found the frustum still drawn
(now the size of the view), a camera axis triad in the sky, the viewer's
eye gliding in over the first second of every follow-camera video, map
labels crowding and colliding at the corner, and the belief robots small
at 30 m; all fixed as above and under "Found while building", with the
belief camera at 18 m. The user then found the fleet preset's map small
in a sea of black: a square map in a sixteen-by-nine view over a caption
strip fills a third of the frame. The fleet preset is now the map as a
square the window's height, fitted exactly to its grid through the view's
visual bounds, with the captions in the column beside it.

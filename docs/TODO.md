# Roadmap

SCRIMMAGE-RS is at its initial release: in the tested configurations of the
retained models, it agrees with C++ SCRIMMAGE to floating-point rounding, subject
to documented differences. The [paper](../paper/README.md) shows how, and
`python3 reference/campaign.py` reruns every check. Intentional differences are
in [reference notes](REFERENCE_NOTES.md); usage is in the
[book](../book/src/SUMMARY.md).

Nothing below is required. Keep the C++ comparison passing after any
simulation change.

## Scope decisions

| Area | Decision |
| --- | --- |
| Mission input | XML is frozen for single runs; YAML gets templates and sweeps. |
| External integrations | None planned: no ROS, ArduPilot, or JSBSim. |
| GPU work | None planned; revisit Burn only if RQ1 supports learning. |
| Viewer | Keep Rerun. Terrain is an optional experiment. |
| C++ machinery excluded | Runtime plugin loading, legacy protobuf/string-map spawning, OpenCL, VTK. |

## Terrain experiment

C++ terrain was a display-only VTK mesh plus an unused random height-map
plugin. Try a GeoTIFF workflow instead:

- [ ] Offline script: download a bounding box (OpenTopography API, or a USGS
  3DEP tile), then `gdalwarp` it to a projection centered on the mission origin,
  so each cell is already meters east/north. GDAL stays out of the Rust build.
- [ ] Rust: load the grid read-only, with an interpolated `height_at(east, north)`
  that returns `None` off the grid, and log it once to Rerun as a static
  `Mesh3D`. Rerun's map view can't show custom tiles or elevation.
- [ ] First area: McMillan Airfield (C++ `mcmillan`), 10 × 10 km at 10 m,
  3DEP bare ground.

Open decisions: whether terrain affects the simulation or is display only;
what z = 0 means (C++ uses `altitude_origin`; existing missions treat it as the
ground); the file format Rust reads (raw grid plus header, or GeoTIFF via the
`tiff` crate); and a YAML-only `terrain:` key, since XML is frozen. Mapterhorn
PMTiles are an alternative source that moves reprojection into Rust.

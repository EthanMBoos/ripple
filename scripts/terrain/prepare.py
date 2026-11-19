#!/usr/bin/env python3
"""Cut a square of terrain around a mission origin into Ripple's format.

    python3 scripts/terrain/prepare.py NAME --origin LATITUDE LONGITUDE \\
        --altitude-m ALTITUDE --size-m SIZE --cell-m CELL \\
        --elevation SOURCE [SOURCE ...] [--imagery SOURCE [SOURCE ...]] [--out DIR]

Writes DIR/NAME.terrain.yaml (the header), DIR/NAME.f32 (heights as
little-endian 32-bit floats, row by row from north to south, in metres above
sea level), and with --imagery DIR/NAME.png (an aerial image of the same
square, drawn on the terrain in the viewer). crates/core/src/common/terrain.rs
reads them.

The square is SIZE metres across, centred on the origin, in a transverse
Mercator projection centred there, so each height sample sits at a whole
multiple of CELL metres east and north of the origin. ALTITUDE is the height
above sea level of local z = 0 (the mission's altitude origin).

A SOURCE is a GeoTIFF path or URL, or a name that finds the files covering the
square: usgs-1m and usgs-10m (USGS 3DEP lidar and 1/3 arc-second, bare ground,
US only), copernicus-30m (worldwide, includes trees and buildings), and for
imagery naip (US aerial photos, about 0.6 m). Later sources are layered on top
of earlier ones wherever they have data, so list coarse, complete sources
first and finer, patchy ones after. Sources finer than the output are averaged
down; coarser ones are interpolated. Cloud-optimized GeoTIFFs are read
remotely, fetching only the part inside the square.

Needs rasterio (`pip install rasterio`, which bundles GDAL); the simulator
does not. Example, McMillan Airfield (the C++ `mcmillan` terrain site):

    python3 scripts/terrain/prepare.py mcmillan --origin 35.721025 -120.767925 \\
        --altitude-m 300 --size-m 10000 --cell-m 10 \\
        --elevation usgs-10m usgs-1m --imagery naip --imagery-cell-m 5 \\
        --out missions/terrain
"""

import argparse
import json
import math
import urllib.parse
import urllib.request
import warnings
from pathlib import Path

import numpy as np
import rasterio
import rasterio.errors
from rasterio.enums import Resampling
from rasterio.transform import from_origin
from rasterio.warp import reproject, transform_bounds

NO_DATA = -9999.0


# --- Finding the files that cover an area (bounds are west, south, east, north in degrees).

def get_json(url, body=None):
    request = urllib.request.Request(url, data=body and json.dumps(body).encode(),
                                     headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(request, timeout=120) as response:
        return json.load(response)


def usgs(dataset):
    """USGS National Map products: the newest file for each footprint, oldest first."""
    def find(bounds):
        query = urllib.parse.urlencode({"datasets": dataset, "prodFormats": "GeoTIFF",
                                        "bbox": ",".join(map(str, bounds)), "max": 500})
        found = get_json(f"https://tnmaccess.nationalmap.gov/api/v1/products?{query}")
        if found["total"] > len(found["items"]):
            raise SystemExit(f"{dataset}: {found['total']} files cover this area, more than one "
                             "query returns; use a smaller area")
        newest = {}
        for item in found["items"]:
            footprint = json.dumps(item["boundingBox"], sort_keys=True)
            if footprint not in newest or item["publicationDate"] > newest[footprint]["publicationDate"]:
                newest[footprint] = item
        return [item["downloadURL"]
                for item in sorted(newest.values(), key=lambda item: item["publicationDate"])]
    return find


def copernicus(bounds):
    """Copernicus GLO-30 tiles on AWS, one per whole degree."""
    west, south, east, north = bounds
    urls = []
    for latitude in range(math.floor(south), math.floor(north) + 1):
        for longitude in range(math.floor(west), math.floor(east) + 1):
            tile = (f"Copernicus_DSM_COG_10_{'N' if latitude >= 0 else 'S'}{abs(latitude):02d}_00_"
                    f"{'E' if longitude >= 0 else 'W'}{abs(longitude):03d}_00_DEM")
            urls.append(f"https://copernicus-dem-30m.s3.amazonaws.com/{tile}/{tile}.tif")
    return urls


def naip(bounds):
    """NAIP imagery from Microsoft Planetary Computer: the newest photo of each
    quarter-quadrangle, oldest first, signed for reading. States are flown in
    different years, so an area crossing a state line mixes years."""
    stac = "https://planetarycomputer.microsoft.com/api"
    found = get_json(f"{stac}/stac/v1/search",
                     {"collections": ["naip"], "bbox": list(bounds), "limit": 1000})
    if any(link["rel"] == "next" for link in found["links"]):
        raise SystemExit("naip: more images cover this area than one query returns; "
                         "use a smaller area")
    items = found["features"]
    newest = {}
    for item in items:
        # An id like ca_m_3512018_sw_10_060_20220526 names quarter-quad 3512018_sw.
        quad = "_".join(item["id"].split("_")[2:4])
        if quad not in newest or item["properties"]["datetime"] > newest[quad]["properties"]["datetime"]:
            newest[quad] = item
    token = get_json(f"{stac}/sas/v1/token/naip")["token"]
    return [f"{item['assets']['image']['href']}?{token}"
            for item in sorted(newest.values(), key=lambda item: item["properties"]["datetime"])]


NAMED_SOURCES = {
    "usgs-1m": usgs("Digital Elevation Model (DEM) 1 meter"),
    "usgs-10m": usgs("National Elevation Dataset (NED) 1/3 arc-second"),
    "copernicus-30m": copernicus,
    "naip": naip,
}


def files(sources, bounds):
    found = []
    for source in sources:
        found += NAMED_SOURCES[source](bounds) if source in NAMED_SOURCES else [source]
    return found


# --- Layering sources onto the output grid.

def metres_per_pixel(dataset):
    size = abs(dataset.transform.a)
    return size * 111_320 if dataset.crs.is_geographic else size


def open_near(path, cell_m):
    """Open the file's coarsest stored overview that is still no coarser than
    `cell_m`, so a remote read fetches far less than the full resolution."""
    with rasterio.open(path) as full:
        size_m, factors = metres_per_pixel(full), full.overviews(1)
    usable = [index for index, factor in enumerate(factors) if size_m * factor <= cell_m]
    return rasterio.open(path, overview_level=usable[-1]) if usable else rasterio.open(path)


def layer(paths, grid, bands, dtype, no_data):
    """Reproject each file onto the grid in turn; later files cover earlier ones where they have data."""
    crs, transform, shape = grid
    cell_m = transform.a
    output = np.full((len(bands), *shape), no_data, dtype=dtype)
    for path in paths:
        print(f"  {path.split('?')[0]}", flush=True)
        with open_near(path, cell_m) as source:
            finer = metres_per_pixel(source) < cell_m
            piece = np.full_like(output, no_data)
            reproject(source=rasterio.band(source, bands), destination=piece,
                      dst_transform=transform, dst_crs=crs, dst_nodata=no_data,
                      src_nodata=source.nodata if source.nodata is not None else no_data,
                      resampling=Resampling.average if finer else Resampling.bilinear)
        covered = (piece != no_data).all(axis=0)
        output[:, covered] = piece[:, covered]
    return output


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("name")
    parser.add_argument("--origin", nargs=2, type=float, required=True,
                        metavar=("LATITUDE", "LONGITUDE"))
    parser.add_argument("--altitude-m", type=float, required=True,
                        help="height above sea level of local z = 0")
    parser.add_argument("--size-m", type=float, required=True, help="width of the square")
    parser.add_argument("--cell-m", type=float, required=True, help="height sample spacing")
    parser.add_argument("--elevation", nargs="+", required=True, metavar="SOURCE")
    parser.add_argument("--imagery", nargs="+", metavar="SOURCE")
    parser.add_argument("--imagery-cell-m", type=float, help="pixel size (default --cell-m)")
    parser.add_argument("--out", type=Path, default=Path("missions/terrain"))
    args = parser.parse_args()

    latitude, longitude = args.origin
    local_frame = (f"+proj=tmerc +lat_0={latitude} +lon_0={longitude} +k=1 "
                   "+x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs")
    samples = round(args.size_m / args.cell_m) + 1
    half_m = (samples - 1) * args.cell_m / 2
    # Samples are cell centres, so the grid's outer edge is half a cell past them.
    edge_m = half_m + args.cell_m / 2
    bounds = transform_bounds(local_frame, "EPSG:4326", -edge_m, -edge_m, edge_m, edge_m)
    args.out.mkdir(parents=True, exist_ok=True)

    print("elevation:")
    elevation = files(args.elevation, bounds)
    heights = layer(elevation, (local_frame, from_origin(-edge_m, edge_m, args.cell_m, args.cell_m),
                                (samples, samples)), [1], np.float32, NO_DATA)[0]
    missing = int((heights == NO_DATA).sum())
    if missing:
        raise SystemExit(f"{missing} height samples have no data; add a source that covers "
                         "them or use a smaller area")
    heights.astype("<f4").tofile(args.out / f"{args.name}.f32")

    imagery_line = ""
    if args.imagery:
        print("imagery:")
        imagery_cell_m = args.imagery_cell_m or args.cell_m
        pixels = round(2 * edge_m / imagery_cell_m)
        image = layer(files(args.imagery, bounds),
                      (local_frame, from_origin(-edge_m, edge_m, imagery_cell_m, imagery_cell_m),
                       (pixels, pixels)), [1, 2, 3], np.uint8, 0)
        # A PNG has no coordinates; the header says where it lies.
        warnings.filterwarnings("ignore", category=rasterio.errors.NotGeoreferencedWarning)
        with rasterio.Env(GDAL_PAM_ENABLED="NO"):
            with rasterio.open(args.out / f"{args.name}.png", "w", driver="PNG", width=pixels,
                               height=pixels, count=3, dtype="uint8") as png:
                png.write(image)
        imagery_line = f"imagery: {args.name}.png\n"
        print(f"  {pixels} x {pixels} pixels at {imagery_cell_m:g} m")

    sources = " + ".join(args.elevation) + (f"; imagery {' + '.join(args.imagery)}" if args.imagery else "")
    (args.out / f"{args.name}.terrain.yaml").write_text(f"""\
# Written by scripts/terrain/prepare.py; see crates/core/src/common/terrain.rs.
# Elevation files, in layering order:
{"".join(f"#   {path.split('?')[0]}{chr(10)}" for path in elevation)}\
format_version: 1
source: "{sources} (GDAL {rasterio.__gdal_version__})"
origin: {{latitude_deg: {latitude}, longitude_deg: {longitude}, altitude_m: {args.altitude_m:g}}}
west_m: {-half_m:g}
north_m: {half_m:g}
cell_m: {args.cell_m:g}
columns: {samples}
rows: {samples}
heights: {args.name}.f32
{imagery_line}no_data: {NO_DATA:g}
""")
    print(f"wrote {args.out / args.name}.*: {samples} x {samples} samples, "
          f"heights {heights.min():.1f} to {heights.max():.1f} m")


if __name__ == "__main__":
    main()

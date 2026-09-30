//! Draw the mission's terrain once: a mesh in the 3D view, textured with the
//! terrain's aerial image if it has one and coloured by height otherwise, and
//! shaded relief under the 2D map. Display only; the simulation reads
//! `Terrain` directly.
use anyhow::{Context, Result, ensure};
use rerun::RecordingStream;
use scrimmage_core::Terrain;
use std::{fs::File, io::BufReader, path::Path};

/// At most this many mesh samples per side; the mesh skips samples to fit.
/// A 1001 x 1001 grid (10 km at 10 m) is drawn whole: about 1M vertices,
/// adding roughly 30 MB to the recording.
const MAX_MESH_SAMPLES: usize = 1024;

pub(super) fn log(recording: &RecordingStream, terrain: &Terrain) -> Result<()> {
    let (low_m, high_m) = height_range(terrain);
    log_mesh(recording, terrain, low_m, high_m)?;
    log_relief(recording, terrain, low_m, high_m)
}

fn height_range(terrain: &Terrain) -> (f64, f64) {
    let (mut low_m, mut high_m) = (f64::INFINITY, f64::NEG_INFINITY);
    for row in 0..terrain.rows() {
        for column in 0..terrain.columns() {
            let height_m = terrain.sample(row, column)[2];
            if height_m.is_finite() {
                low_m = low_m.min(height_m);
                high_m = high_m.max(height_m);
            }
        }
    }
    (low_m, high_m.max(low_m + 1.0))
}

/// Low ground green, middle tan, high ground pale.
fn color(height_m: f64, low_m: f64, high_m: f64) -> [f64; 3] {
    let stops = [
        [72.0, 112.0, 64.0],
        [168.0, 146.0, 98.0],
        [226.0, 220.0, 204.0],
    ];
    let fraction = ((height_m - low_m) / (high_m - low_m)).clamp(0.0, 1.0) * 2.0;
    let (from, to, t) = if fraction < 1.0 {
        (stops[0], stops[1], fraction)
    } else {
        (stops[1], stops[2], fraction - 1.0)
    };
    [0, 1, 2].map(|channel| from[channel] + (to[channel] - from[channel]) * t)
}

/// The upward surface normal at a sample, from its neighbours' heights.
fn normal(terrain: &Terrain, row: usize, column: usize, step: usize) -> [f64; 3] {
    let height = |row: usize, column: usize| {
        let row = row.min(terrain.rows() - 1);
        let column = column.min(terrain.columns() - 1);
        let height_m = terrain.sample(row, column)[2];
        if height_m.is_finite() { height_m } else { 0.0 }
    };
    let span_m = terrain.cell_m() * (2 * step) as f64;
    let east_slope =
        (height(row, column + step) - height(row, column.saturating_sub(step))) / span_m;
    // Rows run from north to south.
    let north_slope =
        (height(row.saturating_sub(step), column) - height(row + step, column)) / span_m;
    let length = (east_slope * east_slope + north_slope * north_slope + 1.0).sqrt();
    [-east_slope / length, -north_slope / length, 1.0 / length]
}

fn log_mesh(recording: &RecordingStream, terrain: &Terrain, low_m: f64, high_m: f64) -> Result<()> {
    let step = terrain
        .rows()
        .max(terrain.columns())
        .div_ceil(MAX_MESH_SAMPLES);
    let rows: Vec<usize> = (0..terrain.rows()).step_by(step).collect();
    let columns: Vec<usize> = (0..terrain.columns()).step_by(step).collect();
    let (mut positions, mut normals, mut colors) = (Vec::new(), Vec::new(), Vec::new());
    for &row in &rows {
        for &column in &columns {
            let [east_m, north_m, height_m] = terrain.sample(row, column);
            let height_m = if height_m.is_finite() {
                height_m
            } else {
                low_m
            };
            positions.push([east_m as f32, north_m as f32, height_m as f32]);
            normals.push(normal(terrain, row, column, step).map(|value| value as f32));
            colors.push(color(height_m, low_m, high_m).map(|value| value as u8));
        }
    }
    let mut triangles = Vec::new();
    let width = columns.len() as u32;
    for row in 0..rows.len() as u32 - 1 {
        for column in 0..width - 1 {
            let north_west = row * width + column;
            let south_west = north_west + width;
            triangles.push([north_west, south_west, north_west + 1]);
            triangles.push([north_west + 1, south_west, south_west + 1]);
        }
    }
    let mesh = rerun::Mesh3D::new(positions.iter().copied())
        .with_vertex_normals(normals)
        .with_triangle_indices(triangles);
    let mesh = match terrain.imagery() {
        Some(path) => {
            let (width, height, pixels) = read_rgb_png(path)?;
            // The image covers the grid's cells: half a cell past the outer samples.
            let [west_m, north_m, _] = terrain.sample(0, 0);
            let half_cell_m = terrain.cell_m() / 2.0;
            let size_m = terrain.cell_m() * terrain.columns() as f64;
            let texcoords = positions.iter().map(|&[east_m, north_m_vertex, _]| {
                [
                    ((f64::from(east_m) - west_m + half_cell_m) / size_m) as f32,
                    ((north_m - f64::from(north_m_vertex) + half_cell_m) / size_m) as f32,
                ]
            });
            mesh.with_vertex_texcoords(texcoords)
                .with_albedo_texture(rerun::datatypes::ImageFormat::rgb8([width, height]), pixels)
        }
        None => mesh.with_vertex_colors(colors),
    };
    recording.log_static("world/terrain", &mesh)?;
    Ok(())
}

/// An 8-bit RGB PNG's width, height, and pixels.
fn read_rgb_png(path: &Path) -> Result<(u32, u32, Vec<u8>)> {
    let file = File::open(path).with_context(|| format!("read {}", path.display()))?;
    let mut reader = png::Decoder::new(BufReader::new(file)).read_info()?;
    let mut pixels = vec![0; reader.output_buffer_size().context("PNG too large")?];
    let frame = reader.next_frame(&mut pixels)?;
    ensure!(
        frame.color_type == png::ColorType::Rgb && frame.bit_depth == png::BitDepth::Eight,
        "{} must be 8-bit RGB",
        path.display()
    );
    pixels.truncate(frame.buffer_size());
    Ok((frame.width, frame.height, pixels))
}

/// Height colours darkened on slopes facing away from a north-west light.
fn log_relief(
    recording: &RecordingStream,
    terrain: &Terrain,
    low_m: f64,
    high_m: f64,
) -> Result<()> {
    let light = {
        let [x, y, z]: [f64; 3] = [-1.0, 1.0, 1.4];
        let length = (x * x + y * y + z * z).sqrt();
        [x / length, y / length, z / length]
    };
    let mut pixels = Vec::with_capacity(terrain.rows() * terrain.columns() * 3);
    for row in 0..terrain.rows() {
        for column in 0..terrain.columns() {
            let height_m = terrain.sample(row, column)[2];
            if !height_m.is_finite() {
                pixels.extend([0, 0, 0]);
                continue;
            }
            let [x, y, z] = normal(terrain, row, column, 1);
            let shade = 0.45 + 0.55 * (x * light[0] + y * light[1] + z * light[2]).max(0.0);
            // Muted and darkened, so agents and tracks stand out on the map.
            pixels.extend(
                color(height_m, low_m, high_m)
                    .map(|value| ((value * 0.5 + 60.0) * shade * 0.7) as u8),
            );
        }
    }
    // Map coordinates are (east, -north); image pixel (0, 0) covers the unit
    // square past its corner, so shift by half a pixel to centre each sample.
    let [west_m, north_m, _] = terrain.sample(0, 0);
    let pixel_m = terrain.cell_m();
    recording.log_static(
        "map/terrain",
        &rerun::Transform3D::from_translation_scale(
            [
                (west_m - pixel_m / 2.0) as f32,
                (-north_m - pixel_m / 2.0) as f32,
                0.0,
            ],
            rerun::Scale3D::uniform(pixel_m as f32),
        ),
    )?;
    recording.log_static(
        "map/terrain",
        &rerun::Image::from_rgb24(pixels, [terrain.columns() as u32, terrain.rows() as u32]),
    )?;
    Ok(())
}

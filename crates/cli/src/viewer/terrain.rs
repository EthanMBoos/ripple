//! Draw the mission's terrain once: a mesh in the 3D view, textured with the
//! terrain's aerial image if it has one and colored by height otherwise, and
//! shaded relief under the 2D map. Display only; the simulation reads
//! `Terrain` directly.
use crate::demo::{Runway, TerrainWindow};
use anyhow::{Context, Result, ensure};
use rerun::RecordingStream;
use ripple_core::Terrain;
use std::{fs::File, io::BufReader, path::Path};

/// At most this many mesh samples per side; the mesh skips samples to fit.
/// A 1001 x 1001 grid (10 km at 10 m) is drawn whole: about 1M vertices,
/// which with its aerial image adds roughly 57 MB to the recording. A demo
/// asks for fewer (`terrain_samples` in its demo file).
pub(super) const MAX_MESH_SAMPLES: usize = 1024;

/// Draw the terrain with at most `max_samples` samples per side: the mesh
/// keeps every nth sample, and the images shrink to match, so the recording
/// holds no more image than the mesh can show. With a window, the mesh and
/// its image cover only that square, so a small window is drawn at full
/// resolution; the map's relief always covers the whole terrain.
/// Draw the terrain, or the demo's window of it, as the 3D mesh and the
/// map's relief; returns the drawn extent as [west, east, south, north] in
/// metres.
pub(super) fn log(
    recording: &RecordingStream,
    terrain: &Terrain,
    max_samples: usize,
    window: Option<TerrainWindow>,
) -> Result<[f64; 4]> {
    let (low_m, high_m) = height_range(terrain);
    let whole = Samples::whole(terrain);
    let shown = window
        .map_or(whole, |window| Samples::window(terrain, &window))
        .thinned(max_samples);
    log_mesh(recording, terrain, low_m, high_m, &shown)?;
    log_relief(recording, terrain, low_m, high_m, &shown)?;
    let [west_m, north_m, _] = terrain.sample(shown.rows[0], shown.columns[0]);
    let [east_m, south_m, _] = terrain.sample(shown.rows[1], shown.columns[1]);
    Ok([west_m, east_m, south_m, north_m])
}

/// The terrain's heights kept by the viewer, for height above ground: the
/// simulation owns the `Terrain`, and the viewer outlives its borrow.
pub(super) struct Ground {
    west_m: f64,
    north_m: f64,
    cell_m: f64,
    rows: usize,
    columns: usize,
    heights_m: Vec<f64>,
}

impl Ground {
    pub(super) fn from(terrain: &Terrain) -> Self {
        let [west_m, north_m, _] = terrain.sample(0, 0);
        let heights_m = (0..terrain.rows())
            .flat_map(|row| (0..terrain.columns()).map(move |column| (row, column)))
            .map(|(row, column)| terrain.sample(row, column)[2])
            .collect();
        Self {
            west_m,
            north_m,
            cell_m: terrain.cell_m(),
            rows: terrain.rows(),
            columns: terrain.columns(),
            heights_m,
        }
    }

    /// The ground height between the four surrounding samples, or none off
    /// the grid or beside a missing sample.
    pub(super) fn height_at(&self, east_m: f64, north_m: f64) -> Option<f64> {
        let column = (east_m - self.west_m) / self.cell_m;
        let row = (self.north_m - north_m) / self.cell_m;
        if column < 0.0 || row < 0.0 {
            return None;
        }
        let (column_0, row_0) = (column.floor() as usize, row.floor() as usize);
        if column_0 + 1 >= self.columns || row_0 + 1 >= self.rows {
            return None;
        }
        let at = |row: usize, column: usize| self.heights_m[row * self.columns + column];
        let (east_weight, north_weight) = (column - column_0 as f64, row - row_0 as f64);
        let top = at(row_0, column_0) * (1.0 - east_weight) + at(row_0, column_0 + 1) * east_weight;
        let bottom = at(row_0 + 1, column_0) * (1.0 - east_weight)
            + at(row_0 + 1, column_0 + 1) * east_weight;
        let height_m = top * (1.0 - north_weight) + bottom * north_weight;
        height_m.is_finite().then_some(height_m)
    }
}

/// The rows and columns of the terrain grid the mesh keeps, every `step`th
/// of a range.
#[derive(Clone, Copy)]
struct Samples {
    rows: [usize; 2],
    columns: [usize; 2],
    step: usize,
}

impl Samples {
    fn whole(terrain: &Terrain) -> Self {
        Self {
            rows: [0, terrain.rows() - 1],
            columns: [0, terrain.columns() - 1],
            step: 1,
        }
    }

    /// The samples inside the window, clipped to the grid.
    fn window(terrain: &Terrain, window: &TerrainWindow) -> Self {
        let [west_m, north_m, _] = terrain.sample(0, 0);
        let [east_center_m, north_center_m] = window.center_m;
        let half_m = window.half_span_m;
        let column = |east_m: f64| {
            (((east_m - west_m) / terrain.cell_m()).round().max(0.0) as usize)
                .min(terrain.columns() - 1)
        };
        let row = |north_at_m: f64| {
            (((north_m - north_at_m) / terrain.cell_m()).round().max(0.0) as usize)
                .min(terrain.rows() - 1)
        };
        Self {
            rows: [row(north_center_m + half_m), row(north_center_m - half_m)],
            columns: [
                column(east_center_m - half_m),
                column(east_center_m + half_m),
            ],
            step: 1,
        }
    }

    fn thinned(self, max_samples: usize) -> Self {
        let count = (self.rows[1] - self.rows[0]).max(self.columns[1] - self.columns[0]) + 1;
        Self {
            step: count.div_ceil(max_samples.max(2)).max(1),
            ..self
        }
    }

    fn row_indices(&self) -> Vec<usize> {
        (self.rows[0]..=self.rows[1]).step_by(self.step).collect()
    }

    fn column_indices(&self) -> Vec<usize> {
        (self.columns[0]..=self.columns[1])
            .step_by(self.step)
            .collect()
    }
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

fn log_mesh(
    recording: &RecordingStream,
    terrain: &Terrain,
    low_m: f64,
    high_m: f64,
    samples: &Samples,
) -> Result<()> {
    let step = samples.step;
    let rows = samples.row_indices();
    let columns = samples.column_indices();
    ensure!(
        rows.len() >= 2 && columns.len() >= 2,
        "the terrain window holds fewer than two samples a side"
    );
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
            // The image covers the grid's cells: half a cell past the outer
            // samples. Keep the part under the kept samples, then two
            // texels per kept sample, which are all the mesh can show.
            let pixels_per_cell = f64::from(width) / terrain.columns() as f64;
            let crop = |first: usize, last: usize, limit: u32| {
                let from = ((first as f64 * pixels_per_cell).floor() as u32).min(limit - 1);
                let to =
                    (((last + 1) as f64 * pixels_per_cell).ceil() as u32).clamp(from + 1, limit);
                [from, to]
            };
            let x = crop(columns[0], columns[columns.len() - 1], width);
            let y = crop(rows[0], rows[rows.len() - 1], height);
            let (width, height, pixels) = crop_rgb(width, &pixels, x, y);
            let shrink = (width as usize / (2 * columns.len())).max(1);
            let (width, height, pixels) = shrink_rgb(width, height, &pixels, shrink);
            let [west_m, north_m, _] = terrain.sample(0, 0);
            let half_cell_m = terrain.cell_m() / 2.0;
            let texcoords = positions.iter().map(|&[east_m, north_m_vertex, _]| {
                let column = (f64::from(east_m) - west_m + half_cell_m) / terrain.cell_m();
                let row = (north_m - f64::from(north_m_vertex) + half_cell_m) / terrain.cell_m();
                [
                    ((column * pixels_per_cell - f64::from(x[0])) / f64::from(x[1] - x[0])) as f32,
                    ((row * pixels_per_cell - f64::from(y[0])) / f64::from(y[1] - y[0])) as f32,
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

/// The pixels of columns `x[0]..x[1]` and rows `y[0]..y[1]` of an RGB image
/// `width` pixels wide, with the crop's width and height.
fn crop_rgb(width: u32, pixels: &[u8], x: [u32; 2], y: [u32; 2]) -> (u32, u32, Vec<u8>) {
    let (width, x, y) = (width as usize, x.map(|v| v as usize), y.map(|v| v as usize));
    let mut out = Vec::with_capacity((x[1] - x[0]) * (y[1] - y[0]) * 3);
    for row in y[0]..y[1] {
        out.extend_from_slice(&pixels[(row * width + x[0]) * 3..(row * width + x[1]) * 3]);
    }
    ((x[1] - x[0]) as u32, (y[1] - y[0]) as u32, out)
}

/// A runway on the ground: a dark strip following the terrain's height,
/// with a dashed centreline. Display only; the aircraft rolls on the
/// terrain itself.
pub(super) fn log_runway(
    recording: &RecordingStream,
    terrain: &Terrain,
    runway: &Runway,
) -> Result<()> {
    const ABOVE_GROUND_M: f64 = 0.15;
    const SLICE_M: f64 = 10.0;
    const DASH_M: f64 = 30.0;
    const GAP_M: f64 = 20.0;
    let heading_rad = runway.heading_deg.to_radians();
    let along = [heading_rad.sin(), heading_rad.cos()];
    let across = [heading_rad.cos(), -heading_rad.sin()];
    let [center_east_m, center_north_m] = runway.center_m;
    let at = |along_m: f64, across_m: f64| {
        let east_m = center_east_m + along[0] * along_m + across[0] * across_m;
        let north_m = center_north_m + along[1] * along_m + across[1] * across_m;
        [
            east_m as f32,
            north_m as f32,
            (terrain.ground_height(east_m, north_m) + ABOVE_GROUND_M) as f32,
        ]
    };
    let slices = (runway.length_m / SLICE_M).ceil().max(1.0) as usize;
    let half_length_m = runway.length_m / 2.0;
    let half_width_m = runway.width_m / 2.0;
    let mut vertices = Vec::with_capacity((slices + 1) * 2);
    for slice in 0..=slices {
        let along_m = -half_length_m + runway.length_m * slice as f64 / slices as f64;
        vertices.push(at(along_m, -half_width_m));
        vertices.push(at(along_m, half_width_m));
    }
    let triangles: Vec<[u32; 3]> = (0..slices as u32)
        .flat_map(|slice| {
            let left = slice * 2;
            [[left, left + 2, left + 1], [left + 1, left + 2, left + 3]]
        })
        .collect();
    recording.log_static(
        "world/runway",
        &rerun::Mesh3D::new(vertices)
            .with_triangle_indices(triangles)
            .with_vertex_normals(vec![[0.0, 0.0, 1.0]; (slices + 1) * 2])
            .with_albedo_factor(rerun::Rgba32::from_rgb(66, 66, 70)),
    )?;
    let mut dashes = Vec::new();
    let mut along_m = -half_length_m + GAP_M;
    while along_m + DASH_M <= half_length_m {
        dashes.push(vec![at(along_m, 0.0), at(along_m + DASH_M, 0.0)]);
        along_m += DASH_M + GAP_M;
    }
    recording.log_static(
        "world/runway/centreline",
        &rerun::LineStrips3D::new(dashes)
            .with_colors([[235, 235, 235]])
            .with_radii([rerun::Radius::new_scene_units(0.4)]),
    )?;
    Ok(())
}

/// The image at a `factor`th of its width and height, each pixel the mean
/// of the block it replaces; the same image for a factor of one.
fn shrink_rgb(width: u32, height: u32, pixels: &[u8], factor: usize) -> (u32, u32, Vec<u8>) {
    if factor <= 1 {
        return (width, height, pixels.to_vec());
    }
    let (width, height) = (width as usize, height as usize);
    let (out_width, out_height) = ((width / factor).max(1), (height / factor).max(1));
    let mut out = Vec::with_capacity(out_width * out_height * 3);
    for row in 0..out_height {
        for column in 0..out_width {
            let mut sum = [0u32; 3];
            for y in row * factor..(row + 1) * factor {
                for x in column * factor..(column + 1) * factor {
                    let at = (y * width + x) * 3;
                    for channel in 0..3 {
                        sum[channel] += u32::from(pixels[at + channel]);
                    }
                }
            }
            let count = (factor * factor) as u32;
            out.extend(sum.map(|value| (value / count) as u8));
        }
    }
    (out_width as u32, out_height as u32, out)
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

/// Height colors darkened on slopes facing away from a north-west light.
fn log_relief(
    recording: &RecordingStream,
    terrain: &Terrain,
    low_m: f64,
    high_m: f64,
    samples: &Samples,
) -> Result<()> {
    let step = samples.step;
    let light = {
        let [x, y, z]: [f64; 3] = [-1.0, 1.0, 1.4];
        let length = (x * x + y * y + z * z).sqrt();
        [x / length, y / length, z / length]
    };
    let rows = samples.row_indices();
    let columns = samples.column_indices();
    let mut pixels = Vec::with_capacity(rows.len() * columns.len() * 3);
    for &row in &rows {
        for &column in &columns {
            let height_m = terrain.sample(row, column)[2];
            if !height_m.is_finite() {
                pixels.extend([0, 0, 0]);
                continue;
            }
            let [x, y, z] = normal(terrain, row, column, step);
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
    let [west_m, north_m, _] = terrain.sample(rows[0], columns[0]);
    let pixel_m = terrain.cell_m() * step as f64;
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
        &rerun::Image::from_rgb24(pixels, [columns.len() as u32, rows.len() as u32]),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Ground, crop_rgb, shrink_rgb};
    use ripple_core::Terrain;

    #[test]
    fn the_ground_matches_the_terrain_between_its_samples() {
        let terrain = Terrain::from_grid(
            0.0,
            20.0,
            10.0,
            3,
            vec![0.0, 10.0, 20.0, 0.0, 10.0, 20.0, 0.0, 10.0, 20.0],
        )
        .unwrap();
        let ground = Ground::from(&terrain);
        for (east_m, north_m) in [(5.0, 5.0), (12.5, 17.0), (0.0, 20.0)] {
            assert_eq!(
                ground.height_at(east_m, north_m),
                terrain.height_at(east_m, north_m)
            );
        }
        assert_eq!(ground.height_at(5.0, 5.0), Some(5.0));
        assert_eq!(ground.height_at(-1.0, 5.0), None);
        assert_eq!(ground.height_at(5.0, 25.0), None);
    }

    #[test]
    fn cropping_keeps_the_named_columns_and_rows() {
        let pixels: Vec<u8> = (0..4 * 3 * 3).map(|value| value as u8).collect();
        let (width, height, crop) = crop_rgb(4, &pixels, [1, 3], [1, 3]);
        assert_eq!((width, height), (2, 2));
        // Row 1 starts at pixel 4; its columns 1 and 2 are pixels 5 and 6.
        assert_eq!(&crop[..6], &pixels[5 * 3..7 * 3]);
        assert_eq!(&crop[6..], &pixels[9 * 3..11 * 3]);
    }

    #[test]
    fn shrinking_averages_each_block_and_a_factor_of_one_copies() {
        let pixels: Vec<u8> = (0..4 * 4 * 3).map(|value| (value % 7 * 30) as u8).collect();
        let (width, height, same) = shrink_rgb(4, 4, &pixels, 1);
        assert_eq!((width, height), (4, 4));
        assert_eq!(same, pixels);
        let (width, height, half) = shrink_rgb(4, 4, &pixels, 2);
        assert_eq!((width, height), (2, 2));
        assert_eq!(half.len(), 2 * 2 * 3);
        let block = [0usize, 1, 4, 5];
        for channel in 0..3 {
            let mean = block
                .iter()
                .map(|&pixel| u32::from(pixels[pixel * 3 + channel]))
                .sum::<u32>()
                / 4;
            assert_eq!(u32::from(half[channel]), mean);
        }
    }
}

//! A read-only height map: ground height at any east/north position.
//!
//! A mission names a terrain header (`terrain:` in YAML). The header describes
//! a grid of heights sampled every `cell_m` metres in the mission's local
//! frame, starting at the north-west corner, and names a raw file beside it
//! holding the heights as little-endian 32-bit floats, row by row from north to
//! south. It may also name an aerial image of the same square, which only the
//! viewer draws. `scripts/terrain/prepare.py` writes these from GeoTIFFs,
//! projected so each sample already sits at whole metres east and north of the
//! origin.
//!
//! Heights in the file are above sea level. Local `z = 0` is the header's
//! `origin.altitude_m`, as in C++ SCRIMMAGE's `altitude_origin`, so loading
//! subtracts it. Between samples, `height_at` interpolates from the four
//! surrounding ones; off the grid, or next to a missing sample, there is no
//! height.
//!
//! No C++ counterpart: C++ drew its mission terrain in the viewer only, and its
//! TerrainGenerator's random height map was used by no stock mission.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path, sync::Arc};

/// Where the grid's local frame is on the Earth.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeographicOrigin {
    pub latitude_deg: f64,
    pub longitude_deg: f64,
    /// Height above sea level of local `z = 0`.
    pub altitude_m: f64,
}

/// The header file's fields.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    format_version: u32,
    source: String,
    origin: GeographicOrigin,
    /// East coordinate of the first column of samples.
    west_m: f64,
    /// North coordinate of the first row of samples.
    north_m: f64,
    cell_m: f64,
    columns: usize,
    rows: usize,
    /// The heights file, relative to the header.
    heights: String,
    /// An RGB PNG covering the grid's cells, relative to the header; display only.
    imagery: Option<String>,
    /// The value marking a missing sample in the heights file.
    no_data: f32,
}

#[derive(Clone, Debug)]
pub struct Terrain {
    source: Arc<str>,
    origin: Option<GeographicOrigin>,
    imagery: Option<Arc<Path>>,
    west_m: f64,
    north_m: f64,
    cell_m: f64,
    columns: usize,
    rows: usize,
    /// Local heights (`z`), row by row from north to south; NaN where missing.
    /// Every field is shared or small, so cloning a `Terrain` never copies it.
    heights_m: Arc<[f32]>,
}

impl Terrain {
    /// Load a terrain header and the heights file it names.
    pub fn load(header_path: &Path) -> Result<Self> {
        let text = fs::read_to_string(header_path)
            .with_context(|| format!("read terrain {}", header_path.display()))?;
        let header: Header = serde_yaml_ng::from_str(&text)
            .with_context(|| format!("terrain {}", header_path.display()))?;
        ensure!(
            header.format_version == 1,
            "unsupported terrain format_version"
        );
        let directory = header_path.parent().unwrap_or(Path::new("."));
        let heights_path = directory.join(&header.heights);
        let bytes = fs::read(&heights_path)
            .with_context(|| format!("read terrain heights {}", heights_path.display()))?;
        ensure!(
            bytes.len() == header.columns * header.rows * 4,
            "{} holds {} bytes; {} x {} samples need {}",
            heights_path.display(),
            bytes.len(),
            header.columns,
            header.rows,
            header.columns * header.rows * 4
        );
        let heights_m = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|word| {
                let height_m = f32::from_le_bytes(*word);
                if height_m == header.no_data {
                    f32::NAN
                } else {
                    (f64::from(height_m) - header.origin.altitude_m) as f32
                }
            })
            .collect();
        let mut terrain = Self::from_grid(
            header.west_m,
            header.north_m,
            header.cell_m,
            header.columns,
            heights_m,
        )?;
        terrain.source = header.source.into();
        terrain.origin = Some(header.origin);
        terrain.imagery = header.imagery.map(|imagery| directory.join(imagery).into());
        Ok(terrain)
    }

    /// A terrain from local heights (`z`), row by row from north to south, with
    /// NaN for a missing sample. For synthetic terrain and tests.
    pub fn from_grid(
        west_m: f64,
        north_m: f64,
        cell_m: f64,
        columns: usize,
        heights_m: Vec<f32>,
    ) -> Result<Self> {
        ensure!(
            cell_m.is_finite() && cell_m > 0.0,
            "terrain cell_m must be positive"
        );
        ensure!(
            west_m.is_finite() && north_m.is_finite(),
            "terrain corner must be finite"
        );
        ensure!(
            columns >= 2
                && heights_m.len().is_multiple_of(columns)
                && heights_m.len() / columns >= 2,
            "terrain needs at least 2 x 2 samples in whole rows"
        );
        Ok(Self {
            source: "grid".into(),
            origin: None,
            imagery: None,
            west_m,
            north_m,
            cell_m,
            columns,
            rows: heights_m.len() / columns,
            heights_m: heights_m.into(),
        })
    }

    /// Ground height (local `z`) at a position, interpolated between the four
    /// surrounding samples. `None` off the grid or next to a missing sample.
    // TODO(terrain): the ground off the grid is TerrainCollision's
    // `off_terrain_z` today. Once motion models also touch the ground, it must
    // be shared, so collision and contact agree on one ground.
    pub fn height_at(&self, east_m: f64, north_m: f64) -> Option<f64> {
        let column = (east_m - self.west_m) / self.cell_m;
        let row = (self.north_m - north_m) / self.cell_m;
        let last_column = (self.columns - 1) as f64;
        let last_row = (self.rows - 1) as f64;
        if !(0.0..=last_column).contains(&column) || !(0.0..=last_row).contains(&row) {
            return None;
        }
        // On the last row or column, interpolate within the cell before it.
        let column0 = column.floor().min(last_column - 1.0);
        let row0 = row.floor().min(last_row - 1.0);
        let (east_fraction, south_fraction) = (column - column0, row - row0);
        let (column0, row0) = (column0 as usize, row0 as usize);
        let sample =
            |row: usize, column: usize| f64::from(self.heights_m[row * self.columns + column]);
        let north_edge = sample(row0, column0) * (1.0 - east_fraction)
            + sample(row0, column0 + 1) * east_fraction;
        let south_edge = sample(row0 + 1, column0) * (1.0 - east_fraction)
            + sample(row0 + 1, column0 + 1) * east_fraction;
        let height_m = north_edge * (1.0 - south_fraction) + south_edge * south_fraction;
        height_m.is_finite().then_some(height_m)
    }

    /// The sample at a row (from the north) and column (from the west): its
    /// east and north position and local height, NaN if missing.
    pub fn sample(&self, row: usize, column: usize) -> [f64; 3] {
        [
            self.west_m + column as f64 * self.cell_m,
            self.north_m - row as f64 * self.cell_m,
            f64::from(self.heights_m[row * self.columns + column]),
        ]
    }

    pub fn missing_samples(&self) -> usize {
        self.heights_m
            .iter()
            .filter(|height| height.is_nan())
            .count()
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    pub fn columns(&self) -> usize {
        self.columns
    }

    pub fn cell_m(&self) -> f64 {
        self.cell_m
    }

    /// Where the data came from, as the header states it.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// An aerial image covering the grid's cells (from half a cell outside the
    /// north-west sample to half a cell outside the south-east one), if any.
    pub fn imagery(&self) -> Option<&Path> {
        self.imagery.as_deref()
    }

    /// The header's origin; `None` for a terrain built with `from_grid`.
    pub fn origin(&self) -> Option<GeographicOrigin> {
        self.origin
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 3 x 3 samples 10 m apart, from (0, 20) in the north-west to (20, 0).
    fn slope() -> Terrain {
        // Height rises 1 m per metre east; the south-west sample is missing.
        let heights_m = vec![0.0, 10.0, 20.0, 0.0, 10.0, 20.0, f32::NAN, 10.0, 20.0];
        Terrain::from_grid(0.0, 20.0, 10.0, 3, heights_m).unwrap()
    }

    #[test]
    fn interpolates_between_samples_and_hits_samples_exactly() {
        let terrain = slope();
        assert_eq!(terrain.height_at(0.0, 20.0), Some(0.0));
        assert_eq!(terrain.height_at(20.0, 20.0), Some(20.0));
        assert_eq!(terrain.height_at(5.0, 15.0), Some(5.0));
        assert_eq!(terrain.height_at(17.5, 12.0), Some(17.5));
        // The last row and column are on the grid too.
        assert_eq!(terrain.height_at(20.0, 10.0), Some(20.0));
    }

    #[test]
    fn has_no_height_off_the_grid_or_beside_a_missing_sample() {
        let terrain = slope();
        assert_eq!(terrain.height_at(-0.1, 10.0), None);
        assert_eq!(terrain.height_at(20.1, 10.0), None);
        assert_eq!(terrain.height_at(10.0, 20.1), None);
        assert_eq!(terrain.height_at(10.0, -0.1), None);
        assert_eq!(terrain.height_at(3.0, 3.0), None);
        assert_eq!(terrain.height_at(5.0, 15.0), Some(5.0));
    }

    #[test]
    fn loads_heights_relative_to_the_origin_altitude() -> Result<()> {
        let directory = std::env::temp_dir().join(format!("terrain-test-{}", std::process::id()));
        fs::create_dir_all(&directory)?;
        let heights: Vec<u8> = [300.0_f32, 310.0, 320.0, 330.0]
            .iter()
            .flat_map(|height| height.to_le_bytes())
            .collect();
        fs::write(directory.join("area.f32"), heights)?;
        fs::write(
            directory.join("area.terrain.yaml"),
            "format_version: 1\nsource: test\n\
             origin: {latitude_deg: 35.0, longitude_deg: -120.0, altitude_m: 300}\n\
             west_m: -5\nnorth_m: 5\ncell_m: 10\ncolumns: 2\nrows: 2\n\
             heights: area.f32\nno_data: -9999\n",
        )?;
        let terrain = Terrain::load(&directory.join("area.terrain.yaml"))?;
        fs::remove_dir_all(&directory)?;
        assert_eq!(terrain.height_at(-5.0, 5.0), Some(0.0));
        assert_eq!(terrain.height_at(5.0, 5.0), Some(10.0));
        assert_eq!(terrain.height_at(0.0, 0.0), Some(15.0));
        assert_eq!(
            terrain.origin().map(|origin| origin.altitude_m),
            Some(300.0)
        );
        Ok(())
    }
}

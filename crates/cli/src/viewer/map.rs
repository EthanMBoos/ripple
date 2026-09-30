//! Local ENU display coordinates and a padded, full-mission reference grid.
use anyhow::Result;
use rerun::RecordingStream;

/// Rerun's 2D canvas has downward-positive Y; simulation North must point up.
pub(super) fn project(position_world_m: [f32; 3]) -> [f32; 2] {
    [position_world_m[0], -position_world_m[1]]
}

struct Bounds {
    east_center_m: f32,
    north_center_m: f32,
    half_span_m: f32,
    grid_step_m: f32,
    altitude_min_m: f32,
}

impl Bounds {
    fn from_positions(positions: impl Iterator<Item = [f32; 3]>) -> Self {
        let mut east_min_m = f32::INFINITY;
        let mut east_max_m = f32::NEG_INFINITY;
        let mut north_min_m = f32::INFINITY;
        let mut north_max_m = f32::NEG_INFINITY;
        let mut altitude_min_m = f32::INFINITY;
        for [east_m, north_m, altitude_m] in positions {
            east_min_m = east_min_m.min(east_m);
            east_max_m = east_max_m.max(east_m);
            north_min_m = north_min_m.min(north_m);
            north_max_m = north_max_m.max(north_m);
            altitude_min_m = altitude_min_m.min(altitude_m);
        }
        if !east_min_m.is_finite() {
            return Self {
                east_center_m: 0.0,
                north_center_m: 0.0,
                half_span_m: 60.0,
                grid_step_m: 20.0,
                altitude_min_m: 0.0,
            };
        }
        let span_m = (east_max_m - east_min_m)
            .max(north_max_m - north_min_m)
            .max(40.0);
        Self {
            east_center_m: (east_min_m + east_max_m) * 0.5,
            north_center_m: (north_min_m + north_max_m) * 0.5,
            // Keep edge agents, headings and labels inside the fit at every replay time.
            half_span_m: span_m * 0.65 + 12.0,
            grid_step_m: grid_step_m(span_m),
            altitude_min_m,
        }
    }

    /// Exactly the terrain's square: [west, east, south, north] in metres.
    fn around_terrain([west_m, east_m, south_m, north_m]: [f32; 4]) -> Self {
        let span_m = (east_m - west_m).max(north_m - south_m);
        Self {
            east_center_m: (west_m + east_m) * 0.5,
            north_center_m: (south_m + north_m) * 0.5,
            half_span_m: span_m * 0.5,
            grid_step_m: grid_step_m(span_m),
            altitude_min_m: 0.0,
        }
    }
}

/// A round grid spacing giving about six lines across `span_m`.
fn grid_step_m(span_m: f32) -> f32 {
    let rough_step_m = span_m / 6.0;
    let decade_m = 10.0_f32.powf(rough_step_m.log10().floor());
    [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .find(|factor| factor * decade_m >= rough_step_m)
        .unwrap_or(10.0)
        * decade_m
}

/// With terrain ([west, east, south, north]), the grid covers exactly the
/// terrain, and the 3D reference plane is skipped: it would cut through hills.
pub(super) fn log_grid(
    recording: &RecordingStream,
    positions: impl Iterator<Item = [f32; 3]>,
    terrain_extent_m: Option<[f32; 4]>,
) -> Result<()> {
    let bounds =
        terrain_extent_m.map_or_else(|| Bounds::from_positions(positions), Bounds::around_terrain);
    let east_min_m = bounds.east_center_m - bounds.half_span_m;
    let east_max_m = bounds.east_center_m + bounds.half_span_m;
    let north_min_m = bounds.north_center_m - bounds.half_span_m;
    let north_max_m = bounds.north_center_m + bounds.half_span_m;
    let mut grid = Vec::new();
    let mut label_positions = Vec::new();
    let mut labels = Vec::new();
    let inset_m = bounds.half_span_m * 0.08;
    let mut east_m = (east_min_m / bounds.grid_step_m).ceil() * bounds.grid_step_m;
    while east_m < east_max_m {
        grid.push(vec![[east_m, -north_min_m], [east_m, -north_max_m]]);
        if east_m > east_min_m + inset_m && east_m < east_max_m - inset_m {
            label_positions.push([east_m, -north_min_m - inset_m]);
            labels.push(format!("E {east_m:.0} m"));
        }
        east_m += bounds.grid_step_m;
    }
    let mut north_m = (north_min_m / bounds.grid_step_m).ceil() * bounds.grid_step_m;
    while north_m < north_max_m {
        grid.push(vec![[east_min_m, -north_m], [east_max_m, -north_m]]);
        if north_m > north_min_m + inset_m && north_m < north_max_m - inset_m {
            label_positions.push([east_min_m + inset_m, -north_m]);
            labels.push(format!("N {north_m:.0} m"));
        }
        north_m += bounds.grid_step_m;
    }
    // A reference plane below the flight paths also pads the optional 3D fit.
    // This is a coordinate aid, not terrain or a simulated ground surface.
    let world_grid: Vec<Vec<[f32; 3]>> = grid
        .iter()
        .map(|line| {
            line.iter()
                .map(|[east_m, canvas_y_m]| [*east_m, -*canvas_y_m, bounds.altitude_min_m - 10.0])
                .collect()
        })
        .collect();
    if terrain_extent_m.is_none() {
        recording.log_static(
            "world/reference_grid",
            &rerun::LineStrips3D::new(world_grid)
                .with_colors([[55, 65, 75]])
                .with_radii([rerun::Radius::new_ui_points(0.5)]),
        )?;
    }
    recording.log_static(
        "map/grid",
        &rerun::LineStrips2D::new(grid)
            .with_colors([[40, 48, 58]])
            .with_radii([rerun::Radius::new_ui_points(0.5)]),
    )?;
    recording.log_static(
        "map/grid_labels",
        &rerun::Points2D::new(label_positions)
            .with_labels(labels)
            .with_colors([[145, 155, 165]])
            .with_radii([rerun::Radius::new_ui_points(0.0)]),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn north_is_up_and_full_run_extremes_have_label_padding() {
        assert_eq!(project([20.0, 30.0, 200.0]), [20.0, -30.0]);
        let bounds = Bounds::from_positions([[0.0, 0.0, 200.0], [2000.0, 0.0, 200.0]].into_iter());
        assert!(bounds.east_center_m - bounds.half_span_m < -100.0);
        assert!(bounds.east_center_m + bounds.half_span_m > 2100.0);
        let empty = Bounds::from_positions(std::iter::empty());
        assert!(empty.half_span_m.is_finite() && empty.half_span_m > 0.0);
    }
}

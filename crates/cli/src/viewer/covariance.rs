//! One-sigma ellipsoids for `--draw-covariance`: each entity's own position
//! uncertainty and that of each track it keeps, from the position block of a
//! belief's covariance. The 3D world gets an ellipsoid; the map gets the
//! ellipse of the east-north block. A zero covariance (a perfect belief)
//! draws nothing, and so does one with a non-finite entry, such as a
//! quantity an estimator reports as unknown.
use anyhow::Result;
use nalgebra::{Matrix2, Rotation3, SymmetricEigen, UnitQuaternion};
use rerun::RecordingStream;
use ripple_core::{Matrix3, Vec3, record::Belief};
use std::collections::BTreeSet;

use super::{map, team_color};

/// One uncertain position to draw.
pub(crate) struct Uncertain {
    /// Where it is drawn, under `world/` and `map/`.
    path: String,
    team_id: i32,
    /// A track is drawn fainter than an entity's own belief.
    is_track: bool,
    mean_world_m: Vec3,
    covariance_m2: Matrix3,
}

/// What to draw for one frame: the belief records the trace wrote for it,
/// read live from the sink or back from the record.
pub(crate) fn from_beliefs(beliefs: &[Belief]) -> Vec<Uncertain> {
    let mut items = Vec::new();
    for belief in beliefs {
        items.push(Uncertain {
            path: format!("beliefs/{}/own", belief.entity_id),
            team_id: belief.team_id,
            is_track: false,
            mean_world_m: belief.state.mean.position_world_m,
            covariance_m2: belief.state.covariance.position(),
        });
        for track in &belief.tracks {
            items.push(Uncertain {
                path: format!("beliefs/{}/tracks/{}", belief.entity_id, track.track_id),
                team_id: track.team_id.unwrap_or(belief.team_id),
                is_track: true,
                mean_world_m: track.state.mean.position_world_m,
                covariance_m2: track.state.covariance.position(),
            });
        }
    }
    items
}

/// The ellipsoids on screen, so one that is no longer drawn can be cleared.
#[derive(Default)]
pub(super) struct Covariances {
    drawn: BTreeSet<String>,
}

impl Covariances {
    /// Log every item at the recording's current time.
    pub(super) fn log(&mut self, recording: &RecordingStream, items: &[Uncertain]) -> Result<()> {
        let mut drawn = BTreeSet::new();
        for item in items {
            let Some(ellipsoid) = Ellipsoid::one_sigma(&item.covariance_m2) else {
                continue;
            };
            let [red, green, blue] = team_color(item.team_id);
            let color = [red, green, blue, if item.is_track { 110 } else { 220 }];
            let center = item.mean_world_m;
            let center = [center.x as f32, center.y as f32, center.z as f32];
            recording.log(
                format!("world/{}", item.path),
                &rerun::Ellipsoids3D::from_centers_and_half_sizes(
                    [center],
                    [ellipsoid.half_sizes_m],
                )
                .with_quaternions([rerun::Quaternion::from_xyzw(ellipsoid.rotation_xyzw)])
                .with_colors([color])
                .with_line_radii([rerun::Radius::new_ui_points(1.0)])
                .with_fill_mode(rerun::FillMode::MajorWireframe),
            )?;
            recording.log(
                format!("map/{}", item.path),
                &rerun::LineStrips2D::new([ellipse_map(center, &item.covariance_m2)])
                    .with_colors([color])
                    .with_radii([rerun::Radius::new_ui_points(1.0)]),
            )?;
            drawn.insert(item.path.clone());
        }
        for path in self.drawn.difference(&drawn) {
            recording.log(format!("world/{path}"), &rerun::Clear::flat())?;
            recording.log(format!("map/{path}"), &rerun::Clear::flat())?;
        }
        self.drawn = drawn;
        Ok(())
    }
}

/// A covariance's one-sigma ellipsoid: the square roots of its eigenvalues
/// along its eigenvectors.
struct Ellipsoid {
    half_sizes_m: [f32; 3],
    rotation_xyzw: [f32; 4],
}

impl Ellipsoid {
    /// `None` for a zero or non-finite covariance: nothing to draw.
    fn one_sigma(covariance_m2: &Matrix3) -> Option<Self> {
        if !covariance_m2.iter().all(|value| value.is_finite()) || covariance_m2.norm() == 0.0 {
            return None;
        }
        let eigen = SymmetricEigen::new(*covariance_m2);
        let mut axes = eigen.eigenvectors;
        // Eigenvectors are a rotation only if right-handed; flip one if not.
        if axes.determinant() < 0.0 {
            let flipped = -axes.column(0);
            axes.set_column(0, &flipped);
        }
        let rotation =
            UnitQuaternion::from_rotation_matrix(&Rotation3::from_matrix_unchecked(axes));
        let half_sizes_m = [0, 1, 2].map(|axis| eigen.eigenvalues[axis].max(0.0).sqrt() as f32);
        Some(Self {
            half_sizes_m,
            rotation_xyzw: [
                rotation.i as f32,
                rotation.j as f32,
                rotation.k as f32,
                rotation.w as f32,
            ],
        })
    }
}

/// The one-sigma ellipse of the east-north block, as map points.
fn ellipse_map(center: [f32; 3], covariance_m2: &Matrix3) -> Vec<[f32; 2]> {
    const POINTS: usize = 48;
    let block = Matrix2::new(
        covariance_m2[(0, 0)],
        covariance_m2[(0, 1)],
        covariance_m2[(1, 0)],
        covariance_m2[(1, 1)],
    );
    let eigen = SymmetricEigen::new(block);
    let [a_m, b_m] = [0, 1].map(|axis| eigen.eigenvalues[axis].max(0.0).sqrt());
    (0..=POINTS)
        .map(|index| {
            let angle = std::f64::consts::TAU * index as f64 / POINTS as f64;
            let local = nalgebra::Vector2::new(a_m * angle.cos(), b_m * angle.sin());
            let offset = eigen.eigenvectors * local;
            map::project([
                center[0] + offset.x as f32,
                center[1] + offset.y as f32,
                center[2],
            ])
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ellipsoid_axes_are_the_standard_deviations_along_the_eigenvectors() {
        // Variances 4 and 1 in the east-north plane, rotated 45 degrees, and 0.25 up.
        let c = std::f64::consts::FRAC_1_SQRT_2;
        let axes = Matrix3::new(c, -c, 0.0, c, c, 0.0, 0.0, 0.0, 1.0);
        let covariance =
            axes * Matrix3::from_diagonal(&Vec3::new(4.0, 1.0, 0.25)) * axes.transpose();
        let ellipsoid = Ellipsoid::one_sigma(&covariance).expect("drawable");
        let mut sizes = ellipsoid.half_sizes_m;
        sizes.sort_by(f32::total_cmp);
        assert_eq!(sizes, [0.5, 1.0, 2.0]);
        let [x, y, z, w] = ellipsoid.rotation_xyzw;
        assert!(((x * x + y * y + z * z + w * w) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_perfect_or_unknown_covariance_draws_nothing() {
        assert!(Ellipsoid::one_sigma(&Matrix3::zeros()).is_none());
        assert!(Ellipsoid::one_sigma(&(Matrix3::identity() * f64::INFINITY)).is_none());
    }

    #[test]
    fn the_map_ellipse_closes_and_reaches_one_sigma_east() {
        let points = ellipse_map(
            [10.0, 20.0, 0.0],
            &Matrix3::from_diagonal(&Vec3::new(9.0, 1.0, 1.0)),
        );
        assert_eq!(points.first(), points.last());
        let east_most = points.iter().map(|point| point[0]).fold(f32::MIN, f32::max);
        assert!((east_most - 13.0).abs() < 1e-5);
    }
}

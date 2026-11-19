//! Vehicle models for the 3D view: a fixed-wing aircraft, a quadrotor, and
//! a ground robot, each a few boxes in the body frame (forward, left, up)
//! with a wingspan or width of one, scaled by a transform where they are
//! drawn. Built here rather than loaded from files so the repository holds
//! no binary assets and every model matches its team's color.
use rerun::Mesh3D;

/// The model a mission's `visual_model` name gets, or `None` for a marker.
pub(super) fn mesh(visual_model: &str, color: [u8; 3]) -> Option<Mesh3D> {
    let name = visual_model.to_ascii_lowercase();
    let mut body = Body::default();
    if name == "sphere" || name.is_empty() {
        return None;
    } else if name.contains("quad") || name.contains("rotor") || name.contains("copter") {
        quadrotor(&mut body);
    } else if name.contains("rover") || name.contains("car") || name.contains("ground") {
        ground_robot(&mut body);
    } else {
        // "aircraft", "zephyr-blue", and the like.
        fixed_wing(&mut body);
    }
    Some(body.into_mesh(color))
}

/// A fuselage, a main wing, a tailplane, and a fin.
fn fixed_wing(body: &mut Body) {
    body.push_box([0.0, 0.0, 0.0], [0.7, 0.08, 0.1], 0.0);
    body.push_box([0.05, 0.0, 0.02], [0.14, 1.0, 0.02], 0.0);
    body.push_box([-0.3, 0.0, 0.03], [0.08, 0.35, 0.015], 0.0);
    body.push_box([-0.3, 0.0, 0.1], [0.08, 0.015, 0.14], 0.0);
}

/// A body, two crossed arms, and four rotor discs.
fn quadrotor(body: &mut Body) {
    body.push_box([0.0, 0.0, 0.0], [0.25, 0.25, 0.08], 0.0);
    let quarter_turn = std::f32::consts::FRAC_PI_4;
    body.push_box([0.0, 0.0, 0.0], [1.0, 0.04, 0.03], quarter_turn);
    body.push_box([0.0, 0.0, 0.0], [1.0, 0.04, 0.03], -quarter_turn);
    for (x, y) in [(0.35, 0.35), (0.35, -0.35), (-0.35, 0.35), (-0.35, -0.35)] {
        body.push_disc([x, y, 0.03], 0.15);
    }
}

/// A chassis on four wheels.
fn ground_robot(body: &mut Body) {
    body.push_box([0.0, 0.0, 0.3], [0.8, 0.5, 0.25], 0.0);
    for (x, y) in [(0.3, 0.3), (0.3, -0.3), (-0.3, 0.3), (-0.3, -0.3)] {
        body.push_box([x, y, 0.125], [0.25, 0.08, 0.25], 0.0);
    }
}

#[derive(Default)]
struct Body {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    triangles: Vec<[u32; 3]>,
}

impl Body {
    /// A box of this size centred here, turned `yaw_rad` about up; its faces
    /// have their own vertices so each is lit flat.
    fn push_box(&mut self, center: [f32; 3], size: [f32; 3], yaw_rad: f32) {
        let (sin, cos) = yaw_rad.sin_cos();
        let turn = |[x, y, z]: [f32; 3]| [x * cos - y * sin, x * sin + y * cos, z];
        let half = size.map(|side| side / 2.0);
        // Each face: its outward normal axis and sign, then its four corners
        // counter-clockwise seen from outside.
        for (axis, sign) in [
            (0, 1.0),
            (0, -1.0),
            (1, 1.0),
            (1, -1.0),
            (2, 1.0),
            (2, -1.0),
        ] {
            let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
            let mut corners = [[0.0f32; 3]; 4];
            for (corner, (su, sv)) in
                corners
                    .iter_mut()
                    .zip([(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)])
            {
                corner[axis] = sign * half[axis];
                corner[u] = su * half[u];
                corner[v] = sv * half[v];
            }
            if sign < 0.0 {
                corners.swap(1, 3);
            }
            let mut normal = [0.0; 3];
            normal[axis] = sign;
            self.push_face(
                corners.map(|corner| {
                    let [x, y, z] = turn(corner);
                    [x + center[0], y + center[1], z + center[2]]
                }),
                turn(normal),
            );
        }
    }

    /// A flat octagon facing up.
    fn push_disc(&mut self, center: [f32; 3], radius: f32) {
        let first = self.positions.len() as u32;
        self.positions.push(center);
        self.normals.push([0.0, 0.0, 1.0]);
        let sides = 8;
        for side in 0..sides {
            let angle = side as f32 / sides as f32 * std::f32::consts::TAU;
            self.positions.push([
                center[0] + radius * angle.cos(),
                center[1] + radius * angle.sin(),
                center[2],
            ]);
            self.normals.push([0.0, 0.0, 1.0]);
        }
        for side in 0..sides {
            let next = (side + 1) % sides;
            self.triangles
                .push([first, first + 1 + side, first + 1 + next]);
        }
    }

    fn push_face(&mut self, corners: [[f32; 3]; 4], normal: [f32; 3]) {
        let first = self.positions.len() as u32;
        self.positions.extend(corners);
        self.normals.extend([normal; 4]);
        self.triangles.push([first, first + 1, first + 2]);
        self.triangles.push([first, first + 2, first + 3]);
    }

    fn into_mesh(self, color: [u8; 3]) -> Mesh3D {
        let count = self.positions.len();
        Mesh3D::new(self.positions)
            .with_vertex_normals(self.normals)
            .with_triangle_indices(self.triangles)
            .with_vertex_colors(vec![color; count])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn built(name: &str) -> Body {
        let mut body = Body::default();
        match name {
            "wing" => fixed_wing(&mut body),
            "quad" => quadrotor(&mut body),
            _ => ground_robot(&mut body),
        }
        body
    }

    #[test]
    fn every_model_is_closed_triangles_within_a_unit_span() {
        for name in ["wing", "quad", "ground"] {
            let body = built(name);
            assert!(!body.triangles.is_empty(), "{name}");
            assert_eq!(body.positions.len(), body.normals.len(), "{name}");
            for triangle in &body.triangles {
                for &index in triangle {
                    assert!((index as usize) < body.positions.len(), "{name}");
                }
            }
            for position in &body.positions {
                assert!(position.iter().all(|value| value.abs() <= 0.51), "{name}");
            }
        }
    }

    #[test]
    fn names_choose_a_model_and_a_sphere_keeps_its_marker() {
        assert!(mesh("sphere", [0, 0, 0]).is_none());
        assert!(mesh("", [0, 0, 0]).is_none());
        for name in ["aircraft", "zephyr-blue", "Quadrotor", "rover"] {
            assert!(mesh(name, [1, 2, 3]).is_some(), "{name}");
        }
    }

    #[test]
    fn a_box_face_normal_points_away_from_its_centre() {
        let mut body = Body::default();
        body.push_box([1.0, 0.0, 0.0], [0.2, 0.2, 0.2], 0.0);
        for (position, normal) in body.positions.iter().zip(&body.normals) {
            let outward: f32 = (0..3)
                .map(|axis| (position[axis] - [1.0, 0.0, 0.0][axis]) * normal[axis])
                .sum();
            assert!(outward > 0.0);
        }
    }
}

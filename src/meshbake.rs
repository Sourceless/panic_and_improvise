//! Bakes simple solids (boxes, roofs, cylinders, cones) into one mesh with a colour on every vertex,
//! so that a whole building, however many pieces it has, is a single thing to draw.

use std::f32::consts::TAU;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

/// A colour as it is chosen, in sRGB (what the eye expects of 0.5 grey). Vertex colours are used as
/// linear by the renderer, so they are converted on the way in.
pub type Rgb = [f32; 3];

fn linear(c: Rgb) -> [f32; 4] {
    let l = |v: f32| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    [l(c[0]), l(c[1]), l(c[2]), 1.0]
}

#[derive(Default)]
pub struct MeshBuilder {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colours: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl MeshBuilder {
    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }

    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }

    /// A quad whose corners are given counter-clockwise as seen from the side `normal` points to.
    fn quad(&mut self, corners: [Vec3; 4], normal: Vec3, colour: Rgb) {
        let base = self.positions.len() as u32;
        for c in corners {
            self.positions.push(c.to_array());
            self.normals.push(normal.to_array());
            self.colours.push(linear(colour));
        }
        self.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    fn triangle(&mut self, corners: [Vec3; 3], colour: Rgb) {
        let normal = (corners[1] - corners[0]).cross(corners[2] - corners[0]).normalize_or_zero();
        let base = self.positions.len() as u32;
        for c in corners {
            self.positions.push(c.to_array());
            self.normals.push(normal.to_array());
            self.colours.push(linear(colour));
        }
        self.indices.extend_from_slice(&[base, base + 1, base + 2]);
    }

    /// A box of the given size about `centre`, turned `yaw` about the vertical through its middle.
    pub fn cuboid(&mut self, centre: Vec3, size: Vec3, yaw: f32, colour: Rgb) {
        let turn = Quat::from_rotation_y(yaw);
        let h = size * 0.5;
        let p = |x: f32, y: f32, z: f32| centre + turn * Vec3::new(x * h.x, y * h.y, z * h.z);
        let faces: [([Vec3; 4], Vec3); 6] = [
            ([p(-1., -1., 1.), p(1., -1., 1.), p(1., 1., 1.), p(-1., 1., 1.)], Vec3::Z),
            ([p(1., -1., -1.), p(-1., -1., -1.), p(-1., 1., -1.), p(1., 1., -1.)], -Vec3::Z),
            ([p(1., -1., 1.), p(1., -1., -1.), p(1., 1., -1.), p(1., 1., 1.)], Vec3::X),
            ([p(-1., -1., -1.), p(-1., -1., 1.), p(-1., 1., 1.), p(-1., 1., -1.)], -Vec3::X),
            ([p(-1., 1., 1.), p(1., 1., 1.), p(1., 1., -1.), p(-1., 1., -1.)], Vec3::Y),
            ([p(-1., -1., -1.), p(1., -1., -1.), p(1., -1., 1.), p(-1., -1., 1.)], -Vec3::Y),
        ];
        for (corners, normal) in faces {
            self.quad(corners, turn * normal, colour);
        }
    }

    /// A pitched roof: a triangular prism standing on the rectangle `length` by `span` about `base`,
    /// the ridge `rise` above it and running along the length, with `overhang` of eave beyond the
    /// walls on every side. Turned `yaw`.
    #[allow(clippy::too_many_arguments)]
    pub fn gable(&mut self, base: Vec3, length: f32, span: f32, rise: f32, overhang: f32, yaw: f32, colour: Rgb) {
        let turn = Quat::from_rotation_y(yaw);
        let (l, s) = (length * 0.5 + overhang, span * 0.5 + overhang);
        // The eaves hang a little below the top of the walls.
        let drop = overhang * rise / (span * 0.5).max(0.1);
        let p = |x: f32, y: f32, z: f32| base + turn * Vec3::new(x, y, z);
        let (a, b, c, d) = (p(-l, -drop, -s), p(l, -drop, -s), p(l, -drop, s), p(-l, -drop, s));
        let (r0, r1) = (p(-l, rise, 0.0), p(l, rise, 0.0));
        // The two slopes, then the two gable ends, then the underside.
        self.quad([b, a, r0, r1], turn * Vec3::new(0.0, span * 0.5 + overhang, -rise - drop).normalize(), colour);
        self.quad([d, c, r1, r0], turn * Vec3::new(0.0, span * 0.5 + overhang, rise + drop).normalize(), colour);
        self.triangle([c, b, r1], colour);
        self.triangle([a, d, r0], colour);
        self.quad([a, b, c, d], -(turn * Vec3::Y), colour);
    }

    /// An upright cylinder standing on `base`.
    pub fn cylinder(&mut self, base: Vec3, radius: f32, height: f32, segments: usize, colour: Rgb) {
        let ring = |k: usize| {
            let a = k as f32 / segments as f32 * TAU;
            Vec3::new(a.cos(), 0.0, a.sin())
        };
        for k in 0..segments {
            let (u0, u1) = (ring(k), ring(k + 1));
            let up = Vec3::Y * height;
            let (a, b) = (base + u0 * radius, base + u1 * radius);
            self.quad([a, a + up, b + up, b], ((u0 + u1) * 0.5).normalize(), colour);
            self.triangle([base + up, b + up, a + up], colour);
        }
    }

    /// An upright cone standing on `base`.
    pub fn cone(&mut self, base: Vec3, radius: f32, height: f32, segments: usize, colour: Rgb) {
        let ring = |k: usize| {
            let a = k as f32 / segments as f32 * TAU;
            Vec3::new(a.cos(), 0.0, a.sin())
        };
        let tip = base + Vec3::Y * height;
        for k in 0..segments {
            let (u0, u1) = (ring(k), ring(k + 1));
            let (a, b) = (base + u0 * radius, base + u1 * radius);
            self.triangle([a, tip, b], colour);
        }
    }

    /// A flat-shaded box lit from nowhere in particular is too grey, so the colour is passed through
    /// unchanged and the material does the shading.
    pub fn into_mesh(self) -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colours)
            .with_inserted_indices(Indices::U32(self.indices))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every triangle's winding agrees with its normal, so that nothing is drawn inside out.
    fn winding_matches_normals(b: &MeshBuilder) -> bool {
        b.indices.chunks(3).all(|t| {
            let p = |i: u32| Vec3::from_array(b.positions[i as usize]);
            let face = (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0]));
            face.dot(Vec3::from_array(b.normals[t[0] as usize])) > 0.0
        })
    }

    #[test]
    fn a_box_has_six_faces_facing_outward_even_when_turned() {
        let mut b = MeshBuilder::default();
        b.cuboid(Vec3::new(1.0, 2.0, 3.0), Vec3::new(2.0, 1.0, 4.0), 0.7, [1.0; 3]);
        assert_eq!(b.triangles(), 12);
        assert!(winding_matches_normals(&b));
    }

    #[test]
    fn roofs_cylinders_and_cones_face_outward() {
        let mut roof = MeshBuilder::default();
        roof.gable(Vec3::ZERO, 8.0, 5.0, 2.5, 0.3, 0.4, [0.5; 3]);
        assert!(winding_matches_normals(&roof), "roof");
        let mut tube = MeshBuilder::default();
        tube.cylinder(Vec3::ZERO, 2.0, 5.0, 12, [0.5; 3]);
        assert!(winding_matches_normals(&tube), "cylinder");
        let mut cone = MeshBuilder::default();
        cone.cone(Vec3::Y * 5.0, 2.2, 3.0, 12, [0.5; 3]);
        assert!(winding_matches_normals(&cone), "cone");
        // And the triangles' normals really point away from the middle.
        let away = |b: &MeshBuilder, middle: Vec3| {
            b.indices.chunks(3).all(|t| {
                let p = |i: u32| Vec3::from_array(b.positions[i as usize]);
                let c = (p(t[0]) + p(t[1]) + p(t[2])) / 3.0;
                let n = (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0]));
                n.dot(c - middle) > 0.0
            })
        };
        assert!(away(&tube, Vec3::new(0.0, 2.5, 0.0)), "cylinder faces out");
        assert!(away(&cone, Vec3::new(0.0, 6.0, 0.0)), "cone faces out");
        assert!(away(&roof, Vec3::new(0.0, 0.5, 0.0)), "roof faces out");
    }

    #[test]
    fn the_roof_slopes_face_up_and_out() {
        let mut b = MeshBuilder::default();
        b.gable(Vec3::ZERO, 8.0, 5.0, 2.5, 0.0, 0.0, [0.5; 3]);
        let front = Vec3::from_array(b.normals[0]);
        assert!(front.y > 0.0 && front.z < 0.0, "the slope toward -z faces -z and up: {front:?}");
    }
}

// The gun's looks: a Sterling L2A3 submachine gun, built from boxes, tubes and rings.
//
// Every dimension here was measured off public-domain photographs of a real L2A3 (US Navy
// museum photos, Wikimedia Commons "Submachine Gun, 9mm, L2A3, Sterling, British, S-N UF57A5347
// (NHHC 2002-11-2)"). The right-side photo was levelled and measured against the 4-inch
// ruler in the shot; `photo(x, y)` converts a position in that levelled photo (in its pixels)
// into the gun's own space, so the numbers below can be checked against the picture:
//
//   scale 1.752 photo pixels per millimetre; muzzle at x = 1262; bore axis at y = 214.7.
//
// The parts are merged into three meshes by material (gunmetal, black grip plastic, and the
// dark of holes and ports), so the whole gun is three draws however many holes it has.
// Gun space: forward is -Z, up is +Y, the bore runs along y = BORE_Y, the muzzle is at
// z = MUZZLE_Z. Right is +X, and the magazine is on the left, as it is on a real Sterling.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

pub const BORE_Y: f32 = 0.02;
pub const MUZZLE_Z: f32 = -0.5;
/// Height of the line of sight above the bore: through the rear peep to the front post
/// (the real gun's is about 25 mm).
pub const SIGHT_LINE: f32 = BORE_Y + 0.0255;

/// The receiver and the barrel jacket are the same tube, about 35 mm across.
const TUBE_R: f32 = 0.0175;
const JACKET_FRONT_Z: f32 = -0.459;
const JACKET_REAR_Z: f32 = -0.302;
const RECEIVER_REAR_Z: f32 = -0.059;
/// Where the rear peep and front hood stand.
pub const REAR_PEEP_Z: f32 = -0.0736;
const FRONT_HOOD_Z: f32 = -0.4585;

/// A point in the levelled right-side photo (its own pixels) -> (z, y) in gun space.
pub fn photo(x: f32, y: f32) -> (f32, f32) {
    (MUZZLE_Z + (1262.0 - x) / 1752.0, BORE_Y + (214.7 - y) / 1752.0)
}

/// Accumulates geometry into one mesh.
#[derive(Default)]
pub struct Parts {
    pos: Vec<[f32; 3]>,
    nor: Vec<[f32; 3]>,
    idx: Vec<u32>,
}

impl Parts {
    /// A box of `size` centred on `center`, turned by `rot`.
    pub fn add_box(&mut self, center: Vec3, size: Vec3, rot: Quat) {
        let h = size * 0.5;
        // (face normal, two tangent axes) for the six faces.
        let faces = [
            (Vec3::X, Vec3::Y, Vec3::Z),
            (Vec3::NEG_X, Vec3::Z, Vec3::Y),
            (Vec3::Y, Vec3::Z, Vec3::X),
            (Vec3::NEG_Y, Vec3::X, Vec3::Z),
            (Vec3::Z, Vec3::X, Vec3::Y),
            (Vec3::NEG_Z, Vec3::Y, Vec3::X),
        ];
        for (n, u, v) in faces {
            let base = self.pos.len() as u32;
            for (su, sv) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                let extent = |axis: Vec3| (h * axis.abs()).length();
                let local = n * extent(n) + u * extent(u) * su + v * extent(v) * sv;
                self.pos.push((center + rot * local).to_array());
                self.nor.push((rot * n).to_array());
            }
            // Wound so that the normal faces outward.
            if u.cross(v).dot(n) < 0.0 {
                self.idx.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
            } else {
                self.idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
            }
        }
    }

    /// A cylinder along the local Z axis (centred on `center`), then turned by `rot`.
    pub fn add_tube(&mut self, center: Vec3, radius: f32, length: f32, sides: usize, rot: Quat) {
        let base = self.pos.len() as u32;
        for ring in 0..2 {
            let z = (ring as f32 - 0.5) * length;
            for k in 0..=sides {
                let a = k as f32 / sides as f32 * TAU;
                let n = Vec3::new(a.cos(), a.sin(), 0.0);
                self.pos.push((center + rot * (n * radius + Vec3::Z * z)).to_array());
                self.nor.push((rot * n).to_array());
            }
        }
        let ring = (sides + 1) as u32;
        for k in 0..sides as u32 {
            let (a, b, c, d) = (base + k, base + k + 1, base + ring + k, base + ring + k + 1);
            self.idx.extend_from_slice(&[a, b, c, b, d, c]);
        }
        // End caps.
        for (end, z, facing) in [(0, -0.5 * length, Vec3::NEG_Z), (1, 0.5 * length, Vec3::Z)] {
            let cap = self.pos.len() as u32;
            self.pos.push((center + rot * Vec3::Z * z).to_array());
            self.nor.push((rot * facing).to_array());
            for k in 0..=sides {
                let a = k as f32 / sides as f32 * TAU;
                self.pos.push((center + rot * Vec3::new(a.cos() * radius, a.sin() * radius, z)).to_array());
                self.nor.push((rot * facing).to_array());
            }
            for k in 0..sides as u32 {
                if end == 0 {
                    self.idx.extend_from_slice(&[cap, cap + 2 + k, cap + 1 + k]);
                } else {
                    self.idx.extend_from_slice(&[cap, cap + 1 + k, cap + 2 + k]);
                }
            }
        }
    }

    /// A flat ring (a washer) with its axis along local Z, `thickness` deep, then turned by `rot`:
    /// the peep sight, the sight hood, the trigger guard.
    pub fn add_ring(&mut self, center: Vec3, outer: f32, inner: f32, thickness: f32, sides: usize, rot: Quat) {
        let half = thickness * 0.5;
        let at = |r: f32, a: f32, z: f32| center + rot * Vec3::new(a.cos() * r, a.sin() * r, z);
        let quad = |parts: &mut Parts, corners: [Vec3; 4], normal: Vec3| {
            let base = parts.pos.len() as u32;
            for c in corners {
                parts.pos.push(c.to_array());
                parts.nor.push(normal.to_array());
            }
            // Choose the winding that agrees with the stated normal.
            let geometric = (corners[1] - corners[0]).cross(corners[2] - corners[0]);
            if geometric.dot(normal) >= 0.0 {
                parts.idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
            } else {
                parts.idx.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
            }
        };
        for k in 0..sides {
            let (a0, a1) = (k as f32 / sides as f32 * TAU, (k + 1) as f32 / sides as f32 * TAU);
            let mid = (a0 + a1) * 0.5;
            let radial = rot * Vec3::new(mid.cos(), mid.sin(), 0.0);
            let z = rot * Vec3::Z;
            // Front and back faces, the outer wall, and the wall of the hole.
            quad(self, [at(inner, a0, half), at(outer, a0, half), at(outer, a1, half), at(inner, a1, half)], z);
            quad(self, [at(inner, a0, -half), at(outer, a0, -half), at(outer, a1, -half), at(inner, a1, -half)], -z);
            quad(self, [at(outer, a0, -half), at(outer, a0, half), at(outer, a1, half), at(outer, a1, -half)], radial);
            quad(self, [at(inner, a0, -half), at(inner, a0, half), at(inner, a1, half), at(inner, a1, -half)], -radial);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.pos.is_empty()
    }

    pub fn triangle_count(&self) -> usize {
        self.idx.len() / 3
    }

    pub fn bounds(&self) -> (Vec3, Vec3) {
        self.pos.iter().fold((Vec3::MAX, Vec3::MIN), |(lo, hi), p| (lo.min(Vec3::from(*p)), hi.max(Vec3::from(*p))))
    }

    pub fn into_mesh(self) -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.nor)
            .with_inserted_indices(Indices::U32(self.idx))
    }
}

/// The two ends of the line of sight, in gun space: the middle of the rear peep's hole, and
/// the top of the front sight blade. Aiming puts both on the camera's axis.
pub fn sight_points() -> (Vec3, Vec3) {
    (Vec3::new(0.0, SIGHT_LINE, REAR_PEEP_Z), Vec3::new(0.0, SIGHT_LINE, FRONT_HOOD_Z))
}

const NONE: Quat = Quat::IDENTITY;

/// Turns a ring or tube's local Z axis to point along `direction`.
fn facing(direction: Vec3) -> Quat {
    Quat::from_rotation_arc(Vec3::Z, direction.normalize())
}

/// The three meshes: gunmetal, grip plastic, and the dark of holes, ports and slots.
pub fn build() -> (Parts, Parts, Parts) {
    let (mut metal, mut plastic, mut dark) = (Parts::default(), Parts::default(), Parts::default());
    let axis = Vec3::new(0.0, BORE_Y, 0.0);
    let on_tube = |angle: f32, radius: f32, z: f32| axis + Vec3::new(angle.cos() * radius, angle.sin() * radius, z);

    // --- Barrel jacket, muzzle nose, the ring where the jacket meets the receiver ---
    metal.add_tube(axis + Vec3::Z * ((JACKET_FRONT_Z + JACKET_REAR_Z) * 0.5), TUBE_R, JACKET_REAR_Z - JACKET_FRONT_Z, 28, NONE);
    metal.add_tube(axis + Vec3::Z * (JACKET_FRONT_Z + MUZZLE_Z) * 0.5, 0.014, JACKET_FRONT_Z - MUZZLE_Z, 20, NONE);
    metal.add_tube(axis + Vec3::Z * (MUZZLE_Z + 0.002), 0.0105, 0.004, 14, NONE);
    metal.add_tube(axis + Vec3::Z * JACKET_REAR_Z, TUBE_R + 0.0011, 0.008, 28, NONE);

    // --- Cooling holes: eight staggered rows of round holes (measured: 9 and 8 per row,
    // 16.9 mm apart, about 9.5 mm across, rows every 46 degrees round the jacket) ---
    let pitch = 29.6 / 1752.0;
    let first_z = photo(949.0, 0.0).0;
    for row in 0..8 {
        let angle = (70.0f32 - 46.0 * row as f32).to_radians();
        let (offset, count) = if row % 2 == 0 { (0.0, 9) } else { (0.5, 8) };
        for n in 0..count {
            let z = first_z - (n as f32 + offset) * pitch;
            let spot = on_tube(angle, TUBE_R + 0.0003, z);
            let outward = Vec3::new(angle.cos(), angle.sin(), 0.0);
            dark.add_tube(spot, 0.0047, 0.0008, 12, facing(outward));
        }
    }

    // --- Receiver tube, rear cap and the plug behind it ---
    let rz = (JACKET_REAR_Z + RECEIVER_REAR_Z) * 0.5;
    metal.add_tube(axis + Vec3::Z * rz, TUBE_R, RECEIVER_REAR_Z - JACKET_REAR_Z, 28, NONE);
    let (cap_front, cap_rear) = (RECEIVER_REAR_Z, photo(442.0, 0.0).0);
    metal.add_tube(axis + Vec3::Z * ((cap_front + cap_rear) * 0.5), 0.0198, cap_rear - cap_front, 28, NONE);
    metal.add_tube(axis + Vec3::Z * (cap_rear + 0.004), 0.0085, 0.008, 12, NONE);

    // --- Ejection port (an oval cut on the right, just behind the jacket), the long cocking
    // slot along the upper right, and the cocking handle rising from it ---
    let (port_z, _) = photo(840.0, 0.0);
    dark.add_box(on_tube(8f32.to_radians(), TUBE_R + 0.0004, port_z), Vec3::new(0.0035, 0.021, 0.058), Quat::from_rotation_z(8f32.to_radians()));
    let (slot_a, slot_b) = (photo(500.0, 0.0).0, photo(790.0, 0.0).0);
    dark.add_box(
        on_tube(50f32.to_radians(), TUBE_R + 0.0004, (slot_a + slot_b) * 0.5),
        Vec3::new(0.0035, 0.0042, (slot_a - slot_b).abs()),
        Quat::from_rotation_z(50f32.to_radians() - FRAC_PI_2),
    );
    let (handle_z, _) = photo(768.0, 0.0);
    metal.add_box(axis + Vec3::new(0.0115, 0.0265, handle_z), Vec3::new(0.0045, 0.031, 0.0045), Quat::from_rotation_x(-0.28));
    metal.add_tube(axis + Vec3::new(0.0115, 0.0425, handle_z - 0.0085), 0.0045, 0.006, 10, Quat::from_rotation_x(FRAC_PI_2));

    // --- Sights: a round peep disc over the back of the receiver (flipped up, as photographed)
    // and, at the muzzle, a hood of two rings with the blade between them ---
    metal.add_box(axis + Vec3::new(0.0, TUBE_R + 0.001, REAR_PEEP_Z), Vec3::new(0.014, 0.008, 0.010), NONE);
    metal.add_ring(Vec3::new(0.0, SIGHT_LINE, REAR_PEEP_Z), 0.0115, 0.0033, 0.003, 24, NONE);
    metal.add_box(axis + Vec3::new(0.0, TUBE_R - 0.0005, FRONT_HOOD_Z), Vec3::new(0.016, 0.007, 0.022), NONE);
    for dz in [-0.0045, 0.0045] {
        metal.add_ring(Vec3::new(0.0, SIGHT_LINE, FRONT_HOOD_Z + dz), 0.0082, 0.0058, 0.0014, 20, NONE);
    }
    metal.add_box(Vec3::new(0.0, SIGHT_LINE - 0.0035, FRONT_HOOD_Z), Vec3::new(0.0022, 0.0072, 0.0035), NONE);

    // --- Bayonet lug under the jacket, near the muzzle ---
    metal.add_box(axis + Vec3::new(0.0, -TUBE_R - 0.0035, photo(1108.0, 0.0).0), Vec3::new(0.010, 0.008, 0.013), NONE);

    // --- Trigger housing: a flat stamped body under the receiver from the jacket back past the
    // grip, with its right-hand side plate, and the block at its front ---
    let (hz_front, hz_rear) = (photo(895.0, 0.0).0, photo(680.0, 0.0).0);
    let hz = (hz_front + hz_rear) * 0.5;
    metal.add_box(axis + Vec3::new(0.0, -TUBE_R - 0.0035, hz), Vec3::new(0.030, 0.009, (hz_rear - hz_front).abs()), NONE);
    metal.add_box(axis + Vec3::new(0.0165, -0.011, hz), Vec3::new(0.0035, 0.032, (hz_rear - hz_front).abs()), NONE);
    metal.add_box(axis + Vec3::new(0.0, -0.0245, photo(886.0, 0.0).0), Vec3::new(0.022, 0.012, 0.011), NONE);

    // --- Trigger guard (a loop), trigger and the pistol grip behind it ---
    let (guard_z, guard_y) = photo(828.0, 294.0);
    metal.add_ring(Vec3::new(0.0, guard_y, guard_z), 0.0205, 0.0172, 0.004, 24, Quat::from_rotation_y(FRAC_PI_2));
    metal.add_box(Vec3::new(0.0, guard_y + 0.0105, guard_z + 0.004), Vec3::new(0.004, 0.022, 0.004), Quat::from_rotation_x(0.35));
    // The grip leans back about 21 degrees: its top is mid-receiver and its heel well behind.
    let (top_z, top_y) = photo(727.0, 240.0);
    let (heel_z, heel_y) = photo(656.0, 420.0);
    let centre = Vec3::new(0.0, (top_y + heel_y) * 0.5, (top_z + heel_z) * 0.5);
    let lean = ((heel_z - top_z) / (top_y - heel_y)).atan();
    let tilt = Quat::from_rotation_x(-lean);
    plastic.add_box(centre, Vec3::new(0.029, 0.116, 0.040), tilt);
    plastic.add_box(Vec3::new(0.0, heel_y + 0.004, heel_z + 0.002), Vec3::new(0.0305, 0.014, 0.0475), tilt);

    // --- Magazine housing on the left, and the magazine: a flat box, curving forward as it runs
    // out sideways (measured about 190 mm long), with ribs and an end plate ---
    let mag_z = photo(828.0, 0.0).0 + 0.03;
    metal.add_box(Vec3::new(-0.0335, BORE_Y + 0.001, mag_z), Vec3::new(0.032, 0.040, 0.034), NONE);
    let (mut at, segments, seg_len) = (Vec3::new(-0.0495, BORE_Y, mag_z), 5, 0.038);
    for k in 0..segments {
        let bend = 0.055 * (k as f32 + 0.5) * (k as f32 + 1.0) * 0.5;
        let toward = Vec3::new(-bend.cos(), 0.0, -bend.sin());
        let centre = at + toward * (seg_len * 0.5);
        metal.add_box(centre, Vec3::new(seg_len + 0.002, 0.040, 0.025), Quat::from_rotation_y(-bend));
        // A rib on the magazine's top face, as the stamped box has.
        metal.add_box(centre + Vec3::Y * 0.0205, Vec3::new(seg_len * 0.55, 0.0016, 0.010), Quat::from_rotation_y(-bend));
        at += toward * seg_len;
    }
    let end_bend = 0.055 * (segments as f32) * (segments as f32 + 1.0) * 0.5 * 0.0 + 0.055 * (segments as f32 - 0.5) * segments as f32 * 0.5;
    metal.add_box(at + Vec3::new(-end_bend.cos(), 0.0, -end_bend.sin()) * 0.002, Vec3::new(0.006, 0.046, 0.030), Quat::from_rotation_y(-end_bend));

    // --- Folding stock: two bars either side, level with the receiver at the hinge and dropping
    // away behind it (the real stock drop), two struts, and the butt plate across the back ---
    let (hinge_z, hinge_y) = photo(662.0, 221.0);
    let (elbow_z, elbow_y) = photo(370.0, 262.0);
    let (butt_z, butt_y) = photo(110.0, 283.0);
    for side in [-1.0f32, 1.0] {
        let x = side * 0.0255;
        metal.add_tube(Vec3::new(x, hinge_y, hinge_z), 0.0055, 0.004, 10, Quat::from_rotation_y(FRAC_PI_2));
        for (a, b, h, w) in [((hinge_z, hinge_y), (elbow_z, elbow_y), 0.018, 0.0045), ((elbow_z, elbow_y), (butt_z, butt_y), 0.016, 0.012)] {
            let (mid_z, mid_y) = ((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5);
            let slope = ((b.1 - a.1) / (a.0 - b.0)).atan();
            metal.add_box(Vec3::new(x, mid_y, mid_z), Vec3::new(w, h, (a.0 - b.0).abs() / slope.cos()), Quat::from_rotation_x(slope));
        }
        // The strut from the bottom of the butt up to the bar, a round tube.
        let (sz0, sy0) = photo(147.0, 425.0);
        let (sz1, sy1) = photo(364.0, 266.0);
        let along = Vec3::new(0.0, sy1 - sy0, sz1 - sz0);
        metal.add_tube(Vec3::new(x * 0.8, (sy0 + sy1) * 0.5, (sz0 + sz1) * 0.5), 0.0038, along.length(), 10, facing(along));
    }
    // Butt plate: a stamped steel plate, tall and lightly dished, with a lower hinge block.
    let (plate_z, plate_y) = photo(105.0, 375.0);
    metal.add_box(Vec3::new(0.0, plate_y, plate_z), Vec3::new(0.062, 0.112, 0.008), Quat::from_rotation_x(-0.04));
    metal.add_box(Vec3::new(0.0, butt_y + 0.004, butt_z + 0.003), Vec3::new(0.062, 0.016, 0.02), NONE);

    (metal, plastic, dark)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> [Parts; 3] {
        let (a, b, c) = build();
        [a, b, c]
    }

    #[test]
    fn photo_coordinates_land_where_measured() {
        // The muzzle, the bore axis, and the 4-inch ruler (178 photo pixels).
        let (z, y) = photo(1262.0, 214.7);
        assert!((z - MUZZLE_Z).abs() < 1e-6 && (y - BORE_Y).abs() < 1e-6);
        let (z0, _) = photo(1007.0, 0.0);
        let (z1, _) = photo(1185.0, 0.0);
        assert!(((z0 - z1) - 0.1016).abs() < 0.002, "4 inches spans {} m", z0 - z1);
    }

    #[test]
    fn every_part_is_well_formed() {
        for parts in all() {
            assert!(!parts.is_empty());
            assert_eq!(parts.pos.len(), parts.nor.len());
            assert_eq!(parts.idx.len() % 3, 0);
            assert!(parts.idx.iter().all(|&i| (i as usize) < parts.pos.len()), "index out of range");
            assert!(parts.pos.iter().flatten().all(|v| v.is_finite()));
            assert!(parts.nor.iter().all(|n| (Vec3::from(*n).length() - 1.0).abs() < 1e-3), "normals are unit length");
        }
    }

    #[test]
    fn every_triangle_faces_the_way_its_normal_says() {
        for (which, parts) in all().iter().enumerate() {
            let (mut wrong, mut total) = (0, 0);
            for tri in parts.idx.chunks(3) {
                let p = |i: u32| Vec3::from(parts.pos[i as usize]);
                let geometric = (p(tri[1]) - p(tri[0])).cross(p(tri[2]) - p(tri[0]));
                if geometric.length() < 1e-9 {
                    continue; // a sliver or the degenerate centre of a cap
                }
                total += 1;
                if geometric.dot(Vec3::from(parts.nor[tri[0] as usize])) <= 0.0 {
                    wrong += 1;
                }
            }
            assert_eq!(wrong, 0, "mesh {which}: {wrong} of {total} triangles wound inside out");
        }
    }

    #[test]
    fn the_muzzle_is_the_front_of_the_gun_and_the_length_is_a_sterlings() {
        let (mut lo, mut hi) = (Vec3::MAX, Vec3::MIN);
        for parts in all() {
            let (l, h) = parts.bounds();
            lo = lo.min(l);
            hi = hi.max(h);
        }
        assert!((lo.z - MUZZLE_Z).abs() < 0.002, "front of the gun at {} but the muzzle is at {MUZZLE_Z}", lo.z);
        // 686 mm with the stock out (the photographed gun measures 676).
        assert!((0.65..0.70).contains(&(hi.z - lo.z)), "length {}", hi.z - lo.z);
    }

    #[test]
    fn the_tube_is_slim_like_the_real_thing() {
        // The jacket and receiver are about 35 mm across; nothing on the tube should be much fatter
        // than the rear cap. Measure the jacket's height at its front ring.
        let (metal, _, _) = build();
        let ys: Vec<f32> = metal.pos.iter().filter(|p| (p[2] - JACKET_FRONT_Z).abs() < 1e-4 && p[1] < BORE_Y + 0.019 && p[1] > BORE_Y - 0.019).map(|p| p[1]).collect();
        assert!(ys.len() >= 8, "found only {} vertices on the jacket's front ring", ys.len());
        let (lo, hi) = (ys.iter().cloned().fold(f32::MAX, f32::min), ys.iter().cloned().fold(f32::MIN, f32::max));
        assert!((0.033..0.037).contains(&(hi - lo)), "jacket is {} m across", hi - lo);
    }

    #[test]
    fn the_jacket_has_the_measured_pattern_of_round_holes() {
        let (_, _, dark) = build();
        // 8 rows alternating 9 and 8 holes, 12 sides each (side walls and two caps).
        let holes = dark.pos.iter().filter(|p| p[2] < JACKET_REAR_Z && p[2] > JACKET_FRONT_Z).count() / (2 * 13 + 2 * 13 + 2);
        assert!(holes >= 60, "only about {holes} holes on the jacket");
        let zs: Vec<i32> = dark.pos.iter().filter(|p| p[2] < JACKET_REAR_Z).map(|p| (p[2] * 4000.0).round() as i32).collect();
        assert!(zs.iter().min().unwrap() < &-1800 && zs.iter().max().unwrap() > &-1300, "holes run the length of the jacket");
    }

    #[test]
    fn the_magazine_sticks_out_to_the_left_and_curves_forward() {
        let (metal, _, _) = build();
        let (lo, hi) = metal.bounds();
        assert!(lo.x < -0.2, "magazine reaches x = {}", lo.x);
        assert!(hi.x < 0.045, "the right-hand side stays clean, reaching {}", hi.x);
        // Its far end is further forward than where it joins the receiver.
        let tip: Vec<&[f32; 3]> = metal.pos.iter().filter(|p| p[0] < lo.x + 0.01).collect();
        let tip_z = tip.iter().map(|p| p[2]).sum::<f32>() / tip.len() as f32;
        let root_z = photo(828.0, 0.0).0 + 0.03;
        assert!(tip_z < root_z - 0.03, "magazine end at z = {tip_z}, joined at {root_z}");
    }

    #[test]
    fn the_grip_is_mid_receiver_and_leans_back_with_the_trigger_in_front_of_it() {
        let (_, plastic, _) = build();
        let (lo, hi) = plastic.bounds();
        assert!(lo.y < -0.1, "grip bottom at {}", lo.y);
        // The grip's top is about 19 cm behind the muzzle, not at the back of the gun.
        let top: Vec<f32> = plastic.pos.iter().filter(|p| p[1] > lo.y + 0.09).map(|p| p[2]).collect();
        let heel: Vec<f32> = plastic.pos.iter().filter(|p| p[1] < lo.y + 0.02).map(|p| p[2]).collect();
        let (top_z, heel_z) = (top.iter().sum::<f32>() / top.len() as f32, heel.iter().sum::<f32>() / heel.len() as f32);
        assert!(heel_z > top_z + 0.03, "the heel ({heel_z}) sits well behind the top ({top_z}): a raked grip");
        assert!(hi.z < 0.0 && lo.z < -0.16, "grip spans z = {}..{}", lo.z, hi.z);
        let (guard_z, _) = photo(828.0, 294.0);
        assert!(guard_z < lo.z, "the trigger guard ({guard_z}) is in front of the grip ({})", lo.z);
    }

    #[test]
    fn the_sights_line_up_on_the_line_of_sight() {
        let (rear, front) = sight_points();
        assert!((rear.y - SIGHT_LINE).abs() < 1e-6 && (front.y - SIGHT_LINE).abs() < 1e-6 && rear.x == 0.0 && front.x == 0.0);
        assert!(rear.z > front.z + 0.3, "rear peep is well behind the front hood");
        // The line of sight is about 25 mm above the bore, as on the photographed gun.
        assert!((SIGHT_LINE - BORE_Y - 0.0255).abs() < 1e-6);
        // The peep is a ring around that point: nothing of the disc is *at* its centre.
        let (metal, _, _) = build();
        let blocking = metal.pos.chunks(4).filter(|q| {
            let c = q.iter().map(|p| Vec3::from(*p)).sum::<Vec3>() / q.len() as f32;
            (c.z - REAR_PEEP_Z).abs() < 0.002 && Vec2::new(c.x, c.y - SIGHT_LINE).length() < 0.002
        });
        assert_eq!(blocking.count(), 0, "the hole in the peep is clear");
    }

    #[test]
    fn the_stock_drops_below_the_receiver_and_the_butt_plate_is_tall_and_at_the_back() {
        let (metal, _, _) = build();
        let (lo, hi) = metal.bounds();
        assert!(hi.z > 0.15, "stock reaches back to {}", hi.z);
        let plate: Vec<f32> = metal.pos.iter().filter(|p| p[2] > hi.z - 0.015).map(|p| p[1]).collect();
        let (plate_lo, plate_hi) = (plate.iter().cloned().fold(f32::MAX, f32::min), plate.iter().cloned().fold(f32::MIN, f32::max));
        assert!(plate_hi - plate_lo > 0.09, "butt plate only {} m tall", plate_hi - plate_lo);
        assert!(plate_hi < BORE_Y, "the butt is below the bore axis (stock drop): top at {plate_hi}");
        // ...and out of the aimed line of sight by a good margin.
        assert!(plate_hi - SIGHT_LINE < -0.04, "butt plate top is {} below the line of sight", SIGHT_LINE - plate_hi);
        let _ = lo;
    }
}

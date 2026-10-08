// The gun's looks: a Sterling L2A3 submachine gun, modelled from boxes and cylinders.
//
// The parts are merged into three meshes by material (gunmetal, black grip plastic, and dark
// recesses) instead of being separate entities, so the whole gun is three draws however many
// cooling slots it has. Everything is in the gun's own space: forward is -Z, up is +Y, the
// bore is along the line y = BORE_Y, and the muzzle is at z = MUZZLE_Z.
//
// What makes a Sterling a Sterling, all present here: a plain tubular receiver; a barrel jacket
// perforated with rows of cooling slots; a long magazine sticking out of the LEFT side,
// horizontally; a short pistol grip behind it; a skeleton stock of tube and a butt plate; a
// cocking handle and ejection port on the right; and a front sight post guarded by two ears.

use std::f32::consts::TAU;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

pub const BORE_Y: f32 = 0.02;
pub const MUZZLE_Z: f32 = -0.5;
/// Height (in gun space) of the tops of both sights.
pub const SIGHT_TOP: f32 = 0.08;

const RECEIVER_RADIUS: f32 = 0.032;
const JACKET_RADIUS: f32 = 0.026;

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
            let flip = (u.cross(v)).dot(n) < 0.0;
            if flip {
                self.idx.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
            } else {
                self.idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
            }
        }
    }

    /// A cylinder along the gun's Z axis (centre `center`), then turned by `rot`.
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
                self.pos.push((center + rot * (Vec3::new(a.cos() * radius, a.sin() * radius, z))).to_array());
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

    /// A unit cube transform (as the sights are given) added as a box.
    pub fn add_cube(&mut self, t: Transform) {
        self.add_box(t.translation, t.scale, t.rotation);
    }

    pub fn is_empty(&self) -> bool {
        self.pos.is_empty()
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

/// The front sight post, in the gun's space, as a transform of a unit cube. Its top is at
/// `SIGHT_TOP`, and its foot rests on the barrel jacket.
pub fn front_sight() -> Transform {
    let foot = BORE_Y + JACKET_RADIUS;
    let size = Vec3::new(0.005, SIGHT_TOP - foot, 0.005);
    Transform::from_xyz(0.0, foot + size.y * 0.5, MUZZLE_Z + 0.02).with_scale(size)
}

/// The rear sight: two uprights either side of a notch, over the back of the receiver, both
/// topping out at `SIGHT_TOP`. The front post shows in the gap between them when aiming.
pub fn rear_sights() -> [Transform; 2] {
    let foot = BORE_Y + RECEIVER_RADIUS - 0.004;
    let size = Vec3::new(0.0065, SIGHT_TOP - foot, 0.01);
    [-1.0f32, 1.0].map(|side| Transform::from_xyz(side * 0.0095, foot + size.y * 0.5, -0.06).with_scale(size))
}

/// The three meshes: gunmetal, grip plastic, and the dark of slots, ports and sight notches.
pub fn build() -> (Parts, Parts, Parts) {
    let (mut metal, mut plastic, mut dark) = (Parts::default(), Parts::default(), Parts::default());
    let none = Quat::IDENTITY;
    let axis = Vec3::new(0.0, BORE_Y, 0.0);

    // Barrel jacket, muzzle stub and the nut where the jacket meets the receiver.
    metal.add_tube(axis + Vec3::new(0.0, 0.0, -0.39), JACKET_RADIUS, 0.20, 20, none);
    metal.add_tube(axis + Vec3::new(0.0, 0.0, MUZZLE_Z + 0.0075), 0.011, 0.015, 12, none);
    metal.add_tube(axis + Vec3::new(0.0, 0.0, -0.295), 0.0295, 0.012, 20, none);
    // Receiver tube and its end cap.
    metal.add_tube(axis + Vec3::new(0.0, 0.0, -0.145), RECEIVER_RADIUS, 0.29, 24, none);
    metal.add_tube(axis + Vec3::new(0.0, 0.0, 0.012), 0.034, 0.024, 24, none);

    // Front sight: the post, its housing, and the two protecting ears either side.
    metal.add_cube(front_sight());
    metal.add_box(Vec3::new(0.0, BORE_Y + JACKET_RADIUS + 0.005, MUZZLE_Z + 0.04), Vec3::new(0.03, 0.012, 0.034), none);
    for side in [-1.0, 1.0] {
        metal.add_box(Vec3::new(side * 0.0135, BORE_Y + JACKET_RADIUS + 0.017, MUZZLE_Z + 0.025), Vec3::new(0.004, 0.032, 0.016), none);
    }
    // Rear sight: a base across the receiver and the two uprights with the notch between.
    for upright in rear_sights() {
        metal.add_cube(upright);
    }
    metal.add_box(Vec3::new(0.0, BORE_Y + RECEIVER_RADIUS + 0.001, -0.06), Vec3::new(0.034, 0.008, 0.016), none);

    // Cooling slots in the jacket: eight rings of slots across its upper half.
    for station in 0..8 {
        let z = -0.475 + station as f32 * 0.0235;
        for k in 0..9 {
            let a = (-88.0f32 + k as f32 * 22.0).to_radians();
            let at = axis + Vec3::new(a.sin() * (JACKET_RADIUS + 0.0005), a.cos() * (JACKET_RADIUS + 0.0005), z);
            dark.add_box(at, Vec3::new(0.0085, 0.004, 0.016), Quat::from_rotation_z(-a));
        }
    }

    // Magazine housing on the left, then the magazine itself running out sideways, with the
    // gentle curve a 34-round Sterling magazine has.
    metal.add_box(Vec3::new(-0.038, 0.016, -0.12), Vec3::new(0.052, 0.072, 0.046), none);
    metal.add_box(Vec3::new(-0.115, 0.012, -0.12), Vec3::new(0.11, 0.07, 0.034), none);
    metal.add_box(Vec3::new(-0.2, 0.0, -0.12), Vec3::new(0.09, 0.07, 0.034), Quat::from_rotation_z(0.09));
    metal.add_box(Vec3::new(-0.235, -0.012, -0.12), Vec3::new(0.012, 0.074, 0.038), Quat::from_rotation_z(0.15));
    // Ribs along the magazine.
    for k in 0..4 {
        metal.add_box(Vec3::new(-0.085 - k as f32 * 0.03, 0.012 - k as f32 * 0.0035, -0.1365), Vec3::new(0.004, 0.06, 0.003), none);
    }

    // Ejection port and cocking slot on the right, the cocking handle's knob on its rod.
    dark.add_box(Vec3::new(0.0318, 0.034, -0.05), Vec3::new(0.004, 0.022, 0.07), none);
    dark.add_box(Vec3::new(0.0322, 0.012, -0.11), Vec3::new(0.003, 0.008, 0.13), none);
    metal.add_box(Vec3::new(0.0375, 0.012, -0.075), Vec3::new(0.012, 0.012, 0.012), none);
    metal.add_box(Vec3::new(0.034, 0.012, -0.075), Vec3::new(0.007, 0.005, 0.005), none);

    // Pistol grip, leaning back, with the trigger guard and trigger in front of it.
    plastic.add_box(Vec3::new(0.0, -0.062, -0.012), Vec3::new(0.032, 0.115, 0.038), Quat::from_rotation_x(-0.2));
    plastic.add_box(Vec3::new(0.0, -0.118, 0.001), Vec3::new(0.034, 0.012, 0.042), Quat::from_rotation_x(-0.2));
    metal.add_box(Vec3::new(0.0, -0.034, -0.075), Vec3::new(0.005, 0.036, 0.005), none);
    metal.add_box(Vec3::new(0.0, -0.05, -0.052), Vec3::new(0.005, 0.005, 0.05), none);
    metal.add_box(Vec3::new(0.0, -0.03, -0.048), Vec3::new(0.004, 0.02, 0.004), Quat::from_rotation_x(0.2));

    // Skeleton stock: two tubes running back from the receiver, a top strut, and the butt plate.
    for side in [-1.0, 1.0] {
        metal.add_tube(Vec3::new(side * 0.022, -0.004, 0.105), 0.0045, 0.19, 8, Quat::from_rotation_y(side * 0.03));
    }
    metal.add_tube(Vec3::new(0.0, BORE_Y + 0.02, 0.105), 0.004, 0.19, 8, none);
    metal.add_box(Vec3::new(0.0, -0.045, 0.205), Vec3::new(0.062, 0.078, 0.01), none);
    metal.add_box(Vec3::new(0.0, -0.004, 0.2), Vec3::new(0.04, 0.012, 0.006), none);

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
            let mut wrong = 0;
            let mut total = 0;
            for tri in parts.idx.chunks(3) {
                let p = |i: u32| Vec3::from(parts.pos[i as usize]);
                let geometric = (p(tri[1]) - p(tri[0])).cross(p(tri[2]) - p(tri[0]));
                if geometric.length() < 1e-9 {
                    continue; // a sliver or the degenerate centre of a cap
                }
                total += 1;
                let stored = Vec3::from(parts.nor[tri[0] as usize]);
                if geometric.dot(stored) <= 0.0 {
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
        // About 70 cm with the stock out.
        assert!((0.66..0.74).contains(&(hi.z - lo.z)), "length {}", hi.z - lo.z);
    }

    #[test]
    fn cooling_slots_run_the_length_of_the_jacket() {
        let (_, _, dark) = build();
        let zs: Vec<f32> = dark.pos.iter().map(|p| p[2]).collect();
        let (lo, hi) = (zs.iter().cloned().fold(f32::MAX, f32::min), zs.iter().cloned().fold(f32::MIN, f32::max));
        assert!(lo < -0.45 && hi > -0.07, "dark detail only spans z = {lo}..{hi}");
        // Rings of slots: several distinct stations along the barrel.
        let mut stations: Vec<i32> = dark.pos.iter().filter(|p| p[2] < -0.28).map(|p| (p[2] * 200.0).round() as i32).collect();
        stations.sort_unstable();
        stations.dedup();
        assert!(stations.len() >= 8, "only {} distinct z positions among the slots", stations.len());
    }

    #[test]
    fn the_magazine_sticks_out_to_the_left_and_nothing_much_to_the_right() {
        let (metal, _, _) = build();
        let (lo, hi) = metal.bounds();
        assert!(lo.x < -0.2, "magazine reaches x = {}", lo.x);
        assert!(hi.x < 0.05, "right-hand side stays clean, reaching {}", hi.x);
    }

    #[test]
    fn the_sights_top_out_together_and_sit_on_the_metal_below_them() {
        let top = front_sight().transform_point(Vec3::new(0.0, 0.5, 0.0));
        assert!((top.y - SIGHT_TOP).abs() < 1e-5 && top.x.abs() < 1e-6, "front post is on the centre line");
        let [left, right] = rear_sights();
        for upright in [left, right] {
            let t = upright.transform_point(Vec3::new(0.0, 0.5, 0.0));
            assert!((t.y - SIGHT_TOP).abs() < 1e-5, "rear upright tops at {}", t.y);
        }
        assert!((left.translation.x + right.translation.x).abs() < 1e-6, "the notch is centred on the front post");
        let gap = right.translation.x - left.translation.x - left.scale.x;
        assert!(gap > front_sight().scale.x * 1.5, "the front post fits in the notch with room either side ({gap})");
        let front_foot = front_sight().transform_point(Vec3::new(0.0, -0.5, 0.0)).y;
        assert!((front_foot - (BORE_Y + JACKET_RADIUS)).abs() < 1e-5, "front sight stands on the jacket");
        let rear_foot = rear_sights()[0].transform_point(Vec3::new(0.0, -0.5, 0.0)).y;
        assert!(rear_foot < BORE_Y + RECEIVER_RADIUS, "rear sight is seated into the receiver");
    }

    #[test]
    fn the_grip_hangs_below_the_receiver_and_the_stock_stays_behind_it() {
        let (_, plastic, _) = build();
        let (lo, _) = plastic.bounds();
        assert!(lo.y < -0.1, "grip bottom at {}", lo.y);
        let (metal, _, _) = build();
        let (_, hi) = metal.bounds();
        assert!(hi.z > 0.15, "stock reaches back to {}", hi.z);
        // ...and its butt plate stays out of the line of sight (below the sight tops by a good way).
        let plate_top = -0.045 + 0.039 - super::SIGHT_TOP;
        assert!(plate_top < -0.07, "butt plate top is {plate_top} below the line of sight when aimed");
    }
}

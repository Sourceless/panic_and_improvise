// Trees and shrubs.
//
// Every tree is generated procedurally as real branching geometry: tapered bark tubes for the
// trunk and limbs, covered in cards textured with clusters of real leaves (baked from CC0
// leaf atlases by tools/bake_foliage.py). A handful of variants per species are generated
// once and shared by every instance of that species.
//
// Level of detail: every tree is also a cheap low-poly blob in a mesh merged per chunk, which
// is always drawn. Near the camera, chunks are streamed in as the full trees (and out again
// when the camera moves away), so there are only ever a few thousand detailed trees alive.

use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::map::{fbm, TerrainMap, CELL, HALF_SIZE, MAP_SIZE};
use crate::mipmaps::MipQueue;
use crate::wind_material::{WindExtension, WindMaterial};
use crate::terrain::TerrainRoot;
use crate::zones::{Zone, ZoneMap};

const CHUNK: f32 = 96.0;
// Detailed trees exist within this distance of the camera, and are removed beyond the second.
const STREAM_IN: f32 = 280.0;
const STREAM_OUT: f32 = 340.0;
const VARIANTS: usize = 5;
const BARK_TILE: f32 = 1.4;

pub struct VegetationPlugin;

impl Plugin for VegetationPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, build_vegetation_assets.after(crate::terrain::load_textures))
            .add_systems(Update, stream_detailed_trees);
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Species {
    Oak,
    Beech,
    Birch,
    Maple,
    Spruce,
    Pine,
    Apple,
    Hawthorn,
    Gorse,
}

impl Species {
    const ALL: [Species; 9] = [
        Species::Oak,
        Species::Beech,
        Species::Birch,
        Species::Maple,
        Species::Spruce,
        Species::Pine,
        Species::Apple,
        Species::Hawthorn,
        Species::Gorse,
    ];

    // Wind: how far the top of the tree sways (m), the height that is reached at, and how much
    // individual leaves flutter (m).
    fn sway(self) -> f32 {
        match self {
            Species::Spruce | Species::Pine => 0.28,
            Species::Hawthorn | Species::Gorse => 0.07,
            Species::Apple => 0.14,
            _ => 0.38,
        }
    }

    fn wind_height(self) -> f32 {
        match self {
            Species::Oak => 15.0,
            Species::Beech => 17.0,
            Species::Birch => 13.0,
            Species::Maple => 12.0,
            Species::Spruce => 16.0,
            Species::Pine => 14.0,
            Species::Apple => 4.6,
            Species::Hawthorn | Species::Gorse => 1.4,
        }
    }

    fn flutter(self) -> f32 {
        match self {
            Species::Spruce | Species::Pine => 0.015,
            _ => 0.05,
        }
    }

    /// The radius of the trunk near the ground, metres (before the tree's own size is applied),
    /// as the models are grown; it is what a person bumps into.
    pub fn trunk_radius(self) -> f32 {
        match self {
            Species::Oak => 0.5,
            Species::Beech => 0.38,
            Species::Birch => 0.17,
            Species::Maple => 0.3,
            Species::Apple => 0.16,
            Species::Spruce => 0.27,
            Species::Pine => 0.25,
            Species::Hawthorn | Species::Gorse => 0.0,
        }
    }

    fn is_bush(self) -> bool {
        matches!(self, Species::Hawthorn | Species::Gorse)
    }

    fn bark(self) -> &'static str {
        match self {
            Species::Oak | Species::Beech | Species::Maple | Species::Apple => "textures/pbr/bark_oak.jpg",
            Species::Birch => "textures/pbr/bark_birch.jpg",
            _ => "textures/pbr/bark_conifer.jpg",
        }
    }

    fn bark_normal(self) -> &'static str {
        match self {
            Species::Oak | Species::Beech | Species::Maple | Species::Apple => "textures/pbr/bark_oak_n.jpg",
            Species::Birch => "textures/pbr/bark_birch_n.jpg",
            _ => "textures/pbr/bark_conifer_n.jpg",
        }
    }

    fn leaves(self) -> &'static str {
        match self {
            Species::Oak => "textures/veg/cluster_beech.png",
            Species::Beech => "textures/veg/cluster_lime.png",
            Species::Birch => "textures/veg/cluster_willow.png",
            Species::Maple => "textures/veg/cluster_maple.png",
            Species::Apple => "textures/veg/cluster_lime.png",
            Species::Hawthorn => "textures/veg/cluster_beech.png",
            Species::Gorse => "textures/veg/cluster_willow.png",
            Species::Spruce | Species::Pine => "textures/veg/cluster_conifer.png",
        }
    }

    // Multiplier on the leaf texture (the baked clusters are on the pale side).
    fn leaf_tint(self) -> [f32; 3] {
        match self {
            Species::Oak => [0.42, 0.52, 0.32],
            Species::Beech => [0.40, 0.52, 0.27],
            Species::Birch => [0.50, 0.62, 0.34],
            Species::Maple => [0.70, 0.62, 0.45],
            Species::Apple => [0.44, 0.58, 0.30],
            Species::Hawthorn => [0.38, 0.50, 0.28],
            Species::Gorse => [0.36, 0.46, 0.26],
            Species::Spruce => [0.40, 0.55, 0.42],
            Species::Pine => [0.45, 0.56, 0.38],
        }
    }

    // Colour of the distant low-poly blob, roughly the leaf texture's mean once lit.
    fn blob_colour(self) -> [f32; 3] {
        match self {
            Species::Oak => [0.16, 0.26, 0.09],
            Species::Beech => [0.20, 0.30, 0.08],
            Species::Birch => [0.22, 0.32, 0.12],
            Species::Maple => [0.36, 0.30, 0.10],
            Species::Apple => [0.22, 0.32, 0.10],
            Species::Spruce => [0.07, 0.16, 0.10],
            Species::Pine => [0.10, 0.19, 0.10],
            Species::Hawthorn => [0.16, 0.26, 0.10],
            Species::Gorse => [0.16, 0.24, 0.09],
        }
    }
}

// ---------------------------------------------------------------------------------------
// Small helpers

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03 | 1)
    }

    fn unit(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }
}

fn hash2(x: i32, z: i32, salt: u64) -> f32 {
    let mut h = salt ^ (x as u32 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (z as u32 as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    h = (h ^ (h >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h = (h ^ (h >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^= h >> 31;
    (h >> 40) as f32 / (1u64 << 24) as f32
}

#[derive(Default)]
struct MeshBuf {
    pos: Vec<[f32; 3]>,
    nor: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    col: Vec<[f32; 4]>,
    idx: Vec<u32>,
}

impl MeshBuf {
    // A tapered tube along a polyline, using parallel-transport frames so it doesn't twist.
    fn tube(&mut self, pts: &[Vec3], radii: &[f32], sides: usize) {
        if pts.len() < 2 {
            return;
        }
        let tangent = |i: usize| {
            let (a, b) = (pts[i.saturating_sub(1)], pts[(i + 1).min(pts.len() - 1)]);
            (b - a).normalize_or_zero()
        };
        let repeats = (TAU * radii[0] / BARK_TILE).round().max(1.0);
        let t0 = tangent(0);
        let mut normal = if t0.y.abs() < 0.9 { Vec3::Y.cross(t0) } else { Vec3::X.cross(t0) }.normalize_or_zero();
        let base = self.pos.len() as u32;
        let mut dist = 0.0;
        for i in 0..pts.len() {
            let t = tangent(i);
            if i > 0 {
                let prev = tangent(i - 1);
                let axis = prev.cross(t);
                if axis.length() > 1e-5 {
                    normal = Quat::from_axis_angle(axis.normalize(), prev.dot(t).clamp(-1.0, 1.0).acos()) * normal;
                }
                normal = (normal - t * normal.dot(t)).normalize_or_zero();
                dist += pts[i].distance(pts[i - 1]);
            }
            let binormal = t.cross(normal);
            for k in 0..=sides {
                let a = k as f32 / sides as f32 * TAU;
                let dir = normal * a.cos() + binormal * a.sin();
                let p = pts[i] + dir * radii[i];
                self.pos.push(p.to_array());
                self.nor.push(dir.to_array());
                self.uv.push([k as f32 / sides as f32 * repeats, dist / BARK_TILE]);
                self.col.push([1.0; 4]);
            }
        }
        let ring = (sides + 1) as u32;
        for i in 0..pts.len() as u32 - 1 {
            for k in 0..sides as u32 {
                let a = base + i * ring + k;
                let (b, c, d) = (a + 1, a + ring, a + ring + 1);
                self.idx.extend_from_slice(&[a, b, c, b, d, c]);
            }
        }
    }

    // A flat leaf-cluster card. `shade` is the (spherical) normal used for lighting.
    fn card(&mut self, centre: Vec3, right: Vec3, up: Vec3, shade: Vec3, tone: f32) {
        let base = self.pos.len() as u32;
        let corners = [(-1.0, -1.0, 0.0, 1.0), (1.0, -1.0, 1.0, 1.0), (1.0, 1.0, 1.0, 0.0), (-1.0, 1.0, 0.0, 0.0)];
        for (sx, sy, u, v) in corners {
            self.pos.push((centre + right * sx + up * sy).to_array());
            self.nor.push(shade.to_array());
            self.uv.push([u, v]);
            self.col.push([tone, tone, tone, 1.0]);
        }
        self.idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    fn into_mesh(self, textured: bool) -> Mesh {
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.nor)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.col)
            .with_inserted_indices(Indices::U32(self.idx));
        if textured {
            mesh = mesh.with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uv);
        }
        mesh
    }
}

// ---------------------------------------------------------------------------------------
// Tree generation

struct TreeGeometry {
    trunk: MeshBuf,
    foliage: MeshBuf,
    crown_centre: Vec3,
    crown_radii: Vec3,
    height: f32,
}

// Collects foliage cards for a crown, then shades them from the crown's own centre so the
// whole crown reads as one rounded volume rather than a heap of flat cards.
struct CrownBuilder {
    cards: Vec<(Vec3, f32)>,
}

impl CrownBuilder {
    fn new() -> Self {
        CrownBuilder { cards: Vec::new() }
    }

    fn add(&mut self, p: Vec3, size: f32) {
        self.cards.push((p, size));
    }

    fn finish(self, rng: &mut Rng, foliage: &mut MeshBuf) -> (Vec3, Vec3) {
        if self.cards.is_empty() {
            return (Vec3::ZERO, Vec3::ONE);
        }
        let centre = self.cards.iter().map(|c| c.0).sum::<Vec3>() / self.cards.len() as f32;
        let radius = self.cards.iter().map(|c| (c.0 - centre).length()).fold(0.1, f32::max);
        for (p, size) in &self.cards {
            let out = (*p - centre) / radius;
            let facing = (out + Vec3::new(rng.range(-0.5, 0.5), rng.range(-0.3, 0.5), rng.range(-0.5, 0.5))).normalize_or_zero();
            let facing = if facing == Vec3::ZERO { Vec3::Y } else { facing };
            let mut right = facing.cross(Vec3::Y);
            if right.length() < 1e-3 {
                right = Vec3::X;
            }
            let right = Quat::from_axis_angle(facing, rng.range(0.0, TAU)) * right.normalize();
            let up = facing.cross(right);
            let s = size * rng.range(0.85, 1.2);
            // Interior cards are darker, as in a real canopy.
            let tone = (0.62 + 0.5 * out.length().min(1.0) + rng.range(-0.07, 0.07)).clamp(0.45, 1.15);
            let shade = (out * 0.7 + Vec3::Y * 0.5).normalize_or_zero();
            foliage.card(*p, right * s, up * s, shade, tone);
        }
        let mut max = Vec3::splat(0.1);
        for (p, _) in &self.cards {
            max = max.max((*p - centre).abs());
        }
        (centre, max)
    }
}

// A bending branch: returns points and radii tapering from r0 to r1.
fn branch_path(rng: &mut Rng, start: Vec3, dir: Vec3, length: f32, segs: usize, r0: f32, r1: f32, up_bias: f32, wander: f32) -> (Vec<Vec3>, Vec<f32>) {
    let mut pts = vec![start];
    let mut radii = vec![r0];
    let mut d = dir.normalize();
    for i in 1..=segs {
        let jitter = Vec3::new(rng.range(-1.0, 1.0), rng.range(-0.6, 0.6), rng.range(-1.0, 1.0)) * wander;
        d = (d + Vec3::Y * up_bias + jitter).normalize();
        let p = *pts.last().unwrap() + d * (length / segs as f32);
        pts.push(p);
        radii.push(r0 + (r1 - r0) * i as f32 / segs as f32);
    }
    (pts, radii)
}

struct Broadleaf {
    height: f32,
    trunk_r: f32,
    crown_start: f32,
    limbs: (u32, u32),
    limb_len: f32,
    pitch: (f32, f32),
    subs: u32,
    card: f32,
    cards_per_tip: u32,
    droop: f32,
    interior: u32,
}

fn grow_broadleaf(p: &Broadleaf, rng: &mut Rng) -> TreeGeometry {
    let mut trunk = MeshBuf::default();
    let mut foliage = MeshBuf::default();
    let mut crown = CrownBuilder::new();
    let h = p.height * rng.range(0.85, 1.15);
    let trunk_h = h * p.crown_start;

    // Trunk, with a little lean and flare at the base.
    let lean = Vec3::new(rng.range(-0.08, 0.08), 0.0, rng.range(-0.08, 0.08));
    let mut pts = Vec::new();
    let mut radii = Vec::new();
    for i in 0..=6 {
        let t = i as f32 / 6.0;
        pts.push(Vec3::new(0.0, trunk_h * t, 0.0) + lean * trunk_h * t * t + Vec3::new(rng.range(-0.04, 0.04), 0.0, rng.range(-0.04, 0.04)) * t);
        radii.push(p.trunk_r * (1.0 - 0.3 * t) * if i == 0 { 1.35 } else if i == 1 { 1.1 } else { 1.0 });
    }
    let top = *pts.last().unwrap();
    trunk.tube(&pts, &radii, 8);

    // A leader continues up through the crown.
    let (lp, lr) = branch_path(rng, top, Vec3::Y, h - trunk_h, 4, p.trunk_r * 0.62, 0.04, 0.05, 0.12);
    trunk.tube(&lp, &lr, 6);
    let leader_tip = *lp.last().unwrap();
    for _ in 0..p.cards_per_tip {
        crown.add(leader_tip + Vec3::new(rng.range(-0.5, 0.5), rng.range(-0.3, 0.3), rng.range(-0.5, 0.5)), p.card * 0.9);
    }

    let n = rng.range(p.limbs.0 as f32, p.limbs.1 as f32 + 0.99) as u32;
    let yaw0 = rng.range(0.0, TAU);
    for i in 0..n {
        let yaw = yaw0 + i as f32 / n as f32 * TAU + rng.range(-0.3, 0.3);
        let pitch = rng.range(p.pitch.0, p.pitch.1).to_radians();
        let dir = Vec3::new(pitch.sin() * yaw.cos(), pitch.cos(), pitch.sin() * yaw.sin());
        let base_t = rng.range(0.82, 1.0);
        let start = *pts.get(((base_t * 6.0) as usize).min(6)).unwrap_or(&top) + Vec3::new(0.0, rng.range(0.0, 0.12) * h, 0.0);
        let len = p.limb_len * h * rng.range(0.8, 1.15);
        let limb_r = p.trunk_r * rng.range(0.3, 0.42);
        let (bp, br) = branch_path(rng, start, dir, len, 4, limb_r, 0.035, 0.12 - p.droop, 0.16);
        trunk.tube(&bp, &br, 5);
        crown.add(*bp.last().unwrap(), p.card);
        for _ in 1..p.cards_per_tip {
            crown.add(*bp.last().unwrap() + Vec3::new(rng.range(-1.0, 1.0), rng.range(-0.6, 0.8), rng.range(-1.0, 1.0)) * p.card * 0.5, p.card);
        }
        for s in 0..p.subs {
            let t = rng.range(0.35, 0.9);
            let k = ((t * 4.0) as usize).min(3);
            let origin = bp[k].lerp(bp[k + 1], t * 4.0 - k as f32);
            let d0 = (bp[k + 1] - bp[k]).normalize();
            let side = Quat::from_axis_angle(Vec3::Y, rng.range(0.7, 1.4) * if s % 2 == 0 { 1.0 } else { -1.0 }) * d0;
            let sub_dir = (side + Vec3::Y * 0.25).normalize();
            let sub_len = len * rng.range(0.3, 0.5);
            let (sp, sr) = branch_path(rng, origin, sub_dir, sub_len, 3, br[k] * 0.55, 0.025, 0.1 - p.droop, 0.2);
            trunk.tube(&sp, &sr, 4);
            crown.add(*sp.last().unwrap(), p.card * 0.9);
            if p.cards_per_tip > 2 {
                crown.add(sp[sp.len() / 2], p.card * 0.8);
            }
        }
        // A card partway along the limb fills the gaps near the trunk.
        crown.add(bp[2] + Vec3::new(0.0, 0.15 * p.card, 0.0), p.card * 0.8);
    }
    // Interior fill, scattered through the crown ellipsoid.
    let (c0, r0) = {
        let tip_centre = crown.cards.iter().map(|c| c.0).sum::<Vec3>() / crown.cards.len().max(1) as f32;
        let extent = crown.cards.iter().fold(Vec3::splat(0.5), |m, c| m.max((c.0 - tip_centre).abs()));
        (tip_centre, extent)
    };
    for _ in 0..p.interior {
        let v = Vec3::new(rng.range(-1.0, 1.0), rng.range(-0.8, 1.0), rng.range(-1.0, 1.0));
        if v.length() < 1.0 {
            crown.add(c0 + v * r0 * 0.85, p.card * 0.9);
        }
    }
    let (centre, radii3) = crown.finish(rng, &mut foliage);
    TreeGeometry { trunk, foliage, crown_centre: centre, crown_radii: radii3 + Vec3::splat(p.card * 0.5), height: h }
}

fn grow_spruce(rng: &mut Rng) -> TreeGeometry {
    let mut trunk = MeshBuf::default();
    let mut foliage = MeshBuf::default();
    let h = rng.range(13.0, 19.0);
    let r = rng.range(0.22, 0.32);
    let lean = Vec3::new(rng.range(-0.04, 0.04), 0.0, rng.range(-0.04, 0.04));
    let pts: Vec<Vec3> = (0..=8).map(|i| {
        let t = i as f32 / 8.0;
        Vec3::new(0.0, h * t, 0.0) + lean * h * t * t
    }).collect();
    let radii: Vec<f32> = (0..=8).map(|i| {
        let t = i as f32 / 8.0;
        r * (1.0 - 0.93 * t) * if i == 0 { 1.3 } else { 1.0 }
    }).collect();
    trunk.tube(&pts, &radii, 7);
    let trunk_at = |y: f32| {
        let t = (y / h).clamp(0.0, 1.0);
        Vec3::new(0.0, y, 0.0) + lean * h * t * t
    };

    let l_max = 0.2 * h;
    let mut y = 1.4;
    while y < h * 0.97 {
        let t = y / h;
        let len = (l_max * (1.0 - t).powf(0.85) + 0.3) * rng.range(0.9, 1.1);
        let count = if t < 0.6 { 6 } else { 5 };
        let yaw0 = rng.range(0.0, TAU);
        for i in 0..count {
            let yaw = yaw0 + i as f32 / count as f32 * TAU + rng.range(-0.2, 0.2);
            let droop = (28.0 - 22.0 * t).to_radians();
            let dir = Vec3::new(droop.cos() * yaw.cos(), -droop.sin(), droop.cos() * yaw.sin());
            let start = trunk_at(y) + dir * (r * (1.0 - t) * 0.7);
            let tip = start + dir * len;
            trunk.tube(&[start, start.lerp(tip, 0.5) + Vec3::Y * 0.04 * len, tip], &[0.035 + 0.01 * len, 0.025, 0.008], 4);
            // The spray hangs along the branch, widest toward the middle.
            let mid = start + dir * len * 0.55 + Vec3::Y * (-0.05 * len);
            let right = dir.cross(Vec3::Y).normalize_or_zero() * len * 0.55;
            let along = dir * len * 0.6;
            foliage.card(mid, right, along, (dir + Vec3::Y * 0.9).normalize(), rng.range(0.78, 1.05));
            if rng.unit() < 0.55 {
                // A second spray tilted into the vertical plane gives the branch volume.
                let up = Vec3::Y.cross(dir).cross(dir).normalize_or_zero() * len * 0.4;
                let _ = up;
                let tilt = Quat::from_axis_angle(dir, rng.range(0.6, 1.2)) * right;
                foliage.card(mid + Vec3::Y * 0.1 * len, tilt * 0.8, along * 0.9, (dir + Vec3::Y * 0.9).normalize(), rng.range(0.7, 1.0));
            }
        }
        y += rng.range(0.6, 0.95);
    }
    // Leader: two crossed vertical sprays.
    let top = trunk_at(h);
    for k in 0..2 {
        let yaw = k as f32 * FRAC_PI_2 + rng.range(-0.3, 0.3);
        let right = Vec3::new(yaw.cos(), 0.0, yaw.sin()) * 0.55;
        foliage.card(top - Vec3::Y * 0.9, right, Vec3::Y * 1.5, Vec3::Y, 1.0);
    }
    TreeGeometry { trunk, foliage, crown_centre: Vec3::new(0.0, h * 0.45, 0.0), crown_radii: Vec3::new(l_max * 0.95, h * 0.55, l_max * 0.95), height: h }
}

fn grow_pine(rng: &mut Rng) -> TreeGeometry {
    let mut trunk = MeshBuf::default();
    let mut foliage = MeshBuf::default();
    let mut crown = CrownBuilder::new();
    let h = rng.range(12.0, 17.0);
    let r = rng.range(0.22, 0.3);
    let trunk_h = h * rng.range(0.55, 0.65);
    let sway = Vec3::new(rng.range(-0.1, 0.1), 0.0, rng.range(-0.1, 0.1));
    let mut pts = Vec::new();
    let mut radii = Vec::new();
    for i in 0..=8 {
        let t = i as f32 / 8.0;
        pts.push(Vec3::new((t * 5.0).sin() * 0.25, trunk_h * t, (t * 4.0).cos() * 0.2 - 0.2) + sway * trunk_h * t * t);
        radii.push(r * (1.0 - 0.45 * t) * if i == 0 { 1.3 } else { 1.0 });
    }
    trunk.tube(&pts, &radii, 7);
    let top = *pts.last().unwrap();
    let (lp, lr) = branch_path(rng, top, Vec3::Y, h - trunk_h, 3, r * 0.5, 0.03, 0.05, 0.12);
    trunk.tube(&lp, &lr, 5);
    let crown_tip = *lp.last().unwrap();
    crown.add(crown_tip, 2.3);
    crown.add(crown_tip - Vec3::Y * 1.0, 2.4);
    let n = rng.range(7.0, 10.0) as u32;
    let yaw0 = rng.range(0.0, TAU);
    for i in 0..n {
        let yaw = yaw0 + i as f32 / n as f32 * TAU + rng.range(-0.4, 0.4);
        let pitch = rng.range(48.0, 85.0).to_radians();
        let dir = Vec3::new(pitch.sin() * yaw.cos(), pitch.cos(), pitch.sin() * yaw.sin());
        let start = pts[(rng.range(5.0, 8.99)) as usize] + Vec3::Y * rng.range(0.0, 1.5);
        let arm = h * rng.range(0.16, 0.26);
        let (bp, br) = branch_path(rng, start, dir, arm, 3, r * 0.3, 0.03, 0.1, 0.15);
        trunk.tube(&bp, &br, 4);
        let tip = *bp.last().unwrap();
        for _ in 0..3 {
            crown.add(tip + Vec3::new(rng.range(-1.0, 1.0), rng.range(-0.4, 0.8), rng.range(-1.0, 1.0)) * 0.9, rng.range(1.6, 2.2));
        }
        crown.add(bp[2], 1.4);
    }
    let (centre, radii3) = crown.finish(rng, &mut foliage);
    TreeGeometry { trunk, foliage, crown_centre: centre, crown_radii: radii3 + Vec3::splat(1.0), height: h }
}

fn grow_bush(species: Species, rng: &mut Rng) -> TreeGeometry {
    let mut foliage = MeshBuf::default();
    let mut crown = CrownBuilder::new();
    let (radius, height, count, size) = match species {
        Species::Gorse => (rng.range(0.7, 1.2), rng.range(0.6, 1.1), 11, 0.7),
        _ => (rng.range(0.9, 1.5), rng.range(1.0, 1.7), 14, 0.85),
    };
    for _ in 0..count {
        let theta = rng.range(0.0, TAU);
        let rr = rng.unit().sqrt() * radius;
        let y = (1.0 - (rr / radius).powi(2)).max(0.0).sqrt() * height * rng.range(0.55, 1.0) + 0.25;
        crown.add(Vec3::new(theta.cos() * rr, y, theta.sin() * rr), size * rng.range(0.8, 1.15));
    }
    let (centre, radii3) = crown.finish(rng, &mut foliage);
    TreeGeometry { trunk: MeshBuf::default(), foliage, crown_centre: centre, crown_radii: radii3 + Vec3::splat(0.4), height }
}

fn grow(species: Species, seed: u64) -> TreeGeometry {
    let mut rng = Rng::new(seed ^ (species as u64 + 1).wrapping_mul(0x51_7C_C1_B7_27_22_0A_95));
    match species {
        Species::Oak => grow_broadleaf(
            &Broadleaf { height: 15.0, trunk_r: 0.5, crown_start: 0.26, limbs: (5, 7), limb_len: 0.5, pitch: (52.0, 78.0), subs: 4, card: 3.0, cards_per_tip: 3, droop: 0.03, interior: 22 },
            &mut rng,
        ),
        Species::Beech => grow_broadleaf(
            &Broadleaf { height: 17.0, trunk_r: 0.38, crown_start: 0.34, limbs: (6, 8), limb_len: 0.46, pitch: (30.0, 58.0), subs: 3, card: 2.8, cards_per_tip: 3, droop: 0.0, interior: 18 },
            &mut rng,
        ),
        Species::Birch => grow_broadleaf(
            &Broadleaf { height: 13.0, trunk_r: 0.17, crown_start: 0.36, limbs: (6, 9), limb_len: 0.42, pitch: (22.0, 48.0), subs: 3, card: 1.9, cards_per_tip: 3, droop: 0.08, interior: 14 },
            &mut rng,
        ),
        Species::Maple => grow_broadleaf(
            &Broadleaf { height: 12.0, trunk_r: 0.3, crown_start: 0.3, limbs: (5, 7), limb_len: 0.5, pitch: (40.0, 70.0), subs: 3, card: 2.5, cards_per_tip: 3, droop: 0.02, interior: 16 },
            &mut rng,
        ),
        Species::Apple => grow_broadleaf(
            &Broadleaf { height: 4.6, trunk_r: 0.16, crown_start: 0.3, limbs: (5, 6), limb_len: 0.62, pitch: (55.0, 82.0), subs: 2, card: 1.35, cards_per_tip: 2, droop: 0.04, interior: 10 },
            &mut rng,
        ),
        Species::Spruce => grow_spruce(&mut rng),
        Species::Pine => grow_pine(&mut rng),
        Species::Hawthorn | Species::Gorse => grow_bush(species, &mut rng),
    }
}

// ---------------------------------------------------------------------------------------
// Low-poly distant blobs

const ICO_V: [[f32; 3]; 12] = {
    const T: f32 = 1.618034;
    [
        [-1.0, T, 0.0], [1.0, T, 0.0], [-1.0, -T, 0.0], [1.0, -T, 0.0],
        [0.0, -1.0, T], [0.0, 1.0, T], [0.0, -1.0, -T], [0.0, 1.0, -T],
        [T, 0.0, -1.0], [T, 0.0, 1.0], [-T, 0.0, -1.0], [-T, 0.0, 1.0],
    ]
};
const ICO_F: [[u32; 3]; 20] = [
    [0, 11, 5], [0, 5, 1], [0, 1, 7], [0, 7, 10], [0, 10, 11],
    [1, 5, 9], [5, 11, 4], [11, 10, 2], [10, 7, 6], [7, 1, 8],
    [3, 9, 4], [3, 4, 2], [3, 2, 6], [3, 6, 8], [3, 8, 9],
    [4, 9, 5], [2, 4, 11], [6, 2, 10], [8, 6, 7], [9, 8, 1],
];

// Appends a blob for one tree to a chunk's merged mesh: a squashed icosphere for broadleaves,
// a cone for conifers. Vertices carry colour (lighter toward the top), so no texture is needed.
fn push_blob(buf: &mut MeshBuf, species: Species, centre: Vec3, radii: Vec3, height: f32, colour: [f32; 3]) {
    let base = buf.pos.len() as u32;
    let conifer = matches!(species, Species::Spruce | Species::Pine);
    if conifer && species == Species::Spruce {
        // Apex plus a ring of 8, and a lower ring: a faceted cone.
        let rings = [(height, 0.0f32), (height * 0.55, 0.62), (height * 0.16, 1.0)];
        let sides = 8;
        for (ri, &(y, rf)) in rings.iter().enumerate() {
            let count = if ri == 0 { 1 } else { sides };
            for k in 0..count {
                let a = k as f32 / sides as f32 * TAU;
                let p = centre + Vec3::new(a.cos() * radii.x * rf, y, a.sin() * radii.z * rf);
                let shade = 0.7 + 0.5 * (y / height);
                buf.pos.push(p.to_array());
                buf.nor.push((Vec3::new(a.cos() * rf, 0.55, a.sin() * rf)).normalize_or_zero().to_array());
                buf.uv.push([0.0, 0.0]);
                buf.col.push([colour[0] * shade, colour[1] * shade, colour[2] * shade, 1.0]);
            }
        }
        // Apex fan, then side bands.
        for k in 0..sides as u32 {
            buf.idx.extend_from_slice(&[base, base + 1 + (k + 1) % sides as u32, base + 1 + k]);
        }
        for k in 0..sides as u32 {
            let (a, b) = (base + 1 + k, base + 1 + (k + 1) % sides as u32);
            let (c, d) = (a + sides as u32, b + sides as u32);
            buf.idx.extend_from_slice(&[a, b, c, b, d, c]);
        }
        return;
    }
    // Larger crowns get a once-subdivided icosphere with noisy radii so they read as lumpy
    // foliage; small ones stay at 12 vertices to keep the merged chunk meshes light.
    let (verts, faces) = if matches!(species, Species::Apple) { (&ico().0, &ico().1) } else { (&ico_fine().0, &ico_fine().1) };
    let seed = (centre.x * 31.7 + centre.z * 17.3) as i32;
    for (i, n) in verts.iter().enumerate() {
        let lump = 0.88 + 0.24 * hash2(seed, i as i32, 77);
        let p = centre + *n * radii * lump;
        let shade = 0.68 + 0.45 * (0.5 + 0.5 * n.y) + 0.12 * (hash2(seed, i as i32, 78) - 0.5);
        buf.pos.push(p.to_array());
        buf.nor.push((*n / radii.max(Vec3::splat(0.01))).normalize_or_zero().to_array());
        buf.uv.push([0.0, 0.0]);
        buf.col.push([colour[0] * shade, colour[1] * shade, colour[2] * shade, 1.0]);
    }
    for f in faces {
        buf.idx.extend_from_slice(&[base + f[0], base + f[1], base + f[2]]);
    }
}

fn ico() -> &'static (Vec<Vec3>, Vec<[u32; 3]>) {
    static ICO: std::sync::OnceLock<(Vec<Vec3>, Vec<[u32; 3]>)> = std::sync::OnceLock::new();
    ICO.get_or_init(|| (ICO_V.iter().map(|v| Vec3::from(*v).normalize()).collect(), ICO_F.to_vec()))
}

// The icosahedron with each triangle split in four (42 vertices, 80 faces).
fn ico_fine() -> &'static (Vec<Vec3>, Vec<[u32; 3]>) {
    static FINE: std::sync::OnceLock<(Vec<Vec3>, Vec<[u32; 3]>)> = std::sync::OnceLock::new();
    FINE.get_or_init(|| {
        let (v0, f0) = ico();
        let mut verts = v0.clone();
        let mut midpoints: HashMap<(u32, u32), u32> = HashMap::new();
        let mut mid = |a: u32, b: u32, verts: &mut Vec<Vec3>| {
            let key = (a.min(b), a.max(b));
            *midpoints.entry(key).or_insert_with(|| {
                verts.push(((verts[a as usize] + verts[b as usize]) * 0.5).normalize());
                verts.len() as u32 - 1
            })
        };
        let mut faces = Vec::new();
        for f in f0 {
            let (ab, bc, ca) = (mid(f[0], f[1], &mut verts), mid(f[1], f[2], &mut verts), mid(f[2], f[0], &mut verts));
            faces.extend_from_slice(&[[f[0], ab, ca], [f[1], bc, ab], [f[2], ca, bc], [ab, bc, ca]]);
        }
        (verts, faces)
    })
}

// ---------------------------------------------------------------------------------------
// Assets, planning and streaming

struct Variant {
    trunk: Option<Handle<Mesh>>,
    foliage: Handle<Mesh>,
}

struct SpeciesAssets {
    variants: Vec<Variant>,
    bark: Handle<WindMaterial>,
    leaves: Vec<Handle<WindMaterial>>,
}

#[derive(Resource)]
pub struct VegetationAssets {
    species: HashMap<Species, SpeciesAssets>,
}

fn build_vegetation_assets(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<WindMaterial>>,
    mut mips: ResMut<MipQueue>,
) {
    let mut load = |path: &'static str, repeat: bool, srgb: bool| {
        let handle: Handle<Image> = asset_server
            .load_builder()
            .with_settings(move |settings: &mut ImageLoaderSettings| {
                settings.is_srgb = srgb;
                let mode = if repeat { ImageAddressMode::Repeat } else { ImageAddressMode::ClampToEdge };
                settings.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                    address_mode_u: mode,
                    address_mode_v: mode,
                    anisotropy_clamp: 8,
                    ..ImageSamplerDescriptor::linear()
                });
            })
            .load(path);
        mips.0.push(handle.clone());
        handle
    };

    let mut barks: HashMap<&'static str, Handle<WindMaterial>> = HashMap::new();
    let mut leaf_textures: HashMap<&'static str, Handle<Image>> = HashMap::new();
    let mut species = HashMap::new();
    for sp in Species::ALL {
        let bark = barks
            .entry(sp.bark())
            .or_insert_with(|| {
                let tex = load(sp.bark(), true, true);
                let normal = load(sp.bark_normal(), true, false);
                materials.add(WindMaterial {
                    base: StandardMaterial {
                        base_color_texture: Some(tex),
                        normal_map_texture: Some(normal),
                        base_color: Color::srgb(0.75, 0.72, 0.68),
                        perceptual_roughness: 0.95,
                        ..default()
                    },
                    extension: WindExtension::new(sp.sway(), sp.wind_height(), 0.0),
                })
            })
            .clone();
        let leaf_tex = leaf_textures.entry(sp.leaves()).or_insert_with(|| load(sp.leaves(), false, true)).clone();
        let t = sp.leaf_tint();
        let leaves = [0.88_f32, 1.0, 1.12]
            .iter()
            .map(|&k| {
                materials.add(WindMaterial {
                    base: StandardMaterial {
                        base_color_texture: Some(leaf_tex.clone()),
                        base_color: Color::srgb((t[0] * k).min(1.0), (t[1] * k).min(1.0), (t[2] * k).min(1.0)),
                        alpha_mode: AlphaMode::Mask(0.42),
                        cull_mode: None,
                        double_sided: false,
                        perceptual_roughness: 0.8,
                        reflectance: 0.08,
                        ..default()
                    },
                    extension: WindExtension::new(sp.sway(), sp.wind_height(), sp.flutter()),
                })
            })
            .collect();
        let variants = (0..VARIANTS)
            .map(|v| {
                let g = grow(sp, v as u64 * 7919 + 13);
                let trunk = (!g.trunk.pos.is_empty()).then(|| {
                    // Tangents are what let the bark's normal map light correctly.
                    let mesh = g.trunk.into_mesh(true);
                    meshes.add(mesh.clone().with_generated_tangents().unwrap_or(mesh))
                });
                Variant { trunk, foliage: meshes.add(g.foliage.into_mesh(true)) }
            })
            .collect();
        species.insert(sp, SpeciesAssets { variants, bark, leaves });
    }
    commands.insert_resource(VegetationAssets { species });
}

#[derive(Clone, Copy)]
struct Instance {
    species: Species,
    variant: u8,
    tone: u8,
    pos: Vec3,
    yaw: f32,
    scale: f32,
}

#[derive(Resource, Default)]
pub struct VegetationPlan {
    chunks: HashMap<(i32, i32), Vec<Instance>>,
    spawned: HashMap<(i32, i32), Vec<Entity>>,
    // The always-visible blob mesh for each chunk, hidden while its detailed trees exist.
    blobs: HashMap<(i32, i32), Entity>,
    last_camera: Option<Vec3>,
}

impl VegetationPlan {
    /// Centre of the chunk holding the most trees, for benchmarks and diagnostics.
    pub fn densest_chunk_centre(&self) -> Option<Vec2> {
        self.chunks
            .iter()
            .max_by_key(|(_, trees)| trees.len())
            .map(|(k, _)| Vec2::new((k.0 as f32 + 0.5) * CHUNK, (k.1 as f32 + 0.5) * CHUNK))
    }

    /// World-space positions and species of every planned tree, for diagnostics and tests.
    pub fn instances(&self) -> Vec<(Species, Vec3)> {
        self.chunks.values().flatten().map(|i| (i.species, i.pos)).collect()
    }

    /// Every tree's trunk is solid (a person walks round it, and can't climb it); shrubs are not,
    /// as they are pushed through.
    pub fn add_colliders(&self, colliders: &mut crate::collision::Colliders) {
        for tree in self.chunks.values().flatten().filter(|t| !t.species.is_bush()) {
            colliders.add(
                crate::collision::Solid::circle(Vec2::new(tree.pos.x, tree.pos.z), tree.species.trunk_radius() * tree.scale * 1.15, tree.pos.y + TREE_SOLID_HEIGHT)
                    .of(crate::collision::Material::Wood),
            );
        }
    }

    pub fn tree_count(&self) -> usize {
        self.chunks.values().map(Vec::len).sum()
    }

    fn add(&mut self, inst: Instance) {
        let key = ((inst.pos.x / CHUNK).floor() as i32, (inst.pos.z / CHUNK).floor() as i32);
        self.chunks.entry(key).or_default().push(inst);
    }
}

fn ground_at(map: &TerrainMap, p: Vec2) -> Vec3 {
    Vec3::new(p.x, map.height_at(p), p.y)
}

fn zone_at(map: &TerrainMap, zones: &ZoneMap, p: Vec2) -> Zone {
    let n = map.grid_size();
    let ix = (((p.x + HALF_SIZE) / CELL).round().max(0.0) as usize).min(n - 1);
    let iz = (((p.y + HALF_SIZE) / CELL).round().max(0.0) as usize).min(n - 1);
    zones.zone_at(ix, iz)
}

/// How high a trunk is for collision: well above anything a person can climb.
const TREE_SOLID_HEIGHT: f32 = 6.0;

/// Decides where every tree and shrub goes. `hedge_points` are spots along hedgerows where an
/// occasional full-size tree stands.
pub fn plan_vegetation(map: &TerrainMap, zones: &ZoneMap, hedge_points: &[Vec2]) -> VegetationPlan {
    let mut plan = VegetationPlan::default();
    let seed = map.seed;
    let place = |plan: &mut VegetationPlan, species: Species, p: Vec2, a: i32, b: i32| {
        let size = 0.78 + 0.5 * hash2(a, b, 4);
        plan.add(Instance {
            species,
            variant: (hash2(a, b, 5) * VARIANTS as f32) as u8 % VARIANTS as u8,
            tone: (hash2(a, b, 6) * 3.0) as u8 % 3,
            pos: ground_at(map, p),
            yaw: hash2(a, b, 7) * TAU,
            scale: size,
        });
    };

    // A jittered grid per land use, thinned by low-frequency noise into thickets and clearings.
    let scan = |spacing: f32, salt: u64, f: &mut dyn FnMut(Vec2, i32, i32, f32)| {
        let n = (MAP_SIZE / spacing) as i32;
        for iz in 0..n {
            for ix in 0..n {
                let jx = hash2(ix, iz, salt) - 0.5;
                let jz = hash2(ix, iz, salt + 1) - 0.5;
                let p = Vec2::new(
                    -HALF_SIZE + (ix as f32 + 0.5 + jx * 0.8) * spacing,
                    -HALF_SIZE + (iz as f32 + 0.5 + jz * 0.8) * spacing,
                );
                let density = fbm(p.x / 140.0, p.y / 140.0, seed ^ 0x7733, 3);
                f(p, ix, iz, density);
            }
        }
    };

    scan(8.5, 11, &mut |p, ix, iz, density| match zone_at(map, zones, p) {
        Zone::Woodland if hash2(ix, iz, 12) < 0.35 + 0.9 * density => {
            let mix = fbm(p.x / 90.0, p.y / 90.0, seed ^ 0x51, 2) + 0.25 * (hash2(ix, iz, 13) - 0.5);
            let species = match mix {
                m if m < 0.38 => Species::Oak,
                m if m < 0.58 => Species::Beech,
                m if m < 0.74 => Species::Birch,
                m if m < 0.80 => Species::Maple,
                _ => Species::Oak,
            };
            place(&mut plan, species, p, ix, iz);
        }
        _ => {}
    });
    scan(6.0, 21, &mut |p, ix, iz, density| {
        if zone_at(map, zones, p) == Zone::Conifer && hash2(ix, iz, 22) < 0.4 + 0.8 * density {
            let species = if fbm(p.x / 110.0, p.y / 110.0, seed ^ 0x52, 2) < 0.55 { Species::Spruce } else { Species::Pine };
            place(&mut plan, species, p, ix, iz);
        }
    });
    // Orchards are planted in rows on a regular lattice.
    scan(7.0, 31, &mut |_, ix, iz, _| {
        let lattice = Vec2::new(-HALF_SIZE + (ix as f32 + 0.5) * 7.0, -HALF_SIZE + (iz as f32 + 0.5) * 7.0);
        if zone_at(map, zones, lattice) == Zone::Orchard && hash2(ix, iz, 32) < 0.92 {
            place(&mut plan, Species::Apple, lattice, ix, iz);
        }
    });
    // Undergrowth in woods, and scrub on rough ground.
    scan(7.5, 41, &mut |p, ix, iz, density| {
        let zone = zone_at(map, zones, p);
        // Shrubs thicken into proper underbrush in woods, and thin out under conifers (where
        // little grows in the shade of the needles).
        let chance = match zone {
            Zone::Woodland => 0.42 + 0.45 * density,
            Zone::Wetland => 0.28 + 0.3 * density,
            Zone::Conifer => 0.07 + 0.12 * density,
            _ => 0.0,
        };
        if chance > 0.0 && hash2(ix, iz, 42) < chance {
            let sp = if hash2(ix, iz, 43) < 0.7 { Species::Hawthorn } else { Species::Gorse };
            place(&mut plan, sp, p, ix, iz);
        }
    });
    for (i, &p) in hedge_points.iter().enumerate() {
        let species = match hash2(i as i32, 0, 51) {
            h if h < 0.6 => Species::Oak,
            h if h < 0.85 => Species::Beech,
            _ => Species::Birch,
        };
        place(&mut plan, species, p, i as i32, 91);
    }
    plan
}

/// Spawns the always-visible low-poly blobs, merged per chunk, and registers the plan so
/// detailed trees stream in near the camera.
pub fn spawn_vegetation(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    map: &TerrainMap,
    zones: &ZoneMap,
    hedge_points: &[Vec2],
    colliders: &mut crate::collision::Colliders,
) {
    let plan = plan_vegetation(map, zones, hedge_points);
    plan.add_colliders(colliders);
    // The blob dimensions come from the same generated variants the detailed trees use.
    let blob_dims: HashMap<Species, Vec<(Vec3, Vec3, f32)>> = Species::ALL
        .iter()
        .filter(|s| !s.is_bush())
        .map(|&sp| {
            let dims = (0..VARIANTS)
                .map(|v| {
                    let g = grow(sp, v as u64 * 7919 + 13);
                    (g.crown_centre, g.crown_radii, g.height)
                })
                .collect();
            (sp, dims)
        })
        .collect();
    let material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 1.0,
        reflectance: 0.05,
        ..default()
    });
    let mut blob_entities = HashMap::new();
    for (key, instances) in &plan.chunks {
        let mut buf = MeshBuf::default();
        for inst in instances {
            let Some(dims) = blob_dims.get(&inst.species) else { continue };
            let (centre, radii, height) = dims[inst.variant as usize % dims.len()];
            let rot = Quat::from_rotation_y(inst.yaw);
            let shade = 0.85 + 0.3 * (inst.tone as f32 / 2.0);
            let c = inst.species.blob_colour().map(|v| v * shade * 0.6);
            // Slightly shrunk so close-up detailed crowns poke out through the blob.
            let world_centre = inst.pos + rot * (centre * inst.scale);
            // Cones are built up from the tree's base; the rounded blobs sit on the crown centre.
            let anchor = if inst.species == Species::Spruce { inst.pos } else { world_centre };
            push_blob(&mut buf, inst.species, anchor, radii * inst.scale * 0.9, height * inst.scale, c);
        }
        if buf.pos.is_empty() {
            continue;
        }
        let entity = commands
            .spawn((TerrainRoot, bevy::light::NotShadowCaster, Mesh3d(meshes.add(buf.into_mesh(false))), MeshMaterial3d(material.clone())))
            .id();
        blob_entities.insert(*key, entity);
    }
    let mut plan = plan;
    plan.blobs = blob_entities;
    commands.insert_resource(plan);
}

fn stream_detailed_trees(
    mut commands: Commands,
    assets: Option<Res<VegetationAssets>>,
    plan: Option<ResMut<VegetationPlan>>,
    cameras: Query<&GlobalTransform, With<Camera3d>>,
) {
    let (Some(assets), Some(mut plan)) = (assets, plan) else { return };
    let Some(camera) = cameras.iter().next() else { return };
    let eye = camera.translation();
    if let Some(last) = plan.last_camera {
        if last.distance(eye) < 12.0 {
            return;
        }
    }
    plan.last_camera = Some(eye);

    let centre_of = |key: (i32, i32)| Vec2::new((key.0 as f32 + 0.5) * CHUNK, (key.1 as f32 + 0.5) * CHUNK);
    let here = Vec2::new(eye.x, eye.z);

    let stale: Vec<(i32, i32)> = plan
        .spawned
        .keys()
        .copied()
        .filter(|&k| centre_of(k).distance(here) > STREAM_OUT)
        .collect();
    for key in stale {
        for e in plan.spawned.remove(&key).unwrap_or_default() {
            commands.entity(e).despawn();
        }
        if let Some(&blob) = plan.blobs.get(&key) {
            commands.entity(blob).insert(Visibility::Inherited);
        }
    }

    let wanted: Vec<(i32, i32)> = plan
        .chunks
        .keys()
        .copied()
        .filter(|k| !plan.spawned.contains_key(k) && centre_of(*k).distance(here) < STREAM_IN)
        .collect();
    for key in wanted {
        let mut entities = Vec::new();
        for inst in &plan.chunks[&key] {
            let Some(sp) = assets.species.get(&inst.species) else { continue };
            let variant = &sp.variants[inst.variant as usize % sp.variants.len()];
            let transform = Transform::from_translation(inst.pos)
                .with_rotation(Quat::from_rotation_y(inst.yaw))
                .with_scale(Vec3::splat(inst.scale));
            if let Some(trunk) = &variant.trunk {
                entities.push(commands.spawn((TerrainRoot, Mesh3d(trunk.clone()), MeshMaterial3d(sp.bark.clone()), transform)).id());
            }
            entities.push(
                commands
                    .spawn((
                        TerrainRoot,
                        Mesh3d(variant.foliage.clone()),
                        MeshMaterial3d(sp.leaves[inst.tone as usize % sp.leaves.len()].clone()),
                        transform,
                    ))
                    .id(),
            );
        }
        if let Some(&blob) = plan.blobs.get(&key) {
            commands.entity(blob).insert(Visibility::Hidden);
        }
        plan.spawned.insert(key, entities);
    }
}

#[allow(dead_code)]
const _: f32 = PI;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::GenParams;

    #[test]
    fn every_species_grows_sane_geometry() {
        for sp in Species::ALL {
            for seed in 0..VARIANTS as u64 {
                let g = grow(sp, seed * 7919 + 13);
                assert!(!g.foliage.pos.is_empty(), "{sp:?} has no foliage");
                assert_eq!(g.trunk.pos.is_empty(), sp.is_bush(), "{sp:?}: only bushes lack a trunk");
                for buf in [&g.trunk, &g.foliage] {
                    assert_eq!(buf.pos.len(), buf.nor.len());
                    assert_eq!(buf.pos.len(), buf.uv.len());
                    assert_eq!(buf.pos.len(), buf.col.len());
                    assert!(buf.idx.iter().all(|&i| (i as usize) < buf.pos.len()), "{sp:?}: index out of range");
                    assert!(buf.pos.iter().flatten().all(|v| v.is_finite()), "{sp:?}: non-finite vertex");
                    assert!(buf.nor.iter().flatten().all(|v| v.is_finite()), "{sp:?}: non-finite normal");
                }
                let top = g.foliage.pos.iter().chain(&g.trunk.pos).map(|p| p[1]).fold(f32::MIN, f32::max);
                assert!(top > 0.5 && top < 30.0, "{sp:?}: implausible height {top}");
                assert!(g.crown_radii.min_element() > 0.0 && g.crown_radii.max_element() < 20.0);
            }
        }
    }

    #[test]
    fn species_grow_to_believable_sizes() {
        let height = |sp: Species| (0..VARIANTS as u64).map(|s| grow(sp, s).height).sum::<f32>() / VARIANTS as f32;
        assert!(height(Species::Oak) > 10.0 && height(Species::Oak) < 20.0);
        assert!(height(Species::Spruce) > 12.0);
        assert!(height(Species::Apple) < 7.0, "orchard trees are small");
        assert!(height(Species::Hawthorn) < 3.0, "bushes are low");
    }

    #[test]
    fn plan_puts_trees_in_the_right_zones_and_is_deterministic() {
        let params = GenParams::default();
        let map = TerrainMap::generate(crate::MAP_SEED, &params);
        let zones = ZoneMap::generate(&map, &params);
        let plan = plan_vegetation(&map, &zones, &[]);
        let again = plan_vegetation(&map, &zones, &[]);
        assert_eq!(plan.tree_count(), again.tree_count());
        assert!(plan.tree_count() > 10_000, "only {} trees", plan.tree_count());
        for (species, pos) in plan.instances() {
            let zone = zone_at(&map, &zones, Vec2::new(pos.x, pos.z));
            let ok = match species {
                Species::Oak | Species::Beech | Species::Birch | Species::Maple => zone == Zone::Woodland,
                Species::Spruce | Species::Pine => zone == Zone::Conifer,
                Species::Apple => zone == Zone::Orchard,
                Species::Hawthorn | Species::Gorse => matches!(zone, Zone::Woodland | Zone::Wetland | Zone::Conifer),
            };
            assert!(ok, "{species:?} planted in {zone:?}");
            assert!((pos.y - map.height_at(Vec2::new(pos.x, pos.z))).abs() < 0.01, "{species:?} is off the ground");
        }
    }

    #[test]
    fn hedgerow_trees_are_planted_where_asked() {
        let params = GenParams::default();
        let map = TerrainMap::generate(crate::MAP_SEED, &params);
        let zones = ZoneMap::generate(&map, &params);
        let base = plan_vegetation(&map, &zones, &[]).tree_count();
        let points: Vec<Vec2> = (0..50).map(|i| Vec2::new(i as f32 * 20.0 - 500.0, 100.0)).collect();
        assert_eq!(plan_vegetation(&map, &zones, &points).tree_count(), base + 50);
    }
}

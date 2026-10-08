// Grass and ground cover close to the camera.
//
// Tufts of real-looking grass are grown on meadow, pasture, rough ground and verges within
// about 110 m of the camera, and removed again as it moves away. Each 6 m square of ground gets
// one merged mesh, so there are only ever about a thousand entities no matter how many tens of
// thousands of tufts, and the blades sway in the same wind as the trees. To reach that far the
// grass thins with distance: near cells have every tuft, middle cells keep two in five, far cells
// one in six, and the tufts that stay are bigger, so the ground still looks covered. Beyond that
// range the field textures carry the look.

use std::collections::HashMap;
use std::f32::consts::TAU;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::map::{grid_pos, TerrainMap, CELL, HALF_SIZE};
use crate::mipmaps::MipQueue;
use crate::terrain::TerrainRoot;
use crate::wind_material::{WindExtension, WindMaterial};

const GRASS_CELL: f32 = 6.0;
/// Cells start to grow this far out, and are removed again at `STREAM_OUT`.
const STREAM_IN: f32 = 112.0;
const STREAM_OUT: f32 = 126.0;
const MAX_NEW_CELLS_PER_FRAME: usize = 6;
/// Candidate tuft positions tried per square metre (each is kept with the cover's density / this).
const ATTEMPTS_PER_M2: f32 = 4.0;

/// How a grass cell is built, by how far it is from the camera.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    Near,
    Mid,
    Far,
}

impl Tier {
    /// The fraction of a cell's tufts that are kept. The tufts of a thinner tier are always a
    /// subset of a denser one's (the same random number decides each), so a cell changing tier
    /// doesn't reshuffle its grass, it just gains or loses some tufts.
    pub fn keep(self) -> f32 {
        match self {
            Tier::Near => 1.0,
            Tier::Mid => 0.4,
            Tier::Far => 0.16,
        }
    }

    /// How much bigger each tuft is, to make up for there being fewer of them.
    pub fn size(self) -> f32 {
        match self {
            Tier::Near => 1.0,
            Tier::Mid => 1.35,
            Tier::Far => 1.8,
        }
    }

    /// Crossed quads per tuft: three read as round from any side, two do from afar.
    pub fn quads(self) -> usize {
        if self == Tier::Far { 2 } else { 3 }
    }
}

const NEAR_LIMIT: f32 = 45.0;
const MID_LIMIT: f32 = 80.0;
/// A cell already in a tier stays in it this much further out, so that moving about a band's
/// edge doesn't rebuild cells back and forth.
const HYSTERESIS: f32 = 7.0;

/// The tier a cell `distance` metres away should be in, given the tier it is in now, or `None`
/// if it is too far for grass.
pub fn tier_for(distance: f32, current: Option<Tier>) -> Option<Tier> {
    let stay = |tier| if current == Some(tier) { HYSTERESIS } else { 0.0 };
    if distance < NEAR_LIMIT + stay(Tier::Near) {
        Some(Tier::Near)
    } else if distance < MID_LIMIT + stay(Tier::Mid) {
        Some(Tier::Mid)
    } else if distance < STREAM_IN + stay(Tier::Far) {
        Some(Tier::Far)
    } else {
        None
    }
}

pub struct GrassPlugin;

impl Plugin for GrassPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GrassState>()
            .add_systems(Startup, load_grass.after(crate::terrain::load_textures))
            .add_systems(Update, stream_grass);
    }
}

/// What grows on each terrain vertex.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Cover {
    None = 0,
    /// Tall hay meadow.
    Meadow = 1,
    /// Short grazed pasture.
    Pasture = 2,
    /// Rough, tussocky grazing.
    Rough = 3,
    /// Sparse grass on verges and open ground.
    Verge = 4,
    /// The floor of a wood: ferns and sedges, ivy mats, leaf rosettes, a little grass.
    Understory = 5,
}

#[derive(Resource)]
pub struct GroundCover {
    n: usize,
    kinds: Vec<u8>,
}

impl GroundCover {
    pub fn new(n: usize, kinds: Vec<u8>) -> Self {
        assert_eq!(kinds.len(), n * n);
        GroundCover { n, kinds }
    }

    /// The middle of a patch of `want` cover at least three cells across, nearest the map
    /// centre (for benchmarks and screenshots).
    pub fn find(&self, want: Cover) -> Option<Vec2> {
        let n = self.n;
        let mut best: Option<(f32, Vec2)> = None;
        for iz in 4..n - 4 {
            for ix in 4..n - 4 {
                let all = (-1..=1isize).all(|dz| (-1..=1isize).all(|dx| self.kinds[(iz as isize + dz) as usize * n + (ix as isize + dx) as usize] == want as u8));
                if all {
                    let p = grid_pos(ix, iz);
                    if best.map_or(true, |(d, _)| p.length() < d) {
                        best = Some((p.length(), p));
                    }
                }
            }
        }
        best.map(|b| b.1)
    }

    pub fn at(&self, p: Vec2) -> Cover {
        let ix = (((p.x + HALF_SIZE) / CELL).round().max(0.0) as usize).min(self.n - 1);
        let iz = (((p.y + HALF_SIZE) / CELL).round().max(0.0) as usize).min(self.n - 1);
        match self.kinds[iz * self.n + ix] {
            1 => Cover::Meadow,
            2 => Cover::Pasture,
            3 => Cover::Rough,
            4 => Cover::Verge,
            5 => Cover::Understory,
            _ => Cover::None,
        }
    }
}

#[derive(Resource)]
struct GrassAssets {
    material: Handle<WindMaterial>,
}

struct GrassCell {
    tier: Tier,
    // None means the cell was checked and has no grass, so it isn't retried every frame.
    entity: Option<Entity>,
}

#[derive(Resource, Default)]
struct GrassState {
    cells: HashMap<(i32, i32), GrassCell>,
}

fn load_grass(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut materials: ResMut<Assets<WindMaterial>>,
    mut mips: ResMut<MipQueue>,
) {
    let texture: Handle<Image> = asset_server
        .load_builder()
        .with_settings(|settings: &mut ImageLoaderSettings| {
            settings.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                address_mode_u: ImageAddressMode::ClampToEdge,
                address_mode_v: ImageAddressMode::ClampToEdge,
                anisotropy_clamp: 8,
                ..ImageSamplerDescriptor::linear()
            });
        })
        .load("textures/veg/ground_plants.png");
    mips.0.push(texture.clone());
    let material = materials.add(WindMaterial {
        base: StandardMaterial {
            base_color_texture: Some(texture),
            base_color: Color::srgb(0.62, 0.74, 0.42),
            alpha_mode: AlphaMode::Mask(0.4),
            cull_mode: None,
            double_sided: false,
            perceptual_roughness: 0.9,
            reflectance: 0.05,
            ..default()
        },
        // Sways more toward the blade tips; reaches full sway at about a metre.
        extension: WindExtension::new(0.16, 1.0, 0.02),
    });
    commands.insert_resource(GrassAssets { material });
}

fn hash(x: i32, z: i32, salt: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (z as u32).wrapping_mul(0x85EB_CA6B) ^ salt.wrapping_mul(0xC2B2_AE35);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h >> 8) as f32 / (1u32 << 24) as f32
}

// Tufts per square metre, and height range, by kind of cover.
fn tuft_profile(cover: Cover) -> (f32, f32, f32) {
    match cover {
        Cover::Meadow => (3.6, 0.45, 0.8),
        Cover::Pasture => (4.0, 0.2, 0.36),
        Cover::Rough => (2.4, 0.55, 1.0),
        Cover::Verge => (1.4, 0.3, 0.6),
        Cover::Understory => (2.8, 0.3, 0.6),
        Cover::None => (0.0, 0.0, 0.0),
    }
}

// The cards in the ground-plants atlas, left to right.
const CARD_GRASS: u32 = 0;
const CARD_FERN: u32 = 1;
const CARD_IVY: u32 = 2;
const CARD_ROSETTE: u32 = 3;

// Which card a tuft uses, from a 0..1 roll, and its (height scale, width / height).
fn pick_card(cover: Cover, roll: f32) -> (u32, f32, f32) {
    if cover != Cover::Understory {
        return (CARD_GRASS, 1.0, 0.5);
    }
    match roll {
        r if r < 0.38 => (CARD_FERN, 1.35, 1.25),
        r if r < 0.62 => (CARD_IVY, 0.9, 1.0),
        r if r < 0.82 => (CARD_ROSETTE, 0.8, 1.15),
        _ => (CARD_GRASS, 1.0, 0.5),
    }
}

fn build_cell_mesh(map: &TerrainMap, cover: &GroundCover, cx: i32, cz: i32, tier: Tier) -> Option<Mesh> {
    let origin = Vec2::new(cx as f32, cz as f32) * GRASS_CELL;
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    let attempts = (GRASS_CELL * GRASS_CELL * ATTEMPTS_PER_M2) as i32;
    for k in 0..attempts {
        let p = origin + Vec2::new(hash(cx, cz, k as u32 * 4 + 1), hash(cx, cz, k as u32 * 4 + 2)) * GRASS_CELL;
        if p.x.abs() > HALF_SIZE || p.y.abs() > HALF_SIZE {
            continue;
        }
        let kind = cover.at(p);
        let (density, h_lo, h_hi) = tuft_profile(kind);
        // `attempts` candidates per cell stand in for ATTEMPTS_PER_M2 per square metre; thin to the density.
        if hash(cx, cz, k as u32 * 4 + 3) > density / ATTEMPTS_PER_M2 {
            continue;
        }
        // Further out, fewer of them (the same ones every time, see `Tier::keep`).
        if hash(cx, cz, k as u32 * 4 + 5) > tier.keep() {
            continue;
        }
        // Rest on the rendered ground: the terrain mesh's own surface, and above the field
        // fill (which is laid a little above the terrain) wherever there is any.
        let lift = if matches!(kind, Cover::Verge) { 0.0 } else { crate::fill::FIELD_LIFT };
        let ground = map.surface_height_at(p) + lift;
        // Nothing grows under water.
        if map.water_level(
            (((p.x + HALF_SIZE) / CELL).round() as usize).min(map.grid_size() - 1),
            (((p.y + HALF_SIZE) / CELL).round() as usize).min(map.grid_size() - 1),
        )
        .is_some()
        {
            continue;
        }
        let (card, height_scale, aspect) = pick_card(kind, hash(cx, cz, k as u32 * 9 + 300));
        let height = (h_lo + (h_hi - h_lo) * hash(cx, cz, k as u32 * 4 + 4)) * height_scale * tier.size();
        let half_width = height * aspect * 1.15;
        let (u0, u1) = (card as f32 * 0.25, card as f32 * 0.25 + 0.25);
        let yaw0 = hash(cx, cz, k as u32 * 7 + 100) * TAU;
        let tone = 0.75 + 0.4 * hash(cx, cz, k as u32 * 7 + 101);
        // Crossed quads read as a round tuft from any side.
        let quads = tier.quads();
        for q in 0..quads {
            let yaw = yaw0 + q as f32 * TAU / (2 * quads) as f32;
            let along = Vec3::new(yaw.cos(), 0.0, yaw.sin()) * half_width;
            let base = Vec3::new(p.x, ground - 0.03, p.y);
            let top = base + Vec3::Y * height;
            let first = positions.len() as u32;
            for (corner, uv) in [(base - along, [u0, 1.0]), (base + along, [u1, 1.0]), (top + along, [u1, 0.0]), (top - along, [u0, 0.0])] {
                positions.push(corner.to_array());
                normals.push([0.0, 1.0, 0.0]);
                uvs.push(uv);
                colors.push([tone, tone, tone, 1.0]);
            }
            indices.extend_from_slice(&[first, first + 1, first + 2, first, first + 2, first + 3]);
        }
    }
    if positions.is_empty() {
        return None;
    }
    Some(
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
            .with_inserted_indices(Indices::U32(indices)),
    )
}

fn stream_grass(
    mut commands: Commands,
    assets: Option<Res<GrassAssets>>,
    cover: Option<Res<GroundCover>>,
    map: Res<TerrainMap>,
    mut state: ResMut<GrassState>,
    mut meshes: ResMut<Assets<Mesh>>,
    cameras: Query<&GlobalTransform, With<Camera3d>>,
) {
    let (Some(assets), Some(cover)) = (assets, cover) else { return };
    // A new world replaces the ground cover; its old entities were already despawned with it.
    if cover.is_changed() {
        state.cells.clear();
    }
    let Some(camera) = cameras.iter().next() else { return };
    let here = Vec2::new(camera.translation().x, camera.translation().z);
    let centre = |key: (i32, i32)| Vec2::new(key.0 as f32 + 0.5, key.1 as f32 + 0.5) * GRASS_CELL;

    let stale: Vec<(i32, i32)> = state.cells.keys().copied().filter(|&key| centre(key).distance(here) > STREAM_OUT).collect();
    for key in stale {
        if let Some(GrassCell { entity: Some(entity), .. }) = state.cells.remove(&key) {
            commands.entity(entity).despawn();
        }
    }

    // Cells that are missing, or are in the wrong tier for how far away they now are.
    let reach = (STREAM_IN / GRASS_CELL).ceil() as i32;
    let (hx, hz) = ((here.x / GRASS_CELL).floor() as i32, (here.y / GRASS_CELL).floor() as i32);
    let mut wanted: Vec<((i32, i32), Tier, f32)> = Vec::new();
    for dz in -reach..=reach {
        for dx in -reach..=reach {
            let key = (hx + dx, hz + dz);
            let d = centre(key).distance(here);
            let current = state.cells.get(&key).map(|c| c.tier);
            if let Some(tier) = tier_for(d, current) {
                if current != Some(tier) {
                    wanted.push((key, tier, d));
                }
            }
        }
    }
    // Nearest first, and only a few per frame so moving never hitches.
    wanted.sort_by(|a, b| a.2.total_cmp(&b.2));
    for (key, tier, _) in wanted.into_iter().take(MAX_NEW_CELLS_PER_FRAME) {
        if let Some(GrassCell { entity: Some(old), .. }) = state.cells.remove(&key) {
            commands.entity(old).despawn();
        }
        let entity = build_cell_mesh(&map, &cover, key.0, key.1, tier).map(|mesh| {
            commands
                .spawn((
                    TerrainRoot,
                    bevy::light::NotShadowCaster,
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(assets.material.clone()),
                ))
                .id()
        });
        state.cells.insert(key, GrassCell { tier, entity });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grass_reaches_much_further_than_it_did() {
        assert!(STREAM_IN > 100.0, "was 48 m");
        assert!(STREAM_OUT > STREAM_IN, "cells are only removed a little beyond where they start");
        assert_eq!(tier_for(STREAM_IN + 1.0, None), None);
        assert_eq!(tier_for(STREAM_IN - 1.0, None), Some(Tier::Far));
    }

    #[test]
    fn the_nearer_the_denser() {
        assert_eq!(tier_for(5.0, None), Some(Tier::Near));
        assert_eq!(tier_for(60.0, None), Some(Tier::Mid));
        assert_eq!(tier_for(100.0, None), Some(Tier::Far));
        assert!(Tier::Near.keep() > Tier::Mid.keep() && Tier::Mid.keep() > Tier::Far.keep());
        assert!(Tier::Near.size() < Tier::Mid.size() && Tier::Mid.size() < Tier::Far.size());
    }

    #[test]
    fn a_cell_does_not_flip_tier_back_and_forth_at_a_boundary() {
        // Just past the near limit, a near cell stays near, while a mid cell stays mid.
        let d = NEAR_LIMIT + 2.0;
        assert_eq!(tier_for(d, Some(Tier::Near)), Some(Tier::Near));
        assert_eq!(tier_for(d, Some(Tier::Mid)), Some(Tier::Mid));
        // But well past it, near gives way.
        assert_eq!(tier_for(NEAR_LIMIT + HYSTERESIS + 1.0, Some(Tier::Near)), Some(Tier::Mid));
        // Same at the mid / far boundary and the outer edge.
        assert_eq!(tier_for(MID_LIMIT + 2.0, Some(Tier::Mid)), Some(Tier::Mid));
        assert_eq!(tier_for(MID_LIMIT + 2.0, Some(Tier::Far)), Some(Tier::Far));
        assert_eq!(tier_for(STREAM_IN + 2.0, Some(Tier::Far)), Some(Tier::Far));
    }

    fn lawn() -> (TerrainMap, GroundCover) {
        let map = TerrainMap::flat(0.0);
        let n = map.grid_size();
        (map, GroundCover::new(n, vec![Cover::Meadow as u8; n * n]))
    }

    fn vertex_count(mesh: &Option<Mesh>) -> usize {
        mesh.as_ref().map_or(0, Mesh::count_vertices)
    }

    #[test]
    fn farther_cells_cost_much_less() {
        let (map, cover) = lawn();
        let near = vertex_count(&build_cell_mesh(&map, &cover, 3, 3, Tier::Near));
        let mid = vertex_count(&build_cell_mesh(&map, &cover, 3, 3, Tier::Mid));
        let far = vertex_count(&build_cell_mesh(&map, &cover, 3, 3, Tier::Far));
        assert!(near > 0 && mid < near / 2 && far < mid / 2, "{near} {mid} {far}");
    }

    #[test]
    fn a_thinner_cell_keeps_a_subset_of_the_tufts_of_a_denser_one() {
        let (map, cover) = lawn();
        let bases = |tier| {
            let mesh = build_cell_mesh(&map, &cover, -4, 2, tier).expect("grass");
            let bevy::mesh::VertexAttributeValues::Float32x3(p) = mesh.attribute(Mesh::ATTRIBUTE_POSITION).expect("positions") else { panic!() };
            // A tuft's first quad's first vertex is on the ground; the centre of its base is the
            // midpoint of that quad's two bottom corners.
            let per_tuft = tier.quads() * 4;
            (0..p.len() / per_tuft)
                .map(|t| {
                    let (a, b) = (p[t * per_tuft], p[t * per_tuft + 1]);
                    (((a[0] + b[0]) * 500.0).round() as i32, ((a[2] + b[2]) * 500.0).round() as i32)
                })
                .collect::<std::collections::HashSet<_>>()
        };
        let (near, mid, far) = (bases(Tier::Near), bases(Tier::Mid), bases(Tier::Far));
        assert!(mid.is_subset(&near), "mid tufts are all near tufts");
        assert!(far.is_subset(&mid), "far tufts are all mid tufts");
        assert!(!far.is_empty());
    }

    #[test]
    fn the_tufts_that_remain_are_bigger() {
        let (map, cover) = lawn();
        let height = |tier| {
            let mesh = build_cell_mesh(&map, &cover, 5, 5, tier).expect("grass");
            let bevy::mesh::VertexAttributeValues::Float32x3(p) = mesh.attribute(Mesh::ATTRIBUTE_POSITION).expect("positions") else { panic!() };
            let tall = p.iter().map(|v| v[1]).fold(0.0f32, f32::max);
            let count = (p.len() / (tier.quads() * 4)) as f32;
            (tall, count)
        };
        let (near_tallest, _) = height(Tier::Near);
        let (far_tallest, far_count) = height(Tier::Far);
        assert!(far_tallest > near_tallest * 1.3, "{far_tallest} vs {near_tallest}");
        assert!(far_count > 0.0);
    }

    #[test]
    fn the_whole_ring_costs_about_twice_what_the_old_one_did_for_over_twice_the_reach() {
        // Triangles of a full ring of grass around the camera, at the densest cover (meadow), for
        // the old 48 m reach (everything Near) and the new one.
        let (map, cover) = lawn();
        let tris = |tier| vertex_count(&build_cell_mesh(&map, &cover, 7, 7, tier)) / 4 * 2 * 3 / 3;
        let ring = |inner: f32, outer: f32| std::f32::consts::PI * (outer * outer - inner * inner) / (GRASS_CELL * GRASS_CELL);
        let old = ring(0.0, 48.0) * tris(Tier::Near) as f32;
        let new = ring(0.0, NEAR_LIMIT) * tris(Tier::Near) as f32
            + ring(NEAR_LIMIT, MID_LIMIT) * tris(Tier::Mid) as f32
            + ring(MID_LIMIT, STREAM_IN) * tris(Tier::Far) as f32;
        eprintln!("grass geometry around the camera: {old:.0} triangles before (48 m), {new:.0} now ({STREAM_IN} m): {:.2}x", new / old);
        assert!(new < old * 2.6, "new {new:.0} vs old {old:.0}: more than 2.6x the geometry");
        assert!(new > old, "and it does reach further");
    }
}

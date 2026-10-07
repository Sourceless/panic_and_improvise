// Grass and ground cover close to the camera.
//
// Tufts of real-looking grass are grown on meadow, pasture, rough ground and verges within
// about 50 m of the camera, and removed again as it moves away. Each 6 m square of ground gets
// one merged mesh, so there are only ever a couple of hundred entities no matter how many
// thousand tufts, and the blades sway in the same wind as the trees. Beyond that range the
// field textures carry the look.

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
const STREAM_IN: f32 = 48.0;
const STREAM_OUT: f32 = 60.0;
const MAX_NEW_CELLS_PER_FRAME: usize = 6;

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
            _ => Cover::None,
        }
    }
}

#[derive(Resource)]
struct GrassAssets {
    material: Handle<WindMaterial>,
}

#[derive(Resource, Default)]
struct GrassState {
    // None means the cell was checked and has no grass, so it isn't retried every frame.
    cells: HashMap<(i32, i32), Option<Entity>>,
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
        .load("textures/veg/grass_tuft.png");
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
        Cover::Meadow => (1.25, 0.45, 0.75),
        Cover::Pasture => (1.4, 0.18, 0.32),
        Cover::Rough => (0.75, 0.55, 0.95),
        Cover::Verge => (0.5, 0.3, 0.55),
        Cover::None => (0.0, 0.0, 0.0),
    }
}

fn build_cell_mesh(map: &TerrainMap, cover: &GroundCover, cx: i32, cz: i32) -> Option<Mesh> {
    let origin = Vec2::new(cx as f32, cz as f32) * GRASS_CELL;
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    let attempts = (GRASS_CELL * GRASS_CELL * 1.5) as i32;
    for k in 0..attempts {
        let p = origin + Vec2::new(hash(cx, cz, k as u32 * 4 + 1), hash(cx, cz, k as u32 * 4 + 2)) * GRASS_CELL;
        if p.x.abs() > HALF_SIZE || p.y.abs() > HALF_SIZE {
            continue;
        }
        let kind = cover.at(p);
        let (density, h_lo, h_hi) = tuft_profile(kind);
        // `attempts` candidates per cell stand in for 1.5 per square metre; thin to the density.
        if hash(cx, cz, k as u32 * 4 + 3) > density / 1.5 {
            continue;
        }
        let ground = map.height_at(p);
        // Nothing grows under water.
        if map.water_level(
            (((p.x + HALF_SIZE) / CELL).round() as usize).min(map.grid_size() - 1),
            (((p.y + HALF_SIZE) / CELL).round() as usize).min(map.grid_size() - 1),
        )
        .is_some()
        {
            continue;
        }
        let height = h_lo + (h_hi - h_lo) * hash(cx, cz, k as u32 * 4 + 4);
        let half_width = height * 0.5;
        let yaw0 = hash(cx, cz, k as u32 * 7 + 100) * TAU;
        let tone = 0.75 + 0.4 * hash(cx, cz, k as u32 * 7 + 101);
        // Three crossed quads read as a round tuft from any side.
        for q in 0..3 {
            let yaw = yaw0 + q as f32 * TAU / 6.0;
            let along = Vec3::new(yaw.cos(), 0.0, yaw.sin()) * half_width;
            let base = Vec3::new(p.x, ground - 0.02, p.y);
            let top = base + Vec3::Y * height;
            let first = positions.len() as u32;
            for (corner, uv) in [(base - along, [0.0, 1.0]), (base + along, [1.0, 1.0]), (top + along, [1.0, 0.0]), (top - along, [0.0, 0.0])] {
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

    let stale: Vec<(i32, i32)> = state
        .cells
        .keys()
        .copied()
        .filter(|&(x, z)| (Vec2::new(x as f32 + 0.5, z as f32 + 0.5) * GRASS_CELL).distance(here) > STREAM_OUT)
        .collect();
    for key in stale {
        if let Some(Some(entity)) = state.cells.remove(&key) {
            commands.entity(entity).despawn();
        }
    }

    let reach = (STREAM_IN / GRASS_CELL).ceil() as i32;
    let (hx, hz) = ((here.x / GRASS_CELL).floor() as i32, (here.y / GRASS_CELL).floor() as i32);
    let mut wanted: Vec<((i32, i32), f32)> = Vec::new();
    for dz in -reach..=reach {
        for dx in -reach..=reach {
            let key = (hx + dx, hz + dz);
            let d = (Vec2::new(key.0 as f32 + 0.5, key.1 as f32 + 0.5) * GRASS_CELL).distance(here);
            if d < STREAM_IN && !state.cells.contains_key(&key) {
                wanted.push((key, d));
            }
        }
    }
    // Nearest first, and only a few per frame so moving never hitches.
    wanted.sort_by(|a, b| a.1.total_cmp(&b.1));
    for (key, _) in wanted.into_iter().take(MAX_NEW_CELLS_PER_FRAME) {
        let entity = build_cell_mesh(&map, &cover, key.0, key.1).map(|mesh| {
            commands
                .spawn((
                    TerrainRoot,
                    bevy::light::NotShadowCaster,
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(assets.material.clone()),
                ))
                .id()
        });
        state.cells.insert(key, entity);
    }
}

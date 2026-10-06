use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

use crate::map::{grid_pos, TerrainMap, CELL};

const WATER_LIFT: f32 = 0.05;
const YARD_RADIUS: f32 = 10.0;

pub struct TerrainPlugin;

impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<TerrainMaterial>::default())
            .add_systems(Startup, (load_textures, spawn_world).chain());
    }
}

#[derive(Component)]
pub struct TerrainRoot;

pub type TerrainMaterial = ExtendedMaterial<StandardMaterial, TerrainExtension>;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct TerrainExtension {
    #[texture(100)]
    #[sampler(101)]
    pub grass: Handle<Image>,
    #[texture(102)]
    #[sampler(103)]
    pub dirt: Handle<Image>,
    #[texture(104)]
    #[sampler(105)]
    pub stone: Handle<Image>,
}

impl MaterialExtension for TerrainExtension {
    fn fragment_shader() -> ShaderRef {
        "shaders/terrain.wgsl".into()
    }
}

#[derive(Resource, Clone)]
pub struct TerrainTextures {
    pub grass: Handle<Image>,
    pub dirt: Handle<Image>,
    pub stone: Handle<Image>,
}

fn load_textures(mut commands: Commands, asset_server: Res<AssetServer>) {
    let load = |path: &'static str| {
        asset_server
            .load_builder()
            .with_settings(|settings: &mut ImageLoaderSettings| {
                settings.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                    address_mode_u: ImageAddressMode::Repeat,
                    address_mode_v: ImageAddressMode::Repeat,
                    ..ImageSamplerDescriptor::linear()
                });
            })
            .load(path)
    };
    commands.insert_resource(TerrainTextures {
        grass: load("textures/grass_diffuse.jpg"),
        dirt: load("textures/dirt_diffuse.jpg"),
        stone: load("textures/stone_diffuse.jpg"),
    });
}

fn spawn_world(
    mut commands: Commands,
    map: Res<TerrainMap>,
    textures: Res<TerrainTextures>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut standard: ResMut<Assets<StandardMaterial>>,
    mut terrain: ResMut<Assets<TerrainMaterial>>,
) {
    spawn_terrain(
        &mut commands,
        &mut meshes,
        &mut standard,
        &mut terrain,
        &textures,
        &map,
    );
}

pub fn spawn_terrain(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    standard: &mut Assets<StandardMaterial>,
    terrain: &mut Assets<TerrainMaterial>,
    textures: &TerrainTextures,
    map: &TerrainMap,
) {
    let ground = terrain.add(ExtendedMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 1.0,
            ..default()
        },
        extension: TerrainExtension {
            grass: textures.grass.clone(),
            dirt: textures.dirt.clone(),
            stone: textures.stone.clone(),
        },
    });
    commands.spawn((
        TerrainRoot,
        Mesh3d(meshes.add(terrain_mesh(map))),
        MeshMaterial3d(ground),
    ));

    commands.spawn((
        TerrainRoot,
        Mesh3d(meshes.add(river_mesh(map))),
        MeshMaterial3d(standard.add(StandardMaterial {
            base_color: Color::srgb(0.18, 0.38, 0.62),
            perceptual_roughness: 0.2,
            ..default()
        })),
    ));

    crate::settlement::spawn_settlements(commands, meshes, standard, map);
}

fn terrain_mesh(map: &TerrainMap) -> Mesh {
    let n = map.grid_size();
    let mut positions = Vec::with_capacity(n * n);
    let mut weights = Vec::with_capacity(n * n);
    for iz in 0..n {
        for ix in 0..n {
            let p = grid_pos(ix, iz);
            let h = map.vertex_height(ix, iz);
            positions.push([p.x, h, p.y]);
            weights.push(surface_weights(map, ix, iz, p));
        }
    }

    let mut indices = Vec::with_capacity((n - 1) * (n - 1) * 6);
    let row = n as u32;
    for iz in 0..n - 1 {
        for ix in 0..n - 1 {
            let i = iz as u32 * row + ix as u32;
            indices.extend_from_slice(&[i, i + row, i + 1, i + 1, i + row, i + row + 1]);
        }
    }

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, weights)
        .with_inserted_indices(Indices::U32(indices));
    mesh.compute_smooth_normals();
    mesh
}

// Blend weights for the terrain shader: r = grass, g = dirt, b = stone.
fn surface_weights(map: &TerrainMap, ix: usize, iz: usize, p: Vec2) -> [f32; 4] {
    let n = map.grid_size();
    let dx = map.vertex_height((ix + 1).min(n - 1), iz) - map.vertex_height(ix.saturating_sub(1), iz);
    let dz = map.vertex_height(ix, (iz + 1).min(n - 1)) - map.vertex_height(ix, iz.saturating_sub(1));
    let slope = ((dx * dx + dz * dz).sqrt() / (2.0 * CELL)).min(1.0);

    let stone = smoothstep(0.35, 0.7, slope).max(smoothstep(75.0, 95.0, map.vertex_height(ix, iz)));
    let bank = 1.0 - smoothstep(CELL, CELL * 3.0, map.river_distance(p));
    let yard = map.pois.iter().any(|poi| poi.position.distance(p) < YARD_RADIUS);
    let dirt = (bank.max(if yard { 1.0 } else { 0.0 }) * (1.0 - stone)).clamp(0.0, 1.0);
    let grass = (1.0 - dirt - stone).clamp(0.0, 1.0);
    [grass, dirt, stone, 1.0]
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn river_mesh(map: &TerrainMap) -> Mesh {
    let n = map.grid_size();
    let half = CELL * 0.5;
    let mut positions = Vec::new();
    let mut indices = Vec::new();
    for iz in 0..n {
        for ix in 0..n {
            let Some(level) = map.water_level(ix, iz) else {
                continue;
            };
            let p = grid_pos(ix, iz);
            let y = level + WATER_LIFT;
            let base = positions.len() as u32;
            for (dx, dz) in [(-half, -half), (half, -half), (half, half), (-half, half)] {
                positions.push([p.x + dx, y, p.y + dz]);
            }
            indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
        }
    }
    let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_indices(Indices::U32(indices))
}

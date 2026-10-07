use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

use crate::contour::{triangulate, Contour, Smoothing, OPEN};
use crate::map::{grid_pos, TerrainMap, CELL};

const WATER_LIFT: f32 = 0.05;
const YARD_RADIUS: f32 = 10.0;

pub struct TerrainPlugin;

impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            crate::mipmaps::MipmapPlugin,
            crate::vegetation::VegetationPlugin,
            MaterialPlugin::<TerrainMaterial>::default(),
            MaterialPlugin::<crate::field_material::FieldMaterial>::default(),
        ))
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
    pub soil_plough: Handle<Image>,
    pub soil_loam: Handle<Image>,
    pub meadow: Handle<Image>,
    pub pasture: Handle<Image>,
    pub hedge: Handle<Image>,
    pub wood: Handle<Image>,
}

pub fn load_textures(mut commands: Commands, asset_server: Res<AssetServer>, mut mips: ResMut<crate::mipmaps::MipQueue>) {
    let load = |path: &'static str| {
        asset_server
            .load_builder()
            .with_settings(|settings: &mut ImageLoaderSettings| {
                settings.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                    address_mode_u: ImageAddressMode::Repeat,
                    address_mode_v: ImageAddressMode::Repeat,
                    anisotropy_clamp: 8,
                    ..ImageSamplerDescriptor::linear()
                });
            })
            .load(path)
    };
    let textures = TerrainTextures {
        grass: load("textures/grass_diffuse.jpg"),
        dirt: load("textures/dirt_diffuse.jpg"),
        stone: load("textures/stone_diffuse.jpg"),
        soil_plough: load("textures/pbr/soil_plough.jpg"),
        soil_loam: load("textures/pbr/soil_loam.jpg"),
        meadow: load("textures/pbr/meadow.jpg"),
        pasture: load("textures/pbr/pasture.jpg"),
        hedge: load("textures/veg/hedge.jpg"),
        wood: load("textures/pbr/bark_conifer.jpg"),
    };
    mips.0.extend([
        textures.grass.clone(),
        textures.dirt.clone(),
        textures.stone.clone(),
        textures.soil_plough.clone(),
        textures.soil_loam.clone(),
        textures.meadow.clone(),
        textures.pasture.clone(),
        textures.hedge.clone(),
        textures.wood.clone(),
    ]);
    commands.insert_resource(textures);
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

    let stone = smoothstep(0.35, 0.7, slope);
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

// Water is contoured like the fields (see contour.rs) rather than drawn as a square per wet
// cell, so the river and lake shores curve instead of stair-stepping. Each cell's wet
// polygon takes its height from bilinear interpolation of the cell's water levels, with any
// dry corner borrowing the average of the wet ones.
fn river_mesh(map: &TerrainMap) -> Mesh {
    const WATER: u32 = 0;
    let n = map.grid_size();
    let labels: Vec<u32> = (0..n * n)
        .map(|i| if map.water_level(i % n, i / n).is_some() { WATER } else { OPEN })
        .collect();
    let contour = Contour::build(n, &labels, map.seed ^ 0x77A7, Some(WATER), Smoothing::WATER);

    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    for iz in 0..n - 1 {
        for ix in 0..n - 1 {
            let corners = [(ix, iz), (ix + 1, iz), (ix + 1, iz + 1), (ix, iz + 1)];
            let levels = corners.map(|(x, z)| map.water_level(x, z));
            let wet: Vec<f32> = levels.iter().flatten().copied().collect();
            if wet.is_empty() {
                continue;
            }
            let mean = wet.iter().sum::<f32>() / wet.len() as f32;
            let [l_tl, l_tr, l_br, l_bl] = levels.map(|l| l.unwrap_or(mean));
            let cell = levels.map(|l| if l.is_some() { WATER } else { OPEN });
            let origin = grid_pos(ix, iz);
            let height = |p: Vec2| {
                let top = l_tl + (l_tr - l_tl) * p.x;
                let bottom = l_bl + (l_br - l_bl) * p.x;
                top + (bottom - top) * p.y + WATER_LIFT
            };
            for (_, poly) in contour.cell_regions(ix, iz, cell) {
                for mut tri in triangulate(&poly) {
                    // Wind upward; (u, v) maps to (x, z).
                    let area = (tri[1].y - tri[0].y) * (tri[2].x - tri[0].x) - (tri[1].x - tri[0].x) * (tri[2].y - tri[0].y);
                    if area.abs() < 1e-7 {
                        continue;
                    }
                    if area < 0.0 {
                        tri.swap(1, 2);
                    }
                    let base = positions.len() as u32;
                    for p in tri {
                        let w = origin + p * CELL;
                        positions.push([w.x, height(p), w.y]);
                    }
                    indices.extend_from_slice(&[base, base + 1, base + 2]);
                }
            }
        }
    }
    let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_indices(Indices::U32(indices))
}

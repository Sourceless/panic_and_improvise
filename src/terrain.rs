use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

use crate::contour::{triangulate, Contour, Smoothing, OPEN};
use crate::map::{grid_pos, PoiKind, TerrainMap, CELL};
use crate::params::GenParams;
use crate::zones::{Zone, ZoneMap};

const WATER_LIFT: f32 = 0.05;
const YARD_RADIUS: f32 = 10.0;

pub struct TerrainPlugin;

impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GenParams>().add_plugins((
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
    #[texture(106)]
    #[sampler(107)]
    pub sand: Handle<Image>,
    #[texture(108)]
    #[sampler(109)]
    pub gravel: Handle<Image>,
    #[texture(110)]
    #[sampler(111)]
    pub litter: Handle<Image>,
    #[texture(112)]
    #[sampler(113)]
    pub needles: Handle<Image>,
    #[texture(114)]
    #[sampler(115)]
    pub mud: Handle<Image>,
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
    pub sand: Handle<Image>,
    pub gravel: Handle<Image>,
    pub litter: Handle<Image>,
    pub needles: Handle<Image>,
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
        sand: load("textures/pbr/sand.jpg"),
        gravel: load("textures/pbr/gravel.jpg"),
        litter: load("textures/pbr/forest_broadleaf.jpg"),
        needles: load("textures/pbr/forest_conifer.jpg"),
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
        textures.sand.clone(),
        textures.gravel.clone(),
        textures.litter.clone(),
        textures.needles.clone(),
    ]);
    commands.insert_resource(textures);
}

fn spawn_world(
    mut commands: Commands,
    map: Res<TerrainMap>,
    params: Res<GenParams>,
    textures: Res<TerrainTextures>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut standard: ResMut<Assets<StandardMaterial>>,
    mut terrain: ResMut<Assets<TerrainMaterial>>,
    mut fields: ResMut<Assets<crate::field_material::FieldMaterial>>,
) {
    crate::world::build_world(
        &mut commands,
        &mut meshes,
        &mut standard,
        &mut terrain,
        &mut fields,
        &textures,
        &map,
        &params,
    );
}

pub fn spawn_terrain(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    standard: &mut Assets<StandardMaterial>,
    terrain: &mut Assets<TerrainMaterial>,
    textures: &TerrainTextures,
    map: &TerrainMap,
    zones: &ZoneMap,
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
            sand: textures.sand.clone(),
            gravel: textures.gravel.clone(),
            litter: textures.litter.clone(),
            needles: textures.needles.clone(),
            mud: textures.soil_plough.clone(),
        },
    });
    commands.spawn((
        TerrainRoot,
        Mesh3d(meshes.add(terrain_mesh(map, zones))),
        MeshMaterial3d(ground),
    ));

    commands.spawn((
        TerrainRoot,
        bevy::light::NotShadowCaster,
        Mesh3d(meshes.add(river_mesh(map))),
        MeshMaterial3d(standard.add(StandardMaterial {
            base_color: Color::srgb(0.18, 0.38, 0.62),
            perceptual_roughness: 0.2,
            ..default()
        })),
    ));

    crate::settlement::spawn_settlements(commands, meshes, standard, map);
}

fn terrain_mesh(map: &TerrainMap, zones: &ZoneMap) -> Mesh {
    let n = map.grid_size();
    let mut positions = Vec::with_capacity(n * n);
    let mut weights_a = Vec::with_capacity(n * n);
    let mut weights_b = Vec::with_capacity(n * n);
    let mut weights_c = Vec::with_capacity(n * n);
    for iz in 0..n {
        for ix in 0..n {
            let p = grid_pos(ix, iz);
            let h = map.vertex_height(ix, iz);
            positions.push([p.x, h, p.y]);
            // Eight blend weights spread over the vertex colour and two UV sets.
            let w = surface_weights(map, zones, ix, iz, p);
            weights_a.push([w[0], w[1], w[2], w[3]]);
            weights_b.push([w[4], w[5]]);
            weights_c.push([w[6], w[7]]);
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
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, weights_a)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, weights_b)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, weights_c)
        .with_inserted_indices(Indices::U32(indices));
    mesh.compute_smooth_normals();
    mesh
}

// Settlement ground: worn bare earth and gravel in the built-up core, fading to gardens and
// grass toward the edge, relative to the size of the nearest settlement.
fn urban_ground(map: &TerrainMap, p: Vec2) -> [f32; 8] {
    let t = map
        .pois
        .iter()
        .filter(|poi| poi.kind == PoiKind::Village)
        .map(|poi| poi.position.distance(p) / poi.radius)
        .fold(f32::MAX, f32::min);
    let core = 1.0 - smoothstep(0.35, 1.1, t);
    let (edge, centre) = ([0.85, 0.15, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], [0.1, 0.4, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0]);
    std::array::from_fn(|i| edge[i] + (centre[i] - edge[i]) * core)
}

// Blend weights for the terrain shader, normalised to sum to 1, in this order:
// grass, dirt, stone, sand, gravel, leaf litter, needle litter, mud.
fn surface_weights(map: &TerrainMap, zones: &ZoneMap, ix: usize, iz: usize, p: Vec2) -> [f32; 8] {
    let n = map.grid_size();
    let dx = map.vertex_height((ix + 1).min(n - 1), iz) - map.vertex_height(ix.saturating_sub(1), iz);
    let dz = map.vertex_height(ix, (iz + 1).min(n - 1)) - map.vertex_height(ix, iz.saturating_sub(1));
    let slope = ((dx * dx + dz * dz).sqrt() / (2.0 * CELL)).min(1.0);

    // What the ground is like where nothing else overrides it, by land use.
    let mut w = match zones.zone_at(ix, iz) {
        Zone::Woodland => [0.15, 0.0, 0.0, 0.0, 0.0, 0.85, 0.0, 0.0],
        Zone::Conifer => [0.10, 0.0, 0.0, 0.0, 0.0, 0.0, 0.90, 0.0],
        Zone::Wetland => [0.55, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.45],
        Zone::Urban => urban_ground(map, p),
        Zone::Industrial => [0.0, 0.2, 0.0, 0.0, 0.8, 0.0, 0.0, 0.0],
        Zone::Quarry => [0.0, 0.0, 0.5, 0.0, 0.5, 0.0, 0.0, 0.0],
        Zone::Military => [0.75, 0.25, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        Zone::Moorland => [0.6, 0.4, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        _ => [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    };

    // Steep ground sheds soil: bare rock on cliffs, loose scree on the slopes below.
    let stone = smoothstep(0.35, 0.7, slope);
    let scree = smoothstep(0.2, 0.38, slope) * (1.0 - stone);
    let soil = 1.0 - stone - scree;
    for v in &mut w {
        *v *= soil;
    }
    w[2] += stone;
    w[4] += scree;

    // Shores: sand on gentle banks, and wet mud right at the water's edge.
    let water_d = map.water_distance(p);
    let sand = (1.0 - smoothstep(3.0, 22.0, water_d)) * (1.0 - slope * 3.0).clamp(0.0, 1.0) * 0.9;
    let mud = (1.0 - smoothstep(1.0, 9.0, water_d)) * 0.7;
    // River banks (as before) turn to dirt rather than sand close to the channel.
    let bank = (1.0 - smoothstep(CELL, CELL * 3.0, map.river_distance(p))) * (1.0 - stone);
    for v in &mut w {
        *v *= 1.0 - sand.max(mud);
    }
    w[3] += sand * (1.0 - mud);
    w[7] += mud;
    w[1] += bank * 0.5;

    // Farmyards and settlement centres are bare earth.
    if map.pois.iter().any(|poi| poi.position.distance(p) < YARD_RADIUS) {
        w = [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    }
    let total: f32 = w.iter().sum();
    if total <= 0.0 {
        return [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    }
    w.map(|v| v / total)
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
    // The surface is built on the wet vertices grown by one cell. The terrain crosses the water
    // level somewhere between the last wet vertex and the first dry one, which on a gentle
    // shore is often well past the midpoint a plain contour would stop at, leaving a strip of
    // submerged bed showing. Water that overshoots into the bank is simply hidden by the
    // terrain; dilated vertices take the average level of the wet cells around them.
    let wet = |x: usize, z: usize| map.water_level(x, z);
    let levels_grid: Vec<Option<f32>> = (0..n * n)
        .map(|i| {
            let (x, z) = (i % n, i / n);
            if let Some(level) = wet(x, z) {
                return Some(level);
            }
            let mut sum = 0.0;
            let mut count = 0;
            for dz in -1..=1isize {
                for dx in -1..=1isize {
                    let (nx, nz) = (x as isize + dx, z as isize + dz);
                    if nx < 0 || nz < 0 || nx >= n as isize || nz >= n as isize {
                        continue;
                    }
                    if let Some(level) = wet(nx as usize, nz as usize) {
                        sum += level;
                        count += 1;
                    }
                }
            }
            (count > 0).then(|| sum / count as f32)
        })
        .collect();
    let labels: Vec<u32> = levels_grid.iter().map(|l| if l.is_some() { WATER } else { OPEN }).collect();
    let contour = Contour::build(n, &labels, map.seed ^ 0x77A7, Some(WATER), Smoothing::WATER);

    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    for iz in 0..n - 1 {
        for ix in 0..n - 1 {
            let corners = [(ix, iz), (ix + 1, iz), (ix + 1, iz + 1), (ix, iz + 1)];
            let levels = corners.map(|(x, z)| levels_grid[z * n + x]);
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

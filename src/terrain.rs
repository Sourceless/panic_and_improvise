use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

use crate::contour::{triangulate, Contour, Smoothing, OPEN};
use crate::map::{grid_pos, PoiKind, TerrainMap, CELL, HALF_SIZE, TILE_CELLS};
use crate::params::GenParams;
use crate::zones::{Zone, ZoneMap};

const WATER_LIFT: f32 = 0.05;
const YARD_RADIUS: f32 = 10.0;

pub struct TerrainPlugin;

impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GenParams>().add_plugins((
            crate::mipmaps::MipmapPlugin,
            crate::ground_textures::GroundTexturesPlugin,
            crate::vegetation::VegetationPlugin,
            crate::perf::PerfPlugin,
            crate::look::LookPlugin,
            crate::wind::WindPlugin,
            crate::cloud_shadows::CloudShadowPlugin,
            crate::grass::GrassPlugin,
            crate::settlement::SettlementPlugin,
            MaterialPlugin::<TerrainMaterial>::default(),
            MaterialPlugin::<crate::field_material::FieldMaterial>::default(),
            MaterialPlugin::<crate::road_material::RoadMaterial>::default(),
            MaterialPlugin::<crate::water_material::WaterMaterial>::default(),
            MaterialPlugin::<crate::wind_material::WindMaterial>::default(),
            MaterialPlugin::<crate::cloud_material::CloudMaterial>::default(),
        ))
            .add_systems(Startup, (load_textures, spawn_world).chain());
    }
}

#[derive(Component)]
pub struct TerrainRoot;

pub type TerrainMaterial = ExtendedMaterial<StandardMaterial, TerrainExtension>;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct TerrainExtension {
    /// Colour of every kind of ground, one array layer each (see ground_textures::LAYERS).
    #[texture(100, dimension = "2d_array")]
    #[sampler(101)]
    pub diffuse: Handle<Image>,
    /// Matching normal maps.
    #[texture(102, dimension = "2d_array")]
    #[sampler(103)]
    pub normal: Handle<Image>,
}

impl MaterialExtension for TerrainExtension {
    fn fragment_shader() -> ShaderRef {
        "shaders/terrain.wgsl".into()
    }
}

#[derive(Resource, Clone)]
pub struct TerrainTextures {
    /// The ground layers, shared by the terrain and field shaders.
    pub ground: crate::ground_textures::GroundArrays,
    pub hedge: Handle<Image>,
    pub wood: Handle<Image>,
    /// Weathered stone for field walls.
    pub wall_stone: Handle<Image>,
    /// The surface of main roads (tarmac) and of farm tracks (gravel).
    pub asphalt: Handle<Image>,
    pub track: Handle<Image>,
}

pub fn load_textures(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut mips: ResMut<crate::mipmaps::MipQueue>,
    mut images: ResMut<Assets<Image>>,
) {
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
    let ground = crate::ground_textures::start_loading(&mut commands, &asset_server, &mut images);
    let textures = TerrainTextures {
        ground,
        hedge: load("textures/veg/hedge.jpg"),
        wood: load("textures/pbr/bark_conifer.jpg"),
        wall_stone: load("textures/stone_diffuse.jpg"),
        asphalt: load("textures/pbr/asphalt.jpg"),
        track: load("textures/pbr/gravel.jpg"),
    };
    mips.0.extend([textures.hedge.clone(), textures.wood.clone(), textures.wall_stone.clone(), textures.asphalt.clone(), textures.track.clone()]);
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
    mut road_materials: ResMut<Assets<crate::road_material::RoadMaterial>>,
    mut waters: ResMut<Assets<crate::water_material::WaterMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    crate::world::build_world(
        &mut commands,
        &mut meshes,
        &mut standard,
        &mut terrain,
        &mut fields,
        &mut road_materials,
        &mut waters,
        &mut images,
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
    waters: &mut Assets<crate::water_material::WaterMaterial>,
    images: &mut Assets<Image>,
    plan: &crate::settlement_plan::SettlementPlan,
    colliders: &mut crate::collision::Colliders,
) {
    let ground = terrain.add(ExtendedMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 1.0,
            ..default()
        },
        extension: TerrainExtension {
            diffuse: textures.ground.diffuse.clone(),
            normal: textures.ground.normal.clone(),
        },
    });
    let tiles = (map.grid_size() - 1).div_ceil(TILE_CELLS);
    for tz in 0..tiles {
        for tx in 0..tiles {
            commands.spawn((
                TerrainRoot,
                Mesh3d(meshes.add(terrain_tile_mesh(map, zones, tx, tz))),
                MeshMaterial3d(ground.clone()),
            ));
        }
    }

    let water = waters.add(crate::water_material::water_material(map, images));
    for mesh in river_meshes(map).into_iter().filter(|_| !crate::world::skip("water")) {
        commands.spawn((TerrainRoot, bevy::light::NotShadowCaster, Mesh3d(meshes.add(mesh)), MeshMaterial3d(water.clone())));
    }
    // Past the map's edge the sea carries on to the horizon, where the atmosphere takes over.
    for mesh in ocean_meshes() {
        commands.spawn((TerrainRoot, bevy::light::NotShadowCaster, Mesh3d(meshes.add(mesh)), MeshMaterial3d(water.clone())));
    }

    if !crate::world::skip("settlements") {
        crate::settlement::spawn_settlements(commands, meshes, standard, map, plan, colliders);
    }
}

// One tile of the terrain: the vertices from (tx, tz) * TILE_CELLS through the shared border
// with the next tile. Normals come from the heights of the whole map (not just the tile), so
// there is no lighting seam where two tiles meet.
fn terrain_tile_mesh(map: &TerrainMap, zones: &ZoneMap, tx: usize, tz: usize) -> Mesh {
    let n = map.grid_size();
    let (x0, z0) = (tx * TILE_CELLS, tz * TILE_CELLS);
    let (x1, z1) = ((x0 + TILE_CELLS).min(n - 1), (z0 + TILE_CELLS).min(n - 1));
    let (w, h) = (x1 - x0 + 1, z1 - z0 + 1);
    let mut positions = Vec::with_capacity(w * h);
    let mut normals = Vec::with_capacity(w * h);
    let mut weights_a = Vec::with_capacity(w * h);
    let mut weights_b = Vec::with_capacity(w * h);
    let mut weights_c = Vec::with_capacity(w * h);
    let height = |x: usize, z: usize| map.vertex_height(x.min(n - 1), z.min(n - 1));
    for iz in z0..=z1 {
        for ix in x0..=x1 {
            let p = grid_pos(ix, iz);
            positions.push([p.x, map.vertex_height(ix, iz), p.y]);
            let dx = (height(ix + 1, iz) - height(ix.saturating_sub(1), iz)) / (2.0 * CELL);
            let dz = (height(ix, iz + 1) - height(ix, iz.saturating_sub(1))) / (2.0 * CELL);
            normals.push(Vec3::new(-dx, 1.0, -dz).normalize().to_array());
            // Eight blend weights spread over the vertex colour and two UV sets.
            let wgt = surface_weights(map, zones, ix, iz, p);
            weights_a.push([wgt[0], wgt[1], wgt[2], wgt[3]]);
            weights_b.push([wgt[4], wgt[5]]);
            weights_c.push([wgt[6], wgt[7]]);
        }
    }
    let mut indices = Vec::with_capacity((w - 1) * (h - 1) * 6);
    let row = w as u32;
    for iz in 0..h - 1 {
        for ix in 0..w - 1 {
            let i = iz as u32 * row + ix as u32;
            indices.extend_from_slice(&[i, i + row, i + 1, i + 1, i + row, i + row + 1]);
        }
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, weights_a)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, weights_b)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, weights_c)
        .with_inserted_indices(Indices::U32(indices))
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
// Four big rectangles around the map, at sea level, that stand in for the open sea. They sit
// a hair below the lake surface inside the map so the two never z-fight where they overlap.
fn ocean_meshes() -> Vec<Mesh> {
    const REACH: f32 = 60_000.0;
    let inner = HALF_SIZE - 3.0;
    let rects = [
        (-REACH, -REACH, REACH, -inner),
        (-REACH, inner, REACH, REACH),
        (-REACH, -inner, -inner, inner),
        (inner, -inner, REACH, inner),
    ];
    rects
        .into_iter()
        .map(|(x0, z0, x1, z1)| {
            let positions = vec![[x0, -0.02, z0], [x1, -0.02, z0], [x1, -0.02, z1], [x0, -0.02, z1]];
            Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
                .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
                .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; 4])
                // Deep open water everywhere.
                .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[40.0, 0.0, 0.0, 1.0]; 4])
                .with_inserted_indices(Indices::U32(vec![0, 2, 1, 0, 3, 2]))
        })
        .collect()
}

fn river_meshes(map: &TerrainMap) -> Vec<Mesh> {
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

    // One mesh per tile, so frustum culling can skip the water that isn't in view.
    let mut tiles: HashMap<(usize, usize), (Vec<[f32; 3]>, Vec<[f32; 4]>, Vec<u32>)> = HashMap::new();
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
            let (positions, colors, indices) = tiles.entry((ix / TILE_CELLS, iz / TILE_CELLS)).or_default();
            for (_, poly) in contour.cell_regions_world(ix, iz, cell) {
                for mut tri in triangulate(&poly) {
                    // Wind upward; world (x, z) is the plane.
                    let area = (tri[1].y - tri[0].y) * (tri[2].x - tri[0].x) - (tri[1].x - tri[0].x) * (tri[2].y - tri[0].y);
                    if area.abs() < 1e-5 {
                        continue;
                    }
                    if area < 0.0 {
                        tri.swap(1, 2);
                    }
                    let base = positions.len() as u32;
                    for w in tri {
                        let surface = height((w - origin) / CELL);
                        positions.push([w.x, surface, w.y]);
                        // How deep the water is here, which the shader turns into colour,
                        // clarity and shoreline foam.
                        colors.push([(surface - map.height_at(w)).max(0.0), 0.0, 0.0, 1.0]);
                    }
                    indices.extend_from_slice(&[base, base + 1, base + 2]);
                }
            }
        }
    }
    tiles
        .into_values()
        .map(|(positions, colors, indices)| {
            let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
            Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
                .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
                .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
                .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
                .with_inserted_indices(Indices::U32(indices))
        })
        .collect()
}

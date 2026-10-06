use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::map::{fbm, grid_pos, Poi, PoiKind, TerrainMap, CELL};

const WATER_LIFT: f32 = 0.05;

pub struct TerrainPlugin;

impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_world);
    }
}

#[derive(Component)]
pub struct TerrainRoot;

struct PoiMaterials {
    building: Handle<StandardMaterial>,
    stone: Handle<StandardMaterial>,
    brick: Handle<StandardMaterial>,
    white: Handle<StandardMaterial>,
}

enum Shape {
    Box(Vec3),
    Cylinder(f32, f32),
}

fn spawn_world(
    mut commands: Commands,
    map: Res<TerrainMap>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    spawn_terrain(&mut commands, &mut meshes, &mut materials, &map);
}

pub fn spawn_terrain(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    map: &TerrainMap,
) {
    commands.spawn((
        TerrainRoot,
        Mesh3d(meshes.add(terrain_mesh(map))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 1.0,
            ..default()
        })),
    ));

    commands.spawn((
        TerrainRoot,
        Mesh3d(meshes.add(river_mesh(map))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.18, 0.38, 0.62),
            perceptual_roughness: 0.2,
            ..default()
        })),
    ));

    let poi_materials = PoiMaterials {
        building: materials.add(StandardMaterial {
            base_color: Color::srgb(0.86, 0.8, 0.68),
            ..default()
        }),
        stone: materials.add(StandardMaterial {
            base_color: Color::srgb(0.6, 0.6, 0.58),
            ..default()
        }),
        brick: materials.add(StandardMaterial {
            base_color: Color::srgb(0.55, 0.22, 0.16),
            ..default()
        }),
        white: materials.add(StandardMaterial {
            base_color: Color::srgb(0.92, 0.92, 0.9),
            ..default()
        }),
    };
    for poi in &map.pois {
        let ground = map.height_at(poi.position);
        spawn_poi(commands, meshes, &poi_materials, poi, ground);
    }
}

fn spawn_poi(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &PoiMaterials,
    poi: &Poi,
    ground: f32,
) {
    let root = commands
        .spawn((
            TerrainRoot,
            Transform::from_xyz(poi.position.x, ground, poi.position.y),
            Visibility::default(),
        ))
        .id();

    let parts: Vec<(Shape, Handle<StandardMaterial>, Vec3)> = match poi.kind {
        PoiKind::Village => (0..6)
            .map(|i| {
                let x = (i % 3) as f32 * 14.0 - 14.0;
                let z = (i / 3) as f32 * 16.0 - 8.0;
                (Shape::Box(Vec3::new(7.0, 4.0, 6.0)), materials.building.clone(), Vec3::new(x, 2.0, z))
            })
            .collect(),
        PoiKind::Church => vec![
            (Shape::Box(Vec3::new(6.0, 14.0, 6.0)), materials.stone.clone(), Vec3::new(0.0, 7.0, 0.0)),
            (Shape::Box(Vec3::new(2.0, 8.0, 2.0)), materials.stone.clone(), Vec3::new(0.0, 18.0, 0.0)),
        ],
        PoiKind::Farm => vec![
            (Shape::Box(Vec3::new(14.0, 7.0, 8.0)), materials.brick.clone(), Vec3::new(0.0, 3.5, 0.0)),
            (Shape::Cylinder(2.5, 9.0), materials.white.clone(), Vec3::new(-14.0, 4.5, 0.0)),
        ],
        PoiKind::Mill => vec![(Shape::Cylinder(4.0, 13.0), materials.white.clone(), Vec3::new(0.0, 6.5, 0.0))],
    };

    for (shape, material, offset) in parts {
        let mesh = match shape {
            Shape::Box(size) => meshes.add(Cuboid::new(size.x, size.y, size.z)),
            Shape::Cylinder(radius, height) => meshes.add(Cylinder::new(radius, height)),
        };
        let child = commands
            .spawn((Mesh3d(mesh), MeshMaterial3d(material), Transform::from_translation(offset)))
            .id();
        commands.entity(root).add_child(child);
    }
}

fn terrain_mesh(map: &TerrainMap) -> Mesh {
    let n = map.grid_size();
    let mut positions = Vec::with_capacity(n * n);
    let mut colors = Vec::with_capacity(n * n);
    for iz in 0..n {
        for ix in 0..n {
            let p = grid_pos(ix, iz);
            let h = map.vertex_height(ix, iz);
            positions.push([p.x, h, p.y]);
            colors.push(biome_color(map, ix, iz, h, p));
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
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
        .with_inserted_indices(Indices::U32(indices));
    mesh.compute_smooth_normals();
    mesh
}

fn biome_color(map: &TerrainMap, ix: usize, iz: usize, h: f32, p: Vec2) -> [f32; 4] {
    let n = map.grid_size();
    let dx = map.vertex_height((ix + 1).min(n - 1), iz) - map.vertex_height(ix.saturating_sub(1), iz);
    let dz = map.vertex_height(ix, (iz + 1).min(n - 1)) - map.vertex_height(ix, iz.saturating_sub(1));
    let slope = ((dx * dx + dz * dz).sqrt() / (2.0 * CELL)).min(1.0);

    let grass = [0.36, 0.56, 0.25];
    let upland = [0.42, 0.50, 0.30];
    let rock = [0.45, 0.40, 0.32];
    let woodland = [0.15, 0.34, 0.17];

    let height_t = ((h - 40.0) / 60.0).clamp(0.0, 1.0);
    let mut base = mix(grass, upland, height_t);
    base = mix(base, rock, ((slope - 0.35) / 0.25).clamp(0.0, 1.0));

    let cover = fbm(p.x / 260.0, p.y / 260.0, map.seed ^ 0x5151, 3);
    base = mix(base, woodland, ((cover - 0.58) / 0.08).clamp(0.0, 1.0));

    [base[0], base[1], base[2], 1.0]
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
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

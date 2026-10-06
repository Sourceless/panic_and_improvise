use std::f32::consts::FRAC_PI_2;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::PrimitiveTopology;
use bevy::prelude::*;

use crate::map::{PoiKind, TerrainMap};

#[derive(Component)]
pub struct SettlementRoot;

struct Palette {
    walls: Vec<Handle<StandardMaterial>>,
    slate: Handle<StandardMaterial>,
    tile: Handle<StandardMaterial>,
    stone: Handle<StandardMaterial>,
    brick: Handle<StandardMaterial>,
    white: Handle<StandardMaterial>,
    timber: Handle<StandardMaterial>,
}

struct Part {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    transform: Transform,
}

pub fn spawn_settlements(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    map: &TerrainMap,
) {
    let mut colour = |r: f32, g: f32, b: f32| {
        materials.add(StandardMaterial {
            base_color: Color::srgb(r, g, b),
            perceptual_roughness: 0.9,
            ..default()
        })
    };
    let palette = Palette {
        walls: vec![
            colour(0.9, 0.86, 0.74),
            colour(0.93, 0.87, 0.6),
            colour(0.92, 0.92, 0.9),
            colour(0.62, 0.6, 0.56),
            colour(0.85, 0.72, 0.65),
        ],
        slate: colour(0.25, 0.27, 0.3),
        tile: colour(0.55, 0.25, 0.18),
        stone: colour(0.6, 0.6, 0.58),
        brick: colour(0.55, 0.22, 0.16),
        white: colour(0.92, 0.92, 0.9),
        timber: colour(0.4, 0.3, 0.2),
    };

    for poi in &map.pois {
        let ground = map.height_at(poi.position);
        let root = commands
            .spawn((
                SettlementRoot,
                Transform::from_xyz(poi.position.x, ground, poi.position.y),
                Visibility::default(),
            ))
            .id();
        let mut rng = Rng::from_position(poi.position);
        let parts = match poi.kind {
            PoiKind::Village => {
                let church_offset = Vec3::new(
                    poi.landmark.x - poi.position.x,
                    map.height_at(poi.landmark) - ground,
                    poi.landmark.y - poi.position.y,
                );
                village(meshes, &palette, &mut rng, church_offset)
            }
            PoiKind::Farm => farm(meshes, &palette),
            PoiKind::Mill => mill(meshes, &palette),
        };
        for part in parts {
            let child = commands
                .spawn((
                    Mesh3d(part.mesh),
                    MeshMaterial3d(part.material),
                    part.transform,
                ))
                .id();
            commands.entity(root).add_child(child);
        }
    }
}

fn village(meshes: &mut Assets<Mesh>, palette: &Palette, rng: &mut Rng, church_offset: Vec3) -> Vec<Part> {
    let mut parts = Vec::new();
    for i in 0..9 {
        for &row_z in &[-22.0_f32, -9.0, 9.0, 22.0] {
            if rng.unit() < 0.15 {
                continue;
            }
            let x = -48.0 + i as f32 * 12.0 + rng.range(-1.5, 1.5);
            let z = row_z + rng.range(-1.0, 1.0);
            let yaw = if rng.unit() < 0.15 { FRAC_PI_2 } else { 0.0 };
            let walls = palette.walls[(rng.unit() * palette.walls.len() as f32) as usize % palette.walls.len()].clone();
            let roof = if rng.unit() < 0.7 { palette.slate.clone() } else { palette.tile.clone() };
            house(
                &mut parts,
                meshes,
                walls,
                roof,
                Vec3::new(x, 0.0, z),
                yaw,
                rng.range(7.0, 9.0),
                rng.range(5.5, 6.5),
                rng.range(3.8, 4.4),
            );
        }
    }
    for mut part in church(meshes, palette) {
        part.transform.translation += church_offset;
        parts.push(part);
    }
    parts
}

fn church(meshes: &mut Assets<Mesh>, palette: &Palette) -> Vec<Part> {
    let mut parts = Vec::new();
    let nave = Vec3::new(7.0, 6.0, 14.0);
    push(&mut parts, meshes.add(Cuboid::new(nave.x, nave.y, nave.z)), palette.stone.clone(), Transform::from_xyz(0.0, nave.y / 2.0, 0.0));
    let roof = meshes.add(gable(nave.z, nave.x, 3.5));
    push(&mut parts, roof, palette.slate.clone(), Transform::from_xyz(0.0, nave.y, 0.0).with_rotation(Quat::from_rotation_y(FRAC_PI_2)));
    let tower_pos = Vec3::new(0.0, 8.0, -9.0);
    push(&mut parts, meshes.add(Cuboid::new(5.0, 16.0, 5.0)), palette.stone.clone(), Transform::from_translation(tower_pos));
    push(&mut parts, meshes.add(Cone::new(3.2, 7.0)), palette.slate.clone(), Transform::from_xyz(0.0, 16.0 + 3.5, -9.0));
    parts
}

fn farm(meshes: &mut Assets<Mesh>, palette: &Palette) -> Vec<Part> {
    let mut parts = Vec::new();
    house(
        &mut parts,
        meshes,
        palette.walls[0].clone(),
        palette.tile.clone(),
        Vec3::new(-12.0, 0.0, 0.0),
        0.0,
        9.0,
        6.5,
        4.5,
    );
    house(
        &mut parts,
        meshes,
        palette.brick.clone(),
        palette.slate.clone(),
        Vec3::new(8.0, 0.0, 6.0),
        0.0,
        16.0,
        9.0,
        6.5,
    );
    house(
        &mut parts,
        meshes,
        palette.brick.clone(),
        palette.slate.clone(),
        Vec3::new(10.0, 0.0, -14.0),
        0.0,
        22.0,
        10.0,
        7.0,
    );
    for x in [-4.0, 0.0] {
        push(
            &mut parts,
            meshes.add(Cylinder::new(2.6, 9.5)),
            palette.white.clone(),
            Transform::from_xyz(x - 2.0, 4.75, -9.0),
        );
    }
    push(
        &mut parts,
        meshes.add(Cylinder::new(3.0, 12.0)),
        palette.stone.clone(),
        Transform::from_xyz(-14.0, 6.0, 14.0),
    );
    parts
}

fn mill(meshes: &mut Assets<Mesh>, palette: &Palette) -> Vec<Part> {
    let mut parts = Vec::new();
    push(&mut parts, meshes.add(Cylinder::new(3.8, 11.0)), palette.stone.clone(), Transform::from_xyz(0.0, 5.5, 0.0));
    push(&mut parts, meshes.add(Cone::new(4.6, 4.5)), palette.tile.clone(), Transform::from_xyz(0.0, 13.25, 0.0));
    push(
        &mut parts,
        meshes.add(Cylinder::new(3.0, 0.6)),
        palette.timber.clone(),
        Transform::from_xyz(-4.6, 2.5, 0.0).with_rotation(Quat::from_rotation_z(FRAC_PI_2)),
    );
    parts
}

#[allow(clippy::too_many_arguments)]
fn house(
    parts: &mut Vec<Part>,
    meshes: &mut Assets<Mesh>,
    walls: Handle<StandardMaterial>,
    roof: Handle<StandardMaterial>,
    centre: Vec3,
    yaw: f32,
    width: f32,
    depth: f32,
    wall_height: f32,
) {
    let rotation = Quat::from_rotation_y(yaw);
    let rise = depth * 0.5;
    let local = |offset: Vec3| Transform::from_translation(centre + rotation * offset).with_rotation(rotation);
    push(parts, meshes.add(Cuboid::new(width, wall_height, depth)), walls, local(Vec3::new(0.0, wall_height / 2.0, 0.0)));
    push(parts, meshes.add(gable(width, depth, rise)), roof, local(Vec3::new(0.0, wall_height, 0.0)));
}

fn push(parts: &mut Vec<Part>, mesh: Handle<Mesh>, material: Handle<StandardMaterial>, transform: Transform) {
    parts.push(Part { mesh, material, transform });
}

// A pitched roof whose ridge runs along X, with gable ends, built from flat-shaded triangles.
fn gable(length: f32, span: f32, rise: f32) -> Mesh {
    let (l, s) = (length / 2.0, span / 2.0);
    let centre = Vec3::new(0.0, rise * 0.3, 0.0);
    let mut tris = Vec::new();
    let eave_front = [Vec3::new(-l, 0.0, -s), Vec3::new(l, 0.0, -s), Vec3::new(l, rise, 0.0), Vec3::new(-l, rise, 0.0)];
    let eave_back = [Vec3::new(-l, 0.0, s), Vec3::new(l, 0.0, s), Vec3::new(l, rise, 0.0), Vec3::new(-l, rise, 0.0)];
    for quad in [eave_front, eave_back] {
        outward(&mut tris, [quad[0], quad[1], quad[2]], centre);
        outward(&mut tris, [quad[0], quad[2], quad[3]], centre);
    }
    outward(&mut tris, [Vec3::new(l, 0.0, -s), Vec3::new(l, 0.0, s), Vec3::new(l, rise, 0.0)], centre);
    outward(&mut tris, [Vec3::new(-l, 0.0, -s), Vec3::new(-l, 0.0, s), Vec3::new(-l, rise, 0.0)], centre);

    let mut positions = Vec::with_capacity(tris.len() * 3);
    let mut normals = Vec::with_capacity(tris.len() * 3);
    for tri in &tris {
        let n = (tri[1] - tri[0]).cross(tri[2] - tri[0]).normalize_or_zero().to_array();
        for v in tri {
            positions.push(v.to_array());
            normals.push(n);
        }
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
}

fn outward(tris: &mut Vec<[Vec3; 3]>, [a, b, c]: [Vec3; 3], inside: Vec3) {
    let centroid = (a + b + c) / 3.0;
    let normal = (b - a).cross(c - a);
    if normal.dot(centroid - inside) < 0.0 {
        tris.push([a, c, b]);
    } else {
        tris.push([a, b, c]);
    }
}

struct Rng(u64);

impl Rng {
    fn from_position(p: Vec2) -> Self {
        Rng(((p.x.to_bits() as u64) << 32) ^ p.y.to_bits() as u64 ^ 0x9E37_79B9_7F4A_7C15)
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

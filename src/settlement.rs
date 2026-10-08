use std::f32::consts::FRAC_PI_2;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::PrimitiveTopology;
use bevy::prelude::*;

use crate::collision::{Colliders, Solid};
use crate::map::{Poi, TerrainMap};
use crate::settlement_plan::{footprint_points, Building, BuildingKind, Layout, Rng, SettlementPlan};

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
    /// Shop awnings and signs.
    awnings: Vec<Handle<StandardMaterial>>,
    glass: Handle<StandardMaterial>,
    /// Barn roofs, canopies, kiosk roofs.
    metal: Handle<StandardMaterial>,
    /// Forecourts.
    asphalt: Handle<StandardMaterial>,
    // Unit-sized shapes every house is scaled from, so a town of hundreds of houses shares
    // two meshes instead of allocating two each.
    unit_cube: Handle<Mesh>,
    unit_gable: Handle<Mesh>,
}

/// The solid footprint of a part, for collision: a part without one (a roof, a cone, a wheel)
/// can't be bumped into.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Footprint {
    /// A rectangular block this big (width, height, depth), before it is turned.
    Block(Vec3),
    Cylinder { radius: f32, height: f32 },
}

struct Part {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    transform: Transform,
    solid: Option<Footprint>,
}

/// The solid a part makes in the world, given where its settlement stands (`origin`: its ground
/// point) and the part's own transform, relative to that.
fn solid_for(origin: Vec3, transform: &Transform, footprint: Footprint) -> Solid {
    let centre = Vec2::new(origin.x + transform.translation.x, origin.z + transform.translation.z);
    let yaw = transform.rotation.to_euler(EulerRot::YXZ).0;
    match footprint {
        Footprint::Block(size) => Solid::rect(centre, Vec2::new(size.x, size.z) * 0.5, yaw, origin.y + transform.translation.y + size.y * 0.5),
        Footprint::Cylinder { radius, height } => Solid::circle(centre, radius, origin.y + transform.translation.y + height * 0.5),
    }
}

pub fn spawn_settlements(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    map: &TerrainMap,
    plan: &SettlementPlan,
    colliders: &mut Colliders,
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
        timber: colour(0.52, 0.4, 0.28),
        awnings: vec![colour(0.7, 0.12, 0.1), colour(0.12, 0.4, 0.2), colour(0.15, 0.25, 0.55), colour(0.85, 0.7, 0.15)],
        glass: colour(0.1, 0.14, 0.18),
        metal: colour(0.45, 0.47, 0.5),
        asphalt: colour(0.12, 0.12, 0.13),
        unit_cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        unit_gable: meshes.add(gable(1.0, 1.0, 1.0)),
    };

    for (poi, layout) in map.pois.iter().zip(&plan.layouts) {
        let ground = map.height_at(poi.position);
        let root = commands
            .spawn((
                SettlementRoot,
                Transform::from_xyz(poi.position.x, ground, poi.position.y),
                Visibility::default(),
            ))
            .id();
        let mut rng = Rng::from_position(poi.position);
        let _ = &mut rng;
        let parts = layout_parts(meshes, &palette, map, poi, layout);
        let origin = Vec3::new(poi.position.x, ground, poi.position.y);
        for part in parts {
            if let Some(footprint) = part.solid {
                colliders.add(solid_for(origin, &part.transform, footprint));
            }
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

/// A placement: where a building is and how it is turned, for putting its parts relative to it.
struct Frame {
    centre: Vec3,
    rotation: Quat,
}

impl Frame {
    fn new(centre: Vec3, yaw: f32) -> Frame {
        Frame { centre, rotation: Quat::from_rotation_y(yaw) }
    }

    /// A transform for a part at `offset` from the building's middle (in its own turned axes), scaled.
    fn at(&self, offset: Vec3, scale: Vec3) -> Transform {
        Transform::from_translation(self.centre + self.rotation * offset).with_rotation(self.rotation).with_scale(scale)
    }
}

/// Every building the plan has for a settlement, as parts: each stands where it was put, turned so
/// that its front (local -Z) faces the street, the yard or the water.
fn layout_parts(meshes: &mut Assets<Mesh>, palette: &Palette, map: &TerrainMap, poi: &Poi, layout: &Layout) -> Vec<Part> {
    let ground = map.height_at(poi.position);
    let relative = |p: Vec2| Vec3::new(p.x - poi.position.x, map.height_at(p) - ground, p.y - poi.position.y);
    let mut parts = Vec::new();
    for building in &layout.buildings {
        let centre = relative(building.centre);
        // Where the ground falls away from the middle of a building, its walls go down to meet it.
        let lowest = footprint_points(&building.shape()).iter().map(|&p| map.height_at(p) - ground).fold(f32::MAX, f32::min);
        let sink = (centre.y - lowest).max(0.0) + 0.15;
        let mut rng = Rng::from_position(building.centre);
        building_parts(&mut parts, meshes, palette, &mut rng, building, centre, sink);
    }
    parts
}

fn pick(rng: &mut Rng, list: &[Handle<StandardMaterial>]) -> Handle<StandardMaterial> {
    list[(rng.unit() * list.len() as f32) as usize % list.len()].clone()
}

fn building_parts(parts: &mut Vec<Part>, meshes: &mut Assets<Mesh>, palette: &Palette, rng: &mut Rng, b: &Building, centre: Vec3, sink: f32) {
    let frame = Frame::new(centre, b.yaw);
    let (w, d, h) = (b.width, b.depth, b.wall_height);
    let rise = d * 0.5;
    // The main walls and roof of a gabled building, sunk to meet the ground.
    let gabled = |parts: &mut Vec<Part>, walls: Handle<StandardMaterial>, roof: Handle<StandardMaterial>| {
        house(parts, palette, walls, roof, Vec3::new(centre.x, centre.y - sink, centre.z), b.yaw, w, d, h + sink);
    };
    let cube = palette.unit_cube.clone();
    let chimney = |parts: &mut Vec<Part>, along: f32| {
        push(parts, cube.clone(), palette.brick.clone(), frame.at(Vec3::new(along * w, h + rise + 0.3, 0.0), Vec3::new(0.85, 2.0, 0.95)));
    };
    match b.kind {
        BuildingKind::Cottage | BuildingKind::House | BuildingKind::Terrace | BuildingKind::Farmhouse => {
            let walls = if b.kind == BuildingKind::Farmhouse { palette.walls[0].clone() } else { pick(rng, &palette.walls) };
            let roof = if b.kind == BuildingKind::Farmhouse || rng.unit() >= 0.7 { palette.tile.clone() } else { palette.slate.clone() };
            gabled(parts, walls, roof);
            if b.kind == BuildingKind::Terrace {
                // A chimney for every house in the row.
                for k in 0..((w / 6.0) as usize).max(2) {
                    let along = (k as f32 + 0.5) / ((w / 6.0) as usize).max(2) as f32 - 0.5;
                    chimney(parts, along);
                }
            } else if rng.unit() < 0.7 {
                chimney(parts, if rng.unit() < 0.5 { -0.3 } else { 0.3 });
            }
        }
        BuildingKind::Church => {
            // The church is modelled around its own origin: shifted so its footprint is centred, then
            // turned and put where the plan says.
            for mut part in church(meshes, palette) {
                part.transform.translation = centre + frame.rotation * (part.transform.translation + Vec3::new(0.0, 0.0, CHURCH_SHIFT));
                part.transform.rotation = frame.rotation * part.transform.rotation;
                parts.push(part);
            }
        }
        BuildingKind::Pub => {
            gabled(parts, palette.walls[2].clone(), palette.slate.clone());
            chimney(parts, -0.32);
            chimney(parts, 0.32);
            // A hanging sign, sticking out from the front wall near the door.
            let sign = pick(rng, &palette.awnings);
            push(parts, cube.clone(), sign, frame.at(Vec3::new(w * 0.5 - 1.0, h - 0.9, -(d * 0.5 + 0.55)), Vec3::new(0.07, 0.9, 1.1)));
            // A porch over the door.
            push(parts, cube.clone(), palette.timber.clone(), frame.at(Vec3::new(-w * 0.15, h - 1.2, -(d * 0.5 + 0.7)), Vec3::new(2.6, 0.14, 1.4)));
        }
        BuildingKind::Shop => {
            gabled(parts, pick(rng, &palette.walls), palette.slate.clone());
            chimney(parts, 0.3);
            let awning = pick(rng, &palette.awnings);
            // Shop window, awning over the pavement, and the sign above.
            push(parts, cube.clone(), palette.glass.clone(), frame.at(Vec3::new(0.0, 1.35, -(d * 0.5 + 0.04)), Vec3::new(w * 0.8, 1.7, 0.08)));
            push(parts, cube.clone(), awning.clone(), frame.at(Vec3::new(0.0, 2.75, -(d * 0.5 + 0.75)), Vec3::new(w * 0.92, 0.1, 1.5)));
            push(parts, cube.clone(), awning, frame.at(Vec3::new(0.0, h - 0.4, -(d * 0.5 + 0.06)), Vec3::new(w * 0.7, 0.5, 0.12)));
        }
        BuildingKind::School => {
            gabled(parts, palette.brick.clone(), palette.slate.clone());
            // A bell turret on the ridge.
            push(parts, cube.clone(), palette.white.clone(), frame.at(Vec3::new(0.0, h + rise + 0.5, 0.0), Vec3::new(1.4, 1.7, 1.4)));
            push(parts, meshes.add(Cone::new(1.1, 1.5)), palette.slate.clone(), frame.at(Vec3::new(0.0, h + rise + 2.1, 0.0), Vec3::ONE));
            chimney(parts, -0.35);
        }
        BuildingKind::Hall => {
            gabled(parts, palette.walls[2].clone(), palette.tile.clone());
            push(parts, cube.clone(), palette.timber.clone(), frame.at(Vec3::new(0.0, 1.3, -(d * 0.5 + 0.9)), Vec3::new(3.2, 2.6, 1.8)));
            push(parts, cube.clone(), palette.metal.clone(), frame.at(Vec3::new(0.0, 2.7, -(d * 0.5 + 0.9)), Vec3::new(3.6, 0.2, 2.2)));
        }
        BuildingKind::Barn => {
            gabled(parts, palette.timber.clone(), palette.metal.clone());
            // A big door in the front.
            push(parts, cube.clone(), palette.glass.clone(), frame.at(Vec3::new(0.0, 2.0, -(d * 0.5 + 0.04)), Vec3::new(4.2, 4.0, 0.08)));
        }
        BuildingKind::Silo => {
            let r = w * 0.5;
            push_solid(parts, meshes.add(Cylinder::new(r, h)), palette.white.clone(), Transform::from_translation(centre + Vec3::Y * (h * 0.5 - sink)).with_scale(Vec3::new(1.0, 1.0, 1.0)), Footprint::Cylinder { radius: r, height: h });
            push(parts, meshes.add(Cone::new(r * 1.05, 1.6)), palette.metal.clone(), Transform::from_translation(centre + Vec3::Y * (h + 0.8 - sink)));
        }
        BuildingKind::Mill => {
            let r = w * 0.5;
            push_solid(parts, meshes.add(Cylinder::new(r, h)), palette.stone.clone(), Transform::from_translation(centre + Vec3::Y * (h * 0.5 - sink)), Footprint::Cylinder { radius: r, height: h });
            push(parts, meshes.add(Cone::new(r + 0.8, 4.5)), palette.tile.clone(), Transform::from_translation(centre + Vec3::Y * (h + 2.25 - sink)));
            // The wheel is on the water side (the building's front), turning about a horizontal axis.
            let wheel = frame.at(Vec3::new(0.0, 2.5, -(r + 0.5)), Vec3::ONE);
            push(parts, meshes.add(Cylinder::new(3.0, 0.6)), palette.timber.clone(), Transform { rotation: frame.rotation * Quat::from_rotation_x(FRAC_PI_2), ..wheel });
        }
        BuildingKind::PetrolStation => {
            // Local -Z faces the road: the canopy over the pumps nearest it, the kiosk behind.
            push(parts, cube.clone(), palette.asphalt.clone(), frame.at(Vec3::new(0.0, 0.04, 0.0), Vec3::new(w * 0.95, 0.08, d * 0.95)));
            push(parts, cube.clone(), palette.white.clone(), frame.at(Vec3::new(0.0, 5.0, -5.0), Vec3::new(13.0, 0.4, 8.5)));
            push(parts, cube.clone(), pick(rng, &palette.awnings), frame.at(Vec3::new(0.0, 4.75, -5.0), Vec3::new(13.2, 0.3, 8.7)));
            for (x, z) in [(-5.8, -8.2), (5.8, -8.2), (-5.8, -1.8), (5.8, -1.8)] {
                push_solid(parts, meshes.add(Cylinder::new(0.2, 4.75)), palette.metal.clone(), frame.at(Vec3::new(x, 2.375, z), Vec3::ONE), Footprint::Cylinder { radius: 0.2, height: 4.75 });
            }
            for (x, z) in [(-2.2, -6.2), (2.2, -6.2), (-2.2, -3.8), (2.2, -3.8)] {
                push_solid(parts, cube.clone(), palette.awnings[0].clone(), frame.at(Vec3::new(x, 0.75, z), Vec3::new(0.75, 1.5, 0.45)), Footprint::Block(Vec3::new(0.75, 1.5, 0.45)));
            }
            // The kiosk.
            push_solid(parts, cube.clone(), palette.white.clone(), frame.at(Vec3::new(0.0, 1.6, 6.0), Vec3::new(8.5, 3.2, 5.0)), Footprint::Block(Vec3::new(8.5, 3.2, 5.0)));
            push(parts, cube.clone(), palette.glass.clone(), frame.at(Vec3::new(0.0, 1.5, 6.0 - 2.54), Vec3::new(6.5, 1.6, 0.08)));
            push(parts, cube.clone(), palette.metal.clone(), frame.at(Vec3::new(0.0, 3.35, 6.0), Vec3::new(9.2, 0.3, 5.7)));
            // The price sign, on a pole by the road.
            push_solid(parts, cube.clone(), palette.metal.clone(), frame.at(Vec3::new(9.0, 3.5, -9.0), Vec3::new(0.35, 7.0, 0.35)), Footprint::Block(Vec3::new(0.35, 7.0, 0.35)));
            push(parts, cube.clone(), palette.awnings[3].clone(), frame.at(Vec3::new(9.0, 6.6, -9.0), Vec3::new(1.9, 1.7, 0.25)));
        }
    }
}

/// How far the church model's footprint is from its origin along its length (the nave runs from -7 to
/// 7 and the tower out to -11.5, so the middle of the whole is at -2.25).
const CHURCH_SHIFT: f32 = 2.25;

fn church(meshes: &mut Assets<Mesh>, palette: &Palette) -> Vec<Part> {
    let mut parts = Vec::new();
    let nave = Vec3::new(7.0, 6.0, 14.0);
    push_solid(&mut parts, meshes.add(Cuboid::new(nave.x, nave.y, nave.z)), palette.stone.clone(), Transform::from_xyz(0.0, nave.y / 2.0, 0.0), Footprint::Block(nave));
    let roof = meshes.add(gable(nave.z, nave.x, 3.5));
    push(&mut parts, roof, palette.slate.clone(), Transform::from_xyz(0.0, nave.y, 0.0).with_rotation(Quat::from_rotation_y(FRAC_PI_2)));
    let tower_pos = Vec3::new(0.0, 8.0, -9.0);
    push_solid(&mut parts, meshes.add(Cuboid::new(5.0, 16.0, 5.0)), palette.stone.clone(), Transform::from_translation(tower_pos), Footprint::Block(Vec3::new(5.0, 16.0, 5.0)));
    push(&mut parts, meshes.add(Cone::new(3.2, 7.0)), palette.slate.clone(), Transform::from_xyz(0.0, 16.0 + 3.5, -9.0));
    parts
}

#[allow(clippy::too_many_arguments)]
fn house(
    parts: &mut Vec<Part>,
    palette: &Palette,
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
    let local = |offset: Vec3, scale: Vec3| {
        Transform::from_translation(centre + rotation * offset)
            .with_rotation(rotation)
            .with_scale(scale)
    };
    push_solid(
        parts,
        palette.unit_cube.clone(),
        walls,
        local(Vec3::new(0.0, wall_height / 2.0, 0.0), Vec3::new(width, wall_height, depth)),
        Footprint::Block(Vec3::new(width, wall_height, depth)),
    );
    push(
        parts,
        palette.unit_gable.clone(),
        roof,
        local(Vec3::new(0.0, wall_height, 0.0), Vec3::new(width, rise, depth)),
    );
}

fn push(parts: &mut Vec<Part>, mesh: Handle<Mesh>, material: Handle<StandardMaterial>, transform: Transform) {
    parts.push(Part { mesh, material, transform, solid: None });
}

/// A part that can be bumped into.
fn push_solid(parts: &mut Vec<Part>, mesh: Handle<Mesh>, material: Handle<StandardMaterial>, transform: Transform, solid: Footprint) {
    parts.push(Part { mesh, material, transform, solid: Some(solid) });
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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_house_makes_a_solid_where_and_how_it_is_drawn() {
        // A house 8 x 6 m turned 0.6 radians, standing on ground at 20 m in a settlement at (100, 50).
        let origin = Vec3::new(100.0, 20.0, 50.0);
        let yaw = 0.6;
        let transform = Transform::from_translation(Vec3::new(3.0, 2.0, -5.0)).with_rotation(Quat::from_rotation_y(yaw)).with_scale(Vec3::new(8.0, 4.0, 6.0));
        let solid = solid_for(origin, &transform, Footprint::Block(Vec3::new(8.0, 4.0, 6.0)));
        assert!((solid.top_at(Vec2::ZERO) - 24.0).abs() < 1e-4, "walls reach 4 m above the ground: {}", solid.top_at(Vec2::ZERO));
        // The corners of the drawn house are on the solid's edge, so it's turned the same way.
        let corner = transform.transform_point(Vec3::new(0.5, 0.0, 0.5));
        let (distance, _) = solid.shape.separation(Vec2::new(origin.x + corner.x, origin.z + corner.z));
        assert!(distance.abs() < 1e-3, "corner is {distance} m from the solid's edge");
        let middle = Vec2::new(origin.x + 3.0, origin.z - 5.0);
        assert!(solid.shape.separation(middle).0 < -2.9, "the middle is well inside");
    }

    #[test]
    fn a_silo_makes_a_round_solid() {
        let transform = Transform::from_xyz(-4.0, 4.75, -9.0);
        let solid = solid_for(Vec3::new(10.0, 5.0, 10.0), &transform, Footprint::Cylinder { radius: 2.6, height: 9.5 });
        assert!((solid.top_at(Vec2::ZERO) - 14.5).abs() < 1e-4);
        let (distance, _) = solid.shape.separation(Vec2::new(6.0 + 2.6 + 1.0, 1.0));
        assert!((distance - 1.0).abs() < 1e-4);
    }

    /// A diagnostic, not an assertion: builds every settlement on a real map and measures how well the
    /// buildings sit among the roads, each other, water and slopes.
    ///   cargo test --lib how_well_do_settlements -- --ignored --nocapture
    #[test]
    #[ignore]
    fn how_well_do_settlements_fit_their_surroundings() {
        use crate::collision::Shape;
        use crate::params::GenParams;
        use crate::roads::{road_ribbons, RoadClearance, RoadNetwork};
        let params = GenParams::default();
        let map = TerrainMap::generate(crate::MAP_SEED, &params);
        let roads = RoadNetwork::generate(&map, &params);
        let plan = SettlementPlan::generate(&map, &roads);
        let mut streets = road_ribbons(&map, &roads);
        streets.extend(plan.lanes());
        let clearance = RoadClearance::new(&streets);
        #[derive(Default, Debug)]
        struct Tally {
            settlements: usize,
            buildings: usize,
            on_road: usize,
            by_road: usize,
            overlapping: usize,
            on_water: usize,
            on_slope: usize,
        }
        let mut tiers: std::collections::BTreeMap<&str, Tally> = Default::default();
        for (poi, layout) in map.pois.iter().zip(&plan.layouts) {
            let shapes: Vec<Shape> = layout.buildings.iter().map(|b| b.shape()).collect();
            let tier = match (poi.kind, poi.radius) {
                (crate::map::PoiKind::Mill, _) => "mill",
                (crate::map::PoiKind::Farm, _) => "farm",
                (_, r) if r >= 150.0 => "town",
                (_, r) if r >= 90.0 => "large village",
                (_, r) if r >= 45.0 => "village",
                _ => "hamlet",
            };
            let tally = tiers.entry(tier).or_default();
            tally.settlements += 1;
            for (i, shape) in shapes.iter().enumerate() {
                tally.buildings += 1;
                let pts = footprint_points(shape);
                let worst = pts.iter().map(|&p| clearance.clearance(p)).fold(f32::MAX, f32::min);
                if worst < 0.0 {
                    tally.on_road += 1;
                } else if worst < 3.0 {
                    tally.by_road += 1;
                }
                if shapes.iter().enumerate().any(|(j, other)| j != i && pts.iter().any(|&p| other.separation(p).0 < -0.01)) {
                    tally.overlapping += 1;
                }
                if pts.iter().any(|&p| map.water_distance(p) < 3.0 || map.water_surface_at(p).is_some()) {
                    tally.on_water += 1;
                }
                let heights: Vec<f32> = pts.iter().map(|&p| map.height_at(p)).collect();
                let range = heights.iter().cloned().fold(f32::MIN, f32::max) - heights.iter().cloned().fold(f32::MAX, f32::min);
                if range > 1.5 {
                    tally.on_slope += 1;
                }
            }
        }
        println!("{:14} {:>5} {:>9} {:>8} {:>9} {:>11} {:>8} {:>8}", "tier", "count", "buildings", "on road", "by road", "overlapping", "in water", ">1.5m slope");
        for (name, t) in &tiers {
            let pct = |n: usize| if t.buildings == 0 { 0.0 } else { 100.0 * n as f32 / t.buildings as f32 };
            println!("{name:14} {:>5} {:>9} {:>7.0}% {:>8.0}% {:>10.0}% {:>7.0}% {:>10.0}%", t.settlements, t.buildings, pct(t.on_road), pct(t.by_road), pct(t.overlapping), pct(t.on_water), pct(t.on_slope));
        }
    }
}

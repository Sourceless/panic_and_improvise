//! Buildings with insides: a floor plan of rooms, doors, windows and stairs for every kind of
//! building, turned into boxes that are baked into meshes and registered as solids.

pub mod furnish;
pub mod geom;
pub mod kinds;
pub mod plan;

use bevy::prelude::*;

use crate::collision::Material;
use crate::meshbake::Rgb;
use crate::settlement_plan::{Building, BuildingKind, Rng};
use geom::{Layer, Model, Piece};
use plan::Style;

pub mod colour {
    use crate::meshbake::Rgb;
    pub const WALLS: [Rgb; 5] = [[0.9, 0.86, 0.74], [0.93, 0.87, 0.6], [0.92, 0.92, 0.9], [0.62, 0.6, 0.56], [0.85, 0.72, 0.65]];
    pub const SLATE: Rgb = [0.25, 0.27, 0.3];
    pub const TILE: Rgb = [0.55, 0.25, 0.18];
    pub const STONE: Rgb = [0.6, 0.6, 0.58];
    pub const BRICK: Rgb = [0.55, 0.22, 0.16];
    pub const WHITE: Rgb = [0.92, 0.92, 0.9];
    pub const TIMBER: Rgb = [0.52, 0.4, 0.28];
    pub const AWNINGS: [Rgb; 4] = [[0.7, 0.12, 0.1], [0.12, 0.4, 0.2], [0.15, 0.25, 0.55], [0.85, 0.7, 0.15]];
    pub const GLASS: Rgb = [0.1, 0.14, 0.18];
    pub const METAL: Rgb = [0.45, 0.47, 0.5];
    pub const ASPHALT: Rgb = [0.12, 0.12, 0.13];
}

/// What the ground is like where a building stands.
#[derive(Clone, Copy, Debug)]
pub struct Site {
    /// How far below the floor the outside walls reach, to meet the ground at its lowest.
    pub skirt: f32,
    /// How far the floor is above the ground at the front door.
    pub door_rise: f32,
}

fn pick(rng: &mut Rng, list: &[Rgb]) -> Rgb {
    list[(rng.unit() * list.len() as f32) as usize % list.len()]
}

/// The look of a building: its walls and roof, by kind.
fn style_for(b: &Building, rng: &mut Rng) -> Style {
    let mut style = Style::default();
    let roof = if rng.unit() < 0.7 { colour::TILE } else { colour::SLATE };
    match b.kind {
        BuildingKind::Cottage | BuildingKind::House | BuildingKind::Terrace => {
            style.outer = pick(rng, &colour::WALLS);
            style.roof = roof;
        }
        BuildingKind::Farmhouse => {
            style.outer = colour::WALLS[0];
            style.roof = colour::TILE;
        }
        BuildingKind::Pub => {
            style.outer = colour::WALLS[2];
            style.roof = colour::SLATE;
        }
        BuildingKind::Shop => {
            style.outer = pick(rng, &colour::WALLS);
            style.roof = colour::SLATE;
        }
        BuildingKind::School => {
            style.outer = colour::BRICK;
            style.roof = colour::SLATE;
        }
        BuildingKind::Hall => {
            style.outer = colour::WALLS[2];
            style.roof = colour::TILE;
        }
        BuildingKind::Barn => {
            style.outer = colour::TIMBER;
            style.roof = colour::METAL;
            style.windows = false;
        }
        _ => {}
    }
    style.inner = [style.outer[0] * 0.5 + 0.45, style.outer[1] * 0.5 + 0.45, style.outer[2] * 0.5 + 0.45];
    style
}

/// The model of a building, in its own frame.
pub fn build(b: &Building, site: &Site, rng: &mut Rng) -> Model {
    match b.kind {
        BuildingKind::Silo => silo(b, site),
        BuildingKind::Mill => mill(b, site),
        BuildingKind::Church => church(b, site),
        BuildingKind::PetrolStation => petrol_station(b, site, rng),
        _ => gabled(b, site, rng),
    }
}

/// A building with a plan and a pitched roof.
fn gabled(b: &Building, site: &Site, rng: &mut Rng) -> Model {
    planned(b, site, rng).1
}

/// The plan of a building and the model made from it.
pub fn planned(b: &Building, site: &Site, rng: &mut Rng) -> (plan::Layout, Model) {
    let style = style_for(b, rng);
    let layout = kinds::layout(b, style, rng);
    let mut model = plan::assemble(&layout, site.skirt, rng);
    let height = layout.height();
    let (w, d) = (b.width, b.depth);
    let rise = (d * 0.5).min(3.6);
    let base = Vec3::new(layout.inside.centre().x, height, layout.inside.centre().y);
    model.push(Piece::Gable { base, length: w, span: d, rise, overhang: 0.35, yaw: 0.0 }, style.roof, Layer::Shell, None);

    let chimney = |model: &mut Model, along: f32| {
        model.block(Vec3::new(along * w, height + rise + 0.3, 0.0), Vec3::new(0.85, 2.0, 0.95), colour::BRICK, Layer::Shell, Some(Material::Stone));
    };
    match b.kind {
        BuildingKind::Cottage | BuildingKind::House | BuildingKind::Farmhouse => {
            if rng.unit() < 0.7 {
                chimney(&mut model, if rng.unit() < 0.5 { -0.3 } else { 0.3 });
            }
        }
        BuildingKind::Terrace => {
            let n = ((w / 6.0) as usize).max(2);
            for k in 0..n {
                chimney(&mut model, (k as f32 + 0.5) / n as f32 - 0.5);
            }
        }
        BuildingKind::Pub => {
            chimney(&mut model, -0.32);
            chimney(&mut model, 0.32);
            let sign = pick(rng, &colour::AWNINGS);
            model.block(Vec3::new(w * 0.5 - 1.0, height - 0.9, -(d * 0.5 + 0.55)), Vec3::new(0.07, 0.9, 1.1), sign, Layer::Shell, None);
            model.block(Vec3::new(-w * 0.15, 2.8, -(d * 0.5 + 0.7)), Vec3::new(2.6, 0.14, 1.4), colour::TIMBER, Layer::Shell, None);
        }
        BuildingKind::Shop => {
            chimney(&mut model, 0.3);
            let awning = pick(rng, &colour::AWNINGS);
            model.block(Vec3::new(0.0, 2.75, -(d * 0.5 + 0.75)), Vec3::new(w * 0.92, 0.1, 1.5), awning, Layer::Shell, None);
            model.block(Vec3::new(0.0, height - 0.4, -(d * 0.5 + 0.06)), Vec3::new(w * 0.7, 0.5, 0.12), awning, Layer::Shell, None);
        }
        BuildingKind::School => {
            model.block(Vec3::new(0.0, height + rise + 0.5, 0.0), Vec3::new(1.4, 1.7, 1.4), colour::WHITE, Layer::Shell, None);
            model.push(Piece::Cone { base: Vec3::new(0.0, height + rise + 1.35, 0.0), radius: 1.1, height: 1.5 }, colour::SLATE, Layer::Shell, None);
            chimney(&mut model, -0.35);
        }
        BuildingKind::Hall => {
            model.block(Vec3::new(0.0, 1.3, -(d * 0.5 + 0.9)), Vec3::new(3.2, 2.6, 1.8), colour::TIMBER, Layer::Shell, Some(Material::Wood));
            model.block(Vec3::new(0.0, 2.7, -(d * 0.5 + 0.9)), Vec3::new(3.6, 0.2, 2.2), colour::METAL, Layer::Shell, None);
        }
        _ => {}
    }
    steps(&mut model, &layout, site, d);
    (layout, model)
}

/// Steps up to each front door from the ground outside, if the floor is raised above it.
fn steps(model: &mut Model, layout: &plan::Layout, site: &Site, depth: f32) {
    let rise = site.door_rise;
    if rise < 0.3 {
        return;
    }
    let count = (rise / 0.3).ceil() as usize;
    let step = rise / count as f32;
    for door in &layout.storeys[0].doors_outside {
        if door.side != plan::Side::Front {
            continue;
        }
        for i in 0..count - 1 {
            // The outermost first: each is a step lower and a step further out.
            let top = -rise + (i as f32 + 1.0) * step;
            let out = (count - 1 - i) as f32 * 0.4;
            model.span(
                Vec3::new(door.along - door.width * 0.5 - 0.3, -rise - 0.6, -depth * 0.5 - out - 0.4),
                Vec3::new(door.along + door.width * 0.5 + 0.3, top, -depth * 0.5 - out + 0.0),
                colour::STONE,
                Layer::Shell,
                Some(Material::Stone),
            );
        }
    }
}

fn silo(b: &Building, site: &Site) -> Model {
    let mut model = Model::default();
    let r = b.width * 0.5;
    let h = b.wall_height;
    model.push(Piece::Cylinder { base: Vec3::new(0.0, -site.skirt, 0.0), radius: r, height: h + site.skirt }, colour::WHITE, Layer::Shell, Some(Material::Metal));
    model.push(Piece::Cone { base: Vec3::new(0.0, h, 0.0), radius: r * 1.05, height: 1.6 }, colour::METAL, Layer::Shell, None);
    model
}

fn mill(b: &Building, site: &Site) -> Model {
    let mut model = Model::default();
    let r = b.width * 0.5;
    let h = b.wall_height;
    model.push(Piece::Cylinder { base: Vec3::new(0.0, -site.skirt, 0.0), radius: r, height: h + site.skirt }, colour::STONE, Layer::Shell, Some(Material::Stone));
    model.push(Piece::Cone { base: Vec3::new(0.0, h, 0.0), radius: r + 0.8, height: 4.5 }, colour::TILE, Layer::Shell, None);
    // The wheel, on the water side, turning about a horizontal axis.
    model.block(Vec3::new(0.0, 2.5, -(r + 0.5)), Vec3::new(0.6, 6.0, 6.0), colour::TIMBER, Layer::Shell, None);
    model
}

fn church(b: &Building, site: &Site) -> Model {
    let mut model = Model::default();
    let _ = b;
    // Modelled from the nave out: nave 7 x 14, the tower 5 x 5 at its front end.
    let shift = 2.25;
    let nave = Vec3::new(7.0, 6.0, 14.0);
    model.block(Vec3::new(0.0, nave.y * 0.5 - site.skirt * 0.5, shift), Vec3::new(nave.x, nave.y + site.skirt, nave.z), colour::STONE, Layer::Shell, Some(Material::Stone));
    model.push(Piece::Gable { base: Vec3::new(0.0, nave.y, shift), length: nave.x, span: nave.z, rise: 3.5, overhang: 0.0, yaw: std::f32::consts::FRAC_PI_2 }, colour::SLATE, Layer::Shell, None);
    model.block(Vec3::new(0.0, 8.0 - site.skirt * 0.5, -9.0 + shift), Vec3::new(5.0, 16.0 + site.skirt, 5.0), colour::STONE, Layer::Shell, Some(Material::Stone));
    model.push(Piece::Cone { base: Vec3::new(0.0, 16.0, -9.0 + shift), radius: 3.2, height: 7.0 }, colour::SLATE, Layer::Shell, None);
    model
}

fn petrol_station(b: &Building, _site: &Site, rng: &mut Rng) -> Model {
    let mut model = Model::default();
    let (w, d) = (b.width, b.depth);
    let cube = |model: &mut Model, c: Vec3, size: Vec3, colour: Rgb, solid: Option<Material>| model.block(c, size, colour, Layer::Shell, solid);
    cube(&mut model, Vec3::new(0.0, 0.04, 0.0), Vec3::new(w * 0.95, 0.08, d * 0.95), colour::ASPHALT, None);
    cube(&mut model, Vec3::new(0.0, 5.0, -5.0), Vec3::new(13.0, 0.4, 8.5), colour::WHITE, None);
    cube(&mut model, Vec3::new(0.0, 4.75, -5.0), Vec3::new(13.2, 0.3, 8.7), pick(rng, &colour::AWNINGS), None);
    for (x, z) in [(-5.8, -8.2), (5.8, -8.2), (-5.8, -1.8), (5.8, -1.8)] {
        model.push(Piece::Cylinder { base: Vec3::new(x, 0.0, z), radius: 0.2, height: 4.75 }, colour::METAL, Layer::Shell, Some(Material::Metal));
    }
    for (x, z) in [(-2.2, -6.2), (2.2, -6.2), (-2.2, -3.8), (2.2, -3.8)] {
        cube(&mut model, Vec3::new(x, 0.75, z), Vec3::new(0.75, 1.5, 0.45), colour::AWNINGS[0], Some(Material::Metal));
    }
    // The kiosk, solid for now.
    cube(&mut model, Vec3::new(0.0, 1.6, 6.0), Vec3::new(8.5, 3.2, 5.0), colour::WHITE, Some(Material::Stone));
    cube(&mut model, Vec3::new(0.0, 1.5, 3.46), Vec3::new(6.5, 1.6, 0.08), colour::GLASS, None);
    cube(&mut model, Vec3::new(0.0, 3.35, 6.0), Vec3::new(9.2, 0.3, 5.7), colour::METAL, None);
    cube(&mut model, Vec3::new(9.0, 3.5, -9.0), Vec3::new(0.35, 7.0, 0.35), colour::METAL, Some(Material::Metal));
    cube(&mut model, Vec3::new(9.0, 6.6, -9.0), Vec3::new(1.9, 1.7, 0.25), colour::AWNINGS[3], None);
    model
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::TerrainMap;
    use crate::params::GenParams;
    use crate::settlement_plan::SettlementPlan;

    /// Writes a model of one building of a kind on the real map as JSON, for `tools/draw_building.py`.
    ///   BUILDING_KIND=House BUILDING_INDEX=0 BUILDING_DUMP=/tmp/house.json cargo test --lib dump_a_building -- --ignored
    #[test]
    #[ignore = "writes a file for the drawing tool"]
    fn dump_a_building() {
        let kind = std::env::var("BUILDING_KIND").unwrap_or_else(|_| "House".into());
        let index: usize = std::env::var("BUILDING_INDEX").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
        let path = std::env::var("BUILDING_DUMP").expect("BUILDING_DUMP names the file");
        let params = GenParams::default();
        let map = TerrainMap::generate(crate::MAP_SEED, &params);
        let roads = crate::roads::RoadNetwork::generate(&map, &params);
        let plan = SettlementPlan::generate(&map, &roads);
        let b = plan.layouts.iter().flat_map(|l| &l.buildings).filter(|b| format!("{:?}", b.kind) == kind).nth(index).expect("no such building");
        let ground = crate::settlement::ground_under(&map, b);
        let mut rng = Rng::from_position(b.centre);
        let model = build(b, &ground.site, &mut rng);
        let items: Vec<String> = model
            .items
            .iter()
            .map(|i| {
                let (kind, a, b, c, d) = match i.piece {
                    Piece::Block { centre, size, yaw } => ("block", centre, size, yaw, 0.0),
                    Piece::Gable { base, length, span, rise, yaw, .. } => ("gable", base, Vec3::new(length, rise, span), yaw, 0.0),
                    Piece::Cylinder { base, radius, height } => ("cylinder", base, Vec3::new(radius, height, 0.0), 0.0, 0.0),
                    Piece::Cone { base, radius, height } => ("cone", base, Vec3::new(radius, height, 0.0), 0.0, 0.0),
                };
                let _ = d;
                format!(
                    "{{\"kind\":\"{kind}\",\"centre\":[{},{},{}],\"size\":[{},{},{}],\"yaw\":{},\"colour\":[{},{},{}],\"interior\":{},\"solid\":{}}}",
                    a.x, a.y, a.z, b.x, b.y, b.z, c, i.colour[0], i.colour[1], i.colour[2], i.layer == Layer::Interior, i.solid.is_some()
                )
            })
            .collect();
        std::fs::write(&path, format!("{{\"building\":\"{:?} {:.1}x{:.1}\",\"items\":[{}]}}", b.kind, b.width, b.depth, items.join(","))).unwrap();
        eprintln!("{:?} {:.1} x {:.1}: {} items", b.kind, b.width, b.depth, model.items.len());
    }

    use crate::collision::{Colliders, PLAYER_RADIUS, STEP_DOWN, STEP_UP};
    use std::collections::{HashSet, VecDeque};

    const CELL: f32 = 0.1;

    /// Where a person could walk in a building, found by walking: from outside the front door, over
    /// every cell they can step to without being pushed back by something solid or dropping off an edge,
    /// following the floor (and the stairs) up and down. Returns the cells reached, with the height of
    /// the feet in each.
    fn walk(model: &geom::Model, layout: &plan::Layout, ground: f32) -> Vec<(IVec2, f32)> {
        let mut colliders = Colliders::default();
        for solid in model.solids(Vec3::ZERO, 0.0) {
            colliders.add(solid);
        }
        let bounds = layout.inside.grown(2.5);
        let cell_at = |c: IVec2| Vec2::new(bounds.x0 + (c.x as f32 + 0.5) * CELL, bounds.z0 + (c.y as f32 + 0.5) * CELL);
        let feet_at = |p: Vec2, from: f32| -> Option<f32> {
            // The floor under a point as far as someone whose feet are at `from` can tell: what they can
            // stand on, or else the ground (which has to be near enough to step down to).
            let support = colliders.support(p, from, STEP_UP);
            let floor = support.unwrap_or(ground).max(ground);
            ((floor - from) <= STEP_UP + 1e-3 && (from - floor) <= STEP_DOWN).then_some(floor)
        };
        let mut seen: HashSet<(IVec2, i32)> = HashSet::new();
        let mut reached: Vec<(IVec2, f32)> = Vec::new();
        let mut queue = VecDeque::new();
        // From just outside each front door.
        for door in &layout.storeys[0].doors_outside {
            let start = IVec2::new(((door.along - bounds.x0) / CELL) as i32, ((layout.inside.z0 - 1.0 - bounds.z0) / CELL) as i32);
            seen.insert((start, 0));
            queue.push_back((start, ground));
        }
        while let Some((c, feet)) = queue.pop_front() {
            reached.push((c, feet));
            for step in [IVec2::X, -IVec2::X, IVec2::Y, -IVec2::Y] {
                let n = c + step;
                if n.x < 0 || n.y < 0 || n.x >= (bounds.width() / CELL) as i32 || n.y >= (bounds.depth() / CELL) as i32 {
                    continue;
                }
                let p = cell_at(n);
                let Some(floor) = feet_at(p, feet) else { continue };
                // Not pushed back by anything solid at the height of the new floor.
                if colliders.resolve(p, PLAYER_RADIUS, floor, STEP_UP).distance(p) > 0.02 {
                    continue;
                }
                // The same square at another height (another storey) is a different place.
                if !seen.insert((n, (floor * 10.0).round() as i32)) {
                    continue;
                }
                queue.push_back((n, floor));
            }
        }
        reached
    }

    /// Every room of a building can be walked to from the front door, at its own floor.
    fn check_every_room_is_reachable(b: &Building, map: &TerrainMap) -> Result<(), String> {
        let ground = crate::settlement::ground_under(map, b);
        let mut rng = Rng::from_position(b.centre);
        let (layout, model) = planned(b, &ground.site, &mut rng);
        let reached = walk(&model, &layout, -0.12);
        let bounds = layout.inside.grown(2.5);
        for (s, storey) in layout.storeys.iter().enumerate() {
            let floor = layout.floor_of(s);
            for (i, room) in storey.rooms.iter().enumerate() {
                let inside = room.rect.grown(-0.3);
                let found = reached.iter().any(|&(c, feet)| {
                    let p = Vec2::new(bounds.x0 + (c.x as f32 + 0.5) * CELL, bounds.z0 + (c.y as f32 + 0.5) * CELL);
                    inside.contains(p) && (feet - floor).abs() < 0.3
                });
                if !found {
                    return Err(format!("{:?} {:.1}x{:.1}: the {:?} (room {i}) of storey {s} can't be reached", b.kind, b.width, b.depth, room.kind));
                }
            }
        }
        Ok(())
    }

    #[test]
    fn every_room_of_every_house_can_be_walked_to_from_the_front_door() {
        let params = GenParams::default();
        let map = TerrainMap::generate(crate::MAP_SEED, &params);
        let roads = crate::roads::RoadNetwork::generate(&map, &params);
        let plan = SettlementPlan::generate(&map, &roads);
        let mut checked = 0;
        let mut failures = Vec::new();
        for (i, b) in plan.layouts.iter().flat_map(|l| &l.buildings).filter(|b| matches!(b.kind, BuildingKind::Cottage | BuildingKind::House | BuildingKind::Farmhouse | BuildingKind::Terrace)).enumerate() {
            if i % 4 != 0 {
                continue;
            }
            checked += 1;
            if let Err(e) = check_every_room_is_reachable(b, &map) {
                failures.push(e);
            }
        }
        eprintln!("{checked} buildings checked, {} failures", failures.len());
        if let Ok(which) = std::env::var("SHOW_FAILURE") {
            let which: usize = which.parse().unwrap_or(0);
            let b = plan.layouts.iter().flat_map(|l| &l.buildings).filter(|b| matches!(b.kind, BuildingKind::Cottage | BuildingKind::House | BuildingKind::Farmhouse | BuildingKind::Terrace)).enumerate().filter(|(i, b)| i % 4 == 0 && check_every_room_is_reachable(b, &map).is_err()).map(|(_, b)| b).nth(which).unwrap();
            let ground = crate::settlement::ground_under(&map, b);
            let mut rng = Rng::from_position(b.centre);
            let (layout, model) = planned(b, &ground.site, &mut rng);
            let reached = walk(&model, &layout, -0.12);
            let bounds = layout.inside.grown(2.5);
            let (nx, nz) = ((bounds.width() / CELL) as i32, (bounds.depth() / CELL) as i32);
            for level in 0..layout.storeys.len() {
                eprintln!("storey {level}  ({:?} {:.1}x{:.1})", b.kind, b.width, b.depth);
                let floor = layout.floor_of(level);
                for z in (0..nz).step_by(2) {
                    let row: String = (0..nx.min(70)).step_by(1).map(|x| if reached.iter().any(|&(c, f)| c == IVec2::new(x, z) && (f - floor).abs() < 0.3) { '.' } else { '#' }).collect();
                    eprintln!("{row}");
                }
            }
        }
        assert!(failures.is_empty(), "{} of {checked} buildings have rooms that can't be reached:\n{}", failures.len(), failures.iter().take(12).cloned().collect::<Vec<_>>().join("\n"));
    }

    #[test]
    #[ignore = "debugging aid"]
    fn why_cant_they_step_here() {
        let params = GenParams::default();
        let map = TerrainMap::generate(crate::MAP_SEED, &params);
        let roads = crate::roads::RoadNetwork::generate(&map, &params);
        let plan = SettlementPlan::generate(&map, &roads);
        let kind = std::env::var("BKIND").unwrap_or_else(|_| "House".into());
        let index: usize = std::env::var("BINDEX").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
        let b = plan.layouts.iter().flat_map(|l| &l.buildings).filter(|b| format!("{:?}", b.kind) == kind).nth(index).unwrap();
        let ground = crate::settlement::ground_under(&map, b);
        let mut rng = Rng::from_position(b.centre);
        let (layout, model) = planned(b, &ground.site, &mut rng);
        eprintln!("inside {:?}", layout.inside);
        for (s, st) in layout.storeys.iter().enumerate() {
            for r in &st.rooms {
                eprintln!("storey {s} {:?} {:?}", r.kind, r.rect);
            }
            for stair in &st.stairs_up {
                eprintln!("storey {s} stair {:?}", stair);
            }
        }
        let mut colliders = Colliders::default();
        for solid in model.solids(Vec3::ZERO, 0.0) {
            colliders.add(solid);
        }
        let x: f32 = std::env::var("PX").unwrap().parse().unwrap();
        let z: f32 = std::env::var("PZ").unwrap().parse().unwrap();
        let from: f32 = std::env::var("FEET").unwrap().parse().unwrap();
        let p = Vec2::new(x, z);
        let support = colliders.support(p, from, STEP_UP);
        let floor = support.unwrap_or(-0.12).max(-0.12);
        let pushed = colliders.resolve(p, PLAYER_RADIUS, floor, STEP_UP);
        eprintln!("at {p:?} from {from}: support {support:?}, floor {floor}, resolve -> {pushed:?} (moved {:.3})", pushed.distance(p));
    }
}

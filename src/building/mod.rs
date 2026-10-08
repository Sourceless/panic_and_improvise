//! Buildings with insides: a floor plan of rooms, doors, windows and stairs for every kind of
//! building, turned into boxes that are baked into meshes and registered as solids.

pub mod furnish;
pub mod geom;
pub mod kinds;
pub mod mill;
pub mod plan;
pub mod public;

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
    pub const GLASS_PANE: Rgb = [0.55, 0.7, 0.8];
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
        BuildingKind::Church => {
            style.outer = colour::STONE;
            style.roof = colour::SLATE;
        }
        BuildingKind::Shed => {
            style.outer = [0.62, 0.62, 0.64];
            style.roof = colour::METAL;
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
    planned(b, site, rng).1
}

/// The plans of the parts of a building that have insides, and the model made from them.
pub fn planned(b: &Building, site: &Site, rng: &mut Rng) -> (Vec<plan::Layout>, Model) {
    match b.kind {
        BuildingKind::Silo => (Vec::new(), silo(b, site)),
        BuildingKind::Mill => (Vec::new(), mill(b, site)),
        BuildingKind::PetrolStation => petrol_station(b, site, rng),
        _ => gabled(b, site, rng),
    }
}

/// A building with a plan and a pitched roof.
fn gabled(b: &Building, site: &Site, rng: &mut Rng) -> (Vec<plan::Layout>, Model) {
    let style = style_for(b, rng);
    let volumes = kinds::volumes(b, style, rng);
    let mut model = Model::default();
    for v in &volumes {
        model.extend(plan::assemble(&v.layout, site.skirt, rng));
        roof(&mut model, v, &style);
    }
    let height = volumes[0].layout.height();
    let (w, d) = (b.width, b.depth);
    let rise = (d * 0.5).min(3.6);

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
            // A porch over the door: a roof on two posts.
            let along = volumes[0].layout.storeys[0].doors_outside[0].along;
            model.block(Vec3::new(along, 2.9, -(d * 0.5 + 0.9)), Vec3::new(3.6, 0.2, 2.2), colour::METAL, Layer::Shell, None);
            for sx in [-1.5, 1.5] {
                model.block(Vec3::new(along + sx, 1.45, -(d * 0.5 + 1.8)), Vec3::new(0.15, 2.9, 0.15), colour::TIMBER, Layer::Shell, Some(Material::Wood));
            }
        }
        BuildingKind::Church => {
            // The tower goes on up above its room, solid, with a spire; and a porch roof for the door.
            model.block(Vec3::new(0.0, 4.6 + 5.7, -6.75), Vec3::new(5.0, 11.4, 5.0), colour::STONE, Layer::Shell, Some(Material::Stone));
            model.push(Piece::Cone { base: Vec3::new(0.0, 16.0, -6.75), radius: 3.2, height: 7.0 }, colour::SLATE, Layer::Shell, None);
        }
        _ => {}
    }
    steps(&mut model, &volumes[0].layout, site);
    (volumes.into_iter().map(|v| v.layout).collect(), model)
}

/// The roof over a part of a building.
fn roof(model: &mut Model, v: &plan::Volume, style: &Style) {
    let outside = v.layout.inside.grown(style.wall_thickness * 0.5);
    let c = outside.centre();
    let height = v.layout.height();
    match v.roof {
        plan::Roof::Gable { along: plan::Axis::X, rise } => {
            model.push(Piece::Gable { base: Vec3::new(c.x, height, c.y), length: outside.width(), span: outside.depth(), rise, overhang: 0.35, yaw: 0.0 }, style.roof, Layer::Shell, None);
        }
        plan::Roof::Gable { along: plan::Axis::Z, rise } => {
            model.push(
                Piece::Gable { base: Vec3::new(c.x, height, c.y), length: outside.depth(), span: outside.width(), rise, overhang: 0.35, yaw: std::f32::consts::FRAC_PI_2 },
                style.roof,
                Layer::Shell,
                None,
            );
        }
        plan::Roof::Flat => {
            model.block(Vec3::new(c.x, height + 0.12, c.y), Vec3::new(outside.width() + 0.3, 0.24, outside.depth() + 0.3), colour::METAL, Layer::Shell, None);
        }
        plan::Roof::Open => {}
    }
}

/// Steps up to each front door from the ground outside, if the floor is raised above it.
fn steps(model: &mut Model, layout: &plan::Layout, site: &Site) {
    let front = layout.inside.z0 - layout.style.wall_thickness * 0.5;
    for door in &layout.storeys[0].doors_outside {
        if door.side == plan::Side::Front {
            steps_at(model, door.along, door.width, front, site.door_rise);
        }
    }
}

/// Steps up to a door at `along` in the front wall at `front` (z), a floor `rise` above the ground.
pub(crate) fn steps_at(model: &mut Model, along: f32, width: f32, front: f32, rise: f32) {
    if rise < 0.3 {
        return;
    }
    let count = (rise / 0.3).ceil() as usize;
    let step = rise / count as f32;
    for i in 0..count - 1 {
        // The outermost first: each is a step lower and a step further out.
        let top = -rise + (i as f32 + 1.0) * step;
        let out = (count - 1 - i) as f32 * 0.4;
        model.span(
            Vec3::new(along - width * 0.5 - 0.3, -rise - 0.6, front - out - 0.4),
            Vec3::new(along + width * 0.5 + 0.3, top, front - out),
            colour::STONE,
            Layer::Shell,
            Some(Material::Stone),
        );
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
    mill::build(b, site)
}

fn petrol_station(b: &Building, _site: &Site, rng: &mut Rng) -> (Vec<plan::Layout>, Model) {
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
    // The kiosk is a small shop.
    let volume = public::kiosk(style_for(b, rng));
    model.extend(plan::assemble(&volume.layout, _site.skirt, rng));
    roof(&mut model, &volume, &volume.layout.style);
    let layouts = vec![volume.layout.clone()];
    cube(&mut model, Vec3::new(9.0, 3.5, -9.0), Vec3::new(0.35, 7.0, 0.35), colour::METAL, Some(Material::Metal));
    cube(&mut model, Vec3::new(9.0, 6.6, -9.0), Vec3::new(1.9, 1.7, 0.25), colour::AWNINGS[3], None);
    (layouts, model)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::TerrainMap;
    use crate::params::GenParams;
    use crate::settlement_plan::SettlementPlan;

    /// The model of a building as JSON, for the drawing tool, and how many items it has.
    fn model_json(b: &Building, map: &TerrainMap) -> (String, usize) {
        let ground = crate::settlement::ground_under(map, b);
        let mut rng = Rng::from_position(b.centre);
        let model = build(b, &ground.site, &mut rng);
        let items: Vec<String> = model
            .items
            .iter()
            .map(|i| {
                let (kind, a, b, c) = match i.piece {
                    Piece::Block { centre, size, yaw } => ("block", centre, size, yaw),
                    Piece::Gable { base, length, span, rise, yaw, .. } => ("gable", base, Vec3::new(length, rise, span), yaw),
                    Piece::Cylinder { base, radius, height } => ("cylinder", base, Vec3::new(radius, height, 0.0), 0.0),
                    Piece::Cone { base, radius, height } => ("cone", base, Vec3::new(radius, height, 0.0), 0.0),
                };
                format!(
                    "{{\"kind\":\"{kind}\",\"centre\":[{},{},{}],\"size\":[{},{},{}],\"yaw\":{},\"colour\":[{},{},{}],\"interior\":{},\"solid\":{}}}",
                    a.x, a.y, a.z, b.x, b.y, b.z, c, i.colour[0], i.colour[1], i.colour[2], i.layer == Layer::Interior, i.solid.is_some()
                )
            })
            .collect();
        (format!("{{\"building\":\"{:?} {:.1}x{:.1}\",\"items\":[{}]}}", b.kind, b.width, b.depth, items.join(",")), model.items.len())
    }

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
        let (json, count) = model_json(b, &map);
        std::fs::write(&path, json).unwrap();
        eprintln!("{:?} {:.1} x {:.1}: {} items", b.kind, b.width, b.depth, count);
    }

    use crate::collision::{Colliders, PLAYER_RADIUS, STEP_DOWN, STEP_UP};
    use geom::Rect;
    use std::collections::{HashSet, VecDeque};

    const CELL: f32 = 0.1;

    /// Where a person could walk in a building, found by walking: from outside the front door, over
    /// every cell they can step to without being pushed back by something solid or dropping off an edge,
    /// following the floor (and the stairs) up and down. Returns the cells reached, with the height of
    /// the feet in each, and the bounds the cells are numbered in.
    fn walk(model: &geom::Model, layouts: &[plan::Layout], ground: f32) -> (Vec<(IVec2, f32)>, geom::Rect) {
        let mut colliders = Colliders::default();
        for solid in model.solids(Vec3::ZERO, 0.0) {
            colliders.add(solid);
        }
        let mut bounds = layouts[0].inside;
        for l in layouts {
            bounds.x0 = bounds.x0.min(l.inside.x0);
            bounds.z0 = bounds.z0.min(l.inside.z0);
            bounds.x1 = bounds.x1.max(l.inside.x1);
            bounds.z1 = bounds.z1.max(l.inside.z1);
        }
        let bounds = bounds.grown(2.5);
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
        // From just outside each front door of the first part.
        let first = &layouts[0];
        for door in first.storeys[0].doors_outside.iter().filter(|d| d.side == plan::Side::Front) {
            let start = IVec2::new(((door.along - bounds.x0) / CELL) as i32, ((first.inside.z0 - 1.0 - bounds.z0) / CELL) as i32);
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
        (reached, bounds)
    }

    /// Every room of a building can be walked to from the front door, at its own floor.
    fn check_every_room_is_reachable(b: &Building, map: &TerrainMap) -> Result<(), String> {
        let ground = crate::settlement::ground_under(map, b);
        let mut rng = Rng::from_position(b.centre);
        let (layouts, model) = planned(b, &ground.site, &mut rng);
        if layouts.is_empty() {
            return Ok(());
        }
        let (reached, bounds) = walk(&model, &layouts, -0.12);
        for layout in &layouts {
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
        }
        Ok(())
    }

    #[test]
    fn every_room_of_every_house_can_be_walked_to_from_the_front_door() {
        let params = GenParams::default();
        let map = TerrainMap::generate(crate::MAP_SEED, &params);
        let roads = crate::roads::RoadNetwork::generate(&map, &params);
        let mut plan = SettlementPlan::generate(&map, &roads);
        plan.sheds = crate::fill::shed_buildings(&map, &crate::zones::ZoneMap::generate(&map, &params));
        let mut checked = 0;
        let mut failures = Vec::new();
        for (i, b) in plan.layouts.iter().flat_map(|l| &l.buildings).chain(&plan.sheds).filter(|b| !matches!(b.kind, BuildingKind::Silo | BuildingKind::Mill)).enumerate() {
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
            let b = plan.layouts.iter().flat_map(|l| &l.buildings).filter(|b| !matches!(b.kind, BuildingKind::Silo | BuildingKind::Mill)).enumerate().filter(|(i, b)| i % 4 == 0 && check_every_room_is_reachable(b, &map).is_err()).map(|(_, b)| b).nth(which).unwrap();
            if let Ok(path) = std::env::var("SHOW_DUMP") {
                std::fs::write(path, model_json(b, &map).0).unwrap();
            }
            let ground = crate::settlement::ground_under(&map, b);
            let mut rng = Rng::from_position(b.centre);
            let (layouts, model) = planned(b, &ground.site, &mut rng);
            let (reached, bounds) = walk(&model, &layouts, -0.12);
            for l in &layouts {
                for (si, st) in l.storeys.iter().enumerate() {
                    for (ri, r) in st.rooms.iter().enumerate() {
                        eprintln!("storey {si} room {ri} {:?} {:.2},{:.2} to {:.2},{:.2}", r.kind, r.rect.x0, r.rect.z0, r.rect.x1, r.rect.z1);
                    }
                }
            }
            if let Ok(room) = std::env::var("SHOW_ROOM") {
                let ri: usize = room.parse().unwrap();
                let r = layouts[0].storeys[0].rooms[ri].rect.grown(0.3);
                for it in &model.items {
                    if let Piece::Block { centre, size, .. } = it.piece {
                        let b = Rect::new(centre.x - size.x * 0.5, centre.z - size.z * 0.5, centre.x + size.x * 0.5, centre.z + size.z * 0.5);
                        if b.overlaps(&r) && centre.y - size.y * 0.5 < 1.0 && it.solid.is_some() {
                            eprintln!("  item {:.2},{:.2} to {:.2},{:.2} y {:.2}..{:.2}", b.x0, b.z0, b.x1, b.z1, centre.y - size.y * 0.5, centre.y + size.y * 0.5);
                        }
                    }
                }
            }
            let (nx, nz) = ((bounds.width() / CELL) as i32, (bounds.depth() / CELL) as i32);
            let levels = layouts.iter().map(|l| l.storeys.len()).max().unwrap_or(1);
            for level in 0..levels {
                eprintln!("storey {level}  ({:?} {:.1}x{:.1})", b.kind, b.width, b.depth);
                let floor = layouts[0].floor_of(level.min(layouts[0].storeys.len() - 1));
                for z in (0..nz).step_by(3) {
                    let row: String = (0..nx.min(180)).step_by(2).map(|x| if reached.iter().any(|&(c, f)| c == IVec2::new(x, z) && (f - floor).abs() < 0.3) { '.' } else { '#' }).collect();
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
        let b = match std::env::var("BFAILING") {
            Ok(n) => plan
                .layouts
                .iter()
                .flat_map(|l| &l.buildings)
                .filter(|b| !matches!(b.kind, BuildingKind::Silo | BuildingKind::Mill))
                .enumerate()
                .filter(|(i, b)| i % 4 == 0 && check_every_room_is_reachable(b, &map).is_err())
                .map(|(_, b)| b)
                .nth(n.parse().unwrap())
                .unwrap(),
            Err(_) => plan.layouts.iter().flat_map(|l| &l.buildings).filter(|b| format!("{:?}", b.kind) == kind).nth(index).unwrap(),
        };
        let ground = crate::settlement::ground_under(&map, b);
        let mut rng = Rng::from_position(b.centre);
        let (layouts, model) = planned(b, &ground.site, &mut rng);
        let layout = layouts[0].clone();
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

    #[test]
    fn the_mills_floors_are_all_reached_by_its_stairs() {
        let params = GenParams::default();
        let map = TerrainMap::generate(crate::MAP_SEED, &params);
        let roads = crate::roads::RoadNetwork::generate(&map, &params);
        let plan = SettlementPlan::generate(&map, &roads);
        let mills: Vec<&Building> = plan.layouts.iter().flat_map(|l| &l.buildings).filter(|b| b.kind == BuildingKind::Mill).collect();
        assert!(!mills.is_empty());
        for b in mills {
            let ground = crate::settlement::ground_under(&map, b);
            let mut rng = Rng::from_position(b.centre);
            let (_, model) = planned(b, &ground.site, &mut rng);
            // A stand-in for a plan: the floors and a door.
            let storey = b.wall_height / 3.0;
            let room = plan::Room { rect: Rect::new(-1.0, -1.0, 1.0, 1.0), kind: plan::RoomKind::Mill };
            let floors: Vec<plan::Storey> = (0..3)
                .map(|_| plan::Storey {
                    rooms: vec![room],
                    height: storey,
                    doors_outside: vec![plan::ExteriorDoor { side: plan::Side::Front, along: 0.0, width: 1.2, height: 2.2 }],
                    ..Default::default()
                })
                .collect();
            let layout = plan::Layout { inside: Rect::new(-3.0, -3.0, 3.0, 3.0), storeys: floors, style: plan::Style::default() };
            // The door is at the front of the ring, 3.7 m out; start outside it.
            let (reached, bounds) = walk(&model, &[layout.clone()], -0.12);
            for s in 0..3 {
                let floor = layout.floor_of(s);
                let found = reached.iter().any(|&(c, feet)| {
                    let p = Vec2::new(bounds.x0 + (c.x as f32 + 0.5) * CELL, bounds.z0 + (c.y as f32 + 0.5) * CELL);
                    p.length() < 3.0 && (feet - floor).abs() < 0.3
                });
                assert!(found, "floor {s} of the mill can't be reached");
            }
        }
    }

    /// How long it takes to build every building of the map, and how much there is of it.
    ///   cargo test --lib how_much_is_built -- --ignored --nocapture
    #[test]
    #[ignore = "a measurement"]
    fn how_much_is_built() {
        let params = GenParams::default();
        let map = TerrainMap::generate(crate::MAP_SEED, &params);
        let roads = crate::roads::RoadNetwork::generate(&map, &params);
        let mut plan = SettlementPlan::generate(&map, &roads);
        plan.sheds = crate::fill::shed_buildings(&map, &crate::zones::ZoneMap::generate(&map, &params));
        let start = std::time::Instant::now();
        let (mut items, mut solids, mut shell_tris, mut count) = (0, 0, 0usize, 0);
        for b in plan.layouts.iter().flat_map(|l| &l.buildings).chain(&plan.sheds) {
            let ground = crate::settlement::ground_under(&map, b);
            let mut rng = Rng::from_position(b.centre);
            let model = build(b, &ground.site, &mut rng);
            items += model.items.len();
            solids += model.solids(Vec3::ZERO, 0.0).len();
            shell_tris += model.items.iter().filter(|i| i.layer == Layer::Shell).count() * 12;
            count += 1;
        }
        eprintln!("{count} buildings in {:.2?}: {items} items, {solids} solids, about {shell_tris} shell triangles", start.elapsed());
    }
}

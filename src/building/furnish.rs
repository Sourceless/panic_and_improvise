//! What goes in a room. Furniture is boxes, placed against walls and in the middle of rooms that are
//! big enough, never across a doorway, a stair or the way round the room.

use bevy::prelude::*;

use super::geom::{Layer, Model, Rect};
use super::plan::{Room, RoomKind, Side, Style};
use crate::collision::Material;
use crate::meshbake::Rgb;
use crate::settlement_plan::Rng;

const WOOD: Rgb = [0.45, 0.3, 0.18];
const DARK_WOOD: Rgb = [0.28, 0.19, 0.12];
const PALE_WOOD: Rgb = [0.7, 0.58, 0.4];
const LINEN: Rgb = [0.86, 0.86, 0.9];
const WHITE: Rgb = [0.9, 0.9, 0.92];
const BLACK: Rgb = [0.12, 0.12, 0.14];
const COUNTER: Rgb = [0.78, 0.76, 0.7];
const FABRICS: [Rgb; 5] = [[0.55, 0.2, 0.2], [0.2, 0.35, 0.5], [0.3, 0.45, 0.3], [0.5, 0.45, 0.38], [0.35, 0.3, 0.45]];
const STEEL: Rgb = [0.6, 0.62, 0.65];

/// Everything needed to place things in one room.
pub struct Ctx<'a> {
    pub model: &'a mut Model,
    /// The part of the room that is floor, inside the walls.
    pub clear: Rect,
    /// Places that must stay open (doorways, stairs).
    avoid: &'a [Rect],
    taken: Vec<Rect>,
    pub floor: f32,
    pub height: f32,
    pub rng: &'a mut Rng,
}

pub fn inward(side: Side) -> Vec2 {
    match side {
        Side::Front => Vec2::Y,
        Side::Back => -Vec2::Y,
        Side::Left => Vec2::X,
        Side::Right => -Vec2::X,
    }
}

impl Ctx<'_> {
    pub fn pick(&mut self, list: &[Rgb]) -> Rgb {
        list[(self.rng.unit() * list.len() as f32) as usize % list.len()]
    }

    /// Whether `r` is on the floor and in nobody's way.
    pub fn free(&self, r: &Rect) -> bool {
        r.x0 >= self.clear.x0 - 1e-3
            && r.x1 <= self.clear.x1 + 1e-3
            && r.z0 >= self.clear.z0 - 1e-3
            && r.z1 <= self.clear.z1 + 1e-3
            && !self.avoid.iter().any(|a| a.overlaps(r))
            && !self.taken.iter().any(|t| t.overlaps(&r.grown(0.04)))
    }

    /// The length of a side of the room.
    pub fn length(&self, side: Side) -> f32 {
        match side {
            Side::Front | Side::Back => self.clear.width(),
            Side::Left | Side::Right => self.clear.depth(),
        }
    }

    /// A `w` by `d` rectangle against `side`, starting `along` from its start (the left/near corner).
    pub fn at_wall(&self, side: Side, along: f32, w: f32, d: f32) -> Rect {
        let c = self.clear;
        match side {
            Side::Front => Rect::new(c.x0 + along, c.z0, c.x0 + along + w, c.z0 + d),
            Side::Back => Rect::new(c.x0 + along, c.z1 - d, c.x0 + along + w, c.z1),
            Side::Left => Rect::new(c.x0, c.z0 + along, c.x0 + d, c.z0 + along + w),
            Side::Right => Rect::new(c.x1 - d, c.z0 + along, c.x1, c.z0 + along + w),
        }
    }

    /// The strip of `r` against the wall `side`, `t` thick.
    pub fn wall_strip(&self, side: Side, r: &Rect, t: f32) -> Rect {
        match side {
            Side::Front => Rect::new(r.x0, r.z0, r.x1, r.z0 + t),
            Side::Back => Rect::new(r.x0, r.z1 - t, r.x1, r.z1),
            Side::Left => Rect::new(r.x0, r.z0, r.x0 + t, r.z1),
            Side::Right => Rect::new(r.x1 - t, r.z0, r.x1, r.z1),
        }
    }

    /// Claims `r`, if it is free.
    pub fn take(&mut self, r: Rect) -> bool {
        if self.free(&r) {
            self.taken.push(r);
            true
        } else {
            false
        }
    }

    /// Finds room for a `w` by `d` thing against one of the walls, trying the corners first and then
    /// places along them, and claims it. Gives back where it went and which wall it is against.
    pub fn against_a_wall(&mut self, w: f32, d: f32, sides: &[Side]) -> Option<(Rect, Side)> {
        let mut order: Vec<Side> = sides.to_vec();
        // A different wall first each time.
        if order.len() > 1 {
            let k = (self.rng.unit() * order.len() as f32) as usize % order.len();
            order.rotate_left(k);
        }
        for side in order {
            let len = self.length(side);
            if len < w {
                continue;
            }
            let mut spots = vec![0.0, len - w, (len - w) * 0.5, (len - w) * 0.25, (len - w) * 0.75];
            if self.rng.unit() < 0.5 {
                spots.swap(0, 1);
            }
            for along in spots {
                let r = self.at_wall(side, along, w, d);
                if self.take(r) {
                    return Some((r, side));
                }
            }
        }
        None
    }

    /// Adds a box standing on the floor.
    pub fn solid_box(&mut self, r: &Rect, y0: f32, h: f32, colour: Rgb, material: Option<Material>) {
        self.model.span(Vec3::new(r.x0, self.floor + y0, r.z0), Vec3::new(r.x1, self.floor + y0 + h, r.z1), colour, Layer::Interior, material);
    }

    /// A box that can be walked through (a rug, a picture on the wall).
    pub fn flat(&mut self, r: &Rect, y0: f32, h: f32, colour: Rgb) {
        self.solid_box(r, y0, h, colour, None);
    }
}

pub fn furnish(model: &mut Model, room: &Room, avoid: &[Rect], floor: f32, height: f32, _style: &Style, rng: &mut Rng) {
    let mut ctx = Ctx { model, clear: room.rect.grown(-0.22), avoid, taken: Vec::new(), floor, height, rng };
    match room.kind {
        RoomKind::Bedroom => bedroom(&mut ctx),
        RoomKind::Living => living(&mut ctx),
        RoomKind::Kitchen => kitchen(&mut ctx),
        RoomKind::Dining => dining(&mut ctx),
        RoomKind::Bathroom => bathroom(&mut ctx),
        RoomKind::Toilet => toilet(&mut ctx),
        RoomKind::Bar => bar(&mut ctx),
        RoomKind::Lounge => lounge(&mut ctx),
        RoomKind::Shop => shop(&mut ctx),
        RoomKind::Store | RoomKind::Cellar => store(&mut ctx),
        RoomKind::Classroom => classroom(&mut ctx),
        RoomKind::Cloakroom => cloakroom(&mut ctx),
        RoomKind::StaffRoom => {
            living(&mut ctx);
            office_desk(&mut ctx);
        }
        RoomKind::Office => office_desk(&mut ctx),
        RoomKind::MainHall => main_hall(&mut ctx),
        RoomKind::Barn => barn(&mut ctx),
        RoomKind::Workshop => workshop(&mut ctx),
        RoomKind::Nave => nave(&mut ctx),
        RoomKind::Chancel => chancel(&mut ctx),
        _ => {}
    }
}

const WALLS: [Side; 4] = [Side::Back, Side::Left, Side::Right, Side::Front];

fn bedroom(c: &mut Ctx) {
    let big = c.clear.area() > 11.0 && c.clear.width().min(c.clear.depth()) > 2.8;
    let (w, d) = if big { (1.55, 2.05) } else { (0.95, 1.95) };
    // The head of the bed against a wall, the bed pointing into the room.
    let bed = (0..4).find_map(|_| {
        let sides = WALLS;
        let (side, along_wall) = {
            let side = sides[(c.rng.unit() * 4.0) as usize % 4];
            (side, c.length(side))
        };
        // A bed is long across the room: its width runs along the wall.
        if along_wall < w + 1.2 {
            return None;
        }
        let along = c.rng.range(0.4, (along_wall - w - 0.4).max(0.4));
        let r = c.at_wall(side, along, w, d);
        c.take(r).then_some((r, side))
    });
    let Some((bed, side)) = bed.or_else(|| c.against_a_wall(w, d, &WALLS)) else { return };
    let linen = c.pick(&FABRICS);
    c.solid_box(&bed, 0.0, 0.3, WOOD, Some(Material::Wood));
    let mattress = Rect::new(bed.x0 + 0.04, bed.z0 + 0.04, bed.x1 - 0.04, bed.z1 - 0.04);
    c.solid_box(&mattress, 0.3, 0.22, linen, None);
    let head = c.wall_strip(side, &bed, 0.1);
    c.solid_box(&head, 0.3, 0.6, DARK_WOOD, None);
    // A pillow or two at the head.
    let pillow_depth = 0.35;
    let strip = c.wall_strip(side, &mattress, pillow_depth + 0.1);
    let pillows = match side {
        Side::Front | Side::Back => Rect::new(strip.x0 + 0.1, strip.z0 + 0.1, strip.x1 - 0.1, strip.z1 - 0.1),
        _ => Rect::new(strip.x0 + 0.1, strip.z0 + 0.1, strip.x1 - 0.1, strip.z1 - 0.1),
    };
    c.flat(&pillows, 0.52, 0.1, LINEN);

    if let Some((r, _)) = c.against_a_wall(1.2, 0.6, &WALLS) {
        c.solid_box(&r, 0.0, 1.9, WOOD, Some(Material::Wood));
    }
    for _ in 0..2 {
        if let Some((r, _)) = c.against_a_wall(0.45, 0.4, &WALLS) {
            c.solid_box(&r, 0.0, 0.5, PALE_WOOD, Some(Material::Wood));
        }
    }
    if big && c.rng.unit() < 0.5 {
        if let Some((r, _)) = c.against_a_wall(1.2, 0.6, &WALLS) {
            c.solid_box(&r, 0.0, 0.75, PALE_WOOD, Some(Material::Wood));
        }
    }
    rug(c, 0.5);
}

fn rug(c: &mut Ctx, chance: f32) {
    if c.rng.unit() > chance || c.clear.width().min(c.clear.depth()) < 2.6 {
        return;
    }
    let centre = c.clear.centre();
    let (w, d) = ((c.clear.width() * 0.45).min(2.6), (c.clear.depth() * 0.4).min(2.0));
    let r = Rect::new(centre.x - w * 0.5, centre.y - d * 0.5, centre.x + w * 0.5, centre.y + d * 0.5);
    let colour = c.pick(&FABRICS);
    c.flat(&r, 0.0, 0.02, [colour[0] * 0.8, colour[1] * 0.8, colour[2] * 0.8]);
}

fn living(c: &mut Ctx) {
    // The television on one wall and the sofa facing it across the room.
    let min_side = c.clear.width().min(c.clear.depth());
    let tv = c.against_a_wall(1.3, 0.45, &WALLS);
    if let Some((r, side)) = tv {
        c.solid_box(&r, 0.0, 0.5, DARK_WOOD, Some(Material::Wood));
        let set = c.wall_strip(side, &Rect::new(r.x0 + 0.1, r.z0 + 0.1, r.x1 - 0.1, r.z1 - 0.1), 0.08);
        c.flat(&set, 0.55, 0.6, BLACK);
        if min_side > 3.0 {
            // The sofa on the far side, facing the set.
            let opposite = match side {
                Side::Front => Side::Back,
                Side::Back => Side::Front,
                Side::Left => Side::Right,
                Side::Right => Side::Left,
            };
            let along = (c.length(opposite) - 2.0) * 0.5;
            let r = c.at_wall(opposite, along.max(0.0), 2.0, 0.9);
            if c.take(r) {
                let fabric = c.pick(&FABRICS);
                c.solid_box(&r, 0.0, 0.45, fabric, Some(Material::Wood));
                let back = c.wall_strip(opposite, &r, 0.22);
                c.solid_box(&back, 0.45, 0.4, fabric, None);
            }
        }
    }
    if let Some((r, _)) = c.against_a_wall(0.9, 0.85, &WALLS) {
        let fabric = c.pick(&FABRICS);
        c.solid_box(&r, 0.0, 0.45, fabric, Some(Material::Wood));
        c.solid_box(&Rect::new(r.x0, r.z0, r.x1, r.z1).grown(-0.05), 0.45, 0.25, fabric, None);
    }
    if let Some((r, _)) = c.against_a_wall(1.0, 0.3, &WALLS) {
        c.solid_box(&r, 0.0, 1.8, WOOD, Some(Material::Wood));
    }
    if min_side > 3.3 {
        let m = c.clear.centre();
        let r = Rect::new(m.x - 0.5, m.y - 0.3, m.x + 0.5, m.y + 0.3);
        if c.take(r) {
            c.solid_box(&r, 0.0, 0.4, PALE_WOOD, Some(Material::Wood));
        }
    }
    rug(c, 0.7);
}

fn kitchen(c: &mut Ctx) {
    // A run of units along a wall: fridge, counter, cooker, sink, counter.
    let sides = WALLS;
    let mut best: Option<(Side, f32)> = None;
    for side in sides {
        let len = c.length(side);
        // How much of it is clear, counting the first free stretch from the corner.
        let mut free_len = 0.0;
        while free_len + 0.6 <= len {
            let r = c.at_wall(side, free_len, 0.6, 0.62);
            if !c.free(&r) {
                break;
            }
            free_len += 0.6;
        }
        if best.is_none_or(|(_, l)| free_len > l) {
            best = Some((side, free_len));
        }
    }
    if let Some((side, len)) = best {
        let units = ((len / 0.6) as usize).min(6);
        let run = c.at_wall(side, 0.0, units as f32 * 0.6, 0.62);
        if units >= 2 && c.take(run) {
            let order = [Unit::Fridge, Unit::Counter, Unit::Cooker, Unit::Counter, Unit::Sink, Unit::Counter];
            for (i, unit) in order.iter().take(units).enumerate() {
                let r = c.at_wall(side, i as f32 * 0.6, 0.6, 0.62);
                match unit {
                    Unit::Fridge => c.solid_box(&r, 0.0, 1.75, WHITE, Some(Material::Metal)),
                    Unit::Cooker => {
                        c.solid_box(&r, 0.0, 0.9, STEEL, Some(Material::Metal));
                        c.flat(&Rect::new(r.x0 + 0.05, r.z0 + 0.05, r.x1 - 0.05, r.z1 - 0.05), 0.9, 0.03, BLACK);
                    }
                    Unit::Sink => {
                        c.solid_box(&r, 0.0, 0.9, COUNTER, Some(Material::Wood));
                        c.flat(&Rect::new(r.x0 + 0.08, r.z0 + 0.08, r.x1 - 0.08, r.z1 - 0.08), 0.9, 0.04, STEEL);
                    }
                    Unit::Counter => {
                        c.solid_box(&r, 0.0, 0.88, COUNTER, Some(Material::Wood));
                        c.flat(&r, 0.88, 0.04, DARK_WOOD);
                    }
                }
            }
        }
    }
    if c.clear.width().min(c.clear.depth()) > 3.4 {
        table_and_chairs(c, 1.2, 0.8, 4);
    }
}

#[derive(Clone, Copy)]
enum Unit {
    Fridge,
    Counter,
    Cooker,
    Sink,
}

fn dining(c: &mut Ctx) {
    let (w, d) = if c.clear.width().max(c.clear.depth()) > 3.8 { (1.8, 1.0) } else { (1.3, 0.9) };
    table_and_chairs(c, w, d, 6);
    if let Some((r, _)) = c.against_a_wall(1.4, 0.45, &WALLS) {
        c.solid_box(&r, 0.0, 0.9, DARK_WOOD, Some(Material::Wood));
    }
    rug(c, 0.0);
}

/// A table in the middle of the room with a chair each side of it (and each end, if there's room).
fn table_and_chairs(c: &mut Ctx, w: f32, d: f32, chairs: usize) {
    let m = c.clear.centre();
    table_set(c, m, w, d, chairs, false);
}

/// A table `w` by `d` centred on `m`, with chairs round it. `any_way` leaves it the way round it was
/// given; otherwise it lies along the longer way of the room.
fn table_set(c: &mut Ctx, m: Vec2, w: f32, d: f32, chairs: usize, any_way: bool) -> bool {
    // Along the longer way of the room.
    let (w, d) = if any_way || c.clear.width() >= c.clear.depth() { (w, d) } else { (d, w) };
    let t = Rect::new(m.x - w * 0.5, m.y - d * 0.5, m.x + w * 0.5, m.y + d * 0.5);
    // Room for chairs all round it.
    if !c.free(&t.grown(0.55)) {
        return false;
    }
    c.taken.push(t.grown(0.45));
    c.solid_box(&t, 0.72, 0.05, WOOD, Some(Material::Wood));
    for (dx, dz) in [(-0.5, -0.5), (0.5, -0.5), (-0.5, 0.5), (0.5, 0.5)] {
        let leg = Rect::new(m.x + dx * (w - 0.14) - 0.04, m.y + dz * (d - 0.14) - 0.04, m.x + dx * (w - 0.14) + 0.04, m.y + dz * (d - 0.14) + 0.04);
        c.solid_box(&leg, 0.0, 0.72, WOOD, None);
    }
    let seats: Vec<(f32, f32, bool)> = {
        let mut v = vec![(-0.25, -1.0, true), (0.25, -1.0, true), (-0.25, 1.0, true), (0.25, 1.0, true)];
        if chairs > 4 {
            v.push((-1.0, 0.0, false));
            v.push((1.0, 0.0, false));
        }
        v
    };
    for (i, (a, b, along_x)) in seats.iter().enumerate() {
        if i >= chairs {
            break;
        }
        let (cx, cz) = if *along_x { (m.x + a * w, m.y + b * (d * 0.5 + 0.28)) } else { (m.x + a * (w * 0.5 + 0.28), m.y + b * d * 0.5) };
        let seat = Rect::new(cx - 0.2, cz - 0.2, cx + 0.2, cz + 0.2);
        c.solid_box(&seat, 0.0, 0.45, PALE_WOOD, None);
        let back = if *along_x { Rect::new(seat.x0, cz + b.signum() * 0.16 - 0.02, seat.x1, cz + b.signum() * 0.16 + 0.02) } else { Rect::new(cx + a.signum() * 0.16 - 0.02, seat.z0, cx + a.signum() * 0.16 + 0.02, seat.z1) };
        c.solid_box(&back, 0.45, 0.4, PALE_WOOD, None);
    }
    true
}

fn bathroom(c: &mut Ctx) {
    // The bath against the short wall, then the toilet and basin.
    let (long, short) = if c.clear.width() >= c.clear.depth() { (c.clear.width(), c.clear.depth()) } else { (c.clear.depth(), c.clear.width()) };
    if short >= 1.7 && long >= 2.4 {
        let sides: &[Side] = if c.clear.width() >= c.clear.depth() { &[Side::Left, Side::Right] } else { &[Side::Back, Side::Front] };
        if let Some((r, _)) = c.against_a_wall(0.75, 1.7, sides) {
            c.solid_box(&r, 0.0, 0.55, WHITE, Some(Material::Metal));
            c.flat(&Rect::new(r.x0 + 0.06, r.z0 + 0.06, r.x1 - 0.06, r.z1 - 0.06), 0.55, 0.02, [0.7, 0.8, 0.9]);
        }
    }
    toilet(c);
}

fn toilet(c: &mut Ctx) {
    if let Some((r, side)) = c.against_a_wall(0.4, 0.65, &WALLS) {
        c.solid_box(&r, 0.0, 0.4, WHITE, Some(Material::Metal));
        let cistern = c.wall_strip(side, &r, 0.2);
        c.solid_box(&cistern, 0.4, 0.4, WHITE, None);
    }
    if let Some((r, _)) = c.against_a_wall(0.5, 0.4, &WALLS) {
        c.solid_box(&r, 0.0, 0.85, WHITE, Some(Material::Metal));
    }
}


/// Things fixed along a wall in pieces, each of which goes where there is room: shelves, a run of
/// cupboards. `f` draws the piece at the rectangle it is given.
fn along_wall(c: &mut Ctx, side: Side, from: f32, to: f32, depth: f32, piece: f32, mut f: impl FnMut(&mut Ctx, Rect)) {
    let mut at = from;
    while at + piece <= to + 1e-3 {
        let r = c.at_wall(side, at, piece, depth);
        if c.take(r) {
            f(c, r);
        }
        at += piece;
    }
}

/// Bottles and tins: small boxes of assorted colour standing on a shelf.
fn goods(c: &mut Ctx, r: &Rect, y: f32, colours: &[Rgb]) {
    let n = ((r.width().max(r.depth())) / 0.22) as usize;
    for i in 0..n {
        let t = (i as f32 + 0.5) / n.max(1) as f32;
        let colour = colours[(c.rng.unit() * colours.len() as f32) as usize % colours.len()];
        let h = c.rng.range(0.12, 0.3);
        let small = if r.width() >= r.depth() {
            let x = r.x0 + r.width() * t;
            Rect::new(x - 0.07, r.z0 + r.depth() * 0.5 - 0.07, x + 0.07, r.z0 + r.depth() * 0.5 + 0.07)
        } else {
            let z = r.z0 + r.depth() * t;
            Rect::new(r.x0 + r.width() * 0.5 - 0.07, z - 0.07, r.x0 + r.width() * 0.5 + 0.07, z + 0.07)
        };
        c.flat(&small, y, h, colour);
    }
}

const BOTTLES: [Rgb; 5] = [[0.2, 0.45, 0.25], [0.55, 0.3, 0.1], [0.7, 0.7, 0.75], [0.5, 0.1, 0.12], [0.15, 0.2, 0.45]];
const PRODUCE: [Rgb; 6] = [[0.8, 0.2, 0.15], [0.9, 0.75, 0.2], [0.25, 0.55, 0.25], [0.85, 0.85, 0.8], [0.3, 0.4, 0.75], [0.75, 0.45, 0.2]];

fn bar(c: &mut Ctx) {
    let r = c.clear;
    if r.depth() < 3.6 || r.width() < 4.0 {
        return lounge(c);
    }
    // The bar runs across the room, the barman's side of it against the back wall.
    // As long as the doors allow.
    let mut counter = Rect::new(r.x0 + 1.1, r.z1 - 1.95, r.x1 - 0.5, r.z1 - 1.25);
    while counter.width() > 2.4 && !c.free(&counter) {
        counter.x1 -= 0.25;
    }
    if counter.width() > 2.4 && c.take(counter) {
        c.solid_box(&counter, 0.0, 1.0, DARK_WOOD, Some(Material::Wood));
        c.flat(&counter.grown(0.04), 1.0, 0.05, PALE_WOOD);
        // Stools along the front of it.
        let mut x = counter.x0 + 0.5;
        while x < counter.x1 - 0.3 {
            let stool = Rect::new(x - 0.18, counter.z0 - 0.62, x + 0.18, counter.z0 - 0.26);
            if c.take(stool) {
                c.solid_box(&stool, 0.0, 0.65, WOOD, Some(Material::Wood));
            }
            x += 0.95;
        }
    }
    // Shelves of bottles behind it.
    along_wall(c, Side::Back, 1.1, c.length(Side::Back) - 0.5, 0.35, 0.8, |c, shelf| {
        c.solid_box(&shelf, 0.0, 2.0, DARK_WOOD, Some(Material::Wood));
        for y in [0.9, 1.35, 1.8] {
            goods(c, &shelf, y, &BOTTLES);
        }
    });
    // Tables in front of the bar, a fireplace on the side wall.
    let front = Rect::new(r.x0, r.z0, r.x1, r.z1 - 1.95 - 0.9);
    let mut placed = 0;
    for row in 0..2 {
        for col in 0..3 {
            let m = Vec2::new(front.x0 + 1.3 + col as f32 * 2.0, front.z0 + 1.0 + row as f32 * 1.7);
            if front.contains(m) && placed < 4 && table_set(c, m, 0.8, 0.8, 2, true) {
                placed += 1;
            }
        }
    }
    if let Some((fire, _)) = c.against_a_wall(1.4, 0.45, &[Side::Left, Side::Front]) {
        c.solid_box(&fire, 0.0, 1.3, [0.5, 0.2, 0.15], Some(Material::Stone));
        c.flat(&fire.grown(-0.2), 0.0, 0.5, BLACK);
    }
}

fn lounge(c: &mut Ctx) {
    let r = c.clear;
    // Benches along the side walls with tables in front of them, then tables further in.
    for side in [Side::Left, Side::Right] {
        along_wall(c, side, 0.4, c.length(side) - 0.4, 0.5, 1.6, |c, bench| {
            let fabric = FABRICS[1];
            c.solid_box(&bench, 0.0, 0.45, fabric, Some(Material::Wood));
            let back = c.wall_strip(side, &bench, 0.12);
            c.solid_box(&back, 0.45, 0.45, fabric, None);
        });
    }
    let mut placed = 0;
    for row in 0..3 {
        for col in 0..3 {
            let m = Vec2::new(r.x0 + 1.6 + col as f32 * 1.9, r.z0 + 0.9 + row as f32 * 1.4);
            if r.contains(m) && placed < 4 && table_set(c, m, 0.8, 0.8, 2, true) {
                placed += 1;
            }
        }
    }
}

fn shop(c: &mut Ctx) {
    let r = c.clear;
    // A counter against the right wall with the till, shelves round the other walls, one aisle of stock
    // in the middle with room to walk right round it, and fridges at the back.
    let mut counter = Rect::new(r.x1 - 0.7, r.z0 + 1.6, r.x1, r.z0 + 3.2);
    while counter.depth() > 0.9 && !c.free(&counter) {
        counter.z1 -= 0.2;
    }
    if counter.depth() > 0.9 && c.take(counter) {
        c.solid_box(&counter, 0.0, 1.0, PALE_WOOD, Some(Material::Wood));
        c.flat(&Rect::new(counter.x0 + 0.1, counter.z0 + 0.2, counter.x0 + 0.45, counter.z0 + 0.6), 1.0, 0.25, BLACK);
    }
    for side in [Side::Left, Side::Back] {
        along_wall(c, side, 0.0, c.length(side), 0.4, 0.9, |c, shelf| {
            c.solid_box(&shelf, 0.0, 1.9, WOOD, Some(Material::Wood));
            for y in [0.5, 1.0, 1.5] {
                goods(c, &shelf.grown(-0.05), y, &PRODUCE);
            }
        });
    }
    let m = r.centre();
    let aisle = Rect::new(m.x - 1.3, m.y - 0.25, m.x + 1.3, m.y + 0.25);
    if r.width() > 5.5 && c.free(&aisle.grown(0.45)) && c.take(aisle) {
        c.solid_box(&aisle, 0.0, 1.5, WOOD, Some(Material::Wood));
        goods(c, &aisle.grown(-0.05), 1.5, &PRODUCE);
    }
    along_wall(c, Side::Right, c.length(Side::Right) * 0.6, c.length(Side::Right), 0.7, 0.8, |c, f| {
        c.solid_box(&f, 0.0, 1.9, WHITE, Some(Material::Metal));
    });
}

fn store(c: &mut Ctx) {
    // Shelves of boxes round the walls and a few crates in the middle.
    for side in [Side::Back, Side::Left, Side::Right] {
        along_wall(c, side, 0.0, c.length(side), 0.45, 1.0, |c, shelf| {
            c.solid_box(&shelf, 0.0, 1.9, WOOD, Some(Material::Wood));
            for y in [0.6, 1.2, 1.8] {
                goods(c, &shelf.grown(-0.06), y, &PRODUCE);
            }
        });
    }
    let m = c.clear.centre();
    if c.clear.width().min(c.clear.depth()) > 3.2 {
        let crate_box = Rect::new(m.x - 0.4, m.y - 0.4, m.x + 0.4, m.y + 0.4);
        if c.take(crate_box) {
            c.solid_box(&crate_box, 0.0, 0.8, PALE_WOOD, Some(Material::Wood));
        }
    }
}

fn classroom(c: &mut Ctx) {
    let r = c.clear;
    // The board on the back wall, the teacher's desk before it, and rows of desks facing it.
    let width = (r.width() - 2.0).clamp(2.0, 4.0);
    let board = Rect::new(r.x0 + (r.width() - width) * 0.5, r.z1 - 0.07, r.x0 + (r.width() + width) * 0.5, r.z1);
    c.flat(&board, 1.0, 1.2, [0.12, 0.25, 0.2]);
    let desk = Rect::new(r.x0 + (r.width() - 1.5) * 0.5, r.z1 - 1.6, r.x0 + (r.width() + 1.5) * 0.5, r.z1 - 0.9);
    if c.take(desk) {
        c.solid_box(&desk, 0.0, 0.78, WOOD, Some(Material::Wood));
    }
    let cols = (((r.width() - 1.8) / 1.5) as usize).clamp(1, 4);
    let x_start = r.x0 + (r.width() - cols as f32 * 1.5) * 0.5 + 0.1;
    let mut z = r.z0 + 1.6;
    while z + 0.6 < r.z1 - 2.4 {
        for col in 0..cols {
            let x = x_start + col as f32 * 1.5;
            let d = Rect::new(x, z, x + 1.2, z + 0.6);
            if c.take(d) {
                c.solid_box(&d, 0.0, 0.74, PALE_WOOD, Some(Material::Wood));
                let chair = Rect::new(x + 0.4, z - 0.5, x + 0.8, z - 0.1);
                if c.take(chair) {
                    c.solid_box(&chair, 0.0, 0.45, FABRICS[1], None);
                }
            }
        }
        z += 1.35;
    }
    along_wall(c, Side::Left, 1.2, c.length(Side::Left) - 1.0, 0.45, 1.0, |c, cupboard| {
        c.solid_box(&cupboard, 0.0, 1.8, WOOD, Some(Material::Wood));
    });
}

fn cloakroom(c: &mut Ctx) {
    along_wall(c, Side::Back, 0.0, c.length(Side::Back), 0.35, 1.6, |c, bench| {
        c.solid_box(&bench, 0.0, 0.45, PALE_WOOD, Some(Material::Wood));
        let hooks = c.wall_strip(Side::Back, &bench, 0.05);
        c.flat(&hooks, 1.3, 0.3, FABRICS[0]);
    });
}

fn office_desk(c: &mut Ctx) {
    if let Some((d, side)) = c.against_a_wall(1.5, 0.75, &WALLS) {
        c.solid_box(&d, 0.0, 0.76, WOOD, Some(Material::Wood));
        let chair = {
            let m = inward(side);
            let centre = d.centre() + Vec2::new(m.x * 0.8, m.y * 0.8);
            Rect::new(centre.x - 0.2, centre.y - 0.2, centre.x + 0.2, centre.y + 0.2)
        };
        if c.take(chair) {
            c.solid_box(&chair, 0.0, 0.45, FABRICS[3], None);
        }
    }
    if let Some((f, _)) = c.against_a_wall(0.5, 0.6, &WALLS) {
        c.solid_box(&f, 0.0, 1.3, STEEL, Some(Material::Metal));
    }
}

fn main_hall(c: &mut Ctx) {
    let r = c.clear;
    // The stage across the back wall, with steps up from the middle.
    let stage_w = (r.width() - 1.6).min(7.0);
    let stage = Rect::new(r.x0 + (r.width() - stage_w) * 0.5, r.z1 - 2.4, r.x0 + (r.width() + stage_w) * 0.5, r.z1);
    if c.take(stage) {
        c.solid_box(&stage, 0.0, 0.8, WOOD, Some(Material::Wood));
        let c0 = (stage.x0 + stage.x1) * 0.5;
        for (i, depth) in [0.6, 1.2].iter().enumerate() {
            let step = Rect::new(c0 - 0.8, stage.z0 - depth, c0 + 0.8, stage.z0);
            c.taken.push(step);
            c.solid_box(&step, 0.0, 0.27 * (2 - i) as f32, PALE_WOOD, Some(Material::Wood));
        }
        c.solid_box(&Rect::new(stage.x0, stage.z1 - 0.3, stage.x1, stage.z1), 0.8, 2.6, [0.45, 0.1, 0.15], None);
    }
    // Trestle tables along the side walls and stacks of chairs in the corners.
    for side in [Side::Left, Side::Right] {
        along_wall(c, side, 1.0, c.length(side) - 3.2, 0.8, 1.8, |c, t| {
            c.solid_box(&t, 0.0, 0.75, PALE_WOOD, Some(Material::Wood));
        });
    }
    for (x, z) in [(r.x0, r.z0), (r.x1 - 0.5, r.z0)] {
        let stack = Rect::new(x, z, x + 0.5, z + 0.5);
        if c.take(stack) {
            c.solid_box(&stack, 0.0, 1.3, FABRICS[1], Some(Material::Wood));
        }
    }
}

fn barn(c: &mut Ctx) {
    let r = c.clear;
    // Hay stacked against the back wall and a corner, a workbench, and a tractor if there's space.
    let stack_len = (r.width() * 0.5).min(8.0);
    let mut x = r.x0;
    while x + 0.9 <= r.x0 + stack_len {
        let bale = Rect::new(x, r.z1 - 0.9, x + 0.9, r.z1);
        if c.take(bale) {
            let tiers = 1 + (c.rng.unit() * 3.0) as usize;
            for t in 0..tiers {
                c.solid_box(&bale, 0.45 * t as f32, 0.45, [0.75, 0.62, 0.25], Some(Material::Wood));
            }
        }
        x += 0.95;
    }
    along_wall(c, Side::Right, 0.5, c.length(Side::Right) - 1.0, 0.7, 1.8, |c, bench| {
        c.solid_box(&bench, 0.0, 0.9, WOOD, Some(Material::Wood));
    });
    if r.width() > 12.0 && r.depth() > 6.0 {
        let t = Rect::new(r.x1 - 6.0, r.z0 + 1.6, r.x1 - 3.5, r.z0 + 3.0);
        if c.take(t) {
            c.solid_box(&t, 0.35, 0.9, [0.15, 0.4, 0.2], Some(Material::Metal));
            c.solid_box(&Rect::new(t.x0 + 1.4, t.z0 + 0.3, t.x0 + 2.4, t.z1 - 0.3), 1.25, 0.7, [0.15, 0.4, 0.2], None);
        }
    }
    for i in 0..3 {
        let barrel = Rect::new(r.x0 + 0.2 + i as f32 * 0.7, r.z0 + 0.2, r.x0 + 0.8 + i as f32 * 0.7, r.z0 + 0.8);
        if c.take(barrel) {
            c.solid_box(&barrel, 0.0, 0.9, [0.3, 0.35, 0.45], Some(Material::Metal));
        }
    }
}

fn nave(c: &mut Ctx) {
    let r = c.clear;
    let aisle = 1.7;
    let side_w = (r.width() - aisle) * 0.5;
    // Pews in rows either side of the aisle, from behind the font to short of the chancel step.
    let mut z = r.z0 + 3.2;
    while z + 0.55 < r.z1 - 2.6 {
        for left in [true, false] {
            let (x0, x1) = if left { (r.x0 + 0.15, r.x0 + side_w) } else { (r.x1 - side_w, r.x1 - 0.15) };
            let pew = Rect::new(x0, z, x1, z + 0.55);
            if c.take(pew) {
                c.solid_box(&pew, 0.0, 0.45, DARK_WOOD, Some(Material::Wood));
                c.solid_box(&Rect::new(x0, z + 0.45, x1, z + 0.55), 0.45, 0.5, DARK_WOOD, None);
            }
        }
        z += 1.15;
    }
    // The font by the door, the pulpit by the chancel arch.
    let font = Rect::new(r.x0 + 0.3, r.z0 + 0.6, r.x0 + 1.1, r.z0 + 1.4);
    if c.take(font) {
        c.solid_box(&font, 0.0, 1.0, [0.6, 0.6, 0.58], Some(Material::Stone));
    }
    let pulpit = Rect::new(r.x0 + 0.1, r.z1 - 1.6, r.x0 + 1.2, r.z1 - 0.5);
    if c.take(pulpit) {
        c.solid_box(&pulpit, 0.0, 1.4, DARK_WOOD, Some(Material::Wood));
    }
}

fn chancel(c: &mut Ctx) {
    let r = c.clear;
    // The altar under the end window, raised on a step, with the choir stalls along the sides.
    let step = Rect::new(r.x0, r.z1 - 1.8, r.x1, r.z1);
    if c.take(step) {
        c.solid_box(&step, 0.0, 0.25, [0.55, 0.55, 0.52], Some(Material::Stone));
        let altar = Rect::new((r.x0 + r.x1) * 0.5 - 0.9, r.z1 - 1.0, (r.x0 + r.x1) * 0.5 + 0.9, r.z1 - 0.4);
        c.solid_box(&altar, 0.25, 0.9, [0.85, 0.85, 0.82], Some(Material::Stone));
        c.flat(&altar.grown(0.05), 1.15, 0.04, [0.6, 0.1, 0.15]);
    }
    for side in [Side::Left, Side::Right] {
        along_wall(c, side, 0.2, c.length(side) - 2.2, 0.5, 1.3, |c, stall| {
            c.solid_box(&stall, 0.0, 0.5, DARK_WOOD, Some(Material::Wood));
            let back = c.wall_strip(side, &stall, 0.1);
            c.solid_box(&back, 0.5, 0.6, DARK_WOOD, None);
        });
    }
}

fn workshop(c: &mut Ctx) {
    let r = c.clear;
    // Racking along the side and back walls, crates on pallets in rows with aisles between, a forklift.
    for side in [Side::Back, Side::Left, Side::Right] {
        along_wall(c, side, 0.0, c.length(side), 1.0, 2.4, |c, rack| {
            c.solid_box(&rack, 0.0, 3.2, [0.25, 0.35, 0.6], Some(Material::Metal));
            for y in [0.4, 1.4, 2.4] {
                goods(c, &rack.grown(-0.1), y, &PRODUCE);
            }
        });
    }
    let mut z = r.z0 + 4.5;
    while z + 1.2 < r.z1 - 3.0 {
        let mut x = r.x0 + 3.5;
        while x + 1.2 < r.x1 - 3.5 {
            let pallet = Rect::new(x, z, x + 1.2, z + 1.2);
            if c.rng.unit() < 0.6 && c.free(&pallet.grown(0.8)) && c.take(pallet) {
                let tiers = 1 + (c.rng.unit() * 2.0) as usize;
                c.solid_box(&pallet, 0.0, 0.15, PALE_WOOD, Some(Material::Wood));
                for t in 0..tiers {
                    c.solid_box(&pallet.grown(-0.05), 0.15 + 0.7 * t as f32, 0.7, [0.7, 0.55, 0.35], Some(Material::Wood));
                }
            }
            x += 3.0;
        }
        z += 3.4;
    }
    let m = r.centre();
    let truck = Rect::new(m.x - 0.6, r.z0 + 4.4, m.x + 0.6, r.z0 + 6.6);
    if c.free(&truck.grown(0.6)) && c.take(truck) {
        c.solid_box(&truck, 0.2, 1.1, [0.9, 0.7, 0.1], Some(Material::Metal));
        c.solid_box(&Rect::new(truck.x0 + 0.1, truck.z0 - 0.9, truck.x1 - 0.1, truck.z0), 0.1, 0.1, [0.3, 0.3, 0.3], None);
    }
}

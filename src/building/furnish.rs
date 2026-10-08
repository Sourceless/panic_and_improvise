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
    // Along the longer way of the room.
    let (w, d) = if c.clear.width() >= c.clear.depth() { (w, d) } else { (d, w) };
    let t = Rect::new(m.x - w * 0.5, m.y - d * 0.5, m.x + w * 0.5, m.y + d * 0.5);
    // Room for chairs all round it.
    if !c.free(&t.grown(0.55)) {
        return;
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

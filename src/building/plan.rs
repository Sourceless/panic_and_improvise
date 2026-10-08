//! A building's floor plan: rooms that tile each storey, how they connect (doors), where the windows
//! are, and how the storeys join (stairs). `assemble` turns a plan into the pieces of a model.

use bevy::prelude::*;

use super::furnish;
use super::geom::{Layer, Model, Rect};
use crate::collision::Material;
use crate::meshbake::Rgb;
use crate::settlement_plan::Rng;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RoomKind {
    Hall,
    Landing,
    Corridor,
    Living,
    Kitchen,
    Dining,
    Bedroom,
    Bathroom,
    Toilet,
    Store,
    // Public buildings.
    Bar,
    Lounge,
    Shop,
    Classroom,
    Cloakroom,
    StaffRoom,
    MainHall,
    Stage,
    Nave,
    Chancel,
    Tower,
    Vestry,
    Office,
    Barn,
    Workshop,
    Mill,
    Cellar,
}

impl RoomKind {
    /// Rooms that are mostly for getting about: other rooms open off them.
    pub fn circulation(self) -> bool {
        matches!(self, RoomKind::Hall | RoomKind::Landing | RoomKind::Corridor)
    }

    /// Whether the room gets windows in its outside walls.
    pub fn has_windows(self) -> bool {
        !matches!(self, RoomKind::Toilet | RoomKind::Store | RoomKind::Cellar)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Room {
    pub rect: Rect,
    pub kind: RoomKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Front,
    Back,
    Left,
    Right,
}

/// A way in from outside.
#[derive(Clone, Copy, Debug)]
pub struct ExteriorDoor {
    pub side: Side,
    /// Where along that wall (x for the front and back, z for the sides).
    pub along: f32,
    pub width: f32,
}

/// The way a stair climbs, in the building's frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Climb {
    PosZ,
    NegZ,
    PosX,
    NegX,
}

/// A straight stair from one storey up to the next. `rect` is its footprint (the width of the stair
/// by its run).
#[derive(Clone, Copy, Debug)]
pub struct Stair {
    pub rect: Rect,
    pub climbs: Climb,
}

impl Stair {
    /// The stair's footprint with `by` more at each end, along the way it runs.
    pub fn along_its_run(&self, by: f32) -> Rect {
        match self.climbs {
            Climb::PosZ | Climb::NegZ => Rect { z0: self.rect.z0 - by, z1: self.rect.z1 + by, ..self.rect },
            Climb::PosX | Climb::NegX => Rect { x0: self.rect.x0 - by, x1: self.rect.x1 + by, ..self.rect },
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Storey {
    pub rooms: Vec<Room>,
    /// The stairs that lead up from this storey (a terrace has one for each house).
    pub stairs_up: Vec<Stair>,
    /// Pairs of rooms that get a door between them whether or not they would otherwise (indices).
    pub extra_links: Vec<(usize, usize)>,
    /// Pairs of rooms that must not be joined.
    pub no_links: Vec<(usize, usize)>,
    /// The rooms people arrive in: from outside on the ground floor, from the stair on the floors above
    /// it. (A terrace has one for each of its houses.)
    pub roots: Vec<usize>,
    pub doors_outside: Vec<ExteriorDoor>,
    /// Floor-to-floor height.
    pub height: f32,
    /// A room that is open to the one below (a gallery, a double-height hall): no floor over it.
    pub voids: Vec<Rect>,
}

/// Colours and sizes that give a building its look.
#[derive(Clone, Copy, Debug)]
pub struct Style {
    pub outer: Rgb,
    pub inner: Rgb,
    pub floor: Rgb,
    pub ceiling: Rgb,
    pub trim: Rgb,
    pub roof: Rgb,
    pub window_width: f32,
    pub window_sill: f32,
    pub window_head: f32,
    /// How far apart windows are, at least, centre to centre.
    pub window_spacing: f32,
    pub wall_thickness: f32,
    pub door_width: f32,
    /// Rooms above the ground floor get windows too.
    /// Whether the outside walls have windows (a barn's don't).
    pub windows: bool,
}

impl Default for Style {
    fn default() -> Self {
        Style {
            outer: [0.9, 0.86, 0.74],
            inner: [0.88, 0.85, 0.78],
            floor: [0.5, 0.38, 0.27],
            ceiling: [0.93, 0.92, 0.88],
            trim: [0.45, 0.34, 0.24],
            roof: [0.55, 0.25, 0.18],
            window_width: 1.2,
            window_sill: 0.9,
            window_head: 2.05,
            window_spacing: 2.6,
            wall_thickness: 0.3,
            door_width: 1.1,
            windows: true,
        }
    }
}

pub const INTERIOR_WALL: f32 = 0.12;
pub const DOOR_HEIGHT: f32 = 2.05;
pub const INTERIOR_DOOR_WIDTH: f32 = 1.0;
const SLAB: f32 = 0.2;
const EPS: f32 = 1e-3;

/// A whole building's plan.
#[derive(Clone, Debug)]
pub struct Layout {
    /// The rectangle the rooms tile, from wall centre to wall centre.
    pub inside: Rect,
    pub storeys: Vec<Storey>,
    pub style: Style,
}

impl Layout {
    pub fn height(&self) -> f32 {
        self.storeys.iter().map(|s| s.height).sum()
    }

    /// The floor level of storey `s`.
    pub fn floor_of(&self, s: usize) -> f32 {
        self.storeys[..s].iter().map(|s| s.height).sum()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// A wall running along x, at some z.
    X,
    /// A wall running along z, at some x.
    Z,
}

/// A straight run of wall between rooms (or between a room and the outside).
#[derive(Clone, Copy, Debug)]
pub struct Edge {
    pub axis: Axis,
    pub at: f32,
    pub lo: f32,
    pub hi: f32,
    pub a: usize,
    pub b: Option<usize>,
}

impl Edge {
    pub fn length(&self) -> f32 {
        self.hi - self.lo
    }
}

/// Every wall run of a storey: those shared by two rooms once, and the parts of the outside wall of
/// each room.
pub fn edges(rooms: &[Room]) -> Vec<Edge> {
    let mut out = Vec::new();
    for (i, room) in rooms.iter().enumerate() {
        let r = room.rect;
        // (axis, fixed coordinate, range, and the neighbours' sides that would meet this one)
        let sides = [
            (Axis::X, r.z0, r.x0, r.x1),
            (Axis::X, r.z1, r.x0, r.x1),
            (Axis::Z, r.x0, r.z0, r.z1),
            (Axis::Z, r.x1, r.z0, r.z1),
        ];
        for (side_index, &(axis, at, lo, hi)) in sides.iter().enumerate() {
            let low_side = side_index % 2 == 0;
            let mut covered: Vec<(f32, f32, usize)> = Vec::new();
            for (j, other) in rooms.iter().enumerate() {
                if j == i {
                    continue;
                }
                let o = other.rect;
                // The other room's side that faces this one.
                let (o_at, o_lo, o_hi) = match (axis, low_side) {
                    (Axis::X, true) => (o.z1, o.x0, o.x1),
                    (Axis::X, false) => (o.z0, o.x0, o.x1),
                    (Axis::Z, true) => (o.x1, o.z0, o.z1),
                    (Axis::Z, false) => (o.x0, o.z0, o.z1),
                };
                if (o_at - at).abs() > EPS {
                    continue;
                }
                let (s, e) = (lo.max(o_lo), hi.min(o_hi));
                if e - s > EPS {
                    covered.push((s, e, j));
                }
            }
            covered.sort_by(|x, y| x.0.total_cmp(&y.0));
            let mut cursor = lo;
            for &(s, e, j) in &covered {
                if s - cursor > EPS {
                    out.push(Edge { axis, at, lo: cursor, hi: s, a: i, b: None });
                }
                if i < j {
                    out.push(Edge { axis, at, lo: s, hi: e, a: i, b: Some(j) });
                }
                cursor = cursor.max(e);
            }
            if hi - cursor > EPS {
                out.push(Edge { axis, at, lo: cursor, hi, a: i, b: None });
            }
        }
    }
    out
}

/// A door in an interior or exterior wall: the edge and where along it.
#[derive(Clone, Copy, Debug)]
pub struct Door {
    pub edge: usize,
    pub centre: f32,
    pub width: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Window {
    pub edge: usize,
    pub centre: f32,
    pub width: f32,
}

/// Where the doors go: the way in, then enough doors that every room can be reached from the first.
pub fn place_doors(storey: &Storey, edges: &[Edge], keep_clear: &[Rect], rng: &mut Rng) -> Vec<Door> {
    let mut doors: Vec<Door> = Vec::new();
    // Ways in from outside.
    for outside in &storey.doors_outside {
        let found = edges.iter().position(|e| {
            e.b.is_none()
                && match outside.side {
                    Side::Front => e.axis == Axis::X && (e.at - front_line(storey)).abs() < EPS,
                    Side::Back => e.axis == Axis::X && (e.at - back_line(storey)).abs() < EPS,
                    Side::Left => e.axis == Axis::Z && (e.at - left_line(storey)).abs() < EPS,
                    Side::Right => e.axis == Axis::Z && (e.at - right_line(storey)).abs() < EPS,
                }
                && outside.along - outside.width * 0.5 >= e.lo - EPS
                && outside.along + outside.width * 0.5 <= e.hi + EPS
        });
        if let Some(edge) = found {
            doors.push(Door { edge, centre: outside.along, width: outside.width });
        }
    }
    // Rooms joined to the first, one at a time: the nearest unjoined room to a joined one, and a
    // passage (hall, landing, corridor) preferred as the way through.
    let n = storey.rooms.len();
    let mut joined = vec![false; n];
    if n == 0 {
        return doors;
    }
    for &root in &storey.roots {
        joined[root.min(n - 1)] = true;
    }
    let linked = |a: usize, b: usize, list: &[(usize, usize)]| list.iter().any(|&(x, y)| (x, y) == (a, b) || (x, y) == (b, a));
    let can_door = |e: &Edge| e.length() >= INTERIOR_DOOR_WIDTH + 0.5;
    // The links the plan insists on come first.
    let mut wanted: Vec<(usize, usize)> = storey.extra_links.clone();
    loop {
        let mut best: Option<(f32, usize)> = None;
        for (k, e) in edges.iter().enumerate() {
            let Some(b) = e.b else { continue };
            if !can_door(e) || linked(e.a, b, &storey.no_links) {
                continue;
            }
            let (a, b) = (e.a, b);
            if joined[a] == joined[b] {
                continue;
            }
            let (from, to) = if joined[a] { (a, b) } else { (b, a) };
            let (kf, kt) = (storey.rooms[from].kind, storey.rooms[to].kind);
            let mut score = e.length() * 0.2;
            if kf.circulation() {
                score += 4.0;
            }
            if kt.circulation() {
                score += 1.5;
            }
            if storey.roots.contains(&from) {
                score += 1.0;
            }
            if best.is_none_or(|(s, _)| score > s) {
                best = Some((score, k));
            }
        }
        let Some((_, k)) = best else { break };
        let e = edges[k];
        let (a, b) = (e.a, e.b.unwrap());
        joined[a] = true;
        joined[b] = true;
        wanted.push((a, b));
        if joined.iter().all(|&j| j) {
            break;
        }
    }
    for (a, b) in wanted {
        let mut candidates: Vec<usize> = edges.iter().enumerate().filter(|(_, e)| can_door(e) && ((e.a == a && e.b == Some(b)) || (e.a == b && e.b == Some(a)))).map(|(k, _)| k).collect();
        candidates.sort_by(|&x, &y| edges[y].length().total_cmp(&edges[x].length()));
        'edges: for k in candidates {
            let e = edges[k];
            let width = INTERIOR_DOOR_WIDTH;
            let (lo, hi) = (e.lo + width * 0.5 + 0.2, e.hi - width * 0.5 - 0.2);
            if hi < lo {
                continue;
            }
            // A few places along it, the favourite ones first.
            let favourite = rng.unit();
            let mut spots: Vec<f32> = [favourite, 0.2, 0.8, 0.5, 0.35, 0.65, 0.05, 0.95].iter().map(|f| lo + (hi - lo) * f).collect();
            spots.dedup();
            for c in spots {
                let door_rect = match e.axis {
                    Axis::X => Rect::new(c - width * 0.5, e.at - 0.45, c + width * 0.5, e.at + 0.45),
                    Axis::Z => Rect::new(e.at - 0.45, c - width * 0.5, e.at + 0.45, c + width * 0.5),
                };
                let clashes = keep_clear.iter().any(|r| r.overlaps(&door_rect))
                    || doors.iter().any(|d| {
                        let de = edges[d.edge];
                        de.axis == e.axis && (de.at - e.at).abs() < EPS && (d.centre - c).abs() < (d.width + width) * 0.5 + 0.3
                    });
                if !clashes {
                    doors.push(Door { edge: k, centre: c, width });
                    break 'edges;
                }
            }
        }
    }
    doors
}

fn bounds(storey: &Storey) -> Rect {
    let mut r = storey.rooms[0].rect;
    for room in &storey.rooms {
        r.x0 = r.x0.min(room.rect.x0);
        r.z0 = r.z0.min(room.rect.z0);
        r.x1 = r.x1.max(room.rect.x1);
        r.z1 = r.z1.max(room.rect.z1);
    }
    r
}

fn front_line(s: &Storey) -> f32 {
    bounds(s).z0
}
fn back_line(s: &Storey) -> f32 {
    bounds(s).z1
}
fn left_line(s: &Storey) -> f32 {
    bounds(s).x0
}
fn right_line(s: &Storey) -> f32 {
    bounds(s).x1
}

/// Windows along the outside walls, avoiding doors and the ends of walls.
pub fn place_windows(storey: &Storey, edges: &[Edge], doors: &[Door], style: &Style, rng: &mut Rng) -> Vec<Window> {
    let mut windows = Vec::new();
    for (k, e) in edges.iter().enumerate() {
        if !style.windows || e.b.is_some() || !storey.rooms[e.a].kind.has_windows() {
            continue;
        }
        let margin = style.window_width * 0.5 + 0.45;
        let (lo, hi) = (e.lo + margin, e.hi - margin);
        if hi < lo {
            continue;
        }
        let span = hi - lo;
        let count = (span / style.window_spacing).floor() as usize + 1;
        let count = if span < 0.01 { 1 } else { count.max(1) };
        for i in 0..count {
            let c = if count == 1 { (lo + hi) * 0.5 } else { lo + span * i as f32 / (count - 1) as f32 };
            let c = c + rng.range(-0.05, 0.05);
            let clash = doors.iter().any(|d| d.edge == k && (d.centre - c).abs() < (d.width + style.window_width) * 0.5 + 0.35);
            if !clash {
                windows.push(Window { edge: k, centre: c, width: style.window_width });
            }
        }
    }
    windows
}

#[derive(Clone, Copy, Debug)]
struct Opening {
    lo: f32,
    hi: f32,
    sill: f32,
    head: f32,
}

/// A wall run as boxes with gaps for its openings.
#[allow(clippy::too_many_arguments)]
fn wall(model: &mut Model, axis: Axis, at: f32, lo: f32, hi: f32, thickness: f32, bottom: f32, floor: f32, y1: f32, openings: &mut [Opening], colour: Rgb, layer: Layer, solid: Material) {
    openings.sort_by(|a, b| a.lo.total_cmp(&b.lo));
    let mut piece = |a: f32, b: f32, from: f32, to: f32| {
        if b - a < 0.01 || to - from < 0.01 {
            return;
        }
        let (lo3, hi3) = match axis {
            Axis::X => (Vec3::new(a, from, at - thickness * 0.5), Vec3::new(b, to, at + thickness * 0.5)),
            Axis::Z => (Vec3::new(at - thickness * 0.5, from, a), Vec3::new(at + thickness * 0.5, to, b)),
        };
        model.span(lo3, hi3, colour, layer, Some(solid));
    };
    // The run reaches half its thickness past each end, so that walls meet in a corner.
    let mut cursor = lo - thickness * 0.5;
    for o in openings.iter() {
        piece(cursor, o.lo, bottom, y1);
        piece(o.lo, o.hi, bottom, floor + o.sill.max(0.0));
        piece(o.lo, o.hi, floor + o.head, y1);
        cursor = o.hi;
    }
    piece(cursor, hi + thickness * 0.5, bottom, y1);
}

/// Everything a plan builds: walls with their doors and windows, floors, ceilings, stairs and
/// furniture. `skirt` is how far below the floor the ground-floor walls reach (so that they meet a
/// sloping ground).
pub fn assemble(layout: &Layout, skirt: f32, rng: &mut Rng) -> Model {
    let mut model = Model::default();
    let style = &layout.style;
    let outside = layout.inside.grown(style.wall_thickness * 0.5);
    let top = layout.height();

    for (s, storey) in layout.storeys.iter().enumerate() {
        let y0 = layout.floor_of(s);
        let y1 = y0 + storey.height;
        let edges = edges(&storey.rooms);
        // The stair up from here, and the one that arrives from below: nothing is put in their way.
        let mut keep_clear: Vec<Rect> = storey.stairs_up.iter().map(|st| st.rect.grown(0.55)).collect();
        if s > 0 {
            keep_clear.extend(layout.storeys[s - 1].stairs_up.iter().map(|st| st.rect.grown(0.55)));
        }
        // Doors may open beside a stair but not onto either end of it.
        let stair_footprints: Vec<Rect> = storey
            .stairs_up
            .iter()
            .chain(if s > 0 { layout.storeys[s - 1].stairs_up.iter() } else { [].iter() })
            .map(|st| st.along_its_run(1.0))
            .collect();
        let doors = place_doors(storey, &edges, &stair_footprints, rng);
        let windows = place_windows(storey, &edges, &doors, style, rng);

        for (k, e) in edges.iter().enumerate() {
            let exterior = e.b.is_none();
            let mut openings: Vec<Opening> = Vec::new();
            for d in doors.iter().filter(|d| d.edge == k) {
                openings.push(Opening { lo: d.centre - d.width * 0.5, hi: d.centre + d.width * 0.5, sill: 0.0, head: DOOR_HEIGHT.min(storey.height - 0.4) });
            }
            for w in windows.iter().filter(|w| w.edge == k) {
                openings.push(Opening { lo: w.centre - w.width * 0.5, hi: w.centre + w.width * 0.5, sill: style.window_sill, head: style.window_head.min(storey.height - 0.4) });
            }
            let bottom = if s == 0 { -skirt } else { y0 - SLAB };
            if exterior {
                wall(&mut model, e.axis, e.at, e.lo, e.hi, style.wall_thickness, bottom, y0, y1, &mut openings, style.outer, Layer::Shell, Material::Stone);
            } else {
                wall(&mut model, e.axis, e.at, e.lo, e.hi, INTERIOR_WALL, y0, y0, y1 - SLAB, &mut openings, style.inner, Layer::Interior, Material::Wood);
            }
        }

        // The floor: the whole footprint, less whatever is open to below.
        let mut holes: Vec<Rect> = storey.voids.clone();
        if s > 0 {
            holes.extend(layout.storeys[s - 1].stairs_up.iter().map(|st| st.rect));
        }
        if s == 0 {
            model.slab(&outside, 0.0, 0.5 + skirt, style.floor, Layer::Shell, Some(Material::Stone));
        } else {
            model.slab_with_holes(&outside, &holes, y0, SLAB, style.floor, Layer::Shell, Some(Material::Wood));
            // A rail round every hole in the floor, open at the top of a stair.
            for void in &storey.voids {
                balustrade(&mut model, void, None, y0, style, &layout.inside);
            }
            for st in &layout.storeys[s - 1].stairs_up {
                balustrade(&mut model, &st.rect, Some(st.climbs), y0, style, &layout.inside);
            }
        }

        // The stair up.
        for st in &storey.stairs_up {
            stair(&mut model, st, y0, storey.height, s == 0, style);
        }

        // A light in the ceiling of each room that is big enough to want one.
        for room in &storey.rooms {
            if room.rect.area() > 3.5 {
                let c = room.rect.centre();
                model.lights.push((Vec3::new(c.x, y1 - 0.4, c.y), room.rect.width().max(room.rect.depth()) * 0.8 + 1.5));
            }
        }

        // Fittings.
        for (i, room) in storey.rooms.iter().enumerate() {
            let door_zones: Vec<Rect> = doors
                .iter()
                .filter(|d| edges[d.edge].a == i || edges[d.edge].b == Some(i))
                .map(|d| {
                    let e = edges[d.edge];
                    match e.axis {
                        Axis::X => Rect::new(d.centre - d.width * 0.5 - 0.25, e.at - 1.1, d.centre + d.width * 0.5 + 0.25, e.at + 1.1),
                        Axis::Z => Rect::new(e.at - 1.1, d.centre - d.width * 0.5 - 0.25, e.at + 1.1, d.centre + d.width * 0.5 + 0.25),
                    }
                })
                .collect();
            let mut avoid = door_zones;
            avoid.extend(keep_clear.iter().copied());
            avoid.extend(holes.iter().map(|h| h.grown(0.3)));
            furnish::furnish(&mut model, room, &avoid, y0, storey.height, style, rng);
        }
    }

    // The top ceiling.
    model.slab(&outside, top, 0.15, style.ceiling, Layer::Shell, Some(Material::Wood));
    model
}

fn balustrade(model: &mut Model, hole: &Rect, open_end: Option<Climb>, y: f32, style: &Style, inside: &Rect) {
    let colour = style.trim;
    let t = 0.06;
    let h = 1.0;
    // Which edge of the hole the stair arrives at (left open).
    let by_the_wall = |side: Side| match side {
        Side::Front => hole.z0 - inside.z0 < 0.45,
        Side::Back => inside.z1 - hole.z1 < 0.45,
        Side::Left => hole.x0 - inside.x0 < 0.45,
        Side::Right => inside.x1 - hole.x1 < 0.45,
    };
    let skip = |side: Side| {
        by_the_wall(side)
            || matches!((open_end, side), (Some(Climb::PosZ), Side::Back) | (Some(Climb::NegZ), Side::Front) | (Some(Climb::PosX), Side::Right) | (Some(Climb::NegX), Side::Left))
    };
    let rails = [
        (Side::Front, Vec3::new(hole.x0, y, hole.z0 - t), Vec3::new(hole.x1, y + h, hole.z0)),
        (Side::Back, Vec3::new(hole.x0, y, hole.z1), Vec3::new(hole.x1, y + h, hole.z1 + t)),
        (Side::Left, Vec3::new(hole.x0 - t, y, hole.z0), Vec3::new(hole.x0, y + h, hole.z1)),
        (Side::Right, Vec3::new(hole.x1, y, hole.z0), Vec3::new(hole.x1 + t, y + h, hole.z1)),
    ];
    for (side, lo, hi) in rails {
        if !skip(side) {
            model.span(lo, hi, colour, Layer::Interior, Some(Material::Wood));
        }
    }
}

/// The steps of a stair: one fewer than the risers, the last riser being from the top step onto the
/// floor above.
fn stair(model: &mut Model, st: &Stair, y0: f32, storey_height: f32, ground: bool, style: &Style) {
    let risers = (storey_height / 0.2).round().max(2.0);
    let rise = storey_height / risers;
    let steps = risers as usize - 1;
    let r = st.rect;
    let run = match st.climbs {
        Climb::PosZ | Climb::NegZ => r.depth(),
        _ => r.width(),
    };
    let tread = run / steps as f32;
    let bottom = if ground { -0.4 } else { y0 };
    for i in 0..steps {
        let top = y0 + rise * (i as f32 + 1.0);
        let (a, b) = (i as f32 * tread, (i as f32 + 1.0) * tread);
        let (lo, hi) = match st.climbs {
            Climb::PosZ => (Vec3::new(r.x0, bottom, r.z0 + a), Vec3::new(r.x1, top, r.z0 + b)),
            Climb::NegZ => (Vec3::new(r.x0, bottom, r.z1 - b), Vec3::new(r.x1, top, r.z1 - a)),
            Climb::PosX => (Vec3::new(r.x0 + a, bottom, r.z0), Vec3::new(r.x0 + b, top, r.z1)),
            Climb::NegX => (Vec3::new(r.x1 - b, bottom, r.z0), Vec3::new(r.x1 - a, top, r.z1)),
        };
        model.span(lo, hi, style.trim, Layer::Interior, Some(Material::Wood));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room(x0: f32, z0: f32, x1: f32, z1: f32, kind: RoomKind) -> Room {
        Room { rect: Rect::new(x0, z0, x1, z1), kind }
    }

    #[test]
    fn two_rooms_side_by_side_share_one_wall_and_have_the_rest_outside() {
        let rooms = [room(0.0, 0.0, 4.0, 5.0, RoomKind::Living), room(4.0, 0.0, 8.0, 5.0, RoomKind::Kitchen)];
        let e = edges(&rooms);
        let shared: Vec<&Edge> = e.iter().filter(|e| e.b.is_some()).collect();
        assert_eq!(shared.len(), 1);
        assert_eq!((shared[0].axis, shared[0].at, shared[0].lo, shared[0].hi), (Axis::Z, 4.0, 0.0, 5.0));
        let outer: f32 = e.iter().filter(|e| e.b.is_none()).map(Edge::length).sum();
        assert!((outer - 26.0).abs() < 1e-3, "the perimeter of 8 x 5: {outer}");
    }

    #[test]
    fn a_wall_between_a_big_room_and_two_small_ones_is_in_pieces() {
        let rooms = [
            room(0.0, 0.0, 6.0, 3.0, RoomKind::Living),
            room(0.0, 3.0, 3.0, 6.0, RoomKind::Bedroom),
            room(3.0, 3.0, 6.0, 6.0, RoomKind::Bathroom),
        ];
        let e = edges(&rooms);
        let between: f32 = e.iter().filter(|e| e.b.is_some() && e.axis == Axis::X).map(Edge::length).sum();
        assert!((between - 6.0).abs() < 1e-3, "{between}");
        assert_eq!(e.iter().filter(|e| e.b.is_some() && e.axis == Axis::X).count(), 2);
    }

    #[test]
    fn every_room_is_joined_to_the_first_by_doors() {
        let storey = Storey {
            rooms: vec![
                room(0.0, 0.0, 2.2, 7.0, RoomKind::Hall),
                room(2.2, 0.0, 8.0, 3.5, RoomKind::Living),
                room(2.2, 3.5, 8.0, 7.0, RoomKind::Kitchen),
            ],
            height: 2.8,
            roots: vec![0],
            doors_outside: vec![ExteriorDoor { side: Side::Front, along: 1.1, width: 1.0 }],
            ..Default::default()
        };
        let e = edges(&storey.rooms);
        let mut rng = Rng::from_position(Vec2::new(1.0, 2.0));
        let doors = place_doors(&storey, &e, &[], &mut rng);
        assert!(doors.iter().any(|d| e[d.edge].b.is_none()), "a way in");
        let interior: Vec<&Door> = doors.iter().filter(|d| e[d.edge].b.is_some()).collect();
        assert_eq!(interior.len(), 2, "two more rooms, two doors");
        // Through the hall, which is the passage.
        assert!(interior.iter().all(|d| e[d.edge].a == 0 || e[d.edge].b == Some(0)), "both off the hall");
    }
}

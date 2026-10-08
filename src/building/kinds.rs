//! The floor plan of each kind of building: what rooms it has, how big they are, where the stair
//! and the front door go. The sizes vary with the building, so that a big house has more rooms than
//! a small one and no two are quite alike.

use super::geom::Rect;
use super::plan::{Climb, ExteriorDoor, Layout, Room, RoomKind, Side, Stair, Storey, Style};
use crate::settlement_plan::{Building, BuildingKind, Rng};

/// Floor-to-floor height of a house.
pub const HOUSE_STOREY: f32 = 2.75;
/// The width of a stair, and of the walkway beside it in a hall.
const STAIR_WIDTH: f32 = 1.1;
/// The height of a riser, near enough: the run of a stair is worked out from it.
const RISER: f32 = 0.2;
const TREAD: f32 = 0.23;
/// Clear floor wanted where a stair begins, and where it comes out.
const FOOT_ROOM: f32 = 0.6;
const HEAD_ROOM: f32 = 1.0;

fn room(rect: Rect, kind: RoomKind) -> Room {
    Room { rect, kind }
}

/// How long a stair between floors `height` apart runs.
pub fn stair_run(height: f32) -> f32 {
    let risers = (height / RISER).round().max(2.0);
    (risers - 1.0) * TREAD
}

/// The plan of a building.
pub fn layout(b: &Building, style: Style, rng: &mut Rng) -> Layout {
    let inside = Rect::new(-b.width * 0.5, -b.depth * 0.5, b.width * 0.5, b.depth * 0.5).grown(-style.wall_thickness * 0.5);
    match b.kind {
        BuildingKind::Cottage => cottage(inside, b.wall_height, style, rng),
        BuildingKind::House | BuildingKind::Farmhouse => house(inside, style, rng),
        BuildingKind::Terrace => terrace(inside, style, rng),
        _ => single_room(inside, style, RoomKind::Store, 3.2),
    }
}

/// An empty box of a building, for kinds that don't have a plan of their own yet.
fn single_room(inside: Rect, style: Style, kind: RoomKind, height: f32) -> Layout {
    let along = (inside.x0 + inside.x1) * 0.5;
    let storey = Storey {
        rooms: vec![room(inside, kind)],
        roots: vec![0],
        doors_outside: vec![ExteriorDoor { side: Side::Front, along, width: style.door_width }],
        height,
        ..Default::default()
    };
    Layout { inside, storeys: vec![storey], style }
}

/// Small houses: one storey, or two if the walls are tall.
fn cottage(inside: Rect, wall_height: f32, style: Style, rng: &mut Rng) -> Layout {
    if wall_height >= 4.6 {
        if let Some(layout) = open_stair_house(inside, style, rng) {
            return layout;
        }
    }
    single_storey(inside, style, rng)
}

/// A house of one storey, with as many rooms as will fit.
fn single_storey(inside: Rect, style: Style, rng: &mut Rng) -> Layout {
    let h = 2.7;
    let (w, d) = (inside.width(), inside.depth());
    let area = w * d;
    let mut rooms = Vec::new();
    let front_door_x;
    if area < 24.0 || w.min(d) < 3.4 {
        rooms.push(room(inside, RoomKind::Living));
        front_door_x = inside.centre().x;
    } else if area < 36.0 {
        // A living room across the front and a bedroom behind.
        let z = inside.z0 + d * rng.range(0.5, 0.6);
        rooms.push(room(Rect::new(inside.x0, inside.z0, inside.x1, z), RoomKind::Living));
        rooms.push(room(Rect::new(inside.x0, z, inside.x1, inside.z1), RoomKind::Bedroom));
        front_door_x = inside.x0 + w * rng.range(0.3, 0.7);
    } else {
        let z = inside.z0 + d * 0.5;
        let split = inside.x0 + w * rng.range(0.55, 0.65);
        rooms.push(room(Rect::new(inside.x0, inside.z0, split, z), RoomKind::Living));
        rooms.push(room(Rect::new(split, inside.z0, inside.x1, z), RoomKind::Kitchen));
        let bath = inside.x1 - 2.2;
        rooms.push(room(Rect::new(inside.x0, z, bath, inside.z1), RoomKind::Bedroom));
        rooms.push(room(Rect::new(bath, z, inside.x1, inside.z1), RoomKind::Bathroom));
        front_door_x = inside.x0 + (split - inside.x0) * 0.5;
    }
    let storey = Storey {
        rooms,
        roots: vec![0],
        doors_outside: vec![ExteriorDoor { side: Side::Front, along: front_door_x, width: style.door_width }],
        height: h,
        ..Default::default()
    };
    Layout { inside, storeys: vec![storey], style }
}

/// A two-storey house whose stair rises in a room rather than a hall: the small houses and each
/// house of a terrace. `None` if the house is too shallow to have a stair with room at both ends.
fn open_stair_house(inside: Rect, style: Style, rng: &mut Rng) -> Option<Layout> {
    let h = HOUSE_STOREY;
    let run = stair_run(h);
    let (w, d) = (inside.width(), inside.depth());
    let stair_left = rng.unit() < 0.5;
    let sx = if stair_left { (inside.x0 + 0.15, inside.x0 + 0.15 + STAIR_WIDTH) } else { (inside.x1 - 0.15 - STAIR_WIDTH, inside.x1 - 0.15) };
    // The door is at the other side from the stair.
    let door_x = if stair_left { inside.x1 - (w - STAIR_WIDTH) * 0.5 } else { inside.x0 + (w - STAIR_WIDTH) * 0.5 };
    let stair_room_depth = FOOT_ROOM + run + HEAD_ROOM + 0.2;
    let (ground, upper, stair);
    if d >= stair_room_depth + 2.2 {
        // Two rooms deep, the stair in the back one: it starts a little way in from the wall between
        // them and comes out toward the back wall.
        let back_depth = stair_room_depth.max(d * 0.5);
        let z = inside.z1 - back_depth;
        let front = Rect::new(inside.x0, inside.z0, inside.x1, z);
        let back = Rect::new(inside.x0, z, inside.x1, inside.z1);
        let z0 = z + FOOT_ROOM + 0.06;
        stair = Stair { rect: Rect::new(sx.0, z0, sx.1, z0 + run), climbs: Climb::PosZ };
        ground = vec![room(front, RoomKind::Living), room(back, RoomKind::Kitchen)];
        let second = if w >= 5.0 { RoomKind::Bedroom } else { RoomKind::Bathroom };
        upper = vec![room(front, RoomKind::Bedroom), room(back, second)];
    } else if d >= FOOT_ROOM + run + HEAD_ROOM + 0.4 {
        // One room a storey, the stair along a side wall.
        let z0 = inside.z0 + 1.0;
        stair = Stair { rect: Rect::new(sx.0, z0, sx.1, z0 + run), climbs: Climb::PosZ };
        ground = vec![room(inside, RoomKind::Living)];
        upper = vec![room(inside, RoomKind::Bedroom)];
    } else {
        return None;
    }
    // The first floor is entered from the stair, in the room that has it.
    let stair_room = ground.len() - 1;
    let lower = Storey {
        rooms: ground,
        stairs_up: vec![stair],
        roots: vec![0],
        doors_outside: vec![ExteriorDoor { side: Side::Front, along: door_x, width: style.door_width }],
        height: h,
        ..Default::default()
    };
    let top = Storey { rooms: upper, roots: vec![stair_room], height: h, ..Default::default() };
    Some(Layout { inside, storeys: vec![lower, top], style })
}

/// A house with a hall: a passage down one side with the stair in it, and the rooms off it.
fn house(inside: Rect, style: Style, rng: &mut Rng) -> Layout {
    let h = HOUSE_STOREY;
    let run = stair_run(h);
    let (w, d) = (inside.width(), inside.depth());
    if w < 6.6 || d < 1.4 + run + HEAD_ROOM + 0.2 {
        return open_stair_house(inside, style, rng).unwrap_or_else(|| single_storey(inside, style, rng));
    }
    let hall_width = STAIR_WIDTH + 1.2;
    let hall_left = rng.unit() < 0.5;
    let (hall, rest) = if hall_left {
        (Rect::new(inside.x0, inside.z0, inside.x0 + hall_width, inside.z1), Rect::new(inside.x0 + hall_width, inside.z0, inside.x1, inside.z1))
    } else {
        (Rect::new(inside.x1 - hall_width, inside.z0, inside.x1, inside.z1), Rect::new(inside.x0, inside.z0, inside.x1 - hall_width, inside.z1))
    };
    // The stair is against the outside wall, the walkway beside it against the rooms.
    let stair_x = if hall_left { (hall.x0 + 0.15, hall.x0 + 0.15 + STAIR_WIDTH) } else { (hall.x1 - 0.15 - STAIR_WIDTH, hall.x1 - 0.15) };
    let z0 = inside.z0 + 1.4;
    let stair = Stair { rect: Rect::new(stair_x.0, z0, stair_x.1, z0 + run), climbs: Climb::PosZ };
    let door_x = if hall_left { hall.x1 - 0.15 - (hall_width - STAIR_WIDTH) * 0.5 } else { hall.x0 + 0.15 + (hall_width - STAIR_WIDTH) * 0.5 };

    let front_depth = rest.depth() * rng.range(0.46, 0.56);
    let zf = rest.z0 + front_depth;
    let living = Rect::new(rest.x0, rest.z0, rest.x1, zf);
    let back = Rect::new(rest.x0, zf, rest.x1, rest.z1);
    let mut ground = vec![room(hall, RoomKind::Hall), room(living, RoomKind::Living)];
    let mut links = Vec::new();
    if back.width() >= 6.6 {
        let xs = back.x0 + back.width() * rng.range(0.5, 0.62);
        ground.push(room(Rect::new(back.x0, back.z0, xs, back.z1), RoomKind::Kitchen));
        ground.push(room(Rect::new(xs, back.z0, back.x1, back.z1), RoomKind::Dining));
        links.push((2, 3));
    } else {
        ground.push(room(back, RoomKind::Kitchen));
    }
    // Often the living room opens through to the kitchen as well.
    if rng.unit() < 0.5 {
        links.push((1, 2));
    }
    let lower = Storey {
        rooms: ground,
        stairs_up: vec![stair],
        extra_links: links,
        roots: vec![0],
        doors_outside: vec![ExteriorDoor { side: Side::Front, along: door_x, width: style.door_width }],
        height: h,
        ..Default::default()
    };

    let mut upper = vec![room(hall, RoomKind::Landing), room(living, RoomKind::Bedroom)];
    if back.width() >= 6.0 {
        let bath_width = rng.range(2.3, 2.8);
        let xs = if hall_left { back.x1 - bath_width } else { back.x0 + bath_width };
        let (bed, bath) = if hall_left {
            (Rect::new(back.x0, back.z0, xs, back.z1), Rect::new(xs, back.z0, back.x1, back.z1))
        } else {
            (Rect::new(xs, back.z0, back.x1, back.z1), Rect::new(back.x0, back.z0, xs, back.z1))
        };
        upper.push(room(bed, RoomKind::Bedroom));
        upper.push(room(bath, RoomKind::Bathroom));
    } else {
        upper.push(room(back, RoomKind::Bedroom));
    }
    let top = Storey { rooms: upper, roots: vec![0], height: h, ..Default::default() };
    Layout { inside, storeys: vec![lower, top], style }
}

/// A row of houses, each its own width, side by side.
fn terrace(inside: Rect, style: Style, rng: &mut Rng) -> Layout {
    let units = ((inside.width() / 5.4).round() as usize).max(2);
    // Each house a little different in width.
    let mut cuts = vec![0.0f32];
    let mut weights: Vec<f32> = (0..units).map(|_| rng.range(0.85, 1.15)).collect();
    let total: f32 = weights.iter().sum();
    for w in &mut weights {
        *w *= inside.width() / total;
    }
    for w in &weights {
        cuts.push(cuts.last().unwrap() + w);
    }
    let mut lower = Storey { height: HOUSE_STOREY, ..Default::default() };
    let mut top = Storey { height: HOUSE_STOREY, ..Default::default() };
    for u in 0..units {
        let r = Rect::new(inside.x0 + cuts[u], inside.z0, inside.x0 + cuts[u + 1], inside.z1);
        let single = open_stair_house(r, style, rng).expect("a terrace is deep enough for a stair");
        let (mut a, mut b) = (single.storeys[0].clone(), single.storeys[1].clone());
        lower.roots.push(lower.rooms.len());
        top.roots.push(top.rooms.len() + b.roots[0]);
        lower.rooms.append(&mut a.rooms);
        top.rooms.append(&mut b.rooms);
        lower.doors_outside.append(&mut a.doors_outside);
        lower.stairs_up.append(&mut a.stairs_up);
    }
    lower.no_links = cross_house_links(&lower.rooms, &cuts, inside.x0);
    top.no_links = cross_house_links(&top.rooms, &cuts, inside.x0);
    Layout { inside, storeys: vec![lower, top], style }
}

/// Pairs of rooms that belong to different houses of a row, which must not be joined.
fn cross_house_links(rooms: &[Room], cuts: &[f32], x0: f32) -> Vec<(usize, usize)> {
    let house_of = |r: &Room| {
        let x = r.rect.centre().x - x0;
        cuts.windows(2).position(|c| x >= c[0] && x <= c[1]).unwrap_or(0)
    };
    let mut out = Vec::new();
    for (i, a) in rooms.iter().enumerate() {
        for (j, b) in rooms.iter().enumerate().skip(i + 1) {
            if house_of(a) != house_of(b) {
                out.push((i, j));
            }
        }
    }
    out
}

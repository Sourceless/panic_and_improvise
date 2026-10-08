//! The plans of the buildings people go to: the pub, the shop, the school, the village hall, the
//! church, the barn and the petrol station's kiosk.

use super::geom::Rect;
use super::kinds::{stair_run, HOUSE_STOREY};
use super::plan::{Axis, Climb, ExteriorDoor, Layout, Room, RoomKind, Roof, Side, Stair, Storey, Style, Volume, DOOR_HEIGHT};
use crate::settlement_plan::{Building, BuildingKind, Rng};

fn room(rect: Rect, kind: RoomKind) -> Room {
    Room { rect, kind }
}

fn door(along: f32, width: f32) -> ExteriorDoor {
    ExteriorDoor { side: Side::Front, along, width, height: DOOR_HEIGHT }
}

/// The plan of a public building, if it is one.
pub fn volumes(b: &Building, style: Style, rng: &mut Rng) -> Option<Vec<Volume>> {
    let inside = Rect::new(-b.width * 0.5, -b.depth * 0.5, b.width * 0.5, b.depth * 0.5).grown(-style.wall_thickness * 0.5);
    let rise = (b.depth * 0.5).min(3.6);
    let gable = Roof::Gable { along: Axis::X, rise };
    Some(match b.kind {
        BuildingKind::Pub => vec![Volume { layout: pub_house(inside, style, rng), roof: gable }],
        BuildingKind::Shop => vec![Volume { layout: shop(inside, style, rng), roof: gable }],
        BuildingKind::School => vec![Volume { layout: school(inside, style, rng), roof: gable }],
        BuildingKind::Hall => vec![Volume { layout: village_hall(inside, style, rng), roof: gable }],
        BuildingKind::Barn => vec![Volume { layout: barn(inside, style), roof: gable }],
        BuildingKind::Church => church(style),
        BuildingKind::Shed => vec![Volume { layout: shed(inside, style, rng), roof: Roof::Gable { along: Axis::X, rise: 1.3 } }],
        _ => return None,
    })
}

/// A pub: the bar and the lounge across the front, the kitchen, the lavatories and a stair hall behind,
/// and a flat upstairs.
fn pub_house(inside: Rect, style: Style, rng: &mut Rng) -> Layout {
    let run = stair_run(2.9);
    let (w, d) = (inside.width(), inside.depth());
    let fz = inside.z0 + (d * 0.54).min(4.6);
    let hall_w = run + 1.1 + 1.0 + 0.1;
    let hx = inside.x1 - hall_w;
    let split = inside.x0 + w * rng.range(0.5, 0.58);
    let kitchen_end = inside.x0 + (hx - inside.x0) * 0.55;
    let zm = (fz + inside.z1) * 0.5;
    let bar = Rect::new(inside.x0, inside.z0, split, fz);
    let ground = vec![
        room(bar, RoomKind::Bar),
        room(Rect::new(split, inside.z0, inside.x1, fz), RoomKind::Lounge),
        room(Rect::new(inside.x0, fz, kitchen_end, inside.z1), RoomKind::Kitchen),
        room(Rect::new(kitchen_end, fz, hx, zm), RoomKind::Toilet),
        room(Rect::new(kitchen_end, zm, hx, inside.z1), RoomKind::Toilet),
        room(Rect::new(hx, fz, inside.x1, inside.z1), RoomKind::Hall),
    ];
    let stair = Stair { rect: Rect::new(hx + 1.1, inside.z1 - 0.15 - 1.1, hx + 1.1 + run, inside.z1 - 0.15), climbs: Climb::PosX };
    let lower = Storey {
        rooms: ground,
        stairs_up: vec![stair],
        // The kitchen is the bar's, as well as the hall's, and the lounge opens to the bar.
        extra_links: vec![(0, 1), (0, 2)],
        roots: vec![0],
        doors_outside: vec![door(inside.x0 + (split - inside.x0) * 0.45, style.door_width)],
        height: 2.9,
        ..Default::default()
    };
    // Upstairs: a landing, a passage along the front of the back rooms, bedrooms and a sitting room.
    let pz = fz + 1.6;
    let upper = vec![
        room(Rect::new(hx, fz, inside.x1, inside.z1), RoomKind::Landing),
        room(Rect::new(inside.x0, fz, hx, pz), RoomKind::Corridor),
        room(Rect::new(inside.x0, inside.z0, split, fz), RoomKind::Bedroom),
        room(Rect::new(split, inside.z0, inside.x1, fz), RoomKind::Living),
        room(Rect::new(inside.x0, pz, inside.x0 + 2.8, inside.z1), RoomKind::Bathroom),
        room(Rect::new(inside.x0 + 2.8, pz, hx, inside.z1), RoomKind::Bedroom),
    ];
    let top = Storey { rooms: upper, roots: vec![0], height: HOUSE_STOREY, ..Default::default() };
    Layout { inside, storeys: vec![lower, top], style }
}

/// A shop: the shop floor across the front, a stock room and a lavatory behind.
fn shop(inside: Rect, mut style: Style, rng: &mut Rng) -> Layout {
    style.window_width = 2.2;
    style.window_sill = 0.5;
    style.window_head = 2.5;
    style.window_spacing = 3.0;
    let d = inside.depth();
    let fz = inside.z0 + d * rng.range(0.62, 0.7);
    let loo = inside.x1 - 2.0;
    let rooms = vec![
        room(Rect::new(inside.x0, inside.z0, inside.x1, fz), RoomKind::Shop),
        room(Rect::new(inside.x0, fz, loo, inside.z1), RoomKind::Store),
        room(Rect::new(loo, fz, inside.x1, inside.z1), RoomKind::Toilet),
    ];
    let storey = Storey {
        rooms,
        roots: vec![0],
        doors_outside: vec![door(inside.x1 - 1.4, style.door_width)],
        height: 3.2,
        ..Default::default()
    };
    Layout { inside, storeys: vec![storey], style }
}

/// A school: a passage across the front with the office and the cloakroom at its ends, and a
/// classroom each side of the way back.
fn school(inside: Rect, style: Style, rng: &mut Rng) -> Layout {
    let (w, d) = (inside.width(), inside.depth());
    let cz = inside.z0 + 2.2;
    let office = 3.6;
    let cloaks = 3.6 + rng.range(0.0, 1.0);
    let mut rooms = vec![
        room(Rect::new(inside.x0, inside.z0, inside.x0 + office, cz), RoomKind::Office),
        room(Rect::new(inside.x0 + office, inside.z0, inside.x1 - cloaks, cz), RoomKind::Corridor),
        room(Rect::new(inside.x1 - cloaks, inside.z0, inside.x1, cz), RoomKind::Cloakroom),
    ];
    let classes = ((w / 7.0).round() as usize).clamp(2, 4);
    for k in 0..classes {
        let x0 = inside.x0 + w * k as f32 / classes as f32;
        let x1 = inside.x0 + w * (k + 1) as f32 / classes as f32;
        rooms.push(room(Rect::new(x0, cz, x1, inside.z1), if k == classes - 1 && classes > 2 { RoomKind::StaffRoom } else { RoomKind::Classroom }));
    }
    let _ = d;
    let along = inside.x0 + office + (w - office - cloaks) * 0.5;
    let storey = Storey { rooms, roots: vec![1], doors_outside: vec![door(along, style.door_width + 0.3)], height: 3.4, ..Default::default() };
    Layout { inside, storeys: vec![storey], style }
}

/// A village hall: a foyer with the lavatories and a store across the front, the hall itself, and
/// a kitchen at the side.
fn village_hall(inside: Rect, style: Style, rng: &mut Rng) -> Layout {
    let (w, d) = (inside.width(), inside.depth());
    let fz = inside.z0 + 2.5;
    let side = (w * 0.27).max(3.6);
    let lav = 2.3;
    let kz = inside.z0 + d * rng.range(0.55, 0.68);
    let rooms = vec![
        room(Rect::new(inside.x0, inside.z0, inside.x0 + lav, fz), RoomKind::Toilet),
        room(Rect::new(inside.x0 + lav, inside.z0, inside.x0 + lav * 2.0, fz), RoomKind::Toilet),
        room(Rect::new(inside.x0 + lav * 2.0, inside.z0, inside.x1 - side, fz), RoomKind::Corridor),
        room(Rect::new(inside.x1 - side, inside.z0, inside.x1, fz), RoomKind::Store),
        room(Rect::new(inside.x0, fz, inside.x1 - side, inside.z1), RoomKind::MainHall),
        room(Rect::new(inside.x1 - side, fz, inside.x1, kz), RoomKind::Kitchen),
        room(Rect::new(inside.x1 - side, kz, inside.x1, inside.z1), RoomKind::Store),
    ];
    let along = inside.x0 + lav * 2.0 + (inside.x1 - side - inside.x0 - lav * 2.0) * 0.5;
    let storey = Storey { rooms, roots: vec![2], doors_outside: vec![door(along, style.door_width + 0.4)], height: 4.0, ..Default::default() };
    Layout { inside, storeys: vec![storey], style }
}

/// A barn: one big room with a wide door.
fn barn(inside: Rect, style: Style) -> Layout {
    let storey = Storey {
        rooms: vec![room(inside, RoomKind::Barn)],
        roots: vec![0],
        doors_outside: vec![ExteriorDoor { side: Side::Front, along: (inside.x0 + inside.x1) * 0.5, width: 3.4, height: 3.2 }],
        height: 4.4,
        ..Default::default()
    };
    Layout { inside, storeys: vec![storey], style }
}

/// The church: a tower with its door at the front, the nave, and a narrower chancel at the far end.
fn church(mut style: Style) -> Vec<Volume> {
    style.window_width = 0.9;
    style.window_sill = 1.2;
    style.window_head = 4.4;
    style.window_spacing = 3.2;
    let t = style.wall_thickness * 0.5;
    let wide = |along: f32, height: f32| ExteriorDoor { side: Side::Front, along, width: 1.8, height };
    let tower_inside = Rect::new(-2.5, -9.25, 2.5, -4.25).grown(-t);
    let nave_inside = Rect::new(-3.5, -4.25, 3.5, 5.75).grown(-t);
    let chancel_inside = Rect::new(-2.5, 5.75, 2.5, 9.25).grown(-t);
    let tower = Storey {
        rooms: vec![room(tower_inside, RoomKind::Tower)],
        roots: vec![0],
        doors_outside: vec![
            wide(0.0, 2.8),
            ExteriorDoor { side: Side::Back, along: 0.0, width: 1.8, height: 2.8 },
        ],
        height: 4.6,
        ..Default::default()
    };
    let nave = Storey {
        rooms: vec![room(nave_inside, RoomKind::Nave)],
        roots: vec![0],
        doors_outside: vec![wide(0.0, 2.8), ExteriorDoor { side: Side::Back, along: 0.0, width: 2.2, height: 3.0 }],
        height: 6.0,
        ..Default::default()
    };
    let chancel = Storey {
        rooms: vec![room(chancel_inside, RoomKind::Chancel)],
        roots: vec![0],
        doors_outside: vec![ExteriorDoor { side: Side::Front, along: 0.0, width: 2.2, height: 3.0 }],
        height: 5.4,
        ..Default::default()
    };
    vec![
        Volume { layout: Layout { inside: tower_inside, storeys: vec![tower], style }, roof: Roof::Open },
        Volume { layout: Layout { inside: nave_inside, storeys: vec![nave], style }, roof: Roof::Gable { along: Axis::Z, rise: 3.2 } },
        Volume { layout: Layout { inside: chancel_inside, storeys: vec![chancel], style }, roof: Roof::Gable { along: Axis::Z, rise: 2.4 } },
    ]
}

/// The kiosk of a petrol station, on the forecourt's back edge.
pub fn kiosk(mut style: Style) -> Volume {
    style.window_width = 2.4;
    style.window_sill = 0.7;
    style.window_head = 2.4;
    style.window_spacing = 3.0;
    style.outer = [0.92, 0.92, 0.9];
    let inside = Rect::new(-4.25, 3.5, 4.25, 8.5).grown(-style.wall_thickness * 0.5);
    let fz = inside.z0 + 3.1;
    let storey = Storey {
        rooms: vec![
            room(Rect::new(inside.x0, inside.z0, inside.x1, fz), RoomKind::Shop),
            room(Rect::new(inside.x0, fz, inside.x1, inside.z1), RoomKind::Store),
        ],
        roots: vec![0],
        doors_outside: vec![door(inside.x1 - 1.5, style.door_width)],
        height: 3.0,
        ..Default::default()
    };
    Volume { layout: Layout { inside, storeys: vec![storey], style }, roof: Roof::Flat }
}

/// A big metal shed: a workshop floor with a small office in a front corner, a roller door in the
/// front and a plain door beside the office.
fn shed(inside: Rect, mut style: Style, rng: &mut Rng) -> Layout {
    style.windows = false;
    style.floor = [0.5, 0.5, 0.5];
    style.inner = [0.7, 0.72, 0.74];
    let office = Rect::new(inside.x0, inside.z0, inside.x0 + 4.6, inside.z0 + 3.6);
    let rooms = vec![
        room(Rect::new(inside.x0 + 4.6, inside.z0, inside.x1, inside.z0 + 3.6), RoomKind::Workshop),
        room(Rect::new(inside.x0, inside.z0 + 3.6, inside.x1, inside.z1), RoomKind::Workshop),
        room(office, RoomKind::Office),
    ];
    let roller = ExteriorDoor { side: Side::Front, along: inside.x0 + 4.6 + (inside.width() - 4.6) * rng.range(0.4, 0.6), width: 4.2, height: 4.2 };
    let storey = Storey {
        rooms,
        roots: vec![0],
        doors_outside: vec![roller, door(inside.x0 + 2.3, style.door_width)],
        height: (inside.depth() * 0.0 + 6.5).max(5.5),
        ..Default::default()
    };
    Layout { inside, storeys: vec![storey], style }
}

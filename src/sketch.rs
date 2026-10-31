//! A picture of the world as it is made, for the loading screen to show.
//!
//! The map and everything planned on it (land use, roads, the villages) are made on another thread. As each part
//! is done that thread puts it here, with a line saying what it will do next and how far along the work is; the
//! screen draws from this once a frame. The picture is a coarse one: the land every few vertices, and the roads and
//! buildings as they are placed.

use std::sync::{Arc, Mutex};

use bevy::prelude::*;

use crate::map::{TerrainMap, CELLS, HALF_SIZE, MAP_SIZE};
use crate::roads::RoadKind;
use crate::settlement_plan::Building;
use crate::zones::{Zone, ZoneMap};

/// Every this-many'th vertex of the map is a pixel of the picture.
const STEP: usize = 4;
/// Road points closer together than this (metres) are not kept for the picture.
const ROAD_GAP: f32 = 8.0;
/// How many pixels across the picture of the land is.
pub const SIDE: usize = CELLS / STEP + 1;

/// What the thread has drawn so far. `version` goes up with each change, so a screen can tell whether to redraw.
#[derive(Clone, Default)]
pub struct Drawing {
    pub version: u32,
    /// What is being done, and how far along the whole job is (0 to 1).
    pub line: String,
    pub share: f32,
    /// The land, `SIDE` by `SIDE`, row by row; empty until there is some.
    pub heights: Vec<f32>,
    pub water: Vec<bool>,
    /// What the land is used for, as a colour; empty until the zones are made.
    pub zones: Vec<[u8; 3]>,
    /// Where the villages, farms and mills are, and how big.
    pub settlements: Vec<(Vec2, f32)>,
    pub roads: Vec<(RoadKind, Vec<Vec2>)>,
    /// Footprints: centre, half the width and depth, the turn, and whether it is round.
    pub buildings: Vec<(Vec2, Vec2, f32, bool)>,
}

/// A [`Drawing`] shared between the thread that makes the world and the screen that shows it.
#[derive(Clone, Default)]
pub struct Sketch(Arc<Mutex<Drawing>>);

impl Sketch {
    fn change(&self, f: impl FnOnce(&mut Drawing)) {
        if let Ok(mut drawing) = self.0.lock() {
            f(&mut drawing);
            drawing.version += 1;
        }
    }

    /// A copy of the drawing, if it has changed since `seen`.
    pub fn since(&self, seen: u32) -> Option<Drawing> {
        let drawing = self.0.lock().ok()?;
        (drawing.version != seen).then(|| drawing.clone())
    }

    /// What is being done, and how far along (0 to 1) the job is.
    pub fn say(&self, line: &str, share: f32) {
        self.change(|d| {
            d.line = line.to_string();
            d.share = share;
        });
    }

    /// How far along the job is, without saying anything new.
    pub fn progress(&self, share: f32) {
        self.change(|d| d.share = share);
    }

    /// The land as it is in `heights`, a square of `verts` vertices a side, with the sea at zero.
    pub fn land(&self, heights: &[f32], verts: usize) {
        let pixels = (0..SIDE * SIDE).map(|i| heights[(i / SIDE * STEP).min(verts - 1) * verts + (i % SIDE * STEP).min(verts - 1)]).collect();
        self.change(|d| {
            d.water = vec![false; SIDE * SIDE];
            d.heights = pixels;
        });
    }

    /// The finished land, with its rivers and lakes.
    pub fn map(&self, map: &TerrainMap) {
        let at = |i: usize| ((i % SIDE * STEP).min(CELLS), (i / SIDE * STEP).min(CELLS));
        let heights = (0..SIDE * SIDE).map(|i| { let (x, z) = at(i); map.vertex_height(x, z) }).collect();
        // A pixel is wet if any vertex of the block it stands for is, or a river is narrower than the picture's pixels.
        let water = (0..SIDE * SIDE)
            .map(|i| {
                let (x, z) = at(i);
                (x..(x + STEP).min(CELLS + 1)).any(|x| (z..(z + STEP).min(CELLS + 1)).any(|z| map.water_level(x, z).is_some()))
            })
            .collect();
        self.change(|d| {
            d.heights = heights;
            d.water = water;
        });
    }

    pub fn settlements(&self, map: &TerrainMap) {
        let found = map.pois.iter().map(|p| (p.position, p.radius)).collect();
        self.change(|d| d.settlements = found);
    }

    pub fn zones(&self, zones: &ZoneMap) {
        let tints = (0..SIDE * SIDE).map(|i| tint(zones.zone_at((i % SIDE * STEP).min(CELLS), (i / SIDE * STEP).min(CELLS)))).collect();
        self.change(|d| d.zones = tints);
    }

    /// The roads, thinned: the picture does not need a point every metre.
    pub fn roads(&self, ribbons: &[crate::roads::RoadRibbon]) {
        let lines = ribbons.iter().map(|r| (r.kind, thinned(&r.points))).collect();
        self.change(|d| d.roads = lines);
    }

    pub fn buildings<'a>(&self, buildings: impl IntoIterator<Item = &'a Building>) {
        let found = buildings.into_iter().map(|b| (b.centre, Vec2::new(b.width, b.depth) * 0.5, b.yaw, b.kind.is_round())).collect();
        self.change(|d| d.buildings = found);
    }
}

/// `points` with those closer than `ROAD_GAP` to the one before left out, but the last kept.
fn thinned(points: &[Vec2]) -> Vec<Vec2> {
    let mut kept: Vec<Vec2> = Vec::new();
    for &p in points {
        if kept.last().is_none_or(|&last| last.distance(p) >= ROAD_GAP) {
            kept.push(p);
        }
    }
    if let (Some(&end), Some(&last)) = (points.last(), kept.last()) {
        if end != last {
            kept.push(end);
        }
    }
    kept
}

/// The colour a kind of land is washed with on the picture: muted, so the roads and buildings stand out.
fn tint(zone: Zone) -> [u8; 3] {
    match zone {
        Zone::Water => [58, 96, 140],
        Zone::Urban => [170, 120, 104],
        Zone::Industrial => [112, 112, 118],
        Zone::Military => [100, 108, 70],
        Zone::Orchard => [134, 160, 84],
        Zone::Arable => [188, 166, 92],
        Zone::Pasture => [128, 160, 96],
        Zone::Woodland => [56, 100, 58],
        Zone::Conifer => [44, 80, 66],
        Zone::Wetland => [84, 130, 116],
        Zone::Moorland => [130, 108, 124],
        Zone::Quarry => [140, 130, 112],
        Zone::Open => [160, 152, 104],
    }
}

impl Drawing {
    /// Whether any land has been drawn yet.
    pub fn has_land(&self) -> bool {
        self.heights.len() == SIDE * SIDE
    }

    /// The land as a picture: water blue, land washed with what it is used for once that is known, and shaded as if
    /// lit from the north-west. Row by row, `SIDE` by `SIDE`. Black where there is no land yet.
    pub fn picture(&self) -> Vec<[u8; 3]> {
        if !self.has_land() {
            return vec![[0, 0, 0]; SIDE * SIDE];
        }
        let (mut low, mut high) = (f32::MAX, f32::MIN);
        for (&h, &wet) in self.heights.iter().zip(&self.water) {
            if !wet {
                low = low.min(h);
                high = high.max(h);
            }
        }
        let span = (high - low).max(1.0);
        let at = |x: usize, z: usize| self.heights[z.min(SIDE - 1) * SIDE + x.min(SIDE - 1)];
        let mut pixels = Vec::with_capacity(SIDE * SIDE);
        for z in 0..SIDE {
            for x in 0..SIDE {
                let i = z * SIDE + x;
                // The slope toward the light, per pixel: how much higher the land is to the south-east than the north-west.
                let slope = at(x + 1, z + 1) - at(x.saturating_sub(1), z.saturating_sub(1));
                let light = (1.0 + (-slope / span * 5.0).clamp(-0.45, 0.45)).max(0.0);
                let height = ((self.heights[i] - low) / span).clamp(0.0, 1.0);
                let base = if self.water[i] || self.heights[i] < 0.0 {
                    [34.0, 70.0, 112.0]
                } else if let Some(zone) = self.zones.get(i) {
                    zone.map(|c| c as f32)
                } else {
                    // Before the land is put to use: lowland green climbing to bare tan.
                    [72.0 + 110.0 * height, 118.0 + 40.0 * height, 66.0 + 50.0 * height]
                };
                pixels.push(base.map(|c| (c * light).clamp(0.0, 255.0) as u8));
            }
        }
        pixels
    }
}

/// Where a point of the world is on a picture `side` pixels across.
pub fn to_picture(p: Vec2, side: f32) -> Vec2 {
    Vec2::new((p.x + HALF_SIZE) / MAP_SIZE, (p.y + HALF_SIZE) / MAP_SIZE) * side
}

/// The four corners of a footprint, as `Quat::from_rotation_y(yaw)` turns it, in the ground plane.
pub fn corners(centre: Vec2, half: Vec2, yaw: f32) -> [Vec2; 4] {
    let turn = Quat::from_rotation_y(yaw);
    [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].map(|(sx, sz)| {
        let world = turn * Vec3::new(sx * half.x, 0.0, sz * half.y);
        centre + Vec2::new(world.x, world.z)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat_land(height: f32) -> Vec<f32> {
        vec![height; (CELLS + 1) * (CELLS + 1)]
    }

    #[test]
    fn the_screen_only_sees_a_new_drawing_when_something_has_changed() {
        let sketch = Sketch::default();
        let first = sketch.since(u32::MAX).expect("a screen that has seen nothing is given the drawing");
        assert!(sketch.since(first.version).is_none(), "and is not given it again");
        sketch.say("Laying the roads", 0.5);
        let next = sketch.since(first.version).expect("a change is seen");
        assert_eq!((next.line.as_str(), next.share), ("Laying the roads", 0.5));
    }

    #[test]
    fn there_is_no_land_to_draw_until_some_is_given_and_then_the_sea_is_blue() {
        let sketch = Sketch::default();
        assert!(!sketch.since(u32::MAX).unwrap().has_land());
        let verts = CELLS + 1;
        let mut heights = flat_land(20.0);
        // The west edge is under the sea.
        for z in 0..verts {
            for x in 0..verts / 4 {
                heights[z * verts + x] = -5.0;
            }
        }
        sketch.land(&heights, verts);
        let drawing = sketch.since(0).unwrap();
        assert!(drawing.has_land());
        let picture = drawing.picture();
        assert_eq!(picture.len(), SIDE * SIDE);
        let row = SIDE * (SIDE / 2);
        let (sea, land) = (picture[row + 2], picture[row + SIDE - 3]);
        assert!(sea[2] > sea[0] && sea[2] > sea[1], "{sea:?} is blue");
        assert!(land[1] > land[2], "{land:?} is green or brown");
    }

    #[test]
    fn land_that_is_put_to_use_takes_the_colour_of_its_use() {
        let sketch = Sketch::default();
        sketch.land(&flat_land(20.0), CELLS + 1);
        let before = sketch.since(0).unwrap().picture()[SIDE * 10 + 10];
        let mut drawing = sketch.since(0).unwrap();
        drawing.zones = vec![tint(Zone::Woodland); SIDE * SIDE];
        let after = drawing.picture()[SIDE * 10 + 10];
        assert_ne!(before, after);
        assert!(after[1] > after[0] && after[1] > after[2], "{after:?} is the green of a wood");
    }

    #[test]
    fn the_corners_of_a_footprint_are_where_the_building_would_be() {
        let c = corners(Vec2::new(10.0, 20.0), Vec2::new(2.0, 1.0), 0.0);
        let xs: Vec<f32> = c.iter().map(|p| p.x).collect();
        let zs: Vec<f32> = c.iter().map(|p| p.y).collect();
        assert_eq!((xs.iter().cloned().fold(f32::MAX, f32::min), xs.iter().cloned().fold(f32::MIN, f32::max)), (8.0, 12.0));
        assert_eq!((zs.iter().cloned().fold(f32::MAX, f32::min), zs.iter().cloned().fold(f32::MIN, f32::max)), (19.0, 21.0));
        let turned = corners(Vec2::ZERO, Vec2::new(2.0, 1.0), std::f32::consts::FRAC_PI_2);
        assert!(turned.iter().all(|p| p.x.abs() <= 1.0 + 1e-4 && p.y.abs() <= 2.0 + 1e-4), "a quarter turn swaps the sides: {turned:?}");
    }

    #[test]
    fn a_point_of_the_world_is_placed_on_the_picture_by_its_share_of_the_map() {
        assert_eq!(to_picture(Vec2::splat(-HALF_SIZE), 100.0), Vec2::ZERO);
        assert_eq!(to_picture(Vec2::ZERO, 100.0), Vec2::splat(50.0));
    }
}

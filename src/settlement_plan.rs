//! Where a settlement's streets and buildings go.
//!
//! A real village is built along its streets: the roads came first, and the houses stand beside them,
//! facing them, with a bit of garden in between. So that is what is done here. The settlement's own
//! roads (the ones the road network already runs to it) are its main streets; side lanes are grown off
//! them; and buildings are placed along the frontage of all of these, the important ones (the church)
//! first. Every building has to fit: clear of the roads, of its neighbours, of water, and on ground
//! that is flat enough. Whatever doesn't fit is simply left out, leaving a gap.
//!
//! This is the plan, as plain data; `settlement.rs` turns it into meshes.

use std::collections::HashMap;
use std::f32::consts::PI;

use bevy::prelude::*;

use crate::collision::Shape;
use crate::map::{Poi, PoiKind, TerrainMap, SMALL_SETTLEMENT_RADIUS};
use crate::roads::{road_ribbons, RoadClearance, RoadKind, RoadNetwork, RoadRibbon, LANE_HALF_WIDTH};

/// What a building is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum BuildingKind {
    /// A small house.
    Cottage,
    /// A bigger, detached house.
    House,
    /// A row of houses sharing walls, in the middle of a town.
    Terrace,
    Church,
}

/// One building in a plan.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Building {
    pub kind: BuildingKind,
    /// The middle of its footprint, in the world's ground plane.
    pub centre: Vec2,
    /// How it is turned, as `Quat::from_rotation_y(yaw)` turns it; its front faces local -Z.
    pub yaw: f32,
    /// Its footprint: across (local X) and deep (local Z), metres.
    pub width: f32,
    pub depth: f32,
    /// How high its walls are.
    pub wall_height: f32,
    /// A unit vector from the building toward the street it faces.
    pub front: Vec2,
}

impl Building {
    pub fn shape(&self) -> Shape {
        Shape::Box { centre: self.centre, half: Vec2::new(self.width, self.depth) * 0.5, yaw: self.yaw }
    }
}

/// An area that is kept clear of houses, such as a churchyard.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Yard {
    pub centre: Vec2,
    pub radius: f32,
}

/// Everything planned for one settlement.
#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub buildings: Vec<Building>,
    /// Streets grown off the roads that reach it.
    pub lanes: Vec<RoadRibbon>,
    pub yards: Vec<Yard>,
    /// How many spots were turned down, and why (for finding out what is wrong, not for building).
    pub rejected: HashMap<&'static str, usize>,
}

/// The plans for every settlement on the map, in the order of `map.pois`.
#[derive(Resource, Clone, Debug, Default)]
pub struct SettlementPlan {
    pub layouts: Vec<Layout>,
}

impl SettlementPlan {
    pub fn generate(map: &TerrainMap, roads: &RoadNetwork) -> SettlementPlan {
        let ribbons = road_ribbons(map, roads);
        SettlementPlan { layouts: map.pois.iter().map(|poi| Layout::generate(map, poi, &ribbons)).collect() }
    }

    /// Every lane of every settlement.
    pub fn lanes(&self) -> impl Iterator<Item = RoadRibbon> + '_ {
        self.layouts.iter().flat_map(|l| l.lanes.iter().cloned())
    }
}

// ---- tuning --------------------------------------------------------------------------------------

/// Buildings stay inside this fraction of the settlement's radius.
const BUILD_LIMIT: f32 = 0.95;
/// A building keeps this far from the edge of a road, and from another building, metres.
const ROAD_GAP: f32 = 1.0;
const BUILDING_GAP: f32 = 1.2;
/// The most the ground may vary across a footprint, metres.
const MAX_FOOTPRINT_RANGE: f32 = 1.8;
/// Ground this near the sea, or water this near, is not built on.
const SEA_MARGIN: f32 = 0.6;
const WATER_MARGIN: f32 = 6.0;
const SAMPLE_STEP: f32 = 2.0;
const STREET_STEP: f32 = 2.0;

/// What kind of settlement a radius makes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    Hamlet,
    Village,
    LargeVillage,
    Town,
}

impl Tier {
    pub fn of(poi: &Poi) -> Option<Tier> {
        (poi.kind == PoiKind::Village).then_some(match poi.radius {
            r if r >= 150.0 => Tier::Town,
            r if r >= 90.0 => Tier::LargeVillage,
            r if r >= SMALL_SETTLEMENT_RADIUS => Tier::Village,
            _ => Tier::Hamlet,
        })
    }

    /// How far apart side lanes leave a street, and the most of them.
    fn lane_spacing(self) -> (f32, usize) {
        match self {
            Tier::Hamlet => (70.0, 2),
            Tier::Village => (55.0, 6),
            Tier::LargeVillage => (42.0, 18),
            Tier::Town => (30.0, 64),
        }
    }

    /// The chance that a free spot on a street gets a building.
    fn density(self) -> f32 {
        match self {
            Tier::Hamlet => 0.9,
            Tier::Village => 0.92,
            Tier::LargeVillage => 0.94,
            Tier::Town => 0.97,
        }
    }

    /// The gap between neighbouring houses, metres (low, high): tighter the more built up the place.
    fn gaps(self, core: f32) -> (f32, f32) {
        match self {
            Tier::Hamlet => (3.0, 9.0),
            Tier::Village => (2.0, 6.5),
            Tier::LargeVillage => (1.5, 5.0),
            Tier::Town if core > 0.5 => (0.8, 2.5),
            Tier::Town => (1.2, 4.0),
        }
    }

    /// How far back from the street houses stand (low, high).
    fn setbacks(self) -> (f32, f32) {
        match self {
            Tier::Town => (1.5, 3.5),
            _ => (2.0, 5.0),
        }
    }
}

// ---- randomness -----------------------------------------------------------------------------------

pub struct Rng(u64);

impl Rng {
    pub fn from_position(p: Vec2) -> Rng {
        Rng(((p.x.to_bits() as u64) << 32 ^ p.y.to_bits() as u64) ^ 0xA076_1D64_78BD_642F | 1)
    }

    pub fn unit(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }
}

// ---- geometry --------------------------------------------------------------------------------------

fn left_of(direction: Vec2) -> Vec2 {
    Vec2::new(-direction.y, direction.x)
}

/// The yaw that points a building's front (local -Z) along `front`.
pub fn yaw_facing(front: Vec2) -> f32 {
    (-front.x).atan2(-front.y)
}

/// Points over a footprint, a couple of metres apart, edges and corners included.
pub fn footprint_points(shape: &Shape) -> Vec<Vec2> {
    match *shape {
        Shape::Box { centre, half, yaw } => {
            let (nx, nz) = (((half.x * 2.0) / SAMPLE_STEP).ceil().max(1.0) as i32, ((half.y * 2.0) / SAMPLE_STEP).ceil().max(1.0) as i32);
            let turn = Quat::from_rotation_y(yaw);
            let mut points = Vec::with_capacity(((nx + 1) * (nz + 1)) as usize);
            for i in 0..=nx {
                for j in 0..=nz {
                    let local = Vec3::new((i as f32 / nx as f32 * 2.0 - 1.0) * half.x, 0.0, (j as f32 / nz as f32 * 2.0 - 1.0) * half.y);
                    let world = turn * local;
                    points.push(centre + Vec2::new(world.x, world.z));
                }
            }
            points
        }
        Shape::Circle { centre, radius } => vec![centre, centre + Vec2::X * radius, centre - Vec2::X * radius, centre + Vec2::Y * radius, centre - Vec2::Y * radius],
        Shape::Wall { a, b, .. } => vec![a, (a + b) * 0.5, b],
    }
}

fn bounding_radius(shape: &Shape) -> f32 {
    match *shape {
        Shape::Box { half, .. } => half.length(),
        Shape::Circle { radius, .. } => radius,
        Shape::Wall { a, b, half_thickness } => a.distance(b) * 0.5 + half_thickness,
    }
}

fn shape_centre(shape: &Shape) -> Vec2 {
    match *shape {
        Shape::Box { centre, .. } | Shape::Circle { centre, .. } => centre,
        Shape::Wall { a, b, .. } => (a + b) * 0.5,
    }
}

/// What has been placed so far, findable by where it is.
struct Placed {
    shapes: Vec<Shape>,
    grid: HashMap<(i32, i32), Vec<usize>>,
}

const GRID: f32 = 16.0;

impl Placed {
    fn new() -> Placed {
        Placed { shapes: Vec::new(), grid: HashMap::new() }
    }

    fn add(&mut self, shape: Shape) {
        let (c, r) = (shape_centre(&shape), bounding_radius(&shape));
        let id = self.shapes.len();
        for gz in ((c.y - r) / GRID).floor() as i32..=((c.y + r) / GRID).floor() as i32 {
            for gx in ((c.x - r) / GRID).floor() as i32..=((c.x + r) / GRID).floor() as i32 {
                self.grid.entry((gx, gz)).or_default().push(id);
            }
        }
        self.shapes.push(shape);
    }

    /// Whether `shape`, kept `gap` from everything, would overlap something already here.
    fn collides(&self, shape: &Shape, gap: f32) -> bool {
        let (c, r) = (shape_centre(shape), bounding_radius(shape));
        let mut seen = std::collections::HashSet::new();
        let mine = footprint_points(shape);
        for gz in ((c.y - r - gap) / GRID).floor() as i32..=((c.y + r + gap) / GRID).floor() as i32 {
            for gx in ((c.x - r - gap) / GRID).floor() as i32..=((c.x + r + gap) / GRID).floor() as i32 {
                for &id in self.grid.get(&(gx, gz)).into_iter().flatten() {
                    if !seen.insert(id) {
                        continue;
                    }
                    let other = &self.shapes[id];
                    if shape_centre(other).distance(c) > bounding_radius(other) + r + gap {
                        continue;
                    }
                    if mine.iter().any(|&p| other.separation(p).0 < gap) || footprint_points(other).iter().any(|&p| shape.separation(p).0 < gap) {
                        return true;
                    }
                }
            }
        }
        false
    }
}

// ---- streets ----------------------------------------------------------------------------------------

/// A street resampled evenly, for walking along.
struct Frontage {
    points: Vec<Vec2>,
    half_widths: Vec<f32>,
}

impl Frontage {
    fn length(&self) -> f32 {
        (self.points.len().saturating_sub(1)) as f32 * STREET_STEP
    }

    /// The street's point, direction of travel and half width at distance `s` along it.
    fn at(&self, s: f32) -> (Vec2, Vec2, f32) {
        let f = (s / STREET_STEP).clamp(0.0, (self.points.len() - 1) as f32);
        let i = (f.floor() as usize).min(self.points.len() - 2);
        let t = f - i as f32;
        let point = self.points[i].lerp(self.points[i + 1], t);
        let direction = (self.points[i + 1] - self.points[i]).normalize_or(Vec2::X);
        (point, direction, self.half_widths[i] + (self.half_widths[i + 1] - self.half_widths[i]) * t)
    }
}

/// The parts of a street inside a disc, resampled every `STREET_STEP` metres.
fn frontages_of(ribbon: &RoadRibbon, centre: Vec2, limit: f32) -> Vec<Frontage> {
    let mut samples: Vec<(Vec2, f32)> = Vec::new();
    let mut leftover = 0.0f32;
    samples.push((ribbon.points[0], ribbon.half_widths[0]));
    for i in 0..ribbon.points.len() - 1 {
        let (a, b) = (ribbon.points[i], ribbon.points[i + 1]);
        let length = a.distance(b);
        let mut d = STREET_STEP - leftover;
        while d <= length {
            let t = d / length;
            samples.push((a.lerp(b, t), ribbon.half_widths[i] + (ribbon.half_widths[i + 1] - ribbon.half_widths[i]) * t));
            d += STREET_STEP;
        }
        leftover = length - (d - STREET_STEP);
    }
    let mut runs: Vec<Frontage> = Vec::new();
    let mut current = Frontage { points: vec![], half_widths: vec![] };
    for (p, hw) in samples {
        if p.distance(centre) <= limit {
            current.points.push(p);
            current.half_widths.push(hw);
        } else if current.points.len() >= 2 {
            runs.push(std::mem::replace(&mut current, Frontage { points: vec![], half_widths: vec![] }));
        } else {
            current = Frontage { points: vec![], half_widths: vec![] };
        }
    }
    if current.points.len() >= 2 {
        runs.push(current);
    }
    runs
}

// ---- the plan for one settlement --------------------------------------------------------------------

struct Site<'a> {
    map: &'a TerrainMap,
    centre: Vec2,
    radius: f32,
    streets: RoadClearance,
    placed: Placed,
    /// Why spots were turned down, for finding out what is wrong when too few buildings fit.
    rejected: std::cell::RefCell<HashMap<&'static str, usize>>,
}

impl Site<'_> {
    /// Whether a building of this shape can stand here.
    fn fits(&self, shape: &Shape) -> bool {
        match self.why_not(shape) {
            None => true,
            Some(reason) => {
                *self.rejected.borrow_mut().entry(reason).or_default() += 1;
                false
            }
        }
    }

    /// What stops a building of this shape standing here, if anything.
    fn why_not(&self, shape: &Shape) -> Option<&'static str> {
        let points = footprint_points(shape);
        let limit = self.radius * BUILD_LIMIT;
        let (mut low, mut high) = (f32::MAX, f32::MIN);
        for &p in &points {
            if p.distance(self.centre) > limit {
                return Some("outside the settlement");
            }
            if self.streets.clearance(p) <= ROAD_GAP {
                return Some("on or against a road");
            }
            if self.map.water_surface_at(p).is_some() {
                return Some("in water");
            }
            let h = self.map.height_at(p);
            low = low.min(h);
            high = high.max(h);
        }
        let middle = shape_centre(shape);
        if low <= SEA_MARGIN {
            return Some("at sea level");
        }
        if high - low > MAX_FOOTPRINT_RANGE {
            return Some("on a slope");
        }
        if self.map.water_distance(middle) <= WATER_MARGIN {
            return Some("too near water");
        }
        if self.placed.collides(shape, BUILDING_GAP) {
            return Some("against another building");
        }
        None
    }
}

impl Layout {
    /// Plans one settlement. (Only villages and hamlets so far; farms and mills are left as they were.)
    pub fn generate(map: &TerrainMap, poi: &Poi, ribbons: &[RoadRibbon]) -> Layout {
        let Some(tier) = Tier::of(poi) else { return Layout::default() };
        let (centre, radius) = (poi.position, poi.radius);
        let mut rng = Rng::from_position(centre);

        // The roads that reach here are its main streets.
        let mut streets: Vec<RoadRibbon> = ribbons
            .iter()
            .filter(|r| matches!(r.kind, RoadKind::Major | RoadKind::Minor) && r.points.iter().any(|p| p.distance(centre) < radius + 20.0))
            .cloned()
            .collect();
        let main_count = streets.len();
        if main_count == 0 {
            // No road comes here: give it a street of its own, so there is somewhere to build along.
            streets.push(through_street(&mut rng, map, centre, radius));
        }
        let mut lanes = grow_lanes(&mut rng, map, centre, radius, tier, &streets);
        streets.extend(lanes.iter().cloned());
        let mut layout = Layout::default();
        let mut site = Site { map, centre, radius, streets: RoadClearance::new(&streets), placed: Placed::new(), rejected: Default::default() };

        let frontages: Vec<(Frontage, RoadKind)> = streets.iter().flat_map(|r| frontages_of(r, centre, radius * BUILD_LIMIT).into_iter().map(move |f| (f, r.kind))).collect();

        // The church first, where it has the pick of the plots, then everything else around it.
        if radius >= SMALL_SETTLEMENT_RADIUS {
            place_church(&mut rng, &mut site, &mut layout, &frontages, poi.landmark);
        }
        fill_with_houses(&mut rng, &mut site, &mut layout, &frontages, tier);
        layout.lanes = std::mem::take(&mut lanes);
        layout.rejected = site.rejected.into_inner();
        drop_unused_lanes(&mut layout);
        layout
    }
}

/// A street straight through a settlement that no road reaches.
fn through_street(rng: &mut Rng, map: &TerrainMap, centre: Vec2, radius: f32) -> RoadRibbon {
    let heading = rng.range(0.0, PI);
    let direction = Vec2::new(heading.cos(), heading.sin());
    let half = radius * 0.8;
    let points: Vec<Vec2> = (0..=((half * 2.0 / 6.0) as usize)).map(|i| centre + direction * (-half + i as f32 * 6.0)).collect();
    let _ = map;
    RoadRibbon { kind: RoadKind::Lane, half_widths: vec![LANE_HALF_WIDTH; points.len()], points, start_junction: false, end_junction: false }
}

/// Side lanes grown off the settlement's main streets.
fn grow_lanes(rng: &mut Rng, map: &TerrainMap, centre: Vec2, radius: f32, tier: Tier, mains: &[RoadRibbon]) -> Vec<RoadRibbon> {
    let (spacing, max_lanes) = tier.lane_spacing();
    let mut lanes: Vec<RoadRibbon> = Vec::new();
    let mut all: Vec<RoadRibbon> = mains.to_vec();
    let mut clearance = RoadClearance::new(&all);
    // Candidate starts: along every main street, every `spacing` metres, on alternate sides.
    let mut starts: Vec<(Vec2, Vec2, f32)> = Vec::new();
    let mut flip = rng.unit() < 0.5;
    for ribbon in mains {
        for frontage in frontages_of(ribbon, centre, radius * 0.8) {
            let mut s = rng.range(spacing * 0.3, spacing * 0.8);
            while s < frontage.length() {
                let (p, t, hw) = frontage.at(s);
                // Not too near the middle (that's the junction of the roads themselves).
                if p.distance(centre) > 10.0 {
                    starts.push((p, if flip { left_of(t) } else { -left_of(t) }, hw));
                    flip = !flip;
                }
                s += spacing * rng.range(0.8, 1.25);
            }
        }
    }
    // Nearest the middle first: the centre of a place is built up first.
    starts.sort_by(|a, b| a.0.distance(centre).total_cmp(&b.0.distance(centre)));
    for (start, outward, hw) in starts {
        // A town's side streets cross the main street; elsewhere they leave it on alternate sides.
        let sides: &[f32] = match tier {
            Tier::Town => &[1.0, -1.0],
            Tier::LargeVillage if rng.unit() < 0.4 => &[1.0, -1.0],
            _ => &[1.0],
        };
        for &side in sides {
            if lanes.len() >= max_lanes {
                break;
            }
            if let Some(lane) = grow_lane(rng, map, centre, radius, start, outward * side, hw, &clearance) {
                all.push(lane.clone());
                clearance = RoadClearance::new(&all);
                lanes.push(lane);
            }
        }
    }
    lanes
}

/// One lane, leaving a street at `start` in direction `outward`, wandering a little, avoiding steep
/// ground, water and other streets, and ending where it runs into one (a junction) or runs out of room
/// (a dead end).
#[allow(clippy::too_many_arguments)]
fn grow_lane(rng: &mut Rng, map: &TerrainMap, centre: Vec2, radius: f32, start: Vec2, outward: Vec2, parent_hw: f32, clearance: &RoadClearance) -> Option<RoadRibbon> {
    const STEP: f32 = 5.0;
    let target = rng.range(0.3, 0.75) * radius;
    // Start at the parent street's edge, and a little way in so the two overlap into a junction.
    let mut position = start + outward * (parent_hw * 0.5);
    let mut heading = outward;
    let mut points = vec![position];
    let mut length = 0.0;
    let mut joined = false;
    while length < target {
        // Wander: drift the heading a little each step.
        let drift = rng.range(-0.12, 0.12);
        let turn = |angle: f32| Vec2::from_angle(angle).rotate(heading);
        let mut chosen = None;
        for angle in [drift, drift + 0.3, drift - 0.3, drift + 0.6, drift - 0.6] {
            let direction = turn(angle);
            let next = position + direction * STEP;
            let rise = (map.height_at(next) - map.height_at(position)).abs() / STEP;
            if rise < 0.14 && next.distance(centre) < radius * 0.92 && map.water_surface_at(next).is_none() && map.water_distance(next) > 3.0 {
                chosen = Some((direction, next));
                break;
            }
        }
        let Some((direction, next)) = chosen else { break };
        // Close to another street, other than the one it started from: it ends there, as a T.
        if length > parent_hw * 2.0 + 8.0 && clearance.clearance(next) < LANE_HALF_WIDTH + 2.0 {
            joined = true;
            points.push(next);
            break;
        }
        position = next;
        heading = direction;
        points.push(position);
        length += STEP;
    }
    (length >= 18.0 || (joined && length >= 12.0)).then(|| RoadRibbon {
        kind: RoadKind::Lane,
        half_widths: vec![LANE_HALF_WIDTH; points.len()],
        points,
        start_junction: true,
        end_junction: joined,
    })
}

/// A building whose front faces `front`, set `setback` metres back from a point on a street.
fn building_on_frontage(kind: BuildingKind, street_point: Vec2, tangent: Vec2, half_width: f32, side: f32, setback: f32, width: f32, depth: f32, wall_height: f32) -> Building {
    let normal = left_of(tangent) * side;
    let front = -normal;
    Building {
        kind,
        centre: street_point + normal * (half_width + setback + depth * 0.5),
        yaw: yaw_facing(front),
        width,
        depth,
        wall_height,
        front,
    }
}

/// The church: on a plot beside a street as near the settlement's highest ground (the landmark) as one
/// can be had, with its churchyard kept clear around it.
fn place_church(rng: &mut Rng, site: &mut Site, layout: &mut Layout, frontages: &[(Frontage, RoadKind)], landmark: Vec2) {
    // The model: a nave 7 m wide and 14 long, with a tower 5 m square on one end.
    const WIDTH: f32 = 7.6;
    const DEPTH: f32 = 19.0;
    let mut candidates: Vec<(f32, Building)> = Vec::new();
    for (frontage, _) in frontages {
        for side in [1.0, -1.0] {
            let mut s = DEPTH * 0.5;
            while s < frontage.length() - DEPTH * 0.5 {
                let (p, t, hw) = frontage.at(s);
                // Facing the street end-on, with its tower toward the road.
                let normal = left_of(t) * side;
                let b = Building {
                    kind: BuildingKind::Church,
                    centre: p + normal * (hw + 5.0 + DEPTH * 0.5),
                    yaw: yaw_facing(-normal),
                    width: WIDTH,
                    depth: DEPTH,
                    wall_height: 6.0,
                    front: -normal,
                };
                // Prefer near the landmark, and not too far from the middle.
                let score = b.centre.distance(landmark) + 0.3 * b.centre.distance(site.centre);
                candidates.push((score, b));
                s += 4.0;
            }
        }
    }
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, church) in candidates {
        if site.fits(&church.shape()) {
            layout.yards.push(Yard { centre: church.centre, radius: DEPTH * 0.5 + 8.0 });
            site.placed.add(church.shape());
            // Nothing else is built in the churchyard.
            site.placed.add(Shape::Circle { centre: church.centre, radius: DEPTH * 0.5 + 6.0 });
            layout.buildings.push(church);
            let _ = rng;
            return;
        }
    }
}

/// Fills the frontage of every street with houses.
fn fill_with_houses(rng: &mut Rng, site: &mut Site, layout: &mut Layout, frontages: &[(Frontage, RoadKind)], tier: Tier) {
    for (frontage, _) in frontages {
        for side in [1.0f32, -1.0] {
            let mut s = rng.range(1.0, 6.0);
            while s < frontage.length() - 4.0 {
                let here = frontage.at(s).0;
                let core = (1.0 - here.distance(site.centre) / site.radius).clamp(0.0, 1.0);
                let chance = tier.density() * (1.0 - 0.5 * (1.0 - core) * (1.0 - core));
                let mut placed = false;
                if rng.unit() < chance {
                    // A plot that won't take the house first tried gets a smaller one.
                    for attempt in 0..3 {
                        let (kind, width, depth, wall) = pick_house(rng, tier, core, attempt);
                        let (p, t, hw) = frontage.at((s + width * 0.5).min(frontage.length()));
                        let (back_low, back_high) = tier.setbacks();
                        let building = building_on_frontage(kind, p, t, hw, side, rng.range(back_low, back_high), width, depth, wall);
                        if site.fits(&building.shape()) {
                            site.placed.add(building.shape());
                            layout.buildings.push(building);
                            // Terraced houses touch their neighbours; detached ones don't.
                            let (gap_low, gap_high) = tier.gaps(core);
                            s += width + if kind == BuildingKind::Terrace { 0.3 } else { rng.range(gap_low, gap_high) };
                            placed = true;
                            break;
                        }
                    }
                }
                if !placed {
                    s += 3.0;
                }
            }
        }
    }
}

/// What kind of house to put at a place `core` (0 at the edge, 1 in the middle) of the settlement: the
/// middle of a big place is built tighter and taller. A higher `attempt` is a smaller house, for a plot
/// that wouldn't take the first.
fn pick_house(rng: &mut Rng, tier: Tier, core: f32, attempt: usize) -> (BuildingKind, f32, f32, f32) {
    let boost = if tier == Tier::Town { 1.0 + 0.6 * core } else { 1.0 };
    let roll = rng.unit();
    match attempt {
        0 if tier == Tier::Town && core > 0.5 && roll < 0.55 => (BuildingKind::Terrace, rng.range(14.0, 34.0), rng.range(6.5, 8.0), rng.range(5.0, 6.5) * (0.9 + 0.1 * boost)),
        0 if roll >= 0.55 => (BuildingKind::House, rng.range(8.0, 11.0), rng.range(6.0, 8.0), rng.range(3.8, 4.5) * boost),
        0 | 1 => (BuildingKind::Cottage, rng.range(5.5, 7.5), rng.range(5.0, 6.2), rng.range(2.9, 3.5) * boost),
        _ => (BuildingKind::Cottage, rng.range(4.6, 5.4), rng.range(4.4, 5.0), rng.range(2.7, 3.1)),
    }
}

/// Drops the lanes that nothing is built along (and that don't lead anywhere), so there are no roads
/// to nowhere.
fn drop_unused_lanes(layout: &mut Layout) {
    let clearance_of = |lane: &RoadRibbon, p: Vec2| RoadClearance::new(std::slice::from_ref(lane)).clearance(p);
    let buildings = layout.buildings.clone();
    layout.lanes.retain(|lane| {
        let served = buildings.iter().filter(|b| clearance_of(lane, b.centre) < 14.0).count();
        served >= 2 || lane.end_junction
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::GenParams;

    fn flat_site(radius: f32, streets: &[RoadRibbon]) -> (TerrainMap, Poi) {
        let map = TerrainMap::flat(20.0);
        let _ = streets;
        (map, Poi { kind: PoiKind::Village, position: Vec2::ZERO, landmark: Vec2::new(10.0, 10.0), radius })
    }

    fn main_road(length: f32) -> RoadRibbon {
        let points: Vec<Vec2> = (0..=(length / 5.0) as usize).map(|i| Vec2::new(-length / 2.0 + i as f32 * 5.0, 0.0)).collect();
        RoadRibbon { kind: RoadKind::Major, half_widths: vec![4.0; points.len()], points, start_junction: false, end_junction: false }
    }

    #[test]
    fn houses_stand_along_a_street_facing_it() {
        let (map, poi) = flat_site(80.0, &[]);
        let road = main_road(200.0);
        let layout = Layout::generate(&map, &poi, &[road.clone()]);
        assert!(layout.buildings.len() > 15, "{} buildings", layout.buildings.len());
        let mut streets = vec![road];
        streets.extend(layout.lanes.iter().cloned());
        let clearance = RoadClearance::new(&streets);
        for b in layout.buildings.iter().filter(|b| b.kind != BuildingKind::Church) {
            // Just in front of the building there is a street (within a setback and a bit), and behind
            // it, further off, there is not one right up against it.
            let in_front = clearance.clearance(b.centre + b.front * (b.depth * 0.5 + 0.5));
            let behind = clearance.clearance(b.centre - b.front * (b.depth * 0.5 + 0.5));
            assert!(in_front < 5.5, "{b:?}: the street is {in_front} m from its front");
            assert!(in_front <= behind + 0.5, "{b:?} has its back to the street ({in_front} in front, {behind} behind)");
        }
    }

    #[test]
    fn no_building_is_on_a_road_or_on_another() {
        let (map, poi) = flat_site(120.0, &[]);
        let road = main_road(300.0);
        let layout = Layout::generate(&map, &poi, &[road.clone()]);
        let mut streets = vec![road];
        streets.extend(layout.lanes.iter().cloned());
        let clearance = RoadClearance::new(&streets);
        for (i, b) in layout.buildings.iter().enumerate() {
            for p in footprint_points(&b.shape()) {
                assert!(clearance.clearance(p) > 0.0, "{b:?} is on a road");
            }
            for (j, other) in layout.buildings.iter().enumerate() {
                if i != j {
                    assert!(footprint_points(&b.shape()).iter().all(|&p| other.shape().separation(p).0 > 0.0), "{b:?} overlaps {other:?}");
                }
            }
        }
    }

    #[test]
    fn a_big_settlement_has_lanes_and_more_buildings_than_a_small_one() {
        let count = |radius: f32| {
            let (map, poi) = flat_site(radius, &[]);
            let layout = Layout::generate(&map, &poi, &[main_road(radius * 2.0)]);
            (layout.buildings.len(), layout.lanes.len())
        };
        let (hamlet, hamlet_lanes) = count(35.0);
        let (village, village_lanes) = count(70.0);
        let (town, town_lanes) = count(230.0);
        assert!(hamlet < village && village < town, "{hamlet} {village} {town}");
        assert!(hamlet_lanes <= village_lanes && village_lanes < town_lanes, "{hamlet_lanes} {village_lanes} {town_lanes}");
        assert!(town > 150, "a town has hundreds of buildings: {town}");
    }

    #[test]
    fn only_a_settlement_big_enough_has_a_church_and_there_is_at_most_one() {
        let churches = |radius: f32| {
            let (map, poi) = flat_site(radius, &[]);
            Layout::generate(&map, &poi, &[main_road(radius * 2.0)]).buildings.iter().filter(|b| b.kind == BuildingKind::Church).count()
        };
        assert_eq!(churches(35.0), 0, "a hamlet has no church");
        assert_eq!(churches(70.0), 1);
        assert_eq!(churches(230.0), 1);
    }

    #[test]
    fn the_church_is_near_the_landmark_with_a_churchyard_kept_clear() {
        let (map, poi) = flat_site(90.0, &[]);
        let layout = Layout::generate(&map, &poi, &[main_road(200.0)]);
        let church = layout.buildings.iter().find(|b| b.kind == BuildingKind::Church).expect("a church");
        assert!(church.centre.distance(poi.landmark) < 45.0, "{:?}", church.centre);
        let yard = layout.yards[0];
        for b in layout.buildings.iter().filter(|b| b.kind != BuildingKind::Church) {
            assert!(b.centre.distance(yard.centre) > 12.0, "{b:?} is in the churchyard");
        }
    }

    #[test]
    fn a_village_with_no_road_gets_a_street_of_its_own() {
        let (map, poi) = flat_site(70.0, &[]);
        let layout = Layout::generate(&map, &poi, &[]);
        assert!(layout.buildings.len() > 10, "{}", layout.buildings.len());
    }

    #[test]
    fn nothing_is_built_in_water_or_on_a_steep_slope() {
        // On a real map: every building of every village has dry, level ground.
        let params = GenParams::default();
        let map = TerrainMap::generate(crate::MAP_SEED, &params);
        let roads = crate::roads::RoadNetwork::generate(&map, &params);
        let plan = SettlementPlan::generate(&map, &roads);
        let mut total = 0;
        for (poi, layout) in map.pois.iter().zip(&plan.layouts) {
            for b in &layout.buildings {
                total += 1;
                let points = footprint_points(&b.shape());
                assert!(points.iter().all(|&p| map.water_surface_at(p).is_none()), "{poi:?} {b:?} in water");
                let heights: Vec<f32> = points.iter().map(|&p| map.height_at(p)).collect();
                let range = heights.iter().cloned().fold(f32::MIN, f32::max) - heights.iter().cloned().fold(f32::MAX, f32::min);
                assert!(range <= MAX_FOOTPRINT_RANGE + 1e-3, "{b:?} on a {range} m slope");
            }
        }
        assert!(total > 500, "only {total} buildings on the whole map");
    }

    #[test]
    fn on_a_real_map_nothing_is_on_a_road_or_on_another_building() {
        let params = GenParams::default();
        let map = TerrainMap::generate(crate::MAP_SEED, &params);
        let roads = crate::roads::RoadNetwork::generate(&map, &params);
        let plan = SettlementPlan::generate(&map, &roads);
        let mut streets = road_ribbons(&map, &roads);
        streets.extend(plan.lanes());
        let clearance = RoadClearance::new(&streets);
        let (mut on_road, mut overlapping, mut total) = (0, 0, 0);
        for layout in &plan.layouts {
            for (i, b) in layout.buildings.iter().enumerate() {
                total += 1;
                let points = footprint_points(&b.shape());
                if points.iter().any(|&p| clearance.clearance(p) <= 0.0) {
                    on_road += 1;
                }
                if layout.buildings.iter().enumerate().any(|(j, o)| j != i && points.iter().any(|&p| o.shape().separation(p).0 < 0.0)) {
                    overlapping += 1;
                }
            }
        }
        eprintln!("{total} buildings: {on_road} on a road, {overlapping} overlapping another");
        assert_eq!((on_road, overlapping), (0, 0));
    }

    #[test]
    fn the_same_map_always_gives_the_same_plan() {
        let params = GenParams::default();
        let map = TerrainMap::generate(crate::MAP_SEED, &params);
        let roads = crate::roads::RoadNetwork::generate(&map, &params);
        let (a, b) = (SettlementPlan::generate(&map, &roads), SettlementPlan::generate(&map, &roads));
        let count = |p: &SettlementPlan| p.layouts.iter().map(|l| (l.buildings.len(), l.lanes.len())).collect::<Vec<_>>();
        assert_eq!(count(&a), count(&b));
    }

    /// A diagnostic: what each settlement got, in streets and buildings.
    ///   cargo test --lib what_does_each_settlement_get -- --ignored --nocapture
    #[test]
    #[ignore]
    fn what_does_each_settlement_get() {
        let params = GenParams::default();
        let map = TerrainMap::generate(crate::MAP_SEED, &params);
        let roads = crate::roads::RoadNetwork::generate(&map, &params);
        let ribbons = road_ribbons(&map, &roads);
        let plan = SettlementPlan::generate(&map, &roads);
        println!("{:>8} {:>7} {:>6} {:>8} {:>10} {:>10} {:>9}", "radius", "mains", "lanes", "lane m", "main m in", "buildings", "per 100m");
        let mut rows: Vec<_> = map.pois.iter().zip(&plan.layouts).filter(|(p, _)| p.kind == PoiKind::Village).collect();
        rows.sort_by(|a, b| b.0.radius.total_cmp(&a.0.radius));
        for (poi, layout) in rows {
            let mains: Vec<&RoadRibbon> = ribbons.iter().filter(|r| matches!(r.kind, RoadKind::Major | RoadKind::Minor) && r.points.iter().any(|p| p.distance(poi.position) < poi.radius + 20.0)).collect();
            let inside = |r: &RoadRibbon| r.points.windows(2).filter(|w| w[0].distance(poi.position) < poi.radius * 0.95).map(|w| w[0].distance(w[1])).sum::<f32>();
            let main_m: f32 = mains.iter().map(|r| inside(r)).sum();
            let lane_m: f32 = layout.lanes.iter().map(|r| inside(r)).sum();
            let frontage = main_m + lane_m;
            let mut why: Vec<_> = layout.rejected.iter().collect();
            why.sort_by(|a, b| b.1.cmp(a.1));
            println!("{:>8.0} {:>7} {:>6} {:>8.0} {:>10.0} {:>10} {:>9.1}   {:?}", poi.radius, mains.len(), layout.lanes.len(), lane_m, main_m, layout.buildings.len(), layout.buildings.len() as f32 / (frontage / 100.0).max(0.01), why);
        }
    }

    /// A diagnostic: writes the layout of the settlement of a given rank (largest first) as JSON, to
    /// draw. `SETTLEMENT_DUMP` is where to write, and `SETTLEMENT_RANK` which one (default 0).
    ///   SETTLEMENT_DUMP=/tmp/town.json cargo test --lib dump_a_settlement -- --ignored --nocapture
    #[test]
    #[ignore]
    fn dump_a_settlement() {
        let Ok(path) = std::env::var("SETTLEMENT_DUMP") else { return };
        let rank: usize = std::env::var("SETTLEMENT_RANK").ok().and_then(|r| r.parse().ok()).unwrap_or(0);
        let params = GenParams::default();
        let map = TerrainMap::generate(crate::MAP_SEED, &params);
        let roads = crate::roads::RoadNetwork::generate(&map, &params);
        let ribbons = road_ribbons(&map, &roads);
        let plan = SettlementPlan::generate(&map, &roads);
        let mut rows: Vec<_> = map.pois.iter().zip(&plan.layouts).filter(|(p, _)| p.kind == PoiKind::Village).collect();
        rows.sort_by(|a, b| b.0.radius.total_cmp(&a.0.radius));
        let (poi, layout) = rows[rank];
        let near = |p: &Vec2| p.distance(poi.position) < poi.radius * 1.4;
        let mut out = format!("{{\"centre\":[{},{}],\"radius\":{},\"landmark\":[{},{}],", poi.position.x, poi.position.y, poi.radius, poi.landmark.x, poi.landmark.y);
        let ribbon_json = |r: &RoadRibbon| {
            let pts: Vec<String> = r.points.iter().filter(|p| near(p)).map(|p| format!("[{:.1},{:.1}]", p.x, p.y)).collect();
            format!("{{\"kind\":\"{:?}\",\"hw\":{},\"points\":[{}]}}", r.kind, r.half_widths[0], pts.join(","))
        };
        let all: Vec<String> = ribbons.iter().filter(|r| r.points.iter().any(near)).map(ribbon_json).chain(layout.lanes.iter().map(ribbon_json)).collect();
        out += &format!("\"roads\":[{}],", all.join(","));
        let b: Vec<String> = layout
            .buildings
            .iter()
            .map(|b| format!("{{\"kind\":\"{:?}\",\"c\":[{:.1},{:.1}],\"yaw\":{:.3},\"w\":{:.1},\"d\":{:.1}}}", b.kind, b.centre.x, b.centre.y, b.yaw, b.width, b.depth))
            .collect();
        out += &format!("\"buildings\":[{}],", b.join(","));
        let y: Vec<String> = layout.yards.iter().map(|y| format!("[{:.1},{:.1},{:.1}]", y.centre.x, y.centre.y, y.radius)).collect();
        out += &format!("\"yards\":[{}]}}", y.join(","));
        std::fs::write(&path, out).expect("write");
    }
}

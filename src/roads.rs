use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::map::{grid_pos, PoiKind, TerrainMap, CELL, HALF_SIZE};
use crate::params::GenParams;
use crate::road_material::{RoadExtension, RoadMaterial};
use crate::terrain::TerrainTextures;

const ROAD_LIFT: f32 = 0.1;
/// No two points of a road are further apart than this, so that the road follows the ground closely and does not
/// have to stand clear of it by much to keep it from coming up through.
const MAX_POINT_GAP: f32 = 1.0;
/// How many rings a junction's patch is made of, from its middle out, and the most metres between points round its edge.
const PATCH_RINGS: u32 = 3;
const PATCH_STEP: f32 = 2.5;
/// A junction's patch lies this much above the roads that meet there, so that it is what is seen where they overlap.
const PATCH_RAISE: f32 = 0.012;
/// The patch of a junction reaches no further than this many road half widths (and a few metres) along a road,
/// where the roads meeting leave a long thin wedge between them.
const PATCH_REACH: f32 = 3.0;
/// The corner between two roads that meet is rounded with a curve this many times the narrower road's half width
/// in radius, and no less or more than this many metres.
const FILLET_PER_WIDTH: f32 = 1.6;
const FILLET_MIN: f32 = 1.0;
const FILLET_MAX: f32 = 6.0;
/// How far along each road, metres, its direction at a junction is taken from.
const ARM_SPAN: f32 = 3.0;
/// The roads at a junction that are this near to straight on from one another (degrees) need no patch.
const STRAIGHT_ON: f32 = 5.0;
/// How far from a road vertex, along the road and across it, the ground is looked at for its highest: a crease
/// between two vertices does not come up through the road.
const REACH_ALONG: f32 = 0.5;
const REACH_ACROSS: f32 = 0.3;
/// How many points a road's mesh has across its width.
const ACROSS: usize = 9;
/// How far below its edge a road reaches: the face down its side.
const ROAD_THICKNESS: f32 = 0.7;
/// Farm tracks lie a hair below main roads, so that where they join, the main road wins.
const MINOR_SINK: f32 = 0.02;
const MAJOR_HALF_WIDTH: f32 = 4.0;
const MINOR_HALF_WIDTH: f32 = 2.2;
pub const LANE_HALF_WIDTH: f32 = 2.4;
pub const PATH_HALF_WIDTH: f32 = 0.7;
pub const PAVEMENT_HALF_WIDTH: f32 = 0.9;
pub const WALKWAY_HALF_WIDTH: f32 = 0.5;
/// How far a pavement stands above the road beside it: the kerb.
const KERB: f32 = 0.1;
/// The marked lines stop this far short of the end of a stretch of road that finishes at a
/// junction: a centre line doesn't run through a junction, and nor does an edge line cross the
/// mouth of a side road.
const JUNCTION_CLEAR: f32 = 8.0;
/// The edge line stops this far (beyond a side road's own half width) from where it joins.
const MOUTH_MARGIN: f32 = 1.5;
/// How many metres of road one repeat of the surface texture covers.
const ASPHALT_TILE: f32 = 3.0;
const TRACK_TILE: f32 = 2.5;
const PATH_TILE: f32 = 2.0;

// Major roads (between villages and the mill) tolerate more climbing and wider water
// crossings than minor roads (farm tracks), which stick closer to flat, dry ground.
const MAJOR_SLOPE_PENALTY: f32 = 4.0;
const MAJOR_WATER_COST: f32 = 3.0;
const MINOR_SLOPE_PENALTY: f32 = 9.0;
const MINOR_WATER_COST: f32 = 6.0;
const EXISTING_ROAD_DISCOUNT: f32 = 0.12;
const PATH_SLOPE_PENALTY: f32 = 5.0;
const PATH_WATER_COST: f32 = 4.0;
/// How much dearer it is for a footpath to follow a road than to run beside it across the fields.
const ON_THE_ROAD_PENALTY: f32 = 3.0;
/// Footpaths join each settlement to its nearest neighbours up to this far away, and each farm to the
/// nearest settlement within the second distance.
const PATH_LINK_RANGE: f32 = 1600.0;
const PATH_SECOND_LINK_RANGE: f32 = 900.0;
const FARM_PATH_RANGE: f32 = 1500.0;
const FARM_NEIGHBOUR_RANGE: f32 = 450.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RoadKind {
    /// Between villages: tarmac, with the white lines.
    Major,
    /// A farm track: gravel.
    Minor,
    /// A street inside a village or town: narrower tarmac, unmarked.
    Lane,
    /// A footpath: packed earth, a person wide.
    Path,
    /// A pavement beside a street in a settlement: flagstones, with a kerb up from the road.
    Pavement,
    /// A paved path from a building's door out to the pavement.
    Walkway,
}

#[derive(Resource)]
pub struct RoadNetwork {
    cells: Vec<Option<RoadKind>>,
    edges: Vec<(usize, usize, RoadKind)>,
    verts: usize,
    /// Streets laid out by something other than the grid search (a village's lanes), kept as they are.
    extra: Vec<RoadRibbon>,
}

impl RoadNetwork {
    pub fn generate(map: &TerrainMap, params: &GenParams) -> Self {
        let n = map.grid_size();
        let mut cells: Vec<Option<RoadKind>> = vec![None; n * n];
        let mut edges: Vec<(usize, usize, RoadKind)> = Vec::new();

        let hubs: Vec<usize> = map
            .pois
            .iter()
            .filter(|p| matches!(p.kind, PoiKind::Village | PoiKind::Mill))
            .map(|p| nearest_cell(map, p.position))
            .collect();
        let farms: Vec<usize> = map
            .pois
            .iter()
            .filter(|p| p.kind == PoiKind::Farm)
            .map(|p| nearest_cell(map, p.position))
            .collect();

        if let Some((&first, rest)) = hubs.split_first() {
            grow_network(map, vec![first], rest.to_vec(), RoadKind::Major, params, &mut cells, &[], &mut edges);
        }

        let seeds: Vec<usize> = hubs
            .iter()
            .copied()
            .chain((0..n * n).filter(|&i| cells[i].is_some()))
            .collect();
        grow_network(map, seeds, farms, RoadKind::Minor, params, &mut cells, &[], &mut edges);

        lay_footpaths(map, params, &mut cells, &mut edges);

        RoadNetwork { cells, edges, verts: n, extra: Vec::new() }
    }

    /// The road on a grid cell, if there is one. Footpaths are not counted: they run across fields
    /// and don't clear a path through them.
    pub fn kind_at(&self, ix: usize, iz: usize) -> Option<RoadKind> {
        self.cells[iz * self.verts + ix].filter(|&kind| kind != RoadKind::Path)
    }

    /// Adds streets that were laid out directly (a settlement's lanes), so that they are built,
    /// kept clear of, and avoided like any other road.
    pub fn add_ribbons(&mut self, ribbons: impl IntoIterator<Item = RoadRibbon>) {
        self.extra.extend(ribbons);
    }

    pub fn length(&self, kind: RoadKind) -> f32 {
        self.cells.iter().filter(|c| **c == Some(kind)).count() as f32 * CELL
    }
}

/// Footpaths: from each village, hamlet and mill to the nearest others, and from each farm to the
/// nearest settlement (and to a close neighbour). They start and end on a road at the settlement, so
/// they come off it the way a public footpath does, and they cross the fields on their own line
/// instead of following the tarmac. Their cells are not recorded in `cells`, so a path never clears
/// ground the way a road does.
fn lay_footpaths(map: &TerrainMap, params: &GenParams, cells: &mut [Option<RoadKind>], edges: &mut Vec<(usize, usize, RoadKind)>) {
    let n = map.grid_size();
    // Where a settlement's footpaths start: the nearest road cell inside it, or its middle.
    let gate = |poi: &crate::map::Poi| -> usize {
        let centre = nearest_cell(map, poi.position);
        let reach = (poi.radius * 1.2 / CELL).ceil() as isize;
        let (cx, cz) = ((centre % n) as isize, (centre / n) as isize);
        let mut best: Option<(f32, usize)> = None;
        for dz in -reach..=reach {
            for dx in -reach..=reach {
                let (x, z) = (cx + dx, cz + dz);
                if !(0..n as isize).contains(&x) || !(0..n as isize).contains(&z) {
                    continue;
                }
                let idx = z as usize * n + x as usize;
                let d = ((dx * dx + dz * dz) as f32).sqrt() * CELL;
                if cells[idx].is_some() && d <= poi.radius * 1.2 && best.is_none_or(|(b, _)| d < b) {
                    best = Some((d, idx));
                }
            }
        }
        best.map_or(centre, |(_, idx)| idx)
    };
    let places: Vec<(&crate::map::Poi, usize)> = map.pois.iter().map(|p| (p, gate(p))).collect();
    let is_town = |k: PoiKind| matches!(k, PoiKind::Village | PoiKind::Mill);

    let mut links: Vec<(f32, usize, usize)> = Vec::new();
    let mut linked: HashSet<(usize, usize)> = HashSet::new();
    let mut link = |a: usize, b: usize, links: &mut Vec<(f32, usize, usize)>| {
        if a != b && linked.insert((a.min(b), a.max(b))) {
            links.push((places[a].0.position.distance(places[b].0.position), a, b));
        }
    };
    let distance = |a: usize, b: usize| places[a].0.position.distance(places[b].0.position);
    for (i, (poi, _)) in places.iter().enumerate() {
        let mut others: Vec<(f32, usize)> = places
            .iter()
            .enumerate()
            .filter(|&(j, (other, _))| j != i && is_town(other.kind) == is_town(poi.kind) && (!is_town(poi.kind) || is_town(other.kind)))
            .map(|(j, _)| (distance(i, j), j))
            .collect();
        others.sort_by(|a, b| a.0.total_cmp(&b.0));
        if is_town(poi.kind) {
            if let Some(&(d, j)) = others.first().filter(|(d, _)| *d < PATH_LINK_RANGE) {
                let _ = d;
                link(i, j, &mut links);
            }
            if let Some(&(_, j)) = others.get(1).filter(|(d, _)| *d < PATH_SECOND_LINK_RANGE) {
                link(i, j, &mut links);
            }
        } else if poi.kind == PoiKind::Farm {
            let nearest_town = places
                .iter()
                .enumerate()
                .filter(|(_, (other, _))| is_town(other.kind) && other.kind == PoiKind::Village)
                .map(|(j, _)| (distance(i, j), j))
                .min_by(|a, b| a.0.total_cmp(&b.0));
            if let Some((_, j)) = nearest_town.filter(|(d, _)| *d < FARM_PATH_RANGE) {
                link(i, j, &mut links);
            }
            if let Some(&(_, j)) = others.first().filter(|(d, _)| *d < FARM_NEIGHBOUR_RANGE) {
                link(i, j, &mut links);
            }
        }
    }
    // Short links first: they make the trunk the longer ones then join.
    links.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut path_cells: Vec<Option<RoadKind>> = vec![None; n * n];
    let mut drawn: HashSet<(usize, usize)> = HashSet::new();
    for (_, a, b) in links {
        let (from, to) = (places[a].1, places[b].1);
        if from != to {
            let Some((reached, previous)) = dijkstra_to_any(map, &[from], &HashSet::from([to]), RoadKind::Path, params, &path_cells, cells) else {
                continue;
            };
            // Walk back to where it started, recording the steps that no earlier path has already taken.
            let mut current = reached;
            while let Some(p) = previous[current] {
                if drawn.insert((current.min(p), current.max(p))) {
                    edges.push((current, p, RoadKind::Path));
                }
                path_cells[current] = Some(RoadKind::Path);
                current = p;
            }
            path_cells[current] = Some(RoadKind::Path);
        }
    }
    // A footpath stops at the edge of a farm's land: it doesn't go through the yard.
    let farms: Vec<(Vec2, f32)> = map.pois.iter().filter(|p| p.kind == PoiKind::Farm).map(|p| (p.position, p.radius * 0.95)).collect();
    let inside_a_farm = |idx: usize| {
        let at = grid_pos(idx % n, idx / n);
        farms.iter().any(|&(c, r)| c.distance(at) < r)
    };
    edges.retain(|&(a, b, kind)| kind != RoadKind::Path || !(inside_a_farm(a) || inside_a_farm(b)));
}

// Grows the network by repeatedly finding, with a single Dijkstra search seeded from
// everything already connected, the cheapest path to the nearest still-unconnected
// target. The search stops as soon as it reaches any target, so each step costs roughly
// the distance to the nearest unconnected settlement rather than the whole grid.
fn grow_network(
    map: &TerrainMap,
    seeds: Vec<usize>,
    targets: Vec<usize>,
    kind: RoadKind,
    params: &GenParams,
    cells: &mut [Option<RoadKind>],
    avoid: &[Option<RoadKind>],
    edges: &mut Vec<(usize, usize, RoadKind)>,
) {
    let mut connected = seeds;
    let mut remaining: HashSet<usize> = targets.into_iter().collect();

    while !remaining.is_empty() {
        let Some((reached, previous)) = dijkstra_to_any(map, &connected, &remaining, kind, params, cells, avoid) else {
            break;
        };
        // Walk back from the newly reached settlement toward the network, recording each
        // step as an edge. Stop as soon as a cell that was already part of the network is
        // hit, since the rest of the path back to its root was already recorded earlier -
        // without this, every later connection would re-walk and re-record the whole shared
        // corridor back to the root, multiplying the ribbon geometry.
        let mut current = reached;
        loop {
            if cells[current].is_some() {
                break;
            }
            cells[current] = Some(kind);
            match previous[current] {
                Some(p) => {
                    edges.push((current, p, kind));
                    current = p;
                }
                None => break,
            }
        }
        remaining.remove(&reached);
        connected.push(reached);
    }
}

fn dijkstra_to_any(
    map: &TerrainMap,
    sources: &[usize],
    targets: &HashSet<usize>,
    kind: RoadKind,
    params: &GenParams,
    cells: &[Option<RoadKind>],
    avoid: &[Option<RoadKind>],
) -> Option<(usize, Vec<Option<usize>>)> {
    let n = map.grid_size();
    let count = n * n;
    let mut cost = vec![f32::MAX; count];
    let mut previous: Vec<Option<usize>> = vec![None; count];
    let mut heap = BinaryHeap::new();
    for &s in sources {
        if cost[s] > 0.0 {
            cost[s] = 0.0;
            heap.push(Reverse((0u32, s as u32)));
        }
    }

    while let Some(Reverse((_, current))) = heap.pop() {
        let c = current as usize;
        if targets.contains(&c) {
            return Some((c, previous));
        }
        for neighbour in neighbours(n, c) {
            let step_cost = step_cost(map, cells, avoid, c, neighbour, kind, params);
            let next = cost[c] + step_cost;
            if next < cost[neighbour] {
                cost[neighbour] = next;
                previous[neighbour] = Some(c);
                heap.push(Reverse((next.to_bits(), neighbour as u32)));
            }
        }
    }
    None
}

fn step_cost(map: &TerrainMap, cells: &[Option<RoadKind>], avoid: &[Option<RoadKind>], from: usize, to: usize, kind: RoadKind, params: &GenParams) -> f32 {
    let n = map.grid_size();
    let diagonal = (to % n != from % n) && (to / n != from / n);
    let step = if diagonal { CELL * std::f32::consts::SQRT_2 } else { CELL };
    let (fx, fz) = (from % n, from / n);
    let (tx, tz) = (to % n, to / n);
    let climb = (map.vertex_height(tx, tz) - map.vertex_height(fx, fz)).abs() / CELL;
    let (slope_penalty, water_cost) = match kind {
        RoadKind::Major => (MAJOR_SLOPE_PENALTY, MAJOR_WATER_COST),
        RoadKind::Minor | RoadKind::Lane | RoadKind::Pavement | RoadKind::Walkway => (MINOR_SLOPE_PENALTY, MINOR_WATER_COST),
        // A walker puts up with more slope than a car, and will wade a stream sooner than go round.
        RoadKind::Path => (PATH_SLOPE_PENALTY, PATH_WATER_COST),
    };
    let slope_penalty = slope_penalty * params.road_slope_scale;
    let water_cost = water_cost * params.road_water_scale;
    let mut c = step * (1.0 + slope_penalty * climb);
    if map.water_level(tx, tz).is_some() {
        c += step * water_cost;
    }
    if cells[to].is_some() {
        c *= EXISTING_ROAD_DISCOUNT;
    } else if avoid.get(to).is_some_and(Option::is_some) {
        // A walker keeps off a road they have no need to be on.
        c *= ON_THE_ROAD_PENALTY;
    }
    c
}

fn neighbours(n: usize, idx: usize) -> impl Iterator<Item = usize> {
    let (ix, iz) = ((idx % n) as isize, (idx / n) as isize);
    (-1..=1isize)
        .flat_map(move |dz| (-1..=1isize).map(move |dx| (dx, dz)))
        .filter(|&(dx, dz)| (dx, dz) != (0, 0))
        .filter_map(move |(dx, dz)| {
            let (x, z) = (ix + dx, iz + dz);
            let inside = (0..n as isize).contains(&x) && (0..n as isize).contains(&z);
            inside.then(|| z as usize * n + x as usize)
        })
}

fn nearest_cell(map: &TerrainMap, p: Vec2) -> usize {
    let n = map.grid_size();
    let ix = ((p.x + HALF_SIZE) / CELL).round().clamp(0.0, (n - 1) as f32) as usize;
    let iz = ((p.y + HALF_SIZE) / CELL).round().clamp(0.0, (n - 1) as f32) as usize;
    iz * n + ix
}

pub fn half_width(kind: RoadKind) -> f32 {
    match kind {
        RoadKind::Major => MAJOR_HALF_WIDTH,
        RoadKind::Minor => MINOR_HALF_WIDTH,
        RoadKind::Lane => LANE_HALF_WIDTH,
        RoadKind::Path => PATH_HALF_WIDTH,
        RoadKind::Pavement => PAVEMENT_HALF_WIDTH,
        RoadKind::Walkway => WALKWAY_HALF_WIDTH,
    }
}

/// One stretch of road, laid out: its smooth centreline and, at each point, how far the road
/// reaches to either side. Both the road's mesh and the clearance kept around it come from this,
/// so they always agree.
#[derive(Clone, Debug)]
pub struct RoadRibbon {
    pub kind: RoadKind,
    pub points: Vec<Vec2>,
    pub half_widths: Vec<f32>,
    /// Whether the stretch begins / ends at a junction (as opposed to a dead end).
    pub start_junction: bool,
    pub end_junction: bool,
}

// Road edges form a tree of grid-cell steps. They're first chained into polylines that
// break wherever roads meet or end, and each polyline is relaxed and corner-cut (Chaikin) into a
// smooth curve. The road then reaches `half_width` to either side, a little more on bends
// so that it keeps its width.
pub fn road_ribbons(map: &TerrainMap, roads: &RoadNetwork) -> Vec<RoadRibbon> {
    let mut ribbons = Vec::new();
    for kind in [RoadKind::Major, RoadKind::Minor, RoadKind::Path] {
        for chain in road_chains(roads, kind) {
            let points = smooth_polyline(chain.cells.iter().map(|&c| idx_to_pos(map, c)).collect());
            if points.len() < 2 {
                continue;
            }
            let half_widths = (0..points.len())
                .map(|i| {
                    let (p, prev, next) = (points[i], points[i.saturating_sub(1)], points[(i + 1).min(points.len() - 1)]);
                    // Widen a little on bends so the ribbon keeps its width, capped so a sharp
                    // kink can't spike.
                    let bend = if i > 0 && i + 1 < points.len() {
                        let (d0, d1) = ((p - prev).normalize_or_zero(), (next - p).normalize_or_zero());
                        (1.0 / ((1.0 + d0.dot(d1)) * 0.5).max(0.01).sqrt()).min(1.5)
                    } else {
                        1.0
                    };
                    half_width(kind) * bend
                })
                .collect();
            ribbons.push(RoadRibbon { kind, points, half_widths, start_junction: chain.start_junction, end_junction: chain.end_junction });
        }
    }
    ribbons.extend(roads.extra.iter().cloned());
    ribbons
}

/// `ribbon` with points added along any stretch longer than `MAX_POINT_GAP`, evenly, and the width interpolated.
/// For making the road's mesh: nothing that plans along a road needs the points so close.
fn refined(ribbon: RoadRibbon) -> RoadRibbon {
    let mut points = Vec::with_capacity(ribbon.points.len());
    let mut half_widths = Vec::with_capacity(ribbon.points.len());
    for i in 0..ribbon.points.len() {
        points.push(ribbon.points[i]);
        half_widths.push(ribbon.half_widths[i]);
        if let Some(&next) = ribbon.points.get(i + 1) {
            let pieces = (ribbon.points[i].distance(next) / MAX_POINT_GAP).ceil().max(1.0) as usize;
            for k in 1..pieces {
                let t = k as f32 / pieces as f32;
                points.push(ribbon.points[i].lerp(next, t));
                half_widths.push(ribbon.half_widths[i] + (ribbon.half_widths[i + 1] - ribbon.half_widths[i]) * t);
            }
        }
    }
    RoadRibbon { points, half_widths, ..ribbon }
}

/// The vertex colour of a road vertex, which carries what the road shader needs besides position:
/// r is 1 where lane markings are painted and 0 where they aren't (near junctions), g the road's
/// half width at the vertex in tens of metres, b is the surface (0 tarmac, 0.5 footpath, 1 farm track), and a is
/// 1 where the edge line runs and 0 where it is broken for the mouth of a side road.
pub fn road_vertex_data(kind: RoadKind, painted: bool, edge_line: bool, half_width: f32) -> [f32; 4] {
    [
        if kind == RoadKind::Major && painted { 1.0 } else { 0.0 },
        half_width / 10.0,
        // What the surface is: 0 tarmac, 0.1 paving, 0.5 a footpath, 1 a farm track.
        match kind {
            RoadKind::Minor => 1.0,
            RoadKind::Path => 0.5,
            RoadKind::Pavement | RoadKind::Walkway => 0.1,
            _ => 0.0,
        },
        if edge_line { 1.0 } else { 0.0 },
    ]
}

/// Where roads meet: the end of every stretch that finishes at a junction, and how far the edge
/// line of the road it runs into should stay broken either side (the side road's half width and
/// a margin). The edge line of a through road does not cross the mouth of a side road.
fn junction_mouths(ribbons: &[RoadRibbon]) -> Vec<(Vec2, f32)> {
    let mut mouths = Vec::new();
    for ribbon in ribbons {
        if ribbon.start_junction {
            mouths.push((ribbon.points[0], ribbon.half_widths[0] + MOUTH_MARGIN));
        }
        if ribbon.end_junction {
            let last = ribbon.points.len() - 1;
            mouths.push((ribbon.points[last], ribbon.half_widths[last] + MOUTH_MARGIN));
        }
    }
    mouths
}

/// The surface that fills the space where roads meet at an angle. Roads end flat at the junction, so on the
/// outside of a bend there is a gap between them, and where two meet at a shallow angle the corner of one
/// sticks out past the edge of the other. The patch is cut to the edges of the roads that meet: it takes in the
/// crotch between neighbouring roads, and rounds off the outside of a bend.
struct Patch {
    kind: RoadKind,
    centre: Vec2,
    half_width: f32,
    /// Counter-clockwise round the centre.
    outline: Vec<Vec2>,
    /// Whether the edge from each point of the outline to the next is open to be seen, so that it has a face
    /// down it (the others lie along the edge of a road, which has its own).
    wall: Vec<bool>,
}

/// One road leaving a junction.
struct Arm {
    kind: RoadKind,
    /// The way it leaves, a unit vector, and the angle of that.
    out: Vec2,
    angle: f32,
    half_width: f32,
}

fn left(v: Vec2) -> Vec2 {
    Vec2::new(-v.y, v.x)
}

fn cross(a: Vec2, b: Vec2) -> f32 {
    a.x * b.y - a.y * b.x
}

/// Where the left edge of `a` meets the right edge of `b`, which is the next arm round: how far along each (negative
/// if it is behind the junction).
fn crotch(a: &Arm, b: &Arm) -> Option<(f32, f32)> {
    let v = -left(b.out) * b.half_width - left(a.out) * a.half_width;
    let det = cross(a.out, b.out);
    if det.abs() < 1e-3 {
        return None;
    }
    // Behind the junction (negative), where a wide road meets a narrow one at a wide angle: the edges still cross.
    Some((cross(v, b.out) / det, -cross(a.out, v) / det))
}

fn junction_patches(ribbons: &[RoadRibbon]) -> Vec<Patch> {
    // Every end of a road that finishes at a junction, grouped by where it is.
    let mut groups: HashMap<(i32, i32), (Vec2, Vec<Arm>)> = HashMap::new();
    for ribbon in ribbons {
        let n = ribbon.points.len();
        if n < 2 {
            continue;
        }
        let ends = [(ribbon.start_junction, 0usize, true), (ribbon.end_junction, n - 1, false)];
        for (junction, end, forwards) in ends {
            if !junction {
                continue;
            }
            let at = ribbon.points[end];
            let along: Vec<usize> = if forwards { (1..n).collect() } else { (0..n - 1).rev().collect() };
            let Some(&towards) = along.iter().find(|&&i| ribbon.points[i].distance(at) >= ARM_SPAN).or(along.last()) else { continue };
            let out = (ribbon.points[towards] - at).normalize_or_zero();
            if out == Vec2::ZERO {
                continue;
            }
            let arm = Arm { kind: ribbon.kind, out, angle: out.y.atan2(out.x), half_width: ribbon.half_widths[end] };
            let key = ((at.x * 2.0).round() as i32, (at.y * 2.0).round() as i32);
            groups.entry(key).or_insert_with(|| (at, Vec::new())).1.push(arm);
        }
    }
    let mut patches = Vec::new();
    let mut keys: Vec<(i32, i32)> = groups.keys().copied().collect();
    keys.sort_unstable();
    for key in keys {
        let (centre, mut arms) = groups.remove(&key).unwrap();
        if arms.len() < 2 {
            continue;
        }
        arms.sort_by(|a, b| a.angle.total_cmp(&b.angle));
        if let Some(patch) = patch_for(centre, &arms) {
            patches.push(patch);
        }
    }
    patches
}

fn patch_for(centre: Vec2, arms: &[Arm]) -> Option<Patch> {
    let n = arms.len();
    // The angle from each arm round to the next, counter-clockwise.
    let gaps: Vec<f32> = (0..n).map(|i| (arms[(i + 1) % n].angle - arms[i].angle).rem_euclid(std::f32::consts::TAU)).collect();
    let straight = STRAIGHT_ON.to_radians();
    if n == 2 && (gaps[0] - std::f32::consts::PI).abs() < straight {
        return None;
    }
    // Between each arm and the next: either they cross, and the patch reaches to where their edges do, or they
    // are far enough apart that the outside of the bend between them has to be filled.
    let reach = |a: &Arm| a.half_width * PATCH_REACH + 3.0;
    let mut meets: Vec<Option<(f32, f32)>> = Vec::new();
    for i in 0..n {
        let (a, b) = (&arms[i], &arms[(i + 1) % n]);
        let crossing = gaps[i] < std::f32::consts::PI - straight && gaps[i] > 0.02;
        meets.push(if crossing { crotch(a, b) } else { None });
    }
    // The corner between neighbouring roads that cross is rounded: how far each edge runs on past the crotch to where
    // the curve touches it, and the radius of the curve.
    let mut fillets: Vec<Option<(f32, f32)>> = vec![None; n];
    for i in 0..n {
        if let Some((t, s)) = meets[i].filter(|&(t, s)| t > 0.0 && s > 0.0) {
            let j = (i + 1) % n;
            let half = gaps[i] * 0.5;
            let radius = (FILLET_PER_WIDTH * arms[i].half_width.min(arms[j].half_width)).clamp(FILLET_MIN, FILLET_MAX);
            let run = (radius / half.tan()).min(reach(&arms[i]) - t).min(reach(&arms[j]) - s);
            if run >= 0.3 {
                fillets[i] = Some((run, run * half.tan()));
            }
        }
    }
    // How far along each arm the patch goes: to the crotch on either side of it, and past it to the curve.
    let mut along = vec![0.0f32; n];
    for i in 0..n {
        if let Some((t, s)) = meets[i] {
            let run = fillets[i].map_or(0.0, |f| f.0);
            along[i] = along[i].max((t.max(0.0) + run).min(reach(&arms[i])));
            let j = (i + 1) % n;
            along[j] = along[j].max((s.max(0.0) + run).min(reach(&arms[j])));
        }
    }
    let edge = |i: usize, d: f32, side: f32| centre + arms[i].out * d + left(arms[i].out) * (side * arms[i].half_width);
    let mut points: Vec<(Vec2, bool)> = Vec::new();
    for i in 0..n {
        let j = (i + 1) % n;
        // Out along the right edge of the arm, across its end, and back along its left edge.
        points.push((edge(i, along[i], -1.0), false));
        points.push((edge(i, along[i], 1.0), false));
        match meets[i] {
            // The crotch between this arm and the next, rounded if there is room.
            Some((t, s)) if t <= reach(&arms[i]) && s <= reach(&arms[j]) => match fillets[i] {
                Some((run, radius)) => {
                    let x = edge(i, t, 1.0);
                    let (to_i, to_j) = (x + arms[i].out * run, x + arms[j].out * run);
                    let middle = (arms[i].out + arms[j].out).normalize_or_zero();
                    let centre_of_curve = x + middle * (radius / (gaps[i] * 0.5).sin());
                    let (a, b) = ((to_i - centre_of_curve).to_angle(), (to_j - centre_of_curve).to_angle());
                    let turn = (b - a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
                    points.push((to_i, true));
                    let steps = 6;
                    for k in 1..steps {
                        points.push((centre_of_curve + Vec2::from_angle(a + turn * k as f32 / steps as f32) * radius, true));
                    }
                    points.push((to_j, false));
                }
                // Behind the junction on one of the roads: the corner of its end, and the crossing on the other.
                None => {
                    points.push((edge(i, t.max(0.0), 1.0), false));
                    points.push((edge(j, s.max(0.0), -1.0), false));
                }
            },
            // The edges do not meet within reach: the wedge between them is closed off across, open to be seen.
            Some(_) => {
                if let Some(last) = points.last_mut() {
                    last.1 = true;
                }
            }
            // The outside of a bend: round, from the corner of this arm's end to the corner of the next's.
            None if gaps[i] > std::f32::consts::PI + straight => {
                points.push((edge(i, 0.0, 1.0), true));
                let from = arms[i].angle + std::f32::consts::FRAC_PI_2;
                let to = arms[j].angle - std::f32::consts::FRAC_PI_2;
                let to = if to <= from { to + std::f32::consts::TAU } else { to };
                let steps = 6;
                for k in 1..steps {
                    let f = k as f32 / steps as f32;
                    let radius = arms[i].half_width + (arms[j].half_width - arms[i].half_width) * f;
                    points.push((centre + Vec2::from_angle(from + (to - from) * f) * radius, true));
                }
                points.push((edge(j, 0.0, -1.0), false));
            }
            None => {}
        }
    }
    let (outline, wall): (Vec<Vec2>, Vec<bool>) = points.into_iter().unzip();
    // The widest of the roads sets the surface; the one that lies highest is the one that shows.
    let top = arms.iter().min_by(|a, b| (kind_rank(a.kind)).cmp(&kind_rank(b.kind)))?;
    Some(Patch { kind: top.kind, centre, half_width: arms.iter().map(|a| a.half_width).fold(0.0, f32::max), outline, wall })
}

/// Which of two roads is on top, where they cross: lower is higher.
fn kind_rank(kind: RoadKind) -> u8 {
    match kind {
        RoadKind::Major => 0,
        RoadKind::Lane => 1,
        RoadKind::Minor => 2,
        RoadKind::Path => 3,
        RoadKind::Pavement => 4,
        RoadKind::Walkway => 5,
    }
}

// A ribbon of the given kind, with vertex heights from the smooth bilinear terrain rather than
// the grid vertex, raised above any water for bridges. UV 0 tiles the surface texture in world
// metres; UV 1 is (metres across from the centre line, metres along the road), for the lane
// markings; the normals follow the ground.
/// How far above the ground a road of `kind` lies. Where roads cross, the one that matters more is on top: a lane
/// joins a main road without a seam.
pub fn lift(kind: RoadKind) -> f32 {
    ROAD_LIFT
        - match kind {
            RoadKind::Major => 0.0,
            RoadKind::Lane => MINOR_SINK * 0.5,
            RoadKind::Minor => MINOR_SINK,
            RoadKind::Path => MINOR_SINK * 1.5,
            // A pavement is up a kerb from the road; a walkway a hair under it, so that the pavement wins where they meet.
            RoadKind::Pavement => -KERB,
            RoadKind::Walkway => -KERB + MINOR_SINK * 0.5,
        }
}

pub fn road_mesh(map: &TerrainMap, ribbons: &[RoadRibbon], kind: RoadKind) -> Mesh {
    let tile = match kind {
        RoadKind::Major | RoadKind::Lane | RoadKind::Pavement | RoadKind::Walkway => ASPHALT_TILE,
        RoadKind::Minor => TRACK_TILE,
        RoadKind::Path => PATH_TILE,
    };
    let lift = lift(kind);
    let n = map.grid_size();
    let mouths = junction_mouths(ribbons);
    let (mut positions, mut normals, mut uv0, mut uv1, mut colours, mut indices) = (vec![], vec![], vec![], vec![], vec![], vec![]);
    // Where each ribbon's rows of points start, for the skirts below.
    let mut bases: Vec<(u32, usize)> = Vec::new();
    for (ribbon_index, ribbon) in ribbons.iter().enumerate().filter(|(_, r)| r.kind == kind) {
        let points = &ribbon.points;
        bases.push((positions.len() as u32, ribbon_index));
        let mut along = vec![0.0f32; points.len()];
        for i in 1..points.len() {
            along[i] = along[i - 1] + points[i].distance(points[i - 1]);
        }
        let total = *along.last().unwrap_or(&0.0);
        let base = positions.len() as u32;
        for (i, &p) in points.iter().enumerate() {
            let prev = points[i.saturating_sub(1)];
            let next = points[(i + 1).min(points.len() - 1)];
            let tangent = (next - prev).normalize_or_zero();
            let perp = Vec2::new(-tangent.y, tangent.x);
            let hw = ribbon.half_widths[i];
            let painted = !(ribbon.start_junction && along[i] < JUNCTION_CLEAR || ribbon.end_junction && total - along[i] < JUNCTION_CLEAR);
            // Several points across, each on the terrain mesh itself (not the smooth surface it is cut
            // from), so that nothing underneath pokes through the road between its edges.
            for column in 0..ACROSS {
                let side = column as f32 / (ACROSS - 1) as f32 * 2.0 - 1.0;
                let q = p + perp * hw * side;
                let cell = nearest_cell(map, q);
                // The highest of the mesh at this point and a little way along and across it, so that a crease
                // in the ground between two of the road's points doesn't come up through it.
                let around = [Vec2::ZERO, tangent * REACH_ALONG, -tangent * REACH_ALONG, perp * REACH_ACROSS, -perp * REACH_ACROSS];
                let mesh_height = around.iter().map(|d| map.surface_height_at(q + *d)).fold(f32::MIN, f32::max);
                let ground = mesh_height.max(map.water_level(cell % n, cell / n).unwrap_or(f32::MIN));
                positions.push([q.x, ground + lift, q.y]);
                normals.push(map.normal_at(q).to_array());
                uv0.push([q.x / tile, q.y / tile]);
                uv1.push([side * hw, along[i]]);
                let edge_line = !mouths.iter().any(|&(at, reach)| at.distance(p) < reach);
                colours.push(road_vertex_data(kind, painted, edge_line, hw));
            }
        }
        let across = ACROSS as u32;
        for i in 0..points.len() as u32 - 1 {
            for c in 0..across - 1 {
                let (l0, r0, l1, r1) = (base + i * across + c, base + i * across + c + 1, base + (i + 1) * across + c, base + (i + 1) * across + c + 1);
                indices.extend_from_slice(&[l0, r1, l1, l0, r0, r1]);
            }
        }
    }
    // The road has thickness: a face down each edge, into the ground, so that it doesn't seem to float
    // over it where the ground falls away.
    let cap_bases = bases.clone();
    for (base, ribbon_index) in bases {
        let ribbon = &ribbons[ribbon_index];
        let across = ACROSS as u32;
        for column in [0u32, across - 1] {
            for i in 0..ribbon.points.len() as u32 - 1 {
                let (t0, t1) = ((base + i * across + column) as usize, (base + (i + 1) * across + column) as usize);
                let (p0, p1) = (Vec3::from_array(positions[t0]), Vec3::from_array(positions[t1]));
                let run = Vec3::new(p1.x - p0.x, 0.0, p1.z - p0.z).normalize_or_zero();
                // Facing out from the road: the left edge faces the way the road's perpendicular points away from.
                let outward = Vec3::new(-run.z, 0.0, run.x) * if column == 0 { -1.0 } else { 1.0 };
                let (b0, b1) = (p0 - Vec3::Y * ROAD_THICKNESS, p1 - Vec3::Y * ROAD_THICKNESS);
                let first = positions.len() as u32;
                let hw = ribbon.half_widths[i as usize];
                for (q, along_at) in [(p0, i as usize), (p1, i as usize + 1), (b1, i as usize + 1), (b0, i as usize)] {
                    positions.push(q.to_array());
                    normals.push(outward.to_array());
                    uv0.push([(q.x + q.z) / tile, q.y / tile]);
                    uv1.push([0.0, ribbon.points[..=along_at].windows(2).map(|w| w[0].distance(w[1])).sum::<f32>()]);
                    colours.push(road_vertex_data(kind, false, false, hw));
                }
                // Counter-clockwise seen from outside.
                let face = (p1 - p0).cross(b0 - p0);
                if face.dot(outward) > 0.0 {
                    indices.extend_from_slice(&[first, first + 1, first + 2, first, first + 2, first + 3]);
                } else {
                    indices.extend_from_slice(&[first, first + 2, first + 1, first, first + 3, first + 2]);
                }
            }
        }
    }
    // And a face across each end where the road stops short (not at a junction, where another road covers it), or
    // the road is open underneath there and can be seen through.
    for &(base, ribbon_index) in &cap_bases {
        let ribbon = &ribbons[ribbon_index];
        let across = ACROSS as u32;
        let last = ribbon.points.len() - 1;
        if last == 0 {
            continue;
        }
        let length: f32 = ribbon.points.windows(2).map(|w| w[0].distance(w[1])).sum();
        let ends = [(0usize, ribbon.start_junction, ribbon.points[0] - ribbon.points[1], 0.0), (last, ribbon.end_junction, ribbon.points[last] - ribbon.points[last - 1], length)];
        for (row, junction, away, along_at) in ends {
            if junction {
                continue;
            }
            let outward = Vec3::new(away.x, 0.0, away.y).normalize_or_zero();
            let hw = ribbon.half_widths[row];
            for column in 0..across - 1 {
                let (t0, t1) = ((base + row as u32 * across + column) as usize, (base + row as u32 * across + column + 1) as usize);
                let (p0, p1) = (Vec3::from_array(positions[t0]), Vec3::from_array(positions[t1]));
                let (b0, b1) = (p0 - Vec3::Y * ROAD_THICKNESS, p1 - Vec3::Y * ROAD_THICKNESS);
                let first = positions.len() as u32;
                for q in [p0, p1, b1, b0] {
                    positions.push(q.to_array());
                    normals.push(outward.to_array());
                    uv0.push([(q.x + q.z) / tile, q.y / tile]);
                    uv1.push([0.0, along_at]);
                    colours.push(road_vertex_data(kind, false, false, hw));
                }
                let face = (p1 - p0).cross(b0 - p0);
                if face.dot(outward) > 0.0 {
                    indices.extend_from_slice(&[first, first + 1, first + 2, first, first + 2, first + 3]);
                } else {
                    indices.extend_from_slice(&[first, first + 2, first + 1, first, first + 3, first + 2]);
                }
            }
        }
    }
    // Where roads meet at an angle, a patch over the space between them (see `Patch`).
    for patch in junction_patches(ribbons).iter().filter(|p| p.kind == kind) {
        // The outline, with a point at least every few metres, so that the patch follows the ground.
        let mut outline: Vec<(Vec2, bool)> = Vec::new();
        for k in 0..patch.outline.len() {
            let (a, b) = (patch.outline[k], patch.outline[(k + 1) % patch.outline.len()]);
            let pieces = (a.distance(b) / PATCH_STEP).ceil().max(1.0) as usize;
            for piece in 0..pieces {
                outline.push((a.lerp(b, piece as f32 / pieces as f32), patch.wall[k]));
            }
        }
        let height = |q: Vec2| {
            let around = [Vec2::ZERO, Vec2::X * REACH_ALONG, Vec2::NEG_X * REACH_ALONG, Vec2::Y * REACH_ALONG, Vec2::NEG_Y * REACH_ALONG];
            let mesh_height = around.iter().map(|d| map.surface_height_at(q + *d)).fold(f32::MIN, f32::max);
            let cell = nearest_cell(map, q);
            mesh_height.max(map.water_level(cell % n, cell / n).unwrap_or(f32::MIN)) + lift + PATCH_RAISE
        };
        let m = outline.len() as u32;
        let first = positions.len() as u32;
        let mut vertex = |q: Vec2, positions: &mut Vec<[f32; 3]>| {
            positions.push([q.x, height(q), q.y]);
            normals.push(map.normal_at(q).to_array());
            uv0.push([q.x / tile, q.y / tile]);
            // No lane, no edge and no paint: it is all plain road.
            uv1.push([0.0, 0.0]);
            colours.push(road_vertex_data(kind, false, false, patch.half_width));
        };
        vertex(patch.centre, &mut positions);
        for ring in 1..=PATCH_RINGS {
            for &(p, _) in &outline {
                vertex(patch.centre + (p - patch.centre) * (ring as f32 / PATCH_RINGS as f32), &mut positions);
            }
        }
        let at = |ring: u32, k: u32| first + 1 + (ring - 1) * m + k % m;
        let mut triangles: Vec<[u32; 3]> = Vec::new();
        for k in 0..m {
            triangles.push([first, at(1, k), at(1, k + 1)]);
            for ring in 1..PATCH_RINGS {
                triangles.push([at(ring, k), at(ring + 1, k), at(ring + 1, k + 1)]);
                triangles.push([at(ring, k), at(ring + 1, k + 1), at(ring, k + 1)]);
            }
        }
        // Each wound to face up.
        for [a, b, c] in triangles {
            let v = |i: u32| Vec3::from_array(positions[i as usize]);
            if (v(b) - v(a)).cross(v(c) - v(a)).y >= 0.0 {
                indices.extend_from_slice(&[a, b, c]);
            } else {
                indices.extend_from_slice(&[a, c, b]);
            }
        }
        // A face down each edge that is open to be seen.
        for k in 0..m {
            if !outline[k as usize].1 {
                continue;
            }
            let (p0, p1) = (Vec3::from_array(positions[at(PATCH_RINGS, k) as usize]), Vec3::from_array(positions[at(PATCH_RINGS, k + 1) as usize]));
            let run = Vec3::new(p1.x - p0.x, 0.0, p1.z - p0.z).normalize_or_zero();
            let mut outward = Vec3::new(-run.z, 0.0, run.x);
            if outward.dot((p0 + p1) * 0.5 - Vec3::new(patch.centre.x, 0.0, patch.centre.y)) < 0.0 {
                outward = -outward;
            }
            let (b0, b1) = (p0 - Vec3::Y * ROAD_THICKNESS, p1 - Vec3::Y * ROAD_THICKNESS);
            let face_first = positions.len() as u32;
            for q in [p0, p1, b1, b0] {
                positions.push(q.to_array());
                normals.push(outward.to_array());
                uv0.push([(q.x + q.z) / tile, q.y / tile]);
                uv1.push([0.0, 0.0]);
                colours.push(road_vertex_data(kind, false, false, patch.half_width));
            }
            let face = (p1 - p0).cross(b0 - p0);
            if face.dot(outward) > 0.0 {
                indices.extend_from_slice(&[face_first, face_first + 1, face_first + 2, face_first, face_first + 2, face_first + 3]);
            } else {
                indices.extend_from_slice(&[face_first, face_first + 2, face_first + 1, face_first, face_first + 3, face_first + 2]);
            }
        }
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv0)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, uv1)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colours)
        .with_inserted_indices(Indices::U32(indices))
}

/// A spatial index of the roads' outlines, for keeping other things (walls, hedges) clear of them.
pub struct RoadClearance {
    edges: Vec<ClearEdge>,
    buckets: HashMap<(i32, i32), Vec<u32>>,
}

struct ClearEdge {
    a: Vec2,
    b: Vec2,
    ha: f32,
    hb: f32,
    kind: RoadKind,
}

/// Bucket side, metres. Edges are filed under every bucket within `REACH` of them, so a query
/// only needs to look in its own bucket for anything within `REACH` of the point.
const BUCKET: f32 = 16.0;
const REACH: f32 = 9.0;

impl RoadClearance {
    pub fn new(ribbons: &[RoadRibbon]) -> Self {
        let mut edges = Vec::new();
        let mut buckets: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for ribbon in ribbons {
            for i in 0..ribbon.points.len().saturating_sub(1) {
                let id = edges.len() as u32;
                let edge = ClearEdge { a: ribbon.points[i], b: ribbon.points[i + 1], ha: ribbon.half_widths[i], hb: ribbon.half_widths[i + 1], kind: ribbon.kind };
                let (lo, hi) = (edge.a.min(edge.b) - Vec2::splat(REACH), edge.a.max(edge.b) + Vec2::splat(REACH));
                for bz in (lo.y / BUCKET).floor() as i32..=(hi.y / BUCKET).floor() as i32 {
                    for bx in (lo.x / BUCKET).floor() as i32..=(hi.x / BUCKET).floor() as i32 {
                        buckets.entry((bx, bz)).or_default().push(id);
                    }
                }
                edges.push(edge);
            }
        }
        RoadClearance { edges, buckets }
    }

    /// How far `p` is outside the nearest road's edge, metres: negative if it is on a road, and
    /// `f32::MAX` if no road is near (within about `REACH`).
    pub fn clearance(&self, p: Vec2) -> f32 {
        let Some(ids) = self.buckets.get(&((p.x / BUCKET).floor() as i32, (p.y / BUCKET).floor() as i32)) else {
            return f32::MAX;
        };
        ids.iter()
            .map(|&id| {
                let e = &self.edges[id as usize];
                let ab = e.b - e.a;
                let t = if ab.length_squared() > 1e-9 { ((p - e.a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
                p.distance(e.a + ab * t) - (e.ha + (e.hb - e.ha) * t)
            })
            .fold(f32::MAX, f32::min)
    }

    /// The height of the surface of the road at `p`, if it is on one: where the road's mesh is, which lies above the
    /// ground, so that something standing on a road stands on it and not in it.
    pub fn top_at(&self, map: &TerrainMap, p: Vec2) -> Option<f32> {
        let kind = self.kind_at(p)?;
        // As the mesh is made: the highest of the ground here and a little way around, and the water if there is
        // any, so that a crease in the ground doesn't come up through the road.
        let around = [Vec2::ZERO, Vec2::X * REACH_ALONG, Vec2::NEG_X * REACH_ALONG, Vec2::Y * REACH_ACROSS, Vec2::NEG_Y * REACH_ACROSS];
        let ground = around.iter().map(|d| map.surface_height_at(p + *d)).fold(f32::MIN, f32::max);
        let n = map.grid_size();
        let cell = nearest_cell(map, p);
        let water = map.water_level(cell % n, cell / n).unwrap_or(f32::MIN);
        Some(ground.max(water) + lift(kind))
    }

    /// The kind of road `p` is on, if it is on one (the nearest, where several overlap).
    pub fn kind_at(&self, p: Vec2) -> Option<RoadKind> {
        let ids = self.buckets.get(&((p.x / BUCKET).floor() as i32, (p.y / BUCKET).floor() as i32))?;
        ids.iter()
            .filter_map(|&id| {
                let e = &self.edges[id as usize];
                let ab = e.b - e.a;
                let t = if ab.length_squared() > 1e-9 { ((p - e.a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
                let clearance = p.distance(e.a + ab * t) - (e.ha + (e.hb - e.ha) * t);
                (clearance < 0.0).then_some((clearance, e.kind))
            })
            // A path across a road is under it; the road wins. Otherwise the deepest in.
            .min_by(|a, b| matches!(a.1, RoadKind::Path | RoadKind::Walkway).cmp(&matches!(b.1, RoadKind::Path | RoadKind::Walkway)).then(a.0.total_cmp(&b.0)))
            .map(|(_, kind)| kind)
    }

    /// The parts of the straight run `a` to `b` that are at least `margin` metres clear of every
    /// road, as ranges of the way along it (0 to 1). Pieces shorter than `min_length` are dropped.
    pub fn open_runs(&self, a: Vec2, b: Vec2, margin: f32, min_length: f32) -> Vec<(f32, f32)> {
        const STEP: f32 = 0.4;
        let length = a.distance(b);
        // Nothing near enough to matter: the whole run is open. (Clearance changes by at most about
        // two metres per metre moved, since a road's width varies along it.)
        if self.clearance(a.lerp(b, 0.5)) > length + margin {
            return vec![(0.0, 1.0)];
        }
        let steps = (length / STEP).ceil().max(1.0) as usize;
        let mut runs = Vec::new();
        let mut start: Option<usize> = None;
        for i in 0..=steps {
            let open = self.clearance(a.lerp(b, i as f32 / steps as f32)) > margin;
            match (open, start) {
                (true, None) => start = Some(i),
                (false, Some(s)) => {
                    runs.push((s, i - 1));
                    start = None;
                }
                _ => {}
            }
        }
        if let Some(s) = start {
            runs.push((s, steps));
        }
        runs.into_iter()
            .map(|(s, e)| (s as f32 / steps as f32, e as f32 / steps as f32))
            .filter(|(s, e)| (e - s) * length >= min_length || (*s == 0.0 && *e == 1.0))
            .collect()
    }
}

// Chains of cells for one road kind, each running between junctions / dead ends (a node
// where roads of any kind meet or stop). Chains are sequences of cell indices.
struct Chain {
    cells: Vec<usize>,
    start_junction: bool,
    end_junction: bool,
}

fn road_chains(roads: &RoadNetwork, kind: RoadKind) -> Vec<Chain> {
    let mut degree: HashMap<usize, usize> = HashMap::new();
    let mut adj: HashMap<usize, Vec<usize>> = HashMap::new();
    for &(a, b, k) in &roads.edges {
        *degree.entry(a).or_default() += 1;
        *degree.entry(b).or_default() += 1;
        if k == kind {
            adj.entry(a).or_default().push(b);
            adj.entry(b).or_default().push(a);
        }
    }
    let is_break = |c: usize| degree[&c] != 2 || adj.get(&c).map_or(0, Vec::len) != 2;
    let key = |a: usize, b: usize| (a.min(b), a.max(b));
    let mut used: HashSet<(usize, usize)> = HashSet::new();
    let mut chains = Vec::new();
    let walk = |start: usize, first: usize, used: &mut HashSet<(usize, usize)>| {
        let mut chain = vec![start, first];
        used.insert(key(start, first));
        let mut current = first;
        while !is_break(current) {
            let next = adj[&current].iter().copied().find(|&nb| !used.contains(&key(current, nb)));
            let Some(next) = next else { break };
            used.insert(key(current, next));
            chain.push(next);
            current = next;
        }
        chain
    };
    let junction = |c: usize| degree.get(&c).is_some_and(|&d| d >= 3);
    let mut nodes: Vec<usize> = adj.keys().copied().collect();
    nodes.sort_unstable();
    for &node in nodes.iter().filter(|&&c| is_break(c)) {
        for &nb in &adj[&node] {
            if !used.contains(&key(node, nb)) {
                let cells = walk(node, nb, &mut used);
                chains.push(Chain { start_junction: junction(cells[0]), end_junction: junction(cells[cells.len() - 1]), cells });
            }
        }
    }
    // Anything left is a closed loop with no break node.
    for &node in &nodes {
        for &nb in &adj[&node] {
            if !used.contains(&key(node, nb)) {
                let cells = walk(node, nb, &mut used);
                chains.push(Chain { start_junction: false, end_junction: false, cells });
            }
        }
    }
    chains
}

// Relaxes a grid-step path so it stops zigzagging, then cuts its corners twice (Chaikin),
// which converges on a smooth curve while staying inside the original path's hull. The two
// end points never move, so chains still meet exactly at junctions.
fn smooth_polyline(mut pts: Vec<Vec2>) -> Vec<Vec2> {
    for _ in 0..2 {
        if pts.len() < 3 {
            break;
        }
        let prev = pts.clone();
        for i in 1..pts.len() - 1 {
            pts[i] = (prev[i - 1] + prev[i] * 2.0 + prev[i + 1]) * 0.25;
        }
    }
    for _ in 0..2 {
        if pts.len() < 3 {
            break;
        }
        let mut out = Vec::with_capacity(pts.len() * 2);
        out.push(pts[0]);
        for w in pts.windows(2) {
            out.push(w[0] * 0.75 + w[1] * 0.25);
            out.push(w[0] * 0.25 + w[1] * 0.75);
        }
        out.push(pts[pts.len() - 1]);
        pts = out;
    }
    pts
}

fn idx_to_pos(map: &TerrainMap, idx: usize) -> Vec2 {
    let n = map.grid_size();
    grid_pos(idx % n, idx / n)
}

/// Spawns the road ribbons: tarmac with painted lines for the main roads, gravel tracks for the farms.
pub fn spawn_roads(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    road_materials: &mut Assets<RoadMaterial>,
    textures: &TerrainTextures,
    map: &TerrainMap,
    roads: &RoadNetwork,
) {
    let ribbons: Vec<RoadRibbon> = road_ribbons(map, roads).into_iter().map(refined).collect();
    let mut material = |surface: &Handle<Image>, roughness: f32| {
        road_materials.add(RoadMaterial {
            base: StandardMaterial { base_color: Color::WHITE, perceptual_roughness: roughness, reflectance: 0.25, ..default() },
            extension: RoadExtension { surface: surface.clone() },
        })
    };
    let major = material(&textures.asphalt, 0.92);
    let minor = material(&textures.track, 1.0);
    for (kind, material) in [(RoadKind::Major, major.clone()), (RoadKind::Lane, major.clone()), (RoadKind::Pavement, major.clone()), (RoadKind::Walkway, major), (RoadKind::Minor, minor.clone()), (RoadKind::Path, minor)] {
        commands.spawn((
            crate::terrain::TerrainRoot,
            bevy::light::NotShadowCaster,
            Mesh3d(meshes.add(road_mesh(map, &ribbons, kind))),
            MeshMaterial3d(material),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How many of a mesh's vertices are the road's top (the skirts down its sides come after).
    fn top_vertices(ribbons: &[RoadRibbon], kind: RoadKind) -> usize {
        ribbons.iter().filter(|r| r.kind == kind).map(|r| r.points.len() * ACROSS).sum()
    }

    /// A straight east-west road of the given kind, `length` long, centred on the origin.
    fn straight(kind: RoadKind, length: f32, start_junction: bool, end_junction: bool) -> RoadRibbon {
        let points: Vec<Vec2> = (0..=(length / 2.5) as usize).map(|i| Vec2::new(-length / 2.0 + i as f32 * 2.5, 0.0)).collect();
        let half_widths = vec![half_width(kind); points.len()];
        RoadRibbon { kind, points, half_widths, start_junction, end_junction }
    }

    #[test]
    fn a_road_that_stops_short_is_closed_off_at_its_end_but_one_that_meets_another_is_not() {
        let map = TerrainMap::flat(10.0);
        let triangles = |road: RoadRibbon| match road_mesh(&map, &[road], RoadKind::Path).indices() {
            Some(Indices::U32(i)) => i.len() / 3,
            _ => panic!("no indices"),
        };
        let (dead, one_end, joined) = (triangles(straight(RoadKind::Path, 20.0, false, false)), triangles(straight(RoadKind::Path, 20.0, true, false)), triangles(straight(RoadKind::Path, 20.0, true, true)));
        // A face across an end is a strip of ACROSS - 1 quads, two triangles each.
        let cap = (ACROSS - 1) * 2;
        assert_eq!((dead - one_end, one_end - joined), (cap, cap));
    }

    #[test]
    fn a_road_has_a_surface_above_the_ground_to_stand_on_and_the_ground_beside_it_is_not_that() {
        let map = TerrainMap::flat(10.0);
        let clearance = RoadClearance::new(&[straight(RoadKind::Major, 100.0, false, false)]);
        let top = clearance.top_at(&map, Vec2::new(5.0, 1.0)).unwrap();
        assert!((top - (10.0 + lift(RoadKind::Major))).abs() < 1e-3, "{top}");
        assert!(top > 10.05, "it is raised, as it is drawn");
        assert_eq!(clearance.top_at(&map, Vec2::new(5.0, 9.0)), None, "the verge is the ground");
        let track = RoadClearance::new(&[straight(RoadKind::Minor, 100.0, false, false)]);
        assert!(track.top_at(&map, Vec2::new(5.0, 0.0)).unwrap() < top, "a farm track lies a hair under a main road");
    }

    #[test]
    fn a_point_on_the_road_has_negative_clearance_and_the_edge_is_zero() {
        let clearance = RoadClearance::new(&[straight(RoadKind::Major, 100.0, false, false)]);
        assert!(clearance.clearance(Vec2::new(0.0, 0.0)) < -3.9);
        assert!(clearance.clearance(Vec2::new(10.0, 3.0)) < 0.0, "inside the 4 m half width");
        assert!(clearance.clearance(Vec2::new(10.0, MAJOR_HALF_WIDTH)).abs() < 1e-3, "right on the edge");
        assert!((clearance.clearance(Vec2::new(10.0, 6.0)) - 2.0).abs() < 1e-3, "two metres off");
        assert_eq!(clearance.clearance(Vec2::new(10.0, 200.0)), f32::MAX, "nothing near");
    }

    #[test]
    fn a_wall_across_a_road_is_cut_to_leave_the_road_and_its_verge() {
        let clearance = RoadClearance::new(&[straight(RoadKind::Major, 100.0, false, false)]);
        // A wall running north-south across the road, 40 m long.
        let runs = clearance.open_runs(Vec2::new(0.0, -20.0), Vec2::new(0.0, 20.0), 1.2, 0.8);
        assert_eq!(runs.len(), 2, "one piece each side: {runs:?}");
        let (end_of_first, start_of_second) = (runs[0].1 * 40.0 - 20.0, runs[1].0 * 40.0 - 20.0);
        assert!((end_of_first + 5.2).abs() < 0.5, "first piece ends at the road's edge plus margin: {end_of_first}");
        assert!((start_of_second - 5.2).abs() < 0.5, "second piece starts at the other side: {start_of_second}");
    }

    #[test]
    fn a_wall_alongside_a_road_is_left_alone_but_one_on_it_is_removed() {
        let clearance = RoadClearance::new(&[straight(RoadKind::Major, 100.0, false, false)]);
        let beside = clearance.open_runs(Vec2::new(-10.0, 8.0), Vec2::new(10.0, 8.0), 1.2, 0.8);
        assert_eq!(beside, vec![(0.0, 1.0)], "8 m off the centre line is clear");
        let on_it = clearance.open_runs(Vec2::new(-10.0, 1.0), Vec2::new(10.0, 1.0), 1.2, 0.8);
        assert!(on_it.is_empty(), "a wall lying along the road is gone: {on_it:?}");
    }

    #[test]
    fn a_short_wall_nowhere_near_a_road_is_never_dropped_for_being_short() {
        let clearance = RoadClearance::new(&[straight(RoadKind::Major, 100.0, false, false)]);
        let runs = clearance.open_runs(Vec2::new(0.0, 30.0), Vec2::new(0.3, 30.0), 1.2, 0.8);
        assert_eq!(runs, vec![(0.0, 1.0)]);
    }

    #[test]
    fn a_track_is_narrower_than_a_main_road() {
        let main = RoadClearance::new(&[straight(RoadKind::Major, 100.0, false, false)]);
        let track = RoadClearance::new(&[straight(RoadKind::Minor, 100.0, false, false)]);
        let p = Vec2::new(0.0, 3.0);
        assert!(main.clearance(p) < 0.0 && track.clearance(p) > 0.0);
    }

    #[test]
    fn the_road_mesh_has_what_the_shader_needs() {
        let map = TerrainMap::flat(10.0);
        let ribbon = straight(RoadKind::Major, 80.0, false, false);
        let mesh = road_mesh(&map, &[ribbon.clone()], RoadKind::Major);
        let attr = |a| mesh.attribute(a).expect("attribute");
        let bevy::mesh::VertexAttributeValues::Float32x3(positions) = attr(Mesh::ATTRIBUTE_POSITION) else { panic!() };
        let bevy::mesh::VertexAttributeValues::Float32x3(normals) = attr(Mesh::ATTRIBUTE_NORMAL) else { panic!() };
        let bevy::mesh::VertexAttributeValues::Float32x2(uv0) = attr(Mesh::ATTRIBUTE_UV_0) else { panic!() };
        let bevy::mesh::VertexAttributeValues::Float32x2(uv1) = attr(Mesh::ATTRIBUTE_UV_1) else { panic!() };
        let bevy::mesh::VertexAttributeValues::Float32x4(colours) = attr(Mesh::ATTRIBUTE_COLOR) else { panic!() };
        let top = ribbon.points.len() * ACROSS;
        assert!(positions.len() > top, "and a skirt down each side");
        for i in 0..top {
            assert!((positions[i][1] - (10.0 + ROAD_LIFT)).abs() < 1e-4, "lifted off the ground");
            assert!((normals[i][1] - 1.0).abs() < 1e-4, "faces up on flat ground");
            // UV 0 tiles in world metres, UV 1 is metres across and along.
            assert!((uv0[i][0] * ASPHALT_TILE - positions[i][0]).abs() < 1e-3);
            assert!(uv1[i][0].abs() <= MAJOR_HALF_WIDTH + 1e-4, "across: within the road's half width");
            if i % ACROSS == 0 || i % ACROSS == ACROSS - 1 {
                assert!((uv1[i][0].abs() - MAJOR_HALF_WIDTH).abs() < 1e-4, "the outer points are at the road's edges");
            }
            assert_eq!(colours[i][0], 1.0, "painted all along a road with no junctions");
            assert!((colours[i][1] * 10.0 - MAJOR_HALF_WIDTH).abs() < 1e-4);
            assert_eq!(colours[i][2], 0.0, "tarmac, not a track");
        }
        // The first and last vertices of one cross-section are on opposite sides, and along only grows.
        assert!(uv1[0][0] < 0.0 && uv1[ACROSS - 1][0] > 0.0);
        assert!(uv1[..top].windows(2).all(|w| w[1][1] >= w[0][1]));
        assert!((uv1[top - 1][1] - 80.0).abs() < 0.01, "metres along the whole road");
    }

    #[test]
    fn lane_markings_stop_short_of_a_junction_but_run_to_a_dead_end() {
        let map = TerrainMap::flat(0.0);
        let colours = |ribbon: RoadRibbon| {
            let mesh = road_mesh(&map, &[ribbon.clone()], RoadKind::Major);
            let bevy::mesh::VertexAttributeValues::Float32x4(c) = mesh.attribute(Mesh::ATTRIBUTE_COLOR).expect("colour") else { panic!() };
            let bevy::mesh::VertexAttributeValues::Float32x2(uv1) = mesh.attribute(Mesh::ATTRIBUTE_UV_1).expect("uv1") else { panic!() };
            let top = top_vertices(&[ribbon], RoadKind::Major);
            c.iter().zip(uv1).take(top).map(|(c, uv)| (uv[1], c[0])).collect::<Vec<_>>()
        };
        let at_junction = colours(straight(RoadKind::Major, 80.0, true, true));
        for &(along, painted) in &at_junction {
            let near_end = along < JUNCTION_CLEAR || 80.0 - along < JUNCTION_CLEAR;
            assert_eq!(painted == 0.0, near_end, "at {along} m");
        }
        assert!(at_junction.iter().any(|&(_, p)| p == 1.0), "but painted in the middle");
        let dead_end = colours(straight(RoadKind::Major, 80.0, false, false));
        assert!(dead_end.iter().all(|&(_, p)| p == 1.0));
    }

    #[test]
    fn the_edge_line_is_broken_where_a_side_road_joins() {
        let map = TerrainMap::flat(0.0);
        // A main road along x, with a track joining it from the north at x = 0.
        let main = straight(RoadKind::Major, 100.0, false, false);
        let side = RoadRibbon {
            kind: RoadKind::Minor,
            points: (0..=10).map(|i| Vec2::new(0.0, 25.0 - i as f32 * 2.5)).collect(),
            half_widths: vec![MINOR_HALF_WIDTH; 11],
            start_junction: false,
            end_junction: true,
        };
        let mesh = road_mesh(&map, &[main.clone(), side], RoadKind::Major);
        let bevy::mesh::VertexAttributeValues::Float32x4(c) = mesh.attribute(Mesh::ATTRIBUTE_COLOR).expect("colour") else { panic!() };
        let bevy::mesh::VertexAttributeValues::Float32x3(p) = mesh.attribute(Mesh::ATTRIBUTE_POSITION).expect("positions") else { panic!() };
        let reach = MINOR_HALF_WIDTH + MOUTH_MARGIN;
        let top = main.points.len() * ACROSS;
        for (colour, pos) in c.iter().zip(p).take(top) {
            let near_mouth = Vec2::new(pos[0], pos[2]).abs().x < reach - 0.5;
            let far = Vec2::new(pos[0], pos[2]).abs().x > reach + 3.0;
            if near_mouth {
                assert_eq!(colour[3], 0.0, "edge line should be broken at x = {}", pos[0]);
            }
            if far {
                assert_eq!(colour[3], 1.0, "edge line should run at x = {}", pos[0]);
            }
            assert_eq!(colour[0], 1.0, "the centre line runs on through (the main road has the priority)");
        }
        // And a road with no side roads has its edge line all the way.
        let alone = road_mesh(&map, &[main.clone()], RoadKind::Major);
        let bevy::mesh::VertexAttributeValues::Float32x4(c) = alone.attribute(Mesh::ATTRIBUTE_COLOR).expect("colour") else { panic!() };
        assert!(c.iter().take(top).all(|c| c[3] == 1.0));
    }

    #[test]
    fn a_track_is_never_painted_and_lies_a_little_below_the_main_road() {
        let map = TerrainMap::flat(0.0);
        let mesh = road_mesh(&map, &[straight(RoadKind::Minor, 40.0, false, false)], RoadKind::Minor);
        let bevy::mesh::VertexAttributeValues::Float32x4(colours) = mesh.attribute(Mesh::ATTRIBUTE_COLOR).expect("colour") else { panic!() };
        let bevy::mesh::VertexAttributeValues::Float32x3(positions) = mesh.attribute(Mesh::ATTRIBUTE_POSITION).expect("positions") else { panic!() };
        let ribbon = straight(RoadKind::Minor, 40.0, false, false);
        let top = ribbon.points.len() * ACROSS;
        assert!(colours.iter().take(top).all(|c| c[0] == 0.0 && c[2] == 1.0));
        assert!(positions.iter().take(top).all(|p| p[1] < ROAD_LIFT && p[1] > ROAD_LIFT - 0.1));
    }

    #[test]
    fn the_lines_follow_the_british_standard() {
        // The shader's pattern: marks of 3 m every 9 m (diagram 1008.1, over 40 mph) and lines 100 mm
        // wide; the shader keeps its own copies of these, so they're stated here as the standard.
        let shader = std::fs::read_to_string("assets/shaders/road.wgsl").expect("shader");
        assert!(shader.contains("const MARK_LENGTH: f32 = 3.0;"));
        assert!(shader.contains("const MARK_PERIOD: f32 = 9.0;"));
        assert!(shader.contains("const LINE_HALF_WIDTH: f32 = 0.05;"));
        // A road wide enough for a centre line (5.5 m) has one: the main roads are 8 m.
        assert!(MAJOR_HALF_WIDTH * 2.0 >= 5.5);
    }

    #[test]
    fn no_ground_pokes_up_through_a_road() {
        let params = GenParams::default();
        let map = TerrainMap::generate(crate::MAP_SEED, &params);
        let roads = RoadNetwork::generate(&map, &params);
        let ribbons: Vec<RoadRibbon> = road_ribbons(&map, &roads).into_iter().map(refined).collect();
        let mut worst = f32::MIN;
        let mut samples = 0;
        let (mut above, mut shown) = (0, 0);
        let mut clearances: Vec<f32> = Vec::new();
        for kind in [RoadKind::Major, RoadKind::Minor] {
            let mesh = road_mesh(&map, &ribbons, kind);
            let bevy::mesh::VertexAttributeValues::Float32x3(positions) = mesh.attribute(Mesh::ATTRIBUTE_POSITION).expect("positions") else { panic!() };
            let rows: Vec<&[[f32; 3]]> = positions[..top_vertices(&ribbons, kind)].chunks(ACROSS).collect();
            // Every pair of neighbouring rows of one ribbon is a strip; the first row of the next ribbon
            // follows the last of the one before, so skip a pair that jumps far.
            for pair in rows.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                if Vec2::new(a[0][0] - b[0][0], a[0][2] - b[0][2]).length() > 6.0 {
                    continue;
                }
                for t in [0.25f32, 0.5, 0.75] {
                    for c in 0..ACROSS - 1 {
                        for u in [0.25f32, 0.5, 0.75] {
                            let lerp = |p: [f32; 3], q: [f32; 3], f: f32| Vec3::from_array(p).lerp(Vec3::from_array(q), f);
                            let top = lerp(a[c], b[c], t).lerp(lerp(a[c + 1], b[c + 1], t), u);
                            let ground = map.surface_height_at(Vec2::new(top.x, top.z));
                            worst = worst.max(ground - top.y);
                            clearances.push(top.y - ground);
                            samples += 1;
                            if ground > top.y {
                                above += 1;
                            }
                            if ground > top.y + 0.1 && shown < 5 {
                                shown += 1;
                                eprintln!("ground {:.2} over road {:.2} at ({:.1}, {:.1}) kind {kind:?}", ground, top.y, top.x, top.z);
                            }
                        }
                    }
                }
            }
        }
        // How far the road stands off the ground: low enough that it does not look like a causeway.
        clearances.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let q = |f: f32| clearances[((clearances.len() - 1) as f32 * f) as usize];
        eprintln!("clearance: min {:.3} p50 {:.3} p90 {:.3} p99 {:.3} max {:.3}", q(0.0), q(0.5), q(0.9), q(0.99), q(1.0));
        assert!(q(0.5) < 0.15 && q(0.9) < 0.25 && q(1.0) < 0.8, "the road stands too high off the ground");
        eprintln!("{samples} samples; {above} have ground over the road, by {worst:.3} m at worst");
        assert!(worst < -0.03, "ground reaches {worst} m above the road somewhere");
    }

    /// Prints places where roads meet at an angle, with where to stand to look at them: `FPS_AT`, `FPS_YAW`.
    #[test]
    #[ignore = "prints places to look at"]
    fn where_to_look_at_junctions() {
        let params = GenParams::default();
        let map = TerrainMap::generate(crate::MAP_SEED, &params);
        let roads = RoadNetwork::generate(&map, &params);
        let ribbons = road_ribbons(&map, &roads);
        // Every ribbon end that is a junction, with the way the road leaves it.
        let mut ends: Vec<(Vec2, Vec2, RoadKind)> = Vec::new();
        for r in &ribbons {
            let n = r.points.len();
            if n < 3 {
                continue;
            }
            if r.start_junction {
                ends.push((r.points[0], (r.points[3.min(n - 1)] - r.points[0]).normalize_or_zero(), r.kind));
            }
            if r.end_junction {
                ends.push((r.points[n - 1], (r.points[n - 1 - 3.min(n - 1)] - r.points[n - 1]).normalize_or_zero(), r.kind));
            }
        }
        let mut shown = 0;
        for (i, &(at, _, _)) in ends.iter().enumerate() {
            let here: Vec<&(Vec2, Vec2, RoadKind)> = ends.iter().filter(|e| e.0.distance(at) < 0.5).collect();
            if here.len() < 3 || ends[..i].iter().any(|e| e.0.distance(at) < 0.5) {
                continue;
            }
            if map.pois.iter().any(|p| p.position.distance(at) < p.radius * 1.5) || map.water_surface_at(at).is_some() {
                continue;
            }
            let kinds: Vec<RoadKind> = here.iter().map(|e| e.2).collect();
            let angles: Vec<i32> = here.iter().map(|e| e.1.y.atan2(e.1.x).to_degrees() as i32).collect();
            // Stand 14 m back from the junction along the first road, looking at it.
            let stand = at + here[0].1 * 14.0;
            let look = -here[0].1;
            eprintln!("{kinds:?} at {angles:?}: FPS_AT={:.0},{:.0} FPS_YAW={:.2}", stand.x, stand.y, (-look.x).atan2(-look.y));
            shown += 1;
            if shown >= 8 {
                break;
            }
        }
    }

    /// Prints the junction patch near `JUNCTION_AT=x,z`, and the roads that end there.
    #[test]
    #[ignore = "prints a junction"]
    fn what_the_junction_looks_like() {
        let Ok(at) = std::env::var("JUNCTION_AT") else { return };
        let at: Vec<f32> = at.split(',').map(|v| v.parse().unwrap()).collect();
        let at = Vec2::new(at[0], at[1]);
        let params = GenParams::default();
        let map = TerrainMap::generate(crate::MAP_SEED, &params);
        let roads = RoadNetwork::generate(&map, &params);
        let ribbons = road_ribbons(&map, &roads);
        for r in &ribbons {
            let n = r.points.len();
            for (junction, end) in [(r.start_junction, 0), (r.end_junction, n - 1)] {
                if r.points[end].distance(at) < 25.0 {
                    let towards = if end == 0 { r.points[1.min(n - 1)] } else { r.points[n.saturating_sub(2)] };
                    eprintln!("{:?} end {end}/{n} junction {junction} at {:?} hw {:.1} dir {:.0} deg", r.kind, r.points[end], r.half_widths[end], (towards - r.points[end]).to_angle().to_degrees());
                }
            }
        }
        for patch in junction_patches(&ribbons).iter().filter(|p| p.centre.distance(at) < 25.0) {
            eprintln!("patch {:?} at {:?}: {} points", patch.kind, patch.centre, patch.outline.len());
            for (p, w) in patch.outline.iter().zip(&patch.wall) {
                eprintln!("   {:6.1} {:6.1} wall {w}  ({:.1} m from centre)", p.x - patch.centre.x, p.y - patch.centre.y, p.distance(patch.centre));
            }
        }
    }

    /// Draws the road meshes seen from above, near `JUNCTION_AT=x,z`, to a PPM at `JUNCTION_OUT`.
    #[test]
    #[ignore = "writes an image"]
    fn the_junction_from_above() {
        let (Ok(at), Ok(out)) = (std::env::var("JUNCTION_AT"), std::env::var("JUNCTION_OUT")) else { return };
        let at: Vec<f32> = at.split(',').map(|v| v.parse().unwrap()).collect();
        let at = Vec2::new(at[0], at[1]);
        let params = GenParams::default();
        let map = TerrainMap::generate(crate::MAP_SEED, &params);
        let roads = RoadNetwork::generate(&map, &params);
        let ribbons: Vec<RoadRibbon> = road_ribbons(&map, &roads).into_iter().map(refined).collect();
        let (size, scale) = (600usize, 10.0f32);
        let mut pixels = vec![[20u8, 60, 20]; size * size];
        let to = |x: f32, z: f32| (((x - at.x) * scale + size as f32 / 2.0) as i32, ((z - at.y) * scale + size as f32 / 2.0) as i32);
        for (kind, colour) in [(RoadKind::Major, [120u8, 120, 130]), (RoadKind::Lane, [150, 150, 160]), (RoadKind::Minor, [190, 150, 90]), (RoadKind::Path, [170, 130, 80])] {
            let mesh = road_mesh(&map, &ribbons, kind);
            let bevy::mesh::VertexAttributeValues::Float32x3(p) = mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap() else { panic!() };
            let Some(Indices::U32(idx)) = mesh.indices() else { panic!() };
            for t in idx.chunks(3) {
                let v: Vec<Vec3> = t.iter().map(|&i| Vec3::from_array(p[i as usize])).collect();
                // Only faces that look up.
                if (v[1] - v[0]).cross(v[2] - v[0]).y <= 0.0 {
                    continue;
                }
                let pts: Vec<(i32, i32)> = v.iter().map(|q| to(q.x, q.z)).collect();
                let (lo, hi) = ((pts.iter().map(|q| q.0).min().unwrap()).max(0), (pts.iter().map(|q| q.0).max().unwrap()).min(size as i32 - 1));
                let (zlo, zhi) = ((pts.iter().map(|q| q.1).min().unwrap()).max(0), (pts.iter().map(|q| q.1).max().unwrap()).min(size as i32 - 1));
                for z in zlo..=zhi {
                    for x in lo..=hi {
                        let (a, b, c) = (pts[0], pts[1], pts[2]);
                        let area = ((b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)) as f32;
                        if area == 0.0 { continue; }
                        let w0 = (((b.0 - x) * (c.1 - z) - (b.1 - z) * (c.0 - x)) as f32) / area;
                        let w1 = (((c.0 - x) * (a.1 - z) - (c.1 - z) * (a.0 - x)) as f32) / area;
                        let w2 = 1.0 - w0 - w1;
                        if w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0 {
                            // Shaded by height, so overlaps show.
                            let shade = ((v[0].y + v[1].y + v[2].y) / 3.0 * 40.0).rem_euclid(30.0) as u8;
                            pixels[z as usize * size + x as usize] = colour.map(|c| c.saturating_add(shade));
                        }
                    }
                }
            }
        }
        for patch in junction_patches(&ribbons) {
            for k in 0..patch.outline.len() {
                let (a, b) = (patch.outline[k], patch.outline[(k + 1) % patch.outline.len()]);
                let (pa, pb) = (to(a.x, a.y), to(b.x, b.y));
                let n = (pa.0 - pb.0).abs().max((pa.1 - pb.1).abs()).max(1);
                for f in 0..=n {
                    let (x, z) = (pa.0 + (pb.0 - pa.0) * f / n, pa.1 + (pb.1 - pa.1) * f / n);
                    if x >= 0 && z >= 0 && (x as usize) < size && (z as usize) < size {
                        pixels[z as usize * size + x as usize] = if patch.wall[k] { [255, 0, 0] } else { [255, 255, 0] };
                    }
                }
            }
        }
        let mut data = format!("P6 {size} {size} 255\n").into_bytes();
        data.extend(pixels.iter().flatten());
        std::fs::write(out, data).unwrap();
    }

    /// Prints places on country roads to stand and look along them (for FPS_AT and FPS_YAW).
    #[test]
    #[ignore = "prints places to look at"]
    fn where_to_look_at_a_country_road() {
        let params = GenParams::default();
        let map = TerrainMap::generate(crate::MAP_SEED, &params);
        let roads = RoadNetwork::generate(&map, &params);
        let ribbons = road_ribbons(&map, &roads);
        let mut shown = 0;
        for r in ribbons.iter().filter(|r| matches!(r.kind, RoadKind::Major | RoadKind::Minor) && r.points.len() > 20) {
            let mid = r.points[r.points.len() / 2];
            if map.pois.iter().any(|p| p.position.distance(mid) < p.radius * 1.5) || map.water_surface_at(mid).is_some() {
                continue;
            }
            let d = (r.points[r.points.len() / 2 + 3] - r.points[r.points.len() / 2]).normalize();
            // Looking along it: yaw 0 looks toward -z, so yaw = atan2(-d.x, -d.y).
            eprintln!("{:?}: FPS_AT={:.0},{:.0} FPS_YAW={:.2}", r.kind, mid.x, mid.y, (-d.x).atan2(-d.y));
            shown += 1;
            if shown >= 6 {
                break;
            }
        }
    }

    /// A road of `kind` leaving the origin along `direction`, as one that starts at a junction.
    fn leaving(kind: RoadKind, direction: Vec2) -> RoadRibbon {
        let points: Vec<Vec2> = (0..=12).map(|i| direction.normalize() * i as f32 * 2.5).collect();
        RoadRibbon { kind, half_widths: vec![half_width(kind); points.len()], points, start_junction: true, end_junction: false }
    }

    /// How far `p` is from the surface of a road made by `leaving`, as the mesh has it: flat ended at the origin.
    fn off(road: &RoadRibbon, p: Vec2) -> f32 {
        let (d, hw) = (road.points[1].normalize(), road.half_widths[0]);
        let length = road.points.last().unwrap().length();
        let (along, across) = (p.dot(d), cross(d, p).abs());
        Vec2::new((-along).max(along - length).max(0.0), (across - hw).max(0.0)).length()
    }

    fn inside(polygon: &[Vec2], p: Vec2) -> bool {
        let mut crossings = 0;
        for k in 0..polygon.len() {
            let (a, b) = (polygon[k], polygon[(k + 1) % polygon.len()]);
            if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
                crossings += 1;
            }
        }
        crossings % 2 == 1
    }

    #[test]
    fn the_gap_on_the_outside_of_a_bend_is_covered_by_the_junction_and_a_straight_join_needs_nothing() {
        let (west, north) = (leaving(RoadKind::Major, Vec2::NEG_X), leaving(RoadKind::Major, Vec2::Y));
        let patches = junction_patches(&[west.clone(), north.clone()]);
        assert_eq!(patches.len(), 1);
        // A point outside the corner, where neither road reaches.
        let gap = Vec2::new(2.5, -2.5);
        assert!(off(&west, gap) > 0.5 && off(&north, gap) > 0.5, "neither road covers it");
        assert!(inside(&patches[0].outline, gap), "the patch does");
        // Two roads that run straight on into one another leave no gap.
        let straight = junction_patches(&[leaving(RoadKind::Major, Vec2::NEG_X), leaving(RoadKind::Major, Vec2::X)]);
        assert!(straight.is_empty());
        // And a road alone at its end has nothing to meet.
        assert!(junction_patches(&[leaving(RoadKind::Major, Vec2::X)]).is_empty());
    }

    #[test]
    fn where_a_road_forks_into_two_at_a_shallow_angle_the_patch_does_not_reach_past_their_edges() {
        // A stem that forks into two roads 40 degrees apart: the corner of each branch's flat end would stick out
        // past the other's edge.
        let stem = leaving(RoadKind::Major, Vec2::NEG_X);
        let (a, b) = (leaving(RoadKind::Major, Vec2::from_angle(20f32.to_radians())), leaving(RoadKind::Major, Vec2::from_angle(-20f32.to_radians())));
        let patches = junction_patches(&[stem.clone(), a.clone(), b.clone()]);
        assert_eq!(patches.len(), 1);
        // Everything it covers is on a road, give or take the little the rounded corner between the branches takes.
        let outline = &patches[0].outline;
        for k in 0..outline.len() {
            let (p, q) = (outline[k], outline[(k + 1) % outline.len()]);
            for f in [0.0, 0.5] {
                let at = p.lerp(q, f);
                let away = off(&stem, at).min(off(&a, at)).min(off(&b, at));
                assert!(away < 1.0, "{at} is {away} m off the roads");
            }
        }
    }

    #[test]
    fn a_junction_patch_is_a_surface_just_above_the_roads_with_a_face_down_its_open_edges() {
        let map = TerrainMap::flat(10.0);
        let ribbons = [leaving(RoadKind::Major, Vec2::NEG_X), leaving(RoadKind::Major, Vec2::Y)];
        let with = road_mesh(&map, &ribbons, RoadKind::Major);
        let bevy::mesh::VertexAttributeValues::Float32x3(p) = with.attribute(Mesh::ATTRIBUTE_POSITION).expect("positions") else { panic!() };
        let top = 10.0 + ROAD_LIFT + PATCH_RAISE;
        assert!(p.iter().any(|v| v[0] > 1.0 && v[2] < -1.0 && (v[1] - top).abs() < 1e-3), "a patch vertex out in the gap");
        // Its faces all look up, or out.
        let Some(Indices::U32(indices)) = with.indices().map(|i| match i { Indices::U32(v) => Indices::U32(v.clone()), Indices::U16(v) => Indices::U32(v.iter().map(|&x| x as u32).collect()) }) else { panic!() };
        assert!(indices.len() % 3 == 0 && indices.iter().all(|&i| (i as usize) < p.len()));
        let walls = p.iter().filter(|v| (v[1] - (top - ROAD_THICKNESS)).abs() < 1e-3 && v[0] > 0.5 && v[2] < -0.5).count();
        assert!(walls > 0, "the rounded corner has a face down it");
    }

    #[test]
    fn a_road_has_a_face_down_each_edge_that_looks_outward_and_goes_into_the_ground() {
        let map = TerrainMap::flat(10.0);
        let ribbon = straight(RoadKind::Major, 40.0, false, false);
        let mesh = road_mesh(&map, &[ribbon.clone()], RoadKind::Major);
        let bevy::mesh::VertexAttributeValues::Float32x3(p) = mesh.attribute(Mesh::ATTRIBUTE_POSITION).expect("positions") else { panic!() };
        let bevy::mesh::VertexAttributeValues::Float32x3(n) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL).expect("normals") else { panic!() };
        let Some(Indices::U32(indices)) = mesh.indices().map(|i| match i {
            Indices::U32(v) => Indices::U32(v.clone()),
            Indices::U16(v) => Indices::U32(v.iter().map(|&x| x as u32).collect()),
        }) else { panic!() };
        let top = ribbon.points.len() * ACROSS;
        let skirt = &p[top..];
        assert!(!skirt.is_empty());
        let lowest = skirt.iter().map(|v| v[1]).fold(f32::MAX, f32::min);
        assert!((lowest - (10.0 + ROAD_LIFT - ROAD_THICKNESS)).abs() < 1e-3, "{lowest}: it reaches into the ground");
        // Each skirt triangle is wound to face the way its vertices' normals point, which is away from the road's middle.
        for tri in indices.chunks(3).filter(|t| t[0] as usize >= top) {
            let v = |i: u32| Vec3::from_array(p[i as usize]);
            let face = (v(tri[1]) - v(tri[0])).cross(v(tri[2]) - v(tri[0]));
            let normal = Vec3::from_array(n[tri[0] as usize]);
            assert!(face.dot(normal) > 0.0, "wound the wrong way");
            let mid = (v(tri[0]) + v(tri[1]) + v(tri[2])) / 3.0;
            assert!(normal.z * mid.z >= -1e-4 || normal.z.abs() < 1e-4 || mid.z.abs() < 1e-4, "faces away from the road's middle");
        }
    }
}

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::map::{grid_pos, PoiKind, TerrainMap, CELL, HALF_SIZE};
use crate::params::GenParams;
use crate::road_material::{RoadExtension, RoadMaterial};
use crate::terrain::TerrainTextures;

const ROAD_LIFT: f32 = 0.25;
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
        RoadKind::Minor | RoadKind::Lane => (MINOR_SLOPE_PENALTY, MINOR_WATER_COST),
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

/// The vertex colour of a road vertex, which carries what the road shader needs besides position:
/// r is 1 where lane markings are painted and 0 where they aren't (near junctions), g the road's
/// half width at the vertex in tens of metres, b is the surface (0 tarmac, 0.5 footpath, 1 farm track), and a is
/// 1 where the edge line runs and 0 where it is broken for the mouth of a side road.
pub fn road_vertex_data(kind: RoadKind, painted: bool, edge_line: bool, half_width: f32) -> [f32; 4] {
    [
        if kind == RoadKind::Major && painted { 1.0 } else { 0.0 },
        half_width / 10.0,
        // What the surface is: 0 tarmac, 0.5 a footpath, 1 a farm track.
        match kind {
            RoadKind::Minor => 1.0,
            RoadKind::Path => 0.5,
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

// A ribbon of the given kind, with vertex heights from the smooth bilinear terrain rather than
// the grid vertex, raised above any water for bridges. UV 0 tiles the surface texture in world
// metres; UV 1 is (metres across from the centre line, metres along the road), for the lane
// markings; the normals follow the ground.
pub fn road_mesh(map: &TerrainMap, ribbons: &[RoadRibbon], kind: RoadKind) -> Mesh {
    let tile = match kind {
        RoadKind::Major | RoadKind::Lane => ASPHALT_TILE,
        RoadKind::Minor => TRACK_TILE,
        RoadKind::Path => PATH_TILE,
    };
    // Where roads cross the one that matters more is on top: a lane joins a main road without a seam.
    let lift = ROAD_LIFT
        - match kind {
            RoadKind::Major => 0.0,
            RoadKind::Lane => MINOR_SINK * 0.5,
            RoadKind::Minor => MINOR_SINK,
            RoadKind::Path => MINOR_SINK * 1.5,
        };
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
                let around = [Vec2::ZERO, tangent * 1.2, -tangent * 1.2, perp * 0.7, -perp * 0.7];
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
            .min_by(|a, b| (a.1 == RoadKind::Path).cmp(&(b.1 == RoadKind::Path)).then(a.0.total_cmp(&b.0)))
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
    let ribbons = road_ribbons(map, roads);
    let mut material = |surface: &Handle<Image>, roughness: f32| {
        road_materials.add(RoadMaterial {
            base: StandardMaterial { base_color: Color::WHITE, perceptual_roughness: roughness, reflectance: 0.25, ..default() },
            extension: RoadExtension { surface: surface.clone() },
        })
    };
    let major = material(&textures.asphalt, 0.92);
    let minor = material(&textures.track, 1.0);
    for (kind, material) in [(RoadKind::Major, major.clone()), (RoadKind::Lane, major), (RoadKind::Minor, minor.clone()), (RoadKind::Path, minor)] {
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
        let ribbons = road_ribbons(&map, &roads);
        let mut worst = f32::MIN;
        let mut samples = 0;
        let (mut above, mut shown) = (0, 0);
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
        eprintln!("{samples} samples; {above} have ground over the road, by {worst:.3} m at worst");
        assert!(worst < -0.03, "ground reaches {worst} m above the road somewhere");
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

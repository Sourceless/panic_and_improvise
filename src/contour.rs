// Smoothed marching-squares contours over a grid of region labels.
//
// Every boundary between two labels is traced through the midpoints of the grid edges it
// crosses, which on its own gives straight 22-45 degree facets. Those midpoint vertices are
// shared between neighbouring cells, so they can be relaxed along the contour (and given a
// little low-frequency wobble) without breaking anything: the wall segments and the clipped
// fill polygons are both built from the same relaxed vertices, so fills still meet walls
// exactly and neighbouring regions still tile with no gaps or overlaps.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::map::{fbm, grid_pos, CELL};

/// Label for cells that belong to no region (open ground, dry land, ...).
pub const OPEN: u32 = u32::MAX;

const SMOOTH_ITERATIONS: usize = 8;
const SMOOTH_STRENGTH: f32 = 0.5;
// Relaxation can't move a vertex further than this from where the grid put it, which keeps
// every cell's polygons well-formed regardless of how many iterations run.
const MAX_SMOOTH_DISPLACEMENT: f32 = CELL * 0.35;
const WOBBLE: f32 = CELL * 0.2;
const WOBBLE_SCALE: f32 = 45.0;

pub struct Seg {
    pub a: Vec2,
    pub b: Vec2,
    /// The two labels either side of this segment.
    pub pair: (u32, u32),
}

pub struct Contour {
    n: usize,
    joined: Option<u32>,
    mids: HashMap<u64, Vec2>,
    pub segs: Vec<Seg>,
}

#[derive(Clone, Copy)]
enum End {
    Mid(u64),
    Centre(Vec2),
}

#[derive(Clone, Copy)]
enum Nb {
    Idx(usize),
    Fixed(Vec2),
}

// Keys for the four edge midpoints of a cell, in top, right, bottom, left order. Each grid
// edge has one key no matter which of its two cells asks.
fn edge_keys(n: usize, ix: usize, iz: usize) -> [u64; 4] {
    let h = |x: usize, z: usize| ((z * n + x) as u64) << 1;
    let v = |x: usize, z: usize| (((z * n + x) as u64) << 1) | 1;
    [h(ix, iz), v(ix + 1, iz), h(ix, iz + 1), v(ix, iz)]
}

fn cell_mids(ix: usize, iz: usize) -> [Vec2; 4] {
    let (tl, tr, br, bl) = (grid_pos(ix, iz), grid_pos(ix + 1, iz), grid_pos(ix + 1, iz + 1), grid_pos(ix, iz + 1));
    [(tl + tr) * 0.5, (tr + br) * 0.5, (bl + br) * 0.5, (tl + bl) * 0.5]
}

// For a cell where exactly 2 distinct labels are present among its corners, the standard
// marching-squares connectivity between its 4 edge midpoints (0=top, 1=right, 2=bottom,
// 3=left), keyed by a bitmask of which corners equal the smaller of the two labels
// (bit0=TL, bit1=TR, bit2=BR, bit3=BL). Cases 5 and 10 are the ambiguous "saddle"
// configurations; a fixed pick is used rather than resolving it from a continuous field.
fn case_connections(mask: u8) -> &'static [(u8, u8)] {
    match mask {
        1 | 14 => &[(3, 0)],
        2 | 13 => &[(0, 1)],
        4 | 11 => &[(1, 2)],
        8 | 7 => &[(2, 3)],
        3 | 12 => &[(3, 1)],
        6 | 9 => &[(0, 2)],
        5 => &[(3, 0), (1, 2)],
        10 => &[(0, 1), (2, 3)],
        _ => &[],
    }
}

// At a saddle (two labels on opposite corners), one label is cut into separate corner
// pieces and the other stays connected through the middle. The lower label is cut by
// default; `joined` names a label that should stay connected instead (water, so a diagonal
// run of river cells doesn't break into disconnected diamonds).
fn cut_off_label(distinct: &[u32], joined: Option<u32>) -> u32 {
    match joined {
        Some(j) if distinct.contains(&j) => distinct.iter().copied().find(|&d| d != j).unwrap_or(j),
        _ => distinct.iter().copied().min().unwrap_or(OPEN),
    }
}

impl Contour {
    /// `labels` is an n*n grid of region ids ([`OPEN`] for none); `seed` varies the wobble;
    /// `joined` is the label (if any) that wins saddle cases, see `cut_off_label`.
    pub fn build(n: usize, labels: &[u32], seed: u64, joined: Option<u32>) -> Self {
        let get = |ix: usize, iz: usize| labels[iz * n + ix];
        let mut raw: Vec<(End, End, (u32, u32))> = Vec::new();
        let mut origin: HashMap<u64, Vec2> = HashMap::new();

        for iz in 0..n - 1 {
            for ix in 0..n - 1 {
                let (tl, tr, br, bl) = (get(ix, iz), get(ix + 1, iz), get(ix + 1, iz + 1), get(ix, iz + 1));
                if tl == tr && tr == br && br == bl {
                    continue;
                }
                let keys = edge_keys(n, ix, iz);
                let mids = cell_mids(ix, iz);
                let mut link = |i: usize| {
                    origin.insert(keys[i], mids[i]);
                    End::Mid(keys[i])
                };

                let mut distinct: Vec<u32> = Vec::with_capacity(4);
                for &v in &[tl, tr, br, bl] {
                    if !distinct.contains(&v) {
                        distinct.push(v);
                    }
                }
                if distinct.len() == 2 {
                    let a = cut_off_label(&distinct, joined);
                    let mask = (tl == a) as u8 | ((tr == a) as u8) << 1 | ((br == a) as u8) << 2 | ((bl == a) as u8) << 3;
                    for &(e0, e1) in case_connections(mask) {
                        raw.push((link(e0 as usize), link(e1 as usize), (distinct[0], distinct[1])));
                    }
                } else {
                    // 3 or 4 labels meet here, a rare junction: connect each differing
                    // edge's midpoint to the cell centre.
                    let centre = End::Centre((grid_pos(ix, iz) + grid_pos(ix + 1, iz + 1)) * 0.5);
                    let edges = [(tl, tr), (tr, br), (bl, br), (tl, bl)];
                    for (i, &(x, y)) in edges.iter().enumerate() {
                        if x != y {
                            raw.push((link(i), centre, (x, y)));
                        }
                    }
                }
            }
        }

        // Relax the shared midpoint vertices along the contour.
        let mut keys: Vec<u64> = origin.keys().copied().collect();
        keys.sort_unstable();
        let index: HashMap<u64, usize> = keys.iter().enumerate().map(|(i, &k)| (k, i)).collect();
        let orig: Vec<Vec2> = keys.iter().map(|k| origin[k]).collect();
        let mut nbrs: Vec<Vec<Nb>> = vec![Vec::new(); keys.len()];
        for &(a, b, _) in &raw {
            let as_nb = |e: End| match e {
                End::Mid(k) => Nb::Idx(index[&k]),
                End::Centre(p) => Nb::Fixed(p),
            };
            if let End::Mid(k) = a {
                nbrs[index[&k]].push(as_nb(b));
            }
            if let End::Mid(k) = b {
                nbrs[index[&k]].push(as_nb(a));
            }
        }
        let mut pos = orig.clone();
        for _ in 0..SMOOTH_ITERATIONS {
            let mut next = pos.clone();
            for i in 0..pos.len() {
                // Only vertices with exactly two contour neighbours are interior to a
                // curve; anything else (a map-edge end) stays where the grid put it.
                if nbrs[i].len() != 2 {
                    continue;
                }
                let at = |nb: Nb| match nb {
                    Nb::Idx(j) => pos[j],
                    Nb::Fixed(p) => p,
                };
                let target = (at(nbrs[i][0]) + at(nbrs[i][1])) * 0.5;
                let p = pos[i] + (target - pos[i]) * SMOOTH_STRENGTH;
                let d = p - orig[i];
                next[i] = if d.length() > MAX_SMOOTH_DISPLACEMENT {
                    orig[i] + d.normalize() * MAX_SMOOTH_DISPLACEMENT
                } else {
                    p
                };
            }
            pos = next;
        }
        // Low-frequency wobble, so edges meander rather than reading as ruled curves.
        let mut mids: HashMap<u64, Vec2> = HashMap::with_capacity(keys.len());
        for (i, &k) in keys.iter().enumerate() {
            let p = pos[i];
            let w = Vec2::new(
                fbm(p.x / WOBBLE_SCALE, p.y / WOBBLE_SCALE, seed ^ 0xC0_17_0A, 2),
                fbm(p.x / WOBBLE_SCALE + 57.0, p.y / WOBBLE_SCALE + 91.0, seed ^ 0xC0_17_0B, 2),
            );
            let moved = if nbrs[i].len() == 2 { p + (w - Vec2::splat(0.5)) * 2.0 * WOBBLE } else { p };
            mids.insert(k, moved);
        }

        let resolve = |e: End| match e {
            End::Mid(k) => mids[&k],
            End::Centre(p) => p,
        };
        let segs = raw.iter().map(|&(a, b, pair)| Seg { a: resolve(a), b: resolve(b), pair }).collect();
        Contour { n, joined, mids, segs }
    }

    /// The part of cell (ix, iz) belonging to each label, as convex polygons in cell-local
    /// (u, v) coordinates (0..1, u along x, v along z) - the fill-side counterpart of
    /// `segs`, built from the same relaxed vertices so the two always agree. `cell` holds
    /// the corner labels in TL, TR, BR, BL order. [`OPEN`] gets no polygon.
    pub fn cell_regions(&self, ix: usize, iz: usize, cell: [u32; 4]) -> Vec<(u32, Vec<Vec2>)> {
        let corner = [Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0), Vec2::new(1.0, 1.0), Vec2::new(0.0, 1.0)];
        let mut distinct: Vec<u32> = Vec::with_capacity(4);
        for &v in &cell {
            if !distinct.contains(&v) {
                distinct.push(v);
            }
        }
        if distinct.len() == 1 {
            return if cell[0] == OPEN { Vec::new() } else { vec![(cell[0], corner.to_vec())] };
        }

        let origin = grid_pos(ix, iz);
        let keys = edge_keys(self.n, ix, iz);
        let default = cell_mids(ix, iz);
        let mid: Vec<Vec2> = (0..4)
            .map(|i| (self.mids.get(&keys[i]).copied().unwrap_or(default[i]) - origin) / CELL)
            .collect();
        let centre = Vec2::splat(0.5);

        // Saddle: the lower label holds two opposite corners that the contour cuts off,
        // leaving the other label connected through the middle.
        let saddle = distinct.len() == 2 && cell[0] == cell[2] && cell[1] == cell[3];
        let cut_off = cut_off_label(&distinct, self.joined);
        let junction = distinct.len() > 2;

        let mut regions = Vec::new();
        for &id in distinct.iter().filter(|&&d| d != OPEN) {
            if saddle && id == cut_off {
                for i in 0..4 {
                    if cell[i] == id {
                        regions.push((id, vec![corner[i], mid[i], mid[(i + 3) % 4]]));
                    }
                }
                continue;
            }
            let mut poly = Vec::with_capacity(7);
            for i in 0..4 {
                let (c0, c1) = (cell[i], cell[(i + 1) % 4]);
                if c0 == id {
                    poly.push(corner[i]);
                }
                if c0 != c1 && (c0 == id || c1 == id) {
                    poly.push(mid[i]);
                    // Where three or more labels meet, the contour runs mid-edge to centre.
                    if junction && c0 == id {
                        poly.push(centre);
                    }
                }
            }
            regions.push((id, poly));
        }
        regions
    }
}

/// Sutherland-Hodgman clip of a convex polygon to the half-plane where `side(p) >= 0`.
pub fn clip_halfplane(poly: &[Vec2], side: impl Fn(Vec2) -> f32) -> Vec<Vec2> {
    let mut out = Vec::with_capacity(poly.len() + 2);
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        let (sa, sb) = (side(a), side(b));
        if sa >= 0.0 {
            out.push(a);
        }
        if (sa > 0.0 && sb < 0.0) || (sa < 0.0 && sb > 0.0) {
            out.push(a + (b - a) * (sa / (sa - sb)));
        }
    }
    out
}

/// A convex polygon's part inside the unit cell and on one side of its TR-BL diagonal
/// (`lower` is the TL side), matching how the terrain mesh splits each cell into triangles.
pub fn clip_to_terrain_triangle(poly: &[Vec2], lower: bool) -> Vec<Vec2> {
    let mut p = clip_halfplane(poly, |p| p.x);
    p = clip_halfplane(&p, |p| 1.0 - p.x);
    p = clip_halfplane(&p, |p| p.y);
    p = clip_halfplane(&p, |p| 1.0 - p.y);
    if lower {
        clip_halfplane(&p, |p| 1.0 - p.x - p.y)
    } else {
        clip_halfplane(&p, |p| p.x + p.y - 1.0)
    }
}

/// Ear-clipping triangulation of a small simple polygon of either winding. Relaxed contour
/// vertices can make a cell's polygon non-convex, where a simple fan from one vertex would
/// throw triangles outside it.
pub fn triangulate(poly: &[Vec2]) -> Vec<[Vec2; 3]> {
    let cross = |a: Vec2, b: Vec2, c: Vec2| (b - a).perp_dot(c - a);
    let mut idx: Vec<usize> = (0..poly.len()).collect();
    let area: f32 = (0..poly.len()).map(|i| poly[i].perp_dot(poly[(i + 1) % poly.len()])).sum();
    let orient = if area >= 0.0 { 1.0 } else { -1.0 };
    let mut tris = Vec::with_capacity(poly.len().saturating_sub(2));
    while idx.len() > 3 {
        let m = idx.len();
        let ear = (0..m).find(|&k| {
            let (a, b, c) = (poly[idx[(k + m - 1) % m]], poly[idx[k]], poly[idx[(k + 1) % m]]);
            if cross(a, b, c) * orient <= 1e-9 {
                return false;
            }
            !idx.iter().any(|&j| {
                let p = poly[j];
                p != a && p != b && p != c && cross(a, b, p) * orient > 0.0 && cross(b, c, p) * orient > 0.0 && cross(c, a, p) * orient > 0.0
            })
        });
        match ear {
            Some(k) => {
                tris.push([poly[idx[(k + m - 1) % m]], poly[idx[k]], poly[idx[(k + 1) % m]]]);
                idx.remove(k);
            }
            // Degenerate (self-touching) polygon: drop the vertex rather than loop forever.
            None => {
                idx.remove(0);
            }
        }
    }
    if idx.len() == 3 {
        tris.push([poly[idx[0]], poly[idx[1]], poly[idx[2]]]);
    }
    tris
}

//! Solid things the player can't walk through: tree trunks, buildings, field walls, hedges and
//! fences. Each is a shape on the ground plane with a height, kept in a spatial grid.
//!
//! The player is a short upright cylinder. Anything higher than a step blocks them (they slide
//! along it); anything they have got up to the top of, they can stand on; and anything not too
//! much higher than they can reach, they can climb ("mantle") by pressing into it with jump held.
//! A walker gets over a fence by jumping at it, or a wall or hedge by hauling themselves up and
//! over, then dropping down the far side.
//!
//! Everything here is plain geometry on `Vec2` and `f32`, so it can be tested without a game.

use std::collections::HashMap;

use bevy::prelude::*;

/// How wide the player is (a radius, so 0.8 m across).
pub const PLAYER_RADIUS: f32 = 0.4;
/// The tallest thing the player simply steps onto while walking.
pub const STEP_UP: f32 = 0.5;
/// A drop shorter than this is walked down; a longer one is a fall.
pub const STEP_DOWN: f32 = 0.6;
/// The most the top of something can be above the player's feet for them to climb it.
pub const MANTLE_REACH: f32 = 1.8;
/// How far beyond the body a hand can reach out to grab an edge.
const ARM: f32 = 0.35;
/// How far past the edge of something a person standing on it can have their middle before they
/// step off.
const LEDGE: f32 = 0.1;

/// The side of a square of the grid the solids are filed in, metres. Small squares mean a lookup
/// has few solids to look at; a solid is filed under every square it touches, so big solids (a
/// shed) take a few dozen entries and a tree takes four.
pub const DEFAULT_BUCKET: f32 = 4.0;
/// Shapes are filed under every square within this of them, so that a lookup in a point's own square
/// finds everything within this of the point. It must be at least as large as the longest reach any
/// lookup uses (the body's radius plus an arm).
const FILE_MARGIN: f32 = 1.0;

/// What a solid is made of, which decides what a bullet does to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Material {
    /// Tree trunks and fences.
    Wood,
    /// Walls and buildings.
    Stone,
    /// Hedges.
    Leaves,
    /// Sheds.
    Metal,
}

/// A shape on the ground plane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    Circle { centre: Vec2, radius: f32 },
    /// A thick straight line: a wall, a hedge, a fence.
    Wall { a: Vec2, b: Vec2, half_thickness: f32 },
    /// A rectangle turned `yaw` (as an `Quat::from_rotation_y` would turn it) about its centre.
    Box { centre: Vec2, half: Vec2, yaw: f32 },
}

fn rotate(v: Vec2, yaw: f32) -> Vec2 {
    // The same turn as `Quat::from_rotation_y(yaw)` gives a point in the (x, z) plane.
    let (s, c) = yaw.sin_cos();
    Vec2::new(v.x * c + v.y * s, -v.x * s + v.y * c)
}

impl Shape {
    /// How far `p` is outside the shape's surface (negative if inside), and the way the surface
    /// faces at the nearest point (pointing away from the shape).
    pub fn separation(&self, p: Vec2) -> (f32, Vec2) {
        match *self {
            Shape::Circle { centre, radius } => {
                let d = p - centre;
                (d.length() - radius, d.try_normalize().unwrap_or(Vec2::Y))
            }
            Shape::Wall { a, b, half_thickness } => {
                let q = closest_on_segment(a, b, p);
                let d = p - q;
                let away = (b - a).try_normalize().map_or(Vec2::Y, |u| Vec2::new(-u.y, u.x));
                (d.length() - half_thickness, d.try_normalize().unwrap_or(away))
            }
            Shape::Box { centre, half, yaw } => {
                let local = rotate(p - centre, -yaw);
                let q = local.abs() - half;
                let outside = q.max(Vec2::ZERO);
                let normal_local = if outside.length() > 1e-6 {
                    outside * local.signum()
                } else if q.x > q.y {
                    Vec2::new(local.x.signum(), 0.0)
                } else {
                    Vec2::new(0.0, local.y.signum())
                };
                (outside.length() + q.x.max(q.y).min(0.0), rotate(normal_local.normalize_or(Vec2::Y), yaw))
            }
        }
    }

    /// The part of the straight run from `a` to `b` that is inside the shape, as a range of the way
    /// along it (0 to 1): where it goes in, and where it comes out (or the end, if it stops inside).
    pub fn segment_interval(&self, a: Vec2, b: Vec2) -> Option<(f32, f32)> {
        match *self {
            Shape::Circle { centre, radius } => circle_interval(a, b, centre, radius),
            Shape::Box { centre, half, yaw } => slab_interval(rotate(a - centre, -yaw), rotate(b - centre, -yaw), half),
            Shape::Wall { a: wa, b: wb, half_thickness } => {
                // A capsule: the rectangle along the wall's middle line and a round cap at each end.
                // Their union is convex, so the run inside it is one range.
                let axis = wb - wa;
                let length = axis.length();
                let u = axis.try_normalize().unwrap_or(Vec2::X);
                let to_local = |p: Vec2| {
                    let d = p - (wa + wb) * 0.5;
                    Vec2::new(d.dot(u), d.dot(Vec2::new(-u.y, u.x)))
                };
                [
                    slab_interval(to_local(a), to_local(b), Vec2::new(length * 0.5, half_thickness)),
                    circle_interval(a, b, wa, half_thickness),
                    circle_interval(a, b, wb, half_thickness),
                ]
                .into_iter()
                .flatten()
                .reduce(|x, y| (x.0.min(y.0), x.1.max(y.1)))
            }
        }
    }

    fn bounds(&self) -> (Vec2, Vec2) {
        match *self {
            Shape::Circle { centre, radius } => (centre - Vec2::splat(radius), centre + Vec2::splat(radius)),
            Shape::Wall { a, b, half_thickness } => (a.min(b) - Vec2::splat(half_thickness), a.max(b) + Vec2::splat(half_thickness)),
            Shape::Box { centre, half, .. } => (centre - Vec2::splat(half.length()), centre + Vec2::splat(half.length())),
        }
    }

    /// Where a person who has climbed onto this shape stands, climbing from `p`: the middle line
    /// of a wall, the middle of a post, a little way in from the edge of a box.
    fn standing_point(&self, p: Vec2) -> Vec2 {
        match *self {
            Shape::Circle { centre, .. } => centre,
            Shape::Wall { a, b, .. } => closest_on_segment(a, b, p),
            Shape::Box { centre, half, yaw } => {
                // The nearest point of the rectangle, then half a metre in from there toward its middle
                // (but not past it).
                let local = rotate(p - centre, -yaw);
                let edge = local.clamp(-half, half);
                let inward = Vec2::new(edge.x.signum() * edge.x.abs().min(0.5), edge.y.signum() * edge.y.abs().min(0.5));
                centre + rotate(edge - inward, yaw)
            }
        }
    }
}

/// Where the run `a` to `b` is inside a circle, as a range of the way along it.
fn circle_interval(a: Vec2, b: Vec2, centre: Vec2, radius: f32) -> Option<(f32, f32)> {
    let d = b - a;
    let f = a - centre;
    let (qa, qb, qc) = (d.dot(d), 2.0 * f.dot(d), f.dot(f) - radius * radius);
    if qa < 1e-12 {
        return (qc <= 0.0).then_some((0.0, 1.0));
    }
    let disc = qb * qb - 4.0 * qa * qc;
    if disc < 0.0 {
        return None;
    }
    let root = disc.sqrt();
    let (t0, t1) = ((-qb - root) / (2.0 * qa), (-qb + root) / (2.0 * qa));
    let (t0, t1) = (t0.max(0.0), t1.min(1.0));
    (t0 <= t1).then_some((t0, t1))
}

/// Where the run `a` to `b` is inside the rectangle that is `half` either way of the origin.
fn slab_interval(a: Vec2, b: Vec2, half: Vec2) -> Option<(f32, f32)> {
    let d = b - a;
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for axis in 0..2 {
        if d[axis].abs() < 1e-9 {
            if a[axis].abs() > half[axis] {
                return None;
            }
        } else {
            let (x, y) = ((-half[axis] - a[axis]) / d[axis], (half[axis] - a[axis]) / d[axis]);
            t0 = t0.max(x.min(y));
            t1 = t1.min(x.max(y));
            if t0 > t1 {
                return None;
            }
        }
    }
    Some((t0, t1))
}

fn closest_on_segment(a: Vec2, b: Vec2, p: Vec2) -> Vec2 {
    let ab = b - a;
    let len2 = ab.length_squared();
    if len2 < 1e-9 {
        return a;
    }
    a + ab * ((p - a).dot(ab) / len2).clamp(0.0, 1.0)
}

/// A solid: a shape, and how high it reaches (absolute heights, at the first and second end for
/// a wall, so that it can follow a slope; the same for every other shape).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Solid {
    pub shape: Shape,
    pub top: [f32; 2],
    pub material: Material,
}

impl Solid {
    pub fn circle(centre: Vec2, radius: f32, top: f32) -> Self {
        Solid { shape: Shape::Circle { centre, radius }, top: [top, top], material: Material::Stone }
    }

    pub fn wall(a: Vec2, b: Vec2, half_thickness: f32, top_a: f32, top_b: f32) -> Self {
        Solid { shape: Shape::Wall { a, b, half_thickness }, top: [top_a, top_b], material: Material::Stone }
    }

    pub fn rect(centre: Vec2, half: Vec2, yaw: f32, top: f32) -> Self {
        Solid { shape: Shape::Box { centre, half, yaw }, top: [top, top], material: Material::Stone }
    }

    /// The same solid, made of something else.
    pub fn of(self, material: Material) -> Self {
        Solid { material, ..self }
    }

    /// How high the solid is above or beside `p`.
    pub fn top_at(&self, p: Vec2) -> f32 {
        match self.shape {
            Shape::Wall { a, b, .. } => {
                let ab = b - a;
                let t = if ab.length_squared() > 1e-9 { ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
                self.top[0] + (self.top[1] - self.top[0]) * t
            }
            _ => self.top[0],
        }
    }
}

/// Somewhere the player can haul themselves up onto.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mantle {
    /// Where they end up standing, and how high that is.
    pub land: Vec2,
    pub top: f32,
}

/// The crown of a tree: an ellipsoid of leaves, high above the ground. Nobody walks into it, but a
/// bullet going through it is slowed and bent by the leaves and twigs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Canopy {
    pub centre: Vec3,
    /// Half its width, height and depth.
    pub radii: Vec3,
}

impl Canopy {
    /// The part of the straight run from `a` to `b` inside the crown, as a range of the way along it.
    fn interval(&self, a: Vec3, b: Vec3) -> Option<(f32, f32)> {
        // In units of the radii the crown is a unit sphere.
        let (pa, pb) = ((a - self.centre) / self.radii, (b - self.centre) / self.radii);
        let d = pb - pa;
        let (qa, qb, qc) = (d.dot(d), 2.0 * pa.dot(d), pa.dot(pa) - 1.0);
        if qa < 1e-12 {
            return (qc <= 0.0).then_some((0.0, 1.0));
        }
        let disc = qb * qb - 4.0 * qa * qc;
        if disc < 0.0 {
            return None;
        }
        let root = disc.sqrt();
        let (t0, t1) = (((-qb - root) / (2.0 * qa)).max(0.0), ((-qb + root) / (2.0 * qa)).min(1.0));
        (t0 <= t1).then_some((t0, t1))
    }
}

/// How much a metre of a hedge counts as, and a metre of a tree's crown: a bullet is slowed by the
/// "foliage depth" it goes through, which is the length weighted by these. A hedge is dense; a crown,
/// mostly gaps between the leaves.
pub const HEDGE_DENSITY: f32 = 1.0;
pub const CANOPY_DENSITY: f32 = 0.35;

/// Where a flying thing met a solid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SolidHit {
    /// How far along the run it was (0 to 1).
    pub t: f32,
    pub point: Vec3,
    /// The way the surface faces there.
    pub normal: Vec3,
    pub material: Material,
}

/// Every solid in the world.
///
/// They are kept in a uniform grid of small squares (a spatial hash), each holding the solids that
/// touch it, so finding what is near a point is looking in one square rather than through all of
/// them. That beats a tree here: a lookup is one hash and a handful of solids however many there are
/// (a tree's is a descent of a dozen levels), the solids are spread fairly evenly, and no solid is
/// bigger than a few squares.
#[derive(Resource)]
pub struct Colliders {
    solids: Vec<Solid>,
    /// Each solid's bounding box, tested before the shape itself.
    bounds: Vec<(Vec2, Vec2)>,
    bucket: f32,
    buckets: HashMap<(i32, i32), Vec<u32>>,
    canopies: Vec<Canopy>,
    canopy_buckets: HashMap<(i32, i32), Vec<u32>>,
}

impl Default for Colliders {
    fn default() -> Self {
        Colliders::with_bucket(DEFAULT_BUCKET)
    }
}

impl Colliders {
    /// An empty world whose grid has squares of the given size.
    pub fn with_bucket(bucket: f32) -> Self {
        Colliders { solids: Vec::new(), bounds: Vec::new(), bucket, buckets: HashMap::new(), canopies: Vec::new(), canopy_buckets: HashMap::new() }
    }

    pub fn len(&self) -> usize {
        self.solids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.solids.is_empty()
    }

    pub fn clear(&mut self) {
        self.solids.clear();
        self.bounds.clear();
        self.buckets.clear();
        self.canopies.clear();
        self.canopy_buckets.clear();
    }

    pub fn add_canopy(&mut self, canopy: Canopy) {
        let id = self.canopies.len() as u32;
        let reach = canopy.radii.x.max(canopy.radii.z) + FILE_MARGIN;
        let (lo, hi) = (Vec2::new(canopy.centre.x, canopy.centre.z) - Vec2::splat(reach), Vec2::new(canopy.centre.x, canopy.centre.z) + Vec2::splat(reach));
        for bz in (lo.y / self.bucket).floor() as i32..=(hi.y / self.bucket).floor() as i32 {
            for bx in (lo.x / self.bucket).floor() as i32..=(hi.x / self.bucket).floor() as i32 {
                self.canopy_buckets.entry((bx, bz)).or_default().push(id);
            }
        }
        self.canopies.push(canopy);
    }

    pub fn canopy_count(&self) -> usize {
        self.canopies.len()
    }

    pub fn add(&mut self, solid: Solid) {
        let id = self.solids.len() as u32;
        let (lo, hi) = solid.shape.bounds();
        let (filed_lo, filed_hi) = (lo - Vec2::splat(FILE_MARGIN), hi + Vec2::splat(FILE_MARGIN));
        for bz in (filed_lo.y / self.bucket).floor() as i32..=(filed_hi.y / self.bucket).floor() as i32 {
            for bx in (filed_lo.x / self.bucket).floor() as i32..=(filed_hi.x / self.bucket).floor() as i32 {
                self.buckets.entry((bx, bz)).or_default().push(id);
            }
        }
        self.bounds.push((lo, hi));
        self.solids.push(solid);
    }

    /// The solids that could be within `reach` of `p`: those filed in its square whose bounding
    /// boxes, grown by `reach`, hold the point.
    fn near(&self, p: Vec2, reach: f32) -> impl Iterator<Item = &Solid> {
        debug_assert!(reach <= FILE_MARGIN, "lookups can't reach further than solids are filed for");
        self.buckets
            .get(&((p.x / self.bucket).floor() as i32, (p.y / self.bucket).floor() as i32))
            .into_iter()
            .flatten()
            .filter(move |&&id| {
                let (lo, hi) = self.bounds[id as usize];
                p.x >= lo.x - reach && p.x <= hi.x + reach && p.y >= lo.y - reach && p.y <= hi.y + reach
            })
            .map(|&id| &self.solids[id as usize])
    }

    /// The first solid that a straight run from `from` to `to` (a bullet's path over a moment) hits,
    /// if any. The solids stand from the ground up to their tops, so a run that stays above one
    /// passes over it; one that comes down onto it from above hits its top.
    pub fn segment_hit(&self, from: Vec3, to: Vec3) -> Option<SolidHit> {
        // Long runs are looked at a piece at a time, so that every lookup is a short one.
        const PIECE: f32 = 1.5;
        let pieces = ((from.distance(to) / PIECE).ceil() as usize).max(1);
        for i in 0..pieces {
            let (a, b) = (from.lerp(to, i as f32 / pieces as f32), from.lerp(to, (i + 1) as f32 / pieces as f32));
            if let Some(mut hit) = self.piece_hit(a, b) {
                hit.t = (i as f32 + hit.t) / pieces as f32;
                return Some(hit);
            }
        }
        None
    }

    /// How much foliage a straight run from `from` to `to` goes through: the length of it that is
    /// inside a hedge (below its top) or the crown of a tree, each weighted by how dense it is.
    pub fn foliage_depth(&self, from: Vec3, to: Vec3) -> f32 {
        const PIECE: f32 = 1.5;
        let pieces = ((from.distance(to) / PIECE).ceil() as usize).max(1);
        (0..pieces)
            .map(|i| self.piece_foliage(from.lerp(to, i as f32 / pieces as f32), from.lerp(to, (i + 1) as f32 / pieces as f32)))
            .sum()
    }

    fn piece_foliage(&self, from: Vec3, to: Vec3) -> f32 {
        let (a, b) = (Vec2::new(from.x, from.z), Vec2::new(to.x, to.z));
        let (middle, length) = ((a + b) * 0.5, from.distance(to));
        let mut depth = 0.0;
        // Hedges: the part of the run inside one that is also lower than its top.
        for solid in self.near(middle, (b - a).length() * 0.5 + 0.01).filter(|s| s.material == Material::Leaves) {
            let Some((t_in, t_out)) = solid.shape.segment_interval(a, b) else { continue };
            let top = solid.top_at(a.lerp(b, t_in));
            let dy = to.y - from.y;
            let (lo, hi) = if dy.abs() < 1e-9 {
                if from.y > top {
                    continue;
                }
                (t_in, t_out)
            } else {
                let t_top = (top - from.y) / dy;
                if dy < 0.0 { (t_in.max(t_top), t_out) } else { (t_in, t_out.min(t_top)) }
            };
            if hi > lo {
                depth += (hi - lo) * length * HEDGE_DENSITY;
            }
        }
        // Tree crowns.
        if let Some(ids) = self.canopy_buckets.get(&((middle.x / self.bucket).floor() as i32, (middle.y / self.bucket).floor() as i32)) {
            for &id in ids {
                let canopy = &self.canopies[id as usize];
                if let Some((t0, t1)) = canopy.interval(from, to) {
                    depth += (t1 - t0) * length * CANOPY_DENSITY;
                }
            }
        }
        depth
    }

    fn piece_hit(&self, from: Vec3, to: Vec3) -> Option<SolidHit> {
        let (a, b) = (Vec2::new(from.x, from.z), Vec2::new(to.x, to.z));
        let mut best: Option<SolidHit> = None;
        for solid in self.near((a + b) * 0.5, (b - a).length() * 0.5 + 0.01) {
            // A bullet goes through foliage rather than stopping at it (see `foliage_depth`).
            if solid.material == Material::Leaves {
                continue;
            }
            let Some((t_in, t_out)) = solid.shape.segment_interval(a, b) else { continue };
            let height_at = |t: f32| from.y + (to.y - from.y) * t;
            let entry = a.lerp(b, t_in);
            let top = solid.top_at(entry);
            let hit = if height_at(t_in) <= top {
                // Goes in through the side.
                let (_, n) = solid.shape.separation(entry);
                Some((t_in, Vec3::new(n.x, 0.0, n.y)))
            } else if height_at(t_out) < solid.top_at(a.lerp(b, t_out)) {
                // Over the edge, but coming down onto the top before it is out the other side.
                let drop = height_at(t_in) - top;
                let fall = height_at(t_in) - height_at(t_out);
                Some((t_in + (t_out - t_in) * (drop / fall.max(1e-6)).clamp(0.0, 1.0), Vec3::Y))
            } else {
                None
            };
            if let Some((t, normal)) = hit {
                if best.is_none_or(|b| t < b.t) {
                    let at = a.lerp(b, t);
                    let y = if normal == Vec3::Y { solid.top_at(at) } else { height_at(t) };
                    best = Some(SolidHit { t, point: Vec3::new(at.x, y, at.y), normal, material: solid.material });
                }
            }
        }
        best
    }

    /// For diagnostics: how many solids a lookup at `p` with this `reach` has to look at, before
    /// and after the bounding-box test.
    pub fn examined(&self, p: Vec2, reach: f32) -> (usize, usize) {
        let filed = self
            .buckets
            .get(&((p.x / self.bucket).floor() as i32, (p.y / self.bucket).floor() as i32))
            .map_or(0, Vec::len);
        (filed, self.near(p, reach).count())
    }

    /// Moves a body of `radius` standing with its feet at height `feet`, which has been moved to
    /// `p`, back out of everything it can't pass: solids higher than `step` above its feet. It
    /// slides along what it meets, because only the part of the move that goes into a surface is
    /// undone.
    pub fn resolve(&self, p: Vec2, radius: f32, feet: f32, step: f32) -> Vec2 {
        let mut pos = p;
        for _ in 0..4 {
            let mut pushed = false;
            for solid in self.near(pos, radius) {
                if solid.top_at(pos) <= feet + step {
                    continue;
                }
                let (distance, normal) = solid.shape.separation(pos);
                if distance < radius {
                    pos += normal * (radius - distance);
                    pushed = true;
                }
            }
            if !pushed {
                break;
            }
        }
        pos
    }

    /// The height of the highest thing a body at `p` whose feet are at `feet` is standing on top
    /// of, if any: a solid whose top is no more than `step` above its feet (so it can't be one
    /// it's blocked by) and that it is over.
    pub fn support(&self, p: Vec2, feet: f32, step: f32) -> Option<f32> {
        self.near(p, LEDGE)
            .filter(|solid| solid.shape.separation(p).0 <= LEDGE)
            .map(|solid| solid.top_at(p))
            .filter(|&top| top <= feet + step)
            .max_by(f32::total_cmp)
    }

    /// Something ahead (in direction `facing`) of a body at `p` that it can climb: its top is more
    /// than `step` above the feet (otherwise it would just be stepped onto) and no more than
    /// `reach` above the `floor` they are over (a jump doesn't lengthen an arm), and close enough
    /// to grab.
    pub fn mantle_target(&self, p: Vec2, facing: Vec2, radius: f32, feet: f32, floor: f32, step: f32, reach: f32) -> Option<Mantle> {
        let facing = facing.try_normalize()?;
        let mut best: Option<(f32, Mantle)> = None;
        for solid in self.near(p, radius + ARM) {
            let (distance, normal) = solid.shape.separation(p);
            let top = solid.top_at(p);
            // Within arm's reach, in front of them, and a height that can be got up to.
            if distance > radius + ARM || normal.dot(facing) > -0.35 || top <= feet + step || top > floor + reach {
                continue;
            }
            let land = solid.shape.standing_point(p);
            let mantle = Mantle { land, top: solid.top_at(land) };
            if best.is_none_or(|(d, _)| distance < d) {
                best = Some((distance, mantle));
            }
        }
        best.map(|(_, m)| m)
    }
}

/// What happens to a body when the floor beneath it is `floor` high and its feet are at `feet`
/// (before it has settled): its height above the floor, and whether it is standing on it. A body
/// that was on the ground stays on it over bumps and slopes (up, or down by no more than
/// `step_down`), but steps off a bigger drop and falls; one in the air comes down onto the floor.
pub fn settle(feet: f32, was_grounded: bool, floor: f32, step_down: f32) -> (f32, bool) {
    let above = feet - floor;
    if was_grounded {
        if above <= step_down {
            (0.0, true)
        } else {
            (above, false)
        }
    } else if above <= 0.0 {
        (0.0, true)
    } else {
        (above, false)
    }
}

pub struct CollisionPlugin;

impl Plugin for CollisionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Colliders>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: f32 = PLAYER_RADIUS;

    fn world(solids: &[Solid]) -> Colliders {
        let mut c = Colliders::default();
        for &s in solids {
            c.add(s);
        }
        c
    }

    #[test]
    fn a_circle_is_measured_from_its_surface() {
        let c = Shape::Circle { centre: Vec2::new(2.0, 0.0), radius: 0.5 };
        let (d, n) = c.separation(Vec2::new(4.0, 0.0));
        assert!((d - 1.5).abs() < 1e-5 && (n - Vec2::X).length() < 1e-5);
        assert!(c.separation(Vec2::new(2.2, 0.0)).0 < 0.0, "inside");
    }

    #[test]
    fn a_wall_is_a_capsule_round_its_centre_line() {
        let w = Shape::Wall { a: Vec2::new(-5.0, 0.0), b: Vec2::new(5.0, 0.0), half_thickness: 0.3 };
        let (d, n) = w.separation(Vec2::new(0.0, 1.3));
        assert!((d - 1.0).abs() < 1e-5 && (n - Vec2::Y).length() < 1e-5);
        let (d, n) = w.separation(Vec2::new(0.0, -2.3));
        assert!((d - 2.0).abs() < 1e-5 && (n + Vec2::Y).length() < 1e-5);
        let (d, _) = w.separation(Vec2::new(6.0, 0.0));
        assert!((d - 0.7).abs() < 1e-5, "past its end it is a rounded cap: {d}");
        assert!(w.separation(Vec2::new(1.0, 0.1)).0 < 0.0, "inside");
    }

    #[test]
    fn a_box_agrees_with_the_way_the_rendered_one_is_turned() {
        // A building 8 m by 6 m, turned 0.7 radians as `Quat::from_rotation_y` would turn it.
        let (centre, half, yaw) = (Vec2::new(10.0, -4.0), Vec2::new(4.0, 3.0), 0.7);
        let shape = Shape::Box { centre, half, yaw };
        let turn = Quat::from_rotation_y(yaw);
        for (local, inside) in [(Vec3::new(3.5, 0.0, 2.5), true), (Vec3::new(4.5, 0.0, 0.0), false), (Vec3::new(0.0, 0.0, 3.4), false), (Vec3::new(-3.9, 0.0, -2.9), true)] {
            let world = turn * local;
            let p = centre + Vec2::new(world.x, world.z);
            assert_eq!(shape.separation(p).0 < 0.0, inside, "{local:?} -> {p:?}");
        }
        // Outside one end: 1 m from the surface along the building's own length axis.
        let end = turn * Vec3::new(5.0, 0.0, 0.0);
        let (d, n) = shape.separation(centre + Vec2::new(end.x, end.z));
        let expected = turn * Vec3::X;
        assert!((d - 1.0).abs() < 1e-4, "{d}");
        assert!((n - Vec2::new(expected.x, expected.z)).length() < 1e-4);
    }

    #[test]
    fn walking_into_a_wall_stops_you_a_body_radius_from_its_face() {
        let wall = Solid::wall(Vec2::new(-10.0, -3.0), Vec2::new(10.0, -3.0), 0.3, 3.0, 3.0);
        let c = world(&[wall]);
        let stopped = c.resolve(Vec2::new(0.0, -2.9), R, 0.0, STEP_UP);
        assert!((stopped.y - (-3.0 + 0.3 + R)).abs() < 1e-4, "{stopped:?}");
        assert_eq!(stopped.x, 0.0);
    }

    #[test]
    fn you_slide_along_a_wall_rather_than_sticking_to_it() {
        let wall = Solid::wall(Vec2::new(-10.0, -3.0), Vec2::new(10.0, -3.0), 0.3, 3.0, 3.0);
        let c = world(&[wall]);
        // Moving diagonally into it: the part along the wall is kept.
        let before = Vec2::new(0.0, -2.3);
        let wanted = before + Vec2::new(0.5, -0.2);
        let after = c.resolve(wanted, R, 0.0, STEP_UP);
        assert!((after.x - 0.5).abs() < 1e-4, "kept the sideways move: {after:?}");
        assert!((after.y - (-3.0 + 0.3 + R)).abs() < 1e-4);
    }

    #[test]
    fn low_things_are_stepped_over_not_blocked_by() {
        let kerb = Solid::wall(Vec2::new(-10.0, -3.0), Vec2::new(10.0, -3.0), 0.3, 0.4, 0.4);
        let c = world(&[kerb]);
        let p = Vec2::new(0.0, -3.0);
        assert_eq!(c.resolve(p, R, 0.0, STEP_UP), p, "0.4 m is a step");
        let wall = Solid::wall(Vec2::new(-10.0, -3.0), Vec2::new(10.0, -3.0), 0.3, 0.7, 0.7);
        assert_ne!(world(&[wall]).resolve(p, R, 0.0, STEP_UP), p, "0.7 m is not");
    }

    #[test]
    fn something_you_are_above_does_not_block_you() {
        // On top of a 1.1 m wall (feet at 1.1), the wall is no obstacle.
        let wall = Solid::wall(Vec2::new(-10.0, -3.0), Vec2::new(10.0, -3.0), 0.3, 1.1, 1.1);
        let c = world(&[wall]);
        let p = Vec2::new(0.0, -3.0);
        assert_eq!(c.resolve(p, R, 1.1, STEP_UP), p);
        // And mid-jump, with the feet above the step height of it.
        assert_eq!(c.resolve(p, R, 0.7, STEP_UP), p);
        assert_ne!(c.resolve(p, R, 0.3, STEP_UP), p, "but a low jump isn't enough");
    }

    #[test]
    fn a_wall_that_climbs_a_slope_is_higher_where_the_ground_is() {
        let wall = Solid::wall(Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), 0.3, 1.0, 3.0);
        assert!((wall.top_at(Vec2::new(0.0, 0.0)) - 1.0).abs() < 1e-5);
        assert!((wall.top_at(Vec2::new(5.0, 0.0)) - 2.0).abs() < 1e-5);
        assert!((wall.top_at(Vec2::new(10.0, 1.0)) - 3.0).abs() < 1e-5);
    }

    #[test]
    fn trees_and_buildings_block_from_every_side() {
        let c = world(&[Solid::circle(Vec2::new(5.0, 5.0), 0.5, 8.0), Solid::rect(Vec2::new(-20.0, 0.0), Vec2::new(4.0, 3.0), 0.3, 6.0)]);
        let out = c.resolve(Vec2::new(5.2, 5.1), R, 0.0, STEP_UP);
        assert!((out.distance(Vec2::new(5.0, 5.0)) - (0.5 + R)).abs() < 1e-4);
        let out = c.resolve(Vec2::new(-20.0, 0.0), R, 0.0, STEP_UP);
        assert!(Shape::Box { centre: Vec2::new(-20.0, 0.0), half: Vec2::new(4.0, 3.0), yaw: 0.3 }.separation(out).0 >= R - 1e-3, "pushed fully out of the house: {out:?}");
    }

    #[test]
    fn you_can_stand_on_top_of_a_wall_you_have_got_up_to() {
        let wall = Solid::wall(Vec2::new(-10.0, -3.0), Vec2::new(10.0, -3.0), 0.3, 1.1, 1.1);
        let c = world(&[wall]);
        let on = Vec2::new(0.0, -3.0);
        assert_eq!(c.support(on, 1.1, STEP_UP), Some(1.1));
        assert_eq!(c.support(on, 0.8, STEP_UP), Some(1.1), "close enough below to step up");
        assert_eq!(c.support(on, 0.0, STEP_UP), None, "from the ground it is not underfoot, it is in the way");
        assert_eq!(c.support(Vec2::new(0.0, -2.0), 1.1, STEP_UP), None, "beside it is not on it");
        assert_eq!(c.support(Vec2::new(0.0, -3.0 + 0.3 + 0.05), 1.1, STEP_UP), Some(1.1), "a little over the edge is still on");
    }

    #[test]
    fn the_highest_thing_underfoot_wins() {
        let c = world(&[
            Solid::wall(Vec2::new(-5.0, 0.0), Vec2::new(5.0, 0.0), 0.5, 0.3, 0.3),
            Solid::wall(Vec2::new(-5.0, 0.0), Vec2::new(5.0, 0.0), 0.2, 0.6, 0.6),
        ]);
        assert_eq!(c.support(Vec2::ZERO, 0.6, STEP_UP), Some(0.6));
    }

    #[test]
    fn a_wall_ahead_within_reach_can_be_climbed() {
        let wall = Solid::wall(Vec2::new(-10.0, -3.0), Vec2::new(10.0, -3.0), 0.3, 1.1, 1.1);
        let c = world(&[wall]);
        let m = c.mantle_target(Vec2::new(2.0, -2.3), Vec2::NEG_Y, R, 0.0, 0.0, STEP_UP, MANTLE_REACH).expect("climbable");
        assert!((m.top - 1.1).abs() < 1e-5);
        assert!((m.land - Vec2::new(2.0, -3.0)).length() < 1e-4, "lands on the middle of the wall: {:?}", m.land);
    }

    #[test]
    fn what_cannot_be_climbed_is_not() {
        let from = Vec2::new(0.0, -2.3);
        // Too high, too low (just a step), behind, too far, and facing away.
        let too_high = world(&[Solid::wall(Vec2::new(-10.0, -3.0), Vec2::new(10.0, -3.0), 0.3, 2.5, 2.5)]);
        assert!(too_high.mantle_target(from, Vec2::NEG_Y, R, 0.0, 0.0, STEP_UP, MANTLE_REACH).is_none());
        let a_step = world(&[Solid::wall(Vec2::new(-10.0, -3.0), Vec2::new(10.0, -3.0), 0.3, 0.4, 0.4)]);
        assert!(a_step.mantle_target(from, Vec2::NEG_Y, R, 0.0, 0.0, STEP_UP, MANTLE_REACH).is_none());
        let wall = world(&[Solid::wall(Vec2::new(-10.0, -3.0), Vec2::new(10.0, -3.0), 0.3, 1.1, 1.1)]);
        assert!(wall.mantle_target(Vec2::new(0.0, -5.0), Vec2::NEG_Y, R, 0.0, 0.0, STEP_UP, MANTLE_REACH).is_none(), "too far to grab");
        assert!(wall.mantle_target(from, Vec2::Y, R, 0.0, 0.0, STEP_UP, MANTLE_REACH).is_none(), "facing away");
        assert!(wall.mantle_target(from, Vec2::NEG_Y, R, 0.0, 0.0, STEP_UP, MANTLE_REACH).is_some());
        assert!(wall.mantle_target(from, Vec2::ZERO, R, 0.0, 0.0, STEP_UP, MANTLE_REACH).is_none(), "not moving anywhere");
    }

    #[test]
    fn a_jump_does_not_lengthen_your_reach() {
        // Reach is measured from the floor: a 2.6 m hedge is out of reach even at the top of a jump.
        let hedge = world(&[Solid::wall(Vec2::new(-10.0, -3.0), Vec2::new(10.0, -3.0), 0.6, 2.6, 2.6)]);
        let from = Vec2::new(0.0, -2.0);
        assert!(hedge.mantle_target(from, Vec2::NEG_Y, R, 0.0, 0.0, STEP_UP, MANTLE_REACH).is_none());
        assert!(hedge.mantle_target(from, Vec2::NEG_Y, R, 1.0, 0.0, STEP_UP, MANTLE_REACH).is_none());
        // But standing on something higher, the reach goes up with the floor.
        assert!(hedge.mantle_target(from, Vec2::NEG_Y, R, 1.2, 1.2, STEP_UP, MANTLE_REACH).is_some());
    }

    #[test]
    fn a_low_building_roof_can_be_climbed_onto_from_any_side() {
        let shed = world(&[Solid::rect(Vec2::new(0.0, 0.0), Vec2::new(4.0, 3.0), 0.0, 1.4)]);
        let m = shed.mantle_target(Vec2::new(4.6, 0.0), Vec2::NEG_X, R, 0.0, 0.0, STEP_UP, MANTLE_REACH).expect("climbable");
        assert!(Shape::Box { centre: Vec2::ZERO, half: Vec2::new(4.0, 3.0), yaw: 0.0 }.separation(m.land).0 < 0.0, "lands on the roof: {:?}", m.land);
    }

    #[test]
    fn a_body_stays_on_the_ground_over_small_drops_and_falls_off_big_ones() {
        assert_eq!(settle(1.0, true, 1.0, STEP_DOWN), (0.0, true), "level");
        assert_eq!(settle(1.0, true, 0.6, STEP_DOWN), (0.0, true), "down a slope or a step");
        let (air, grounded) = settle(1.1, true, 0.0, STEP_DOWN);
        assert!(!grounded && (air - 1.1).abs() < 1e-6, "off a wall top, falling");
        assert_eq!(settle(0.5, true, 0.9, STEP_DOWN), (0.0, true), "up a step");
    }

    #[test]
    fn something_in_the_air_lands_on_the_floor() {
        assert_eq!(settle(0.8, false, 0.5, STEP_DOWN), (0.3, false), "still above it");
        assert_eq!(settle(0.5, false, 0.5, STEP_DOWN), (0.0, true), "touching down");
        assert_eq!(settle(0.2, false, 0.5, STEP_DOWN), (0.0, true), "onto a ledge it jumped up to");
    }

    #[test]
    fn solids_far_away_cost_nothing_and_a_big_world_is_quick() {
        let mut c = Colliders::default();
        for i in 0..200_000 {
            let (x, z) = ((i % 1000) as f32 * 4.0 - 2000.0, (i / 1000) as f32 * 4.0 - 400.0);
            c.add(Solid::circle(Vec2::new(x, z), 0.4, 8.0));
        }
        assert_eq!(c.len(), 200_000);
        let start = std::time::Instant::now();
        for i in 0..10_000 {
            let p = Vec2::new(i as f32 * 0.37 % 3000.0 - 1500.0, 100.0);
            let _ = c.resolve(p, R, 0.0, STEP_UP);
            let _ = c.support(p, 0.0, STEP_UP);
        }
        assert!(start.elapsed().as_millis() < 500, "10k lookups took {:?}", start.elapsed());
    }

    #[test]
    fn clearing_removes_everything() {
        let mut c = world(&[Solid::circle(Vec2::ZERO, 1.0, 5.0)]);
        c.clear();
        assert!(c.is_empty());
        assert_eq!(c.resolve(Vec2::ZERO, R, 0.0, STEP_UP), Vec2::ZERO);
    }

    // ---- bullets ------------------------------------------------------------------------------

    #[test]
    fn a_run_through_a_circle_goes_in_and_out_at_its_edges() {
        let c = Shape::Circle { centre: Vec2::new(0.0, -5.0), radius: 1.0 };
        let (t0, t1) = c.segment_interval(Vec2::ZERO, Vec2::new(0.0, -10.0)).expect("hits");
        assert!((t0 - 0.4).abs() < 1e-5 && (t1 - 0.6).abs() < 1e-5, "{t0} {t1}");
        assert!(c.segment_interval(Vec2::new(3.0, 0.0), Vec2::new(3.0, -10.0)).is_none(), "misses");
        assert!(c.segment_interval(Vec2::ZERO, Vec2::new(0.0, -3.0)).is_none(), "stops short");
        assert_eq!(c.segment_interval(Vec2::new(0.0, -5.0), Vec2::new(0.0, -10.0)).map(|i| i.0), Some(0.0), "starts inside");
    }

    #[test]
    fn a_run_through_a_wall_goes_in_at_its_face() {
        let w = Shape::Wall { a: Vec2::new(-10.0, -3.0), b: Vec2::new(10.0, -3.0), half_thickness: 0.3 };
        let (t0, t1) = w.segment_interval(Vec2::ZERO, Vec2::new(0.0, -6.0)).expect("hits");
        assert!((t0 - 2.7 / 6.0).abs() < 1e-5 && (t1 - 3.3 / 6.0).abs() < 1e-5, "{t0} {t1}");
        // Past its end it is the rounded cap that is hit.
        // 0.2 m in from the end, the cap is 0.3^2 - 0.2^2 = 0.05 wide either way of the wall's line.
        let cap = w.segment_interval(Vec2::new(10.2, 0.0), Vec2::new(10.2, -6.0)).expect("clips the cap");
        let half_chord = (0.3f32 * 0.3 - 0.2 * 0.2).sqrt();
        assert!((cap.0 - (3.0 - half_chord) / 6.0).abs() < 1e-4 && (cap.1 - (3.0 + half_chord) / 6.0).abs() < 1e-4, "{cap:?}");
        assert!(w.segment_interval(Vec2::new(10.6, 0.0), Vec2::new(10.6, -6.0)).is_none(), "beyond the cap");
        // Along it at an angle.
        assert!(w.segment_interval(Vec2::new(-12.0, -2.0), Vec2::new(12.0, -4.0)).is_some());
    }

    #[test]
    fn a_run_through_a_turned_box_goes_in_where_the_box_is() {
        let (centre, half, yaw) = (Vec2::new(5.0, 5.0), Vec2::new(4.0, 3.0), 0.7);
        let b = Shape::Box { centre, half, yaw };
        // Straight at the middle from far off: in at the face, out at the far one, symmetrically.
        let from = centre + Vec2::new(-20.0, 0.0);
        let (t0, t1) = b.segment_interval(from, centre + Vec2::new(20.0, 0.0)).expect("hits");
        assert!(((t0 + t1) * 0.5 - 0.5).abs() < 1e-4, "symmetric about the middle: {t0} {t1}");
        let entry = from.lerp(centre + Vec2::new(20.0, 0.0), t0);
        assert!(b.separation(entry).0.abs() < 1e-3, "entry is on the surface: {}", b.separation(entry).0);
        assert!(b.segment_interval(Vec2::new(-30.0, -30.0), Vec2::new(-20.0, -30.0)).is_none());
    }

    fn wall_world(height: f32) -> Colliders {
        world(&[Solid::wall(Vec2::new(-10.0, -3.0), Vec2::new(10.0, -3.0), 0.3, height, height).of(Material::Wood)])
    }

    #[test]
    fn a_bullet_going_at_a_wall_hits_its_face() {
        let c = wall_world(3.0);
        let hit = c.segment_hit(Vec3::new(0.0, 1.5, 0.0), Vec3::new(0.0, 1.5, -6.0)).expect("hits");
        assert!((hit.point.z - -2.7).abs() < 1e-4 && (hit.point.y - 1.5).abs() < 1e-4, "{:?}", hit.point);
        assert!((hit.normal - Vec3::Z).length() < 1e-4, "the face turned toward the shooter: {:?}", hit.normal);
        assert_eq!(hit.material, Material::Wood);
        assert!((hit.t - 2.7 / 6.0).abs() < 1e-4);
    }

    #[test]
    fn a_bullet_goes_over_a_low_wall_and_into_a_tall_one() {
        assert!(wall_world(1.1).segment_hit(Vec3::new(0.0, 1.5, 0.0), Vec3::new(0.0, 1.5, -6.0)).is_none(), "1.5 m up clears a 1.1 m wall");
        assert!(wall_world(1.1).segment_hit(Vec3::new(0.0, 0.9, 0.0), Vec3::new(0.0, 0.9, -6.0)).is_some(), "0.9 m up does not");
        assert!(wall_world(2.0).segment_hit(Vec3::new(0.0, 1.5, 0.0), Vec3::new(0.0, 1.5, -6.0)).is_some());
    }

    #[test]
    fn a_bullet_coming_down_onto_a_wall_hits_its_top() {
        let c = wall_world(1.1);
        // Over the near edge at 1.3 m, down to 0.9 m by the far edge: hits the top partway across.
        let hit = c.segment_hit(Vec3::new(0.0, 1.3, -2.5), Vec3::new(0.0, 0.7, -3.5)).expect("lands on top");
        assert!((hit.normal - Vec3::Y).length() < 1e-4, "{:?}", hit.normal);
        assert!((hit.point.y - 1.1).abs() < 1e-4 && hit.point.z < -2.7 && hit.point.z > -3.3, "{:?}", hit.point);
    }

    #[test]
    fn the_nearest_of_several_things_is_what_a_bullet_hits() {
        let c = world(&[
            Solid::circle(Vec2::new(0.0, -8.0), 0.5, 5.0).of(Material::Stone),
            Solid::circle(Vec2::new(0.0, -4.0), 0.5, 5.0).of(Material::Wood),
        ]);
        let hit = c.segment_hit(Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 1.0, -12.0)).expect("hits");
        assert_eq!(hit.material, Material::Wood);
        assert!((hit.point.z - -3.5).abs() < 1e-3);
    }

    #[test]
    fn long_runs_are_checked_the_whole_way() {
        // A 40 m run that only meets something at the far end.
        let c = world(&[Solid::circle(Vec2::new(0.0, -38.0), 0.5, 5.0)]);
        let hit = c.segment_hit(Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 1.0, -40.0)).expect("hits");
        assert!((hit.t - 37.5 / 40.0).abs() < 1e-4, "{}", hit.t);
        assert!(c.segment_hit(Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 1.0, -30.0)).is_none());
    }

    #[test]
    fn a_house_stops_a_bullet_from_any_side() {
        let c = world(&[Solid::rect(Vec2::new(10.0, 10.0), Vec2::new(4.0, 3.0), 0.7, 6.0)]);
        for angle in [0.0f32, 1.0, 2.0, 3.0, 4.0, 5.0] {
            let dir = Vec2::new(angle.cos(), angle.sin());
            let from = Vec2::new(10.0, 10.0) + dir * 20.0;
            let hit = c.segment_hit(Vec3::new(from.x, 2.0, from.y), Vec3::new(10.0, 2.0, 10.0)).expect("hits");
            let out = Vec2::new(hit.normal.x, hit.normal.z);
            assert!(out.dot(dir) > 0.3, "from {angle}: the face hit looks back toward the shooter: {out:?}");
            assert!(Shape::Box { centre: Vec2::new(10.0, 10.0), half: Vec2::new(4.0, 3.0), yaw: 0.7 }.separation(Vec2::new(hit.point.x, hit.point.z)).0.abs() < 1e-3);
        }
    }

    // ---- foliage ------------------------------------------------------------------------------

    fn hedge_world(top: f32) -> Colliders {
        world(&[Solid::wall(Vec2::new(-10.0, -3.0), Vec2::new(10.0, -3.0), 0.6, top, top).of(Material::Leaves)])
    }

    #[test]
    fn a_bullet_goes_through_a_hedge_not_into_it() {
        let c = hedge_world(1.5);
        assert!(c.segment_hit(Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 1.0, -6.0)).is_none(), "foliage doesn't stop it");
        // ...but a person still can't walk through it.
        assert_ne!(c.resolve(Vec2::new(0.0, -2.9), R, 0.0, STEP_UP), Vec2::new(0.0, -2.9));
    }

    #[test]
    fn a_hedges_depth_is_how_far_through_it_the_bullet_goes() {
        let c = hedge_world(1.5);
        let through = c.foliage_depth(Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 1.0, -6.0));
        assert!((through - 1.2).abs() < 1e-4, "the hedge is 1.2 m thick: {through}");
        // At an angle there is more of it.
        let slant = c.foliage_depth(Vec3::new(-3.0, 1.0, 0.0), Vec3::new(3.0, 1.0, -6.0));
        assert!((slant - 1.2 * 2.0f32.sqrt()).abs() < 1e-3, "{slant}");
        assert_eq!(c.foliage_depth(Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 1.0, -2.0)), 0.0, "stops short");
    }

    #[test]
    fn over_the_top_of_a_hedge_there_is_no_foliage() {
        let c = hedge_world(1.5);
        assert_eq!(c.foliage_depth(Vec3::new(0.0, 1.8, 0.0), Vec3::new(0.0, 1.8, -6.0)), 0.0);
        // Coming down into it, only the part below the top counts.
        let down = c.foliage_depth(Vec3::new(0.0, 1.8, -2.4), Vec3::new(0.0, 1.2, -3.6));
        assert!(down > 0.4 && down < 0.7, "about half of it: {down}");
    }

    #[test]
    fn a_trees_crown_slows_a_bullet_by_how_much_of_it_is_crossed() {
        let mut c = Colliders::default();
        c.add_canopy(Canopy { centre: Vec3::new(0.0, 6.0, -5.0), radii: Vec3::new(3.0, 3.0, 3.0) });
        let straight = c.foliage_depth(Vec3::new(0.0, 6.0, 0.0), Vec3::new(0.0, 6.0, -12.0));
        assert!((straight - 6.0 * CANOPY_DENSITY).abs() < 1e-3, "6 m across, at crown density: {straight}");
        let edge = c.foliage_depth(Vec3::new(2.0, 6.0, 0.0), Vec3::new(2.0, 6.0, -12.0));
        assert!(edge > 0.0 && edge < straight, "a chord near the edge is shorter: {edge}");
        assert_eq!(c.foliage_depth(Vec3::new(0.0, 12.0, 0.0), Vec3::new(0.0, 12.0, -12.0)), 0.0, "well above it");
        assert_eq!(c.foliage_depth(Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 1.0, -12.0)), 0.0, "and beneath it");
    }

    #[test]
    fn a_crown_is_not_an_obstacle_to_a_person() {
        let mut c = Colliders::default();
        c.add_canopy(Canopy { centre: Vec3::new(0.0, 6.0, 0.0), radii: Vec3::splat(3.0) });
        assert_eq!(c.resolve(Vec2::ZERO, R, 0.0, STEP_UP), Vec2::ZERO);
        assert!(c.segment_hit(Vec3::new(0.0, 6.0, 5.0), Vec3::new(0.0, 6.0, -5.0)).is_none(), "nor does it stop a bullet outright");
    }

    #[test]
    fn foliage_depth_adds_up_over_a_long_run() {
        let mut c = Colliders::default();
        c.add_canopy(Canopy { centre: Vec3::new(0.0, 5.0, -20.0), radii: Vec3::splat(4.0) });
        let whole = c.foliage_depth(Vec3::new(0.0, 5.0, 0.0), Vec3::new(0.0, 5.0, -40.0));
        let halves = c.foliage_depth(Vec3::new(0.0, 5.0, 0.0), Vec3::new(0.0, 5.0, -20.0)) + c.foliage_depth(Vec3::new(0.0, 5.0, -20.0), Vec3::new(0.0, 5.0, -40.0));
        assert!((whole - halves).abs() < 1e-3 && (whole - 8.0 * CANOPY_DENSITY).abs() < 1e-3, "{whole} {halves}");
    }
}

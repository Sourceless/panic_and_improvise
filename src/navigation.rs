//! Finding a way round things: a search over a grid laid on the ground, for anything that has to get from
//! one place to another with walls, fences, water and stairs between. It knows nothing of the world; it is
//! asked, a point at a time, where a body that is going there would end up standing, if it can stand there
//! at all.
//!
//! Places have a height as well as a position, so that the same square on two floors of a house are two
//! different places, and the way from one to the other goes by the stair.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use bevy::prelude::*;

/// How far apart the points searched are, metres. A doorway is a metre wide and a body takes up half of it,
/// so the points must be closer together than the gap that is left.
pub const CELL: f32 = 0.4;

/// The most points a search looks at before it settles for the nearest it has got.
pub const SEARCH_LIMIT: usize = 6000;

/// How near the goal, in cells, is near enough (the goal itself may be inside a wall or a counter).
const GOAL_CELLS: i32 = 2;

/// How far, metres, the height of a place can be from the goal's for it to be at the goal (on its floor).
const SAME_LEVEL: f32 = 1.0;

/// How far, metres, the height of a place on the way can be from where a straight walk to it comes out, for
/// the walk to be taken for a way to it: a step, not a flight.
const SAME_PLACE: f32 = 0.3;

/// How much the search favours points that are nearer the goal over points that are nearer the start:
/// more is quicker and finds longer ways round, less is truer to the shortest.
const GREED: f32 = 1.8;

/// How much a metre of height still to be gained or lost counts for against a cell still to be crossed. A
/// stair or a ramp gains less than a cell of height for each it crosses, so this is big enough that
/// climbing one always looks like getting somewhere.
const HEIGHT_CELLS: f32 = 8.0;

/// While the goal is a floor up or down, how much the search favours places nearer it in the plan: little, so
/// that it looks about for the way up instead of crowding in underneath it.
const GREED_ELSEWHERE: f32 = 1.0;

/// A way found: the places to go through in turn (not the start; x, feet, z), and whether the last of them
/// is at the goal or only the nearest that could be got to.
#[derive(Debug, Clone, PartialEq)]
pub struct Route {
    pub points: Vec<Vec3>,
    pub arrives: bool,
}

#[derive(PartialEq)]
struct Queued {
    score: f32,
    place: Place,
}

impl Eq for Queued {}

impl Ord for Queued {
    fn cmp(&self, other: &Self) -> Ordering {
        // The lowest score first.
        other.score.total_cmp(&self.score)
    }
}

impl PartialOrd for Queued {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// A square, and which floor of it: the height in half metres.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct Place {
    cell: IVec2,
    level: i32,
}

fn level_of(feet: f32) -> i32 {
    (feet * 2.0).round() as i32
}

/// The distance over a grid, steps of one and of a diagonal, in cells.
fn octile(a: IVec2, b: IVec2) -> f32 {
    let d = (a - b).abs();
    let (long, short) = (d.x.max(d.y) as f32, d.x.min(d.y) as f32);
    long + (std::f32::consts::SQRT_2 - 1.0) * short
}

fn xz(p: Vec3) -> Vec2 {
    Vec2::new(p.x, p.z)
}

/// Whether a body can go straight from `from` to `to`: every point of the way, a step or less apart, is
/// somewhere it can stand, and it ends up at the height of `to`. `stand(at, feet)` is where the feet of a
/// body going to `at` from feet at `feet` come to rest, if it can stand there.
pub fn clear_between(from: Vec3, to: Vec3, stand: impl FnMut(Vec2, f32) -> Option<f32>) -> bool {
    clear_to(from, to, SAME_LEVEL, stand)
}

/// As `clear_between`, but the height it comes out at has to be within `tolerance` of `to`'s.
fn clear_to(from: Vec3, to: Vec3, tolerance: f32, mut stand: impl FnMut(Vec2, f32) -> Option<f32>) -> bool {
    let (a, b) = (xz(from), xz(to));
    let steps = (a.distance(b) / (CELL * 0.5)).ceil().max(1.0) as usize;
    let mut feet = from.y;
    for i in 1..=steps {
        match stand(a.lerp(b, i as f32 / steps as f32), feet) {
            Some(next) => feet = next,
            None => return false,
        }
    }
    (feet - to.y).abs() < tolerance
}

/// The way from `from` to `to` (x, feet, z), or as near as can be got, for a body that can stand wherever
/// `stand` says (see `clear_between`). `limit` is how many places are looked at.
pub fn find_route(from: Vec3, to: Vec3, limit: usize, mut stand: impl FnMut(Vec2, f32) -> Option<f32>) -> Route {
    let origin = xz(from);
    let world = |cell: IVec2| origin + cell.as_vec2() * CELL;
    let goal = ((xz(to) - origin) / CELL).round().as_ivec2();
    // What standing at a square comes to, for a body coming from a given floor: asked once.
    let mut known: HashMap<(IVec2, i32), Option<f32>> = HashMap::new();
    // (Whether a square can be stood on from a floor depends on how high the floor is to within a step, so the
    // floors are told apart to a tenth of a metre.)
    let mut at_cell = |cell: IVec2, from_feet: f32, stand: &mut dyn FnMut(Vec2, f32) -> Option<f32>| -> Option<f32> { *known.entry((cell, (from_feet * 10.0).round() as i32)).or_insert_with(|| stand(world(cell), from_feet)) };
    // How far there is to go (cells, and heights counted as cells), and what the search makes of it.
    let to_go = |place: Place, feet: f32| octile(place.cell, goal) + HEIGHT_CELLS * (feet - to.y).abs();
    let wanted = |place: Place, feet: f32| {
        let greed = if (feet - to.y).abs() < SAME_LEVEL { GREED } else { GREED_ELSEWHERE };
        greed * octile(place.cell, goal) + HEIGHT_CELLS * (feet - to.y).abs()
    };
    let start = Place { cell: IVec2::ZERO, level: level_of(from.y) };
    let mut feet_at: HashMap<Place, f32> = HashMap::new();
    let mut cost: HashMap<Place, f32> = HashMap::new();
    let mut came: HashMap<Place, Place> = HashMap::new();
    let mut queue = BinaryHeap::new();
    feet_at.insert(start, from.y);
    cost.insert(start, 0.0);
    queue.push(Queued { score: wanted(start, from.y), place: start });
    let mut best = (to_go(start, from.y), start);
    let mut looked = 0;
    let mut arrived = None;
    while let Some(Queued { place, .. }) = queue.pop() {
        let here = cost[&place];
        let feet = feet_at[&place];
        let remaining = to_go(place, feet);
        if remaining < best.0 {
            best = (remaining, place);
        }
        if (place.cell - goal).abs().max_element() <= GOAL_CELLS && (feet - to.y).abs() < SAME_LEVEL {
            arrived = Some(place);
            break;
        }
        looked += 1;
        if looked > limit {
            break;
        }
        for step in STEPS {
            let next = place.cell + step;
            let Some(next_feet) = at_cell(next, feet, &mut stand) else { continue };
            // No cutting a corner: both squares the diagonal passes between must be open too.
            if step.x != 0 && step.y != 0 && (at_cell(place.cell + IVec2::new(step.x, 0), feet, &mut stand).is_none() || at_cell(place.cell + IVec2::new(0, step.y), feet, &mut stand).is_none()) {
                continue;
            }
            let next_place = Place { cell: next, level: level_of(next_feet) };
            let g = here + if step.x != 0 && step.y != 0 { std::f32::consts::SQRT_2 } else { 1.0 };
            if cost.get(&next_place).is_none_or(|&known| g < known) {
                cost.insert(next_place, g);
                feet_at.insert(next_place, next_feet);
                came.insert(next_place, place);
                queue.push(Queued { score: g + wanted(next_place, next_feet), place: next_place });
            }
        }
    }
    let arrives = arrived.is_some();
    let mut end = arrived.unwrap_or(best.1);
    let mut places = vec![end];
    while let Some(&before) = came.get(&end) {
        places.push(before);
        end = before;
    }
    places.reverse();
    // Only the corners: a place is dropped when the way from the last kept to the one after it is clear.
    let points: Vec<Vec3> = places
        .iter()
        .skip(1)
        .map(|p| {
            let w = world(p.cell);
            Vec3::new(w.x, feet_at[p], w.y)
        })
        .collect();
    let mut kept = Vec::new();
    let mut last = from;
    let mut i = 0;
    while i < points.len() {
        let mut farthest = i;
        for (j, &p) in points.iter().enumerate().skip(i + 1).take(SMOOTH_LOOKAHEAD) {
            if clear_to(last, p, SAME_PLACE, &mut stand) {
                farthest = j;
            }
        }
        kept.push(points[farthest]);
        last = points[farthest];
        i = farthest + 1;
    }
    // When the goal itself is the place wanted, finish on it rather than on the cell beside it.
    if arrives && kept.last().is_some_and(|&end| xz(end).distance(xz(to)) < GOAL_CELLS as f32 * CELL * 2.0) {
        let beside = kept.last().copied().unwrap_or(from);
        let finish = Vec3::new(to.x, beside.y, to.z);
        if clear_to(beside, finish, SAME_PLACE, &mut stand) {
            kept.push(finish);
        }
    }
    Route { points: kept, arrives }
}

/// How often, seconds, a body going after something checks that its way is still the best one.
const REROUTE_EVERY: f32 = 0.6;

/// How near, metres, a point of a route has to be to count as reached (and not further than a floor away).
const REACHED: f32 = 0.4;
const REACHED_HEIGHT: f32 = 0.15;

/// A body going after something that moves: it heads straight for it where it can, and where something is in
/// the way (or it is on another floor), along a route round, which it looks at again every so often.
#[derive(Debug, Default, Clone)]
pub struct Pursuit {
    route: Vec<Vec3>,
    replan_in: f32,
}

impl Pursuit {
    /// Forgets the way: for when it is not going after anything.
    pub fn clear(&mut self) {
        self.route.clear();
    }

    /// The places it still has to go through.
    pub fn waypoints(&self) -> &[Vec3] {
        &self.route
    }

    /// Whether it is going by a route round something and not straight.
    pub fn detouring(&self) -> bool {
        !self.route.is_empty()
    }

    /// Where the body at `at` (x, feet, z) should head for this moment, going after `goal`: the goal itself,
    /// or the next point of the way round. `stand` says where a body can stand (see `clear_between`). A new
    /// route is worked out only while `routes_left` is above zero (and uses one up), so that a crowd does not
    /// all search in the same frame; `rand` is a source of numbers in 0..1 that keeps the crowd out of step.
    pub fn aim(&mut self, at: Vec3, goal: Vec3, dt: f32, routes_left: &mut usize, mut rand: impl FnMut() -> f32, mut stand: impl FnMut(Vec2, f32) -> Option<f32>) -> Vec2 {
        self.replan_in -= dt;
        if self.replan_in <= 0.0 {
            if clear_between(at, goal, &mut stand) {
                self.route.clear();
                self.replan_in = REROUTE_EVERY * (0.7 + 0.6 * rand());
            } else if *routes_left > 0 {
                *routes_left -= 1;
                self.route = find_route(at, goal, SEARCH_LIMIT, &mut stand).points;
                self.replan_in = REROUTE_EVERY * (0.7 + 0.6 * rand());
            }
        }
        while self.route.first().is_some_and(|&p| xz(at).distance(xz(p)) < REACHED && (at.y - p.y).abs() < REACHED_HEIGHT) {
            self.route.remove(0);
        }
        xz(self.route.first().copied().unwrap_or(goal))
    }
}

/// How many points on, at most, one is looked for to go straight to.
const SMOOTH_LOOKAHEAD: usize = 24;

const STEPS: [IVec2; 8] = [IVec2::new(1, 0), IVec2::new(-1, 0), IVec2::new(0, 1), IVec2::new(0, -1), IVec2::new(1, 1), IVec2::new(1, -1), IVec2::new(-1, 1), IVec2::new(-1, -1)];

#[cfg(test)]
mod tests {
    use super::*;

    /// Level ground, standing wherever `open` says.
    fn flat(open: impl Fn(Vec2) -> bool) -> impl FnMut(Vec2, f32) -> Option<f32> {
        move |p, _| open(p).then_some(0.0)
    }

    /// A world of a wall across x = 0 from z = -400 to z = 400 with a gap of `gap` metres at z = 8, and open
    /// elsewhere; a body is a circle of radius 0.28.
    fn walled(gap: f32) -> impl Fn(Vec2) -> bool {
        move |p: Vec2| {
            let in_wall = p.x.abs() < 0.2 + 0.28 && p.y.abs() < 400.0;
            let in_gap = (p.y - 8.0).abs() < gap * 0.5 - 0.28;
            !in_wall || in_gap
        }
    }

    fn at(x: f32, z: f32) -> Vec3 {
        Vec3::new(x, 0.0, z)
    }

    fn length(from: Vec3, route: &Route) -> f32 {
        let mut last = from;
        route
            .points
            .iter()
            .map(|&p| {
                let d = xz(last).distance(xz(p));
                last = p;
                d
            })
            .sum()
    }

    #[test]
    fn in_the_open_it_goes_straight() {
        let route = find_route(Vec3::ZERO, at(10.0, 3.0), SEARCH_LIMIT, flat(|_| true));
        assert!(route.arrives);
        assert!(route.points.len() <= 2, "{:?}", route.points);
        assert!(xz(*route.points.last().unwrap()).distance(Vec2::new(10.0, 3.0)) < 0.5);
    }

    #[test]
    fn a_wall_across_the_way_is_gone_round_through_the_gap() {
        let (from, to) = (at(-6.0, 0.0), at(6.0, 0.0));
        let open = walled(1.0);
        let route = find_route(from, to, SEARCH_LIMIT, flat(&open));
        assert!(route.arrives, "{:?}", route.points);
        // Every stretch of the way is open.
        let mut last = from;
        for &p in &route.points {
            assert!(clear_between(last, p, flat(&open)), "{last} to {p}");
            last = p;
        }
        // And is not much longer than it has to be: to the gap and back, about 2 * sqrt(6^2 + 8^2) = 20.
        assert!(length(from, &route) < 24.0, "{}", length(from, &route));
    }

    #[test]
    fn a_doorway_a_body_can_just_get_through_is_found() {
        let route = find_route(at(-4.0, 8.1), at(4.0, 7.9), SEARCH_LIMIT, flat(walled(1.0)));
        assert!(route.arrives);
    }

    #[test]
    fn a_gap_too_narrow_for_a_body_is_not() {
        let route = find_route(at(-4.0, 0.0), at(4.0, 0.0), SEARCH_LIMIT, flat(walled(0.5)));
        assert!(!route.arrives);
        // It gets as near as it can, on its own side.
        let end = route.points.last().map_or(Vec2::new(-4.0, 0.0), |&p| xz(p));
        assert!(end.x < 0.0 && end.x > -1.5, "{end}");
    }

    #[test]
    fn a_goal_inside_a_wall_is_reached_as_near_as_can_be() {
        let open = |p: Vec2| p.distance(Vec2::new(5.0, 0.0)) > 0.8;
        let route = find_route(Vec3::ZERO, at(5.0, 0.0), SEARCH_LIMIT, flat(open));
        assert!(route.arrives);
        assert!(xz(*route.points.last().unwrap()).distance(Vec2::new(5.0, 0.0)) < 1.6);
    }

    #[test]
    fn a_search_that_runs_out_of_room_still_heads_the_right_way() {
        let route = find_route(Vec3::ZERO, at(500.0, 0.0), 300, flat(|_| true));
        assert!(!route.arrives);
        assert!(route.points.last().unwrap().x > 5.0);
    }

    #[test]
    fn it_is_quick_enough_to_do_many_times_a_second() {
        let started = std::time::Instant::now();
        let mut asked = 0;
        for i in 0..5 {
            let route = find_route(at(-30.0, i as f32), at(30.0, 0.0), SEARCH_LIMIT, |p, f| {
                asked += 1;
                flat(walled(1.0))(p, f)
            });
            assert!(route.arrives);
        }
        assert!(asked > 0);
        assert!(started.elapsed().as_secs_f32() < 2.0, "{:?}", started.elapsed());
    }

    #[test]
    fn the_points_are_a_straight_line_apart_when_it_can_be() {
        assert!(clear_between(Vec3::ZERO, at(10.0, 0.0), flat(|_| true)));
        assert!(!clear_between(at(-3.0, 0.0), at(3.0, 0.0), flat(walled(1.0))));
    }

    /// Level ground, and a floor 3 m up over z > 12, got to by a ramp up the strip 0 < x < 2 from z = 2 to z =
    /// 12 (a gentle slope); nothing walkable under the floor but the ground.
    fn upstairs(p: Vec2, feet: f32) -> Option<f32> {
        let mut surfaces = vec![0.0];
        if (0.0..2.0).contains(&p.x) && (2.0..=12.0).contains(&p.y) {
            surfaces.push((p.y - 2.0) * 0.3);
        }
        if p.y > 12.0 {
            surfaces.push(3.0);
        }
        // The highest thing that can be stepped onto, and nothing to step down from but a short drop.
        let floor = surfaces.into_iter().filter(|&s| s <= feet + 0.5).fold(f32::MIN, f32::max);
        (feet - floor <= 0.6).then_some(floor)
    }

    #[test]
    fn the_way_to_somewhere_up_a_stair_goes_by_the_stair() {
        let route = find_route(at(8.0, 2.0), Vec3::new(8.0, 3.0, 17.0), SEARCH_LIMIT, upstairs);
        assert!(route.arrives, "{:?}", route.points.last());
        let climbed = route.points.iter().any(|p| p.y > 0.5 && p.x < 2.5 && p.z < 13.0);
        assert!(climbed, "it went up by the ramp: {:?}", route.points);
        assert!((route.points.last().unwrap().y - 3.0).abs() < 0.2);
        // Going straight at it from below is not on: it is under the floor, a level down.
        assert!(!clear_between(at(8.0, 2.0), Vec3::new(8.0, 3.0, 17.0), upstairs));
    }

    #[test]
    fn the_way_back_down_is_found_too() {
        let route = find_route(Vec3::new(8.0, 3.0, 17.0), at(8.0, 2.0), SEARCH_LIMIT, upstairs);
        assert!(route.arrives);
        assert!(route.points.last().unwrap().y.abs() < 0.2);
    }

    #[test]
    fn a_pursuit_follows_a_goal_on_another_floor_up_the_stair() {
        let mut pursuit = Pursuit::default();
        let (mut at, goal) = (Vec3::new(8.0, 0.0, 2.0), Vec3::new(8.0, 3.0, 17.0));
        for _ in 0..4000 {
            let mut routes_left = 1;
            let aim = pursuit.aim(at, goal, 0.05, &mut routes_left, || 0.5, upstairs);
            let to = xz(at) + (aim - xz(at)).normalize_or_zero() * 0.2;
            let feet = upstairs(to, at.y).unwrap_or(at.y);
            at = Vec3::new(to.x, feet, to.y);
            if xz(at).distance(xz(goal)) < 1.0 && (at.y - goal.y).abs() < 0.5 {
                break;
            }
        }
        assert!(xz(at).distance(xz(goal)) < 1.5 && (at.y - 3.0).abs() < 0.5, "{at}");
    }
}

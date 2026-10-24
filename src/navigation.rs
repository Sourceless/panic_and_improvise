//! Finding a way round things: a search over a grid laid on the ground, for anything that has to get from
//! one place to another with walls, fences and water between. It knows nothing of the world; it is
//! told, a point at a time, whether a body can stand there.

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

/// How much the search favours points that are nearer the goal over points that are nearer the start:
/// more is quicker and finds longer ways round, less is truer to the shortest.
const GREED: f32 = 1.8;

/// A way found: the points to go through in turn (not the start), and whether the last of them is at the goal
/// or only the nearest that could be got to.
#[derive(Debug, Clone, PartialEq)]
pub struct Route {
    pub points: Vec<Vec2>,
    pub arrives: bool,
}

#[derive(PartialEq)]
struct Queued {
    score: f32,
    cell: IVec2,
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

/// The distance over a grid, steps of one and of a diagonal, in cells.
fn octile(a: IVec2, b: IVec2) -> f32 {
    let d = (a - b).abs();
    let (long, short) = (d.x.max(d.y) as f32, d.x.min(d.y) as f32);
    long + (std::f32::consts::SQRT_2 - 1.0) * short
}

/// Whether a body can go straight from `from` to `to`: every point of the way, a step or less apart, is
/// somewhere it can stand.
pub fn clear_between(from: Vec2, to: Vec2, mut open: impl FnMut(Vec2) -> bool) -> bool {
    let length = from.distance(to);
    let steps = (length / (CELL * 0.5)).ceil().max(1.0) as usize;
    (1..=steps).all(|i| open(from.lerp(to, i as f32 / steps as f32)))
}

/// The way from `from` to `to`, or as near as can be got, for a body that can stand wherever `open` says.
/// `limit` is how many points are looked at.
pub fn find_route(from: Vec2, to: Vec2, limit: usize, mut open: impl FnMut(Vec2) -> bool) -> Route {
    let world = |cell: IVec2| from + cell.as_vec2() * CELL;
    let goal = ((to - from) / CELL).round().as_ivec2();
    let mut seen: HashMap<IVec2, bool> = HashMap::new();
    let mut free = |cell: IVec2, open: &mut dyn FnMut(Vec2) -> bool| -> bool { *seen.entry(cell).or_insert_with(|| cell == IVec2::ZERO || open(world(cell))) };
    let mut cost: HashMap<IVec2, f32> = HashMap::new();
    let mut came: HashMap<IVec2, IVec2> = HashMap::new();
    let mut queue = BinaryHeap::new();
    cost.insert(IVec2::ZERO, 0.0);
    queue.push(Queued { score: GREED * octile(IVec2::ZERO, goal), cell: IVec2::ZERO });
    let mut best = (octile(IVec2::ZERO, goal), IVec2::ZERO);
    let mut looked = 0;
    let mut arrived = None;
    while let Some(Queued { cell, .. }) = queue.pop() {
        let here = cost[&cell];
        let to_go = octile(cell, goal);
        if to_go < best.0 {
            best = (to_go, cell);
        }
        if (cell - goal).abs().max_element() <= GOAL_CELLS {
            arrived = Some(cell);
            break;
        }
        looked += 1;
        if looked > limit {
            break;
        }
        for step in STEPS {
            let next = cell + step;
            if !free(next, &mut open) {
                continue;
            }
            // No cutting a corner: both squares the diagonal passes between must be open too.
            if step.x != 0 && step.y != 0 && !(free(cell + IVec2::new(step.x, 0), &mut open) && free(cell + IVec2::new(0, step.y), &mut open)) {
                continue;
            }
            let g = here + if step.x != 0 && step.y != 0 { std::f32::consts::SQRT_2 } else { 1.0 };
            if cost.get(&next).is_none_or(|&known| g < known) {
                cost.insert(next, g);
                came.insert(next, cell);
                queue.push(Queued { score: g + GREED * octile(next, goal), cell: next });
            }
        }
    }
    let arrives = arrived.is_some();
    let mut end = arrived.unwrap_or(best.1);
    let mut cells = vec![end];
    while let Some(&before) = came.get(&end) {
        cells.push(before);
        end = before;
    }
    cells.reverse();
    // Only the corners: a point is dropped when the way from the last kept to the one after it is clear.
    let points: Vec<Vec2> = cells.iter().skip(1).map(|&c| world(c)).collect();
    let mut kept = Vec::new();
    let mut last = from;
    let mut i = 0;
    while i < points.len() {
        let mut farthest = i;
        for (j, &p) in points.iter().enumerate().skip(i + 1).take(SMOOTH_LOOKAHEAD) {
            if clear_between(last, p, &mut open) {
                farthest = j;
            }
        }
        kept.push(points[farthest]);
        last = points[farthest];
        i = farthest + 1;
    }
    // When the goal itself is the place wanted, finish on it rather than on the cell beside it.
    if arrives && kept.last().is_some_and(|&end| end.distance(to) < GOAL_CELLS as f32 * CELL * 2.0) && clear_between(kept.last().copied().unwrap_or(from), to, &mut open) {
        kept.push(to);
    }
    Route { points: kept, arrives }
}

/// How many points on, at most, one is looked for to go straight to.
const SMOOTH_LOOKAHEAD: usize = 24;

const STEPS: [IVec2; 8] = [IVec2::new(1, 0), IVec2::new(-1, 0), IVec2::new(0, 1), IVec2::new(0, -1), IVec2::new(1, 1), IVec2::new(1, -1), IVec2::new(-1, 1), IVec2::new(-1, -1)];

#[cfg(test)]
mod tests {
    use super::*;

    /// A world of a wall across x = 0 from z = -400 to z = 400 with a gap of `gap` metres at z = 8, and open
    /// elsewhere; a body is a circle of radius 0.28.
    fn walled(gap: f32) -> impl Fn(Vec2) -> bool {
        move |p: Vec2| {
            let in_wall = p.x.abs() < 0.2 + 0.28 && p.y.abs() < 400.0;
            let in_gap = (p.y - 8.0).abs() < gap * 0.5 - 0.28;
            !in_wall || in_gap
        }
    }

    fn length(from: Vec2, route: &Route) -> f32 {
        let mut last = from;
        route.points.iter().map(|&p| {
            let d = last.distance(p);
            last = p;
            d
        }).sum()
    }

    #[test]
    fn in_the_open_it_goes_straight() {
        let route = find_route(Vec2::ZERO, Vec2::new(10.0, 3.0), SEARCH_LIMIT, |_| true);
        assert!(route.arrives);
        assert!(route.points.len() <= 2, "{:?}", route.points);
        assert!(route.points.last().unwrap().distance(Vec2::new(10.0, 3.0)) < 0.5);
    }

    #[test]
    fn a_wall_across_the_way_is_gone_round_through_the_gap() {
        let (from, to) = (Vec2::new(-6.0, 0.0), Vec2::new(6.0, 0.0));
        let open = walled(1.0);
        let route = find_route(from, to, SEARCH_LIMIT, &open);
        assert!(route.arrives, "{:?}", route.points);
        // Every stretch of the way is open, and it passes through the gap.
        let mut last = from;
        for &p in &route.points {
            assert!(clear_between(last, p, &open), "{last} to {p}");
            last = p;
        }
        // And is not much longer than it has to be: to the gap and back, about 2 * sqrt(6^2 + 8^2) = 20.
        assert!(length(from, &route) < 24.0, "{}", length(from, &route));
    }

    #[test]
    fn a_doorway_a_body_can_just_get_through_is_found() {
        let route = find_route(Vec2::new(-4.0, 8.1), Vec2::new(4.0, 7.9), SEARCH_LIMIT, walled(1.0));
        assert!(route.arrives);
    }

    #[test]
    fn a_gap_too_narrow_for_a_body_is_not() {
        let route = find_route(Vec2::new(-4.0, 0.0), Vec2::new(4.0, 0.0), SEARCH_LIMIT, walled(0.5));
        assert!(!route.arrives);
        // It gets as near as it can, on its own side.
        let end = route.points.last().copied().unwrap_or(Vec2::new(-4.0, 0.0));
        assert!(end.x < 0.0 && end.x > -1.5, "{end}");
    }

    #[test]
    fn a_goal_inside_a_wall_is_reached_as_near_as_can_be() {
        let open = |p: Vec2| p.distance(Vec2::new(5.0, 0.0)) > 0.8;
        let route = find_route(Vec2::ZERO, Vec2::new(5.0, 0.0), SEARCH_LIMIT, open);
        assert!(route.arrives);
        assert!(route.points.last().unwrap().distance(Vec2::new(5.0, 0.0)) < 1.6);
    }

    #[test]
    fn a_search_that_runs_out_of_room_still_heads_the_right_way() {
        let route = find_route(Vec2::ZERO, Vec2::new(500.0, 0.0), 300, |_| true);
        assert!(!route.arrives);
        assert!(route.points.last().unwrap().x > 5.0);
    }

    #[test]
    fn it_is_quick_enough_to_do_many_times_a_second() {
        let started = std::time::Instant::now();
        let mut open = 0;
        for i in 0..5 {
            let route = find_route(Vec2::new(-30.0, i as f32), Vec2::new(30.0, 0.0), SEARCH_LIMIT, |p| {
                open += 1;
                walled(1.0)(p)
            });
            assert!(route.arrives);
        }
        assert!(open > 0);
        assert!(started.elapsed().as_secs_f32() < 2.0, "{:?}", started.elapsed());
    }

    #[test]
    fn the_points_are_a_straight_line_apart_when_it_can_be() {
        assert!(clear_between(Vec2::ZERO, Vec2::new(10.0, 0.0), |_| true));
        assert!(!clear_between(Vec2::new(-3.0, 0.0), Vec2::new(3.0, 0.0), walled(1.0)));
    }
}

//! How the bodies that are not the player walk: over the ground and up and down steps, stairs and floors,
//! stopped by whatever is solid, falling off an edge. The player's own walking is in `player`; this is the
//! same rules for the zombies and for a bot, which are only asked to stay on their feet.

use bevy::prelude::*;

use crate::collision::{Colliders, STEP_DOWN, STEP_UP};
use crate::map::TerrainMap;
use crate::MAP_HALF_SIZE;

/// How steep the ground can be before it will not be walked on (the vertical part of the surface normal).
pub const MIN_FLATNESS: f32 = 0.8;

/// How fast, metres a second squared, a body that has stepped off an edge falls.
const GRAVITY: f32 = 20.0;

/// How far from the edge of the map nothing goes.
const EDGE_MARGIN: f32 = 10.0;

/// Where the feet of a body of `radius` going to `p` from feet at height `feet` would come to rest, if it
/// can stand there: on dry, not-too-steep ground, or on whatever it can step onto (a stair, a floor, a low
/// table), not into anything solid, and not at the bottom of a drop it would not step down. The same
/// rules `Colliders` gives the player.
pub fn stand(map: &TerrainMap, colliders: &Colliders, p: Vec2, feet: f32, radius: f32) -> Option<f32> {
    if p.abs().max_element() > MAP_HALF_SIZE - EDGE_MARGIN {
        return None;
    }
    let deck = map.road_top_at(p);
    let ground = map.floor_height_at(p);
    let support = colliders.support(p, feet, STEP_UP);
    let floor = support.unwrap_or(ground).max(ground);
    if floor - feet > STEP_UP + 1e-3 || feet - floor > STEP_DOWN {
        return None;
    }
    // Water and cliffs are for the ground only; a floor over them, or a road, is another matter.
    let on_ground = deck.is_none() && support.is_none_or(|s| s <= ground + 0.05);
    if on_ground && (map.water_surface_at(p).is_some() || map.normal_at(p).y < MIN_FLATNESS) {
        return None;
    }
    // Not into anything solid at the height it ends up. And not into something too high to step onto: a tread
    // above the one a body stands on is a wall to it, though when it has stepped up it is not. (Only the
    // middle of the body is tried against that, since a stair is climbed a tread at a time, and the places
    // looked at are further apart than that.)
    let clear = |at_feet: f32, reach: f32| colliders.resolve(p, reach, at_feet, STEP_UP).distance(p) < 0.02;
    (clear(floor, radius) && clear(feet, POINT)).then_some(floor)
}

/// How far from its middle, metres, a body is taken to reach when only whether its middle is in something is
/// wanted.
const POINT: f32 = 0.05;

/// Moves a body of `radius`, standing at `at` with its feet at `feet`, by `velocity` for `dt` seconds:
/// where it ends up and the height of its feet. It is pushed back out of what is solid and slides along it;
/// steps and stairs it goes up and down; an edge it falls from, speeding up (`falling` is how fast it is
/// going down, and is kept between calls).
pub fn walk(map: &TerrainMap, colliders: &Colliders, at: Vec2, feet: f32, falling: &mut f32, velocity: Vec2, dt: f32, radius: f32) -> (Vec2, f32) {
    let moved = colliders.resolve(at + velocity * dt, radius, feet, STEP_UP);
    let ground = map.floor_height_at(moved);
    let floor = colliders.support(moved, feet, STEP_UP).unwrap_or(f32::MIN).max(ground);
    if feet - floor > STEP_DOWN {
        *falling += GRAVITY * dt;
        let lower = (feet - *falling * dt).max(floor);
        if lower <= floor {
            *falling = 0.0;
        }
        (moved, lower)
    } else {
        *falling = 0.0;
        (moved, floor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::Solid;

    /// Ten steps up a stair along +Z from z = 0, each 0.2 m high and 0.4 deep, 1.2 m wide, to a landing 2 m up
    /// that is a floor over z > 4 (wide); the slab's underside is 1.8 m above the foot of the stair, so a body
    /// walks under it.
    fn staircase() -> Colliders {
        let mut c = Colliders::default();
        for i in 0..10 {
            let z = 0.2 + i as f32 * 0.4;
            c.add(Solid::rect(Vec2::new(0.0, z), Vec2::new(0.6, 0.2), 0.0, (i + 1) as f32 * 0.2));
        }
        c.add(Solid::rect(Vec2::new(0.0, 6.0), Vec2::new(4.0, 2.0), 0.0, 2.0).from(1.8));
        c
    }

    #[test]
    fn it_stands_on_a_step_it_can_reach_and_not_on_one_it_cannot() {
        let map = TerrainMap::flat(0.0);
        let c = staircase();
        assert_eq!(stand(&map, &c, Vec2::new(0.0, 0.2), 0.0, 0.28), Some(0.2));
        assert_eq!(stand(&map, &c, Vec2::new(0.0, 0.6), 0.2, 0.28), Some(0.4));
        // The fifth step from the ground is a leap.
        assert_eq!(stand(&map, &c, Vec2::new(0.0, 2.2), 0.0, 0.28), None);
        // Step off the side of the stair and it is the ground again, if the drop is a short one.
        assert_eq!(stand(&map, &c, Vec2::new(1.5, 1.0), 0.4, 0.28), Some(0.0));
        assert_eq!(stand(&map, &c, Vec2::new(1.5, 1.0), 1.0, 0.28), None);
        // Off the top of the stair onto the landing.
        assert_eq!(stand(&map, &c, Vec2::new(0.0, 4.4), 1.9, 0.28), Some(2.0));
        // Under the landing, from the ground, is the ground: the floor is a ceiling to someone below it.
        assert_eq!(stand(&map, &c, Vec2::new(3.0, 4.4), 0.0, 0.28), Some(0.0));
    }

    #[test]
    fn it_climbs_a_stair_by_walking_at_it() {
        let map = TerrainMap::flat(0.0);
        let c = staircase();
        let (mut at, mut feet, mut falling) = (Vec2::new(0.0, -2.0), 0.0, 0.0);
        for _ in 0..80 {
            (at, feet) = walk(&map, &c, at, feet, &mut falling, Vec2::new(0.0, 2.0), 0.05, 0.3);
        }
        assert!(at.y > 5.5 && (feet - 2.0).abs() < 0.01, "ended at {at}, feet {feet}");
    }

    #[test]
    fn it_walks_under_a_floor_without_climbing_it() {
        let map = TerrainMap::flat(0.0);
        let c = staircase();
        let (mut at, mut feet, mut falling) = (Vec2::new(3.0, 3.0), 0.0, 0.0);
        for _ in 0..100 {
            (at, feet) = walk(&map, &c, at, feet, &mut falling, Vec2::new(0.0, 2.0), 0.05, 0.3);
        }
        assert!(at.y > 6.0 && feet.abs() < 0.01, "under the landing: {at}, feet {feet}");
    }

    #[test]
    fn it_falls_off_the_edge_of_a_floor_and_lands() {
        let map = TerrainMap::flat(0.0);
        let c = staircase();
        let (mut at, mut feet, mut falling) = (Vec2::new(0.0, 7.0), 2.0, 0.0);
        let mut least_while_falling = f32::MAX;
        for _ in 0..80 {
            (at, feet) = walk(&map, &c, at, feet, &mut falling, Vec2::new(0.0, 3.0), 0.05, 0.3);
            if feet < 2.0 && feet > 0.01 {
                least_while_falling = least_while_falling.min(feet);
            }
        }
        assert!(at.y > 8.0 && feet.abs() < 0.01 && falling == 0.0, "{at} {feet}");
        assert!(least_while_falling < 2.0, "it came down, not jumped down");
    }

    /// Flat ground at 10 m with a main road along the X axis.
    fn map_with_a_road() -> TerrainMap {
        use crate::roads::{half_width, RoadKind, RoadClearance, RoadRibbon};
        let map = TerrainMap::flat(10.0);
        let points: Vec<Vec2> = (0..=40).map(|i| Vec2::new(-50.0 + i as f32 * 2.5, 0.0)).collect();
        let half_widths = vec![half_width(RoadKind::Major); points.len()];
        map.set_roads(RoadClearance::new(&[RoadRibbon { kind: RoadKind::Major, points, half_widths, start_junction: false, end_junction: false }]));
        map
    }

    #[test]
    fn a_road_is_walked_on_and_not_in() {
        let map = map_with_a_road();
        let c = Colliders::default();
        let road = map.road_top_at(Vec2::new(0.0, 0.0)).unwrap();
        assert!(road > 10.2);
        assert_eq!(stand(&map, &c, Vec2::new(0.0, 0.0), 10.0, 0.28), Some(road), "stepped up onto it from the verge");
        // Walk across it from the verge: the feet are on its surface in the middle and on the ground again after.
        let (mut at, mut feet, mut falling) = (Vec2::new(0.0, -8.0), 10.0, 0.0);
        let mut highest = feet;
        for _ in 0..200 {
            (at, feet) = walk(&map, &c, at, feet, &mut falling, Vec2::new(0.0, 3.0), 0.05, 0.3);
            highest = highest.max(feet);
            if at.y.abs() < 3.0 {
                assert!((feet - road).abs() < 0.01, "on the road at {at}: feet {feet}, road {road}");
            }
        }
        assert!((highest - road).abs() < 0.01);
        assert!(at.y > 8.0 && (feet - 10.0).abs() < 0.01, "down off the far side: {at}, feet {feet}");
    }

    #[test]
    fn a_road_over_water_is_still_a_road() {
        // Ground below the water's level at a place with a road over it: a bridge.
        let map = map_with_a_road();
        assert!(map.water_surface_at(Vec2::new(0.0, 0.0)).is_none());
        let c = Colliders::default();
        assert!(stand(&map, &c, Vec2::new(0.0, 0.0), map.road_top_at(Vec2::ZERO).unwrap(), 0.28).is_some());
    }

    #[test]
    fn it_is_stopped_by_a_wall() {
        let map = TerrainMap::flat(0.0);
        let mut c = Colliders::default();
        c.add(Solid::wall(Vec2::new(-5.0, 3.0), Vec2::new(5.0, 3.0), 0.15, 3.0, 3.0));
        let (mut at, mut feet, mut falling) = (Vec2::ZERO, 0.0, 0.0);
        for _ in 0..100 {
            (at, feet) = walk(&map, &c, at, feet, &mut falling, Vec2::new(0.0, 3.0), 0.05, 0.3);
        }
        assert!(at.y < 3.0 - 0.15 && at.y > 2.4, "{at}");
        assert_eq!(feet, 0.0);
    }

    #[test]
    fn water_and_cliffs_are_not_stood_in() {
        let map = TerrainMap::flat(0.0);
        let c = Colliders::default();
        assert_eq!(stand(&map, &c, Vec2::new(5.0, 5.0), 0.0, 0.28), Some(0.0));
        assert_eq!(stand(&map, &c, Vec2::splat(crate::MAP_HALF_SIZE), 0.0, 0.28), None, "the edge of the map");
    }
}

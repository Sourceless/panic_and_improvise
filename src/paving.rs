//! Pavements beside the streets of the villages and towns, and a paved path from each front door to them.
//!
//! The pavements are ribbons like the roads' (of kind `Pavement`), so that they are meshed, walked on and kept clear
//! of trees and walls with all the rest. They run along both sides of every street inside a settlement, stopping short
//! of any other road that crosses them. The paths (`Walkway`) start at the front door of each building and go
//! straight out from it, the way it faces, to the first pavement or road they meet.

use bevy::prelude::*;

use crate::building::front_doors;
use crate::map::{Poi, TerrainMap};
use crate::roads::{half_width, RoadClearance, RoadKind, RoadRibbon, PAVEMENT_HALF_WIDTH};
use crate::settlement_plan::{Layout, SettlementPlan, Tier};

/// Pavements are laid inside this fraction of a settlement's radius, as the buildings are.
const PAVED_LIMIT: f32 = 0.95;
/// The distance between the points a pavement is laid out by, metres.
const STEP: f32 = 1.5;
/// A pavement stays this far (centre to edge) from another road that crosses it: not on it.
const ROAD_MARGIN: f32 = PAVEMENT_HALF_WIDTH - 0.05;
/// And this far from another pavement, which it would otherwise lie across.
const PAVEMENT_MARGIN: f32 = 0.6;
/// Pieces of pavement shorter than this, in points, are not laid.
const MIN_POINTS: usize = 3;
/// How far a path from a door goes looking for the pavement, metres, and how far in it goes.
const WALKWAY_REACH: f32 = 10.0;
const WALKWAY_STEP: f32 = 0.25;
const WALKWAY_INTO: f32 = 0.2;
/// The settlements smaller than this are not paved: a hamlet's road has no pavement.
fn paved(tier: Tier) -> bool {
    tier != Tier::Hamlet
}

/// The pavements and paths of the whole plan, given every road (the settlements' lanes among them).
pub fn lay(map: &TerrainMap, plan: &SettlementPlan, ribbons: &[RoadRibbon]) -> Vec<RoadRibbon> {
    let roads = RoadClearance::new(ribbons);
    let mut laid = Vec::new();
    for (poi, layout) in map.pois.iter().zip(&plan.layouts) {
        if !Tier::of(poi).is_some_and(paved) {
            continue;
        }
        let pavements = pavements(poi, ribbons, &roads);
        let here: Vec<RoadRibbon> = ribbons.iter().cloned().chain(pavements.iter().cloned()).collect();
        let walkways = walkways(poi, layout, &RoadClearance::new(&here));
        laid.extend(pavements);
        laid.extend(walkways);
    }
    laid
}

/// The streets of a settlement, the main roads first: the ones that have buildings along them.
fn streets<'a>(poi: &Poi, ribbons: &'a [RoadRibbon]) -> Vec<&'a RoadRibbon> {
    let limit = poi.radius * PAVED_LIMIT;
    let mut streets: Vec<&RoadRibbon> = ribbons.iter().filter(|r| matches!(r.kind, RoadKind::Major | RoadKind::Lane) && r.points.iter().any(|p| p.distance(poi.position) < limit)).collect();
    streets.sort_by_key(|r| r.kind != RoadKind::Major);
    streets
}

/// Points along `ribbon` every `STEP` metres, each with the way the road runs there and its half width.
fn along(ribbon: &RoadRibbon) -> Vec<(Vec2, Vec2, f32)> {
    let mut points = Vec::new();
    let mut carry = 0.0f32;
    for i in 0..ribbon.points.len().saturating_sub(1) {
        let (a, b) = (ribbon.points[i], ribbon.points[i + 1]);
        let length = a.distance(b);
        let direction = (b - a).normalize_or_zero();
        if direction == Vec2::ZERO {
            continue;
        }
        let mut at = carry;
        while at < length {
            let f = at / length;
            points.push((a.lerp(b, f), direction, ribbon.half_widths[i] + (ribbon.half_widths[i + 1] - ribbon.half_widths[i]) * f));
            at += STEP;
        }
        carry = at - length;
    }
    points
}

/// The pavements along the streets of one settlement.
fn pavements(poi: &Poi, ribbons: &[RoadRibbon], roads: &RoadClearance) -> Vec<RoadRibbon> {
    let limit = poi.radius * PAVED_LIMIT;
    let mut laid: Vec<RoadRibbon> = Vec::new();
    for street in streets(poi, ribbons) {
        // What has been laid so far in this settlement, which this must not lie across.
        let before = RoadClearance::new(&laid);
        let samples = along(street);
        for side in [1.0f32, -1.0] {
            let mut run: Vec<Vec2> = Vec::new();
            let flush = |run: &mut Vec<Vec2>, laid: &mut Vec<RoadRibbon>| {
                if run.len() >= MIN_POINTS {
                    let points = std::mem::take(run);
                    laid.push(RoadRibbon { kind: RoadKind::Pavement, half_widths: vec![PAVEMENT_HALF_WIDTH; points.len()], points, start_junction: false, end_junction: false });
                }
                run.clear();
            };
            for &(p, direction, hw) in &samples {
                let at = p + Vec2::new(-direction.y, direction.x) * side * (hw + PAVEMENT_HALF_WIDTH);
                let clear = at.distance(poi.position) <= limit && roads.clearance(at) >= ROAD_MARGIN && before.clearance(at) >= PAVEMENT_MARGIN;
                if clear {
                    run.push(at);
                } else {
                    flush(&mut run, &mut laid);
                }
            }
            flush(&mut run, &mut laid);
        }
    }
    laid
}

/// The path from the front door of each building of a settlement out to the first road or pavement in front of it.
fn walkways(poi: &Poi, layout: &Layout, roads: &RoadClearance) -> Vec<RoadRibbon> {
    let mut laid = Vec::new();
    for (i, building) in layout.buildings.iter().enumerate() {
        // As the settlement is spawned: what starts the building's random numbers.
        let jitter = Vec2::splat(i as f32 * 0.37) + poi.position * 0.001;
        for door in front_doors(building, jitter) {
            let start = door + building.front * 0.05;
            let mut end = None;
            let mut distance = WALKWAY_STEP;
            while distance <= WALKWAY_REACH {
                let at = start + building.front * distance;
                if roads.clearance(at) <= -WALKWAY_INTO {
                    end = Some(at);
                    break;
                }
                distance += WALKWAY_STEP;
            }
            let Some(end) = end else { continue };
            laid.push(RoadRibbon { kind: RoadKind::Walkway, half_widths: vec![half_width(RoadKind::Walkway); 2], points: vec![start, end], start_junction: false, end_junction: false });
        }
    }
    laid
}

#[cfg(test)]
mod tests {
    use super::*;

    fn street(kind: RoadKind, from: Vec2, to: Vec2) -> RoadRibbon {
        let n = (from.distance(to) / 2.5).ceil() as usize;
        let points: Vec<Vec2> = (0..=n).map(|i| from.lerp(to, i as f32 / n as f32)).collect();
        RoadRibbon { kind, half_widths: vec![half_width(kind); points.len()], points, start_junction: false, end_junction: false }
    }

    fn village(at: Vec2) -> Poi {
        Poi { kind: crate::map::PoiKind::Village, position: at, landmark: at, radius: 100.0 }
    }

    /// Prints places to stand and look at pavements and the paths to doors (`FPS_AT`, `FPS_YAW`).
    #[test]
    #[ignore = "prints places to look at"]
    fn where_to_look_at_paving() {
        let params = crate::params::GenParams::default();
        let map = TerrainMap::generate(crate::MAP_SEED, &params);
        let plans = crate::world::plan_world(&map, &params, &crate::sketch::Sketch::default());
        let ribbons = crate::roads::road_ribbons(&map, &plans.roads);
        let count = |kind| ribbons.iter().filter(|r| r.kind == kind).count();
        eprintln!("{} pavements, {} walkways, of {} roads", count(RoadKind::Pavement), count(RoadKind::Walkway), ribbons.len());
        let mut biggest: Vec<&Poi> = map.pois.iter().filter(|p| Tier::of(p).is_some_and(paved)).collect();
        biggest.sort_by(|a, b| b.radius.total_cmp(&a.radius));
        for poi in biggest.iter().take(3) {
            let near: Vec<&RoadRibbon> = ribbons.iter().filter(|r| r.kind == RoadKind::Walkway && r.points[0].distance(poi.position) < poi.radius * 0.6).take(2).collect();
            for w in near {
                let (start, end) = (w.points[0], w.points[1]);
                let out = (end - start).normalize();
                let stand = end + out * 5.0;
                eprintln!("village r {:.0} at {:?}: walkway: FPS_AT={:.1},{:.1} FPS_YAW={:.2}", poi.radius, poi.position, stand.x, stand.y, (out.x).atan2(out.y));
            }
        }
    }

    #[test]
    fn a_street_has_a_pavement_on_each_side_just_clear_of_its_edge() {
        let main = street(RoadKind::Major, Vec2::new(-80.0, 0.0), Vec2::new(80.0, 0.0));
        let roads = RoadClearance::new(&[main.clone()]);
        let laid = pavements(&village(Vec2::ZERO), &[main.clone()], &roads);
        assert_eq!(laid.len(), 2, "one each side");
        for pavement in &laid {
            assert_eq!(pavement.kind, RoadKind::Pavement);
            for p in &pavement.points {
                // Its inner edge is the road's edge.
                assert!((p.y.abs() - (half_width(RoadKind::Major) + PAVEMENT_HALF_WIDTH)).abs() < 1e-3, "{p}");
            }
        }
        assert!(laid[0].points.iter().all(|p| p.y > 0.0) != laid[1].points.iter().all(|p| p.y > 0.0), "on opposite sides");
    }

    #[test]
    fn a_pavement_stops_short_of_a_street_that_crosses_it() {
        let main = street(RoadKind::Major, Vec2::new(-80.0, 0.0), Vec2::new(80.0, 0.0));
        let lane = street(RoadKind::Lane, Vec2::new(0.0, -60.0), Vec2::new(0.0, 60.0));
        let ribbons = [main, lane];
        let roads = RoadClearance::new(&ribbons);
        let laid = pavements(&village(Vec2::ZERO), &ribbons, &roads);
        for pavement in &laid {
            assert!(pavement.points.iter().all(|&p| roads.clearance(p) >= ROAD_MARGIN - 1e-3), "none of it is on a road");
        }
        // No piece of the main street's pavement crosses the lane.
        let along_main: Vec<&RoadRibbon> = laid.iter().filter(|r| r.points.iter().all(|p| p.y.abs() < 7.0)).collect();
        assert!(!along_main.is_empty());
        for pavement in along_main {
            assert!(pavement.points.iter().all(|p| p.x.abs() > half_width(RoadKind::Lane) + 0.5) || pavement.points.iter().all(|p| p.x.abs() > 0.0 && p.x.signum() == pavement.points[0].x.signum()), "it does not run across the lane");
        }
    }

    #[test]
    fn nothing_is_paved_outside_the_settlement_or_in_a_hamlet() {
        let main = street(RoadKind::Major, Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0));
        let roads = RoadClearance::new(&[main.clone()]);
        let laid = pavements(&village(Vec2::ZERO), &[main], &roads);
        assert!(laid.iter().flat_map(|r| &r.points).all(|p| p.length() <= 100.0 * PAVED_LIMIT + 1.0), "within the settlement");
        assert!(!paved(Tier::Hamlet) && paved(Tier::Village));
    }

    #[test]
    fn every_door_of_a_terrace_has_its_own_path_to_the_pavement() {
        use crate::settlement_plan::{Building, BuildingKind};
        let main = street(RoadKind::Major, Vec2::new(-80.0, 0.0), Vec2::new(80.0, 0.0));
        let roads = RoadClearance::new(&[main.clone()]);
        let poi = village(Vec2::ZERO);
        let pavements = pavements(&poi, &[main.clone()], &roads);
        let front = Vec2::new(0.0, -1.0);
        let terrace = Building { kind: BuildingKind::Terrace, centre: Vec2::new(0.0, half_width(RoadKind::Major) + 3.0 + 4.0), yaw: crate::settlement_plan::yaw_facing(front), width: 30.0, depth: 8.0, wall_height: 5.0, front };
        let doors = front_doors(&terrace, Vec2::ZERO);
        assert!(doors.len() >= 3, "a long terrace is several houses: {} doors", doors.len());
        let all: Vec<RoadRibbon> = std::iter::once(main).chain(pavements).collect();
        let paths = walkways(&poi, &Layout { buildings: vec![terrace], ..Default::default() }, &RoadClearance::new(&all));
        assert_eq!(paths.len(), doors.len(), "a path for each door");
    }

    #[test]
    fn a_path_goes_from_a_front_door_straight_out_to_the_pavement() {
        use crate::settlement_plan::{Building, BuildingKind};
        let main = street(RoadKind::Major, Vec2::new(-80.0, 0.0), Vec2::new(80.0, 0.0));
        let roads = RoadClearance::new(&[main.clone()]);
        let poi = village(Vec2::ZERO);
        let pavements = pavements(&poi, &[main.clone()], &roads);
        // A house on the north side, its front facing south to the street, set back 3 m from the road's edge.
        let front = Vec2::new(0.0, -1.0);
        let depth = 8.0;
        let house = Building { kind: BuildingKind::House, centre: Vec2::new(10.0, half_width(RoadKind::Major) + 3.0 + depth * 0.5), yaw: crate::settlement_plan::yaw_facing(front), width: 9.0, depth, wall_height: 5.0, front };
        let layout = Layout { buildings: vec![house], ..Default::default() };
        let all: Vec<RoadRibbon> = std::iter::once(main).chain(pavements).collect();
        let paths = walkways(&poi, &layout, &RoadClearance::new(&all));
        assert_eq!(paths.len(), 1);
        let path = &paths[0];
        let (start, end) = (path.points[0], path.points[1]);
        // It starts at the wall and ends in the pavement, going straight toward the street.
        assert!((start.y - (half_width(RoadKind::Major) + 3.0)).abs() < 0.5, "starts at the wall on the street side: {start}");
        assert!(end.y < half_width(RoadKind::Major) + PAVEMENT_HALF_WIDTH * 2.0 && end.y > half_width(RoadKind::Major) + 0.5, "ends in the pavement: {end}");
        assert!((end.x - start.x).abs() < 1e-3, "straight out");
    }
}

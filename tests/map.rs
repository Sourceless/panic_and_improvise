use bevy::math::Vec2;
use fps_prototype::map::{PoiKind, TerrainMap, HALF_SIZE, MAP_SIZE};
use fps_prototype::MAP_SEED;

fn map() -> TerrainMap {
    TerrainMap::generate(MAP_SEED)
}

#[test]
fn map_is_five_kilometres_square() {
    assert_eq!(MAP_SIZE, 5000.0);
    let map = map();
    let n = map.grid_size();
    assert_eq!(n, 501);
    assert_eq!(map.height_at(Vec2::new(-HALF_SIZE, -HALF_SIZE)), map.vertex_height(0, 0));
}

#[test]
fn map_has_at_least_one_hill() {
    let map = map();
    let tallest = map.hills.iter().map(|h| h.height).fold(0.0, f32::max);
    assert!(tallest >= 60.0, "tallest hill is {tallest}m");

    let hill = map.hills.iter().max_by(|a, b| a.height.total_cmp(&b.height)).unwrap();
    assert!(
        map.height_at(hill.center) >= 60.0,
        "terrain at hill centre is only {}m",
        map.height_at(hill.center)
    );
}

#[test]
fn map_has_a_river_crossing_from_edge_to_edge() {
    let map = map();
    let first = map.river.first().expect("river has points").pos;
    let last = map.river.last().expect("river has points").pos;
    assert!((first.x + HALF_SIZE).abs() < 1.0, "river starts at x={}", first.x);
    assert!((last.x - HALF_SIZE).abs() < 1.0, "river ends at x={}", last.x);
    assert!(map.river_length() >= 4500.0, "river length is {}m", map.river_length());
}

#[test]
fn river_bed_is_carved_below_its_banks() {
    let map = map();
    for point in map.river.iter().skip(5).step_by(20).take(20) {
        let bed = map.height_at(point.pos);
        let bank = map.height_at(point.pos + Vec2::new(0.0, 60.0));
        assert!(bed < bank, "river bed {bed}m is not below bank {bank}m at {:?}", point.pos);
    }
}

#[test]
fn map_has_points_of_interest_on_dry_land() {
    let map = map();
    assert!(map.pois.len() >= 5, "only {} POIs generated", map.pois.len());
    for kind in [PoiKind::Village, PoiKind::Church, PoiKind::Farm, PoiKind::Mill] {
        assert!(map.pois.iter().any(|p| p.kind == kind), "no {kind:?} generated");
    }
    for poi in &map.pois {
        let inside = poi.position.x.abs() < HALF_SIZE && poi.position.y.abs() < HALF_SIZE;
        assert!(inside, "{:?} is outside the map at {:?}", poi.kind, poi.position);
        let near_river = map
            .river
            .windows(2)
            .map(|w| {
                let ab = w[1].pos - w[0].pos;
                let t = ((poi.position - w[0].pos).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
                poi.position.distance(w[0].pos + ab * t)
            })
            .fold(f32::MAX, f32::min);
        assert!(near_river >= 4.0, "{:?} sits in the river", poi.kind);
    }
}

#[test]
fn spawn_area_is_flat_ground() {
    let map = map();
    assert_eq!(map.height_at(Vec2::ZERO), 0.0);
}

#[test]
fn same_seed_gives_same_map() {
    let a = TerrainMap::generate(7);
    let b = TerrainMap::generate(7);
    for p in [Vec2::new(-1234.0, 987.0), Vec2::new(400.0, -2100.0)] {
        assert_eq!(a.height_at(p), b.height_at(p));
    }
    assert_eq!(a.pois.len(), b.pois.len());
}

#[test]
fn terrain_has_real_relief() {
    let map = map();
    let n = map.grid_size();
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for iz in (0..n).step_by(5) {
        for ix in (0..n).step_by(5) {
            let h = map.vertex_height(ix, iz);
            lo = lo.min(h);
            hi = hi.max(h);
        }
    }
    assert!(hi - lo >= 60.0, "height range is only {}m", hi - lo);
    assert!(lo >= 0.0, "terrain dips below zero to {lo}m");
}

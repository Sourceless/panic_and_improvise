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
    assert_eq!(map.grid_size(), 501);
    assert_eq!(map.height_at(Vec2::new(-HALF_SIZE, -HALF_SIZE)), map.vertex_height(0, 0));
}

#[test]
fn map_has_at_least_one_hill() {
    let map = map();
    let hill = map.hills.iter().max_by(|a, b| a.height.total_cmp(&b.height)).unwrap();
    assert!(hill.height >= 50.0, "tallest hill is {}m", hill.height);
    assert!(
        map.height_at(hill.center) >= 50.0,
        "terrain at hill centre is only {}m",
        map.height_at(hill.center)
    );
}

#[test]
fn river_drains_off_the_map_edge() {
    let map = map();
    let n = map.grid_size();
    let on_edge = (0..n).any(|i| {
        [(i, 0), (i, n - 1), (0, i), (n - 1, i)]
            .iter()
            .any(|&(ix, iz)| map.water_level(ix, iz).is_some())
    });
    assert!(on_edge, "no river reaches the map edge");
    assert!(map.river_length() >= 3500.0, "river is only {}m long", map.river_length());
}

#[test]
fn river_bed_sits_below_its_water() {
    let map = map();
    let n = map.grid_size();
    let mut checked = 0;
    for iz in 1..n - 1 {
        for ix in 1..n - 1 {
            let Some(level) = map.water_level(ix, iz) else {
                continue;
            };
            let bed = map.vertex_height(ix, iz);
            assert!(bed < level - 0.5, "river bed {bed}m is not below its water {level}m");
            checked += 1;
        }
    }
    assert!(checked > 50);
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
        if poi.kind != PoiKind::Church {
            assert!(map.river_distance(poi.position) > 2.0, "{:?} sits in the river", poi.kind);
        }
    }
}

#[test]
fn terrain_has_real_relief() {
    let map = map();
    let n = map.grid_size();
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for iz in 0..n {
        for ix in 0..n {
            let h = map.vertex_height(ix, iz);
            lo = lo.min(h);
            hi = hi.max(h);
        }
    }
    assert!(hi - lo >= 30.0, "height range is only {}m", hi - lo);
    assert!(map.river_length() > 0.0);
}

#[test]
fn same_seed_gives_same_map() {
    let a = TerrainMap::generate(7);
    let b = TerrainMap::generate(7);
    for p in [Vec2::new(-234.0, 187.0), Vec2::new(300.0, -210.0)] {
        assert_eq!(a.height_at(p), b.height_at(p));
    }
    assert_eq!(a.pois.len(), b.pois.len());
}

#[test]
fn every_settlement_has_water_nearby() {
    for seed in [MAP_SEED, 1, 2, 3, 4] {
        let map = TerrainMap::generate(seed);
        for poi in &map.pois {
            assert!(
                map.river_distance(poi.position) <= 390.0,
                "seed {seed}: {:?} is {}m from water",
                poi.kind,
                map.river_distance(poi.position)
            );
        }
    }
}

#[test]
fn river_has_a_minimum_length() {
    for seed in [MAP_SEED, 1, 2, 3] {
        let map = TerrainMap::generate(seed);
        assert!(map.river_length() >= 3500.0, "seed {seed}: river is {}m", map.river_length());
    }
}

#[test]
fn land_below_sea_level_is_lake_water() {
    let map = map();
    let n = map.grid_size();
    for iz in 0..n {
        for ix in 0..n {
            if let Some(level) = map.water_level(ix, iz) {
                if map.vertex_height(ix, iz) < 0.0 {
                    assert_eq!(level, 0.0, "lake cell at ({ix},{iz}) not at sea level");
                }
            }
        }
    }
}

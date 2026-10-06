use fps_prototype::map::{grid_pos, PoiKind, TerrainMap};
use fps_prototype::zones::{Zone, ZoneMap};
use fps_prototype::MAP_SEED;

fn setup() -> (TerrainMap, ZoneMap) {
    let map = TerrainMap::generate(MAP_SEED);
    let zones = ZoneMap::generate(&map);
    (map, zones)
}

#[test]
fn water_cells_are_water_zone() {
    let (map, zones) = setup();
    let n = map.grid_size();
    for iz in 0..n {
        for ix in 0..n {
            if map.water_level(ix, iz).is_some() {
                assert_eq!(zones.zone_at(ix, iz), Zone::Water);
            }
        }
    }
}

#[test]
fn several_zone_types_appear_on_a_map() {
    let (_, zones) = setup();
    assert!(zones.distinct_zones() >= 5, "only {} zone types", zones.distinct_zones());
}

#[test]
fn arable_is_never_on_high_ground() {
    let (map, zones) = setup();
    let n = map.grid_size();
    for iz in 0..n {
        for ix in 0..n {
            if zones.zone_at(ix, iz) == Zone::Arable {
                assert!(map.vertex_height(ix, iz) < 90.0, "arable at {}m", map.vertex_height(ix, iz));
            }
        }
    }
}

#[test]
fn conifer_is_only_upland() {
    let (map, zones) = setup();
    let n = map.grid_size();
    for iz in 0..n {
        for ix in 0..n {
            if zones.zone_at(ix, iz) == Zone::Conifer {
                assert!(map.vertex_height(ix, iz) > 110.0, "conifer at {}m", map.vertex_height(ix, iz));
            }
        }
    }
}

#[test]
fn urban_zone_sits_around_villages() {
    let (map, zones) = setup();
    let n = map.grid_size();
    for iz in 0..n {
        for ix in 0..n {
            if zones.zone_at(ix, iz) == Zone::Urban {
                let p = grid_pos(ix, iz);
                let near = map
                    .pois
                    .iter()
                    .any(|poi| poi.kind == PoiKind::Village && poi.position.distance(p) < 150.0);
                assert!(near, "urban cell at {:?} is not near a village", p);
            }
        }
    }
}

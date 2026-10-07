use fps_prototype::map::{grid_pos, PoiKind, TerrainMap};
use fps_prototype::roads::{RoadKind, RoadNetwork};
use fps_prototype::MAP_SEED;

#[test]
fn every_settlement_connects_to_the_network() {
    for seed in [MAP_SEED, 1, 2, 3] {
        let map = TerrainMap::generate(seed);
        let roads = RoadNetwork::generate(&map);
        let n = map.grid_size();
        for poi in map.pois.iter().filter(|p| matches!(p.kind, PoiKind::Village | PoiKind::Mill | PoiKind::Farm)) {
            let near = (0..n).flat_map(|z| (0..n).map(move |x| (x, z))).any(|(x, z)| {
                roads.kind_at(x, z).is_some() && grid_pos(x, z).distance(poi.position) < 15.0
            });
            assert!(near, "seed {seed}: {:?} at {:?} is not connected", poi.kind, poi.position);
        }
    }
}

#[test]
fn road_lengths_are_sensible() {
    let map = TerrainMap::generate(MAP_SEED);
    let roads = RoadNetwork::generate(&map);
    assert!(roads.length(RoadKind::Major) > 500.0, "major network is too short");
    assert!(roads.length(RoadKind::Minor) > 500.0, "minor network is too short");
}

#[test]
fn some_roads_bridge_water_but_not_too_many() {
    let map = TerrainMap::generate(MAP_SEED);
    let roads = RoadNetwork::generate(&map);
    let n = map.grid_size();
    let mut road_cells = 0;
    let mut water_road_cells = 0;
    for iz in 0..n {
        for ix in 0..n {
            if roads.kind_at(ix, iz).is_some() {
                road_cells += 1;
                if map.water_level(ix, iz).is_some() {
                    water_road_cells += 1;
                }
            }
        }
    }
    let fraction = water_road_cells as f32 / road_cells as f32;
    assert!(fraction < 0.1, "{water_road_cells} of {road_cells} road cells are over water");
}

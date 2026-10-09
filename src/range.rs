//! A shooting range to try the guns on: `FPS_RANGE=1 cargo run` (or `fps_prototype --range`) puts an
//! armoury of every gun and load on the ground just ahead of where you start, with a Bergen on your back
//! and a line of dummies out to 300 m.

use bevy::prelude::*;

use crate::ammo::AMMO;
use crate::inventory::{Inventory, ItemKind, PackKind, Supply};
use crate::loot::{Found, LootAssets, LootBox, Pickup, Rarity};
use crate::map::TerrainMap;
use crate::target::spawn_dummy_at;
use crate::weapons::WeaponKind;

pub struct RangePlugin;

impl Plugin for RangePlugin {
    fn build(&self, app: &mut App) {
        if std::env::var_os("FPS_RANGE").is_some() {
            app.add_systems(Update, build_range);
        }
    }
}

/// How far down the range the dummies stand.
const DUMMY_DISTANCES: [f32; 7] = [10.0, 25.0, 50.0, 100.0, 150.0, 200.0, 300.0];

fn build_range(
    mut done: Local<bool>,
    mut commands: Commands,
    map: Res<TerrainMap>,
    loot: Option<Res<LootAssets>>,
    mut inventory: ResMut<Inventory>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(loot) = loot else { return };
    if *done {
        return;
    }
    *done = true;
    let start = map.spawn_point();
    let at = |x: f32, z: f32| {
        let p = start + Vec2::new(x, z);
        Vec3::new(p.x, map.height_at(p), p.y)
    };
    // A big pack to carry it all in.
    let pack = inventory.make(ItemKind::Pack(PackKind::Bergen), 1);
    let id = pack.id;
    if inventory.add(pack).is_ok() {
        inventory.wear_pack(id);
    }
    // The armoury: guns, then a box of every load, then supplies.
    let mut things: Vec<(ItemKind, u32, u32)> = WeaponKind::ALL.iter().map(|&k| (ItemKind::Weapon(k), 1, k.def().magazine)).collect();
    things.extend(AMMO.iter().map(|a| (ItemKind::Ammo(a.kind), a.per_stack, 0)));
    things.extend([Supply::Bandage, Supply::FirstAid, Supply::Canteen, Supply::TinnedFood, Supply::Torch, Supply::Compass].map(|s| (ItemKind::Supply(s), 1, 0)));
    const PER_ROW: usize = 9;
    for (n, (kind, count, loaded)) in things.into_iter().enumerate() {
        let (row, column) = (n / PER_ROW, n % PER_ROW);
        let position = at((column as f32 - (PER_ROW as f32 - 1.0) * 0.5) * 1.5, -4.0 - row as f32 * 1.5);
        let rarity = Rarity::Common;
        let size = 0.3;
        commands.spawn((
            LootBox { rarity },
            Pickup { rarity, found: Found { kind, count, loaded }, spot: (usize::MAX, n) },
            Mesh3d(loot.mesh.clone()),
            MeshMaterial3d(loot.material(rarity)),
            Transform::from_translation(position + Vec3::Y * (size * 0.5 + 0.03)).with_scale(Vec3::splat(size)),
        ));
    }
    // The targets, down the range, a little apart so that none hides another.
    for (i, distance) in DUMMY_DISTANCES.into_iter().enumerate() {
        let side = if i % 2 == 0 { -1.5 } else { 1.5 };
        spawn_dummy_at(&mut commands, &mut meshes, &mut materials, at(side, -12.0 - distance));
    }
}

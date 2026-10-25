//! Picking things up: look at a box of loot from near enough and press the interact key to open it, and
//! a handful of things fall out round it. Look at one of those and press the key again to take it, if
//! there is room in the inventory.

use bevy::prelude::*;

use crate::controls::{Action, Controls, Keyboard};
use crate::inventory::{Inventory, Item, ItemKind};
use crate::inventory_ui::{DropQueue, InventoryScreen};
use crate::loot::{lay_contents, rarity_of, Dropped, Lying, LIFT, roll_contents, spawn_drop, spot_numbers, Found, LootAssets, LootBox, OpenedLoot, Pickup};
use crate::menu::Menu;
use crate::player::FpsCamera;

/// How near the player has to be to pick something up, and how far from the middle of the view it can be.
const REACH: f32 = 2.6;
const VIEW_CONE: f32 = 0.3;

/// What the player is told: what they could pick up, and what has just happened.
#[derive(Resource, Default, Debug)]
pub struct PickupPrompt {
    pub looking_at: Option<String>,
    pub message: String,
    pub message_time: f32,
}

pub struct PickupPlugin;

impl Plugin for PickupPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PickupPrompt>().add_systems(Update, (pick_up, put_down_thrown));
    }
}

/// Puts what was thrown out of the inventory on the ground just in front of the player, side by side.
fn put_down_thrown(mut commands: Commands, mut queue: ResMut<DropQueue>, assets: Option<Res<LootAssets>>, camera: Query<(&GlobalTransform, &FpsCamera)>, mut thrown: Local<usize>) {
    let (Some(assets), Ok((eye, cam))) = (assets, camera.single()) else { return };
    for item in queue.0.drain(..) {
        let mut ahead = eye.forward().as_vec3();
        ahead.y = 0.0;
        let ahead = ahead.try_normalize().unwrap_or(Vec3::NEG_Z);
        let across = ahead.cross(Vec3::Y);
        let side = (*thrown % 5) as f32 - 2.0;
        let at = Vec3::new(eye.translation().x, cam.floor, eye.translation().z) + ahead * 0.9 + across * side * 0.2;
        let found = Found { kind: item.kind, count: item.count, loaded: item.loaded };
        let lying = Lying { drop: Dropped { rarity: rarity_of(item.kind), found }, at, yaw: 0.0 };
        spawn_drop(&mut commands, &assets, None, &lying, (usize::MAX - 1, *thrown), 0);
        *thrown += 1;
    }
}

/// The words for something found: "9mm x32", "L1A1 SLR".
pub fn describe(found: &Found) -> String {
    match found.kind {
        ItemKind::Ammo(a) => format!("{} x{}", a.def().name, found.count),
        other => other.name().to_string(),
    }
}

/// Makes the item a spot holds, in `inventory`.
pub fn make_item(inventory: &mut Inventory, found: &Found) -> Item {
    match found.kind {
        ItemKind::Weapon(w) => inventory.make_gun(w, w.def().default_ammo, found.loaded),
        other => inventory.make(other, found.count),
    }
}

/// Takes what has been picked up: a pack goes on the back at once, a weapon is taken up ready (putting down
/// the gun in hand if both slots are full, which comes back as the `Ok`), and the rest goes in the grids.
/// Gives back what could not be taken, less any part that was.
fn receive(inventory: &mut Inventory, item: Item) -> Result<Option<Item>, Item> {
    match item.kind {
        ItemKind::Pack(_) => match inventory.put_on(item) {
            Ok(()) => Ok(None),
            // If it will not go on (the old one's things have nowhere to go) it can still be carried.
            Err(item) => inventory.add(item).map(|()| None),
        },
        ItemKind::Weapon(_) => Ok(inventory.take_up_weapon(item)),
        _ => inventory.add(item).map(|()| None),
    }
}

/// Of some things, the one most nearly in the middle of the view, so long as it is near enough: whatever
/// is being looked at, and not merely whatever is nearest. How far from the middle counts (as cosine, so
/// larger is nearer the middle) comes back with it.
fn most_looked_at<'a, T>(items: impl Iterator<Item = (Entity, &'a GlobalTransform, T)>, position: Vec3, forward: Vec3) -> Option<(f32, Entity, T)> {
    items
        .filter_map(|(entity, at, rest)| {
            let to = at.translation() - position;
            let distance = to.length();
            let aim = forward.dot(to / distance.max(1e-4));
            (distance < REACH && distance > 0.01 && aim > 1.0 - VIEW_CONE).then_some((aim, entity, rest))
        })
        .max_by(|a, b| a.0.total_cmp(&b.0))
}

fn pick_up(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<Keyboard>,
    controls: Res<Controls>,
    menu: Res<Menu>,
    screen: Res<InventoryScreen>,
    mut inventory: ResMut<Inventory>,
    mut opened: ResMut<OpenedLoot>,
    mut thrown: ResMut<DropQueue>,
    mut prompt: ResMut<PickupPrompt>,
    assets: Option<Res<LootAssets>>,
    camera: Query<&GlobalTransform, With<FpsCamera>>,
    mut pickups: Query<(Entity, &GlobalTransform, &mut Pickup)>,
    boxes: Query<(Entity, &GlobalTransform, &Transform, &LootBox, Option<&ChildOf>)>,
) {
    prompt.message_time = (prompt.message_time - time.delta_secs()).max(0.0);
    if prompt.message_time == 0.0 {
        prompt.message.clear();
    }
    prompt.looking_at = None;
    let Ok(eye) = camera.single() else { return };
    if menu.open || screen.open {
        return;
    }
    let (position, forward) = (eye.translation(), eye.forward().as_vec3());
    let thing = most_looked_at(pickups.iter_mut(), position, forward);
    let closed = most_looked_at(boxes.iter().map(|(e, at, local, b, parent)| (e, at, (local, b, parent))), position, forward);
    // Whichever of the two is more nearly in the middle of the view gets the key.
    let box_first = match (&thing, &closed) {
        (Some((a, ..)), Some((b, ..))) => b > a,
        (None, Some(_)) => true,
        _ => false,
    };
    if box_first {
        let Some((_, entity, (local, lootbox, parent))) = closed else { return };
        prompt.looking_at = Some(format!("Military crate  [{}]  open", lootbox.rarity.name()));
        if !controls.just_pressed(Action::Interact, &keys) {
            return;
        }
        let Some(assets) = assets else { return };
        let spot = lootbox.spot;
        let contents = roll_contents(lootbox.rarity, spot_numbers(spot.0, spot.1));
        // The crate's floor: it hangs from a point `LIFT` above it.
        let floor = local.translation.y - LIFT;
        let lying = lay_contents(&assets, &lootbox.room, floor, &contents);
        for (slot, thing) in lying.iter().enumerate() {
            spawn_drop(&mut commands, &assets, parent.map(|p| p.parent()), thing, spot, slot);
        }
        prompt.message = format!("Opened the crate: {} things fall out.", contents.len());
        prompt.message_time = 3.0;
        opened.0.insert(spot, lying.into_iter().map(Some).collect());
        commands.entity(entity).despawn();
        return;
    }
    let Some((_, entity, mut pickup)) = thing else { return };
    prompt.looking_at = Some(format!("{}  [{}]", describe(&pickup.found), pickup.rarity.name()));
    if !controls.just_pressed(Action::Interact, &keys) {
        return;
    }
    let item = make_item(&mut inventory, &pickup.found);
    match receive(&mut inventory, item) {
        Ok(displaced) => {
            if let Some(slot) = opened.0.get_mut(&pickup.spot).and_then(|d| d.get_mut(pickup.slot)) {
                *slot = None;
            }
            commands.entity(entity).despawn();
            prompt.message = match (item.kind, &displaced) {
                (ItemKind::Weapon(_), Some(old)) => format!("Took the {} and put down the {}.", item.kind.name(), old.kind.name()),
                (ItemKind::Weapon(_), None) => format!("Took the {}.", item.kind.name()),
                (ItemKind::Pack(_), _) => format!("Put on the {}.", item.kind.name()),
                _ => format!("Picked up {}.", describe(&pickup.found)),
            };
            if let Some(old) = displaced {
                thrown.0.push(old);
            }
        }
        Err(left) if left.count < item.count && matches!(item.kind, ItemKind::Ammo(_)) => {
            // Some of the rounds fitted: the rest stay where they are.
            prompt.message = format!("Took some; no room for the other {} rounds.", left.count);
            pickup.found.count = left.count;
            if let Some(Some(lying)) = opened.0.get_mut(&pickup.spot).and_then(|d| d.get_mut(pickup.slot)) {
                lying.drop.found.count = left.count;
            }
        }
        Err(_) => prompt.message = format!("No room for {}.", describe(&pickup.found)),
    }
    prompt.message_time = 3.0;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ammo::AmmoKind;
    use crate::weapons::WeaponKind;

    #[test]
    fn a_found_gun_comes_loaded_with_what_was_in_it_and_rounds_come_as_a_stack() {
        let mut inv = Inventory::default();
        let gun = make_item(&mut inv, &Found { kind: ItemKind::Weapon(WeaponKind::Slr), count: 1, loaded: 7 });
        assert_eq!((gun.loaded, gun.loaded_with), (7, Some(AmmoKind::NatoBall)));
        let rounds = make_item(&mut inv, &Found { kind: ItemKind::Ammo(AmmoKind::NineFmj), count: 32, loaded: 0 });
        assert_eq!(rounds.count, 32);
        assert_ne!(gun.id, rounds.id);
        assert_eq!(describe(&Found { kind: ItemKind::Ammo(AmmoKind::NineFmj), count: 32, loaded: 0 }), "9mm x32");
    }

    #[test]
    fn what_is_looked_at_wins_over_what_is_merely_nearest() {
        let at = |x: f32, z: f32| GlobalTransform::from_translation(Vec3::new(x, 0.0, z));
        let near_aside = at(0.6, -0.9);
        let far_ahead = at(0.0, -2.2);
        let items = [(Entity::PLACEHOLDER, &near_aside, "aside"), (Entity::PLACEHOLDER, &far_ahead, "ahead")];
        let chosen = most_looked_at(items.into_iter(), Vec3::ZERO, Vec3::NEG_Z).map(|(_, _, name)| name);
        assert_eq!(chosen, Some("ahead"));
        // Out of reach or out of the view is not chosen at all.
        let too_far = at(0.0, -5.0);
        let behind = at(0.0, 1.0);
        assert!(most_looked_at([(Entity::PLACEHOLDER, &too_far, ()), (Entity::PLACEHOLDER, &behind, ())].into_iter(), Vec3::ZERO, Vec3::NEG_Z).is_none());
    }

    #[test]
    fn a_thrown_item_lands_on_the_ground_in_front_of_the_player() {
        use crate::inventory::{Inventory, Supply};
        use crate::loot::ModelLook;
        use std::collections::HashMap;
        let mut app = App::new();
        let kind = ItemKind::Supply(Supply::Torch);
        let look = ModelLook { parts: Vec::new(), middle: Vec2::ZERO, bottom: 0.0, radius: 0.1, long_in_z: true };
        app.insert_resource(LootAssets { mesh: Handle::default(), materials: vec![Handle::default(); 5], looks: HashMap::from([(kind, look)]), crates: Vec::new() });
        let mut inv = Inventory::default();
        let item = inv.make(kind, 1);
        app.insert_resource(DropQueue(vec![item]));
        let camera = FpsCamera { yaw: 0.0, pitch: 0.0, velocity: Vec2::ZERO, air_height: 0.0, vertical_speed: 0.0, stance: crate::player::Stance::Stand, eye_height: 1.7, jump_spent_standing: false, floor: 3.0, mantle: None, mantles: 0, sprint_suppressed: false, sliding: false, view_feet: 3.0, lean: 0.0, lean_offset: Vec2::ZERO };
        app.world_mut().spawn((camera, GlobalTransform::from(Transform::from_xyz(10.0, 4.7, 20.0).looking_to(Vec3::NEG_Z, Vec3::Y))));
        app.add_systems(Update, put_down_thrown);
        app.update();
        let mut things = app.world_mut().query::<(&Pickup, &Transform)>();
        let found: Vec<_> = things.iter(app.world()).collect();
        assert_eq!(found.len(), 1);
        let (pickup, at) = found[0];
        assert_eq!(pickup.found.kind, kind);
        assert!((at.translation.z - 19.1).abs() < 0.01 && (at.translation.x - 10.0).abs() < 0.5, "{:?}", at.translation);
        assert!(at.translation.y > 3.0 && at.translation.y < 3.5, "{:?}", at.translation);
        assert!(app.world().resource::<DropQueue>().0.is_empty());
    }

    #[test]
    fn a_pack_is_put_on_a_weapon_is_taken_up_ready_and_the_gun_in_hand_is_put_down_when_both_slots_are_full() {
        use crate::inventory::PackKind;
        let mut inv = Inventory::starting();
        let carried = inv.slots;
        // A pack goes on at once.
        let bergen = inv.make(ItemKind::Pack(PackKind::Bergen), 1);
        assert!(matches!(receive(&mut inv, bergen), Ok(None)));
        assert_eq!(inv.back.map(|b| b.id), Some(bergen.id));
        // Both slots full: the new gun goes in hand and the old comes back to be put down.
        let slr = inv.make_gun(WeaponKind::Slr, AmmoKind::NatoBall, 20);
        let put_down = receive(&mut inv, slr).expect("taken").expect("the gun in hand is put down");
        assert_eq!(put_down.id, carried[0].unwrap().id);
        assert_eq!(inv.in_hand().map(|g| g.id), Some(slr.id));
        // Anything else goes in the grids.
        let tin = inv.make(ItemKind::Supply(crate::inventory::Supply::TinnedFood), 1);
        assert!(matches!(receive(&mut inv, tin), Ok(None)));
        assert!(inv.find(tin.id).is_some());
    }
}

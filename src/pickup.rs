//! Picking things up: look at a box of loot from near enough and press the interact key to open it, and
//! a handful of things fall out round it. Look at one of those and press the key again to take it, if
//! there is room in the inventory.

use bevy::prelude::*;

use crate::controls::{Action, Controls, Keyboard};
use crate::inventory::{Inventory, Item, ItemKind};
use crate::inventory_ui::InventoryScreen;
use crate::loot::{drop_bundle, roll_contents, spot_numbers, Found, LootAssets, LootBox, OpenedLoot, Pickup};
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
        app.init_resource::<PickupPrompt>().add_systems(Update, pick_up);
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

/// The nearest of some things that is in front of the player and near enough.
fn nearest<'a, T>(items: impl Iterator<Item = (Entity, &'a GlobalTransform, T)>, position: Vec3, forward: Vec3) -> Option<(f32, Entity, T)> {
    items
        .filter_map(|(entity, at, rest)| {
            let to = at.translation() - position;
            let distance = to.length();
            (distance < REACH && distance > 0.01 && forward.dot(to / distance) > 1.0 - VIEW_CONE).then_some((distance, entity, rest))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
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
    let thing = nearest(pickups.iter_mut(), position, forward);
    let closed = nearest(boxes.iter().map(|(e, at, local, b, parent)| (e, at, (local, b, parent))), position, forward);
    // Whichever of the two is nearer gets the key.
    let box_first = match (&thing, &closed) {
        (Some((a, ..)), Some((b, ..))) => b < a,
        (None, Some(_)) => true,
        _ => false,
    };
    if box_first {
        let Some((_, entity, (local, lootbox, parent))) = closed else { return };
        prompt.looking_at = Some(format!("Loot box  [{}]  open", lootbox.rarity.name()));
        if !controls.just_pressed(Action::Interact, &keys) {
            return;
        }
        let Some(assets) = assets else { return };
        let spot = lootbox.spot;
        let contents = roll_contents(lootbox.rarity, spot_numbers(spot.0, spot.1));
        // The box's floor: its middle sits half its height and a skin above it.
        let floor = local.translation - Vec3::Y * (local.scale.x * 0.5 + 0.03);
        for (slot, drop) in contents.iter().enumerate() {
            let mut thing = commands.spawn(drop_bundle(&assets, floor, spot, slot, contents.len(), *drop));
            if let Some(parent) = parent {
                thing.insert(ChildOf(parent.parent()));
            }
        }
        prompt.message = format!("Opened the box: {} things fall out.", contents.len());
        prompt.message_time = 3.0;
        opened.0.insert(spot, contents.into_iter().map(Some).collect());
        commands.entity(entity).despawn();
        return;
    }
    let Some((_, entity, mut pickup)) = thing else { return };
    prompt.looking_at = Some(format!("{}  [{}]", describe(&pickup.found), pickup.rarity.name()));
    if !controls.just_pressed(Action::Interact, &keys) {
        return;
    }
    let item = make_item(&mut inventory, &pickup.found);
    match inventory.add(item) {
        Ok(()) => {
            if let Some(slot) = opened.0.get_mut(&pickup.spot).and_then(|d| d.get_mut(pickup.slot)) {
                *slot = None;
            }
            commands.entity(entity).despawn();
            prompt.message = format!("Picked up {}.", describe(&pickup.found));
        }
        Err(left) if left.count < item.count && matches!(item.kind, ItemKind::Ammo(_)) => {
            // Some of the rounds fitted: the rest stay where they are.
            prompt.message = format!("Took some; no room for the other {} rounds.", left.count);
            pickup.found.count = left.count;
            if let Some(Some(drop)) = opened.0.get_mut(&pickup.spot).and_then(|d| d.get_mut(pickup.slot)) {
                drop.found.count = left.count;
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
}

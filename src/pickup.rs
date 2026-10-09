//! Picking things up: look at a box of loot from near enough and press the interact key. What it
//! holds goes into the inventory if there is room, and the box is gone for good.

use bevy::prelude::*;

use crate::controls::{Action, Controls, Keyboard};
use crate::inventory::{Inventory, Item, ItemKind};
use crate::inventory_ui::InventoryScreen;
use crate::loot::{Found, Pickup, TakenLoot};
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

/// The words for something found: "9mm full metal jacket x32", "L1A1 SLR".
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

fn pick_up(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<Keyboard>,
    controls: Res<Controls>,
    menu: Res<Menu>,
    screen: Res<InventoryScreen>,
    mut inventory: ResMut<Inventory>,
    mut taken: ResMut<TakenLoot>,
    mut prompt: ResMut<PickupPrompt>,
    camera: Query<&GlobalTransform, With<FpsCamera>>,
    mut pickups: Query<(Entity, &GlobalTransform, &mut Pickup)>,
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
    // The nearest box in front of the player and near enough.
    let best = pickups
        .iter_mut()
        .filter_map(|(entity, at, pickup)| {
            let to = at.translation() - position;
            let distance = to.length();
            (distance < REACH && distance > 0.01 && forward.dot(to / distance) > 1.0 - VIEW_CONE).then_some((distance, entity, pickup))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0));
    let Some((_, entity, mut pickup)) = best else { return };
    prompt.looking_at = Some(format!("{}  [{}]", describe(&pickup.found), pickup.rarity.name()));
    if !controls.just_pressed(Action::Interact, &keys) {
        return;
    }
    let item = make_item(&mut inventory, &pickup.found);
    match inventory.add(item) {
        Ok(()) => {
            taken.0.insert(pickup.spot);
            commands.entity(entity).despawn();
            prompt.message = format!("Picked up {}.", describe(&pickup.found));
        }
        Err(left) if left.count < item.count && matches!(item.kind, ItemKind::Ammo(_)) => {
            // Some of the rounds fitted: the rest stay in the box.
            prompt.message = format!("Took some; no room for the other {} rounds.", left.count);
            pickup.found.count = left.count;
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
        assert_eq!(describe(&Found { kind: ItemKind::Ammo(AmmoKind::NineFmj), count: 32, loaded: 0 }), "9mm full metal jacket x32");
    }
}

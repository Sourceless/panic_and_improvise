//! What happens to the things in the world, which every player in a game has to see happen the same way: boxes
//! opened, things dropped and taken, panes broken.
//!
//! The host is the authority. What the host's own player does, it writes down as a [`WorldChange`]; what a client
//! asks for (see `net::Request`) the host does, if it can, and writes down the same way. The host's book of them
//! is sent to every client in order and kept for the whole game, so that a client that joins late is told of
//! everything that has happened and ends up with the same world. A client does nothing of the kind to the world
//! itself: it asks, and the world changes when the host says it has.
//!
//! The things that come out of a box are not sent: which they are is worked out from the map (the box, and the
//! number of the spot it is in), the same everywhere, so only that the box was opened is. What is dropped by a
//! zombie or thrown down is sent, with where it lies.
//!
//! The functions here do the changes to the world, the same for the host's own player, a request and an event.

use bevy::prelude::*;

use crate::inventory::{Inventory, ItemKind};
use crate::loot::{lay_contents, rarity_of, roll_contents, spawn_drop, spot_numbers, Dropped, Found, LootAssets, LootBox, Lying, OpenedLoot, Pickup, Rarity};
use crate::net::{ItemKey, WorldEvent};
use crate::settlement::Buildings;

/// Something has happened in the world that a game that is hosted tells its players of. Nothing else reads these,
/// so in a game that is played alone they are written and forgotten.
#[derive(Message, Clone, Copy, Debug)]
pub struct WorldChange(pub WorldEvent);

pub struct SyncPlugin;

impl Plugin for SyncPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<WorldChange>();
    }
}

/// `x` as the number the network carries: the markers that stand for no building (the top of the range) keep their
/// place, counting down from the top.
fn narrow(x: usize) -> u32 {
    if x > u32::MAX as usize { u32::MAX - (usize::MAX - x) as u32 } else { x as u32 }
}

fn widen(x: u32) -> usize {
    // Only the markers are near the top; no building is.
    if x > u32::MAX - 16 { usize::MAX - (u32::MAX - x) as usize } else { x as usize }
}

/// The name the network gives to a thing lying at `spot` (see `Pickup`), in that place `slot`.
pub fn key_of(spot: (usize, usize), slot: usize) -> ItemKey {
    ItemKey { a: narrow(spot.0), b: narrow(spot.1), slot: slot as u8 }
}

/// Where a thing lies, and which of that place's it is, from its name.
pub fn spot_of(key: ItemKey) -> ((usize, usize), usize) {
    ((widen(key.a), widen(key.b)), key.slot as usize)
}

/// What the host says of a thing that is lying at `spot`, `slot` as `lying` says.
pub fn dropped_event(lying: &Lying, spot: (usize, usize), slot: usize) -> WorldEvent {
    let found = lying.drop.found;
    WorldEvent::Dropped {
        key: key_of(spot, slot),
        item: found.kind.code(),
        count: found.count.min(u16::MAX as u32) as u16,
        loaded: found.loaded.min(u8::MAX as u32) as u8,
        rarity: lying.drop.rarity.code(),
        at: lying.at.to_array(),
        yaw: lying.yaw,
    }
}

/// The thing that a `Dropped` event says is lying somewhere, and where (see `spot_of`), if it makes sense.
pub fn lying_of(event: &WorldEvent) -> Option<(Lying, (usize, usize), usize)> {
    let WorldEvent::Dropped { key, item, count, loaded, rarity, at, yaw } = *event else { return None };
    let kind = ItemKind::from_code(item)?;
    let found = Found { kind, count: (count as u32).max(1), loaded: loaded as u32 };
    let (spot, slot) = spot_of(key);
    Some((Lying { drop: Dropped { rarity: Rarity::from_code(rarity).unwrap_or_else(|| rarity_of(kind)), found }, at: Vec3::from_array(at), yaw }, spot, slot))
}

/// Opens box `spot` of building `index`: what is in it falls out round it. How many things, if it was there to
/// be opened; nothing if it is open already, or is not a box.
pub fn open_box(commands: &mut Commands, buildings: &Buildings, loot: &LootAssets, opened: &mut OpenedLoot, crates: &Query<(Entity, &LootBox)>, index: usize, spot: usize) -> Option<usize> {
    if opened.0.contains_key(&(index, spot)) {
        return None;
    }
    let (lootspot, interior) = buildings.loot_spot(index, spot)?;
    let contents = roll_contents(lootspot.rarity, lootspot.size, spot_numbers(index, spot));
    // Where they lie is where the box's floor is, in the frame of the building.
    let lying = lay_contents(loot, &lootspot.room, lootspot.at.y, &contents);
    // They are only made as things if the insides are there to hold them; otherwise they are made when they are.
    if let Some(parent) = interior {
        for (slot, thing) in lying.iter().enumerate() {
            spawn_drop(commands, loot, Some(parent), thing, (index, spot), slot);
        }
    }
    for (entity, closed) in crates.iter() {
        if closed.spot == (index, spot) {
            commands.entity(entity).despawn();
        }
    }
    let count = lying.len();
    opened.0.insert((index, spot), lying.into_iter().map(Some).collect());
    Some(count)
}

/// Takes `took` of the thing lying at `key` away (all of it, if that is as many as there are), and says what was
/// taken; nothing if there is nothing like that there. What is taken is what was there, with the number taken.
pub fn remove_item(commands: &mut Commands, opened: &mut OpenedLoot, pickups: &mut Query<(Entity, &GlobalTransform, &mut Pickup)>, key: ItemKey, took: u32) -> Option<Found> {
    let (spot, slot) = spot_of(key);
    // What it is, and how many there are: from the box's record if it is one of a box's, else from the thing itself.
    let mut found = None;
    if let Some(lying) = opened.0.get_mut(&spot).and_then(|things| things.get_mut(slot)) {
        if let Some(thing) = lying {
            let there = thing.drop.found;
            found = Some(Found { count: took.clamp(1, there.count), ..there });
            if took >= there.count {
                *lying = None;
            } else {
                thing.drop.found.count -= took;
            }
        }
    } else if let Some((_, _, pickup)) = pickups.iter().find(|(_, _, p)| p.spot == spot && p.slot == slot) {
        found = Some(Found { count: took.clamp(1, pickup.found.count), ..pickup.found });
    }
    let found = found?;
    for (entity, _, mut pickup) in pickups.iter_mut() {
        if pickup.spot == spot && pickup.slot == slot {
            if took >= pickup.found.count {
                commands.entity(entity).despawn();
            } else {
                pickup.found.count -= took;
            }
        }
    }
    Some(found)
}

/// Puts the thing a `Dropped` event is about on the ground, if it is a thing that makes sense.
pub fn put_down(commands: &mut Commands, loot: &LootAssets, event: &WorldEvent) {
    if let Some((lying, spot, slot)) = lying_of(event) {
        spawn_drop(commands, loot, None, &lying, spot, slot);
    }
}

/// What is left of a client's inventory room for `found`, tried on a copy: how many of it would be taken.
pub fn would_fit(inventory: &Inventory, found: &Found) -> u32 {
    let mut copy = inventory.clone();
    let item = crate::pickup::make_item(&mut copy, found);
    match crate::pickup::receive(&mut copy, item) {
        Ok(_) => found.count,
        Err(left) => found.count.saturating_sub(left.count),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ammo::AmmoKind;
    use crate::weapons::WeaponKind;

    #[test]
    fn the_name_of_a_thing_gives_back_where_it_is_even_for_the_markers_near_the_top() {
        for spot in [(0, 0), (17, 3), (4000, 255), (crate::loot::LOOSE, 12), (usize::MAX - 1, 4), (usize::MAX, 9)] {
            for slot in [0usize, 5, 200] {
                assert_eq!(spot_of(key_of(spot, slot)), (spot, slot), "{spot:?} {slot}");
            }
        }
    }

    #[test]
    fn what_is_said_to_be_dropped_is_what_is_put_down() {
        let lying = Lying { drop: Dropped { rarity: Rarity::Rare, found: Found { kind: ItemKind::Weapon(WeaponKind::Sterling), count: 1, loaded: 17 } }, at: Vec3::new(10.5, 2.0, -7.25), yaw: 1.5 };
        let event = dropped_event(&lying, (crate::loot::LOOSE, 41), 2);
        let (back, spot, slot) = lying_of(&event).expect("it makes sense");
        assert_eq!((back, spot, slot), (lying, (crate::loot::LOOSE, 41), 2));
        let rounds = Lying { drop: Dropped { rarity: Rarity::Common, found: Found { kind: ItemKind::Ammo(AmmoKind::NineFmj), count: 32, loaded: 0 } }, at: Vec3::ZERO, yaw: 0.0 };
        assert_eq!(lying_of(&dropped_event(&rounds, (3, 1), 0)).unwrap().0, rounds);
        // Nothing but a drop is one, and a made-up thing is nothing.
        assert!(lying_of(&WorldEvent::Opened { building: 1, spot: 1 }).is_none());
        let WorldEvent::Dropped { key, count, loaded, rarity, at, yaw, .. } = event else { unreachable!() };
        assert!(lying_of(&WorldEvent::Dropped { key, item: 0x0999, count, loaded, rarity, at, yaw }).is_none());
    }
}

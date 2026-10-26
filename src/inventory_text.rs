//! An inventory as one line of text, and back: what a recording keeps of what the player carries.
//!
//! `active=0 next=9 back=- slots=<item>/- pockets=6x4:<placed>;<placed> pack=-`
//! An item is `id,kind,count,loaded,loaded_with,fire_mode` (a dash for none) and a kind is a letter and a name
//! (`W:Sterling`); a placed item is the item, `@`, then `x,y,turned`.

use crate::ammo::AmmoKind;
use crate::inventory::{Container, Inventory, Item, ItemKind, PackKind, Placed, Supply};
use crate::weapons::{FireMode, WeaponKind};

fn kind_text(kind: ItemKind) -> String {
    match kind {
        ItemKind::Weapon(w) => format!("W:{w:?}"),
        ItemKind::Ammo(a) => format!("A:{a:?}"),
        ItemKind::Pack(p) => format!("P:{p:?}"),
        ItemKind::Supply(s) => format!("S:{s:?}"),
    }
}

fn named<T: Copy + std::fmt::Debug>(all: &[T], name: &str) -> Option<T> {
    all.iter().copied().find(|v| format!("{v:?}") == name)
}

fn parse_kind(text: &str) -> Option<ItemKind> {
    let (letter, name) = text.split_once(':')?;
    match letter {
        "W" => named(&WeaponKind::ALL, name).map(ItemKind::Weapon),
        "A" => named(&AmmoKind::ALL, name).map(ItemKind::Ammo),
        "P" => named(&PackKind::ALL, name).map(ItemKind::Pack),
        "S" => named(&Supply::ALL, name).map(ItemKind::Supply),
        _ => None,
    }
}

/// An item as text: `id,kind,count,loaded,loaded_with,fire_mode`.
pub fn item_text(item: &Item) -> String {
    let with = item.loaded_with.map_or("-".to_string(), |a| format!("{a:?}"));
    let fire = item.fire_mode.map_or("-".to_string(), |f| format!("{f:?}"));
    format!("{},{},{},{},{with},{fire}", item.id, kind_text(item.kind), item.count, item.loaded)
}

pub fn parse_item(text: &str) -> Option<Item> {
    let mut parts = text.split(',');
    let id = parts.next()?.parse().ok()?;
    let kind = parse_kind(parts.next()?)?;
    let count = parts.next()?.parse().ok()?;
    let loaded = parts.next()?.parse().ok()?;
    let with = parts.next()?;
    let fire = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    let loaded_with = if with == "-" { None } else { Some(named(&AmmoKind::ALL, with)?) };
    let fire_mode = if fire == "-" { None } else { Some(named(&FireMode::ALL, fire)?) };
    Some(Item { id, kind, count, loaded, loaded_with, fire_mode })
}

fn optional_item(item: &Option<Item>) -> String {
    item.as_ref().map_or("-".to_string(), item_text)
}

fn parse_optional_item(text: &str) -> Option<Option<Item>> {
    if text == "-" { Some(None) } else { parse_item(text).map(Some) }
}

fn container_text(container: &Container) -> String {
    let placed: Vec<String> = container.items.iter().map(|p| format!("{}@{},{},{}", item_text(&p.item), p.x, p.y, u8::from(p.turned))).collect();
    format!("{}x{}:{}", container.width, container.height, placed.join(";"))
}

fn parse_container(text: &str) -> Option<Container> {
    let (size, placed) = text.split_once(':')?;
    let (width, height) = size.split_once('x')?;
    let mut container = Container::new(width.parse().ok()?, height.parse().ok()?);
    for entry in placed.split(';').filter(|e| !e.is_empty()) {
        let (item, at) = entry.split_once('@')?;
        let mut at = at.split(',');
        let (x, y, turned) = (at.next()?.parse().ok()?, at.next()?.parse().ok()?, at.next()?);
        container.items.push(Placed { item: parse_item(item)?, x, y, turned: turned == "1" });
    }
    Some(container)
}

impl Inventory {
    pub fn to_text(&self) -> String {
        let pack = self.pack.as_ref().map_or("-".to_string(), container_text);
        format!(
            "active={} next={} back={} slots={}/{} pockets={} pack={}",
            self.active,
            self.next_id(),
            optional_item(&self.back),
            optional_item(&self.slots[0]),
            optional_item(&self.slots[1]),
            container_text(&self.pockets),
            pack
        )
    }

    /// The inventory `to_text` made, or nothing if the text is not one.
    pub fn from_text(text: &str) -> Option<Inventory> {
        let mut inventory = Inventory::default();
        let mut seen = 0;
        for field in text.split_whitespace() {
            let (name, value) = field.split_once('=')?;
            match name {
                "active" => inventory.active = value.parse().ok()?,
                "next" => inventory.set_next_id(value.parse().ok()?),
                "back" => inventory.back = parse_optional_item(value)?,
                "slots" => {
                    let (a, b) = value.split_once('/')?;
                    inventory.slots = [parse_optional_item(a)?, parse_optional_item(b)?];
                }
                "pockets" => inventory.pockets = parse_container(value)?,
                "pack" => inventory.pack = if value == "-" { None } else { Some(parse_container(value)?) },
                _ => return None,
            }
            seen += 1;
        }
        (seen == 6 && inventory.active < 2).then_some(inventory)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_starting_kit_comes_back_as_it_was() {
        let inventory = Inventory::starting();
        let again = Inventory::from_text(&inventory.to_text()).expect("it reads its own text");
        assert_eq!(again, inventory);
        assert_eq!(again.next_id(), inventory.next_id());
    }

    #[test]
    fn a_kit_with_a_pack_and_things_turned_and_stacked_comes_back_as_it_was() {
        let mut inventory = Inventory::starting();
        let pack = inventory.make(ItemKind::Pack(PackKind::Daypack), 1);
        inventory.back = Some(pack);
        inventory.pack = Some(Container::new(5, 4));
        let food = inventory.make(ItemKind::Supply(Supply::TinnedFood), 1);
        assert!(inventory.pack.as_mut().unwrap().place(food, 2, 1, false));
        let rifle = inventory.make_gun(WeaponKind::HiPower, AmmoKind::NineFmj, 7);
        assert!(inventory.pack.as_mut().unwrap().place(rifle, 0, 2, true));
        inventory.slots[1] = None;
        inventory.active = 0;
        let mut auto = inventory.make_gun(WeaponKind::Bren, AmmoKind::BritishBall, 20);
        auto.fire_mode = Some(FireMode::Auto);
        inventory.slots[1] = Some(auto);
        let again = Inventory::from_text(&inventory.to_text()).expect("it reads its own text");
        assert_eq!(again, inventory);
    }

    #[test]
    fn no_slots_and_no_pack_come_back_empty() {
        let inventory = Inventory::default();
        assert_eq!(Inventory::from_text(&inventory.to_text()), Some(inventory));
        let mut one = Inventory::default();
        let gun = one.make_gun(WeaponKind::HiPower, AmmoKind::NineFmj, 13);
        one.slots[1] = Some(gun);
        assert_eq!(Inventory::from_text(&one.to_text()), Some(one));
    }

    #[test]
    fn text_that_is_not_an_inventory_is_refused() {
        assert_eq!(Inventory::from_text(""), None);
        assert_eq!(Inventory::from_text("hello"), None);
        let good = Inventory::starting().to_text();
        assert_eq!(Inventory::from_text(&good.replace("Sterling", "Blunderbuss")), None);
        assert_eq!(Inventory::from_text(&good.replace("active=0", "active=7")), None);
    }

    #[test]
    fn an_item_reads_back_exactly() {
        let mut inventory = Inventory::default();
        let mut gun = inventory.make_gun(WeaponKind::Auto5, AmmoKind::Buckshot, 3);
        gun.fire_mode = Some(FireMode::Semi);
        assert_eq!(parse_item(&item_text(&gun)), Some(gun));
        assert_eq!(parse_item("1,W:Sterling,1,30"), None, "too few fields");
        assert_eq!(parse_item("1,Q:Sterling,1,30,-,-"), None, "no such kind of thing");
    }
}

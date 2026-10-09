//! What the player carries: two weapon slots, a back for a pack, and grids of squares (the pockets,
//! and whatever the pack gives) that everything else is fitted into, bigger things taking more of them.
//!
//! This is the plain logic of it: where things fit, what stacks, what can be moved where. The screen
//! that shows it (`inventory_ui`) only turns clicks into calls on it.

use bevy::prelude::*;

use crate::ammo::AmmoKind;
use crate::weapons::WeaponKind;

/// How big the pockets are, in squares.
pub const POCKETS: (u32, u32) = (6, 4);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum PackKind {
    Satchel,
    Daypack,
    Bergen,
}

impl PackKind {
    pub const ALL: [PackKind; 3] = [PackKind::Satchel, PackKind::Daypack, PackKind::Bergen];

    pub fn name(self) -> &'static str {
        match self {
            PackKind::Satchel => "Satchel",
            PackKind::Daypack => "Daypack",
            PackKind::Bergen => "Bergen rucksack",
        }
    }

    /// The squares it adds to what the player can carry.
    pub fn capacity(self) -> (u32, u32) {
        match self {
            PackKind::Satchel => (4, 3),
            PackKind::Daypack => (5, 4),
            PackKind::Bergen => (6, 6),
        }
    }

    /// How big the pack itself is while it is carried in the pockets.
    pub fn size(self) -> (u32, u32) {
        match self {
            PackKind::Satchel => (2, 2),
            PackKind::Daypack => (3, 3),
            PackKind::Bergen => (4, 3),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum Supply {
    Bandage,
    FirstAid,
    Canteen,
    TinnedFood,
    Torch,
    Compass,
}

impl Supply {
    pub const ALL: [Supply; 6] = [Supply::Bandage, Supply::FirstAid, Supply::Canteen, Supply::TinnedFood, Supply::Torch, Supply::Compass];

    pub fn name(self) -> &'static str {
        match self {
            Supply::Bandage => "Field dressing",
            Supply::FirstAid => "First aid kit",
            Supply::Canteen => "Water bottle",
            Supply::TinnedFood => "Tinned food",
            Supply::Torch => "Torch",
            Supply::Compass => "Compass",
        }
    }

    pub fn size(self) -> (u32, u32) {
        match self {
            Supply::Bandage | Supply::TinnedFood | Supply::Compass => (1, 1),
            Supply::FirstAid => (2, 2),
            Supply::Canteen => (1, 2),
            Supply::Torch => (1, 2),
        }
    }

    pub fn colour(self) -> [f32; 3] {
        match self {
            Supply::Bandage => [0.85, 0.85, 0.8],
            Supply::FirstAid => [0.75, 0.15, 0.15],
            Supply::Canteen => [0.3, 0.4, 0.55],
            Supply::TinnedFood => [0.6, 0.5, 0.3],
            Supply::Torch => [0.2, 0.2, 0.22],
            Supply::Compass => [0.55, 0.5, 0.2],
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum ItemKind {
    Weapon(WeaponKind),
    Ammo(AmmoKind),
    Pack(PackKind),
    Supply(Supply),
}

impl ItemKind {
    /// How many squares across and down it covers, standing as it is.
    pub fn size(self) -> (u32, u32) {
        match self {
            ItemKind::Weapon(w) => w.def().size,
            ItemKind::Ammo(_) => (1, 1),
            ItemKind::Pack(p) => p.size(),
            ItemKind::Supply(s) => s.size(),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ItemKind::Weapon(w) => w.def().name,
            ItemKind::Ammo(a) => a.def().name,
            ItemKind::Pack(p) => p.name(),
            ItemKind::Supply(s) => s.name(),
        }
    }

    /// The most that goes in one square: rounds for ammunition, 1 for anything else.
    pub fn stack_limit(self) -> u32 {
        match self {
            ItemKind::Ammo(a) => a.def().per_stack,
            _ => 1,
        }
    }

    pub fn colour(self) -> [f32; 3] {
        match self {
            ItemKind::Weapon(_) => [0.35, 0.33, 0.3],
            ItemKind::Ammo(a) => a.def().colour,
            ItemKind::Pack(_) => [0.3, 0.38, 0.22],
            ItemKind::Supply(s) => s.colour(),
        }
    }
}

/// A thing carried.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Item {
    /// Told apart from every other item, so that the screen can say which one it means.
    pub id: u32,
    pub kind: ItemKind,
    /// Rounds, for ammunition; 1 for anything else.
    pub count: u32,
    /// For a weapon: the rounds in it and what they are.
    pub loaded: u32,
    pub loaded_with: Option<AmmoKind>,
}

impl Item {
    pub fn size(&self) -> (u32, u32) {
        self.kind.size()
    }
}

/// An item as it lies in a grid: where its top-left square is, and whether it is turned on its side.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Placed {
    pub item: Item,
    pub x: u32,
    pub y: u32,
    pub turned: bool,
}

impl Placed {
    /// The squares it covers: across, down.
    pub fn extent(&self) -> (u32, u32) {
        let (w, h) = self.item.size();
        if self.turned {
            (h, w)
        } else {
            (w, h)
        }
    }

    fn covers(&self, x: u32, y: u32) -> bool {
        let (w, h) = self.extent();
        x >= self.x && x < self.x + w && y >= self.y && y < self.y + h
    }
}

/// A grid of squares with things in it.
#[derive(Clone, Debug, PartialEq)]
pub struct Container {
    pub width: u32,
    pub height: u32,
    pub items: Vec<Placed>,
}

impl Container {
    pub fn new(width: u32, height: u32) -> Container {
        Container { width, height, items: Vec::new() }
    }

    /// Whether something `size` across and down fits with its top-left square at (x, y), ignoring
    /// the item with id `except` (the one being moved).
    pub fn fits(&self, size: (u32, u32), x: u32, y: u32, except: Option<u32>) -> bool {
        if x + size.0 > self.width || y + size.1 > self.height || size.0 == 0 || size.1 == 0 {
            return false;
        }
        !self.items.iter().filter(|p| Some(p.item.id) != except).any(|p| {
            let (w, h) = p.extent();
            x < p.x + w && p.x < x + size.0 && y < p.y + h && p.y < y + size.1
        })
    }

    /// The first place `item` fits, trying it as it is and then on its side.
    pub fn find_space(&self, item: &Item) -> Option<(u32, u32, bool)> {
        let (w, h) = item.size();
        for turned in [false, true] {
            if turned && w == h {
                break;
            }
            let size = if turned { (h, w) } else { (w, h) };
            for y in 0..self.height {
                for x in 0..self.width {
                    if self.fits(size, x, y, None) {
                        return Some((x, y, turned));
                    }
                }
            }
        }
        None
    }

    pub fn place(&mut self, item: Item, x: u32, y: u32, turned: bool) -> bool {
        let (w, h) = item.size();
        let size = if turned { (h, w) } else { (w, h) };
        if !self.fits(size, x, y, None) {
            return false;
        }
        self.items.push(Placed { item, x, y, turned });
        true
    }

    pub fn remove(&mut self, id: u32) -> Option<Placed> {
        let i = self.items.iter().position(|p| p.item.id == id)?;
        Some(self.items.remove(i))
    }

    pub fn get(&self, id: u32) -> Option<&Placed> {
        self.items.iter().find(|p| p.item.id == id)
    }

    pub fn at(&self, x: u32, y: u32) -> Option<&Placed> {
        self.items.iter().find(|p| p.covers(x, y))
    }

    pub fn squares_used(&self) -> u32 {
        self.items.iter().map(|p| p.extent().0 * p.extent().1).sum()
    }
}

/// Where in the inventory something is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Place {
    Slot(usize),
    Back,
    Pockets,
    Pack,
}

#[derive(Resource, Clone, Debug)]
pub struct Inventory {
    /// The two weapon slots.
    pub slots: [Option<Item>; 2],
    /// Which of them is in hand.
    pub active: usize,
    /// The pack on the player's back, and the grid it gives.
    pub back: Option<Item>,
    pub pockets: Container,
    pub pack: Option<Container>,
    next_id: u32,
}

impl Default for Inventory {
    fn default() -> Self {
        Inventory { slots: [None, None], active: 0, back: None, pockets: Container::new(POCKETS.0, POCKETS.1), pack: None, next_id: 1 }
    }
}

impl Inventory {
    /// A new thing, with a number nobody else has.
    pub fn make(&mut self, kind: ItemKind, count: u32) -> Item {
        let id = self.next_id;
        self.next_id += 1;
        let (loaded, loaded_with) = match kind {
            ItemKind::Weapon(w) => (w.def().magazine, Some(w.def().default_ammo)),
            _ => (0, None),
        };
        Item { id, kind, count, loaded, loaded_with }
    }

    /// A gun with `loaded` rounds of `with` in it.
    pub fn make_gun(&mut self, kind: WeaponKind, with: AmmoKind, loaded: u32) -> Item {
        let mut item = self.make(ItemKind::Weapon(kind), 1);
        item.loaded = loaded.min(kind.def().magazine);
        item.loaded_with = Some(with);
        item
    }

    /// The starting kit: the Sterling in hand with a full magazine, the Hi-Power on the second slot, and some rounds.
    pub fn starting() -> Inventory {
        let mut inv = Inventory::default();
        let sterling = inv.make_gun(WeaponKind::Sterling, AmmoKind::NineFmj, 30);
        let pistol = inv.make_gun(WeaponKind::HiPower, AmmoKind::NineFmj, 13);
        inv.slots = [Some(sterling), Some(pistol)];
        for (kind, count) in [(AmmoKind::NineFmj, 150), (AmmoKind::NineHollowPoint, 50)] {
            let item = inv.make(ItemKind::Ammo(kind), count);
            inv.add(item).expect("the pockets hold the starting rounds");
        }
        let dressing = inv.make(ItemKind::Supply(Supply::Bandage), 1);
        inv.add(dressing).expect("and a dressing");
        inv
    }

    fn containers(&self) -> impl Iterator<Item = &Container> {
        std::iter::once(&self.pockets).chain(self.pack.as_ref())
    }

    pub fn container(&self, place: Place) -> Option<&Container> {
        match place {
            Place::Pockets => Some(&self.pockets),
            Place::Pack => self.pack.as_ref(),
            _ => None,
        }
    }

    fn container_mut(&mut self, place: Place) -> Option<&mut Container> {
        match place {
            Place::Pockets => Some(&mut self.pockets),
            Place::Pack => self.pack.as_mut(),
            _ => None,
        }
    }

    /// Finds an item by its number, wherever it is.
    pub fn find(&self, id: u32) -> Option<(Place, Item)> {
        for (i, s) in self.slots.iter().enumerate() {
            if let Some(item) = s.filter(|it| it.id == id) {
                return Some((Place::Slot(i), item));
            }
        }
        if let Some(item) = self.back.filter(|it| it.id == id) {
            return Some((Place::Back, item));
        }
        if let Some(p) = self.pockets.get(id) {
            return Some((Place::Pockets, p.item));
        }
        self.pack.as_ref().and_then(|c| c.get(id)).map(|p| (Place::Pack, p.item))
    }

    /// Every item carried, in the grids.
    pub fn grid_items(&self) -> impl Iterator<Item = &Item> {
        self.containers().flat_map(|c| c.items.iter().map(|p| &p.item))
    }

    /// How many rounds of `kind` are carried loose in the grids.
    pub fn rounds(&self, kind: AmmoKind) -> u32 {
        self.grid_items().filter(|i| i.kind == ItemKind::Ammo(kind)).map(|i| i.count).sum()
    }

    /// Takes up to `wanted` rounds of `kind` out of the grids (emptying the smallest stacks first, so
    /// that full ones stay full) and says how many there were.
    pub fn take_rounds(&mut self, kind: AmmoKind, wanted: u32) -> u32 {
        let mut stacks: Vec<(Place, u32, u32)> = Vec::new();
        for (place, container) in [(Place::Pockets, Some(&self.pockets)), (Place::Pack, self.pack.as_ref())] {
            for p in container.into_iter().flat_map(|c| &c.items) {
                if p.item.kind == ItemKind::Ammo(kind) {
                    stacks.push((place, p.item.id, p.item.count));
                }
            }
        }
        stacks.sort_by_key(|s| s.2);
        let mut left = wanted;
        for (place, id, count) in stacks {
            if left == 0 {
                break;
            }
            let take = count.min(left);
            left -= take;
            let container = self.container_mut(place).expect("it was found there");
            if take == count {
                container.remove(id);
            } else if let Some(p) = container.items.iter_mut().find(|p| p.item.id == id) {
                p.item.count -= take;
            }
        }
        wanted - left
    }

    /// Puts an item in the first place it fits (rounds first topping up any stack of the same kind
    /// that has room). Gives the item back, less whatever went in, if it doesn't all fit.
    pub fn add(&mut self, mut item: Item) -> Result<(), Item> {
        if let ItemKind::Ammo(_) = item.kind {
            let limit = item.kind.stack_limit();
            for container in [Some(&mut self.pockets), self.pack.as_mut()].into_iter().flatten() {
                for p in &mut container.items {
                    if p.item.kind == item.kind && p.item.count < limit && item.count > 0 {
                        let moved = (limit - p.item.count).min(item.count);
                        p.item.count += moved;
                        item.count -= moved;
                    }
                }
            }
            // What is left goes in new stacks, a square each.
            while item.count > 0 {
                let count = item.count.min(limit);
                let stack = Item { count, id: if count == item.count { item.id } else { self.fresh_id() }, ..item };
                let Some(place) = self.find_place(&stack) else { return Err(item) };
                let (container, (x, y, turned)) = place;
                self.container_mut(container).expect("exists").place(stack, x, y, turned);
                item.count -= count;
            }
            return Ok(());
        }
        let Some((container, (x, y, turned))) = self.find_place(&item) else { return Err(item) };
        self.container_mut(container).expect("exists").place(item, x, y, turned);
        Ok(())
    }

    fn fresh_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn find_place(&self, item: &Item) -> Option<(Place, (u32, u32, bool))> {
        [Place::Pockets, Place::Pack].into_iter().find_map(|place| self.container(place)?.find_space(item).map(|spot| (place, spot)))
    }

    /// Whether `item` would fit somewhere in the grids, if it were not where it is.
    pub fn has_room_for(&self, item: &Item) -> bool {
        if let ItemKind::Ammo(_) = item.kind {
            let room: u32 = self
                .grid_items()
                .filter(|i| i.kind == item.kind)
                .map(|i| item.kind.stack_limit() - i.count.min(item.kind.stack_limit()))
                .sum();
            if room >= item.count {
                return true;
            }
        }
        self.find_place(item).is_some()
    }

    /// Takes an item out of wherever it is and gives it to the caller.
    pub fn take(&mut self, id: u32) -> Option<Item> {
        let (place, _) = self.find(id)?;
        match place {
            Place::Slot(i) => self.slots[i].take(),
            Place::Back => {
                let pack = self.back.take();
                self.pack = None;
                pack
            }
            Place::Pockets => self.pockets.remove(id).map(|p| p.item),
            Place::Pack => self.pack.as_mut()?.remove(id).map(|p| p.item),
        }
    }

    /// Moves an item to a square of a grid (turned or not), if it fits there. Whatever is there
    /// already is left alone: the move is refused.
    pub fn move_to_grid(&mut self, id: u32, to: Place, x: u32, y: u32, turned: bool) -> bool {
        let Some((from, item)) = self.find(id) else { return false };
        let Some(target) = self.container(to) else { return false };
        let (w, h) = item.size();
        let size = if turned { (h, w) } else { (w, h) };
        // Moving within a grid doesn't count the item itself as in the way.
        let except = (from == to).then_some(id);
        if !target.fits(size, x, y, except) {
            return false;
        }
        // A pack on the back comes off into the pockets, with what is in it, wherever there is room.
        if from == Place::Back {
            return to == Place::Pockets && self.remove_pack();
        }
        let Some(item) = self.take(id) else { return false };
        self.container_mut(to).expect("checked").items.push(Placed { item, x, y, turned });
        true
    }

    /// Moves what is in the pack into the pockets, if it all fits there.
    fn unload_pack(&mut self) -> bool {
        let Some(pack) = &self.pack else { return true };
        let mut trial = self.pockets.clone();
        for p in &pack.items {
            let Some((x, y, turned)) = trial.find_space(&p.item) else { return false };
            trial.place(p.item, x, y, turned);
        }
        self.pockets = trial;
        if let Some(pack) = &mut self.pack {
            pack.items.clear();
        }
        true
    }

    /// Puts a weapon from the grids (or the other slot) into weapon slot `slot`. A gun already there
    /// goes back where the new one came from if it fits, else anywhere there's room, else the move is refused.
    pub fn equip_weapon(&mut self, id: u32, slot: usize) -> bool {
        let Some((from, item)) = self.find(id) else { return false };
        if !matches!(item.kind, ItemKind::Weapon(_)) || slot >= 2 || from == Place::Slot(slot) {
            return false;
        }
        // Whatever is in the slot has to have somewhere to go.
        if let Some(old) = self.slots[slot] {
            if let Place::Slot(other) = from {
                // Two guns swap places.
                self.slots[other] = Some(old);
                self.slots[slot] = Some(item);
                return true;
            }
            let mut trial = self.clone();
            trial.take(id);
            if trial.add(old).is_err() {
                return false;
            }
            *self = trial;
            self.slots[slot] = Some(item);
            return true;
        }
        let Some(item) = self.take(id) else { return false };
        self.slots[slot] = Some(item);
        true
    }

    /// Takes the gun out of a slot and puts it in the grids, if there is room.
    pub fn unequip_weapon(&mut self, slot: usize) -> bool {
        let Some(item) = self.slots.get(slot).copied().flatten() else { return false };
        if !self.has_room_for(&item) {
            return false;
        }
        self.slots[slot] = None;
        self.add(item).is_ok()
    }

    /// Puts a pack from the grids on the player's back. The one already there goes back in the
    /// grids; if the grids can't take what its grid held, nothing happens.
    pub fn wear_pack(&mut self, id: u32) -> bool {
        let Some((from, item)) = self.find(id) else { return false };
        let ItemKind::Pack(kind) = item.kind else { return false };
        if from == Place::Back {
            return false;
        }
        let mut trial = self.clone();
        trial.take(id);
        // The pack that was on, with what was in it, goes to the pockets.
        if let Some(old) = trial.back {
            if !trial.unload_pack() {
                return false;
            }
            trial.back = None;
            trial.pack = None;
            if trial.add(old).is_err() {
                return false;
            }
        }
        let (w, h) = kind.capacity();
        trial.back = Some(item);
        trial.pack = Some(Container::new(w, h));
        *self = trial;
        true
    }

    /// Takes the pack off, putting what was in it, and then the pack, in the pockets. Refused if they don't fit.
    pub fn remove_pack(&mut self) -> bool {
        let Some(old) = self.back else { return false };
        let mut trial = self.clone();
        if !trial.unload_pack() {
            return false;
        }
        trial.back = None;
        trial.pack = None;
        if trial.add(old).is_err() {
            return false;
        }
        *self = trial;
        true
    }

    /// The gun in hand, if the slot isn't empty.
    pub fn in_hand(&self) -> Option<&Item> {
        self.slots[self.active].as_ref()
    }

    pub fn in_hand_mut(&mut self) -> Option<&mut Item> {
        self.slots[self.active].as_mut()
    }

    /// How many squares there are to put things in, and how many are used.
    pub fn space(&self) -> (u32, u32) {
        let total: u32 = self.containers().map(|c| c.width * c.height).sum();
        let used: u32 = self.containers().map(Container::squares_used).sum();
        (used, total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_items() -> Inventory {
        Inventory::default()
    }

    #[test]
    fn a_big_thing_takes_more_squares_and_is_found_a_place_that_it_fits() {
        let mut inv = with_items();
        let rifle = inv.make(ItemKind::Weapon(WeaponKind::Slr), 1);
        assert_eq!(rifle.size(), (6, 2));
        assert!(inv.add(rifle).is_ok());
        let placed = inv.pockets.get(rifle.id).expect("in the pockets");
        assert_eq!((placed.x, placed.y, placed.turned), (0, 0, false));
        assert_eq!(inv.pockets.squares_used(), 12);
        // A second one goes below it; a third doesn't fit (6 x 4 is full after two).
        let second = inv.make(ItemKind::Weapon(WeaponKind::Slr), 1);
        assert!(inv.add(second).is_ok());
        let third = inv.make(ItemKind::Weapon(WeaponKind::Slr), 1);
        assert!(inv.add(third).is_err());
        assert_eq!(inv.pockets.squares_used(), 24);
    }

    #[test]
    fn things_are_turned_on_their_side_when_that_is_the_only_way_they_fit() {
        let mut inv = with_items();
        // Fill all but a 2-wide, 4-tall strip.
        for k in 0..4 {
            let bandage = inv.make(ItemKind::Supply(Supply::FirstAid), 1);
            assert!(inv.pockets.place(bandage, (k % 2) * 2, (k / 2) * 2, false));
        }
        // Pockets are 6 wide: the first 4 columns are full; a 1-wide, 2-tall canteen goes in the strip, upright.
        let canteen = inv.make(ItemKind::Supply(Supply::Canteen), 1);
        assert_eq!(inv.pockets.find_space(&canteen), Some((4, 0, false)));
        // A pistol (2 x 2) needs the strip's two columns.
        let pistol = inv.make(ItemKind::Weapon(WeaponKind::HiPower), 1);
        assert_eq!(inv.pockets.find_space(&pistol), Some((4, 0, false)));
        // A long gun lying across a free row would need turning if the room were tall and narrow.
        let mut tall = Container::new(2, 6);
        let rifle = inv.make(ItemKind::Weapon(WeaponKind::Slr), 1);
        assert_eq!(tall.find_space(&rifle), Some((0, 0, true)));
        assert!(tall.place(rifle, 0, 0, true));
        assert_eq!(tall.items[0].extent(), (2, 6));
    }

    #[test]
    fn rounds_top_up_stacks_before_starting_new_ones() {
        let mut inv = with_items();
        let a = inv.make(ItemKind::Ammo(AmmoKind::NineFmj), 30);
        inv.add(a).unwrap();
        let b = inv.make(ItemKind::Ammo(AmmoKind::NineFmj), 40);
        inv.add(b).unwrap();
        // 70 rounds: a full stack of 50 and one of 20.
        let mut counts: Vec<u32> = inv.pockets.items.iter().map(|p| p.item.count).collect();
        counts.sort();
        assert_eq!(counts, vec![20, 50]);
        assert_eq!(inv.rounds(AmmoKind::NineFmj), 70);
        assert_eq!(inv.rounds(AmmoKind::NineHollowPoint), 0);
    }

    #[test]
    fn taking_rounds_empties_the_smallest_stack_first() {
        let mut inv = with_items();
        let a = inv.make(ItemKind::Ammo(AmmoKind::NineFmj), 120);
        inv.add(a).unwrap();
        assert_eq!(inv.pockets.items.len(), 3);
        assert_eq!(inv.take_rounds(AmmoKind::NineFmj, 25), 25);
        let mut counts: Vec<u32> = inv.pockets.items.iter().map(|p| p.item.count).collect();
        counts.sort();
        assert_eq!(counts, vec![45, 50], "the stack of 20 went, and 5 more off the next");
        assert_eq!(inv.take_rounds(AmmoKind::NineFmj, 500), 95, "only what there is");
        assert!(inv.pockets.items.is_empty());
    }

    #[test]
    fn what_wont_fit_comes_back() {
        let mut inv = with_items();
        for _ in 0..24 {
            let it = inv.make(ItemKind::Supply(Supply::TinnedFood), 1);
            inv.add(it).unwrap();
        }
        let extra = inv.make(ItemKind::Supply(Supply::TinnedFood), 1);
        assert_eq!(inv.add(extra), Err(extra));
        let rounds = inv.make(ItemKind::Ammo(AmmoKind::NineFmj), 10);
        assert!(!inv.has_room_for(&rounds));
        // Ammunition that tops up a stack that has room doesn't need a new square.
        let mut inv = with_items();
        let a = inv.make(ItemKind::Ammo(AmmoKind::NineFmj), 10);
        inv.add(a).unwrap();
        for _ in 0..23 {
            let it = inv.make(ItemKind::Supply(Supply::TinnedFood), 1);
            inv.add(it).unwrap();
        }
        let more = inv.make(ItemKind::Ammo(AmmoKind::NineFmj), 30);
        assert!(inv.has_room_for(&more) && inv.add(more).is_ok());
        assert_eq!(inv.rounds(AmmoKind::NineFmj), 40);
    }

    #[test]
    fn a_pack_gives_more_room_and_cannot_come_off_unless_what_is_in_it_fits_in_the_pockets() {
        let mut inv = with_items();
        let pack = inv.make(ItemKind::Pack(PackKind::Bergen), 1);
        inv.add(pack).unwrap();
        assert_eq!(inv.space().1, 24);
        assert!(inv.wear_pack(pack.id));
        assert_eq!(inv.back.map(|b| b.id), Some(pack.id));
        assert_eq!(inv.space().1, 24 + 36 - 0, "the pockets and the pack's grid");
        assert!(inv.pockets.get(pack.id).is_none(), "it is on the back now");
        // Things go in the pack when the pockets are full.
        for _ in 0..24 {
            let it = inv.make(ItemKind::Supply(Supply::TinnedFood), 1);
            inv.add(it).unwrap();
        }
        let in_pack = inv.make(ItemKind::Supply(Supply::TinnedFood), 1);
        inv.add(in_pack).unwrap();
        assert_eq!(inv.pack.as_ref().unwrap().items.len(), 1);
        // The pockets are full, so the pack can't come off with something in it.
        assert!(!inv.remove_pack());
        assert!(inv.back.is_some());
        // Make room, and it can.
        inv.pockets.items.clear();
        assert!(inv.remove_pack());
        assert!(inv.back.is_none() && inv.pack.is_none());
        assert_eq!(inv.pockets.items.len(), 2, "the tin and the pack");
    }

    #[test]
    fn two_weapon_slots_take_any_weapon_and_swap_with_what_is_there() {
        let mut inv = with_items();
        let a = inv.make_gun(WeaponKind::Slr, AmmoKind::NatoBall, 12);
        let b = inv.make_gun(WeaponKind::HiPower, AmmoKind::NineFmj, 13);
        let c = inv.make_gun(WeaponKind::Bren, AmmoKind::NatoBall, 30);
        for g in [a, b] {
            inv.add(g).unwrap();
        }
        assert!(inv.equip_weapon(a.id, 0));
        assert!(inv.equip_weapon(b.id, 1));
        assert_eq!(inv.slots[0].map(|g| g.id), Some(a.id));
        assert_eq!(inv.slots[1].map(|g| g.id), Some(b.id));
        inv.add(c).unwrap();
        // A third gun displaces what is in a slot, which goes back to the grid.
        assert!(inv.equip_weapon(c.id, 0));
        assert_eq!(inv.slots[0].map(|g| g.id), Some(c.id));
        assert!(inv.pockets.get(a.id).is_some(), "the displaced rifle is carried");
        // The loaded state goes with the gun.
        assert_eq!(inv.pockets.get(a.id).unwrap().item.loaded, 12);
        // The two slots swap.
        assert!(inv.equip_weapon(b.id, 0));
        assert_eq!((inv.slots[0].map(|g| g.id), inv.slots[1].map(|g| g.id)), (Some(b.id), Some(c.id)));
        // Only guns go in the slots.
        let tin = inv.make(ItemKind::Supply(Supply::TinnedFood), 1);
        inv.add(tin).unwrap();
        assert!(!inv.equip_weapon(tin.id, 1));
    }

    #[test]
    fn a_displaced_gun_with_nowhere_to_go_blocks_the_swap() {
        let mut inv = with_items();
        let mag = inv.make_gun(WeaponKind::Mag, AmmoKind::NatoBall, 100);
        inv.slots[0] = Some(mag);
        // The pockets hold a pistol (4 squares) and are otherwise full of tins (20 squares).
        let pistol = inv.make_gun(WeaponKind::HiPower, AmmoKind::NineFmj, 13);
        inv.add(pistol).unwrap();
        for _ in 0..20 {
            let it = inv.make(ItemKind::Supply(Supply::TinnedFood), 1);
            inv.add(it).unwrap();
        }
        assert_eq!(inv.pockets.squares_used(), 24);
        // The machine gun (18 squares) can't go where the pistol (4) was, so the pistol can't replace it.
        let before = inv.clone();
        assert!(!inv.equip_weapon(pistol.id, 0));
        assert_eq!(inv.slots, before.slots);
        assert_eq!(inv.pockets, before.pockets, "and nothing was disturbed");
        // And the machine gun can't be put away either.
        assert!(!inv.unequip_weapon(0));
    }

    #[test]
    fn moving_within_a_grid_ignores_the_item_itself_and_refuses_overlaps() {
        let mut inv = with_items();
        let a = inv.make(ItemKind::Weapon(WeaponKind::HiPower), 1);
        let b = inv.make(ItemKind::Weapon(WeaponKind::HiPower), 1);
        assert!(inv.pockets.place(a, 0, 0, false));
        assert!(inv.pockets.place(b, 2, 0, false));
        // One square along overlaps the other.
        assert!(!inv.move_to_grid(a.id, Place::Pockets, 1, 0, false));
        // Down a row doesn't.
        assert!(inv.move_to_grid(a.id, Place::Pockets, 0, 2, false));
        assert_eq!(inv.pockets.get(a.id).map(|p| (p.x, p.y)), Some((0, 2)));
        // Off the edge doesn't.
        assert!(!inv.move_to_grid(a.id, Place::Pockets, 5, 0, false));
        // And an item can be moved onto the squares it is on now (a small nudge).
        assert!(inv.move_to_grid(b.id, Place::Pockets, 3, 0, false));
    }

    #[test]
    fn the_starting_kit_has_two_guns_and_rounds_for_both() {
        let inv = Inventory::starting();
        assert_eq!(inv.slots[0].map(|g| g.kind), Some(ItemKind::Weapon(WeaponKind::Sterling)));
        assert_eq!(inv.slots[1].map(|g| g.kind), Some(ItemKind::Weapon(WeaponKind::HiPower)));
        assert!(inv.rounds(AmmoKind::NineFmj) >= 100);
        assert_eq!(inv.slots[0].unwrap().loaded, 30);
        // Every id is different.
        let mut ids: Vec<u32> = inv.grid_items().map(|i| i.id).collect();
        ids.extend(inv.slots.iter().flatten().map(|i| i.id));
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n);
    }

    #[test]
    fn find_and_take_work_wherever_a_thing_is() {
        let mut inv = Inventory::starting();
        let gun = inv.slots[1].unwrap();
        assert_eq!(inv.find(gun.id).map(|(p, _)| p), Some(Place::Slot(1)));
        assert_eq!(inv.take(gun.id).map(|i| i.id), Some(gun.id));
        assert!(inv.find(gun.id).is_none());
        let stack = inv.pockets.items[0].item;
        assert_eq!(inv.find(stack.id).map(|(p, _)| p), Some(Place::Pockets));
        assert!(inv.take(stack.id).is_some());
    }
}

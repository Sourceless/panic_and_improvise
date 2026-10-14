//! Things lying about to be found, for now as coloured boxes: the colour says how rare. A closed box
//! holds a handful of things, which fall out round it when it is opened.

use bevy::prelude::*;

use crate::ammo::AmmoKind;
use crate::inventory::{ItemKind, PackKind, Supply};
use crate::settlement_plan::Rng;
use crate::weapons::WeaponKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Rarity {
    Common,
    Uncommon,
    Rare,
    VeryRare,
    Legendary,
}

impl Rarity {
    pub const ALL: [Rarity; 5] = [Rarity::Common, Rarity::Uncommon, Rarity::Rare, Rarity::VeryRare, Rarity::Legendary];

    /// How often each turns up, of everything that does, in an ordinary house.
    fn base_weight(self) -> f32 {
        match self {
            Rarity::Common => 0.56,
            Rarity::Uncommon => 0.25,
            Rarity::Rare => 0.12,
            Rarity::VeryRare => 0.05,
            Rarity::Legendary => 0.015,
        }
    }

    /// The colour of its box: grey, green, blue, purple and orange.
    pub fn colour(self) -> Color {
        match self {
            Rarity::Common => Color::srgb(0.78, 0.78, 0.8),
            Rarity::Uncommon => Color::srgb(0.2, 0.85, 0.25),
            Rarity::Rare => Color::srgb(0.2, 0.45, 1.0),
            Rarity::VeryRare => Color::srgb(0.7, 0.25, 0.95),
            Rarity::Legendary => Color::srgb(1.0, 0.6, 0.1),
        }
    }

    /// How big its box is: the rarer, the bigger.
    pub fn size(self) -> f32 {
        0.2 + 0.035 * self as usize as f32
    }

    pub fn name(self) -> &'static str {
        match self {
            Rarity::Common => "common",
            Rarity::Uncommon => "uncommon",
            Rarity::Rare => "rare",
            Rarity::VeryRare => "very rare",
            Rarity::Legendary => "legendary",
        }
    }

    /// Which it is, given a roll in 0 to 1 and how rich the place is (0 for an ordinary home; higher
    /// shifts the odds toward the rare ones, each step up the ladder `1 + richness` times likelier).
    pub fn roll(u: f32, richness: f32) -> Rarity {
        let weights: Vec<f32> = Rarity::ALL.iter().enumerate().map(|(i, r)| r.base_weight() * (1.0 + richness).powi(i as i32)).collect();
        let total: f32 = weights.iter().sum();
        let mut left = u.clamp(0.0, 0.999_999) * total;
        for (r, w) in Rarity::ALL.iter().zip(weights) {
            if left < w {
                return *r;
            }
            left -= w;
        }
        Rarity::Common
    }
}

/// What a spot of loot turns out to be when it is looked at: what it is, how many (rounds, for ammunition),
/// and for a gun how many rounds are in it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Found {
    pub kind: ItemKind,
    pub count: u32,
    pub loaded: u32,
}

/// A thing that can be found, and how likely: its weight among those of its rarity.
struct Entry {
    weight: f32,
    kind: ItemKind,
    /// How many rounds, for ammunition: (least, most).
    rounds: (u32, u32),
}

const fn ammo(weight: f32, kind: AmmoKind, least: u32, most: u32) -> Entry {
    Entry { weight, kind: ItemKind::Ammo(kind), rounds: (least, most) }
}

const fn thing(weight: f32, kind: ItemKind) -> Entry {
    Entry { weight, kind, rounds: (1, 1) }
}

/// What turns up at each rarity: a few rounds and bits and pieces are common; bigger piles of rounds,
/// bags and first aid less so; pistols, rifles, shotguns and bigger packs are rarer; machine guns and the
/// sniper rifle are legendary.
fn table(rarity: Rarity) -> Vec<Entry> {
    use AmmoKind::*;
    use ItemKind::{Pack, Supply as Sup, Weapon};
    match rarity {
        Rarity::Common => vec![
            ammo(3.0, NineFmj, 15, 50),
            ammo(2.0, BritishBall, 5, 20),
            ammo(1.5, NatoBall, 5, 20),
            ammo(1.5, Buckshot, 2, 6),
            thing(2.0, Sup(Supply::Bandage)),
            thing(1.5, Sup(Supply::TinnedFood)),
            thing(1.0, Sup(Supply::Canteen)),
            thing(0.5, Sup(Supply::Compass)),
        ],
        Rarity::Uncommon => vec![
            ammo(1.5, NineFmj, 20, 50),
            ammo(1.0, BritishBall, 10, 20),
            ammo(1.0, NatoBall, 10, 20),
            ammo(1.2, Buckshot, 3, 10),
            thing(1.5, Sup(Supply::FirstAid)),
            thing(1.0, Sup(Supply::Torch)),
            thing(1.5, Pack(PackKind::Satchel)),
        ],
        Rarity::Rare => vec![
            thing(2.0, Weapon(WeaponKind::HiPower)),
            thing(2.0, Weapon(WeaponKind::LeeEnfield)),
            thing(1.5, Weapon(WeaponKind::Auto5)),
            thing(1.5, Pack(PackKind::Daypack)),
            ammo(1.0, NatoBall, 15, 20),
            ammo(0.5, BritishBall, 15, 20),
            ammo(0.5, Buckshot, 5, 10),
        ],
        Rarity::VeryRare => vec![
            thing(2.0, Weapon(WeaponKind::Slr)),
            thing(1.5, Weapon(WeaponKind::Sterling)),
            thing(1.5, Weapon(WeaponKind::Bren)),
            thing(1.5, Pack(PackKind::Bergen)),
            ammo(1.0, NatoBall, 15, 20),
        ],
        Rarity::Legendary => vec![
            thing(2.0, Weapon(WeaponKind::Mag)),
            thing(2.0, Weapon(WeaponKind::L42)),
            ammo(1.5, NatoBall, 20, 20),
            thing(1.0, Pack(PackKind::Bergen)),
        ],
    }
}

/// What is at a spot of loot of `rarity`, from a source of numbers in 0..1.
pub fn roll_item(rarity: Rarity, mut next: impl FnMut() -> f32) -> Found {
    let entries = table(rarity);
    let total: f32 = entries.iter().map(|e| e.weight).sum();
    let mut left = next() * total;
    let entry = entries.iter().find(|e| {
        left -= e.weight;
        left < 0.0
    });
    let entry = entry.unwrap_or(&entries[0]);
    let count = if entry.rounds.0 == entry.rounds.1 { entry.rounds.0 } else { entry.rounds.0 + (next() * (entry.rounds.1 - entry.rounds.0 + 1) as f32) as u32 };
    // A gun is found with something in it, from empty to full.
    let loaded = match entry.kind {
        ItemKind::Weapon(w) => (next() * (w.def().magazine + 1) as f32) as u32,
        _ => 0,
    };
    Found { kind: entry.kind, count: count.max(1), loaded }
}

/// One thing that fell out of a box, and how rare it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dropped {
    pub rarity: Rarity,
    pub found: Found,
}

impl Rarity {
    /// How many things fall out of a box of this rarity: (least, most).
    pub fn handful(self) -> (u32, u32) {
        match self {
            Rarity::Common => (2, 3),
            Rarity::Uncommon => (3, 4),
            Rarity::Rare => (3, 5),
            Rarity::VeryRare => (4, 5),
            Rarity::Legendary => (4, 6),
        }
    }
}

/// What falls out of a box of `rarity`: a handful, the first of the box's own rarity and the rest
/// whatever might be lying about, no better than the box.
pub fn roll_contents(rarity: Rarity, mut next: impl FnMut() -> f32) -> Vec<Dropped> {
    let (least, most) = rarity.handful();
    let count = least + (next() * (most - least + 1) as f32) as u32;
    (0..count.min(most))
        .map(|i| {
            let this = if i == 0 { rarity } else { Rarity::roll(next(), 0.0).min(rarity) };
            Dropped { rarity: this, found: roll_item(this, &mut next) }
        })
        .collect()
}

/// Where the `slot`th of `count` things from the box at `spot` lies, as an offset from the box's floor:
/// spread round it, and the same every time.
pub fn drop_offset(spot: (usize, usize), slot: usize, count: usize) -> Vec3 {
    let turn = ((spot.0.wrapping_mul(31).wrapping_add(spot.1.wrapping_mul(17))) % 360) as f32 * std::f32::consts::TAU / 360.0;
    let angle = turn + slot as f32 / count.max(1) as f32 * std::f32::consts::TAU;
    let radius = 0.55 + 0.2 * (slot % 2) as f32;
    Vec3::new(angle.cos() * radius, 0.0, angle.sin() * radius)
}

/// A repeatable source of numbers for one spot of loot: the same spot is the same thing every time.
pub fn spot_numbers(building: usize, index: usize) -> impl FnMut() -> f32 {
    let mut rng = Rng::from_position(Vec2::new(building as f32 * 1.37 + 0.11, index as f32 * 2.71 + 0.53));
    // The first few numbers of a fresh generator are alike for near seeds: let it settle.
    for _ in 0..4 {
        rng.unit();
    }
    move || rng.unit()
}

/// A thing lying on the ground to be picked up.
#[derive(Component)]
pub struct Pickup {
    pub rarity: Rarity,
    pub found: Found,
    /// The box it came from, and which of its things it is, so that it stays gone once taken.
    pub spot: (usize, usize),
    pub slot: usize,
}

/// Boxes that have been opened, and what of their contents is still lying there (`None` once taken).
#[derive(Resource, Default)]
pub struct OpenedLoot(pub std::collections::HashMap<(usize, usize), Vec<Option<Dropped>>>);

/// A closed box of loot lying where it was put: open it to see what is in it.
#[derive(Component)]
pub struct LootBox {
    pub rarity: Rarity,
    pub spot: (usize, usize),
}

/// The one box mesh and a glowing material for each rarity.
#[derive(Resource)]
pub struct LootAssets {
    pub mesh: Handle<Mesh>,
    pub materials: Vec<Handle<StandardMaterial>>,
}

impl LootAssets {
    pub fn material(&self, rarity: Rarity) -> Handle<StandardMaterial> {
        self.materials[rarity as usize].clone()
    }
}

/// A thing from an opened box, lying `floor` (the box's floor) and offset round it.
pub fn drop_bundle(assets: &LootAssets, floor: Vec3, spot: (usize, usize), slot: usize, count: usize, drop: Dropped) -> impl Bundle {
    let size = drop.rarity.size() * 0.7;
    (
        Pickup { rarity: drop.rarity, found: drop.found, spot, slot },
        Mesh3d(assets.mesh.clone()),
        MeshMaterial3d(assets.material(drop.rarity)),
        Transform::from_translation(floor + drop_offset(spot, slot, count) + Vec3::Y * (size * 0.5 + 0.03)).with_scale(Vec3::splat(size)),
    )
}

pub struct LootPlugin;

impl Plugin for LootPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OpenedLoot>().add_systems(Startup, make_assets).add_systems(Update, spin);
    }
}

fn make_assets(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    // Lit by nothing, so that they show up in a dark cellar as well as in the sun.
    let materials = Rarity::ALL.iter().map(|r| materials.add(StandardMaterial { base_color: r.colour(), unlit: true, ..default() })).collect();
    commands.insert_resource(LootAssets { mesh, materials });
}

/// Boxes turn slowly, so that they catch the eye.
fn spin(time: Res<Time>, mut boxes: Query<&mut Transform, Or<(With<LootBox>, With<Pickup>)>>) {
    for mut t in &mut boxes {
        t.rotate_y(time.delta_secs() * 0.9);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn most_loot_is_common_and_the_ladder_thins_out() {
        let mut counts = [0usize; 5];
        for i in 0..10_000 {
            counts[Rarity::roll(i as f32 / 10_000.0, 0.0) as usize] += 1;
        }
        assert!(counts.windows(2).all(|w| w[0] > w[1]), "each rung rarer than the one below: {counts:?}");
        assert!(counts[0] > 5000 && counts[4] > 0 && counts[4] < 400, "{counts:?}");
    }

    #[test]
    fn a_richer_place_has_better_loot() {
        let rare_or_better = |richness: f32| (0..10_000).filter(|&i| Rarity::roll(i as f32 / 10_000.0, richness) >= Rarity::Rare).count();
        assert!(rare_or_better(2.0) > rare_or_better(0.0) * 2);
    }

    #[test]
    fn the_rarer_the_bigger_and_each_has_its_own_colour() {
        assert!(Rarity::ALL.windows(2).all(|w| w[0].size() < w[1].size()));
        for (i, a) in Rarity::ALL.iter().enumerate() {
            for b in &Rarity::ALL[i + 1..] {
                assert_ne!(a.colour(), b.colour());
            }
        }
    }

    #[test]
    fn what_is_found_is_the_same_every_time_for_the_same_spot() {
        assert_eq!(roll_contents(Rarity::Rare, spot_numbers(12, 3)), roll_contents(Rarity::Rare, spot_numbers(12, 3)));
        let a = roll_item(Rarity::Rare, spot_numbers(12, 3));
        let b = roll_item(Rarity::Rare, spot_numbers(12, 3));
        assert_eq!(a, b);
        let many: std::collections::HashSet<String> = (0..40).map(|i| format!("{:?}", roll_item(Rarity::Rare, spot_numbers(i, 0)).kind)).collect();
        assert!(many.len() >= 3, "there is a choice: {many:?}");
    }

    #[test]
    fn the_rarer_the_spot_the_better_what_is_in_it() {
        // Count the guns that turn up at each rarity.
        let guns = |rarity| (0..400).filter(|&i| matches!(roll_item(rarity, spot_numbers(i, 1)).kind, ItemKind::Weapon(_))).count();
        assert_eq!(guns(Rarity::Common), 0);
        assert_eq!(guns(Rarity::Uncommon), 0);
        assert!(guns(Rarity::Rare) > 100 && guns(Rarity::VeryRare) > 200 && guns(Rarity::Legendary) > 150);
        // The machine gun and the sniper rifle are only found at the top.
        for rarity in [Rarity::Common, Rarity::Uncommon, Rarity::Rare, Rarity::VeryRare] {
            for i in 0..300 {
                let kind = roll_item(rarity, spot_numbers(i, 2)).kind;
                assert!(!matches!(kind, ItemKind::Weapon(WeaponKind::Mag | WeaponKind::L42)), "{rarity:?} gave {kind:?}");
            }
        }
        assert!((0..300).any(|i| matches!(roll_item(Rarity::Legendary, spot_numbers(i, 2)).kind, ItemKind::Weapon(WeaponKind::Mag))));
        assert!((0..300).any(|i| matches!(roll_item(Rarity::Legendary, spot_numbers(i, 2)).kind, ItemKind::Weapon(WeaponKind::L42))));
    }

    #[test]
    fn rounds_come_in_amounts_a_square_can_hold_and_guns_have_no_more_than_a_magazine_in_them() {
        for rarity in Rarity::ALL {
            for i in 0..300 {
                let found = roll_item(rarity, spot_numbers(i, 5));
                match found.kind {
                    ItemKind::Ammo(a) => assert!(found.count >= 1 && found.count <= a.def().per_stack, "{a:?} x{}", found.count),
                    ItemKind::Weapon(w) => assert!(found.loaded <= w.def().magazine),
                    _ => assert_eq!(found.count, 1),
                }
            }
        }
    }

    #[test]
    fn a_box_gives_a_handful_the_richer_the_more_and_nothing_better_than_the_box() {
        for rarity in Rarity::ALL {
            let (least, most) = rarity.handful();
            for i in 0..200 {
                let contents = roll_contents(rarity, spot_numbers(i, 7));
                assert!((least as usize..=most as usize).contains(&contents.len()), "{rarity:?}: {}", contents.len());
                assert_eq!(contents[0].rarity, rarity, "the first is the box's own");
                assert!(contents.iter().all(|d| d.rarity <= rarity));
            }
        }
        assert!(Rarity::ALL.windows(2).all(|w| w[0].handful().1 <= w[1].handful().1 && w[0].handful().0 <= w[1].handful().0));
    }

    #[test]
    fn what_falls_out_lies_apart_round_the_box() {
        let spots: Vec<Vec3> = (0..5).map(|slot| drop_offset((3, 1), slot, 5)).collect();
        for (i, a) in spots.iter().enumerate() {
            assert!(a.length() > 0.4 && a.length() < 1.0 && a.y == 0.0);
            for b in &spots[i + 1..] {
                assert!(a.distance(*b) > 0.35, "{a} {b}");
            }
        }
        assert_eq!(drop_offset((3, 1), 2, 5), drop_offset((3, 1), 2, 5));
    }
}

//! Things lying about to be found, for now as coloured boxes: the colour says how good what is in it is,
//! and the size of the box how much. A closed box holds a handful of things, which fall out round it when
//! it is opened.

use bevy::prelude::*;

use std::collections::HashMap;

use crate::ammo::AmmoKind;
use crate::building::geom::Rect;
use crate::gun_models::{self, Finish};
use crate::item_models::{self, Paint};
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
            Rarity::Legendary => 0.005,
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

    /// The number the network knows it by.
    pub fn code(self) -> u8 {
        self as u8
    }

    pub fn from_code(code: u8) -> Option<Rarity> {
        Rarity::ALL.get(code as usize).copied()
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

/// How big a box is, and so how much is in it: small, medium, large, extra large and extra extra large.
/// Nothing to do with its rarity (what is in it is as good as that says, however many things there are).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum BoxSize {
    S,
    M,
    L,
    XL,
    XXL,
}

impl BoxSize {
    pub const ALL: [BoxSize; 5] = [BoxSize::S, BoxSize::M, BoxSize::L, BoxSize::XL, BoxSize::XXL];

    /// How often each turns up: the bigger, the less.
    fn weight(self) -> f32 {
        match self {
            BoxSize::S => 0.42,
            BoxSize::M => 0.30,
            BoxSize::L => 0.17,
            BoxSize::XL => 0.08,
            BoxSize::XXL => 0.03,
        }
    }

    /// Which it is, given a roll in 0 to 1.
    pub fn roll(u: f32) -> BoxSize {
        let total: f32 = BoxSize::ALL.iter().map(|s| s.weight()).sum();
        let mut left = u.clamp(0.0, 0.999_999) * total;
        for s in BoxSize::ALL {
            if left < s.weight() {
                return s;
            }
            left -= s.weight();
        }
        BoxSize::S
    }

    pub fn name(self) -> &'static str {
        match self {
            BoxSize::S => "S",
            BoxSize::M => "M",
            BoxSize::L => "L",
            BoxSize::XL => "XL",
            BoxSize::XXL => "XXL",
        }
    }

    /// How many things fall out of a box this size: (least, most).
    pub fn handful(self) -> (u32, u32) {
        match self {
            BoxSize::S => (1, 2),
            BoxSize::M => (2, 3),
            BoxSize::L => (3, 4),
            BoxSize::XL => (4, 5),
            BoxSize::XXL => (5, 6),
        }
    }

    /// How long the crate is, metres.
    pub fn length(self) -> f32 {
        match self {
            BoxSize::S => 0.24,
            BoxSize::M => 0.32,
            BoxSize::L => 0.42,
            BoxSize::XL => 0.54,
            BoxSize::XXL => 0.68,
        }
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

/// What turns up at each rarity: a few rounds and bits and pieces are common, with the odd revolver; bigger
/// piles of rounds, bags, first aid, the pistol, the old bolt-action and the double-barrel are uncommon;
/// the submachine gun, the self-loading shotgun and bigger packs are rare; self-loading and light machine
/// guns are very rare; the machine gun and the sniper rifle are legendary.
///
/// Each gun is in one table only, so it is the same rarity (and colour) wherever it is found: the box it
/// comes out of only sets how likely it is, never what colour the gun is.
fn table(rarity: Rarity) -> Vec<Entry> {
    use AmmoKind::*;
    use ItemKind::{Pack, Supply as Sup, Weapon};
    match rarity {
        Rarity::Common => vec![
            ammo(2.0, NineFmj, 10, 30),
            ammo(1.5, BritishBall, 5, 15),
            ammo(1.5, NatoBall, 5, 15),
            ammo(1.5, Buckshot, 3, 6),
            thing(2.0, Sup(Supply::Bandage)),
            thing(1.5, Sup(Supply::TinnedFood)),
            thing(1.0, Sup(Supply::Canteen)),
            thing(0.5, Sup(Supply::Compass)),
            ammo(1.2, Webley455, 6, 12),
            thing(0.7, Weapon(WeaponKind::Revolver)),
        ],
        Rarity::Uncommon => vec![
            ammo(1.0, NineFmj, 15, 40),
            ammo(1.0, BritishBall, 10, 20),
            ammo(1.0, NatoBall, 10, 20),
            ammo(1.6, Buckshot, 4, 10),
            thing(1.5, Sup(Supply::FirstAid)),
            thing(1.0, Sup(Supply::Torch)),
            thing(1.5, Pack(PackKind::Satchel)),
            ammo(1.0, Webley455, 8, 18),
            thing(1.2, Weapon(WeaponKind::HiPower)),
            thing(1.4, Weapon(WeaponKind::LeeEnfield)),
            thing(1.2, Weapon(WeaponKind::DoubleBarrel)),
        ],
        Rarity::Rare => vec![
            thing(1.8, Weapon(WeaponKind::Sterling)),
            thing(1.2, Weapon(WeaponKind::Auto5)),
            ammo(0.6, Webley455, 12, 24),
            thing(1.5, Pack(PackKind::Daypack)),
            ammo(1.0, NatoBall, 15, 20),
            ammo(0.8, BritishBall, 15, 20),
            ammo(1.1, Buckshot, 6, 10),
            ammo(0.8, NineFmj, 30, 50),
        ],
        Rarity::VeryRare => vec![
            thing(2.0, Weapon(WeaponKind::Slr)),
            thing(1.5, Weapon(WeaponKind::Bren)),
            thing(1.5, Pack(PackKind::Bergen)),
            ammo(2.0, NatoBall, 15, 20),
        ],
        Rarity::Legendary => vec![
            thing(2.0, Weapon(WeaponKind::Mag)),
            thing(2.0, Weapon(WeaponKind::L42)),
            ammo(2.5, NatoBall, 20, 20),
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

/// How rare a kind of thing is: the commonest tier it turns up in (common for what is not in any table).
pub fn rarity_of(kind: ItemKind) -> Rarity {
    Rarity::ALL.into_iter().find(|&r| table(r).iter().any(|e| e.kind == kind)).unwrap_or(Rarity::Common)
}

/// One thing that fell out of a box, and how rare it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dropped {
    pub rarity: Rarity,
    pub found: Found,
}

impl Rarity {
    /// The rarity of one of the other things in a box of this rarity, given a roll in 0 to 1: mostly a step or
    /// two down, so that a good box still has good things in it, and never better than the box.
    pub fn below(self, u: f32) -> Rarity {
        // How likely it is to be this many steps down.
        const STEPS: [f32; 5] = [0.15, 0.40, 0.25, 0.12, 0.08];
        let reach = self as usize + 1;
        let total: f32 = STEPS[..reach].iter().sum();
        let mut left = u.clamp(0.0, 0.999_999) * total;
        for (down, w) in STEPS[..reach].iter().enumerate() {
            if left < *w {
                return Rarity::ALL[self as usize - down];
            }
            left -= w;
        }
        Rarity::Common
    }
}

/// What falls out of a box of `rarity` and `size`: as many things as the size gives, the first of the box's
/// own rarity and the rest a step or two below it (see `Rarity::below`). Where there is a gun among them,
/// the rest lean toward rounds for it (see `lean_toward_ammo`).
pub fn roll_contents(rarity: Rarity, size: BoxSize, mut next: impl FnMut() -> f32) -> Vec<Dropped> {
    let (least, most) = size.handful();
    let count = least + (next() * (most - least + 1) as f32) as u32;
    let mut contents: Vec<Dropped> = (0..count.min(most))
        .map(|i| {
            let this = if i == 0 { rarity } else { rarity.below(next()) };
            Dropped { rarity: this, found: roll_item(this, &mut next) }
        })
        .collect();
    lean_toward_ammo(&mut contents, &mut next);
    contents
}

/// Where there is a gun among `contents`, makes the rest lean toward rounds for it: each other thing is that,
/// more often than not, and at least one is.
pub fn lean_toward_ammo(contents: &mut [Dropped], next: &mut impl FnMut() -> f32) {
    let guns: Vec<WeaponKind> = contents.iter().filter_map(|d| if let ItemKind::Weapon(w) = d.found.kind { Some(w) } else { None }).collect();
    let others: Vec<usize> = (0..contents.len()).filter(|&i| !matches!(contents[i].found.kind, ItemKind::Weapon(_))).collect();
    if guns.is_empty() || others.is_empty() {
        return;
    }
    let mut swapped = false;
    for (n, &i) in others.iter().enumerate() {
        // The last of them is the one that makes sure, if none of the others were.
        if next() < AMMO_FOR_THE_GUN || (n + 1 == others.len() && !swapped) {
            let gun = guns[(next() * guns.len() as f32) as usize % guns.len()];
            let rarity = contents[i].rarity;
            contents[i].found = rounds_for(gun, rarity, &mut *next);
            swapped = true;
        }
    }
}

/// How likely each other thing in a box with a gun in it is to be rounds for the gun.
const AMMO_FOR_THE_GUN: f32 = 0.6;

/// Some rounds of the load `gun` is found with, as many as the tables give in a box of `rarity` (or, for a
/// load they do not list there, up to what a square holds).
fn rounds_for(gun: WeaponKind, rarity: Rarity, mut next: impl FnMut() -> f32) -> Found {
    let load = gun.def().default_ammo;
    let listed = Rarity::ALL.into_iter().filter(|&r| r <= rarity).rev().chain(Rarity::ALL).find_map(|r| table(r).into_iter().find(|e| e.kind == ItemKind::Ammo(load)).map(|e| e.rounds));
    let (least, most) = listed.unwrap_or((1, load.def().per_stack));
    let count = least + (next() * (most - least + 1) as f32) as u32;
    Found { kind: ItemKind::Ammo(load), count: count.clamp(1, load.def().per_stack), loaded: 0 }
}

/// The most that can fall out of one box.
pub const MAX_DROPS: usize = 6;

/// Where things of these `radii` (how far each reaches from its middle, metres) lie in `area`, a rectangle
/// on a floor or a table top: clear of one another where there is room, the biggest nearest the middle, and
/// the same every time. Anything bigger than the area is centred on it.
pub fn lay_out(area: Rect, radii: &[f32]) -> Vec<Vec2> {
    const GRID: f32 = 0.06;
    const GAP: f32 = 0.02;
    let centre = area.centre();
    let mut order: Vec<usize> = (0..radii.len()).collect();
    order.sort_by(|&a, &b| radii[b].total_cmp(&radii[a]).then(a.cmp(&b)));
    let mut placed: Vec<(Vec2, f32)> = Vec::new();
    let mut out = vec![centre; radii.len()];
    for i in order {
        let r = radii[i];
        // Where its middle may be: the area less its own reach, or the middle if it will not fit.
        let range = |lo: f32, hi: f32, mid: f32| if hi - lo >= 2.0 * r { (lo + r, hi - r) } else { (mid, mid) };
        let (x0, x1) = range(area.x0, area.x1, centre.x);
        let (z0, z1) = range(area.z0, area.z1, centre.y);
        let (nx, nz) = (((x1 - x0) / GRID) as usize + 1, ((z1 - z0) / GRID) as usize + 1);
        let mut best: Option<(f32, f32, Vec2)> = None;
        for ix in 0..nx {
            for iz in 0..nz {
                let p = Vec2::new(if nx == 1 { x0 } else { x0 + (x1 - x0) * ix as f32 / (nx - 1) as f32 }, if nz == 1 { z0 } else { z0 + (z1 - z0) * iz as f32 / (nz - 1) as f32 });
                let clearance = placed.iter().map(|&(q, qr)| p.distance(q) - r - qr - GAP).fold(f32::MAX, f32::min);
                let from_middle = p.distance(centre);
                // Clear places beat crowded ones; among clear places the nearest the middle; among
                // crowded ones the least crowded.
                let key = if clearance >= 0.0 { from_middle } else { 1000.0 - clearance };
                if best.is_none_or(|(k, ..)| key < k) {
                    best = Some((key, clearance, p));
                }
            }
        }
        let p = best.map_or(centre, |b| b.2);
        out[i] = p;
        placed.push((p, r));
    }
    out
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

/// How many groups of things have been put down loose in the world (dropped by a zombie, or thrown out of an
/// inventory): each group has the number that says where it lies, `(LOOSE, group)`.
#[derive(Resource, Default)]
pub struct Drops(pub usize);

/// What the first of the pair that says where a loose thing lies is.
pub const LOOSE: usize = usize::MAX - 2;

/// A thing lying on the ground to be picked up.
#[derive(Component)]
pub struct Pickup {
    pub rarity: Rarity,
    pub found: Found,
    /// The box it came from, and which of its things it is, so that it stays gone once taken.
    pub spot: (usize, usize),
    pub slot: usize,
}

/// A thing lying where a box was opened: what it is, where (the point on the surface under it, in the
/// frame of the building) and which way it points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lying {
    pub drop: Dropped,
    pub at: Vec3,
    pub yaw: f32,
}

/// Boxes that have been opened, and what of their contents is still lying there (`None` once taken).
#[derive(Resource, Default)]
pub struct OpenedLoot(pub HashMap<(usize, usize), Vec<Option<Lying>>>);

/// A closed box of loot lying where it was put: open it to see what is in it.
#[derive(Component)]
pub struct LootBox {
    pub rarity: Rarity,
    pub size: BoxSize,
    pub spot: (usize, usize),
    /// The clear space round it for what falls out, in the frame of the building.
    pub room: Rect,
}

/// Turns slowly, so as to catch the eye.
#[derive(Component)]
pub struct Spin;

/// How high a thing's middle is lifted above the floor, so that it is easy to look at: its parts hang
/// from a point this far above where it lies.
pub const LIFT: f32 = 0.12;

/// How high a thing that has fallen out of a box hangs above the surface, so that it is easy to see: its
/// parts turn about a point this high, and the flat tag of its colour stays on the surface under it.
pub const HOVER: f32 = 0.32;

/// How far below that point the underside of the thing is.
const HOVER_GAP: f32 = 0.1;

/// The flat tag of its rarity's colour, and a glowing material for each rarity; the looks of every kind of
/// thing (guns from their models, the rest from `item_models`); and the military crates that loot comes in.
#[derive(Resource)]
pub struct LootAssets {
    pub mesh: Handle<Mesh>,
    pub materials: Vec<Handle<StandardMaterial>>,
    pub looks: HashMap<ItemKind, ModelLook>,
    /// The parts of the crate of each size and rarity, by size and then rarity: body, then its markings in
    /// the rarity's colour.
    pub crates: Vec<Vec<Vec<(Handle<Mesh>, Handle<StandardMaterial>)>>>,
}

/// A thing's model for lying about: its parts, and where the middle of its footprint is.
pub struct ModelLook {
    pub parts: Vec<(Handle<Mesh>, Handle<StandardMaterial>)>,
    /// The middle of its footprint, in model space (x, z), and the lowest point.
    pub middle: Vec2,
    pub bottom: f32,
    /// How far it reaches from that middle.
    pub radius: f32,
    /// Whether it is longer along z than along x.
    pub long_in_z: bool,
}

impl LootAssets {
    pub fn material(&self, rarity: Rarity) -> Handle<StandardMaterial> {
        self.materials[rarity as usize].clone()
    }

    /// How far a thing reaches from its middle as it lies.
    pub fn radius(&self, drop: &Dropped) -> f32 {
        self.looks[&drop.found.kind].radius
    }

    /// Which way a thing lies, in an area: a long thing along the long way of it.
    pub fn yaw(&self, drop: &Dropped, area: &Rect) -> f32 {
        let along_z = area.depth() >= area.width();
        if along_z == self.looks[&drop.found.kind].long_in_z {
            0.0
        } else {
            std::f32::consts::FRAC_PI_2
        }
    }
}

/// Where the things in `contents` lie when a box is opened onto `room`, with its floor at `y`.
pub fn lay_contents(assets: &LootAssets, room: &Rect, y: f32, contents: &[Dropped]) -> Vec<Lying> {
    let radii: Vec<f32> = contents.iter().map(|d| assets.radius(d)).collect();
    lay_out(*room, &radii)
        .into_iter()
        .zip(contents)
        .map(|(p, d)| Lying { drop: *d, at: Vec3::new(p.x, y, p.y), yaw: assets.yaw(d, room) })
        .collect()
}

/// Puts a thing on the ground as its model, with a flat tag of its rarity's colour under it. `parent` is
/// what it lies in.
pub fn spawn_drop(commands: &mut Commands, assets: &LootAssets, parent: Option<Entity>, lying: &Lying, spot: (usize, usize), slot: usize) {
    let drop = lying.drop;
    let look = &assets.looks[&drop.found.kind];
    let root = (
        Pickup { rarity: drop.rarity, found: drop.found, spot, slot },
        Spin,
        Transform::from_translation(lying.at + Vec3::Y * HOVER).with_rotation(Quat::from_rotation_y(lying.yaw)),
        Visibility::default(),
    );
    let id = commands.spawn(root).id();
    for (mesh, material) in &look.parts {
        let at = Vec3::new(-look.middle.x, -HOVER_GAP - look.bottom, -look.middle.y);
        commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), Transform::from_translation(at), ChildOf(id)));
    }
    let tag = (look.radius * 1.1).max(0.15);
    commands.spawn((
        Mesh3d(assets.mesh.clone()),
        MeshMaterial3d(assets.material(drop.rarity)),
        Transform::from_translation(Vec3::new(0.0, 0.012 - HOVER, 0.0)).with_scale(Vec3::new(tag, 0.01, tag)),
        ChildOf(id),
    ));
    if let Some(parent) = parent {
        commands.entity(id).insert(ChildOf(parent));
    }
}

/// Puts a closed crate of loot down with its floor at `at`.
pub fn spawn_crate(commands: &mut Commands, assets: &LootAssets, parent: Option<Entity>, at: Vec3, rarity: Rarity, size: BoxSize, spot: (usize, usize), room: Rect) {
    let id = commands.spawn((LootBox { rarity, size, spot, room }, Spin, Transform::from_translation(at + Vec3::Y * LIFT), Visibility::default())).id();
    for (mesh, material) in &assets.crates[size as usize][rarity as usize] {
        commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), Transform::from_translation(Vec3::new(0.0, -LIFT + 0.01, 0.0)), ChildOf(id)));
    }
    if let Some(parent) = parent {
        commands.entity(id).insert(ChildOf(parent));
    }
}

/// How long the crate of this size is.
pub fn crate_length(size: BoxSize) -> f32 {
    size.length()
}

pub struct LootPlugin;

impl Plugin for LootPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OpenedLoot>().init_gizmo_group::<CrateGlow>().add_systems(Startup, (make_assets, set_up_glow)).add_systems(Update, (spin, outline_crates));
    }
}

/// The outlines of the crates. They are hidden by what is in front of them, as the crates are: a crate
/// behind a wall is not to be seen through it.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct CrateGlow;

/// How near, metres, a crate has to be to be outlined.
const GLOW_RANGE: f32 = 20.0;

fn set_up_glow(mut store: ResMut<GizmoConfigStore>) {
    let (config, _) = store.config_mut::<CrateGlow>();
    // Only enough of a nudge forward that the lines do not flicker against the faces of the crate.
    config.depth_bias = -0.002;
    config.line.width = 2.5;
}

/// The twelve edges of a box with its floor at `base` (a crate's local frame), as pairs of points.
fn box_edges(half_x: f32, half_z: f32, height: f32) -> [(Vec3, Vec3); 12] {
    let c = |x: f32, y: f32, z: f32| Vec3::new(x * half_x, y * height, z * half_z);
    let mut edges = [(Vec3::ZERO, Vec3::ZERO); 12];
    let mut n = 0;
    for y in [0.0, 1.0] {
        for (a, b) in [((-1.0, -1.0), (1.0, -1.0)), ((1.0, -1.0), (1.0, 1.0)), ((1.0, 1.0), (-1.0, 1.0)), ((-1.0, 1.0), (-1.0, -1.0))] {
            edges[n] = (c(a.0, y, a.1), c(b.0, y, b.1));
            n += 1;
        }
    }
    for (x, z) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        edges[n] = (c(x, 0.0, z), c(x, 1.0, z));
        n += 1;
    }
    edges
}

/// Draws each near crate's outline in its rarity's colour, and a short beam up from it.
fn outline_crates(mut gizmos: Gizmos<CrateGlow>, time: Res<Time>, camera: Query<&GlobalTransform, With<Camera3d>>, crates: Query<(&GlobalTransform, &LootBox)>) {
    let Ok(eye) = camera.single() else { return };
    // A slow pulse, so that it reads as a light and not a part of the scene.
    let pulse = 0.75 + 0.25 * (time.elapsed_secs() * 3.0).sin();
    for (at, lootbox) in &crates {
        let origin = at.translation() - Vec3::Y * (LIFT - 0.01);
        if origin.distance(eye.translation()) > GLOW_RANGE {
            continue;
        }
        let length = crate_length(lootbox.size);
        let colour = lootbox.rarity.colour().with_alpha(pulse);
        let (_, turn, _) = at.to_scale_rotation_translation();
        for (a, b) in box_edges(length * 0.54, length * 0.34, length * 0.6) {
            gizmos.line(origin + turn * a, origin + turn * b, colour);
        }
        let top = origin + Vec3::Y * length * 0.6;
        gizmos.line(top, top + Vec3::Y * 0.9, colour);
    }
}

fn make_assets(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    // Lit by nothing, so that they show up in a dark cellar as well as in the sun.
    let rarity_materials: Vec<Handle<StandardMaterial>> = Rarity::ALL.iter().map(|r| materials.add(StandardMaterial { base_color: r.colour(), unlit: true, ..default() })).collect();
    let finishes: HashMap<Finish, Handle<StandardMaterial>> = Finish::ALL.into_iter().map(|f| (f, materials.add(crate::weapon::finish_material(f)))).collect();
    let paints: HashMap<Paint, Handle<StandardMaterial>> = Paint::ALL.into_iter().map(|p| (p, materials.add(p.material()))).collect();
    // How a model lies: its footprint, from the bounds of all its parts.
    let look = |parts: Vec<(Handle<StandardMaterial>, crate::gun_model::Parts)>, meshes: &mut Assets<Mesh>| {
        let (lo, hi) = parts.iter().map(|(_, p)| p.bounds()).fold((Vec3::MAX, Vec3::MIN), |(lo, hi), (a, b)| (lo.min(a), hi.max(b)));
        let (width, length) = (hi.x - lo.x, hi.z - lo.z);
        ModelLook {
            middle: Vec2::new((lo.x + hi.x) * 0.5, (lo.z + hi.z) * 0.5),
            bottom: lo.y,
            radius: 0.5 * width.hypot(length),
            long_in_z: length >= width,
            parts: parts.into_iter().map(|(m, p)| (meshes.add(p.into_mesh()), m)).collect(),
        }
    };
    let mut looks = HashMap::new();
    for &kind in WeaponKind::ALL.iter() {
        let model = gun_models::build(kind);
        looks.insert(ItemKind::Weapon(kind), look(model.parts.into_iter().map(|(f, p)| (finishes[&f].clone(), p)).collect(), &mut meshes));
    }
    for kind in item_models::kinds() {
        if let Some(model) = item_models::build(kind) {
            looks.insert(kind, look(model.parts.into_iter().map(|(p, parts)| (paints[&p].clone(), parts)).collect(), &mut meshes));
        }
    }
    // The crates: olive, with the markings in the colour of the rarity.
    let crates = BoxSize::ALL
        .iter()
        .map(|&size| {
            let (model, marks) = item_models::crate_model(crate_length(size));
            let body: Vec<(Handle<Mesh>, Handle<StandardMaterial>)> = model.parts.into_iter().map(|(p, parts)| (meshes.add(parts.into_mesh()), paints[&p].clone())).collect();
            let marks = meshes.add(marks.into_mesh());
            Rarity::ALL
                .iter()
                .map(|&rarity| {
                    let mut parts = body.clone();
                    parts.push((marks.clone(), rarity_materials[rarity as usize].clone()));
                    parts
                })
                .collect()
        })
        .collect();
    commands.insert_resource(LootAssets { mesh, materials: rarity_materials, looks, crates });
}

/// Boxes turn slowly, so that they catch the eye.
fn spin(time: Res<Time>, mut boxes: Query<&mut Transform, With<Spin>>) {
    for mut t in &mut boxes {
        t.rotate_y(time.delta_secs() * 0.9);
    }
}

#[cfg(test)]
impl LootAssets {
    /// Looks for every kind of thing (with no parts, for nothing to be drawn), so that things can be put down in a
    /// test that has no renderer.
    pub(crate) fn for_tests() -> LootAssets {
        let mut kinds: Vec<ItemKind> = WeaponKind::ALL.iter().map(|&w| ItemKind::Weapon(w)).collect();
        kinds.extend(item_models::kinds());
        let looks = kinds.into_iter().map(|kind| (kind, ModelLook { parts: Vec::new(), middle: Vec2::ZERO, bottom: 0.0, radius: 0.1, long_in_z: true })).collect();
        LootAssets { mesh: Handle::default(), materials: vec![Handle::default(); 5], looks, crates: Vec::new() }
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
        assert_eq!(roll_contents(Rarity::Rare, BoxSize::L, spot_numbers(12, 3)), roll_contents(Rarity::Rare, BoxSize::L, spot_numbers(12, 3)));
        let a = roll_item(Rarity::Rare, spot_numbers(12, 3));
        let b = roll_item(Rarity::Rare, spot_numbers(12, 3));
        assert_eq!(a, b);
        let many: std::collections::HashSet<String> = (0..40).map(|i| format!("{:?}", roll_item(Rarity::Rare, spot_numbers(i, 0)).kind)).collect();
        assert!(many.len() >= 3, "there is a choice: {many:?}");
    }

    #[test]
    fn the_rarer_the_spot_the_stronger_the_guns_in_it() {
        let found = |rarity: Rarity| -> std::collections::HashSet<WeaponKind> {
            (0..600)
                .filter_map(|i| match roll_item(rarity, spot_numbers(i, 1)).kind {
                    ItemKind::Weapon(w) => Some(w),
                    _ => None,
                })
                .collect()
        };
        // An old revolver turns up in the commonest boxes; the strong guns only in the rarest.
        assert_eq!(found(Rarity::Common), [WeaponKind::Revolver].into());
        assert!(found(Rarity::Uncommon).contains(&WeaponKind::LeeEnfield) && found(Rarity::Uncommon).contains(&WeaponKind::DoubleBarrel));
        // The self-loading shotgun and the Hi-Power are rarer than the guns they were once found beside.
        assert!(!found(Rarity::Uncommon).contains(&WeaponKind::Auto5) && found(Rarity::Rare).contains(&WeaponKind::Auto5) && !found(Rarity::VeryRare).contains(&WeaponKind::Auto5));
        assert!(!found(Rarity::Common).contains(&WeaponKind::HiPower) && found(Rarity::Uncommon).contains(&WeaponKind::HiPower));
        let strong = [WeaponKind::Slr, WeaponKind::Bren, WeaponKind::Mag, WeaponKind::L42];
        for rarity in [Rarity::Common, Rarity::Uncommon, Rarity::Rare] {
            assert!(strong.iter().all(|w| !found(rarity).contains(w)), "{rarity:?}: {:?}", found(rarity));
        }
        // The machine gun and the sniper rifle are only found at the top.
        for rarity in [Rarity::Common, Rarity::Uncommon, Rarity::Rare, Rarity::VeryRare] {
            assert!(!found(rarity).contains(&WeaponKind::Mag) && !found(rarity).contains(&WeaponKind::L42), "{rarity:?}");
        }
        assert!(found(Rarity::Rare).contains(&WeaponKind::Sterling));
        assert!(found(Rarity::VeryRare).contains(&WeaponKind::Slr) && found(Rarity::VeryRare).contains(&WeaponKind::Bren));
        assert!(found(Rarity::Legendary).contains(&WeaponKind::Mag) && found(Rarity::Legendary).contains(&WeaponKind::L42));
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
    fn a_box_gives_as_many_things_as_its_size_and_the_first_is_the_boxs_own_rarity_and_none_is_better() {
        for rarity in Rarity::ALL {
            for size in BoxSize::ALL {
                let (least, most) = size.handful();
                for i in 0..200 {
                    let contents = roll_contents(rarity, size, spot_numbers(i, 7));
                    assert!((least as usize..=most as usize).contains(&contents.len()), "{size:?}: {}", contents.len());
                    assert_eq!(contents[0].rarity, rarity, "the first is the box's own");
                    assert!(contents.iter().all(|d| d.rarity <= rarity));
                }
            }
        }
        assert!(BoxSize::ALL.windows(2).all(|w| w[0].handful().1 <= w[1].handful().1 && w[0].handful().0 <= w[1].handful().0 && w[0].length() < w[1].length()));
    }

    #[test]
    fn the_rest_of_a_good_box_is_still_good_and_a_common_box_has_nothing_better_than_common() {
        let mut rest = Vec::new();
        for i in 0..300 {
            rest.extend(roll_contents(Rarity::Legendary, BoxSize::XXL, spot_numbers(i, 9)).into_iter().skip(1).map(|d| d.rarity));
        }
        let good = rest.iter().filter(|&&r| r >= Rarity::Rare).count() as f32 / rest.len() as f32;
        assert!(good > 0.7, "{good} of a legendary box's other things are rare or better");
        assert!(rest.contains(&Rarity::Legendary) && rest.contains(&Rarity::Common), "from the top to the bottom");
        for i in 0..100 {
            assert!(roll_contents(Rarity::Common, BoxSize::XXL, spot_numbers(i, 9)).iter().all(|d| d.rarity == Rarity::Common));
        }
    }

    #[test]
    fn the_bigger_the_box_the_scarcer() {
        let mut counts = [0usize; 5];
        for i in 0..10_000 {
            counts[BoxSize::roll(i as f32 / 10_000.0) as usize] += 1;
        }
        assert!(counts.windows(2).all(|w| w[0] > w[1]), "{counts:?}");
        assert!(counts[4] > 100, "there are some of the biggest: {counts:?}");
    }

    #[test]
    fn a_thrown_away_thing_is_tagged_with_the_lowest_tier_it_turns_up_in() {
        assert_eq!(rarity_of(ItemKind::Weapon(WeaponKind::Revolver)), Rarity::Common);
        assert_eq!(rarity_of(ItemKind::Weapon(WeaponKind::HiPower)), Rarity::Uncommon);
        assert_eq!(rarity_of(ItemKind::Weapon(WeaponKind::Auto5)), Rarity::Rare);
        assert_eq!(rarity_of(ItemKind::Weapon(WeaponKind::Mag)), Rarity::Legendary);
        assert_eq!(rarity_of(ItemKind::Pack(PackKind::Bergen)), Rarity::VeryRare);
        assert_eq!(rarity_of(ItemKind::Supply(Supply::Bandage)), Rarity::Common);
    }

    #[test]
    fn every_gun_is_in_exactly_one_table_so_it_is_one_colour_wherever_it_turns_up() {
        for gun in WeaponKind::ALL {
            let tables = Rarity::ALL.into_iter().filter(|&r| table(r).iter().any(|e| e.kind == ItemKind::Weapon(gun))).count();
            assert_eq!(tables, 1, "{gun:?} is in {tables} tables");
        }
    }

    #[test]
    fn a_handful_never_exceeds_what_a_box_can_lay_out() {
        assert!(BoxSize::ALL.iter().all(|s| s.handful().1 as usize <= MAX_DROPS));
    }

    #[test]
    fn things_lie_apart_inside_the_area_when_it_has_room() {
        let area = Rect::new(0.0, 0.0, 1.6, 1.6);
        let radii = [0.1, 0.1, 0.15, 0.1, 0.12, 0.1];
        let spots = lay_out(area, &radii);
        for (i, (a, ra)) in spots.iter().zip(radii).enumerate() {
            assert!(a.x - ra >= area.x0 && a.x + ra <= area.x1 && a.y - ra >= area.z0 && a.y + ra <= area.z1, "{a} reaches out");
            for (b, rb) in spots[i + 1..].iter().zip(&radii[i + 1..]) {
                assert!(a.distance(*b) >= ra + rb, "{a} and {b} overlap");
            }
        }
        assert_eq!(spots, lay_out(area, &radii));
    }

    #[test]
    fn a_thing_too_big_for_the_area_is_centred_on_it() {
        let area = Rect::new(0.0, 0.0, 0.5, 0.5);
        assert_eq!(lay_out(area, &[0.55]), vec![Vec2::new(0.25, 0.25)]);
    }

    #[test]
    fn several_things_on_a_small_top_still_sit_inside_it_as_far_as_they_can() {
        let area = Rect::new(0.0, 0.0, 0.6, 0.45);
        for p in lay_out(area, &[0.1, 0.1, 0.1, 0.1, 0.1]) {
            assert!(p.x >= 0.1 - 1e-4 && p.x <= 0.5 + 1e-4 && p.y >= 0.1 - 1e-4 && p.y <= 0.35 + 1e-4, "{p}");
        }
    }

    #[test]
    fn a_box_with_a_gun_in_it_leans_toward_rounds_for_that_gun() {
        let mut with_gun = 0;
        let mut with_its_rounds = 0;
        let mut others = 0;
        let mut its_rounds = 0;
        for rarity in [Rarity::Common, Rarity::Uncommon, Rarity::Rare, Rarity::VeryRare, Rarity::Legendary] {
            for i in 0..400 {
                let contents = roll_contents(rarity, BoxSize::L, spot_numbers(i, 21));
                let Some(gun) = contents.iter().find_map(|d| if let ItemKind::Weapon(w) = d.found.kind { Some(w) } else { None }) else { continue };
                let guns = contents.iter().filter(|d| matches!(d.found.kind, ItemKind::Weapon(_))).count();
                let rest: Vec<&Dropped> = contents.iter().filter(|d| !matches!(d.found.kind, ItemKind::Weapon(_))).collect();
                if rest.is_empty() {
                    continue;
                }
                with_gun += 1;
                let load = ItemKind::Ammo(gun.def().default_ammo);
                let loads: Vec<ItemKind> = contents.iter().filter_map(|d| if let ItemKind::Weapon(w) = d.found.kind { Some(ItemKind::Ammo(w.def().default_ammo)) } else { None }).collect();
                with_its_rounds += contents.iter().any(|d| loads.contains(&d.found.kind)) as usize;
                if guns == 1 {
                    others += rest.len();
                    its_rounds += rest.iter().filter(|d| d.found.kind == load).count();
                    for d in rest.iter().filter(|d| d.found.kind == load) {
                        let ItemKind::Ammo(a) = d.found.kind else { unreachable!() };
                        assert!(d.found.count >= 1 && d.found.count <= a.def().per_stack);
                    }
                }
            }
        }
        assert!(with_gun > 100, "{with_gun} boxes had a gun");
        assert_eq!(with_its_rounds, with_gun, "every box with a gun in it has rounds for one of its guns");
        assert!(its_rounds * 10 >= others * 5, "{its_rounds} of {others} others are its rounds");
    }

    #[test]
    fn a_box_without_a_gun_is_as_it_was() {
        // Rounds are not made up for a gun that is not there: a box with none has whatever it rolled.
        let mut guns_seen = 0;
        for i in 0..300 {
            let contents = roll_contents(Rarity::Common, BoxSize::L, spot_numbers(i, 33));
            if contents.iter().any(|d| matches!(d.found.kind, ItemKind::Weapon(_))) {
                guns_seen += 1;
                continue;
            }
            assert!(contents.iter().all(|d| d.rarity <= Rarity::Common));
        }
        assert!(guns_seen > 0 && guns_seen < 300);
    }

    #[test]
    fn a_crates_outline_is_a_box_of_twelve_edges_standing_on_its_floor() {
        let edges = box_edges(0.5, 0.3, 0.6);
        assert_eq!(edges.len(), 12);
        for (a, b) in edges {
            assert!(a.y >= 0.0 && b.y <= 0.6 + 1e-6, "{a} {b}");
            assert!(a.x.abs() <= 0.5 + 1e-6 && a.z.abs() <= 0.3 + 1e-6);
            // Every edge runs along one axis only.
            let d = (b - a).abs();
            assert_eq!([d.x, d.y, d.z].iter().filter(|&&v| v > 1e-6).count(), 1, "{a} {b}");
        }
        let mut ends: Vec<_> = edges.iter().flat_map(|(a, b)| [*a, *b]).map(|p| (p.x.signum() as i32, (p.y > 0.3) as i32, p.z.signum() as i32)).collect();
        ends.sort();
        ends.dedup();
        assert_eq!(ends.len(), 8, "eight corners");
    }

    #[test]
    fn loot_is_scarce_and_every_calibre_has_a_fair_share_of_rounds_for_its_guns() {
        for richness in [0.0, 0.6, 1.8] {
            let (mut things, mut guns, mut stacks) = (0usize, 0usize, 0usize);
            let mut rounds: HashMap<AmmoKind, u32> = HashMap::new();
            let mut loads: HashMap<AmmoKind, u32> = HashMap::new();
            let boxes = 6000;
            for i in 0..boxes {
                let mut next = spot_numbers(i, 3);
                let rarity = Rarity::roll(next(), richness);
                let size = BoxSize::roll(next());
                for d in roll_contents(rarity, size, &mut next) {
                    things += 1;
                    match d.found.kind {
                        ItemKind::Weapon(w) => {
                            guns += 1;
                            *loads.entry(w.def().default_ammo).or_default() += 1;
                        }
                        ItemKind::Ammo(a) => {
                            stacks += 1;
                            *rounds.entry(a).or_default() += d.found.count;
                        }
                        _ => {}
                    }
                }
            }
            let per_box = |n: usize| n as f32 / boxes as f32;
            assert!(per_box(things) < 3.0, "{richness}: {} things a box", per_box(things));
            assert!(per_box(guns) < 1.0 && per_box(stacks) > per_box(guns), "{richness}: guns {} and rounds {} a box", per_box(guns), per_box(stacks));
            for (load, guns) in loads {
                let each = rounds.get(&load).copied().unwrap_or(0) as f32 / guns as f32;
                assert!((10.0..=100.0).contains(&each), "{richness}: {each} {load:?} rounds for each gun that takes them");
            }
        }
    }
}

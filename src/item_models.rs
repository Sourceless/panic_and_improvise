//! The looks of everything that is not a gun: ammunition cartons and cans, dressings, a first aid
//! tin, a water bottle, a torch, a compass, a tin of food and the three packs. Each is built of
//! boxes and tubes, to the size of the real thing, standing on its own base with y up.

use std::f32::consts::FRAC_PI_2;

use bevy::prelude::*;

use crate::ammo::AmmoKind;
use crate::gun_model::Parts;
use crate::inventory::{ItemKind, PackKind, Supply};

/// What a part is made of.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Paint {
    Olive,
    Canvas,
    Khaki,
    Card,
    White,
    Red,
    Brass,
    Steel,
    Black,
    Yellow,
    Glass,
}

impl Paint {
    pub const ALL: [Paint; 11] = [Paint::Olive, Paint::Canvas, Paint::Khaki, Paint::Card, Paint::White, Paint::Red, Paint::Brass, Paint::Steel, Paint::Black, Paint::Yellow, Paint::Glass];

    pub fn material(self) -> StandardMaterial {
        let plain = |r: f32, g: f32, b: f32, roughness: f32, metallic: f32| StandardMaterial { base_color: Color::srgb(r, g, b), perceptual_roughness: roughness, metallic, ..default() };
        match self {
            Paint::Olive => plain(0.16, 0.19, 0.09, 0.6, 0.3),
            Paint::Canvas => plain(0.22, 0.25, 0.13, 0.95, 0.0),
            Paint::Khaki => plain(0.42, 0.37, 0.21, 0.95, 0.0),
            Paint::Card => plain(0.45, 0.32, 0.17, 0.9, 0.0),
            Paint::White => plain(0.78, 0.78, 0.74, 0.8, 0.0),
            Paint::Red => plain(0.55, 0.07, 0.05, 0.7, 0.0),
            Paint::Brass => plain(0.7, 0.5, 0.14, 0.35, 0.85),
            Paint::Steel => plain(0.5, 0.51, 0.53, 0.4, 0.8),
            Paint::Black => plain(0.03, 0.03, 0.035, 0.5, 0.0),
            Paint::Yellow => plain(0.8, 0.64, 0.08, 0.7, 0.0),
            Paint::Glass => StandardMaterial { base_color: Color::srgb(0.1, 0.14, 0.18), perceptual_roughness: 0.08, metallic: 0.3, reflectance: 0.9, ..default() },
        }
    }
}

/// One thing's parts, each in its paint.
pub struct ItemModel {
    pub parts: Vec<(Paint, Parts)>,
}

/// Collects boxes and tubes by paint.
#[derive(Default)]
struct Builder {
    parts: Vec<(Paint, Parts)>,
}

impl Builder {
    fn of(&mut self, paint: Paint) -> &mut Parts {
        let i = self.parts.iter().position(|(p, _)| *p == paint).unwrap_or_else(|| {
            self.parts.push((paint, Parts::default()));
            self.parts.len() - 1
        });
        &mut self.parts[i].1
    }

    /// A box sitting on y = `base`, centred over (`x`, `z`).
    fn block(&mut self, paint: Paint, x: f32, base: f32, z: f32, size: Vec3) -> &mut Self {
        self.of(paint).add_box(Vec3::new(x, base + size.y * 0.5, z), size, Quat::IDENTITY);
        self
    }

    /// A box turned about the vertical by `yaw`.
    fn turned(&mut self, paint: Paint, x: f32, base: f32, z: f32, size: Vec3, yaw: f32) -> &mut Self {
        self.of(paint).add_box(Vec3::new(x, base + size.y * 0.5, z), size, Quat::from_rotation_y(yaw));
        self
    }

    /// An upright cylinder standing on y = `base`.
    fn upright(&mut self, paint: Paint, x: f32, base: f32, z: f32, radius: f32, height: f32) -> &mut Self {
        self.of(paint).add_tube(Vec3::new(x, base + height * 0.5, z), radius, height, 16, Quat::from_rotation_x(FRAC_PI_2));
        self
    }

    /// A cylinder lying along the x axis, its underside on y = `base`.
    fn along_x(&mut self, paint: Paint, x: f32, base: f32, z: f32, radius: f32, length: f32) -> &mut Self {
        self.of(paint).add_tube(Vec3::new(x, base + radius, z), radius, length, 16, Quat::from_rotation_y(FRAC_PI_2));
        self
    }

    /// A cylinder lying along the z axis, its underside on y = `base`.
    fn along_z(&mut self, paint: Paint, x: f32, base: f32, z: f32, radius: f32, length: f32) -> &mut Self {
        self.of(paint).add_tube(Vec3::new(x, base + radius, z), radius, length, 16, Quat::IDENTITY);
        self
    }

    fn done(self) -> ItemModel {
        ItemModel { parts: self.parts.into_iter().filter(|(_, p)| !p.is_empty()).collect() }
    }
}

/// Every kind of item that is not a gun.
pub fn kinds() -> Vec<ItemKind> {
    let mut all: Vec<ItemKind> = crate::ammo::AMMO.iter().map(|a| ItemKind::Ammo(a.kind)).collect();
    all.extend(PackKind::ALL.map(ItemKind::Pack));
    all.extend(Supply::ALL.map(ItemKind::Supply));
    all
}

/// A military crate for loot: an olive box on skids, with steel straps, rope handles at the ends and a
/// latch, `length` metres long. What marks its rarity is given apart, to be drawn in the rarity's own colour:
/// a band across the lid and a panel on the front.
pub fn crate_model(length: f32) -> (ItemModel, Parts) {
    let mut b = Builder::default();
    // Built one metre long and made the size wanted after.
    b.block(Paint::Card, 0.0, 0.0, -0.2, Vec3::new(0.96, 0.035, 0.09));
    b.block(Paint::Card, 0.0, 0.0, 0.2, Vec3::new(0.96, 0.035, 0.09));
    b.block(Paint::Olive, 0.0, 0.035, 0.0, Vec3::new(1.0, 0.47, 0.6));
    b.block(Paint::Olive, 0.0, 0.505, 0.0, Vec3::new(1.03, 0.07, 0.63));
    for x in [-0.4, 0.4] {
        b.block(Paint::Steel, x, 0.035, 0.0, Vec3::new(0.045, 0.47, 0.612));
        b.block(Paint::Steel, x, 0.505, 0.0, Vec3::new(0.05, 0.072, 0.634));
    }
    // Rope handles on the ends, and the latch.
    for x in [-0.525, 0.525] {
        b.block(Paint::Black, x, 0.2, 0.0, Vec3::new(0.03, 0.05, 0.22));
    }
    b.block(Paint::Steel, 0.0, 0.43, 0.31, Vec3::new(0.08, 0.1, 0.02));
    b.block(Paint::Black, 0.0, 0.46, 0.322, Vec3::new(0.03, 0.03, 0.01));
    // Stencilled lettering, in black.
    b.block(Paint::Black, -0.22, 0.26, 0.301, Vec3::new(0.3, 0.035, 0.002));
    b.block(Paint::Black, -0.22, 0.2, 0.301, Vec3::new(0.2, 0.025, 0.002));
    let mut marks = Builder::default();
    marks.block(Paint::Red, 0.2, 0.17, 0.3, Vec3::new(0.2, 0.16, 0.004));
    marks.block(Paint::Red, 0.0, 0.575, 0.0, Vec3::new(0.7, 0.004, 0.16));
    let mut mark = marks.done().parts.into_iter().next().map(|(_, p)| p).unwrap_or_default();
    let model = b.done();
    mark = mark.scaled(length);
    (ItemModel { parts: model.parts.into_iter().map(|(p, parts)| (p, parts.scaled(length))).collect() }, mark)
}

/// The model of an item that is not a gun. Guns have theirs in `gun_models`.
pub fn build(kind: ItemKind) -> Option<ItemModel> {
    match kind {
        ItemKind::Weapon(_) => None,
        ItemKind::Ammo(a) => Some(ammunition(a)),
        ItemKind::Pack(p) => Some(pack(p)),
        ItemKind::Supply(s) => Some(supply(s)),
    }
}

fn ammunition(kind: AmmoKind) -> ItemModel {
    let mut b = Builder::default();
    match kind {
        // A cardboard carton of fifty, with its label.
        AmmoKind::NineFmj => {
            b.block(Paint::Card, 0.0, 0.0, 0.0, Vec3::new(0.1, 0.045, 0.065));
            b.block(Paint::Yellow, 0.0, 0.045, 0.0, Vec3::new(0.07, 0.002, 0.035));
            b.block(Paint::Black, 0.0, 0.047, 0.0, Vec3::new(0.05, 0.001, 0.012));
        }
        // An olive steel can, with a handle on the lid and its stencilling.
        AmmoKind::NatoBall => {
            b.block(Paint::Olive, 0.0, 0.0, 0.0, Vec3::new(0.18, 0.09, 0.075));
            b.block(Paint::Black, 0.0, 0.086, 0.0, Vec3::new(0.185, 0.004, 0.08));
            b.block(Paint::Steel, 0.0, 0.09, 0.0, Vec3::new(0.07, 0.012, 0.012));
            b.block(Paint::Yellow, 0.0, 0.05, 0.0378, Vec3::new(0.09, 0.02, 0.002));
        }
        // A carton with a red band.
        AmmoKind::BritishBall => {
            b.block(Paint::Card, 0.0, 0.0, 0.0, Vec3::new(0.12, 0.05, 0.08));
            b.block(Paint::Red, 0.0, 0.0, 0.0, Vec3::new(0.122, 0.052, 0.022));
            b.block(Paint::White, 0.0, 0.052, 0.0, Vec3::new(0.05, 0.001, 0.016));
        }
        // A red box of shells, with a brass strip across the top.
        AmmoKind::Buckshot => {
            b.block(Paint::Red, 0.0, 0.0, 0.0, Vec3::new(0.11, 0.055, 0.075));
            b.block(Paint::Brass, 0.0, 0.055, 0.0, Vec3::new(0.11, 0.003, 0.014));
            b.block(Paint::White, -0.03, 0.055, 0.0, Vec3::new(0.04, 0.001, 0.05));
        }
    }
    b.done()
}

fn supply(kind: Supply) -> ItemModel {
    let mut b = Builder::default();
    match kind {
        // A wrapped field dressing: an olive packet with a white pad showing.
        Supply::Bandage => {
            b.block(Paint::Olive, 0.0, 0.0, 0.0, Vec3::new(0.1, 0.03, 0.07));
            b.block(Paint::White, 0.0, 0.03, 0.0, Vec3::new(0.08, 0.004, 0.05));
            b.block(Paint::Red, 0.0, 0.034, 0.0, Vec3::new(0.012, 0.001, 0.04));
        }
        // A red case with a white cross on the lid and a black handle.
        Supply::FirstAid => {
            b.block(Paint::Red, 0.0, 0.0, 0.0, Vec3::new(0.26, 0.1, 0.18));
            b.block(Paint::White, 0.0, 0.1, 0.0, Vec3::new(0.1, 0.002, 0.03));
            b.block(Paint::White, 0.0, 0.1, 0.0, Vec3::new(0.03, 0.002, 0.1));
            b.block(Paint::Black, 0.0, 0.1, -0.075, Vec3::new(0.09, 0.012, 0.014));
            b.block(Paint::Steel, 0.0, 0.03, 0.091, Vec3::new(0.03, 0.025, 0.006));
        }
        // A water bottle in its canvas cover, with a black cup over the neck.
        Supply::Canteen => {
            b.block(Paint::Canvas, 0.0, 0.0, 0.0, Vec3::new(0.1, 0.19, 0.055));
            b.upright(Paint::Black, 0.0, 0.19, 0.0, 0.026, 0.045);
            b.block(Paint::Khaki, 0.0, 0.1, 0.0, Vec3::new(0.102, 0.02, 0.057));
        }
        // A tin with its paper label.
        Supply::TinnedFood => {
            b.upright(Paint::Steel, 0.0, 0.0, 0.0, 0.042, 0.1);
            b.upright(Paint::Red, 0.0, 0.015, 0.0, 0.0432, 0.07);
            b.upright(Paint::Yellow, 0.0, 0.04, 0.0, 0.0436, 0.02);
        }
        // A torch: black body, steel head and a lens at the front.
        Supply::Torch => {
            b.along_z(Paint::Black, 0.0, 0.0, 0.02, 0.0165, 0.16);
            b.along_z(Paint::Steel, 0.0, -0.008, -0.095, 0.0285, 0.05);
            b.along_z(Paint::Glass, 0.0, -0.002, -0.121, 0.0235, 0.004);
            b.block(Paint::Steel, 0.0, 0.032, 0.03, Vec3::new(0.01, 0.006, 0.02));
        }
        // A brass compass: case, glass lid and a red needle.
        Supply::Compass => {
            b.upright(Paint::Brass, 0.0, 0.0, 0.0, 0.04, 0.02);
            b.upright(Paint::Glass, 0.0, 0.02, 0.0, 0.035, 0.004);
            b.turned(Paint::Red, 0.0, 0.0225, 0.0, Vec3::new(0.004, 0.002, 0.05), 0.6);
            b.upright(Paint::Brass, 0.0, 0.0, 0.052, 0.008, 0.012);
        }
    }
    b.done()
}

fn pack(kind: PackKind) -> ItemModel {
    let mut b = Builder::default();
    match kind {
        // A canvas satchel: a body, a flap over it, a strap and a buckle.
        PackKind::Satchel => {
            b.block(Paint::Khaki, 0.0, 0.0, 0.0, Vec3::new(0.3, 0.22, 0.1));
            b.block(Paint::Khaki, 0.0, 0.1, 0.056, Vec3::new(0.31, 0.13, 0.012));
            b.block(Paint::Black, 0.0, 0.0, 0.0, Vec3::new(0.025, 0.221, 0.104));
            b.block(Paint::Steel, 0.0, 0.1, 0.064, Vec3::new(0.03, 0.04, 0.006));
            b.block(Paint::Black, 0.0, 0.22, 0.0, Vec3::new(0.28, 0.01, 0.014));
        }
        // A daypack: body, front pocket, lid and two straps.
        PackKind::Daypack => {
            b.block(Paint::Canvas, 0.0, 0.0, 0.0, Vec3::new(0.3, 0.44, 0.16));
            b.block(Paint::Canvas, 0.0, 0.02, 0.1, Vec3::new(0.24, 0.2, 0.05));
            b.block(Paint::Canvas, 0.0, 0.42, -0.005, Vec3::new(0.31, 0.06, 0.17));
            b.block(Paint::Black, 0.0, 0.0, -0.09, Vec3::new(0.2, 0.3, 0.02));
            b.block(Paint::Steel, 0.0, 0.2, 0.128, Vec3::new(0.03, 0.025, 0.006));
        }
        // A bergen: a big body on a frame, a lid, a pouch each side and a rolled mat on top.
        PackKind::Bergen => {
            b.block(Paint::Olive, 0.0, 0.04, 0.0, Vec3::new(0.36, 0.55, 0.24));
            b.block(Paint::Olive, 0.0, 0.58, -0.005, Vec3::new(0.37, 0.07, 0.25));
            b.block(Paint::Olive, -0.215, 0.06, 0.02, Vec3::new(0.07, 0.24, 0.17));
            b.block(Paint::Olive, 0.215, 0.06, 0.02, Vec3::new(0.07, 0.24, 0.17));
            b.block(Paint::Black, 0.0, 0.0, -0.13, Vec3::new(0.3, 0.62, 0.025));
            b.block(Paint::Steel, 0.0, 0.0, -0.13, Vec3::new(0.32, 0.04, 0.03));
            b.along_x(Paint::Khaki, 0.0, 0.65, 0.0, 0.055, 0.32);
            b.block(Paint::Steel, 0.0, 0.4, 0.122, Vec3::new(0.04, 0.03, 0.006));
        }
    }
    b.done()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_item_that_is_not_a_gun_has_a_model_of_a_believable_size() {
        for kind in kinds() {
            let model = build(kind).unwrap_or_else(|| panic!("{kind:?} has no model"));
            assert!(!model.parts.is_empty());
            let (lo, hi) = model.parts.iter().map(|(_, p)| p.bounds()).fold((Vec3::MAX, Vec3::MIN), |(lo, hi), (a, b)| (lo.min(a), hi.max(b)));
            let size = hi - lo;
            assert!(size.max_element() > 0.05 && size.max_element() < 0.8, "{kind:?} is {size}");
            assert!(lo.y > -0.05 && lo.y < 0.05, "{kind:?} sits at {}", lo.y);
        }
        assert!(build(ItemKind::Weapon(crate::weapons::WeaponKind::Slr)).is_none());
    }

    #[test]
    fn the_crate_is_a_crate_of_the_length_asked_for_with_its_markings_apart() {
        for length in [0.3, 0.45] {
            let (model, mark) = crate_model(length);
            let (lo, hi) = model.parts.iter().map(|(_, p)| p.bounds()).fold((Vec3::MAX, Vec3::MIN), |(lo, hi), (a, b)| (lo.min(a), hi.max(b)));
            assert!((hi.x - lo.x - length * 1.05).abs() < 0.01 * length * 10.0, "{}", hi.x - lo.x);
            assert!(lo.y.abs() < 1e-4 && hi.y < length * 0.6);
            assert!(!mark.is_empty());
        }
    }

    #[test]
    fn bigger_packs_look_bigger() {
        let height = |p: PackKind| {
            let m = build(ItemKind::Pack(p)).unwrap();
            m.parts.iter().map(|(_, parts)| parts.bounds().1.y).fold(0.0, f32::max)
        };
        assert!(height(PackKind::Satchel) < height(PackKind::Daypack) && height(PackKind::Daypack) < height(PackKind::Bergen));
    }
}

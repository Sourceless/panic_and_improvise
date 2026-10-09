//! The looks of every gun: each built from boxes, tubes and rings (see `gun_model` for the parts and
//! for the Sterling, which was measured off photographs; these are made to the proportions of
//! the real guns from their published dimensions).
//!
//! Gun space: forward is -Z, up is +Y, the bore runs along y = 0 (the Sterling's runs at its own
//! height, which its spec records), the muzzle is at the front. A model also says where its sights are,
//! which is what lines up on the player's eye when aiming.


use bevy::prelude::*;

use crate::gun_model::{self, Parts};
use crate::weapons::WeaponKind;

/// How a part is finished, which decides the material it is drawn with.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Finish {
    /// Blued or parkerised steel.
    Metal,
    /// Black plastic or rubber.
    Black,
    /// The dark of holes, ports and slots.
    Dark,
    /// A wooden stock.
    Wood,
    /// Olive drab paint (an ammunition box).
    Olive,
    /// Telescope glass.
    Glass,
}

impl Finish {
    pub const ALL: [Finish; 6] = [Finish::Metal, Finish::Black, Finish::Dark, Finish::Wood, Finish::Olive, Finish::Glass];
}

/// Where the muzzle and the sights are, in gun space.
#[derive(Clone, Copy, Debug)]
pub struct ModelSpec {
    pub muzzle: Vec3,
    /// The line of sight: its height and sideways position above the origin, and where along the gun
    /// the rear sight (or the eyepiece of a telescope) and the front sight are.
    pub sight_x: f32,
    pub sight_y: f32,
    pub rear_z: f32,
    pub front_z: f32,
    /// How far behind the rear sight the eye is when aiming.
    pub eye_relief: f32,
}

impl ModelSpec {
    /// The two ends of the line of sight in gun space.
    pub fn sight_points(&self) -> (Vec3, Vec3) {
        (Vec3::new(self.sight_x, self.sight_y, self.rear_z), Vec3::new(self.sight_x, self.sight_y, self.front_z))
    }

    /// Where the gun sits in the camera's space when aimed: the line of sight on the camera's axis,
    /// the rear sight the eye relief in front of the eye.
    pub fn aimed_position(&self) -> Vec3 {
        Vec3::new(-self.sight_x, -self.sight_y, -self.eye_relief - self.rear_z)
    }
}

pub struct GunModel {
    pub spec: ModelSpec,
    pub parts: Vec<(Finish, Parts)>,
}

const NONE: Quat = Quat::IDENTITY;

/// A rod from `a` to `b`.
fn rod(p: &mut Parts, a: Vec3, b: Vec3, radius: f32) {
    let d = b - a;
    p.add_tube((a + b) * 0.5, radius, d.length(), 12, Quat::from_rotation_arc(Vec3::Z, d.normalize()));
}

/// A tube along the bore from `z0` back to `z1`... (given front first, then rear), `radius` across,
/// with its axis `y` above the origin.
fn barrel(p: &mut Parts, y: f32, front: f32, rear: f32, radius: f32) {
    p.add_tube(Vec3::new(0.0, y, (front + rear) * 0.5), radius, (rear - front).abs(), 18, NONE);
}

fn slab(p: &mut Parts, c: Vec3, size: Vec3) {
    p.add_box(c, size, NONE);
}

/// A box turned about the X axis (nose up for positive angles).
fn tilted(p: &mut Parts, c: Vec3, size: Vec3, pitch: f32) {
    p.add_box(c, size, Quat::from_rotation_x(pitch));
}

fn finish_model(spec: ModelSpec, parts: Vec<(Finish, Parts)>) -> GunModel {
    GunModel { spec, parts: parts.into_iter().filter(|(_, p)| !p.is_empty()).collect() }
}

pub fn build(kind: WeaponKind) -> GunModel {
    match kind {
        WeaponKind::Sterling => sterling(),
        WeaponKind::HiPower => hi_power(),
        WeaponKind::Slr => slr(),
        WeaponKind::Bren => bren(),
        WeaponKind::Mag => mag(),
        WeaponKind::L42 => l42(),
        WeaponKind::Auto5 => auto5(),
        WeaponKind::LeeEnfield => lee_enfield(),
    }
}

fn sterling() -> GunModel {
    let (metal, plastic, dark) = gun_model::build();
    let (rear, front) = gun_model::sight_points();
    let spec = ModelSpec { muzzle: Vec3::new(0.0, gun_model::BORE_Y, gun_model::MUZZLE_Z), sight_x: 0.0, sight_y: rear.y, rear_z: rear.z, front_z: front.z, eye_relief: 0.2 };
    finish_model(spec, vec![(Finish::Metal, metal), (Finish::Black, plastic), (Finish::Dark, dark)])
}

/// The Browning Hi-Power: a slide 190 mm long on a frame with a raked grip, the hammer behind,
/// small fixed sights.
fn hi_power() -> GunModel {
    let (mut metal, mut black, mut dark) = (Parts::default(), Parts::default(), Parts::default());
    // The slide and the barrel's tip.
    slab(&mut metal, Vec3::new(0.0, 0.002, -0.108), Vec3::new(0.026, 0.034, 0.196));
    barrel(&mut metal, 0.0, -0.2, -0.216, 0.0058);
    // The frame under it, and the dust cover to the front.
    slab(&mut metal, Vec3::new(0.0, -0.026, -0.1), Vec3::new(0.024, 0.024, 0.17));
    // The grip, raked back, with its panels, and the magazine's base.
    tilted(&mut black, Vec3::new(0.0, -0.088, -0.012), Vec3::new(0.032, 0.115, 0.052), 0.26);
    tilted(&mut metal, Vec3::new(0.0, -0.145, 0.005), Vec3::new(0.028, 0.012, 0.05), 0.26);
    // The trigger guard and the trigger.
    slab(&mut metal, Vec3::new(0.0, -0.047, -0.078), Vec3::new(0.008, 0.005, 0.056));
    slab(&mut metal, Vec3::new(0.0, -0.037, -0.103), Vec3::new(0.008, 0.024, 0.006));
    slab(&mut dark, Vec3::new(0.0, -0.032, -0.062), Vec3::new(0.005, 0.02, 0.006));
    // The hammer, the ejection port, the slide's serrations.
    slab(&mut metal, Vec3::new(0.0, 0.012, 0.0), Vec3::new(0.008, 0.012, 0.012));
    slab(&mut dark, Vec3::new(0.0132, 0.012, -0.075), Vec3::new(0.0006, 0.012, 0.034));
    for k in 0..6 {
        slab(&mut dark, Vec3::new(0.0, 0.016, -0.012 - k as f32 * 0.0035), Vec3::new(0.027, 0.003, 0.0012));
    }
    // The sights: a notch at the back of the slide, a blade at the front.
    slab(&mut metal, Vec3::new(-0.0075, 0.0205, -0.016), Vec3::new(0.009, 0.007, 0.007));
    slab(&mut metal, Vec3::new(0.0075, 0.0205, -0.016), Vec3::new(0.009, 0.007, 0.007));
    slab(&mut metal, Vec3::new(0.0, 0.0205, -0.196), Vec3::new(0.004, 0.007, 0.006));
    let spec = ModelSpec { muzzle: Vec3::new(0.0, 0.0, -0.218), sight_x: 0.0, sight_y: 0.0225, rear_z: -0.016, front_z: -0.196, eye_relief: 0.42 };
    finish_model(spec, vec![(Finish::Metal, metal), (Finish::Black, black), (Finish::Dark, dark)])
}

/// The L1A1: a long, slim rifle with a wooden stock and hand guard, a gas piston above the barrel,
/// a prominent flash hider and a magazine of twenty rounds.
fn slr() -> GunModel {
    let (mut metal, mut black, mut dark, mut wood) = (Parts::default(), Parts::default(), Parts::default(), Parts::default());
    // The receiver, its cover and the aperture sight on the back of it.
    slab(&mut metal, Vec3::new(0.0, -0.002, -0.14), Vec3::new(0.04, 0.06, 0.26));
    slab(&mut metal, Vec3::new(0.0, 0.034, -0.15), Vec3::new(0.032, 0.012, 0.22));
    slab(&mut metal, Vec3::new(0.0, 0.045, -0.002), Vec3::new(0.016, 0.022, 0.012));
    metal.add_ring(Vec3::new(0.0, 0.052, 0.0), 0.011, 0.0035, 0.004, 14, NONE);
    slab(&mut dark, Vec3::new(0.0207, 0.008, -0.1), Vec3::new(0.001, 0.014, 0.07));
    // The barrel, the gas tube above it and the gas block.
    barrel(&mut metal, 0.0, -0.62, -0.27, 0.0095);
    rod(&mut metal, Vec3::new(0.0, 0.025, -0.34), Vec3::new(0.0, 0.025, -0.5), 0.0075);
    slab(&mut metal, Vec3::new(0.0, 0.012, -0.5), Vec3::new(0.026, 0.04, 0.03));
    // The flash hider: a slotted cylinder at the muzzle.
    barrel(&mut metal, 0.0, -0.66, -0.57, 0.0135);
    for k in 0..3 {
        slab(&mut dark, Vec3::new(0.0, 0.0, -0.585 - k as f32 * 0.025), Vec3::new(0.0275, 0.004, 0.012));
    }
    // The front sight: a blade in a hood on a post.
    slab(&mut metal, Vec3::new(0.0, 0.03, -0.555), Vec3::new(0.006, 0.05, 0.012));
    metal.add_ring(Vec3::new(0.0, 0.048, -0.56), 0.011, 0.0085, 0.01, 12, NONE);
    // The wooden hand guard and fore end.
    slab(&mut wood, Vec3::new(0.0, -0.003, -0.38), Vec3::new(0.044, 0.048, 0.22));
    slab(&mut wood, Vec3::new(0.0, 0.026, -0.38), Vec3::new(0.034, 0.012, 0.22));
    // The magazine, a little curved, and the pistol grip behind the trigger guard.
    tilted(&mut metal, Vec3::new(0.0, -0.085, -0.14), Vec3::new(0.034, 0.11, 0.06), -0.08);
    slab(&mut metal, Vec3::new(0.0, -0.045, -0.04), Vec3::new(0.008, 0.006, 0.07));
    tilted(&mut wood, Vec3::new(0.0, -0.07, 0.01), Vec3::new(0.03, 0.09, 0.04), 0.3);
    // The stock, with its heel, and the butt plate.
    slab(&mut wood, Vec3::new(0.0, -0.012, 0.2), Vec3::new(0.04, 0.075, 0.37));
    tilted(&mut wood, Vec3::new(0.0, -0.028, 0.37), Vec3::new(0.042, 0.1, 0.1), 0.0);
    slab(&mut black, Vec3::new(0.0, -0.03, 0.425), Vec3::new(0.046, 0.115, 0.012));
    slab(&mut metal, Vec3::new(0.0, -0.065, 0.02), Vec3::new(0.012, 0.02, 0.012));
    let spec = ModelSpec { muzzle: Vec3::new(0.0, 0.0, -0.665), sight_x: 0.0, sight_y: 0.052, rear_z: 0.0, front_z: -0.56, eye_relief: 0.11 };
    finish_model(spec, vec![(Finish::Metal, metal), (Finish::Black, black), (Finish::Dark, dark), (Finish::Wood, wood)])
}

/// The L4A4 Bren: a thirty round curved magazine on top, a carrying handle on the barrel, a gas
/// cylinder beneath it, the bipod folded under, wooden furniture, sights offset to the left to clear the magazine.
fn bren() -> GunModel {
    let (mut metal, mut black, mut dark, mut wood) = (Parts::default(), Parts::default(), Parts::default(), Parts::default());
    // The body: a deep receiver.
    slab(&mut metal, Vec3::new(0.0, -0.005, -0.2), Vec3::new(0.052, 0.088, 0.4));
    slab(&mut metal, Vec3::new(0.0, 0.0, 0.03), Vec3::new(0.05, 0.075, 0.07));
    // The magazine on top: an arc of boxes tilting forward as it climbs, the feed housing under it.
    slab(&mut metal, Vec3::new(0.0, 0.052, -0.2), Vec3::new(0.04, 0.02, 0.08));
    for (k, (y, z, tilt)) in [(0.085, -0.2, 0.0f32), (0.125, -0.206, -0.1), (0.162, -0.22, -0.22)].into_iter().enumerate() {
        tilted(&mut metal, Vec3::new(0.0, y, z), Vec3::new(0.034, 0.045, 0.082), tilt);
        let _ = k;
    }
    // The sights, to the left of the magazine: an aperture at the back and a blade in a hood at the front.
    slab(&mut metal, Vec3::new(-0.04, 0.047, 0.005), Vec3::new(0.012, 0.03, 0.012));
    metal.add_ring(Vec3::new(-0.04, 0.066, 0.005), 0.01, 0.0035, 0.004, 14, NONE);
    // The barrel with its flash hider, the carrying handle and the gas cylinder.
    barrel(&mut metal, 0.0, -0.76, -0.4, 0.0125);
    barrel(&mut metal, 0.0, -0.85, -0.75, 0.0185);
    for k in 0..4 {
        slab(&mut dark, Vec3::new(0.0, 0.0, -0.77 - k as f32 * 0.02), Vec3::new(0.0382, 0.004, 0.01));
    }
    barrel(&mut metal, -0.034, -0.64, -0.42, 0.0135);
    slab(&mut metal, Vec3::new(0.0, 0.035, -0.52), Vec3::new(0.012, 0.062, 0.016));
    slab(&mut metal, Vec3::new(0.0, 0.07, -0.52), Vec3::new(0.014, 0.012, 0.16));
    slab(&mut metal, Vec3::new(-0.04, 0.04, -0.72), Vec3::new(0.012, 0.045, 0.014));
    metal.add_ring(Vec3::new(-0.04, 0.066, -0.725), 0.011, 0.0085, 0.01, 12, NONE);
    // The bipod, folded back along the underside of the barrel.
    for side in [-1.0f32, 1.0] {
        rod(&mut metal, Vec3::new(side * 0.03, -0.055, -0.48), Vec3::new(side * 0.034, -0.055, -0.7), 0.0065);
    }
    slab(&mut metal, Vec3::new(0.0, -0.045, -0.48), Vec3::new(0.06, 0.012, 0.03));
    // The wooden fore-grip under the receiver, the pistol grip and trigger guard.
    slab(&mut wood, Vec3::new(0.0, -0.062, -0.3), Vec3::new(0.034, 0.04, 0.16));
    tilted(&mut black, Vec3::new(0.0, -0.075, 0.02), Vec3::new(0.034, 0.1, 0.05), 0.28);
    slab(&mut metal, Vec3::new(0.0, -0.055, -0.05), Vec3::new(0.008, 0.006, 0.08));
    // The wooden butt, with its heel and a rubber pad.
    slab(&mut wood, Vec3::new(0.0, -0.004, 0.2), Vec3::new(0.046, 0.082, 0.26));
    tilted(&mut wood, Vec3::new(0.0, -0.025, 0.34), Vec3::new(0.048, 0.112, 0.1), 0.0);
    slab(&mut black, Vec3::new(0.0, -0.025, 0.395), Vec3::new(0.05, 0.12, 0.012));
    let spec = ModelSpec { muzzle: Vec3::new(0.0, 0.0, -0.855), sight_x: -0.04, sight_y: 0.066, rear_z: 0.005, front_z: -0.725, eye_relief: 0.12 };
    finish_model(spec, vec![(Finish::Metal, metal), (Finish::Black, black), (Finish::Dark, dark), (Finish::Wood, wood)])
}

/// The FN MAG: a box of a receiver with the feed cover on top, a belt coming in from an ammunition
/// box on the left, a long barrel with a gas cylinder, a bipod, a black butt and pistol grip.
fn mag() -> GunModel {
    let (mut metal, mut black, mut dark, mut olive) = (Parts::default(), Parts::default(), Parts::default(), Parts::default());
    // The receiver and the feed cover.
    slab(&mut metal, Vec3::new(0.0, -0.005, -0.21), Vec3::new(0.074, 0.1, 0.4));
    slab(&mut metal, Vec3::new(0.0, 0.055, -0.19), Vec3::new(0.068, 0.026, 0.3));
    slab(&mut dark, Vec3::new(0.0, 0.0695, -0.19), Vec3::new(0.05, 0.002, 0.2));
    // The sights: a leaf on the cover and a blade on the barrel.
    slab(&mut metal, Vec3::new(0.0, 0.082, -0.07), Vec3::new(0.012, 0.03, 0.014));
    metal.add_ring(Vec3::new(0.0, 0.092, -0.07), 0.01, 0.0035, 0.004, 14, NONE);
    slab(&mut metal, Vec3::new(0.0, 0.0445, -0.82), Vec3::new(0.006, 0.062, 0.012));
    metal.add_ring(Vec3::new(0.0, 0.0775, -0.82), 0.0105, 0.008, 0.01, 12, NONE);
    // The barrel with a carrying handle, the flash hider, and the gas cylinder beneath.
    barrel(&mut metal, 0.0, -0.9, -0.41, 0.0145);
    barrel(&mut metal, 0.0, -0.98, -0.89, 0.0205);
    for k in 0..3 {
        slab(&mut dark, Vec3::new(0.0, 0.0, -0.9 - k as f32 * 0.02), Vec3::new(0.0415, 0.004, 0.01));
    }
    barrel(&mut metal, -0.032, -0.78, -0.42, 0.0115);
    slab(&mut metal, Vec3::new(0.0, 0.0, -0.58), Vec3::new(0.012, 0.074, 0.016));
    slab(&mut metal, Vec3::new(0.0, 0.04, -0.58), Vec3::new(0.014, 0.012, 0.15));
    // The bipod, folded forward under the barrel.
    for side in [-1.0f32, 1.0] {
        rod(&mut metal, Vec3::new(side * 0.032, -0.05, -0.55), Vec3::new(side * 0.044, -0.05, -0.8), 0.0065);
    }
    // The ammunition box on the left, and the belt looping up into the feed tray.
    slab(&mut olive, Vec3::new(-0.075, -0.07, -0.16), Vec3::new(0.092, 0.13, 0.19));
    slab(&mut olive, Vec3::new(-0.075, -0.003, -0.16), Vec3::new(0.09, 0.012, 0.17));
    for k in 0..6 {
        slab(&mut metal, Vec3::new(-0.062 + k as f32 * 0.004, 0.012 + k as f32 * 0.006, -0.19 + (k as f32 * 0.008)), Vec3::new(0.02, 0.01, 0.15));
    }
    // The pistol grip, trigger guard and the butt.
    tilted(&mut black, Vec3::new(0.0, -0.09, 0.025), Vec3::new(0.034, 0.1, 0.05), 0.3);
    slab(&mut metal, Vec3::new(0.0, -0.052, -0.04), Vec3::new(0.008, 0.006, 0.08));
    slab(&mut black, Vec3::new(0.0, -0.01, 0.17), Vec3::new(0.062, 0.1, 0.24));
    slab(&mut black, Vec3::new(0.0, -0.01, 0.298), Vec3::new(0.066, 0.108, 0.016));
    let spec = ModelSpec { muzzle: Vec3::new(0.0, 0.0, -0.985), sight_x: 0.0, sight_y: 0.092, rear_z: -0.07, front_z: -0.82, eye_relief: 0.16 };
    finish_model(spec, vec![(Finish::Metal, metal), (Finish::Black, black), (Finish::Dark, dark), (Finish::Olive, olive)])
}

/// The Lee-Enfield action with its bolt handle and box magazine, the wooden stock and fore end,
/// the long barrel, the nose cap. `scoped` fits the telescopic sight and cheek piece of the L42A1.
fn bolt_rifle(scoped: bool) -> GunModel {
    let (mut metal, mut black, mut dark, mut wood, mut glass) = (Parts::default(), Parts::default(), Parts::default(), Parts::default(), Parts::default());
    // The receiver, the bolt body and its handle: out to the right, swept back and down, with a knob.
    slab(&mut metal, Vec3::new(0.0, -0.003, -0.1), Vec3::new(0.038, 0.058, 0.22));
    barrel(&mut metal, 0.011, -0.012, 0.04, 0.0105);
    rod(&mut metal, Vec3::new(0.017, 0.012, 0.03), Vec3::new(0.062, -0.002, 0.058), 0.0048);
    metal.add_tube(Vec3::new(0.068, -0.005, 0.063), 0.0105, 0.02, 12, Quat::from_rotation_y(0.9));
    slab(&mut dark, Vec3::new(0.0197, 0.014, -0.1), Vec3::new(0.001, 0.016, 0.08));
    // The barrel: the Lee-Enfield's is long and heavy; the sniper's heavier still.
    let muzzle = if scoped { -0.74 } else { -0.66 };
    barrel(&mut metal, 0.0, muzzle, -0.22, if scoped { 0.0135 } else { 0.0115 });
    // The magazine under the action, the trigger guard and trigger.
    slab(&mut metal, Vec3::new(0.0, -0.062, -0.1), Vec3::new(0.034, 0.05, 0.12));
    slab(&mut metal, Vec3::new(0.0, -0.048, -0.01), Vec3::new(0.008, 0.005, 0.1));
    slab(&mut dark, Vec3::new(0.0, -0.032, -0.05), Vec3::new(0.005, 0.02, 0.006));
    // The wooden fore end running up the barrel to the nose cap, and the stock behind the action.
    let fore_end = if scoped { -0.5 } else { -0.45 };
    slab(&mut wood, Vec3::new(0.0, -0.012, (fore_end - 0.22) * 0.5), Vec3::new(0.04, 0.052, 0.22 - fore_end));
    slab(&mut wood, Vec3::new(0.0, 0.016, (fore_end - 0.22) * 0.5), Vec3::new(0.03, 0.012, 0.22 - fore_end));
    slab(&mut metal, Vec3::new(0.0, -0.002, fore_end - 0.01), Vec3::new(0.034, 0.04, 0.02));
    tilted(&mut wood, Vec3::new(0.0, -0.012, 0.02), Vec3::new(0.036, 0.06, 0.1), 0.0);
    let cheek = if scoped { 0.02 } else { 0.0 };
    slab(&mut wood, Vec3::new(0.0, -0.014 + cheek * 0.5, 0.24), Vec3::new(0.04, 0.07 + cheek, 0.34));
    tilted(&mut wood, Vec3::new(0.0, -0.03 + cheek * 0.5, 0.41), Vec3::new(0.042, 0.1 + cheek, 0.1), 0.0);
    slab(&mut black, Vec3::new(0.0, -0.03 + cheek * 0.5, 0.465), Vec3::new(0.046, 0.115 + cheek, 0.012));
    // The sights.
    let (sight_y, rear_z, front_z, eye_relief);
    if scoped {
        // A telescope on a bracket along the left of the receiver: tube, objective bell, eyepiece.
        slab(&mut metal, Vec3::new(0.0, 0.038, -0.12), Vec3::new(0.012, 0.035, 0.02));
        slab(&mut metal, Vec3::new(0.0, 0.038, -0.01), Vec3::new(0.012, 0.035, 0.02));
        barrel(&mut metal, 0.065, -0.3, 0.03, 0.0165);
        barrel(&mut metal, 0.065, -0.4, -0.3, 0.0245);
        barrel(&mut glass, 0.065, -0.4005, -0.3995, 0.022);
        barrel(&mut metal, 0.065, 0.03, 0.095, 0.0215);
        barrel(&mut glass, 0.065, 0.0955, 0.0965, 0.014);
        slab(&mut metal, Vec3::new(0.0, 0.088, -0.12), Vec3::new(0.015, 0.012, 0.03));
        slab(&mut metal, Vec3::new(0.0, 0.089, -0.04), Vec3::new(0.015, 0.012, 0.03));
        (sight_y, rear_z, front_z, eye_relief) = (0.065, 0.097, -0.4, 0.07);
    } else {
        // A flip-up aperture on the receiver's bridge and a blade between protecting ears at the muzzle.
        slab(&mut metal, Vec3::new(0.0, 0.04, -0.015), Vec3::new(0.014, 0.02, 0.012));
        metal.add_ring(Vec3::new(0.0, 0.049, -0.015), 0.011, 0.0035, 0.004, 14, NONE);
        slab(&mut metal, Vec3::new(0.0, 0.017, muzzle + 0.04), Vec3::new(0.007, 0.028, 0.012));
        for side in [-1.0f32, 1.0] {
            slab(&mut metal, Vec3::new(side * 0.011, 0.02, muzzle + 0.04), Vec3::new(0.003, 0.034, 0.012));
        }
        (sight_y, rear_z, front_z, eye_relief) = (0.049, -0.015, muzzle + 0.04, 0.2);
    }
    let spec = ModelSpec { muzzle: Vec3::new(0.0, 0.0, muzzle - 0.005), sight_x: 0.0, sight_y, rear_z, front_z, eye_relief };
    finish_model(spec, vec![(Finish::Metal, metal), (Finish::Black, black), (Finish::Dark, dark), (Finish::Wood, wood), (Finish::Glass, glass)])
}

fn l42() -> GunModel {
    bolt_rifle(true)
}

fn lee_enfield() -> GunModel {
    bolt_rifle(false)
}

/// The Auto-5: the humpbacked receiver, a long barrel with a tube magazine below it, a wooden
/// fore end and stock with a pistol-grip wrist, a bead at the muzzle.
fn auto5() -> GunModel {
    let (mut metal, mut black, mut dark, mut wood) = (Parts::default(), Parts::default(), Parts::default(), Parts::default());
    // The receiver and its hump, the carrier below.
    slab(&mut metal, Vec3::new(0.0, -0.004, -0.12), Vec3::new(0.04, 0.066, 0.26));
    tilted(&mut metal, Vec3::new(0.0, 0.03, 0.04), Vec3::new(0.036, 0.05, 0.14), 0.12);
    slab(&mut dark, Vec3::new(0.0207, 0.002, -0.12), Vec3::new(0.001, 0.016, 0.07));
    // The barrel with its rib, the tube below it, the clamp between, the muzzle bead.
    barrel(&mut metal, 0.0, -0.78, -0.25, 0.013);
    slab(&mut metal, Vec3::new(0.0, 0.0165, -0.52), Vec3::new(0.009, 0.004, 0.53));
    barrel(&mut metal, -0.034, -0.73, -0.25, 0.0115);
    slab(&mut metal, Vec3::new(0.0, -0.016, -0.7), Vec3::new(0.014, 0.036, 0.018));
    slab(&mut metal, Vec3::new(0.0, 0.0195, -0.775), Vec3::new(0.004, 0.006, 0.006));
    // The wooden fore end round the tube.
    slab(&mut wood, Vec3::new(0.0, -0.034, -0.42), Vec3::new(0.05, 0.05, 0.25));
    // The trigger guard and a pistol-grip wrist, the stock with its comb and butt pad.
    slab(&mut metal, Vec3::new(0.0, -0.05, -0.02), Vec3::new(0.008, 0.006, 0.08));
    tilted(&mut wood, Vec3::new(0.0, -0.05, 0.1), Vec3::new(0.036, 0.075, 0.1), 0.28);
    tilted(&mut wood, Vec3::new(0.0, -0.025, 0.31), Vec3::new(0.04, 0.088, 0.38), -0.04);
    slab(&mut black, Vec3::new(0.0, -0.075, 0.51), Vec3::new(0.044, 0.125, 0.014));
    // The cocking handle, small, on the right of the bolt.
    slab(&mut metal, Vec3::new(0.026, 0.01, -0.02), Vec3::new(0.012, 0.01, 0.02));
    let spec = ModelSpec { muzzle: Vec3::new(0.0, 0.0, -0.785), sight_x: 0.0, sight_y: 0.0575, rear_z: 0.0, front_z: -0.775, eye_relief: 0.17 };
    finish_model(spec, vec![(Finish::Metal, metal), (Finish::Black, black), (Finish::Dark, dark), (Finish::Wood, wood)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_gun_has_a_model_with_its_muzzle_ahead_of_its_sights_and_its_sights_aligned() {
        for kind in WeaponKind::ALL {
            let model = build(kind);
            let spec = model.spec;
            assert!(!model.parts.is_empty(), "{kind:?}");
            assert!(spec.muzzle.z < spec.front_z + 0.02, "{kind:?}: muzzle {} vs front sight {}", spec.muzzle.z, spec.front_z);
            assert!(spec.front_z < spec.rear_z - 0.25 || kind == WeaponKind::HiPower && spec.front_z < spec.rear_z - 0.15, "{kind:?}: the sights are a sight radius apart");
            // On the sights both ends of the line of sight are on the camera's axis.
            let at = |p: Vec3| p + spec.aimed_position();
            let (rear, front) = spec.sight_points();
            for p in [at(rear), at(front)] {
                assert!(p.x.abs() < 1e-4 && p.y.abs() < 1e-4, "{kind:?}: sight at {p:?}");
            }
            // The rear sight is the eye relief in front of the eye.
            assert!((at(rear).z + spec.eye_relief).abs() < 1e-4, "{kind:?}");
        }
    }

    #[test]
    fn every_model_is_about_the_size_of_the_real_gun() {
        // Overall lengths in metres, from the published specifications (to within a few centimetres).
        let lengths = [
            (WeaponKind::Sterling, 0.48, 0.75),
            (WeaponKind::HiPower, 0.18, 0.26),
            (WeaponKind::Slr, 1.0, 1.2),
            (WeaponKind::Bren, 1.05, 1.3),
            (WeaponKind::Mag, 1.05, 1.35),
            (WeaponKind::L42, 1.1, 1.25),
            (WeaponKind::Auto5, 1.0, 1.4),
            (WeaponKind::LeeEnfield, 1.05, 1.2),
        ];
        for (kind, low, high) in lengths {
            let model = build(kind);
            let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
            for (_, parts) in &model.parts {
                let (a, b) = parts.bounds();
                lo = lo.min(a);
                hi = hi.max(b);
            }
            let length = hi.z - lo.z;
            assert!(length > low && length < high, "{kind:?} is {length:.2} m long");
            // And not absurdly tall or wide.
            assert!(hi.y - lo.y < 0.4 && hi.x - lo.x < 0.3, "{kind:?}: {:?}", hi - lo);
        }
    }

    #[test]
    fn the_sniper_rifle_has_glass_and_the_machine_gun_an_ammunition_box() {
        assert!(build(WeaponKind::L42).parts.iter().any(|(f, _)| *f == Finish::Glass));
        assert!(!build(WeaponKind::LeeEnfield).parts.iter().any(|(f, _)| *f == Finish::Glass));
        assert!(build(WeaponKind::Mag).parts.iter().any(|(f, _)| *f == Finish::Olive));
    }

    #[test]
    fn rifles_have_wood_and_pistols_do_not() {
        for kind in [WeaponKind::Slr, WeaponKind::Bren, WeaponKind::L42, WeaponKind::LeeEnfield, WeaponKind::Auto5] {
            assert!(build(kind).parts.iter().any(|(f, _)| *f == Finish::Wood), "{kind:?}");
        }
        assert!(!build(WeaponKind::HiPower).parts.iter().any(|(f, _)| *f == Finish::Wood));
    }

}

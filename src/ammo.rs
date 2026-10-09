//! Ammunition: the calibres the guns take, and the loads available in each.
//!
//! Every load is a real round with real figures (bullet weight and diameter, ballistic coefficient,
//! muzzle velocity from the barrel the guns here have), so a heavier bullet holds its speed and a
//! faster one flattens its flight. What a load does to a target follows from its energy, scaled by
//! what the bullet is made to do: a hollow point gives up more of it, an armour-piercing core
//! keeps most of it.

use crate::ballistics::Cartridge;

/// What a gun is chambered for.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum Caliber {
    /// 9 x 19 mm Parabellum.
    Nine,
    /// 7.62 x 51 mm NATO.
    Nato,
    /// .303 British.
    Britain,
    /// 12 gauge, 2 3/4 inch.
    Gauge12,
}

impl Caliber {
    pub const ALL: [Caliber; 4] = [Caliber::Nine, Caliber::Nato, Caliber::Britain, Caliber::Gauge12];

    pub fn name(self) -> &'static str {
        match self {
            Caliber::Nine => "9x19mm",
            Caliber::Nato => "7.62x51mm",
            Caliber::Britain => ".303 British",
            Caliber::Gauge12 => "12 gauge",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum AmmoKind {
    NineFmj,
    NineHollowPoint,
    NinePlusP,
    NatoBall,
    NatoTracer,
    NatoArmourPiercing,
    NatoMatch,
    BritishBall,
    BritishTracer,
    BritishArmourPiercing,
    Buckshot,
    Slug,
    Birdshot,
}

/// How a round is made, which decides how it hits.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Build {
    /// A jacketed bullet.
    Full,
    /// Opens up on impact: gives up its energy at once.
    Expanding,
    /// A hardened core: goes through, giving up less.
    Piercing,
    /// A load of pellets.
    Shot,
    /// One heavy lump of lead.
    Slug,
}

#[derive(Clone, Copy, Debug)]
pub struct AmmoDef {
    pub kind: AmmoKind,
    pub caliber: Caliber,
    pub name: &'static str,
    /// A word or two for the HUD.
    pub short: &'static str,
    /// One bullet (or one pellet): its weight, shape and speed.
    pub cartridge: Cartridge,
    /// How many projectiles one round throws, and how widely they scatter (half-angle, radians).
    pub pellets: u32,
    pub pellet_spread: f32,
    /// What it does to a target for each unit of energy it carries.
    pub build: Build,
    /// Whether the bullet glows in flight.
    pub tracer: bool,
    /// How many rounds make a full stack in one square of the inventory.
    pub per_stack: u32,
    /// The colour of its box, for the inventory and the pickup.
    pub colour: [f32; 3],
}

/// A projectile keeps its full damage while it is above this fraction of its muzzle speed.
const FULL_DAMAGE_FRACTION: f32 = 0.82;

/// Damage a joule of bullet energy does to something, before the build of the bullet counts:
/// the 124 grain 9 mm round at its muzzle speed (544 J) does 25.
pub const DAMAGE_PER_JOULE: f32 = 25.0 / 544.0;

impl Build {
    /// How much of its energy it puts into what it hits.
    pub fn effect(self) -> f32 {
        match self {
            Build::Full => 1.0,
            Build::Expanding => 1.3,
            Build::Piercing => 0.8,
            Build::Shot => 1.0,
            Build::Slug => 1.0,
        }
    }
}

impl AmmoDef {
    /// What one of its projectiles does on hitting at `speed`. At the speed it leaves the muzzle at it does
    /// what its energy and build say; it keeps doing that while it is still above four fifths of that
    /// speed, and after that it goes with the energy it has left.
    pub fn damage_at(&self, speed: f32) -> f32 {
        let full = DAMAGE_PER_JOULE * self.cartridge.energy(self.cartridge.muzzle_velocity) * self.build.effect();
        let reference = self.cartridge.muzzle_velocity * FULL_DAMAGE_FRACTION;
        full * (speed / reference).powi(2).min(1.0)
    }

    /// What one round does at the muzzle if every projectile hits.
    pub fn damage_at_muzzle(&self) -> f32 {
        self.damage_at(self.cartridge.muzzle_velocity) * self.pellets as f32
    }
}

const GRAIN: f32 = 0.000_064_798_9;
const INCH: f32 = 0.0254;

const fn cartridge(grains: f32, diameter_inches: f32, bc: f32, velocity: f32) -> Cartridge {
    Cartridge { mass: grains * GRAIN, diameter: diameter_inches * INCH, bc, muzzle_velocity: velocity }
}

/// Every load there is.
pub const AMMO: [AmmoDef; 13] = [
    // 9 x 19: the Sterling and the Hi-Power. The Sterling's barrel is longer, so it gets a little more
    // out of each round than a pistol does; the figures are for the pistol, and the submachine gun's
    // velocity is made up by the weapon.
    AmmoDef { kind: AmmoKind::NineFmj, caliber: Caliber::Nine, name: "9mm full metal jacket", short: "FMJ", cartridge: cartridge(124.0, 0.355, 0.145, 368.0), pellets: 1, pellet_spread: 0.0, build: Build::Full, tracer: false, per_stack: 50, colour: [0.72, 0.62, 0.3] },
    AmmoDef { kind: AmmoKind::NineHollowPoint, caliber: Caliber::Nine, name: "9mm hollow point", short: "HP", cartridge: cartridge(115.0, 0.355, 0.125, 380.0), pellets: 1, pellet_spread: 0.0, build: Build::Expanding, tracer: false, per_stack: 50, colour: [0.8, 0.5, 0.2] },
    AmmoDef { kind: AmmoKind::NinePlusP, caliber: Caliber::Nine, name: "9mm +P overpressure", short: "+P", cartridge: cartridge(124.0, 0.355, 0.145, 405.0), pellets: 1, pellet_spread: 0.0, build: Build::Full, tracer: false, per_stack: 50, colour: [0.85, 0.35, 0.2] },
    // 7.62 x 51: the SLR, the Bren, the MAG and the L42A1 sniper rifle.
    AmmoDef { kind: AmmoKind::NatoBall, caliber: Caliber::Nato, name: "7.62mm NATO ball", short: "Ball", cartridge: cartridge(147.0, 0.308, 0.40, 838.0), pellets: 1, pellet_spread: 0.0, build: Build::Full, tracer: false, per_stack: 20, colour: [0.55, 0.62, 0.35] },
    AmmoDef { kind: AmmoKind::NatoTracer, caliber: Caliber::Nato, name: "7.62mm NATO tracer", short: "Tracer", cartridge: cartridge(142.0, 0.308, 0.36, 830.0), pellets: 1, pellet_spread: 0.0, build: Build::Full, tracer: true, per_stack: 20, colour: [0.85, 0.25, 0.2] },
    AmmoDef { kind: AmmoKind::NatoArmourPiercing, caliber: Caliber::Nato, name: "7.62mm NATO armour piercing", short: "AP", cartridge: cartridge(139.0, 0.308, 0.39, 850.0), pellets: 1, pellet_spread: 0.0, build: Build::Piercing, tracer: false, per_stack: 20, colour: [0.2, 0.2, 0.22] },
    AmmoDef { kind: AmmoKind::NatoMatch, caliber: Caliber::Nato, name: "7.62mm NATO match", short: "Match", cartridge: cartridge(168.0, 0.308, 0.46, 790.0), pellets: 1, pellet_spread: 0.0, build: Build::Expanding, tracer: false, per_stack: 20, colour: [0.9, 0.9, 0.85] },
    // .303 British: the Lee-Enfield.
    AmmoDef { kind: AmmoKind::BritishBall, caliber: Caliber::Britain, name: ".303 Mk VII ball", short: "Ball", cartridge: cartridge(174.0, 0.311, 0.42, 744.0), pellets: 1, pellet_spread: 0.0, build: Build::Full, tracer: false, per_stack: 20, colour: [0.6, 0.45, 0.25] },
    AmmoDef { kind: AmmoKind::BritishTracer, caliber: Caliber::Britain, name: ".303 Mk VII tracer", short: "Tracer", cartridge: cartridge(166.0, 0.311, 0.38, 730.0), pellets: 1, pellet_spread: 0.0, build: Build::Full, tracer: true, per_stack: 20, colour: [0.8, 0.3, 0.25] },
    AmmoDef { kind: AmmoKind::BritishArmourPiercing, caliber: Caliber::Britain, name: ".303 armour piercing", short: "AP", cartridge: cartridge(174.0, 0.311, 0.44, 740.0), pellets: 1, pellet_spread: 0.0, build: Build::Piercing, tracer: false, per_stack: 20, colour: [0.25, 0.25, 0.28] },
    // 12 gauge: the Auto-5. Nine pellets of 00 buck, a one-ounce slug, or a load of fine shot.
    AmmoDef { kind: AmmoKind::Buckshot, caliber: Caliber::Gauge12, name: "12 gauge 00 buckshot", short: "Buck", cartridge: cartridge(54.0, 0.33, 0.035, 400.0), pellets: 9, pellet_spread: 0.032, build: Build::Shot, tracer: false, per_stack: 10, colour: [0.7, 0.15, 0.15] },
    AmmoDef { kind: AmmoKind::Slug, caliber: Caliber::Gauge12, name: "12 gauge slug", short: "Slug", cartridge: cartridge(438.0, 0.72, 0.12, 440.0), pellets: 1, pellet_spread: 0.0, build: Build::Slug, tracer: false, per_stack: 10, colour: [0.15, 0.3, 0.65] },
    AmmoDef { kind: AmmoKind::Birdshot, caliber: Caliber::Gauge12, name: "12 gauge birdshot", short: "Bird", cartridge: cartridge(1.8, 0.11, 0.012, 380.0), pellets: 90, pellet_spread: 0.05, build: Build::Shot, tracer: false, per_stack: 10, colour: [0.75, 0.7, 0.2] },
];

impl AmmoKind {
    pub fn def(self) -> &'static AmmoDef {
        AMMO.iter().find(|a| a.kind == self).expect("every kind has a def")
    }

    /// Every load in a calibre.
    pub fn of(caliber: Caliber) -> impl Iterator<Item = AmmoKind> {
        AMMO.iter().filter(move |a| a.caliber == caliber).map(|a| a.kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_is_a_def_for_every_kind_in_the_table_once() {
        for def in &AMMO {
            assert_eq!(AMMO.iter().filter(|a| a.kind == def.kind).count(), 1, "{:?}", def.kind);
            assert_eq!(def.kind.def().name, def.name);
        }
        for caliber in Caliber::ALL {
            assert!(AmmoKind::of(caliber).count() >= 2, "{caliber:?} has a choice of loads");
        }
    }

    #[test]
    fn the_standard_nine_millimetre_round_does_what_it_always_did() {
        let fmj = AmmoKind::NineFmj.def();
        assert!((fmj.damage_at(368.0) - 25.0).abs() < 0.2, "{}", fmj.damage_at(368.0));
    }

    #[test]
    fn rifle_rounds_hit_far_harder_than_pistol_rounds_and_a_hollow_point_beats_a_jacket() {
        let pistol = AmmoKind::NineFmj.def().damage_at_muzzle();
        assert!(AmmoKind::NatoBall.def().damage_at_muzzle() > pistol * 4.0);
        assert!(AmmoKind::NineHollowPoint.def().damage_at_muzzle() > pistol);
        // An armour-piercing round gives up less than ball of the same size.
        assert!(AmmoKind::NatoArmourPiercing.def().damage_at_muzzle() < AmmoKind::NatoBall.def().damage_at_muzzle() * 1.05);
    }

    #[test]
    fn a_shotgun_at_point_blank_is_deadly_and_loses_it_with_distance_by_scatter() {
        let buck = AmmoKind::Buckshot.def();
        assert!(buck.damage_at_muzzle() > 100.0, "nine pellets together: {}", buck.damage_at_muzzle());
        assert!(buck.damage_at(buck.cartridge.muzzle_velocity) < 25.0, "one pellet alone is not much");
        // Bird shot is a lot of small pellets: weak up close compared with buck.
        assert!(AmmoKind::Birdshot.def().damage_at_muzzle() < buck.damage_at_muzzle());
        assert!(AmmoKind::Slug.def().damage_at_muzzle() > 100.0);
    }

    #[test]
    fn the_figures_are_those_of_real_rounds() {
        // 9 mm: 8 g at 368 m/s is 540 J. 7.62 ball: 9.5 g at 838 m/s is 3.3 kJ.
        let nine = AmmoKind::NineFmj.def().cartridge;
        assert!((nine.energy(nine.muzzle_velocity) - 544.0).abs() < 20.0);
        let ball = AmmoKind::NatoBall.def().cartridge;
        assert!((ball.energy(ball.muzzle_velocity) - 3340.0).abs() < 100.0, "{}", ball.energy(ball.muzzle_velocity));
        // Every bullet's ballistic coefficient is physically sensible for its weight and calibre.
        for def in &AMMO {
            assert!(def.cartridge.form_factor() > 0.5 && def.cartridge.form_factor() < 4.0, "{:?} form factor {}", def.kind, def.cartridge.form_factor());
        }
    }

    #[test]
    fn a_match_bullet_holds_its_speed_better_than_ball() {
        assert!(AmmoKind::NatoMatch.def().cartridge.bc > AmmoKind::NatoBall.def().cartridge.bc);
    }
}

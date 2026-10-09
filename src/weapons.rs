//! The guns: which there are, what they are chambered for, how their actions work and how they
//! handle. Everything a gun's behaviour depends on is in its `WeaponDef`, so the gun code itself
//! doesn't know which gun it is.

use bevy::prelude::*;

use crate::ammo::{AmmoKind, Caliber};
use crate::gun_state::{BoltType, Mechanism};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum WeaponKind {
    Sterling,
    HiPower,
    Slr,
    Bren,
    Mag,
    L42,
    Auto5,
    LeeEnfield,
}

impl WeaponKind {
    pub const ALL: [WeaponKind; 8] = [
        WeaponKind::Sterling,
        WeaponKind::HiPower,
        WeaponKind::Slr,
        WeaponKind::Bren,
        WeaponKind::Mag,
        WeaponKind::L42,
        WeaponKind::Auto5,
        WeaponKind::LeeEnfield,
    ];

    pub fn def(self) -> &'static WeaponDef {
        &WEAPONS[self as usize]
    }
}

/// How the trigger works.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FireMode {
    /// One round a pull.
    Semi,
    /// Rounds for as long as the trigger is held.
    Auto,
}

impl FireMode {
    pub fn name(self) -> &'static str {
        match self {
            FireMode::Semi => "semi",
            FireMode::Auto => "auto",
        }
    }
}

/// A telescopic sight.
#[derive(Clone, Copy, Debug)]
pub struct Scope {
    /// What the field of view is narrowed to when looking through it, as a fraction.
    pub fov_scale: f32,
}

/// How a gun feels in the hands.
#[derive(Clone, Copy, Debug)]
pub struct Handling {
    /// Where it sits in the camera's space at the hip.
    pub hip_position: Vec3,
    /// How far a shot can stray (half-angle, radians): from the hip, and on the sights.
    pub hip_spread: f32,
    pub ads_spread: f32,
    /// Extra at a full run and in the air.
    pub moving_spread: f32,
    pub airborne_spread: f32,
    /// What each shot adds to the burst bloom, and the most it can come to.
    pub bloom_per_shot: f32,
    pub bloom_max: f32,
    /// What a shot kicks the view: up, and a little sideways, in radians.
    pub kick_pitch: f32,
    pub kick_yaw: f32,
    /// How hard the gun model itself jolts back and tips up.
    pub model_kick: f32,
    /// How quickly it comes up to the shoulder (per second, exponential): heavy guns are slower.
    pub aim_rate: f32,
    /// How much the view narrows on the sights.
    pub ads_fov_scale: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct WeaponDef {
    pub kind: WeaponKind,
    pub name: &'static str,
    pub caliber: Caliber,
    pub mechanism: Mechanism,
    /// The ways its trigger can be set; the first is what it starts in.
    pub modes: &'static [FireMode],
    /// Rounds it holds (the magazine, the belt, the tube).
    pub magazine: u32,
    /// The range its sights are zeroed for, metres.
    pub zero: f32,
    pub handling: Handling,
    pub scope: Option<Scope>,
    /// How much of a round's rated muzzle velocity this barrel gets: a pistol's short barrel less,
    /// a rifle's long one a little more.
    pub velocity_scale: f32,
    /// The loudness and pitch of its shot, relative to the recording the sounds are made from.
    pub shot_volume: f32,
    pub shot_pitch: f32,
    /// How big it is in the inventory (squares across, squares down).
    pub size: (u32, u32),
    /// What it has in it when found, if the loot doesn't say.
    pub default_ammo: AmmoKind,
}

impl WeaponDef {
    /// The loads it can fire.
    pub fn ammo(&self) -> impl Iterator<Item = AmmoKind> {
        AmmoKind::of(self.caliber)
    }

    pub fn is_automatic(&self) -> bool {
        self.modes.contains(&FireMode::Auto)
    }
}

const fn mech(bolt_type: BoltType, cycle_time: f32, reload_time: f32, charge_time: f32, reload_per_round: f32) -> Mechanism {
    Mechanism { bolt_type, cycle_time, reload_time, charge_time, reload_per_round }
}

const fn handling(
    hip_spread: f32,
    ads_spread: f32,
    moving_spread: f32,
    bloom_per_shot: f32,
    kick_pitch: f32,
    model_kick: f32,
    aim_rate: f32,
    ads_fov_scale: f32,
) -> Handling {
    Handling {
        hip_position: Vec3::new(0.2, -0.2, -0.5),
        hip_spread,
        ads_spread,
        moving_spread,
        airborne_spread: 0.035,
        bloom_per_shot,
        bloom_max: 0.03,
        kick_pitch,
        kick_yaw: kick_pitch * 0.33,
        model_kick,
        aim_rate,
        ads_fov_scale,
    }
}

/// In the order of `WeaponKind`.
pub const WEAPONS: [WeaponDef; 8] = [
    // The Sterling L2A3: open bolt, about 500 rounds a minute, a 9 mm submachine gun.
    WeaponDef {
        kind: WeaponKind::Sterling,
        name: "Sterling L2A3",
        caliber: Caliber::Nine,
        mechanism: mech(BoltType::Open, 0.12, 2.0, 0.7, 0.0),
        modes: &[FireMode::Auto, FireMode::Semi],
        magazine: 30,
        zero: 20.0,
        handling: handling(0.030, 0.0035, 0.022, 0.0045, 0.0085, 1.0, 14.0, 0.72),
        scope: None,
        velocity_scale: 1.0,
        shot_volume: 1.0,
        shot_pitch: 1.0,
        size: (5, 2),
        default_ammo: AmmoKind::NineFmj,
    },
    // The Browning Hi-Power: the L9A1, a 13 round 9 mm pistol, semi-automatic, closed bolt.
    WeaponDef {
        kind: WeaponKind::HiPower,
        name: "Browning Hi-Power",
        caliber: Caliber::Nine,
        mechanism: mech(BoltType::Closed, 0.13, 1.9, 0.35, 0.0),
        modes: &[FireMode::Semi],
        magazine: 13,
        zero: 25.0,
        handling: handling(0.026, 0.0045, 0.020, 0.006, 0.0125, 1.2, 18.0, 0.8),
        scope: None,
        velocity_scale: 0.95,
        shot_volume: 0.85,
        shot_pitch: 1.12,
        size: (2, 2),
        default_ammo: AmmoKind::NineFmj,
    },
    // The L1A1 self-loading rifle: 20 rounds of 7.62 NATO, semi-automatic.
    WeaponDef {
        kind: WeaponKind::Slr,
        name: "L1A1 SLR",
        caliber: Caliber::Nato,
        mechanism: mech(BoltType::Closed, 0.15, 2.4, 0.5, 0.0),
        modes: &[FireMode::Semi],
        magazine: 20,
        zero: 100.0,
        handling: handling(0.034, 0.0013, 0.026, 0.0065, 0.020, 1.5, 11.0, 0.7),
        scope: None,
        velocity_scale: 1.0,
        shot_volume: 1.35,
        shot_pitch: 0.7,
        size: (6, 2),
        default_ammo: AmmoKind::NatoBall,
    },
    // The L4A4 Bren: the Bren gun rechambered for 7.62 NATO, a 30 round magazine on top, open bolt,
    // about 500 rounds a minute, single shots or bursts.
    WeaponDef {
        kind: WeaponKind::Bren,
        name: "L4A4 Bren",
        caliber: Caliber::Nato,
        mechanism: mech(BoltType::Open, 0.12, 2.6, 0.7, 0.0),
        modes: &[FireMode::Auto, FireMode::Semi],
        magazine: 30,
        zero: 100.0,
        handling: handling(0.036, 0.0016, 0.030, 0.0050, 0.0135, 1.1, 8.5, 0.72),
        scope: None,
        velocity_scale: 0.99,
        shot_volume: 1.4,
        shot_pitch: 0.68,
        size: (6, 3),
        default_ammo: AmmoKind::NatoBall,
    },
    // The FN MAG, the L7A2 general purpose machine gun: a belt, open bolt, about 750 rounds a minute.
    WeaponDef {
        kind: WeaponKind::Mag,
        name: "FN MAG",
        caliber: Caliber::Nato,
        mechanism: mech(BoltType::Open, 0.08, 7.5, 1.2, 0.0),
        modes: &[FireMode::Auto],
        magazine: 100,
        zero: 100.0,
        handling: handling(0.042, 0.0022, 0.040, 0.0042, 0.0105, 0.9, 6.5, 0.75),
        scope: None,
        velocity_scale: 1.01,
        shot_volume: 1.5,
        shot_pitch: 0.64,
        size: (6, 3),
        default_ammo: AmmoKind::NatoTracer,
    },
    // The L42A1 sniper rifle: a Lee-Enfield action in 7.62 NATO, ten rounds, bolt action, with a
    // telescopic sight.
    WeaponDef {
        kind: WeaponKind::L42,
        name: "L42A1",
        caliber: Caliber::Nato,
        mechanism: mech(BoltType::Closed, 1.3, 3.2, 0.5, 0.0),
        modes: &[FireMode::Semi],
        magazine: 10,
        zero: 200.0,
        handling: handling(0.045, 0.0004, 0.034, 0.012, 0.024, 1.6, 8.0, 0.75),
        scope: Some(Scope { fov_scale: 0.2 }),
        velocity_scale: 1.01,
        shot_volume: 1.4,
        shot_pitch: 0.72,
        size: (6, 2),
        default_ammo: AmmoKind::NatoMatch,
    },
    // The Browning Auto-5: a long-recoil semi-automatic shotgun, four in the tube and one up the spout.
    WeaponDef {
        kind: WeaponKind::Auto5,
        name: "Browning Auto-5",
        caliber: Caliber::Gauge12,
        mechanism: mech(BoltType::Closed, 0.3, 0.6, 0.5, 0.55),
        modes: &[FireMode::Semi],
        magazine: 5,
        zero: 35.0,
        handling: handling(0.020, 0.010, 0.022, 0.012, 0.032, 1.8, 12.0, 0.8),
        scope: None,
        velocity_scale: 1.0,
        shot_volume: 1.45,
        shot_pitch: 0.6,
        size: (6, 2),
        default_ammo: AmmoKind::Buckshot,
    },
    // The Lee-Enfield: the No. 4 Mk I, ten rounds of .303, bolt action.
    WeaponDef {
        kind: WeaponKind::LeeEnfield,
        name: "Lee-Enfield No.4",
        caliber: Caliber::Britain,
        mechanism: mech(BoltType::Closed, 0.85, 3.4, 0.4, 0.0),
        modes: &[FireMode::Semi],
        magazine: 10,
        zero: 100.0,
        handling: handling(0.038, 0.0016, 0.028, 0.010, 0.025, 1.5, 10.5, 0.7),
        scope: None,
        velocity_scale: 1.0,
        shot_volume: 1.3,
        shot_pitch: 0.74,
        size: (6, 2),
        default_ammo: AmmoKind::BritishBall,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_in_the_order_of_the_kinds() {
        for (i, def) in WEAPONS.iter().enumerate() {
            assert_eq!(def.kind as usize, i, "{}", def.name);
            assert_eq!(def.kind.def().name, def.name);
        }
        assert_eq!(WeaponKind::ALL.len(), WEAPONS.len());
    }

    #[test]
    fn every_gun_can_fire_what_it_starts_with_and_has_a_choice_of_loads() {
        for def in &WEAPONS {
            assert_eq!(def.default_ammo.def().caliber, def.caliber, "{}", def.name);
            assert!(def.ammo().count() >= 2, "{} has a choice of rounds", def.name);
        }
    }

    #[test]
    fn the_guns_are_the_ones_asked_for() {
        let names: Vec<&str> = WEAPONS.iter().map(|w| w.name).collect();
        for want in ["Hi-Power", "SLR", "Bren", "MAG", "L42A1", "Auto-5", "Lee-Enfield"] {
            assert!(names.iter().any(|n| n.contains(want)), "{want}");
        }
        // And the calibres they are known for.
        assert_eq!(WeaponKind::HiPower.def().caliber, Caliber::Nine);
        assert_eq!(WeaponKind::Slr.def().caliber, Caliber::Nato);
        assert_eq!(WeaponKind::Bren.def().caliber, Caliber::Nato);
        assert_eq!(WeaponKind::Mag.def().caliber, Caliber::Nato);
        assert_eq!(WeaponKind::L42.def().caliber, Caliber::Nato);
        assert_eq!(WeaponKind::Auto5.def().caliber, Caliber::Gauge12);
        assert_eq!(WeaponKind::LeeEnfield.def().caliber, Caliber::Britain);
    }

    #[test]
    fn bolt_guns_are_slow_and_machine_guns_are_fast_and_only_the_sniper_has_a_scope() {
        let rate = |k: WeaponKind| 1.0 / k.def().mechanism.cycle_time;
        assert!(rate(WeaponKind::LeeEnfield) < 1.5 && rate(WeaponKind::L42) < 1.0);
        assert!(rate(WeaponKind::Mag) > rate(WeaponKind::Bren) && rate(WeaponKind::Bren) > rate(WeaponKind::Slr));
        assert!(WEAPONS.iter().filter(|w| w.scope.is_some()).count() == 1 && WeaponKind::L42.def().scope.is_some());
        assert!(WeaponKind::Mag.def().is_automatic() && !WeaponKind::Slr.def().is_automatic());
    }

    #[test]
    fn heavier_guns_come_up_to_the_shoulder_more_slowly_and_precision_guns_are_tighter() {
        let h = |k: WeaponKind| k.def().handling;
        assert!(h(WeaponKind::Mag).aim_rate < h(WeaponKind::Bren).aim_rate && h(WeaponKind::Bren).aim_rate < h(WeaponKind::HiPower).aim_rate);
        assert!(h(WeaponKind::L42).ads_spread < h(WeaponKind::Slr).ads_spread && h(WeaponKind::Slr).ads_spread < h(WeaponKind::Sterling).ads_spread);
        // Heavy rounds kick harder.
        assert!(h(WeaponKind::Slr).kick_pitch > h(WeaponKind::HiPower).kick_pitch);
    }

    #[test]
    fn the_inventory_sizes_fit_a_pair_of_pockets() {
        for def in &WEAPONS {
            assert!(def.size.0 <= 6 && def.size.1 <= 3, "{} is {:?}", def.name, def.size);
        }
    }
}

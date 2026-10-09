//! Things lying about to be found, for now as coloured boxes: the colour says how rare.

use bevy::prelude::*;

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

/// A box of loot lying where it was put.
#[derive(Component)]
pub struct LootBox {
    pub rarity: Rarity,
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

pub struct LootPlugin;

impl Plugin for LootPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, make_assets).add_systems(Update, spin);
    }
}

fn make_assets(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    // Lit by nothing, so that they show up in a dark cellar as well as in the sun.
    let materials = Rarity::ALL.iter().map(|r| materials.add(StandardMaterial { base_color: r.colour(), unlit: true, ..default() })).collect();
    commands.insert_resource(LootAssets { mesh, materials });
}

/// Boxes turn slowly, so that they catch the eye.
fn spin(time: Res<Time>, mut boxes: Query<&mut Transform, With<LootBox>>) {
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
}

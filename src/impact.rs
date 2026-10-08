//! What happens where a bullet lands: a bullet hole, a spray of chips and a puff of dust, and a
//! thump that reaches the player at the speed of sound.
//!
//! The bullet code reports each landing as an [`Impact`]; everything visible and audible is made
//! here. The geometry (where exactly the bullet hit, which way the surface faces, which way chips
//! fly) is in plain functions so it can be tested without a running game.

use std::collections::VecDeque;

use bevy::asset::RenderAssetUsages;
use bevy::audio::AudioSource;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::map::TerrainMap;
use crate::player::FpsCamera;
use crate::sound::{arrival_delay, play_after};
use crate::wind::Wind;

/// What was hit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Surface {
    Ground,
    Water,
    Target,
}

/// A bullet has landed.
#[derive(Message, Clone, Copy, Debug)]
pub struct Impact {
    pub position: Vec3,
    /// Which way the surface faces there (a unit vector pointing out of it).
    pub normal: Vec3,
    pub surface: Surface,
    /// How fast the bullet was going, m/s.
    pub speed: f32,
    /// The thing that was hit, if it's an object rather than the land.
    pub target: Option<Entity>,
}

// ---- geometry -------------------------------------------------------------------------------

/// The way the ground faces at `p`: straight up on the flat, tipped on a slope.
pub fn terrain_normal(map: &TerrainMap, p: Vec2) -> Vec3 {
    map.normal_at(p)
}

/// The height of whatever a bullet falling through `p` meets first: the water's surface where
/// there is water, the ground otherwise.
fn surface_at(map: &TerrainMap, p: Vec2) -> (f32, Surface) {
    match map.water_surface_at(p) {
        Some(level) => (level, Surface::Water),
        None => (map.surface_height_at(p), Surface::Ground),
    }
}

/// Where the straight path from `from` to `to` first goes below the land or the water, if it does.
/// Returns the point on the surface, the way the surface faces, and which it is.
pub fn surface_hit(map: &TerrainMap, from: Vec3, to: Vec3) -> Option<(Vec3, Vec3, Surface)> {
    let below = |p: Vec3| p.y < surface_at(map, Vec2::new(p.x, p.z)).0;
    if below(from) || !below(to) {
        // Already under it (so it isn't a landing), or still above.
        return None;
    }
    let (mut above, mut under) = (0.0f32, 1.0f32);
    for _ in 0..14 {
        let mid = 0.5 * (above + under);
        if below(from.lerp(to, mid)) {
            under = mid;
        } else {
            above = mid;
        }
    }
    let p = from.lerp(to, under);
    let (height, surface) = surface_at(map, Vec2::new(p.x, p.z));
    let point = Vec3::new(p.x, height, p.z);
    let normal = if surface == Surface::Water { Vec3::Y } else { terrain_normal(map, Vec2::new(p.x, p.z)) };
    Some((point, normal, surface))
}

/// Where the segment `p0`-`p1` first enters the box, as a fraction of the way along it, and the
/// way the face it enters through points.
pub fn segment_aabb_hit(p0: Vec3, p1: Vec3, min: Vec3, max: Vec3) -> Option<(f32, Vec3)> {
    let d = p1 - p0;
    let (mut t_min, mut t_max) = (0.0f32, 1.0f32);
    let mut normal = Vec3::ZERO;
    for i in 0..3 {
        if d[i].abs() < f32::EPSILON {
            if p0[i] < min[i] || p0[i] > max[i] {
                return None;
            }
            continue;
        }
        let inv = 1.0 / d[i];
        let (mut t1, mut t2) = ((min[i] - p0[i]) * inv, (max[i] - p0[i]) * inv);
        // Entering through the low face if moving up the axis, the high face if moving down it.
        let mut face = -1.0;
        if t1 > t2 {
            std::mem::swap(&mut t1, &mut t2);
            face = 1.0;
        }
        if t1 > t_min {
            t_min = t1;
            normal = Vec3::ZERO;
            normal[i] = face;
        }
        t_max = t_max.min(t2);
        if t_min > t_max {
            return None;
        }
    }
    if normal == Vec3::ZERO {
        // Started inside the box: call the face the one it's heading out of.
        normal = -d.normalize_or(Vec3::Y);
    }
    Some((t_min, normal))
}

// ---- randomness -----------------------------------------------------------------------------

/// A small xorshift generator: plenty for scattering chips.
#[derive(Clone, Copy, Debug)]
pub struct Rng(pub u32);

impl Rng {
    /// The next number in 0..1.
    pub fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 >> 8) as f32 / (1 << 24) as f32
    }

    pub fn range(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * self.next()
    }
}

// ---- chips (dirt, splinters, droplets) -------------------------------------------------------

/// One piece thrown up by an impact.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chip {
    pub velocity: Vec3,
    /// Edge length, metres.
    pub size: f32,
    /// How long it flies before it's gone, seconds.
    pub life: f32,
}

/// `count` pieces thrown out of a surface facing `normal`: they spray away from it in a cone, mostly
/// up and out, a few fast and most slow. `energy` (about 1 for a bullet) scales how hard.
pub fn chips(normal: Vec3, energy: f32, count: usize, rng: &mut Rng) -> Vec<Chip> {
    let tangent = normal.cross(if normal.y.abs() < 0.9 { Vec3::Y } else { Vec3::X }).normalize();
    let bitangent = normal.cross(tangent);
    (0..count)
        .map(|_| {
            let theta = rng.range(0.0, std::f32::consts::TAU);
            // Mostly slow, a few fast: squaring the random number bunches speeds at the low end.
            let speed = (1.2 + 5.0 * rng.next().powi(2)) * energy;
            let spread = rng.range(0.15, 0.9);
            let direction = (normal + (tangent * theta.cos() + bitangent * theta.sin()) * spread).normalize();
            Chip { velocity: direction * speed, size: rng.range(0.012, 0.04), life: rng.range(0.5, 1.1) }
        })
        .collect()
}

// ---- budgets --------------------------------------------------------------------------------

const MAX_HOLES: usize = 250;
const MAX_CHIPS: usize = 450;
const MAX_DUST: usize = 60;

/// Adds `entity` to the newest end of `queue`, and returns the oldest if the queue is over `limit`
/// (so that the caller can remove it).
pub fn push_bounded(queue: &mut VecDeque<Entity>, entity: Entity, limit: usize) -> Option<Entity> {
    queue.push_back(entity);
    if queue.len() > limit {
        queue.pop_front()
    } else {
        None
    }
}

// ---- assets ---------------------------------------------------------------------------------

#[derive(Resource)]
pub struct ImpactAssets {
    hole_mesh: Handle<Mesh>,
    hole_material: Handle<StandardMaterial>,
    chip_mesh: Handle<Mesh>,
    soil: Vec<Handle<StandardMaterial>>,
    sand: Handle<StandardMaterial>,
    splinter: Handle<StandardMaterial>,
    droplet: Handle<StandardMaterial>,
    dust_mesh: Handle<Mesh>,
    pub dirt_sounds: Vec<Handle<AudioSource>>,
    pub target_sounds: Vec<Handle<AudioSource>>,
    rng: Rng,
    holes: VecDeque<Entity>,
}

impl ImpactAssets {
    pub fn sounds(&self) -> impl Iterator<Item = &Handle<AudioSource>> {
        self.dirt_sounds.iter().chain(&self.target_sounds)
    }
}

/// What has happened so far (for tests and for curiosity).
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct ImpactStats {
    pub impacts: u32,
    pub holes: u32,
    pub chips: u32,
    pub puffs: u32,
    pub thumps: u32,
    /// The surface of the latest impact, and how it faced.
    pub last: Option<(Surface, Vec3)>,
    /// Where the latest impact was.
    pub last_position: Option<Vec3>,
}

#[derive(Component)]
pub struct BulletHole;

/// A piece of dirt, splinter or drop of water in flight.
#[derive(Component)]
pub struct Chipping {
    velocity: Vec3,
    life: f32,
    total: f32,
    size: f32,
}

/// A puff of dust or mist.
#[derive(Component)]
pub struct Dust {
    velocity: Vec3,
    life: f32,
    total: f32,
    size: f32,
    opacity: f32,
    material: Handle<StandardMaterial>,
}

/// The picture of a bullet hole, drawn in code: a dark hole in a ragged ring of churned earth that
/// fades out into the ground around it.
fn hole_image() -> Image {
    const N: usize = 64;
    let hash = |x: i32, y: i32| -> f32 {
        let mut h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA6B);
        h ^= h >> 15;
        h = h.wrapping_mul(0x2C1B_3C6D);
        h ^= h >> 12;
        (h & 0xFFFF) as f32 / 65535.0
    };
    let mut data = Vec::with_capacity(N * N * 4);
    for y in 0..N {
        for x in 0..N {
            let (u, v) = ((x as f32 + 0.5) / N as f32 * 2.0 - 1.0, (y as f32 + 0.5) / N as f32 * 2.0 - 1.0);
            let r = (u * u + v * v).sqrt();
            // A ragged edge: the radius of the disturbed earth varies with the angle.
            let angle = v.atan2(u);
            let ragged = 0.55 + 0.30 * hash((angle * 5.0).floor() as i32, 7) + 0.1 * (angle * 3.0).sin();
            let grain = hash(x as i32, y as i32);
            let (colour, alpha) = if r < 0.10 {
                ([12.0, 9.0, 7.0], 1.0)
            } else if r < ragged {
                let fade = 1.0 - ((r - 0.10) / (ragged - 0.10)).clamp(0.0, 1.0);
                let shade = 0.65 + 0.7 * grain;
                ([58.0 * shade, 42.0 * shade, 30.0 * shade], (fade * 1.3).min(1.0) * 0.92)
            } else {
                ([0.0, 0.0, 0.0], 0.0)
            };
            data.extend_from_slice(&[colour[0] as u8, colour[1] as u8, colour[2] as u8, (alpha * 255.0) as u8]);
        }
    }
    Image::new(
        Extent3d { width: N as u32, height: N as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

fn load_assets(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let matte = |colour: Color| StandardMaterial { base_color: colour, perceptual_roughness: 1.0, reflectance: 0.05, ..default() };
    let hole_texture = images.add(hole_image());
    commands.insert_resource(ImpactAssets {
        hole_mesh: meshes.add(Rectangle::new(1.0, 1.0)),
        hole_material: materials.add(StandardMaterial {
            base_color_texture: Some(hole_texture),
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 1.0,
            reflectance: 0.02,
            // The hole is laid a centimetre off the surface; this keeps it from fighting with it.
            depth_bias: 4.0,
            ..default()
        }),
        chip_mesh: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        soil: vec![
            materials.add(matte(Color::srgb(0.20, 0.14, 0.09))),
            materials.add(matte(Color::srgb(0.30, 0.22, 0.14))),
            materials.add(matte(Color::srgb(0.16, 0.12, 0.08))),
            materials.add(matte(Color::srgb(0.25, 0.27, 0.12))),
        ],
        sand: materials.add(matte(Color::srgb(0.68, 0.6, 0.42))),
        splinter: materials.add(matte(Color::srgb(0.72, 0.58, 0.38))),
        droplet: materials.add(StandardMaterial {
            base_color: Color::srgba(0.85, 0.92, 0.96, 0.8),
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 0.2,
            ..default()
        }),
        dust_mesh: meshes.add(Sphere::new(1.0).mesh().ico(1).expect("icosphere")),
        dirt_sounds: (1..=3).map(|i| asset_server.load(format!("sounds/impact/dirt_{i}.wav"))).collect(),
        target_sounds: (1..=2).map(|i| asset_server.load(format!("sounds/impact/target_{i}.wav"))).collect(),
        rng: Rng(0x2545_F491),
        holes: VecDeque::new(),
    });
}

// ---- making the effects ---------------------------------------------------------------------

/// The orientation that lays a flat square (facing +Z) against a surface facing `normal`, turned
/// by `roll` around that normal so that no two holes look alike.
pub fn lay_flat(normal: Vec3, roll: f32) -> Quat {
    Quat::from_rotation_arc(Vec3::Z, normal) * Quat::from_rotation_z(roll)
}

/// How far a hole sits off the surface it is on, metres.
const HOLE_LIFT: f32 = 0.012;

fn spawn_impacts(
    mut commands: Commands,
    mut impacts: MessageReader<Impact>,
    mut assets: ResMut<ImpactAssets>,
    mut stats: ResMut<ImpactStats>,
    wind: Res<Wind>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    listener: Query<&Transform, With<FpsCamera>>,
    globals: Query<&GlobalTransform>,
    live_chips: Query<(), With<Chipping>>,
    live_dust: Query<(), With<Dust>>,
) {
    let mut chip_count = live_chips.iter().count();
    let mut dust_count = live_dust.iter().count();
    let breeze = Vec3::new(wind.direction().x, 0.0, wind.direction().y) * wind.speed;
    for impact in impacts.read() {
        stats.impacts += 1;
        stats.last = Some((impact.surface, impact.normal));
        stats.last_position = Some(impact.position);
        let mut rng = assets.rng;
        // Mix the position in, so repeated impacts on one spot still differ.
        rng.0 ^= (impact.position.x * 1000.0) as i32 as u32 ^ ((impact.position.z * 1000.0) as i32 as u32).rotate_left(11);
        if rng.0 == 0 {
            rng.0 = 0x2545_F491;
        }

        // The bullet hole (not in water).
        if impact.surface != Surface::Water {
            let size = if impact.surface == Surface::Target { rng.range(0.07, 0.1) } else { rng.range(0.1, 0.15) };
            let rotation = lay_flat(impact.normal, rng.range(0.0, std::f32::consts::TAU));
            let world = Transform { translation: impact.position + impact.normal * HOLE_LIFT, rotation, scale: Vec3::new(size, size, 1.0) };
            let hole = (
                BulletHole,
                Mesh3d(assets.hole_mesh.clone()),
                MeshMaterial3d(assets.hole_material.clone()),
                NotShadowCaster,
            );
            let parent = impact.target.and_then(|t| globals.get(t).ok().map(|g| (t, *g)));
            let entity = match parent {
                // A hole in an object moves with it.
                Some((target, global)) => {
                    let into_local = global.affine().inverse();
                    let local = Transform {
                        translation: into_local.transform_point3(world.translation),
                        rotation: global.rotation().inverse() * rotation,
                        scale: world.scale,
                    };
                    let mut id = Entity::PLACEHOLDER;
                    commands.entity(target).with_children(|p| {
                        id = p.spawn((hole, local)).id();
                    });
                    id
                }
                None => commands.spawn((hole, world)).id(),
            };
            if let Some(oldest) = push_bounded(&mut assets.holes, entity, MAX_HOLES) {
                if let Ok(mut old) = commands.get_entity(oldest) {
                    old.despawn();
                }
            }
            stats.holes += 1;
        }

        // Chips of dirt, splinters of the target, or drops of water.
        let (count, material) = match impact.surface {
            Surface::Ground => {
                let sandy = impact.position.y < 2.5;
                let material = if sandy { assets.sand.clone() } else { assets.soil[(rng.next() * assets.soil.len() as f32) as usize % assets.soil.len()].clone() };
                (rng.range(9.0, 15.0) as usize, material)
            }
            Surface::Target => (rng.range(5.0, 9.0) as usize, assets.splinter.clone()),
            Surface::Water => (rng.range(8.0, 13.0) as usize, assets.droplet.clone()),
        };
        // A faster bullet throws things harder (a bullet from far off lands slowly).
        let energy = (impact.speed / 350.0).clamp(0.4, 1.2);
        for chip in chips(impact.normal, energy, count, &mut rng) {
            if chip_count >= MAX_CHIPS {
                break;
            }
            chip_count += 1;
            stats.chips += 1;
            commands.spawn((
                Chipping { velocity: chip.velocity, life: chip.life, total: chip.life, size: chip.size },
                Mesh3d(assets.chip_mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(impact.position + impact.normal * 0.02)
                    .with_scale(Vec3::splat(chip.size))
                    .with_rotation(Quat::from_euler(EulerRot::XYZ, rng.next() * 6.0, rng.next() * 6.0, rng.next() * 6.0)),
                NotShadowCaster,
            ));
        }

        // A puff of dust (or mist off the water) that swells, drifts with the wind and thins out.
        if impact.surface != Surface::Target || rng.next() < 0.5 {
            let colour = match impact.surface {
                Surface::Water => Color::srgba(0.85, 0.9, 0.95, 0.35),
                _ if impact.position.y < 2.5 => Color::srgba(0.75, 0.68, 0.52, 0.45),
                _ => Color::srgba(0.45, 0.38, 0.3, 0.45),
            };
            for _ in 0..(if impact.surface == Surface::Target { 1 } else { 3 }) {
                if dust_count >= MAX_DUST {
                    break;
                }
                dust_count += 1;
                stats.puffs += 1;
                let material = materials.add(StandardMaterial { base_color: colour, alpha_mode: AlphaMode::Blend, unlit: false, perceptual_roughness: 1.0, ..default() });
                let total = rng.range(0.7, 1.3);
                let size = rng.range(0.07, 0.12);
                commands.spawn((
                    Dust {
                        velocity: impact.normal * rng.range(0.4, 1.0) + breeze * 0.25 + Vec3::new(rng.range(-0.3, 0.3), 0.0, rng.range(-0.3, 0.3)),
                        life: total,
                        total,
                        size,
                        opacity: colour.alpha(),
                        material: material.clone(),
                    },
                    Mesh3d(assets.dust_mesh.clone()),
                    MeshMaterial3d(material),
                    Transform::from_translation(impact.position + impact.normal * 0.05).with_scale(Vec3::splat(size)),
                    NotShadowCaster,
                ));
            }
        }

        // The thump, placed where it happened, and arriving after the time sound takes to travel.
        let (sounds, speed, volume) = match impact.surface {
            Surface::Target => (&assets.target_sounds, rng.range(0.92, 1.08), 1.0),
            Surface::Ground => (&assets.dirt_sounds, rng.range(0.88, 1.1), 1.0),
            // Water: the dirt thump, lower and softer.
            Surface::Water => (&assets.dirt_sounds, rng.range(0.6, 0.72), 0.55),
        };
        if !sounds.is_empty() {
            let pick = sounds[(rng.next() * sounds.len() as f32) as usize % sounds.len()].clone();
            let distance = listener.single().map_or(0.0, |ear| ear.translation.distance(impact.position));
            play_after(&mut commands, arrival_delay(distance), pick, speed, volume, Some(impact.position));
            stats.thumps += 1;
        }
        assets.rng = rng;
    }
}

const CHIP_GRAVITY: f32 = 9.8;
const CHIP_DRAG: f32 = 1.6;

fn update_chips(
    mut commands: Commands,
    time: Res<Time>,
    map: Res<TerrainMap>,
    mut chips: Query<(Entity, &mut Transform, &mut Chipping)>,
) {
    let dt = time.delta_secs();
    for (entity, mut transform, mut chip) in &mut chips {
        chip.life -= dt;
        if chip.life <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        chip.velocity.y -= CHIP_GRAVITY * dt;
        chip.velocity *= (-CHIP_DRAG * dt).exp();
        transform.translation += chip.velocity * dt;
        // It tumbles as it flies.
        transform.rotate_local(Quat::from_euler(EulerRot::XYZ, 7.0 * dt, 5.0 * dt, 3.0 * dt));
        let ground = map.surface_height_at(Vec2::new(transform.translation.x, transform.translation.z));
        if transform.translation.y < ground + 0.01 && chip.velocity.y < 0.0 {
            // It lands and stops where it fell, to shrink away with the rest.
            transform.translation.y = ground + 0.01;
            chip.velocity = Vec3::ZERO;
        }
        // The last third of its life it shrinks to nothing.
        let shrink = (chip.life / (chip.total * 0.35)).clamp(0.0, 1.0);
        transform.scale = Vec3::splat(chip.size * shrink);
    }
}

fn update_dust(
    mut commands: Commands,
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut puffs: Query<(Entity, &mut Transform, &mut Dust)>,
) {
    let dt = time.delta_secs();
    for (entity, mut transform, mut dust) in &mut puffs {
        dust.life -= dt;
        if dust.life <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        let age = 1.0 - dust.life / dust.total;
        transform.translation += dust.velocity * dt;
        dust.velocity *= (-1.5 * dt).exp();
        transform.scale = Vec3::splat(dust.size * (1.0 + 4.0 * age));
        if let Some(mut material) = materials.get_mut(&dust.material) {
            material.base_color = material.base_color.with_alpha(dust.opacity * (1.0 - age).powf(1.5));
        }
    }
}

pub struct ImpactPlugin;

impl Plugin for ImpactPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Impact>()
            .init_resource::<ImpactStats>()
            .init_resource::<Wind>()
            .add_systems(Startup, load_assets)
            .add_systems(Update, (spawn_impacts, update_chips, update_dust).chain());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat() -> TerrainMap {
        TerrainMap::flat(0.0)
    }

    #[test]
    fn a_falling_path_lands_on_flat_ground_and_faces_up() {
        let (point, normal, surface) = surface_hit(&flat(), Vec3::new(3.0, 0.3, 4.0), Vec3::new(3.0, -0.5, 4.0)).expect("lands");
        assert_eq!(surface, Surface::Ground);
        assert!(point.y.abs() < 0.001 && (point.x - 3.0).abs() < 0.001, "{point:?}");
        assert!((normal - Vec3::Y).length() < 0.001);
    }

    #[test]
    fn a_shallow_path_lands_where_it_crosses_the_ground_not_where_it_ends() {
        let (point, _, _) = surface_hit(&flat(), Vec3::new(0.0, 0.2, 0.0), Vec3::new(10.0, -0.2, 0.0)).expect("lands");
        assert!((point.x - 5.0).abs() < 0.01, "crossed at {}", point.x);
    }

    #[test]
    fn a_path_that_falls_into_the_sea_lands_on_the_water_not_the_sea_floor() {
        // Land at -5 m is under the sea, which stands at 0.
        let sea = TerrainMap::flat(-5.0);
        let (point, normal, surface) = surface_hit(&sea, Vec3::new(2.0, 1.0, 2.0), Vec3::new(2.0, -1.0, 2.0)).expect("lands");
        assert_eq!(surface, Surface::Water);
        assert!(point.y.abs() < 0.001, "on the surface, not at the bottom: {point:?}");
        assert_eq!(normal, Vec3::Y);
    }

    #[test]
    fn a_path_that_stays_above_the_ground_does_not_land() {
        assert!(surface_hit(&flat(), Vec3::new(0.0, 2.0, 0.0), Vec3::new(10.0, 1.0, 0.0)).is_none());
    }

    #[test]
    fn a_path_that_starts_underground_is_not_a_landing() {
        assert!(surface_hit(&flat(), Vec3::new(0.0, -1.0, 0.0), Vec3::new(1.0, -2.0, 0.0)).is_none());
    }

    #[test]
    fn the_ground_faces_up_on_the_flat() {
        assert!((terrain_normal(&flat(), Vec2::new(3.0, 3.0)) - Vec3::Y).length() < 1e-4);
    }

    #[test]
    fn a_box_is_entered_through_the_face_that_was_hit() {
        let (min, max) = (Vec3::new(-1.0, 0.0, -1.0), Vec3::new(1.0, 2.0, 1.0));
        let (t, normal) = segment_aabb_hit(Vec3::new(0.0, 1.0, 5.0), Vec3::new(0.0, 1.0, -5.0), min, max).expect("hits");
        assert!((t - 0.4).abs() < 1e-4, "t {t}");
        assert_eq!(normal, Vec3::Z, "came from +Z, so hit the +Z face");
        let (_, normal) = segment_aabb_hit(Vec3::new(-5.0, 1.0, 0.0), Vec3::new(5.0, 1.0, 0.0), min, max).expect("hits");
        assert_eq!(normal, Vec3::NEG_X);
        let (_, normal) = segment_aabb_hit(Vec3::new(0.0, 5.0, 0.0), Vec3::new(0.0, -5.0, 0.0), min, max).expect("hits");
        assert_eq!(normal, Vec3::Y);
    }

    #[test]
    fn a_segment_that_misses_or_stops_short_of_a_box_does_not_hit_it() {
        let (min, max) = (Vec3::new(-1.0, 0.0, -1.0), Vec3::new(1.0, 2.0, 1.0));
        assert!(segment_aabb_hit(Vec3::new(5.0, 1.0, 5.0), Vec3::new(5.0, 1.0, -5.0), min, max).is_none());
        assert!(segment_aabb_hit(Vec3::new(0.0, 1.0, 5.0), Vec3::new(0.0, 1.0, 3.0), min, max).is_none());
    }

    #[test]
    fn a_segment_that_starts_inside_a_box_hits_at_once() {
        let (min, max) = (Vec3::new(-1.0, 0.0, -1.0), Vec3::new(1.0, 2.0, 1.0));
        let (t, _) = segment_aabb_hit(Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 1.0, -5.0), min, max).expect("hits");
        assert_eq!(t, 0.0);
    }

    #[test]
    fn chips_fly_out_of_the_surface_and_not_into_it() {
        let mut rng = Rng(12345);
        for normal in [Vec3::Y, Vec3::Z, Vec3::new(1.0, 1.0, 0.0).normalize()] {
            for chip in chips(normal, 1.0, 60, &mut rng) {
                assert!(chip.velocity.dot(normal) > 0.5, "chip going into the surface: {:?}", chip.velocity);
                assert!((0.5..=1.1).contains(&chip.life) && chip.size > 0.01 && chip.size < 0.05);
            }
        }
    }

    #[test]
    fn chips_are_mostly_slow_and_some_are_fast() {
        let mut rng = Rng(777);
        let speeds: Vec<f32> = chips(Vec3::Y, 1.0, 400, &mut rng).iter().map(|c| c.velocity.length()).collect();
        let slow = speeds.iter().filter(|&&s| s < 3.0).count();
        assert!(slow > speeds.len() / 2, "{slow} of {} slow", speeds.len());
        assert!(speeds.iter().any(|&s| s > 5.0), "and a few fast ones");
        assert!(speeds.iter().all(|&s| s > 1.0 && s < 7.0));
    }

    #[test]
    fn a_harder_hit_throws_chips_faster() {
        let mean = |energy| {
            let mut rng = Rng(99);
            chips(Vec3::Y, energy, 300, &mut rng).iter().map(|c| c.velocity.length()).sum::<f32>() / 300.0
        };
        assert!(mean(1.2) > mean(0.5) * 1.8);
    }

    #[test]
    fn the_generator_gives_numbers_between_zero_and_one() {
        let mut rng = Rng(1);
        let numbers: Vec<f32> = (0..1000).map(|_| rng.next()).collect();
        assert!(numbers.iter().all(|&n| (0.0..1.0).contains(&n)));
        let mean = numbers.iter().sum::<f32>() / 1000.0;
        assert!((0.4..0.6).contains(&mean), "mean {mean}");
    }

    #[test]
    fn a_hole_lies_flat_against_the_surface_it_is_on() {
        for normal in [Vec3::Y, Vec3::X, Vec3::new(0.3, 1.0, -0.2).normalize()] {
            let rotation = lay_flat(normal, 1.3);
            assert!((rotation * Vec3::Z - normal).length() < 1e-4, "faces {normal:?}");
        }
    }

    #[test]
    fn the_oldest_holes_go_first_when_there_are_too_many() {
        let mut queue = VecDeque::new();
        let ids: Vec<Entity> = (0..5).map(Entity::from_raw_u32).map(|e| e.expect("entity")).collect();
        assert_eq!(push_bounded(&mut queue, ids[0], 3), None);
        assert_eq!(push_bounded(&mut queue, ids[1], 3), None);
        assert_eq!(push_bounded(&mut queue, ids[2], 3), None);
        assert_eq!(push_bounded(&mut queue, ids[3], 3), Some(ids[0]));
        assert_eq!(push_bounded(&mut queue, ids[4], 3), Some(ids[1]));
        assert_eq!(queue.len(), 3);
    }

    #[test]
    fn the_bullet_hole_picture_has_a_dark_centre_and_clear_edges() {
        let image = hole_image();
        let data = image.data.as_ref().expect("pixels");
        let pixel = |x: usize, y: usize| &data[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4];
        let centre = pixel(32, 32);
        assert!(centre[0] < 30 && centre[3] == 255, "dark and solid in the middle: {centre:?}");
        assert_eq!(pixel(0, 0)[3], 0, "clear at the corners");
        assert_eq!(pixel(63, 31)[3], 0, "and at the edges");
        let ring = pixel(32 + 12, 32);
        assert!(ring[3] > 60, "churned earth round the hole: {ring:?}");
    }
}

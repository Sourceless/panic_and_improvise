use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use crate::gun_model::{self, BORE_Y, MUZZLE_Z, REAR_PEEP_Z, SIGHT_LINE};
use crate::ballistics::{self, Cartridge, Flight, NINE_PARA};
use crate::collision::Colliders;
use crate::controls::{Action, Controls, Keyboard};
use crate::gun_state::{Bolt, Mechanism, State, STERLING};
use crate::impact::{segment_aabb_hit, surface_hit, Impact, Rng, Surface};
use crate::sound::play_after;
use crate::wind::Wind;
use crate::player::{spawn_player, toggle_cursor_grab, AimBlend, FpsCamera, Stance};
pub use crate::gun_model::sight_points;
use crate::map::TerrainMap;
use crate::target::{dummy_aabb, TargetDummy};

/// Things the gun's mechanism does that make a sound.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GunEvent {
    /// A reload begins; `charge` if the bolt had to be hauled back as well.
    ReloadStarted { charge: bool },
    /// The trigger is pulled and nothing happens.
    DryClick,
    /// The trigger is pulled on an empty open-bolt gun: the bolt slams forward on nothing.
    BoltDrop,
}

/// How many of each gun sound have been made (for tests).
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct SoundLog {
    pub reloads: u32,
    pub charged_reloads: u32,
    pub dry_clicks: u32,
    pub bolt_drops: u32,
}

#[derive(Resource)]
pub struct GunSounds {
    pub reload_swap: Handle<AudioSource>,
    pub reload_charge: Handle<AudioSource>,
    pub dry_clicks: Vec<Handle<AudioSource>>,
}

impl GunSounds {
    pub fn all(&self) -> impl Iterator<Item = &Handle<AudioSource>> {
        [&self.reload_swap, &self.reload_charge].into_iter().chain(&self.dry_clicks)
    }
}

/// An empty click comes this long after the trigger is pulled (the action takes a moment), and
/// there is at least this long between one empty click and the next, so that mashing the trigger on
/// an empty gun doesn't rattle.
const DRY_CLICK_DELAY: f32 = 0.15;
const DRY_CLICK_INTERVAL: f32 = 0.6;
/// How loud the empty clicks are: quiet, next to a shot.
const DRY_CLICK_VOLUME: f32 = 0.3;
const BOLT_DROP_VOLUME: f32 = 0.4;

/// The gun's mechanism: bolt type and timings (rate of fire, how long a reload takes).
pub const MECHANISM: Mechanism = STERLING;
/// Rounds in a full magazine.
pub const MAGAZINE_SIZE: u32 = 30;
/// Seconds the gun takes to drop away at the start of a reload, and again to come back up at the end.
const RELOAD_LOWER_TIME: f32 = 0.45;
/// How far the gun drops (metres) and tips muzzle-down (radians) at the bottom of a reload: far
/// enough to be right off the bottom of the screen.
const RELOAD_DROP: f32 = 0.6;
const RELOAD_TIP: f32 = -0.6;
/// A bullet that hasn't hit anything by now is long gone.
const BULLET_LIFETIME: f32 = 5.0;
/// The range the Sterling's sights are zeroed for: a shot crosses the line of sight here.
pub const ZERO_DISTANCE: f32 = 20.0;
/// How finely a bullet's flight is stepped, seconds (a frame is cut into steps this long or shorter).
const FLIGHT_STEP: f32 = 0.002;
/// The ammunition the gun fires.
pub const CARTRIDGE: Cartridge = NINE_PARA;
const BULLET_DAMAGE: f32 = 25.0;
/// Tracers appear after the bullet has flown this far (metres), and are this long.
const TRACER_START: f32 = 15.0;
const TRACER_LENGTH: f32 = 3.0;
/// The tracer's thickness at the muzzle, metres.
const TRACER_WIDTH: f32 = 0.04;

/// Where the gun sits in camera space when carried at the hip, and when aimed down its sights.
const HIP_POSITION: Vec3 = Vec3::new(0.2, -0.2, -0.5);
/// Centred, and low enough that the line of sight (rear peep to front blade) is the camera's axis.
/// Eye relief: how far the eye is behind the rear peep when aiming (a real peep sight is held
/// close, about this far, with the stock passing behind the eye).
const EYE_RELIEF: f32 = 0.2;
const ADS_POSITION: Vec3 = Vec3::new(0.0, -SIGHT_LINE, -EYE_RELIEF - REAR_PEEP_Z);
/// The barrel's tip, in the gun's own space: the bullets start here.
pub const MUZZLE_LOCAL: Vec3 = Vec3::new(0.0, BORE_Y, MUZZLE_Z);
/// How fast the gun moves between hip and sights (per second, exponential).
const AIM_RATE: f32 = 14.0;
/// When aimed the view zooms to this fraction of the field of view, and look speed drops.
pub const ADS_FOV_SCALE: f32 = 0.72;
/// Recoil. Each shot kicks the view up (and a little sideways, at random), by this much in
/// radians, less when braced on the sights...
const KICK_PITCH: f32 = 0.0085;
const KICK_YAW: f32 = 0.0028;
const ADS_KICK_SCALE: f32 = 0.6;
/// ...and once the gun has been quiet this long, a good part of the climb comes back down:
/// this fraction of it, at this rate (per second). The rest is yours to pull down.
const KICK_RECOVERY_DELAY: f32 = 0.14;
const KICK_RECOVERY_FRACTION: f32 = 0.65;
const KICK_RECOVERY_RATE: f32 = 6.0;
/// The gun model itself jolts back and tips up with each shot, then settles quickly.
const GUN_KICK_DECAY: f32 = 16.0;
const GUN_KICK_BACK: f32 = 0.032;
const GUN_KICK_TIP: f32 = 0.045;
/// Accuracy. A shot strays from where the gun is pointed by up to this half-angle (radians),
/// anywhere inside the cone: wide from the hip, very tight on the sights. It widens while
/// moving, much more in the air, and as a burst "blooms" it.
const HIP_SPREAD: f32 = 0.030; // about 1.7 degrees
const ADS_SPREAD: f32 = 0.0035; // about 0.2 degrees
const MOVING_SPREAD: f32 = 0.022; // extra at a full run
const AIRBORNE_SPREAD: f32 = 0.035; // extra in the air
const BLOOM_PER_SHOT: f32 = 0.0045;
const BLOOM_MAX: f32 = 0.03;
const BLOOM_DECAY: f32 = 5.5; // per second
/// How much of the movement and bloom penalty is left when fully on the sights.
const ADS_PENALTY_LEFT: f32 = 0.25;

pub struct WeaponPlugin;

impl Plugin for WeaponPlugin {
    fn build(&self, app: &mut App) {
        // The wind that bullets drift in (the terrain plugin's, when that is loaded too).
        app.init_resource::<Wind>()
            .add_systems(Startup, spawn_gun.after(spawn_player))
            .add_systems(
                Update,
                (aim.before(fire), fire.before(toggle_cursor_grab), play_gun_sounds.after(fire), recover_view, animate_flash, move_bullets),
            );
    }
}

#[derive(Component)]
pub struct Gun {
    /// Where the gun's mechanism is: ready, cycling a shot, dry, or being reloaded.
    pub state: State,
    trigger_blocked: bool,
    pub shots_fired: u32,
    /// Whether the player has toggled aiming down the sights.
    pub aiming: bool,
    /// Where the most recent bullet started, in world space.
    pub last_shot_origin: Option<Vec3>,
    /// 0 at the hip, 1 fully on the sights, easing between.
    pub aim_blend: f32,
    /// How far the view has been kicked up and across by recoil and not yet recovered (radians).
    pub view_kick: Vec2,
    /// Seconds since the last shot.
    pub since_shot: f32,
    /// Extra inaccuracy from firing in a burst; builds with each shot, fades when you stop.
    pub bloom: f32,
    /// Seconds left of the muzzle flash, and the twist and size this shot's flash was given.
    pub flash_time: f32,
    pub flash_roll: f32,
    pub flash_size: f32,
    /// Where each shot goes is random; this is the generator's state.
    rng: u32,
    /// How far (radians) the latest shot strayed from where the gun was pointed, and the worst so far.
    pub last_shot_error: f32,
    pub worst_shot_error: f32,
    /// The cone shots would land in if fired this instant (radians); the crosshair shows it.
    pub current_spread: f32,
    /// The gun model's jolt, 1 right after a shot, decaying to 0.
    pub gun_kick: f32,
    /// Rounds left in the magazine.
    pub ammo: u32,
    /// The reload key was pressed mid-shot; the reload starts as soon as the shot has cycled.
    reload_queued: bool,
    /// Sounds the mechanism has made since they were last played.
    pending_sounds: Vec<GunEvent>,
    /// Seconds before another empty click is allowed.
    dry_click_cooldown: f32,
    pub sound_log: SoundLog,
    /// How far the gun is lowered for the reload right now, 0 to 1.
    pub lowered: f32,
}

impl Gun {
    pub fn reloading(&self) -> bool {
        self.state.is_reloading()
    }

    /// Acts on a reload request, as soon as the mechanism is free to (not mid-shot). A request the
    /// gun can't use at all (a full magazine, a reload already going) is dropped.
    fn try_queued_reload(&mut self) {
        if !self.reload_queued || matches!(self.state, State::Cycling { .. }) {
            return;
        }
        self.reload_queued = false;
        let was_reloading = self.reloading();
        self.state = MECHANISM.press_reload(self.state, self.ammo, MAGAZINE_SIZE);
        if !was_reloading && self.reloading() {
            let charge = self.reload_seconds() > MECHANISM.reload_time + 0.01;
            self.pending_sounds.push(GunEvent::ReloadStarted { charge });
        }
    }

    /// Where the bolt is, when the gun is at rest.
    pub fn bolt(&self) -> Option<Bolt> {
        MECHANISM.bolt(self.state)
    }

    /// How long the current reload will take, or 0 when not reloading.
    pub fn reload_seconds(&self) -> f32 {
        match self.state {
            State::Reloading { total, .. } => total,
            _ => 0.0,
        }
    }

    /// How far down the gun is for the reload, 0 to 1.
    fn lowering(&self) -> f32 {
        match self.state {
            State::Reloading { elapsed, total } => reload_lowering(elapsed, total),
            _ => 0.0,
        }
    }
}

/// The gun's inaccuracy right now: how far a shot could stray (radians, half-angle of the cone),
/// given how far it's on the sights, how the player is moving and standing, and the burst bloom.
pub fn current_spread(gun: &Gun, player: &FpsCamera) -> f32 {
    spread_half_angle(gun.aim_blend, player.speed() / FULL_RUN_SPEED, player.airborne(), gun.bloom, player.stance())
}

impl Gun {
    /// The next random number in 0..1 (a small xorshift generator: plenty for scatter).
    fn random(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng >> 8) as f32 / (1u32 << 24) as f32
    }
}

/// How fast the player runs flat out (the sprint speed), which counts as "moving fully" for
/// accuracy purposes.
const FULL_RUN_SPEED: f32 = 10.8;

/// The gun's transform in camera space, `blend` of the way from the hip to the sights.
pub fn gun_transform(blend: f32) -> Transform {
    Transform::from_translation(HIP_POSITION.lerp(ADS_POSITION, blend.clamp(0.0, 1.0)))
}

/// The gun with its recoil jolt applied: shoved back toward the shoulder and tipped muzzle-up,
/// about half as much when braced on the sights.
pub fn gun_pose(blend: f32, kick: f32) -> Transform {
    gun_pose_lowered(blend, kick, 0.0)
}

/// The gun's pose with `lowered` (0 to 1) of the way down for a reload: dropped away below the view
/// and tipped muzzle-down.
pub fn gun_pose_lowered(blend: f32, kick: f32, lowered: f32) -> Transform {
    let k = kick * (1.0 - 0.5 * blend.clamp(0.0, 1.0));
    let mut t = gun_transform(blend);
    t.translation.z += GUN_KICK_BACK * k;
    t.rotation = Quat::from_rotation_x(GUN_KICK_TIP * k + RELOAD_TIP * lowered);
    t.translation.y -= RELOAD_DROP * lowered;
    t
}

/// How far down the gun is, 0 to 1, `elapsed` seconds into a reload of `total`: it drops away, stays
/// down while the magazine is changed (and the bolt charged, if it needs it), then comes back up.
pub fn reload_lowering(elapsed: f32, total: f32) -> f32 {
    let smooth = |x: f32| x.clamp(0.0, 1.0).powi(2) * (3.0 - 2.0 * x.clamp(0.0, 1.0));
    smooth(elapsed / RELOAD_LOWER_TIME).min(smooth((total - elapsed) / RELOAD_LOWER_TIME))
}

/// How much one shot kicks the view: (pitch up, yaw), in radians. The sideways part is a fixed
/// pseudo-random wander per shot number, so it is repeatable.
pub fn recoil_kick(shot: u32, aiming: bool) -> Vec2 {
    let scale = if aiming { ADS_KICK_SCALE } else { 1.0 };
    let mut h = shot.wrapping_mul(0x9E37_79B1) ^ 0x85EB_CA6B;
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    let wander = ((h >> 8) as f32 / (1u32 << 24) as f32) * 2.0 - 1.0;
    // A little variation in the climb too.
    let climb = 0.85 + 0.3 * (((h >> 3) & 0xFF) as f32 / 255.0);
    Vec2::new(KICK_PITCH * climb, KICK_YAW * wander) * scale
}

/// How much of the unrecovered kick comes back in `dt` seconds, once the gun has been quiet.
/// Returns (the part of the kick to remove from the bookkeeping, the part to bring the view back by).
pub fn recover_kick(kick: Vec2, since_shot: f32, dt: f32) -> (Vec2, Vec2) {
    if since_shot < KICK_RECOVERY_DELAY {
        return (Vec2::ZERO, Vec2::ZERO);
    }
    let settled = kick * (1.0 - (-KICK_RECOVERY_RATE * dt).exp());
    (settled, settled * KICK_RECOVERY_FRACTION)
}

/// How far a shot may stray from where the gun is pointed (the half-angle of the cone it lands
/// in, in radians). `blend` is how far the gun is on its sights, `speed` how fast the player
/// moves as a fraction of a full run, `bloom` the burst penalty built up so far, and `stance`
/// whether they're standing, crouched or prone (each steadier than the last).
pub fn spread_half_angle(blend: f32, speed: f32, airborne: bool, bloom: f32, stance: Stance) -> f32 {
    let blend = blend.clamp(0.0, 1.0);
    let base = HIP_SPREAD + (ADS_SPREAD - HIP_SPREAD) * blend;
    let penalties = MOVING_SPREAD * speed.clamp(0.0, 1.0) + if airborne { AIRBORNE_SPREAD } else { 0.0 } + bloom.clamp(0.0, BLOOM_MAX);
    (base + penalties * (1.0 - (1.0 - ADS_PENALTY_LEFT) * blend)) * stance.spread_scale()
}

/// A direction picked uniformly inside the cone of half-angle `half_angle` around `aim`, from two
/// random numbers `u` and `v` in 0..1.
pub fn scatter_direction(aim: Vec3, half_angle: f32, u: f32, v: f32) -> Vec3 {
    let right = aim.cross(Vec3::Y).try_normalize().unwrap_or(Vec3::X);
    let up = right.cross(aim);
    // sqrt(u) spreads the shots evenly over the disc rather than bunching them in the middle.
    let (radius, theta) = (half_angle.tan() * u.sqrt(), v * std::f32::consts::TAU);
    (aim + right * (radius * theta.cos()) + up * (radius * theta.sin())).normalize()
}

#[derive(Component)]
pub struct Bullet {
    velocity: Vec3,
    age: f32,
    /// How far it has flown, metres. The tracer isn't drawn until it's flown a way.
    travelled: f32,
    /// Whether it is in foliage right now (so that a burst of leaves is made when it goes in, and not
    /// again every step it is in there).
    in_foliage: bool,
}

impl Bullet {
    /// How far the bullet has flown, metres.
    pub fn travelled(&self) -> f32 {
        self.travelled
    }

    /// Its velocity, m/s.
    pub fn velocity(&self) -> Vec3 {
        self.velocity
    }

    /// How fast the bullet is going, m/s.
    pub fn speed(&self) -> f32 {
        self.velocity.length()
    }
}

/// How much to scale the tracer streak once the bullet has flown `distance` metres. A streak of
/// fixed size shrinks to nothing as it flies away; growing it with distance keeps it about as big
/// on the screen, so the arc of the fall can be followed all the way out.
pub fn tracer_scale(distance: f32) -> Vec3 {
    let width = (1.0 + 0.05 * distance).min(10.0);
    let length = (1.0 + 0.02 * distance).min(5.0);
    Vec3::new(width, width, length)
}

/// Whether a bullet that has flown `distance` metres shows its tracer yet. Right at the muzzle a
/// glowing streak is just a distraction in your face (and spoils the sight picture), so it only
/// appears once the bullet is well on its way.
pub fn tracer_visible(distance: f32) -> bool {
    distance >= TRACER_START
}

#[derive(Resource)]
pub struct BulletAssets {
    pub mesh: Handle<Mesh>,
    pub material: Handle<StandardMaterial>,
    /// The first shot recording, kept for anything that just needs one to check loading.
    pub shot_sound: Handle<AudioSource>,
    /// All the recordings; each shot picks one, at a slightly different pitch.
    pub shot_sounds: Vec<Handle<AudioSource>>,
}

pub fn spawn_gun(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
    cameras: Query<Entity, With<FpsCamera>>,
) {
    let Ok(cam) = cameras.single() else {
        return;
    };

    // The Sterling, merged into three meshes by material (see gun_model.rs).
    let (metal, plastic, dark) = gun_model::build();
    let finish = |color: Color, roughness: f32, metallic: f32| StandardMaterial {
        base_color: color,
        perceptual_roughness: roughness,
        metallic,
        ..default()
    };
    let parts = [
        (meshes.add(metal.into_mesh()), materials.add(finish(Color::srgb(0.13, 0.135, 0.145), 0.45, 0.75))),
        (meshes.add(plastic.into_mesh()), materials.add(finish(Color::srgb(0.035, 0.035, 0.04), 0.35, 0.0))),
        (meshes.add(dark.into_mesh()), materials.add(finish(Color::srgb(0.008, 0.008, 0.01), 0.9, 0.0))),
    ];

    // The shot sounds: real recordings of a 9 mm Carl Gustav M45 submachine gun. FPS_SHOT_SOUNDS=old
    // plays the original single pistol shot instead, for comparison.
    let shot_sounds: Vec<Handle<AudioSource>> = match std::env::var("FPS_SHOT_SOUNDS").as_deref() {
        Ok("old") => vec![asset_server.load("sounds/gunshots/pistol_shot.wav")],
        Ok(set) if !set.is_empty() => (1..=3).map(|i| asset_server.load(format!("sounds/{set}/smg_shot_{i}.wav"))).collect(),
        _ => (1..=3).map(|i| asset_server.load(format!("sounds/smg/smg_shot_{i}.wav"))).collect(),
    };
    commands.insert_resource(GunSounds {
        reload_swap: asset_server.load("sounds/gun/reload_swap.wav"),
        reload_charge: asset_server.load("sounds/gun/reload_charge.wav"),
        dry_clicks: (1..=2).map(|i| asset_server.load(format!("sounds/gun/dry_click_{i}.wav"))).collect(),
    });
    commands.insert_resource(BulletAssets {
        // A thin streak along the line of flight (the bullet is turned to face the way it goes).
        mesh: meshes.add(Cuboid::new(TRACER_WIDTH, TRACER_WIDTH, TRACER_LENGTH)),
        // Unlit and additive, with HDR values, so it glows (and blooms) against any sky.
        material: materials.add(StandardMaterial {
            base_color: Color::linear_rgb(40.0, 24.0, 7.0),
            unlit: true,
            alpha_mode: AlphaMode::Add,
            ..default()
        }),
        shot_sound: shot_sounds[0].clone(),
        shot_sounds,
    });

    commands.entity(cam).with_children(|cam_children| {
        cam_children
            .spawn((
                Gun {
                    state: State::Ready,
                    trigger_blocked: false,
                    shots_fired: 0,
                    aiming: false,
                    last_shot_origin: None,
                    aim_blend: 0.0,
                    view_kick: Vec2::ZERO,
                    since_shot: 10.0,
                    bloom: 0.0,
                    flash_time: 0.0,
                    flash_roll: 0.0,
                    flash_size: 1.0,
                    rng: 0x9E37_79B9,
                    last_shot_error: 0.0,
                    worst_shot_error: 0.0,
                    current_spread: HIP_SPREAD,
                    gun_kick: 0.0,
                    ammo: MAGAZINE_SIZE,
                    reload_queued: false,
                    pending_sounds: Vec::new(),
                    dry_click_cooldown: 0.0,
                    sound_log: SoundLog::default(),
                    lowered: 0.0,
                },
                gun_transform(0.0),
                Visibility::default(),
            ))
            .with_children(|gun| {
                for (mesh, material) in parts {
                    gun.spawn((Mesh3d(mesh), MeshMaterial3d(material), Transform::default()));
                }
                crate::muzzle_flash::spawn(gun, &mut meshes, &mut materials, MUZZLE_LOCAL, false);
            });
    });
}

// Right click toggles aiming down the sights (only while the mouse is captured, so the click
// that recaptures it doesn't also raise the gun). Sprinting drops the gun back to the hip. The
// gun then eases toward its target, and the blend is shared (as AimBlend) so the camera can
// zoom and the player slow down in step with it.
fn aim(
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<Keyboard>,
    controls: Res<Controls>,
    cursors: Query<&CursorOptions, With<PrimaryWindow>>,
    mut guns: Query<(&mut Gun, &mut Transform)>,
    player: Query<&FpsCamera>,
    mut blend: ResMut<AimBlend>,
) {
    let Ok((mut gun, mut transform)) = guns.single_mut() else {
        return;
    };
    let captured = cursors.single().is_ok_and(|c| c.grab_mode == CursorGrabMode::Locked);
    // Reloading is always something the player asks for: nothing reloads by itself.
    if captured && controls.just_pressed(Action::Reload, &keys) {
        gun.reload_queued = true;
    }
    gun.try_queued_reload();
    gun.lowered = gun.lowering();
    // The sights come down for a reload, and can't be raised again until it's done.
    if captured && mouse.just_pressed(MouseButton::Right) && !gun.reloading() {
        gun.aiming = !gun.aiming;
    }
    if gun.reloading() {
        gun.aiming = false;
    }
    if controls.pressed(Action::Sprint, &keys) && controls.pressed(Action::Forward, &keys) {
        gun.aiming = false;
    }
    if !captured {
        gun.aiming = false;
    }
    let target = if gun.aiming { 1.0 } else { 0.0 };
    gun.aim_blend += (target - gun.aim_blend) * (1.0 - (-AIM_RATE * time.delta_secs()).exp());
    if (gun.aim_blend - target).abs() < 0.002 {
        gun.aim_blend = target;
    }
    gun.bloom *= (-BLOOM_DECAY * time.delta_secs()).exp();
    gun.flash_time = (gun.flash_time - time.delta_secs()).max(0.0);
    gun.gun_kick *= (-GUN_KICK_DECAY * time.delta_secs()).exp();
    if gun.gun_kick < 0.002 {
        gun.gun_kick = 0.0;
    }
    *transform = gun_pose_lowered(gun.aim_blend, gun.gun_kick, gun.lowered);
    blend.0 = gun.aim_blend;
    if let Ok(player) = player.single() {
        gun.current_spread = current_spread(&gun, player);
    }
}

fn fire(
    mut commands: Commands,
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    cursors: Query<&CursorOptions, With<PrimaryWindow>>,
    mut camera: Query<(&mut Transform, &mut FpsCamera)>,
    mut guns: Query<&mut Gun>,
    assets: Res<BulletAssets>,
) {
    let Ok(mut gun) = guns.single_mut() else {
        return;
    };
    // The mechanism moves on with time: a shot finishes cycling, a reload finishes.
    let (state, reload_done) = MECHANISM.tick(gun.state, gun.ammo, mouse.pressed(MouseButton::Left), time.delta_secs());
    gun.state = state;
    if reload_done {
        gun.ammo = MAGAZINE_SIZE;
    }
    gun.try_queued_reload();
    gun.since_shot += time.delta_secs();
    gun.dry_click_cooldown = (gun.dry_click_cooldown - time.delta_secs()).max(0.0);

    if !mouse.pressed(MouseButton::Left) {
        gun.trigger_blocked = false;
        return;
    }
    let Ok(cursor) = cursors.single() else {
        return;
    };
    if cursor.grab_mode != CursorGrabMode::Locked {
        // The click that recaptures the mouse must not fire.
        gun.trigger_blocked = true;
        return;
    }
    if gun.trigger_blocked {
        return;
    }
    let Ok((mut cam, mut view)) = camera.single_mut() else {
        return;
    };
    // The trigger: the mechanism decides whether that fires a round (it won't while cycling,
    // reloading or dry, and on an empty open-bolt gun it just lets the bolt go forward).
    let before = gun.state;
    let (state, fires) = MECHANISM.pull_trigger(gun.state, gun.ammo);
    gun.state = state;
    if !fires {
        let clicked = if before == State::Ready && state == State::Dry {
            Some(GunEvent::BoltDrop)
        } else if before == State::Dry && mouse.just_pressed(MouseButton::Left) {
            Some(GunEvent::DryClick)
        } else {
            None
        };
        if let Some(event) = clicked.filter(|_| gun.dry_click_cooldown == 0.0) {
            gun.pending_sounds.push(event);
            gun.dry_click_cooldown = DRY_CLICK_INTERVAL;
        }
        return;
    }

    // The bullet leaves the end of the barrel, wherever the gun is right now (hip or sights).
    let gun_in_world = cam.mul_transform(gun_pose(gun.aim_blend, gun.gun_kick));
    let muzzle = gun_in_world.transform_point(MUZZLE_LOCAL);
    let (eye, forward) = (cam.translation, *cam.forward());
    // The barrel points where the bullet must start to cross the line of sight at the zero range.
    let direction = ballistics::zeroed_direction(&CARTRIDGE, muzzle, eye, forward, ZERO_DISTANCE);

    // Where the shot actually goes: anywhere inside the gun's cone of inaccuracy.
    let half_angle = current_spread(&gun, &view);
    let (u, v) = (gun.random(), gun.random());
    let direction_shot = scatter_direction(direction, half_angle, u, v);
    let error = direction.dot(direction_shot).clamp(-1.0, 1.0).acos();
    gun.last_shot_error = error;
    gun.worst_shot_error = gun.worst_shot_error.max(error);
    gun.bloom = (gun.bloom + BLOOM_PER_SHOT).min(BLOOM_MAX);

    gun.shots_fired += 1;
    gun.ammo -= 1;
    gun.last_shot_origin = Some(muzzle);
    gun.since_shot = 0.0;
    // A different recording each time, a little faster or slower, so a burst doesn't sound like
    // one sample on repeat.
    let take = (gun.random() * assets.shot_sounds.len() as f32) as usize % assets.shot_sounds.len();
    let (pitch, loudness) = (0.97 + 0.06 * gun.random(), 0.9 + 0.1 * gun.random());
    commands.spawn((
        AudioPlayer(assets.shot_sounds[take].clone()),
        PlaybackSettings { speed: pitch, volume: bevy::audio::Volume::Linear(loudness), ..PlaybackSettings::DESPAWN },
    ));
    // The muzzle flash, with a new twist and size each time (smaller through the sights, so it
    // doesn't blot out the sight picture).
    gun.flash_time = crate::muzzle_flash::FLASH_TIME;
    gun.flash_roll = gun.random() * std::f32::consts::TAU;
    gun.flash_size = (0.8 + 0.4 * gun.random()) * (1.0 - 0.45 * gun.aim_blend);
    commands.spawn((
        Bullet {
            velocity: direction_shot * CARTRIDGE.muzzle_velocity,
            age: 0.0,
            travelled: 0.0,
            in_foliage: false,
        },
        Mesh3d(assets.mesh.clone()),
        MeshMaterial3d(assets.material.clone()),
        bevy::light::NotShadowCaster,
        // Hidden until it has flown a way (see `tracer_visible`).
        Visibility::Hidden,
        Transform::from_translation(muzzle).looking_to(direction_shot, Vec3::Y),
    ));

    // Recoil: the shot has left; now the gun jolts and the view climbs.
    let kick = recoil_kick(gun.shots_fired, gun.aiming);
    gun.gun_kick = (gun.gun_kick + 1.0).min(1.6);
    gun.view_kick += kick;
    view.nudge(&mut cam, kick.x, kick.y);
}

// Once the gun has been quiet a moment, most of the climb settles back by itself, like the
// muzzle coming back down as the shooter recovers; what's left is for the player to pull down.
// Shows the muzzle flash and its light for the few frames after a shot.
fn animate_flash(
    guns: Query<&Gun>,
    mut flashes: Query<(&mut Transform, &mut Visibility), With<crate::muzzle_flash::MuzzleFlash>>,
    mut lights: Query<&mut PointLight, With<crate::muzzle_flash::MuzzleFlashLight>>,
) {
    let Ok(gun) = guns.single() else { return };
    let (scale, intensity) = crate::muzzle_flash::flash_state(gun.flash_time);
    for (mut transform, mut visibility) in &mut flashes {
        *visibility = if scale > 0.0 { Visibility::Inherited } else { Visibility::Hidden };
        transform.rotation = Quat::from_rotation_z(gun.flash_roll);
        transform.scale = Vec3::splat(scale * gun.flash_size);
    }
    for mut light in &mut lights {
        light.intensity = intensity * gun.flash_size;
    }
}

fn recover_view(time: Res<Time>, mut guns: Query<&mut Gun>, mut camera: Query<(&mut Transform, &mut FpsCamera)>) {
    let (Ok(mut gun), Ok((mut cam, mut view))) = (guns.single_mut(), camera.single_mut()) else {
        return;
    };
    let (settled, bring_back) = recover_kick(gun.view_kick, gun.since_shot, time.delta_secs());
    gun.view_kick -= settled;
    if bring_back != Vec2::ZERO {
        view.nudge(&mut cam, -bring_back.x, -bring_back.y);
    }
}

/// Plays what the mechanism has made noises with since last frame.
fn play_gun_sounds(mut commands: Commands, mut guns: Query<&mut Gun>, sounds: Res<GunSounds>) {
    let Ok(mut gun) = guns.single_mut() else { return };
    for event in std::mem::take(&mut gun.pending_sounds) {
        let roll = gun.random();
        let pitch = |low: f32, high: f32| low + (high - low) * roll;
        match event {
            GunEvent::ReloadStarted { charge } => {
                gun.sound_log.reloads += 1;
                // The magazine comes out once the gun has dropped away from the shoulder.
                let sound = if charge { &sounds.reload_charge } else { &sounds.reload_swap };
                gun.sound_log.charged_reloads += charge as u32;
                // A little lower than the handgun it was recorded from: this is a bigger gun.
                play_after(&mut commands, RELOAD_LOWER_TIME * 0.5, sound.clone(), pitch(0.9, 0.96), 0.9, None);
            }
            GunEvent::DryClick => {
                gun.sound_log.dry_clicks += 1;
                let pick = (roll * sounds.dry_clicks.len() as f32) as usize % sounds.dry_clicks.len();
                play_after(&mut commands, DRY_CLICK_DELAY, sounds.dry_clicks[pick].clone(), pitch(0.95, 1.05), DRY_CLICK_VOLUME, None);
            }
            GunEvent::BoltDrop => {
                gun.sound_log.bolt_drops += 1;
                // The same click, lower: a heavy bolt running home.
                play_after(&mut commands, DRY_CLICK_DELAY, sounds.dry_clicks[0].clone(), pitch(0.62, 0.7), BOLT_DROP_VOLUME, None);
            }
        }
    }
}

fn move_bullets(
    mut commands: Commands,
    time: Res<Time>,
    wind: Res<Wind>,
    mut bullets: Query<(Entity, &mut Transform, &mut Bullet, &mut Visibility)>,
    mut dummies: Query<(Entity, &Transform, &mut TargetDummy), Without<Bullet>>,
    map: Res<TerrainMap>,
    colliders: Option<Res<Colliders>>,
    mut impacts: MessageWriter<Impact>,
) {
    let dt = time.delta_secs();
    // The air moves with the wind, which drags the bullet along with it.
    let air = Vec3::new(wind.direction().x, 0.0, wind.direction().y) * wind.speed;
    // A frame is many metres of flight, so it is stepped in small pieces.
    let steps = (dt / FLIGHT_STEP).ceil().max(1.0) as usize;
    let h = dt / steps as f32;
    for (entity, mut transform, mut bullet, mut visibility) in &mut bullets {
        let mut flight = Flight { position: transform.translation, velocity: bullet.velocity };
        let mut finished = false;
        for _ in 0..steps {
            let start = flight.position;
            flight = ballistics::step(&CARTRIDGE, flight, air, h);
            bullet.age += h;
            bullet.travelled += (flight.position - start).length();
            let speed = flight.velocity.length();

            // What does this little step run into first: the land or water, or a target?
            let mut landing = surface_hit(&map, start, flight.position)
                .map(|(position, normal, surface)| (start.distance(position), Impact { position, normal, surface, speed, target: None }));
            for (target, dummy_transform, dummy) in &dummies {
                if dummy.health <= 0.0 {
                    continue;
                }
                let (min, max) = dummy_aabb(dummy_transform.translation);
                if let Some((t, normal)) = segment_aabb_hit(start, flight.position, min, max) {
                    let distance = t * start.distance(flight.position);
                    if landing.as_ref().is_none_or(|(d, _)| distance < *d) {
                        let position = start.lerp(flight.position, t);
                        landing = Some((distance, Impact { position, normal, surface: Surface::Target, speed, target: Some(target) }));
                    }
                }
            }
            // ...or a tree, a wall, a hedge, a building.
            if let Some(hit) = colliders.as_ref().and_then(|c| c.segment_hit(start, flight.position)) {
                let distance = hit.t * start.distance(flight.position);
                if landing.as_ref().is_none_or(|(d, _)| distance < *d) {
                    landing = Some((distance, Impact { position: hit.point, normal: hit.normal, surface: Surface::Solid(hit.material), speed, target: None }));
                }
            }
            if let Some((_, impact)) = landing {
                if let Some(target) = impact.target {
                    let hit_speed = impact.speed;
                    if let Ok((_, _, mut dummy)) = dummies.get_mut(target) {
                        dummy.take_hit(ballistics::damage_at_speed(BULLET_DAMAGE, hit_speed));
                    }
                }
                impacts.write(impact);
                finished = true;
                break;
            }
            // Foliage on the way (a hedge, a tree's crown) slows the bullet and knocks it a little off
            // line, with a burst of leaves where it goes in; if it is slowed enough, it stops there.
            let depth = colliders.as_ref().map_or(0.0, |c| c.foliage_depth(start, flight.position));
            if depth > 0.0 {
                let mut noise = Rng(((bullet.travelled * 977.0) as u32) | 1);
                let jitter = Vec3::new(noise.range(-1.0, 1.0), noise.range(-1.0, 1.0), noise.range(-1.0, 1.0));
                flight.velocity = ballistics::through_foliage(flight.velocity, depth, jitter);
                let direction = flight.velocity.normalize_or(Vec3::NEG_Z);
                let stopped = flight.velocity.length() < ballistics::FOLIAGE_STOP_SPEED;
                if !bullet.in_foliage || stopped {
                    impacts.write(Impact { position: flight.position, normal: -direction, surface: Surface::Foliage, speed, target: None });
                }
                bullet.in_foliage = true;
                if stopped {
                    finished = true;
                    break;
                }
            } else {
                bullet.in_foliage = false;
            }
            if bullet.age > BULLET_LIFETIME {
                finished = true;
                break;
            }
        }
        if finished {
            commands.entity(entity).despawn();
            continue;
        }
        bullet.velocity = flight.velocity;
        transform.translation = flight.position;
        // The tracer points the way the bullet is going, which turns downward as it falls.
        if let Ok(heading) = Dir3::new(flight.velocity) {
            transform.look_to(heading, Vec3::Y);
        }
        transform.scale = tracer_scale(bullet.travelled);
        if tracer_visible(bullet.travelled) && *visibility == Visibility::Hidden {
            *visibility = Visibility::Inherited;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A point in the gun's space, as seen in the camera's space at a given aim blend.
    fn in_camera(blend: f32, gun_space: Vec3) -> Vec3 {
        gun_transform(blend).transform_point(gun_space)
    }

    #[test]
    fn on_the_sights_the_peep_and_the_front_blade_both_sit_on_the_cameras_axis() {
        let (rear_gun, front_gun) = sight_points();
        let (rear, front) = (in_camera(1.0, rear_gun), in_camera(1.0, front_gun));
        for (name, p) in [("rear peep", rear), ("front blade", front)] {
            assert!(p.x.abs() < 1e-4 && p.y.abs() < 1e-4, "{name} at ({}, {}) in camera space", p.x, p.y);
        }
        // The rear sight is nearer the eye than the front, as it must be to line up.
        assert!(rear.z > front.z + 0.3);
        // At the hip they are not on the axis at all: the gun is off to the side and below.
        let hip = in_camera(0.0, rear_gun);
        assert!(hip.x > 0.1 && hip.y < -0.1);
    }

    #[test]
    fn at_the_hip_the_gun_is_off_to_the_right_and_below() {
        let p = in_camera(0.0, MUZZLE_LOCAL);
        assert!(p.x > 0.1 && p.y < -0.1 && p.z < -0.3, "muzzle at {p:?}");
        let aimed = in_camera(1.0, MUZZLE_LOCAL);
        assert!(aimed.x.abs() < 1e-4 && aimed.y > -0.1, "aimed muzzle at {aimed:?}");
    }

    #[test]
    fn the_zero_is_twenty_metres() {
        assert_eq!(ZERO_DISTANCE, 20.0);
    }

    #[test]
    fn the_barrel_is_pointed_to_cross_the_line_of_sight_at_the_zero() {
        // From the hip, the muzzle is off to one side and below; from the sights it is nearly on
        // the line of sight. Either way a shot crosses the line of sight at the zero range.
        let (eye, forward) = (Vec3::new(10.0, 5.0, 3.0), Vec3::NEG_Z);
        for muzzle in [eye + Vec3::new(0.2, -0.18, -0.5), eye + Vec3::new(0.0, -0.04, -0.5)] {
            let dir = ballistics::zeroed_direction(&CARTRIDGE, muzzle, eye, forward, ZERO_DISTANCE);
            let target = eye + forward * ZERO_DISTANCE;
            let line = (target - muzzle).normalize();
            let flight = ballistics::point_at_range(&CARTRIDGE, muzzle, dir, line, (target - muzzle).length());
            let miss = (flight.position - target).length();
            assert!(miss < 0.005, "missed the zero point by {miss}");
        }
    }

    #[test]
    fn every_shot_kicks_the_view_up_and_less_on_the_sights() {
        for shot in 1..200 {
            let hip = recoil_kick(shot, false);
            let ads = recoil_kick(shot, true);
            assert!(hip.x > 0.006 && hip.x < 0.012, "pitch kick {}", hip.x);
            assert!(hip.y.abs() <= KICK_YAW + 1e-6);
            assert!((ads.x - hip.x * ADS_KICK_SCALE).abs() < 1e-6 && ads.x < hip.x);
            assert_eq!(recoil_kick(shot, false), hip, "repeatable");
        }
        // Sideways wander goes both ways and averages out near nothing.
        let sum: f32 = (1..400).map(|s| recoil_kick(s, false).y).sum();
        assert!(sum.abs() < 0.2, "yaw drifts one way: {sum}");
        assert!((1..50).any(|s| recoil_kick(s, false).y > 0.0) && (1..50).any(|s| recoil_kick(s, false).y < 0.0));
    }

    #[test]
    fn the_climb_only_recovers_once_the_gun_has_gone_quiet_and_never_completely() {
        let kick = Vec2::new(0.05, 0.01);
        assert_eq!(recover_kick(kick, 0.05, 0.016), (Vec2::ZERO, Vec2::ZERO), "no recovery mid-burst");
        // Run recovery for a few seconds, as the weapon does.
        let (mut left, mut returned) = (kick, Vec2::ZERO);
        for _ in 0..600 {
            let (settled, back) = recover_kick(left, 1.0, 1.0 / 120.0);
            left -= settled;
            returned += back;
        }
        assert!(left.length() < 1e-3, "the bookkeeping settles to zero");
        let fraction = returned.x / kick.x;
        assert!((fraction - KICK_RECOVERY_FRACTION).abs() < 0.01, "{fraction} of the climb came back");
        assert!(kick.x - returned.x > 0.01, "some climb is left for the player to pull down");
    }

    #[test]
    fn the_gun_jolts_back_and_up_on_a_shot_and_less_on_the_sights() {
        let (rest, hip, ads) = (gun_pose(0.0, 0.0), gun_pose(0.0, 1.0), gun_pose(1.0, 1.0));
        assert_eq!(rest.translation, gun_transform(0.0).translation);
        assert!(hip.translation.z > rest.translation.z, "shoved back toward the shoulder");
        // Tipped muzzle-up: the muzzle (at -Z) rises.
        assert!(hip.transform_point(MUZZLE_LOCAL).y > rest.transform_point(MUZZLE_LOCAL).y);
        let hip_shove = hip.translation.z - rest.translation.z;
        let ads_shove = ads.translation.z - gun_transform(1.0).translation.z;
        assert!(ads_shove < hip_shove);
        // Sight tops stay close to the line of sight even in the jolt.
        let blade = ads.transform_point(sight_points().1);
        assert!(blade.y.abs() < 0.03, "front blade {} off the line of sight under recoil", blade.y);
    }

    #[test]
    fn hip_fire_is_loose_and_aimed_fire_is_tight() {
        let hip = spread_half_angle(0.0, 0.0, false, 0.0, Stance::Stand);
        let ads = spread_half_angle(1.0, 0.0, false, 0.0, Stance::Stand);
        assert!((0.025..0.04).contains(&hip), "hip cone {hip} rad");
        assert!(ads < 0.006, "sights cone {ads} rad");
        assert!(hip > ads * 6.0, "hip should be several times looser than aimed");
        // Easing onto the sights tightens it steadily.
        let mut last = hip;
        for step in 1..=10 {
            let now = spread_half_angle(step as f32 / 10.0, 0.0, false, 0.0, Stance::Stand);
            assert!(now < last, "spread should shrink as the gun comes up");
            last = now;
        }
    }

    #[test]
    fn moving_jumping_and_long_bursts_all_widen_the_spread_but_less_on_the_sights() {
        let still = spread_half_angle(0.0, 0.0, false, 0.0, Stance::Stand);
        assert!(spread_half_angle(0.0, 1.0, false, 0.0, Stance::Stand) > still + 0.015, "running");
        assert!(spread_half_angle(0.0, 0.0, true, 0.0, Stance::Stand) > still + 0.03, "in the air");
        assert!(spread_half_angle(0.0, 0.0, false, BLOOM_MAX, Stance::Stand) > still + 0.025, "after a long burst");
        // The same penalties cost far less when braced on the sights.
        let hip_cost = spread_half_angle(0.0, 1.0, true, BLOOM_MAX, Stance::Stand) - still;
        let ads_cost = spread_half_angle(1.0, 1.0, true, BLOOM_MAX, Stance::Stand) - spread_half_angle(1.0, 0.0, false, 0.0, Stance::Stand);
        assert!(ads_cost < hip_cost * 0.3, "ads penalty {ads_cost} vs hip {hip_cost}");
        // Bloom can't grow without bound.
        assert_eq!(spread_half_angle(0.0, 0.0, false, 10.0, Stance::Stand), spread_half_angle(0.0, 0.0, false, BLOOM_MAX, Stance::Stand));
    }

    #[test]
    fn scattered_shots_stay_in_the_cone_and_fill_it_evenly() {
        let aim = Vec3::new(0.3, -0.2, -1.0).normalize();
        let half = 0.03;
        let mut seed = 12345u32;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed >> 8) as f32 / (1u32 << 24) as f32
        };
        let (mut inner, mut total, mut worst, mut sum) = (0, 4000, 0.0f32, Vec3::ZERO);
        for _ in 0..total {
            let d = scatter_direction(aim, half, next(), next());
            assert!((d.length() - 1.0).abs() < 1e-4);
            let angle = aim.dot(d).clamp(-1.0, 1.0).acos();
            worst = worst.max(angle);
            if angle < half * 0.7071 {
                inner += 1;
            }
            sum += d;
        }
        assert!(worst <= half + 1e-4, "a shot strayed {worst} rad, past the cone's {half}");
        assert!(worst > half * 0.97, "the cone's edge is actually used ({worst})");
        // Half the area of a disc lies inside radius / sqrt(2): an even fill puts half the shots there.
        let share = inner as f32 / total as f32;
        assert!((share - 0.5).abs() < 0.05, "{share} of shots in the inner 70% radius; an even spread gives 0.5");
        // And the average shot goes where the gun points.
        let mean = (sum / total as f32).normalize();
        assert!(mean.dot(aim) > 0.9999, "average shot is off-centre");
        total += 0;
        let _ = total;
    }

    #[test]
    fn zero_spread_goes_exactly_where_aimed() {
        let aim = Vec3::new(0.0, 0.0, -1.0);
        assert!((scatter_direction(aim, 0.0, 0.7, 0.3) - aim).length() < 1e-6);
        // Straight up or down must not break the basis.
        assert!(scatter_direction(Vec3::Y, 0.02, 0.5, 0.5).is_finite());
    }

    #[test]
    fn lower_stances_are_steadier() {
        let spread = |stance| spread_half_angle(0.0, 0.0, false, 0.0, stance);
        assert!(spread(Stance::Crouch) < spread(Stance::Stand) * 0.75, "crouching");
        assert!(spread(Stance::Prone) < spread(Stance::Crouch) * 0.75, "prone");
    }

    #[test]
    fn stances_scale_every_source_of_inaccuracy() {
        // Running, jumping and a long burst are each less wild when crouched or prone.
        for (speed, airborne, bloom) in [(1.0, false, 0.0), (0.0, true, 0.0), (0.0, false, BLOOM_MAX)] {
            let stand = spread_half_angle(0.0, speed, airborne, bloom, Stance::Stand);
            let crouch = spread_half_angle(0.0, speed, airborne, bloom, Stance::Crouch);
            let prone = spread_half_angle(0.0, speed, airborne, bloom, Stance::Prone);
            assert!(prone < crouch && crouch < stand, "{speed} {airborne} {bloom}");
        }
    }

    #[test]
    fn crouched_and_moving_is_still_worse_than_crouched_and_still() {
        let still = spread_half_angle(0.0, 0.0, false, 0.0, Stance::Crouch);
        assert!(spread_half_angle(0.0, 0.3, false, 0.0, Stance::Crouch) > still);
    }

    #[test]
    fn tracers_stay_hidden_near_the_muzzle() {
        assert!(!tracer_visible(0.0));
        assert!(!tracer_visible(TRACER_START - 0.1));
        assert!(tracer_visible(TRACER_START));
        assert!(tracer_visible(200.0));
    }

    #[test]
    fn the_tracer_grows_with_distance_up_to_a_limit() {
        let near = tracer_scale(TRACER_START);
        let far = tracer_scale(100.0);
        assert!(far.x > near.x * 2.0 && far.z > near.z);
        assert_eq!(tracer_scale(1.0e6), tracer_scale(1.0e7), "capped");
        assert!(tracer_scale(0.0).x >= 1.0);
    }

    #[test]
    fn a_reload_drops_the_gun_away_and_brings_it_back() {
        for total in [2.0, 2.7] {
            assert_eq!(reload_lowering(0.0, total), 0.0);
            assert_eq!(reload_lowering(total / 2.0, total), 1.0, "fully down mid-reload");
            assert_eq!(reload_lowering(total, total), 0.0);
            let steps: Vec<f32> = (0..=100).map(|i| reload_lowering(i as f32 / 100.0 * total, total)).collect();
            assert!(steps.windows(2).take(15).all(|p| p[1] >= p[0]), "going down");
            assert!(steps.windows(2).skip(85).all(|p| p[1] <= p[0]), "coming back up");
        }
    }

    #[test]
    fn a_longer_reload_spends_the_extra_time_with_the_gun_down() {
        // The drop and the recovery take the same time either way; the extra seconds are at the bottom.
        let (quick, slow) = (2.0, 2.7);
        assert_eq!(reload_lowering(RELOAD_LOWER_TIME, quick), 1.0);
        assert_eq!(reload_lowering(RELOAD_LOWER_TIME, slow), 1.0);
        assert_eq!(reload_lowering(slow - RELOAD_LOWER_TIME, slow), 1.0);
        assert!(reload_lowering(quick - 0.1, slow) == 1.0, "still down when a quick reload would be rising");
    }

    #[test]
    fn a_lowered_gun_is_below_the_view() {
        let rest = gun_pose_lowered(0.0, 0.0, 0.0);
        let down = gun_pose_lowered(0.0, 0.0, 1.0);
        assert_eq!(rest, gun_pose(0.0, 0.0));
        // Even the muzzle end is well below the bottom of a 45-degree-or-so view.
        let muzzle = down.transform_point(MUZZLE_LOCAL);
        assert!(-muzzle.y / -muzzle.z > 1.0, "muzzle at {muzzle:?}");
        assert!(down.transform_point(Vec3::ZERO).y < rest.translation.y - 0.4);
    }

    #[test]
    fn the_magazine_holds_thirty_rounds() {
        assert_eq!(MAGAZINE_SIZE, 30);
    }
}

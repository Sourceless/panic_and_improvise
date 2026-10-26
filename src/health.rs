//! The player's health: it goes down when something hurts them (the view jolts and a cry is heard) and
//! comes back slowly when nothing has for a while, or faster while a dressing or the like is doing its work.
//! At nothing they are back at the start, whole, with a word to say what happened.

use bevy::audio::AudioSource;
use bevy::prelude::*;

use crate::map::TerrainMap;
use crate::player::FpsCamera;
use crate::sound::play_after;
use crate::zombie::Zombie;

pub const MAX_HEALTH: f32 = 100.0;
/// How long after a hurt before health starts to come back by itself, seconds, and how fast it then does.
const MEND_AFTER: f32 = 8.0;
const MEND_RATE: f32 = 0.5;
/// How long the red of a hurt lingers, how long the view shakes, and how long "you died" stays up, seconds.
const FLASH_TIME: f32 = 0.6;
const WOBBLE_TIME: f32 = 0.55;
const DIED_TIME: f32 = 4.0;
/// How many cries of pain there are (`sounds/player/hurt_N.wav`), and how soon after one another they may be heard.
const HURT_SOUNDS: usize = 6;
const HURT_SOUND_GAP: f32 = 0.35;

#[derive(Resource, Debug, Clone, PartialEq)]
pub struct PlayerHealth {
    pub current: f32,
    pub max: f32,
    /// Seconds since last hurt.
    since_hurt: f32,
    /// How red the view is, 1 just after a hurt down to 0.
    flash: f32,
    /// How hard the last hurt shook the view, and how long ago it did.
    hit_strength: f32,
    hit_age: f32,
    /// How many times the player has been hurt, so that a cry can be made for each.
    hurts: u32,
    /// Healing under way: what is still to come, and how fast, for each dressing or the like in use.
    healing: Vec<Healing>,
    /// How much longer "you died" is shown.
    pub died_message: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Healing {
    left: f32,
    per_second: f32,
}

impl Default for PlayerHealth {
    fn default() -> Self {
        PlayerHealth { current: MAX_HEALTH, max: MAX_HEALTH, since_hurt: MEND_AFTER, flash: 0.0, hit_strength: 0.0, hit_age: f32::MAX, hurts: 0, healing: Vec::new(), died_message: 0.0 }
    }
}

impl PlayerHealth {
    /// All of it as one line of text, exactly (a recording keeps it), and back.
    pub fn to_text(&self) -> String {
        use crate::recording::bits;
        let healing: Vec<String> = self.healing.iter().map(|h| format!("{}/{}", bits(h.left), bits(h.per_second))).collect();
        format!(
            "{} {} {} {} {} {} {} {} {}",
            bits(self.current),
            bits(self.max),
            bits(self.since_hurt),
            bits(self.flash),
            bits(self.hit_strength),
            bits(self.hit_age),
            self.hurts,
            bits(self.died_message),
            if healing.is_empty() { "-".to_string() } else { healing.join(",") }
        )
    }

    pub fn from_text(text: &str) -> Option<PlayerHealth> {
        use crate::recording::unbits;
        let mut parts = text.split_whitespace();
        let mut next = || parts.next();
        let current = unbits(next()?)?;
        let max = unbits(next()?)?;
        let since_hurt = unbits(next()?)?;
        let flash = unbits(next()?)?;
        let hit_strength = unbits(next()?)?;
        let hit_age = unbits(next()?)?;
        let hurts = next()?.parse().ok()?;
        let died_message = unbits(next()?)?;
        let doses = next()?;
        if next().is_some() {
            return None;
        }
        let mut healing = Vec::new();
        if doses != "-" {
            for dose in doses.split(',') {
                let (left, per_second) = dose.split_once('/')?;
                healing.push(Healing { left: unbits(left)?, per_second: unbits(per_second)? });
            }
        }
        Some(PlayerHealth { current, max, since_hurt, flash, hit_strength, hit_age, hurts, healing, died_message })
    }

    pub fn hurt(&mut self, amount: f32) {
        if amount <= 0.0 || self.current <= 0.0 {
            return;
        }
        self.current = (self.current - amount).max(0.0);
        self.since_hurt = 0.0;
        self.flash = 1.0;
        // A harder blow shakes the view more, but even a light one is felt.
        self.hit_strength = (amount / 15.0).clamp(0.5, 1.6);
        self.hit_age = 0.0;
        self.hurts += 1;
    }

    /// Starts healing: `total` health, given evenly over `seconds`. Refused (false) when there is no need.
    pub fn heal_over_time(&mut self, total: f32, seconds: f32) -> bool {
        if !self.alive() || self.current + self.pending() >= self.max - 0.5 || total <= 0.0 || seconds <= 0.0 {
            return false;
        }
        self.healing.push(Healing { left: total, per_second: total / seconds });
        true
    }

    /// How much health healing already under way has still to give.
    pub fn pending(&self) -> f32 {
        self.healing.iter().map(|h| h.left).sum()
    }

    pub fn alive(&self) -> bool {
        self.current > 0.0
    }

    /// How full, 0 to 1.
    pub fn fraction(&self) -> f32 {
        (self.current / self.max).clamp(0.0, 1.0)
    }

    /// How full it will be when the healing under way is done, 0 to 1.
    pub fn fraction_after_healing(&self) -> f32 {
        ((self.current + self.pending()) / self.max).clamp(0.0, 1.0)
    }

    /// How red the view is, 0 to 1.
    pub fn flash(&self) -> f32 {
        self.flash
    }

    /// How far the view is turned now by the last blow, radians: (yaw, pitch, roll).
    pub fn wobble(&self) -> (f32, f32, f32) {
        wobble_at(self.hit_age, self.hit_strength)
    }

    fn tick(&mut self, dt: f32) {
        self.flash = (self.flash - dt / FLASH_TIME).max(0.0);
        self.died_message = (self.died_message - dt).max(0.0);
        self.since_hurt += dt;
        self.hit_age = (self.hit_age + dt).min(1000.0);
        if self.alive() {
            if self.since_hurt >= MEND_AFTER {
                self.current = (self.current + MEND_RATE * dt).min(self.max);
            }
            for dose in &mut self.healing {
                let given = (dose.per_second * dt).min(dose.left);
                dose.left -= given;
                self.current = (self.current + given).min(self.max);
            }
            self.healing.retain(|h| h.left > 1e-4);
        }
    }

    /// Whole again, as at the start.
    fn restore(&mut self) {
        let hurts = self.hurts;
        *self = PlayerHealth { died_message: DIED_TIME, hurts, ..default() };
    }
}

/// How far the view is turned `age` seconds after a blow of `strength`: a jolt that dips and rolls, and dies
/// away. (yaw, pitch, roll), radians.
pub fn wobble_at(age: f32, strength: f32) -> (f32, f32, f32) {
    if age >= WOBBLE_TIME || strength <= 0.0 {
        return (0.0, 0.0, 0.0);
    }
    let fade = (1.0 - age / WOBBLE_TIME).powi(2);
    let s = strength * fade;
    (0.025 * s * (age * 27.0 + 1.0).sin(), -0.05 * s * (age * 20.0).cos(), 0.06 * s * (age * 36.0).sin())
}

pub struct HealthPlugin;

impl Plugin for HealthPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayerHealth>()
            .add_systems(Startup, load_cries)
            .add_systems(Update, (tick_health, cry_when_hurt, die_and_restart).chain())
            .add_systems(PostUpdate, shake_view.before(TransformSystems::Propagate));
    }
}

#[derive(Resource)]
struct Cries(Vec<Handle<AudioSource>>);

fn load_cries(mut commands: Commands, assets: Option<Res<AssetServer>>) {
    let Some(assets) = assets else { return };
    commands.insert_resource(Cries((1..=HURT_SOUNDS).map(|i| assets.load(format!("sounds/player/hurt_{i}.wav"))).collect()));
}

fn tick_health(time: Res<Time>, mut health: ResMut<PlayerHealth>) {
    health.tick(time.delta_secs());
}

/// A cry of pain for each hurt, a different one each time and a little different in pitch.
fn cry_when_hurt(mut commands: Commands, time: Res<Time>, health: Res<PlayerHealth>, cries: Option<Res<Cries>>, mut seen: Local<u32>, mut quiet_for: Local<f32>) {
    *quiet_for += time.delta_secs();
    if health.hurts == *seen {
        return;
    }
    *seen = health.hurts;
    let Some(cries) = cries else { return };
    if cries.0.is_empty() || *quiet_for < HURT_SOUND_GAP {
        return;
    }
    *quiet_for = 0.0;
    let n = health.hurts as usize;
    let pitch = 0.94 + 0.12 * (((n * 37) % 11) as f32 / 10.0);
    play_after(&mut commands, 0.0, cries.0[(n * 5 + 1) % cries.0.len()].clone(), pitch, 0.9, None);
}

/// Turns the view a little off its line while a blow's jolt lasts. The view is always rebuilt from where the
/// player is looking, so this only sits on top: when it is over, the view is exactly where it was.
fn shake_view(health: Res<PlayerHealth>, mut camera: Query<(&mut Transform, &FpsCamera)>, mut shaking: Local<bool>) {
    let (yaw, pitch, roll) = health.wobble();
    let now = yaw != 0.0 || pitch != 0.0 || roll != 0.0;
    if !now && !*shaking {
        return;
    }
    *shaking = now;
    let Ok((mut transform, cam)) = camera.single_mut() else { return };
    transform.rotation = Quat::from_euler(EulerRot::YXZ, cam.yaw + yaw, cam.pitch + pitch, roll);
}

/// At no health the player is put back where they began, standing and whole. The zombies are cleared
/// away too (they come again out of sight), so as not to be standing on them when they get up.
fn die_and_restart(
    mut commands: Commands,
    mut health: ResMut<PlayerHealth>,
    map: Res<TerrainMap>,
    mut player: Query<(&mut Transform, &mut FpsCamera)>,
    zombies: Query<Entity, With<Zombie>>,
) {
    if health.alive() {
        return;
    }
    let Ok((mut transform, mut camera)) = player.single_mut() else { return };
    camera.restart_at(&mut transform, map.spawn_point(), &map);
    for zombie in &zombies {
        commands.entity(zombie).despawn();
    }
    health.restore();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::Stance;

    #[test]
    fn health_in_any_state_reads_back_from_its_text_exactly() {
        let mut h = PlayerHealth::default();
        assert_eq!(PlayerHealth::from_text(&h.to_text()).unwrap().to_text(), h.to_text());
        h.hurt(40.0);
        assert!(h.heal_over_time(30.0, 12.0));
        assert!(h.heal_over_time(70.0, 10.0));
        h.tick(0.37);
        let again = PlayerHealth::from_text(&h.to_text()).expect("it reads its own text");
        assert_eq!((again.current, again.pending(), again.flash(), again.wobble()), (h.current, h.pending(), h.flash(), h.wobble()));
        assert_eq!(again.to_text(), h.to_text());
        assert!(PlayerHealth::from_text("nonsense").is_none());
    }

    #[test]
    fn a_hurt_takes_health_down_but_not_below_nothing_and_flashes() {
        let mut h = PlayerHealth::default();
        assert_eq!((h.fraction(), h.flash()), (1.0, 0.0));
        h.hurt(30.0);
        assert_eq!(h.current, 70.0);
        assert_eq!(h.flash(), 1.0);
        h.hurt(500.0);
        assert_eq!(h.current, 0.0);
        assert!(!h.alive());
        h.hurt(-5.0);
        assert_eq!(h.current, 0.0, "a hurt of less than nothing is not a heal");
    }

    #[test]
    fn the_red_fades_and_health_mends_only_after_a_quiet_while() {
        let mut h = PlayerHealth::default();
        h.hurt(50.0);
        h.tick(FLASH_TIME * 0.5);
        assert!(h.flash() > 0.0 && h.flash() < 1.0);
        h.tick(FLASH_TIME);
        assert_eq!(h.flash(), 0.0);
        assert_eq!(h.current, 50.0, "not yet");
        h.tick(MEND_AFTER);
        h.tick(10.0);
        assert!(h.current > 50.0 && h.current < 60.0, "{}", h.current);
        h.tick(1000.0);
        assert_eq!(h.current, h.max);
        // Another hurt starts the wait over.
        h.hurt(10.0);
        h.tick(MEND_AFTER - 1.0);
        assert_eq!(h.current, h.max - 10.0);
    }

    #[test]
    fn healing_comes_a_little_at_a_time_over_the_time_it_takes_and_is_not_wasted_or_overdone() {
        let mut h = PlayerHealth::default();
        // Nothing to heal at full health.
        assert!(!h.heal_over_time(30.0, 10.0));
        h.hurt(60.0);
        assert!(h.heal_over_time(30.0, 10.0));
        assert_eq!((h.current, h.pending()), (40.0, 30.0));
        assert!((h.fraction_after_healing() - 0.7).abs() < 1e-5);
        // Half-way through, half has been given (the hurt's own wait for mending has not passed).
        for _ in 0..50 {
            h.tick(0.1);
        }
        assert!((h.current - 55.0).abs() < 0.01 && (h.pending() - 15.0).abs() < 0.01, "{} {}", h.current, h.pending());
        for _ in 0..60 {
            h.tick(0.1);
        }
        // All of it has come (and the slow mending that begins after eight quiet seconds has added a little).
        assert!(h.current >= 70.0 - 0.01 && h.current < 72.0 && h.pending() == 0.0, "{}", h.current);
        // Two at once add up, and cannot take it past full.
        let mut h = PlayerHealth::default();
        h.hurt(20.0);
        assert!(h.heal_over_time(15.0, 5.0));
        assert!(h.heal_over_time(15.0, 5.0), "there is still room for more");
        h.tick(10.0);
        assert_eq!(h.current, h.max);
        assert_eq!(h.pending(), 0.0);
        // And no use when what is coming already makes up what is lost.
        let mut h = PlayerHealth::default();
        h.hurt(10.0);
        assert!(h.heal_over_time(10.0, 5.0));
        assert!(!h.heal_over_time(10.0, 5.0));
    }

    #[test]
    fn a_blow_shakes_the_view_and_it_settles_back_to_exactly_where_it_was() {
        assert_eq!(wobble_at(WOBBLE_TIME, 1.0), (0.0, 0.0, 0.0));
        assert_eq!(wobble_at(5.0, 1.0), (0.0, 0.0, 0.0));
        let swing = |strength: f32| (0..55).map(|i| wobble_at(i as f32 * 0.01, strength)).map(|(y, p, r)| y.abs().max(p.abs()).max(r.abs())).fold(0.0, f32::max);
        assert!(swing(1.0) > 0.03 && swing(1.0) < 0.12, "a noticeable jolt, not a spin: {}", swing(1.0));
        assert!(swing(1.5) > swing(0.5), "harder blows shake more");
        // It dies away.
        assert!(wobble_at(0.5, 1.0).2.abs() < wobble_at(0.05, 1.0).2.abs().max(0.02));
        // And the health says so only after a hurt.
        let mut h = PlayerHealth::default();
        assert_eq!(h.wobble(), (0.0, 0.0, 0.0));
        h.hurt(20.0);
        h.tick(0.04);
        assert_ne!(h.wobble(), (0.0, 0.0, 0.0));
        h.tick(2.0);
        assert_eq!(h.wobble(), (0.0, 0.0, 0.0));
    }

    #[test]
    fn every_hurt_is_counted_so_that_each_gets_a_cry() {
        let mut h = PlayerHealth::default();
        h.hurt(5.0);
        h.hurt(5.0);
        assert_eq!(h.hurts, 2);
        h.hurt(0.0);
        assert_eq!(h.hurts, 2, "no hurt, no cry");
    }

    #[test]
    fn the_view_is_turned_by_a_blow_and_put_back_by_the_end_of_it() {
        let mut app = App::new();
        app.init_resource::<PlayerHealth>().add_systems(PostUpdate, shake_view);
        let camera = FpsCamera { yaw: 0.7, pitch: -0.2, velocity: Vec2::ZERO, air_height: 0.0, vertical_speed: 0.0, stance: Stance::Stand, eye_height: 1.8, jump_spent_standing: false, floor: 0.0, mantle: None, mantles: 0, sprint_suppressed: false, sliding: false, view_feet: 0.0, lean: 0.0, lean_offset: Vec2::ZERO };
        let steady = Quat::from_euler(EulerRot::YXZ, 0.7, -0.2, 0.0);
        let eye = app.world_mut().spawn((camera, Transform::from_xyz(0.0, 1.8, 0.0).with_rotation(steady))).id();
        app.update();
        assert_eq!(app.world().get::<Transform>(eye).unwrap().rotation, steady, "no blow, no change");
        {
            let mut health = app.world_mut().resource_mut::<PlayerHealth>();
            health.hurt(20.0);
            health.tick(0.05);
        }
        app.update();
        let shaken = app.world().get::<Transform>(eye).unwrap().rotation;
        assert!(shaken.angle_between(steady) > 0.005, "turned: {}", shaken.angle_between(steady));
        app.world_mut().resource_mut::<PlayerHealth>().tick(2.0);
        app.update();
        assert!(app.world().get::<Transform>(eye).unwrap().rotation.angle_between(steady) < 1e-6, "back where it was");
    }

    #[test]
    fn at_no_health_the_player_is_back_at_the_start_whole_and_the_zombies_are_gone() {
        let mut app = App::new();
        app.init_resource::<Time>().insert_resource(TerrainMap::flat(5.0)).init_resource::<PlayerHealth>().add_systems(Update, die_and_restart);
        let camera = FpsCamera { yaw: 0.0, pitch: 0.0, velocity: Vec2::new(3.0, 4.0), air_height: 2.0, vertical_speed: 1.0, stance: Stance::Prone, eye_height: 0.4, jump_spent_standing: false, floor: 9.0, mantle: None, mantles: 0, sprint_suppressed: false, sliding: true, view_feet: 9.0, lean: 0.0, lean_offset: Vec2::ZERO };
        let player = app.world_mut().spawn((camera, Transform::from_xyz(300.0, 12.0, -200.0))).id();
        let zombie = app.world_mut().spawn((Zombie::new(crate::zombie::ZombieKind::Walker, 0.0), Transform::default())).id();
        // Alive: nothing changes.
        app.update();
        assert_eq!(app.world().get::<Transform>(player).unwrap().translation, Vec3::new(300.0, 12.0, -200.0));
        assert!(app.world().get::<Zombie>(zombie).is_some());
        // Dead: back at the start (the middle of a flat map), standing on the ground.
        app.world_mut().resource_mut::<PlayerHealth>().hurt(MAX_HEALTH);
        app.update();
        let at = app.world().get::<Transform>(player).unwrap().translation;
        assert!(at.x.abs() < 1e-4 && at.z.abs() < 1e-4 && at.y > 5.0 && at.y < 7.0, "{at}");
        let camera = app.world().get::<FpsCamera>(player).unwrap();
        assert_eq!((camera.velocity, camera.stance(), camera.sliding()), (Vec2::ZERO, Stance::Stand, false));
        assert_eq!(camera.feet(), 5.0);
        assert!(app.world().get::<Zombie>(zombie).is_none());
        let health = app.world().resource::<PlayerHealth>();
        assert_eq!(health.current, MAX_HEALTH);
        assert!(health.died_message > 0.0);
    }
}

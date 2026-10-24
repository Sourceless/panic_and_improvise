//! A teammate with no window, no renderer and no sound: a client that joins a hosted game as another player,
//! follows the host's player about, and shoots the zombies that come near.
//!
//! It keeps a few steps behind the player, goes round walls, through doors and up and down stairs to get to
//! them, breaks into a run when they get away, and stops and turns to face them when it has caught up. It
//! makes the same map as the host from the seed in the welcome, so that it knows where the ground is and what
//! is solid, and walks the way a zombie does: the same pathfinding, the same bumping into things.
//!
//! The host is the authority on everything but where the bot is: the zombies are the host's, and the host is
//! told only which one the bot is firing at, and works out whether it hits. Zombies go after the bot as they
//! do the player. If it is brought down it lies where it fell until the host gets it up again.

use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::{Duration, Instant};

use bevy::prelude::*;

use crate::collision::Colliders;
use crate::impact::surface_hit;
use crate::map::TerrainMap;
use crate::navigation::Pursuit;
use crate::net::{Packet, PlayerState, ZombieSeen, CLIENT_HEALTH, EYE_HEIGHT, HOST_ID, SEND_RATE, SILENCE_LIMIT};
use crate::walking::{stand, walk};

/// How fast it walks and how fast it runs, metres a second: a little under the player's own, so that a player
/// who keeps going gets away.
pub const WALK: f32 = 5.0;
pub const RUN: f32 = 9.5;

/// How far behind the host's player it runs rather than walks, metres.
const RUN_FROM: f32 = 14.0;

/// It stops this near the host's player, and starts after them again when they are this far off.
const STOP_WITHIN: f32 = 2.5;
const START_BEYOND: f32 = 4.0;

/// How far from its middle it is stopped by a wall, and how big a body the routes are worked out for.
const BODY: f32 = 0.29;
const ROUTE_BODY: f32 = 0.28;

/// How quickly, per second, its velocity and its facing catch up with what it wants.
const ACCELERATION: f32 = 9.0;
const TURNING: f32 = 8.0;

/// How far it will shoot, metres, how long it waits between shots, and how nearly it must face the target
/// (radians) to fire.
const ENGAGE: f32 = 40.0;
const FIRE_EVERY: f32 = 0.45;
const AIMED: f32 = 0.2;

/// How high on a zombie it aims, metres above its feet.
const AIM_HEIGHT: f32 = 1.1;

/// The kinds of zombie by the host's numbering (see `ZombieKind`), for talking about them.
const KIND_NAMES: [&str; 3] = ["zombie", "brute", "runner"];

/// The next of a run of numbers in 0..1 (a cheap generator: it only keeps bots from moving in step).
fn next_luck(luck: &mut u32) -> f32 {
    *luck ^= *luck << 13;
    *luck ^= *luck >> 17;
    *luck ^= *luck << 5;
    (*luck >> 8) as f32 / (1u32 << 24) as f32
}

/// What it is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// It has not yet been told where the host's player is.
    Waiting,
    Following,
    Running,
    /// Beside them.
    Arrived,
    /// Told to stay where it is.
    Staying,
    /// Brought down, and lying there until the host gets it up.
    Down,
    /// The host has stopped answering.
    Lost,
}

pub struct Bot {
    socket: UdpSocket,
    pub name: String,
    pub id: u8,
    pub seed: u64,
    pub me: PlayerState,
    pub host: Option<PlayerState>,
    pub mode: Mode,
    /// Whether it has been told to stay put rather than follow.
    pub stay: bool,
    /// Whether it shoots at zombies (it can be told not to).
    pub shoot: bool,
    /// How it is, as the host last said.
    pub health: f32,
    pub respawn_in: f32,
    /// The zombies the host last said were about, and the one it is firing at, if any.
    pub zombies: Vec<ZombieSeen>,
    pub target: Option<u32>,
    /// How many shots it has fired.
    pub shots: u32,
    velocity: Vec2,
    falling: f32,
    pursuit: Pursuit,
    since_send: f32,
    since_shot: f32,
    silent_for: f32,
    was_down: bool,
    luck: u32,
}

impl Bot {
    /// Asks the host at `server` to let it in as `name`, trying again every half second for `patience`.
    pub fn join(server: impl ToSocketAddrs, name: &str, patience: Duration) -> Result<Bot, String> {
        let server: SocketAddr = server.to_socket_addrs().map_err(|e| format!("no such address: {e}"))?.next().ok_or("no such address")?;
        let socket = UdpSocket::bind(if server.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" }).map_err(|e| format!("could not open a socket: {e}"))?;
        socket.connect(server).map_err(|e| format!("could not reach {server}: {e}"))?;
        socket.set_read_timeout(Some(Duration::from_millis(500))).map_err(|e| e.to_string())?;
        let started = Instant::now();
        let mut buffer = [0u8; 1500];
        while started.elapsed() < patience {
            socket.send(&Packet::Join { name: name.to_string() }.encode()).map_err(|e| format!("could not send to {server}: {e}"))?;
            // Anything that is not the answer is skipped over until the half second is up.
            let asked = Instant::now();
            while asked.elapsed() < Duration::from_millis(500) {
                let Ok(n) = socket.recv(&mut buffer) else { continue };
                match Packet::decode(&buffer[..n]) {
                    Some(Packet::Welcome { id, seed, host }) => {
                        socket.set_nonblocking(true).map_err(|e| e.to_string())?;
                        return Ok(Bot {
                            socket,
                            name: name.to_string(),
                            id,
                            seed,
                            me: host,
                            host: Some(host),
                            mode: Mode::Waiting,
                            stay: false,
                            shoot: true,
                            health: CLIENT_HEALTH,
                            respawn_in: 0.0,
                            zombies: Vec::new(),
                            target: None,
                            shots: 0,
                            velocity: Vec2::ZERO,
                            falling: 0.0,
                            pursuit: Pursuit::default(),
                            since_send: 0.0,
                            since_shot: FIRE_EVERY,
                            silent_for: 0.0,
                            was_down: false,
                            luck: 0x2545_F491 ^ seed as u32 ^ id as u32,
                        });
                    }
                    Some(Packet::Refused) => return Err("the game is full, or is a different version".into()),
                    _ => {}
                }
            }
        }
        Err(format!("nobody answered at {server}: is the game running with --host?"))
    }

    fn position(&self) -> Vec2 {
        Vec2::new(self.me.x, self.me.z)
    }

    fn feet(&self) -> Vec3 {
        Vec3::new(self.me.x, self.me.y, self.me.z)
    }

    fn alive(&self) -> bool {
        self.health > 0.0
    }

    /// Puts it down on the ground at `at`.
    pub fn put_at(&mut self, map: &TerrainMap, at: Vec2) {
        self.me.x = at.x;
        self.me.z = at.y;
        self.me.y = map.surface_height_at(at);
        self.velocity = Vec2::ZERO;
        self.falling = 0.0;
        self.pursuit.clear();
    }

    /// Stands it a few metres from the host's player, on their floor, somewhere it can stand, to begin with
    /// (and again when it has been got up).
    pub fn arrive(&mut self, map: &TerrainMap, colliders: &Colliders) {
        let Some(host) = self.host else { return };
        let there = Vec2::new(host.x, host.z);
        let spot = (0..32).find_map(|i| {
            let p = there + Vec2::from_angle(i as f32 * 0.7 + self.id as f32) * (4.0 + (i / 8) as f32 * 2.0);
            stand(map, colliders, p, host.y, BODY).map(|feet| (p, feet))
        });
        match spot {
            Some((p, feet)) => {
                self.put_at(map, p);
                self.me.y = feet;
            }
            None => {
                self.put_at(map, there);
                self.me.y = host.y;
            }
        }
    }

    /// Reads everything that has arrived: where the host's player is, which zombies are about, how it is.
    fn listen(&mut self) {
        let mut buffer = [0u8; 1500];
        while let Ok(n) = self.socket.recv(&mut buffer) {
            match Packet::decode(&buffer[..n]) {
                Some(Packet::Snapshot { players, .. }) => {
                    if let Some((_, state)) = players.into_iter().find(|(id, _)| *id == HOST_ID) {
                        self.host = Some(state);
                        self.silent_for = 0.0;
                    }
                }
                Some(Packet::Zombies(list)) => self.zombies = list,
                Some(Packet::Status { health, respawn_in }) => {
                    self.health = health;
                    self.respawn_in = respawn_in;
                }
                _ => {}
            }
        }
    }

    /// The zombie to shoot at, if there is one: the nearest it has a clear line to.
    fn choose_target(&self, map: &TerrainMap, colliders: &Colliders) -> Option<ZombieSeen> {
        let eye = self.feet() + Vec3::Y * EYE_HEIGHT;
        self.zombies
            .iter()
            .filter_map(|z| {
                let chest = Vec3::new(z.x, z.y + AIM_HEIGHT, z.z);
                let distance = eye.distance(chest);
                let clear = surface_hit(map, eye, chest).is_none() && colliders.segment_hit(eye, chest).is_none();
                (distance <= ENGAGE && clear).then_some((distance, *z))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, z)| z)
    }

    /// Lets `dt` seconds go by: listens, decides, walks, shoots, and says where it is.
    pub fn tick(&mut self, dt: f32, map: &TerrainMap, colliders: &Colliders) {
        self.listen();
        self.silent_for += dt;
        self.since_shot += dt;
        self.target = None;
        let Some(host) = self.host.filter(|_| self.silent_for < SILENCE_LIMIT) else {
            self.mode = if self.silent_for >= SILENCE_LIMIT { Mode::Lost } else { Mode::Waiting };
            self.velocity = Vec2::ZERO;
            return self.say(dt);
        };
        // Brought down: it lies there. Got up: it comes back to the host's player.
        if !self.alive() {
            self.mode = Mode::Down;
            self.was_down = true;
            self.velocity = Vec2::ZERO;
            self.pursuit.clear();
            return self.say(dt);
        }
        if self.was_down {
            self.was_down = false;
            self.arrive(map, colliders);
        }
        let here = self.position();
        let goal = Vec3::new(host.x, host.y, host.z);
        let distance = here.distance(Vec2::new(host.x, host.z));
        // Stopping and starting off at different distances, so that it does not shuffle on the edge.
        self.mode = match self.mode {
            _ if self.stay => Mode::Staying,
            Mode::Arrived if distance < START_BEYOND && (self.me.y - host.y).abs() < 1.0 => Mode::Arrived,
            _ if distance < STOP_WITHIN && (self.me.y - host.y).abs() < 1.0 => Mode::Arrived,
            _ if distance > RUN_FROM => Mode::Running,
            Mode::Running if distance > RUN_FROM * 0.6 => Mode::Running,
            _ => Mode::Following,
        };
        let wanted = if matches!(self.mode, Mode::Arrived | Mode::Staying) {
            self.pursuit.clear();
            Vec2::ZERO
        } else {
            let mut routes_left = 1;
            let mut luck = self.luck;
            let aim = self.pursuit.aim(self.feet(), goal, dt, &mut routes_left, || next_luck(&mut luck), |p, feet| stand(map, colliders, p, feet, ROUTE_BODY));
            self.luck = luck;
            let speed = if self.mode == Mode::Running { RUN } else { WALK };
            (aim - here).normalize_or_zero() * speed
        };
        self.velocity = self.velocity.lerp(wanted, 1.0 - (-ACCELERATION * dt).exp());
        let (moved, feet) = walk(map, colliders, here, self.me.y, &mut self.falling, self.velocity, dt, BODY);
        self.me.x = moved.x;
        self.me.z = moved.y;
        self.me.y = feet;

        // Zombies about: it faces the nearest it can see and fires at it.
        let target = if self.shoot { self.choose_target(map, colliders) } else { None };
        let look = match target {
            Some(z) => Vec2::new(z.x, z.z) - self.position(),
            None if self.velocity.length() > 0.5 => self.velocity,
            None => Vec2::new(host.x, host.z) - self.position(),
        };
        let mut aimed = false;
        if look.length() > 1e-3 {
            let heading = (-look.x).atan2(-look.y);
            let turn = (heading - self.me.yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
            self.me.yaw += turn * (1.0 - (-TURNING * dt).exp());
            aimed = turn.abs() < AIMED;
        }
        if let Some(z) = target {
            self.target = Some(z.id);
            if aimed && self.since_shot >= FIRE_EVERY {
                self.since_shot = 0.0;
                self.shots += 1;
                let _ = self.socket.send(&Packet::Fire { target: z.id }.encode());
            }
        }
        self.say(dt);
    }

    /// Keeps the host hearing from it while it has no time to do anything else (making the world, say).
    pub fn hold(&mut self, dt: f32) {
        self.listen();
        self.say(dt);
    }

    /// Tells the host where it is, at the usual rate.
    fn say(&mut self, dt: f32) {
        self.since_send += dt;
        if self.since_send >= 1.0 / SEND_RATE {
            self.since_send = 0.0;
            let _ = self.socket.send(&Packet::Update(self.me).encode());
        }
    }

    /// How far it is from the host's player, metres, if it knows where they are.
    pub fn distance(&self) -> Option<f32> {
        self.host.map(|h| self.position().distance(Vec2::new(h.x, h.z)))
    }

    /// A line saying how it is getting on.
    pub fn report(&self) -> String {
        let me = format!("({:.1}, {:.1})", self.me.x, self.me.z);
        let who = format!("{} (player {}) at {me}", self.name, self.id);
        match (self.mode, self.host) {
            (Mode::Waiting, _) | (_, None) => format!("{who}: waiting to hear where you are"),
            (Mode::Lost, _) => format!("{who}: the host has gone quiet"),
            (Mode::Down, _) => format!("{who}: DOWN, back on its feet in {:.0} s", self.respawn_in),
            (mode, Some(host)) => {
                let doing = match mode {
                    Mode::Arrived => "next to you",
                    Mode::Staying => "staying put",
                    Mode::Running => "running after you",
                    _ if self.pursuit.detouring() => "going round something to you",
                    _ => "following you",
                };
                let shooting = self.target.and_then(|id| self.zombies.iter().find(|z| z.id == id)).map(|z| {
                    let d = self.feet().distance(Vec3::new(z.x, z.y, z.z));
                    format!(", shooting a {} {d:.0} m off", KIND_NAMES.get(z.kind as usize).copied().unwrap_or("zombie"))
                });
                format!("{who}, you at ({:.1}, {:.1}), {:.1} m apart, health {:.0}: {doing}{}", host.x, host.z, self.distance().unwrap_or(0.0), self.health, shooting.unwrap_or_default())
            }
        }
    }

    /// Tells the host it is going.
    pub fn leave(&self) {
        let _ = self.socket.send(&Packet::Leave.encode());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::Solid;
    use crate::health::PlayerHealth;
    use crate::net_host::{Host, HostPlugin, RemotePlayer};
    use crate::player::FpsCamera;
    use crate::zombie::{move_zombies, Zombie, ZombieDice, ZombieKind};

    /// A hosted game whose player stands at `player`, on a flat map, with zombies that move.
    fn hosted(player: Vec2, colliders: Colliders) -> App {
        let mut app = App::new();
        app.init_resource::<Time>()
            .init_resource::<ZombieDice>()
            .init_resource::<PlayerHealth>()
            .insert_resource(TerrainMap::flat(0.0))
            .insert_resource(colliders)
            .add_plugins(HostPlugin::at("127.0.0.1:0"))
            .add_systems(Update, move_zombies);
        let camera = FpsCamera { yaw: 0.0, pitch: 0.0, velocity: Vec2::ZERO, air_height: 0.0, vertical_speed: 0.0, stance: crate::player::Stance::Stand, eye_height: 1.8, jump_spent_standing: false, floor: 0.0, mantle: None, mantles: 0, sprint_suppressed: false, sliding: false, view_feet: 0.0 };
        app.world_mut().spawn((camera, GlobalTransform::from(Transform::from_xyz(player.x, 1.8, player.y)), Transform::from_xyz(player.x, 1.8, player.y)));
        app
    }

    fn join(app: &mut App, name: &str) -> Bot {
        let server = app.world().resource::<Host>().local_addr();
        // The host answers from this thread, so the bot's asking is done on another and the game is run
        // in the meantime.
        let name = name.to_string();
        let asking = std::thread::spawn(move || Bot::join(server, &name, Duration::from_secs(5)));
        while !asking.is_finished() {
            app.world_mut().resource_mut::<Time>().advance_by(Duration::from_millis(10));
            app.update();
            std::thread::sleep(Duration::from_millis(5));
        }
        asking.join().unwrap().expect("let in")
    }

    /// Runs the game and the bot together for `seconds` of game time.
    fn run(app: &mut App, bot: &mut Bot, colliders: &Colliders, seconds: f32) {
        let map = TerrainMap::flat(0.0);
        for _ in 0..(seconds * 20.0) as usize {
            app.world_mut().resource_mut::<Time>().advance_by(Duration::from_millis(50));
            app.update();
            std::thread::sleep(Duration::from_millis(1));
            bot.tick(0.05, &map, colliders);
        }
    }

    fn move_player(app: &mut App, to: Vec2) {
        let mut q = app.world_mut().query::<(&mut Transform, &FpsCamera)>();
        q.single_mut(app.world_mut()).unwrap().0.translation = Vec3::new(to.x, 1.8, to.y);
        let mut g = app.world_mut().query::<(&mut GlobalTransform, &FpsCamera)>();
        *g.single_mut(app.world_mut()).unwrap().0 = GlobalTransform::from(Transform::from_xyz(to.x, 1.8, to.y));
    }

    fn add_zombie(app: &mut App, kind: ZombieKind, at: Vec2, id: u32) -> Entity {
        app.world_mut().spawn((Zombie::numbered(kind, 0.0, id), Transform::from_xyz(at.x, 0.0, at.y))).id()
    }

    fn health_of(app: &App, id: Entity) -> f32 {
        app.world().get::<Zombie>(id).unwrap().health
    }

    #[test]
    fn the_bot_joins_follows_the_player_across_the_map_and_stops_beside_them() {
        let none = Colliders::default();
        let mut app = hosted(Vec2::new(10.0, 10.0), Colliders::default());
        let mut bot = join(&mut app, "Botty");
        assert_eq!(bot.id, 1);
        bot.arrive(&TerrainMap::flat(0.0), &none);
        run(&mut app, &mut bot, &none, 2.0);
        assert!(bot.distance().unwrap() < 8.0, "{}", bot.report());
        // The player walks off a long way; the bot runs after them and ends up beside them.
        move_player(&mut app, Vec2::new(120.0, -40.0));
        run(&mut app, &mut bot, &none, 6.0);
        assert_eq!(bot.mode, Mode::Running, "{}", bot.report());
        run(&mut app, &mut bot, &none, 20.0);
        let near = bot.distance().unwrap();
        assert!((STOP_WITHIN * 0.5..=START_BEYOND).contains(&near), "{}", bot.report());
        assert_eq!(bot.mode, Mode::Arrived);
        // And the host's world has a figure of the bot standing there, too.
        let mut figures = app.world_mut().query::<(&Transform, &RemotePlayer)>();
        let (at, who) = figures.single(app.world()).unwrap();
        assert_eq!(who.0, 1);
        assert!(Vec2::new(at.translation.x, at.translation.z).distance(Vec2::new(120.0, -40.0)) < START_BEYOND + 0.5, "{}", at.translation);
        // It leaves, and the figure goes.
        bot.leave();
        run(&mut app, &mut bot, &none, 0.5);
        assert!(figures.iter(app.world()).next().is_none());
    }

    #[test]
    fn the_bot_goes_round_a_wall_through_the_doorway_to_get_to_the_player() {
        let mut walls = Colliders::default();
        // A long wall with a doorway a metre wide at x = 10, between the bot and the player.
        walls.add(Solid::wall(Vec2::new(-60.0, -6.0), Vec2::new(9.5, -6.0), 0.15, 3.0, 3.0));
        walls.add(Solid::wall(Vec2::new(10.5, -6.0), Vec2::new(60.0, -6.0), 0.15, 3.0, 3.0));
        let mut app = hosted(Vec2::new(0.0, 0.0), Colliders::default());
        let mut bot = join(&mut app, "Botty");
        bot.put_at(&TerrainMap::flat(0.0), Vec2::new(0.0, -14.0));
        let mut through = false;
        for _ in 0..40 {
            run(&mut app, &mut bot, &walls, 0.5);
            through |= bot.me.z > -6.0 && bot.me.x > 9.0;
            assert!(!(bot.me.z > -6.2 && bot.me.z < -5.8 && bot.me.x < 9.2), "went through the wall at ({}, {})", bot.me.x, bot.me.z);
        }
        assert!(through, "never came in by the doorway: {}", bot.report());
        assert_eq!(bot.mode, Mode::Arrived, "{}", bot.report());
    }

    #[test]
    fn told_to_stay_it_stays_and_told_to_follow_again_it_does() {
        let none = Colliders::default();
        let mut app = hosted(Vec2::new(0.0, 0.0), Colliders::default());
        let mut bot = join(&mut app, "Botty");
        bot.put_at(&TerrainMap::flat(0.0), Vec2::new(20.0, 0.0));
        bot.stay = true;
        run(&mut app, &mut bot, &none, 3.0);
        assert_eq!(bot.mode, Mode::Staying);
        assert!((bot.me.x - 20.0).abs() < 0.1 && bot.me.z.abs() < 0.1, "{}", bot.report());
        bot.stay = false;
        run(&mut app, &mut bot, &none, 8.0);
        assert!(bot.distance().unwrap() < START_BEYOND, "{}", bot.report());
    }

    #[test]
    fn a_bot_whose_host_goes_quiet_says_so_and_stands_still() {
        let none = Colliders::default();
        let mut app = hosted(Vec2::new(0.0, 0.0), Colliders::default());
        let mut bot = join(&mut app, "Botty");
        bot.arrive(&TerrainMap::flat(0.0), &none);
        let map = TerrainMap::flat(0.0);
        // The game is not run, so nothing is heard.
        for _ in 0..(SILENCE_LIMIT * 20.0) as usize + 20 {
            bot.tick(0.05, &map, &none);
        }
        assert_eq!(bot.mode, Mode::Lost);
        assert!(bot.report().contains("quiet"));
    }

    #[test]
    fn asking_a_game_that_is_not_there_gives_up_with_a_reason() {
        let nowhere = UdpSocket::bind("127.0.0.1:0").unwrap();
        let address = nowhere.local_addr().unwrap();
        let error = Bot::join(address, "Botty", Duration::from_millis(700)).err().expect("nobody answered");
        assert!(error.contains("nobody answered"), "{error}");
    }

    #[test]
    fn the_bot_shoots_a_zombie_that_comes_near_until_it_is_dead() {
        let none = Colliders::default();
        let mut app = hosted(Vec2::new(0.0, 0.0), Colliders::default());
        let mut bot = join(&mut app, "Botty");
        bot.arrive(&TerrainMap::flat(0.0), &none);
        let zombie = add_zombie(&mut app, ZombieKind::Walker, Vec2::new(25.0, 3.0), 77);
        // Stood where it is told, so that the zombie is dealt with by shooting and not by walking off.
        bot.stay = true;
        let before = health_of(&app, zombie);
        run(&mut app, &mut bot, &none, 1.5);
        assert!(bot.shots > 0, "{}", bot.report());
        run(&mut app, &mut bot, &none, 12.0);
        assert!(health_of(&app, zombie) < before, "{}", bot.report());
        assert!(!app.world().get::<Zombie>(zombie).unwrap().alive(), "a walker takes five hits, and it has had {} shots: {}", bot.shots, bot.report());
        // With nothing left to shoot at, it stops.
        let shots = bot.shots;
        run(&mut app, &mut bot, &none, 2.0);
        assert_eq!(bot.shots, shots);
    }

    #[test]
    fn the_bot_does_not_shoot_what_it_cannot_see_nor_when_told_not_to() {
        let mut walls = Colliders::default();
        walls.add(Solid::wall(Vec2::new(10.0, -50.0), Vec2::new(10.0, 50.0), 0.3, 6.0, 6.0));
        let mut app = hosted(Vec2::new(0.0, 0.0), Colliders::default());
        let mut bot = join(&mut app, "Botty");
        bot.put_at(&TerrainMap::flat(0.0), Vec2::new(0.0, 4.0));
        bot.stay = true;
        let behind = add_zombie(&mut app, ZombieKind::Walker, Vec2::new(25.0, 4.0), 5);
        run(&mut app, &mut bot, &walls, 3.0);
        assert_eq!(bot.shots, 0, "a wall between: {}", bot.report());
        assert_eq!(health_of(&app, behind), ZombieKind::Walker.traits().health);
        // Out in the open it would; told not to, it does not.
        bot.shoot = false;
        run(&mut app, &mut bot, &Colliders::default(), 3.0);
        assert_eq!(bot.shots, 0, "{}", bot.report());
    }

    #[test]
    fn a_zombie_goes_for_the_bot_when_it_is_nearer_and_wears_it_down_until_it_is_down_and_then_it_gets_up() {
        let none = Colliders::default();
        // The player is a long way off, so the nearer one is the bot.
        let mut app = hosted(Vec2::new(300.0, 0.0), Colliders::default());
        let mut bot = join(&mut app, "Botty");
        bot.put_at(&TerrainMap::flat(0.0), Vec2::new(0.0, 0.0));
        bot.stay = true;
        bot.shoot = false;
        add_zombie(&mut app, ZombieKind::Walker, Vec2::new(3.0, 0.0), 1);
        let mut least = CLIENT_HEALTH;
        let mut went_down = false;
        for _ in 0..40 {
            run(&mut app, &mut bot, &none, 1.0);
            least = least.min(bot.health);
            went_down |= bot.mode == Mode::Down;
            if went_down {
                break;
            }
        }
        assert!(least < CLIENT_HEALTH, "the zombie never hurt the bot");
        assert!(went_down, "the bot never went down: {}", bot.report());
        assert_eq!(app.world().resource::<PlayerHealth>().current, crate::health::MAX_HEALTH, "the player was a long way off");
        assert!(bot.report().contains("DOWN"));
        // The figure lies down.
        let mut figures = app.world_mut().query::<(&Transform, &RemotePlayer)>();
        run(&mut app, &mut bot, &none, 0.5);
        let (at, _) = figures.single(app.world()).unwrap();
        assert!((at.rotation * Vec3::Y).y < 0.5, "lying, not standing: {:?}", at.rotation * Vec3::Y);
        // Left lying: the zombie does not hurt it again.
        run(&mut app, &mut bot, &none, 2.0);
        assert_eq!(bot.health, 0.0);
        // After the timeout, it is up again, with its health back, and goes back to the player.
        bot.zombies.clear();
        run(&mut app, &mut bot, &none, crate::net::RESPAWN_AFTER);
        assert_eq!(bot.health, CLIENT_HEALTH, "{}", bot.report());
        assert_ne!(bot.mode, Mode::Down);
    }

    #[test]
    fn the_bot_goes_up_the_stair_of_any_building_with_a_floor_above_to_the_player_on_it() {
        use crate::building::{plan::Layout, planned};
        let params = crate::params::GenParams::default();
        // Level ground at the height of the floor (below it is taken for water).
        let flat = TerrainMap::flat(0.0);
        let map = TerrainMap::generate(crate::MAP_SEED, &params);
        let roads = crate::roads::RoadNetwork::generate(&map, &params);
        let settlements = crate::settlement_plan::SettlementPlan::generate(&map, &roads);
        let mut tried = 0;
        for b in settlements.layouts.iter().flat_map(|l| &l.buildings) {
            let ground = crate::settlement::ground_under(&map, b);
            let mut rng = crate::settlement_plan::Rng::from_position(b.centre);
            let (layouts, model) = planned(b, &ground.site, &mut rng);
            let Some(layout): Option<&Layout> = layouts.first().filter(|l| l.storeys.len() >= 2) else { continue };
            let mut colliders = Colliders::default();
            for solid in model.solids(Vec3::ZERO, 0.0) {
                colliders.add(solid);
            }
            let Some(door) = layout.storeys[0].doors_outside.iter().find(|d| d.side == crate::building::plan::Side::Front) else { continue };
            let upstairs = layout.storeys[1].rooms.iter().max_by(|a, b| a.rect.area().total_cmp(&b.rect.area())).unwrap().rect.centre();
            let floor = layout.floor_of(1);
            // The bot starts outside the front door; the player is upstairs.
            let start = Vec2::new(door.along, layout.inside.z0 - 2.0);
            let mut pursuit = Pursuit::default();
            let (mut at, mut feet, mut falling) = (start, 0.0f32, 0.0f32);
            let goal = Vec3::new(upstairs.x, floor, upstairs.y);
            let mut arrived = false;
            for _ in 0..(120.0 / 0.05) as usize {
                let mut routes_left = 1;
                let aim = pursuit.aim(Vec3::new(at.x, feet, at.y), goal, 0.05, &mut routes_left, || 0.5, |p, f| stand(&flat, &colliders, p, f, ROUTE_BODY));
                let velocity = (aim - at).normalize_or_zero() * WALK;
                (at, feet) = walk(&flat, &colliders, at, feet, &mut falling, velocity, 0.05, BODY);
                if at.distance(upstairs) < 1.5 && (feet - floor).abs() < 0.4 {
                    arrived = true;
                    break;
                }
            }
            assert!(arrived, "{:?} at {:?}: got to {at} at height {feet}, wanted {upstairs} at {floor}", b.kind, b.centre);
            tried += 1;
            if tried >= 40 {
                break;
            }
        }
        eprintln!("climbed in {tried} houses");
        assert!(tried >= 3, "only {tried} two-storey houses to try");
    }
}

//! The game as a host: with `--host` (or `FPS_HOST=address:port`) it listens on a UDP port, lets other
//! players in (see `net`), tells each of them where everyone is twenty times a second, and puts a figure
//! in the world for each one at the place they say they are.
//!
//! The host's own player is the one at the keyboard. The world is the host's: a client is sent the seed the
//! map was made from and builds the same one for itself, then only says where it is.

use std::io::ErrorKind;
use std::net::{SocketAddr, UdpSocket};

use bevy::prelude::*;

use crate::collision::Colliders;
use crate::impact::surface_hit;
use crate::map::TerrainMap;
use crate::net::{Heard, Lobby, Packet, Passed, PlayerState, ZombieSeen, DEFAULT_PORT, EYE_HEIGHT, MAX_ZOMBIES_SEEN, SEND_RATE};
use crate::player::FpsCamera;
use crate::sound::play_after;
use crate::weapon::BulletAssets;
use crate::zombie::Zombie;

/// The most packets read in one frame, so that a flood cannot stall the game.
const MOST_PER_FRAME: usize = 512;

/// How quickly, per second, a figure closes on the place it was last told it is at.
const GLIDE: f32 = 14.0;

/// What a joined player's gun does: how much a hit takes off a zombie, how far it carries (metres), and
/// the odds of a hit at point blank, falling away with distance to the least that is left at the limit.
pub const SHOT_DAMAGE: f32 = 30.0;
pub const SHOT_RANGE: f32 = 60.0;
const HIT_NEAR: f32 = 0.9;
const HIT_FAR: f32 = 0.3;
/// How often a hit is in the head, and what that does to the damage.
const HEAD_CHANCE: f32 = 0.12;
const HEAD_DAMAGE: f32 = 2.0;
/// Where on a zombie a shot is aimed, as a share of its height, and how long its streak shows, seconds.
const CHEST: f32 = 0.6;
const TRACER_TIME: f32 = 0.07;

/// The streak of a shot, to be drawn for a moment.
struct Tracer {
    from: Vec3,
    to: Vec3,
    left: f32,
}

/// The game's end of the wire.
#[derive(Resource)]
pub struct Host {
    socket: UdpSocket,
    pub lobby: Lobby,
    since_send: f32,
    tracers: Vec<Tracer>,
    luck: u32,
}

impl Host {
    pub fn local_addr(&self) -> SocketAddr {
        self.socket.local_addr().expect("a bound socket has an address")
    }

    /// Hurts the joined player `id`; says so in the log if that put them down.
    pub fn hurt(&mut self, id: u8, damage: f32) {
        let name = self.lobby.clients.iter().find(|c| c.id == id).map(|c| c.name.clone());
        if self.lobby.hurt(id, damage) {
            info!("{} was killed by a zombie, and will be back in {} seconds", name.unwrap_or_default(), crate::net::RESPAWN_AFTER);
        }
    }

    /// A number in 0..1 (a cheap generator: it only decides what a shot hits).
    fn roll(&mut self) -> f32 {
        self.luck ^= self.luck << 13;
        self.luck ^= self.luck >> 17;
        self.luck ^= self.luck << 5;
        (self.luck >> 8) as f32 / (1u32 << 24) as f32
    }

    fn send(&self, to: SocketAddr, packet: &Packet) {
        // A full buffer or a client that has gone: either way the next snapshot says it again.
        let _ = self.socket.send_to(&packet.encode(), to);
    }
}

/// A figure standing for a player who has joined.
#[derive(Component)]
pub struct RemotePlayer(pub u8);

#[derive(Resource)]
struct FigureLook {
    cuboid: Handle<Mesh>,
    skin: Handle<StandardMaterial>,
    shirt: Handle<StandardMaterial>,
    trousers: Handle<StandardMaterial>,
}

/// Hosts the game, if asked to.
pub struct HostPlugin {
    address: Option<String>,
}

impl HostPlugin {
    /// Hosts at `address` (like `0.0.0.0:7777`; port 0 picks a free one).
    pub fn at(address: impl Into<String>) -> Self {
        HostPlugin { address: Some(address.into()) }
    }

    /// Hosts where `FPS_HOST` says, if it is set, and not at all if it is not.
    pub fn from_env() -> Self {
        HostPlugin { address: std::env::var("FPS_HOST").ok().filter(|a| !a.is_empty()).map(|a| with_port(&a)) }
    }
}

/// `address` with the default port put on if it has none: "0.0.0.0" is "0.0.0.0:7777".
pub fn with_port(address: &str) -> String {
    if address.parse::<SocketAddr>().is_ok() || address.rsplit_once(':').is_some_and(|(_, port)| port.parse::<u16>().is_ok()) {
        address.to_string()
    } else {
        format!("{address}:{DEFAULT_PORT}")
    }
}

impl Plugin for HostPlugin {
    fn build(&self, app: &mut App) {
        let Some(address) = &self.address else { return };
        let socket = match UdpSocket::bind(address).and_then(|s| s.set_nonblocking(true).map(|()| s)) {
            Ok(socket) => socket,
            Err(error) => {
                error!("could not host on {address}: {error}");
                return;
            }
        };
        let host = Host { socket, lobby: Lobby::default(), since_send: 0.0, tracers: Vec::new(), luck: 0x1F2E_3D4C };
        info!("hosting on {}: other players can join with `cargo run --bin bot -- <this machine's address>:{}`", host.local_addr(), host.local_addr().port());
        app.insert_resource(host).add_systems(Startup, make_look).add_systems(Update, (receive, broadcast, show_players).chain()).add_systems(Update, draw_tracers.run_if(resource_exists::<GizmoConfigStore>));
    }
}

fn make_look(mut commands: Commands, meshes: Option<ResMut<Assets<Mesh>>>, materials: Option<ResMut<Assets<StandardMaterial>>>) {
    let (Some(mut meshes), Some(mut materials)) = (meshes, materials) else { return };
    let paint = |materials: &mut Assets<StandardMaterial>, rgb: [f32; 3]| materials.add(StandardMaterial { base_color: Color::srgb(rgb[0], rgb[1], rgb[2]), perceptual_roughness: 0.9, ..default() });
    commands.insert_resource(FigureLook {
        cuboid: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        skin: paint(&mut materials, [0.86, 0.68, 0.55]),
        shirt: paint(&mut materials, [0.1, 0.55, 0.62]),
        trousers: paint(&mut materials, [0.18, 0.2, 0.28]),
    });
}

/// Where the player at the keyboard is: their feet and which way they face.
fn host_state(player: &Query<(&Transform, &FpsCamera)>) -> PlayerState {
    match player.single() {
        Ok((at, cam)) => PlayerState { x: at.translation.x, y: cam.floor + cam.air_height, z: at.translation.z, yaw: cam.yaw },
        Err(_) => PlayerState::default(),
    }
}

/// Reads whatever has arrived and deals with it.
#[allow(clippy::too_many_arguments)]
fn receive(
    mut commands: Commands,
    mut host: ResMut<Host>,
    map: Option<Res<TerrainMap>>,
    colliders: Option<Res<Colliders>>,
    bullets: Option<Res<BulletAssets>>,
    player: Query<(&Transform, &FpsCamera)>,
    mut zombies: Query<(&Transform, &mut Zombie)>,
) {
    let me = host_state(&player);
    let seed = map.as_ref().map_or(0, |m| m.seed);
    let mut buffer = [0u8; 1500];
    for _ in 0..MOST_PER_FRAME {
        let (length, from) = match host.socket.recv_from(&mut buffer) {
            Ok(got) => got,
            Err(e) if e.kind() == ErrorKind::WouldBlock => break,
            // A client that has gone away can make the next read fail; that is not the end of the others.
            Err(_) => continue,
        };
        let Some(packet) = Packet::decode(&buffer[..length]) else { continue };
        for heard in host.lobby.hear(from, packet, seed, me) {
            match heard {
                Heard::Reply(reply) => host.send(from, &reply),
                Heard::Joined(id, name) => info!("{name} joined as player {id}, from {from}"),
                Heard::Left(id) => info!("player {id} left"),
                Heard::Fired(id, target) => shoot(&mut commands, &mut host, id, target, map.as_deref(), colliders.as_deref(), bullets.as_deref(), &mut zombies),
                Heard::Nothing => {}
            }
        }
    }
}

/// What a joined player's shot at the zombie numbered `target` comes to: a streak, a bang, and, if it hits, a
/// wound. The host decides everything but who shot at what: the zombie must be in range and in the shooter's
/// line of sight, and how likely a hit is falls with distance.
#[allow(clippy::too_many_arguments)]
fn shoot(commands: &mut Commands, host: &mut Host, shooter: u8, target: u32, map: Option<&TerrainMap>, colliders: Option<&Colliders>, bullets: Option<&BulletAssets>, zombies: &mut Query<(&Transform, &mut Zombie)>) {
    let Some(client) = host.lobby.clients.iter().find(|c| c.id == shooter && c.alive()) else { return };
    let (name, feet) = (client.name.clone(), Vec3::new(client.state.x, client.state.y, client.state.z));
    let muzzle = feet + Vec3::Y * EYE_HEIGHT;
    let Some((at, mut zombie)) = zombies.iter_mut().find(|(_, z)| z.net_id == target && z.alive()) else { return };
    let chest = at.translation + Vec3::Y * CHEST * 1.8 * zombie.kind.traits().scale;
    let distance = muzzle.distance(chest);
    if distance > SHOT_RANGE {
        return;
    }
    // Anything in the way: the land, or something solid.
    let blocked = map.is_some_and(|m| surface_hit(m, muzzle, chest).is_some()) || colliders.is_some_and(|c| c.segment_hit(muzzle, chest).is_some());
    let hit_chance = HIT_NEAR + (HIT_FAR - HIT_NEAR) * (distance / SHOT_RANGE).clamp(0.0, 1.0);
    let hit = !blocked && host.roll() < hit_chance;
    let mut end = chest;
    if blocked {
        // The streak stops short of what is in the way.
        end = muzzle.lerp(chest, 0.5);
    } else if hit {
        let damage = if host.roll() < HEAD_CHANCE { SHOT_DAMAGE * HEAD_DAMAGE } else { SHOT_DAMAGE };
        let was_alive = zombie.alive();
        zombie.take_hit(damage);
        if was_alive && !zombie.alive() {
            info!("{name} shot a {:?}", zombie.kind);
        }
    } else {
        // A miss goes wide, past it.
        let wide = Vec3::new(host.roll() - 0.5, host.roll() - 0.5, host.roll() - 0.5) * 1.2;
        end = muzzle + (chest + wide - muzzle) * 1.3;
    }
    host.tracers.push(Tracer { from: muzzle, to: end, left: TRACER_TIME });
    if let Some(bullets) = bullets.filter(|b| !b.shot_sounds.is_empty()) {
        let pick = (host.roll() * bullets.shot_sounds.len() as f32) as usize % bullets.shot_sounds.len();
        play_after(commands, 0.0, bullets.shot_sounds[pick].clone(), 0.95 + 0.1 * host.roll(), 1.4, Some(muzzle));
    }
}

/// Draws the streaks of the shots for as long as they last.
fn draw_tracers(mut gizmos: Gizmos, mut host: ResMut<Host>, time: Res<Time>) {
    let dt = time.delta_secs();
    for tracer in &mut host.tracers {
        gizmos.line(tracer.from + Vec3::new(0.0, -0.1, 0.0), tracer.to, Color::srgb(1.0, 0.85, 0.4));
        tracer.left -= dt;
    }
    host.tracers.retain(|t| t.left > 0.0);
}

/// Tells everybody where everybody is (and each of them which zombies are about and how they are), and drops
/// those who have gone quiet.
fn broadcast(mut host: ResMut<Host>, time: Res<Time>, player: Query<(&Transform, &FpsCamera)>, zombies: Query<(&Transform, &Zombie)>) {
    host.since_send += time.delta_secs();
    if host.since_send < 1.0 / SEND_RATE {
        return;
    }
    let elapsed = std::mem::take(&mut host.since_send);
    for happened in host.lobby.pass(elapsed) {
        match happened {
            Passed::Dropped(id) => info!("player {id} stopped answering and was dropped"),
            Passed::Revived(id) => info!("player {id} is back on their feet"),
        }
    }
    let snapshot = host.lobby.snapshot(host_state(&player));
    let alive: Vec<ZombieSeen> = zombies.iter().filter(|(_, z)| z.alive()).map(|(t, z)| ZombieSeen { id: z.net_id, kind: z.kind as u8, x: t.translation.x, y: t.translation.y, z: t.translation.z }).collect();
    for client in &host.lobby.clients {
        host.send(client.addr, &snapshot);
        // The nearest of the living, as many as a packet holds.
        let here = Vec3::new(client.state.x, client.state.y, client.state.z);
        let mut near: Vec<&ZombieSeen> = alive.iter().filter(|z| Vec3::new(z.x, z.y, z.z).distance(here) <= SHOT_RANGE * 1.5).collect();
        near.sort_by(|a, b| Vec3::new(a.x, a.y, a.z).distance_squared(here).total_cmp(&Vec3::new(b.x, b.y, b.z).distance_squared(here)));
        near.truncate(MAX_ZOMBIES_SEEN);
        host.send(client.addr, &Packet::Zombies(near.into_iter().copied().collect()));
        if let Some(status) = host.lobby.status(client.id) {
            host.send(client.addr, &status);
        }
    }
}

/// A figure for each player who has joined, moving to where they are; one for each that has gone is taken away.
fn show_players(mut commands: Commands, host: Res<Host>, time: Res<Time>, look: Option<Res<FigureLook>>, mut figures: Query<(Entity, &mut Transform, &RemotePlayer)>) {
    for (entity, _, figure) in &figures {
        if host.lobby.clients.iter().all(|c| c.id != figure.0) {
            commands.entity(entity).despawn();
        }
    }
    let glide = 1.0 - (-GLIDE * time.delta_secs()).exp();
    for client in &host.lobby.clients {
        // One who is down lies on their back.
        let down = !client.alive();
        let goal = Vec3::new(client.state.x, client.state.y + if down { 0.2 } else { 0.0 }, client.state.z);
        let turn = Quat::from_rotation_y(client.state.yaw) * if down { Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2) } else { Quat::IDENTITY };
        match figures.iter_mut().find(|(_, _, f)| f.0 == client.id) {
            Some((_, mut at, _)) => {
                at.translation = at.translation.lerp(goal, glide);
                at.rotation = at.rotation.slerp(turn, glide);
            }
            None => {
                let mut figure = commands.spawn((RemotePlayer(client.id), Transform::from_translation(goal).with_rotation(turn), Visibility::default()));
                if let Some(look) = &look {
                    figure.with_children(|f| spawn_body(f, look));
                }
            }
        }
    }
}

/// The parts of a person, standing about 1.8 m tall, facing -Z.
fn spawn_body(parent: &mut ChildSpawnerCommands, look: &FigureLook) {
    let mut part = |centre: Vec3, size: Vec3, material: &Handle<StandardMaterial>| {
        parent.spawn((Mesh3d(look.cuboid.clone()), MeshMaterial3d(material.clone()), Transform::from_translation(centre).with_scale(size)));
    };
    part(Vec3::new(0.0, 1.15, 0.0), Vec3::new(0.5, 0.65, 0.28), &look.shirt);
    part(Vec3::new(0.0, 1.62, 0.0), Vec3::new(0.24, 0.26, 0.24), &look.skin);
    for side in [-1.0, 1.0] {
        part(Vec3::new(side * 0.12, 0.4, 0.0), Vec3::new(0.2, 0.8, 0.22), &look.trousers);
        part(Vec3::new(side * 0.36, 1.1, 0.0), Vec3::new(0.12, 0.6, 0.16), &look.shirt);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::HOST_ID;
    use std::time::Duration;

    #[test]
    fn an_address_gets_the_default_port_only_if_it_has_none() {
        assert_eq!(with_port("0.0.0.0"), format!("0.0.0.0:{DEFAULT_PORT}"));
        assert_eq!(with_port("0.0.0.0:9000"), "0.0.0.0:9000");
        assert_eq!(with_port("[::1]:9000"), "[::1]:9000");
        assert_eq!(with_port("myhost"), format!("myhost:{DEFAULT_PORT}"));
        assert_eq!(with_port("myhost:12"), "myhost:12");
    }

    fn game() -> App {
        let mut app = App::new();
        app.init_resource::<Time>().insert_resource(TerrainMap::flat(0.0)).add_plugins(HostPlugin::at("127.0.0.1:0"));
        let camera = FpsCamera { yaw: 0.0, pitch: 0.0, velocity: Vec2::ZERO, air_height: 0.0, vertical_speed: 0.0, stance: crate::player::Stance::Stand, eye_height: 1.8, jump_spent_standing: false, floor: 0.0, mantle: None, mantles: 0, sprint_suppressed: false, sliding: false, view_feet: 0.0 };
        app.world_mut().spawn((camera, Transform::from_xyz(20.0, 1.8, -5.0)));
        app
    }

    fn tick(app: &mut App) {
        app.world_mut().resource_mut::<Time>().advance_by(Duration::from_millis(50));
        app.update();
    }

    fn hear(socket: &UdpSocket) -> Vec<Packet> {
        let mut got = Vec::new();
        let mut buffer = [0u8; 1500];
        while let Ok(n) = socket.recv(&mut buffer) {
            got.extend(Packet::decode(&buffer[..n]));
        }
        got
    }

    #[test]
    fn a_client_over_a_real_socket_joins_is_seen_in_the_world_and_is_told_where_the_host_is() {
        let mut app = game();
        let server = app.world().resource::<Host>().local_addr();
        let client = UdpSocket::bind("127.0.0.1:0").unwrap();
        client.set_nonblocking(true).unwrap();
        client.connect(server).unwrap();
        // Rubbish first: it must not matter.
        client.send(b"not a packet").unwrap();
        client.send(&Packet::Join { name: "Tester".into() }.encode()).unwrap();
        for _ in 0..3 {
            std::thread::sleep(Duration::from_millis(10));
            tick(&mut app);
        }
        let got = hear(&client);
        let welcome = got.iter().find_map(|p| if let Packet::Welcome { id, seed, host } = p { Some((*id, *seed, *host)) } else { None }).expect("welcomed");
        assert_eq!(welcome.0, 1);
        assert!((welcome.2.x - 20.0).abs() < 1e-3 && (welcome.2.z + 5.0).abs() < 1e-3, "told where the host is: {:?}", welcome.2);
        // It says where it is; a figure turns up there, and the host's snapshots say so too.
        client.send(&Packet::Update(PlayerState { x: 30.0, y: 0.0, z: 4.0, yaw: 1.0 }).encode()).unwrap();
        for _ in 0..40 {
            std::thread::sleep(Duration::from_millis(2));
            tick(&mut app);
        }
        let mut figures = app.world_mut().query::<(&Transform, &RemotePlayer)>();
        let (at, who) = figures.single(app.world()).expect("one figure");
        assert_eq!(who.0, 1);
        assert!((at.translation - Vec3::new(30.0, 0.0, 4.0)).length() < 0.1, "{}", at.translation);
        let snapshots: Vec<_> = hear(&client).into_iter().filter_map(|p| if let Packet::Snapshot { players, .. } = p { Some(players) } else { None }).collect();
        let last = snapshots.last().expect("snapshots");
        assert_eq!(last.len(), 2);
        assert_eq!(last[0].0, HOST_ID);
        assert!((last[0].1.x - 20.0).abs() < 1e-3);
        assert!((last[1].1.x - 30.0).abs() < 1e-3);
        // Leaving takes the figure away.
        client.send(&Packet::Leave.encode()).unwrap();
        for _ in 0..5 {
            std::thread::sleep(Duration::from_millis(5));
            tick(&mut app);
        }
        assert!(figures.iter(app.world()).next().is_none());
    }

    #[test]
    fn a_client_that_goes_quiet_is_dropped_by_the_host() {
        let mut app = game();
        let server = app.world().resource::<Host>().local_addr();
        let client = UdpSocket::bind("127.0.0.1:0").unwrap();
        client.send_to(&Packet::Join { name: "Ghost".into() }.encode(), server).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        tick(&mut app);
        assert_eq!(app.world().resource::<Host>().lobby.clients.len(), 1);
        for _ in 0..(crate::net::SILENCE_LIMIT * 20.0) as usize + 10 {
            tick(&mut app);
        }
        assert!(app.world().resource::<Host>().lobby.clients.is_empty());
    }

    #[test]
    fn not_asked_to_host_the_game_listens_to_nobody() {
        let mut app = App::new();
        app.add_plugins(HostPlugin { address: None });
        assert!(app.world().get_resource::<Host>().is_none());
    }
}

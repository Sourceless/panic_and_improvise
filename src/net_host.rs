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
use crate::figure::{spawn_figure, FigureLook, Gait, Seen};
use crate::inventory::ItemKind;
use crate::loot::{spawn_drop, Drops, Dropped, Found, LootAssets, LootBox, Lying, OpenedLoot, Pickup, LOOSE};
use crate::map::TerrainMap;
use crate::net::{Heard, Lobby, Packet, Passed, PlayerState, Request, ShotInfo, WorldEvent, ZombieSeen, DEFAULT_PORT, MAX_ZOMBIES_SEEN, SEND_RATE};
use crate::player::FpsCamera;
use crate::settlement::Buildings;
use crate::sync::{dropped_event, open_box, remove_item, WorldChange};
use crate::weapon::{launch, shot_heard, BulletAssets, ShotFired};
use crate::weapons::WeaponKind;
use crate::zombie::Zombie;

/// The most packets read in one frame, so that a flood cannot stall the game.
const MOST_PER_FRAME: usize = 512;

/// How quickly, per second, a figure closes on the place it was last told it is at.
const GLIDE: f32 = 14.0;

/// How far from a joined player, metres, the zombies it is told of are.
pub const SEEN_RANGE: f32 = 90.0;

/// The game's end of the wire.
#[derive(Resource)]
pub struct Host {
    socket: UdpSocket,
    pub lobby: Lobby,
    since_send: f32,
    luck: u32,
    /// Shots heard, and what clients have asked for, waiting to be seen to by the systems that have what is needed.
    shots: Vec<(u8, crate::net::ShotInfo)>,
    asked: Vec<(u8, crate::net::Request)>,
}

impl Host {
    /// Opens a socket at `address` (like `0.0.0.0:7777`; port 0 picks a free one) to be hosted on.
    pub fn bind(address: &str) -> Result<Host, String> {
        let socket = UdpSocket::bind(address).and_then(|s| s.set_nonblocking(true).map(|()| s)).map_err(|e| format!("could not host on {address}: {e}"))?;
        let host = Host { socket, lobby: Lobby::default(), since_send: 0.0, luck: 0x1F2E_3D4C, shots: Vec::new(), asked: Vec::new() };
        info!("hosting on {}: other players can join with `cargo run --bin bot -- <this machine's address>:{}`", host.local_addr(), host.local_addr().port());
        Ok(host)
    }

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
        // The game can be hosted from the start (as asked on the command line) or from the title screen, which
        // puts a `Host` in the world when it is chosen; either way, the systems wait for one.
        let hosting = resource_exists::<Host>;
        app.add_message::<WorldChange>().add_message::<ShotFired>().init_resource::<OpenedLoot>().init_resource::<Drops>()
            .add_systems(Update, (receive, collect_changes, fire_shots, answer_requests, tell_of_my_shots, broadcast, show_players).chain().run_if(hosting));
        let Some(address) = &self.address else { return };
        match Host::bind(address) {
            Ok(host) => {
                app.insert_resource(host);
            }
            Err(error) => error!("{error}"),
        }
    }
}

/// Where the player at the keyboard is: their feet and which way they face.
fn host_state(player: &Query<(&Transform, &FpsCamera)>) -> PlayerState {
    match player.single() {
        Ok((at, cam)) => PlayerState { x: at.translation.x, y: cam.floor + cam.air_height, z: at.translation.z, yaw: cam.yaw },
        Err(_) => PlayerState::default(),
    }
}

/// Reads whatever has arrived and deals with it.
fn receive(mut host: ResMut<Host>, map: Option<Res<TerrainMap>>, player: Query<(&Transform, &FpsCamera)>) {
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
                Heard::Joined(id, name) => info!("{name} joined as player {id}, from {from} ({} things have happened in the world since the start)", host.lobby.log.len()),
                Heard::Left(id) => info!("player {id} left"),
                Heard::Shot(id, shot) => {
                    // Everyone else sees and hears it, and the host sends it on its way.
                    let told = Packet::Fired { player: id, shot };
                    for client in host.lobby.clients.iter().filter(|c| c.id != id) {
                        host.send(client.addr, &told);
                    }
                    host.shots.push((id, shot));
                }
                Heard::Asked(id, request) => host.asked.push((id, request)),
                Heard::Nothing => {}
            }
        }
    }
}

/// What is written down as having happened in the world (by what the host's player did) goes in the book that every
/// client is told from.
fn collect_changes(mut host: ResMut<Host>, mut changes: MessageReader<WorldChange>) {
    for WorldChange(event) in changes.read() {
        host.lobby.push_event(*event);
    }
}

/// Sends the shots that clients have fired on their way, as the host's own would be: the same bullets, flying the
/// same, hitting what they hit and doing what they do. What is claimed is only believed as far as it makes sense.
fn fire_shots(mut commands: Commands, mut host: ResMut<Host>, bullets: Option<Res<BulletAssets>>) {
    for (_, shot) in std::mem::take(&mut host.shots) {
        let (Some(weapon), Some(ammo)) = (WeaponKind::from_code(shot.weapon), crate::ammo::AmmoKind::from_code(shot.ammo)) else { continue };
        // The load must be one the gun takes.
        if ammo.def().caliber != weapon.def().default_ammo.def().caliber {
            continue;
        }
        let Some(direction) = Vec3::from_array(shot.direction).try_normalize() else { continue };
        let fired = ShotFired { weapon, ammo, origin: Vec3::from_array(shot.origin), direction, scale: shot.scale.max(1.0) };
        let Some(bullets) = bullets.as_deref() else { continue };
        launch(&mut commands, bullets, &fired, || host.roll());
        shot_heard(&mut commands, bullets, &fired, || host.roll());
    }
}

/// Tells the clients of the shots that the host's own player fires, for them to see and hear.
fn tell_of_my_shots(host: Res<Host>, mut shots: MessageReader<ShotFired>) {
    for shot in shots.read() {
        let told = Packet::Fired { player: crate::net::HOST_ID, shot: ShotInfo { weapon: shot.weapon.code(), ammo: shot.ammo.code(), origin: shot.origin.to_array(), direction: shot.direction.to_array(), scale: shot.scale } };
        for client in &host.lobby.clients {
            host.send(client.addr, &told);
        }
    }
}

/// Does what clients have asked for, if it can be done: each thing is done in the world as it would be for the host's
/// own player, and written down, for everybody to be told. (The host believes what it is asked as far as it
/// makes sense; this is for playing with friends, not strangers.)
#[allow(clippy::too_many_arguments)]
fn answer_requests(
    mut commands: Commands,
    mut host: ResMut<Host>,
    buildings: Option<ResMut<Buildings>>,
    loot: Option<Res<LootAssets>>,
    mut opened: ResMut<OpenedLoot>,
    crates: Query<(Entity, &LootBox)>,
    mut pickups: Query<(Entity, &GlobalTransform, &mut Pickup)>,
    mut drops: ResMut<Drops>,
    colliders: Option<ResMut<Colliders>>,
    meshes: Option<ResMut<Assets<Mesh>>>,
    sounds: Option<Res<crate::windows::GlassSounds>>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
) {
    if host.asked.is_empty() {
        return;
    }
    let (Some(mut buildings), Some(loot)) = (buildings, loot) else { return };
    let (mut colliders, mut meshes) = (colliders, meshes);
    let ear = camera.single().ok().map(|c| c.translation());
    // Things that are put down (a box opened, a thing thrown) are not there to be taken until the frame is over; a
    // request to take one waits for the next.
    let mut put_down = false;
    let mut asked = std::mem::take(&mut host.asked).into_iter();
    while let Some((id, request)) = asked.next() {
        if put_down && matches!(request, Request::Take { .. }) {
            host.asked.push((id, request));
            host.asked.extend(asked.by_ref());
            break;
        }
        let Some(client) = host.lobby.clients.iter().find(|c| c.id == id) else { continue };
        let feet = Vec3::new(client.state.x, client.state.y, client.state.z);
        match request {
            Request::Open { building, spot } => {
                if open_box(&mut commands, &buildings, &loot, &mut opened, &crates, building as usize, spot as usize).is_some() {
                    host.lobby.push_event(WorldEvent::Opened { building, spot });
                    put_down = true;
                }
            }
            Request::Take { key, took } => {
                if let Some(found) = remove_item(&mut commands, &mut opened, &mut pickups, key, took as u32) {
                    host.lobby.push_event(WorldEvent::Taken { key, by: id, took: found.count.min(u16::MAX as u32) as u16 });
                }
            }
            Request::Throw { item, count, loaded, at } => {
                let Some(kind) = ItemKind::from_code(item) else { continue };
                // It is put down near whoever threw it, wherever they say.
                let at = Vec3::from_array(at);
                let at = if at.distance(feet) <= 6.0 { at } else { feet };
                let found = Found { kind, count: (count as u32).clamp(1, 999), loaded: loaded as u32 };
                let lying = Lying { drop: Dropped { rarity: crate::loot::rarity_of(kind), found }, at, yaw: 0.0 };
                let spot = (LOOSE, drops.0);
                drops.0 += 1;
                spawn_drop(&mut commands, &loot, None, &lying, spot, 0);
                host.lobby.push_event(dropped_event(&lying, spot, 0));
                put_down = true;
            }
            Request::Glass { building, pane } => {
                if let (Some(colliders), Some(meshes)) = (colliders.as_deref_mut(), meshes.as_deref_mut()) {
                    if crate::windows::break_pane_at(building as usize, pane as usize, &mut commands, colliders, &mut buildings, meshes, sounds.as_deref(), ear) {
                        host.lobby.push_event(WorldEvent::Glass { building, pane });
                    }
                }
            }
        }
    }
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
    // Those that are down, and not yet gone, as well: so that they are seen to fall.
    let seen: Vec<ZombieSeen> = zombies
        .iter()
        .map(|(t, z)| {
            let health = if z.alive() { (100.0 * z.health / z.kind.traits().health).ceil().clamp(1.0, 100.0) as u8 } else { 0 };
            ZombieSeen { id: z.net_id, kind: z.kind as u8, health, x: t.translation.x, y: t.translation.y, z: t.translation.z }
        })
        .collect();
    for client in &host.lobby.clients {
        host.send(client.addr, &snapshot);
        // The nearest of the living, as many as a packet holds.
        let here = Vec3::new(client.state.x, client.state.y, client.state.z);
        let mut near: Vec<&ZombieSeen> = seen.iter().filter(|z| Vec3::new(z.x, z.y, z.z).distance(here) <= SEEN_RANGE).collect();
        near.sort_by(|a, b| Vec3::new(a.x, a.y, a.z).distance_squared(here).total_cmp(&Vec3::new(b.x, b.y, b.z).distance_squared(here)));
        near.truncate(MAX_ZOMBIES_SEEN);
        host.send(client.addr, &Packet::Zombies(near.into_iter().copied().collect()));
        if let Some(events) = host.lobby.events_for(client.id) {
            host.send(client.addr, &events);
        }
        if let Some(status) = host.lobby.status(client.id) {
            host.send(client.addr, &status);
        }
    }
}

/// A player to be shown as a figure: who, where, and whether they are down.
pub struct FigureWanted {
    pub id: u8,
    pub state: PlayerState,
    pub down: bool,
}

/// A figure for each of `wanted`, moving to where they are; one for each that is no longer wanted is taken away.
pub fn show_figures(commands: &mut Commands, time: &Time, look: Option<&FigureLook>, guns: Option<&crate::weapon::WeaponAssets>, figures: &mut Query<(Entity, &mut Transform, &RemotePlayer)>, wanted: &[FigureWanted]) {
    for (entity, _, figure) in figures.iter() {
        if wanted.iter().all(|w| w.id != figure.0) {
            commands.entity(entity).despawn();
        }
    }
    let glide = 1.0 - (-GLIDE * time.delta_secs()).exp();
    for w in wanted {
        // One who is down lies on their back.
        let goal = Vec3::new(w.state.x, w.state.y + if w.down { 0.2 } else { 0.0 }, w.state.z);
        let turn = Quat::from_rotation_y(w.state.yaw) * if w.down { Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2) } else { Quat::IDENTITY };
        match figures.iter_mut().find(|(_, _, f)| f.0 == w.id) {
            Some((_, mut at, _)) => {
                at.translation = at.translation.lerp(goal, glide);
                at.rotation = at.rotation.slerp(turn, glide);
            }
            None => {
                let mut figure = commands.spawn((RemotePlayer(w.id), Transform::from_translation(goal).with_rotation(turn), Visibility::default()));
                figure.insert(Gait::default());
                if let Some(look) = look {
                    let carried = guns.map(|g| crate::figure::gun_parts(g, WeaponKind::Sterling)).unwrap_or_default();
                    figure.with_children(|f| spawn_figure(f, look, Seen::FromOutside, w.id as usize, &carried));
                }
            }
        }
    }
}

/// A figure for each player who has joined, moving to where they are; one for each that has gone is taken away.
fn show_players(mut commands: Commands, host: Res<Host>, time: Res<Time>, look: Option<Res<FigureLook>>, guns: Option<Res<crate::weapon::WeaponAssets>>, mut figures: Query<(Entity, &mut Transform, &RemotePlayer)>) {
    let wanted: Vec<FigureWanted> = host.lobby.clients.iter().map(|c| FigureWanted { id: c.id, state: c.state, down: !c.alive() }).collect();
    show_figures(&mut commands, &time, look.as_deref(), guns.as_deref(), &mut figures, &wanted);
}

/// The parts of a person, standing about 1.8 m tall, facing -Z.

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
        let camera = FpsCamera { yaw: 0.0, pitch: 0.0, velocity: Vec2::ZERO, air_height: 0.0, vertical_speed: 0.0, stance: crate::player::Stance::Stand, eye_height: 1.8, jump_spent_standing: false, floor: 0.0, mantle: None, mantles: 0, sprint_suppressed: false, sliding: false, view_feet: 0.0, lean: 0.0, lean_offset: Vec2::ZERO };
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

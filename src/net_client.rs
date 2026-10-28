//! The game as a client: joins a game somebody else is hosting (see `net_host`), builds the map the host
//! made from the seed in the welcome, and plays in it as one of its players.
//!
//! The host is the authority on the world. Here the zombies are only shown, where the host says they are, and
//! the host is told which one a shot was aimed at and works out whether it hits; how hurt this player is
//! comes from the host too. The player says where they are twenty times a second and sees every other player
//! as a figure.
//!
//! Joining is in two parts. [`Connection::join`] is the handshake, which needs no world and which the title
//! screen does while it shows "Connecting"; the connection is then kept alive (a thread tells the host the
//! player is still there) while the world is made, which takes longer than the host would wait. Then
//! `ClientPlugin` plays the game over it.

use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bevy::prelude::*;

use crate::figure::FigureLook;
use crate::health::PlayerHealth;
use crate::map::TerrainMap;
use crate::net::{Packet, PlayerState, ZombieSeen, HOST_ID, SEND_RATE, SILENCE_LIMIT};
use crate::net_host::{show_figures, FigureWanted};
use crate::player::FpsCamera;
use crate::weapon::Gun;
use crate::zombie::Zombie;

/// How often, seconds, the host is told the player is still there while the world is being made.
const KEEP_ALIVE: f32 = 0.1;

/// How long, seconds, to wait for each answer before asking again.
const ASK_EVERY: Duration = Duration::from_millis(500);

/// How quickly, per second, a figure closes on the place it was last told it is at.
const GLIDE: f32 = 14.0;

/// How far off the line of fire, metres, a zombie can be and still be the one a shot was aimed at (a body is
/// about half a metre across).
const AIM_SLACK: f32 = 0.9;

/// How far a shot reaches, metres: the same as the host's rule, so as not to ask about what it would turn down.
const REACH: f32 = crate::net_host::SHOT_RANGE;

/// A game that has let this player in: the way to it, who this player is there, the map to make and where
/// the host's player was when it did.
pub struct Connection {
    pub socket: UdpSocket,
    pub server: SocketAddr,
    pub name: String,
    pub id: u8,
    pub seed: u64,
    pub host: PlayerState,
}

impl Connection {
    /// Asks the host at `server` to let this player in as `name`, trying again every half second until
    /// `patience` runs out or `cancel` is set.
    pub fn join(server: impl ToSocketAddrs, name: &str, patience: Duration, cancel: &AtomicBool) -> Result<Connection, String> {
        let server: SocketAddr = server.to_socket_addrs().map_err(|e| format!("no such address: {e}"))?.next().ok_or("no such address")?;
        let socket = UdpSocket::bind(if server.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" }).map_err(|e| format!("could not open a socket: {e}"))?;
        socket.connect(server).map_err(|e| format!("could not reach {server}: {e}"))?;
        socket.set_read_timeout(Some(Duration::from_millis(100))).map_err(|e| e.to_string())?;
        let started = Instant::now();
        let mut buffer = [0u8; 1500];
        while started.elapsed() < patience {
            socket.send(&Packet::Join { name: name.to_string() }.encode()).map_err(|e| format!("could not send to {server}: {e}"))?;
            // Anything that is not the answer is skipped over until the half second is up.
            let asked = Instant::now();
            while asked.elapsed() < ASK_EVERY {
                if cancel.load(Ordering::Relaxed) {
                    return Err("cancelled".into());
                }
                let Ok(n) = socket.recv(&mut buffer) else { continue };
                match Packet::decode(&buffer[..n]) {
                    Some(Packet::Welcome { id, seed, host }) => {
                        socket.set_nonblocking(true).map_err(|e| e.to_string())?;
                        return Ok(Connection { socket, server, name: name.to_string(), id, seed, host });
                    }
                    Some(Packet::Refused) => return Err("the game is full, or is a different version".into()),
                    _ => {}
                }
            }
        }
        Err(format!("nobody answered at {server}: is a game being hosted there?"))
    }

    /// Starts telling the host, from another thread, that this player is still there (standing where the
    /// host's player was); the world is being made, and nothing else is saying so. Dropping the returned
    /// guard stops it.
    pub fn keep_alive(&self) -> KeepAlive {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let (socket, at) = (self.socket.try_clone().ok(), self.host);
        let thread = socket.map(|socket| {
            std::thread::spawn(move || {
                while !flag.load(Ordering::Relaxed) {
                    // A full buffer or a host that has gone: either way the next one says it again.
                    let _ = socket.send(&Packet::Update(at).encode());
                    std::thread::sleep(Duration::from_secs_f32(KEEP_ALIVE));
                }
            })
        });
        KeepAlive { stop, thread }
    }
}

/// A thread keeping a connection alive; it stops when this is dropped.
pub struct KeepAlive {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for KeepAlive {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The game this one has joined, as far as this end knows.
#[derive(Resource)]
pub struct NetClient {
    pub connection: Connection,
    /// Everyone else in the game, by id, as the host last said (the host's own player is `HOST_ID`).
    pub players: Vec<(u8, PlayerState)>,
    /// The zombies the host last said were about.
    pub zombies: Vec<ZombieSeen>,
    /// How this player is, as the host last said.
    pub health: f32,
    pub respawn_in: f32,
    /// How long, seconds, since the host last said anything.
    pub silent_for: f32,
    /// Where this player was last put on the ground at the start, and where they fell, if they are down.
    placed: bool,
    fell_at: Option<Vec3>,
    since_send: f32,
    /// The number of shots the gun had fired when last looked, to tell when it has fired.
    shots_seen: u32,
    /// How many have been fired at zombies, for the screen and for tests.
    pub shots_sent: u32,
}

impl NetClient {
    pub fn new(connection: Connection) -> Self {
        NetClient {
            players: vec![(HOST_ID, connection.host)],
            connection,
            zombies: Vec::new(),
            health: crate::net::CLIENT_HEALTH,
            respawn_in: 0.0,
            silent_for: 0.0,
            placed: false,
            fell_at: None,
            since_send: 0.0,
            shots_seen: 0,
            shots_sent: 0,
        }
    }

    pub fn id(&self) -> u8 {
        self.connection.id
    }

    pub fn down(&self) -> bool {
        self.health <= 0.0
    }

    pub fn lost(&self) -> bool {
        self.silent_for >= SILENCE_LIMIT
    }

    /// Reads everything that has arrived.
    fn listen(&mut self) {
        let mut buffer = [0u8; 1500];
        while let Ok(n) = self.connection.socket.recv(&mut buffer) {
            match Packet::decode(&buffer[..n]) {
                Some(Packet::Snapshot { players, .. }) => {
                    self.players = players;
                    self.silent_for = 0.0;
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

    fn say(&self, packet: &Packet) {
        let _ = self.connection.socket.send(&packet.encode());
    }

    /// Tells the host this player is going.
    pub fn leave(&self) {
        self.say(&Packet::Leave);
    }
}

/// The figure of a zombie as it is on a client: where the host says it is, with none of its own thinking.
#[derive(Component)]
pub struct Puppet;

pub struct ClientPlugin;

impl Plugin for ClientPlugin {
    fn build(&self, app: &mut App) {
        let joined = resource_exists::<NetClient>;
        app.add_systems(PreUpdate, hear_the_host.run_if(joined))
            .add_systems(Update, (place_at_the_start, tell_the_host, show_other_players, show_zombies, follow_status, fire_at_zombies).chain().run_if(joined))
            .add_systems(Last, say_goodbye.run_if(joined));
    }
}

/// Whether the game is one that is joined, rather than one that is the host's or played alone: for what is
/// done on the host's side only.
pub fn not_a_client(client: Option<Res<NetClient>>) -> bool {
    client.is_none()
}

fn hear_the_host(mut client: ResMut<NetClient>, time: Res<Time>) {
    client.silent_for += time.delta_secs();
    client.listen();
}

/// Where the player stands: their feet and which way they face.
fn my_state(player: &Query<(&Transform, &FpsCamera)>) -> Option<PlayerState> {
    let (at, cam) = player.single().ok()?;
    Some(PlayerState { x: at.translation.x, y: cam.floor + cam.air_height, z: at.translation.z, yaw: cam.yaw })
}

/// Puts the player down beside the host's, the first thing: the start of the map is nowhere near them.
fn place_at_the_start(mut client: ResMut<NetClient>, map: Res<TerrainMap>, colliders: Option<Res<crate::collision::Colliders>>, mut player: Query<(&mut Transform, &mut FpsCamera)>) {
    if client.placed {
        return;
    }
    let Ok((mut at, mut camera)) = player.single_mut() else { return };
    let host = client.connection.host;
    let here = Vec2::new(host.x, host.z);
    let id = client.id() as f32;
    // A few metres from them, on their floor, somewhere to stand.
    let spot = (0..32).find_map(|i| {
        let p = here + Vec2::from_angle(i as f32 * 0.7 + id) * (3.0 + (i / 8) as f32 * 1.5);
        colliders.as_deref().and_then(|c| crate::walking::stand(&map, c, p, host.y, 0.29)).map(|feet| (p, feet))
    });
    let (p, feet) = spot.unwrap_or((here, host.y));
    camera.restart_at(&mut at, p, &map);
    at.translation = Vec3::new(p.x, feet + camera.eye_height, p.y);
    camera.floor = feet;
    camera.view_feet = feet;
    camera.air_height = 0.0;
    client.placed = true;
}

/// Says where the player is, at the usual rate.
fn tell_the_host(mut client: ResMut<NetClient>, time: Res<Time>, player: Query<(&Transform, &FpsCamera)>) {
    client.since_send += time.delta_secs();
    if client.since_send < 1.0 / SEND_RATE {
        return;
    }
    client.since_send = 0.0;
    if !client.placed {
        return;
    }
    if let Some(state) = my_state(&player) {
        client.say(&Packet::Update(state));
    }
}

/// A figure for every other player, moving to where the host says they are.
fn show_other_players(mut commands: Commands, client: Res<NetClient>, time: Res<Time>, look: Option<Res<FigureLook>>, guns: Option<Res<crate::weapon::WeaponAssets>>, mut figures: Query<(Entity, &mut Transform, &crate::net_host::RemotePlayer)>) {
    let me = client.id();
    let wanted: Vec<FigureWanted> = client.players.iter().filter(|(id, _)| *id != me).map(|(id, state)| FigureWanted { id: *id, state: *state, down: false }).collect();
    show_figures(&mut commands, &time, look.as_deref(), guns.as_deref(), &mut figures, &wanted);
}

/// A zombie for each the host has told of, where it says it is; those it no longer mentions are taken away.
/// They do nothing of their own.
fn show_zombies(mut commands: Commands, client: Res<NetClient>, time: Res<Time>, assets: Option<Res<crate::zombie::ZombieAssets>>, mut puppets: Query<(Entity, &mut Transform, &mut Zombie), With<Puppet>>) {
    for (entity, _, zombie) in &puppets {
        if client.zombies.iter().all(|z| z.id != zombie.net_id) {
            commands.entity(entity).despawn();
        }
    }
    let glide = 1.0 - (-GLIDE * time.delta_secs()).exp();
    for seen in &client.zombies {
        let goal = Vec3::new(seen.x, seen.y, seen.z);
        match puppets.iter_mut().find(|(_, _, z)| z.net_id == seen.id) {
            Some((_, mut at, mut zombie)) => {
                let step = goal - at.translation;
                // It faces the way it is going.
                if step.xz().length() > 0.02 {
                    let yaw = (-step.x).atan2(-step.z);
                    at.rotation = at.rotation.slerp(Quat::from_rotation_y(yaw), glide);
                }
                at.translation += step * glide;
                // Whatever a shot here did, it is the host that says what the zombie is now.
                zombie.health = zombie.kind.traits().health;
            }
            None => {
                if let Some(assets) = &assets {
                    if let Some(kind) = crate::zombie::ZombieKind::from_index(seen.kind as usize) {
                        let id = crate::zombie::spawn_zombie(&mut commands, assets, kind, goal, 0.0, seen.id);
                        commands.entity(id).insert(Puppet);
                    }
                }
            }
        }
    }
}

/// How hurt the player is is the host's to say: the health shown is the host's, and one who is down stays
/// where they fell until the host gets them up.
fn follow_status(mut client: ResMut<NetClient>, mut health: ResMut<PlayerHealth>, mut player: Query<(&mut Transform, &mut FpsCamera)>) {
    if client.down() {
        health.current = 0.0;
        let Ok((mut at, mut camera)) = player.single_mut() else { return };
        let fell = *client.fell_at.get_or_insert(at.translation);
        at.translation = fell;
        camera.velocity = Vec2::ZERO;
        camera.vertical_speed = 0.0;
    } else {
        client.fell_at = None;
        // The host's word for it, but not above what is already shown (a bandage takes a while to work).
        if health.current > 0.0 && (client.health - health.current).abs() > 0.5 {
            health.current = client.health.min(health.max);
        } else if health.current <= 0.0 {
            health.current = client.health.min(health.max);
        }
    }
}

/// When the gun has been fired, tells the host which zombie the shot was aimed at, if it was aimed at one.
fn fire_at_zombies(mut client: ResMut<NetClient>, guns: Query<&Gun>, eye: Query<&GlobalTransform, With<FpsCamera>>, puppets: Query<(&Transform, &Zombie), With<Puppet>>) {
    let (Ok(gun), Ok(eye)) = (guns.single(), eye.single()) else { return };
    let fired = gun.shots_fired > client.shots_seen;
    client.shots_seen = gun.shots_fired;
    if !fired || client.down() {
        return;
    }
    let (origin, aim) = (eye.translation(), eye.forward().as_vec3());
    let target = puppets
        .iter()
        .filter(|(_, z)| z.alive())
        .filter_map(|(at, z)| {
            let chest = at.translation + Vec3::Y * 1.1 * z.kind.traits().scale;
            let to = chest - origin;
            let along = to.dot(aim);
            // Ahead of the player, within reach, and near the line of fire.
            let off = (to - aim * along).length();
            (along > 0.0 && along <= REACH && off <= AIM_SLACK).then_some((off, z.net_id))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0));
    if let Some((_, id)) = target {
        client.say(&Packet::Fire { target: id });
        client.shots_sent += 1;
    }
}

/// Tells the host the player is leaving, when the game is.
fn say_goodbye(client: Res<NetClient>, mut exits: MessageReader<AppExit>) {
    if exits.read().next().is_some() {
        client.leave();
    }
}

/// What to say over the view of a game that has been joined: who is there, and anything wrong.
pub fn status_line(client: &NetClient) -> Option<String> {
    if client.lost() {
        Some("Lost the connection to the host.".to_string())
    } else if client.down() {
        Some(format!("You are down. Back on your feet in {:.0} s.", client.respawn_in.ceil()))
    } else {
        None
    }
}

/// Over the view of a game that has been joined: who is there, and anything wrong with the connection.
pub fn draw_client_status(mut contexts: bevy_egui::EguiContexts, client: Option<Res<NetClient>>) -> Result {
    use bevy_egui::egui;
    let Some(client) = client else { return Ok(()) };
    let ctx = contexts.ctx_mut()?;
    egui::Area::new(egui::Id::new("client_status")).anchor(egui::Align2::RIGHT_TOP, egui::vec2(-12.0, 10.0)).show(ctx, |ui| {
        let others = client.players.iter().filter(|(id, _)| *id != client.id()).count();
        ui.label(egui::RichText::new(format!("{} {}", others + 1, if others == 0 { "player" } else { "players" })).size(14.0).color(egui::Color32::from_rgb(200, 200, 190)));
    });
    if let Some(line) = status_line(&client) {
        egui::Area::new(egui::Id::new("client_alert")).anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 80.0)).show(ctx, |ui| {
            ui.label(egui::RichText::new(line).size(26.0).strong().color(egui::Color32::from_rgb(235, 90, 70)));
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net_host::{Host, HostPlugin, RemotePlayer};
    use crate::weapons::WeaponKind;
    use crate::zombie::{ZombieAssets, ZombieKind};

    fn camera_at(x: f32, z: f32) -> impl Bundle {
        let camera = FpsCamera { yaw: 0.0, pitch: 0.0, velocity: Vec2::ZERO, air_height: 0.0, vertical_speed: 0.0, stance: crate::player::Stance::Stand, eye_height: 1.6, jump_spent_standing: false, floor: 0.0, mantle: None, mantles: 0, sprint_suppressed: false, sliding: false, view_feet: 0.0, lean: 0.0, lean_offset: Vec2::ZERO };
        (camera, Transform::from_xyz(x, 1.6, z), GlobalTransform::from(Transform::from_xyz(x, 1.6, z)))
    }

    fn tick(app: &mut App) {
        app.world_mut().resource_mut::<Time>().advance_by(Duration::from_millis(50));
        app.update();
    }

    /// A hosted game whose player stands at (20, -5) with a walker, numbered 7, ten metres north of where the client
    /// will be put; and a client that has joined it, standing nowhere in particular.
    fn game() -> (App, App) {
        let mut host = App::new();
        host.init_resource::<Time>().insert_resource(TerrainMap::flat(0.0)).add_plugins(HostPlugin::at("127.0.0.1:0"));
        host.world_mut().spawn(camera_at(20.0, -5.0));
        host.world_mut().spawn((Zombie::numbered(ZombieKind::Walker, 0.0, 7), Transform::from_xyz(20.0, 0.0, -17.0)));
        let server = host.world().resource::<Host>().local_addr();
        // The handshake blocks until the host answers, so the host is kept going meanwhile.
        let joining = std::thread::spawn(move || Connection::join(server, "Rex", Duration::from_secs(5), &AtomicBool::new(false)));
        while !joining.is_finished() {
            tick(&mut host);
            std::thread::sleep(Duration::from_millis(5));
        }
        let connection = joining.join().unwrap().expect("let in");
        assert_eq!((connection.id, connection.seed), (1, 0));
        assert!((connection.host.x - 20.0).abs() < 1e-3);
        let mut client = App::new();
        client
            .init_resource::<Time>()
            .insert_resource(TerrainMap::flat(0.0))
            .init_resource::<PlayerHealth>()
            .insert_resource(NetClient::new(connection))
            .insert_resource(ZombieAssets { looks: vec![(Handle::default(), Handle::default()); 3], cuboid: Handle::default() })
            .add_plugins(ClientPlugin);
        client.world_mut().spawn(camera_at(0.0, 0.0));
        client.world_mut().spawn(Gun::new(WeaponKind::Sterling));
        (host, client)
    }

    fn run_both(host: &mut App, client: &mut App, frames: usize) {
        for _ in 0..frames {
            std::thread::sleep(Duration::from_millis(3));
            tick(client);
            tick(host);
        }
    }

    #[test]
    fn a_joined_game_puts_the_player_by_the_host_shows_the_other_players_and_the_hosts_zombies() {
        let (mut host, mut client) = game();
        run_both(&mut host, &mut client, 60);
        // The player is put down near the host's, not at the start of the map.
        let mut me = client.world_mut().query::<(&Transform, &FpsCamera)>();
        let at = me.single(client.world()).unwrap().0.translation;
        assert!(Vec2::new(at.x, at.z).distance(Vec2::new(20.0, -5.0)) < 8.0, "{at}");
        // The host, as a figure; and the host has one of the client, where the client says it is.
        let mut figures = client.world_mut().query::<&RemotePlayer>();
        assert_eq!(figures.iter(client.world()).map(|f| f.0).collect::<Vec<_>>(), vec![0]);
        let mut theirs = host.world_mut().query::<(&Transform, &RemotePlayer)>();
        let (there, who) = theirs.single(host.world()).unwrap();
        assert_eq!(who.0, 1);
        assert!(there.translation.xz().distance(at.xz()) < 0.5, "{} against {at}", there.translation);
        // The zombie, with none of its own thinking: it is where the host has it.
        let mut puppets = client.world_mut().query::<(&Transform, &Zombie, &Puppet)>();
        let (seen, zombie, _) = puppets.single(client.world()).expect("the host's zombie");
        assert_eq!(zombie.net_id, 7);
        assert!((seen.translation - Vec3::new(20.0, 0.0, -17.0)).length() < 0.3, "{}", seen.translation);
    }

    #[test]
    fn a_shot_aimed_at_a_zombie_is_told_to_the_host_which_decides_whether_it_hits() {
        let (mut host, mut client) = game();
        run_both(&mut host, &mut client, 60);
        // Face the zombie, which is straight ahead (north is -z), and fire until it has been hurt.
        let mut me = client.world_mut().query::<(&mut Transform, &mut GlobalTransform, &FpsCamera)>();
        {
            let (mut at, mut global, _) = me.single_mut(client.world_mut()).unwrap();
            at.translation = Vec3::new(20.0, 1.6, -3.0);
            *global = GlobalTransform::from(*at);
        }
        run_both(&mut host, &mut client, 5);
        let health = |host: &mut App| host.world_mut().query::<&Zombie>().single(host.world()).unwrap().health;
        let before = health(&mut host);
        for _ in 0..12 {
            client.world_mut().query::<&mut Gun>().single_mut(client.world_mut()).unwrap().shots_fired += 1;
            // Let the host hear that it was fired, and its shot cool down.
            for _ in 0..8 {
                run_both(&mut host, &mut client, 1);
                std::thread::sleep(Duration::from_millis(40));
            }
        }
        // (Told of until the zombie is dead, and then there is nothing to aim at.)
        assert!(client.world().resource::<NetClient>().shots_sent >= 3, "{} told", client.world().resource::<NetClient>().shots_sent);
        assert!(health(&mut host) < before, "the host's zombie took a hit");
    }

    #[test]
    fn a_shot_at_nothing_in_particular_is_not_told() {
        let (mut host, mut client) = game();
        run_both(&mut host, &mut client, 60);
        // Facing the way with nothing in it (the zombie is to the north; this is looking south).
        let mut me = client.world_mut().query::<(&mut Transform, &mut GlobalTransform, &FpsCamera)>();
        {
            let (mut at, mut global, _) = me.single_mut(client.world_mut()).unwrap();
            at.rotation = Quat::from_rotation_y(std::f32::consts::PI);
            *global = GlobalTransform::from(*at);
        }
        client.world_mut().query::<&mut Gun>().single_mut(client.world_mut()).unwrap().shots_fired += 1;
        run_both(&mut host, &mut client, 5);
        assert_eq!(client.world().resource::<NetClient>().shots_sent, 0);
    }

    #[test]
    fn when_the_host_says_the_player_is_down_they_stay_where_they_fell_and_the_screen_says_so() {
        let (mut host, mut client) = game();
        run_both(&mut host, &mut client, 60);
        host.world_mut().resource_mut::<Host>().hurt(1, 1000.0);
        run_both(&mut host, &mut client, 30);
        assert!(client.world().resource::<NetClient>().down());
        assert_eq!(client.world().resource::<PlayerHealth>().current, 0.0);
        let line = status_line(client.world().resource::<NetClient>()).expect("it says so");
        assert!(line.contains("down"), "{line}");
        // Moved by something (a push, a fall), they are put back.
        let mut me = client.world_mut().query::<(&mut Transform, &FpsCamera)>();
        let fell = me.single(client.world()).unwrap().0.translation;
        me.single_mut(client.world_mut()).unwrap().0.translation += Vec3::new(5.0, 0.0, 0.0);
        run_both(&mut host, &mut client, 3);
        assert!(me.single(client.world()).unwrap().0.translation.distance(fell) < 1e-3);
    }

    #[test]
    fn a_host_that_goes_quiet_is_noticed() {
        let (mut host, mut client) = game();
        run_both(&mut host, &mut client, 20);
        assert!(!client.world().resource::<NetClient>().lost());
        // Only the client runs: the host says nothing for longer than it will wait.
        for _ in 0..(SILENCE_LIMIT * 20.0) as usize + 5 {
            tick(&mut client);
        }
        assert!(client.world().resource::<NetClient>().lost());
        assert!(status_line(client.world().resource::<NetClient>()).unwrap().contains("Lost"));
    }

    #[test]
    fn the_connection_is_kept_alive_while_the_world_is_made_and_a_cancel_gives_up_asking() {
        let mut host = App::new();
        host.init_resource::<Time>().insert_resource(TerrainMap::flat(0.0)).add_plugins(HostPlugin::at("127.0.0.1:0"));
        host.world_mut().spawn(camera_at(0.0, 0.0));
        let server = host.world().resource::<Host>().local_addr();
        let joining = std::thread::spawn(move || Connection::join(server, "Rex", Duration::from_secs(5), &AtomicBool::new(false)));
        while !joining.is_finished() {
            tick(&mut host);
            std::thread::sleep(Duration::from_millis(5));
        }
        let connection = joining.join().unwrap().unwrap();
        let alive = connection.keep_alive();
        // A long time (to the host) with nothing said but what the thread says: it is not dropped.
        for _ in 0..((SILENCE_LIMIT + 2.0) * 20.0) as usize {
            std::thread::sleep(Duration::from_millis(2));
            tick(&mut host);
        }
        assert_eq!(host.world().resource::<Host>().lobby.clients.len(), 1, "still in");
        drop(alive);
        // Nobody to ask: cancelling stops it at once, not after the patience.
        let nothing = UdpSocket::bind("127.0.0.1:0").unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let address = nothing.local_addr().unwrap();
        let asking = std::thread::spawn(move || Connection::join(address, "Rex", Duration::from_secs(60), &flag));
        std::thread::sleep(Duration::from_millis(200));
        let started = Instant::now();
        cancel.store(true, Ordering::Relaxed);
        assert!(asking.join().unwrap().is_err());
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}

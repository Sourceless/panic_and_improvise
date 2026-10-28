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
use crate::net::{Packet, PlayerState, Request, ShotInfo, WorldEvent, ZombieSeen, HOST_ID, SEND_RATE, SILENCE_LIMIT};
use crate::net_host::{show_figures, FigureWanted};
use crate::player::FpsCamera;
use crate::zombie::Zombie;

/// How often, seconds, the host is told the player is still there while the world is being made.
const KEEP_ALIVE: f32 = 0.1;

/// How long, seconds, to wait for each answer before asking again.
const ASK_EVERY: Duration = Duration::from_millis(500);

/// How quickly, per second, a figure closes on the place it was last told it is at.
const GLIDE: f32 = 14.0;

/// How often, seconds, what has been asked of the host and not yet answered is asked again.
const ASK_AGAIN: f32 = 0.25;

/// The most requests sent again in one go.
const MOST_ASKED_AGAIN: usize = 8;

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
    /// How many shots have been told to the host, for tests.
    pub shots_sent: u32,
    /// How many world events have been had (they are had in order, each once), and those not yet seen to.
    pub applied: u32,
    pub inbox: Vec<WorldEvent>,
    /// What has been asked of the host and not yet answered, numbered from one, and how long since it was asked.
    pending: Vec<(u32, Request)>,
    next_ask: u32,
    since_asked: f32,
    /// Shots that others have fired, to be shown.
    pub heard: Vec<(u8, ShotInfo)>,
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
            shots_sent: 0,
            applied: 0,
            inbox: Vec::new(),
            pending: Vec::new(),
            next_ask: 1,
            since_asked: 0.0,
            heard: Vec::new(),
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
                Some(Packet::Status { health, respawn_in, acked }) => {
                    self.health = health;
                    self.respawn_in = respawn_in;
                    self.pending.retain(|(seq, _)| *seq > acked);
                }
                Some(Packet::Fired { player, shot }) => self.heard.push((player, shot)),
                // The world events, in order, each once: what is ahead of what has been had waits to be sent again,
                // and what has been had already is skipped.
                Some(Packet::Events { first, events }) if first <= self.applied => {
                    let seen = (self.applied - first) as usize;
                    for event in events.into_iter().skip(seen) {
                        self.inbox.push(event);
                        self.applied += 1;
                    }
                    self.say(&Packet::Ack { upto: self.applied });
                }
                _ => {}
            }
        }
    }

    fn say(&self, packet: &Packet) {
        let _ = self.connection.socket.send(&packet.encode());
    }

    /// Asks the host to do something: it is asked again until the host says it has seen to it.
    pub fn ask(&mut self, request: Request) {
        let seq = self.next_ask;
        self.next_ask += 1;
        self.pending.push((seq, request));
        self.say(&Packet::Ask { seq, request });
    }

    /// Asks again what has not been answered (in order, the oldest first, so that the host does them in order).
    fn ask_again(&mut self, dt: f32) {
        self.since_asked += dt;
        if self.since_asked < ASK_AGAIN {
            return;
        }
        self.since_asked = 0.0;
        for (seq, request) in self.pending.iter().take(MOST_ASKED_AGAIN) {
            self.say(&Packet::Ask { seq: *seq, request: *request });
        }
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
        app.add_message::<crate::weapon::ShotFired>()
            .add_message::<crate::sync::WorldChange>()
            .add_systems(PreUpdate, hear_the_host.run_if(joined))
            .add_systems(Update, (place_at_the_start, tell_the_host, show_other_players, show_zombies, follow_status, tell_of_my_shots, show_their_shots, apply_events, forward_glass).chain().run_if(joined))
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
    client.ask_again(time.delta_secs());
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
    let health_of = |kind: crate::zombie::ZombieKind, seen: &ZombieSeen| if seen.health == 0 { 0.0 } else { kind.traits().health };
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
                // One that is down falls where it is (the same as it does for the host) and is not moved; whatever a
                // shot here did, it is the host that says how it is.
                if seen.health > 0 {
                    at.translation += step * glide;
                }
                zombie.health = health_of(zombie.kind, seen);
            }
            // One that has been seen for the first time, if it is not already down.
            None if seen.health > 0 => {
                if let Some(assets) = &assets {
                    if let Some(kind) = crate::zombie::ZombieKind::from_index(seen.kind as usize) {
                        let id = crate::zombie::spawn_puppet(&mut commands, assets, kind, goal, seen.id);
                        commands.entity(id).insert(Puppet);
                    }
                }
            }
            None => {}
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

/// Tells the host of each shot the player fires: what was fired, from where and which way. It is the host that sends
/// the bullets on their way and says what they hit; the ones that fly here are for the look of it.
fn tell_of_my_shots(mut client: ResMut<NetClient>, mut shots: MessageReader<crate::weapon::ShotFired>) {
    for shot in shots.read() {
        if client.down() {
            continue;
        }
        client.say(&Packet::Shot(ShotInfo { weapon: shot.weapon.code(), ammo: shot.ammo.code(), origin: shot.origin.to_array(), direction: shot.direction.to_array(), scale: shot.scale }));
        client.shots_sent += 1;
    }
}

/// Shows (and lets be heard) the shots that other players have fired.
fn show_their_shots(mut commands: Commands, mut client: ResMut<NetClient>, bullets: Option<Res<crate::weapon::BulletAssets>>, mut luck: Local<u32>) {
    let heard = std::mem::take(&mut client.heard);
    let Some(bullets) = bullets else { return };
    for (_, shot) in heard {
        let (Some(weapon), Some(ammo)) = (crate::weapons::WeaponKind::from_code(shot.weapon), crate::ammo::AmmoKind::from_code(shot.ammo)) else { continue };
        let Some(direction) = Vec3::from_array(shot.direction).try_normalize() else { continue };
        let fired = crate::weapon::ShotFired { weapon, ammo, origin: Vec3::from_array(shot.origin), direction, scale: 1.0 };
        if *luck == 0 {
            *luck = 0x2F6E_2B1;
        }
        let mut roll = || {
            *luck ^= *luck << 13;
            *luck ^= *luck >> 17;
            *luck ^= *luck << 5;
            (*luck >> 8) as f32 / (1u32 << 24) as f32
        };
        crate::weapon::launch(&mut commands, &bullets, &fired, &mut roll);
        crate::weapon::shot_heard(&mut commands, &bullets, &fired, &mut roll);
    }
}

/// Sees to what the host has said has happened in the world: a box opened, something dropped or taken, a pane broken.
#[allow(clippy::too_many_arguments)]
fn apply_events(
    mut commands: Commands,
    mut client: ResMut<NetClient>,
    buildings: Option<ResMut<crate::settlement::Buildings>>,
    loot: Option<Res<crate::loot::LootAssets>>,
    mut opened: ResMut<crate::loot::OpenedLoot>,
    crates: Query<(Entity, &crate::loot::LootBox)>,
    mut pickups: Query<(Entity, &GlobalTransform, &mut crate::loot::Pickup)>,
    mut inventory: ResMut<crate::inventory::Inventory>,
    mut thrown: ResMut<crate::inventory_ui::DropQueue>,
    mut prompt: ResMut<crate::pickup::PickupPrompt>,
    colliders: Option<ResMut<crate::collision::Colliders>>,
    meshes: Option<ResMut<Assets<Mesh>>>,
    sounds: Option<Res<crate::windows::GlassSounds>>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
) {
    // Not before there is a world to do it to.
    let (Some(mut buildings), Some(loot)) = (buildings, loot) else { return };
    if client.inbox.is_empty() {
        return;
    }
    let (mut colliders, mut meshes) = (colliders, meshes);
    let ear = camera.single().ok().map(|c| c.translation());
    let me = client.id();
    // Things that are put down are not there to be taken until the frame is over (what is done to the world waits
    // for it): an event that takes one waits for the next frame, with all that comes after it.
    let mut put_down = false;
    let mut events = std::mem::take(&mut client.inbox).into_iter();
    while let Some(event) = events.next() {
        if put_down && matches!(event, WorldEvent::Taken { .. }) {
            client.inbox.push(event);
            client.inbox.extend(events.by_ref());
            break;
        }
        match event {
            WorldEvent::Opened { building, spot } => {
                put_down |= crate::sync::open_box(&mut commands, &buildings, &loot, &mut opened, &crates, building as usize, spot as usize).is_some();
            }
            WorldEvent::Dropped { .. } => {
                crate::sync::put_down(&mut commands, &loot, &event);
                put_down = true;
            }
            WorldEvent::Taken { key, by, took } => {
                let found = crate::sync::remove_item(&mut commands, &mut opened, &mut pickups, key, took as u32);
                if let (true, Some(found)) = (by == me, found) {
                    crate::pickup::give(&mut inventory, &found, &mut thrown, &mut prompt);
                }
            }
            WorldEvent::Glass { building, pane } => {
                if let (Some(colliders), Some(meshes)) = (colliders.as_deref_mut(), meshes.as_deref_mut()) {
                    crate::windows::break_pane_at(building as usize, pane as usize, &mut commands, colliders, &mut buildings, meshes, sounds.as_deref(), ear);
                }
            }
        }
    }
}

/// A pane that the player broke by going through it is a thing for the host to know: it is asked to break it too, and
/// tells everyone.
fn forward_glass(mut client: ResMut<NetClient>, mut changes: MessageReader<crate::sync::WorldChange>) {
    for crate::sync::WorldChange(event) in changes.read() {
        if let WorldEvent::Glass { building, pane } = *event {
            client.ask(Request::Glass { building, pane });
        }
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
    use crate::collision::Colliders;
    use crate::inventory::{Inventory, ItemKind};
    use crate::loot::{Drops, LootAssets, OpenedLoot, Pickup};
    use crate::net::ItemKey;
    use crate::net_host::{Host, HostPlugin, RemotePlayer};
    use crate::weapon::{Bullet, BulletAssets, ShotFired};
    use crate::weapons::WeaponKind;
    use crate::zombie::{ZombieAssets, ZombieDice, ZombieKind};

    fn camera_at(x: f32, z: f32) -> impl Bundle {
        let camera = FpsCamera { yaw: 0.0, pitch: 0.0, velocity: Vec2::ZERO, air_height: 0.0, vertical_speed: 0.0, stance: crate::player::Stance::Stand, eye_height: 1.6, jump_spent_standing: false, floor: 0.0, mantle: None, mantles: 0, sprint_suppressed: false, sliding: false, view_feet: 0.0, lean: 0.0, lean_offset: Vec2::ZERO };
        (camera, Transform::from_xyz(x, 1.6, z), GlobalTransform::from(Transform::from_xyz(x, 1.6, z)))
    }

    fn tick(app: &mut App) {
        app.world_mut().resource_mut::<Time>().advance_by(Duration::from_millis(50));
        app.update();
    }

    fn bullet_assets() -> BulletAssets {
        BulletAssets { mesh: Handle::default(), material: Handle::default(), tracer_material: Handle::default(), shot_sound: Handle::default(), shot_sounds: Vec::new() }
    }

    /// What both ends have of the world: a flat map, a building with a window and a box of loot in it, and what is
    /// needed to put things down and send bullets.
    fn world(app: &mut App) {
        app.init_resource::<Time>()
            .insert_resource(TerrainMap::flat(0.0))
            .init_resource::<Colliders>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<OpenedLoot>()
            .init_resource::<Drops>()
            .init_resource::<crate::wind::Wind>()
            .init_resource::<ZombieDice>()
            .init_resource::<Inventory>()
            .init_resource::<crate::inventory_ui::DropQueue>()
            .init_resource::<crate::pickup::PickupPrompt>()
            .init_resource::<PlayerHealth>()
            .add_message::<crate::impact::Impact>()
            .add_message::<ShotFired>()
            .add_message::<crate::sync::WorldChange>()
            .insert_resource(LootAssets::for_tests())
            .insert_resource(bullet_assets())
            .insert_resource(ZombieAssets { looks: vec![(Handle::default(), Handle::default()); 3], cuboid: Handle::default() });
        crate::settlement::Buildings::add_one_window_and_box(app.world_mut());
    }

    /// A hosted game whose player stands at (20, -5), with a walker, numbered 7, twelve metres north of where the
    /// client will be.
    fn hosted() -> App {
        let mut host = App::new();
        world(&mut host);
        host.add_plugins(HostPlugin::at("127.0.0.1:0")).add_systems(Update, (crate::weapon::move_bullets, crate::zombie::fall_and_drop));
        host.world_mut().spawn(camera_at(20.0, -5.0));
        host.world_mut().spawn((Zombie::numbered(ZombieKind::Brute, 0.0, 7), Transform::from_xyz(20.0, 0.0, -17.0)));
        host
    }

    /// A client that has joined the game `host` is hosting, in a world of its own like it.
    fn joined(host: &mut App, name: &'static str) -> App {
        let server = host.world().resource::<Host>().local_addr();
        // The handshake blocks until the host answers, so the host is kept going meanwhile.
        let joining = std::thread::spawn(move || Connection::join(server, name, Duration::from_secs(5), &AtomicBool::new(false)));
        while !joining.is_finished() {
            tick(host);
            std::thread::sleep(Duration::from_millis(5));
        }
        let connection = joining.join().unwrap().expect("let in");
        let mut client = App::new();
        world(&mut client);
        client.insert_resource(NetClient::new(connection)).add_plugins(ClientPlugin).add_systems(Update, crate::zombie::fall_and_drop);
        client.world_mut().spawn(camera_at(0.0, 0.0));
        client
    }

    fn game() -> (App, App) {
        let mut host = hosted();
        let client = joined(&mut host, "Rex");
        (host, client)
    }

    fn run_both(host: &mut App, client: &mut App, frames: usize) {
        for _ in 0..frames {
            std::thread::sleep(Duration::from_millis(3));
            tick(client);
            tick(host);
        }
    }

    /// Puts the client's player at `x`, `z`.
    fn stand_at(client: &mut App, x: f32, z: f32) {
        let mut me = client.world_mut().query::<(&mut Transform, &mut GlobalTransform, &FpsCamera)>();
        let (mut at, mut global, _) = me.single_mut(client.world_mut()).unwrap();
        at.translation = Vec3::new(x, 1.6, z);
        *global = GlobalTransform::from(*at);
    }

    fn zombie_health(app: &mut App) -> f32 {
        app.world_mut().query::<&Zombie>().iter(app.world()).next().unwrap().health
    }

    fn things_lying(app: &mut App) -> Vec<(ItemKey, ItemKind, u32)> {
        let mut found: Vec<_> = app.world_mut().query::<&Pickup>().iter(app.world()).map(|p| (crate::sync::key_of(p.spot, p.slot), p.found.kind, p.found.count)).collect();
        found.sort_by_key(|(key, ..)| (key.a, key.b, key.slot));
        found
    }

    fn opened(app: &App) -> Vec<((usize, usize), Vec<Option<(ItemKind, u32)>>)> {
        let mut all: Vec<_> = app.world().resource::<OpenedLoot>().0.iter().map(|(spot, things)| (*spot, things.iter().map(|t| t.map(|l| (l.drop.found.kind, l.drop.found.count))).collect())).collect();
        all.sort_by_key(|(spot, _)| *spot);
        all
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
    fn a_shot_a_client_fires_is_sent_on_its_way_by_the_host_as_the_real_bullet_and_does_what_it_does() {
        let hurt_by = |weapon: WeaponKind, ammo: crate::ammo::AmmoKind| {
            let (mut host, mut client) = game();
            run_both(&mut host, &mut client, 40);
            stand_at(&mut client, 20.0, -3.0);
            run_both(&mut host, &mut client, 5);
            let before = zombie_health(&mut host);
            client.world_mut().write_message(ShotFired { weapon, ammo, origin: Vec3::new(20.0, 1.6, -3.5), direction: Vec3::NEG_Z, scale: 1.0 });
            run_both(&mut host, &mut client, 6);
            assert_eq!(client.world().resource::<NetClient>().shots_sent, 1);
            before - zombie_health(&mut host)
        };
        let nine = hurt_by(WeaponKind::Sterling, crate::ammo::AmmoKind::NineFmj);
        let buck = hurt_by(WeaponKind::Auto5, crate::ammo::AmmoKind::Buckshot);
        // A real bullet: the damage is the bullet's own, and a load of pellets does what a load of pellets does.
        assert!(nine > 15.0 && nine < 80.0, "a 9mm round did {nine}");
        assert!(buck > nine * 2.0, "buckshot did {buck} against {nine}");
    }

    #[test]
    fn a_shot_that_makes_no_sense_does_nothing_and_one_made_for_a_load_the_gun_does_not_take_is_refused() {
        let (mut host, mut client) = game();
        run_both(&mut host, &mut client, 40);
        stand_at(&mut client, 20.0, -3.0);
        run_both(&mut host, &mut client, 5);
        let before = zombie_health(&mut host);
        let socket = client.world().resource::<NetClient>().connection.socket.try_clone().unwrap();
        let shot = |weapon: u8, ammo: u8, scale: f32| Packet::Shot(ShotInfo { weapon, ammo, origin: [20.0, 1.6, -3.5], direction: [0.0, 0.0, -1.0], scale });
        // No such gun; the load of a rifle in a pistol; and an absurd amount of damage.
        for packet in [shot(99, 0, 1.0), shot(WeaponKind::Sterling.code(), crate::ammo::AmmoKind::NatoBall.code(), 1.0), shot(WeaponKind::Sterling.code(), 0, 1000.0)] {
            socket.send(&packet.encode()).unwrap();
            std::thread::sleep(Duration::from_millis(40));
            run_both(&mut host, &mut client, 3);
        }
        assert_eq!(zombie_health(&mut host), before);
    }

    #[test]
    fn the_shots_of_the_others_are_shown_to_a_client_the_hosts_and_those_of_another_client() {
        let mut host = hosted();
        let mut one = joined(&mut host, "One");
        let mut two = joined(&mut host, "Two");
        let bullets = |app: &mut App| app.world_mut().query::<&Bullet>().iter(app.world()).count();
        for _ in 0..30 {
            std::thread::sleep(Duration::from_millis(3));
            tick(&mut one);
            tick(&mut two);
            tick(&mut host);
        }
        stand_at(&mut one, 20.0, -3.0);
        // The host's own shot: both see a load of pellets.
        host.world_mut().write_message(ShotFired { weapon: WeaponKind::Auto5, ammo: crate::ammo::AmmoKind::Buckshot, origin: Vec3::new(20.0, 1.6, -5.0), direction: Vec3::X, scale: 1.0 });
        for _ in 0..4 {
            std::thread::sleep(Duration::from_millis(5));
            tick(&mut host);
            tick(&mut one);
            tick(&mut two);
        }
        assert_eq!((bullets(&mut one), bullets(&mut two)), (9, 9), "nine pellets each");
        // One's shot is shown to Two (and not again to One, which fired it).
        let before = bullets(&mut one);
        one.world_mut().write_message(ShotFired { weapon: WeaponKind::Sterling, ammo: crate::ammo::AmmoKind::NineFmj, origin: Vec3::new(20.0, 1.6, -3.5), direction: Vec3::X, scale: 1.0 });
        for _ in 0..6 {
            std::thread::sleep(Duration::from_millis(5));
            tick(&mut one);
            tick(&mut two);
            tick(&mut host);
        }
        assert_eq!(bullets(&mut two), 9 + 1);
        assert_eq!(bullets(&mut one), before, "not shown to the one who fired it");
    }

    #[test]
    fn a_zombie_that_is_shot_dead_falls_over_for_a_client_too_and_what_it_drops_is_where_the_host_says() {
        let (mut host, mut client) = game();
        run_both(&mut host, &mut client, 40);
        // It is up, and then it is not.
        assert!(client.world_mut().query::<&Puppet>().iter(client.world()).count() == 1);
        host.world_mut().query::<&mut Zombie>().single_mut(host.world_mut()).unwrap().health = 0.0;
        run_both(&mut host, &mut client, 12);
        let mut puppets = client.world_mut().query::<(&Transform, &Zombie, &Puppet)>();
        let (at, zombie, _) = puppets.single(client.world()).expect("the body is still there");
        assert!(!zombie.alive());
        // It has gone over on its back: its up is no longer up.
        assert!((at.rotation * Vec3::Y).y < 0.5, "{:?}", at.rotation);
        // Whatever it dropped (a brute always leaves something) lies in the same places on both, once.
        let theirs = things_lying(&mut host);
        assert!(!theirs.is_empty(), "the host's brute left something");
        assert_eq!(things_lying(&mut client), theirs);
    }

    #[test]
    fn a_client_opens_a_box_and_takes_what_is_in_it_and_the_host_and_the_client_agree_about_all_of_it() {
        let (mut host, mut client) = game();
        run_both(&mut host, &mut client, 20);
        assert!(opened(&host).is_empty() && opened(&client).is_empty());
        client.world_mut().resource_mut::<NetClient>().ask(Request::Open { building: 0, spot: 0 });
        run_both(&mut host, &mut client, 20);
        // Both have it open, with the same things in it, lying in the same places.
        assert_eq!(opened(&host).len(), 1);
        assert_eq!(opened(&host), opened(&client));
        let lying = things_lying(&mut host);
        assert!(!lying.is_empty());
        assert_eq!(things_lying(&mut client), lying);
        assert!(client.world().resource::<NetClient>().pending.is_empty(), "the host has said it has seen to it");
        // Asking again changes nothing.
        client.world_mut().resource_mut::<NetClient>().ask(Request::Open { building: 0, spot: 0 });
        run_both(&mut host, &mut client, 10);
        assert_eq!(things_lying(&mut host), lying);
        // The client takes one: it is gone from both and it is in the client's hands.
        let (key, kind, count) = lying.iter().find(|(_, kind, _)| matches!(kind, ItemKind::Ammo(_))).or(lying.first()).copied().unwrap();
        let before = format!("{:?}", client.world().resource::<Inventory>());
        client.world_mut().resource_mut::<NetClient>().ask(Request::Take { key, took: count as u16 });
        run_both(&mut host, &mut client, 20);
        assert!(things_lying(&mut host).iter().all(|(k, ..)| *k != key), "gone from the host's world");
        assert!(things_lying(&mut client).iter().all(|(k, ..)| *k != key), "and the client's");
        assert_eq!(opened(&host), opened(&client));
        assert_ne!(format!("{:?}", client.world().resource::<Inventory>()), before, "{kind:?} is in the client's inventory");
        // Somebody else cannot take it again.
        let mut other = joined(&mut host, "Other");
        other.world_mut().resource_mut::<NetClient>().ask(Request::Take { key, took: count as u16 });
        for _ in 0..30 {
            std::thread::sleep(Duration::from_millis(3));
            tick(&mut other);
            tick(&mut host);
        }
        assert_eq!(format!("{:?}", other.world().resource::<Inventory>()), format!("{:?}", Inventory::default()));
    }

    #[test]
    fn a_client_that_joins_late_ends_up_with_the_world_as_it_has_become() {
        let (mut host, mut client) = game();
        run_both(&mut host, &mut client, 20);
        // Things happen: a box is opened, something is taken, something is thrown down, a pane is broken.
        client.world_mut().resource_mut::<NetClient>().ask(Request::Open { building: 0, spot: 0 });
        run_both(&mut host, &mut client, 20);
        let (key, _, count) = things_lying(&mut host)[0];
        client.world_mut().resource_mut::<NetClient>().ask(Request::Take { key, took: count as u16 });
        client.world_mut().resource_mut::<NetClient>().ask(Request::Throw { item: ItemKind::Supply(crate::inventory::Supply::Torch).code(), count: 1, loaded: 0, at: [20.0, 0.0, -4.0] });
        client.world_mut().resource_mut::<NetClient>().ask(Request::Glass { building: 0, pane: 0 });
        run_both(&mut host, &mut client, 30);
        assert!(host.world().resource::<Host>().lobby.log.len() >= 4, "{:?}", host.world().resource::<Host>().lobby.log);
        // Someone arrives, long after.
        let mut late = joined(&mut host, "Late");
        for _ in 0..80 {
            std::thread::sleep(Duration::from_millis(3));
            tick(&mut late);
            tick(&mut host);
        }
        assert_eq!(opened(&late), opened(&host));
        assert_eq!(things_lying(&mut late), things_lying(&mut host));
        assert!(things_lying(&mut late).iter().any(|(_, kind, _)| *kind == ItemKind::Supply(crate::inventory::Supply::Torch)), "what was thrown down is there");
        assert!(late.world().resource::<crate::settlement::Buildings>().whole_pane(0, 0).is_none(), "the broken window is broken");
        assert!(host.world().resource::<crate::settlement::Buildings>().whole_pane(0, 0).is_none());
        // And a client that was there all along has the same.
        assert_eq!(things_lying(&mut client), things_lying(&mut host));
    }

    #[test]
    fn a_pane_the_host_breaks_is_broken_for_the_clients() {
        let (mut host, mut client) = game();
        run_both(&mut host, &mut client, 20);
        // The host's player goes through the window (the host says so as any pane broken there is).
        host.world_mut().write_message(crate::sync::WorldChange(WorldEvent::Glass { building: 0, pane: 0 }));
        run_both(&mut host, &mut client, 20);
        assert!(client.world().resource::<crate::settlement::Buildings>().whole_pane(0, 0).is_none());
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

//! The wire: what a game and the players joining it say to one another over UDP, and the host's book of who
//! has joined.
//!
//! The host is the game being played; it is the authority on the world, and everyone else is a client that
//! tells it where *it* is and is told where everyone is. A packet is a few bytes by hand (no crates): the
//! magic `CRWL`, a version, a tag and the fields of that tag, little-endian. Anything that does not read
//! exactly as it should is thrown away, so that whatever turns up at the port, the host carries on.
//!
//! There is no sign-in and no encryption: this is for playing on a local network and for trying things out.
//!
//! This module has no sockets in it and knows nothing of the game; `net_host` plays the host in the game and
//! `bot` is the client that follows the player about.

use std::net::SocketAddr;

/// The port a game is hosted on unless told otherwise.
pub const DEFAULT_PORT: u16 = 7777;

/// The first four bytes of every packet, and the version of what follows them.
const MAGIC: [u8; 4] = *b"CRWL";
pub const VERSION: u8 = 2;

/// How many are in a game at once, the host's player included.
pub const MAX_PLAYERS: usize = 8;

/// How often, per second, the host tells everybody where everybody is, and a client where it is.
pub const SEND_RATE: f32 = 20.0;

/// How long, seconds, a client can go unheard before it is taken to have gone.
pub const SILENCE_LIMIT: f32 = 5.0;

/// The longest name, bytes.
pub const NAME_BYTES: usize = 16;

/// The id of the host's own player.
pub const HOST_ID: u8 = 0;

/// How high, metres, a joined player's eyes are above their feet, and how much damage they can take before
/// they go down.
pub const EYE_HEIGHT: f32 = 1.6;
pub const CLIENT_HEALTH: f32 = 100.0;

/// How long, seconds, a joined player who has gone down stays down before they are back on their feet.
pub const RESPAWN_AFTER: f32 = 20.0;

/// The least time, seconds, between one shot of a joined player's and the next: well under what any gun does, so
/// that this stops a flood and no gun.
pub const SHOT_COOLDOWN: f32 = 0.03;

/// How far, metres, from where the host last heard a player is the muzzle a shot of theirs may come from.
pub const MUZZLE_SLACK: f32 = 5.0;

/// The most that a shot may be said to do more than usual (a perfect reload's first shot does 1.6).
pub const MOST_SCALE: f32 = 3.0;

/// The most world events one packet carries.
pub const EVENTS_PER_PACKET: usize = 10;

/// The most zombies one packet tells a client of.
pub const MAX_ZOMBIES_SEEN: usize = 24;

/// Nothing in the world is further than this from the middle, metres; a position beyond it is made up.
const WORLD_LIMIT: f32 = 20_000.0;

/// Just enough of a point in space for `net`, which knows nothing of the game's own types.
struct Vec3Ish(f32, f32, f32);

impl Vec3Ish {
    fn new(x: f32, y: f32, z: f32) -> Self {
        Vec3Ish(x, y, z)
    }

    fn distance(&self, other: &Vec3Ish) -> f32 {
        ((self.0 - other.0).powi(2) + (self.1 - other.1).powi(2) + (self.2 - other.2).powi(2)).sqrt()
    }
}

/// Where a player is: their feet, in the world, and which way they face (radians about the vertical).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct PlayerState {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub yaw: f32,
}

impl PlayerState {
    /// Whether this could be a real place: numbers, in the world.
    pub fn sane(&self) -> bool {
        [self.x, self.y, self.z].iter().all(|v| v.is_finite() && v.abs() <= WORLD_LIMIT) && self.yaw.is_finite()
    }
}

/// A zombie, as the host tells a client of it: a number to refer to it by, which kind (the host's own
/// numbering), how much of its health it has (a percentage; none means it is down), and where its feet are.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZombieSeen {
    pub id: u32,
    pub kind: u8,
    pub health: u8,
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// A shot: what was fired (a weapon and a load, by the game's own numbering), from where, which way, and how much
/// more than usual it does. A load of pellets is one shot; the pellets are scattered by whoever works out where it goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShotInfo {
    pub weapon: u8,
    pub ammo: u8,
    pub origin: [f32; 3],
    pub direction: [f32; 3],
    pub scale: f32,
}

impl ShotInfo {
    pub fn sane(&self) -> bool {
        let finite = |v: &[f32; 3]| v.iter().all(|c| c.is_finite() && c.abs() <= WORLD_LIMIT);
        finite(&self.origin) && finite(&self.direction) && self.scale.is_finite() && (0.0..=MOST_SCALE).contains(&self.scale)
    }
}

/// Which thing in the world: where it lies (a box and which of its things, or the group a thing was dropped in)
/// and which of the group it is. The numbers are the game's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ItemKey {
    pub a: u32,
    pub b: u32,
    pub slot: u8,
}

/// Something that happened in the world, which everyone is told, in order, and which is kept so that someone
/// who joins late is told of all of it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WorldEvent {
    /// A box (the building's number and the box's in it) has been opened.
    Opened { building: u16, spot: u8 },
    /// A thing is lying here: what it is (`item` is the game's own code), how many, how many rounds in it if it
    /// is a gun, how rare, where, and which way it points.
    Dropped { key: ItemKey, item: u16, count: u16, loaded: u8, rarity: u8, at: [f32; 3], yaw: f32 },
    /// Player `by` took `took` of what lay there: all of it, or some, if it is what that many would be left.
    Taken { key: ItemKey, by: u8, took: u16 },
    /// A pane of glass (the building's number and the pane's in it) has been broken.
    Glass { building: u16, pane: u16 },
}

/// Something a client asks the host to do, which the host answers by it happening (or not) in the world.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Request {
    Open { building: u16, spot: u8 },
    Take { key: ItemKey, took: u16 },
    /// Put a thing down at a place (the host numbers it and tells everyone).
    Throw { item: u16, count: u16, loaded: u8, at: [f32; 3] },
    Glass { building: u16, pane: u16 },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Packet {
    /// A client asks to be let in, under this name.
    Join { name: String },
    /// The host lets a client in: its id, the seed the map was made from (so that it can make the same one),
    /// and where the host's player is.
    Welcome { id: u8, seed: u64, host: PlayerState },
    /// The host will not have it: the game is full, or the client speaks another version.
    Refused,
    /// A client says where it is.
    Update(PlayerState),
    /// The host says where everyone is (the host's own player has id `HOST_ID`).
    Snapshot { tick: u32, players: Vec<(u8, PlayerState)> },
    /// A client is leaving.
    Leave,
    /// The host tells a client which zombies are about (the living ones nearest it).
    Zombies(Vec<ZombieSeen>),
    /// The host tells a client how it is: its health, and, if it is down, how long until it is back up; and how
    /// many of its requests (counting from one) the host has dealt with.
    Status { health: f32, respawn_in: f32, acked: u32 },
    /// A client fires. The host sends the shot on its way as it would its own: it is the host that works out
    /// what it hits.
    Shot(ShotInfo),
    /// The host tells the others that player `player` fired, for them to show and hear.
    Fired { player: u8, shot: ShotInfo },
    /// The host tells a client the world events numbered from `first` (counting from zero).
    Events { first: u32, events: Vec<WorldEvent> },
    /// A client says how many world events it has had, all of them in order.
    Ack { upto: u32 },
    /// A client asks for something, numbered from one: the host does them in order, and says how far it has got.
    Ask { seq: u32, request: Request },
}

const JOIN: u8 = 1;
const WELCOME: u8 = 2;
const REFUSED: u8 = 3;
const UPDATE: u8 = 4;
const SNAPSHOT: u8 = 5;
const LEAVE: u8 = 6;
const ZOMBIES: u8 = 7;
const STATUS: u8 = 8;
const SHOT: u8 = 9;
const FIRED: u8 = 10;
const EVENTS: u8 = 11;
const ACK: u8 = 12;
const ASK: u8 = 13;

fn put_vec3(out: &mut Vec<u8>, v: &[f32; 3]) {
    for c in v {
        out.extend_from_slice(&c.to_le_bytes());
    }
}

fn put_shot(out: &mut Vec<u8>, shot: &ShotInfo) {
    out.extend_from_slice(&[shot.weapon, shot.ammo]);
    put_vec3(out, &shot.origin);
    put_vec3(out, &shot.direction);
    out.extend_from_slice(&shot.scale.to_le_bytes());
}

fn put_key(out: &mut Vec<u8>, key: &ItemKey) {
    out.extend_from_slice(&key.a.to_le_bytes());
    out.extend_from_slice(&key.b.to_le_bytes());
    out.push(key.slot);
}

fn put_event(out: &mut Vec<u8>, event: &WorldEvent) {
    match event {
        WorldEvent::Opened { building, spot } => {
            out.push(1);
            out.extend_from_slice(&building.to_le_bytes());
            out.push(*spot);
        }
        WorldEvent::Dropped { key, item, count, loaded, rarity, at, yaw } => {
            out.push(2);
            put_key(out, key);
            out.extend_from_slice(&item.to_le_bytes());
            out.extend_from_slice(&count.to_le_bytes());
            out.extend_from_slice(&[*loaded, *rarity]);
            put_vec3(out, at);
            out.extend_from_slice(&yaw.to_le_bytes());
        }
        WorldEvent::Taken { key, by, took } => {
            out.push(3);
            put_key(out, key);
            out.push(*by);
            out.extend_from_slice(&took.to_le_bytes());
        }
        WorldEvent::Glass { building, pane } => {
            out.push(4);
            out.extend_from_slice(&building.to_le_bytes());
            out.extend_from_slice(&pane.to_le_bytes());
        }
    }
}

fn put_request(out: &mut Vec<u8>, request: &Request) {
    match request {
        Request::Open { building, spot } => {
            out.push(1);
            out.extend_from_slice(&building.to_le_bytes());
            out.push(*spot);
        }
        Request::Take { key, took } => {
            out.push(2);
            put_key(out, key);
            out.extend_from_slice(&took.to_le_bytes());
        }
        Request::Throw { item, count, loaded, at } => {
            out.push(3);
            out.extend_from_slice(&item.to_le_bytes());
            out.extend_from_slice(&count.to_le_bytes());
            out.push(*loaded);
            put_vec3(out, at);
        }
        Request::Glass { building, pane } => {
            out.push(4);
            out.extend_from_slice(&building.to_le_bytes());
            out.extend_from_slice(&pane.to_le_bytes());
        }
    }
}

fn put_state(out: &mut Vec<u8>, s: &PlayerState) {
    for v in [s.x, s.y, s.z, s.yaw] {
        out.extend_from_slice(&v.to_le_bytes());
    }
}

/// A reader over the bytes of a packet, which says `None` rather than reading past the end.
struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let (head, rest) = self.0.split_at_checked(n)?;
        self.0 = rest;
        Some(head)
    }

    fn byte(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }

    fn place(&mut self) -> Option<[f32; 3]> {
        let v = [self.float()?, self.float()?, self.float()?];
        v.iter().all(|c| c.is_finite() && c.abs() <= WORLD_LIMIT).then_some(v)
    }

    fn shot(&mut self) -> Option<ShotInfo> {
        let shot = ShotInfo { weapon: self.byte()?, ammo: self.byte()?, origin: self.place()?, direction: self.place()?, scale: self.float()? };
        shot.sane().then_some(shot)
    }

    fn key(&mut self) -> Option<ItemKey> {
        Some(ItemKey { a: self.u32()?, b: self.u32()?, slot: self.byte()? })
    }

    fn event(&mut self) -> Option<WorldEvent> {
        Some(match self.byte()? {
            1 => WorldEvent::Opened { building: self.u16()?, spot: self.byte()? },
            2 => {
                let key = self.key()?;
                let (item, count, loaded, rarity) = (self.u16()?, self.u16()?, self.byte()?, self.byte()?);
                let at = self.place()?;
                let yaw = self.float()?;
                if !yaw.is_finite() {
                    return None;
                }
                WorldEvent::Dropped { key, item, count, loaded, rarity, at, yaw }
            }
            3 => WorldEvent::Taken { key: self.key()?, by: self.byte()?, took: self.u16()? },
            4 => WorldEvent::Glass { building: self.u16()?, pane: self.u16()? },
            _ => return None,
        })
    }

    fn request(&mut self) -> Option<Request> {
        Some(match self.byte()? {
            1 => Request::Open { building: self.u16()?, spot: self.byte()? },
            2 => Request::Take { key: self.key()?, took: self.u16()? },
            3 => Request::Throw { item: self.u16()?, count: self.u16()?, loaded: self.byte()?, at: self.place()? },
            4 => Request::Glass { building: self.u16()?, pane: self.u16()? },
            _ => return None,
        })
    }

    fn float(&mut self) -> Option<f32> {
        Some(f32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn state(&mut self) -> Option<PlayerState> {
        let s = PlayerState { x: self.float()?, y: self.float()?, z: self.float()?, yaw: self.float()? };
        s.sane().then_some(s)
    }

    fn done(&self) -> bool {
        self.0.is_empty()
    }
}

impl Packet {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        out.push(VERSION);
        match self {
            Packet::Join { name } => {
                out.push(JOIN);
                let name = clean_name(name);
                out.push(name.len() as u8);
                out.extend_from_slice(name.as_bytes());
            }
            Packet::Welcome { id, seed, host } => {
                out.extend_from_slice(&[WELCOME, *id]);
                out.extend_from_slice(&seed.to_le_bytes());
                put_state(&mut out, host);
            }
            Packet::Refused => out.push(REFUSED),
            Packet::Update(state) => {
                out.push(UPDATE);
                put_state(&mut out, state);
            }
            Packet::Snapshot { tick, players } => {
                out.push(SNAPSHOT);
                out.extend_from_slice(&tick.to_le_bytes());
                out.push(players.len().min(MAX_PLAYERS) as u8);
                for (id, state) in players.iter().take(MAX_PLAYERS) {
                    out.push(*id);
                    put_state(&mut out, state);
                }
            }
            Packet::Leave => out.push(LEAVE),
            Packet::Zombies(list) => {
                out.push(ZOMBIES);
                out.push(list.len().min(MAX_ZOMBIES_SEEN) as u8);
                for z in list.iter().take(MAX_ZOMBIES_SEEN) {
                    out.extend_from_slice(&z.id.to_le_bytes());
                    out.extend_from_slice(&[z.kind, z.health]);
                    for v in [z.x, z.y, z.z] {
                        out.extend_from_slice(&v.to_le_bytes());
                    }
                }
            }
            Packet::Status { health, respawn_in, acked } => {
                out.push(STATUS);
                out.extend_from_slice(&health.to_le_bytes());
                out.extend_from_slice(&respawn_in.to_le_bytes());
                out.extend_from_slice(&acked.to_le_bytes());
            }
            Packet::Shot(shot) => {
                out.push(SHOT);
                put_shot(&mut out, shot);
            }
            Packet::Fired { player, shot } => {
                out.extend_from_slice(&[FIRED, *player]);
                put_shot(&mut out, shot);
            }
            Packet::Events { first, events } => {
                out.push(EVENTS);
                out.extend_from_slice(&first.to_le_bytes());
                out.push(events.len().min(EVENTS_PER_PACKET) as u8);
                for event in events.iter().take(EVENTS_PER_PACKET) {
                    put_event(&mut out, event);
                }
            }
            Packet::Ack { upto } => {
                out.push(ACK);
                out.extend_from_slice(&upto.to_le_bytes());
            }
            Packet::Ask { seq, request } => {
                out.push(ASK);
                out.extend_from_slice(&seq.to_le_bytes());
                put_request(&mut out, request);
            }
        }
        out
    }

    /// The packet in `bytes`, or `None` if they are anything other than exactly one well-formed packet of this
    /// version.
    pub fn decode(bytes: &[u8]) -> Option<Packet> {
        let mut r = Reader(bytes);
        if r.take(4)? != MAGIC || r.byte()? != VERSION {
            return None;
        }
        let packet = match r.byte()? {
            JOIN => {
                let length = r.byte()? as usize;
                if length > NAME_BYTES {
                    return None;
                }
                let name = std::str::from_utf8(r.take(length)?).ok()?.to_string();
                Packet::Join { name }
            }
            WELCOME => {
                let id = r.byte()?;
                let seed = u64::from_le_bytes(r.take(8)?.try_into().ok()?);
                Packet::Welcome { id, seed, host: r.state()? }
            }
            REFUSED => Packet::Refused,
            UPDATE => Packet::Update(r.state()?),
            SNAPSHOT => {
                let tick = r.u32()?;
                let count = r.byte()? as usize;
                if count > MAX_PLAYERS {
                    return None;
                }
                let mut players = Vec::with_capacity(count);
                for _ in 0..count {
                    players.push((r.byte()?, r.state()?));
                }
                Packet::Snapshot { tick, players }
            }
            LEAVE => Packet::Leave,
            ZOMBIES => {
                let count = r.byte()? as usize;
                if count > MAX_ZOMBIES_SEEN {
                    return None;
                }
                let mut list = Vec::with_capacity(count);
                for _ in 0..count {
                    let id = r.u32()?;
                    let (kind, health) = (r.byte()?, r.byte()?);
                    if health > 100 {
                        return None;
                    }
                    let at = PlayerState { x: r.float()?, y: r.float()?, z: r.float()?, yaw: 0.0 };
                    if !at.sane() {
                        return None;
                    }
                    list.push(ZombieSeen { id, kind, health, x: at.x, y: at.y, z: at.z });
                }
                Packet::Zombies(list)
            }
            STATUS => {
                let (health, respawn_in, acked) = (r.float()?, r.float()?, r.u32()?);
                if !(health.is_finite() && (0.0..=10_000.0).contains(&health) && respawn_in.is_finite() && (0.0..=3_600.0).contains(&respawn_in)) {
                    return None;
                }
                Packet::Status { health, respawn_in, acked }
            }
            SHOT => Packet::Shot(r.shot()?),
            FIRED => Packet::Fired { player: r.byte()?, shot: r.shot()? },
            EVENTS => {
                let first = r.u32()?;
                let count = r.byte()? as usize;
                if count > EVENTS_PER_PACKET {
                    return None;
                }
                let mut events = Vec::with_capacity(count);
                for _ in 0..count {
                    events.push(r.event()?);
                }
                Packet::Events { first, events }
            }
            ACK => Packet::Ack { upto: r.u32()? },
            ASK => Packet::Ask { seq: r.u32()?, request: r.request()? },
            _ => return None,
        };
        r.done().then_some(packet)
    }
}

/// A name a client can be known by: printable characters only, cut to fit.
pub fn clean_name(name: &str) -> String {
    let mut cleaned = String::new();
    for c in name.chars().filter(|c| !c.is_control()) {
        if cleaned.len() + c.len_utf8() > NAME_BYTES {
            break;
        }
        cleaned.push(c);
    }
    if cleaned.trim().is_empty() { "player".to_string() } else { cleaned }
}

/// One who has joined.
#[derive(Clone, Debug)]
pub struct Client {
    pub addr: SocketAddr,
    pub id: u8,
    pub name: String,
    pub state: PlayerState,
    /// How long, seconds, since anything was heard from it.
    pub silent_for: f32,
    /// How much it can take yet; at nothing it is down.
    pub health: f32,
    /// How long, seconds, it has been down.
    pub down_for: f32,
    /// How long, seconds, since it last fired.
    pub since_shot: f32,
    /// How many world events it has told the host it has had (they are sent from there on).
    pub events_acked: u32,
    /// How many of its requests the host has dealt with.
    pub requests_done: u32,
}

impl Client {
    pub fn alive(&self) -> bool {
        self.health > 0.0
    }

    /// How long until it is back on its feet, if it is down (nothing if it is up).
    pub fn respawn_in(&self) -> f32 {
        if self.alive() { 0.0 } else { (RESPAWN_AFTER - self.down_for).max(0.0) }
    }
}

/// The host's book of who is in the game, kept apart from the sockets that carry the packets so that the
/// rules can be tried without them.
#[derive(Debug, Default)]
pub struct Lobby {
    pub clients: Vec<Client>,
    tick: u32,
    /// Everything that has happened in the world that clients are told of, in order, from the start of the game:
    /// whoever joins is told it all.
    pub log: Vec<WorldEvent>,
}

/// What came of hearing from a client.
#[derive(Debug, PartialEq)]
pub enum Heard {
    /// Say this back to it.
    Reply(Packet),
    /// Somebody new is in, with this id and name.
    Joined(u8, String),
    /// This id has left, or been dropped.
    Left(u8),
    /// This id fired this shot.
    Shot(u8, ShotInfo),
    /// This id asked for this to be done (the next of its requests, in order).
    Asked(u8, Request),
    Nothing,
}

/// What came of time passing.
#[derive(Debug, PartialEq)]
pub enum Passed {
    /// This id has gone quiet and been dropped.
    Dropped(u8),
    /// This id was down and is back on its feet.
    Revived(u8),
}

impl Lobby {
    /// Deals with one packet from `from`. `seed` and `host` are what a newcomer is welcomed with.
    pub fn hear(&mut self, from: SocketAddr, packet: Packet, seed: u64, host: PlayerState) -> Vec<Heard> {
        let known = self.clients.iter().position(|c| c.addr == from);
        match (packet, known) {
            // Asking again (the welcome may have been lost): welcomed again under the same id.
            (Packet::Join { .. }, Some(i)) => {
                self.clients[i].silent_for = 0.0;
                vec![Heard::Reply(Packet::Welcome { id: self.clients[i].id, seed, host })]
            }
            (Packet::Join { name }, None) => {
                // The lowest id not in use (the host is 0).
                let Some(id) = (1..MAX_PLAYERS as u8).find(|id| self.clients.iter().all(|c| c.id != *id)) else {
                    return vec![Heard::Reply(Packet::Refused)];
                };
                let name = clean_name(&name);
                self.clients.push(Client { addr: from, id, name: name.clone(), state: host, silent_for: 0.0, health: CLIENT_HEALTH, down_for: 0.0, since_shot: SHOT_COOLDOWN, events_acked: 0, requests_done: 0 });
                vec![Heard::Reply(Packet::Welcome { id, seed, host }), Heard::Joined(id, name)]
            }
            (Packet::Update(state), Some(i)) => {
                self.clients[i].state = state;
                self.clients[i].silent_for = 0.0;
                vec![Heard::Nothing]
            }
            (Packet::Leave, Some(i)) => vec![Heard::Left(self.clients.remove(i).id)],
            // A shot, from one on its feet, near where it says it is, and not firing impossibly fast.
            (Packet::Shot(shot), Some(i)) => {
                let client = &mut self.clients[i];
                client.silent_for = 0.0;
                let eye = Vec3Ish::new(client.state.x, client.state.y + EYE_HEIGHT, client.state.z);
                let near = Vec3Ish::new(shot.origin[0], shot.origin[1], shot.origin[2]).distance(&eye) <= MUZZLE_SLACK;
                let aimed = shot.direction.iter().map(|c| c * c).sum::<f32>() > 1e-6;
                if client.alive() && client.since_shot >= SHOT_COOLDOWN && near && aimed {
                    client.since_shot = 0.0;
                    vec![Heard::Shot(client.id, shot)]
                } else {
                    vec![Heard::Nothing]
                }
            }
            // The events it has had: from here on, it is sent what comes after.
            (Packet::Ack { upto }, Some(i)) => {
                let (client, known) = (&mut self.clients[i], self.log.len() as u32);
                client.silent_for = 0.0;
                client.events_acked = client.events_acked.max(upto.min(known));
                vec![Heard::Nothing]
            }
            // A request, done once, in order: one it has asked again (the answer was lost) is not done twice, and one
            // that comes ahead of those before it waits for them to be asked again.
            (Packet::Ask { seq, request }, Some(i)) => {
                let client = &mut self.clients[i];
                client.silent_for = 0.0;
                if seq == client.requests_done + 1 {
                    client.requests_done = seq;
                    if client.alive() {
                        return vec![Heard::Asked(client.id, request)];
                    }
                }
                vec![Heard::Nothing]
            }
            // Updates from strangers, and everything only a host says: ignored.
            _ => vec![Heard::Nothing],
        }
    }

    /// Lets `dt` seconds go by: drops those unheard from for too long, and gets up those who have been down
    /// long enough.
    pub fn pass(&mut self, dt: f32) -> Vec<Passed> {
        let mut happened = Vec::new();
        for c in &mut self.clients {
            c.silent_for += dt;
            c.since_shot += dt;
            if !c.alive() {
                c.down_for += dt;
                if c.down_for >= RESPAWN_AFTER {
                    c.health = CLIENT_HEALTH;
                    c.down_for = 0.0;
                    happened.push(Passed::Revived(c.id));
                }
            }
        }
        for c in self.clients.iter().filter(|c| c.silent_for > SILENCE_LIMIT) {
            happened.push(Passed::Dropped(c.id));
        }
        self.clients.retain(|c| c.silent_for <= SILENCE_LIMIT);
        happened
    }

    /// Hurts the client `id` by `damage`. True if that put it down.
    pub fn hurt(&mut self, id: u8, damage: f32) -> bool {
        let Some(client) = self.clients.iter_mut().find(|c| c.id == id && c.alive()) else { return false };
        client.health = (client.health - damage).max(0.0);
        if client.health <= 0.0 {
            client.down_for = 0.0;
        }
        client.health <= 0.0
    }

    /// What to tell client `id` of how it is.
    pub fn status(&self, id: u8) -> Option<Packet> {
        self.clients.iter().find(|c| c.id == id).map(|c| Packet::Status { health: c.health, respawn_in: c.respawn_in(), acked: c.requests_done })
    }

    /// Something has happened in the world that everyone is to be told.
    pub fn push_event(&mut self, event: WorldEvent) {
        self.log.push(event);
    }

    /// The next of the world events that client `id` has not said it has had, as many as a packet holds.
    pub fn events_for(&self, id: u8) -> Option<Packet> {
        let client = self.clients.iter().find(|c| c.id == id)?;
        let from = client.events_acked as usize;
        (from < self.log.len()).then(|| Packet::Events { first: client.events_acked, events: self.log[from..self.log.len().min(from + EVENTS_PER_PACKET)].to_vec() })
    }

    /// Where everyone is, to send to everyone, with the host's player first.
    pub fn snapshot(&mut self, host: PlayerState) -> Packet {
        self.tick = self.tick.wrapping_add(1);
        let mut players = vec![(HOST_ID, host)];
        players.extend(self.clients.iter().map(|c| (c.id, c.state)));
        Packet::Snapshot { tick: self.tick, players }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f32, z: f32) -> PlayerState {
        PlayerState { x, y: 1.0, z, yaw: 0.5 }
    }

    fn addr(port: u16) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], port))
    }

    fn a_shot() -> ShotInfo {
        ShotInfo { weapon: 3, ammo: 1, origin: [10.0, 1.6, -4.0], direction: [0.0, 0.0, -1.0], scale: 1.6 }
    }

    fn a_key() -> ItemKey {
        ItemKey { a: 40, b: u32::MAX - 2, slot: 5 }
    }

    fn every_event() -> Vec<WorldEvent> {
        vec![
            WorldEvent::Opened { building: 301, spot: 2 },
            WorldEvent::Dropped { key: a_key(), item: 0x0100, count: 32, loaded: 0, rarity: 2, at: [5.0, 0.5, -9.0], yaw: 1.2 },
            WorldEvent::Taken { key: a_key(), by: 2, took: 7 },
            WorldEvent::Glass { building: 12, pane: 4 },
        ]
    }

    fn every_kind() -> Vec<Packet> {
        vec![
            Packet::Join { name: "Botty".into() },
            Packet::Welcome { id: 3, seed: 0xDEAD_BEEF_1234, host: at(10.0, -4.5) },
            Packet::Refused,
            Packet::Update(at(-3.25, 99.0)),
            Packet::Snapshot { tick: 7, players: vec![(0, at(1.0, 2.0)), (2, at(3.0, 4.0))] },
            Packet::Snapshot { tick: 0, players: vec![] },
            Packet::Leave,
            Packet::Zombies(vec![ZombieSeen { id: 9, kind: 2, health: 100, x: 1.0, y: 2.0, z: 3.0 }, ZombieSeen { id: 70_000, kind: 0, health: 0, x: -4.0, y: 0.5, z: 8.0 }]),
            Packet::Zombies(vec![]),
            Packet::Status { health: 62.5, respawn_in: 0.0, acked: 7 },
            Packet::Shot(a_shot()),
            Packet::Fired { player: 3, shot: a_shot() },
            Packet::Events { first: 12, events: every_event() },
            Packet::Events { first: 0, events: vec![] },
            Packet::Ack { upto: 99 },
            Packet::Ask { seq: 4, request: Request::Open { building: 301, spot: 2 } },
            Packet::Ask { seq: 5, request: Request::Take { key: a_key(), took: 12 } },
            Packet::Ask { seq: 6, request: Request::Throw { item: 0x0103, count: 24, loaded: 0, at: [1.0, 2.0, 3.0] } },
            Packet::Ask { seq: 7, request: Request::Glass { building: 9, pane: 3 } },
        ]
    }

    #[test]
    fn every_packet_reads_back_as_it_was_written() {
        for packet in every_kind() {
            assert_eq!(Packet::decode(&packet.encode()), Some(packet.clone()), "{packet:?}");
        }
    }

    #[test]
    fn a_packet_cut_short_or_with_something_on_the_end_is_thrown_away() {
        for packet in every_kind() {
            let bytes = packet.encode();
            for cut in 0..bytes.len() {
                assert_eq!(Packet::decode(&bytes[..cut]), None, "{packet:?} cut to {cut}");
            }
            let mut longer = bytes.clone();
            longer.push(0);
            assert_eq!(Packet::decode(&longer), None, "{packet:?} with a byte on the end");
        }
    }

    #[test]
    fn what_is_not_one_of_ours_is_thrown_away() {
        assert_eq!(Packet::decode(b""), None);
        assert_eq!(Packet::decode(b"GET / HTTP/1.1\r\n\r\n"), None);
        let mut wrong_version = Packet::Leave.encode();
        wrong_version[4] = VERSION + 1;
        assert_eq!(Packet::decode(&wrong_version), None);
        let mut unknown = Packet::Leave.encode();
        *unknown.last_mut().unwrap() = 99;
        assert_eq!(Packet::decode(&unknown), None);
        // Whatever the bytes, it does not panic.
        for seed in 0..2000u32 {
            let junk: Vec<u8> = (0..(seed % 60)).map(|i| (seed.wrapping_mul(2654435761).wrapping_add(i * 40503) >> 13) as u8).collect();
            let _ = Packet::decode(&junk);
        }
    }

    #[test]
    fn a_position_that_is_not_a_place_is_thrown_away() {
        for bad in [PlayerState { x: f32::NAN, ..at(0.0, 0.0) }, PlayerState { z: f32::INFINITY, ..at(0.0, 0.0) }, PlayerState { y: 1e9, ..at(0.0, 0.0) }, PlayerState { yaw: f32::NAN, ..at(0.0, 0.0) }] {
            assert_eq!(Packet::decode(&Packet::Update(bad).encode()), None, "{bad:?}");
        }
    }

    #[test]
    fn a_name_is_made_fit_to_show() {
        assert_eq!(clean_name("Botty"), "Botty");
        assert_eq!(clean_name("a\nb\u{7}c"), "abc");
        assert_eq!(clean_name("   "), "player");
        assert_eq!(clean_name(""), "player");
        assert!(clean_name(&"x".repeat(100)).len() <= NAME_BYTES);
        // Cut between characters, not through one.
        assert!(clean_name(&"é".repeat(40)).len() <= NAME_BYTES);
        let long = Packet::Join { name: "y".repeat(100) };
        assert!(matches!(Packet::decode(&long.encode()), Some(Packet::Join { name }) if name.len() == NAME_BYTES));
    }

    #[test]
    fn someone_joining_is_welcomed_with_an_id_and_the_seed_and_asking_again_gives_the_same() {
        let mut lobby = Lobby::default();
        let heard = lobby.hear(addr(5000), Packet::Join { name: "Bot".into() }, 42, at(1.0, 1.0));
        assert_eq!(heard, vec![Heard::Reply(Packet::Welcome { id: 1, seed: 42, host: at(1.0, 1.0) }), Heard::Joined(1, "Bot".into())]);
        let again = lobby.hear(addr(5000), Packet::Join { name: "Bot".into() }, 42, at(2.0, 2.0));
        assert_eq!(again, vec![Heard::Reply(Packet::Welcome { id: 1, seed: 42, host: at(2.0, 2.0) })]);
        assert_eq!(lobby.clients.len(), 1);
        let other = lobby.hear(addr(5001), Packet::Join { name: "Two".into() }, 42, at(0.0, 0.0));
        assert!(matches!(other[0], Heard::Reply(Packet::Welcome { id: 2, .. })));
    }

    #[test]
    fn a_full_game_turns_the_next_away_and_a_freed_id_is_used_again() {
        let mut lobby = Lobby::default();
        for i in 0..(MAX_PLAYERS - 1) as u16 {
            lobby.hear(addr(6000 + i), Packet::Join { name: "p".into() }, 1, at(0.0, 0.0));
        }
        let turned = lobby.hear(addr(7000), Packet::Join { name: "late".into() }, 1, at(0.0, 0.0));
        assert_eq!(turned, vec![Heard::Reply(Packet::Refused)]);
        assert_eq!(lobby.hear(addr(6002), Packet::Leave, 1, at(0.0, 0.0)), vec![Heard::Left(3)]);
        let heard = lobby.hear(addr(7000), Packet::Join { name: "late".into() }, 1, at(0.0, 0.0));
        assert!(matches!(heard[0], Heard::Reply(Packet::Welcome { id: 3, .. })), "{heard:?}");
    }

    #[test]
    fn a_stranger_cannot_move_anyone_and_a_client_cannot_speak_for_the_host() {
        let mut lobby = Lobby::default();
        lobby.hear(addr(5000), Packet::Join { name: "Bot".into() }, 1, at(0.0, 0.0));
        lobby.hear(addr(5000), Packet::Update(at(5.0, 5.0)), 1, at(0.0, 0.0));
        assert_eq!(lobby.clients[0].state, at(5.0, 5.0));
        lobby.hear(addr(9999), Packet::Update(at(-50.0, -50.0)), 1, at(0.0, 0.0));
        lobby.hear(addr(5000), Packet::Snapshot { tick: 1, players: vec![(0, at(77.0, 77.0))] }, 1, at(0.0, 0.0));
        lobby.hear(addr(5000), Packet::Welcome { id: 0, seed: 9, host: at(0.0, 0.0) }, 1, at(0.0, 0.0));
        assert_eq!(lobby.clients.len(), 1);
        assert_eq!(lobby.clients[0].state, at(5.0, 5.0));
        let Packet::Snapshot { players, .. } = lobby.snapshot(at(1.0, 1.0)) else { panic!("a snapshot") };
        assert_eq!(players, vec![(HOST_ID, at(1.0, 1.0)), (1, at(5.0, 5.0))]);
    }

    #[test]
    fn one_unheard_from_for_too_long_is_dropped_and_one_that_keeps_talking_is_not() {
        let mut lobby = Lobby::default();
        lobby.hear(addr(5000), Packet::Join { name: "Quiet".into() }, 1, at(0.0, 0.0));
        lobby.hear(addr(5001), Packet::Join { name: "Chatty".into() }, 1, at(0.0, 0.0));
        let mut gone = Vec::new();
        for _ in 0..80 {
            lobby.hear(addr(5001), Packet::Update(at(1.0, 1.0)), 1, at(0.0, 0.0));
            gone.extend(lobby.pass(0.1));
        }
        assert_eq!(gone, vec![Passed::Dropped(1)]);
        assert_eq!(lobby.clients.len(), 1);
        assert_eq!(lobby.clients[0].name, "Chatty");
    }

    #[test]
    fn snapshots_count_up() {
        let mut lobby = Lobby::default();
        let tick = |p: Packet| if let Packet::Snapshot { tick, .. } = p { tick } else { 0 };
        let (a, b) = (tick(lobby.snapshot(at(0.0, 0.0))), tick(lobby.snapshot(at(0.0, 0.0))));
        assert_eq!(b, a + 1);
    }

    #[test]
    fn a_status_or_a_zombie_that_is_not_possible_is_thrown_away() {
        for bad in [
            Packet::Status { health: f32::NAN, respawn_in: 0.0, acked: 0 },
            Packet::Status { health: -5.0, respawn_in: 0.0, acked: 0 },
            Packet::Status { health: 50.0, respawn_in: f32::INFINITY, acked: 0 },
            Packet::Zombies(vec![ZombieSeen { id: 1, kind: 0, health: 100, x: f32::NAN, y: 0.0, z: 0.0 }]),
            Packet::Zombies(vec![ZombieSeen { id: 1, kind: 0, health: 101, x: 0.0, y: 0.0, z: 0.0 }]),
            Packet::Shot(ShotInfo { scale: f32::NAN, ..a_shot() }),
            Packet::Shot(ShotInfo { scale: MOST_SCALE + 1.0, ..a_shot() }),
            Packet::Shot(ShotInfo { origin: [f32::INFINITY, 0.0, 0.0], ..a_shot() }),
            Packet::Events { first: 0, events: vec![WorldEvent::Dropped { key: a_key(), item: 1, count: 1, loaded: 0, rarity: 0, at: [f32::NAN, 0.0, 0.0], yaw: 0.0 }] },
        ] {
            assert_eq!(Packet::decode(&bad.encode()), None, "{bad:?}");
        }
        let too_many = Packet::Zombies((0..MAX_ZOMBIES_SEEN as u32 + 5).map(|id| ZombieSeen { id, kind: 0, health: 100, x: 0.0, y: 0.0, z: 0.0 }).collect());
        let Some(Packet::Zombies(list)) = Packet::decode(&too_many.encode()) else { panic!("a list") };
        assert_eq!(list.len(), MAX_ZOMBIES_SEEN, "cut to what a packet holds");
    }

    fn joined() -> Lobby {
        let mut lobby = Lobby::default();
        lobby.hear(addr(5000), Packet::Join { name: "Bot".into() }, 1, at(0.0, 0.0));
        lobby
    }

    #[test]
    fn a_client_can_fire_but_not_impossibly_fast_nor_from_far_from_where_it_is_nor_while_down() {
        let mut lobby = joined();
        let fire = |lobby: &mut Lobby, shot: ShotInfo| lobby.hear(addr(5000), Packet::Shot(shot), 1, at(0.0, 0.0));
        // Where the client is, and the eyes a little above: the shot starts there.
        let here = ShotInfo { origin: [0.0, 1.0 + EYE_HEIGHT, 0.0], ..a_shot() };
        let id = lobby.clients[0].id;
        assert_eq!(fire(&mut lobby, here), vec![Heard::Shot(id, here)], "ready to begin with");
        assert_eq!(fire(&mut lobby, here), vec![Heard::Nothing], "not again at once");
        lobby.pass(SHOT_COOLDOWN + 0.01);
        // From somewhere it is not.
        let far = ShotInfo { origin: [MUZZLE_SLACK + 5.0, 1.0 + EYE_HEIGHT, 0.0], ..here };
        assert_eq!(fire(&mut lobby, far), vec![Heard::Nothing]);
        // Aimed at nothing.
        assert_eq!(fire(&mut lobby, ShotInfo { direction: [0.0; 3], ..here }), vec![Heard::Nothing]);
        assert_eq!(fire(&mut lobby, here), vec![Heard::Shot(id, here)]);
        // A stranger's shot is nothing.
        assert_eq!(lobby.hear(addr(6000), Packet::Shot(here), 1, at(0.0, 0.0)), vec![Heard::Nothing]);
        // Down, it cannot.
        assert!(lobby.hurt(1, CLIENT_HEALTH + 1.0));
        lobby.pass(SHOT_COOLDOWN + 0.01);
        assert_eq!(fire(&mut lobby, here), vec![Heard::Nothing]);
    }

    #[test]
    fn requests_are_done_once_and_in_order_and_the_client_is_told_how_far_the_host_has_got() {
        let mut lobby = joined();
        let ask = |lobby: &mut Lobby, seq: u32, building: u16| lobby.hear(addr(5000), Packet::Ask { seq, request: Request::Open { building, spot: 0 } }, 1, at(0.0, 0.0));
        // The second comes first (the first was lost): it waits.
        assert_eq!(ask(&mut lobby, 2, 20), vec![Heard::Nothing]);
        assert_eq!(lobby.status(1), Some(Packet::Status { health: CLIENT_HEALTH, respawn_in: 0.0, acked: 0 }));
        assert_eq!(ask(&mut lobby, 1, 10), vec![Heard::Asked(1, Request::Open { building: 10, spot: 0 })]);
        // Asked again (the answer was lost): not done twice.
        assert_eq!(ask(&mut lobby, 1, 10), vec![Heard::Nothing]);
        assert_eq!(ask(&mut lobby, 2, 20), vec![Heard::Asked(1, Request::Open { building: 20, spot: 0 })]);
        assert!(matches!(lobby.status(1), Some(Packet::Status { acked: 2, .. })));
        // A stranger's asking is nothing, and one who is down is not obeyed (but the number moves on).
        assert_eq!(lobby.hear(addr(6000), Packet::Ask { seq: 1, request: Request::Open { building: 1, spot: 0 } }, 1, at(0.0, 0.0)), vec![Heard::Nothing]);
        lobby.hurt(1, 1000.0);
        assert_eq!(ask(&mut lobby, 3, 30), vec![Heard::Nothing]);
        assert!(matches!(lobby.status(1), Some(Packet::Status { acked: 3, .. })));
    }

    #[test]
    fn what_has_happened_in_the_world_is_sent_from_the_start_to_whoever_has_not_had_it_and_a_late_joiner_has_all_of_it() {
        let mut lobby = Lobby::default();
        for building in 0..25 {
            lobby.push_event(WorldEvent::Opened { building, spot: 1 });
        }
        // Someone joins long after: they are sent it from the beginning, a packet at a time.
        lobby.hear(addr(5000), Packet::Join { name: "Late".into() }, 1, at(0.0, 0.0));
        let mut had = Vec::new();
        for _ in 0..10 {
            let Some(Packet::Events { first, events }) = lobby.events_for(1) else { break };
            assert_eq!(first as usize, had.len(), "what comes next, and no more");
            assert!(events.len() <= EVENTS_PER_PACKET);
            had.extend(events);
            lobby.hear(addr(5000), Packet::Ack { upto: had.len() as u32 }, 1, at(0.0, 0.0));
        }
        assert_eq!(had.len(), 25);
        assert_eq!(had[24], WorldEvent::Opened { building: 24, spot: 1 });
        assert!(lobby.events_for(1).is_none(), "nothing more to say");
        // Not acknowledged, it is sent again; and what is said to have been had cannot be more than there is.
        lobby.push_event(WorldEvent::Glass { building: 1, pane: 1 });
        let again = lobby.events_for(1);
        assert_eq!(lobby.events_for(1), again);
        lobby.hear(addr(5000), Packet::Ack { upto: 9999 }, 1, at(0.0, 0.0));
        assert_eq!(lobby.clients[0].events_acked, 26);
        // One who joins after is told all of it, in order.
        lobby.hear(addr(5001), Packet::Join { name: "Later".into() }, 1, at(0.0, 0.0));
        let Some(Packet::Events { first: 0, events }) = lobby.events_for(2) else { panic!("from the start") };
        assert_eq!(events[0], WorldEvent::Opened { building: 0, spot: 1 });
    }

    #[test]
    fn a_client_is_hurt_goes_down_and_gets_up_again_after_a_while() {
        let mut lobby = joined();
        assert!(!lobby.hurt(1, 30.0));
        assert_eq!(lobby.clients[0].health, CLIENT_HEALTH - 30.0);
        assert_eq!(lobby.status(1), Some(Packet::Status { health: CLIENT_HEALTH - 30.0, respawn_in: 0.0, acked: 0 }));
        assert!(lobby.hurt(1, 500.0), "that put it down");
        assert!(!lobby.clients[0].alive());
        assert!(!lobby.hurt(1, 10.0), "nothing more to be done to one already down");
        let Some(Packet::Status { health, respawn_in, .. }) = lobby.status(1) else { panic!("a status") };
        assert_eq!(health, 0.0);
        assert!((respawn_in - RESPAWN_AFTER).abs() < 1e-3);
        // Half the time later (the client keeps talking): still down, and told how long is left.
        let wait = |lobby: &mut Lobby, seconds: f32| {
            let mut happened = Vec::new();
            for _ in 0..(seconds as usize) {
                lobby.hear(addr(5000), Packet::Update(at(0.0, 0.0)), 1, at(0.0, 0.0));
                happened.extend(lobby.pass(1.0));
            }
            happened
        };
        assert_eq!(wait(&mut lobby, RESPAWN_AFTER * 0.5), vec![]);
        assert!(matches!(lobby.status(1), Some(Packet::Status { respawn_in, .. }) if (respawn_in - RESPAWN_AFTER * 0.5).abs() < 1e-3));
        assert_eq!(wait(&mut lobby, RESPAWN_AFTER * 0.5), vec![Passed::Revived(1)]);
        assert_eq!(lobby.clients[0].health, CLIENT_HEALTH);
        assert!(!lobby.hurt(9, 10.0), "no such client");
    }
}

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
pub const VERSION: u8 = 1;

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

/// The least time, seconds, between one shot of a joined player's and the next.
pub const SHOT_COOLDOWN: f32 = 0.3;

/// The most zombies one packet tells a client of.
pub const MAX_ZOMBIES_SEEN: usize = 24;

/// Nothing in the world is further than this from the middle, metres; a position beyond it is made up.
const WORLD_LIMIT: f32 = 20_000.0;

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
/// numbering), and where its feet are.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZombieSeen {
    pub id: u32,
    pub kind: u8,
    pub x: f32,
    pub y: f32,
    pub z: f32,
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
    /// The host tells a client how it is: its health, and, if it is down, how long until it is back up.
    Status { health: f32, respawn_in: f32 },
    /// A client fires at a zombie, by its number. The host works out whether it hits.
    Fire { target: u32 },
}

const JOIN: u8 = 1;
const WELCOME: u8 = 2;
const REFUSED: u8 = 3;
const UPDATE: u8 = 4;
const SNAPSHOT: u8 = 5;
const LEAVE: u8 = 6;
const ZOMBIES: u8 = 7;
const STATUS: u8 = 8;
const FIRE: u8 = 9;

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
                    out.push(z.kind);
                    for v in [z.x, z.y, z.z] {
                        out.extend_from_slice(&v.to_le_bytes());
                    }
                }
            }
            Packet::Status { health, respawn_in } => {
                out.push(STATUS);
                out.extend_from_slice(&health.to_le_bytes());
                out.extend_from_slice(&respawn_in.to_le_bytes());
            }
            Packet::Fire { target } => {
                out.push(FIRE);
                out.extend_from_slice(&target.to_le_bytes());
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
                    let kind = r.byte()?;
                    let at = PlayerState { x: r.float()?, y: r.float()?, z: r.float()?, yaw: 0.0 };
                    if !at.sane() {
                        return None;
                    }
                    list.push(ZombieSeen { id, kind, x: at.x, y: at.y, z: at.z });
                }
                Packet::Zombies(list)
            }
            STATUS => {
                let (health, respawn_in) = (r.float()?, r.float()?);
                if !(health.is_finite() && (0.0..=10_000.0).contains(&health) && respawn_in.is_finite() && (0.0..=3_600.0).contains(&respawn_in)) {
                    return None;
                }
                Packet::Status { health, respawn_in }
            }
            FIRE => Packet::Fire { target: r.u32()? },
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
    /// This id fired at the zombie with this number.
    Fired(u8, u32),
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
                self.clients.push(Client { addr: from, id, name: name.clone(), state: host, silent_for: 0.0, health: CLIENT_HEALTH, down_for: 0.0, since_shot: SHOT_COOLDOWN });
                vec![Heard::Reply(Packet::Welcome { id, seed, host }), Heard::Joined(id, name)]
            }
            (Packet::Update(state), Some(i)) => {
                self.clients[i].state = state;
                self.clients[i].silent_for = 0.0;
                vec![Heard::Nothing]
            }
            (Packet::Leave, Some(i)) => vec![Heard::Left(self.clients.remove(i).id)],
            // A shot, from one on its feet and not firing faster than the gun can.
            (Packet::Fire { target }, Some(i)) => {
                let client = &mut self.clients[i];
                client.silent_for = 0.0;
                if client.alive() && client.since_shot >= SHOT_COOLDOWN {
                    client.since_shot = 0.0;
                    vec![Heard::Fired(client.id, target)]
                } else {
                    vec![Heard::Nothing]
                }
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
        self.clients.iter().find(|c| c.id == id).map(|c| Packet::Status { health: c.health, respawn_in: c.respawn_in() })
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

    fn every_kind() -> Vec<Packet> {
        vec![
            Packet::Join { name: "Botty".into() },
            Packet::Welcome { id: 3, seed: 0xDEAD_BEEF_1234, host: at(10.0, -4.5) },
            Packet::Refused,
            Packet::Update(at(-3.25, 99.0)),
            Packet::Snapshot { tick: 7, players: vec![(0, at(1.0, 2.0)), (2, at(3.0, 4.0))] },
            Packet::Snapshot { tick: 0, players: vec![] },
            Packet::Leave,
            Packet::Zombies(vec![ZombieSeen { id: 9, kind: 2, x: 1.0, y: 2.0, z: 3.0 }, ZombieSeen { id: 70_000, kind: 0, x: -4.0, y: 0.5, z: 8.0 }]),
            Packet::Zombies(vec![]),
            Packet::Status { health: 62.5, respawn_in: 0.0 },
            Packet::Fire { target: 12345 },
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
        for bad in [Packet::Status { health: f32::NAN, respawn_in: 0.0 }, Packet::Status { health: -5.0, respawn_in: 0.0 }, Packet::Status { health: 50.0, respawn_in: f32::INFINITY }, Packet::Zombies(vec![ZombieSeen { id: 1, kind: 0, x: f32::NAN, y: 0.0, z: 0.0 }])] {
            assert_eq!(Packet::decode(&bad.encode()), None, "{bad:?}");
        }
        let too_many = Packet::Zombies((0..MAX_ZOMBIES_SEEN as u32 + 5).map(|id| ZombieSeen { id, kind: 0, x: 0.0, y: 0.0, z: 0.0 }).collect());
        let Some(Packet::Zombies(list)) = Packet::decode(&too_many.encode()) else { panic!("a list") };
        assert_eq!(list.len(), MAX_ZOMBIES_SEEN, "cut to what a packet holds");
    }

    fn joined() -> Lobby {
        let mut lobby = Lobby::default();
        lobby.hear(addr(5000), Packet::Join { name: "Bot".into() }, 1, at(0.0, 0.0));
        lobby
    }

    #[test]
    fn a_client_can_fire_but_only_as_fast_as_the_gun_and_not_while_down() {
        let mut lobby = joined();
        let fire = |lobby: &mut Lobby| lobby.hear(addr(5000), Packet::Fire { target: 7 }, 1, at(0.0, 0.0));
        assert_eq!(fire(&mut lobby), vec![Heard::Fired(1, 7)], "ready to begin with");
        assert_eq!(fire(&mut lobby), vec![Heard::Nothing], "not again at once");
        lobby.pass(SHOT_COOLDOWN + 0.01);
        assert_eq!(fire(&mut lobby), vec![Heard::Fired(1, 7)]);
        // A stranger's shot is nothing.
        assert_eq!(lobby.hear(addr(6000), Packet::Fire { target: 7 }, 1, at(0.0, 0.0)), vec![Heard::Nothing]);
        // Down, it cannot.
        assert!(lobby.hurt(1, CLIENT_HEALTH + 1.0));
        lobby.pass(SHOT_COOLDOWN + 0.01);
        assert_eq!(fire(&mut lobby), vec![Heard::Nothing]);
    }

    #[test]
    fn a_client_is_hurt_goes_down_and_gets_up_again_after_a_while() {
        let mut lobby = joined();
        assert!(!lobby.hurt(1, 30.0));
        assert_eq!(lobby.clients[0].health, CLIENT_HEALTH - 30.0);
        assert_eq!(lobby.status(1), Some(Packet::Status { health: CLIENT_HEALTH - 30.0, respawn_in: 0.0 }));
        assert!(lobby.hurt(1, 500.0), "that put it down");
        assert!(!lobby.clients[0].alive());
        assert!(!lobby.hurt(1, 10.0), "nothing more to be done to one already down");
        let Some(Packet::Status { health, respawn_in }) = lobby.status(1) else { panic!("a status") };
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

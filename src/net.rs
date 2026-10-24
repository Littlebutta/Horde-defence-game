//! Co-op multiplayer: deterministic lockstep, like Factorio. Every player runs the same simulation;
//! only the players' commands travel over the network.
//!
//! The host runs the clock. Each tick it bundles the commands it has collected (its own and the
//! ones clients sent it) into a frame for `DELAY` ticks ahead and sends that frame to everybody,
//! itself included. A machine may only simulate tick T once it holds frame T, so everyone applies
//! the same commands on the same tick. The host never waits for a slow client: that client just
//! runs a little behind and catches up by simulating several ticks per frame.
//!
//! Every 100 ticks clients send a checksum of their game to the host, which compares it with its
//! own, so a game that drifts apart is noticed instead of silently showing different worlds.
//!
//! Transport is Steam (lobbies, the friends invite overlay, Steam relay networking) when Steam is
//! running, or plain TCP for local testing (`PH_HOST=port` / `PH_JOIN=addr:port`).

use crate::defs::*;
use crate::game::Game;
use crate::storage::{self, Rd, Wr};
use crate::world::Desc;
use macroquad::prelude::{vec2, Vec2};
use std::collections::{BTreeMap, HashMap, VecDeque};

/// Steam's test App ID (Spacewar) until the game has its own.
pub const APP_ID: u32 = 480;
/// Ticks between a command being collected and it taking effect.
pub const DELAY: u64 = 3;
const HASH_EVERY: u64 = 100;
const CHUNK: usize = 60_000;

// ------------------------------------------------------------------------------------------ commands

/// Everything a player can do to the shared game.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cmd {
    /// a build drag from `a` to `b` (one structure, or a row of them)
    Build { t: S, a: Vec2, b: Vec2 },
    Roof { t: S, b: u32, p: Vec2 },
    Sell(Vec2),
    Deploy(U, Vec2),
    Order(u32, Vec2),
    Ability(A, Vec2, Vec2),
    Research(R),
    /// test horde of this many hostiles
    Stress(u32),
    /// point an artillery piece or a heli pad's gunship at a spot
    Aim(u32, Vec2),
    /// stop an artillery piece firing
    Cease(u32),
}

const ALL_S: [S; 16] = [S::Wall, S::Gun, S::Cannon, S::Extractor, S::Plant, S::Silo, S::Pad, S::Outpost, S::Lab, S::Hq, S::Flame, S::Rail, S::Helium, S::Artillery, S::Helipad, S::Rescue];
const ALL_U: [U; 2] = [U::Rifle, U::Tank];

fn s_id(t: S) -> u32 {
    ALL_S.iter().position(|x| *x == t).unwrap() as u32
}
fn u_id(u: U) -> u32 {
    ALL_U.iter().position(|x| *x == u).unwrap() as u32
}
fn a_id(a: A) -> u32 {
    ABILITIES.iter().position(|x| *x == a).unwrap() as u32
}

impl Wr {
    fn v2(&mut self, p: Vec2) {
        self.f32(p.x);
        self.f32(p.y);
    }
    fn u64(&mut self, v: u64) {
        self.u32((v >> 32) as u32);
        self.u32(v as u32);
    }
    fn cmd(&mut self, c: &Cmd) {
        match *c {
            Cmd::Build { t, a, b } => {
                self.u32(0);
                self.u32(s_id(t));
                self.v2(a);
                self.v2(b);
            }
            Cmd::Roof { t, b, p } => {
                self.u32(1);
                self.u32(s_id(t));
                self.u32(b);
                self.v2(p);
            }
            Cmd::Sell(p) => {
                self.u32(2);
                self.v2(p);
            }
            Cmd::Deploy(u, p) => {
                self.u32(3);
                self.u32(u_id(u));
                self.v2(p);
            }
            Cmd::Order(g, p) => {
                self.u32(4);
                self.u32(g);
                self.v2(p);
            }
            Cmd::Ability(a, f, t) => {
                self.u32(5);
                self.u32(a_id(a));
                self.v2(f);
                self.v2(t);
            }
            Cmd::Research(r) => {
                self.u32(6);
                self.u32(RESEARCH.iter().position(|x| *x == r).unwrap() as u32);
            }
            Cmd::Stress(n) => {
                self.u32(7);
                self.u32(n);
            }
            Cmd::Aim(i, p) => {
                self.u32(20);
                self.u32(i);
                self.v2(p);
            }
            Cmd::Cease(i) => {
                self.u32(21);
                self.u32(i);
            }
        }
    }
}
impl Rd<'_> {
    fn v2(&mut self) -> Option<Vec2> {
        Some(vec2(self.f32()?, self.f32()?))
    }
    fn u64(&mut self) -> Option<u64> {
        Some(((self.u32()? as u64) << 32) | self.u32()? as u64)
    }
    fn s(&mut self) -> Option<S> {
        ALL_S.get(self.u32()? as usize).copied()
    }
    fn a(&mut self) -> Option<A> {
        ABILITIES.get(self.u32()? as usize).copied()
    }
    fn cmd(&mut self) -> Option<Cmd> {
        Some(match self.u32()? {
            0 => Cmd::Build { t: self.s()?, a: self.v2()?, b: self.v2()? },
            1 => Cmd::Roof { t: self.s()?, b: self.u32()?, p: self.v2()? },
            2 => Cmd::Sell(self.v2()?),
            3 => Cmd::Deploy(*ALL_U.get(self.u32()? as usize)?, self.v2()?),
            4 => Cmd::Order(self.u32()?, self.v2()?),
            5 => Cmd::Ability(self.a()?, self.v2()?, self.v2()?),
            6 => Cmd::Research(*RESEARCH.get(self.u32()? as usize)?),
            7 => Cmd::Stress(self.u32()?.min(10_000_000)),
            20 => Cmd::Aim(self.u32()?, self.v2()?),
            21 => Cmd::Cease(self.u32()?),
            _ => return None,
        })
    }
}

/// Placements a build drag makes, as (x, y) of each footprint: walls follow the line in 3-cell
/// steps, everything else fills a grid of copies.
pub fn plan(t: S, a: Vec2, b: Vec2) -> Vec<(i32, i32)> {
    let d = def(t);
    if t == S::Wall {
        let snap = |p: Vec2| ((p.x / 3.0).floor() as i32, (p.y / 3.0).floor() as i32);
        let (x0, y0) = snap(a);
        let (x1, y1) = snap(b);
        let n = (x1 - x0).abs().max((y1 - y0).abs()).min(400);
        let mut v = Vec::new();
        for k in 0..=n {
            let f = if n == 0 { 0.0 } else { k as f32 / n as f32 };
            let gx = (x0 as f32 + (x1 - x0) as f32 * f).round() as i32;
            let gy = (y0 as f32 + (y1 - y0) as f32 * f).round() as i32;
            if v.last() != Some(&(gx * 3, gy * 3)) {
                v.push((gx * 3, gy * 3));
            }
        }
        return v;
    }
    let ox = |p: Vec2| ((p.x - d.w as f32 / 2.0).round() as i32, (p.y - d.h as f32 / 2.0).round() as i32);
    let (x0, y0) = ox(a);
    let (x1, y1) = ox(b);
    let (sx, sy) = (d.w + 1, d.h + 1);
    let nx = ((x1 - x0).abs() / sx).min(30);
    let ny = ((y1 - y0).abs() / sy).min(30);
    let (dx, dy) = ((x1 - x0).signum(), (y1 - y0).signum());
    let mut v = Vec::new();
    for j in 0..=ny {
        for i in 0..=nx {
            v.push((x0 + i * sx * dx, y0 + j * sy * dy));
        }
    }
    v
}

pub const BAD_TARGET: &str = "Can't target that";

/// Carries out one command. Returns a line to show the player who gave it, if any.
pub fn apply(g: &mut Game, c: Cmd) -> Option<&'static str> {
    match c {
        Cmd::Build { t, a, b } => {
            let placed = plan(t, a, b).into_iter().filter(|(x, y)| g.place(t, *x, *y)).count();
            (placed == 0).then_some("Can't build there")
        }
        Cmd::Roof { t, b, p } => {
            let ok = (b as usize) < g.world.buildings.len() && g.place_roof(t, b as usize, p);
            (!ok).then_some("Can't put a turret on that roof")
        }
        Cmd::Sell(p) => g.sell_at(p).then_some("Sold for half price"),
        Cmd::Deploy(u, p) => (!g.deploy(u, p)).then_some("Can't deploy there"),
        Cmd::Order(gi, p) => {
            let ok = (gi as usize) < g.groups.len() && g.order(gi as usize, p);
            (!ok).then_some("Can't send troops there")
        }
        Cmd::Ability(a, f, t) => (!g.use_ability(a, f, t)).then_some(BAD_TARGET),
        Cmd::Research(r) => {
            g.start_research(r);
            None
        }
        Cmd::Stress(n) => {
            g.stress_n(n as f32);
            None
        }
        Cmd::Aim(i, p) => (!g.aim(i as usize, p)).then_some("Can't fire there"),
        Cmd::Cease(i) => {
            g.cease(i as usize);
            None
        }
    }
}

// ------------------------------------------------------------------------------------------ transport

pub enum Ev {
    Leave(u64),
    Data(u64, Vec<u8>),
}

pub trait Link {
    fn me(&self) -> u64;
    fn host(&self) -> u64;
    fn poll(&mut self) -> Vec<Ev>;
    /// false when the message could not be queued right now (try again next frame)
    fn send(&mut self, to: u64, data: &[u8], reliable: bool) -> bool;
    fn name_of(&self, _id: u64) -> String {
        "Player".into()
    }
    fn invite(&self) {}
    fn set_joinable(&self, _joinable: bool) {}
    fn close(&mut self) {}
}

// ------------------------------------------------------------------------------------------ messages

const M_HELLO: u32 = 1;
const M_ROSTER: u32 = 2;
const M_START: u32 = 3;
const M_CHUNK: u32 = 4;
const M_FRAME: u32 = 5;
const M_CMD: u32 = 6;
const M_HASH: u32 = 7;
const M_CURSOR: u32 = 8;
const M_PAUSE: u32 = 9;
const M_PAUSE_REQ: u32 = 10;
const M_DESYNC: u32 = 11;
const M_END: u32 = 12;
const M_BUSY: u32 = 13;
const M_REJECT: u32 = 14;
const M_LOBBY: u32 = 15;

/// What the host has picked in the co-op lobby, shown to everybody waiting there.
#[derive(Clone, Default, PartialEq)]
pub struct Lobby {
    /// one of the `MODE_` values
    pub mode: u32,
    pub fac: u32,
    pub diff: u32,
    /// the real city typed in, or the campaign's next city
    pub city: String,
    /// what the host is doing right now
    pub line: String,
}
pub const MODE_RANDOM: u32 = 0;
pub const MODE_REAL: u32 = 1;
pub const MODE_CAMPAIGN: u32 = 2;

pub struct Player {
    pub id: u64,
    pub name: String,
    pub cursor: Option<Vec2>,
}

/// What the session tells the app.
pub enum Note {
    /// the host started a game (on clients)
    /// `camp` is the host's campaign (as saved text) when this is a campaign city
    Started { desc: Box<Desc>, fac: usize, diff: usize, seed: u64, camp: Option<String> },
    /// the host went back to the menu
    Ended,
    /// the session is over (host left, or we were turned away)
    Closed(String),
    Msg(String),
}

struct Incoming {
    fac: usize,
    diff: usize,
    seed: u64,
    camp: Option<String>,
    len: usize,
    buf: Vec<u8>,
}

pub struct Session {
    link: Box<dyn Link>,
    host: bool,
    /// players in join order; the host is always first
    pub players: Vec<Player>,
    /// my index in `players`
    pub me: usize,
    /// true while a game is running
    pub in_game: bool,
    /// the host is in a game this client did not start with
    pub host_busy: bool,
    pub paused: bool,
    pub desync: bool,
    /// ticks simulated so far in this game
    pub exec: u64,
    /// the host's picks in the lobby (on the host: what was last sent)
    pub lobby: Lobby,
    frames: BTreeMap<u64, Vec<(u8, Cmd)>>,
    pending: Vec<(u8, Cmd)>,
    hashes: BTreeMap<u64, u64>,
    /// reliable messages waiting for room in the transport, per peer, in order
    outq: HashMap<u64, VecDeque<Vec<u8>>>,
    incoming: Option<Incoming>,
    cursor_t: f32,
    last_cursor: Vec2,
    notes: Vec<Note>,
}

fn msg(tag: u32) -> Wr {
    let mut w = Wr(Vec::new());
    w.u32(tag);
    w
}

impl Session {
    pub fn new_host(link: Box<dyn Link>, name: String) -> Session {
        let me = link.me();
        let mut s = Session::empty(link, true);
        s.players.push(Player { id: me, name, cursor: None });
        s
    }
    pub fn new_client(link: Box<dyn Link>, name: String) -> Session {
        let host = link.host();
        let mut s = Session::empty(link, false);
        let mut w = msg(M_HELLO);
        w.str(&name);
        w.u32(crate::update::build_number());
        s.queue(host, w.0);
        s
    }
    fn empty(link: Box<dyn Link>, host: bool) -> Session {
        Session {
            link,
            host,
            players: Vec::new(),
            me: 0,
            in_game: false,
            host_busy: false,
            paused: false,
            desync: false,
            exec: 0,
            lobby: Lobby::default(),
            frames: BTreeMap::new(),
            pending: Vec::new(),
            hashes: BTreeMap::new(),
            outq: HashMap::new(),
            incoming: None,
            cursor_t: 0.0,
            last_cursor: Vec2::ZERO,
            notes: Vec::new(),
        }
    }

    pub fn is_host(&self) -> bool {
        self.host
    }
    pub fn invite(&self) {
        self.link.invite();
    }
    pub fn host_name(&self) -> String {
        self.players.first().map(|p| p.name.clone()).unwrap_or_else(|| "the host".into())
    }
    /// Map download progress on a client, 0..1.
    pub fn receiving(&self) -> Option<f32> {
        self.incoming.as_ref().map(|i| i.buf.len() as f32 / i.len.max(1) as f32)
    }

    fn queue(&mut self, to: u64, data: Vec<u8>) {
        self.outq.entry(to).or_default().push_back(data);
    }
    fn to_clients(&mut self, data: &[u8]) {
        let ids: Vec<u64> = self.players.iter().skip(1).map(|p| p.id).collect();
        for id in ids {
            self.queue(id, data.to_vec());
        }
    }
    fn flush(&mut self) {
        for (to, q) in self.outq.iter_mut() {
            while let Some(m) = q.front() {
                if !self.link.send(*to, m, true) {
                    break;
                }
                q.pop_front();
            }
        }
        self.outq.retain(|_, q| !q.is_empty());
    }
    fn roster(&mut self) {
        let ids: Vec<u64> = self.players.iter().skip(1).map(|p| p.id).collect();
        for (k, id) in ids.into_iter().enumerate() {
            let mut w = msg(M_ROSTER);
            w.u32(k as u32 + 1);
            w.u32(self.players.len() as u32);
            for p in &self.players {
                w.str(&p.name);
            }
            w.u32(self.in_game as u32);
            self.queue(id, w.0);
        }
        self.send_lobby();
    }
    fn send_lobby(&mut self) {
        let mut w = msg(M_LOBBY);
        w.u32(self.lobby.mode);
        w.u32(self.lobby.fac);
        w.u32(self.lobby.diff);
        w.str(&self.lobby.city);
        w.str(&self.lobby.line);
        self.to_clients(&w.0);
    }
    /// Host: tells everybody in the lobby what has been picked, when it changed.
    pub fn set_lobby(&mut self, l: Lobby) {
        if l != self.lobby {
            self.lobby = l;
            self.send_lobby();
        }
    }

    /// Handles everything that arrived and sends what is waiting. Call once per frame.
    pub fn pump(&mut self) -> Vec<Note> {
        for ev in self.link.poll() {
            match ev {
                Ev::Leave(id) => {
                    if !self.host {
                        if id == self.link.host() {
                            self.notes.push(Note::Closed(format!("{} closed the co-op game", self.host_name())));
                        }
                    } else if let Some(i) = self.players.iter().position(|p| p.id == id) {
                        let p = self.players.remove(i);
                        self.outq.remove(&id);
                        self.notes.push(Note::Msg(format!("{} left", p.name)));
                        self.roster();
                    }
                }
                Ev::Data(from, d) => self.on_data(from, &d),
            }
        }
        self.flush();
        std::mem::take(&mut self.notes)
    }

    fn on_data(&mut self, from: u64, d: &[u8]) {
        let mut r = Rd(d, 0);
        let Some(tag) = r.u32() else { return };
        let from_host = from == self.link.host();
        let pidx = self.players.iter().position(|p| p.id == from);
        if self.host {
            match tag {
                M_HELLO => {
                    let (Some(name), Some(build)) = (r.str(), r.u32()) else { return };
                    let mine = crate::update::build_number();
                    if build != mine {
                        let mut w = msg(M_REJECT);
                        w.str(&format!("Version mismatch: the host has build {}, you have build {}. Restart both games so they update.", mine, build));
                        self.queue(from, w.0);
                        return;
                    }
                    if pidx.is_none() {
                        let name = if name.trim().is_empty() { self.link.name_of(from) } else { name };
                        self.notes.push(Note::Msg(format!("{} joined", name)));
                        self.players.push(Player { id: from, name, cursor: None });
                    }
                    self.roster();
                    if self.in_game {
                        self.queue(from, msg(M_BUSY).0);
                    }
                }
                M_CMD => {
                    if let (Some(i), Some(c)) = (pidx, r.cmd()) {
                        if self.in_game {
                            self.pending.push((i as u8, c));
                        }
                    }
                }
                M_HASH => {
                    let (Some(t), Some(h)) = (r.u64(), r.u64()) else { return };
                    if let Some(mine) = self.hashes.get(&t) {
                        if *mine != h && !self.desync {
                            self.desync = true;
                            let who = pidx.map(|i| self.players[i].name.clone()).unwrap_or_default();
                            self.notes.push(Note::Msg(format!("{}'s game went out of sync. Go back to the menu and start a new game.", who)));
                            let mut w = msg(M_DESYNC);
                            w.u64(t);
                            self.to_clients(&w.0);
                        }
                    }
                }
                M_CURSOR => {
                    let (Some(i), Some(p)) = (pidx, r.v2()) else { return };
                    self.players[i].cursor = Some(p);
                    let mut w = msg(M_CURSOR);
                    w.u32(i as u32);
                    w.v2(p);
                    let ids: Vec<u64> = self.players.iter().skip(1).map(|p| p.id).filter(|id| *id != from).collect();
                    for id in ids {
                        self.link.send(id, &w.0, false);
                    }
                }
                M_PAUSE_REQ => self.toggle_pause(),
                _ => {}
            }
            return;
        }
        if !from_host {
            return;
        }
        match tag {
            M_ROSTER => {
                let (Some(me), Some(n)) = (r.u32(), r.u32()) else { return };
                let names: Option<Vec<String>> = (0..n).map(|_| r.str()).collect();
                let Some(names) = names else { return };
                let busy = r.u32().unwrap_or(0) == 1;
                let old: Vec<Option<Vec2>> = self.players.iter().map(|p| p.cursor).collect();
                self.players = names.into_iter().enumerate().map(|(i, name)| Player { id: 0, name, cursor: old.get(i).copied().flatten() }).collect();
                self.me = me as usize;
                if !busy {
                    self.host_busy = false;
                }
            }
            M_BUSY => self.host_busy = true,
            M_LOBBY => {
                let (Some(mode), Some(fac), Some(diff), Some(city), Some(line)) = (r.u32(), r.u32(), r.u32(), r.str(), r.str()) else { return };
                self.lobby = Lobby { mode, fac, diff, city, line };
            }
            M_REJECT => {
                let s = r.str().unwrap_or_default();
                self.notes.push(Note::Closed(s));
            }
            M_START => {
                let (Some(fac), Some(diff), Some(seed), Some(camp), Some(len)) = (r.u32(), r.u32(), r.u64(), r.str(), r.u32()) else { return };
                let camp = (!camp.is_empty()).then_some(camp);
                self.incoming = Some(Incoming { fac: fac as usize, diff: diff as usize, seed, camp, len: len as usize, buf: Vec::with_capacity(len as usize) });
                self.frames.clear();
                self.exec = 0;
                self.paused = false;
                self.desync = false;
                self.host_busy = false;
            }
            M_CHUNK => {
                let Some(inc) = self.incoming.as_mut() else { return };
                inc.buf.extend_from_slice(&d[4..]);
                if inc.buf.len() >= inc.len {
                    let inc = self.incoming.take().unwrap();
                    let desc = miniz_oxide::inflate::decompress_to_vec(&inc.buf).ok().and_then(|b| storage::decode(&b));
                    match desc {
                        Some(desc) => {
                            self.in_game = true;
                            self.notes.push(Note::Started { desc: Box::new(desc), fac: inc.fac, diff: inc.diff, seed: inc.seed, camp: inc.camp });
                        }
                        None => self.notes.push(Note::Closed("The map from the host could not be read".into())),
                    }
                }
            }
            M_FRAME => {
                let (Some(t), Some(n)) = (r.u64(), r.u32()) else { return };
                let mut v = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    let (Some(p), Some(c)) = (r.u32(), r.cmd()) else { return };
                    v.push((p as u8, c));
                }
                self.frames.insert(t, v);
            }
            M_CURSOR => {
                let (Some(i), Some(p)) = (r.u32(), r.v2()) else { return };
                if let Some(pl) = self.players.get_mut(i as usize) {
                    pl.cursor = Some(p);
                }
            }
            M_PAUSE => self.paused = r.u32() == Some(1),
            M_DESYNC => {
                self.desync = true;
                self.notes.push(Note::Msg("This game went out of sync with the host. Go back to the menu and start a new game.".into()));
            }
            M_END => {
                self.in_game = false;
                self.host_busy = false;
                self.incoming = None;
                self.notes.push(Note::Ended);
            }
            _ => {}
        }
    }

    // -------------------------------------------------------------------------------------- game

    /// Host: sends the map and settings (and the campaign, for a campaign city) to everybody and
    /// resets the clock. Returns the map exactly as the clients will rebuild it, so the host plays
    /// on an identical copy.
    pub fn start_game(&mut self, d: &Desc, fac: usize, diff: usize, seed: u64, camp: Option<&str>) -> Desc {
        let raw = storage::encode("co-op", d);
        let packed = miniz_oxide::deflate::compress_to_vec(&raw, 6);
        let mut w = msg(M_START);
        w.u32(fac as u32);
        w.u32(diff as u32);
        w.u64(seed);
        w.str(camp.unwrap_or(""));
        w.u32(packed.len() as u32);
        self.to_clients(&w.0);
        for c in packed.chunks(CHUNK) {
            let mut w = msg(M_CHUNK);
            w.0.extend_from_slice(c);
            self.to_clients(&w.0);
        }
        self.in_game = true;
        self.exec = 0;
        self.frames.clear();
        self.pending.clear();
        self.hashes.clear();
        self.paused = false;
        self.desync = false;
        self.link.set_joinable(false);
        self.flush();
        storage::decode(&raw).unwrap_or_else(|| d.clone())
    }
    /// Host: back to the menu for everybody.
    pub fn end_game(&mut self) {
        self.in_game = false;
        self.to_clients(&msg(M_END).0);
        self.link.set_joinable(true);
        self.flush();
    }
    pub fn leave(&mut self) {
        self.flush();
        self.link.close();
    }

    /// A command from the local player.
    pub fn issue(&mut self, c: Cmd) {
        if self.host {
            self.pending.push((0, c));
        } else {
            let mut w = msg(M_CMD);
            w.cmd(&c);
            let h = self.link.host();
            self.queue(h, w.0);
        }
    }

    /// The commands for the next tick, if this machine may simulate it now. On the host this also
    /// sends out the frame `DELAY` ticks ahead.
    pub fn step(&mut self) -> Option<Vec<(u8, Cmd)>> {
        if self.host {
            let t = self.exec + DELAY;
            let cmds = std::mem::take(&mut self.pending);
            let mut w = msg(M_FRAME);
            w.u64(t);
            w.u32(cmds.len() as u32);
            for (p, c) in &cmds {
                w.u32(*p as u32);
                w.cmd(c);
            }
            self.to_clients(&w.0);
            self.frames.insert(t, cmds);
        }
        if self.exec < DELAY {
            return Some(Vec::new());
        }
        self.frames.remove(&self.exec)
    }
    /// Frames this client holds but has not simulated yet.
    pub fn backlog(&self) -> usize {
        self.frames.len()
    }
    /// Call after simulating the tick `step` allowed.
    pub fn after_tick(&mut self, g: &Game) {
        self.exec += 1;
        if self.exec % HASH_EVERY == 0 {
            let h = g.checksum();
            if self.host {
                self.hashes.insert(self.exec, h);
                while self.hashes.len() > 64 {
                    self.hashes.pop_first();
                }
            } else {
                let mut w = msg(M_HASH);
                w.u64(self.exec);
                w.u64(h);
                let host = self.link.host();
                self.queue(host, w.0);
            }
        }
        if self.exec % 20 == 0 {
            self.flush();
        }
    }

    pub fn toggle_pause(&mut self) {
        if self.host {
            self.paused = !self.paused;
            let mut w = msg(M_PAUSE);
            w.u32(self.paused as u32);
            self.to_clients(&w.0);
        } else {
            let h = self.link.host();
            self.queue(h, msg(M_PAUSE_REQ).0);
        }
    }

    /// Shares where the local player is pointing, ten times a second.
    pub fn cursor(&mut self, p: Vec2, dt: f32) {
        self.cursor_t -= dt;
        if self.cursor_t > 0.0 || p.distance(self.last_cursor) < 0.5 {
            return;
        }
        self.cursor_t = 0.1;
        self.last_cursor = p;
        if self.host {
            let mut w = msg(M_CURSOR);
            w.u32(0);
            w.v2(p);
            let ids: Vec<u64> = self.players.iter().skip(1).map(|p| p.id).collect();
            for id in ids {
                self.link.send(id, &w.0, false);
            }
        } else {
            let mut w = msg(M_CURSOR);
            w.v2(p);
            let h = self.link.host();
            self.link.send(h, &w.0, false);
        }
    }
}

/// Runs one lockstep tick: the frame's commands in order, then the simulation.
/// Returns the messages meant for the local player.
pub fn run_tick(g: &mut Game, cmds: &[(u8, Cmd)], me: usize) -> Vec<&'static str> {
    let mut out = Vec::new();
    for (p, c) in cmds {
        let r = apply(g, *c);
        if std::env::var("PH_NETTEST").is_ok() {
            eprintln!("tick {} player {} {:?} -> {:?}", g.ticks, p, c, r);
        }
        if let Some(m) = r {
            if *p as usize == me {
                out.push(m);
            }
        }
    }
    g.tick();
    out
}

// ------------------------------------------------------------------------------------------ TCP

/// Direct connection for testing on one machine or a LAN.
pub mod tcp {
    use super::{Ev, Link};
    use std::collections::HashMap;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::mpsc::{channel, Receiver, Sender};
    use std::sync::{Arc, Mutex};

    pub struct TcpLink {
        me: u64,
        host: u64,
        peers: Arc<Mutex<HashMap<u64, TcpStream>>>,
        rx: Receiver<Ev>,
    }

    fn reader(id: u64, mut s: TcpStream, tx: Sender<Ev>) {
        std::thread::spawn(move || {
            let mut len = [0u8; 4];
            loop {
                if s.read_exact(&mut len).is_err() {
                    break;
                }
                let mut b = vec![0u8; u32::from_le_bytes(len) as usize];
                if s.read_exact(&mut b).is_err() || tx.send(Ev::Data(id, b)).is_err() {
                    break;
                }
            }
            let _ = tx.send(Ev::Leave(id));
        });
    }

    pub fn host(port: u16) -> std::io::Result<TcpLink> {
        let l = TcpListener::bind(("0.0.0.0", port))?;
        let (tx, rx) = channel();
        let peers: Arc<Mutex<HashMap<u64, TcpStream>>> = Arc::default();
        let pc = peers.clone();
        std::thread::spawn(move || {
            let mut next = 2;
            for s in l.incoming().flatten() {
                let _ = s.set_nodelay(true);
                let id = next;
                next += 1;
                if let Ok(c) = s.try_clone() {
                    pc.lock().unwrap().insert(id, c);
                }
                reader(id, s, tx.clone());
            }
        });
        Ok(TcpLink { me: 1, host: 1, peers, rx })
    }

    pub fn join(addr: &str) -> std::io::Result<TcpLink> {
        let s = TcpStream::connect(addr)?;
        let _ = s.set_nodelay(true);
        let (tx, rx) = channel();
        let peers: Arc<Mutex<HashMap<u64, TcpStream>>> = Arc::default();
        peers.lock().unwrap().insert(1, s.try_clone()?);
        reader(1, s, tx);
        Ok(TcpLink { me: 0, host: 1, peers, rx })
    }

    impl Link for TcpLink {
        fn me(&self) -> u64 {
            self.me
        }
        fn host(&self) -> u64 {
            self.host
        }
        fn poll(&mut self) -> Vec<Ev> {
            self.rx.try_iter().collect()
        }
        fn send(&mut self, to: u64, data: &[u8], _reliable: bool) -> bool {
            if let Some(s) = self.peers.lock().unwrap().get_mut(&to) {
                let _ = s.write_all(&(data.len() as u32).to_le_bytes()).and_then(|_| s.write_all(data));
            }
            true
        }
        fn close(&mut self) {
            for s in self.peers.lock().unwrap().values() {
                let _ = s.shutdown(std::net::Shutdown::Both);
            }
        }
    }
}

// ------------------------------------------------------------------------------------------ Steam

#[cfg(feature = "steam")]
mod steam_impl {
    use super::{Ev, Link, APP_ID};
    use std::sync::{Arc, Mutex};
    use steamworks::networking_types::{NetworkingIdentity, SendFlags};
    use steamworks::{Client, GameLobbyJoinRequested, GameRichPresenceJoinRequested, LobbyId, LobbyType, SteamId};

    /// The Steam client, when Steam is running.
    pub struct Steam {
        client: Client,
        joins: Arc<Mutex<Vec<u64>>>,
        lobby: Arc<Mutex<Option<Result<(u64, bool), String>>>>,
        _cbs: Vec<steamworks::CallbackHandle>,
    }

    /// `+connect_lobby <id>`, the connect string Steam passes on for invites and "Join game".
    fn lobby_arg(s: &str) -> Option<u64> {
        let mut it = s.split_whitespace();
        while let Some(w) = it.next() {
            if w == "+connect_lobby" {
                return it.next()?.parse().ok();
            }
        }
        None
    }

    impl Steam {
        pub fn init() -> Option<Steam> {
            if !super::write_dll() {
                return None;
            }
            let client = Client::init_app(APP_ID).ok()?;
            let joins: Arc<Mutex<Vec<u64>>> = Arc::default();
            let j = joins.clone();
            // accepting an invite (or "Join game" in the friends list) while the game is running
            let cb = client.register_callback(move |r: GameLobbyJoinRequested| j.lock().unwrap().push(r.lobby_steam_id.raw()));
            // "Join game" on a friend in the Steam friends list
            let j = joins.clone();
            let cb2 = client.register_callback(move |r: GameRichPresenceJoinRequested| {
                if let Some(id) = lobby_arg(&r.connect) {
                    j.lock().unwrap().push(id);
                }
            });
            client.networking_messages().session_request_callback(|r| {
                r.accept();
            });
            client.networking_utils().init_relay_network_access();
            // launched by Steam from an invite: `+connect_lobby <id>`
            let args: Vec<String> = std::env::args().collect();
            if let Some(id) = lobby_arg(&args.join(" ")) {
                joins.lock().unwrap().push(id);
            }
            Some(Steam { client, joins, lobby: Arc::default(), _cbs: vec![cb, cb2] })
        }
        pub fn tick(&self) {
            self.client.run_callbacks();
        }
        pub fn name(&self) -> String {
            self.client.friends().name()
        }
        pub fn take_join(&self) -> Option<u64> {
            self.joins.lock().unwrap().pop()
        }
        pub fn create(&self) {
            let l = self.lobby.clone();
            self.client.matchmaking().create_lobby(LobbyType::FriendsOnly, 4, move |r| {
                *l.lock().unwrap() = Some(r.map(|id| (id.raw(), true)).map_err(|e| format!("Could not create a Steam lobby: {}", e)));
            });
        }
        pub fn join(&self, id: u64) {
            let l = self.lobby.clone();
            self.client.matchmaking().join_lobby(LobbyId::from_raw(id), move |r| {
                *l.lock().unwrap() = Some(r.map(|id| (id.raw(), false)).map_err(|_| "Could not join that Steam lobby. It may be full or closed.".to_string()));
            });
        }
        /// A lobby we created or joined: (lobby id, we are the host).
        pub fn take_lobby(&self) -> Option<Result<(u64, bool), String>> {
            self.lobby.lock().unwrap().take()
        }
        pub fn link(&self, lobby: u64) -> Box<dyn Link> {
            let lobby = LobbyId::from_raw(lobby);
            let mm = self.client.matchmaking();
            mm.set_lobby_data(lobby, "build", &crate::update::build_number().to_string());
            let known = mm.lobby_members(lobby);
            self.client.friends().set_rich_presence("connect", Some(&format!("+connect_lobby {}", lobby.raw())));
            Box::new(SteamLink { client: self.client.clone(), lobby, known, me: self.client.user().steam_id() })
        }
    }

    struct SteamLink {
        client: Client,
        lobby: LobbyId,
        known: Vec<SteamId>,
        me: SteamId,
    }

    impl Link for SteamLink {
        fn me(&self) -> u64 {
            self.me.raw()
        }
        fn host(&self) -> u64 {
            self.client.matchmaking().lobby_owner(self.lobby).raw()
        }
        fn poll(&mut self) -> Vec<Ev> {
            let mut out = Vec::new();
            let now = self.client.matchmaking().lobby_members(self.lobby);
            for m in &self.known {
                if !now.contains(m) && *m != self.me {
                    out.push(Ev::Leave(m.raw()));
                }
            }
            if !now.is_empty() {
                self.known = now;
            }
            let nm = self.client.networking_messages();
            loop {
                let batch = nm.receive_messages_on_channel(0, 64);
                if batch.is_empty() {
                    break;
                }
                for m in batch {
                    if let Some(id) = m.identity_peer().steam_id() {
                        // only people in our lobby can talk to the game
                        if self.known.contains(&id) {
                            out.push(Ev::Data(id.raw(), m.data().to_vec()));
                        }
                    }
                }
            }
            out
        }
        fn send(&mut self, to: u64, data: &[u8], reliable: bool) -> bool {
            let flags = if reliable { SendFlags::RELIABLE | SendFlags::AUTO_RESTART_BROKEN_SESSION } else { SendFlags::UNRELIABLE_NO_NAGLE };
            let id = NetworkingIdentity::new_steam_id(SteamId::from_raw(to));
            match self.client.networking_messages().send_message_to_user(id, flags, data, 0) {
                Ok(()) => true,
                // the send buffer is full: keep it queued and try next frame
                Err(steamworks::SteamError::LimitExceeded) => false,
                Err(_) => true,
            }
        }
        fn name_of(&self, id: u64) -> String {
            self.client.friends().get_friend(SteamId::from_raw(id)).name()
        }
        fn invite(&self) {
            self.client.friends().activate_invite_dialog(self.lobby);
        }
        fn set_joinable(&self, joinable: bool) {
            self.client.matchmaking().set_lobby_joinable(self.lobby, joinable);
        }
        fn close(&mut self) {
            self.client.friends().set_rich_presence("connect", None);
            self.client.matchmaking().leave_lobby(self.lobby);
        }
    }
}
#[cfg(feature = "steam")]
pub use steam_impl::Steam;

/// Without Steam support compiled in, Steam is simply never available.
#[cfg(not(feature = "steam"))]
pub struct Steam;
#[cfg(not(feature = "steam"))]
#[allow(dead_code)]
impl Steam {
    pub fn init() -> Option<Steam> {
        None
    }
    pub fn tick(&self) {}
    pub fn name(&self) -> String {
        String::new()
    }
    pub fn take_join(&self) -> Option<u64> {
        None
    }
    pub fn create(&self) {}
    pub fn join(&self, _id: u64) {}
    pub fn take_lobby(&self) -> Option<Result<(u64, bool), String>> {
        None
    }
    pub fn link(&self, _lobby: u64) -> Box<dyn Link> {
        unreachable!()
    }
}

/// Windows: the Steam API DLL ships inside the exe and is written next to it on first start, so
/// the game stays a single download (the exe links it lazily, so this runs before it is needed).
#[cfg(all(feature = "steam", windows))]
fn write_dll() -> bool {
    static DLL: &[u8] = include_bytes!("../assets/steam_api64.dll");
    let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|p| p.to_path_buf())) else { return false };
    let p = dir.join("steam_api64.dll");
    let size = |p: &std::path::Path| std::fs::metadata(p).map(|m| m.len() as usize).ok();
    if size(&p) != Some(DLL.len()) {
        let _ = std::fs::write(&p, DLL);
    }
    // without the DLL the first Steam call would crash, so co-op stays off instead
    size(&p) == Some(DLL.len())
}
#[cfg(all(feature = "steam", not(windows)))]
fn write_dll() -> bool {
    true
}

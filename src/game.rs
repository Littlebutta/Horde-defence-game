//! Everything that runs on the main thread each tick: structures, turrets, economy, civilians,
//! rescue squads, defenders, abilities, research and the faction directors.

use crate::defs::*;
use crate::fields::{Done, FieldWorker, Job, DX, DY, INF};
use crate::swarm::{kind_flag, kind_of, Behaviour, RevIn, StepIn, Swarm, DMG_SCALE, F_FLY, TINT_ARMY_SHIFT};
use crate::world::*;
use macroquad::prelude::{vec2, Vec2};
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
use std::sync::Arc;

pub const DT: f32 = 0.05;
/// Civilians one extraction flight carries, and how long a pad needs before the next flight.
pub const PAD_SEATS: u32 = 16;
pub const PAD_COOLDOWN: f32 = 30.0;

pub struct Structure {
    pub t: S,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub hp: f32,
    pub max: f32,
    pub cd: f32,
    pub roof: i32,
    pub dead: bool,
    pub pending: bool,
    pub last_fire: f32,
    pub aim: f32,
    /// artillery: where it's firing; railgun: the spot it's charging at
    pub target: Option<Vec2>,
    /// railgun: seconds of charge left (0 = not charging)
    pub charge: f32,
    /// last time this structure raised a sound cue
    pub snd: f32,
}
impl Structure {
    pub fn center(&self) -> Vec2 {
        vec2(
            self.x as f32 + self.w as f32 / 2.0,
            self.y as f32 + self.h as f32 / 2.0,
        )
    }
}

#[derive(Clone)]
pub struct Member {
    pub p: Vec2,
    pub hp: f32,
    pub cd: f32,
    pub alive: bool,
    pub home: bool,
    /// rescue troops: civilians this soldier is escorting
    pub carry: u8,
}

#[derive(Clone, Copy, PartialEq)]
pub enum GState {
    Wait,
    Move,
    Hold,
    Garrison,
}
pub struct Group {
    pub kind: U,
    pub m: Vec<Member>,
    pub state: GState,
    pub tf: Option<Arc<Vec<u32>>>,
    pub b: i32,
    pub target: Vec2,
    pub dead: bool,
    pub job: u64,
}

#[derive(Clone, Copy, PartialEq)]
pub enum QState {
    Wait,
    Out,
    Gather,
    Return,
}
pub struct Squad {
    pub state: QState,
    pub target: Vec2,
    pub tf: Option<Arc<Vec<u32>>>,
    pub m: Vec<Member>,
    pub t: f32,
    pub done: bool,
    pub wiped: bool,
    pub job: u64,
    /// field towards the civilians nobody has reached yet
    pub gather: Option<Arc<Vec<u32>>>,
    pub gjob: u64,
    pub gt: f32,
    pub found: u32,
    /// the rescue station that sent it (-1: the Rescue squad ability)
    pub from: i32,
}

pub const C_ROAM: u8 = 0;
pub const C_ESCORT: u8 = 1;
pub const C_CAPTURED: u8 = 5;
pub const C_DEAD: u8 = 9;
/// inside the base, walking to the HQ to wait for a flight
pub const C_TO_PAD: u8 = 11;
pub const C_AT_PAD: u8 = 12;
pub const C_GONE: u8 = 13;
/// called from the HQ to an evac point that has a flight ready for them
pub const C_TO_IDLE: u8 = 14;
/// waiting at the HQ until an evac point has a free seat
pub const C_AT_HQ: u8 = 15;
/// Wandering civilians this close to an HQ (in HQ-field cost, about 10 per cell, so roughly
/// 75 cells past the edge of a fresh base zone) run for it.
const HQ_SEEK: u32 = 1500;
/// Scrap and ammo paid per civilian flown out, to make up for the income they cost while waiting.
pub const EVAC_SCRAP: f32 = 30.0;
pub const EVAC_AMMO: f32 = 10.0;

/// Civilians still on the map (not dead, flown out or taken).
pub fn civ_here(st: u8) -> bool {
    st < C_DEAD || matches!(st, C_TO_PAD | C_AT_PAD | C_TO_IDLE | C_AT_HQ)
}

pub struct Civ {
    pub p: Vec2,
    pub st: u8,
    pub sq: usize,
    /// escorting soldier (rescues) or waiting slot (pads)
    pub esc: usize,
    pub pad: usize,
    pub hd: f32,
    pub flee_t: f32,
}

/// An alien hive: spawns bugs, blocks building nearby, grows and infects city buildings.
pub struct Hive {
    pub p: Vec2,
    pub r: f32,
    pub hp: f32,
    pub max: f32,
    pub spawn_t: f32,
    pub grow_t: f32,
    pub infected: Vec<usize>,
    pub dead: bool,
}

#[derive(Clone, Copy, PartialEq)]
pub enum RevKey {
    Struct(usize),
    Group(usize),
    Squad(usize),
}
/// Something that killed a zombie and is now being swarmed.
pub struct Rev {
    pub key: RevKey,
    pub pos: Vec2,
    pub alive: bool,
    pub field: Option<Arc<Vec<u32>>>,
    pub job: u64,
    pub refresh: f32,
    pub hot: f32,
}

pub enum Strike {
    Air {
        from: Vec2,
        dir: Vec2,
        pos: f32,
        len: f32,
        bombs: Vec<(Vec2, f32, bool)>,
        line: (Vec2, Vec2),
    },
    Icbm {
        from: Vec2,
        to: Vec2,
        t: f32,
        dur: f32,
        nuke: bool,
    },
}
pub struct Gunship {
    /// heli pad that owns it (None: the Gunship ability, which flies from the HQ)
    pub pad: Option<usize>,
    pub home: Vec2,
    pub center: Vec2,
    pub p: Vec2,
    pub t: f32,
    pub ang: f32,
    pub cd: f32,
    pub phase: u8, // 0 going out, 1 circling, 2 going home
}
pub enum HeliKind {
    Deliver(usize),
    Extract,
}
pub struct Heli {
    pub kind: HeliKind,
    pub from: Vec2,
    pub to: Vec2,
    pub t: f32,
    pub dur: f32,
}
/// A pool of burning napalm left by a flame bunker.
pub struct Napalm {
    pub p: Vec2,
    pub r: f32,
    pub t: f32,
}
/// An artillery shell in the air.
pub struct Shell {
    pub from: Vec2,
    pub to: Vec2,
    pub t: f32,
    pub dur: f32,
}
pub struct Tracer {
    pub a: Vec2,
    /// height the shot starts from (rooftops, aircraft)
    pub az: f32,
    pub b: Vec2,
    pub t: f32,
    pub kind: u8,
}
pub struct Boom {
    pub p: Vec2,
    pub r: f32,
    pub t: f32,
    pub life: f32,
    pub kind: u8,
}
/// Sound cues the sim raises for the app to play (not part of the checksum or net state).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Sfx {
    Cannon,
    Flame,
    Railgun,
    ArtilleryShot,
    ArtilleryHit,
    Bomb,
    Nuke,
    /// a flame bunker stopped spraying (ends its Flame sound)
    FlameStop,
}
pub struct Portal {
    pub p: Vec2,
    pub t: f32,
    pub taken: u32,
}
struct SpawnQ {
    at: (f32, f32),
    left: usize,
    rate: f32,
    fl: u8,
    aux: f32,
}

/// What a campaign's tech tree has unlocked so far.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Allowed {
    pub builds: Vec<S>,
    pub units: Vec<U>,
    pub abilities: Vec<A>,
}

#[derive(Default, Clone, Copy)]
pub struct Research {
    pub rescue: bool,
    pub air: bool,
    pub nuke: bool,
    pub armour: bool,
    pub presses: bool,
    pub barrels: bool,
    pub busy: Option<(R, f32)>,
}
impl Research {
    pub fn has(&self, r: R) -> bool {
        match r {
            R::Rescue => self.rescue,
            R::Carpet => self.air,
            R::Nuke => self.nuke,
            R::Armour => self.armour,
            R::Presses => self.presses,
            R::Barrels => self.barrels,
        }
    }
    pub fn set(&mut self, r: R) {
        match r {
            R::Rescue => self.rescue = true,
            R::Carpet => self.air = true,
            R::Nuke => self.nuke = true,
            R::Armour => self.armour = true,
            R::Presses => self.presses = true,
            R::Barrels => self.barrels = true,
        }
    }
}

pub struct Game {
    pub world: World,
    pub fac: usize,
    pub mult: f32,
    /// the army the director is spawning for right now (chaos games have three)
    army: usize,
    pub swarm: Swarm,
    pub obj: Arc<Vec<i32>>,
    /// which neighbours a unit may step to from each cell (see `fields::neighbour_mask`)
    nb: Arc<Vec<u8>>,
    pub hq_dist: Arc<Vec<u32>>,
    pub hunt: Option<Arc<Vec<u16>>>,
    fw_hq: FieldWorker,
    fw_hunt: FieldWorker,
    fw_aux: FieldWorker,
    pub revs: Vec<Rev>,
    lock: Vec<u16>,
    lock_cells: Vec<usize>,
    low: Vec<bool>,
    pub hives: Vec<Hive>,
    hive_t: f32,
    /// one field per evac point (structure index), towards that pad
    pub pad_fields: Vec<(usize, Arc<Vec<u32>>)>,
    pad_jobs: Vec<(u64, usize)>,
    pad_dirty: bool,
    pub portal_field: Option<Arc<Vec<u32>>>,
    portal_job: u64,
    portal_ft: f32,
    pub obj_dirty: bool,
    obj_timer: f32,
    hunt_timer: f32,
    job_seq: u64,
    pub structs: Vec<Structure>,
    pub occ: Vec<i32>,
    pub hq_idx: usize,
    /// co-op: each player's HQ, host first (one entry when playing alone)
    pub hqs: Vec<usize>,
    pub base_zone: Vec<u8>,
    pub groups: Vec<Group>,
    pub squads: Vec<Squad>,
    pub civs: Vec<Civ>,
    pub strikes: Vec<Strike>,
    pub gunships: Vec<Gunship>,
    pub shells: Vec<Shell>,
    pub napalm: Vec<Napalm>,
    /// storage limits, from the depots (recomputed every tick)
    pub scrap_cap: f32,
    pub ammo_cap: f32,
    /// coarse cells inside a helium tower's field (the horde moves slowly there)
    pub slow: Vec<u8>,
    pub helis: Vec<Heli>,
    pub tracers: Vec<Tracer>,
    pub booms: Vec<Boom>,
    /// sound cues since the app last drained them
    pub sfx: Vec<(Sfx, Vec2)>,
    pub portals: Vec<Portal>,
    spawnq: Vec<SpawnQ>,
    conv: Vec<Vec2>,
    pub scrap: f32,
    pub ammo: f32,
    pub score: f32,
    pub kills: u64,
    pub rescued: u32,
    pub waiting: u32,
    pub extracted: u32,
    pub civ_lost: u32,
    pub time: f32,
    /// simulation ticks so far
    pub ticks: u64,
    pub cds: [f32; 4],
    pub research: Research,
    pub rush: bool,
    pub rush_t: f32,
    pub rush_left: f32,
    pub rushes: u32,
    pub rush_forever: bool,
    pub out_t: f32,
    outbreaks: u32,
    pub drop_t: f32,
    pub portal_t: f32,
    portals_opened: u32,
    pub over: bool,
    /// campaign: the city is held once `time` reaches `goal` seconds (0 = endless)
    pub goal: f32,
    pub won: bool,
    /// campaign game: research is unlocked between cities, not at the lab
    pub campaign: bool,
    /// campaign tech tree: what may be built, recruited and used (None: everything, a normal game)
    pub allowed: Option<Allowed>,
    /// campaign turret upgrades bought
    pub ups: Vec<Up>,
    pub income_mult: f32,
    pub banner: (String, f32),
    pub dirty_buildings: Vec<usize>,
    pub zone_dirty: bool,
    pub struct_dirty: bool,
    pub sim_ms: f32,
    prof: [f64; 12],
    prof_n: u64,
    pub rng: Rng,
    sd: Vec<AtomicU32>,
    bd: Vec<AtomicU32>,
}

fn ability_index(a: A) -> usize {
    match a {
        A::Rescue => 0,
        A::Air => 1,
        A::Gunship => 2,
        A::Icbm => 3,
    }
}

impl Game {
    pub fn new(desc: Desc, fac: usize, mult: f32, seed: u64) -> Game {
        let world = World::build(desc);
        let base_zone = world.zone.clone();
        let nb = world.buildings.len();
        let mut g = Game {
            world,
            fac,
            mult,
            army: fac.min(2),
            swarm: Swarm::new(),
            obj: Arc::new(vec![FREE; N]),
            nb: Arc::new(crate::fields::neighbour_mask(&vec![FREE; N])),
            hq_dist: Arc::new(vec![INF; N]),
            hunt: None,
            fw_hq: FieldWorker::new(),
            fw_hunt: FieldWorker::new(),
            fw_aux: FieldWorker::new(),
            revs: Vec::new(),
            lock: vec![0; CW * CH],
            lock_cells: Vec::new(),
            low: Vec::new(),
            hives: Vec::new(),
            hive_t: 75.0,
            pad_fields: Vec::new(),
            pad_jobs: Vec::new(),
            pad_dirty: true,
            portal_field: None,
            portal_job: 0,
            portal_ft: 0.0,
            obj_dirty: true,
            obj_timer: 0.0,
            hunt_timer: 0.0,
            job_seq: 1,
            structs: Vec::new(),
            occ: vec![-1; N],
            hq_idx: 0,
            hqs: Vec::new(),
            base_zone,
            groups: Vec::new(),
            squads: Vec::new(),
            civs: Vec::new(),
            strikes: Vec::new(),
            gunships: Vec::new(),
            shells: Vec::new(),
            napalm: Vec::new(),
            scrap_cap: SCRAP_CAP,
            ammo_cap: AMMO_CAP,
            slow: vec![0; CW * CH],
            helis: Vec::new(),
            tracers: Vec::new(),
            booms: Vec::new(),
            sfx: Vec::new(),
            portals: Vec::new(),
            spawnq: Vec::new(),
            conv: Vec::new(),
            scrap: 700.0,
            ammo: 400.0,
            score: 0.0,
            kills: 0,
            rescued: 0,
            waiting: 0,
            extracted: 0,
            civ_lost: 0,
            time: 0.0,
            ticks: 0,
            cds: [0.0; 4],
            research: Research::default(),
            rush: false,
            rush_t: 150.0,
            rush_left: 0.0,
            rushes: 0,
            rush_forever: false,
            out_t: 70.0,
            outbreaks: 0,
            drop_t: 2.0,
            portal_t: 20.0,
            portals_opened: 0,
            over: false,
            goal: 0.0,
            won: false,
            campaign: false,
            allowed: None,
            ups: Vec::new(),
            income_mult: 1.0,
            banner: (String::new(), 0.0),
            dirty_buildings: Vec::new(),
            zone_dirty: true,
            struct_dirty: true,
            sim_ms: 0.0,
            prof: [0.0; 12],
            prof_n: 0,
            rng: Rng::new(seed),
            sd: Vec::new(),
            bd: (0..nb).map(|_| AtomicU32::new(0)).collect(),
        };
        let hq = g.world.hq;
        g.hq_idx = g.add_struct(S::Hq, hq.x as i32 - 7, hq.y as i32 - 7, -1);
        g.hqs = vec![g.hq_idx];
        // starter base
        let (x0, y0) = (hq.x as i32, hq.y as i32);
        for (t, x, y) in [
            (S::Extractor, x0 - 26, y0 - 3),
            (S::Plant, x0 + 19, y0 - 3),
            (S::Gun, x0 - 2, y0 - 18),
            (S::Gun, x0 - 2, y0 + 13),
            (S::Pad, x0 - 5, y0 + 30),
        ] {
            if g.can_place(t, x, y) {
                g.add_struct(t, x, y, -1);
            }
        }
        g.build_perimeter_wall();
        // blocking first field so the opening spawns have somewhere to go
        let obj = Arc::new(g.build_obj());
        let src = g.hq_src();
        let d = crate::fields::dial(&obj, &src, crate::fields::Mode::Hq, INF - 1);
        g.obj = obj;
        g.hq_dist = Arc::new(d);
        g.obj_dirty = false;
        let ncivs = (9000.0 * (g.world.buildings.len() as f32).sqrt() / 70.0).clamp(3000.0, 14000.0)
            as usize;
        g.init_civilians(ncivs);
        if fac == 0 || fac == CHAOS {
            g.army = 0;
            for _ in 0..6 {
                if let Some(p) = g.random_cell(3000) {
                    g.queue(p, (6.0 * mult).max(3.0), 1.0);
                }
            }
        }
        g
    }

    pub fn faction(&self) -> &'static Faction {
        &FACTIONS[self.fac]
    }
    /// This army is in the game: the one picked, or all three in chaos.
    pub fn has_army(&self, k: usize) -> bool {
        self.fac == k || self.fac == CHAOS
    }
    /// The army of whatever stands in cell `c`.
    fn army_at(&self, c: usize) -> usize {
        if self.fac == CHAOS {
            (self.swarm.tint[c].load(Relaxed) >> TINT_ARMY_SHIFT) as usize
        } else {
            self.fac
        }
    }
    pub fn show(&mut self, s: impl Into<String>, t: f32) {
        self.banner = (s.into(), t);
    }

    // ----------------------------------------------------------------------------- grids

    /// The horde heads for every HQ still standing.
    fn hq_src(&self) -> Vec<u32> {
        let mut v = Vec::new();
        for &i in &self.hqs {
            let s = &self.structs[i];
            if s.dead && self.hqs.len() > 1 {
                continue;
            }
            for y in s.y..s.y + s.h {
                for x in s.x..s.x + s.w {
                    v.push((y as usize * W + x as usize) as u32);
                }
            }
        }
        v
    }

    /// The standing HQ nearest to `p` (the main one when they have all fallen).
    pub fn hq_near(&self, p: Vec2) -> Vec2 {
        self.hqs
            .iter()
            .map(|&i| &self.structs[i])
            .filter(|s| !s.dead)
            .map(|s| s.center())
            .min_by(|a, b| a.distance_squared(p).total_cmp(&b.distance_squared(p)))
            .unwrap_or(self.world.hq)
    }

    /// Co-op: an HQ (with a small starter base and its own base zone) for each player after the
    /// host, spread out around the main one. Both sides call this right after `new`, so it must
    /// stay deterministic.
    pub fn add_player_hqs(&mut self, players: usize) {
        let main = self.world.hq;
        for k in 1..players.min(4) {
            let ang = -std::f32::consts::FRAC_PI_2 + k as f32 * std::f32::consts::TAU / players.min(4) as f32;
            let Some(c) = self.hq_spot(main, ang) else { continue };
            let z = &mut self.base_zone;
            fill_disc(c, OUTPOST_R, |q| z[q] = 1);
            self.world.add_outpost(c);
            let (x0, y0) = (c.x as i32, c.y as i32);
            let i = self.add_struct(S::Hq, x0 - 7, y0 - 7, -1);
            self.hqs.push(i);
            for (t, x, y) in [(S::Extractor, x0 - 26, y0 - 3), (S::Gun, x0 - 2, y0 - 18), (S::Gun, x0 - 2, y0 + 13)] {
                if self.can_place(t, x, y) {
                    self.add_struct(t, x, y, -1);
                }
            }
        }
        if self.hqs.len() > 1 {
            self.zone_dirty = true;
            let obj = Arc::new(self.build_obj());
            let src = self.hq_src();
            let d = crate::fields::dial(&obj, &src, crate::fields::Mode::Hq, INF - 1);
            self.obj = obj;
            self.hq_dist = Arc::new(d);
            self.obj_dirty = false;
        }
    }

    /// A free 14x14 patch of dry ground about 160 cells from `main` in direction `ang`.
    fn hq_spot(&self, main: Vec2, ang: f32) -> Option<Vec2> {
        let free = |x: i32, y: i32| {
            if x < 40 || y < 40 || x > W as i32 - 40 || y > H as i32 - 40 {
                return false;
            }
            for yy in y - 9..y + 9 {
                for xx in x - 9..x + 9 {
                    let i = yy as usize * W + xx as usize;
                    if self.occ[i] >= 0 || self.world.bid[i] >= 0 || self.world.terrain[i] == T_WATER {
                        return false;
                    }
                }
            }
            true
        };
        for ring in 0..12 {
            let dist = 160.0 + ring as f32 * 12.0 * if ring % 2 == 0 { 1.0 } else { -1.0 } * 0.5;
            for step in 0..24 {
                let da = (step as f32 * 0.5).ceil() * 0.07 * if step % 2 == 0 { 1.0 } else { -1.0 };
                let a = ang + da;
                let c = main + vec2(libm::cosf(a), libm::sinf(a)) * dist;
                let (x, y) = (c.x as i32, c.y as i32);
                if free(x, y) {
                    return Some(vec2(x as f32, y as f32));
                }
            }
        }
        None
    }

    pub fn build_obj(&self) -> Vec<i32> {
        let w = &self.world;
        use rayon::prelude::*;
        let mut o: Vec<i32> = (0..N)
            .into_par_iter()
            .map(|i| {
                let t = w.terrain[i];
                if t == T_WATER {
                    SOLID
                } else if w.bid[i] >= 0 {
                    -(w.bid[i] + 2)
                } else {
                    FREE
                }
            })
            .collect();
        for (i, s) in self.structs.iter().enumerate() {
            if s.dead || s.roof >= 0 || s.pending {
                continue;
            }
            for y in s.y..s.y + s.h {
                for x in s.x..s.x + s.w {
                    o[y as usize * W + x as usize] = i as i32;
                }
            }
        }
        o
    }

    fn pump_fields(&mut self) {
        self.ticks += 1;
        for fw in [&mut self.fw_hq, &mut self.fw_aux, &mut self.fw_hunt] {
            fw.now = self.ticks;
        }
        while let Some(d) = self.fw_hq.poll() {
            match d {
                Done::Hq { obj, d, nb } => {
                    self.obj = obj;
                    self.nb = nb;
                    self.hq_dist = d;
                    self.pad_dirty = true;
                }
                Done::Target { id, d } => {
                    for g in &mut self.groups {
                        if g.job == id {
                            g.tf = Some(d.clone());
                            g.state = GState::Move;
                        }
                    }
                    for q in &mut self.squads {
                        if q.job == id {
                            q.tf = Some(d.clone());
                            q.state = QState::Out;
                        }
                    }
                }
                _ => {}
            }
        }
        while let Some(d) = self.fw_aux.poll() {
            if let Done::Multi { id, d } = d {
                if let Some(k) = self.pad_jobs.iter().position(|j| j.0 == id) {
                    let (_, pi) = self.pad_jobs.remove(k);
                    self.pad_fields.retain(|f| f.0 != pi);
                    self.pad_fields.push((pi, d));
                } else if id == self.portal_job {
                    self.portal_field = Some(d);
                } else if let Some(r) = self.revs.iter_mut().find(|r| r.job == id) {
                    r.field = Some(d);
                } else if let Some(q) = self.squads.iter_mut().find(|q| q.gjob == id) {
                    q.gather = Some(d);
                }
            }
        }
        // evac points: a field to each, for civilians called over from the HQ
        if self.pad_dirty && self.fw_aux.busy < 3 {
            self.pad_dirty = false;
            self.pad_jobs.clear();
            let live = |s: &Structure| !s.dead && !s.pending && s.t == S::Pad;
            let structs = &self.structs;
            self.pad_fields.retain(|f| structs.get(f.0).is_some_and(live));
            for pi in 0..self.structs.len() {
                if !live(&self.structs[pi]) {
                    continue;
                }
                let src = self.ring_of(pi);
                if src.is_empty() {
                    continue;
                }
                let id = self.next_job();
                self.pad_jobs.push((id, pi));
                self.fw_aux.submit(Job::Multi { id, obj: self.obj.clone(), src, max: 9000 });
            }
        }
        // orc portals: where captured civilians are dragged
        self.portal_ft -= DT;
        if self.has_army(2) && self.portal_ft <= 0.0 && self.fw_aux.busy < 3 {
            self.portal_ft = 3.0;
            let mut src = Vec::new();
            for p in &self.portals {
                if let Some(c) = self.nearest_walkable(p.p, 12) {
                    src.push(c as u32);
                }
            }
            if src.is_empty() {
                self.portal_field = None;
            } else {
                self.portal_job = self.next_job();
                self.fw_aux.submit(Job::Multi {
                    id: self.portal_job,
                    obj: self.obj.clone(),
                    src,
                    max: 30000,
                });
            }
        }
        while let Some(d) = self.fw_hunt.poll() {
            if let Done::Hunt(h) = d {
                self.hunt = Some(h);
            }
        }
        self.obj_timer -= DT;
        if self.obj_dirty && self.obj_timer <= 0.0 && self.fw_hq.busy == 0 {
            self.obj_dirty = false;
            self.obj_timer = 0.3;
            let obj = Arc::new(self.build_obj());
            let src = self.hq_src();
            self.fw_hq.submit(Job::Hq { obj, src });
        }
        self.hunt_timer -= DT;
        if self.hunt_timer <= 0.0 && self.fw_hunt.busy == 0 {
            self.hunt_timer = 2.0;
            let f = self.faction();
            let mut src: Vec<u32> = Vec::new();
            // hordes go after turrets that have been shooting them; a rooftop turret makes them attack the building
            for s in &self.structs {
                if s.dead || s.pending || !def(s.t).turret || self.time - s.last_fire > 3.0 {
                    continue;
                }
                if s.roof >= 0 {
                    let b = &self.world.buildings[s.roof as usize];
                    for y in b.y0.max(0.0) as usize..=(b.y1 as usize).min(H - 1) {
                        for x in b.x0.max(0.0) as usize..=(b.x1 as usize).min(W - 1) {
                            if self.world.bid[y * W + x] == s.roof {
                                src.push((y * W + x) as u32);
                            }
                        }
                    }
                } else {
                    for y in s.y..s.y + s.h {
                        for x in s.x..s.x + s.w {
                            src.push((y as usize * W + x as usize) as u32);
                        }
                    }
                }
            }
            if f.stats.mode != Behaviour::Alien {
                // a civilian already being dragged to a portal is no longer prey
                for c in &self.civs {
                    if c.st < C_DEAD && c.st != C_CAPTURED {
                        src.push((c.p.y as usize * W + c.p.x as usize) as u32);
                    }
                }
                // walls are never a target in themselves: the horde walks around them, and only
                // chews through one when it blocks the way to the HQ (the HQ field handles that)
                for s in &self.structs {
                    if s.dead || s.pending || s.roof >= 0 || s.t == S::Wall {
                        continue;
                    }
                    for y in s.y..s.y + s.h {
                        for x in s.x..s.x + s.w {
                            src.push((y as usize * W + x as usize) as u32);
                        }
                    }
                }
                for q in &self.squads {
                    for m in &q.m {
                        if m.alive && !m.home {
                            src.push((m.p.y as usize * W + m.p.x as usize) as u32);
                        }
                    }
                }
                for g in &self.groups {
                    if g.dead || g.state == GState::Garrison {
                        continue;
                    }
                    for m in &g.m {
                        if m.alive {
                            src.push((m.p.y as usize * W + m.p.x as usize) as u32);
                        }
                    }
                }
            }
            if src.is_empty() {
                self.hunt = None;
            } else {
                self.fw_hunt.submit(Job::Hunt {
                    obj: self.obj.clone(),
                    src,
                    max: f.sight,
                });
            }
        }
    }

    fn next_job(&mut self) -> u64 {
        let id = self.job_seq;
        self.job_seq += 1;
        id
    }

    /// Open cells just outside every live structure of one type.
    /// The open cells around one structure.
    fn ring_of(&self, i: usize) -> Vec<u32> {
        let s = &self.structs[i];
        let mut v = Vec::new();
        for y in s.y - 1..=s.y + s.h {
            for x in s.x - 1..=s.x + s.w {
                if x < 0 || y < 0 || x >= W as i32 || y >= H as i32 {
                    continue;
                }
                let c = y as usize * W + x as usize;
                if self.obj[c] == FREE {
                    v.push(c as u32);
                }
            }
        }
        v
    }

    fn target_job(&mut self, tc: usize) -> u64 {
        let id = self.next_job();
        self.fw_hq.submit(Job::Target {
            id,
            obj: self.obj.clone(),
            src: tc as u32,
        });
        id
    }

    // ----------------------------------------------------------------------------- structures

    pub fn has(&self, t: S) -> bool {
        self.structs
            .iter()
            .any(|s| !s.dead && !s.pending && s.t == t)
    }

    /// Price of a structure at a spot: outside the base zone, turrets get dearer the further
    /// they are from the HQ. Inside the zone everything costs its base price.
    pub fn cost_at(&self, t: S, at: Vec2) -> f32 {
        let d = def(t);
        let mut c = d.cost;
        if d.turret && !self.world.in_zone(at) {
            let dist = at.distance(self.hq_near(at));
            c *= 1.0 + (dist - 100.0).max(0.0) / 350.0;
        }
        c.round()
    }

    pub fn can_place(&self, t: S, x: i32, y: i32) -> bool {
        let d = def(t);
        if !self.build_ok(t) {
            return false;
        }
        if x < 1 || y < 1 || x + d.w >= W as i32 || y + d.h >= H as i32 {
            return false;
        }
        for yy in y..y + d.h {
            for xx in x..x + d.w {
                let i = yy as usize * W + xx as usize;
                if self.occ[i] >= 0 || self.world.terrain[i] == T_WATER || self.world.bid[i] >= 0 {
                    return false;
                }
            }
        }
        let c = vec2(x as f32 + d.w as f32 / 2.0, y as f32 + d.h as f32 / 2.0);
        if self.world.irradiated_at(c) || self.hive_blocks(c) {
            return false;
        }
        if d.zone && !self.world.in_zone(c) {
            return false;
        }
        true
    }

    /// The building standing on a ground point (the app turns a click on a roof into that point).
    pub fn roof_at(&self, w: Vec2) -> Option<(usize, Vec2)> {
        if w.x < 0.0 || w.y < 0.0 || w.x >= W as f32 || w.y >= H as f32 {
            return None;
        }
        let b = self.world.bid[w.y as usize * W + w.x as usize];
        (b >= 0).then_some((b as usize, w))
    }
    pub fn roof_slots(&self, b: usize) -> usize {
        ((self.world.buildings[b].cells / 70) as usize).clamp(1, 6)
    }
    pub fn can_place_roof(&self, t: S, b: usize, p: Vec2) -> bool {
        let bb = &self.world.buildings[b];
        if !bb.alive || self.world.irradiated_at(p) || self.hive_blocks(p) {
            return false;
        }
        // the whole base has to stand on this roof
        let d = def(t);
        let (x0, y0) = ((p.x - d.w as f32 / 2.0) as i32, (p.y - d.h as f32 / 2.0) as i32);
        for y in y0..y0 + d.h {
            for x in x0..x0 + d.w {
                if x < 0 || y < 0 || x >= W as i32 || y >= H as i32 || self.world.bid[y as usize * W + x as usize] != b as i32 {
                    return false;
                }
            }
        }
        let mut n = 0;
        for s in &self.structs {
            if !s.dead && s.roof == b as i32 {
                n += 1;
                if s.center().distance(p) < 5.0 {
                    return false;
                }
            }
        }
        n < self.roof_slots(b)
    }

    pub fn add_struct(&mut self, t: S, x: i32, y: i32, roof: i32) -> usize {
        let d = def(t);
        let idx = self.structs.len();
        let hp = d.hp * self.armour(t);
        let mut s = Structure {
            t,
            x,
            y,
            w: d.w,
            h: d.h,
            hp,
            max: hp,
            cd: 0.0,
            roof,
            dead: false,
            pending: false,
            last_fire: -100.0,
            aim: -1.2,
            target: None,
            charge: 0.0,
            snd: -100.0,
        };
        if roof < 0 {
            for yy in y..y + d.h {
                for xx in x..x + d.w {
                    self.occ[yy as usize * W + xx as usize] = idx as i32;
                }
            }
        }
        let c = s.center();
        if d.turret && !self.world.in_zone(c) {
            s.pending = true;
            let from = self.hq_near(c);
            let dist = c.distance(from);
            self.helis.push(Heli {
                kind: HeliKind::Deliver(idx),
                from,
                to: c,
                t: 0.0,
                dur: 1.5 + dist / 45.0,
            });
        }
        if t == S::Outpost {
            self.world.add_outpost(c);
            self.zone_dirty = true;
        }
        if roof < 0 && !s.pending {
            self.obj_dirty = true;
            self.struct_dirty = true;
        }
        if t == S::Pad {
            self.pad_dirty = true;
        }
        self.structs.push(s);
        self.sd.push(AtomicU32::new(0));
        self.low.push(t == S::Wall);
        idx
    }

    /// Health multiplier from Reinforced armour (walls and turrets only).
    pub fn armour(&self, t: S) -> f32 {
        let hull = if t == S::Helium && self.has_up(Up::HeliumHull) { 2.0 } else { 1.0 };
        hull * if self.research.armour && (t == S::Wall || def(t).turret) { 1.75 } else { 1.0 }
    }
    /// A turret's reach, with rooftop and Long barrels bonuses.
    pub fn turret_range(&self, t: S, roof: bool) -> f32 {
        let up = match t {
            S::Flame if self.has_up(Up::FlameReach) => 1.3,
            S::Helium if self.has_up(Up::HeliumReach) => 1.35,
            _ => 1.0,
        };
        def(t).range * if roof { 1.6 } else { 1.0 } * if self.research.barrels { 1.25 } else { 1.0 } * up
    }
    pub fn has_up(&self, u: Up) -> bool {
        self.ups.contains(&u)
    }
    /// Damage multiplier from campaign upgrades.
    pub fn dmg_mult(&self, t: S) -> f32 {
        match t {
            S::Gun if self.has_up(Up::GunAp) => 1.5,
            S::Flame if self.has_up(Up::FlameNapalm) => 1.5,
            S::Artillery if self.has_up(Up::ArtyHe) => 1.3,
            _ => 1.0,
        }
    }
    /// Blast or beam width multiplier from campaign upgrades.
    pub fn rad_mult(&self, t: S) -> f32 {
        match t {
            S::Cannon if self.has_up(Up::CannonSplash) => 1.4,
            S::Rail if self.has_up(Up::RailWide) => 1.5,
            S::Artillery if self.has_up(Up::ArtyHe) => 1.4,
            _ => 1.0,
        }
    }
    /// Time between shots, with campaign upgrades.
    pub fn reload(&self, t: S) -> f32 {
        def(t).rate
            * match t {
                S::Gun if self.has_up(Up::GunRapid) => 1.0 / 1.4,
                S::Cannon if self.has_up(Up::CannonAuto) => 0.7,
                S::Rail if self.has_up(Up::RailCaps) => 0.65,
                S::Artillery if self.has_up(Up::ArtyRapid) => 0.7,
                _ => 1.0,
            }
    }
    /// Whether the campaign's tech tree lets this be built (always, outside a campaign).
    pub fn build_ok(&self, t: S) -> bool {
        t == S::Hq || self.allowed.as_ref().is_none_or(|a| a.builds.contains(&t))
    }
    pub fn unit_ok(&self, u: U) -> bool {
        self.allowed.as_ref().is_none_or(|a| a.units.contains(&u))
    }
    pub fn ability_ok(&self, a: A) -> bool {
        self.allowed.as_ref().is_none_or(|al| al.abilities.contains(&a))
    }

    /// Buys and places a structure on the ground. Returns false if it can't go there or you can't afford it.
    pub fn place(&mut self, t: S, x: i32, y: i32) -> bool {
        let d = def(t);
        let c = vec2(x as f32 + d.w as f32 / 2.0, y as f32 + d.h as f32 / 2.0);
        let cost = self.cost_at(t, c);
        if self.scrap < cost || !self.can_place(t, x, y) {
            return false;
        }
        self.scrap -= cost;
        self.add_struct(t, x, y, -1);
        true
    }
    /// Buys and places a turret on a rooftop.
    pub fn place_roof(&mut self, t: S, b: usize, p: Vec2) -> bool {
        let d = def(t);
        let cost = self.cost_at(t, p);
        if !d.turret || d.ground || self.scrap < cost || !self.can_place_roof(t, b, p) || !self.build_ok(t) {
            return false;
        }
        self.scrap -= cost;
        self.add_struct(
            t,
            (p.x - d.w as f32 / 2.0) as i32,
            (p.y - d.h as f32 / 2.0) as i32,
            b as i32,
        );
        true
    }
    /// The structure (not the HQ or a wall) under a point.
    pub fn struct_at(&self, p: Vec2) -> Option<usize> {
        self.structs.iter().enumerate().rev().find(|(_, s)| {
            !s.dead
                && !matches!(s.t, S::Hq | S::Wall)
                && p.x >= s.x as f32
                && p.y >= s.y as f32
                && p.x < (s.x + s.w) as f32
                && p.y < (s.y + s.h) as f32
        }).map(|(i, _)| i)
    }
    /// Sells the structure under a point for half its base price.
    pub fn sell_at(&mut self, p: Vec2) -> bool {
        let hit = self.structs.iter().enumerate().rev().find(|(_, s)| {
            if s.dead || s.t == S::Hq {
                return false;
            }
            let off = if s.roof >= 0 { Vec2::ZERO } else { Vec2::ZERO };
            let q = p - off;
            q.x >= s.x as f32 && q.y >= s.y as f32 && q.x < (s.x + s.w) as f32 && q.y < (s.y + s.h) as f32
        });
        let Some((i, s)) = hit else { return false };
        let refund = def(s.t).cost * 0.5;
        self.gain_scrap(refund);
        self.kill_struct(i);
        true
    }

    pub fn hostiles(&self) -> usize {
        self.swarm.len()
    }

    pub fn kill_struct(&mut self, i: usize) {
        if self.structs[i].dead {
            return;
        }
        self.structs[i].dead = true;
        let s = &self.structs[i];
        if s.roof < 0 {
            for yy in s.y..s.y + s.h {
                for xx in s.x..s.x + s.w {
                    let c = yy as usize * W + xx as usize;
                    if self.occ[c] == i as i32 {
                        self.occ[c] = -1;
                    }
                }
            }
            self.obj_dirty = true;
            self.struct_dirty = true;
        }
        let t = s.t;
        if t == S::Outpost {
            self.rebuild_zone();
        }
        if t == S::Pad {
            self.pad_dirty = true;
        }
        if t == S::Hq {
            // co-op: the game goes on while any HQ stands
            let left: Vec<usize> = self.hqs.iter().copied().filter(|&h| !self.structs[h].dead).collect();
            if left.is_empty() {
                self.over = true;
            } else {
                self.show("An HQ has fallen. Hold out at the others.", 6.0);
                if i == self.hq_idx {
                    self.hq_idx = left[0];
                    self.world.hq = self.structs[left[0]].center();
                }
                self.obj_dirty = true;
            }
        }
    }

    fn rebuild_zone(&mut self) {
        self.world.zone = self.base_zone.clone();
        let outposts: Vec<Vec2> = self
            .structs
            .iter()
            .filter(|s| !s.dead && s.t == S::Outpost)
            .map(|s| s.center())
            .collect();
        for c in outposts {
            self.world.add_outpost(c);
        }
        self.zone_dirty = true;
    }

    /// Walls along the edge of the airport wherever the horde could walk in, leaving the roads open.
    fn build_perimeter_wall(&mut self) {
        let mut edge = vec![false; N];
        let w = &self.world;
        let walk = |c: usize| World::walkable_terrain(w.terrain[c]) && w.bid[c] < 0;
        for y in 3..H - 3 {
            for x in 3..W - 3 {
                let c = y * W + x;
                if w.zone[c] != 1 || !walk(c) || w.terrain[c] == T_ROAD {
                    continue;
                }
                // the fence runs along the whole edge of the base zone
                let mut outside = false;
                'o: for oy in -1i32..=1 {
                    for ox in -1i32..=1 {
                        let nc = (y as i32 + oy) as usize * W + (x as i32 + ox) as usize;
                        if w.zone[nc] != 1 {
                            outside = true;
                            break 'o;
                        }
                    }
                }
                edge[c] = true && outside;
            }
        }
        let mut placed = vec![false; (W / 3) * (H / 3)];
        for y in 0..H {
            for x in 0..W {
                if !edge[y * W + x] {
                    continue;
                }
                let (gx, gy) = (x / 3, y / 3);
                if placed[gy * (W / 3) + gx] {
                    continue;
                }
                placed[gy * (W / 3) + gx] = true;
                let (wx, wy) = (gx as i32 * 3, gy as i32 * 3);
                let mut road = false;
                for yy in wy - 1..wy + 4 {
                    for xx in wx - 1..wx + 4 {
                        if self.world.terrain[yy as usize * W + xx as usize] == T_ROAD {
                            road = true;
                        }
                    }
                }
                if !road && self.can_place(S::Wall, wx, wy) {
                    self.add_struct(S::Wall, wx, wy, -1);
                }
            }
        }
    }

    pub fn damage_buildings(&mut self, at: Vec2, r: f32, per_cell: f32) {
        if per_cell <= 0.0 {
            return;
        }
        let mut hit: std::collections::BTreeMap<i32, f32> = std::collections::BTreeMap::new();
        let bid = &self.world.bid;
        fill_disc(at, r, |c| {
            let b = bid[c];
            if b >= 0 {
                *hit.entry(b).or_insert(0.0) += per_cell;
            }
        });
        for (b, d) in hit {
            let bb = &mut self.world.buildings[b as usize];
            if !bb.alive {
                continue;
            }
            bb.hp -= d;
            if bb.hp <= 0.0 {
                self.destroy_building(b as usize);
            }
        }
    }

    pub fn destroy_building(&mut self, b: usize) {
        let bb = &mut self.world.buildings[b];
        if !bb.alive {
            return;
        }
        bb.alive = false;
        let (x0, y0, x1, y1) = (
            bb.x0.max(0.0) as usize,
            bb.y0.max(0.0) as usize,
            (bb.x1 as usize).min(W - 1),
            (bb.y1 as usize).min(H - 1),
        );
        for y in y0..=y1 {
            for x in x0..=x1 {
                let c = y * W + x;
                if self.world.bid[c] == b as i32 {
                    self.world.bid[c] = -1;
                    self.world.terrain[c] = T_RUBBLE;
                }
            }
        }
        let on_roof: Vec<usize> = self
            .structs
            .iter()
            .enumerate()
            .filter(|(_, s)| !s.dead && s.roof == b as i32)
            .map(|(i, _)| i)
            .collect();
        for i in on_roof {
            self.kill_struct(i);
        }
        self.dirty_buildings.push(b);
        self.obj_dirty = true;
    }

    // ----------------------------------------------------------------------------- spawning

    pub fn random_cell(&mut self, min_hq: u32) -> Option<(f32, f32)> {
        for _ in 0..600 {
            let x = self.rng.below(W);
            let y = self.rng.below(H);
            let c = y * W + x;
            if self.obj[c] != FREE
                || self.hq_dist[c] == INF
                || self.hq_dist[c] < min_hq
                || self.world.zone[c] == 1
            {
                continue;
            }
            return Some((x as f32 + 0.5, y as f32 + 0.5));
        }
        None
    }
    pub fn queue(&mut self, at: (f32, f32), count: f32, secs: f32) {
        self.queue_ex(at, count, secs, 0, 0.0);
    }
    fn queue_ex(&mut self, at: (f32, f32), count: f32, secs: f32, fl: u8, aux: f32) {
        self.queue_army(at, count, secs, fl, aux, self.army);
    }
    fn queue_army(&mut self, at: (f32, f32), count: f32, secs: f32, fl: u8, aux: f32, army: usize) {
        if count >= 1.0 {
            self.spawnq.push(SpawnQ {
                at,
                left: count.round() as usize,
                rate: (count / secs).max(1.0),
                fl: fl | kind_flag(army),
                aux,
            });
        }
    }
    pub fn stress(&mut self) {
        // PH_STRESS_N sets the size of the test horde (default one million)
        let n: f32 = std::env::var("PH_STRESS_N").ok().and_then(|s| s.parse().ok()).unwrap_or(1e6);
        self.stress_n(n);
    }
    /// Test horde of `n` hostiles from eight random spots (the in-game "Spawn 1M" button).
    pub fn stress_n(&mut self, n: f32) {
        for k in 0..8 {
            if let Some(p) = self.random_cell(1500) {
                // chaos: every army gets some
                let army = if self.fac == CHAOS { k % 3 } else { self.army };
                self.queue_army(p, n / 8.0, 6.0, 0, 0.0, army);
            }
        }
        self.show(format!("{} hostiles inbound", n as u64), 4.0);
    }

    /// Each army's own way of coming at you, all three in chaos (each a little smaller).
    fn director(&mut self) {
        if self.fac == CHAOS {
            for k in 0..3 {
                self.army = k;
                self.direct(k, self.mult * 0.6);
            }
        } else {
            self.army = self.fac;
            self.direct(self.fac, self.mult);
        }
    }
    fn direct(&mut self, army: usize, m: f32) {
        match army {
            0 => {
                self.out_t -= DT;
                if self.out_t <= 0.0 {
                    self.outbreaks += 1;
                    let n = (40.0 * 1.5f32.powi(self.outbreaks as i32) * m).round();
                    for _ in 0..2 {
                        if let Some(p) = self.random_cell(1500) {
                            self.queue(p, n / 2.0, 3.0);
                        }
                    }
                    self.show(format!("New outbreak: {} infected", n as u64), 4.0);
                    self.out_t = 70.0;
                }
                if !self.rush_forever {
                    if self.rush {
                        self.rush_left -= DT;
                        if self.rush_left <= 0.0 {
                            self.rush = false;
                            self.rush_t = 150.0;
                        }
                    } else {
                        self.rush_t -= DT;
                        if self.rush_t <= 0.0 {
                            self.rush = true;
                            self.rush_left = 40.0;
                            self.rushes += 1;
                            if self.rushes >= 4 {
                                self.rush_forever = true;
                                self.show("The dead swarm your HQ for good", 4.0);
                            } else {
                                self.show("Horde rush: every zombie is coming for the HQ", 4.0);
                            }
                        }
                    }
                } else {
                    self.rush = true;
                }
            }
            1 => {
                self.drop_t -= DT;
                if self.drop_t <= 0.0 {
                    let n = ((10.0 + self.time * 0.2) * m).round();
                    if let Some(p) = self.random_cell(1800) {
                        // some drops are light flying bugs that cross rooftops and walls
                        if self.time > 40.0 && self.rng.f() < 0.22 {
                            self.queue_ex(p, (n * 0.6).max(3.0), 1.0, F_FLY, 0.0);
                        } else {
                            self.queue(p, n, 1.0);
                        }
                        self.booms.push(Boom {
                            p: vec2(p.0, p.1),
                            r: 10.0,
                            t: 0.8,
                            life: 0.8,
                            kind: 2,
                        });
                    }
                    self.drop_t = 3.0;
                }
                self.hive_t -= DT;
                if self.hive_t <= 0.0 {
                    self.hive_t = 110.0;
                    if self.hives.iter().filter(|h| !h.dead).count() < 6 {
                        if let Some(p) = self.random_cell(2500) {
                            let p = vec2(p.0, p.1);
                            self.hives.push(Hive {
                                p,
                                r: 30.0,
                                hp: 2500.0,
                                max: 2500.0,
                                spawn_t: 4.0,
                                grow_t: 14.0,
                                infected: Vec::new(),
                                dead: false,
                            });
                            self.booms.push(Boom {
                                p,
                                r: 30.0,
                                t: 1.2,
                                life: 1.2,
                                kind: 2,
                            });
                            self.show(
                                "A hive has taken root. Only abilities and troops can kill it",
                                4.0,
                            );
                        }
                    }
                }
                self.hives_tick();
            }
            _ => {
                self.portal_t -= DT;
                if self.portal_t <= 0.0 {
                    let k = self.portals_opened;
                    self.portals_opened += 1;
                    let np = 1 + (k / 2).min(2);
                    let size = (1500.0 * 1.45f32.powi(k as i32) * m).round();
                    for _ in 0..np {
                        if let Some(p) = self.random_cell(2500) {
                            self.portals.push(Portal {
                                p: vec2(p.0, p.1),
                                t: 60.0,
                                taken: 0,
                            });
                            self.queue(p, size / np as f32, 15.0);
                        }
                    }
                    self.show(
                        format!(
                            "{} portal{} open: {} orcs",
                            np,
                            if np > 1 { "s" } else { "" },
                            size as u64
                        ),
                        4.0,
                    );
                    self.portal_t = 70.0;
                }
                for p in &mut self.portals {
                    p.t -= DT;
                }
                self.portals.retain(|p| p.t > 0.0);
            }
        }
    }

    // ----------------------------------------------------------------------------- tick

    /// A fingerprint of the simulation, compared between players to catch a game drifting apart.
    pub fn checksum(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |v: u32| {
            h ^= v as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        };
        mix(self.swarm.len() as u32);
        for u in self.swarm.units() {
            mix(u.x.to_bits() ^ u.y.to_bits().rotate_left(16));
            mix(u.hp.to_bits());
        }
        for c in &self.civs {
            mix(c.p.x.to_bits() ^ c.p.y.to_bits().rotate_left(16) ^ c.st as u32);
        }
        for s in &self.structs {
            mix(s.hp.to_bits() ^ s.dead as u32);
        }
        for g in &self.groups {
            for m in &g.m {
                mix(m.p.x.to_bits() ^ m.p.y.to_bits().rotate_left(16) ^ m.alive as u32);
            }
        }
        for b in &self.world.buildings {
            mix(b.hp.to_bits());
        }
        for n in &self.napalm {
            mix(n.p.x.to_bits() ^ n.p.y.to_bits().rotate_left(16) ^ n.t.to_bits());
        }
        for sh in &self.shells {
            mix(sh.to.x.to_bits() ^ sh.to.y.to_bits().rotate_left(16));
        }
        for g in &self.gunships {
            mix(g.p.x.to_bits() ^ g.p.y.to_bits().rotate_left(16));
        }
        mix(self.scrap.to_bits());
        mix(self.ammo.to_bits());
        mix(self.score.to_bits());
        mix(self.ticks as u32);
        h
    }

    pub fn tick(&mut self) {
        if self.over {
            return;
        }
        let t0 = std::time::Instant::now();
        let mut last = t0;
        let mut lap = |slot: &mut f64| {
            let now = std::time::Instant::now();
            *slot += (now - last).as_secs_f64() * 1000.0;
            last = now;
        };
        self.pump_fields();
        lap(&mut self.prof[0]);
        let stats = self.faction().stats;
        let chaos = self.fac == CHAOS;
        let army_hp = |fl: u8| if chaos { FACTIONS[kind_of(fl).min(2)].stats.hp } else { stats.hp };
        // spawns
        let mut q = std::mem::take(&mut self.spawnq);
        for s in &mut q {
            let k = s.left.min((s.rate * DT).ceil() as usize);
            s.left -= k;
            let seed = self.rng.next_u64();
            let hp = if s.fl & F_FLY != 0 {
                army_hp(s.fl) * 0.6
            } else {
                army_hp(s.fl)
            };
            self.swarm
                .spawn(s.at, k, &self.obj, &self.hq_dist, hp, seed, s.fl, s.aux);
        }
        q.retain(|s| s.left > 0);
        self.spawnq.extend(q);
        lap(&mut self.prof[1]);
        for p in std::mem::take(&mut self.conv) {
            let seed = self.rng.next_u64();
            self.swarm.spawn(
                (p.x, p.y),
                1,
                &self.obj,
                &self.hq_dist,
                army_hp(0),
                seed,
                kind_flag(0),
                0.0,
            );
        }
        // the parallel step
        for a in &self.sd {
            a.store(0, Relaxed);
        }
        if self.bd.len() != self.world.buildings.len() {
            self.bd = (0..self.world.buildings.len())
                .map(|_| AtomicU32::new(0))
                .collect();
        }
        for a in &self.bd {
            a.store(0, Relaxed);
        }
        let obj = self.obj.clone();
        let nb = self.nb.clone();
        let hqd = self.hq_dist.clone();
        let hunt = self.hunt.clone();
        let revs: Vec<RevIn> = self
            .revs
            .iter()
            .map(|r| RevIn {
                alive: r.alive,
                pos: r.pos,
                field: r.field.as_deref().map(|v| v.as_slice()),
            })
            .collect();
        let (killed, deaths) = self.swarm.step(&StepIn {
            dt: DT,
            obj: &obj,
            nb: &nb,
            hq: &hqd,
            hq_pos: self.world.hq,
            hunt: hunt.as_deref().map(|v| v.as_slice()),
            rush: self.rush,
            stats,
            mixed: chaos.then(|| [FACTIONS[0].stats, FACTIONS[1].stats, FACTIONS[2].stats]),
            sd: &self.sd,
            bd: &self.bd,
            low: &self.low,
            lock: &self.lock,
            rev: &revs,
            slow: &self.slow,
        });
        drop(revs);
        lap(&mut self.prof[2]);
        self.update_revs(&deaths);
        lap(&mut self.prof[3]);
        self.kills += killed as u64;
        self.score += killed as f32;
        self.gain_scrap(killed as f32 * self.faction().reward);
        for i in 0..self.sd.len() {
            let v = self.sd[i].load(Relaxed);
            if v > 0 && !self.structs[i].dead {
                self.structs[i].hp -= v as f32 / DMG_SCALE;
                if self.structs[i].hp <= 0.0 {
                    self.kill_struct(i);
                }
            }
        }
        for i in 0..self.bd.len() {
            let v = self.bd[i].load(Relaxed);
            if v > 0 && self.world.buildings[i].alive {
                self.world.buildings[i].hp -= v as f32 / DMG_SCALE;
                if self.world.buildings[i].hp <= 0.0 {
                    self.destroy_building(i);
                }
            }
        }
        self.civilians();
        lap(&mut self.prof[4]);
        self.squads_tick();
        lap(&mut self.prof[5]);
        self.groups_tick();
        lap(&mut self.prof[6]);
        self.economy();
        lap(&mut self.prof[7]);
        self.turrets();
        self.towers();
        lap(&mut self.prof[8]);
        self.abilities();
        lap(&mut self.prof[9]);
        self.director();
        lap(&mut self.prof[10]);
        for v in self.world.irradiated.iter_mut() {
            if *v > 0.0 {
                *v = (*v - DT).max(0.0);
                if *v == 0.0 {
                    self.zone_dirty = true;
                }
            }
        }
        for t in &mut self.tracers {
            t.t -= DT;
        }
        self.tracers.retain(|t| t.t > 0.0);
        for b in &mut self.booms {
            b.t -= DT;
        }
        self.booms.retain(|b| b.t > 0.0);
        // nobody draining (headless runs): keep only the latest cues
        if self.sfx.len() > 256 {
            self.sfx.drain(..self.sfx.len() - 64);
        }
        self.banner.1 -= DT;
        lap(&mut self.prof[11]);
        self.time += DT;
        if self.goal > 0.0 && self.time >= self.goal && !self.over {
            self.won = true;
            self.over = true;
        }
        self.prof_n += 1;
        if self.prof_n % 200 == 0 && std::env::var("PH_PROF").is_ok() {
            let names = ["fields", "spawn", "swarm", "revs", "damage", "civs", "squads", "groups", "econ", "turrets", "abil", "misc"];
            let line: Vec<String> = names.iter().zip(self.prof.iter()).map(|(n, v)| format!("{} {:.2}", n, v / 200.0)).collect();
            eprintln!("ms/tick: {}", line.join(" | "));
            self.prof = [0.0; 12];
        }
        self.sim_ms = self.sim_ms * 0.9 + t0.elapsed().as_secs_f32() * 1000.0 * 0.1;
    }

    /// Income that stops at the storage limit (anything already above it, like a campaign
    /// carry-over, is kept).
    pub fn gain_scrap(&mut self, x: f32) {
        if self.scrap < self.scrap_cap {
            self.scrap = (self.scrap + x).min(self.scrap_cap);
        }
    }
    pub fn gain_ammo(&mut self, x: f32) {
        if self.ammo < self.ammo_cap {
            self.ammo = (self.ammo + x).min(self.ammo_cap);
        }
    }

    fn economy(&mut self) {
        // a crowded base slows everything down
        self.income_mult = if self.waiting > 40 {
            (40.0 / self.waiting as f32).max(0.3)
        } else {
            1.0
        };
        let im = self.income_mult;
        let depots = |t: S| self.structs.iter().filter(|s| !s.dead && !s.pending && s.t == t).count() as f32;
        self.scrap_cap = SCRAP_CAP + depots(S::ScrapDepot) * DEPOT_SCRAP;
        self.ammo_cap = AMMO_CAP + depots(S::AmmoDepot) * DEPOT_AMMO;
        for i in 0..self.structs.len() {
            let s = &self.structs[i];
            if s.dead || s.pending {
                continue;
            }
            match s.t {
                S::Extractor => self.gain_scrap(2.5 * DT * im),
                S::Plant => {
                    // a full ammo store idles the plants instead of burning scrap
                    if self.scrap >= 3.0 * DT && self.ammo < self.ammo_cap {
                        self.scrap -= 3.0 * DT;
                        let rate = if self.research.presses { 10.0 } else { 6.0 };
                        self.gain_ammo(rate * DT * im);
                    }
                }
                _ => {}
            }
        }
        if let Some((r, t)) = self.research.busy {
            let t = t - DT;
            if t <= 0.0 {
                self.research.busy = None;
                self.complete_research(r);
                self.show(format!("Research complete: {}", rdef(r).name), 3.0);
            } else {
                self.research.busy = Some((r, t));
            }
        }
    }

    pub fn find_target(&self, c: Vec2, r: f32) -> Option<Vec2> {
        let gx0 = ((c.x - r) / CG as f32).floor().max(0.0) as usize;
        let gx1 = (((c.x + r) / CG as f32).floor() as usize).min(CW - 1);
        let gy0 = ((c.y - r) / CG as f32).floor().max(0.0) as usize;
        let gy1 = (((c.y + r) / CG as f32).floor() as usize).min(CH - 1);
        let mut best = usize::MAX;
        let mut bd = r * r;
        for gy in gy0..=gy1 {
            for gx in gx0..=gx1 {
                if self.swarm.coarse_at(gy * CW + gx) == 0 {
                    continue;
                }
                let p = vec2((gx * CG) as f32 + 4.0, (gy * CG) as f32 + 4.0) - c;
                let d = p.length_squared();
                if d < bd {
                    bd = d;
                    best = gy * CW + gx;
                }
            }
        }
        if best == usize::MAX {
            return None;
        }
        let (bx, by) = ((best % CW) * CG, (best / CW) * CG);
        let (mut mx, mut mv) = (usize::MAX, 0);
        for y in by..by + CG {
            for x in bx..bx + CG {
                let v = self.swarm.at(y * W + x);
                if v > mv {
                    mv = v;
                    mx = y * W + x;
                }
            }
        }
        if mx == usize::MAX {
            None
        } else {
            Some(vec2((mx % W) as f32 + 0.5, (mx / W) as f32 + 0.5))
        }
    }

    /// Damage to the horde (and city buildings) in a radius. `src` is the attacker zombies will swarm;
    /// `hive` is damage dealt to alien hives, which only abilities and troops can hurt.
    pub fn blast(&mut self, at: Vec2, r: f32, dmg: f32, bdmg: f32, src: u16, hive: f32) {
        self.swarm.add_damage(at.x, at.y, r, dmg, src);
        self.damage_buildings(at, r, bdmg);
        if hive > 0.0 {
            self.damage_hives(at, r, hive);
        }
    }

    pub fn damage_hives(&mut self, at: Vec2, r: f32, d: f32) {
        for h in &mut self.hives {
            if !h.dead && h.p.distance(at) < r + 6.0 + h.r * 0.08 {
                h.hp -= d;
            }
        }
    }

    // ----------------------------------------------------------------------------- zombie revenge

    /// The revenge id for an attacker (0 when the faction doesn't care).
    pub fn rev_src(&mut self, key: RevKey) -> u16 {
        if !self.has_army(0) {
            return 0;
        }
        if let Some(i) = self.revs.iter().position(|r| r.key == key && r.alive) {
            return i as u16 + 1;
        }
        if self.revs.len() >= 60000 {
            return 0;
        }
        self.revs.push(Rev {
            key,
            pos: Vec2::ZERO,
            alive: true,
            field: None,
            job: 0,
            refresh: 0.0,
            hot: 0.0,
        });
        self.revs.len() as u16
    }

    fn rev_cells(&self, key: RevKey) -> Option<(Vec2, Vec<u32>)> {
        let mut v = Vec::new();
        match key {
            RevKey::Struct(i) => {
                let s = &self.structs[i];
                if s.dead {
                    return None;
                }
                if s.roof >= 0 {
                    let b = &self.world.buildings[s.roof as usize];
                    for y in b.y0.max(0.0) as usize..=(b.y1 as usize).min(H - 1) {
                        for x in b.x0.max(0.0) as usize..=(b.x1 as usize).min(W - 1) {
                            if self.world.bid[y * W + x] == s.roof {
                                v.push((y * W + x) as u32);
                            }
                        }
                    }
                } else {
                    for y in s.y..s.y + s.h {
                        for x in s.x..s.x + s.w {
                            v.push((y as usize * W + x as usize) as u32);
                        }
                    }
                }
                Some((s.center(), v))
            }
            RevKey::Group(gi) => {
                let g = &self.groups[gi];
                if g.dead {
                    return None;
                }
                if g.state == GState::Garrison {
                    let b = g.b as usize;
                    return Some((
                        self.world.buildings[b].c,
                        vec![
                            (self.world.buildings[b].c.y as usize * W
                                + self.world.buildings[b].c.x as usize)
                                as u32,
                        ],
                    ));
                }
                let mut c = Vec2::ZERO;
                for m in g.m.iter().filter(|m| m.alive) {
                    v.push((m.p.y as usize * W + m.p.x as usize) as u32);
                    c += m.p;
                }
                if v.is_empty() {
                    return None;
                }
                Some((c / v.len() as f32, v))
            }
            RevKey::Squad(si) => {
                let q = &self.squads[si];
                let mut c = Vec2::ZERO;
                for m in q.m.iter().filter(|m| m.alive && !m.home) {
                    v.push((m.p.y as usize * W + m.p.x as usize) as u32);
                    c += m.p;
                }
                if v.is_empty() {
                    return None;
                }
                Some((c / v.len() as f32, v))
            }
        }
    }

    fn update_revs(&mut self, deaths: &[crate::swarm::Death]) {
        for &c in &self.lock_cells {
            self.lock[c] = 0;
        }
        self.lock_cells.clear();
        for i in 0..self.revs.len() {
            if !self.revs[i].alive {
                continue;
            }
            let key = self.revs[i].key;
            match self.rev_cells(key) {
                None => {
                    self.revs[i].alive = false;
                    self.revs[i].field = None;
                }
                Some((pos, cells)) => {
                    let r = &mut self.revs[i];
                    r.pos = pos;
                    r.hot -= DT;
                    r.refresh -= DT;
                    if r.hot > 0.0 && r.refresh <= 0.0 && self.fw_aux.busy < 4 {
                        r.refresh = if matches!(key, RevKey::Struct(_)) {
                            8.0
                        } else {
                            1.5
                        };
                        let id = self.job_seq;
                        self.job_seq += 1;
                        r.job = id;
                        self.fw_aux.submit(Job::Multi {
                            id,
                            obj: self.obj.clone(),
                            src: cells,
                            max: 3500,
                        });
                    }
                }
            }
        }
        // zombies near a fresh kill lock on to the killer
        const R: i32 = 6; // coarse cells (about 48 map cells)
        for d in deaths {
            let k = d.src as usize;
            if k == 0 || k > self.revs.len() || !self.revs[k - 1].alive {
                continue;
            }
            self.revs[k - 1].hot = 30.0;
            let (gx, gy) = ((d.p.x as usize / CG) as i32, (d.p.y as usize / CG) as i32);
            for oy in -R..=R {
                for ox in -R..=R {
                    if ox * ox + oy * oy > R * R {
                        continue;
                    }
                    let (x, y) = (gx + ox, gy + oy);
                    if x < 0 || y < 0 || x >= CW as i32 || y >= CH as i32 {
                        continue;
                    }
                    let c = y as usize * CW + x as usize;
                    if self.lock[c] == 0 {
                        self.lock[c] = d.src;
                        self.lock_cells.push(c);
                    }
                }
            }
        }
    }


    fn turrets(&mut self) {
        // cells gun turrets are already shooting at this tick, so two never waste shots on one target
        let mut claimed: Vec<usize> = Vec::new();
        for i in 0..self.structs.len() {
            let (t, center, roof) = {
                let s = &self.structs[i];
                if s.dead || s.pending || !matches!(s.t, S::Gun | S::Cannon) {
                    continue;
                }
                (s.t, s.center(), s.roof)
            };
            let d = def(t);
            self.structs[i].cd -= DT;
            if self.structs[i].cd > 0.0 || self.ammo < d.ammo {
                continue;
            }
            let range = self.turret_range(t, roof >= 0);
            let tg = if t == S::Gun {
                let cell = self.target_cell(center, range, &claimed, |_| false);
                if let Some(k) = cell {
                    claimed.push(k);
                }
                cell.map(Self::cell_pos)
            } else {
                self.find_target(center, range)
            };
            if let Some(tg) = tg {
                let src = self.rev_src(RevKey::Struct(i));
                self.blast(tg, d.rad * self.rad_mult(t), d.dmg * self.dmg_mult(t), d.bdmg, src, 0.0);
                self.ammo -= d.ammo;
                self.structs[i].cd = self.reload(t);
                self.structs[i].last_fire = self.time;
                self.structs[i].aim = libm::atan2f(tg.y - center.y, tg.x - center.x);
                let base = if roof >= 0 { self.world.buildings[roof as usize].h } else { 0.0 };
                self.tracers.push(Tracer {
                    a: center,
                    az: base + d.ht,
                    b: tg,
                    t: 0.09,
                    kind: if t == S::Cannon { 1 } else { 0 },
                });
                if t == S::Cannon {
                    self.sfx.push((Sfx::Cannon, center));
                    self.booms.push(Boom {
                        p: tg,
                        r: d.rad,
                        t: 0.25,
                        life: 0.25,
                        kind: 0,
                    });
                }
            }
        }
    }

    // ----------------------------------------------------------------------------- special towers

    /// The densest patch of the horde within `r` of `c` (coarse cell centre) and how many are there.
    pub fn find_cluster(&self, c: Vec2, r: f32) -> Option<(Vec2, u32)> {
        let gx0 = ((c.x - r) / CG as f32).floor().max(0.0) as usize;
        let gx1 = (((c.x + r) / CG as f32).floor().max(0.0) as usize).min(CW - 1);
        let gy0 = ((c.y - r) / CG as f32).floor().max(0.0) as usize;
        let gy1 = (((c.y + r) / CG as f32).floor().max(0.0) as usize).min(CH - 1);
        let mut best = None;
        let mut bn = 0;
        for gy in gy0..=gy1 {
            for gx in gx0..=gx1 {
                let n = self.swarm.coarse_at(gy * CW + gx);
                if n <= bn {
                    continue;
                }
                let p = vec2((gx * CG) as f32 + 4.0, (gy * CG) as f32 + 4.0);
                if p.distance(c) <= r {
                    bn = n;
                    best = Some(p);
                }
            }
        }
        best.map(|p| (p, bn))
    }

    /// Coarse cells with hostiles within `r` of `c`, nearest first.
    fn coarse_near(&self, c: Vec2, r: f32) -> Vec<(f32, usize)> {
        let gx0 = ((c.x - r) / CG as f32).floor().max(0.0) as usize;
        let gx1 = (((c.x + r) / CG as f32).floor().max(0.0) as usize).min(CW - 1);
        let gy0 = ((c.y - r) / CG as f32).floor().max(0.0) as usize;
        let gy1 = (((c.y + r) / CG as f32).floor().max(0.0) as usize).min(CH - 1);
        let mut out = Vec::new();
        for gy in gy0..=gy1 {
            for gx in gx0..=gx1 {
                if self.swarm.coarse_at(gy * CW + gx) == 0 {
                    continue;
                }
                let d = vec2((gx * CG) as f32 + 4.0, (gy * CG) as f32 + 4.0).distance(c);
                if d <= r + 4.0 {
                    out.push((d, gy * CW + gx));
                }
            }
        }
        out.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        out
    }

    /// The nearest occupied cell within `r`, skipping cells next to ones in `claimed` (so gun
    /// turrets spread their fire) and ones `skip` rules out.
    fn target_cell(&self, c: Vec2, r: f32, claimed: &[usize], skip: impl Fn(Vec2) -> bool) -> Option<usize> {
        for (_, gi) in self.coarse_near(c, r) {
            let (bx, by) = ((gi % CW) * CG, (gi / CW) * CG);
            let (mut best, mut bv) = (usize::MAX, 0);
            for y in by..by + CG {
                for x in bx..bx + CG {
                    let v = self.swarm.at(y * W + x);
                    if v <= bv {
                        continue;
                    }
                    let p = vec2(x as f32 + 0.5, y as f32 + 0.5);
                    if p.distance(c) > r || skip(p) {
                        continue;
                    }
                    if claimed.iter().any(|&k| ((k % W) as i32 - x as i32).abs() <= 1 && ((k / W) as i32 - y as i32).abs() <= 1) {
                        continue;
                    }
                    bv = v;
                    best = y * W + x;
                }
            }
            if best != usize::MAX {
                return Some(best);
            }
        }
        None
    }

    /// Where a flame bunker sprays next: the nearest hostiles not already standing in napalm.
    fn flame_target(&self, c: Vec2, r: f32) -> Option<Vec2> {
        let burning = |p: Vec2| self.napalm.iter().any(|n| n.p.distance(p) < n.r + 1.5);
        self.target_cell(c, r, &[], burning).map(Self::cell_pos)
    }

    /// The railgun's aim: of the directions through the biggest groups in range, the one whose
    /// whole beam crosses the most hostiles once they have moved on for the length of the charge.
    fn rail_target(&self, c: Vec2, range: f32) -> Option<Vec2> {
        let st = self.faction().stats;
        let hq = self.world.hq;
        // most hordes walk for the HQ; lead them by how far they get while the slug charges
        let lead = st.spd * RAIL_CHARGE * 0.8;
        let ahead = |p: Vec2| p + (hq - p).normalize_or_zero() * lead.min(p.distance(hq));
        let mut cand: Vec<(u32, usize)> = self
            .coarse_near(c, range)
            .into_iter()
            .map(|(_, gi)| (self.swarm.coarse_at(gi), gi))
            .filter(|(n, _)| *n >= 2)
            .collect();
        cand.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        cand.truncate(40);
        let rad = def(S::Rail).rad * self.rad_mult(S::Rail);
        let mut best = None;
        let mut bs = 5;
        let mut seen: Vec<usize> = Vec::new();
        for (_, gi) in cand {
            let now = vec2(((gi % CW) * CG) as f32 + 4.0, ((gi / CW) * CG) as f32 + 4.0);
            let aim = ahead(now);
            let dir = (aim - c).normalize_or_zero();
            if dir == Vec2::ZERO {
                continue;
            }
            let side = vec2(-dir.y, dir.x) * (rad + 1.0);
            seen.clear();
            let mut score = 0;
            let mut k = 4.0;
            while k <= range {
                let p = c + dir * k;
                for q in [p, p + side, p - side] {
                    // who will be here when the slug flies: those a charge's walk behind it
                    let from = q - (hq - q).normalize_or_zero() * lead.min(q.distance(hq));
                    if from.x < 0.0 || from.y < 0.0 || from.x >= W as f32 || from.y >= H as f32 {
                        continue;
                    }
                    let g = (from.y as usize / CG) * CW + from.x as usize / CG;
                    if !seen.contains(&g) {
                        seen.push(g);
                        score += self.swarm.coarse_at(g);
                    }
                }
                k += 3.0;
            }
            if score > bs {
                bs = score;
                best = Some(aim);
            }
        }
        best
    }

    /// Points an artillery piece at a spot, or sends a heli pad's gunship there.
    pub fn aim(&mut self, i: usize, p: Vec2) -> bool {
        if i >= self.structs.len() || self.structs[i].dead || self.structs[i].pending {
            return false;
        }
        let p = p.clamp(vec2(1.0, 1.0), vec2(W as f32 - 2.0, H as f32 - 2.0));
        match self.structs[i].t {
            S::Artillery => {
                self.structs[i].target = Some(p);
                true
            }
            S::Helipad => {
                if let Some(g) = self.gunships.iter_mut().find(|g| g.pad == Some(i) && g.phase < 2) {
                    g.center = p;
                    g.phase = 0;
                    return true;
                }
                if self.gunships.iter().any(|g| g.pad == Some(i)) || self.structs[i].cd > 0.0 {
                    self.show("The gunship is rearming", 2.0);
                    return true;
                }
                let home = self.structs[i].center();
                self.gunships.push(Gunship {
                    pad: Some(i),
                    home,
                    center: p,
                    p: home,
                    t: HELIPAD_FLIGHT,
                    ang: 0.0,
                    cd: 0.0,
                    phase: 0,
                });
                true
            }
            _ => false,
        }
    }
    pub fn cease(&mut self, i: usize) {
        if let Some(s) = self.structs.get_mut(i) {
            if s.t == S::Artillery {
                s.target = None;
            }
        }
    }
    /// The gunship a heli pad has out, if any.
    pub fn pad_gunship(&self, i: usize) -> Option<&Gunship> {
        self.gunships.iter().find(|g| g.pad == Some(i))
    }

    /// Flame bunkers, railguns, helium towers, artillery, heli pads and rescue stations.
    fn towers(&mut self) {
        self.slow.iter_mut().for_each(|v| *v = 0);
        for i in 0..self.structs.len() {
            let (t, c, roof) = {
                let s = &self.structs[i];
                if s.dead || s.pending {
                    continue;
                }
                (s.t, s.center(), s.roof)
            };
            let d = def(t);
            match t {
                S::Flame => {
                    let range = self.turret_range(t, false);
                    if self.structs[i].charge > 0.0 {
                        // spraying: a cone of fire from the walls out to the target, leaving napalm
                        self.structs[i].charge -= DT;
                        let Some(tg) = self.structs[i].target else { continue };
                        let dir = (tg - c).normalize_or_zero();
                        let reach = (tg.distance(c) + 3.0).clamp(4.0, range);
                        let start = 1.5;
                        let src = self.rev_src(RevKey::Struct(i));
                        let lay = (self.ticks + i as u64) % 5 == 0;
                        for k in 0..5 {
                            let f = k as f32 / 4.0;
                            let p = c + dir * (start + (reach - start) * f);
                            let r = d.rad * (0.55 + f * 0.75);
                            self.swarm.add_damage(p.x, p.y, r, d.dmg * self.dmg_mult(t), src);
                            self.damage_hives(p, r, 1.0);
                            if (self.ticks + k) % 2 == 0 {
                                self.booms.push(Boom { p, r, t: 0.3, life: 0.3, kind: 3 });
                            }
                            if lay && k > 0 {
                                self.napalm.push(Napalm { p, r: r * 0.8, t: NAPALM_LIFE });
                            }
                        }
                        let cd = self.reload(t);
                        let s = &mut self.structs[i];
                        s.last_fire = self.time;
                        if s.charge <= 0.0 {
                            s.charge = 0.0;
                            s.target = None;
                            s.cd = cd;
                            self.sfx.push((Sfx::FlameStop, c));
                        }
                        continue;
                    }
                    self.structs[i].cd = (self.structs[i].cd - DT).max(0.0);
                    if self.structs[i].cd > 0.0 || self.ammo < d.ammo {
                        continue;
                    }
                    let Some(tg) = self.flame_target(c, range) else { continue };
                    self.ammo -= d.ammo;
                    let s = &mut self.structs[i];
                    s.charge = FLAME_SPRAY;
                    s.target = Some(tg);
                    s.aim = libm::atan2f(tg.y - c.y, tg.x - c.x);
                    s.snd = self.time;
                    self.sfx.push((Sfx::Flame, c));
                }
                S::Rail => {
                    let range = self.turret_range(t, roof >= 0);
                    if self.structs[i].charge > 0.0 {
                        self.structs[i].charge -= DT;
                        if self.structs[i].charge > 0.0 {
                            continue;
                        }
                        self.structs[i].charge = 0.0;
                        let Some(tg) = self.structs[i].target.take() else { continue };
                        let dir = (tg - c).normalize_or_zero();
                        let end = c + dir * range;
                        // too far and too fast for zombies to tell where it came from: no revenge
                        let src = 0;
                        let steps = (range / 1.2) as usize;
                        for k in 3..=steps {
                            let p = c + dir * (k as f32 * 1.2);
                            if p.x < 1.0 || p.y < 1.0 || p.x >= W as f32 - 1.0 || p.y >= H as f32 - 1.0 {
                                break;
                            }
                            self.swarm.add_damage(p.x, p.y, d.rad * self.rad_mult(t), d.dmg, src);
                        }
                        for k in 1..=8 {
                            self.damage_hives(c + dir * range * k as f32 / 8.0, d.rad, 40.0);
                        }
                        let base = if roof >= 0 { self.world.buildings[roof as usize].h } else { 0.0 };
                        self.tracers.push(Tracer { a: c, az: base + d.ht + 1.5, b: end, t: 0.4, kind: 6 });
                        self.booms.push(Boom { p: c + dir * 4.0, r: 3.0, t: 0.25, life: 0.25, kind: 4 });
                        self.structs[i].last_fire = self.time;
                        self.sfx.push((Sfx::Railgun, c));
                        continue;
                    }
                    self.structs[i].cd = (self.structs[i].cd - DT).max(0.0);
                    if self.structs[i].cd > 0.0 || self.ammo < d.ammo {
                        continue;
                    }
                    // wait for a line worth the slug
                    if let Some(tg) = self.rail_target(c, range) {
                        self.ammo -= d.ammo;
                        let cd = self.reload(t);
                        let s = &mut self.structs[i];
                        s.target = Some(tg);
                        s.charge = RAIL_CHARGE;
                        s.cd = cd;
                        s.aim = libm::atan2f(tg.y - c.y, tg.x - c.x);
                    }
                }
                S::Helium => {
                    let r = self.turret_range(t, roof >= 0);
                    let rr = r / CG as f32;
                    let (cx, cy) = (c.x / CG as f32, c.y / CG as f32);
                    let (x0, x1) = ((cx - rr).floor().max(0.0) as usize, ((cx + rr).ceil() as usize).min(CW - 1));
                    let (y0, y1) = ((cy - rr).floor().max(0.0) as usize, ((cy + rr).ceil() as usize).min(CH - 1));
                    for gy in y0..=y1 {
                        for gx in x0..=x1 {
                            if vec2(gx as f32 + 0.5 - cx, gy as f32 + 0.5 - cy).length() <= rr {
                                self.slow[gy * CW + gx] = 1;
                            }
                        }
                    }
                }
                S::Artillery => {
                    self.structs[i].cd = (self.structs[i].cd - DT).max(0.0);
                    let Some(to) = self.structs[i].target else { continue };
                    if self.structs[i].cd > 0.0 || self.ammo < d.ammo {
                        continue;
                    }
                    let dist = to.distance(c);
                    let spread = 3.0 + dist * 0.012;
                    let a = self.rng.f() * std::f32::consts::TAU;
                    let r = self.rng.f().sqrt() * spread;
                    let hit = (to + vec2(libm::cosf(a), libm::sinf(a)) * r)
                        .clamp(vec2(1.0, 1.0), vec2(W as f32 - 2.0, H as f32 - 2.0));
                    self.shells.push(Shell { from: c, to: hit, t: 0.0, dur: 1.5 + dist / 260.0 });
                    self.ammo -= d.ammo;
                    let cd = self.reload(t);
                    let s = &mut self.structs[i];
                    s.cd = cd;
                    s.last_fire = self.time;
                    s.aim = libm::atan2f(to.y - c.y, to.x - c.x);
                    self.booms.push(Boom { p: c + vec2(libm::cosf(s.aim), libm::sinf(s.aim)) * 6.0, r: 3.0, t: 0.3, life: 0.3, kind: 0 });
                    self.sfx.push((Sfx::ArtilleryShot, c));
                }
                S::Helipad => {
                    if self.pad_gunship(i).is_none() {
                        self.structs[i].cd = (self.structs[i].cd - DT).max(0.0);
                    }
                }
                S::Rescue => {
                    self.structs[i].cd = (self.structs[i].cd - DT).max(0.0);
                    if self.structs[i].cd > 0.0 {
                        continue;
                    }
                    if self.squads.iter().any(|q| !q.done && q.from == i as i32) {
                        self.structs[i].cd = 2.0;
                        continue;
                    }
                    self.structs[i].cd = match self.rescue_spot(c) {
                        Some(at) => {
                            let s = &self.structs[i];
                            let base = vec2(c.x, (s.y + s.h + 1) as f32);
                            let n = if self.research.rescue { 12 } else { 8 };
                            if self.send_squad_from(base, at, n, i as i32) {
                                RESCUE_EVERY
                            } else {
                                5.0
                            }
                        }
                        None => 5.0,
                    };
                }
                _ => {}
            }
        }
        // burning napalm
        let burn = NAPALM_DPS * DT * self.dmg_mult(S::Flame);
        for n in &mut self.napalm {
            n.t -= DT;
            self.swarm.add_damage(n.p.x, n.p.y, n.r, burn, 0);
        }
        self.napalm.retain(|n| n.t > 0.0);
        // shells in the air
        let mut landed = Vec::new();
        for sh in &mut self.shells {
            sh.t += DT;
            if sh.t >= sh.dur {
                landed.push(sh.to);
            }
        }
        self.shells.retain(|s| s.t < s.dur);
        let d = def(S::Artillery);
        let (rad, dmg) = (d.rad * self.rad_mult(S::Artillery), d.dmg * self.dmg_mult(S::Artillery));
        for p in landed {
            self.blast(p, rad, dmg, d.bdmg, 0, 60.0);
            self.booms.push(Boom { p, r: rad, t: 0.7, life: 0.7, kind: 0 });
            self.sfx.push((Sfx::ArtilleryHit, p));
        }
    }

    /// Where a rescue station should send its next team: the biggest group of wandering civilians
    /// in reach that the horde isn't standing on and no other team is already heading for.
    fn rescue_spot(&self, from: Vec2) -> Option<Vec2> {
        const B: usize = 32;
        let bw = W / B;
        let mut n = vec![0u32; bw * (H / B)];
        let mut sum = vec![Vec2::ZERO; bw * (H / B)];
        for cv in &self.civs {
            if cv.st != C_ROAM || cv.p.distance(from) > RESCUE_REACH {
                continue;
            }
            let k = (cv.p.y as usize / B) * bw + cv.p.x as usize / B;
            n[k] += 1;
            sum[k] += cv.p;
        }
        let mut best = None;
        let mut bn = 2;
        for k in 0..n.len() {
            if n[k] <= bn {
                continue;
            }
            let at = sum[k] / n[k] as f32;
            if self.world.irradiated_at(at)
                || self.hive_blocks(at)
                || self.squads.iter().any(|q| !q.done && q.target.distance(at) < 50.0)
            {
                continue;
            }
            let (gx, gy) = ((k % bw) * (B / CG), (k / bw) * (B / CG));
            let mut danger = 0;
            for y in gy.saturating_sub(1)..(gy + B / CG + 1).min(CH) {
                for x in gx.saturating_sub(1)..(gx + B / CG + 1).min(CW) {
                    danger += self.swarm.coarse_at(y * CW + x);
                }
            }
            if danger > 3 {
                continue;
            }
            bn = n[k];
            best = Some(at);
        }
        best
    }

    // ----------------------------------------------------------------------------- abilities

    pub fn ability_locked(&self, a: A) -> bool {
        (a == A::Icbm && !self.has(S::Silo)) || !self.ability_ok(a)
    }
    pub fn can_use(&self, a: A) -> bool {
        let d = adef(a);
        !self.ability_locked(a)
            && self.cooldown(a) <= 0.0
            && self.scrap >= d.cost
            && self.ammo >= d.ammo
    }
    /// Each silo has its own ICBM cooldown; the ability is ready when any silo is.
    pub fn cooldown(&self, a: A) -> f32 {
        if a == A::Icbm {
            return self
                .structs
                .iter()
                .filter(|s| !s.dead && !s.pending && s.t == S::Silo)
                .map(|s| s.cd)
                .fold(f32::MAX, f32::min)
                .min(1e6)
                .max(0.0)
                * if self.has(S::Silo) { 1.0 } else { 0.0 };
        }
        self.cds[ability_index(a)]
    }
    pub fn silos_ready(&self) -> (usize, usize) {
        let s: Vec<&Structure> = self
            .structs
            .iter()
            .filter(|s| !s.dead && !s.pending && s.t == S::Silo)
            .collect();
        (s.iter().filter(|s| s.cd <= 0.0).count(), s.len())
    }
    pub fn upgraded(&self, a: A) -> bool {
        match a {
            A::Rescue => self.research.rescue,
            A::Air => self.research.air,
            A::Icbm => self.research.nuke,
            A::Gunship => true,
        }
    }
    pub fn can_research(&self, r: R) -> bool {
        !self.campaign
            && !self.research.has(r)
            && self.research.busy.is_none()
            && self.has(S::Lab)
            && self.scrap >= rdef(r).cost
    }
    pub fn start_research(&mut self, r: R) {
        if self.can_research(r) {
            self.scrap -= rdef(r).cost;
            self.research.busy = Some((r, 30.0));
            self.show(format!("Researching {} (30 s)", rdef(r).name), 2.5);
        }
    }
    /// Turns a research topic on, including for what's already built.
    pub fn complete_research(&mut self, r: R) {
        self.research.set(r);
        if r == R::Armour {
            for s in &mut self.structs {
                if s.t == S::Wall || def(s.t).turret {
                    s.hp *= 1.75;
                    s.max *= 1.75;
                }
            }
        }
    }
    fn pay(&mut self, a: A) {
        let d = adef(a);
        self.scrap -= d.cost;
        self.ammo -= d.ammo;
        if a != A::Icbm {
            self.cds[ability_index(a)] = d.cd;
        }
    }

    pub fn use_ability(&mut self, a: A, from: Vec2, to: Vec2) -> bool {
        if !self.can_use(a) {
            return false;
        }
        match a {
            A::Rescue => {
                if !self.send_squad(to) {
                    return false;
                }
            }
            A::Air => {
                let upgraded = self.research.air;
                let (start, end) = if upgraded && from.distance(to) > 8.0 {
                    (from, to)
                } else {
                    let ang = self.rng.f() * std::f32::consts::TAU;
                    let d = vec2(libm::cosf(ang), libm::sinf(ang));
                    (to - d * 24.0, to + d * 24.0)
                };
                let dir = (end - start).normalize_or_zero();
                let back = W.max(H) as f32 * 0.6;
                let entry = start - dir * back;
                let n = if upgraded { 12 } else { 5 };
                let mut bombs = Vec::new();
                let seg = end - start;
                for i in 0..n {
                    let f = if n > 1 {
                        i as f32 / (n - 1) as f32
                    } else {
                        0.5
                    };
                    let jitter = if upgraded {
                        vec2(-dir.y, dir.x) * (self.rng.f() - 0.5) * 10.0
                    } else {
                        Vec2::ZERO
                    };
                    let p = start + seg * f + jitter;
                    bombs.push((p, back + seg.length() * f, false));
                }
                self.strikes.push(Strike::Air {
                    from: entry,
                    dir,
                    pos: 0.0,
                    len: back * 2.0 + seg.length(),
                    bombs,
                    line: (start, end),
                });
            }
            A::Gunship => {
                let home = self.hq_near(to);
                self.gunships.push(Gunship {
                    pad: None,
                    home,
                    center: to,
                    p: home,
                    t: 20.0,
                    ang: 0.0,
                    cd: 0.0,
                    phase: 0,
                });
            }
            A::Icbm => {
                let silo = self
                    .structs
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| !s.dead && !s.pending && s.t == S::Silo && s.cd <= 0.0)
                    .min_by(|a, b| {
                        a.1.center()
                            .distance(to)
                            .partial_cmp(&b.1.center().distance(to))
                            .unwrap()
                    })
                    .map(|(i, s)| (i, s.center()));
                let Some((si, from)) = silo else { return false };
                self.structs[si].cd = adef(A::Icbm).cd;
                self.strikes.push(Strike::Icbm {
                    from,
                    to,
                    t: 0.0,
                    dur: 5.0,
                    nuke: self.research.nuke,
                });
                self.show(
                    if self.research.nuke {
                        "Nuclear launch detected"
                    } else {
                        "ICBM launched"
                    },
                    3.0,
                );
            }
        }
        self.pay(a);
        true
    }

    fn abilities(&mut self) {
        for c in self.cds.iter_mut() {
            *c = (*c - DT).max(0.0);
        }
        for s in self.structs.iter_mut() {
            if !s.dead && (s.t == S::Silo || s.t == S::Pad) {
                s.cd = (s.cd - DT).max(0.0);
            }
        }
        let mut strikes = std::mem::take(&mut self.strikes);
        let mut blasts: Vec<(Vec2, f32, f32, f32, u8, f32)> = Vec::new();
        let mut irradiate: Vec<(Vec2, f32)> = Vec::new();
        strikes.retain_mut(|s| match s {
            Strike::Air {
                pos, len, bombs, ..
            } => {
                *pos += 170.0 * DT;
                for b in bombs.iter_mut() {
                    if !b.2 && *pos >= b.1 {
                        b.2 = true;
                        blasts.push((b.0, 12.0, 600.0, 300.0, 0, 450.0));
                        self.sfx.push((Sfx::Bomb, b.0));
                    }
                }
                *pos < *len
            }
            Strike::Icbm {
                to, t, dur, nuke, ..
            } => {
                *t += DT;
                if *t >= *dur {
                    let r = if *nuke { 190.0 } else { 120.0 };
                    blasts.push((*to, r, 1e9, 1e6, 1, 1e9));
                    self.sfx.push((Sfx::Nuke, *to));
                    if *nuke {
                        irradiate.push((*to, r));
                    }
                    false
                } else {
                    true
                }
            }
        });
        self.strikes = strikes;
        for (p, r, d, bd, k, hv) in blasts {
            self.blast(p, r, d, bd, 0, hv);
            self.booms.push(Boom {
                p,
                r,
                t: if k == 1 { 1.8 } else { 0.6 },
                life: if k == 1 { 1.8 } else { 0.6 },
                kind: k,
            });
        }
        for (p, r) in irradiate {
            let rr = r / CG as f32;
            let (cx, cy) = (p.x / CG as f32, p.y / CG as f32);
            for gy in 0..CH {
                for gx in 0..CW {
                    let d = vec2(gx as f32 + 0.5 - cx, gy as f32 + 0.5 - cy).length();
                    if d <= rr {
                        self.world.irradiated[gy * CW + gx] = 90.0;
                    }
                }
            }
            self.zone_dirty = true;
            // anyone of ours caught in it is lost
            for g in &mut self.groups {
                for m in &mut g.m {
                    if m.p.distance(p) < r {
                        m.alive = false;
                    }
                }
            }
        }
        // gunships
        let mut ships = std::mem::take(&mut self.gunships);
        for g in ships.iter_mut() {
            match g.phase {
                0 => {
                    let d = g.center + vec2(25.0, 0.0) - g.p;
                    if d.length() < 3.0 {
                        g.phase = 1;
                    } else {
                        g.p += d.normalize() * (70.0 * DT).min(d.length());
                    }
                }
                1 => {
                    g.t -= DT;
                    g.ang += DT * 0.9;
                    g.p = g.center + vec2(libm::cosf(g.ang), libm::sinf(g.ang)) * 25.0;
                    g.cd -= DT;
                    if g.cd <= 0.0 {
                        if let Some(t) = self
                            .find_target(g.p, 34.0)
                            .or_else(|| self.hive_in_range(g.p, 34.0))
                        {
                            self.swarm.add_damage(t.x, t.y, 1.6, 14.0, 0);
                            self.damage_hives(t, 1.6, 14.0);
                            self.tracers.push(Tracer {
                                a: g.p,
                                az: 16.0,
                                b: t,
                                t: 0.06,
                                kind: 2,
                            });
                        }
                        g.cd = 0.1;
                    }
                    if g.t <= 0.0 {
                        g.phase = 2;
                    }
                }
                _ => {
                    let d = g.home - g.p;
                    if d.length() < 3.0 {
                        g.phase = 3;
                    } else {
                        g.p += d.normalize() * (70.0 * DT).min(d.length());
                    }
                }
            }
        }
        for g in &ships {
            if let (3, Some(p)) = (g.phase, g.pad) {
                self.structs[p].cd = HELIPAD_COOLDOWN;
            }
        }
        ships.retain(|g| g.phase < 3);
        self.gunships = ships;
        // helicopters
        let mut done = Vec::new();
        for h in &mut self.helis {
            h.t += DT;
            if h.t >= h.dur {
                if let HeliKind::Deliver(i) = h.kind {
                    done.push(i);
                }
            }
        }
        self.helis.retain(|h| h.t < h.dur);
        for i in done {
            if !self.structs[i].dead {
                self.structs[i].pending = false;
                if self.structs[i].roof < 0 {
                    self.obj_dirty = true;
                    self.struct_dirty = true;
                }
            }
        }
        // extraction flights: each pad flies out the civilians waiting at it once everyone it called
        // over from the HQ has arrived (or every seat is taken), then needs time to turn around
        for pi in 0..self.structs.len() {
            let (ok, c) = {
                let s = &self.structs[pi];
                (
                    !s.dead && !s.pending && s.t == S::Pad && s.cd <= 0.0,
                    s.center(),
                )
            };
            if !ok {
                continue;
            }
            let (mut at, mut to) = (0u32, 0u32);
            for cv in &self.civs {
                if cv.pad == pi {
                    match cv.st {
                        C_AT_PAD => at += 1,
                        C_TO_IDLE => to += 1,
                        _ => {}
                    }
                }
            }
            if at == 0 || (at < PAD_SEATS && to > 0) {
                continue;
            }
            let mut n = 0u32;
            for cv in self.civs.iter_mut() {
                if n >= PAD_SEATS {
                    break;
                }
                if cv.st == C_AT_PAD && cv.pad == pi {
                    cv.st = C_GONE;
                    n += 1;
                }
            }
            if n == 0 {
                continue;
            }
            self.extracted += n;
            self.score += 100.0 * n as f32;
            self.gain_scrap(EVAC_SCRAP * n as f32);
            self.gain_ammo(EVAC_AMMO * n as f32);
            let ex = if c.x < W as f32 / 2.0 {
                -30.0
            } else {
                W as f32 + 30.0
            };
            self.helis.push(Heli {
                kind: HeliKind::Extract,
                from: c,
                to: vec2(ex, c.y - 80.0),
                t: 0.0,
                dur: 5.0,
            });
            self.structs[pi].cd = PAD_COOLDOWN;
            self.show(
                format!(
                    "Extraction flight: {} civilian{} out (+{} scrap, +{} ammo)",
                    n,
                    if n > 1 { "s" } else { "" },
                    (EVAC_SCRAP * n as f32) as u32,
                    (EVAC_AMMO * n as f32) as u32
                ),
                2.5,
            );
        }
    }

    // ----------------------------------------------------------------------------- people

    fn step_on_field(obj: &[i32], p: &mut Vec2, field: &[u32], speed: f32, r: &mut Rng) {
        let (cx, cy) = (p.x as i32, p.y as i32);
        let mut best = -1i32;
        let mut bv = field[cy as usize * W + cx as usize] as f32;
        for k in 0..8 {
            let (nx, ny) = (cx + DX[k], cy + DY[k]);
            if nx < 0 || ny < 0 || nx >= W as i32 || ny >= H as i32 {
                continue;
            }
            let nc = ny as usize * W + nx as usize;
            if obj[nc] != FREE || field[nc] == INF {
                continue;
            }
            if k >= 4
                && (obj[cy as usize * W + nx as usize] != FREE
                    || obj[ny as usize * W + cx as usize] != FREE)
            {
                continue;
            }
            let v = field[nc] as f32 + r.f() * 3.0;
            if v < bv {
                bv = v;
                best = k as i32;
            }
        }
        if best < 0 {
            return;
        }
        let t = vec2(
            (cx + DX[best as usize]) as f32 + 0.5,
            (cy + DY[best as usize]) as f32 + 0.5,
        );
        let d = t - *p;
        let st = speed * DT;
        if d.length() > st {
            *p += d.normalize() * st;
        } else {
            *p = t;
        }
    }

    /// Replaces the city's civilians with `n` new ones (the campaign sizes them to the city).
    pub fn set_civilians(&mut self, n: usize) {
        self.civs.clear();
        self.init_civilians(n);
    }

    fn init_civilians(&mut self, n: usize) {
        // most people are holed up in groups (worth sending a rescue squad for), the rest wander alone
        let mut left = n;
        while left > 0 {
            let Some(p) = self.random_cell(0) else { break };
            let k = if self.rng.f() < 0.3 {
                1
            } else {
                (8 + self.rng.below(30)).min(left)
            };
            for _ in 0..k {
                let mut q = vec2(p.0, p.1);
                for _ in 0..8 {
                    let t = q + vec2(self.rng.f() - 0.5, self.rng.f() - 0.5) * 14.0;
                    if t.x > 0.0
                        && t.y > 0.0
                        && t.x < W as f32
                        && t.y < H as f32
                        && self.obj[t.y as usize * W + t.x as usize] == FREE
                    {
                        q = t;
                        break;
                    }
                }
                let hd = self.rng.f() * std::f32::consts::TAU;
                self.civs.push(Civ {
                    p: q,
                    st: C_ROAM,
                    sq: 0,
                    esc: 0,
                    pad: 0,
                    hd,
                    flee_t: 0.0,
                });
            }
            left -= k;
        }
    }

    /// One step towards a nearby point over open ground (no field needed for short hops).
    fn step_toward(obj: &[i32], p: &mut Vec2, to: Vec2, speed: f32) {
        let (cx, cy) = (p.x as i32, p.y as i32);
        let d = to - *p;
        if d.length() < 0.6 {
            return;
        }
        let mut best = -1i32;
        let mut bv = d.length();
        for k in 0..8 {
            let (nx, ny) = (cx + DX[k], cy + DY[k]);
            if nx < 0 || ny < 0 || nx >= W as i32 || ny >= H as i32 {
                continue;
            }
            if obj[ny as usize * W + nx as usize] != FREE {
                continue;
            }
            if k >= 4
                && (obj[cy as usize * W + nx as usize] != FREE
                    || obj[ny as usize * W + cx as usize] != FREE)
            {
                continue;
            }
            let v = (vec2(nx as f32 + 0.5, ny as f32 + 0.5) - to).length();
            if v < bv {
                bv = v;
                best = k as i32;
            }
        }
        let t = if best < 0 {
            // already in the best cell: slide inside it
            let t = *p + d.normalize_or_zero() * (speed * DT).min(d.length());
            if obj[t.y as usize * W + t.x as usize] != FREE {
                return;
            }
            t
        } else {
            vec2(
                (cx + DX[best as usize]) as f32 + 0.5,
                (cy + DY[best as usize]) as f32 + 0.5,
            )
        };
        let dd = t - *p;
        let st = speed * DT;
        if dd.length() > st {
            *p += dd.normalize() * st;
        } else {
            *p = t;
        }
    }

    /// Evac points with a flight ready call civilians over from the HQ, nearest first, until
    /// every seat is spoken for.
    fn dispatch_evac(&mut self) {
        for pi in 0..self.structs.len() {
            let (ok, c) = {
                let s = &self.structs[pi];
                (!s.dead && !s.pending && s.t == S::Pad && s.cd <= 0.0, s.center())
            };
            if !ok {
                continue;
            }
            let taken = self
                .civs
                .iter()
                .filter(|cv| cv.pad == pi && (cv.st == C_AT_PAD || cv.st == C_TO_IDLE))
                .count() as u32;
            if taken >= PAD_SEATS {
                continue;
            }
            let mut wait: Vec<(f32, usize)> = (0..self.civs.len())
                .filter(|&k| self.civs[k].st == C_AT_HQ)
                .map(|k| (self.civs[k].p.distance_squared(c), k))
                .collect();
            if wait.is_empty() {
                return;
            }
            let n = ((PAD_SEATS - taken) as usize).min(wait.len());
            wait.select_nth_unstable_by(n - 1, |a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            for &(_, k) in &wait[..n] {
                let cv = &mut self.civs[k];
                cv.st = C_TO_IDLE;
                cv.pad = pi;
                // give up and go back to the HQ if the walk takes this long
                cv.flee_t = 60.0;
            }
        }
    }

    fn civilians(&mut self) {
        let obj = self.obj.clone();
        let hqd = self.hq_dist.clone();
        let portf = self.portal_field.clone();
        let mut waiting = 0;
        let mut dragged: Vec<Vec2> = Vec::new();
        if self.ticks % 10 == 0 {
            self.dispatch_evac();
        }
        for i in 0..self.civs.len() {
            let st = self.civs[i].st;
            if !civ_here(st) {
                continue;
            }
            let p = self.civs[i].p;
            let (cx, cy) = (p.x as usize, p.y as usize);
            let c = cy * W + cx;
            if st == C_TO_IDLE {
                // called over from the HQ by an evac point with a flight ready
                waiting += 1;
                let pi = self.civs[i].pad;
                self.civs[i].flee_t -= DT;
                match self.structs.get(pi) {
                    Some(s) if !s.dead && s.t == S::Pad && self.civs[i].flee_t > 0.0 => {
                        let to = s.center();
                        let mut pp = p;
                        let f = self.pad_fields.iter().find(|f| f.0 == pi).map(|f| &f.1);
                        match f {
                            Some(f) if pp.distance(to) > 12.0 && f[c] != INF => {
                                Self::step_on_field(&obj, &mut pp, f, 9.0, &mut self.rng)
                            }
                            _ => Self::step_toward(&obj, &mut pp, to, 9.0),
                        }
                        self.civs[i].p = pp;
                        if pp.distance(to) < 6.5 || f.is_some_and(|f| f[pp.y as usize * W + pp.x as usize] <= 20) {
                            self.civs[i].st = C_AT_PAD;
                        }
                    }
                    _ => self.civs[i].st = C_TO_PAD,
                }
                continue;
            }
            if st == C_AT_PAD {
                waiting += 1;
                let pi = self.civs[i].pad;
                if self.structs.get(pi).map_or(true, |s| s.dead) {
                    self.civs[i].st = C_TO_PAD;
                }
                continue;
            }
            if st == C_TO_PAD {
                // safe inside the base: head for the HQ and wait there for a seat on a flight
                waiting += 1;
                let mut pp = p;
                Self::step_on_field(&obj, &mut pp, &hqd, 9.0, &mut self.rng);
                self.civs[i].p = pp;
                if hqd[pp.y as usize * W + pp.x as usize] <= 40 {
                    self.civs[i].st = C_AT_HQ;
                }
                continue;
            }
            if st == C_AT_HQ {
                // mill about next to the HQ (if it falls, walk to the nearest one still standing)
                waiting += 1;
                if hqd[c] > 80 {
                    self.civs[i].st = C_TO_PAD;
                    continue;
                }
                let civ = &mut self.civs[i];
                if self.rng.f() < 0.05 {
                    civ.hd = self.rng.f() * std::f32::consts::TAU;
                }
                let np = civ.p + vec2(libm::cosf(civ.hd), libm::sinf(civ.hd)) * 1.2 * DT;
                let nc = np.y as usize * W + np.x as usize;
                if np.x > 0.0 && np.y > 0.0 && np.x < W as f32 && np.y < H as f32 && obj[nc] == FREE && hqd[nc] <= 60 {
                    civ.p = np;
                } else {
                    civ.hd = self.rng.f() * std::f32::consts::TAU;
                }
                continue;
            }
            if st == C_CAPTURED {
                // dragged towards a portal; troops close by free them
                let mut freed = false;
                'g: for g in &self.groups {
                    if g.dead || g.state == GState::Garrison {
                        continue;
                    }
                    for m in &g.m {
                        if m.alive && m.p.distance(p) < 5.0 {
                            freed = true;
                            break 'g;
                        }
                    }
                }
                if freed {
                    self.civs[i].st = C_ROAM;
                    self.show("Troops freed a captured civilian", 2.0);
                    continue;
                }
                match &portf {
                    Some(f) if !self.portals.is_empty() => {
                        let mut pp = p;
                        Self::step_on_field(&obj, &mut pp, f, 7.5, &mut self.rng);
                        self.civs[i].p = pp;
                        if f[pp.y as usize * W + pp.x as usize] <= 20 {
                            self.civs[i].st = C_GONE;
                            self.civ_lost += 1;
                            dragged.push(pp);
                        }
                    }
                    _ => {
                        if self.portals.is_empty() {
                            self.civs[i].st = C_DEAD;
                            self.civ_lost += 1;
                        }
                    }
                }
                continue;
            }
            let mut hit = self.swarm.at(c) > 0;
            if !hit && self.rng.f() < 0.35 {
                hit = (cx > 0 && self.swarm.at(c - 1) > 0)
                    || (cx < W - 1 && self.swarm.at(c + 1) > 0)
                    || (cy > 0 && self.swarm.at(c - W) > 0)
                    || (cy < H - 1 && self.swarm.at(c + W) > 0);
            }
            if hit {
                let army = self.army_at(c);
                if army == 2 && !self.portals.is_empty() && self.rng.f() < 0.35 {
                    self.civs[i].st = C_CAPTURED;
                    continue;
                }
                self.civs[i].st = C_DEAD;
                self.civ_lost += 1;
                if army == 0 {
                    // every bite turns one civilian into two zombies
                    self.conv.push(p);
                    self.conv.push(p);
                }
                continue;
            }
            if st == C_ESCORT {
                let (sqi, mi) = (self.civs[i].sq, self.civs[i].esc);
                let (state, m) = {
                    let q = &self.squads[sqi];
                    (q.state, q.m[mi].clone())
                };
                if !m.alive {
                    self.civs[i].st = C_ROAM;
                    continue;
                }
                let mut pp = p;
                let dist = m.p.distance(pp);
                let sp = if dist > 5.0 { 15.0 } else { 9.0 };
                if (state == QState::Return || m.home) && (dist > 12.0 || m.home) {
                    Self::step_on_field(&obj, &mut pp, &hqd, sp, &mut self.rng);
                } else if dist > 2.0 {
                    let off = vec2(((i * 7) % 5) as f32 - 2.0, ((i * 3) % 5) as f32 - 2.0) * 0.6;
                    Self::step_toward(&obj, &mut pp, m.p + off, sp);
                }
                self.civs[i].p = pp;
                if self.world.in_zone(pp) {
                    self.civs[i].st = C_TO_PAD;
                    self.rescued += 1;
                    self.score += 10.0;
                }
                continue;
            }
            // scatter away from hostiles in a random direction, otherwise stroll along the street
            let (gx, gy) = ((p.x as usize / CG) as i32, (p.y as usize / CG) as i32);
            let (mut fx, mut fy, mut seen) = (0.0, 0.0, 0u32);
            for oy in -2..=2 {
                for ox in -2..=2 {
                    let (x, y) = (gx + ox, gy + oy);
                    if x < 0 || y < 0 || x >= CW as i32 || y >= CH as i32 {
                        continue;
                    }
                    let v = self.swarm.coarse_at(y as usize * CW + x as usize);
                    if v > 0 {
                        seen += v;
                        fx -= ox as f32 * v as f32;
                        fy -= oy as f32 * v as f32;
                    }
                }
            }
            if hqd[c] <= HQ_SEEK {
                // close enough to an HQ to make a run for it
                let mut pp = p;
                Self::step_on_field(&obj, &mut pp, &hqd, if seen > 0 { 8.5 } else { 4.5 }, &mut self.rng);
                self.civs[i].p = pp;
                if self.world.in_zone(pp) {
                    self.civs[i].st = C_TO_PAD;
                    self.rescued += 1;
                }
                continue;
            }
            let civ = &mut self.civs[i];
            let mut sp = 1.5;
            civ.flee_t -= DT;
            if seen > 0 {
                sp = 8.5;
                if civ.flee_t <= 0.0 {
                    let away = if fx == 0.0 && fy == 0.0 {
                        self.rng.f() * std::f32::consts::TAU
                    } else {
                        libm::atan2f(fy as f32, fx as f32)
                    };
                    civ.hd = away + (self.rng.f() - 0.5) * 2.6;
                    civ.flee_t = 0.8 + self.rng.f() * 1.4;
                }
            } else if self.rng.f() < 0.04 {
                civ.hd += (self.rng.f() - 0.5) * 2.5;
            }
            let np = civ.p + vec2(libm::cosf(civ.hd), libm::sinf(civ.hd)) * sp * DT;
            if np.x > 0.0
                && np.y > 0.0
                && np.x < W as f32
                && np.y < H as f32
                && obj[np.y as usize * W + np.x as usize] == FREE
            {
                civ.p = np;
            } else {
                civ.hd = self.rng.f() * std::f32::consts::TAU;
            }
        }
        self.waiting = waiting;
        // every civilian dragged through a portal brings enraged orcs back out of it
        for p in dragged {
            let Some(k) = (0..self.portals.len()).min_by(|&a, &b| {
                self.portals[a]
                    .p
                    .distance(p)
                    .partial_cmp(&self.portals[b].p.distance(p))
                    .unwrap()
            }) else {
                continue;
            };
            self.portals[k].taken += 1;
            let n = (60.0 * self.mult * (1.0 + self.portals[k].taken as f32 * 0.1).min(2.5)).round();
            let at = (self.portals[k].p.x, self.portals[k].p.y);
            self.queue_army(at, n, 3.0, 0, 1.0, 2);
            self.show(
                format!(
                    "A civilian was dragged through a portal: {} enraged orcs pour out",
                    n as u64
                ),
                3.0,
            );
        }
    }

    fn nearest_walkable(&self, p: Vec2, max_r: i32) -> Option<usize> {
        let (x0, y0) = (p.x as i32, p.y as i32);
        for r in 0..=max_r {
            for oy in -r..=r {
                for ox in -r..=r {
                    if ox.abs().max(oy.abs()) != r {
                        continue;
                    }
                    let (x, y) = (x0 + ox, y0 + oy);
                    if x < 0 || y < 0 || x >= W as i32 || y >= H as i32 {
                        continue;
                    }
                    let c = y as usize * W + x as usize;
                    if self.obj[c] == FREE && self.hq_dist[c] != INF {
                        return Some(c);
                    }
                }
            }
        }
        None
    }
    fn cell_pos(c: usize) -> Vec2 {
        vec2((c % W) as f32 + 0.5, (c / W) as f32 + 0.5)
    }

    pub fn send_squad(&mut self, at: Vec2) -> bool {
        let n = if self.research.rescue { 18 } else { 10 };
        let near = self.hq_near(at);
        let hq = self.hqs.iter().map(|&i| &self.structs[i]).find(|s| !s.dead && s.center() == near).unwrap_or(&self.structs[self.hq_idx]);
        let base = vec2(hq.center().x, (hq.y + hq.h + 2) as f32);
        self.send_squad_from(base, at, n, -1)
    }
    /// Sends `n` rescue troops from `base` (the HQ or a rescue station) to civilians near `at`.
    fn send_squad_from(&mut self, base: Vec2, at: Vec2, n: usize, from: i32) -> bool {
        let Some(tc) = self.nearest_walkable(at, 20) else {
            return false;
        };
        // troops step out onto open ground, never inside a wall or another building
        let Some(bc) = self.nearest_walkable(base, 16) else {
            return false;
        };
        let base = Self::cell_pos(bc);
        let mut m = Vec::new();
        for _ in 0..n {
            let mut p = base + vec2((self.rng.f() - 0.5) * 6.0, (self.rng.f() - 0.5) * 3.0);
            let pc = (p.y.clamp(0.0, H as f32 - 1.0) as usize) * W + p.x.clamp(0.0, W as f32 - 1.0) as usize;
            if self.obj[pc] != FREE {
                p = base;
            }
            m.push(Member {
                p,
                hp: 1.0,
                cd: self.rng.f() * 0.3,
                alive: true,
                home: false,
                carry: 0,
            });
        }
        let job = self.target_job(tc);
        self.squads.push(Squad {
            state: QState::Wait,
            target: Self::cell_pos(tc),
            tf: None,
            m,
            t: 0.0,
            done: false,
            wiped: false,
            job,
            gather: None,
            gjob: 0,
            gt: 0.0,
            found: 0,
            from,
        });
        true
    }

    fn squads_tick(&mut self) {
        let obj = self.obj.clone();
        let hqd = self.hq_dist.clone();
        let (max_pick, pick_r) = if self.research.rescue {
            (120usize, 90.0)
        } else {
            (50usize, 60.0)
        };
        for si in 0..self.squads.len() {
            if self.squads[si].done {
                continue;
            }
            let n = self.squads[si].m.len();
            let cap = max_pick.div_ceil(n.max(1)) as u8;
            let mut live = 0;
            let mut near = false;
            let mut all_full = true;
            let state = self.squads[si].state;
            let tf = self.squads[si].tf.clone();
            let gather = self.squads[si].gather.clone();
            let target = self.squads[si].target;
            let mut shots = Vec::new();
            let mut claimed = 0;
            for mi in 0..n {
                let mut m = self.squads[si].m[mi].clone();
                if !m.alive || m.home {
                    continue;
                }
                live += 1;
                let c = m.p.y as usize * W + m.p.x as usize;
                if self.swarm.at(c) > 0 && self.rng.f() < 0.12 {
                    m.alive = false;
                    live -= 1;
                    self.squads[si].m[mi] = m;
                    continue;
                }
                match state {
                    QState::Out => {
                        if let Some(tf) = &tf {
                            Self::step_on_field(&obj, &mut m.p, tf, 18.0, &mut self.rng);
                            if tf[m.p.y as usize * W + m.p.x as usize] < 60 {
                                near = true;
                            }
                        }
                    }
                    QState::Gather => {
                        // a soldier with room left and a civilian it can reach keeps going
                        let reach = gather
                            .as_ref()
                            .map_or(true, |g| g[m.p.y as usize * W + m.p.x as usize] != INF);
                        if m.carry < cap && reach {
                            all_full = false;
                            if let Some(g) = &gather {
                                Self::step_on_field(&obj, &mut m.p, g, 16.0, &mut self.rng);
                            }
                            // walk up to each civilian in turn and take them along
                            for ci in 0..self.civs.len() {
                                let cv = &mut self.civs[ci];
                                if cv.st == C_ROAM
                                    && cv.p.distance(m.p) < 2.5
                                    && cv.p.distance(target) < pick_r + 15.0
                                {
                                    cv.st = C_ESCORT;
                                    cv.sq = si;
                                    cv.esc = mi;
                                    m.carry += 1;
                                    claimed += 1;
                                    if m.carry >= cap {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    QState::Return => {
                        Self::step_on_field(&obj, &mut m.p, &hqd, 12.0, &mut self.rng);
                        if self.world.in_zone(m.p) {
                            m.home = true;
                        }
                    }
                    QState::Wait => {}
                }
                m.cd -= DT;
                if m.cd <= 0.0 {
                    if let Some(t) = self.find_target(m.p, 14.0) {
                        shots.push((m.p, t));
                    }
                    m.cd = 0.35;
                }
                self.squads[si].m[mi] = m;
            }
            let src = self.rev_src(RevKey::Squad(si));
            for (a, b) in shots {
                self.swarm.add_damage(b.x, b.y, 1.2, 10.0, src);
                self.damage_hives(b, 1.2, 10.0);
                self.tracers.push(Tracer {
                    a,
                    az: 1.0,
                    b,
                    t: 0.07,
                    kind: 3,
                });
            }
            if live == 0 {
                let q = &mut self.squads[si];
                if q.state != QState::Return {
                    q.wiped = true;
                }
                q.done = true;
                continue;
            }
            self.squads[si].found += claimed;
            // a team that can't get anywhere gives up rather than standing there forever
            if matches!(self.squads[si].state, QState::Wait | QState::Out | QState::Return) {
                let q = &mut self.squads[si];
                q.t += DT;
                if q.t > 150.0 {
                    if q.state == QState::Return {
                        q.done = true;
                        continue;
                    }
                    q.state = QState::Return;
                    q.t = 0.0;
                }
            }
            match self.squads[si].state {
                QState::Out if near => {
                    self.squads[si].state = QState::Gather;
                    self.squads[si].t = 25.0;
                    self.squads[si].gt = 0.0;
                }
                QState::Gather => {
                    let q = &mut self.squads[si];
                    q.t -= DT;
                    q.gt -= DT;
                    let refresh = q.gt <= 0.0;
                    if refresh {
                        q.gt = 1.5;
                    }
                    let timeout = q.t <= 0.0;
                    let mut left: Vec<u32> = Vec::new();
                    if refresh && !all_full && !timeout {
                        for cv in &self.civs {
                            if cv.st == C_ROAM && cv.p.distance(target) < pick_r {
                                left.push((cv.p.y as usize * W + cv.p.x as usize) as u32);
                            }
                        }
                        if left.is_empty() {
                            self.squads[si].t = 0.0;
                        } else if self.fw_aux.busy < 4 {
                            let id = self.next_job();
                            self.squads[si].gjob = id;
                            let max = (pick_r * 70.0) as u32;
                            self.fw_aux.submit(Job::Multi {
                                id,
                                obj: obj.clone(),
                                src: left,
                                max,
                            });
                        }
                    }
                    let q = &mut self.squads[si];
                    if all_full || q.t <= 0.0 {
                        q.state = QState::Return;
                        q.t = 0.0;
                        q.gather = None;
                        let f = q.found;
                        // rescue stations work quietly
                        if q.from >= 0 {
                            continue;
                        }
                        self.banner = (
                            if f > 0 {
                                format!(
                                    "Rescue squad escorting {} civilian{} home",
                                    f,
                                    if f > 1 { "s" } else { "" }
                                )
                            } else {
                                "Rescue squad found nobody".into()
                            },
                            2.5,
                        );
                    }
                }
                _ => {}
            }
            if live > 0 && self.squads[si].m.iter().all(|m| !m.alive || m.home) {
                self.squads[si].done = true;
            }
        }
    }

    pub fn deploy(&mut self, kind: U, at: Vec2) -> bool {
        let d = udef(kind);
        if self.scrap < d.cost || self.world.irradiated_at(at) || !self.unit_ok(kind) {
            return false;
        }
        if !self.spawn_group(kind, d.n, at) {
            return false;
        }
        self.scrap -= d.cost;
        true
    }
    /// Sends `n` troops out of the HQ to a spot (free; `deploy` is the paid version).
    pub fn spawn_group(&mut self, kind: U, n: usize, at: Vec2) -> bool {
        let d = udef(kind);
        let roof = if kind == U::Rifle {
            self.roof_at(at)
        } else {
            None
        };
        let Some(tc) = self.order_cell(roof, at) else {
            return false;
        };
        let hq = &self.structs[self.hq_idx];
        let base = vec2(hq.center().x, (hq.y + hq.h + 3) as f32);
        let mut m = Vec::new();
        for _ in 0..n {
            m.push(Member {
                p: base + vec2((self.rng.f() - 0.5) * 8.0, self.rng.f() * 3.0),
                hp: d.hp,
                cd: self.rng.f() * d.rate,
                alive: true,
                home: false,
                carry: 0,
            });
        }
        let job = self.target_job(tc);
        self.groups.push(Group {
            kind,
            m,
            state: GState::Wait,
            tf: None,
            b: roof.map(|r| r.0 as i32).unwrap_or(-1),
            target: Self::cell_pos(tc),
            dead: false,
            job,
        });
        true
    }
    fn order_cell(&self, roof: Option<(usize, Vec2)>, at: Vec2) -> Option<usize> {
        match roof {
            Some((b, _)) => {
                let bb = &self.world.buildings[b];
                let r = ((bb.x1 - bb.x0).max(bb.y1 - bb.y0) / 2.0) as i32 + 14;
                self.nearest_walkable(bb.c, r)
            }
            None => self.nearest_walkable(at, 12),
        }
    }
    /// Re-orders an existing group to a new spot (or into a building, for rifle squads).
    pub fn order(&mut self, gi: usize, at: Vec2) -> bool {
        if self.world.irradiated_at(at) || self.groups[gi].dead {
            return false;
        }
        let kind = self.groups[gi].kind;
        let roof = if kind == U::Rifle {
            self.roof_at(at)
        } else {
            None
        };
        let Some(tc) = self.order_cell(roof, at) else {
            return false;
        };
        // garrisoned troops step back out onto the street first
        if self.groups[gi].state == GState::Garrison {
            let b = self.groups[gi].b as usize;
            let c = self.world.buildings[b].c;
            if let Some(out) = self.nearest_walkable(c, 30) {
                for m in &mut self.groups[gi].m {
                    m.p = Self::cell_pos(out);
                }
            }
        }
        let job = self.target_job(tc);
        let g = &mut self.groups[gi];
        g.job = job;
        g.state = GState::Wait;
        g.tf = None;
        g.b = roof.map(|r| r.0 as i32).unwrap_or(-1);
        g.target = Self::cell_pos(tc);
        true
    }
    pub fn group_at(&self, p: Vec2) -> Option<usize> {
        let mut best = None;
        let mut bd = 6.0;
        for (i, g) in self.groups.iter().enumerate() {
            if g.dead {
                continue;
            }
            if g.state == GState::Garrison {
                let b = g.b as usize;
                let c = self.world.buildings[b].c;
                if c.distance(p) < bd {
                    bd = c.distance(p);
                    best = Some(i);
                }
                continue;
            }
            for m in &g.m {
                if m.alive && m.p.distance(p) < bd {
                    bd = m.p.distance(p);
                    best = Some(i);
                }
            }
        }
        best
    }

    fn groups_tick(&mut self) {
        let obj = self.obj.clone();
        let atk = self.faction().stats.atk;
        for gi in 0..self.groups.len() {
            if self.groups[gi].dead {
                continue;
            }
            let kind = self.groups[gi].kind;
            let d = udef(kind);
            let gar = self.groups[gi].state == GState::Garrison;
            let b = self.groups[gi].b;
            if gar && !self.world.buildings[b as usize].alive {
                for m in &mut self.groups[gi].m {
                    m.alive = false;
                }
            }
            let state = self.groups[gi].state;
            let tf = self.groups[gi].tf.clone();
            let (mut live, mut arrived) = (0, 0);
            let mut shots = Vec::new();
            for mi in 0..self.groups[gi].m.len() {
                let mut m = self.groups[gi].m[mi].clone();
                if !m.alive {
                    continue;
                }
                if !gar {
                    let c = m.p.y as usize * W + m.p.x as usize;
                    let v = self.swarm.at(c)
                        + self.swarm.at(c + 1)
                        + self.swarm.at(c.saturating_sub(1));
                    if v > 0 {
                        m.hp -= (v.min(30)) as f32 * atk * DT;
                        if m.hp <= 0.0 {
                            m.alive = false;
                            self.groups[gi].m[mi] = m;
                            continue;
                        }
                    }
                    if state == GState::Move {
                        if let Some(tf) = &tf {
                            Self::step_on_field(&obj, &mut m.p, tf, d.spd, &mut self.rng);
                            if tf[m.p.y as usize * W + m.p.x as usize] <= 40 {
                                arrived += 1;
                            }
                        }
                    }
                }
                live += 1;
                m.cd -= DT;
                if m.cd <= 0.0 && self.ammo >= d.ammo {
                    let (origin, muzzle, range) = if gar {
                        let bb = &self.world.buildings[b as usize];
                        (bb.c, (bb.c, bb.h + 1.0), d.range * 1.6)
                    } else {
                        (m.p, (m.p, 1.0), d.range)
                    };
                    if let Some(t) = self
                        .find_target(origin, range)
                        .or_else(|| self.hive_in_range(origin, range))
                    {
                        shots.push((muzzle, t));
                        m.cd = d.rate * (0.85 + 0.3 * self.rng.f());
                    } else {
                        m.cd = 0.2;
                    }
                }
                self.groups[gi].m[mi] = m;
            }
            let src = if shots.is_empty() {
                0
            } else {
                self.rev_src(RevKey::Group(gi))
            };
            for ((a, az), t) in shots {
                self.blast(t, d.rad, d.dmg, d.bdmg, src, d.dmg);
                self.ammo -= d.ammo;
                self.tracers.push(Tracer {
                    a,
                    az,
                    b: t,
                    t: 0.08,
                    kind: if kind == U::Tank { 4 } else { 3 },
                });
                if kind == U::Tank {
                    self.sfx.push((Sfx::Cannon, a));
                    self.booms.push(Boom {
                        p: t,
                        r: d.rad,
                        t: 0.25,
                        life: 0.25,
                        kind: 0,
                    });
                }
            }
            let g = &mut self.groups[gi];
            if live == 0 {
                g.dead = true;
                continue;
            }
            if g.state == GState::Move && arrived >= (live as f32 * 0.6).ceil() as usize {
                g.state = if g.b >= 0 && self.world.buildings[g.b as usize].alive {
                    GState::Garrison
                } else {
                    GState::Hold
                };
            }
        }
    }

    pub fn alive_civs(&self) -> usize {
        self.civs.iter().filter(|c| c.st < C_DEAD).count()
    }

    pub fn hive_in_range(&self, p: Vec2, r: f32) -> Option<Vec2> {
        self.hives
            .iter()
            .filter(|h| !h.dead && h.p.distance(p) < r + 5.0)
            .map(|h| h.p)
            .next()
    }
    pub fn hive_blocks(&self, p: Vec2) -> bool {
        self.hives.iter().any(|h| !h.dead && h.p.distance(p) < h.r)
    }

    /// Alien hives: spawn waves (bigger for every infected building), grow, infect, die.
    fn hives_tick(&mut self) {
        let m = self.mult;
        let mut waves = Vec::new();
        let mut msgs = Vec::new();
        for hi in 0..self.hives.len() {
            if self.hives[hi].dead {
                continue;
            }
            let world = &self.world;
            let h = &mut self.hives[hi];
            h.infected.retain(|&b| world.buildings[b].alive);
            if h.hp <= 0.0 {
                h.dead = true;
                msgs.push(("Hive destroyed".to_string(), h.p));
                continue;
            }
            h.spawn_t -= DT;
            if h.spawn_t <= 0.0 {
                h.spawn_t = 7.0;
                waves.push((h.p, (8.0 + 10.0 * h.infected.len() as f32) * m));
            }
            h.grow_t -= DT;
            if h.grow_t <= 0.0 {
                h.grow_t = 14.0;
                h.r = (h.r + 5.0).min(120.0);
                h.max += 300.0;
                h.hp += 300.0;
                let mut best = None;
                let mut bd = h.r;
                for (bi, b) in world.buildings.iter().enumerate() {
                    if !b.alive || h.infected.contains(&bi) {
                        continue;
                    }
                    let d = b.c.distance(h.p);
                    if d < bd {
                        bd = d;
                        best = Some(bi);
                    }
                }
                if let Some(b) = best {
                    h.infected.push(b);
                }
            }
        }
        for (s, p) in msgs {
            self.score += 2000.0;
            self.gain_scrap(300.0);
            self.booms.push(Boom {
                p,
                r: 30.0,
                t: 1.0,
                life: 1.0,
                kind: 2,
            });
            self.show(s, 3.0);
            self.zone_dirty = true;
        }
        for (p, n) in waves {
            self.queue((p.x, p.y), n, 2.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Anything built inside the base zone costs its base price, however far from the HQ.
    #[test]
    fn zone_has_no_distance_cost() {
        let g = Game::new(crate::citygen::generate(11), 0, 1.0, 3);
        let mut n = 0;
        for c in (0..W * H).step_by(97) {
            let p = vec2((c % W) as f32 + 0.5, (c / W) as f32 + 0.5);
            if !g.world.in_zone(p) {
                continue;
            }
            n += 1;
            for t in BUILD_ORDER {
                assert_eq!(g.cost_at(t, p), def(t).cost.round(), "{:?} at {:?}", t, p);
            }
        }
        assert!(n > 100);
    }
    /// Two games from the same map, seed and commands must stay identical tick for tick.
    #[test]
    fn deterministic() {
        for fac in 0..3 {
            let mk = || Game::new(crate::citygen::generate(11), fac, 1.0, 99);
            let (mut a, mut b) = (mk(), mk());
            crate::selftest::abilities(&mut a);
            crate::selftest::abilities(&mut b);
            for _ in 0..300 {
                a.tick();
                b.tick();
            }
            assert_eq!(a.checksum(), b.checksum(), "faction {}", fac);
            assert!(a.hostiles() > 0);
        }
    }
    /// The special towers stay in lockstep too, whatever the thread count.
    #[test]
    fn towers_deterministic() {
        for fac in 0..FACTIONS.len() {
            let mk = || Game::new(crate::citygen::generate(11), fac, 1.0, 7);
            let run = || {
                let mut g = mk();
                crate::selftest::towers(&mut g);
                for _ in 0..200 {
                    g.tick();
                }
                (g.checksum(), g.kills)
            };
            let one = rayon::ThreadPoolBuilder::new().num_threads(1).build().unwrap().install(run);
            let many = rayon::ThreadPoolBuilder::new().num_threads(4).build().unwrap().install(run);
            assert_eq!(one, many, "faction {}", fac);
            assert!(one.1 > 0);
        }
    }

    /// Chaos: all three armies spawn, each keeps its own kind, and the game stays deterministic.
    #[test]
    fn chaos_has_every_army() {
        let run = || {
            let mut g = Game::new(crate::citygen::generate(5), CHAOS, 1.0, 9);
            g.stress_n(30000.0);
            for _ in 0..300 {
                g.tick();
            }
            let mut n = [0usize; 3];
            for u in g.swarm.units() {
                n[kind_of(u.fl)] += 1;
            }
            (g.checksum(), n)
        };
        let one = rayon::ThreadPoolBuilder::new().num_threads(1).build().unwrap().install(run);
        let many = rayon::ThreadPoolBuilder::new().num_threads(4).build().unwrap().install(run);
        assert_eq!(one, many);
        assert!(one.1.iter().all(|&k| k > 1000), "{:?}", one.1);
    }
}

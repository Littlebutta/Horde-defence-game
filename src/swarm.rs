//! The horde: millions of single-pixel units, updated in parallel on every CPU core.
//!
//! The map is cut into 16x16-cell tiles and every tile keeps its own list of the units standing in it.
//! Each tick runs in two passes, both spread over every core by rayon:
//!
//! 1. Step and settle: each tile moves its own units, then (while they are still in cache) drops the
//!    dead, sets aside the units that walked out of the tile, and counts the rest into this tick's
//!    density and tint grids. A busy tile, where the horde is packed, fans out further into runs of a
//!    few thousand units, each run on its own core, counting into a private copy of the tile's cells
//!    that is added up afterwards; packed tiles also work out each cell's path costs once instead of
//!    once per unit. Units only read shared grids (object grid, flow fields, last tick's crowd
//!    density, the damage grid) and write their own slot, plus atomic damage tallies for whatever
//!    they attack. Each tile writes only the grid cells inside itself, so no atomic adds are needed.
//! 2. Hand-over: every tile takes in the units that walked over from its eight neighbours.
//!
//! Dead units and leavers leave holes in a tile's list, which is swept once a quarter of it is holes.
//!
//! Clumps: where the horde is packed (dozens to hundreds of units per cell), the units sharing a cell
//! are merged now and then into clumps of up to `MAX_CLUMP`, each stepped as one unit that carries a
//! member count. The renderer only ever sees units per cell, and a packed cell is drawn the same
//! whether it holds 30 units or 3 clumps of 10, so the swarm looks just as it did while the work per
//! tick drops by the clump size. Each packed cell keeps about `CLUMPS_PER_CELL` clumps (big ones are
//! broken up as the crowd thins), so the crowd still spreads out the way single units do. A clump
//! splits back into single units where the crowd around it thins out (the edge of the horde, where
//! single units are visible), wherever outside a packed crowd its members would have taken different
//! steps, and when it takes damage, which kills its members unevenly (their health is spread like
//! single units' is).
//!
//! Nothing depends on how many threads there are: random seeds come from (tick, tile, chunk), chunk
//! sizes are fixed, and results are gathered in tile order, so co-op lockstep stays in sync.
//!
//! Faction quirks handled here:
//! * zombies go down once and get back up, and swarm whatever killed one of them (locked to that target);
//! * some alien bugs fly, ignoring city buildings and walls on their way to the HQ;
//! * orcs build rage when units around them die, which makes them faster (and redder).
//!
//! In a chaos game all three armies share the map: each unit carries its army in its flags
//! (`kind_of`) and moves by that army's stats and quirks.

use crate::fields::{DX, DY, INF, U16};
use crate::world::*;
use macroquad::prelude::{vec2, Vec2};
use rayon::prelude::*;
use std::sync::atomic::{AtomicU16, AtomicU32, AtomicU8, Ordering::Relaxed};

/// Tile edge in cells. Each tile owns its units and its cells of the density grids.
pub const TS: usize = 16;
pub const TW: usize = W / TS;
pub const TH: usize = H / TS;
pub const NT: usize = TW * TH;
/// Units stepped per task inside a busy tile.
const CHUNK: usize = 4096;
/// Most members one clump carries.
pub const MAX_CLUMP: u16 = 64;
/// A tile merges its packed cells every this many ticks (tiles take turns).
const MERGE_EVERY: u64 = 8;
/// Units per cell, averaged over the 3x3 cells around, from which a cell's units get merged (and
/// above which a clump keeps together even where its members would part ways) ...
const MERGE_AT: u32 = 16;
/// ... as long as none of those cells holds fewer than this (the edge of a crowd).
const MERGE_LEAST: u32 = 4;
/// Units per cell, averaged over the 3x3 cells around, below which a clump splits back into single units.
const SPLIT_AT: u32 = 6;
/// The same two for fliers, which flutter apart as they go: they keep together in thicker crowds only.
const FLY_MERGE_AT: u32 = 32;
const FLY_SPLIT_AT: u32 = 16;
/// Clumps per packed cell to aim for, so a packed crowd still flows out in several directions.
const CLUMPS_PER_CELL: u32 = 8;

/// Cell coordinate of a position: one instruction instead of a saturating cast, which matters when it
/// runs several times per unit per tick.
#[inline(always)]
fn cl(v: f32) -> usize {
    debug_assert!(v >= 0.0 && v < W.max(H) as f32);
    // SAFETY: positions are always finite and on the map (spawns are on the map, every move is
    // clamped or checked against the grid), so the value fits in an i32.
    unsafe { v.to_int_unchecked::<i32>() as usize }
}

#[inline]
fn tile_of(x: f32, y: f32) -> usize {
    (cl(y) / TS) * TW + cl(x) / TS
}

#[derive(Clone, Copy, PartialEq)]
pub enum Behaviour {
    Zombie,
    Alien,
    Orc,
}

#[derive(Clone, Copy)]
pub struct FactionStats {
    pub mode: Behaviour,
    pub spd: f32,
    pub hp: f32,
    pub atk: f32,
    pub jit: f32,
}

pub const F_FLY: u8 = 1;
pub const F_REVIVED: u8 = 2;
pub const F_DOWN: u8 = 4;
/// An empty slot left by a unit that died or walked into another tile (swept out now and then).
const F_GONE: u8 = 8;
/// Bits 4 and 5: which army a unit belongs to (an index into `FACTIONS`). Only read in chaos games.
pub const KIND_SHIFT: u8 = 4;
pub const F_KIND: u8 = 3 << KIND_SHIFT;

#[inline]
pub fn kind_of(fl: u8) -> usize {
    ((fl & F_KIND) >> KIND_SHIFT) as usize
}
pub fn kind_flag(k: usize) -> u8 {
    ((k as u8) << KIND_SHIFT) & F_KIND
}

/// Tint codes written per cell for the renderer (highest wins).
pub const TINT_DOWN: u8 = 1;
pub const TINT_NORMAL: u8 = 2;
pub const TINT_FLY: u8 = 3;
pub const TINT_RAGE: u8 = 4; // 4..=255 = rage level
/// Chaos games: the army sits in the top two bits (`tint >> 6`) and the code above in the rest,
/// with rage squeezed into 4..=63.
pub const TINT_ARMY_SHIFT: u8 = 6;
pub const TINT_MIXED_RAGE: f32 = 59.0;

#[derive(Clone, Copy)]
pub struct Unit {
    pub x: f32,
    pub y: f32,
    pub hp: f32,
    /// zombies: time left lying down; orcs: rage 0..1
    pub aux: f32,
    /// revenge target + 1 (0 = none)
    pub tg: u16,
    pub hd: u8,
    pub fl: u8,
    /// members: 1 for a single unit, more for a clump (see the module notes)
    pub n: u16,
}

/// A zombie that died where damage from attacker `src` landed.
#[derive(Clone, Copy)]
pub struct Death {
    pub p: Vec2,
    pub src: u16,
}

pub struct RevIn<'a> {
    pub alive: bool,
    pub pos: Vec2,
    pub field: Option<&'a [u32]>,
}

pub struct Swarm {
    /// units, bucketed by the tile they stand in
    tiles: Vec<Vec<Unit>>,
    /// units leaving each tile this tick (reused buffers)
    out: Vec<Vec<Unit>>,
    /// empty slots per tile
    holes: Vec<u32>,
    /// bit 0: tile had units this tick, bit 1: last tick (so stale grid cells get cleared)
    seen: Vec<u8>,
    /// the last tick each tile had units (so the renderer knows which parts of the map changed)
    last: Vec<u64>,
    /// members per tile
    members: Vec<u64>,
    count: usize,
    /// units per cell this tick and last tick (16-bit keeps these grids small and cache friendly).
    /// Atomics only so tiles can share the grid; each cell is written by its own tile alone.
    pub dens: Vec<AtomicU16>,
    pub prev: Vec<AtomicU16>,
    pub tint: Vec<AtomicU8>,
    /// units per coarse cell
    coarse: Vec<AtomicU32>,
    pub deaths: Vec<AtomicU32>,
    pub deaths_prev: Vec<AtomicU32>,
    pub dmg: Vec<f32>,
    pub dsrc: Vec<u16>,
    touched: Vec<u32>,
    tick: u64,
}

pub struct StepIn<'a> {
    pub dt: f32,
    pub obj: &'a [i32],
    pub nb: &'a [u8],
    pub hq: &'a [u32],
    pub hq_pos: Vec2,
    pub hunt: Option<&'a [u16]>,
    pub rush: bool,
    pub stats: FactionStats,
    /// chaos game: every army's stats, looked up per unit
    pub mixed: Option<[FactionStats; 3]>,
    pub sd: &'a [AtomicU32],
    pub bd: &'a [AtomicU32],
    /// per structure: can fliers pass over it (walls)
    pub low: &'a [bool],
    /// coarse cells where a zombie just died: revenge target + 1
    pub lock: &'a [u16],
    pub rev: &'a [RevIn<'a>],
    /// coarse cells in a helium tower's slowing field
    pub slow: &'a [u8],
    /// orcs: coarse cells inside a regroup (index + 1 into `rallies`, 0 = none; may be empty)
    pub rally: &'a [u16],
    pub rallies: &'a [Rally],
}

/// Orcs that lost one of their own: they fall back to `to` and bunch up, then (`charge`) come on
/// again together at full rage.
#[derive(Clone, Copy)]
pub struct Rally {
    pub to: Vec2,
    pub charge: bool,
}

/// The step from cell (`cx`, `cy`) that heads most directly for `d` away, with a little noise.
#[inline]
fn toward(m: u8, cx: i32, cy: i32, obj: &[i32], d: Vec2, s: &mut u64) -> i32 {
    let mut best = -1;
    let mut bv = f32::MAX;
    for k in 0..8 {
        let nx = cx + DX[k];
        let ny = cy + DY[k];
        if m & (1 << k) == 0 {
            continue;
        }
        let o = obj[ny as usize * W + nx as usize];
        if o == SOLID || o <= -2 {
            continue;
        }
        let v = -(DX[k] as f32 * d.x + DY[k] as f32 * d.y) / (if k < 4 { 1.0 } else { 1.414 }) + xs(s) * 4.0;
        if v < bv {
            bv = v;
            best = k as i32;
        }
    }
    best
}

/// Fixed-point scale for damage tallies kept in atomics.
pub const DMG_SCALE: f32 = 64.0;

#[inline]
fn xs(s: &mut u64) -> f32 {
    *s ^= *s << 13;
    *s ^= *s >> 7;
    *s ^= *s << 17;
    (*s >> 40) as f32 / (1u64 << 24) as f32
}

/// 64 random bits (xorshift64*), for when one draw can feed several decisions.
#[inline]
fn xr(s: &mut u64) -> u64 {
    *s ^= *s << 13;
    *s ^= *s >> 7;
    *s ^= *s << 17;
    s.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

/// Grid offsets of the eight neighbours, in the same order as `DX`/`DY`.
const OFF: [isize; 8] = [1, -1, W as isize, -(W as isize), W as isize + 1, 1 - W as isize, W as isize - 1, -1 - W as isize];

/// Picks the open neighbour with the lowest field value plus crowding and jitter (a row of the
/// jitter table). Returns -1 when nothing beats `bv`.
#[allow(clippy::too_many_arguments)]
#[inline(always)]
fn descend(m: u8, c: usize, mut bv: f32, field: impl Fn(usize) -> u32, inf: u32, prev: &[AtomicU16], pk: f32, jt: &[f32; 8]) -> i32 {
    let mut best = -1;
    for k in 0..8 {
        if m & (1 << k) == 0 {
            continue;
        }
        let nc = (c as isize + OFF[k]) as usize;
        let fv = field(nc);
        if fv == inf {
            continue;
        }
        let v = fv as f32 + prev[nc].load(Relaxed) as f32 * pk + jt[k];
        if v < bv {
            bv = v;
            best = k as i32;
        }
    }
    best
}

/// The costs `descend` weighs (before jitter), into `out`: infinity where the step is closed.
#[inline(always)]
fn costs(out: &mut [f32; 8], m: u8, c: usize, field: impl Fn(usize) -> u32, inf: u32, prev: &[AtomicU16], pk: f32) {
    for k in 0..8 {
        if m & (1 << k) == 0 {
            continue;
        }
        let nc = (c as isize + OFF[k]) as usize;
        let fv = field(nc);
        if fv != inf {
            out[k] = fv as f32 + prev[nc].load(Relaxed) as f32 * pk;
        }
    }
}

/// `descend` over precomputed costs from `cell_costs`, with one row of the jitter table.
/// Written as two plain passes (sum and min, then find) so the compiler can use SIMD.
#[inline(always)]
fn pick(costs: &[f32; 8], jt: &[f32; 8]) -> i32 {
    let mut v = [0f32; 8];
    for k in 0..8 {
        v[k] = costs[k] + jt[k];
    }
    let m = v.iter().fold(f32::MAX, |a, &b| if b < a { b } else { a });
    if m >= f32::MAX {
        return -1;
    }
    v.iter().position(|&b| b == m).map_or(-1, |k| k as i32)
}

/// Rows of random jitter per neighbour, scaled by the faction's jitter, drawn fresh every tick.
/// Each unit picks a row with one random byte instead of rolling eight numbers.
pub type Jitter = [[f32; 8]; 256];

fn jitter(seed: u64, jit: f32) -> Box<Jitter> {
    let mut s = seed | 1;
    let mut t = Box::new([[0f32; 8]; 256]);
    for row in t.iter_mut() {
        for v in row.iter_mut() {
            *v = xs(&mut s) * jit;
        }
    }
    t
}

/// Everything a unit reads while it moves.
struct Ctx<'a> {
    inp: &'a StepIn<'a>,
    prev: &'a [AtomicU16],
    dprev: &'a [AtomicU32],
    deaths: &'a [AtomicU32],
    dmg: &'a [f32],
    dsrc: &'a [u16],
    any_dmg: bool,
    tick: u64,
    jt: &'a Jitter,
    /// PH_NOCLUMP=1 turns clumping off (to compare the horde's looks and speed)
    merge: bool,
}

/// The cell grids one tile writes: its own cells only.
struct Grids<'a> {
    dens: &'a [AtomicU16],
    tint: &'a [AtomicU8],
    coarse: &'a [AtomicU32],
}

/// Coarse cells across one tile.
const TC: usize = TS / CG;

/// One run of units' share of its tile's grid cells, counted privately and then added to the grids,
/// so the runs of a crowded tile can settle on different cores at once.
#[derive(Clone)]
struct Acc {
    dens: [u16; TS * TS],
    tint: [u8; TS * TS],
    coarse: [u32; TC * TC],
}

impl Acc {
    fn new() -> Self {
        Acc { dens: [0; TS * TS], tint: [0; TS * TS], coarse: [0; TC * TC] }
    }
    fn add(&mut self, o: &Acc) {
        for i in 0..TS * TS {
            self.dens[i] = self.dens[i].saturating_add(o.dens[i]);
            self.tint[i] = self.tint[i].max(o.tint[i]);
        }
        for i in 0..TC * TC {
            self.coarse[i] += o.coarse[i];
        }
    }
}

/// How a game's units are tinted: one army, one army of orcs (rage shows), or chaos.
#[derive(Clone, Copy, PartialEq)]
enum Tm {
    Plain,
    Orc,
    Mixed,
}

#[inline]
fn tint_of(u: &Unit, tm: Tm) -> u8 {
    if tm == Tm::Mixed {
        let k = kind_of(u.fl);
        let code = if u.fl & F_DOWN != 0 {
            TINT_DOWN
        } else if u.fl & F_FLY != 0 {
            TINT_FLY
        } else if k == 2 {
            TINT_RAGE + (u.aux * TINT_MIXED_RAGE) as u8
        } else {
            TINT_NORMAL
        };
        return ((k as u8) << TINT_ARMY_SHIFT) | code;
    }
    if u.fl & F_DOWN != 0 {
        TINT_DOWN
    } else if u.fl & F_FLY != 0 {
        TINT_FLY
    } else if tm == Tm::Orc {
        TINT_RAGE + (u.aux * 251.0) as u8
    } else {
        TINT_NORMAL
    }
}

/// Right after a run of units has moved (while it is still in cache): drops the dead and the leavers
/// (they become holes, leavers also go to `out`) and counts the rest into `acc`. Returns the kills,
/// the holes and the members still standing in the run.
fn settle_units(t: usize, us: &mut [Unit], acc: &mut Acc, out: &mut Vec<Unit>, tm: Tm) -> (u32, u32, u64) {
    let (x0, y0) = ((t % TW) * TS, (t / TW) * TS);
    let mut killed = 0;
    let mut holes = 0;
    let mut members = 0;
    for u in us.iter_mut() {
        if u.fl & F_GONE != 0 {
            holes += 1;
            continue;
        }
        // zombies that just went down count as kills too, but stay (lying down)
        let just_down = u.hd & 0x80 != 0;
        if just_down {
            killed += u.n as u32;
            u.hd &= 0x7f;
        }
        if u.hp <= 0.0 {
            killed += u.n as u32;
            u.fl |= F_GONE;
            holes += 1;
            continue;
        }
        let (lx, ly) = (cl(u.x).wrapping_sub(x0), cl(u.y).wrapping_sub(y0));
        if lx >= TS || ly >= TS {
            out.push(*u);
            u.fl |= F_GONE;
            holes += 1;
            continue;
        }
        members += u.n as u64;
        if just_down {
            continue;
        }
        let i = ly * TS + lx;
        if u.fl & F_DOWN == 0 {
            acc.dens[i] = acc.dens[i].saturating_add(u.n);
        }
        acc.tint[i] = acc.tint[i].max(tint_of(u, tm));
    }
    // coarse counts from the cell counts
    for ly in 0..TS {
        for lx in 0..TS {
            acc.coarse[(ly / CG) * TC + lx / CG] += acc.dens[ly * TS + lx] as u32;
        }
    }
    (killed, holes, members)
}

impl Grids<'_> {
    fn clear(&self, t: usize) {
        self.write(t, &Acc::new());
    }
    /// Overwrites the tile's cells with what its units counted.
    fn write(&self, t: usize, a: &Acc) {
        let (x0, y0) = ((t % TW) * TS, (t / TW) * TS);
        for ly in 0..TS {
            let c0 = (y0 + ly) * W + x0;
            for lx in 0..TS {
                self.dens[c0 + lx].store(a.dens[ly * TS + lx], Relaxed);
                self.tint[c0 + lx].store(a.tint[ly * TS + lx], Relaxed);
            }
        }
        for gy in 0..TC {
            for gx in 0..TC {
                self.coarse[(y0 / CG + gy) * CW + x0 / CG + gx].store(a.coarse[gy * TC + gx], Relaxed);
            }
        }
    }
    /// Counts one unit that walked in. Only call it from the tile the unit stands in.
    #[inline]
    fn mark(&self, u: &Unit, tm: Tm) {
        let c = cl(u.y) * W + cl(u.x);
        if u.fl & F_DOWN == 0 {
            let d = &self.dens[c];
            d.store(d.load(Relaxed).saturating_add(u.n), Relaxed);
            let g = &self.coarse[(cl(u.y) / CG) * CW + cl(u.x) / CG];
            g.store(g.load(Relaxed) + u.n as u32, Relaxed);
        }
        let t = &self.tint[c];
        t.store(t.load(Relaxed).max(tint_of(u, tm)), Relaxed);
    }
}

/// Units per cell above which a tile caches its cells' neighbour costs before stepping.
const PACKED: usize = 2;

/// Per cell of one tile, the cost of stepping to each neighbour along the cell's flow field
/// (field value plus crowding, before jitter), or infinity where the step is blocked. In a packed
/// crowd hundreds of units share a cell, so this is worked out once per cell instead of per unit.
/// It does the same float sums as `descend`, so units move exactly as they would without it.
fn cell_costs(k: &Ctx, t: usize, tab: &mut [[f32; 8]; TS * TS], use_hunt: bool) {
    let inp = k.inp;
    let hunt = inp.hunt.unwrap_or(&[]);
    let (x0, y0) = ((t % TW) * TS, (t / TW) * TS);
    for ly in 0..TS {
        for lx in 0..TS {
            let c = (y0 + ly) * W + x0 + lx;
            let m = inp.nb[c];
            let row = &mut tab[ly * TS + lx];
            for (j, v) in row.iter_mut().enumerate() {
                *v = f32::INFINITY;
                if m & (1 << j) == 0 {
                    continue;
                }
                let nc = (c as isize + OFF[j]) as usize;
                let p = k.prev[nc].load(Relaxed) as f32;
                if use_hunt && hunt[c] < U16 {
                    if hunt[nc] != U16 {
                        *v = hunt[nc] as f32 + p * DK;
                    }
                } else if inp.hq[nc] != INF {
                    *v = inp.hq[nc] as f32 + p * DK;
                }
            }
        }
    }
}

/// Crowding weight when following the HQ or hunting field.
const DK: f32 = 6.0;

/// Splits `n` members off a clump as single units in its cell (they go to `out`, which hands them
/// back to this tile), each with about the clump's health.
fn split_off(u: &Unit, n: u16, hp: f32, s: &mut u64, out: &mut Vec<Unit>) {
    let (cx, cy) = (cl(u.x) as f32, cl(u.y) as f32);
    for _ in 0..n {
        let r = xr(s);
        let fr = |b: u32| ((r >> b) & 0xffff) as f32 * (1.0 / 65536.0);
        out.push(Unit {
            x: (cx + fr(0)).min(cx + 0.999),
            y: (cy + fr(16)).min(cy + 0.999),
            hp: hp * (0.9 + 0.2 * fr(32)),
            n: 1,
            hd: (r >> 48) as u8 & 7,
            ..*u
        });
    }
}

/// Moves a run of units one tick. Returns the members of clumps killed here (single units' deaths
/// are counted when the run settles).
fn step_units(k: &Ctx, t: usize, seed: u64, us: &mut [Unit], ev: &mut Vec<Death>, out: &mut Vec<Unit>) -> u32 {
    let inp = k.inp;
    let mixed = inp.mixed;
    let st0 = inp.stats;
    let zombie0 = st0.mode == Behaviour::Zombie;
    let use_hunt0 = inp.hunt.is_some() && !(zombie0 && inp.rush);
    let wander0 = zombie0 && !inp.rush;
    let (obj, nb, hq, hunt) = (inp.obj, inp.nb, inp.hq, inp.hunt.unwrap_or(&[]));
    let (prev, dprev, deaths, dmg, dsrc, any_dmg) = (k.prev, k.dprev, k.deaths, k.dmg, k.dsrc, k.any_dmg);
    let dk = DK;
    let dt = inp.dt;
    let mut s = seed;
    let mut tab_buf = None;
    // (in chaos the shared table is the one orcs and bugs use; zombies off the hunt work it out themselves)
    let tab_hunt = if mixed.is_some() { inp.hunt.is_some() } else { use_hunt0 };
    if us.len() > PACKED * TS * TS && (tab_hunt || !wander0 || mixed.is_some()) {
        let mut b = Box::new([[0f32; 8]; TS * TS]);
        cell_costs(k, t, &mut b, tab_hunt);
        tab_buf = Some(b);
    }
    let tab = tab_buf.as_deref();
    let mut killed = 0u32;
    // orcs: deaths around each coarse cell of this tile last tick (3x3 coarse cells), worked out once
    let mut rage_near = [0u32; TC * TC];
    if st0.mode == Behaviour::Orc || mixed.is_some() {
        let (gx0, gy0) = (((t % TW) * TC) as i32, ((t / TW) * TC) as i32);
        for (i, v) in rage_near.iter_mut().enumerate() {
            let (gx, gy) = (gx0 + (i % TC) as i32, gy0 + (i / TC) as i32);
            for oy in -1..=1 {
                for ox in -1..=1 {
                    let (qx, qy) = (gx + ox, gy + oy);
                    if qx >= 0 && qy >= 0 && qx < CW as i32 && qy < CH as i32 {
                        *v += dprev[qy as usize * CW + qx as usize].load(Relaxed);
                    }
                }
            }
        }
    }
    let (x0, y0) = (((t % TW) * TS) as i32, ((t / TW) * TS) as i32);
    for u in us.iter_mut() {
        if u.fl & F_GONE != 0 {
            continue;
        }
        let (st, use_hunt, wander) = match &mixed {
            Some(m) => {
                let st = m[kind_of(u.fl).min(2)];
                let z = st.mode == Behaviour::Zombie;
                (st, inp.hunt.is_some() && !(z && inp.rush), z && !inp.rush)
            }
            None => (st0, use_hunt0, wander0),
        };
        let zombie = st.mode == Behaviour::Zombie;
        let orc = st.mode == Behaviour::Orc;
        // the shared table only fits units that follow the same field it was worked out for
        let tab = tab.filter(|_| use_hunt == tab_hunt);
        let (x, y) = (u.x, u.y);
        let cx = cl(x) as i32;
        let cy = cl(y) as i32;
        let c = cy as usize * W + cx as usize;
        let gi = (cy as usize / CG) * CW + cx as usize / CG;
        // zombies lying down get back up once
        if u.fl & F_DOWN != 0 {
            u.aux -= dt;
            if u.aux <= 0.0 {
                u.fl = (u.fl & !F_DOWN) | F_REVIVED;
                u.hp = st.hp;
            }
            continue;
        }
        let dm = if any_dmg { dmg[c] } else { 0.0 };
        if dm > 0.0 && u.n > 1 {
            // members' health is spread ±20% around the clump's, so damage kills them unevenly:
            // the dead drop out (zombies lie down as a clump of their own), the rest split up
            let n = u.n as u32;
            let f = ((dm / u.hp - 0.8) / 0.4).clamp(0.0, 1.0);
            let dead = ((f * n as f32 + xs(&mut s)) as u32).min(n);
            if dead > 0 {
                deaths[gi].fetch_add(dead, Relaxed);
                killed += dead;
                if zombie {
                    if dsrc[c] != 0 {
                        ev.push(Death { p: vec2(x, y), src: dsrc[c] });
                    }
                    if u.fl & F_REVIVED == 0 {
                        out.push(Unit {
                            hp: 0.5,
                            aux: 2.5 + xs(&mut s) * 2.0,
                            tg: 0,
                            hd: u.hd & 7,
                            fl: u.fl | F_DOWN,
                            n: dead as u16,
                            ..*u
                        });
                    }
                }
            }
            let left = n - dead;
            if left == 0 {
                u.fl |= F_GONE;
                continue;
            }
            // the survivors' average health once the hit is taken
            let lo = dm.max(0.8 * u.hp);
            let hp = ((lo + 1.2 * u.hp) * 0.5 - dm).max(0.05);
            split_off(u, left as u16 - 1, hp, &mut s, out);
            u.n = 1;
            u.hp = hp;
            continue;
        }
        if dm > 0.0 {
            u.hp -= dm;
            if u.hp <= 0.0 {
                deaths[gi].fetch_add(1, Relaxed);
                if zombie {
                    if dsrc[c] != 0 {
                        ev.push(Death {
                            p: vec2(x, y),
                            src: dsrc[c],
                        });
                    }
                    if u.fl & F_REVIVED == 0 {
                        u.fl |= F_DOWN;
                        u.aux = 2.5 + xs(&mut s) * 2.0;
                        u.hp = 0.5;
                        u.tg = 0;
                        // still counted as a kill below by the caller via hp marker
                        u.hd |= 0x80;
                    }
                }
                continue;
            }
        }
        // a clump on the thin edge of the crowd breaks up into single units
        let mut packed = false;
        if u.n > 1 && cx > 0 && cy > 0 && cx < W as i32 - 1 && cy < H as i32 - 1 {
            let p = |i: usize| prev[i].load(Relaxed) as u32;
            let near = p(c - W - 1) + p(c - W) + p(c - W + 1) + p(c - 1) + p(c) + p(c + 1) + p(c + W - 1) + p(c + W) + p(c + W + 1);
            packed = near >= MERGE_AT * 9;
            if near < if u.fl & F_FLY != 0 { FLY_SPLIT_AT } else { SPLIT_AT } * 9 {
                split_off(u, u.n - 1, u.hp, &mut s, out);
                u.n = 1;
            }
        }
        let mut spd = st.spd;
        if orc {
            let near = rage_near[((cy - y0) as usize / CG) * TC + (cx - x0) as usize / CG];
            u.aux =
                ((u.aux - dt * 0.015) + near.min(200) as f32 * 0.004).clamp(0.0, 1.0);
            // charging back in after a regroup: all at full rage
            if !inp.rally.is_empty() {
                let ri = inp.rally[gi];
                if ri != 0 && inp.rallies.get(ri as usize - 1).is_some_and(|r| r.charge) {
                    u.aux = 1.0;
                }
            }
            spd *= 1.0 + u.aux * 1.2;
        }
        if inp.slow[gi] != 0 {
            spd *= 0.4;
        }
        // fliers: straight for the HQ over rooftops and walls
        if u.fl & F_FLY != 0 {
            let d = inp.hq_pos - vec2(x, y);
            let len = d.length().max(0.001);
            let wob = (xs(&mut s) - 0.5) * 1.6;
            let dir = vec2(d.x / len, d.y / len);
            let dir =
                vec2(dir.x - dir.y * wob, dir.y + dir.x * wob).normalize_or_zero();
            let stp = spd * 1.15 * dt * (0.75 + 0.5 * xs(&mut s));
            let (px, py) = (
                (x + dir.x * stp).clamp(0.0, W as f32 - 1.0),
                (y + dir.y * stp).clamp(0.0, H as f32 - 1.0),
            );
            let c2 = cl(py) * W + cl(px);
            let o = obj[c2];
            if o >= 0 && !inp.low.get(o as usize).copied().unwrap_or(true) {
                if let Some(a) = inp.sd.get(o as usize) {
                    a.fetch_add((st.atk * dt * DMG_SCALE * u.n as f32) as u32, Relaxed);
                }
            } else {
                u.x = px;
                u.y = py;
            }
            continue;
        }
        let m = nb[c];
        // zombie revenge: lock on to whoever killed one nearby, until it dies
        if zombie {
            if u.tg == 0 {
                let l = inp.lock[gi];
                if l != 0 {
                    u.tg = l;
                }
            }
            if u.tg != 0 {
                match inp.rev.get(u.tg as usize - 1) {
                    Some(r) if r.alive => spd *= 1.7,
                    _ => u.tg = 0,
                }
            }
        }
        let mut best: i32 = -1;
        let mut field_hq = true;
        let rev = if u.tg != 0 {
            inp.rev.get(u.tg as usize - 1)
        } else {
            None
        };
        if let Some(r) = rev {
            match r.field {
                Some(f) if f[c] != INF => {
                    best = descend(m, c, f[c] as f32 + 6.0, |i| f[i], INF, prev, 2.0, &k.jt[(xr(&mut s) & 255) as usize]);
                    field_hq = false;
                }
                _ => {
                    // no field yet: head straight at it
                    best = toward(m, cx, cy, obj, r.pos - vec2(x, y), &mut s);
                    field_hq = false;
                }
            }
        }
        // orcs regrouping after a loss: back to the rally point and wait there for the others
        let ri = if orc && !inp.rally.is_empty() { inp.rally[gi] } else { 0 };
        if let (true, Some(r)) = (field_hq, inp.rallies.get(ri.wrapping_sub(1) as usize)) {
            if !r.charge {
                let d = r.to - vec2(x, y);
                if d.length_squared() < 9.0 + xs(&mut s) * 16.0 {
                    continue;
                }
                best = toward(m, cx, cy, obj, d, &mut s);
                field_hq = false;
            }
        }
        // the field-following cases: each neighbour's cost before jitter, where a clump checks
        // whether its members would all take the same step
        let follow = field_hq && !(wander && !(use_hunt && hunt[c] < U16));
        let mut cs = [f32::INFINITY; 8];
        if !field_hq {
        } else if let (Some(tab), true) = (tab, follow) {
            cs = tab[((cy - y0) * TS as i32 + cx - x0) as usize];
            best = pick(&cs, &k.jt[(xr(&mut s) & 255) as usize]);
        } else if use_hunt && hunt[c] < U16 {
            best = descend(m, c, f32::MAX, |i| hunt[i] as u32, U16 as u32, prev, dk, &k.jt[(xr(&mut s) & 255) as usize]);
            if u.n > 1 {
                costs(&mut cs, m, c, |i| hunt[i] as u32, U16 as u32, prev, dk);
            }
        } else if wander {
            let mut k = (u.hd & 7) as usize;
            if xs(&mut s) < 0.03 {
                k = (xs(&mut s) * 8.0) as usize & 7;
            }
            for _ in 0..3 {
                let nx = cx + DX[k];
                let ny = cy + DY[k];
                if m & (1 << k) != 0 && obj[ny as usize * W + nx as usize] == FREE {
                    best = k as i32;
                    break;
                }
                k = (xs(&mut s) * 8.0) as usize & 7;
            }
            u.hd = (u.hd & 0x80) | k as u8;
        } else {
            best = descend(m, c, f32::MAX, |i| hq[i], INF, prev, dk, &k.jt[(xr(&mut s) & 255) as usize]);
            if u.n > 1 {
                costs(&mut cs, m, c, |i| hq[i], INF, prev, dk);
            }
        }
        // steps one unit or clump to neighbour `best`: into it if it is open, otherwise attacking
        // whatever stands there
        let step = |u: &mut Unit, best: i32, s: &mut u64| {
            if best < 0 {
                return;
            }
            let k = best as usize;
            let nx = cx + DX[k];
            let ny = cy + DY[k];
            let nc = ny as usize * W + nx as usize;
            let o = obj[nc];
            if o >= 0 {
                if let Some(a) = inp.sd.get(o as usize) {
                    a.fetch_add((st.atk * dt * DMG_SCALE * u.n as f32) as u32, Relaxed);
                }
            } else if o <= -2 && o != SOLID {
                if let Some(a) = inp.bd.get((-o - 2) as usize) {
                    a.fetch_add((st.atk * dt * DMG_SCALE * u.n as f32) as u32, Relaxed);
                }
            } else if o == FREE {
                // (an f32 like 1535.99999 can round up to the next cell, so stay inside it)
                let r = xr(s);
                let fr = |b: u32| ((r >> b) & 0xffff) as f32 * (1.0 / 65536.0);
                let tx = (nx as f32 + fr(0)).min(nx as f32 + 0.999);
                let ty = (ny as f32 + fr(16)).min(ny as f32 + 0.999);
                let (dx, dy) = (tx - x, ty - y);
                let len = (dx * dx + dy * dy).sqrt();
                let stp = spd * dt * (0.75 + 0.5 * fr(32));
                let (px, py) = if len > stp {
                    (x + dx / len * stp, y + dy / len * stp)
                } else {
                    (tx, ty)
                };
                let c2 = cl(py) * W + cl(px);
                if obj[c2] == FREE {
                    u.x = px;
                    u.y = py;
                }
            }
        };
        // single units here would part ways when two steps cost about the same (within the jitter).
        // Where the crowd is thin enough to see single units, the clump then breaks up into single
        // units, each taking its own step; inside a packed crowd it keeps together (merging keeps its
        // share of the cell small, so the crowd still spreads out much as single units would).
        if u.n > 1 && follow && !packed {
            let (mut b1, mut b2) = (f32::INFINITY, f32::INFINITY);
            for &v in &cs {
                if v < b1 {
                    b2 = b1;
                    b1 = v;
                } else if v < b2 {
                    b2 = v;
                }
            }
            if b2 < f32::INFINITY && b2 - b1 < st.jit {
                for _ in 1..u.n {
                    let r = xr(&mut s);
                    let mut q = Unit { n: 1, hp: u.hp * (0.9 + 0.2 * (r & 0xffff) as f32 / 65536.0), ..*u };
                    step(&mut q, pick(&cs, &k.jt[(r >> 56) as usize]), &mut s);
                    out.push(q);
                }
                u.n = 1;
            }
        }
        step(u, best, &mut s);
    }
    killed
}

/// Merges the units sharing each packed cell of a tile into clumps, and breaks clumps that are too
/// big for their cell into smaller ones, so a clump never carries more than its share of the cell
/// (see the module notes; without that a crowd that thins out a little would keep moving in big
/// lumps and stop spreading out the way single units do). Units merged into another become holes;
/// returns how many.
fn merge_units(t: usize, us: &mut Vec<Unit>, prev: &[AtomicU16], seed: u64) -> u32 {
    let (x0, y0) = ((t % TW) * TS, (t / TW) * TS);
    // members per cell, counted from the units themselves
    let mut cnt = [0u32; TS * TS];
    let mut clumps = false;
    for u in us.iter() {
        if u.fl & (F_GONE | F_DOWN) == 0 {
            cnt[(cl(u.y) - y0) * TS + cl(u.x) - x0] += u.n as u32;
            clumps |= u.n > 1;
        }
    }
    // the biggest clump each cell holds: its share of the cell's members, where the 3x3 cells
    // around it were packed last tick (elsewhere single units, apart from clumps already formed)
    let mut cap = [1u16; TS * TS];
    let mut merge = [0u8; TS * TS];
    let mut any = clumps;
    for ly in 0..TS {
        for lx in 0..TS {
            let i = ly * TS + lx;
            cap[i] = (cnt[i] / CLUMPS_PER_CELL).clamp(1, MAX_CLUMP as u32) as u16;
            if cap[i] < 2 {
                continue;
            }
            let (x, y) = (x0 + lx, y0 + ly);
            let mut near = 0;
            let mut least = u32::MAX;
            for yy in y.saturating_sub(1)..=(y + 1).min(H - 1) {
                for xx in x.saturating_sub(1)..=(x + 1).min(W - 1) {
                    let v = prev[yy * W + xx].load(Relaxed) as u32;
                    near += v;
                    least = least.min(v);
                }
            }
            if least < MERGE_LEAST {
                continue;
            }
            merge[i] = (near >= MERGE_AT * 9) as u8 + (near >= FLY_MERGE_AT * 9) as u8;
            any |= merge[i] != 0;
        }
    }
    if !any {
        return 0;
    }
    let mut s = seed;
    // per cell and kind of unit (flier, revived zombie, army in chaos), the clump taking in members
    let mut open = [[u32::MAX; TS * TS]; 16];
    let mut gone = 0;
    for i in 0..us.len() {
        let u = us[i];
        if u.fl & (F_GONE | F_DOWN) != 0 {
            continue;
        }
        let ci = (cl(u.y) - y0) * TS + cl(u.x) - x0;
        let cp = cap[ci];
        if u.n > cp {
            // too big for how packed its cell is: break off pieces of the cell's clump size (they
            // land at the end of the list, out of this pass)
            let (cx, cy) = (cl(u.x) as f32, cl(u.y) as f32);
            let mut left = u.n - cp;
            us[i].n = cp;
            while left > 0 {
                let k = left.min(cp);
                left -= k;
                let r = xr(&mut s);
                let fr = |b: u32| ((r >> b) & 0xffff) as f32 * (1.0 / 65536.0);
                us.push(Unit {
                    x: (cx + fr(0)).min(cx + 0.999),
                    y: (cy + fr(16)).min(cy + 0.999),
                    n: k,
                    ..u
                });
            }
            continue;
        }
        let fly = u.fl & F_FLY != 0;
        if merge[ci] < 1 + fly as u8 || u.n == cp || u.tg != 0 {
            continue;
        }
        let kind = (fly as usize) | (((u.fl & F_REVIVED != 0) as usize) << 1) | (kind_of(u.fl) << 2);
        let o = open[kind][ci];
        if o != u32::MAX && us[o as usize].n + u.n <= cp {
            let a = &mut us[o as usize];
            let (na, nb) = (a.n as f32, u.n as f32);
            a.hp = (a.hp * na + u.hp * nb) / (na + nb);
            a.aux = (a.aux * na + u.aux * nb) / (na + nb);
            a.n += u.n;
            us[i].fl |= F_GONE;
            gone += 1;
        } else {
            open[kind][ci] = i as u32;
        }
    }
    gone
}

impl Swarm {
    pub fn new() -> Self {
        let mk = |n: usize| (0..n).map(|_| AtomicU32::new(0)).collect::<Vec<_>>();
        Swarm {
            tiles: (0..NT).map(|_| Vec::new()).collect(),
            out: (0..NT).map(|_| Vec::new()).collect(),
            holes: vec![0; NT],
            seen: vec![0; NT],
            members: vec![0; NT],
            last: vec![0; NT],
            count: 0,
            dens: (0..N).map(|_| AtomicU16::new(0)).collect(),
            prev: (0..N).map(|_| AtomicU16::new(0)).collect(),
            tint: (0..N).map(|_| AtomicU8::new(0)).collect(),
            coarse: mk(CW * CH),
            deaths: mk(CW * CH),
            deaths_prev: mk(CW * CH),
            dmg: vec![0.0; N],
            dsrc: vec![0; N],
            touched: Vec::new(),
            tick: 0,
        }
    }
    pub fn len(&self) -> usize {
        self.count
    }
    /// Every unit and clump, in a fixed order (tile by tile).
    pub fn units(&self) -> impl Iterator<Item = &Unit> {
        self.tiles.iter().flatten().filter(|u| u.fl & F_GONE == 0)
    }
    /// Units and clumps actually stepped (`len` counts every member).
    pub fn entities(&self) -> usize {
        self.tiles.iter().zip(&self.holes).map(|(v, &h)| v.len() - h as usize).sum()
    }
    /// Ticks stepped so far.
    pub fn tick(&self) -> u64 {
        self.tick
    }
    /// The last tick tile `t` had units: its cells are unchanged since any picture of the horde
    /// taken after that.
    #[inline]
    pub fn last_seen(&self, t: usize) -> u64 {
        self.last[t]
    }

    #[allow(clippy::too_many_arguments)]
    pub fn spawn(
        &mut self,
        at: (f32, f32),
        count: usize,
        obj: &[i32],
        hq: &[u32],
        hp: f32,
        seed: u64,
        fl: u8,
        aux: f32,
    ) {
        let mut s = seed | 1;
        let rad = (count as f32).sqrt().max(4.0) * 0.45;
        self.count += count;
        for _ in 0..count {
            let (mut x, mut y) = at;
            for _ in 0..5 {
                let a = xs(&mut s) * std::f32::consts::TAU;
                let r = xs(&mut s) * rad;
                let px = at.0 + libm::cosf(a) * r;
                let py = at.1 + libm::sinf(a) * r;
                if px >= 0.0 && py >= 0.0 && px < W as f32 && py < H as f32 {
                    let c = cl(py) * W + cl(px);
                    if fl & F_FLY != 0 || (obj[c] == FREE && hq[c] != INF) {
                        x = px;
                        y = py;
                        break;
                    }
                }
            }
            let u = Unit {
                x,
                y,
                hp: hp * (0.8 + 0.4 * xs(&mut s)),
                aux,
                tg: 0,
                hd: (xs(&mut s) * 8.0) as u8,
                fl,
                n: 1,
            };
            let t = tile_of(x, y);
            self.tiles[t].push(u);
            self.members[t] += 1;
        }
    }

    /// Queues damage on every cell within `r` of a point. `src` names the attacker (0 = nobody in particular).
    pub fn add_damage(&mut self, x: f32, y: f32, r: f32, d: f32, src: u16) {
        let dmg = &mut self.dmg;
        let ds = &mut self.dsrc;
        let t = &mut self.touched;
        fill_disc(vec2(x, y), r.max(0.8), |c| {
            if dmg[c] == 0.0 {
                t.push(c as u32);
            }
            dmg[c] += d;
            if src != 0 {
                ds[c] = src;
            }
        });
    }

    /// Advances every unit one tick. Returns the number killed and the zombie deaths that call for revenge.
    pub fn step(&mut self, inp: &StepIn) -> (u32, Vec<Death>) {
        self.tick += 1;
        std::mem::swap(&mut self.dens, &mut self.prev);
        std::mem::swap(&mut self.deaths, &mut self.deaths_prev);
        self.deaths.par_iter().for_each(|a| a.store(0, Relaxed));
        let jt = jitter(self.tick.wrapping_mul(0xD6E8_FEB8_6659_FD93), inp.stats.jit);
        let k = Ctx {
            jt: &jt,
            inp,
            prev: &self.prev,
            dprev: &self.deaths_prev,
            deaths: &self.deaths,
            dmg: &self.dmg,
            dsrc: &self.dsrc,
            any_dmg: !self.touched.is_empty(),
            tick: self.tick,
            merge: std::env::var("PH_NOCLUMP").is_err(),
        };
        let gr = Grids {
            dens: &self.dens,
            tint: &self.tint,
            coarse: &self.coarse,
        };
        let tm = if inp.mixed.is_some() {
            Tm::Mixed
        } else if inp.stats.mode == Behaviour::Orc {
            Tm::Orc
        } else {
            Tm::Plain
        };
        // passes 1 and 2: step and settle, one task per tile. Busy tiles fan out into runs of CHUNK
        // units, each run stepping and settling on its own core, and then add up their counts.
        let res: Vec<(u32, Vec<Death>)> = self
            .tiles
            .par_iter_mut()
            .zip(self.out.par_iter_mut())
            .zip(self.seen.par_iter_mut())
            .zip(self.holes.par_iter_mut())
            .zip(self.members.par_iter_mut())
            .enumerate()
            .map(|(t, ((((us, out), seen), holes), members))| {
                out.clear();
                if us.is_empty() {
                    if *seen != 0 {
                        gr.clear(t);
                    }
                    *seen = (*seen << 1) & 3;
                    *members = 0;
                    return (0, Vec::new());
                }
                let base = (k.tick.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (t as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9)) | 1;
                let seed = |ci: usize| (base ^ (ci as u64).wrapping_mul(0x94D0_49BB_1331_11EB)) | 1;
                // tiles take turns merging their packed cells into clumps (and resizing clumps)
                if k.merge && (k.tick + t as u64) % MERGE_EVERY == 0 {
                    *holes += merge_units(t, us, k.prev, seed(usize::MAX));
                }
                // sweep out the holes once they take up a quarter of the list
                if *holes as usize * 4 > us.len() {
                    us.retain(|u| u.fl & F_GONE == 0);
                }
                let mut ev = Vec::new();
                let mut acc = Acc::new();
                let (killed, h, m) = if us.len() <= CHUNK {
                    let kc = step_units(&k, t, seed(0), us, &mut ev, out);
                    let (kl, h, m) = settle_units(t, us, &mut acc, out, tm);
                    (kl + kc, h, m)
                } else {
                    let parts: Vec<(Acc, Vec<Death>, Vec<Unit>, u32, u32, u64)> = us
                        .par_chunks_mut(CHUNK)
                        .enumerate()
                        .map(|(ci, c)| {
                            let (mut a, mut e, mut o) = (Acc::new(), Vec::new(), Vec::new());
                            let kc = step_units(&k, t, seed(ci), c, &mut e, &mut o);
                            let (kl, h, m) = settle_units(t, c, &mut a, &mut o, tm);
                            (a, e, o, kl + kc, h, m)
                        })
                        .collect();
                    let (mut kl, mut h, mut m) = (0, 0, 0);
                    for (a, e, o, k2, h2, m2) in parts {
                        acc.add(&a);
                        ev.extend(e);
                        out.extend(o);
                        kl += k2;
                        h += h2;
                        m += m2;
                    }
                    (kl, h, m)
                };
                *holes = h;
                *members = m;
                if h as usize == us.len() {
                    us.clear();
                    *holes = 0;
                }
                gr.write(t, &acc);
                *seen = ((*seen << 1) | 1) & 3;
                (killed, ev)
            })
            .collect();
        // pass 3: hand-over. Each tile takes in the units that walked over from its neighbours (and the
        // units split off its own clumps).
        let out = &self.out;
        let moved: usize = self
            .tiles
            .par_iter_mut()
            .zip(self.seen.par_iter_mut())
            .zip(self.members.par_iter_mut())
            .enumerate()
            .map(|(t, ((us, seen), members))| {
                let (tx, ty) = ((t % TW) as i32, (t / TW) as i32);
                let mut n = 0;
                for k in 0..9 {
                    let (nx, ny) = if k == 8 { (tx, ty) } else { (tx + DX[k], ty + DY[k]) };
                    if nx < 0 || ny < 0 || nx >= TW as i32 || ny >= TH as i32 {
                        continue;
                    }
                    for u in &out[ny as usize * TW + nx as usize] {
                        if tile_of(u.x, u.y) == t {
                            gr.mark(u, tm);
                            us.push(*u);
                            *members += u.n as u64;
                            n += 1;
                        }
                    }
                }
                if n > 0 {
                    *seen |= 1;
                }
                n
            })
            .sum();
        let left: usize = self.out.iter().map(|o| o.len()).sum();
        if moved != left {
            // something jumped further than one tile (never in normal play): place it directly
            for t in 0..NT {
                let (tx, ty) = ((t % TW) as i32, (t / TW) as i32);
                for i in 0..self.out[t].len() {
                    let u = self.out[t][i];
                    let d = tile_of(u.x, u.y);
                    let (dx, dy) = ((d % TW) as i32 - tx, (d / TW) as i32 - ty);
                    if dx.abs() > 1 || dy.abs() > 1 {
                        gr.mark(&u, tm);
                        self.seen[d] |= 1;
                        self.members[d] += u.n as u64;
                        self.tiles[d].push(u);
                    }
                }
            }
        }
        let tick = self.tick;
        for (l, s) in self.last.iter_mut().zip(&self.seen) {
            if s & 1 != 0 {
                *l = tick;
            }
        }
        // clear the damage that was applied this tick
        for &c in &self.touched {
            self.dmg[c as usize] = 0.0;
            self.dsrc[c as usize] = 0;
        }
        self.touched.clear();
        let mut killed = 0;
        let mut events = Vec::new();
        for (kl, ev) in res {
            killed += kl;
            events.extend(ev);
        }
        self.count = self.members.iter().sum::<u64>() as usize;
        (killed, events)
    }

    #[inline]
    pub fn at(&self, c: usize) -> u32 {
        self.dens[c].load(Relaxed) as u32
    }
    #[inline]
    pub fn coarse_at(&self, i: usize) -> u32 {
        self.coarse[i].load(Relaxed)
    }
}

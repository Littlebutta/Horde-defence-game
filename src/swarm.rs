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
//! Nothing depends on how many threads there are: random seeds come from (tick, tile, chunk), chunk
//! sizes are fixed, and results are gathered in tile order, so co-op lockstep stays in sync.
//!
//! Faction quirks handled here:
//! * zombies go down once and get back up, and swarm whatever killed one of them (locked to that target);
//! * some alien bugs fly, ignoring city buildings and walls on their way to the HQ;
//! * orcs build rage when units around them die, which makes them faster (and redder).

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

/// Tint codes written per cell for the renderer (highest wins).
pub const TINT_DOWN: u8 = 1;
pub const TINT_NORMAL: u8 = 2;
pub const TINT_FLY: u8 = 3;
pub const TINT_RAGE: u8 = 4; // 4..=255 = rage level

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
    pub sd: &'a [AtomicU32],
    pub bd: &'a [AtomicU32],
    /// per structure: can fliers pass over it (walls)
    pub low: &'a [bool],
    /// coarse cells where a zombie just died: revenge target + 1
    pub lock: &'a [u16],
    pub rev: &'a [RevIn<'a>],
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

#[inline]
fn tint_of(u: &Unit, orc: bool) -> u8 {
    if u.fl & F_DOWN != 0 {
        TINT_DOWN
    } else if u.fl & F_FLY != 0 {
        TINT_FLY
    } else if orc {
        TINT_RAGE + (u.aux * 251.0) as u8
    } else {
        TINT_NORMAL
    }
}

/// Right after a run of units has moved (while it is still in cache): drops the dead and the leavers
/// (they become holes, leavers also go to `out`) and counts the rest into `acc`. Returns the kills.
fn settle_units(t: usize, us: &mut [Unit], acc: &mut Acc, out: &mut Vec<Unit>, orc: bool) -> (u32, u32) {
    let (x0, y0) = ((t % TW) * TS, (t / TW) * TS);
    let mut killed = 0;
    let mut holes = 0;
    for u in us.iter_mut() {
        if u.fl & F_GONE != 0 {
            holes += 1;
            continue;
        }
        // zombies that just went down count as kills too, but stay (lying down)
        let just_down = u.hd & 0x80 != 0;
        if just_down {
            killed += 1;
            u.hd &= 0x7f;
        }
        if u.hp <= 0.0 {
            killed += 1;
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
        if just_down {
            continue;
        }
        let i = ly * TS + lx;
        if u.fl & F_DOWN == 0 {
            acc.dens[i] = acc.dens[i].saturating_add(1);
        }
        acc.tint[i] = acc.tint[i].max(tint_of(u, orc));
    }
    // coarse counts from the cell counts
    for ly in 0..TS {
        for lx in 0..TS {
            acc.coarse[(ly / CG) * TC + lx / CG] += acc.dens[ly * TS + lx] as u32;
        }
    }
    (killed, holes)
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
    fn mark(&self, u: &Unit, orc: bool) {
        let c = cl(u.y) * W + cl(u.x);
        if u.fl & F_DOWN == 0 {
            let d = &self.dens[c];
            d.store(d.load(Relaxed).saturating_add(1), Relaxed);
            let g = &self.coarse[(cl(u.y) / CG) * CW + cl(u.x) / CG];
            g.store(g.load(Relaxed) + 1, Relaxed);
        }
        let t = &self.tint[c];
        t.store(t.load(Relaxed).max(tint_of(u, orc)), Relaxed);
    }
}

/// Units per cell above which a tile caches its cells' neighbour costs before stepping.
const PACKED: usize = 2;

/// Per cell of one tile, the cost of stepping to each neighbour along the cell's flow field
/// (field value plus crowding, before jitter), or infinity where the step is blocked. In a packed
/// crowd hundreds of units share a cell, so this is worked out once per cell instead of per unit.
/// It does the same float sums as `descend`, so units move exactly as they would without it.
fn cell_costs(k: &Ctx, t: usize, tab: &mut [[f32; 8]; TS * TS]) {
    let inp = k.inp;
    let zombie = inp.stats.mode == Behaviour::Zombie;
    let use_hunt = inp.hunt.is_some() && !(zombie && inp.rush);
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

fn step_units(k: &Ctx, t: usize, seed: u64, us: &mut [Unit], ev: &mut Vec<Death>) {
    let inp = k.inp;
    let st = inp.stats;
    let zombie = st.mode == Behaviour::Zombie;
    let orc = st.mode == Behaviour::Orc;
    let use_hunt = inp.hunt.is_some() && !(zombie && inp.rush);
    let wander = zombie && !inp.rush;
    let (obj, nb, hq, hunt) = (inp.obj, inp.nb, inp.hq, inp.hunt.unwrap_or(&[]));
    let (prev, dprev, deaths, dmg, dsrc, any_dmg) = (k.prev, k.dprev, k.deaths, k.dmg, k.dsrc, k.any_dmg);
    let dk = DK;
    let dt = inp.dt;
    let mut s = seed;
    let mut tab_buf = None;
    if us.len() > PACKED * TS * TS && (use_hunt || !wander) {
        let mut b = Box::new([[0f32; 8]; TS * TS]);
        cell_costs(k, t, &mut b);
        tab_buf = Some(b);
    }
    let tab = tab_buf.as_deref();
    // orcs: deaths around each coarse cell of this tile last tick (3x3 coarse cells), worked out once
    let mut rage_near = [0u32; TC * TC];
    if orc {
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
        let mut spd = st.spd;
        if orc {
            let near = rage_near[((cy - y0) as usize / CG) * TC + (cx - x0) as usize / CG];
            u.aux =
                ((u.aux - dt * 0.015) + near.min(200) as f32 * 0.004).clamp(0.0, 1.0);
            spd *= 1.0 + u.aux * 1.2;
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
                    a.fetch_add((st.atk * dt * DMG_SCALE) as u32, Relaxed);
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
                    let d = r.pos - vec2(x, y);
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
                        let v = -(DX[k] as f32 * d.x + DY[k] as f32 * d.y)
                            / (if k < 4 { 1.0 } else { 1.414 })
                            + xs(&mut s) * 4.0;
                        if v < bv {
                            bv = v;
                            best = k as i32;
                        }
                    }
                    field_hq = false;
                }
            }
        }
        if !field_hq {
        } else if let (Some(tab), false) = (tab, wander && !(use_hunt && hunt[c] < U16)) {
            best = pick(&tab[((cy - y0) * TS as i32 + cx - x0) as usize], &k.jt[(xr(&mut s) & 255) as usize]);
        } else if use_hunt && hunt[c] < U16 {
            best = descend(m, c, f32::MAX, |i| hunt[i] as u32, U16 as u32, prev, dk, &k.jt[(xr(&mut s) & 255) as usize]);
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
        }
        if best >= 0 {
            let k = best as usize;
            let nx = cx + DX[k];
            let ny = cy + DY[k];
            let nc = ny as usize * W + nx as usize;
            let o = obj[nc];
            if o >= 0 {
                if let Some(a) = inp.sd.get(o as usize) {
                    a.fetch_add((st.atk * dt * DMG_SCALE) as u32, Relaxed);
                }
            } else if o <= -2 && o != SOLID {
                if let Some(a) = inp.bd.get((-o - 2) as usize) {
                    a.fetch_add((st.atk * dt * DMG_SCALE) as u32, Relaxed);
                }
            } else if o == FREE {
                // (an f32 like 1535.99999 can round up to the next cell, so stay inside it)
                let r = xr(&mut s);
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
        }
    }
}

impl Swarm {
    pub fn new() -> Self {
        let mk = |n: usize| (0..n).map(|_| AtomicU32::new(0)).collect::<Vec<_>>();
        Swarm {
            tiles: (0..NT).map(|_| Vec::new()).collect(),
            out: (0..NT).map(|_| Vec::new()).collect(),
            holes: vec![0; NT],
            seen: vec![0; NT],
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
    /// Every unit, in a fixed order (tile by tile).
    pub fn units(&self) -> impl Iterator<Item = &Unit> {
        self.tiles.iter().flatten().filter(|u| u.fl & F_GONE == 0)
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
            };
            self.tiles[tile_of(x, y)].push(u);
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
        };
        let gr = Grids {
            dens: &self.dens,
            tint: &self.tint,
            coarse: &self.coarse,
        };
        let orc = inp.stats.mode == Behaviour::Orc;
        // passes 1 and 2: step and settle, one task per tile. Busy tiles fan out into runs of CHUNK
        // units, each run stepping and settling on its own core, and then add up their counts.
        let res: Vec<(u32, Vec<Death>)> = self
            .tiles
            .par_iter_mut()
            .zip(self.out.par_iter_mut())
            .zip(self.seen.par_iter_mut())
            .zip(self.holes.par_iter_mut())
            .enumerate()
            .map(|(t, (((us, out), seen), holes))| {
                out.clear();
                if us.is_empty() {
                    if *seen != 0 {
                        gr.clear(t);
                    }
                    *seen = (*seen << 1) & 3;
                    return (0, Vec::new());
                }
                // sweep out the holes once they take up a quarter of the list
                if *holes as usize * 4 > us.len() {
                    us.retain(|u| u.fl & F_GONE == 0);
                }
                let base = (k.tick.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (t as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9)) | 1;
                let seed = |ci: usize| (base ^ (ci as u64).wrapping_mul(0x94D0_49BB_1331_11EB)) | 1;
                let mut ev = Vec::new();
                let mut acc = Acc::new();
                let (killed, h) = if us.len() <= CHUNK {
                    step_units(&k, t, seed(0), us, &mut ev);
                    settle_units(t, us, &mut acc, out, orc)
                } else {
                    let parts: Vec<(Acc, Vec<Death>, Vec<Unit>, u32, u32)> = us
                        .par_chunks_mut(CHUNK)
                        .enumerate()
                        .map(|(ci, c)| {
                            let (mut a, mut e, mut o) = (Acc::new(), Vec::new(), Vec::new());
                            step_units(&k, t, seed(ci), c, &mut e);
                            let (kl, h) = settle_units(t, c, &mut a, &mut o, orc);
                            (a, e, o, kl, h)
                        })
                        .collect();
                    let (mut kl, mut h) = (0, 0);
                    for (a, e, o, k2, h2) in parts {
                        acc.add(&a);
                        ev.extend(e);
                        out.extend(o);
                        kl += k2;
                        h += h2;
                    }
                    (kl, h)
                };
                *holes = h;
                if h as usize == us.len() {
                    us.clear();
                    *holes = 0;
                }
                gr.write(t, &acc);
                *seen = ((*seen << 1) | 1) & 3;
                (killed, ev)
            })
            .collect();
        // pass 3: hand-over. Each tile takes in the units that walked over from its neighbours.
        let out = &self.out;
        let moved: usize = self
            .tiles
            .par_iter_mut()
            .zip(self.seen.par_iter_mut())
            .enumerate()
            .map(|(t, (us, seen))| {
                let (tx, ty) = ((t % TW) as i32, (t / TW) as i32);
                let mut n = 0;
                for k in 0..8 {
                    let (nx, ny) = (tx + DX[k], ty + DY[k]);
                    if nx < 0 || ny < 0 || nx >= TW as i32 || ny >= TH as i32 {
                        continue;
                    }
                    for u in &out[ny as usize * TW + nx as usize] {
                        if tile_of(u.x, u.y) == t {
                            gr.mark(u, orc);
                            us.push(*u);
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
                        gr.mark(&u, orc);
                        self.seen[d] |= 1;
                        self.tiles[d].push(u);
                    }
                }
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
        self.count = self.tiles.iter().zip(&self.holes).map(|(v, &h)| v.len() - h as usize).sum();
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

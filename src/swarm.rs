//! The horde: millions of single-pixel units, updated in parallel on every CPU core.
//!
//! Units live in one flat array. Each tick the array is cut into chunks and rayon spreads the chunks
//! across all threads. Every unit reads shared, read-only grids (object grid, flow fields, last tick's
//! crowd density, the damage grid) and writes only its own slot, plus atomic increments into the new
//! density grid and the damage tallies for whatever it is attacking.
//!
//! Faction quirks handled here:
//! * zombies go down once and get back up, and swarm whatever killed one of them (locked to that target);
//! * some alien bugs fly, ignoring city buildings and walls on their way to the HQ;
//! * orcs build rage when units around them die, which makes them faster (and redder).

use crate::fields::{DX, DY, INF, U16};
use crate::world::*;
use macroquad::prelude::{vec2, Vec2};
use rayon::prelude::*;
use std::sync::atomic::{AtomicU32, AtomicU8, Ordering::Relaxed};

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
    pub u: Vec<Unit>,
    pub dens: Vec<AtomicU32>,
    pub prev: Vec<AtomicU32>,
    pub tint: Vec<AtomicU8>,
    pub coarse: Vec<AtomicU32>,
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

impl Swarm {
    pub fn new() -> Self {
        let mk = |n: usize| (0..n).map(|_| AtomicU32::new(0)).collect::<Vec<_>>();
        Swarm {
            u: Vec::new(),
            dens: mk(N),
            prev: mk(N),
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
        self.u.len()
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
        self.u.reserve(count);
        for _ in 0..count {
            let (mut x, mut y) = at;
            for _ in 0..5 {
                let a = xs(&mut s) * std::f32::consts::TAU;
                let r = xs(&mut s) * rad;
                let px = at.0 + a.cos() * r;
                let py = at.1 + a.sin() * r;
                if px >= 0.0 && py >= 0.0 && px < W as f32 && py < H as f32 {
                    let c = py as usize * W + px as usize;
                    if fl & F_FLY != 0 || (obj[c] == FREE && hq[c] != INF) {
                        x = px;
                        y = py;
                        break;
                    }
                }
            }
            self.u.push(Unit {
                x,
                y,
                hp: hp * (0.8 + 0.4 * xs(&mut s)),
                aux,
                tg: 0,
                hd: (xs(&mut s) * 8.0) as u8,
                fl,
            });
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
        self.dens.par_iter().for_each(|a| a.store(0, Relaxed));
        self.tint.par_iter().for_each(|a| a.store(0, Relaxed));
        self.coarse.par_iter().for_each(|a| a.store(0, Relaxed));
        self.deaths.par_iter().for_each(|a| a.store(0, Relaxed));
        let st = inp.stats;
        let zombie = st.mode == Behaviour::Zombie;
        let orc = st.mode == Behaviour::Orc;
        let use_hunt = inp.hunt.is_some() && !(zombie && inp.rush);
        let wander = zombie && !inp.rush;
        let (obj, hq, hunt) = (inp.obj, inp.hq, inp.hunt.unwrap_or(&[]));
        let (dens, prev, tint, coarse, deaths, dprev, dmg, dsrc) = (
            &self.dens,
            &self.prev,
            &self.tint,
            &self.coarse,
            &self.deaths,
            &self.deaths_prev,
            &self.dmg,
            &self.dsrc,
        );
        let dk = 6.0f32;
        let tick = self.tick;
        let dt = inp.dt;
        const CHUNK: usize = 8192;
        let events: Vec<Vec<Death>> = self
            .u
            .par_chunks_mut(CHUNK)
            .enumerate()
            .map(|(ci, us)| {
                let mut ev = Vec::new();
                let mut s: u64 = (tick.wrapping_mul(0x9E37_79B9_7F4A_7C15)
                    ^ (ci as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9))
                    | 1;
                for u in us.iter_mut() {
                    let (x, y) = (u.x, u.y);
                    let cx = x as i32;
                    let cy = y as i32;
                    let mut c = cy as usize * W + cx as usize;
                    let gi = (cy as usize / CG) * CW + cx as usize / CG;
                    // zombies lying down get back up once
                    if u.fl & F_DOWN != 0 {
                        u.aux -= dt;
                        if u.aux <= 0.0 {
                            u.fl = (u.fl & !F_DOWN) | F_REVIVED;
                            u.hp = st.hp;
                        }
                        tint[c].fetch_max(TINT_DOWN, Relaxed);
                        continue;
                    }
                    let dm = dmg[c];
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
                    let mut tcode = TINT_NORMAL;
                    if orc {
                        let mut near = 0u32;
                        let (gx, gy) = ((cx as usize / CG) as i32, (cy as usize / CG) as i32);
                        for oy in -1..=1 {
                            for ox in -1..=1 {
                                let (qx, qy) = (gx + ox, gy + oy);
                                if qx >= 0 && qy >= 0 && qx < CW as i32 && qy < CH as i32 {
                                    near += dprev[qy as usize * CW + qx as usize].load(Relaxed);
                                }
                            }
                        }
                        u.aux =
                            ((u.aux - dt * 0.015) + near.min(200) as f32 * 0.004).clamp(0.0, 1.0);
                        spd *= 1.0 + u.aux * 1.2;
                        tcode = TINT_RAGE + (u.aux * 251.0) as u8;
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
                        let c2 = py as usize * W + px as usize;
                        let o = obj[c2];
                        if o >= 0 && !inp.low.get(o as usize).copied().unwrap_or(true) {
                            if let Some(a) = inp.sd.get(o as usize) {
                                a.fetch_add((st.atk * dt * DMG_SCALE) as u32, Relaxed);
                            }
                        } else {
                            u.x = px;
                            u.y = py;
                            c = c2;
                        }
                        dens[c].fetch_add(1, Relaxed);
                        tint[c].fetch_max(TINT_FLY, Relaxed);
                        coarse[(c / W / CG) * CW + (c % W) / CG].fetch_add(1, Relaxed);
                        continue;
                    }
                    let free_diag = |k: usize, nx: i32, ny: i32| -> bool {
                        k < 4
                            || (obj[cy as usize * W + nx as usize] == FREE
                                && obj[ny as usize * W + cx as usize] == FREE)
                    };
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
                                let mut bv = f[c] as f32 + 6.0;
                                for k in 0..8 {
                                    let nx = cx + DX[k];
                                    let ny = cy + DY[k];
                                    if nx < 0 || ny < 0 || nx >= W as i32 || ny >= H as i32 {
                                        continue;
                                    }
                                    let nc = ny as usize * W + nx as usize;
                                    let fv = f[nc];
                                    if fv == INF || !free_diag(k, nx, ny) {
                                        continue;
                                    }
                                    let v = fv as f32
                                        + prev[nc].load(Relaxed) as f32 * 2.0
                                        + xs(&mut s) * st.jit;
                                    if v < bv {
                                        bv = v;
                                        best = k as i32;
                                    }
                                }
                                field_hq = false;
                            }
                            _ => {
                                // no field yet: head straight at it
                                let d = r.pos - vec2(x, y);
                                let mut bv = f32::MAX;
                                for k in 0..8 {
                                    let nx = cx + DX[k];
                                    let ny = cy + DY[k];
                                    if nx < 0 || ny < 0 || nx >= W as i32 || ny >= H as i32 {
                                        continue;
                                    }
                                    let o = obj[ny as usize * W + nx as usize];
                                    if o == SOLID || o <= -2 || !free_diag(k, nx, ny) {
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
                    } else if use_hunt && hunt[c] < U16 {
                        let mut bv = f32::MAX;
                        for k in 0..8 {
                            let nx = cx + DX[k];
                            let ny = cy + DY[k];
                            if nx < 0 || ny < 0 || nx >= W as i32 || ny >= H as i32 {
                                continue;
                            }
                            let nc = ny as usize * W + nx as usize;
                            let fv = hunt[nc];
                            if fv == U16 || !free_diag(k, nx, ny) {
                                continue;
                            }
                            let v = fv as f32
                                + prev[nc].load(Relaxed) as f32 * dk
                                + xs(&mut s) * st.jit;
                            if v < bv {
                                bv = v;
                                best = k as i32;
                            }
                        }
                    } else if wander {
                        let mut k = (u.hd & 7) as usize;
                        if xs(&mut s) < 0.03 {
                            k = (xs(&mut s) * 8.0) as usize & 7;
                        }
                        for _ in 0..3 {
                            let nx = cx + DX[k];
                            let ny = cy + DY[k];
                            if nx >= 0
                                && ny >= 0
                                && nx < W as i32
                                && ny < H as i32
                                && obj[ny as usize * W + nx as usize] == FREE
                                && free_diag(k, nx, ny)
                            {
                                best = k as i32;
                                break;
                            }
                            k = (xs(&mut s) * 8.0) as usize & 7;
                        }
                        u.hd = (u.hd & 0x80) | k as u8;
                    } else {
                        let mut bv = f32::MAX;
                        for k in 0..8 {
                            let nx = cx + DX[k];
                            let ny = cy + DY[k];
                            if nx < 0 || ny < 0 || nx >= W as i32 || ny >= H as i32 {
                                continue;
                            }
                            let nc = ny as usize * W + nx as usize;
                            let fv = hq[nc];
                            if fv == INF || !free_diag(k, nx, ny) {
                                continue;
                            }
                            let v = fv as f32
                                + prev[nc].load(Relaxed) as f32 * dk
                                + xs(&mut s) * st.jit;
                            if v < bv {
                                bv = v;
                                best = k as i32;
                            }
                        }
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
                            let tx = nx as f32 + xs(&mut s);
                            let ty = ny as f32 + xs(&mut s);
                            let (dx, dy) = (tx - x, ty - y);
                            let len = (dx * dx + dy * dy).sqrt();
                            let stp = spd * dt * (0.75 + 0.5 * xs(&mut s));
                            let (px, py) = if len > stp {
                                (x + dx / len * stp, y + dy / len * stp)
                            } else {
                                (tx, ty)
                            };
                            let c2 = py as usize * W + px as usize;
                            if obj[c2] == FREE {
                                u.x = px;
                                u.y = py;
                                c = c2;
                            }
                        }
                    }
                    dens[c].fetch_add(1, Relaxed);
                    tint[c].fetch_max(tcode, Relaxed);
                    coarse[(c / W / CG) * CW + (c % W) / CG].fetch_add(1, Relaxed);
                }
                ev
            })
            .collect();
        // clear the damage that was applied this tick
        for &c in &self.touched {
            self.dmg[c as usize] = 0.0;
            self.dsrc[c as usize] = 0;
        }
        self.touched.clear();
        // compact out the dead; zombies that just went down count as kills too
        let n = self.u.len();
        let mut j = 0;
        let mut killed = 0u32;
        for i in 0..n {
            let mut u = self.u[i];
            if u.hd & 0x80 != 0 {
                killed += 1;
                u.hd &= 0x7f;
            }
            if u.hp > 0.0 {
                self.u[j] = u;
                j += 1;
            } else {
                killed += 1;
            }
        }
        self.u.truncate(j);
        (killed, events.into_iter().flatten().collect())
    }

    #[inline]
    pub fn at(&self, c: usize) -> u32 {
        self.dens[c].load(Relaxed)
    }
    #[inline]
    pub fn coarse_at(&self, i: usize) -> u32 {
        self.coarse[i].load(Relaxed)
    }
}

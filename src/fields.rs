//! Flow fields (bucketed Dijkstra / Dial's algorithm), computed on a background thread so
//! path updates never stall a frame.
//!
//! * `Mode::Hq`: the field every horde follows to the HQ. City buildings and player structures are
//!   passable at a high cost, so the horde routes around when it can and chews through when it can't.
//! * `Mode::Open`: open ground only, from many sources, cut off at a maximum cost (what a unit "sees").
//!
//! Diagonal steps between two blocked cells are refused, so a wall line with touching corners seals a street.
//!
//! Results are handed back on a fixed tick (`LAG` ticks after the job was sent), waiting for the
//! worker if it is late. That keeps the simulation deterministic, which multiplayer relies on:
//! every machine runs the same game from the same commands.

use crate::world::*;
use std::collections::VecDeque;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

pub const INF: u32 = u32::MAX;
pub const U16: u16 = u16::MAX;
pub const DX: [i32; 8] = [1, -1, 0, 0, 1, 1, -1, -1];
pub const DY: [i32; 8] = [0, 0, 1, -1, 1, -1, 1, -1];
const CS: [u32; 8] = [10, 10, 10, 10, 14, 14, 14, 14];

#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    Hq,
    Open,
}

pub fn dial(obj: &[i32], src: &[u32], mode: Mode, max: u32) -> Vec<u32> {
    let mut d = vec![INF; N];
    const B: usize = 2048;
    let mut bk: Vec<Vec<u32>> = (0..B).map(|_| Vec::new()).collect();
    let mut pending = 0usize;
    for &c in src {
        let c = c as usize;
        if c < N && d[c] != 0 {
            d[c] = 0;
            bk[0].push(c as u32);
            pending += 1;
        }
    }
    let mut cur: u32 = 0;
    while pending > 0 && cur <= max {
        let bi = cur as usize % B;
        while let Some(c) = bk[bi].pop() {
            pending -= 1;
            let c = c as usize;
            if d[c] != cur {
                continue;
            }
            let cx = (c % W) as i32;
            let cy = (c / W) as i32;
            for k in 0..8 {
                let nx = cx + DX[k];
                let ny = cy + DY[k];
                if nx < 0 || ny < 0 || nx >= W as i32 || ny >= H as i32 {
                    continue;
                }
                let nc = ny as usize * W + nx as usize;
                let o = obj[nc];
                if o == SOLID {
                    continue;
                }
                let mut extra = 0;
                if o != FREE {
                    if mode != Mode::Hq {
                        continue;
                    }
                    extra = if o >= 0 { 400 } else { 1500 };
                }
                if k >= 4
                    && (obj[cy as usize * W + nx as usize] != FREE
                        || obj[ny as usize * W + cx as usize] != FREE)
                {
                    continue;
                }
                let nd = cur + CS[k] + extra;
                if nd < d[nc] {
                    d[nc] = nd;
                    bk[nd as usize % B].push(nc as u32);
                    pending += 1;
                }
            }
        }
        cur += 1;
    }
    d
}

/// For every cell, a bit per direction (in `DX`/`DY` order) that is set when a unit may step that
/// way as far as the grid goes: inside the map, and for diagonals not squeezing between two blocked
/// cells. Whether the target cell itself is open is left to the flow field or object grid.
pub fn neighbour_mask(obj: &[i32]) -> Vec<u8> {
    use rayon::prelude::*;
    let mut m = vec![0u8; N];
    m.par_chunks_mut(W).enumerate().for_each(|(y, row)| {
        let y = y as i32;
        for (x, v) in row.iter_mut().enumerate() {
            let x = x as i32;
            let mut bits = 0u8;
            for k in 0..8 {
                let (nx, ny) = (x + DX[k], y + DY[k]);
                if nx < 0 || ny < 0 || nx >= W as i32 || ny >= H as i32 {
                    continue;
                }
                if k >= 4 && (obj[y as usize * W + nx as usize] != FREE || obj[ny as usize * W + x as usize] != FREE) {
                    continue;
                }
                bits |= 1 << k;
            }
            *v = bits;
        }
    });
    m
}

pub enum Job {
    Hq {
        obj: Arc<Vec<i32>>,
        src: Vec<u32>,
    },
    Hunt {
        obj: Arc<Vec<i32>>,
        src: Vec<u32>,
        max: u32,
    },
    Target {
        id: u64,
        obj: Arc<Vec<i32>>,
        src: u32,
    },
    /// open-ground field from several cells (pads, portals, revenge targets, civilians to collect)
    Multi {
        id: u64,
        obj: Arc<Vec<i32>>,
        src: Vec<u32>,
        max: u32,
    },
}
pub enum Done {
    Hq {
        obj: Arc<Vec<i32>>,
        d: Arc<Vec<u32>>,
        nb: Arc<Vec<u8>>,
    },
    Hunt(Arc<Vec<u16>>),
    Target {
        id: u64,
        d: Arc<Vec<u32>>,
    },
    Multi {
        id: u64,
        d: Arc<Vec<u32>>,
    },
}

/// Ticks between sending a job and using its result.
pub const LAG: u64 = 8;

pub struct FieldWorker {
    tx: Sender<Job>,
    rx: Receiver<Done>,
    pub busy: usize,
    /// the current simulation tick, set by the game
    pub now: u64,
    /// alone (no lockstep to keep), a late result is picked up a tick later instead of the
    /// simulation waiting for it
    pub relaxed: bool,
    due: VecDeque<u64>,
}

impl FieldWorker {
    pub fn new() -> Self {
        let (tx, jrx) = channel::<Job>();
        let (dtx, rx) = channel::<Done>();
        std::thread::spawn(move || {
            while let Ok(job) = jrx.recv() {
                let out = match job {
                    Job::Hq { obj, src } => {
                        let d = dial(&obj, &src, Mode::Hq, INF - 1);
                        let nb = Arc::new(neighbour_mask(&obj));
                        Done::Hq { obj, d: Arc::new(d), nb }
                    }
                    Job::Hunt { obj, src, max } => {
                        let d = dial(&obj, &src, Mode::Open, max);
                        Done::Hunt(Arc::new(
                            d.iter()
                                .map(|&v| if v > max { U16 } else { v as u16 })
                                .collect(),
                        ))
                    }
                    Job::Target { id, obj, src } => Done::Target {
                        id,
                        d: Arc::new(dial(&obj, &[src], Mode::Open, INF - 1)),
                    },
                    Job::Multi { id, obj, src, max } => Done::Multi {
                        id,
                        d: Arc::new(dial(&obj, &src, Mode::Open, max)),
                    },
                };
                if dtx.send(out).is_err() {
                    break;
                }
            }
        });
        FieldWorker { tx, rx, busy: 0, now: 0, relaxed: false, due: VecDeque::new() }
    }
    pub fn submit(&mut self, j: Job) {
        self.busy += 1;
        self.due.push_back(self.now + LAG);
        let _ = self.tx.send(j);
    }
    /// The next result whose tick has come, waiting for the worker if needed (unless `relaxed`).
    pub fn poll(&mut self) -> Option<Done> {
        if *self.due.front()? > self.now {
            return None;
        }
        let d = if self.relaxed { self.rx.try_recv().ok()? } else { self.rx.recv().ok()? };
        self.due.pop_front();
        self.busy -= 1;
        Some(d)
    }
}

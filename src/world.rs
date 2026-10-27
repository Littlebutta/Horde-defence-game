//! Map description, rasterising, and the static world grids the simulation runs on.

use macroquad::prelude::{vec2, Vec2};

pub const W: usize = 1536;
pub const H: usize = 1536;
pub const N: usize = W * H;
/// Coarse grid cell size used for targeting, glow and civilian awareness.
pub const CG: usize = 8;
pub const CW: usize = W / CG;
pub const CH: usize = H / CG;

pub const T_LAND: u8 = 0;
pub const T_ROAD: u8 = 1;
pub const T_PARK: u8 = 2;
pub const T_WATER: u8 = 3;
pub const T_AIR: u8 = 4;
pub const T_APRON: u8 = 5;
pub const T_RUNWAY: u8 = 6;
pub const T_RUBBLE: u8 = 7;
pub const T_BASE: u8 = 8;

/// Object grid values: -1 is open street, >= 0 is a player structure id,
/// <= -2 is a city building (-(index + 2)), SOLID cannot be entered at all.
pub const FREE: i32 = -1;
pub const SOLID: i32 = -1_000_000_000;

pub type Poly = Vec<Vec2>;

#[derive(Clone)]
pub struct Road {
    pub pts: Vec<Vec2>,
    pub w: f32,
    pub rank: u8,
}
#[derive(Clone)]
pub struct Line {
    pub pts: Vec<Vec2>,
    pub w: f32,
}
#[derive(Clone)]
pub struct BDesc {
    pub poly: Poly,
    pub h: f32,
}

#[derive(Clone, Default)]
pub struct Desc {
    pub name: String,
    pub note: String,
    /// metres per cell
    #[allow(dead_code)]
    pub mpc: f32,
    pub roads: Vec<Road>,
    pub buildings: Vec<BDesc>,
    pub water: Vec<Poly>,
    /// land inside `water` (the holes of lake and river areas: river islands, lake islands)
    pub islands: Vec<Poly>,
    pub water_lines: Vec<Line>,
    pub parks: Vec<Poly>,
    pub airport: Option<Poly>,
    pub aprons: Vec<Poly>,
    pub runways: Vec<Line>,
    pub hq: Vec2,
    /// co-op: where the other players picked their HQs (set by the game, never saved or sent)
    pub extra_hqs: Vec<Vec2>,
}

// ---------------------------------------------------------------------------------------------
// small helpers

pub struct Rng(u64);
impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    #[inline]
    pub fn f(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }
    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f()
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n.max(1) as u64) as usize
    }
}

/// Whether `p` is inside the polygon (even-odd rule).
pub fn point_in(poly: &[Vec2], p: Vec2) -> bool {
    let mut inside = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + n - 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

pub fn centroid(p: &[Vec2]) -> Vec2 {
    let mut s = Vec2::ZERO;
    for q in p {
        s += *q;
    }
    s / p.len().max(1) as f32
}

pub fn poly_area(p: &[Vec2]) -> f32 {
    let mut a = 0.0;
    let n = p.len();
    for i in 0..n {
        let j = (i + n - 1) % n;
        a += p[j].x * p[i].y - p[i].x * p[j].y;
    }
    a / 2.0
}

pub fn fill_poly(poly: &[Vec2], mut f: impl FnMut(usize)) {
    if poly.len() < 3 {
        return;
    }
    let (mut y0, mut y1) = (f32::MAX, f32::MIN);
    for p in poly {
        y0 = y0.min(p.y);
        y1 = y1.max(p.y);
    }
    let y0 = y0.floor().max(0.0) as i64;
    let y1 = (y1.ceil() as i64).min(H as i64 - 1);
    let n = poly.len();
    let mut xs: Vec<f32> = Vec::with_capacity(16);
    for y in y0..=y1 {
        let sy = y as f32 + 0.5;
        xs.clear();
        for i in 0..n {
            let a = poly[i];
            let b = poly[(i + n - 1) % n];
            if (a.y > sy) != (b.y > sy) {
                xs.push(a.x + (sy - a.y) / (b.y - a.y) * (b.x - a.x));
            }
        }
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mut k = 0;
        while k + 1 < xs.len() {
            let xa = ((xs[k] - 0.5).ceil() as i64).max(0);
            let xb = ((xs[k + 1] - 0.5).floor() as i64).min(W as i64 - 1);
            for x in xa..=xb {
                f(y as usize * W + x as usize);
            }
            k += 2;
        }
    }
}

/// Whether an HQ at `p` takes the airport into its base zone: on it, or within reach of its fence.
pub fn near_airport(d: &Desc, p: Vec2) -> bool {
    let Some(a) = &d.airport else { return false };
    let n = a.len();
    let mut inside = false;
    let mut best = f32::MAX;
    for i in 0..n {
        let (u, v) = (a[i], a[(i + n - 1) % n]);
        if (u.y > p.y) != (v.y > p.y) && p.x < (v.x - u.x) * (p.y - u.y) / (v.y - u.y) + u.x {
            inside = !inside;
        }
        let e = v - u;
        let t = ((p - u).dot(e) / e.length_squared().max(1e-6)).clamp(0.0, 1.0);
        best = best.min(p.distance(u + e * t));
    }
    inside || best < HQ_CLEAR + 40.0
}

pub fn fill_disc(c: Vec2, r: f32, mut f: impl FnMut(usize)) {
    let y0 = ((c.y - r).floor() as i64).max(0);
    let y1 = ((c.y + r).ceil() as i64).min(H as i64 - 1);
    let x0 = ((c.x - r).floor() as i64).max(0);
    let x1 = ((c.x + r).ceil() as i64).min(W as i64 - 1);
    let r2 = r * r;
    for y in y0..=y1 {
        let dy = y as f32 + 0.5 - c.y;
        for x in x0..=x1 {
            let dx = x as f32 + 0.5 - c.x;
            if dx * dx + dy * dy <= r2 {
                f(y as usize * W + x as usize);
            }
        }
    }
}

pub fn thick_line(pts: &[Vec2], w: f32, mut f: impl FnMut(usize)) {
    let r = w / 2.0;
    for s in pts.windows(2) {
        let (a, b) = (s[0], s[1]);
        let d = b - a;
        let len = d.length();
        if len < 1e-6 {
            continue;
        }
        let nrm = vec2(-d.y, d.x) / len * r;
        fill_poly(&[a + nrm, b + nrm, b - nrm, a - nrm], &mut f);
    }
    for p in pts {
        fill_disc(*p, r, &mut f);
    }
}

// ---------------------------------------------------------------------------------------------

pub struct Building {
    pub poly: Poly,
    pub h: f32,
    pub cells: u32,
    pub c: Vec2,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub hp: f32,
    pub max: f32,
    pub alive: bool,
    pub area: f32,
}

pub struct World {
    pub terrain: Vec<u8>,
    pub bid: Vec<i32>,
    pub zone: Vec<u8>,
    pub irradiated: Vec<f32>,
    pub buildings: Vec<Building>,
    pub desc: Desc,
    pub hq: Vec2,
}

pub const HQ_CLEAR: f32 = 60.0;
pub const OUTPOST_R: f32 = 75.0;

impl World {
    pub fn build(desc: Desc) -> World {
        let mut terrain = vec![T_LAND; N];
        let mut bid = vec![-1i32; N];
        for p in &desc.parks {
            fill_poly(p, |c| terrain[c] = T_PARK);
        }
        if let Some(a) = &desc.airport {
            fill_poly(a, |c| terrain[c] = T_AIR);
        }
        for a in &desc.aprons {
            fill_poly(a, |c| terrain[c] = T_APRON);
        }
        for r in &desc.runways {
            thick_line(&r.pts, r.w, |c| terrain[c] = T_RUNWAY);
        }
        for w in &desc.water {
            fill_poly(w, |c| terrain[c] = T_WATER);
        }
        for i in &desc.islands {
            fill_poly(i, |c| terrain[c] = T_LAND);
        }
        for p in desc.parks.iter().filter(|p| desc.islands.iter().any(|i| point_in(i, centroid(p)))) {
            fill_poly(p, |c| terrain[c] = T_PARK);
        }
        for l in &desc.water_lines {
            thick_line(&l.pts, l.w, |c| terrain[c] = T_WATER);
        }
        for r in &desc.roads {
            thick_line(&r.pts, r.w, |c| terrain[c] = T_ROAD);
        }
        let hq = desc.hq;
        // the HQ footprint is always dry
        for &q in std::iter::once(&hq).chain(&desc.extra_hqs) {
            fill_disc(q, 14.0, |c| {
                if terrain[c] == T_WATER || terrain[c] == T_LAND {
                    terrain[c] = T_APRON
                }
            });
        }

        // base zone: the airport, plus a minimum circle around the HQ
        let mut zone = vec![0u8; N];
        let mut have_air = false;
        if let Some(a) = &desc.airport {
            fill_poly(a, |c| {
                zone[c] = 1;
                have_air = true;
            });
            // the airport only counts when the HQ stands on it or next to it (the player can set
            // the HQ up anywhere)
            if !near_airport(&desc, hq) {
                zone.fill(0);
                have_air = false;
            }
        }
        fill_disc(hq, if have_air { HQ_CLEAR + 20.0 } else { 110.0 }, |c| {
            zone[c] = 1
        });

        // roads (plus a one-cell margin) must never be under a building, so a footprint that touches
        // one is shrunk towards its middle until it clears, or dropped
        let mut keep_out = vec![false; N];
        for r in &desc.roads {
            thick_line(&r.pts, r.w + 2.0, |c| keep_out[c] = true);
        }
        for r in &desc.runways {
            thick_line(&r.pts, r.w + 2.0, |c| keep_out[c] = true);
        }
        let mut buildings: Vec<Building> = Vec::new();
        for b in &desc.buildings {
            let c0 = centroid(&b.poly);
            if c0.distance(hq) < HQ_CLEAR || desc.extra_hqs.iter().any(|q| c0.distance(*q) < 30.0) {
                continue;
            }
            let mut cells = Vec::new();
            let mut poly = b.poly.clone();
            for k in 0..4 {
                let f = 1.0 - k as f32 * 0.1;
                let p: Poly = b.poly.iter().map(|q| c0 + (*q - c0) * f).collect();
                cells.clear();
                let mut clash = false;
                fill_poly(&p, |c| {
                    let t = terrain[c];
                    if keep_out[c] || t == T_WATER || bid[c] >= 0 {
                        clash = true;
                    } else {
                        cells.push(c);
                    }
                });
                if !clash {
                    poly = p;
                    break;
                }
                cells.clear();
            }
            if cells.len() < 3 {
                continue;
            }
            let b = BDesc { poly, h: b.h };
            let idx = buildings.len() as i32;
            for &c in &cells {
                bid[c] = idx;
            }
            let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for p in &b.poly {
                x0 = x0.min(p.x);
                y0 = y0.min(p.y);
                x1 = x1.max(p.x);
                y1 = y1.max(p.y);
            }
            let hp = cells.len() as f32 * 40.0;
            buildings.push(Building {
                poly: b.poly.clone(),
                h: b.h,
                cells: cells.len() as u32,
                c: c0,
                x0,
                y0,
                x1,
                y1,
                hp,
                max: hp,
                alive: true,
                area: poly_area(&b.poly),
            });
        }
        for c in 0..N {
            if zone[c] == 1 && terrain[c] == T_LAND && bid[c] < 0 {
                terrain[c] = T_BASE;
            }
        }
        World {
            terrain,
            bid,
            zone,
            irradiated: vec![0.0; N / (CG * CG)],
            buildings,
            desc,
            hq,
        }
    }

    #[inline]
    pub fn walkable_terrain(t: u8) -> bool {
        // everything but water: open ground, streets, parks, airport, rubble. Buildings are handled
        // separately (units can't be inside one)
        t != T_WATER
    }

    pub fn add_outpost(&mut self, at: Vec2) {
        let (terrain, zone, bid) = (&mut self.terrain, &mut self.zone, &self.bid);
        fill_disc(at, OUTPOST_R, |c| {
            zone[c] = 1;
            if terrain[c] == T_LAND && bid[c] < 0 {
                terrain[c] = T_BASE;
            }
        });
    }

    #[inline]
    pub fn in_zone(&self, p: Vec2) -> bool {
        if p.x < 0.0 || p.y < 0.0 || p.x >= W as f32 || p.y >= H as f32 {
            return false;
        }
        self.zone[p.y as usize * W + p.x as usize] == 1
    }

    #[inline]
    pub fn irradiated_at(&self, p: Vec2) -> bool {
        if p.x < 0.0 || p.y < 0.0 || p.x >= W as f32 || p.y >= H as f32 {
            return false;
        }
        self.irradiated[(p.y as usize / CG) * CW + p.x as usize / CG] > 0.0
    }
}

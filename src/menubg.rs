//! The living menu background: a small random city full of civilians, and all three armies
//! hunting them down and fighting each other. Its own little pixel sim, separate from the game's
//! horde, drawn as one screen-sized texture with one cell per unit.

use crate::defs::*;
use crate::world::Rng;
use macroquad::prelude::*;

// cell kinds
const LAND: u8 = 0;
const ROAD: u8 = 1;
const MAJOR: u8 = 2;
const PARK: u8 = 3;
const WATER: u8 = 4;
const BUILDING: u8 = 5;

const CIV: u8 = 0;
const CROWD: u8 = 2;
const FAR: u16 = u16::MAX;
/// how far (cells) a field reaches; past this units wander instead of hunting
const REACH: u16 = 140;

/// (speed in cells/s, hit points, damage per second, how far it hunts in cells) per kind:
/// civilians, zombies, aliens, orcs
const KINDS: [(f32, f32, f32, u16); 4] = [
    (5.0, 3.0, 0.0, 0),
    (4.4, 6.0, 3.5, 40),
    (11.0, 2.5, 2.4, 50),
    (5.0, 11.0, 6.0, 60),
];

#[derive(Clone, Copy)]
struct Agent {
    p: Vec2,
    kind: u8,
    hp: f32,
    /// wander heading
    head: f32,
}

pub struct MenuBg {
    gw: usize,
    gh: usize,
    /// screen pixels per cell
    cs: f32,
    cells: Vec<u8>,
    /// the city's colours, without units
    base: Vec<[u8; 4]>,
    /// fading marks where units died: (colour, strength 0..1)
    stain: Vec<([u8; 3], f32)>,
    agents: Vec<Agent>,
    /// one agent index per cell (the last one in it), for fights
    occ: Vec<u32>,
    /// how many stand in each cell: at most `CROWD`, so hordes spread like water instead of
    /// stacking up in single file
    cnt: Vec<u8>,
    /// distance to the nearest prey, for each army; distance to the nearest army, for civilians;
    /// then distance to each army's rally point
    fields: [Vec<u16>; 7],
    /// where each army is heading when there's nothing to hunt nearby, and for how much longer
    goals: [(Vec2, f32); 3],
    next_field: usize,
    queue: Vec<u32>,
    img: Image,
    tex: Texture2D,
    rng: Rng,
    /// timers until the next zombie outbreak / bug drop / orc wave
    timers: [f32; 3],
    /// open orc portals: (where, seconds left)
    portals: Vec<(Vec2, f32)>,
    /// bug drop flashes: (where, age)
    flashes: Vec<(Vec2, f32)>,
    dark: bool,
    acc: f32,
}

fn col(c: Color) -> [u8; 4] {
    [(c.r * 255.0) as u8, (c.g * 255.0) as u8, (c.b * 255.0) as u8, 255]
}

impl MenuBg {
    pub fn new(sw: f32, sh: f32, th: &Theme, seed: u64) -> MenuBg {
        // cells about 3 px on a 900 px tall screen, a bit bigger on tall ones, so the city reads
        let cs = (sh / 300.0).round().clamp(2.0, 6.0);
        let gw = (sw / cs).ceil() as usize + 1;
        let gh = (sh / cs).ceil() as usize + 1;
        let n = gw * gh;
        let mut b = MenuBg {
            gw,
            gh,
            cs,
            cells: vec![LAND; n],
            base: vec![[0; 4]; n],
            stain: vec![([0; 3], 0.0); n],
            agents: Vec::new(),
            occ: vec![u32::MAX; n],
            cnt: vec![0; n],
            fields: std::array::from_fn(|_| vec![FAR; n]),
            goals: [(Vec2::ZERO, 0.0); 3],
            next_field: 0,
            queue: Vec::with_capacity(n),
            img: Image::gen_image_color(gw as u16, gh as u16, BLACK),
            tex: Texture2D::empty(),
            rng: Rng::new(seed),
            timers: [0.0, 2.0, 5.0],
            portals: Vec::new(),
            flashes: Vec::new(),
            dark: th.dark,
            acc: 0.0,
        };
        b.tex = Texture2D::from_image(&b.img);
        b.tex.set_filter(FilterMode::Nearest);
        b.city();
        b.paint(th);
        for _ in 0..b.civ_target() {
            if let Some(p) = b.open_spot() {
                b.spawn(p, CIV);
            }
        }
        for _ in 0..3 {
            b.outbreak();
        }
        b.bug_drop();
        b.timers[2] = 1.0;
        // PH_MENU_SIM=<seconds>: run the battle ahead before the menu screenshot
        let warm: f32 = std::env::var("PH_MENU_SIM").ok().and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let (mut t, t0) = (0.0, std::time::Instant::now());
        while t < warm {
            b.step(1.0 / 30.0);
            t += 1.0 / 30.0;
        }
        if warm > 0.0 {
            let n: [usize; 4] = std::array::from_fn(|k| b.count(k as u8));
            eprintln!("menu battle {}x{}: {:.2} ms/step, civilians/zombies/bugs/orcs {:?}", gw, gh, t0.elapsed().as_secs_f32() * 1000.0 / (warm * 30.0), n);
        }
        b
    }

    /// Whether this background was made for a screen this size (a resize makes a new city).
    pub fn fits(&self, sw: f32, sh: f32, th: &Theme) -> bool {
        let cs = (sh / 300.0).round().clamp(2.0, 6.0);
        cs == self.cs
            && (sw / cs).ceil() as usize + 1 == self.gw
            && (sh / cs).ceil() as usize + 1 == self.gh
            && th.dark == self.dark
    }

    fn civ_target(&self) -> usize {
        self.gw * self.gh / 45
    }
    fn cap(&self) -> usize {
        self.gw * self.gh / 10
    }

    #[inline]
    fn idx(&self, x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 || x >= self.gw as i32 || y >= self.gh as i32 {
            None
        } else {
            Some(y as usize * self.gw + x as usize)
        }
    }
    #[inline]
    fn cell_of(&self, p: Vec2) -> Option<usize> {
        self.idx(p.x.floor() as i32, p.y.floor() as i32)
    }
    #[inline]
    fn walkable(&self, i: usize) -> bool {
        self.cells[i] < WATER
    }

    // ---------------------------------------------------------------------------------- the city

    fn city(&mut self) {
        let (gw, gh) = (self.gw as i32, self.gh as i32);
        let mut r = Rng::new(self.rng.next_u64());
        // a river winding across, most of the time
        let river = r.f() < 0.7;
        let vertical = r.f() < 0.4;
        let (span, across) = if vertical { (gh, gw) } else { (gw, gh) };
        let mid = r.range(0.25, 0.75) * across as f32;
        let (amp, freq, ph) = (r.range(8.0, 30.0), r.range(0.006, 0.018), r.range(0.0, 6.28));
        let rw = r.range(5.0, 11.0);
        if river {
            for s in 0..span {
                let c = mid + amp * (s as f32 * freq + ph).sin() + 6.0 * (s as f32 * freq * 3.1).sin();
                for a in (c - rw) as i32..=(c + rw) as i32 {
                    let (x, y) = if vertical { (a, s) } else { (s, a) };
                    if let Some(i) = self.idx(x, y) {
                        self.cells[i] = WATER;
                    }
                }
            }
        }
        // the street grid: uneven blocks, every few streets an avenue
        let lines = |r: &mut Rng, len: i32| {
            let mut v = Vec::new();
            let mut p = -(r.range(0.0, 20.0) as i32);
            let mut k = r.below(3);
            while p < len + 30 {
                k += 1;
                v.push((p, k % 4 == 0));
                p += r.range(18.0, 34.0) as i32;
            }
            v
        };
        let xs = lines(&mut r, gw);
        let ys = lines(&mut r, gh);
        let road = |cells: &mut Vec<u8>, x0: i32, y0: i32, x1: i32, y1: i32, major: bool| {
            for y in y0.max(0)..y1.min(gh) {
                for x in x0.max(0)..x1.min(gw) {
                    let i = (y * gw + x) as usize;
                    // only avenues bridge the river
                    if cells[i] != WATER || major {
                        cells[i] = if major { MAJOR } else { ROAD };
                    }
                }
            }
        };
        for &(x, m) in &xs {
            let w = if m { 3 } else { 2 };
            road(&mut self.cells, x, 0, x + w, gh, m);
        }
        for &(y, m) in &ys {
            let w = if m { 3 } else { 2 };
            road(&mut self.cells, 0, y, gw, y + w, m);
        }
        // a diagonal avenue cutting across, like Broadway
        if r.f() < 0.6 {
            let (x0, slope) = (r.range(0.0, gw as f32), r.range(-0.8, 0.8));
            for y in 0..gh {
                let x = (x0 + slope * (y - gh / 2) as f32) as i32;
                road(&mut self.cells, x, y, x + 3, y + 1, true);
            }
        }
        // fill the blocks: parks, or lots of buildings with gaps between them
        for wx in xs.windows(2) {
            for wy in ys.windows(2) {
                let bx0 = wx[0].0 + if wx[0].1 { 3 } else { 2 };
                let by0 = wy[0].0 + if wy[0].1 { 3 } else { 2 };
                let (bx1, by1) = (wx[1].0, wy[1].0);
                if bx1 - bx0 < 4 || by1 - by0 < 4 {
                    continue;
                }
                if r.f() < 0.1 {
                    self.fill(bx0, by0, bx1, by1, PARK, false);
                    continue;
                }
                // a sidewalk, then split the block into lots
                let mut lots = vec![(bx0 + 1, by0 + 1, bx1 - 1, by1 - 1)];
                let mut out = Vec::new();
                while let Some((x0, y0, x1, y1)) = lots.pop() {
                    let (w, h) = (x1 - x0, y1 - y0);
                    if (w <= 9 && h <= 9) || (w < 6 && h < 12) || (h < 6 && w < 12) {
                        out.push((x0, y0, x1, y1));
                        continue;
                    }
                    if w > h {
                        let c = x0 + (w as f32 * r.range(0.35, 0.65)) as i32;
                        lots.push((x0, y0, c, y1));
                        lots.push((c + 1, y0, x1, y1));
                    } else {
                        let c = y0 + (h as f32 * r.range(0.35, 0.65)) as i32;
                        lots.push((x0, y0, x1, c));
                        lots.push((x0, c + 1, x1, y1));
                    }
                }
                for (x0, y0, x1, y1) in out {
                    // some lots stay empty: yards and car parks people run through
                    if x1 - x0 >= 2 && y1 - y0 >= 2 && r.f() < 0.86 {
                        self.fill(x0, y0, x1, y1, BUILDING, true);
                    }
                }
            }
        }
    }

    fn fill(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, k: u8, keep_water: bool) {
        for y in y0..y1 {
            for x in x0..x1 {
                if let Some(i) = self.idx(x, y) {
                    if !(keep_water && self.cells[i] == WATER) && self.cells[i] != ROAD && self.cells[i] != MAJOR {
                        self.cells[i] = k;
                    }
                }
            }
        }
    }

    /// Bake the city's colours: Google Maps style flat ground, roofs lit from the top left.
    fn paint(&mut self, th: &Theme) {
        let mut r = Rng::new(7);
        for y in 0..self.gh as i32 {
            for x in 0..self.gw as i32 {
                let i = (y as usize) * self.gw + x as usize;
                let k = self.cells[i];
                let is = |dx: i32, dy: i32, kk: u8| self.idx(x + dx, y + dy).map_or(false, |j| self.cells[j] == kk);
                let c = match k {
                    ROAD => th.road,
                    MAJOR => th.road_major,
                    PARK => {
                        if r.f() < 0.12 {
                            mix(th.park, th.land, 0.5)
                        } else {
                            th.park
                        }
                    }
                    WATER => {
                        if !is(-1, 0, WATER) || !is(1, 0, WATER) || !is(0, -1, WATER) || !is(0, 1, WATER) {
                            th.water_edge
                        } else {
                            th.water
                        }
                    }
                    BUILDING => {
                        if !is(1, 0, BUILDING) || !is(0, 1, BUILDING) {
                            th.side_a
                        } else if !is(-1, 0, BUILDING) || !is(0, -1, BUILDING) {
                            th.side_b
                        } else {
                            // taller blocks in a few spots
                            let h = ((x / 7) * 31 + (y / 7) * 17) % 5;
                            if h == 0 {
                                th.roof_tall
                            } else {
                                th.roof
                            }
                        }
                    }
                    _ => th.land,
                };
                // dimmed towards the background, so the menu and the armies stand out
                self.base[i] = col(mix(c, th.bg, 0.4));
            }
        }
    }

    // ----------------------------------------------------------------------------------- spawning

    fn open_spot(&mut self) -> Option<Vec2> {
        for _ in 0..40 {
            let x = self.rng.below(self.gw) as i32;
            let y = self.rng.below(self.gh) as i32;
            let i = self.idx(x, y)?;
            if self.walkable(i) {
                return Some(vec2(x as f32 + 0.5, y as f32 + 0.5));
            }
        }
        None
    }

    fn spawn(&mut self, p: Vec2, kind: u8) {
        if self.agents.len() >= self.cap() {
            return;
        }
        if let Some(i) = self.cell_of(p) {
            if self.walkable(i) {
                let head = self.rng.range(0.0, std::f32::consts::TAU);
                self.agents.push(Agent { p, kind, hp: KINDS[kind as usize].1, head });
            }
        }
    }

    fn spawn_around(&mut self, c: Vec2, kind: u8, n: usize, rad: f32) {
        for _ in 0..n {
            let a = self.rng.range(0.0, std::f32::consts::TAU);
            let d = rad * self.rng.f().sqrt();
            self.spawn(c + vec2(a.cos(), a.sin()) * d, kind);
        }
    }

    fn count(&self, kind: u8) -> usize {
        self.agents.iter().filter(|a| a.kind == kind).count()
    }

    fn outbreak(&mut self) {
        if let Some(p) = self.open_spot() {
            self.spawn_around(p, 1, 160, 7.0);
        }
    }
    fn bug_drop(&mut self) {
        if let Some(p) = self.open_spot() {
            let n = 300 + self.rng.below(400);
            self.spawn_around(p, 2, n, 9.0);
            self.flashes.push((p, 0.0));
        }
    }
    /// Orc portals open near the edges and pour out a wave.
    fn orc_portal(&mut self) {
        let (w, h) = (self.gw as f32, self.gh as f32);
        for _ in 0..20 {
            let p = match self.rng.below(4) {
                0 => vec2(self.rng.range(8.0, 30.0), self.rng.range(8.0, h - 8.0)),
                1 => vec2(w - self.rng.range(8.0, 30.0), self.rng.range(8.0, h - 8.0)),
                2 => vec2(self.rng.range(8.0, w - 8.0), self.rng.range(8.0, 30.0)),
                _ => vec2(self.rng.range(8.0, w - 8.0), h - self.rng.range(8.0, 30.0)),
            };
            if self.cell_of(p).map_or(false, |i| self.walkable(i)) {
                self.portals.push((p, 7.0));
                return;
            }
        }
    }

    // --------------------------------------------------------------------------------------- sim

    /// Breadth-first distance from every cell to the nearest source, through walkable cells.
    fn flood(&mut self, f: usize) {
        let mut field = std::mem::take(&mut self.fields[f]);
        field.iter_mut().for_each(|d| *d = FAR);
        self.queue.clear();
        let cap = if f < 4 { REACH } else { FAR - 1 };
        if f >= 4 {
            if let Some(i) = self.cell_of(self.goals[f - 4].0) {
                field[i] = 0;
                self.queue.push(i as u32);
            }
        }
        for a in &self.agents {
            let src = match f {
                0 => a.kind != CIV,
                1..=3 => a.kind as usize != f,
                _ => false,
            };
            if src {
                if let Some(i) = self.cell_of(a.p) {
                    if field[i] != 0 {
                        field[i] = 0;
                        self.queue.push(i as u32);
                    }
                }
            }
        }
        let (gw, n) = (self.gw, self.gw * self.gh);
        let mut head = 0;
        while head < self.queue.len() {
            let i = self.queue[head] as usize;
            head += 1;
            let d = field[i] + 1;
            if d > cap {
                continue;
            }
            let x = i % gw;
            let mut go = |j: usize| {
                if field[j] > d && self.cells[j] < WATER {
                    field[j] = d;
                    self.queue.push(j as u32);
                }
            };
            if x > 0 {
                go(i - 1);
            }
            if x + 1 < gw {
                go(i + 1);
            }
            if i >= gw {
                go(i - gw);
            }
            if i + gw < n {
                go(i + gw);
            }
        }
        self.fields[f] = field;
    }

    /// The neighbouring cell with the lowest (or highest) field value, as a direction.
    fn slope(&self, f: &[u16], i: usize, up: bool) -> Option<Vec2> {
        let (x, y) = ((i % self.gw) as i32, (i / self.gw) as i32);
        let here = f[i];
        let mut best = here;
        let mut dir = None;
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
            if let Some(j) = self.idx(x + dx, y + dy) {
                let v = f[j];
                if !self.walkable(j) || v == FAR {
                    continue;
                }
                // diagonals only when both sides are open, so nobody cuts through a corner
                if dx != 0 && dy != 0 {
                    let a = self.idx(x + dx, y).map_or(false, |k| self.walkable(k));
                    let b = self.idx(x, y + dy).map_or(false, |k| self.walkable(k));
                    if !a || !b {
                        continue;
                    }
                }
                if (up && v > best) || (!up && v < best) {
                    best = v;
                    dir = Some(vec2(dx as f32, dy as f32).normalize());
                }
            }
        }
        dir
    }

    pub fn step(&mut self, dt: f32) {
        // one field a step, round robin: each is at most four steps old
        self.flood(self.next_field);
        self.next_field = (self.next_field + 1) % 4;
        // now and then each army picks somewhere new to march on: another army, or a crowd of
        // civilians, so the hordes pour down the streets and clash
        for f in 0..3 {
            self.goals[f].1 -= dt;
            if self.goals[f].1 <= 0.0 && !self.agents.is_empty() {
                let want = if self.rng.f() < 0.55 { (f as u8 + 1 + self.rng.below(2) as u8) % 3 + 1 } else { CIV };
                let pick = (0..30).map(|_| self.agents[self.rng.below(self.agents.len())]).find(|a| a.kind == want);
                if let Some(a) = pick {
                    self.goals[f] = (a.p, self.rng.range(9.0, 16.0));
                    self.flood(4 + f);
                }
            }
        }

        // who stands where, for fights
        self.occ.iter_mut().for_each(|o| *o = u32::MAX);
        self.cnt.iter_mut().for_each(|c| *c = 0);
        for (k, a) in self.agents.iter().enumerate() {
            if let Some(i) = self.cell_of(a.p) {
                self.occ[i] = k as u32;
                self.cnt[i] = self.cnt[i].saturating_add(1);
            }
        }

        // fights: an army unit next to a civilian or another army hits it
        let mut dmg = vec![0.0f32; self.agents.len()];
        let mut killer = vec![0u8; self.agents.len()];
        let mut busy = vec![false; self.agents.len()];
        for k in 0..self.agents.len() {
            let a = self.agents[k];
            if a.kind == CIV {
                continue;
            }
            let (x, y) = (a.p.x.floor() as i32, a.p.y.floor() as i32);
            'look: for (dx, dy) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
                if let Some(i) = self.idx(x + dx, y + dy) {
                    let o = self.occ[i];
                    if o != u32::MAX && self.agents[o as usize].kind != a.kind {
                        dmg[o as usize] += KINDS[a.kind as usize].2 * dt * self.rng.range(0.5, 1.5);
                        killer[o as usize] = a.kind;
                        busy[k] = true;
                        break 'look;
                    }
                }
            }
        }

        // movement
        for k in 0..self.agents.len() {
            if busy[k] {
                continue;
            }
            let mut a = self.agents[k];
            let Some(i) = self.cell_of(a.p) else { continue };
            let (spd, _, _, sight) = KINDS[a.kind as usize];
            let mut sp = spd;
            let mut dir = None;
            if a.kind == CIV {
                // run from anything closer than this, else stroll
                let d = self.fields[0][i];
                if d < 26 {
                    dir = self.slope(&self.fields[0], i, true);
                    sp *= 1.7;
                }
            } else if self.fields[a.kind as usize][i] < sight {
                dir = self.slope(&self.fields[a.kind as usize], i, false);
            } else {
                let g = &self.fields[3 + a.kind as usize];
                if g[i] > 5 && g[i] != FAR {
                    dir = self.slope(g, i, false);
                }
            }
            let jit = self.rng.range(-0.5, 0.5);
            let v = match dir {
                Some(d) => {
                    let (s, c) = jit.sin_cos();
                    vec2(d.x * c - d.y * s, d.x * s + d.y * c)
                }
                None => {
                    a.head += jit * 0.6;
                    sp *= 0.6;
                    vec2(a.head.cos(), a.head.sin())
                }
            };
            let step = v * sp * dt;
            let side: f32 = if self.rng.f() < 0.5 { 1.0 } else { -1.0 };
            // open ground with room: the cell it's in, or a walkable one not yet full
            let ok = |p: Vec2| self.cell_of(p).map_or(false, |j| j == i || (self.walkable(j) && self.cnt[j] < CROWD));
            let mut moved = false;
            // straight on, else veer round the crowd or wall, a little then a lot
            for turn in [0.0, 0.8, -0.8, 1.6, -1.6] {
                let (s, c) = (turn * side).sin_cos();
                let np = a.p + vec2(step.x * c - step.y * s, step.x * s + step.y * c);
                if ok(np) {
                    a.p = np;
                    if turn != 0.0 {
                        a.head += turn * side;
                    }
                    moved = true;
                    break;
                }
            }
            if !moved {
                a.head += self.rng.range(1.5, 4.5);
            }
            if let Some(j) = self.cell_of(a.p) {
                if j != i {
                    self.cnt[i] = self.cnt[i].saturating_sub(1);
                    self.cnt[j] = self.cnt[j].saturating_add(1);
                }
            }
            self.agents[k] = a;
        }

        // deaths: zombies turn most civilians they kill
        let mut k = 0;
        while k < self.agents.len() {
            self.agents[k].hp -= dmg[k];
            let a = self.agents[k];
            if a.hp <= 0.0 {
                if a.kind == CIV && killer[k] == 1 && self.rng.f() < 0.9 {
                    self.agents[k].kind = 1;
                    self.agents[k].hp = KINDS[1].1;
                } else {
                    let c = match a.kind {
                        CIV => [200, 30, 30],
                        f => FACTIONS[f as usize - 1].deep,
                    };
                    if let Some(i) = self.cell_of(a.p) {
                        self.stain[i] = (c, 1.0);
                    }
                    self.agents.swap_remove(k);
                    dmg.swap_remove(k);
                    killer.swap_remove(k);
                    continue;
                }
            }
            k += 1;
        }

        // keep the city alive: people come out of hiding, the armies send more
        let civs = self.count(CIV);
        let want = self.civ_target();
        if civs < want {
            let n = ((want - civs) as f32 * dt * 0.08).ceil() as usize;
            for _ in 0..n {
                // out of a doorway: a walkable cell next to a building
                if let Some(p) = self.open_spot() {
                    self.spawn(p, CIV);
                }
            }
        }
        let cap3 = self.cap() / 3;
        // wider screens get more outbreaks, drops and portals
        let more = (self.gw * self.gh) as f32 / 160_000.0;
        for t in self.timers.iter_mut() {
            *t -= dt * more.max(1.0);
        }
        if self.timers[0] <= 0.0 {
            self.timers[0] = self.rng.range(4.0, 8.0);
            if self.count(1) < cap3 {
                self.outbreak();
            }
        }
        if self.timers[1] <= 0.0 {
            self.timers[1] = self.rng.range(5.0, 9.0);
            if self.count(2) < cap3 {
                self.bug_drop();
            }
        }
        if self.timers[2] <= 0.0 {
            self.timers[2] = self.rng.range(12.0, 20.0);
            if self.count(3) < cap3 {
                self.orc_portal();
            }
        }
        let mut ps = std::mem::take(&mut self.portals);
        for (p, left) in ps.iter_mut() {
            *left -= dt;
            let n = (dt * 80.0 * self.rng.range(0.5, 1.5)).round() as usize;
            self.spawn_around(*p, 3, n, 3.0);
        }
        ps.retain(|(_, l)| *l > 0.0);
        self.portals = ps;
        for f in self.flashes.iter_mut() {
            f.1 += dt;
        }
        self.flashes.retain(|f| f.1 < 1.2);
        for s in self.stain.iter_mut() {
            if s.1 > 0.0 {
                s.1 -= dt * 0.04;
            }
        }
    }

    /// Advance the battle and draw it over the whole screen.
    pub fn frame(&mut self, dt: f32, th: &Theme) {
        // a fixed 30 Hz step, at most a few per frame
        self.acc = (self.acc + dt).min(0.2);
        while self.acc >= 1.0 / 30.0 {
            self.step(1.0 / 30.0);
            self.acc -= 1.0 / 30.0;
        }
        let civ = if th.dark { [255, 255, 255] } else { [40, 70, 150] };
        let px = &mut self.img.bytes;
        for (i, b) in self.base.iter().enumerate() {
            let (sc, s) = self.stain[i];
            let o = &mut px[i * 4..i * 4 + 4];
            if s > 0.0 {
                let t = s * 0.7;
                for c in 0..3 {
                    o[c] = (b[c] as f32 * (1.0 - t) + sc[c] as f32 * t) as u8;
                }
                o[3] = 255;
            } else {
                o.copy_from_slice(b);
            }
        }
        for a in &self.agents {
            let i = a.p.y as usize * self.gw + a.p.x as usize;
            let c = if a.kind == CIV { civ } else { FACTIONS[a.kind as usize - 1].col };
            px[i * 4..i * 4 + 3].copy_from_slice(&c);
        }
        self.tex.update(&self.img);
        let cs = self.cs;
        draw_texture_ex(
            &self.tex,
            0.0,
            0.0,
            WHITE,
            DrawTextureParams { dest_size: Some(vec2(self.gw as f32 * cs, self.gh as f32 * cs)), ..Default::default() },
        );
        let t = get_time() as f32;
        for &(p, left) in &self.portals {
            let c = FACTIONS[2].col;
            let r = (3.5 + (t * 6.0).sin() * 0.8) * cs * left.min(1.0);
            draw_circle_lines(p.x * cs, p.y * cs, r, 2.0, Color::from_rgba(c[0], c[1], c[2], 220));
            draw_circle(p.x * cs, p.y * cs, r * 0.6, Color::from_rgba(c[2], c[1] / 3, c[0] / 4, 160));
        }
        for &(p, age) in &self.flashes {
            let c = FACTIONS[1].col;
            let a = (1.0 - age / 1.2).max(0.0);
            draw_circle_lines(p.x * cs, p.y * cs, (4.0 + age * 18.0) * cs, 2.0, Color::from_rgba(c[0], c[1], c[2], (a * 200.0) as u8));
        }
    }
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    Color::new(a.r + (b.r - a.r) * t, a.g + (b.g - a.g) * t, a.b + (b.b - a.b) * t, 1.0)
}

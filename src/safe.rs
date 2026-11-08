//! Safehouses: a border the player draws around city buildings, out in the city. A slow truck
//! drives out from the HQ and sets it up; then civilians nearby run into it, turrets go up inside
//! it at base price with no helicopter, and armoured trucks ferry the people waiting there back to
//! the HQ to be flown out. Clicking one buys its upgrades.

use super::*;

/// What setting a safehouse up costs.
pub const SAFE_COST: f32 = 300.0;
/// How many safehouses a game can have at a time.
pub const SAFE_MAX: usize = 3;
/// Corners a border can have.
pub const SAFE_PTS: usize = 12;
pub const SAFE_AREA_MIN: f32 = 250.0;
pub const SAFE_AREA_MAX: f32 = 14000.0;
/// No hostile may be this close (cells) to the border when the truck is sent.
pub const SAFE_CLEAR: f32 = 40.0;
/// Seconds the truck takes to set the safehouse up once it's there.
pub const SAFE_BUILD: f32 = 8.0;
/// Civilians this close (field cost, about 10 a cell) run for a safehouse; loudspeakers double it.
pub const SAFE_SEEK: u32 = 700;
/// Cells a second: about as quick as a tank (Devin: the first, crawling speed was too slow).
pub const SETUP_SPEED: f32 = 14.0;
pub const CONVOY_SPEED: f32 = 18.0;
pub const CONVOY_SEATS: u8 = 8;
pub const CONVOY_SEATS_UP: u8 = 12;
pub const SETUP_HP: f32 = 400.0;
pub const CONVOY_HP: f32 = 600.0;
/// Seconds a convoy truck waits for more people once someone is aboard.
const CONVOY_WAIT: f32 = 10.0;
/// Seconds between rescue teams (Rescue team upgrade), and how many soldiers go.
const RESCUE_EVERY: f32 = 40.0;
const RESCUE_N: usize = 6;

pub const SH_DRIVE: u8 = 0;
pub const SH_BUILD: u8 = 1;
pub const SH_READY: u8 = 2;
pub const SH_LOST: u8 = 3;

/// The upgrades a safehouse can buy: (name, what it does, price).
pub const SAFE_UPS: [(&str, &str, f32); 5] = [
    ("Walls", "A wall all along the border, with gates where streets cross it for your trucks and troops", 250.0),
    ("Rescue team", "Every 40 s six soldiers go out to the nearest civilians and walk them back here", 200.0),
    ("Loudspeakers", "Civilians twice as far away hear the call and run here", 150.0),
    ("Armoured convoy", "A second truck, 12 seats a trip, and much tougher armour", 300.0),
    ("Truck guns", "Every truck gets a small turret on its roof that shoots hostiles close by", 200.0),
];
pub const SU_WALLS: u8 = 0;
pub const SU_RESCUE: u8 = 1;
pub const SU_LOUD: u8 = 2;
pub const SU_CONVOY: u8 = 3;
pub const SU_GUN: u8 = 4;
/// Truck guns: reach, seconds between shots, damage a shot.
pub const TRUCK_GUN_R: f32 = 24.0;
const TRUCK_GUN_RATE: f32 = 0.3;
const TRUCK_GUN_DMG: f32 = 12.0;

/// Civilians waiting inside a safehouse (`Civ::pad` is the safehouse), and riding a truck to the
/// HQ (`Civ::esc` is the truck).
pub const C_SAFE: u8 = 16;
pub const C_RIDE: u8 = 17;

pub struct Safehouse {
    pub poly: Vec<Vec2>,
    /// where the trucks park, inside the border
    pub c: Vec2,
    pub own: u8,
    pub state: u8,
    /// seconds of setting up left
    pub t: f32,
    /// upgrades bought (bit per `SAFE_UPS` entry)
    pub ups: u8,
    /// open-ground distance to the cells inside the border (trucks and rescue teams come back on it)
    pub field: Option<Arc<Vec<u32>>>,
    pub(super) job: u64,
    pub(super) rescue_t: f32,
    /// a convoy truck lost: seconds until the next one is ready
    pub(super) convoy_t: f32,
    /// civilians inside right now, and taken to the HQ so far
    pub held: u32,
    pub saved: u32,
}
impl Safehouse {
    pub fn has(&self, u: u8) -> bool {
        self.ups & (1 << u) != 0
    }
    pub fn seek(&self) -> u32 {
        if self.has(SU_LOUD) { SAFE_SEEK * 2 } else { SAFE_SEEK }
    }
}

pub const TRUCK_SETUP: u8 = 0;
pub const TRUCK_CONVOY: u8 = 1;
/// Convoy trucks: parked loading, driving to the HQ, driving back.
pub const TR_PARK: u8 = 0;
pub const TR_OUT: u8 = 1;
pub const TR_BACK: u8 = 2;

pub struct Truck {
    pub sh: usize,
    pub kind: u8,
    pub p: Vec2,
    pub hd: f32,
    pub state: u8,
    pub load: u8,
    pub hp: f32,
    pub max: f32,
    pub dead: bool,
    /// seconds parked with someone aboard
    pub t: f32,
    /// truck guns: seconds to the next shot, and where the gun points
    pub cd: f32,
    pub aim: f32,
}

/// Whether segments ab and cd cross (touching ends don't count).
fn cross(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    let o = |p: Vec2, q: Vec2, r: Vec2| (q - p).perp_dot(r - p);
    let (d1, d2, d3, d4) = (o(c, d, a), o(c, d, b), o(a, b, c), o(a, b, d));
    ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
}

/// Distance from p to the border's edge.
fn edge_dist(poly: &[Vec2], p: Vec2) -> f32 {
    let mut best = f32::MAX;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        let ab = b - a;
        let t = ((p - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
        best = best.min(p.distance(a + ab * t));
    }
    best
}

impl Game {
    /// Whether player `o` may draw safehouses (in a campaign, once the tech tree has them).
    pub fn safe_ok(&self, o: usize) -> bool {
        !self.campaign || self.has_up(o, Up::Safehouse)
    }

    /// A point inside a safehouse that's up and running.
    pub fn safe_at(&self, p: Vec2) -> Option<usize> {
        if p.x < 0.0 || p.y < 0.0 || p.x >= W as f32 || p.y >= H as f32 || self.world.zone[p.y as usize * W + p.x as usize] != 2 {
            return None;
        }
        self.safes.iter().position(|s| s.state == SH_READY && point_in(&s.poly, p))
    }
    /// The safehouse (any state but lost) whose border holds `p`.
    pub fn safe_under(&self, p: Vec2) -> Option<usize> {
        self.safes.iter().position(|s| s.state != SH_LOST && point_in(&s.poly, p))
    }

    /// Whether player `o` can set a safehouse up inside `pts`, and if not, why.
    pub fn safe_check(&self, o: usize, pts: &[Vec2]) -> Result<(), &'static str> {
        if !self.safe_ok(o) {
            return Err("Safehouses aren't unlocked yet");
        }
        if self.safes.iter().filter(|s| s.state != SH_LOST).count() >= SAFE_MAX {
            return Err("Three safehouses at most");
        }
        if pts.len() < 3 {
            return Err("Place at least three corners");
        }
        if pts.len() > SAFE_PTS {
            return Err("Twelve corners at most");
        }
        if pts.iter().any(|p| p.x < 4.0 || p.y < 4.0 || p.x > W as f32 - 4.0 || p.y > H as f32 - 4.0) {
            return Err("Too close to the edge of the map");
        }
        let n = pts.len();
        for i in 0..n {
            for j in i + 1..n {
                if (j + 1) % n == i || j == i + 1 {
                    continue;
                }
                if cross(pts[i], pts[(i + 1) % n], pts[j], pts[(j + 1) % n]) {
                    return Err("The border crosses itself");
                }
            }
        }
        let area = poly_area(pts).abs();
        if area < SAFE_AREA_MIN {
            return Err("Too small");
        }
        if area > SAFE_AREA_MAX {
            return Err("Too big: keep it to a block or two");
        }
        let (mut taken, mut wet, mut cells) = (false, 0usize, 0usize);
        fill_poly(pts, |c| {
            cells += 1;
            taken |= self.world.zone[c] != 0;
            wet += (self.world.terrain[c] == T_WATER) as usize;
        });
        if taken {
            return Err("It can't overlap your base zone or another safehouse");
        }
        if wet * 2 > cells {
            return Err("That's mostly water");
        }
        // every building it touches has to be wholly inside, and at least one must be
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for p in pts {
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
        }
        let mut whole = 0;
        for b in &self.world.buildings {
            if !b.alive || b.x1 < x0 || b.x0 > x1 || b.y1 < y0 || b.y0 > y1 {
                continue;
            }
            let inside = b.poly.iter().filter(|q| point_in(pts, **q)).count();
            let cut = (0..n).any(|i| {
                let (a, bb) = (pts[i], pts[(i + 1) % n]);
                (0..b.poly.len()).any(|k| cross(a, bb, b.poly[k], b.poly[(k + 1) % b.poly.len()]))
            }) || pts.iter().any(|p| point_in(&b.poly, *p));
            if cut || (inside > 0 && inside < b.poly.len()) {
                return Err("The border cuts through a building: go around the whole of it");
            }
            if inside == b.poly.len() {
                whole += 1;
            }
        }
        if whole == 0 {
            return Err("Draw it around at least one whole building");
        }
        // no hostile nearby
        let (gx0, gy0) = (((x0 - SAFE_CLEAR) / CG as f32).max(0.0) as usize, ((y0 - SAFE_CLEAR) / CG as f32).max(0.0) as usize);
        let (gx1, gy1) = ((((x1 + SAFE_CLEAR) / CG as f32) as usize).min(CW - 1), (((y1 + SAFE_CLEAR) / CG as f32) as usize).min(CH - 1));
        for gy in gy0..=gy1 {
            for gx in gx0..=gx1 {
                if self.swarm.coarse_at(gy * CW + gx) == 0 {
                    continue;
                }
                let q = vec2((gx * CG) as f32 + CG as f32 / 2.0, (gy * CG) as f32 + CG as f32 / 2.0);
                if point_in(pts, q) || edge_dist(pts, q) < SAFE_CLEAR {
                    return Err("The horde is too close to set up here");
                }
            }
        }
        if self.hives.iter().any(|h| !h.dead && (point_in(pts, h.p) || edge_dist(pts, h.p) < SAFE_CLEAR + h.r)) {
            return Err("A hive is too close to set up here");
        }
        if self.safe_park(pts).is_none() {
            return Err("Trucks can't reach it from the HQ");
        }
        if self.scrap < SAFE_COST {
            return Err("Not enough scrap");
        }
        Ok(())
    }

    /// For tests: a box around a building `min`..`max` cells from the HQ, grown until it takes in
    /// every building it touches, that a safehouse could go in (the building and the border).
    pub fn auto_safe_border(&self, min: f32, max: f32) -> Option<(usize, Vec<Vec2>)> {
        let hq = self.world.hq;
        (0..self.world.buildings.len())
            .filter(|&b| {
                let bb = &self.world.buildings[b];
                let d = bb.c.distance(hq);
                bb.alive && d > min && d < max && (bb.x1 - bb.x0) < 40.0 && (bb.y1 - bb.y0) < 40.0
            })
            .find_map(|b| {
                let bb = &self.world.buildings[b];
                let (mut x0, mut y0, mut x1, mut y1) = (bb.x0, bb.y0, bb.x1, bb.y1);
                let m = 4.0;
                for _ in 0..8 {
                    let mut grew = false;
                    for o in self.world.buildings.iter().filter(|o| o.alive) {
                        let touches = o.x1 >= x0 - m && o.x0 <= x1 + m && o.y1 >= y0 - m && o.y0 <= y1 + m;
                        if touches && (o.x0 < x0 || o.y0 < y0 || o.x1 > x1 || o.y1 > y1) {
                            (x0, y0, x1, y1) = (x0.min(o.x0), y0.min(o.y0), x1.max(o.x1), y1.max(o.y1));
                            grew = true;
                        }
                    }
                    if !grew {
                        break;
                    }
                }
                let sq = vec![vec2(x0 - m, y0 - m), vec2(x1 + m, y0 - m), vec2(x1 + m, y1 + m), vec2(x0 - m, y1 + m)];
                self.safe_check(self.actor, &sq).is_ok().then_some((b, sq))
            })
    }

    /// Where trucks park inside a border: the open cell nearest its middle that the HQ can reach.
    fn safe_park(&self, pts: &[Vec2]) -> Option<usize> {
        let mid = centroid(pts);
        let mut best: Option<(f32, usize)> = None;
        fill_poly(pts, |c| {
            if self.pass[c] == FREE && self.home[c] != INF {
                let d = Self::cell_pos(c).distance(mid);
                if best.is_none_or(|b| d < b.0) {
                    best = Some((d, c));
                }
            }
        });
        best.map(|b| b.1)
    }

    /// Sends the setup truck out to a new safehouse (player `actor` pays).
    pub fn place_safe(&mut self, pts: &[Vec2]) -> Result<(), &'static str> {
        self.safe_check(self.actor, pts)?;
        let park = self.safe_park(pts).ok_or("Trucks can't reach it from the HQ")?;
        self.scrap -= SAFE_COST;
        let k = self.safes.len();
        let mut src = Vec::new();
        fill_poly(pts, |c| {
            if self.pass[c] == FREE {
                src.push(c as u32);
            }
        });
        let job = self.next_job();
        self.fw_aux.submit(Job::Multi { id: job, obj: self.pass.clone(), src, max: INF - 1 });
        let from = self.hq_near(Self::cell_pos(park));
        let start = self.nearest_walkable(from + vec2(0.0, 12.0), 20).map(Self::cell_pos).unwrap_or(from);
        self.safes.push(Safehouse {
            poly: pts.to_vec(),
            c: Self::cell_pos(park),
            own: self.actor as u8,
            state: SH_DRIVE,
            t: SAFE_BUILD,
            ups: 0,
            field: None,
            job,
            rescue_t: 10.0,
            convoy_t: 2.0,
            held: 0,
            saved: 0,
        });
        self.trucks.push(Truck { sh: k, kind: TRUCK_SETUP, p: start, hd: 0.0, state: TR_OUT, load: 0, hp: SETUP_HP, max: SETUP_HP, dead: false, t: 0.0, cd: 0.0, aim: 0.0 });
        self.sfx.push((Sfx::Place, start));
        self.show("The safehouse truck is on its way", 3.0);
        Ok(())
    }

    /// Buys upgrade `u` for safehouse `k`.
    pub fn safe_upgrade(&mut self, k: usize, u: u8) -> bool {
        let Some(s) = self.safes.get(k) else { return false };
        let Some(&(_, _, cost)) = SAFE_UPS.get(u as usize) else { return false };
        if s.state != SH_READY || s.has(u) || self.scrap < cost {
            return false;
        }
        self.scrap -= cost;
        self.safes[k].ups |= 1 << u;
        match u {
            SU_WALLS => self.safe_walls(k),
            SU_CONVOY => {
                let s = &self.safes[k];
                let at = s.c;
                for t in self.trucks.iter_mut().filter(|t| !t.dead && t.sh == k && t.kind == TRUCK_CONVOY) {
                    t.max = CONVOY_HP * 1.8;
                    t.hp = t.max;
                }
                self.trucks.push(Truck { sh: k, kind: TRUCK_CONVOY, p: at, hd: 0.0, state: TR_PARK, load: 0, hp: CONVOY_HP * 1.8, max: CONVOY_HP * 1.8, dead: false, t: 0.0, cd: 0.0, aim: 0.0 });
            }
            _ => {}
        }
        self.sfx.push((Sfx::Place, self.safes[k].c));
        true
    }

    /// Walls along a safehouse's border, a gate wherever a street crosses it.
    fn safe_walls(&mut self, k: usize) {
        const G: usize = 3;
        let poly = self.safes[k].poly.clone();
        let inside = |p: Vec2| point_in(&poly, p);
        let mut blocks: Vec<(i32, i32)> = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        fill_poly(&poly, |c| {
            let (x, y) = ((c % W) as i32, (c / W) as i32);
            let edge = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(ox, oy)| !inside(vec2((x + ox) as f32 + 0.5, (y + oy) as f32 + 0.5)));
            if edge {
                let b = (x / G as i32, y / G as i32);
                if seen.insert(b) {
                    blocks.push(b);
                }
            }
        });
        for (gx, gy) in blocks {
            let (wx, wy) = (gx * G as i32, gy * G as i32);
            if wx < 2 || wy < 2 || wx + 4 >= W as i32 || wy + 4 >= H as i32 {
                continue;
            }
            let mut ok = true;
            let mut road = false;
            for yy in wy - 1..wy + G as i32 + 1 {
                for xx in wx - 1..wx + G as i32 + 1 {
                    let c = yy as usize * W + xx as usize;
                    road |= self.world.terrain[c] == T_ROAD;
                    let core = xx >= wx && yy >= wy && xx < wx + G as i32 && yy < wy + G as i32;
                    if core && (self.occ[c] >= 0 || self.world.terrain[c] == T_WATER || self.world.bid[c] >= 0) {
                        ok = false;
                    }
                }
            }
            if ok {
                let i = self.add_struct(if road { S::Gate } else { S::Wall }, wx, wy, -1);
                self.structs[i].owner = self.safes[k].own;
            }
        }
    }

    /// The safehouses' and their trucks' tick.
    pub(super) fn safehouses_tick(&mut self) {
        for s in &mut self.safes {
            s.held = 0;
        }
        for cv in &self.civs {
            if cv.st == C_SAFE {
                if let Some(s) = self.safes.get_mut(cv.pad) {
                    s.held += 1;
                }
            }
        }
        for k in 0..self.safes.len() {
            if self.safes[k].state == SH_BUILD {
                self.safes[k].t -= DT;
                if self.safes[k].t <= 0.0 {
                    self.safes[k].state = SH_READY;
                    self.safe_zone();
                    let at = self.safes[k].c;
                    // the setup truck becomes the first convoy truck
                    if let Some(t) = self.trucks.iter_mut().find(|t| !t.dead && t.sh == k && t.kind == TRUCK_SETUP) {
                        t.kind = TRUCK_CONVOY;
                        t.state = TR_PARK;
                        t.hp = CONVOY_HP;
                        t.max = CONVOY_HP;
                    }
                    self.sfx.push((Sfx::Place, at));
                    self.show("Safehouse ready: civilians nearby are heading for it", 4.0);
                }
            }
            if self.safes[k].state != SH_READY {
                continue;
            }
            // a lost truck is replaced after a while
            let want = if self.safes[k].has(SU_CONVOY) { 2 } else { 1 };
            let have = self.trucks.iter().filter(|t| !t.dead && t.sh == k).count();
            if have < want {
                self.safes[k].convoy_t -= DT;
                if self.safes[k].convoy_t <= 0.0 {
                    self.safes[k].convoy_t = 30.0;
                    let hp = if self.safes[k].has(SU_CONVOY) { CONVOY_HP * 1.8 } else { CONVOY_HP };
                    let at = self.safes[k].c;
                    self.trucks.push(Truck { sh: k, kind: TRUCK_CONVOY, p: at, hd: 0.0, state: TR_PARK, load: 0, hp, max: hp, dead: false, t: 0.0, cd: 0.0, aim: 0.0 });
                }
            }
            if self.safes[k].has(SU_RESCUE) {
                self.safes[k].rescue_t -= DT;
                if self.safes[k].rescue_t <= 0.0 {
                    self.safes[k].rescue_t = RESCUE_EVERY;
                    let c = self.safes[k].c;
                    // the nearest civilian out on the street, not too far
                    let near = self
                        .civs
                        .iter()
                        .filter(|cv| cv.st == C_ROAM && cv.p.distance(c) < 150.0)
                        .map(|cv| cv.p)
                        .min_by(|a, b| a.distance(c).total_cmp(&b.distance(c)));
                    if let Some(at) = near {
                        self.send_squad_from(c, at, RESCUE_N, -2 - k as i32);
                    }
                }
            }
        }
        self.trucks_tick();
        self.trucks.retain(|t| !t.dead);
        // trucks point at their safehouse by index, so lost safehouses stay in the list
    }

    fn trucks_tick(&mut self) {
        let obj = self.pass.clone();
        let home = self.home.clone();
        for ti in 0..self.trucks.len() {
            if self.trucks[ti].dead {
                continue;
            }
            let k = self.trucks[ti].sh;
            let field = self.safes[k].field.clone();
            let mut t = std::mem::replace(&mut self.trucks[ti], Truck { sh: 0, kind: 0, p: Vec2::ZERO, hd: 0.0, state: 0, load: 0, hp: 0.0, max: 0.0, dead: true, t: 0.0, cd: 0.0, aim: 0.0 });
            let before = t.p;
            let c = t.p.y as usize * W + t.p.x as usize;
            // the horde claws at it
            let (cx, cy) = (c % W, c / W);
            let mut n = 0u32;
            for oy in -1i32..=1 {
                for ox in -1i32..=1 {
                    let (x, y) = (cx as i32 + ox, cy as i32 + oy);
                    if x >= 0 && y >= 0 && x < W as i32 && y < H as i32 {
                        n += self.swarm.at(y as usize * W + x as usize);
                    }
                }
            }
            t.hp -= n as f32 * 4.0 * DT;
            if t.hp <= 0.0 {
                t.dead = true;
                self.sfx.push((Sfx::Wrecked, t.p));
                for cv in self.civs.iter_mut().filter(|cv| cv.st == C_RIDE && cv.esc == ti) {
                    cv.st = C_DEAD;
                    self.civ_lost += 1;
                }
                if t.kind == TRUCK_SETUP {
                    // no truck, no safehouse: half the money back
                    self.safes[k].state = SH_LOST;
                    self.scrap += SAFE_COST * 0.5;
                    self.show("The safehouse truck was overrun", 4.0);
                } else {
                    self.show("A safehouse truck was overrun", 3.0);
                }
                self.trucks[ti] = t;
                continue;
            }
            // truck guns: shoot whatever is closest
            if self.safes[k].has(SU_GUN) && t.kind == TRUCK_CONVOY {
                t.cd -= DT;
                if t.cd <= 0.0 {
                    t.cd = TRUCK_GUN_RATE;
                    if let Some(b) = self.find_target(t.p, TRUCK_GUN_R) {
                        t.aim = (b - t.p).to_angle();
                        self.swarm.add_damage(b.x, b.y, 1.2, TRUCK_GUN_DMG, 0);
                        self.damage_hives(b, 1.2, TRUCK_GUN_DMG);
                        self.tracers.push(Tracer { a: t.p, az: 2.4, b, t: 0.07, kind: 0 });
                        self.sfx.push((Sfx::Rifle, t.p));
                    }
                }
            }
            match (t.kind, t.state) {
                (TRUCK_SETUP, _) => {
                    if self.safes[k].state == SH_DRIVE {
                        if let Some(f) = &field {
                            if f[c] <= 10 || t.p.distance(self.safes[k].c) < 3.0 {
                                self.safes[k].state = SH_BUILD;
                            } else if f[c] == INF {
                                Self::step_toward(&obj, &mut t.p, self.safes[k].c, SETUP_SPEED);
                            } else {
                                Self::step_on_field(&obj, &mut t.p, f, SETUP_SPEED, &mut self.rng);
                            }
                        }
                    }
                }
                (_, TR_PARK) => {
                    let seats = if self.safes[k].has(SU_CONVOY) { CONVOY_SEATS_UP } else { CONVOY_SEATS };
                    // people waiting close by climb aboard
                    if t.load < seats {
                        for ci in 0..self.civs.len() {
                            let cv = &mut self.civs[ci];
                            if cv.st == C_SAFE && cv.pad == k && cv.p.distance(t.p) < 3.5 {
                                cv.st = C_RIDE;
                                cv.esc = ti;
                                t.load += 1;
                                if t.load >= seats {
                                    break;
                                }
                            }
                        }
                    }
                    if t.load > 0 {
                        t.t += DT;
                    }
                    // the other truck goes first when both are parked
                    let other_out = self.trucks.iter().enumerate().any(|(j, o)| j != ti && !o.dead && o.sh == k && o.state == TR_PARK && o.load > t.load);
                    if (t.load >= seats || t.t >= CONVOY_WAIT) && !other_out {
                        t.state = TR_OUT;
                        t.t = 0.0;
                    }
                }
                (_, TR_OUT) => {
                    Self::step_on_field(&obj, &mut t.p, &home, CONVOY_SPEED, &mut self.rng);
                    let nc = t.p.y as usize * W + t.p.x as usize;
                    if self.world.in_zone(t.p) || home[nc] <= 60 {
                        // inside the base: everyone off, to wait for a flight
                        let mut n = 0;
                        for cv in self.civs.iter_mut().filter(|cv| cv.st == C_RIDE && cv.esc == ti) {
                            cv.st = C_TO_PAD;
                            cv.p = t.p;
                            n += 1;
                        }
                        self.rescued += n;
                        self.score += 10.0 * n as f32;
                        self.safes[k].saved += n;
                        if n > 0 {
                            self.sfx.push((Sfx::Rescue, t.p));
                        }
                        t.load = 0;
                        t.state = TR_BACK;
                    }
                }
                _ => {
                    // back to the safehouse (a lost one: the truck stays in the base)
                    if self.safes[k].state != SH_READY {
                        t.dead = true;
                    } else if let Some(f) = &field {
                        if f[c] <= 10 || t.p.distance(self.safes[k].c) < 3.0 {
                            t.state = TR_PARK;
                        } else if f[c] == INF {
                            Self::step_toward(&obj, &mut t.p, self.safes[k].c, CONVOY_SPEED);
                        } else {
                            Self::step_on_field(&obj, &mut t.p, f, CONVOY_SPEED, &mut self.rng);
                        }
                    }
                }
            }
            let mv = t.p - before;
            if mv.length_squared() > 1e-4 {
                let want = mv.to_angle();
                let mut d = want - t.hd;
                while d > std::f32::consts::PI {
                    d -= std::f32::consts::TAU;
                }
                while d < -std::f32::consts::PI {
                    d += std::f32::consts::TAU;
                }
                t.hd += d * 0.25;
            }
            for cv in self.civs.iter_mut().filter(|cv| cv.st == C_RIDE && cv.esc == ti) {
                cv.p = t.p;
            }
            self.trucks[ti] = t;
        }
    }

    /// Marks the ground inside every ready safehouse (the base zone is put back first).
    pub(super) fn safe_zone(&mut self) {
        for s in self.safes.iter().filter(|s| s.state == SH_READY) {
            let zone = &mut self.world.zone;
            fill_poly(&s.poly, |c| {
                if zone[c] == 0 {
                    zone[c] = 2;
                }
            });
        }
        self.zone_dirty = true;
    }

    /// A civilian out on the street: the safehouse it should run to, with the field there and how
    /// far it is, if one is close enough.
    pub(super) fn safe_seek(&self, c: usize) -> Option<(usize, Arc<Vec<u32>>, u32)> {
        let mut best: Option<(usize, Arc<Vec<u32>>, u32)> = None;
        for (k, s) in self.safes.iter().enumerate() {
            if s.state != SH_READY {
                continue;
            }
            let Some(f) = &s.field else { continue };
            let d = f[c];
            if d <= s.seek() && best.as_ref().is_none_or(|b| d < b.2) {
                best = Some((k, f.clone(), d));
            }
        }
        best
    }

    /// Waiting inside safehouse `k`: wander about on open ground inside the border, and walk over
    /// to a parked truck.
    pub(super) fn civ_in_safe(&mut self, i: usize, obj: &[i32]) {
        let k = self.civs[i].pad;
        let Some(s) = self.safes.get(k).filter(|s| s.state == SH_READY) else {
            self.civs[i].st = C_ROAM;
            return;
        };
        let park = self.trucks.iter().find(|t| !t.dead && t.sh == k && t.state == TR_PARK).map(|t| t.p);
        let p = self.civs[i].p;
        if let Some(tp) = park {
            let mut pp = p;
            Self::step_toward(obj, &mut pp, tp, 6.0);
            self.civs[i].p = pp;
            return;
        }
        let Some(f) = s.field.clone() else { return };
        let civ = &mut self.civs[i];
        if self.rng.f() < 0.05 {
            civ.hd = self.rng.f() * std::f32::consts::TAU;
        }
        let np = civ.p + vec2(libm::cosf(civ.hd), libm::sinf(civ.hd)) * 1.2 * DT;
        let inb = np.x > 0.0 && np.y > 0.0 && np.x < W as f32 && np.y < H as f32;
        if inb && obj[np.y as usize * W + np.x as usize] == FREE && f[np.y as usize * W + np.x as usize] == 0 {
            civ.p = np;
        } else if f[p.y as usize * W + p.x as usize] > 0 {
            let mut pp = p;
            Self::step_on_field(obj, &mut pp, &f, 4.0, &mut self.rng);
            civ.p = pp;
        } else {
            civ.hd = self.rng.f() * std::f32::consts::TAU;
        }
    }
}

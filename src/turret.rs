//! Custom turrets in a game: the designs players bring from the garage and how each part makes
//! them pick targets, turn, fire and hit. Everything here runs in the lockstep simulation, so it
//! only uses the game's own rng and libm trig.

use super::*;
use crate::garage::{self, Aim, Build, Fire, Spec};

/// A design some player has brought into this game (co-op partners send theirs as build codes).
pub struct Design {
    pub code: String,
    pub build: Build,
    pub spec: Arc<Spec>,
}

/// What a custom turret is doing, on top of the usual `Structure` fields.
#[derive(Clone, Default)]
pub struct Custom {
    /// index into `Game::designs`
    pub d: u16,
    /// rotary barrels: how spun up (1 = still)
    pub spin: f32,
    /// shots left in the current burst, and seconds to the next one
    pub burst: u8,
    pub burst_t: f32,
    /// seconds of charge left before the shot goes (0: not charging)
    pub charge: f32,
    /// lasers and flamers: seconds of beam or spray left
    pub hold: f32,
    pub tg: Option<Vec2>,
    /// casemates: the way it faces (away from the HQ)
    pub face: f32,
    /// a twin cradle's second head
    pub aim2: f32,
    /// pop-up mounts: 0 down in the vault .. 1 up
    pub rise: f32,
    pub shield: f32,
    /// seconds since it was last hit (the shield comes back after 8)
    pub calm: f32,
    pub mine_t: f32,
    pub ciws_t: f32,
    /// scatter heads: where the last few shots went
    pub hist: Vec<Vec2>,
    /// split heads: which kind of target is next
    pub flip: bool,
    /// walkers: where it is and where it was built
    pub pos: Vec2,
    pub home: Vec2,
    /// crane arms: how far the head has swung out
    pub arm: f32,
}

/// What a shot does when it lands.
#[derive(Clone, Copy)]
pub struct Hit {
    pub dmg: f32,
    pub rad: f32,
    pub bdmg: f32,
    pub napalm: f32,
    pub cryo: f32,
    pub cluster: u8,
    pub bounce: u8,
    pub volatile: f32,
    pub recycle: f32,
    pub homing: bool,
    pub flak: bool,
    pub src: u16,
}

/// A shell, mortar bomb, rocket or flak round in the air.
pub struct CShot {
    pub from: Vec2,
    pub to: Vec2,
    pub t: f32,
    pub dur: f32,
    /// how high it arcs, as a share of the distance (rockets fly flat, mortars high)
    pub lob: f32,
    /// 0 shell, 1 mortar, 2 rocket, 3 flak
    pub kind: u8,
    pub hit: Hit,
}
/// A mine waiting for the horde to walk onto it.
pub struct Mine {
    pub p: Vec2,
    pub t: f32,
    pub dmg: f32,
    pub own: u8,
}
/// A frost patch from cryo rounds: the horde moves slowly in it.
pub struct Chill {
    pub p: Vec2,
    pub r: f32,
    pub t: f32,
}

const MINE_LIFE: f32 = 45.0;
const SHIELD_BACK: f32 = 8.0;
const WALK_SPEED: f32 = 3.0;

fn ang_diff(a: f32, b: f32) -> f32 {
    let mut d = (a - b) % std::f32::consts::TAU;
    if d > std::f32::consts::PI {
        d -= std::f32::consts::TAU;
    } else if d < -std::f32::consts::PI {
        d += std::f32::consts::TAU;
    }
    d
}
fn dir(a: f32) -> Vec2 {
    vec2(libm::cosf(a), libm::sinf(a))
}

impl Game {
    /// Adds a design (a build code) to the game for the player carrying out the command and makes
    /// it the one they place next. Refused when the build isn't legal or, in a campaign, uses
    /// parts that player hasn't got.
    pub fn pick_design(&mut self, b: &Build) -> bool {
        // the sim doesn't need its name
        let b = Build {
            name: String::new(),
            ..b.clone()
        };
        let Some(spec) = garage::spec(&b) else {
            return false;
        };
        let o = self.actor;
        if let Some(have) = &self.pk(o).parts {
            if b.part_ids().any(|i| !have.contains(&i)) {
                return false;
            }
        }
        let code = b.code();
        let k = match self.designs.iter().position(|d| d.code == code) {
            Some(k) => k,
            None => {
                if self.designs.len() >= 4000 {
                    return false;
                }
                self.designs.push(Design {
                    code,
                    build: b,
                    spec: Arc::new(spec),
                });
                self.designs.len() - 1
            }
        };
        let o = o.min(self.perks.len() - 1);
        self.perks[o].design = Some(k as u16);
        true
    }
    /// The design player `o` places next.
    pub fn cur_design(&self, o: usize) -> Option<&Design> {
        self.pk(o).design.and_then(|k| self.designs.get(k as usize))
    }
    /// The design of structure `i`, if it's a custom turret.
    pub fn design_of(&self, i: usize) -> Option<&Design> {
        self.structs
            .get(i)?
            .cu
            .as_ref()
            .and_then(|c| self.designs.get(c.d as usize))
    }
    /// Every part of `b` is on player `o`'s shelf (always, outside the campaign).
    pub fn parts_ok(&self, o: usize, b: &Build) -> bool {
        self.pk(o)
            .parts
            .as_ref()
            .is_none_or(|have| b.part_ids().all(|i| have.contains(&i)))
    }
    /// Sets up a freshly added custom turret from the actor's current design.
    pub(super) fn init_custom(&mut self, idx: usize) {
        let Some(k) = self.pk(self.actor).design else {
            return;
        };
        let Some(d) = self.designs.get(k as usize) else {
            return;
        };
        let sp = d.spec.clone();
        let s = &mut self.structs[idx];
        let c = s.center();
        let hq = self.world.hq;
        let mut hp = sp.hp;
        if self.perks[self.actor.min(self.perks.len() - 1)]
            .research
            .armour
        {
            hp *= 1.75;
        }
        s.hp = hp;
        s.max = hp;
        let face = libm::atan2f(c.y - hq.y, c.x - hq.x);
        s.aim = face;
        s.cu = Some(Box::new(Custom {
            d: k,
            spin: 1.0,
            face,
            aim2: face,
            rise: 0.0,
            shield: sp.shield,
            calm: SHIELD_BACK,
            mine_t: sp.mines,
            pos: c,
            home: c,
            ..Default::default()
        }));
    }
    /// Damage a custom turret takes after its shield and vault.
    pub(super) fn absorb(&mut self, i: usize, dmg: f32) -> f32 {
        let Some(k) = self.structs[i].cu.as_ref().map(|c| c.d) else {
            return dmg;
        };
        let sp = self.designs[k as usize].spec.clone();
        let cu = self.structs[i].cu.as_mut().unwrap();
        cu.calm = 0.0;
        let mut d = dmg;
        if sp.rise > 0.0 && cu.rise < 0.5 {
            d *= 0.2;
        }
        let soak = cu.shield.min(d);
        cu.shield -= soak;
        d - soak
    }
    /// Range of a custom turret standing `base` cells up (a roof or a bunker; 0 on the ground):
    /// its own, the height bonus and its owner's Long barrels.
    pub fn custom_range(&self, sp: &Spec, base: f32, o: usize) -> f32 {
        sp.range * height_mult(sp, base) * if self.res(o).barrels { 1.25 } else { 1.0 } + sp.reach
    }

    /// Every custom turret's turn, then the shots in the air, the mines and the frost.
    pub(super) fn custom_turrets(&mut self) {
        if self.structs.iter().any(|s| s.cu.is_some() && !s.dead) {
            // spotter uplinks lend their range and pace to turrets around them
            let links: Vec<(Vec2, f32)> = self
                .structs
                .iter()
                .filter(|s| !s.dead && !s.pending)
                .filter_map(|s| {
                    s.cu.as_ref()
                        .map(|c| (c.pos, self.designs[c.d as usize].spec.uplink))
                })
                .filter(|l| l.1 > 0.0)
                .collect();
            // what the guard heads watch over
            let guarded: Vec<Vec2> = self
                .structs
                .iter()
                .filter(|s| !s.dead && !s.pending)
                .map(|s| s.center())
                .collect();
            for i in 0..self.structs.len() {
                if self.structs[i].dead || self.structs[i].pending {
                    continue;
                }
                let Some(mut cu) = self.structs[i].cu.take() else {
                    continue;
                };
                let sp = self.designs[cu.d as usize].spec.clone();
                self.run_custom(i, &mut cu, &sp, &links, &guarded);
                if !self.structs[i].dead {
                    self.structs[i].cu = Some(cu);
                }
            }
        }
        self.custom_shots();
    }

    fn run_custom(
        &mut self,
        i: usize,
        cu: &mut Custom,
        sp: &Spec,
        links: &[(Vec2, f32)],
        guarded: &[Vec2],
    ) {
        let o = self.structs[i].owner as usize;
        let up = self.structs[i].up();
        // mending, shield
        {
            let s = &mut self.structs[i];
            if sp.regen > 0.0 {
                s.hp = (s.hp + sp.regen * DT).min(s.max);
            }
        }
        cu.calm += DT;
        if cu.calm >= SHIELD_BACK {
            cu.shield = sp.shield;
        }
        let link = links
            .iter()
            .filter(|(p, _)| p.distance(cu.pos) < 40.0 && p.distance(cu.pos) > 0.5)
            .map(|l| l.1)
            .fold(0.0, f32::max);
        let range = self.custom_range(sp, self.struct_base(i), o) * (1.0 + link);
        let c = cu.pos;
        let src = self.rev_src(RevKey::Struct(i));
        let hit = Hit {
            dmg: sp.dmg,
            rad: sp.rad,
            bdmg: sp.bdmg,
            napalm: sp.napalm,
            cryo: sp.cryo,
            cluster: sp.cluster,
            bounce: sp.bounce,
            volatile: sp.volatile,
            recycle: sp.recycle,
            homing: sp.homing,
            flak: sp.fire == Fire::Flak,
            src,
        };
        // the gear that works on its own
        if sp.ciws > 0.0 {
            cu.ciws_t -= DT;
            if cu.ciws_t <= 0.0 && self.ammo >= 1.0 {
                if let Some(k) = self.target_cell(c, sp.ciws, &[], |_| false) {
                    let p = Self::cell_pos(k);
                    for n in 0..3 {
                        let q = p + vec2(self.rng.range(-1.5, 1.5), self.rng.range(-1.5, 1.5));
                        self.swarm.add_damage(q.x, q.y, 1.3, 14.0, src);
                        if n == 0 {
                            self.tracers.push(Tracer {
                                a: c,
                                az: self.struct_base(i) + sp.height + 0.6,
                                b: q,
                                t: 0.06,
                                kind: 0,
                            });
                        }
                    }
                    self.ammo -= 1.0;
                    cu.ciws_t = if sp.ciws > 10.0 { 0.9 } else { 1.2 };
                }
            }
        }
        if sp.mines > 0.0 {
            cu.mine_t -= DT;
            if cu.mine_t <= 0.0 {
                cu.mine_t = 1.0;
                if let Some(p) = self.find_target(c, 22.0) {
                    if self.ammo >= 4.0 && self.mines.len() < 3000 {
                        self.ammo -= 4.0;
                        self.mines.push(Mine {
                            p,
                            t: MINE_LIFE,
                            dmg: 90.0,
                            own: o as u8,
                        });
                        self.tracers.push(Tracer {
                            a: c,
                            az: self.struct_base(i) + sp.height,
                            b: p,
                            t: 0.12,
                            kind: 1,
                        });
                        cu.mine_t = sp.mines;
                    }
                }
            }
        }
        // walkers get mobbed by whatever crowds round their legs (on a roof they stand guard)
        if sp.leash > 0.0 && !up {
            let crowd = self.units_in(c, 3.0) as f32;
            if crowd > 0.0 {
                let atk = self.faction().stats.atk;
                let d = self.absorb_cu(cu, sp, crowd * atk * DT * 0.12);
                self.structs[i].hp -= d;
                if self.structs[i].hp <= 0.0 {
                    self.kill_struct(i);
                    return;
                }
            }
        }
        // what it's in the middle of
        if cu.hold > 0.0 {
            self.hold_fire(i, cu, sp, range, hit);
            return;
        }
        if cu.charge > 0.0 {
            cu.charge -= DT;
            if cu.charge <= 0.0 {
                cu.charge = 0.0;
                if let Some(tg) = cu.tg {
                    self.trigger(i, cu, sp, tg, range, hit);
                }
            }
            return;
        }
        if cu.burst > 0 {
            cu.burst_t -= DT;
            if cu.burst_t <= 0.0 {
                cu.burst -= 1;
                cu.burst_t = sp.burst_gap;
                let tg = self.pick(i, cu, sp, c, range, guarded).or(cu.tg);
                if let Some(tg) = tg {
                    if self.ammo >= sp.ammo {
                        self.ammo -= sp.ammo;
                        self.structs[i].aim = libm::atan2f(tg.y - c.y, tg.x - c.x);
                        self.fire_once(i, cu, sp, tg, range, hit);
                    }
                }
            }
            return;
        }
        if self.time - self.structs[i].last_fire > 0.7 {
            cu.spin = (cu.spin - DT * 2.5).max(1.0);
        }
        self.structs[i].cd -= DT;
        let tg = self.pick(i, cu, sp, c, range, guarded);
        // pop-up vaults rise while there's something to shoot and sink when there isn't
        if sp.rise > 0.0 {
            cu.rise =
                (cu.rise + if tg.is_some() { DT } else { -DT * 0.5 } / sp.rise).clamp(0.0, 1.0);
        } else {
            cu.rise = 1.0;
        }
        // crane arms swing out toward the target, and back in when idle
        if sp.reach > 0.0 {
            let want = tg.map_or(0.0, |t| {
                (t.distance(c) - (range - sp.reach) * 0.55).clamp(0.0, sp.reach)
            });
            cu.arm += (want - cu.arm).clamp(-8.0 * DT, 8.0 * DT);
        }
        if sp.leash > 0.0 && !up {
            self.walk(cu, sp, tg, range);
        }
        let Some(tg) = tg else { return };
        if cu.rise < 1.0 {
            return;
        }
        let want = libm::atan2f(tg.y - c.y, tg.x - c.x);
        let aim = self.structs[i].aim;
        let diff = ang_diff(want, aim);
        let step = sp.traverse * DT;
        self.structs[i].aim = aim + diff.clamp(-step, step);
        if diff.abs() > step + 0.15 && !sp.fire_on_move && sp.aim != Aim::Sweep {
            return;
        }
        if self.structs[i].cd > 0.0 || self.ammo < sp.ammo {
            return;
        }
        let reload = sp.reload * (1.0 - link * 0.7) / cu.spin;
        self.structs[i].cd = reload;
        if sp.spin > 1.0 {
            cu.spin = (cu.spin + 0.35).min(sp.spin);
        }
        if sp.charge > 0.0 {
            // the shot goes when the charge is full; it's paid for now
            self.ammo -= sp.ammo;
            cu.charge = sp.charge;
            cu.tg = Some(tg);
            self.structs[i].last_fire = self.time;
            self.boom_at(c, 2.0, 0.3, 4);
            return;
        }
        self.ammo -= sp.ammo;
        self.trigger(i, cu, sp, tg, range, hit);
    }

    fn absorb_cu(&mut self, cu: &mut Custom, sp: &Spec, dmg: f32) -> f32 {
        cu.calm = 0.0;
        let mut d = dmg;
        if sp.rise > 0.0 && cu.rise < 0.5 {
            d *= 0.2;
        }
        let soak = cu.shield.min(d);
        cu.shield -= soak;
        d - soak
    }

    /// Walker legs: out toward the horde within the leash, home when there's nothing left.
    fn walk(&mut self, cu: &mut Custom, sp: &Spec, tg: Option<Vec2>, range: f32) {
        let want = match tg {
            Some(t) if t.distance(cu.pos) > range * 0.7 => Some(t),
            Some(_) => None,
            None => self.find_target(cu.home, sp.leash + range),
        };
        let goal = match want {
            Some(t) => {
                let d = (t - cu.home).normalize_or_zero();
                let far = (t.distance(cu.home) - range * 0.6).clamp(0.0, sp.leash);
                cu.home + d * far
            }
            None if tg.is_none() => cu.home,
            None => cu.pos,
        };
        let step = WALK_SPEED * DT;
        let d = goal - cu.pos;
        if d.length() > 0.2 {
            let np = cu.pos + d.normalize() * step.min(d.length());
            // keep to dry land on the map
            let k = np.y as usize * W + np.x as usize;
            if np.x > 2.0
                && np.y > 2.0
                && np.x < W as f32 - 2.0
                && np.y < H as f32 - 2.0
                && self.world.terrain[k] != T_WATER
            {
                cu.pos = np;
            }
        }
    }

    /// Hostiles in a little disc.
    fn units_in(&self, p: Vec2, r: f32) -> u32 {
        let k = r.ceil() as i32;
        let (cx, cy) = (p.x as i32, p.y as i32);
        let mut n = 0;
        for y in (cy - k).max(0)..=(cy + k).min(H as i32 - 1) {
            for x in (cx - k).max(0)..=(cx + k).min(W as i32 - 1) {
                if ((x - cx) * (x - cx) + (y - cy) * (y - cy)) as f32 <= r * r {
                    n += self.swarm.at(y as usize * W + x as usize);
                }
            }
        }
        n
    }

    /// The busiest cell of coarse cell `gi` that `ok` allows.
    fn best_in(&self, gi: usize, ok: &impl Fn(Vec2) -> bool) -> Option<Vec2> {
        let (bx, by) = ((gi % CW) * CG, (gi / CW) * CG);
        let (mut best, mut bv) = (None, 0);
        for y in by..by + CG {
            for x in bx..bx + CG {
                let v = self.swarm.at(y * W + x);
                if v <= bv {
                    continue;
                }
                let p = vec2(x as f32 + 0.5, y as f32 + 0.5);
                if ok(p) {
                    bv = v;
                    best = Some(p);
                }
            }
        }
        best
    }

    /// Where the head wants to shoot, if anywhere.
    fn pick(
        &mut self,
        i: usize,
        cu: &mut Custom,
        sp: &Spec,
        c: Vec2,
        range: f32,
        guarded: &[Vec2],
    ) -> Option<Vec2> {
        let (face, arc, min) = (cu.face, sp.arc, sp.min_range);
        let ok = move |p: Vec2| {
            let d = p.distance(c);
            d <= range
                && d >= min
                && (arc >= std::f32::consts::PI - 0.01
                    || ang_diff(libm::atan2f(p.y - c.y, p.x - c.x), face).abs() <= arc)
        };
        let nearest = |g: &Game| g.target_cell(c, range, &[], |p| !ok(p)).map(Self::cell_pos);
        let near = || self.coarse_near(c, range);
        let score_best = |g: &Game, score: &dyn Fn(usize) -> f32| {
            let mut cand: Vec<(f32, usize)> = g
                .coarse_near(c, range)
                .into_iter()
                .map(|(_, gi)| (score(gi), gi))
                .collect();
            cand.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            cand.into_iter().find_map(|(_, gi)| g.best_in(gi, &ok))
        };
        let crowd = |g: &Game, gi: usize| {
            let (gx, gy) = ((gi % CW) as i32, (gi / CW) as i32);
            let mut n = 0;
            for oy in -1..=1 {
                for ox in -1..=1 {
                    let (x, y) = (gx + ox, gy + oy);
                    if x >= 0 && y >= 0 && x < CW as i32 && y < CH as i32 {
                        n += g.swarm.coarse_at(y as usize * CW + x as usize);
                    }
                }
            }
            n as f32
        };
        let dense = |g: &Game| {
            if arc >= std::f32::consts::PI - 0.01 && min <= 0.0 {
                g.cluster_target(c, range, sp.rad.max(2.0), &[])
            } else {
                score_best(g, &|gi| -crowd(g, gi))
            }
        };
        match sp.aim {
            Aim::Near => nearest(self),
            Aim::Dense => dense(self),
            Aim::Far => near()
                .into_iter()
                .rev()
                .find_map(|(_, gi)| self.best_in(gi, &ok)),
            Aim::Guard => score_best(self, &|gi| {
                let p = vec2((gi % CW * CG) as f32 + 4.0, (gi / CW * CG) as f32 + 4.0);
                guarded
                    .iter()
                    .map(|q| q.distance(p))
                    .fold(f32::MAX, f32::min)
            }),
            Aim::Hq => score_best(self, &|gi| {
                let k = (gi / CW * CG + 4) * W + gi % CW * CG + 4;
                self.hq_dist[k].min(INF) as f32
            }),
            Aim::Smart => score_best(self, &|gi| {
                let k = (gi / CW * CG + 4) * W + gi % CW * CG + 4;
                let hq = self.hq_dist[k].min(200_000) as f32 / 10.0;
                -crowd(self, gi) * (1.0 + 3.0 / (1.0 + hq / 40.0))
            }),
            Aim::Lead => {
                let t = nearest(self)?;
                let fly = sp.charge
                    + if sp.travel > 0.0 {
                        sp.travel * t.distance(c) / 50.0
                    } else {
                        0.0
                    }
                    + if sp.fire == Fire::Mortar { 1.6 } else { 0.0 };
                let cells = (self.faction().stats.spd * fly * 0.8) as usize;
                let p = self.downhill(t, cells.min(40));
                Some(if p.distance(c) <= range && p.distance(c) >= min {
                    p
                } else {
                    t
                })
            }
            Aim::Scatter => {
                let hist = cu.hist.clone();
                let p = self
                    .target_cell(c, range, &[], |p| {
                        !ok(p) || hist.iter().any(|h| h.distance(p) < 5.0)
                    })
                    .map(Self::cell_pos)
                    .or_else(|| nearest(self));
                if let Some(p) = p {
                    cu.hist.push(p);
                    if cu.hist.len() > 6 {
                        cu.hist.remove(0);
                    }
                }
                p
            }
            Aim::Sweep => {
                // the head turns all the time; it fires at whatever is along its line
                let a = self.structs[i].aim + sp.traverse * DT;
                self.structs[i].aim = a;
                let d = dir(a);
                let mut r = sp.min_range.max(2.0);
                while r <= range {
                    let p = c + d * r;
                    if p.x < 1.0 || p.y < 1.0 || p.x >= W as f32 - 1.0 || p.y >= H as f32 - 1.0 {
                        break;
                    }
                    if self.units_in(p, 1.5) > 0 && ok(p) {
                        return Some(p);
                    }
                    r += 2.0;
                }
                None
            }
            Aim::Split => {
                cu.flip = !cu.flip;
                if cu.flip {
                    nearest(self)
                } else {
                    dense(self)
                }
            }
        }
    }

    /// Follows the horde's path toward the HQ for `n` cells from `p`.
    fn downhill(&self, p: Vec2, n: usize) -> Vec2 {
        let (mut x, mut y) = (p.x as i32, p.y as i32);
        for _ in 0..n {
            let here = self.hq_dist[y as usize * W + x as usize];
            let mut best = (here, x, y);
            for k in 0..8 {
                let (nx, ny) = (x + DX[k], y + DY[k]);
                if nx < 1 || ny < 1 || nx >= W as i32 - 1 || ny >= H as i32 - 1 {
                    continue;
                }
                let v = self.hq_dist[ny as usize * W + nx as usize];
                if v < best.0 {
                    best = (v, nx, ny);
                }
            }
            if best.0 >= here {
                break;
            }
            (x, y) = (best.1, best.2);
        }
        vec2(x as f32 + 0.5, y as f32 + 0.5)
    }

    /// Where the shot comes from: the head, out on a crane arm if it has one.
    fn muzzle(&self, cu: &Custom, tg: Vec2) -> Vec2 {
        if cu.arm > 0.0 {
            cu.pos + (tg - cu.pos).normalize_or_zero() * cu.arm
        } else {
            cu.pos
        }
    }

    /// One pull of the trigger: the first shot, a burst to follow, twin cradles and twin-links.
    fn trigger(&mut self, i: usize, cu: &mut Custom, sp: &Spec, tg: Vec2, range: f32, hit: Hit) {
        let c = cu.pos;
        self.structs[i].last_fire = self.time;
        self.structs[i].aim = libm::atan2f(tg.y - c.y, tg.x - c.x);
        cu.tg = Some(tg);
        if matches!(sp.fire, Fire::Laser | Fire::Flame) {
            cu.hold = sp.hold;
            self.voice(i, sp, c);
            return;
        }
        self.fire_once(i, cu, sp, tg, range, hit);
        if sp.burst > 1 {
            cu.burst = sp.burst - 1;
            cu.burst_t = sp.burst_gap;
        }
        if sp.twin_mount && self.ammo >= sp.ammo {
            let second = self
                .target_cell(c, range, &[], |p| {
                    p.distance(tg) < 8.0 || p.distance(c) < sp.min_range
                })
                .map(Self::cell_pos);
            if let Some(t2) = second {
                self.ammo -= sp.ammo;
                cu.aim2 = libm::atan2f(t2.y - c.y, t2.x - c.x);
                self.fire_once(i, cu, sp, t2, range, hit);
            }
        } else {
            cu.aim2 = self.structs[i].aim;
        }
    }

    /// One shot (or pellet spread, or bolt) at `tg`, with a twin-link's partner if it has one.
    fn fire_once(&mut self, i: usize, cu: &mut Custom, sp: &Spec, tg: Vec2, range: f32, hit: Hit) {
        self.shot(i, cu, sp, tg, range, hit);
        // each twin-link sends a partner shot at another target near the first
        let mut taken = vec![tg.y as usize * W + tg.x as usize];
        for _ in 0..sp.twin_link {
            if self.ammo < sp.ammo {
                break;
            }
            let Some(k) = self.target_cell(tg, 8.0, &taken, |p| p.distance(cu.pos) > range) else {
                break;
            };
            taken.push(k);
            self.ammo -= sp.ammo;
            self.shot(i, cu, sp, Self::cell_pos(k), range, hit);
        }
    }

    fn scatter(&mut self, from: Vec2, tg: Vec2, spread: f32) -> Vec2 {
        if spread <= 0.0 {
            return tg;
        }
        let a = self.rng.f() * std::f32::consts::TAU;
        let r = self.rng.f() * spread * (tg.distance(from) / 50.0).max(0.2);
        (tg + dir(a) * r).clamp(vec2(1.0, 1.0), vec2(W as f32 - 2.0, H as f32 - 2.0))
    }

    fn shot(&mut self, i: usize, cu: &mut Custom, sp: &Spec, tg: Vec2, range: f32, hit: Hit) {
        let m = self.muzzle(cu, tg);
        let az = self.struct_base(i) + sp.height + 0.9;
        self.voice(i, sp, m);
        match sp.fire {
            Fire::Bullet => {
                let p = self.scatter(m, tg, sp.spread);
                self.tracers.push(Tracer {
                    a: m,
                    az,
                    b: p,
                    t: 0.08,
                    kind: if sp.dmg > 30.0 { 1 } else { 0 },
                });
                self.impact(hit, p);
            }
            Fire::Pellets => {
                let base = libm::atan2f(tg.y - m.y, tg.x - m.x);
                let d0 = tg.distance(m);
                for _ in 0..sp.pellets {
                    let a = base + (self.rng.f() - 0.5) * sp.cone;
                    let d = (d0 * self.rng.range(0.85, 1.15)).min(range);
                    let p = (m + dir(a) * d)
                        .clamp(vec2(1.0, 1.0), vec2(W as f32 - 2.0, H as f32 - 2.0));
                    self.tracers.push(Tracer {
                        a: m,
                        az,
                        b: p,
                        t: 0.06,
                        kind: 0,
                    });
                    self.impact(hit, p);
                }
            }
            Fire::Shell | Fire::Mortar | Fire::Missile | Fire::Flak => {
                let p = self.scatter(m, tg, sp.spread);
                let dist = p.distance(m);
                let (dur, lob, kind) = match sp.fire {
                    Fire::Mortar => (1.6 + dist / 110.0, 0.45, 1),
                    Fire::Missile => ((sp.travel * dist / 50.0).max(0.35), 0.12, 2),
                    Fire::Flak => ((sp.travel * dist / 50.0).max(0.2), 0.08, 3),
                    _ => ((sp.travel * dist / 50.0).max(0.2), 0.18, 0),
                };
                if self.cshots.len() < 20_000 {
                    self.cshots.push(CShot {
                        from: m,
                        to: p,
                        t: 0.0,
                        dur,
                        lob,
                        kind,
                        hit,
                    });
                }
                if kind != 2 {
                    self.boom_at(m + (p - m).normalize_or_zero() * 2.5, 1.6, 0.15, 1);
                }
            }
            Fire::Rail | Fire::Needle => {
                let d = (tg - m).normalize_or_zero();
                let len = if sp.fire == Fire::Rail {
                    range
                } else {
                    (tg.distance(m) + sp.pierce).min(range + sp.pierce)
                };
                let end = m + d * len;
                let steps = (len / 1.2) as usize;
                for k in 2..=steps {
                    let p = m + d * (k as f32 * 1.2);
                    if p.x < 1.0 || p.y < 1.0 || p.x >= W as f32 - 1.0 || p.y >= H as f32 - 1.0 {
                        break;
                    }
                    self.swarm.add_damage(
                        p.x,
                        p.y,
                        sp.rad,
                        sp.dmg,
                        if sp.fire == Fire::Rail { 0 } else { hit.src },
                    );
                }
                self.damage_buildings(tg, sp.rad, sp.bdmg);
                let kind = if sp.fire == Fire::Rail { 6 } else { 12 };
                self.tracers.push(Tracer {
                    a: m,
                    az,
                    b: end,
                    t: if kind == 6 { 0.35 } else { 0.12 },
                    kind,
                });
                // the rest of the payload goes off where it was aimed
                let mut h = hit;
                h.dmg = 0.0;
                self.impact(h, tg);
            }
            Fire::Tesla => {
                self.tracers.push(Tracer {
                    a: m,
                    az,
                    b: tg,
                    t: 0.12,
                    kind: 11,
                });
                let mut h = hit;
                h.bounce = 0;
                self.impact(h, tg);
                let mut at = tg;
                let mut claimed = vec![tg.y as usize * W + tg.x as usize];
                for _ in 0..sp.chain {
                    let from = at;
                    let Some(k) =
                        self.target_cell(from, 10.0, &claimed, |p| p.distance(from) < 1.5)
                    else {
                        break;
                    };
                    let p = Self::cell_pos(k);
                    claimed.push(k);
                    self.tracers.push(Tracer {
                        a: from,
                        az: 0.8,
                        b: p,
                        t: 0.12,
                        kind: 11,
                    });
                    self.impact(h, p);
                    at = p;
                }
            }
            Fire::Sonic => {
                let base = libm::atan2f(tg.y - m.y, tg.x - m.x);
                let mut r = 3.0;
                while r <= range {
                    let n = ((sp.cone * r) / 2.5).ceil().max(1.0) as i32;
                    for k in -n..=n {
                        let a = base + sp.cone * k as f32 / (2 * n).max(1) as f32;
                        let p = m + dir(a) * r;
                        if p.x > 1.0 && p.y > 1.0 && p.x < W as f32 - 1.0 && p.y < H as f32 - 1.0 {
                            self.swarm.add_damage(p.x, p.y, sp.rad, sp.dmg, hit.src);
                        }
                    }
                    r += 3.0;
                }
                for k in 1..=3 {
                    self.boom_at(
                        m + dir(base) * range * k as f32 / 3.5,
                        2.0 + k as f32 * 2.5,
                        0.35,
                        4,
                    );
                }
                if hit.cryo > 0.0 {
                    self.chill(m + dir(base) * range * 0.55, range * 0.5, hit.cryo);
                }
                if hit.napalm > 0.0 {
                    let mut h = hit;
                    h.dmg = 0.0;
                    self.impact(h, tg);
                }
            }
            Fire::Laser | Fire::Flame => {}
        }
    }

    /// A laser beam or flame spray being held on the horde.
    fn hold_fire(&mut self, i: usize, cu: &mut Custom, sp: &Spec, range: f32, hit: Hit) {
        cu.hold -= DT;
        let c = cu.pos;
        // the beam drags along onto whatever is nearest its line
        if self.ticks % 5 == 0 {
            if let Some(t) = self
                .target_cell(c, range, &[], |p| p.distance(c) < sp.min_range)
                .map(Self::cell_pos)
            {
                cu.tg = Some(t);
            }
        }
        let Some(tg) = cu.tg else {
            cu.hold = 0.0;
            return;
        };
        let want = libm::atan2f(tg.y - c.y, tg.x - c.x);
        let aim = self.structs[i].aim;
        let step = sp.traverse * DT * 0.8;
        let a = aim + ang_diff(want, aim).clamp(-step, step);
        self.structs[i].aim = a;
        self.structs[i].last_fire = self.time;
        let d = dir(a);
        let m = self.muzzle(cu, tg);
        let az = self.struct_base(i) + sp.height + 0.9;
        let reach = (tg.distance(m) + 4.0).min(range);
        if sp.fire == Fire::Laser {
            let steps = (reach / 1.5) as usize;
            for k in 2..=steps {
                let p = m + d * (k as f32 * 1.5);
                if p.x < 1.0 || p.y < 1.0 || p.x >= W as f32 - 1.0 || p.y >= H as f32 - 1.0 {
                    break;
                }
                self.swarm.add_damage(p.x, p.y, sp.rad, sp.dmg, hit.src);
            }
            self.tracers.push(Tracer {
                a: m,
                az,
                b: m + d * reach,
                t: 0.06,
                kind: 10,
            });
            if self.ticks % 6 == 0 {
                let mut h = hit;
                h.dmg = 0.0;
                self.impact(h, m + d * reach);
            }
        } else {
            let lay = (self.ticks + i as u64) % 5 == 0;
            for k in 0..5 {
                let f = k as f32 / 4.0;
                let p = m + d * (1.5 + (reach - 1.5) * f);
                let r = sp.rad * (0.55 + f * 0.75);
                self.swarm.add_damage(p.x, p.y, r, sp.dmg, hit.src);
                if (self.ticks + k) % 2 == 0 {
                    self.booms.push(Boom {
                        p,
                        r,
                        t: 0.3,
                        life: 0.3,
                        kind: 3,
                    });
                }
                if lay && k > 0 && hit.napalm > 0.0 {
                    self.napalm.push(Napalm {
                        p,
                        r: r * 0.8,
                        t: NAPALM_LIFE,
                        dmg: hit.napalm * DT,
                    });
                }
                if lay && k == 4 && hit.cryo > 0.0 {
                    self.chill(p, r + 2.0, hit.cryo);
                }
            }
        }
        if cu.hold <= 0.0 {
            cu.hold = 0.0;
            if sp.fire == Fire::Flame {
                self.sfx.push((Sfx::FlameStop, c));
            }
        }
    }

    /// What a shot does where it lands: the blast, then whatever its modifiers add.
    fn impact(&mut self, h: Hit, p: Vec2) {
        let p = if h.homing {
            self.find_target(p, 7.0).unwrap_or(p)
        } else {
            p
        };
        let n = if h.volatile > 0.0 || h.recycle > 0.0 {
            self.units_in(p, h.rad.max(1.5))
        } else {
            0
        };
        if h.flak {
            for k in 0..6 {
                let q = p + dir(k as f32 * std::f32::consts::TAU / 6.0) * 4.0;
                self.blast(q, h.rad, h.dmg, h.bdmg * 0.3, h.src, 0.0);
                self.boom_at(q, h.rad, 0.2, 0);
            }
        } else if h.dmg > 0.0 {
            self.blast(p, h.rad, h.dmg, h.bdmg, h.src, 0.0);
            if h.rad >= 2.5 {
                self.boom_at(p, h.rad, 0.25, 0);
            }
        }
        if h.napalm > 0.0 && self.napalm.len() < 6000 {
            self.napalm.push(Napalm {
                p,
                r: h.rad.max(1.6) * 0.9,
                t: NAPALM_LIFE,
                dmg: h.napalm * DT,
            });
        }
        if h.cryo > 0.0 {
            self.chill(p, h.rad + 3.0, h.cryo);
        }
        for k in 0..h.cluster {
            let a = (k as f32 + self.rng.f()) * std::f32::consts::TAU / h.cluster as f32;
            let q = (p + dir(a) * (h.rad * 1.2 + self.rng.f() * (h.rad + 2.0)))
                .clamp(vec2(1.0, 1.0), vec2(W as f32 - 2.0, H as f32 - 2.0));
            self.blast(
                q,
                (h.rad * 0.6).max(1.0),
                h.dmg * 0.35,
                h.bdmg * 0.3,
                h.src,
                0.0,
            );
            self.boom_at(q, (h.rad * 0.6).max(1.2), 0.2, 0);
        }
        if h.bounce > 0 {
            let mut at = p;
            let mut claimed = vec![p.y as usize * W + p.x as usize];
            for _ in 0..h.bounce {
                let from = at;
                let Some(k) = self.target_cell(from, 10.0, &claimed, |q| q.distance(from) < 2.5)
                else {
                    break;
                };
                let q = Self::cell_pos(k);
                claimed.push(k);
                self.tracers.push(Tracer {
                    a: from,
                    az: 0.6,
                    b: q,
                    t: 0.07,
                    kind: 0,
                });
                self.blast(q, (h.rad * 0.8).max(1.0), h.dmg * 0.6, 0.0, h.src, 0.0);
                at = q;
            }
        }
        if h.volatile > 0.0 && n > 0 {
            let extra = (n as f32 / 30.0).min(2.0) * h.volatile;
            if extra > 0.05 {
                let r = h.rad * (1.0 + extra);
                self.blast(p, r, h.dmg.max(20.0) * 0.5 * extra, 0.0, h.src, 0.0);
                self.boom_at(p, r, 0.3, 0);
            }
        }
        if h.recycle > 0.0 && n > 0 {
            self.gain_scrap(n as f32 * h.recycle);
        }
    }

    fn chill(&mut self, p: Vec2, r: f32, t: f32) {
        if self.chills.len() < 4000 {
            self.chills.push(Chill { p, r, t });
            self.boom_at(p, r, 0.4, 5);
        }
    }
    fn boom_at(&mut self, p: Vec2, r: f32, t: f32, kind: u8) {
        if self.booms.len() < 4000 {
            self.booms.push(Boom {
                p,
                r,
                t,
                life: t,
                kind,
            });
        }
    }

    /// The turret's shot sound: one of the game's effects, pitched and filtered for its barrel.
    /// Gun barrels crack on every shot like the gun turrets; other fast guns only every so often.
    fn voice(&mut self, i: usize, sp: &Spec, at: Vec2) {
        let gap = if sp.voice.base == 0 {
            0.0
        } else if sp.reload < 0.35 || sp.burst > 2 {
            0.4
        } else {
            0.0
        };
        if self.time - self.structs[i].snd >= gap {
            self.structs[i].snd = self.time;
            self.sfx.push((Sfx::Custom(sp.voice), at));
        }
    }

    /// Shells and rockets in the air, mines on the ground, frost patches.
    fn custom_shots(&mut self) {
        if !self.cshots.is_empty() {
            let mut shots = std::mem::take(&mut self.cshots);
            for s in &mut shots {
                s.t += DT;
                if s.t >= s.dur {
                    self.impact(s.hit, s.to);
                    if s.kind == 1 {
                        self.sfx.push((Sfx::ArtilleryHit, s.to));
                    }
                }
            }
            shots.retain(|s| s.t < s.dur);
            // impacts don't add shots, but keep anything that did
            shots.append(&mut self.cshots);
            self.cshots = shots;
        }
        if !self.mines.is_empty() {
            let mut mines = std::mem::take(&mut self.mines);
            for m in &mut mines {
                m.t -= DT;
                // armed after a second
                if m.t < MINE_LIFE - 1.0
                    && self.swarm.at(m.p.y as usize * W + m.p.x as usize) + self.units_in(m.p, 2.0)
                        > 0
                {
                    let src = 0;
                    self.blast(m.p, 5.0, m.dmg, 2.0, src, 0.0);
                    self.boom_at(m.p, 5.0, 0.3, 0);
                    self.sfx.push((Sfx::Cannon, m.p));
                    m.t = -1.0;
                }
            }
            mines.retain(|m| m.t > 0.0);
            self.mines = mines;
        }
        if !self.chills.is_empty() {
            for ch in &mut self.chills {
                ch.t -= DT;
            }
            self.chills.retain(|c| c.t > 0.0);
            for ch in &self.chills {
                let rr = ch.r / CG as f32;
                let (cx, cy) = (ch.p.x / CG as f32, ch.p.y / CG as f32);
                let (x0, x1) = (
                    (cx - rr).floor().max(0.0) as usize,
                    ((cx + rr).ceil() as usize).min(CW - 1),
                );
                let (y0, y1) = (
                    (cy - rr).floor().max(0.0) as usize,
                    ((cy + rr).ceil() as usize).min(CH - 1),
                );
                for gy in y0..=y1 {
                    for gx in x0..=x1 {
                        if vec2(gx as f32 + 0.5 - cx, gy as f32 + 0.5 - cy).length() <= rr + 0.5 {
                            self.slow[gy * CW + gx] = 1;
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::garage::{find, PARTS, SLOTS};

    fn test_game() -> Game {
        let mut g = Game::new(crate::citygen::generate(11), 0, 1.0, 7);
        g.scrap = 1e6;
        g.ammo = 1e6;
        g.scrap_cap = 1e7;
        g.ammo_cap = 1e7;
        // room for every test turret
        g.set_hq_level(0, HQ_MAX);
        // walled in, as every base used to start
        g.complete_research(0, R::Perimeter);
        g
    }

    /// Every barrel with a few mods and gear actually fires into a horde, deterministically.
    #[test]
    fn every_part_fires_and_stays_deterministic() {
        let run = || {
            let mut g = test_game();
            let hq = g.world.hq;
            g.queue((hq.x + 60.0, hq.y), 3000.0, 2.0);
            g.queue((hq.x - 60.0, hq.y), 3000.0, 2.0);
            let barrels: Vec<u16> = (0..PARTS.len() as u16)
                .filter(|i| garage::part(*i).slot() == garage::Slot::Barrel)
                .collect();
            let heads: Vec<u16> = (0..PARTS.len() as u16)
                .filter(|i| garage::part(*i).slot() == garage::Slot::Head)
                .collect();
            let mods: Vec<u16> = (0..PARTS.len() as u16)
                .filter(|i| garage::part(*i).slot() == garage::Slot::Mod)
                .collect();
            let gear: Vec<u16> = (0..PARTS.len() as u16)
                .filter(|i| garage::part(*i).slot() == garage::Slot::Gear)
                .collect();
            let mounts: Vec<u16> = (0..PARTS.len() as u16)
                .filter(|i| garage::part(*i).slot() == garage::Slot::Mount)
                .collect();
            let mut placed = 0;
            for (k, b) in barrels.iter().enumerate() {
                let mut bl = Build::starter();
                bl.parts[0] = Some(mounts[k % mounts.len()]);
                // heavy enough to carry anything
                if garage::spec_raw(&bl).cap < 25.0 {
                    bl.parts[0] = find("m.fort");
                }
                bl.parts[1] = Some(heads[k % heads.len()]);
                bl.parts[2] = Some(*b);
                bl.parts[3] = Some(mods[k % mods.len()]);
                bl.parts[4] = Some(mods[(k * 7 + 3) % mods.len()]);
                bl.parts[5] = Some(gear[k % gear.len()]);
                bl.parts[6] = Some(gear[(k * 5 + 2) % gear.len()]);
                if !bl.ok() {
                    bl.parts[4] = None;
                    bl.parts[6] = None;
                }
                if !bl.ok() {
                    bl.parts[0] = find("m.fort");
                }
                assert!(bl.ok(), "{:?} {:?}", bl.problem(), bl.code());
                assert!(g.pick_design(&bl));
                let a = (k as f32) * 0.5;
                for r in [20.0f32, 26.0, 32.0] {
                    let x = (hq.x + libm::cosf(a) * r) as i32;
                    let y = (hq.y + libm::sinf(a) * r) as i32;
                    if g.place(S::Custom, x, y) {
                        placed += 1;
                        break;
                    }
                }
            }
            assert!(placed > barrels.len() / 2, "placed {}", placed);
            let ammo0 = g.ammo;
            for _ in 0..400 {
                g.tick();
            }
            assert!(g.ammo < ammo0, "nothing fired");
            assert!(g.kills > 0);
            g.checksum()
        };
        assert_eq!(run(), run());
        let _ = SLOTS;
    }

    /// Each barrel kills on its own, from the nearest-target head on a heavy mount.
    #[test]
    fn every_barrel_kills() {
        let mut weak = Vec::new();
        for (i, p) in PARTS
            .iter()
            .enumerate()
            .filter(|(_, p)| p.slot() == garage::Slot::Barrel)
        {
            let mut g = test_game();
            let hq = g.world.hq;
            let mut b = Build::starter();
            b.parts[0] = find("m.fort");
            b.parts[2] = Some(i as u16);
            assert!(g.pick_design(&b), "{}", p.id);
            let mut ok = false;
            for r in [20.0f32, 26.0, 32.0, 40.0] {
                if g.place(S::Custom, (hq.x + r) as i32, hq.y as i32) {
                    ok = true;
                    break;
                }
            }
            assert!(ok, "{} not placed", p.id);
            let range = g.designs[0].spec.range;
            g.queue((hq.x + 26.0 + range * 0.6, hq.y), 2000.0, 1.0);
            for _ in 0..300 {
                g.tick();
            }
            if g.kills < 20 {
                weak.push((p.id, g.kills));
            }
        }
        assert!(weak.is_empty(), "{:?}", weak);
    }
    #[test]
    fn campaign_parts_are_enforced() {
        let mut g = test_game();
        g.perks[0].parts = Some(vec![find("m.ring1").unwrap(), find("h.optic1").unwrap()]);
        assert!(
            !g.pick_design(&Build::starter()),
            "the rifle barrel isn't owned"
        );
        g.perks[0]
            .parts
            .as_mut()
            .unwrap()
            .push(find("b.rifle1").unwrap());
        assert!(g.pick_design(&Build::starter()));
    }
}

/// How much further a custom turret reaches standing `base` cells up: the higher its gun sits (the
/// roof or bunker plus its own mount, so a mast sees furthest), the more, from +35% on a bunker to
/// at most +120% on a tower.
pub fn height_mult(sp: &Spec, base: f32) -> f32 {
    if base <= 0.0 {
        1.0
    } else {
        (1.25 + 0.035 * (base + sp.height)).min(2.2)
    }
}

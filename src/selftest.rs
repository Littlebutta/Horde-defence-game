//! Headless self-test used during development: builds a base, fires every ability and logs what happened.

use crate::defs::*;
use crate::game::Game;
use crate::{game, world};
use macroquad::prelude::*;

pub fn abilities(g: &mut Game) {
    g.scrap = 1e6;
    g.ammo = 1e6;
    let h = g.world.hq;
    let put = |g: &mut game::Game, t: S, dx: i32, dy: i32| {
        for k in 0..40 {
            if g.place(t, h.x as i32 + dx + k * 3, h.y as i32 + dy) {
                return;
            }
        }
        eprintln!("could not place {:?}", t);
    };
    put(g, S::Silo, 30, 20);
    put(g, S::Silo, 30, -40);
    put(g, S::Pad, -60, 30);
    put(g, S::Lab, -60, -40);
    // aim at the biggest group of civilians not too far out
    let far = {
        let pts: Vec<Vec2> = g
            .civs
            .iter()
            .map(|c| c.p)
            .filter(|p| p.distance(h) > 120.0 && p.distance(h) < 320.0)
            .collect();
        *pts.iter()
            .max_by_key(|p| pts.iter().filter(|q| q.distance(**p) < 10.0).count())
            .unwrap()
    };
    eprintln!("rescue {}", g.use_ability(A::Rescue, far, far));
    for _ in 0..500 {
        g.tick();
    }
    let civ = |g: &game::Game| {
        (0..16u8)
            .map(|s| g.civs.iter().filter(|c| c.st == s).count())
            .collect::<Vec<_>>()
    };
    eprintln!(
        "civ states after 25 s: {:?} squads {:?}",
        civ(g),
        g.squads
            .iter()
            .map(|q| (q.found, q.done))
            .collect::<Vec<_>>()
    );
    for _ in 0..900 {
        g.tick();
    }
    eprintln!(
        "civ states after 70 s: {:?} rescued {} waiting {} out {}",
        civ(g),
        g.rescued,
        g.waiting,
        g.extracted
    );
    if let Some(c) = g.civs.iter().find(|c| c.st == game::C_TO_PAD) {
        let f = g.hq_dist[c.p.y as usize * world::W + c.p.x as usize];
        eprintln!("to-hq civ at {:?} field {} hq {:?}", c.p, f, h);
    }
    let t = h + vec2(300.0, 200.0);
    eprintln!(
        "icbm {} cd {:?}",
        g.use_ability(A::Icbm, t, t),
        g.silos_ready()
    );
    eprintln!(
        "icbm2 {} cd {:?}",
        g.use_ability(A::Icbm, t, t),
        g.silos_ready()
    );
    eprintln!("icbm3 {}", g.use_ability(A::Icbm, t, t));
    let a = h + vec2(-120.0, -60.0);
    eprintln!("air {}", g.use_ability(A::Air, a, a));
    eprintln!(
        "gunship {}",
        g.use_ability(A::Gunship, h + vec2(60.0, 60.0), h + vec2(60.0, 60.0))
    );
    eprintln!("rifles {}", g.deploy(U::Rifle, h + vec2(-40.0, 0.0)));
    eprintln!("tank {}", g.deploy(U::Tank, h + vec2(40.0, 70.0)));
    eprintln!(
        "far turret {}",
        g.place(S::Gun, h.x as i32 + 200, h.y as i32 + 3)
    );
    let n: u32 = std::env::var("PH_AFTER")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);
    for _ in 0..n {
        g.tick();
    }
}

/// PH_GALLERY=1: one of every structure around the HQ, for screenshots of their art.
pub fn gallery(g: &mut Game) {
    g.scrap = 1e6;
    g.ammo = 1e6;
    let h = g.world.hq;
    let put = |g: &mut game::Game, t: S, dx: i32, dy: i32| {
        for k in 0..40 {
            if g.place(t, h.x as i32 + dx + (k % 8) * 3, h.y as i32 + dy + (k / 8) * 3) {
                return;
            }
        }
        eprintln!("could not place {:?}", t);
    };
    put(g, S::Gun, 14, -6);
    put(g, S::Cannon, 14, 4);
    put(g, S::Extractor, -24, -8);
    put(g, S::Plant, -24, 4);
    put(g, S::Silo, 26, -10);
    put(g, S::Pad, -14, 16);
    put(g, S::Outpost, 2, 16);
    put(g, S::Lab, 26, 4);
    for k in 0..6 {
        put(g, S::Wall, -12 + k * 3, -16);
    }
    put(g, S::Flame, -30, 30);
    put(g, S::Rail, -20, 30);
    put(g, S::Helium, -9, 30);
    put(g, S::Artillery, 1, 30);
    put(g, S::Helipad, 13, 30);
    put(g, S::Rescue, 28, 30);
    put(g, S::ScrapDepot, -30, 44);
    put(g, S::AmmoDepot, -20, 44);
    put(g, S::Bunker, -10, 44);
    for _ in 0..40 {
        g.tick();
    }
}

/// Headless check of the special towers (`PH_TOWERS=1`): builds one of each, throws a horde at
/// them and logs what each did.
pub fn towers(g: &mut Game) {
    g.scrap = 1e6;
    g.ammo = 1e6;
    let h = g.world.hq;
    let put = |g: &mut game::Game, t: S, dx: i32, dy: i32| -> Option<usize> {
        for k in 0..60 {
            let (x, y) = (h.x as i32 + dx + (k % 10) * 3, h.y as i32 + dy + (k / 10) * 3);
            if g.place(t, x, y) {
                return Some(g.structs.len() - 1);
            }
        }
        eprintln!("could not place {:?}", t);
        None
    };
    let flame = put(g, S::Flame, 20, -40);
    let rail = put(g, S::Rail, 70, -15);
    let helium = put(g, S::Helium, 0, -50);
    let arty = put(g, S::Artillery, 40, 20);
    let pad = put(g, S::Helipad, -50, 20);
    let station = put(g, S::Rescue, -40, -10);
    eprintln!("placed flame {:?} rail {:?} helium {:?} arty {:?} pad {:?} station {:?}", flame, rail, helium, arty, pad, station);
    let far = h + vec2(0.0, -260.0);
    if let Some(a) = arty {
        eprintln!("aim artillery {}", g.aim(a, far));
    }
    if let Some(p) = pad {
        eprintln!("send gunship {}", g.aim(p, h + vec2(120.0, 0.0)));
    }
    let ammo0 = g.ammo;
    let kills0 = g.kills;
    let horde = if std::env::var("PH_TOWER_HORDE").is_ok_and(|v| v == "0") { 0.0 } else { 3000.0 };
    for (dx, dy) in [(0.0, -110.0), (-60.0, -100.0), (60.0, -100.0)] {
        g.queue((h.x + dx, h.y + dy), horde, 4.0);
    }
    if let (Some(f), true) = (flame, horde > 0.0) {
        let p = g.structs[f].center() + vec2(0.0, -16.0);
        g.queue((p.x, p.y), 200.0, 1.0);
    }
    let n: u32 = std::env::var("PH_TOWER_TICKS").ok().and_then(|s| s.parse().ok()).unwrap_or(600);
    for t in 0..n {
        g.tick();
        if t % 100 == 99 {
            let slowed = g.slow.iter().filter(|v| **v != 0).count();
            let fires = g.napalm.len();
            eprintln!(
                "t {:.0}s: kills {} ammo used {:.0} shells {} slow cells {} napalm {} rail charge {:?} gunships {} station squads {} sfx {} orc regroups {}",
                g.time,
                g.kills - kills0,
                ammo0 - g.ammo,
                g.shells.len(),
                slowed,
                fires,
                rail.map(|r| (g.structs[r].charge, g.structs[r].cd)),
                g.gunships.len(),
                g.squads.iter().filter(|q| q.from >= 0).count(),
                g.sfx.len(),
                g.regroups.len()
            );
        }
    }
    for (n, i) in [("flame", flame), ("rail", rail), ("helium", helium), ("arty", arty), ("pad", pad), ("station", station)] {
        if let Some(i) = i {
            let s = &g.structs[i];
            eprintln!("{} at {:?}: hp {:.0}/{:.0} dead {}", n, s.center() - h, s.hp, s.max, s.dead);
        }
    }
    let st: Vec<_> = g.squads.iter().filter(|q| q.from >= 0).map(|q| (q.found, q.done, q.wiped)).collect();
    eprintln!("station squads (found, done, wiped): {:?}; rescued {} waiting {}", st, g.rescued, g.waiting);
    if let Some(p) = pad {
        eprintln!("pad cd {:.1} gunship {:?}", g.structs[p].cd, g.pad_gunship(p).map(|s| (s.phase, s.t)));
    }
}

/// Headless check of troops (`PH_UNITS=1`): a squad spreading out, squads manning a bunker with a
/// mounted turret, a garrisoned building and bunker collapsing, a tank, and a B-2 run.
pub fn units(g: &mut Game) {
    g.scrap = 1e6;
    g.ammo = 1e6;
    let h = g.world.hq;
    let spread = |g: &Game, gi: usize| {
        let ms: Vec<Vec2> = g.groups[gi].m.iter().filter(|m| m.alive).map(|m| m.p).collect();
        let mut close = f32::MAX;
        for a in 0..ms.len() {
            for b in a + 1..ms.len() {
                close = close.min(ms[a].distance(ms[b]));
            }
        }
        (ms.len(), close)
    };
    // a bunker, a turret on it, two squads inside
    let mut bunker = None;
    'b: for k in 0..80 {
        let (x, y) = (h.x as i32 + 20 + (k % 10) * 3, h.y as i32 - 30 + (k / 10) * 3);
        if g.place(S::Bunker, x, y) {
            bunker = Some(g.structs.len() - 1);
            break 'b;
        }
    }
    let Some(bi) = bunker else {
        eprintln!("could not place a bunker");
        return;
    };
    let bc = g.structs[bi].center();
    eprintln!("mount before research {}", g.place_mount(S::Gun, bi));
    g.complete_research(0, R::Mounts);
    eprintln!("mount {} second mount {}", g.place_mount(S::Gun, bi), g.place_mount(S::Cannon, bi));
    let gun = g.structs.len() - 1;
    eprintln!("gun on bunker: base {:.1} range {:.0} occupies ground {}", g.struct_base(gun), g.turret_range(S::Gun, g.structs[gun].up(), 0), !g.structs[gun].up());
    let n0 = g.groups.len();
    eprintln!("rifles to bunker {} {}", g.deploy(U::Rifle, bc), g.deploy(U::Rifle, bc));
    // a squad to open ground, and one into a city building
    let street = h + vec2(-50.0, 0.0);
    eprintln!("rifles to street {}", g.deploy(U::Rifle, street));
    let bld = g.world.buildings.iter().enumerate().filter(|(_, b)| b.alive && b.c.distance(h) > 60.0 && b.c.distance(h) < 400.0).min_by(|a, b| a.1.c.distance(h).total_cmp(&b.1.c.distance(h))).map(|(i, b)| (i, b.c));
    if let Some((_, c)) = bld {
        eprintln!("rifles to building {}", g.deploy(U::Rifle, c));
    }
    eprintln!("tank {}", g.deploy(U::Tank, h + vec2(40.0, 60.0)));
    let ammo0 = g.ammo;
    for _ in 0..900 {
        g.tick();
    }
    for gi in n0..g.groups.len() {
        let gr = &g.groups[gi];
        eprintln!("group {} {:?} {:?} dead {} b {} sb {} alive/closest {:?}", gi, gr.kind, gr.state == game::GState::Garrison, gr.dead, gr.b, gr.sb, spread(g, gi));
    }
    eprintln!("ammo used by troops {:.1}", ammo0 - g.ammo);
    // knock the garrisoned building and the bunker down
    let inside = |g: &Game, f: &dyn Fn(&game::Group) -> bool| g.groups.iter().enumerate().find(|(_, gr)| !gr.dead && gr.state == game::GState::Garrison && f(gr)).map(|(i, _)| i);
    if let Some(gi) = inside(g, &|gr| gr.b >= 0) {
        let before = spread(g, gi).0;
        let b = g.groups[gi].b as usize;
        g.destroy_building(b);
        g.tick();
        eprintln!("building fell: {} inside, {} left standing {:?}", before, spread(g, gi).0, g.groups[gi].state == game::GState::Hold);
    }
    if let Some(gi) = inside(g, &|gr| gr.sb >= 0) {
        let before = spread(g, gi).0;
        g.kill_struct(bi);
        g.tick();
        eprintln!("bunker fell: {} inside, {} left standing; mounted gun dead {}", before, spread(g, gi).0, g.structs[gun].dead);
    }
    let a = h + vec2(-150.0, -80.0);
    eprintln!("b2 {}", g.use_ability(A::Air, a, a + vec2(200.0, 0.0)));
    if let Some(game::Strike::Air { bombs, .. }) = g.strikes.last() {
        let xs: Vec<f32> = bombs.iter().map(|b| (b.0 - a).x.round()).collect();
        eprintln!("bombs at {:?}", xs);
    }
    for _ in 0..40 {
        g.tick();
    }
}

/// Headless check of orc regroups (`PH_ORCS=1`, with PH_FAC=2): a horde walks at two gun turrets;
/// logs how far the orcs are from the HQ and the regroups running, every second.
pub fn orcs(g: &mut Game) {
    g.scrap = 1e6;
    g.ammo = 1e6;
    let h = g.world.hq;
    for dx in [-20, 14] {
        for k in 0..40 {
            if g.place(S::Gun, h.x as i32 + dx + (k % 8) * 3, h.y as i32 - 40 - (k / 8) * 3) {
                break;
            }
        }
    }
    g.queue((h.x, h.y - 85.0), 2500.0, 3.0);
    for t in 0..400 {
        g.tick();
        if t % 20 == 19 {
            let (mut n, mut d) = (0, 0.0);
            for u in g.swarm.units() {
                n += 1;
                d += vec2(u.x, u.y).distance(h);
            }
            // each regroup: its age, and how far its orcs (those near the loss or the rally point) are from the rally point
            let phases: Vec<String> = g
                .regroups
                .iter()
                .map(|r| {
                    let (mut k, mut dd) = (0, 0.0);
                    for u in g.swarm.units() {
                        let p = vec2(u.x, u.y);
                        if p.distance(r.from) < 40.0 || p.distance(r.to) < 40.0 {
                            k += 1;
                            dd += p.distance(r.to);
                        }
                    }
                    format!("{:.0}s:{}@{:.0}", r.t, k, dd / k.max(1) as f32)
                })
                .collect();
            eprintln!("t {:.0}s orcs {} mean dist {:.0} kills {} regroups [{}]", g.time, n, d / n.max(1) as f32, g.kills, phases.join(" "));
        }
    }
    eprintln!("orcs checksum {:016x}", g.checksum());
}

/// `PH_BUILD=<level>`: the HQ at that level, a few structures half built behind scaffolding and a
/// war-drum orc marching in, for checking their art.
pub fn builds(g: &mut Game, level: u8) {
    g.scrap = 1e6;
    g.scrap_cap = 1e7;
    g.set_hq_level(0, level);
    g.set_hq_level(0, level);
    let h = g.world.hq;
    let mut n = 0;
    for (t, dx, dy) in [(S::Cannon, 24, -14), (S::Gun, 24, 4), (S::Extractor, -36, 14), (S::Bunker, -20, -26)] {
        for k in 0..40 {
            if g.place(t, h.x as i32 + dx + (k % 8) * 3, h.y as i32 + dy + (k / 8) * 3) {
                n += 1;
                break;
            }
        }
        // each a little further along than the last
        for _ in 0..8 {
            g.tick();
        }
    }
    eprintln!("{} builds going up at HQ level {}", n, g.hq_level(0));
    if let Some(p) = (0..40).map(|k| h + vec2(-30.0 - k as f32, -30.0)).find(|p| g.world.bid[p.y as usize * world::W + p.x as usize] < 0) {
        g.drummers.push(game::Drummer { p, hp: 2000.0, max: 2400.0, hd: 0.6, beat: 0.4, dead: false });
    }
}

/// PH_CUSTOMS=1: a ring of varied garage designs round the HQ, with a horde coming at them.
pub fn customs(g: &mut Game) {
    use crate::garage::{self, Build, Slot, PARTS};
    g.scrap = 1e6;
    g.ammo = 1e6;
    g.scrap_cap = 1e7;
    g.set_hq_level(0, HQ_MAX);
    let h = g.world.hq;
    let of = |s: Slot| -> Vec<u16> {
        (0..PARTS.len() as u16)
            .filter(|i| PARTS[*i as usize].slot() == s)
            .collect()
    };
    let (mounts, heads, barrels, mods, gear) = (
        of(Slot::Mount),
        of(Slot::Head),
        of(Slot::Barrel),
        of(Slot::Mod),
        of(Slot::Gear),
    );
    let mut placed = 0;
    for k in 0..10usize {
        let mut b = Build::starter();
        b.name = format!("Test {k}");
        b.parts = [
            Some(mounts[(k * 7 + 3) % mounts.len()]),
            Some(heads[(k * 5 + 2) % heads.len()]),
            Some(barrels[(k * 3 + 1) % barrels.len()]),
            Some(mods[(k * 11) % mods.len()]),
            None,
            Some(gear[(k * 13 + 4) % gear.len()]),
            None,
        ];
        b.paint = [
            [60 + k as u8 * 18, 90, 70],
            [200, 200 - k as u8 * 12, 60],
            [40, 40, 48],
        ];
        // drop whatever doesn't fit until it's legal
        for s in [5, 3] {
            if !b.ok() {
                b.parts[s] = None;
            }
        }
        if !b.ok() || !g.pick_design(&b) {
            eprintln!("design {k} refused: {:?}", b.problem());
            continue;
        }
        let a = k as f32 / 10.0 * std::f32::consts::TAU;
        let at = h + vec2(a.cos(), a.sin()) * 46.0;
        let ok = (0..30).any(|j| {
            g.place(
                S::Custom,
                at.x as i32 + (j % 6) * 4 - 10,
                at.y as i32 + (j / 6) * 4 - 10,
            )
        });
        placed += ok as u32;
        eprintln!(
            "{} {:?}: {}",
            b.name,
            garage::spec(&b).map(|s| (s.fire, s.aim)),
            if ok { "placed" } else { "no room" }
        );
    }
    eprintln!("placed {placed} custom turrets");
    // ground mounts on rooftops: a fortress with two of the same plates, and a walker
    for (k, (m, x)) in [("m.fort", "g.plate1"), ("m.walk1", "g.plate1")].iter().enumerate() {
        let mut b = Build::starter();
        b.name = format!("Roof {k}");
        b.parts[0] = garage::find(m);
        b.parts[5] = garage::find(x);
        b.parts[6] = garage::find(x);
        if !b.ok() || !g.pick_design(&b) {
            eprintln!("roof design {k} refused: {:?}", b.problem());
            continue;
        }
        let ground = g.turret_range(S::Custom, false, 0);
        let spot = (0..g.world.buildings.len())
            .filter(|bi| g.world.buildings[*bi].alive)
            .filter(|bi| g.world.buildings[*bi].cells > 60)
            .find_map(|bi| {
                let bb = &g.world.buildings[bi];
                let (x0, y0, x1, y1) = (bb.x0 as i32, bb.y0 as i32, bb.x1 as i32, bb.y1 as i32);
                (y0..=y1).step_by(2).find_map(|y| {
                    (x0..=x1).step_by(2).find_map(|x| {
                        let c = vec2(x as f32 + 0.5, y as f32 + 0.5);
                        g.can_place_roof(S::Custom, bi, c).then_some((bi, c))
                    })
                })
            });
        match spot {
            Some((bi, c)) => {
                let ok = g.place_roof(S::Custom, bi, c);
                eprintln!(
                    "{} on a {:.0}-high roof: placed {} range {:.0} (ground {:.0}) hp {:.0}",
                    b.name,
                    g.world.buildings[bi].h,
                    ok,
                    g.turret_range_on(S::Custom, g.world.buildings[bi].h, 0),
                    ground,
                    garage::spec(&b).map_or(0.0, |s| s.hp)
                );
            }
            None => eprintln!("{}: no roof found", b.name),
        }
    }
    for (dx, dy) in [(0.0, -130.0), (-90.0, -100.0), (100.0, 80.0)] {
        g.queue((h.x + dx, h.y + dy), 2500.0, 4.0);
    }
    let n: u32 = std::env::var("PH_CUSTOM_TICKS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(260);
    let k0 = g.kills;
    for _ in 0..n {
        g.tick();
    }
    eprintln!(
        "custom turrets killed {} in {} ticks, {} shots in the air",
        g.kills - k0,
        n,
        g.cshots.len()
    );
}

/// A fresh campaign's first city played by a simple bot that spends the starting research on the
/// gun turret (extractors, rifles, a rescue station, evac points, then gun turrets), to check a new
/// player can hold it. Prints the HQ's health every minute.
#[cfg(test)]
pub fn first_city(seed: u64, diff: usize, bot: bool) -> (bool, f32, u32) {
    let mut c = crate::campaign::Campaign::with_seed(diff, seed);
    let gun = c.buy(0, crate::campaign::Tech::Build(S::Gun));
    let d = crate::citygen::generate(seed);
    let mut g = Game::new(d, c.army(), crate::app::DIFFS[c.diff].1 * c.horde_mult(), seed);
    g.add_player_hqs_at(1, &[]);
    c.apply(&mut g);
    let h = g.world.hq;
    let mut k = 0i32;
    let mut t = 0u32;
    while !g.over && t < 20 * 60 * 15 {
        g.tick();
        t += 1;
        if bot && t % 40 == 0 {
            let ext = g.structs.iter().filter(|s| !s.dead && s.t == S::Extractor).count();
            k += 1;
            let ang = k as f32 * 2.399_963;
            let at = |r: f32| h + vec2(libm::cosf(ang), libm::sinf(ang)) * r;
            let has = |t: S| g.structs.iter().filter(|s| !s.dead && s.t == t).count();
            if ext < 3 {
                let p = at(25.0 + (k % 5) as f32 * 4.0);
                g.place(S::Extractor, p.x as i32, p.y as i32);
            } else if g.groups.iter().filter(|q| !q.dead).count() < 5 {
                g.deploy(U::Rifle, at(18.0));
            } else if has(S::Rescue) < 1 {
                let p = at(22.0 + (k % 5) as f32 * 4.0);
                g.place(S::Rescue, p.x as i32, p.y as i32);
            } else if has(S::Pad) < 3 {
                let p = at(20.0 + (k % 5) as f32 * 4.0);
                g.place(S::Pad, p.x as i32, p.y as i32);
            } else if g.groups.iter().filter(|q| !q.dead).count() < 8 {
                g.deploy(U::Rifle, at(18.0));
            } else if g.hq_level(0) < 2 && g.scrap > 1100.0 {
                g.upgrade_hq();
            } else {
                let p = at(14.0 + (k % 4) as f32 * 3.0);
                if gun {
                    if g.structs.iter().filter(|s| !s.dead && s.t == S::Plant).count() < 1 {
                        g.place(S::Plant, p.x as i32 + 8, p.y as i32);
                    } else {
                        g.place(S::Gun, p.x as i32, p.y as i32);
                    }
                } else if g.place(S::Bunker, p.x as i32, p.y as i32) {
                    // man it
                    if let Some(gi) = g.groups.iter().position(|q| !q.dead && q.state != crate::game::GState::Garrison) {
                        g.order(gi, p + vec2(1.0, 1.0));
                    }
                }
            }
        }
        if t % 1200 == 0 {
            let hq = &g.structs[g.hq_idx];
            eprintln!("  {} min: hq {:.0}/{:.0} hostiles {} scrap {:.0} squads {} flown {}", t / 1200, hq.hp, hq.max, g.hostiles(), g.scrap, g.groups.iter().filter(|q| !q.dead).count(), g.extracted);
        }
    }
    (g.won, g.time, g.extracted)
}

#[cfg(test)]
mod tests {
    /// cargo test --release first_city -- --ignored --nocapture
    #[test]
    #[ignore]
    fn first_city() {
        for diff in [0usize, 1, 2] {
            for seed in [3u64, 8] {
                for bot in [true] {
                    let r = super::first_city(seed, diff, bot);
                    eprintln!("diff {diff} seed {seed} bot {bot}: won {} at {:.0}s, {} flown out", r.0, r.1, r.2);
                }
            }
        }
    }
}

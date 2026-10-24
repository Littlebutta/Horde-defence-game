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
                "t {:.0}s: kills {} ammo used {:.0} shells {} slow cells {} napalm {} rail charge {:?} gunships {} station squads {} sfx {}",
                g.time,
                g.kills - kills0,
                ammo0 - g.ammo,
                g.shells.len(),
                slowed,
                fires,
                rail.map(|r| (g.structs[r].charge, g.structs[r].cd)),
                g.gunships.len(),
                g.squads.iter().filter(|q| q.from >= 0).count(),
                g.sfx.len()
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

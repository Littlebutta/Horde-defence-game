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
        (0..14u8)
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
    if let Some(c) = g.civs.iter().find(|c| c.st == 11) {
        let f = g
            .pad_field
            .as_ref()
            .map(|f| f[c.p.y as usize * world::W + c.p.x as usize]);
        eprintln!("to-pad civ at {:?} field {:?} hq {:?}", c.p, f, h);
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

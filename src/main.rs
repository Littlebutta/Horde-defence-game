#![windows_subsystem = "windows"]
//! Pixel Horde: a horde-defence game where every enemy is a single pixel.

mod app;
mod campaign;
mod citygen;
mod defs;
mod fields;
mod game;
mod garage;
mod globe;
mod menubg;
mod music;
mod net;
mod news;
mod osm;
mod render;
mod selftest;
mod storage;
mod swarm;
mod ui;
mod update;
mod world;

use macroquad::prelude::*;

fn conf() -> macroquad::conf::Conf {
    macroquad::conf::Conf {
        miniquad_conf: window_conf(),
        update_on: None,
        default_filter_mode: FilterMode::Linear,
        // room for city meshes that grow when they are clipped to the map border. macroquad batches
        // meshes with 16-bit indices, so a draw call must stay under 65536 vertices or they wrap.
        draw_call_vertex_capacity: 60_000,
        draw_call_index_capacity: 150_000,
    }
}

fn window_conf() -> Conf {
    Conf {
        window_title: "Pixel Horde".to_owned(),
        window_width: std::env::var("PH_W").ok().and_then(|s| s.parse().ok()).unwrap_or(1600),
        window_height: std::env::var("PH_H").ok().and_then(|s| s.parse().ok()).unwrap_or(900),
        high_dpi: false,
        sample_count: if std::env::var("PH_SHOT").is_ok() {
            1
        } else {
            4
        },
        window_resizable: true,
        // four squares in the army colours (tools/make_icon.py); the exe's own icon comes from build.rs
        icon: Some(macroquad::miniquad::conf::Icon {
            small: *include_bytes!("../assets/icon16.rgba"),
            medium: *include_bytes!("../assets/icon32.rgba"),
            big: *include_bytes!("../assets/icon64.rgba"),
        }),
        fullscreen: std::env::var("PH_SHOT").is_err() && storage::load_settings().fullscreen,
        ..Default::default()
    }
}

fn main() {
    // PH_FETCH=<dir>: download the campaign cities for the `cities` release (GitHub workflow), no window
    if let Ok(dir) = std::env::var("PH_FETCH") {
        std::process::exit(if campaign::fetch_all(std::path::Path::new(&dir)) { 0 } else { 1 });
    }
    // Steam has to start before the window exists, or its overlay (the invite window) can't hook in
    let steam = net::Steam::init();
    macroquad::Window::from_config(conf(), run(steam));
}

async fn run(steam: Option<net::Steam>) {
    let mut app = app::App::new(steam);
    // PH_SHOT=<png> renders a test game headlessly and saves a screenshot (used in CI-less testing)
    let shot = std::env::var("PH_SHOT").ok();
    let menu_shot = std::env::var("PH_MENU").is_ok();
    // PH_CAMP=<city 0-10>: the test game is that campaign city, with a small veteran army; PH_HUB=1 shows the campaign screen
    let camp: Option<usize> = std::env::var("PH_CAMP").ok().and_then(|s| s.parse().ok());
    if std::env::var("PH_HUB").is_ok() {
        app.test_campaign(camp.unwrap_or(3), false);
    } else if let (Some(l), Some(_), false) = (camp, &shot, menu_shot) {
        app.test_campaign(l, true);
    } else if shot.is_some() && !menu_shot {
        let fac = std::env::var("PH_FAC")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let seed = std::env::var("PH_SEED")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(42);
        app.quick_start(fac, seed);
    }
    // PH_GARAGE=1: the garage open (the campaign's with PH_HUB, the free builder with PH_MENU)
    if std::env::var("PH_GARAGE").is_ok() {
        app.open_garage_test();
    }
    let ticks: u32 = std::env::var("PH_TICKS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(200);
    let mut frame = 0u32;
    // PH_NETTEST=<ticks> with PH_HOST/PH_JOIN: a headless co-op run that logs checksums, then quits
    let net_test: Option<u64> = std::env::var("PH_NETTEST").ok().and_then(|s| s.parse().ok());
    loop {
        if let Some(n) = net_test {
            if app.net_test(n) {
                eprintln!("net test done");
                break;
            }
        }
        if let (Some(_), Some(g)) = (&shot, app.game_mut()) {
            // run the sim fast before the shot
            if frame == 1 {
                let t0 = std::time::Instant::now();
                for _ in 0..ticks {
                    g.tick();
                }
                eprintln!(
                    "{} ticks in {:.2}s, {} hostiles, sim {:.1} ms/tick, {} buildings of {}",
                    ticks,
                    t0.elapsed().as_secs_f32(),
                    g.hostiles(),
                    g.sim_ms,
                    g.world.buildings.len(),
                    g.world.desc.buildings.len()
                );
                if let Ok(v) = std::env::var("PH_STRESS") {
                    if v == "1" {
                        g.stress();
                        // PH_RUSH=1: zombies rush the HQ the whole time
                        if std::env::var("PH_RUSH").is_ok() {
                            g.rush = true;
                            g.rush_forever = true;
                        }
                        let n: u32 = std::env::var("PH_STRESS_TICKS").ok().and_then(|s| s.parse().ok()).unwrap_or(140);
                        // PH_TRACE=1: every 20 ticks, how far the horde is from the HQ on average (to compare movement)
                        let trace = std::env::var("PH_TRACE").is_ok();
                        for i in 0..n + if trace { 400 } else { 0 } {
                            g.tick();
                            if trace && i % 20 == 19 {
                                let (mut sum, mut cnt, mut near) = (0f64, 0f64, 0f64);
                                for c in 0..world::N {
                                    let d = g.swarm.at(c) as f64;
                                    let h = g.hq_dist[c];
                                    if d > 0.0 && h != fields::INF {
                                        sum += d * h as f64;
                                        cnt += d;
                                        if h < 3000 {
                                            near += d;
                                        }
                                    }
                                }
                                eprintln!("trace tick {}: {} hostiles, {} entities, mean HQ distance {:.0}, {:.0} near the HQ", i + 1, g.hostiles(), g.swarm.entities(), sum / cnt.max(1.0), near);
                            }
                        }
                        let t1 = std::time::Instant::now();
                        for _ in 0..100 {
                            g.tick();
                        }
                        eprintln!(
                            "stress: {} hostiles in {} clumps and units, {:.1} ms/tick over the last 100 ticks, checksum {:016x}",
                            g.hostiles(),
                            g.swarm.entities(),
                            t1.elapsed().as_secs_f32() * 10.0,
                            g.checksum()
                        );
                        if std::env::var("PH_HIST").is_ok() {
                            let mut h = [0usize; 12];
                            let mut cells = 0;
                            for c in 0..world::N {
                                let d = g.swarm.at(c) as usize;
                                if d > 0 {
                                    cells += 1;
                                    let b = (usize::BITS - d.leading_zeros()) as usize;
                                    h[b.min(11)] += d;
                                }
                            }
                            eprintln!("occupied cells {cells}; units by cell density (1,2-3,4-7,8-15,...): {:?}", &h[1..]);
                            let (mut e, mut m) = ([0usize; 3], [0usize; 3]);
                            for u in g.swarm.units() {
                                let k = if u.fl & swarm::F_BLOCK != 0 { 2 } else if u.n > 1 { 1 } else { 0 };
                                e[k] += 1;
                                m[k] += u.n as usize;
                            }
                            eprintln!("singles/clumps/blocks: entities {:?}, members {:?}", e, m);
                            let mut sh = [0usize; 12];
                            for u in g.swarm.units() {
                                if u.n == 1 && u.fl & swarm::F_BLOCK == 0 {
                                    let d = g.swarm.at(u.y as usize * world::W + u.x as usize) as usize;
                                    sh[((usize::BITS - d.leading_zeros()) as usize).min(11)] += 1;
                                }
                            }
                            let mut ih = [0usize; 12];
                            for y in 1..world::H - 1 {
                                for x in 1..world::W - 1 {
                                    let mut sum = 0;
                                    for c in [(y - 1) * world::W + x - 1, (y - 1) * world::W + x, (y - 1) * world::W + x + 1, y * world::W + x - 1, y * world::W + x, y * world::W + x + 1, (y + 1) * world::W + x - 1, (y + 1) * world::W + x, (y + 1) * world::W + x + 1] {
                                        sum += g.swarm.at(c);
                                    }
                                    if sum >= 24 * 9 {
                                        let d = g.swarm.at(y * world::W + x) as usize;
                                        ih[if d == 0 { 0 } else { ((usize::BITS - d.leading_zeros()) as usize).min(11) }] += 1;
                                    }
                                }
                            }
                            eprintln!("cells inside crowds by their own crowd (0,1,2-3,4-7,...): {:?}", ih);
                            eprintln!("singles by their cell's crowd (0,1,2-3,4-7,...): {:?}", sh);
                        }
                    }
                }
                if std::env::var("PH_SCRIPT").is_ok() {
                    selftest::abilities(g);
                }
                if std::env::var("PH_GALLERY").is_ok() {
                    selftest::gallery(g);
                }
                if std::env::var("PH_TOWERS").is_ok() {
                    selftest::towers(g);
                }
                if std::env::var("PH_CUSTOMS").is_ok() {
                    selftest::customs(g);
                }
                if std::env::var("PH_UNITS").is_ok() {
                    selftest::units(g);
                }
                if std::env::var("PH_ORCS").is_ok() {
                    selftest::orcs(g);
                }
                if let Some(l) = std::env::var("PH_BUILD").ok().and_then(|v| v.parse().ok()) {
                    selftest::builds(g, l);
                }
                let mut hq = g.world.hq;
                // PH_LOOK=air|gunship|heli|icbm: look at the first one in flight instead
                match std::env::var("PH_LOOK").as_deref() {
                    Ok("air") => {
                        if let Some(game::Strike::Air { from, dir, pos, .. }) = g.strikes.iter().find(|s| matches!(s, game::Strike::Air { .. })) {
                            hq = *from + *dir * *pos;
                        }
                    }
                    Ok("icbm") => {
                        if let Some(game::Strike::Icbm { from, to, t, dur, .. }) = g.strikes.iter().find(|s| matches!(s, game::Strike::Icbm { .. })) {
                            hq = from.lerp(*to, t / dur);
                        }
                    }
                    Ok("gunship") => {
                        if let Some(s) = g.gunships.first() {
                            hq = s.p;
                        }
                    }
                    Ok("heli") => {
                        if let Some(h) = g.helis.first() {
                            hq = h.from.lerp(h.to, (h.t / h.dur).clamp(0.0, 1.0));
                        }
                    }
                    _ => {}
                }
                // PH_HIVE=1: look at the first alien hive instead
                if let (true, Some(h)) = (std::env::var("PH_HIVE").is_ok(), g.hives.iter().find(|h| !h.dead)) {
                    hq = h.p;
                }
                if let Some(v) = std::env::var("PH_AT").ok() {
                    let v: Vec<f32> = v.split(',').filter_map(|x| x.parse().ok()).collect();
                    if v.len() == 2 {
                        hq += vec2(v[0], v[1]);
                    }
                }
                let z: f32 = std::env::var("PH_ZOOM")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(2.5);
                app.set_view(hq, z);
                let ang = |k: &str, d: f32| std::env::var(k).ok().and_then(|s| s.parse().ok()).unwrap_or(d);
                app.set_angles(ang("PH_YAW", 0.0), ang("PH_PITCH", 0.95));
            }
        }
        // PH_RESUME=1: Menu, garage, back and Resume during the test game (see App::resume_test)
        if std::env::var("PH_RESUME").is_ok() && app.resume_test(frame) {
            break;
        }
        app.frame();
        if let (Some(path), true) = (&shot, frame == if menu_shot { 40 } else { std::env::var("PH_FRAMES").ok().and_then(|s| s.parse().ok()).unwrap_or(4) }) {
            get_screen_data().export_png(path);
            break;
        }
        frame += 1;
        next_frame().await;
    }
}

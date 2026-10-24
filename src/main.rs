#![windows_subsystem = "windows"]
//! Pixel Horde: a horde-defence game where every enemy is a single pixel.

mod app;
mod citygen;
mod defs;
mod fields;
mod game;
mod osm;
mod render;
mod selftest;
mod storage;
mod swarm;
mod update;
mod world;

use macroquad::prelude::*;

fn conf() -> Conf {
    Conf {
        window_title: "Pixel Horde".to_owned(),
        window_width: 1600,
        window_height: 900,
        high_dpi: false,
        sample_count: if std::env::var("PH_SHOT").is_ok() {
            1
        } else {
            4
        },
        window_resizable: true,
        ..Default::default()
    }
}

#[macroquad::main(conf)]
async fn main() {
    let mut app = app::App::new();
    // PH_SHOT=<png> renders a test game headlessly and saves a screenshot (used in CI-less testing)
    let shot = std::env::var("PH_SHOT").ok();
    if shot.is_some() {
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
    let ticks: u32 = std::env::var("PH_TICKS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(200);
    let mut frame = 0u32;
    loop {
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
                        for _ in 0..140 {
                            g.tick();
                        }
                        eprintln!(
                            "stress: {} hostiles, sim {:.1} ms/tick",
                            g.hostiles(),
                            g.sim_ms
                        );
                    }
                }
                if std::env::var("PH_SCRIPT").is_ok() {
                    selftest::abilities(g);
                }
                let mut hq = g.world.hq;
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
        app.frame();
        if let (Some(path), true) = (&shot, frame == 4) {
            get_screen_data().export_png(path);
            break;
        }
        frame += 1;
        next_frame().await;
    }
}

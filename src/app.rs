//! Screens, HUD and input: the start menu, the city loader, and the game itself.

use crate::defs::*;
use crate::game::*;
use crate::osm::{self, OsmMsg};
use crate::render::{self, text, text_w, Fonts, Scene, ViewInfo, WorldLabels};
use crate::update::{UpState, Updater};
use crate::world::*;
use macroquad::prelude::*;

#[derive(Clone, Copy, PartialEq)]
enum Tool {
    None,
    Build(S),
    Unit(U),
    Ability(A),
}

#[derive(PartialEq)]
enum Screen {
    Menu,
    Loading,
    Play,
}

const DIFFS: [(&str, f32); 4] = [
    ("Easy", 0.6),
    ("Normal", 1.0),
    ("Hard", 1.6),
    ("Insane", 2.6),
];

pub struct App {
    screen: Screen,
    fonts: Fonts,
    dark: bool,
    fac: usize,
    diff: usize,
    real: bool,
    city: String,
    status: String,
    loader: Option<osm::Loader>,
    game: Option<Game>,
    scene: Option<Scene>,
    cam: Vec2,
    scale: f32,
    tool: Tool,
    press: Option<(Vec2, Vec2)>, // (screen, world) where the left button went down
    panning: bool,
    selected: Option<usize>,
    acc: f32,
    paused: bool,
    updater: Updater,
    clock: f32,
    ui_rects: Vec<Rect>,
    tip: Option<String>,
    msg: (String, f32),
    last_m: Vec2,
}

struct Pal {
    panel: Color,
    panel2: Color,
    text: Color,
    dim: Color,
    accent: Color,
}
fn pal(dark: bool) -> Pal {
    if dark {
        Pal {
            panel: rgba(24, 27, 33, 0.94),
            panel2: rgba(40, 45, 54, 0.98),
            text: rgb(232, 234, 237),
            dim: rgb(150, 156, 166),
            accent: rgb(138, 180, 248),
        }
    } else {
        Pal {
            panel: rgba(255, 255, 255, 0.96),
            panel2: rgba(241, 243, 244, 1.0),
            text: rgb(32, 33, 36),
            dim: rgb(95, 99, 104),
            accent: rgb(26, 115, 232),
        }
    }
}

impl App {
    pub fn new() -> App {
        App {
            screen: Screen::Menu,
            fonts: render::load_fonts(),
            dark: true,
            fac: 0,
            diff: 1,
            real: false,
            city: String::new(),
            status: String::new(),
            loader: None,
            game: None,
            scene: None,
            cam: vec2(W as f32 / 2.0, H as f32 / 2.0),
            scale: 2.0,
            tool: Tool::None,
            press: None,
            panning: false,
            selected: None,
            acc: 0.0,
            paused: false,
            updater: Updater::start(),
            clock: 0.0,
            ui_rects: Vec::new(),
            tip: None,
            msg: (String::new(), 0.0),
            last_m: Vec2::ZERO,
        }
    }

    fn theme(&self) -> &'static Theme {
        if self.dark {
            &DARK
        } else {
            &LIGHT
        }
    }

    pub fn frame(&mut self) {
        let dt = get_frame_time().min(0.1);
        self.clock += dt;
        self.ui_rects.clear();
        self.tip = None;
        match self.screen {
            Screen::Menu => self.menu(),
            Screen::Loading => self.loading(),
            Screen::Play => self.play(dt),
        }
    }

    // ------------------------------------------------------------------------------------- widgets

    fn mouse(&self) -> Vec2 {
        let (x, y) = mouse_position();
        vec2(x, y)
    }
    fn hot(&self, r: Rect) -> bool {
        r.contains(self.mouse())
    }
    fn panel(&mut self, r: Rect) {
        let p = pal(self.dark);
        draw_rectangle(
            r.x + 1.0,
            r.y + 2.0,
            r.w,
            r.h,
            Color::new(0.0, 0.0, 0.0, 0.18),
        );
        draw_rectangle(r.x, r.y, r.w, r.h, p.panel);
        self.ui_rects.push(r);
    }
    /// A button; returns true when clicked.
    fn button(&mut self, r: Rect, label: &str, enabled: bool, active: bool) -> bool {
        let p = pal(self.dark);
        let hot = self.hot(r);
        let bg = if active {
            p.accent
        } else if hot && enabled {
            mix(p.panel2, p.accent, 0.18)
        } else {
            p.panel2
        };
        draw_rectangle(r.x, r.y, r.w, r.h, bg);
        let fc = if active {
            WHITE
        } else if enabled {
            p.text
        } else {
            p.dim
        };
        let size = 16.0;
        let tw = text_w(&self.fonts.head, label, size);
        text(
            &self.fonts.head,
            label,
            r.x + (r.w - tw) / 2.0,
            r.y + r.h / 2.0 + 5.5,
            size,
            fc,
        );
        self.ui_rects.push(r);
        hot && enabled && is_mouse_button_pressed(MouseButton::Left)
    }

    // ------------------------------------------------------------------------------------- menu

    fn menu(&mut self) {
        let th = self.theme();
        clear_background(th.bg);
        let p = pal(self.dark);
        let (sw, sh) = (screen_width(), screen_height());
        // a faint street grid behind the menu
        let gc = if self.dark {
            rgba(255, 255, 255, 0.035)
        } else {
            rgba(255, 255, 255, 0.06)
        };
        let mut x = (self.clock * 6.0) % 60.0 - 60.0;
        while x < sw {
            draw_rectangle(x, 0.0, 10.0, sh, gc);
            x += 60.0;
        }
        let mut y = 0.0;
        while y < sh {
            draw_rectangle(0.0, y, sw, 10.0, gc);
            y += 72.0;
        }
        let w = 760.0f32.min(sw - 32.0);
        let h = 600.0f32.min(sh - 32.0);
        let r = Rect::new((sw - w) / 2.0, (sh - h) / 2.0, w, h);
        self.panel(r);
        let (x0, mut y) = (r.x + 28.0, r.y + 52.0);
        text(&self.fonts.head, "PIXEL HORDE", x0, y, 40.0, p.text);
        y += 26.0;
        text(
            &self.fonts.mono,
            "Hold the airport. Every pixel is an enemy.",
            x0,
            y,
            15.0,
            p.dim,
        );
        y += 34.0;
        text(&self.fonts.head, "Faction", x0, y, 18.0, p.text);
        y += 12.0;
        let cw = (w - 56.0 - 24.0) / 3.0;
        for (i, f) in FACTIONS.iter().enumerate() {
            let cr = Rect::new(x0 + i as f32 * (cw + 12.0), y, cw, 104.0);
            let active = self.fac == i;
            let hot = self.hot(cr);
            draw_rectangle(
                cr.x,
                cr.y,
                cr.w,
                cr.h,
                if active {
                    mix(p.panel2, p.accent, 0.22)
                } else {
                    p.panel2
                },
            );
            if active {
                draw_rectangle_lines(cr.x, cr.y, cr.w, cr.h, 2.0, p.accent);
            }
            let c = Color::from_rgba(f.col[0], f.col[1], f.col[2], 255);
            for k in 0..14 {
                let q = vec2(
                    cr.x + 14.0 + (k * 37 % 23) as f32,
                    cr.y + 14.0 + (k * 13 % 11) as f32,
                );
                draw_rectangle(q.x, q.y, 3.0, 3.0, c);
            }
            text(
                &self.fonts.head,
                f.name,
                cr.x + 46.0,
                cr.y + 26.0,
                18.0,
                p.text,
            );
            wrap(
                &self.fonts.mono,
                f.blurb,
                cr.x + 12.0,
                cr.y + 50.0,
                cr.w - 22.0,
                12.5,
                p.dim,
            );
            if hot && is_mouse_button_pressed(MouseButton::Left) {
                self.fac = i;
            }
            self.ui_rects.push(cr);
        }
        y += 128.0;
        text(&self.fonts.head, "Map", x0, y, 18.0, p.text);
        y += 12.0;
        if self.button(
            Rect::new(x0, y, 170.0, 36.0),
            "Random city",
            true,
            !self.real,
        ) {
            self.real = false;
        }
        if self.button(
            Rect::new(x0 + 180.0, y, 170.0, 36.0),
            "Real city",
            true,
            self.real,
        ) {
            self.real = true;
        }
        y += 48.0;
        if self.real {
            let fr = Rect::new(x0, y, w - 56.0, 38.0);
            draw_rectangle(fr.x, fr.y, fr.w, fr.h, p.panel2);
            draw_rectangle_lines(fr.x, fr.y, fr.w, fr.h, 1.5, p.accent);
            let show = if self.city.is_empty() {
                "Type a city, e.g. Rotterdam or Denver, CO"
            } else {
                &self.city
            };
            let caret = if (self.clock * 2.0) as i32 % 2 == 0 && !self.city.is_empty() {
                "|"
            } else {
                ""
            };
            text(
                &self.fonts.mono,
                &format!("{}{}", show, caret),
                fr.x + 12.0,
                fr.y + 25.0,
                16.0,
                if self.city.is_empty() { p.dim } else { p.text },
            );
            while let Some(ch) = get_char_pressed() {
                if !ch.is_control() && self.city.len() < 60 {
                    self.city.push(ch);
                }
            }
            if is_key_pressed(KeyCode::Backspace) {
                self.city.pop();
            }
            y += 50.0;
            text(&self.fonts.mono, "Streets and buildings come from OpenStreetMap. Your HQ goes at the nearest airport.", x0, y, 13.0, p.dim);
            y += 18.0;
        } else {
            while get_char_pressed().is_some() {}
            text(&self.fonts.mono, "A new random city every game: river, airport, avenues and downtown all move around.", x0, y + 6.0, 13.0, p.dim);
            y += 24.0;
        }
        y += 14.0;
        text(&self.fonts.head, "Difficulty", x0, y, 18.0, p.text);
        y += 12.0;
        for (i, (n, _)) in DIFFS.iter().enumerate() {
            if self.button(
                Rect::new(x0 + i as f32 * 110.0, y, 100.0, 34.0),
                n,
                true,
                self.diff == i,
            ) {
                self.diff = i;
            }
        }
        let by = r.y + r.h - 64.0;
        let can = !self.real || self.city.trim().len() > 1;
        if self.button(
            Rect::new(r.x + r.w - 228.0, by, 200.0, 44.0),
            "Start",
            can,
            false,
        ) || (can && is_key_pressed(KeyCode::Enter))
        {
            self.start();
        }
        if self.button(
            Rect::new(x0, by + 6.0, 130.0, 32.0),
            if self.dark { "Light map" } else { "Dark map" },
            true,
            false,
        ) {
            self.dark = !self.dark;
        }
        // updater line
        let st = self.updater.get();
        text(
            &self.fonts.mono,
            &self.updater.label(),
            r.x + 12.0,
            r.y + r.h + 22.0,
            13.0,
            p.dim,
        );
        if let UpState::Ready(_) = st {
            if self.button(
                Rect::new(r.x + r.w - 160.0, r.y + r.h + 6.0, 160.0, 28.0),
                "Restart now",
                true,
                true,
            ) {
                Updater::restart();
            }
        }
        if !self.status.is_empty() {
            text(
                &self.fonts.mono,
                &self.status,
                x0 + 150.0,
                by + 28.0,
                13.0,
                rgb(250, 82, 82),
            );
        }
    }

    /// Headless test hook: start a random city right away.
    pub fn quick_start(&mut self, fac: usize, seed: u64) {
        self.fac = fac;
        let d = crate::citygen::generate(seed);
        self.begin(d, seed);
    }
    pub fn game_mut(&mut self) -> Option<&mut Game> {
        self.game.as_mut()
    }
    pub fn set_view(&mut self, cam: Vec2, scale: f32) {
        self.cam = cam;
        self.scale = scale;
    }

    fn start(&mut self) {
        self.status.clear();
        if self.real {
            self.loader = Some(osm::start(self.city.trim().to_string()));
            self.status = "Starting download".into();
            self.screen = Screen::Loading;
        } else {
            let seed = (macroquad::miniquad::date::now() * 1000.0) as u64 ^ 0x5DEECE66D;
            let d = crate::citygen::generate(seed);
            self.begin(d, seed);
        }
    }

    fn begin(&mut self, d: Desc, seed: u64) {
        let note = d.note.clone();
        let name = d.name.clone();
        let g = Game::new(d, self.fac, DIFFS[self.diff].1, seed);
        self.cam = g.world.hq;
        self.scale = (screen_height() / 260.0).max(1.5);
        self.scene = Some(Scene::new(&g, self.theme()));
        self.game = Some(g);
        self.tool = Tool::None;
        self.selected = None;
        self.paused = false;
        self.acc = 0.0;
        self.screen = Screen::Play;
        self.msg = (format!("{}. {}", name, note), 6.0);
    }

    fn loading(&mut self) {
        let th = self.theme();
        clear_background(th.bg);
        let p = pal(self.dark);
        if let Some(l) = &self.loader {
            while let Ok(m) = l.rx.try_recv() {
                match m {
                    OsmMsg::Status(s) => self.status = s,
                    OsmMsg::Fail(e) => {
                        self.status = e;
                        self.loader = None;
                        self.screen = Screen::Menu;
                        return;
                    }
                    OsmMsg::Done(d) => {
                        self.loader = None;
                        self.status.clear();
                        self.begin(*d, 7);
                        return;
                    }
                }
            }
        }
        let (sw, sh) = (screen_width(), screen_height());
        let r = Rect::new(sw / 2.0 - 260.0, sh / 2.0 - 80.0, 520.0, 160.0);
        self.panel(r);
        text(
            &self.fonts.head,
            &format!("Loading {}", self.city.trim()),
            r.x + 24.0,
            r.y + 44.0,
            22.0,
            p.text,
        );
        text(
            &self.fonts.mono,
            &self.status,
            r.x + 24.0,
            r.y + 78.0,
            14.0,
            p.dim,
        );
        for k in 0..12 {
            let a = self.clock * 4.0 + k as f32 * 0.52;
            let c = vec2(r.x + r.w - 50.0, r.y + 42.0) + vec2(a.cos(), a.sin()) * 14.0;
            draw_circle(c.x, c.y, 2.0, alpha(p.accent, k as f32 / 12.0));
        }
        if self.button(
            Rect::new(r.x + 24.0, r.y + 104.0, 120.0, 34.0),
            "Cancel",
            true,
            false,
        ) {
            self.loader = None;
            self.status.clear();
            self.screen = Screen::Menu;
        }
    }

    // ------------------------------------------------------------------------------------- game

    fn to_world(&self, s: Vec2) -> Vec2 {
        self.cam + (s - vec2(screen_width(), screen_height()) / 2.0) / self.scale
    }

    fn play(&mut self, dt: f32) {
        let Some(mut g) = self.game.take() else {
            self.screen = Screen::Menu;
            return;
        };
        let mut sc = self.scene.take().unwrap();
        let th = self.theme();
        if sc.dark != self.dark {
            sc.retheme(&g, th);
        }
        // fixed 20 Hz simulation
        if !self.paused && !g.over {
            self.acc += dt;
            let mut n = 0;
            while self.acc >= DT && n < 3 {
                g.tick();
                self.acc -= DT;
                n += 1;
            }
            if n == 3 {
                self.acc = 0.0;
            }
            if n > 0 {
                sc.update_units(&g);
            }
        }
        if !sc.units_fresh {
            sc.update_units(&g);
        }
        sc.maintain(&mut g, th, dt);
        if let Some(s) = self.selected {
            if s >= g.groups.len() || g.groups[s].dead {
                self.selected = None;
            }
        }

        // camera
        let (sw, sh) = (screen_width(), screen_height());
        clear_background(th.bg);
        set_camera(&Camera2D {
            target: self.cam,
            zoom: vec2(2.0 * self.scale / sw, 2.0 * self.scale / sh),
            ..Default::default()
        });
        let tl = self.to_world(vec2(0.0, 0.0));
        let br = self.to_world(vec2(sw, sh));
        let mut labels = WorldLabels { items: Vec::new() };
        render::draw_world(
            &mut sc,
            &g,
            th,
            &ViewInfo {
                tl,
                br,
                scale: self.scale,
                selected: self.selected,
                time: self.clock,
            },
            &mut labels,
        );
        let mw = self.to_world(self.mouse());
        let cursor_label = self.preview(&g, mw);
        set_default_camera();
        for (p, s, c, size) in &labels.items {
            let q = self.cam_to_screen(*p);
            let tw = text_w(&self.fonts.head, s, *size);
            text(
                &self.fonts.head,
                s,
                q.x - tw / 2.0 + 1.0,
                q.y + 1.0,
                *size,
                Color::new(0.0, 0.0, 0.0, 0.6),
            );
            text(&self.fonts.head, s, q.x - tw / 2.0, q.y, *size, *c);
        }

        self.hud(&mut g);
        if let Some((s, c)) = cursor_label {
            let m = self.mouse();
            let tw = text_w(&self.fonts.mono, &s, 14.0);
            draw_rectangle(
                m.x + 16.0,
                m.y + 14.0,
                tw + 14.0,
                24.0,
                Color::new(0.0, 0.0, 0.0, 0.75),
            );
            text(&self.fonts.mono, &s, m.x + 23.0, m.y + 31.0, 14.0, c);
        }
        if let Some(t) = self.tip.clone() {
            self.tooltip(&t);
        }
        self.input(&mut g, &mut sc, mw, dt);
        if self.screen == Screen::Play {
            self.game = Some(g);
            self.scene = Some(sc);
        }
    }

    fn cam_to_screen(&self, p: Vec2) -> Vec2 {
        (p - self.cam) * self.scale + vec2(screen_width(), screen_height()) / 2.0
    }

    fn over_ui(&self) -> bool {
        let m = self.mouse();
        self.ui_rects.iter().any(|r| r.contains(m))
    }

    /// Placements a build drag would make, as (x, y) of each footprint.
    fn plan(&self, g: &Game, t: S, a: Vec2, b: Vec2) -> Vec<(i32, i32)> {
        let d = def(t);
        if t == S::Wall {
            let snap = |p: Vec2| ((p.x / 3.0).floor() as i32, (p.y / 3.0).floor() as i32);
            let (x0, y0) = snap(a);
            let (x1, y1) = snap(b);
            let n = (x1 - x0).abs().max((y1 - y0).abs()).min(400);
            let mut v = Vec::new();
            for k in 0..=n {
                let f = if n == 0 { 0.0 } else { k as f32 / n as f32 };
                let gx = (x0 as f32 + (x1 - x0) as f32 * f).round() as i32;
                let gy = (y0 as f32 + (y1 - y0) as f32 * f).round() as i32;
                if v.last() != Some(&(gx * 3, gy * 3)) {
                    v.push((gx * 3, gy * 3));
                }
            }
            return v;
        }
        let ox = |p: Vec2| {
            (
                (p.x - d.w as f32 / 2.0).round() as i32,
                (p.y - d.h as f32 / 2.0).round() as i32,
            )
        };
        let (x0, y0) = ox(a);
        let (x1, y1) = ox(b);
        let (sx, sy) = (d.w + 1, d.h + 1);
        let nx = ((x1 - x0).abs() / sx).min(30);
        let ny = ((y1 - y0).abs() / sy).min(30);
        let (dx, dy) = ((x1 - x0).signum(), (y1 - y0).signum());
        let mut v = Vec::new();
        for j in 0..=ny {
            for i in 0..=nx {
                v.push((x0 + i * sx * dx, y0 + j * sy * dy));
            }
        }
        let _ = g;
        v
    }

    /// Draws the ghost of whatever the current tool would do. Returns the label for the cursor.
    fn preview(&self, g: &Game, mw: Vec2) -> Option<(String, Color)> {
        if self.over_ui() {
            return None;
        }
        let ok = rgba(64, 192, 87, 0.55);
        let bad = rgba(250, 82, 82, 0.55);
        match self.tool {
            Tool::Build(t) => {
                let d = def(t);
                // rooftop spot for turrets
                if d.turret && self.press.is_none() {
                    if let Some((b, p)) = g.roof_at(mw) {
                        let q = p + g.roof_off(b);
                        let can = g.can_place_roof(b, p);
                        let cost = g.cost_at(t, p);
                        let afford = g.scrap >= cost;
                        draw_rectangle(
                            q.x - d.w as f32 / 2.0,
                            q.y - d.h as f32 / 2.0,
                            d.w as f32,
                            d.h as f32,
                            if can && afford { ok } else { bad },
                        );
                        draw_circle_lines(q.x, q.y, d.range * 1.6, 0.4, rgba(255, 212, 59, 0.6));
                        let extra = cost - d.cost;
                        let s = if !can {
                            "Rooftop full".to_string()
                        } else {
                            format!(
                                "Rooftop  ${:.0}{}  +60% range",
                                cost,
                                if extra > 0.0 {
                                    format!(" (+{:.0} distance)", extra)
                                } else {
                                    String::new()
                                }
                            )
                        };
                        return Some((
                            s,
                            if can && afford {
                                WHITE
                            } else {
                                rgb(255, 140, 140)
                            },
                        ));
                    }
                }
                let (a, b) = match self.press {
                    Some((_, w)) => (w, mw),
                    None => (mw, mw),
                };
                let spots = self.plan(g, t, a, b);
                let mut total = 0.0;
                let mut extra = 0.0;
                let mut n_ok = 0;
                let mut reason = "";
                for (x, y) in &spots {
                    let c = vec2(*x as f32 + d.w as f32 / 2.0, *y as f32 + d.h as f32 / 2.0);
                    let can = g.can_place(t, *x, *y);
                    let cost = g.cost_at(t, c);
                    if can {
                        total += cost;
                        extra += cost - d.cost;
                        n_ok += 1;
                    } else if reason.is_empty() {
                        reason = if g.hive_blocks(c) {
                            "A hive is too close"
                        } else if g.world.irradiated_at(c) {
                            "Radiation"
                        } else if d.zone && !g.world.in_zone(c) {
                            "Base zone only"
                        } else {
                            "Blocked"
                        };
                    }
                    draw_rectangle(
                        *x as f32,
                        *y as f32,
                        d.w as f32,
                        d.h as f32,
                        if can { ok } else { bad },
                    );
                }
                if d.turret && spots.len() == 1 {
                    let c = vec2(
                        spots[0].0 as f32 + d.w as f32 / 2.0,
                        spots[0].1 as f32 + d.h as f32 / 2.0,
                    );
                    draw_circle_lines(c.x, c.y, d.range, 0.4, rgba(255, 212, 59, 0.6));
                }
                let afford = g.scrap >= total;
                let mut s = if n_ok == 0 {
                    reason.to_string()
                } else if n_ok > 1 {
                    format!("{} × {}  ${:.0}", n_ok, d.name, total)
                } else {
                    format!("${:.0}", total)
                };
                if extra > 0.5 && n_ok > 0 {
                    s.push_str(&format!("  (+{:.0} for distance from HQ)", extra));
                }
                if n_ok > 0
                    && d.turret
                    && spots
                        .iter()
                        .any(|(x, y)| !g.world.in_zone(vec2(*x as f32, *y as f32)))
                {
                    s.push_str("  · flown in by helicopter");
                }
                Some((
                    s,
                    if n_ok > 0 && afford {
                        WHITE
                    } else {
                        rgb(255, 140, 140)
                    },
                ))
            }
            Tool::Unit(u) => {
                let d = udef(u);
                draw_circle_lines(mw.x, mw.y, d.range, 0.4, rgba(116, 192, 252, 0.6));
                let roof = if u == U::Rifle { g.roof_at(mw) } else { None };
                let s = if roof.is_some() {
                    format!("${:.0}  garrison building (+60% range)", d.cost)
                } else {
                    format!("${:.0}", d.cost)
                };
                Some((
                    s,
                    if g.scrap >= d.cost {
                        WHITE
                    } else {
                        rgb(255, 140, 140)
                    },
                ))
            }
            Tool::Ability(a) => {
                let blue = rgba(66, 133, 244, 0.8);
                match a {
                    A::Icbm => {
                        let r = if g.research.nuke { 190.0 } else { 120.0 };
                        draw_circle(mw.x, mw.y, r, rgba(234, 67, 53, 0.08));
                        draw_circle_lines(mw.x, mw.y, r, 1.0, rgba(234, 67, 53, 0.8));
                    }
                    A::Rescue => {
                        let r = if g.research.rescue { 90.0 } else { 60.0 };
                        draw_circle_lines(mw.x, mw.y, r, 0.6, rgba(64, 192, 87, 0.8));
                    }
                    A::Gunship => draw_circle_lines(mw.x, mw.y, 25.0 + 34.0, 0.6, blue),
                    A::Air => {
                        let (s, e) = match self.press {
                            Some((_, w)) if g.research.air => (w, mw),
                            _ => (mw - vec2(24.0, 0.0), mw + vec2(24.0, 0.0)),
                        };
                        draw_line(s.x, s.y, e.x, e.y, 1.0, rgba(255, 212, 59, 0.8));
                        draw_circle_lines(mw.x, mw.y, 30.0, 0.5, rgba(255, 212, 59, 0.5));
                    }
                }
                let hint = match a {
                    A::Air if g.research.air => "Drag the bomb run from start to end",
                    A::Air => "Click where the bombs should fall",
                    _ => "Click the target",
                };
                Some((hint.to_string(), WHITE))
            }
            Tool::None => None,
        }
    }

    fn tooltip(&self, t: &str) {
        let p = pal(self.dark);
        let m = self.mouse();
        let w = 320.0;
        let lines = wrap_lines(&self.fonts.mono, t, w - 20.0, 13.0);
        let h = 14.0 + lines.len() as f32 * 17.0;
        let x = (m.x - w / 2.0).clamp(8.0, screen_width() - w - 8.0);
        let y = (m.y - h - 16.0).max(8.0);
        draw_rectangle(x, y, w, h, p.panel2);
        draw_rectangle_lines(x, y, w, h, 1.0, p.accent);
        for (i, l) in lines.iter().enumerate() {
            text(
                &self.fonts.mono,
                l,
                x + 10.0,
                y + 20.0 + i as f32 * 17.0,
                13.0,
                p.text,
            );
        }
    }

    fn hud(&mut self, g: &mut Game) {
        let p = pal(self.dark);
        let (sw, sh) = (screen_width(), screen_height());
        // top bar
        self.panel(Rect::new(0.0, 0.0, sw, 40.0));
        let f = g.faction();
        let fc = Color::from_rgba(f.col[0], f.col[1], f.col[2], 255);
        draw_rectangle(12.0, 14.0, 12.0, 12.0, fc);
        text(&self.fonts.head, f.name, 32.0, 26.0, 17.0, p.text);
        let stats = [
            ("Scrap", format!("{:.0}", g.scrap)),
            ("Ammo", format!("{:.0}", g.ammo)),
            ("Score", format!("{:.0}", g.score)),
            ("Hostiles", fmt_n(g.hostiles())),
            ("Civilians", fmt_n(g.alive_civs())),
            ("Rescued", format!("{}", g.rescued)),
            ("Waiting", format!("{}", g.waiting)),
            ("Flown out", format!("{}", g.extracted)),
        ];
        let mut x = 150.0;
        for (k, v) in stats.iter() {
            text(&self.fonts.mono, k, x, 17.0, 11.0, p.dim);
            text(&self.fonts.head, v, x, 34.0, 16.0, p.text);
            x += (text_w(&self.fonts.head, v, 16.0).max(text_w(&self.fonts.mono, k, 11.0)) + 22.0)
                .max(64.0);
        }
        if g.income_mult < 0.999 {
            text(&self.fonts.mono, "Income", x, 17.0, 11.0, p.dim);
            text(
                &self.fonts.head,
                &format!("×{:.2}", g.income_mult),
                x,
                34.0,
                16.0,
                rgb(250, 82, 82),
            );
            if self.hot(Rect::new(x, 0.0, 60.0, 40.0)) {
                self.tip = Some("Too many rescued civilians are waiting in the base. Build more extraction pads to fly them out faster.".into());
            }
        }
        let bx = sw - 330.0;
        if self.button(
            Rect::new(bx, 6.0, 74.0, 28.0),
            if self.dark { "Light" } else { "Dark" },
            true,
            false,
        ) {
            self.dark = !self.dark;
        }
        if self.button(
            Rect::new(bx + 80.0, 6.0, 74.0, 28.0),
            if g.tilt > 0.0 { "Top view" } else { "2.5D" },
            true,
            false,
        ) {
            g.tilt = if g.tilt > 0.0 { 0.0 } else { 0.85 };
        }
        if self.button(
            Rect::new(bx + 160.0, 6.0, 74.0, 28.0),
            if self.paused { "Resume" } else { "Pause" },
            true,
            self.paused,
        ) {
            self.paused = !self.paused;
        }
        if self.button(Rect::new(bx + 240.0, 6.0, 80.0, 28.0), "Menu", true, false) {
            self.screen = Screen::Menu;
            return;
        }
        text(
            &self.fonts.mono,
            &format!("{:.0} fps · sim {:.1} ms", get_fps(), g.sim_ms),
            sw - 190.0,
            56.0,
            12.0,
            p.dim,
        );

        // build bar
        let items: Vec<Tool> = BUILD_ORDER
            .iter()
            .map(|s| Tool::Build(*s))
            .chain([Tool::Unit(U::Rifle), Tool::Unit(U::Tank)])
            .collect();
        let bw = 92.0;
        let total = items.len() as f32 * (bw + 6.0) + 6.0;
        let x0 = ((sw - total) / 2.0).max(4.0);
        let y0 = sh - 70.0;
        self.panel(Rect::new(x0, y0, total, 70.0));
        for (i, it) in items.iter().enumerate() {
            let r = Rect::new(x0 + 6.0 + i as f32 * (bw + 6.0), y0 + 6.0, bw, 58.0);
            let (key, name, cost, col, tip, ok) = match it {
                Tool::Build(s) => {
                    let d = def(*s);
                    let c = if *s == S::Extractor {
                        g.cost_at(*s, g.world.hq)
                    } else {
                        d.cost
                    };
                    (d.key, d.name, c, d.top, d.tip, g.scrap >= c)
                }
                Tool::Unit(u) => {
                    let d = udef(*u);
                    (d.key, d.name, d.cost, d.col, d.tip, g.scrap >= d.cost)
                }
                _ => unreachable!(),
            };
            let active = self.tool == *it;
            let hot = self.hot(r);
            draw_rectangle(
                r.x,
                r.y,
                r.w,
                r.h,
                if active {
                    mix(p.panel2, p.accent, 0.3)
                } else if hot {
                    mix(p.panel2, p.accent, 0.12)
                } else {
                    p.panel2
                },
            );
            draw_rectangle(r.x, r.y, 4.0, r.h, col);
            text(
                &self.fonts.mono,
                &key.to_string(),
                r.x + r.w - 14.0,
                r.y + 13.0,
                11.0,
                p.dim,
            );
            let nm = match it {
                Tool::Build(S::Gun) => "Gun turret",
                Tool::Build(S::Extractor) => "Extractor",
                Tool::Build(S::Plant) => "Ammo plant",
                Tool::Build(S::Silo) => "Missile silo",
                Tool::Build(S::Pad) => "Extraction",
                Tool::Build(S::Lab) => "Research",
                Tool::Unit(U::Rifle) => "Rifles",
                _ => name,
            };
            text(
                &self.fonts.head,
                nm,
                r.x + 10.0,
                r.y + 30.0,
                14.0,
                if ok { p.text } else { p.dim },
            );
            text(
                &self.fonts.mono,
                &format!("${:.0}", cost),
                r.x + 10.0,
                r.y + 49.0,
                13.0,
                if ok { p.accent } else { rgb(250, 82, 82) },
            );
            self.ui_rects.push(r);
            if hot {
                self.tip = Some(format!("{} [{}]: {}", name, key, tip));
                if is_mouse_button_pressed(MouseButton::Left) {
                    self.tool = if active { Tool::None } else { *it };
                }
            }
        }

        // ability panel
        let aw = 236.0;
        let ax = sw - aw - 10.0;
        let mut ay = 70.0;
        for a in ABILITIES {
            let d = adef(a);
            let r = Rect::new(ax, ay, aw, 84.0);
            self.panel(r);
            let usable = g.can_use(a);
            let active = self.tool == Tool::Ability(a);
            let hot = self.hot(Rect::new(r.x, r.y, r.w, 50.0));
            if active {
                draw_rectangle_lines(r.x, r.y, r.w, r.h, 2.0, p.accent);
            }
            let fc = if usable { p.text } else { p.dim };
            text(
                &self.fonts.head,
                &format!(
                    "{}{}",
                    d.name,
                    if g.upgraded(a) && a != A::Gunship {
                        " +"
                    } else {
                        ""
                    }
                ),
                r.x + 12.0,
                r.y + 22.0,
                16.0,
                fc,
            );
            text(
                &self.fonts.mono,
                &d.key.to_string(),
                r.x + r.w - 18.0,
                r.y + 20.0,
                12.0,
                p.dim,
            );
            let mut line = format!("${:.0}", d.cost);
            if d.ammo > 0.0 {
                line.push_str(&format!(" + {:.0} ammo", d.ammo));
            }
            let cd = g.cooldown(a);
            if a == A::Icbm {
                let (rdy, tot) = g.silos_ready();
                line = if tot == 0 {
                    "Needs a missile silo".into()
                } else {
                    format!("Silos ready {}/{}", rdy, tot)
                };
            }
            text(&self.fonts.mono, &line, r.x + 12.0, r.y + 42.0, 12.0, p.dim);
            if cd > 0.0 {
                let full = d.cd;
                draw_rectangle(
                    r.x,
                    r.y + 48.0,
                    r.w * (1.0 - cd / full).clamp(0.0, 1.0),
                    3.0,
                    p.accent,
                );
                text(
                    &self.fonts.mono,
                    &format!("{:.0}s", cd),
                    r.x + r.w - 44.0,
                    r.y + 42.0,
                    12.0,
                    p.accent,
                );
            }
            if hot {
                self.tip = Some(d.tip.to_string());
                if is_mouse_button_pressed(MouseButton::Left) && usable {
                    self.tool = if active { Tool::None } else { Tool::Ability(a) };
                }
            }
            // research
            let ur = Rect::new(r.x + 8.0, r.y + 54.0, r.w - 16.0, 24.0);
            if a != A::Gunship {
                let (label, en) = if g.upgraded(a) {
                    (format!("Upgraded: {}", d.up_name), false)
                } else if let Some((ra, t)) = g.research.busy {
                    if ra == a {
                        (format!("Researching… {:.0}s", t), false)
                    } else {
                        (format!("Upgrade ${:.0}", d.up_cost), false)
                    }
                } else if !g.has(S::Lab) {
                    ("Upgrade: needs research lab".to_string(), false)
                } else {
                    (
                        format!("Upgrade: {} ${:.0}", d.up_name, d.up_cost),
                        g.can_research(a),
                    )
                };
                let p2 = pal(self.dark);
                let hotu = self.hot(ur);
                draw_rectangle(
                    ur.x,
                    ur.y,
                    ur.w,
                    ur.h,
                    if hotu && en {
                        mix(p2.panel2, p2.accent, 0.2)
                    } else {
                        p2.panel2
                    },
                );
                text(
                    &self.fonts.mono,
                    &label,
                    ur.x + 8.0,
                    ur.y + 16.0,
                    11.5,
                    if en { p2.accent } else { p2.dim },
                );
                if hotu {
                    self.tip = Some(format!("{}: {}", d.up_name, d.upgrade));
                    if en && is_mouse_button_pressed(MouseButton::Left) {
                        g.start_research(a);
                    }
                }
            } else {
                text(
                    &self.fonts.mono,
                    "Hovers and fires for 20 s",
                    ur.x + 8.0,
                    ur.y + 16.0,
                    11.5,
                    p.dim,
                );
            }
            ay += 92.0;
        }

        // selection info
        if let Some(gi) = self.selected {
            let gr = &g.groups[gi];
            let n = gr.m.iter().filter(|m| m.alive).count();
            let s = format!(
                "{} · {} alive{} · right-click to move{}",
                udef(gr.kind).name,
                n,
                if gr.state == GState::Garrison {
                    " · garrisoned"
                } else {
                    ""
                },
                if gr.kind == U::Rifle {
                    " (click a building to garrison)"
                } else {
                    ""
                }
            );
            let r = Rect::new(
                10.0,
                sh - 110.0,
                text_w(&self.fonts.mono, &s, 13.0) + 24.0,
                30.0,
            );
            self.panel(r);
            text(&self.fonts.mono, &s, r.x + 12.0, r.y + 20.0, 13.0, p.text);
        }

        // banners
        let (msg, t) = if g.banner.1 > 0.0 {
            (g.banner.0.clone(), g.banner.1)
        } else {
            (self.msg.0.clone(), self.msg.1)
        };
        self.msg.1 -= get_frame_time();
        if t > 0.0 && !msg.is_empty() {
            let tw = text_w(&self.fonts.head, &msg, 18.0);
            let r = Rect::new((sw - tw) / 2.0 - 16.0, 54.0, tw + 32.0, 34.0);
            draw_rectangle(
                r.x,
                r.y,
                r.w,
                r.h,
                Color::new(0.0, 0.0, 0.0, 0.7 * t.min(1.0)),
            );
            text(
                &self.fonts.head,
                &msg,
                r.x + 16.0,
                r.y + 23.0,
                18.0,
                Color::new(1.0, 1.0, 1.0, t.min(1.0)),
            );
        }
        if self.paused {
            let s = "PAUSED";
            let tw = text_w(&self.fonts.head, s, 30.0);
            text(&self.fonts.head, s, (sw - tw) / 2.0, sh / 2.0, 30.0, WHITE);
        }
        if g.over {
            let r = Rect::new(sw / 2.0 - 220.0, sh / 2.0 - 90.0, 440.0, 180.0);
            self.panel(r);
            text(
                &self.fonts.head,
                "The HQ has fallen",
                r.x + 24.0,
                r.y + 46.0,
                26.0,
                p.text,
            );
            text(
                &self.fonts.mono,
                &format!(
                    "Score {:.0} · {} kills · {} flown out · {:.0} min",
                    g.score,
                    fmt_n(g.kills as usize),
                    g.extracted,
                    g.time / 60.0
                ),
                r.x + 24.0,
                r.y + 80.0,
                13.0,
                p.dim,
            );
            if self.button(
                Rect::new(r.x + 24.0, r.y + 112.0, 180.0, 40.0),
                "Back to menu",
                true,
                true,
            ) {
                self.screen = Screen::Menu;
            }
        }
    }

    fn input(&mut self, g: &mut Game, sc: &mut Scene, mw: Vec2, dt: f32) {
        let ui = self.over_ui();
        // keyboard
        while let Some(ch) = get_char_pressed() {
            let ch = ch.to_ascii_lowercase();
            if let Some(s) = BUILD_ORDER.iter().find(|s| def(**s).key == ch) {
                self.tool = if self.tool == Tool::Build(*s) {
                    Tool::None
                } else {
                    Tool::Build(*s)
                };
            } else if let Some(u) = [U::Rifle, U::Tank].iter().find(|u| udef(**u).key == ch) {
                self.tool = Tool::Unit(*u);
            } else if let Some(a) = ABILITIES.iter().find(|a| adef(**a).key == ch) {
                if g.can_use(*a) {
                    self.tool = Tool::Ability(*a);
                }
            } else {
                match ch {
                    't' => g.tilt = if g.tilt > 0.0 { 0.0 } else { 0.85 },
                    'k' => self.dark = !self.dark,
                    'p' | ' ' => self.paused = !self.paused,
                    'h' => self.cam = g.world.hq,
                    'f' => {
                        self.cam = vec2(W as f32, H as f32) / 2.0;
                        self.scale = (screen_height() / H as f32).min(screen_width() / W as f32);
                    }
                    'x' => {
                        if g.sell_at(mw) {
                            self.msg = ("Sold for half price".into(), 1.2);
                        }
                    }
                    _ => {}
                }
            }
        }
        if is_key_pressed(KeyCode::Escape) {
            self.tool = Tool::None;
            self.selected = None;
            self.press = None;
        }
        if is_key_pressed(KeyCode::F9) {
            g.stress();
        }
        let pan = 500.0 / self.scale * dt;
        if is_key_down(KeyCode::W) || is_key_down(KeyCode::Up) {
            self.cam.y -= pan;
        }
        if is_key_down(KeyCode::S) || is_key_down(KeyCode::Down) {
            self.cam.y += pan;
        }
        if is_key_down(KeyCode::A) || is_key_down(KeyCode::Left) {
            self.cam.x -= pan;
        }
        if is_key_down(KeyCode::D) || is_key_down(KeyCode::Right) {
            self.cam.x += pan;
        }
        let (_, wy) = mouse_wheel();
        if wy != 0.0 && !ui {
            let before = mw;
            let k = if wy > 0.0 { 1.15 } else { 1.0 / 1.15 };
            self.scale = (self.scale * k).clamp(0.35, 14.0);
            let after = self.to_world(self.mouse());
            self.cam += before - after;
        }
        let md = self.mouse() - self.last_m;
        self.last_m = self.mouse();
        if is_mouse_button_down(MouseButton::Middle) {
            self.cam -= md / self.scale;
        }
        self.cam = self.cam.clamp(vec2(0.0, 0.0), vec2(W as f32, H as f32));
        let _ = sc;

        // right click: order the selected group, or drop the tool
        if is_mouse_button_pressed(MouseButton::Right) && !ui {
            if self.tool != Tool::None {
                self.tool = Tool::None;
                self.press = None;
            } else if let Some(gi) = self.selected {
                if !g.order(gi, mw) {
                    self.msg = ("Can't send troops there".into(), 1.5);
                }
            }
        }
        if is_mouse_button_pressed(MouseButton::Left) && !ui {
            self.press = Some((self.mouse(), mw));
            self.panning = false;
        }
        if is_mouse_button_down(MouseButton::Left) {
            if let Some((s0, _)) = self.press {
                if self.tool == Tool::None && (self.mouse() - s0).length() > 6.0 {
                    self.panning = true;
                }
                if self.panning {
                    self.cam -= md / self.scale;
                }
            }
        }
        if is_mouse_button_released(MouseButton::Left) {
            let Some((_, w0)) = self.press.take() else {
                return;
            };
            if self.panning {
                self.panning = false;
                return;
            }
            match self.tool {
                Tool::None => {
                    self.selected = g.group_at(mw);
                }
                Tool::Build(t) => {
                    let d = def(t);
                    if d.turret && w0.distance(mw) < 2.0 {
                        if let Some((b, p)) = g.roof_at(mw) {
                            if !g.place_roof(t, b, p) {
                                self.msg = ("Can't put a turret on that roof".into(), 1.5);
                            }
                            return;
                        }
                    }
                    let spots = self.plan(g, t, w0, mw);
                    let mut placed = 0;
                    for (x, y) in spots {
                        if g.place(t, x, y) {
                            placed += 1;
                        }
                    }
                    if placed == 0 {
                        self.msg = ("Can't build there".into(), 1.5);
                    }
                }
                Tool::Unit(u) => {
                    if !g.deploy(u, mw) {
                        self.msg = ("Can't deploy there".into(), 1.5);
                    }
                }
                Tool::Ability(a) => {
                    let from = if a == A::Air && g.research.air {
                        w0
                    } else {
                        mw
                    };
                    if g.use_ability(a, from, mw) {
                        self.tool = Tool::None;
                    } else {
                        self.msg = ("Can't target that".into(), 1.5);
                    }
                }
            }
        }
    }
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    Color::new(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}
fn alpha(c: Color, a: f32) -> Color {
    Color::new(c.r, c.g, c.b, a)
}
fn fmt_n(n: usize) -> String {
    if n >= 1_000_000 {
        format!("{:.2}M", n as f32 / 1e6)
    } else if n >= 10_000 {
        format!("{:.1}k", n as f32 / 1e3)
    } else {
        format!("{}", n)
    }
}
fn wrap_lines(f: &Font, s: &str, w: f32, size: f32) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = String::new();
    for word in s.split_whitespace() {
        let t = if line.is_empty() {
            word.to_string()
        } else {
            format!("{} {}", line, word)
        };
        if text_w(f, &t, size) > w && !line.is_empty() {
            out.push(std::mem::take(&mut line));
            line = word.to_string();
        } else {
            line = t;
        }
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}
fn wrap(f: &Font, s: &str, x: f32, y: f32, w: f32, size: f32, c: Color) {
    for (i, l) in wrap_lines(f, s, w, size).iter().enumerate() {
        text(f, l, x, y + i as f32 * (size + 4.0), size, c);
    }
}

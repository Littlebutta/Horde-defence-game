//! Screens, HUD and input: the start menu, the city loader, and the game itself.

use crate::campaign::{self, Campaign, CITIES};
use crate::defs::*;
use crate::game::*;
use crate::net::{self, Cmd};
use crate::osm::{self, OsmMsg};
use crate::music;
use crate::storage;
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
    /// the campaign between cities: unlocks, recruits and the next city
    Hub,
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
    music: music::Music,
    /// music volume 0..=100, saved in settings
    music_vol: u32,
    /// effects volume 0..=100, saved in settings
    sfx_vol: u32,
    /// the volume to go back to when M unmutes
    music_prev: u32,
    fac: usize,
    diff: usize,
    real: bool,
    city: String,
    /// "World campaign" picked on the menu
    camp_mode: bool,
    camp: Option<Campaign>,
    /// the game being played (or loaded) is a campaign city
    in_camp: bool,
    pack: campaign::Pack,
    /// loading screen is waiting for the city pack instead of OpenStreetMap
    wait_pack: bool,
    status: String,
    loader: Option<osm::Loader>,
    game: Option<Game>,
    scene: Option<Scene>,
    cam: Orbit,
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
    saved: Vec<storage::SavedMap>,
    steam: Option<net::Steam>,
    sess: Option<net::Session>,
    test_step: u32,
    test_log: u64,
    /// The 3D view is drawn here, with a depth buffer we create ourselves: some Windows drivers
    /// hand the window one that doesn't depth-test, so walls showed through each other.
    rt: Option<RenderTarget>,
    /// how this GPU gets working depth, found by `pick_depth` on the first game frame
    depth: Option<render::DepthMode>,
    /// the match hasn't started: the player is choosing where the HQ goes (the map and seed to
    /// rebuild from)
    placing: Option<(Desc, u64)>,
    /// set during a frame: move the HQ here / start the match
    place_at: Option<Vec2>,
    place_go: bool,
    /// headless tests start right away with the suggested HQ (`PH_PLACE=1` shows the choice)
    auto_place: bool,
}

/// Colours of the co-op players' cursors, host first.
const PLAYER_COLS: [Color; 4] = [
    Color::new(0.26, 0.52, 0.96, 1.0),
    Color::new(0.98, 0.74, 0.02, 1.0),
    Color::new(0.20, 0.66, 0.33, 1.0),
    Color::new(0.85, 0.30, 0.85, 1.0),
];

/// The orbit camera: looks at a point on the ground from `dist` cells away, turned by `yaw` and
/// tilted `pitch` above the horizon (pi/2 would be straight down).
#[derive(Clone, Copy)]
struct Orbit {
    t: Vec2,
    dist: f32,
    yaw: f32,
    pitch: f32,
}
const FOVY: f32 = 0.8;
const TOP_PITCH: f32 = 1.5;
const MIN_PITCH: f32 = 0.3;
impl Orbit {
    /// Ground direction from the look-at point back towards the camera.
    fn back(&self) -> Vec2 {
        vec2(-self.yaw.sin(), self.yaw.cos())
    }
    fn right(&self) -> Vec2 {
        vec2(self.yaw.cos(), self.yaw.sin())
    }
    fn camera(&self) -> Camera3D {
        let eye = render::p3(self.t + self.back() * self.dist * self.pitch.cos(), self.dist * self.pitch.sin());
        Camera3D {
            position: eye,
            target: render::p3(self.t, 0.0),
            up: vec3(0.0, 0.0, -1.0),
            fovy: FOVY,
            z_near: (self.dist * 0.04).max(0.5),
            z_far: self.dist * 3.0 + 3000.0,
            ..Default::default()
        }
    }
    /// Screen pixels per map cell at the look-at point.
    fn scale(&self) -> f32 {
        screen_height() / (2.0 * self.dist * (FOVY / 2.0).tan())
    }
    fn set_scale(&mut self, s: f32) {
        self.dist = (screen_height() / (2.0 * s.max(0.05) * (FOVY / 2.0).tan())).clamp(20.0, 2600.0);
    }
    fn top(&self) -> bool {
        self.pitch > TOP_PITCH - 0.01
    }
}

fn to_screen(m: &Mat4, p: Vec3) -> Option<Vec2> {
    let c = *m * p.extend(1.0);
    if c.w <= 0.01 {
        return None;
    }
    let n = c.truncate() / c.w;
    (n.z <= 1.0).then(|| vec2((n.x + 1.0) / 2.0 * screen_width(), (1.0 - n.y) / 2.0 * screen_height()))
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
    pub fn new(steam: Option<net::Steam>) -> App {
        let st = storage::load_settings();
        App {
            screen: Screen::Menu,
            fonts: render::load_fonts(),
            dark: st.dark,
            fac: st.fac,
            diff: st.diff,
            real: st.real,
            city: st.city,
            camp_mode: st.campaign,
            music: music::Music::start(st.music, st.sfx),
            sfx_vol: st.sfx,
            music_vol: st.music,
            music_prev: if st.music > 0 { st.music } else { 70 },
            camp: None,
            in_camp: false,
            pack: campaign::Pack::start(),
            wait_pack: false,
            saved: storage::list_maps(),
            status: String::new(),
            loader: None,
            game: None,
            scene: None,
            cam: Orbit { t: vec2(W as f32 / 2.0, H as f32 / 2.0), dist: 400.0, yaw: 0.0, pitch: 0.95 },
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
            steam,
            sess: test_session(),
            test_step: 0,
            test_log: 0,
            rt: None,
            depth: None,
            placing: None,
            place_at: None,
            place_go: false,
            auto_place: ["PH_SHOT", "PH_NETTEST"].iter().any(|k| std::env::var(k).is_ok()) && std::env::var("PH_PLACE").is_err(),
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
        self.net_frame();
        self.music.ambience(self.screen == Screen::Play);
        match self.screen {
            Screen::Menu => {
                self.menu();
                if let Some(t) = self.tip.clone() {
                    self.tooltip(&t);
                }
            }
            Screen::Loading => self.loading(),
            Screen::Play => self.play(dt),
            Screen::Hub => {
                self.hub();
                if let Some(t) = self.tip.clone() {
                    self.tooltip(&t);
                }
            }
        }
    }

    // ------------------------------------------------------------------------------------- co-op

    fn net_frame(&mut self) {
        if let Some(st) = &self.steam {
            st.tick();
            if let Some(id) = st.take_join() {
                if self.sess.is_none() && self.screen != Screen::Play {
                    st.join(id);
                    self.status = "Joining your friend's game".into();
                } else {
                    self.msg = ("Leave this game first to join a friend".into(), 3.0);
                }
            }
            if let Some(r) = st.take_lobby() {
                match r {
                    Ok((lobby, host)) => {
                        let (link, name) = (st.link(lobby), st.name());
                        self.status.clear();
                        self.sess = Some(if host {
                            let s = net::Session::new_host(link, name);
                            s.invite();
                            s
                        } else {
                            net::Session::new_client(link, name)
                        });
                    }
                    Err(e) => self.status = e,
                }
            }
        }
        let notes = match self.sess.as_mut() {
            Some(s) => s.pump(),
            None => return,
        };
        for n in notes {
            match n {
                net::Note::Started { desc, fac, diff, seed } => {
                    self.fac = fac;
                    self.diff = diff;
                    self.begin(*desc, seed);
                }
                net::Note::Ended => {
                    if self.screen == Screen::Play {
                        self.screen = Screen::Menu;
                        self.status = "The host ended the game".into();
                    }
                }
                net::Note::Closed(why) => {
                    if let Some(mut s) = self.sess.take() {
                        s.leave();
                    }
                    self.status = why;
                    self.screen = Screen::Menu;
                }
                net::Note::Msg(m) => {
                    if self.screen != Screen::Play {
                        self.status = m.clone();
                    }
                    self.msg = (m, 4.0);
                }
            }
        }
    }

    fn client(&self) -> bool {
        self.sess.as_ref().is_some_and(|s| !s.is_host())
    }
    fn paused_now(&self) -> bool {
        match &self.sess {
            Some(s) => s.paused,
            None => self.paused,
        }
    }
    fn toggle_pause(&mut self) {
        match self.sess.as_mut() {
            Some(s) => s.toggle_pause(),
            None => self.paused = !self.paused,
        }
    }
    /// Back to the menu. The co-op host takes everybody with it; a client leaves the co-op game.
    fn leave_game(&mut self) {
        self.screen = if self.in_camp { Screen::Hub } else { Screen::Menu };
        self.in_camp = false;
        if let Some(s) = self.sess.as_mut() {
            if s.is_host() {
                s.end_game();
            } else {
                s.leave();
                self.sess = None;
            }
        }
    }
    fn leave_coop(&mut self) {
        if let Some(mut s) = self.sess.take() {
            if s.is_host() && s.in_game {
                s.end_game();
            }
            s.leave();
        }
    }
    /// Carries out a player command: straight away when playing alone, through the host in co-op.
    /// Returns the message shown for it (alone only).
    fn act(&mut self, g: &mut Game, c: Cmd) -> Option<&'static str> {
        if let Some(s) = self.sess.as_mut() {
            s.issue(c);
            return None;
        }
        let m = net::apply(g, c);
        if let Some(m) = m {
            self.msg = (m.into(), 1.5);
        }
        m
    }

    /// The waiting room a client sees until the host starts.
    fn client_lobby(&mut self) {
        let p = pal(self.dark);
        let (sw, sh) = (screen_width(), screen_height());
        let r = Rect::new(sw / 2.0 - 300.0, sh / 2.0 - 140.0, 600.0, 280.0);
        self.panel(r);
        let s = self.sess.as_ref().unwrap();
        let title = format!("{}'s co-op game", s.host_name());
        let line = if let Some(f) = s.receiving() {
            format!("Receiving the map  {:.0}%", f * 100.0)
        } else if s.host_busy {
            "The host is in the middle of a game. You will join the next one.".to_string()
        } else {
            "Waiting for the host to start the game".to_string()
        };
        let names: Vec<String> = s.players.iter().map(|p| p.name.clone()).collect();
        text(&self.fonts.head, &title, r.x + 28.0, r.y + 52.0, 26.0, p.text);
        text(&self.fonts.mono, &line, r.x + 28.0, r.y + 92.0, 15.0, p.dim);
        text(&self.fonts.mono, &format!("Players: {}", names.join(", ")), r.x + 28.0, r.y + 126.0, 14.0, p.text);
        text(&self.fonts.mono, "The host picks the faction, city and difficulty. You all share one base.", r.x + 28.0, r.y + 156.0, 13.0, p.dim);
        for k in 0..12 {
            let a = self.clock * 4.0 + k as f32 * 0.52;
            let c = vec2(r.x + r.w - 50.0, r.y + 44.0) + vec2(a.cos(), a.sin()) * 14.0;
            draw_circle(c.x, c.y, 2.0, alpha(p.accent, k as f32 / 12.0));
        }
        if self.button(Rect::new(r.x + 28.0, r.y + r.h - 62.0, 140.0, 38.0), "Leave", true, false) {
            self.leave_coop();
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
    fn set_music(&mut self, v: u32) {
        if self.music_vol > 0 {
            self.music_prev = self.music_vol;
        }
        self.music_vol = v;
        self.music.set_volume(v);
        self.save_settings();
    }
    /// The music volume button: each click steps the volume down, from off back up to full.
    fn music_button(&mut self, r: Rect) {
        let v = self.music_vol;
        let label = if v == 0 { "Music off".to_string() } else { format!("Music {}%", v) };
        if self.button(r, &label, true, false) {
            let next = [75, 50, 25, 0].into_iter().find(|s| *s < v).unwrap_or(100);
            self.set_music(next);
        }
        if self.hot(r) {
            self.tip = Some("Click to change the music volume. M mutes it during a game.".into());
        }
    }
    /// Plays the sound cues the sim raised, louder near the middle of the view and panned to
    /// the side they happened on.
    fn play_sfx(&mut self, g: &mut Game) {
        let cues = std::mem::take(&mut g.sfx);
        if cues.is_empty() || self.sfx_vol == 0 {
            return;
        }
        // half the view's width in map cells, and quieter as the camera pulls back
        let r = (screen_width() / 2.0 / self.cam.scale()).max(10.0);
        let zoom = (500.0 / self.cam.dist).clamp(0.35, 1.0);
        for (s, p) in cues {
            let off = p - self.cam.t;
            let near = (1.0 - (off.length() - r).max(0.0) / (2.0 * r)).clamp(0.0, 1.0);
            let pan = (off.dot(self.cam.right()) / r).clamp(-1.0, 1.0);
            let jitter = rand::gen_range(0.93, 1.07);
            let (snd, gain, speed) = match s {
                Sfx::Cannon => (music::Snd::Cannon, 0.55, jitter),
                Sfx::Flame => (music::Snd::Flame, 0.6, jitter),
                Sfx::Railgun => (music::Snd::Railgun, 0.8, jitter),
                Sfx::ArtilleryShot => (music::Snd::ArtilleryShot, 0.8, jitter),
                Sfx::ArtilleryHit => (music::Snd::ArtilleryHit, 0.8, jitter),
                Sfx::Bomb => (music::Snd::ArtilleryHit, 0.9, 0.85 * jitter),
                Sfx::Nuke => (music::Snd::ArtilleryHit, 1.0, 0.5),
            };
            // a nuke is heard anywhere on the map
            let gain = if s == Sfx::Nuke { gain * near.max(0.7) } else { gain * near * zoom };
            self.music.play(music::Play { snd, gain, pan, speed });
        }
    }
    /// The effects volume button, stepping like the music one.
    fn sfx_button(&mut self, r: Rect) {
        let v = self.sfx_vol;
        let label = if v == 0 { "Effects off".to_string() } else { format!("Effects {}%", v) };
        if self.button(r, &label, true, false) {
            self.sfx_vol = [75, 50, 25, 0].into_iter().find(|s| *s < v).unwrap_or(100);
            self.music.set_sfx_volume(self.sfx_vol);
            self.save_settings();
        }
        if self.hot(r) {
            self.tip = Some("Click to change the volume of weapons, explosions and the battle ambience.".into());
        }
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
        if self.client() {
            while get_char_pressed().is_some() {}
            self.client_lobby();
            return;
        }
        let w = 760.0f32.min(sw - 32.0);
        let h = 650.0f32.min(sh - 32.0);
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
            !self.real && !self.camp_mode,
        ) {
            self.real = false;
            self.camp_mode = false;
        }
        if self.button(
            Rect::new(x0 + 180.0, y, 170.0, 36.0),
            "Real city",
            true,
            self.real && !self.camp_mode,
        ) {
            self.real = true;
            self.camp_mode = false;
        }
        if self.button(
            Rect::new(x0 + 360.0, y, 190.0, 36.0),
            "World campaign",
            true,
            self.camp_mode,
        ) {
            self.camp_mode = true;
        }
        y += 48.0;
        let saved_camp = if self.camp_mode { campaign::load() } else { None };
        if self.camp_mode {
            while get_char_pressed().is_some() {}
            text(&self.fonts.mono, "Ten of the world's biggest cities, Osaka to Tokyo. The horde grows 30% every city.", x0, y + 6.0, 13.0, p.dim);
            text(&self.fonts.mono, "Troops that survive march on to the next city; scrap and buildings stay behind.", x0, y + 24.0, 13.0, p.dim);
            text(&self.fonts.mono, "Civilians you fly out unlock research and recruit troops between cities.", x0, y + 42.0, 13.0, p.dim);
            let ready = self.pack.ready();
            let maps = match self.pack.get() {
                campaign::PackState::Fetching(i) if ready < CITIES.len() => format!("City maps: {}/{} ready, getting {}…", ready, CITIES.len(), CITIES[i].name),
                campaign::PackState::Failed(_) if ready < CITIES.len() => format!("City maps: {}/{} ready. The rest download when you reach them.", ready, CITIES.len()),
                _ => format!("City maps: all {} ready", CITIES.len()),
            };
            text(&self.fonts.mono, &maps, x0, y + 66.0, 13.0, p.text);
            y += 76.0;
            if let Some(c) = &saved_camp {
                let line = if c.finished() {
                    format!("Saved campaign: all ten cities held ({})", FACTIONS[c.fac].name)
                } else {
                    format!(
                        "Saved campaign: city {}/{} {} · {} · {} · {} civilians to spend",
                        c.level + 1,
                        CITIES.len(),
                        c.city().name,
                        FACTIONS[c.fac].name,
                        DIFFS[c.diff].0,
                        c.points
                    )
                };
                text(&self.fonts.mono, &line, x0, y + 22.0, 13.0, p.accent);
                let nr = Rect::new(x0 + w - 56.0 - 150.0, y + 2.0, 150.0, 30.0);
                if self.button(nr, "New campaign", true, false) {
                    self.open_campaign(Some(Campaign::new(self.fac, self.diff)));
                    return;
                }
                if self.hot(nr) {
                    self.tip = Some("Starts over in Osaka with the faction and difficulty picked here. The saved campaign is lost.".into());
                }
                y += 40.0;
            }
        } else if self.real {
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
            // cities downloaded before: one click to play again, no download
            if !self.saved.is_empty() {
                y += 6.0;
                text(&self.fonts.mono, "Played before:", x0, y + 16.0, 13.0, p.dim);
                let mut x = x0 + 112.0;
                let mut pick = None;
                let mut del = None;
                for (i, m) in self.saved.iter().take(8).enumerate() {
                    let label = m.query.clone();
                    let bw = text_w(&self.fonts.head, &label, 16.0) + 24.0;
                    if x + bw + 26.0 > x0 + w - 56.0 {
                        break;
                    }
                    let r = Rect::new(x, y, bw, 26.0);
                    let rx = Rect::new(x + bw, y, 22.0, 26.0);
                    let hot = self.hot(r);
                    draw_rectangle(r.x, r.y, r.w, r.h, if hot { mix(p.panel2, p.accent, 0.2) } else { p.panel2 });
                    text(&self.fonts.head, &label, r.x + 12.0, r.y + 18.0, 15.0, p.text);
                    let hx = self.hot(rx);
                    draw_rectangle(rx.x, rx.y, rx.w, rx.h, if hx { rgb(201, 42, 42) } else { p.panel2 });
                    text(&self.fonts.mono, "x", rx.x + 7.0, rx.y + 17.0, 13.0, if hx { WHITE } else { p.dim });
                    self.ui_rects.push(r);
                    self.ui_rects.push(rx);
                    if hot {
                        self.tip = Some(format!("{}: play again without downloading", m.name));
                    }
                    if is_mouse_button_pressed(MouseButton::Left) {
                        if hot {
                            pick = Some(i);
                        } else if hx {
                            del = Some(i);
                        }
                    }
                    x += bw + 30.0;
                }
                if let Some(i) = del {
                    storage::delete_map(&self.saved[i].path);
                    self.saved = storage::list_maps();
                } else if let Some(i) = pick {
                    self.city = self.saved[i].query.clone();
                    self.start();
                    return;
                }
                y += 34.0;
            }
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
        // co-op with Steam friends
        let cy = by - 50.0;
        if let Some(s) = &self.sess {
            let names: Vec<String> = s.players.iter().map(|p| p.name.clone()).collect();
            let line = format!("Co-op ({}/4): {}", names.len(), names.join(", "));
            if self.button(Rect::new(x0, cy, 150.0, 34.0), "Invite friends", true, true) {
                if let Some(s) = &self.sess {
                    s.invite();
                }
            }
            if self.button(Rect::new(x0 + 160.0, cy, 110.0, 34.0), "Leave co-op", true, false) {
                self.leave_coop();
            }
            text(&self.fonts.mono, &line, x0 + 284.0, cy + 22.0, 13.0, p.text);
        } else {
            let have = self.steam.is_some();
            let br = Rect::new(x0, cy, 250.0, 34.0);
            if self.button(br, "Play co-op with Steam friends", have, false) {
                if let Some(st) = &self.steam {
                    st.create();
                    self.status = "Opening a Steam lobby".into();
                }
            }
            if self.hot(br) {
                self.tip = Some(if have {
                    "Opens a lobby and the Steam invite window. Friends who accept join your base; you pick the map and press Start.".into()
                } else {
                    "Steam is not running. Start Steam, then restart Pixel Horde to play co-op.".into()
                });
            }
        }
        let can = if self.camp_mode { self.sess.is_none() } else { !self.real || self.city.trim().len() > 1 };
        let label = if self.camp_mode && saved_camp.is_some() { "Continue" } else { "Start" };
        let sr = Rect::new(r.x + r.w - 228.0, by, 200.0, 44.0);
        if self.button(sr, label, can, false) || (can && is_key_pressed(KeyCode::Enter)) {
            self.start();
        }
        if self.camp_mode && self.sess.is_some() && self.hot(sr) {
            self.tip = Some("The campaign is single player for now. Leave co-op to play it.".into());
        }
        if self.button(
            Rect::new(x0, by + 6.0, 130.0, 32.0),
            if self.dark { "Light map" } else { "Dark map" },
            true,
            false,
        ) {
            self.dark = !self.dark;
        }
        self.music_button(Rect::new(x0 + 138.0, by + 6.0, 120.0, 32.0));
        self.sfx_button(Rect::new(x0 + 264.0, by + 6.0, 128.0, 32.0));
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
                x0 + 404.0,
                by + 28.0,
                13.0,
                rgb(250, 82, 82),
            );
        }
    }

    // ------------------------------------------------------------------------------------- campaign

    /// Opens the campaign screen with a saved or new campaign.
    fn open_campaign(&mut self, c: Option<Campaign>) {
        let c = c.unwrap_or_else(|| Campaign::new(self.fac, self.diff));
        campaign::save(&c);
        self.camp = Some(c);
        self.camp_mode = true;
        self.save_settings();
        self.status.clear();
        self.screen = Screen::Hub;
    }

    /// Starts the campaign's next city: from the data folder, the city pack, or OpenStreetMap.
    fn start_city(&mut self) {
        let Some(c) = &self.camp else { return };
        let city = c.city();
        self.in_camp = true;
        self.status.clear();
        if let Some(d) = campaign::load_map(city) {
            self.begin(d, 1000 + c.level as u64);
            return;
        }
        self.screen = Screen::Loading;
        if self.pack.busy() {
            self.wait_pack = true;
            self.status = format!("Getting the {} map", city.name);
        } else {
            self.loader = Some(osm::start(city.query.to_string(), campaign::AIRPORT_M));
            self.status = "Starting download".into();
        }
    }

    /// Between cities: the campaign map, unlocks, recruits and the way into the next city.
    fn hub(&mut self) {
        let th = self.theme();
        clear_background(th.bg);
        let p = pal(self.dark);
        let (sw, sh) = (screen_width(), screen_height());
        let Some(mut c) = self.camp.clone() else {
            self.screen = Screen::Menu;
            return;
        };
        let before = (c.points, c.research.len(), c.units.len());
        let w = 900.0f32.min(sw - 32.0);
        let h = 680.0f32.min(sh - 32.0);
        let r = Rect::new((sw - w) / 2.0, (sh - h) / 2.0, w, h);
        self.panel(r);
        let (x0, mut y) = (r.x + 28.0, r.y + 50.0);
        text(&self.fonts.head, "WORLD CAMPAIGN", x0, y, 34.0, p.text);
        y += 26.0;
        text(
            &self.fonts.mono,
            &format!("{} · {} · {} civilians flown out · {} kills", FACTIONS[c.fac].name, DIFFS[c.diff].0, c.flown, fmt_n(c.kills as usize)),
            x0,
            y,
            13.0,
            p.dim,
        );
        y += 22.0;
        // the ten cities
        let n = CITIES.len();
        let cw = (w - 56.0 - (n as f32 - 1.0) * 6.0) / n as f32;
        for (i, city) in CITIES.iter().enumerate() {
            let cr = Rect::new(x0 + i as f32 * (cw + 6.0), y, cw, 58.0);
            let (bg, fc) = if i < c.level {
                (mix(p.panel2, p.accent, 0.45), WHITE)
            } else if i == c.level {
                (mix(p.panel2, p.accent, 0.15), p.text)
            } else {
                (p.panel2, p.dim)
            };
            draw_rectangle(cr.x, cr.y, cr.w, cr.h, bg);
            if i == c.level {
                draw_rectangle_lines(cr.x, cr.y, cr.w, cr.h, 2.0, p.accent);
            }
            text(&self.fonts.mono, &format!("{}", i + 1), cr.x + 6.0, cr.y + 14.0, 11.0, fc);
            let size = if text_w(&self.fonts.head, city.name, 14.0) > cw - 10.0 { 11.5 } else { 14.0 };
            text(&self.fonts.head, city.name, cr.x + 6.0, cr.y + 34.0, size, fc);
            let sub = if i < c.level { "Held".to_string() } else { format!("×{:.1}", campaign::horde_mult(i)) };
            text(&self.fonts.mono, &sub, cr.x + 6.0, cr.y + 51.0, 11.0, fc);
            self.ui_rects.push(cr);
            if self.hot(cr) {
                self.tip = Some(format!(
                    "{}: horde ×{:.2} of {} difficulty, hold out {}. Map {}.",
                    city.query,
                    campaign::horde_mult(i),
                    DIFFS[c.diff].0,
                    mmss(campaign::hold_secs(i)),
                    if campaign::have_map(city) { "downloaded" } else { "not downloaded yet" }
                ));
            }
        }
        y += 84.0;
        let by = r.y + r.h - 64.0;
        if self.button(Rect::new(x0, by + 6.0, 130.0, 32.0), "Main menu", true, false) {
            self.screen = Screen::Menu;
            return;
        }
        if c.finished() {
            text(&self.fonts.head, "All ten cities held.", x0, y + 30.0, 30.0, p.text);
            text(&self.fonts.mono, &format!("{} civilians flown out of the world's biggest cities. The world is safe, for now.", c.flown), x0, y + 64.0, 14.0, p.dim);
            if self.button(Rect::new(r.x + r.w - 228.0, by, 200.0, 44.0), "New campaign", true, true) {
                self.open_campaign(Some(Campaign::new(c.fac, c.diff)));
            }
            return;
        }
        // unlocks
        let lw = (w - 56.0) * 0.6;
        text(&self.fonts.head, "Unlock with civilians", x0, y, 18.0, p.text);
        let pts = format!("{} civilians to spend", c.points);
        text(&self.fonts.head, &pts, x0 + lw - text_w(&self.fonts.head, &pts, 16.0), y, 16.0, p.accent);
        y += 14.0;
        for rs in RESEARCH {
            let d = rdef(rs);
            let rr = Rect::new(x0, y, lw, 52.0);
            draw_rectangle(rr.x, rr.y, rr.w, rr.h, p.panel2);
            text(&self.fonts.head, d.name, rr.x + 10.0, rr.y + 19.0, 15.0, p.text);
            wrap(&self.fonts.mono, d.tip, rr.x + 10.0, rr.y + 33.0, lw - 160.0, 11.0, p.dim);
            let br = Rect::new(rr.x + rr.w - 128.0, rr.y + 11.0, 120.0, 30.0);
            if c.has(rs) {
                self.button(br, "Unlocked", false, true);
            } else if self.button(br, &format!("{} civilians", d.civs), c.points >= d.civs, false) {
                c.buy_research(rs);
            }
            y += 57.0;
        }
        // army
        let ax = x0 + lw + 24.0;
        let aw = w - 56.0 - lw - 24.0;
        let mut ay = r.y + 194.0;
        text(&self.fonts.head, "Your army", ax, ay, 18.0, p.text);
        ay += 14.0;
        let ar = Rect::new(ax, ay, aw, 74.0);
        draw_rectangle(ar.x, ar.y, ar.w, ar.h, p.panel2);
        let (rs, rn) = c.army(U::Rifle);
        let (ts, _) = c.army(U::Tank);
        text(&self.fonts.mono, &format!("Rifle squads: {} ({} soldiers)", rs, rn), ar.x + 10.0, ar.y + 24.0, 13.0, p.text);
        text(&self.fonts.mono, &format!("Tanks: {}", ts), ar.x + 10.0, ar.y + 46.0, 13.0, p.text);
        text(&self.fonts.mono, "They roll out of the HQ when the city starts.", ar.x + 10.0, ar.y + 66.0, 11.0, p.dim);
        ay += 88.0;
        for u in [U::Rifle, U::Tank] {
            let cost = campaign::recruit_cost(u);
            let br = Rect::new(ax, ay, aw, 34.0);
            if self.button(br, &format!("Recruit {} · {} civilians", udef(u).name.to_lowercase(), cost), c.points >= cost, false) {
                c.recruit(u);
            }
            ay += 40.0;
        }
        wrap(&self.fonts.mono, "Troops that survive a city march on to the next. Scrap, ammo and buildings stay behind.", ax, ay + 12.0, aw, 12.0, p.dim);
        // next city
        let city = c.city();
        let ready = campaign::have_map(city);
        let line = format!(
            "Next: {} · hold out {} for the evac · horde ×{:.2}{}",
            city.name,
            mmss(campaign::hold_secs(c.level)),
            campaign::horde_mult(c.level),
            if ready { "" } else { " · map downloads first" }
        );
        text(&self.fonts.mono, &line, x0, by - 14.0, 13.0, p.text);
        if !self.status.is_empty() {
            text(&self.fonts.mono, &self.status, x0 + 150.0, by + 28.0, 13.0, rgb(250, 82, 82));
        }
        let go = self.button(Rect::new(r.x + r.w - 228.0, by, 200.0, 44.0), &format!("Fight in {}", city.name), true, true) || is_key_pressed(KeyCode::Enter);
        if (c.points, c.research.len(), c.units.len()) != before {
            campaign::save(&c);
        }
        self.camp = Some(c);
        if go {
            self.start_city();
        }
    }

    /// Headless test hook: a campaign at city `level` with some troops and unlocks, on its screen or
    /// already playing a random city.
    pub fn test_campaign(&mut self, level: usize, play: bool) {
        let mut c = Campaign::new(std::env::var("PH_FAC").ok().and_then(|s| s.parse().ok()).unwrap_or(0), 1);
        c.level = level.min(CITIES.len());
        c.points = 260;
        c.flown = 640;
        c.kills = 182_000;
        c.research = vec![R::Rescue, R::Armour];
        c.units = vec![(U::Rifle, 6), (U::Rifle, 4), (U::Tank, 1), (U::Rifle, 6)];
        self.camp = Some(c);
        self.screen = Screen::Hub;
        if play {
            self.in_camp = true;
            let city = &CITIES[level.min(CITIES.len() - 1)];
            let d = campaign::load_map(city).unwrap_or_else(|| crate::citygen::generate(42));
            self.begin(d, 1000 + level as u64);
        }
    }

    /// Headless test hook: start a random city right away.
    pub fn quick_start(&mut self, fac: usize, seed: u64) {
        self.fac = fac;
        let d = crate::citygen::generate(seed);
        self.begin(d, seed);
        // PH_HQ=x,y (with PH_PLACE=1): the HQ moved there, as if clicked
        let at: Vec<f32> = std::env::var("PH_HQ").unwrap_or_default().split(',').filter_map(|x| x.parse().ok()).collect();
        if let [x, y] = at[..] {
            self.move_hq(vec2(x, y));
        }
    }
    pub fn game_mut(&mut self) -> Option<&mut Game> {
        self.game.as_mut()
    }
    pub fn set_view(&mut self, at: Vec2, scale: f32) {
        self.cam.t = at;
        self.cam.set_scale(scale);
    }
    pub fn set_angles(&mut self, yaw: f32, pitch: f32) {
        self.cam.yaw = yaw;
        self.cam.pitch = pitch.clamp(MIN_PITCH, TOP_PITCH);
    }

    fn save_settings(&self) {
        storage::save_settings(&storage::Settings { dark: self.dark, fac: self.fac, diff: self.diff, real: self.real, city: self.city.clone(), campaign: self.camp_mode, music: self.music_vol, sfx: self.sfx_vol });
    }

    fn start(&mut self) {
        self.status.clear();
        self.save_settings();
        if self.camp_mode {
            self.open_campaign(campaign::load());
            return;
        }
        if self.real {
            // a city played before loads straight from the data folder
            if let Some(p) = storage::find_map(self.city.trim()) {
                if let Some(d) = storage::load_map(&p) {
                    self.begin(d, 7);
                    return;
                }
            }
            self.loader = Some(osm::start(self.city.trim().to_string(), 35_000));
            self.status = "Starting download".into();
            self.screen = Screen::Loading;
        } else {
            let seed = (macroquad::miniquad::date::now() * 1000.0) as u64 ^ 0x5DEECE66D;
            let d = crate::citygen::generate(seed);
            self.begin(d, seed);
        }
    }

    /// A map is ready: first the player picks where the HQ goes (a co-op guest gets the host's
    /// choice), then the match starts.
    fn begin(&mut self, d: Desc, seed: u64) {
        let guest = self.sess.as_ref().is_some_and(|s| !s.is_host());
        if guest || self.auto_place {
            self.launch(d, seed);
            return;
        }
        let g = self.make_game(d.clone(), seed);
        self.show(g, true);
        self.placing = Some((d, seed));
        self.msg = (String::new(), 0.0);
    }

    fn make_game(&self, d: Desc, seed: u64) -> Game {
        let camp = if self.in_camp { self.camp.as_ref() } else { None };
        let mut g = match camp {
            Some(c) => Game::new(d, c.fac, DIFFS[c.diff].1 * campaign::horde_mult(c.level), seed),
            None => Game::new(d, self.fac, DIFFS[self.diff].1, seed),
        };
        if let Some(c) = camp {
            c.apply(&mut g);
        }
        g
    }

    fn show(&mut self, g: Game, recentre: bool) {
        if recentre {
            self.cam = Orbit { t: g.world.hq, dist: 0.0, yaw: 0.0, pitch: 0.95 };
            self.cam.set_scale((screen_height() / 260.0).max(1.5));
        }
        let painter = *self.depth.get_or_insert_with(pick_depth) == render::DepthMode::Painter;
        self.scene = Some(Scene::new(&g, self.theme(), painter));
        self.game = Some(g);
        self.tool = Tool::None;
        self.selected = None;
        self.paused = false;
        self.acc = 0.0;
        self.screen = Screen::Play;
    }

    /// Starts the match on `d`. The co-op host sends the map (with the HQ where it was put) first.
    fn launch(&mut self, mut d: Desc, seed: u64) {
        let chosen = self.placing.take().is_some();
        if let Some(s) = self.sess.as_mut() {
            if s.is_host() {
                d = s.start_game(&d, self.fac, self.diff, seed);
            }
        }
        let note = d.note.clone();
        let name = d.name.clone();
        // alone, the game built for the chosen spot is the one to play; the host rebuilds from the
        // map as sent, exactly like the guests do
        if !chosen || self.sess.is_some() || self.game.is_none() {
            let g = self.make_game(d, seed);
            self.show(g, !chosen);
        }
        let camp = if self.in_camp { self.camp.as_ref() } else { None };
        self.msg = match camp {
            Some(c) => (format!("{} ({}/{}). {} Hold out {} until the evac.", c.city().name, c.level + 1, CITIES.len(), note, mmss(campaign::hold_secs(c.level))), 8.0),
            None => (format!("{}. {}", name, note), 6.0),
        };
    }

    /// Whether the HQ can be set up at `p`: on dry land, not too close to the edge of the map.
    fn hq_spot_ok(g: &Game, p: Vec2) -> bool {
        let m = 40.0;
        if p.x < m || p.y < m || p.x > W as f32 - m || p.y > H as f32 - m {
            return false;
        }
        [vec2(0.0, 0.0), vec2(-7.0, -7.0), vec2(7.0, -7.0), vec2(-7.0, 7.0), vec2(7.0, 7.0)]
            .iter()
            .all(|o| g.world.terrain[(p.y + o.y) as usize * W + (p.x + o.x) as usize] != T_WATER)
    }

    /// Rebuilds the game around an HQ at `p` (the map itself stays the same).
    fn move_hq(&mut self, p: Vec2) {
        let Some((d, seed)) = self.placing.as_mut() else { return };
        d.hq = p;
        d.note = if near_airport(d, p) { "HQ set up at the airport you picked.".into() } else { "HQ set up where you picked.".into() };
        let (d, seed) = (d.clone(), *seed);
        let g = self.make_game(d, seed);
        self.show(g, false);
    }

    /// The banner while choosing the HQ spot. Returns nothing; the clicks set `place_go`.
    fn place_hud(&mut self) {
        let p = pal(self.dark);
        let sw = screen_width();
        let r = Rect::new(sw / 2.0 - 360.0, 16.0, 720.0, 108.0);
        self.panel(r);
        text(&self.fonts.head, "Choose your HQ location", r.x + 22.0, r.y + 36.0, 22.0, p.text);
        let line = match &self.sess {
            Some(_) => "Click the map to move the HQ. Your co-op team starts there too.",
            None => "Click the map to move the HQ. The horde comes once you start.",
        };
        text(&self.fonts.mono, line, r.x + 22.0, r.y + 64.0, 13.0, p.dim);
        text(&self.fonts.mono, "Drag to look around, wheel to zoom, F for the whole map.", r.x + 22.0, r.y + 86.0, 13.0, p.dim);
        if self.button(Rect::new(r.x + r.w - 160.0, r.y + 60.0, 140.0, 34.0), "Start here", true, true) || is_key_pressed(KeyCode::Enter) {
            self.place_go = true;
        }
        if self.button(Rect::new(r.x + r.w - 160.0, r.y + 16.0, 140.0, 34.0), "Back", true, false) {
            self.placing = None;
            self.screen = if self.in_camp { Screen::Hub } else { Screen::Menu };
            self.in_camp = false;
        }
        self.msg.1 -= get_frame_time();
        if self.msg.1 > 0.0 && !self.msg.0.is_empty() {
            let tw = text_w(&self.fonts.head, &self.msg.0, 16.0);
            draw_rectangle((sw - tw) / 2.0 - 14.0, r.y + r.h + 10.0, tw + 28.0, 30.0, Color::new(0.0, 0.0, 0.0, 0.7));
            text(&self.fonts.head, &self.msg.0, (sw - tw) / 2.0, r.y + r.h + 30.0, 16.0, rgb(255, 170, 170));
        }
    }

    fn loading(&mut self) {
        let th = self.theme();
        clear_background(th.bg);
        let p = pal(self.dark);
        if self.wait_pack {
            let city = self.camp.as_ref().map(|c| c.city()).unwrap_or(&CITIES[0]);
            if let Some(d) = campaign::have_map(city).then(|| campaign::load_map(city)).flatten() {
                self.wait_pack = false;
                self.begin(d, 7);
                return;
            }
            if !self.pack.busy() {
                // the pack didn't have it: straight from OpenStreetMap
                self.wait_pack = false;
                self.loader = Some(osm::start(city.query.to_string(), campaign::AIRPORT_M));
                self.status = "Starting download".into();
            }
        }
        if let Some(l) = &self.loader {
            while let Ok(m) = l.rx.try_recv() {
                match m {
                    OsmMsg::Status(s) => self.status = s,
                    OsmMsg::Fail(e) => {
                        self.status = e;
                        self.loader = None;
                        self.screen = if self.in_camp { Screen::Hub } else { Screen::Menu };
                        self.in_camp = false;
                        return;
                    }
                    OsmMsg::Done(d) => {
                        self.loader = None;
                        self.status.clear();
                        if let (true, Some(c)) = (self.in_camp, &self.camp) {
                            campaign::save_map(c.city(), &d);
                        } else {
                            storage::save_map(self.city.trim(), &d);
                            self.saved = storage::list_maps();
                        }
                        self.begin(*d, 7);
                        return;
                    }
                }
            }
        }
        let (sw, sh) = (screen_width(), screen_height());
        let r = Rect::new(sw / 2.0 - 260.0, sh / 2.0 - 80.0, 520.0, 160.0);
        self.panel(r);
        let what = match (&self.camp, self.in_camp) {
            (Some(c), true) => c.city().name.to_string(),
            _ => self.city.trim().to_string(),
        };
        text(
            &self.fonts.head,
            &format!("Loading {}", what),
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
            self.wait_pack = false;
            self.status.clear();
            self.screen = if self.in_camp { Screen::Hub } else { Screen::Menu };
            self.in_camp = false;
        }
    }

    // ------------------------------------------------------------------------------------- game

    /// The ray under a screen point, as (origin, direction) in world space.
    fn ray(&self, s: Vec2) -> (Vec3, Vec3) {
        let inv = self.cam.camera().matrix().inverse();
        let n = vec2(2.0 * s.x / screen_width() - 1.0, 1.0 - 2.0 * s.y / screen_height());
        let a = inv.project_point3(vec3(n.x, n.y, -1.0));
        let b = inv.project_point3(vec3(n.x, n.y, 1.0));
        (a, (b - a).normalize_or_zero())
    }
    /// Where a screen point lands on the ground plane.
    fn ground_at(&self, s: Vec2) -> Vec2 {
        let (o, d) = self.ray(s);
        if d.z > 1e-3 {
            (o + d * (-o.z / d.z)).truncate()
        } else {
            // above the horizon: somewhere far out along the view
            o.truncate() + vec2(d.x, d.y).normalize_or_zero() * self.cam.dist * 6.0
        }
    }
    /// The map point under the mouse: the first building the ray hits (wall or roof), else the ground.
    fn pick(&self, g: &Game, s: Vec2) -> Vec2 {
        let ground = self.ground_at(s);
        let (o, d) = self.ray(s);
        if d.z <= 1e-3 {
            return ground;
        }
        const TALLEST: f32 = 160.0;
        let t0 = ((-TALLEST - o.z) / d.z).max(0.0);
        let t1 = -o.z / d.z;
        let step = 0.5 / vec2(d.x, d.y).length().max(1e-4);
        let n = (((t1 - t0) / step) as usize).min(6000);
        for k in 0..n {
            let p = o + d * (t0 + k as f32 * step);
            if p.x < 0.0 || p.y < 0.0 || p.x >= W as f32 || p.y >= H as f32 {
                continue;
            }
            let b = g.world.bid[p.y as usize * W + p.x as usize];
            if b >= 0 {
                let bb = &g.world.buildings[b as usize];
                if bb.alive && bb.h >= -p.z {
                    return p.truncate();
                }
            }
        }
        ground
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
        self.play_sfx(&mut g);
        // fixed 20 Hz simulation; in co-op, in lockstep with the host. Nothing runs while the HQ
        // spot is being chosen.
        if self.placing.is_some() {
        } else if let Some(s) = self.sess.as_mut() {
            let me = s.me;
            let mut said = Vec::new();
            let mut n = 0;
            if s.is_host() {
                if !s.paused && !g.over {
                    self.acc += dt;
                    while self.acc >= DT && n < 3 {
                        let f = s.step().unwrap_or_default();
                        said.extend(net::run_tick(&mut g, &f, me));
                        s.after_tick(&g);
                        self.acc -= DT;
                        n += 1;
                    }
                    if n == 3 {
                        self.acc = 0.0;
                    }
                }
            } else {
                // a client runs on the host's frames, faster while catching up
                self.acc += dt;
                while n < 12 && !g.over && (self.acc >= DT || s.backlog() > 2) {
                    let Some(f) = s.step() else {
                        self.acc = self.acc.min(DT);
                        break;
                    };
                    said.extend(net::run_tick(&mut g, &f, me));
                    s.after_tick(&g);
                    self.acc = (self.acc - DT).max(0.0);
                    n += 1;
                }
            }
            if let Some(m) = said.last() {
                self.msg = (m.to_string(), 1.5);
            }
            if n > 0 {
                sc.update_units(&g);
            }
        } else if !self.paused && !g.over {
            self.acc += dt;
            let mut n = 0;
            // a huge horde can make one tick take most of a frame: then run one tick per frame and
            // let the game slow down a little, instead of catching up and freezing the screen
            let most = if g.sim_ms > 20.0 { 1 } else { 3 };
            while self.acc >= DT && n < most {
                g.tick();
                self.acc -= DT;
                n += 1;
            }
            if n == most {
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
        let (rw, rh) = (sw.max(1.0) as u32, sh.max(1.0) as u32);
        let depth = *self.depth.get_or_insert_with(pick_depth);
        match depth {
            render::DepthMode::Target(sample_count) => {
                if self.rt.as_ref().map(|r| (r.texture.width() as u32, r.texture.height() as u32)) != Some((rw, rh)) {
                    let rt = render_target_ex(rw, rh, RenderTargetParams { sample_count, depth: true });
                    rt.texture.set_filter(FilterMode::Nearest);
                    self.rt = Some(rt);
                }
            }
            render::DepthMode::Painter => self.rt = None,
        }
        let mut cam = self.cam.camera();
        cam.render_target = self.rt.clone();
        set_camera(&cam);
        clear_background(th.bg);
        let radius = [vec2(0.0, 0.0), vec2(sw, 0.0), vec2(0.0, sh), vec2(sw, sh)]
            .iter()
            .map(|c| self.ground_at(*c).distance(self.cam.t))
            .fold(0.0, f32::max);
        let mut labels = WorldLabels { items: Vec::new() };
        render::draw_world(
            &mut sc,
            &g,
            th,
            &ViewInfo {
                center: self.cam.t,
                radius,
                scale: self.cam.scale(),
                selected: self.selected,
                time: self.clock,
                eye: cam.position.truncate(),
            },
            &mut labels,
        );
        let mw = self.pick(&g, self.mouse());
        // the other co-op players' cursors
        if let (Some(s), None) = (self.sess.as_mut(), &self.placing) {
            s.cursor(mw, dt);
            let mk = (2.2 / self.cam.scale()).max(1.0);
            for (i, pl) in s.players.iter().enumerate() {
                if let (Some(c), true) = (pl.cursor, i != s.me) {
                    let col = PLAYER_COLS[i % 4];
                    render::draw_pin(c, 2.5 * mk, col, WHITE);
                    labels.items.push((render::p3(c, 7.0 * mk), pl.name.clone(), col, 13.0));
                }
            }
        }
        gl_use_material(&sc.flat);
        let cursor_label = self.preview(&g, &sc, mw);
        gl_use_default_material();
        set_default_camera();
        if let Some(rt) = &self.rt {
            draw_texture_ex(&rt.texture, 0.0, 0.0, WHITE, DrawTextureParams { dest_size: Some(vec2(sw, sh)), flip_y: true, ..Default::default() });
        }
        let m = cam.matrix();
        for (p, s, c, size) in &labels.items {
            let Some(q) = to_screen(&m, *p) else { continue };
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

        if self.placing.is_some() {
            self.place_hud();
        } else {
            self.hud(&mut g);
        }
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
        if self.placing.is_some() {
            if let Some(p) = self.place_at.take() {
                self.move_hq(p);
            }
            if std::mem::take(&mut self.place_go) {
                let (d, seed) = self.placing.clone().unwrap();
                self.launch(d, seed);
            }
        }
        self.place_at = None;
        self.place_go = false;
    }

    fn toggle_top(&mut self) {
        if self.cam.top() {
            self.cam.pitch = 0.95;
        } else {
            self.cam.pitch = TOP_PITCH;
            self.cam.yaw = 0.0;
        }
    }

    fn over_ui(&self) -> bool {
        let m = self.mouse();
        self.ui_rects.iter().any(|r| r.contains(m))
    }

    /// Ground marks are drawn with `flat`; ghosts of buildings are real 3D boxes.
    fn preview(&self, g: &Game, sc: &Scene, mw: Vec2) -> Option<(String, Color)> {
        let flat = &sc.flat;
        let ghost = |x: f32, y: f32, w: f32, h: f32, z: f32, ok: bool| {
            sc.use_solid();
            render::ghost(x, y, w, h, z, ok);
            gl_use_material(flat);
        };
        if self.over_ui() {
            return None;
        }
        if self.placing.is_some() {
            let ok = Self::hq_spot_ok(g, mw);
            let air = near_airport(&g.world.desc, mw);
            let col = if ok { rgba(252, 180, 40, 0.9) } else { rgba(255, 120, 120, 0.9) };
            draw_circle_lines(mw.x, mw.y, if air { HQ_CLEAR + 20.0 } else { 110.0 }, 1.2, col);
            ghost(mw.x - 7.0, mw.y - 7.0, 14.0, 14.0, 6.0, ok);
            let s = match (ok, air) {
                (false, _) => "Can't build the HQ here",
                (true, true) => "HQ here (the airport joins your base zone)",
                (true, false) => "HQ here",
            };
            return Some((s.to_string(), if ok { WHITE } else { rgb(255, 140, 140) }));
        }
        match self.tool {
            Tool::Build(t) => {
                let d = def(t);
                // rooftop spot for turrets
                if d.turret && self.press.is_none() {
                    if let Some((b, p)) = g.roof_at(mw) {
                        let q = p;
                        let can = g.can_place_roof(b, p);
                        let cost = g.cost_at(t, p);
                        let afford = g.scrap >= cost;
                        ghost(q.x - d.w as f32 / 2.0, q.y - d.h as f32 / 2.0, d.w as f32, d.h as f32, g.world.buildings[b].h, can && afford);
                        draw_circle_lines(q.x, q.y, g.turret_range(t, true), 0.4, rgba(255, 212, 59, 0.6));
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
                let spots = net::plan(t, a, b);
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
                    ghost(*x as f32, *y as f32, d.w as f32, d.h as f32, 0.0, can);
                }
                if d.turret && spots.len() == 1 {
                    let c = vec2(
                        spots[0].0 as f32 + d.w as f32 / 2.0,
                        spots[0].1 as f32 + d.h as f32 / 2.0,
                    );
                    draw_circle_lines(c.x, c.y, g.turret_range(t, false), 0.4, rgba(255, 212, 59, 0.6));
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
        let mut stats = vec![
            ("Scrap", format!("{:.0}", g.scrap)),
            ("Ammo", format!("{:.0}", g.ammo)),
            ("Score", format!("{:.0}", g.score)),
            ("Hostiles", fmt_n(g.hostiles())),
            ("Civilians", fmt_n(g.alive_civs())),
            ("Rescued", format!("{}", g.rescued)),
            ("Waiting", format!("{}", g.waiting)),
            ("Flown out", format!("{}", g.extracted)),
        ];
        if g.goal > 0.0 {
            stats.push(("Evac in", mmss((g.goal - g.time).max(0.0))));
        }
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
        self.music_button(Rect::new(bx - 226.0, 6.0, 104.0, 28.0));
        self.sfx_button(Rect::new(bx - 116.0, 6.0, 110.0, 28.0));
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
            if self.cam.top() { "3D view" } else { "Top view" },
            true,
            false,
        ) {
            self.toggle_top();
        }
        if self.button(
            Rect::new(bx + 160.0, 6.0, 74.0, 28.0),
            if self.paused_now() { "Resume" } else { "Pause" },
            true,
            self.paused_now(),
        ) {
            self.toggle_pause();
        }
        if self.button(Rect::new(bx + 240.0, 6.0, 80.0, 28.0), "Menu", true, false) {
            self.leave_game();
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
            .filter(|s| !(g.campaign && **s == S::Lab))
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
            if let Some(rs) = ability_research(a) {
                self.research_button(g, rs, ur, "Upgrade");
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

        // lab research that isn't tied to an ability
        if !g.campaign && g.has(S::Lab) {
            let r = Rect::new(ax, ay, aw, 30.0 + LAB_ONLY.len() as f32 * 30.0);
            self.panel(r);
            text(&self.fonts.head, "Research lab", r.x + 12.0, r.y + 20.0, 15.0, p.text);
            for (i, rs) in LAB_ONLY.iter().enumerate() {
                let ur = Rect::new(r.x + 8.0, r.y + 28.0 + i as f32 * 30.0, r.w - 16.0, 24.0);
                self.research_button(g, *rs, ur, "Research");
            }
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
        if let Some(s) = &self.sess {
            let names: Vec<String> = s.players.iter().map(|p| p.name.clone()).collect();
            text(&self.fonts.mono, &format!("Co-op: {}", names.join(", ")), 12.0, 58.0, 12.0, p.dim);
        }
        if self.paused_now() {
            let s = "PAUSED";
            let tw = text_w(&self.fonts.head, s, 30.0);
            text(&self.fonts.head, s, (sw - tw) / 2.0, sh / 2.0, 30.0, WHITE);
        }
        if g.over && self.in_camp {
            self.campaign_over(g);
        } else if g.over {
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
                self.leave_game();
            }
        }
    }

    /// The end of a campaign city: held (bank civilians, carry the army on) or lost (try again).
    fn campaign_over(&mut self, g: &mut Game) {
        let p = pal(self.dark);
        let (sw, sh) = (screen_width(), screen_height());
        let Some(c) = self.camp.clone() else { return };
        let r = Rect::new(sw / 2.0 - 260.0, sh / 2.0 - 110.0, 520.0, 220.0);
        self.panel(r);
        let squads = g.groups.iter().filter(|gr| !gr.dead).count();
        if g.won {
            let last = c.level + 1 >= CITIES.len();
            text(&self.fonts.head, &format!("{} held", c.city().name), r.x + 24.0, r.y + 46.0, 26.0, p.text);
            text(&self.fonts.mono, &format!("{} civilians flown out: +{} to spend", g.extracted, g.extracted), r.x + 24.0, r.y + 82.0, 14.0, p.accent);
            let army = if last {
                "That was the last city.".to_string()
            } else if squads == 0 {
                format!("No troops survived to march on to {}.", CITIES[c.level + 1].name)
            } else {
                format!("{} squad{} march on to {}.", squads, if squads == 1 { "" } else { "s" }, CITIES[c.level + 1].name)
            };
            text(&self.fonts.mono, &army, r.x + 24.0, r.y + 106.0, 14.0, p.text);
            text(&self.fonts.mono, &format!("{} kills · score {:.0}", fmt_n(g.kills as usize), g.score), r.x + 24.0, r.y + 130.0, 13.0, p.dim);
            if self.button(Rect::new(r.x + 24.0, r.y + 154.0, 200.0, 42.0), "Continue", true, true) {
                let mut c = c;
                c.harvest(g);
                campaign::save(&c);
                self.camp = Some(c);
                self.in_camp = false;
                self.screen = Screen::Hub;
            }
        } else {
            text(&self.fonts.head, &format!("{} has fallen", c.city().name), r.x + 24.0, r.y + 46.0, 26.0, p.text);
            text(&self.fonts.mono, "The HQ was overrun. Your army and civilians are as they were before this city.", r.x + 24.0, r.y + 82.0, 13.0, p.dim);
            text(&self.fonts.mono, &format!("Held for {} of {}.", mmss(g.time), mmss(g.goal)), r.x + 24.0, r.y + 106.0, 13.0, p.dim);
            if self.button(Rect::new(r.x + 24.0, r.y + 154.0, 200.0, 42.0), "Try again", true, true) {
                self.camp = campaign::load().or(Some(c));
                self.in_camp = false;
                self.screen = Screen::Hub;
            }
            if self.button(Rect::new(r.x + 236.0, r.y + 154.0, 160.0, 42.0), "Main menu", true, false) {
                self.in_camp = false;
                self.screen = Screen::Menu;
            }
        }
    }

    /// One research row: shows its state and starts it on click.
    fn research_button(&mut self, g: &mut Game, rs: R, ur: Rect, verb: &str) {
        let p = pal(self.dark);
        let d = rdef(rs);
        let (label, en) = if g.research.has(rs) {
            (format!("Done: {}", d.name), false)
        } else if g.campaign {
            ("Unlock between cities".to_string(), false)
        } else if let Some((busy, t)) = g.research.busy {
            if busy == rs {
                (format!("Researching… {:.0}s", t), false)
            } else {
                (format!("{}: {} ${:.0}", verb, d.name, d.cost), false)
            }
        } else if !g.has(S::Lab) {
            (format!("{}: needs research lab", verb), false)
        } else {
            (format!("{}: {} ${:.0}", verb, d.name, d.cost), g.can_research(rs))
        };
        let hot = self.hot(ur);
        draw_rectangle(ur.x, ur.y, ur.w, ur.h, if hot && en { mix(p.panel2, p.accent, 0.2) } else { p.panel2 });
        text(&self.fonts.mono, &label, ur.x + 8.0, ur.y + 16.0, 11.5, if en { p.accent } else { p.dim });
        if hot {
            self.tip = Some(format!("{}: {}", d.name, d.tip));
            if en && is_mouse_button_pressed(MouseButton::Left) {
                self.act(g, Cmd::Research(rs));
            }
        }
    }

    fn input(&mut self, g: &mut Game, sc: &mut Scene, mw: Vec2, dt: f32) {
        let ui = self.over_ui();
        // keyboard
        while let Some(ch) = get_char_pressed() {
            let ch = ch.to_ascii_lowercase();
            if let Some(s) = BUILD_ORDER.iter().find(|s| def(**s).key == ch && !(g.campaign && **s == S::Lab) && self.placing.is_none()) {
                self.tool = if self.tool == Tool::Build(*s) {
                    Tool::None
                } else {
                    Tool::Build(*s)
                };
            } else if self.placing.is_some() && !"tkmfh".contains(ch) {
            } else if let Some(u) = [U::Rifle, U::Tank].iter().find(|u| udef(**u).key == ch) {
                self.tool = Tool::Unit(*u);
            } else if let Some(a) = ABILITIES.iter().find(|a| adef(**a).key == ch) {
                if g.can_use(*a) {
                    self.tool = Tool::Ability(*a);
                }
            } else {
                match ch {
                    't' => self.toggle_top(),
                    'k' => self.dark = !self.dark,
                    'm' => self.set_music(if self.music_vol > 0 { 0 } else { self.music_prev }),
                    'p' | ' ' => self.toggle_pause(),
                    'h' => self.cam.t = g.world.hq,
                    'f' => {
                        self.cam.t = vec2(W as f32, H as f32) / 2.0;
                        self.cam.set_scale((screen_height() / H as f32).min(screen_width() / W as f32));
                    }
                    'x' => {
                        self.act(g, Cmd::Sell(mw));
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
        if is_key_pressed(KeyCode::F9) && self.sess.is_none() && self.placing.is_none() {
            g.stress();
        }
        // camera: WASD pans along the view, Z/C turn, PageUp/PageDown tilt
        let pan = 500.0 / self.cam.scale() * dt;
        let (fwd, right) = (-self.cam.back(), self.cam.right());
        if is_key_down(KeyCode::W) || is_key_down(KeyCode::Up) {
            self.cam.t += fwd * pan;
        }
        if is_key_down(KeyCode::S) || is_key_down(KeyCode::Down) {
            self.cam.t -= fwd * pan;
        }
        if is_key_down(KeyCode::A) || is_key_down(KeyCode::Left) {
            self.cam.t -= right * pan;
        }
        if is_key_down(KeyCode::D) || is_key_down(KeyCode::Right) {
            self.cam.t += right * pan;
        }
        if is_key_down(KeyCode::Z) {
            self.cam.yaw -= 1.8 * dt;
        }
        if is_key_down(KeyCode::C) {
            self.cam.yaw += 1.8 * dt;
        }
        if is_key_down(KeyCode::PageUp) {
            self.cam.pitch -= 1.2 * dt;
        }
        if is_key_down(KeyCode::PageDown) {
            self.cam.pitch += 1.2 * dt;
        }
        let (_, wy) = mouse_wheel();
        if wy != 0.0 && !ui {
            // zoom towards the point under the cursor
            let before = self.ground_at(self.mouse());
            let k = if wy > 0.0 { 1.0 / 1.15 } else { 1.15 };
            self.cam.dist = (self.cam.dist * k).clamp(20.0, 2600.0);
            let after = self.ground_at(self.mouse());
            self.cam.t += before - after;
        }
        let md = self.mouse() - self.last_m;
        let last = self.last_m;
        self.last_m = self.mouse();
        // middle drag (or Alt + left drag) orbits: sideways turns, up and down tilts
        let alt = is_key_down(KeyCode::LeftAlt) || is_key_down(KeyCode::RightAlt);
        if is_mouse_button_down(MouseButton::Middle) || (alt && is_mouse_button_down(MouseButton::Left) && self.press.is_some()) {
            self.cam.yaw += md.x * 0.008;
            self.cam.pitch += md.y * 0.006;
            if alt {
                self.panning = true;
            }
        }
        self.cam.pitch = self.cam.pitch.clamp(MIN_PITCH, TOP_PITCH);
        self.cam.t = self.cam.t.clamp(vec2(0.0, 0.0), vec2(W as f32, H as f32));
        let _ = sc;

        // right click: order the selected group, or drop the tool
        if is_mouse_button_pressed(MouseButton::Right) && !ui {
            if self.tool != Tool::None {
                self.tool = Tool::None;
                self.press = None;
            } else if let Some(gi) = self.selected {
                self.act(g, Cmd::Order(gi as u32, mw));
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
                if self.panning && !alt {
                    // grab the ground: the point under the cursor stays under the cursor
                    self.cam.t += self.ground_at(last) - self.ground_at(self.mouse());
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
            if self.placing.is_some() {
                if Self::hq_spot_ok(g, mw) {
                    self.place_at = Some(mw);
                } else {
                    self.msg = ("The HQ needs dry land, away from the edge of the map".into(), 2.0);
                }
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
                            self.act(g, Cmd::Roof { t, b: b as u32, p });
                            return;
                        }
                    }
                    self.act(g, Cmd::Build { t, a: w0, b: mw });
                }
                Tool::Unit(u) => {
                    self.act(g, Cmd::Deploy(u, mw));
                }
                Tool::Ability(a) => {
                    let from = if a == A::Air && g.research.air { w0 } else { mw };
                    if self.act(g, Cmd::Ability(a, from, mw)) != Some(net::BAD_TARGET) {
                        self.tool = Tool::None;
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
/// Finds a way of drawing the 3D view whose depth test works on this GPU (see `render::check_depth`)
/// and notes the result in data/gpu.txt. `PH_DEPTH=4|1|window` forces one.
fn pick_depth() -> render::DepthMode {
    use render::DepthMode::*;
    let forced = match std::env::var("PH_DEPTH").ok().as_deref() {
        Some("window" | "painter") => Some(Painter),
        Some(n) => n.parse().ok().map(Target),
        None => None,
    };
    if let Some(m) = forced {
        return m;
    }
    let tries: &[i32] = if std::env::var("PH_SHOT").is_ok() && std::env::var("PH_MSAA").is_err() { &[1] } else { &[4, 1] };
    let mut log = gl_info();
    let mut pick = Painter;
    for &n in tries {
        let (r, px) = render::check_depth(n);
        log.push_str(&format!("render target, {} samples: {:?} {:?}\n", n, r, px));
        if r == render::DepthCheck::Ok {
            pick = Target(n);
            break;
        }
    }
    log.push_str(&format!("using {:?}\n", pick));
    let _ = std::fs::write(storage::data_dir().join("gpu.txt"), &log);
    if std::env::var("PH_SHOT").is_ok() {
        eprint!("{}", log);
    }
    pick
}

/// The GPU and driver, for data/gpu.txt.
fn gl_info() -> String {
    use miniquad::gl::*;
    let get = |k: u32| unsafe {
        let p = glGetString(k);
        if p.is_null() {
            "?".to_string()
        } else {
            std::ffi::CStr::from_ptr(p as _).to_string_lossy().into_owned()
        }
    };
    // GL_RENDERER is 0x1F01
    format!("{} / {} / OpenGL {}\n", get(GL_VENDOR), get(0x1F01), get(GL_VERSION))
}

/// Seconds as m:ss.
fn mmss(t: f32) -> String {
    let s = t.max(0.0).ceil() as u32;
    format!("{}:{:02}", s / 60, s % 60)
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

/// `PH_HOST=port` or `PH_JOIN=addr:port` start a co-op session over plain TCP (testing, LAN).
fn test_session() -> Option<net::Session> {
    if let Some(port) = std::env::var("PH_HOST").ok().and_then(|p| p.parse().ok()) {
        let l = net::tcp::host(port).ok()?;
        return Some(net::Session::new_host(Box::new(l), "Host".into()));
    }
    let addr = std::env::var("PH_JOIN").ok()?;
    for _ in 0..50 {
        if let Ok(l) = net::tcp::join(&addr) {
            return Some(net::Session::new_client(Box::new(l), "Guest".into()));
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    None
}

impl App {
    /// Headless co-op test (`PH_NETTEST`): the host starts as soon as a guest is in, both sides
    /// issue a few commands, and every 100 ticks each prints its checksum. Returns true when done.
    pub fn net_test(&mut self, until: u64) -> bool {
        let Some(s) = self.sess.as_ref() else { return true };
        let host = s.is_host();
        if host && !s.in_game && s.players.len() >= 2 && self.screen == Screen::Menu {
            self.fac = 0;
            self.start();
            return false;
        }
        if self.screen != Screen::Play {
            return false;
        }
        let exec = s.exec;
        let Some(g) = self.game.as_ref() else { return false };
        let hq = g.world.hq;
        let plan: &[(u64, bool, Cmd)] = &[
            (20, true, Cmd::Build { t: S::Gun, a: hq + vec2(24.0, 0.0), b: hq + vec2(24.0, 0.0) }),
            (25, false, Cmd::Build { t: S::Wall, a: hq + vec2(-40.0, 30.0), b: hq + vec2(-10.0, 30.0) }),
            (30, false, Cmd::Deploy(U::Rifle, hq + vec2(0.0, -40.0))),
            (40, true, Cmd::Ability(A::Rescue, hq + vec2(120.0, 60.0), hq + vec2(120.0, 60.0))),
            (60, false, Cmd::Build { t: S::Gun, a: hq + vec2(-24.0, 0.0), b: hq + vec2(-24.0, 0.0) }),
        ];
        let step = self.test_step as usize;
        if let Some((at, by_host, c)) = plan.get(step) {
            if exec >= *at {
                if *by_host == host {
                    self.sess.as_mut().unwrap().issue(*c);
                }
                self.test_step += 1;
            }
        }
        let s = self.sess.as_ref().unwrap();
        if exec % 100 == 0 && exec > self.test_log {
            self.test_log = exec;
            eprintln!("{} tick {} checksum {:016x} structs {} hostiles {}{}", if host { "host" } else { "guest" }, exec, g.checksum(), g.structs.iter().filter(|s| !s.dead).count(), g.hostiles(), if s.desync { " DESYNC" } else { "" });
        }
        exec >= until
    }
}

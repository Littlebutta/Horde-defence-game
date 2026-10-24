//! Screens, HUD and input: the start menu, the city loader, and the game itself.

use crate::campaign::{self, Campaign, Fate, CITIES};
use crate::globe;
use crate::defs::*;
use crate::game::*;
use crate::net::{self, Cmd};
use crate::osm::{self, OsmMsg};
use crate::menubg::MenuBg;
use crate::music;
use crate::storage;
use crate::render::{self, text, text_w, Fonts, Scene, ViewInfo, WorldLabels};
use crate::ui::{self, alpha, mix, pal, rrect, rrect_lines, Flow, RectExt};
use crate::update::{UpState, Updater};
use crate::world::*;
use macroquad::prelude::*;

mod invite;
mod settings;
mod widgets;
use widgets::{R_CTRL, R_PANEL};

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
    /// the campaign's globe, made the first time it's shown
    globe: Option<globe::Globe>,
    /// the city the globe last turned to
    globe_city: Option<usize>,
    /// which part of the campaign's tech tree is showing
    tech_tab: usize,
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
    /// a selected artillery piece or heli pad (right-click aims it)
    sel_struct: Option<usize>,
    /// height of the build bar, so panels above it can sit clear
    bar_h: f32,
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
    /// the "Defending <city>" banner at the start of a campaign city: (city, country, line, start)
    banner: Option<(String, String, String, f32)>,
    /// the slider being dragged
    drag: Option<u32>,
    /// the settings dialog is open over the screen
    settings_open: bool,
    /// opening settings paused the game (it resumes when they close)
    settings_paused: bool,
    /// a dialog is open: controls under it don't react
    blocked: bool,
    /// the invite dialog (Steam friends) is open over the lobby
    invite_open: bool,
    invite_scroll: f32,
    /// the Steam friends list, and when it was last read
    friends: Vec<net::Friend>,
    friends_at: f32,
    /// friends invited from the dialog this session
    invited: Vec<u64>,
    show_fps: bool,
    fullscreen: bool,
    /// anti-aliasing: 0 = 4x MSAA, 1 = off, 2 = compatible (no render target, painter's order)
    aa: u8,
    /// the 3D view's resolution in percent of the window
    render_pct: u32,
    /// the graphics settings changed: rebuild the scene's way of drawing
    regfx: bool,
    /// the battle behind the menu, made on the first menu frame
    bg: Option<MenuBg>,
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
/// Hostiles the "Spawn 1M" stress-test button (and F9) drops in.
const STRESS_N: u32 = 1_000_000;
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

impl App {
    pub fn new(steam: Option<net::Steam>) -> App {
        let st = storage::load_settings();
        ui::set_user_scale(st.ui_scale);
        ui::set_wide(st.hud_wide);
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
            globe: None,
            globe_city: None,
            tech_tab: 0,
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
            sel_struct: None,
            bar_h: 70.0,
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
            banner: None,
            auto_place: ["PH_SHOT", "PH_NETTEST"].iter().any(|k| std::env::var(k).is_ok()) && std::env::var("PH_PLACE").is_err(),
            drag: None,
            settings_open: std::env::var("PH_SETTINGS").is_ok(),
            settings_paused: false,
            blocked: false,
            invite_open: false,
            invite_scroll: 0.0,
            friends: Vec::new(),
            friends_at: -10.0,
            invited: Vec::new(),
            show_fps: st.fps,
            fullscreen: st.fullscreen,
            aa: st.aa,
            render_pct: st.render,
            regfx: false,
            bg: None,
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
        self.sync_lobby();
        if is_key_pressed(KeyCode::F11) {
            self.set_fullscreen(!self.fullscreen);
        }
        if self.screen == Screen::Play {
            // the game draws its 3D view in pixels and switches to points for the HUD itself
            self.play(dt);
            return;
        }
        ui::begin();
        self.blocked = self.settings_open || self.invite_open;
        match self.screen {
            Screen::Menu => self.menu(),
            Screen::Loading => self.loading(),
            Screen::Hub => self.hub(),
            Screen::Play => {}
        }
        self.overlays();
        ui::end();
    }

    /// What goes over any screen, in points: the settings dialog and the tooltip.
    fn overlays(&mut self) {
        self.blocked = false;
        if self.invite_open && self.screen != Screen::Play && !self.settings_open {
            self.tip = None;
            self.invites();
        }
        if self.settings_open {
            self.tip = None;
            self.settings();
        }
        if let Some(t) = self.tip.clone() {
            self.tooltip(&t);
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
                            net::Session::new_host(link, name)
                        } else {
                            net::Session::new_client(link, name)
                        });
                        if host {
                            self.open_invites();
                        }
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
                net::Note::Started { desc, fac, diff, seed, camp } => {
                    self.fac = fac;
                    self.diff = diff;
                    // a campaign city: play it with the host's campaign (never saved here)
                    self.in_camp = camp.is_some();
                    self.camp = camp.map(|t| campaign::from_text(&t));
                    self.begin(*desc, seed);
                }
                net::Note::Ended => {
                    if self.screen == Screen::Play {
                        self.screen = Screen::Menu;
                        self.status = if self.in_camp { "Back in the lobby. The host is picking the next city.".into() } else { "The host ended the game".into() };
                    }
                    self.in_camp = false;
                    self.camp = None;
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
        // a guest's campaign city was the host's campaign: nothing of theirs to go back to
        let guest = self.client();
        self.screen = if self.in_camp && !guest { Screen::Hub } else { Screen::Menu };
        self.in_camp = false;
        if guest {
            self.camp = None;
        }
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

    /// Where the menu column goes, and in co-op the players panel beside it. The column sits on the
    /// left of the HUD area so the rest of the screen shows what's behind it; a window too narrow
    /// for that gets them centred.
    fn menu_rects(&self) -> (Rect, Option<Rect>) {
        let area = ui::hud();
        let h = (area.h - 64.0).min(940.0);
        let (w, pw, gap) = (548.0, 340.0, 24.0);
        let lobby = self.sess.is_some();
        let total = if lobby { w + gap + pw } else { w };
        let narrow = area.w < 1100.0 || area.w < area.h;
        let x = if narrow { area.x + (area.w - total) / 2.0 } else { area.x + 48.0 };
        let r = Rect::new(x, area.y + (area.h - h) / 2.0, w, h);
        let pr = lobby.then(|| Rect::new(r.right() + gap, r.y, pw, 4.0 * 62.0 + 210.0));
        (r, pr)
    }

    /// The co-op players, host first, with open slots that bring up the Steam invite window.
    fn players_panel(&mut self, r: Rect) {
        let p = pal(self.dark);
        let Some(s) = self.sess.as_ref() else { return };
        let rows: Vec<(String, bool, &str)> = s.players.iter().enumerate().map(|(i, pl)| (pl.name.clone(), i == s.me, s.route(i))).collect();
        let mut a = self.card(r, &format!("Players {}/4", rows.len()));
        let mut invite = false;
        for k in 0..4 {
            let pr = a.cut_top(54.0, 8.0);
            if let Some((name, me, route)) = rows.get(k) {
                let col = PLAYER_COLS[k];
                rrect(pr, R_CTRL, mix(p.panel2, col, 0.08));
                rrect_lines(pr, R_CTRL, 1.0, alpha(col, 0.5));
                draw_circle(pr.x + 26.0, pr.y + 27.0, 9.0, col);
                draw_circle_lines(pr.x + 26.0, pr.y + 27.0, 9.0, 1.5, WHITE);
                let name = fit(&self.fonts.head, name, pr.w - 60.0, 16.0);
                text(&self.fonts.head, &name, pr.x + 46.0, pr.y + 24.0, 16.0, p.text);
                let tag = match (k == 0, *me) {
                    (true, true) => "HOST · YOU",
                    (true, false) => "HOST",
                    (false, true) => "YOU",
                    _ => "IN THE LOBBY",
                };
                // how this player's messages travel: straight between the PCs, or through Steam
                let tag = if route.is_empty() { tag.to_string() } else { format!("{} · {}", tag, route.to_uppercase()) };
                text(&self.fonts.mono, &tag, pr.x + 46.0, pr.y + 42.0, 10.5, p.dim);
            } else {
                let hot = self.hot(pr);
                rrect(pr, R_CTRL, if hot { mix(p.panel2, p.text, 0.06) } else { alpha(p.panel2, 0.5) });
                rrect_lines(pr, R_CTRL, 1.0, if hot { alpha(p.accent, 0.8) } else { p.line });
                draw_circle_lines(pr.x + 26.0, pr.y + 27.0, 9.0, 1.5, p.dim);
                text(&self.fonts.mono, "+", pr.x + 22.0, pr.y + 32.0, 14.0, p.dim);
                text(&self.fonts.mono, "Open slot · invite a friend", pr.x + 46.0, pr.y + 32.0, 13.0, if hot { p.text } else { p.dim });
                self.ui_rects.push(pr);
                invite |= hot && is_mouse_button_pressed(MouseButton::Left);
            }
        }
        a.cut_top(4.0, 0.0);
        let line = "Everybody shares one base, its scrap and its ammo, and sees each other's cursors. The host's machine runs the clock.";
        for (i, l) in wrap_lines(&self.fonts.mono, line, a.w, 12.0).iter().enumerate() {
            text(&self.fonts.mono, l, a.x, a.y + 12.0 + i as f32 * 16.0, 12.0, p.dim);
        }
        if invite {
            self.open_invites();
        }
    }

    /// The lobby as a friend sees it: the host's picks, live, until the host starts.
    fn guest_lobby(&mut self) {
        let p = pal(self.dark);
        let (r, pr) = self.menu_rects();
        self.panel(r);
        let s = self.sess.as_ref().unwrap();
        let hn = s.host_name();
        let lob = s.lobby.clone();
        let (recv, busy) = (s.receiving(), s.host_busy);
        let mut a = r.pad(30.0, 26.0);
        let t = a.cut_top(48.0, 2.0);
        text(&self.fonts.head, "CO-OP LOBBY", t.x, t.y + 40.0, 46.0, p.text);
        let t = a.cut_top(18.0, 18.0);
        let sub = fit(&self.fonts.mono, &format!("{}'s game. The host picks what you play.", hn), t.w, 14.0);
        text(&self.fonts.mono, &sub, t.x, t.y + 13.0, 14.0, p.dim);
        let foot = a.cut_bottom(36.0, 14.0);
        let start = a.cut_bottom(56.0, 20.0);
        let state = a.cut_bottom(40.0, 16.0);

        // the host's picks, read only
        let y = self.heading("Game mode", a.x, a.y);
        a.cut_top(y - a.y, 0.0);
        let mode = lob.mode.min(2) as usize;
        let _ = self.segmented(a.cut_top(40.0, 12.0), &["Random city", "Real city", "World campaign"], mode);
        let detail = match lob.mode {
            net::MODE_CAMPAIGN => format!("The host's campaign: {}.", lob.city),
            net::MODE_REAL if !lob.city.is_empty() => format!("{}, from OpenStreetMap.", lob.city),
            net::MODE_REAL => "A real city. The host is typing it in.".into(),
            _ => "A new random city every game: river, airport, avenues and downtown all move around.".into(),
        };
        let lines = wrap_lines(&self.fonts.mono, &detail, a.w, 13.0);
        let d = a.cut_top(lines.len() as f32 * 18.0, 18.0);
        for (i, l) in lines.iter().enumerate() {
            text(&self.fonts.mono, l, d.x, d.y + 13.0 + i as f32 * 18.0, 13.0, p.text);
        }
        let y = self.heading("Faction", a.x, a.y);
        a.cut_top(y - a.y, 0.0);
        let rows = a.cut_top(3.0 * 60.0 + 2.0 * 8.0, 18.0).rows(3, 8.0);
        for (i, f) in FACTIONS.iter().enumerate() {
            let cr = rows[i];
            let active = lob.fac as usize == i;
            let c = Color::from_rgba(f.col[0], f.col[1], f.col[2], 255);
            rrect(cr, R_CTRL, if active { mix(p.panel2, c, 0.16) } else { alpha(p.panel2, 0.5) });
            rrect_lines(cr, R_CTRL, if active { 2.0 } else { 1.0 }, if active { c } else { p.line });
            for k in 0..16 {
                let q = vec2(cr.x + 14.0 + (k * 37 % 27) as f32, cr.y + 16.0 + (k * 13 % 29) as f32);
                draw_rectangle(q.x, q.y, 3.0, 3.0, if active { c } else { alpha(c, 0.4) });
            }
            text(&self.fonts.head, f.name, cr.x + 56.0, cr.y + 22.0, 17.0, if active { p.text } else { p.dim });
            let lines = wrap_lines(&self.fonts.mono, f.blurb, cr.w - 70.0, 11.5);
            for (k, l) in lines.iter().take(2).enumerate() {
                text(&self.fonts.mono, l, cr.x + 56.0, cr.y + 39.0 + k as f32 * 14.0, 11.5, p.dim);
            }
        }
        let y = self.heading("Difficulty", a.x, a.y);
        a.cut_top(y - a.y, 0.0);
        let names: Vec<&str> = DIFFS.iter().map(|d| d.0).collect();
        let _ = self.segmented(a.cut_top(38.0, 0.0), &names, (lob.diff as usize).min(DIFFS.len() - 1));

        // what the host is doing
        let line = if let Some(f) = recv {
            format!("Receiving the map  {:.0}%", f * 100.0)
        } else if busy {
            "The host is in the middle of a game. You will join the next one.".into()
        } else if !lob.line.is_empty() {
            format!("{} is {}", hn, lob.line)
        } else {
            format!("Waiting for {} to start", hn)
        };
        ui::spinner(vec2(state.x + 12.0, state.y + 14.0), 10.0, self.clock, p.accent);
        let line = fit(&self.fonts.mono, &line, state.w - 34.0, 13.0);
        text(&self.fonts.mono, &line, state.x + 34.0, state.y + 19.0, 13.0, p.text);
        if let Some(f) = recv {
            self.progress(Rect::new(state.x, state.y + 32.0, state.w, 4.0), f, p.accent);
        }
        if !self.status.is_empty() {
            let line = fit(&self.fonts.mono, &self.status, start.w, 13.0);
            text(&self.fonts.mono, &line, start.x, start.y - 6.0, 13.0, p.bad);
        }
        self.button_primary(start, "Waiting for the host", false);
        let fc = foot.cols(2, 10.0);
        if self.button(fc[0], "Leave lobby", true, false) {
            self.leave_coop();
            return;
        }
        if self.button(fc[1], "Settings", true, false) {
            self.open_settings();
        }
        if let Some(pr) = pr {
            self.players_panel(pr);
        }
    }

    /// Host: keeps the lobby on everybody's screen in step with what the host is doing.
    fn sync_lobby(&mut self) {
        let Some(s) = self.sess.as_ref() else { return };
        if !s.is_host() || s.in_game {
            return;
        }
        let camp = if self.camp_mode { self.camp.clone().or_else(campaign::load) } else { None };
        let (mode, fac, diff, city) = match &camp {
            _ if !self.camp_mode => (if self.real { net::MODE_REAL } else { net::MODE_RANDOM }, self.fac, self.diff, if self.real { self.city.trim().to_string() } else { String::new() }),
            Some(c) if c.fired() || c.finished() => (net::MODE_CAMPAIGN, c.fac, c.diff, "over, a new one starts in New York".to_string()),
            Some(c) => (net::MODE_CAMPAIGN, c.fac, c.diff, format!("day {:.0}, {} cities fought, next up {}", c.fought as f32 * campaign::DAYS_PER_CITY, c.fought, c.city().name)),
            None => (net::MODE_CAMPAIGN, self.fac, self.diff, "a new one, starting in New York".to_string()),
        };
        let line = match self.screen {
            Screen::Hub => match &self.camp {
                Some(c) if !c.finished() && !c.fired() => format!("on the globe, getting ready to defend {}", c.city().name),
                _ => "on the campaign globe".into(),
            },
            Screen::Loading => "loading the map".into(),
            Screen::Play if self.placing.is_some() => "choosing where the HQ goes".into(),
            _ => String::new(),
        };
        let l = net::Lobby { mode, fac: fac as u32, diff: diff as u32, city, line };
        self.sess.as_mut().unwrap().set_lobby(l);
    }

    // ------------------------------------------------------------------------------------- widgets

    fn set_music(&mut self, v: u32) {
        if self.music_vol > 0 {
            self.music_prev = self.music_vol;
        }
        self.music_vol = v;
        self.music.set_volume(v);
        self.save_settings();
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
    fn set_sfx(&mut self, v: u32) {
        self.sfx_vol = v;
        self.music.set_sfx_volume(v);
    }

    // ------------------------------------------------------------------------------------- menu

    fn menu(&mut self) {
        let th = self.theme();
        clear_background(th.bg);
        let p = pal(self.dark);
        // a random city behind the menu, its civilians hunted by all three armies (in pixels)
        ui::end();
        let (sw, sh) = (screen_width(), screen_height());
        if !self.bg.as_ref().map_or(false, |b| b.fits(sw, sh, th)) {
            self.bg = Some(MenuBg::new(sw, sh, th, macroquad::miniquad::date::now() as u64));
        }
        if let Some(b) = &mut self.bg {
            b.frame(get_frame_time().min(0.1), th);
        }
        ui::begin();
        if self.client() {
            while get_char_pressed().is_some() {}
            self.guest_lobby();
            return;
        }
        // In co-op this is the lobby: the same column, with the players beside it.
        let lobby = self.sess.is_some();
        let (r, pr) = self.menu_rects();
        self.panel(r);
        // a strip in the three faction colours along the top
        for (i, c) in Rect::new(r.x + R_PANEL, r.y, r.w - 2.0 * R_PANEL, 3.0).cols(3, 0.0).into_iter().enumerate() {
            let f = FACTIONS[i].col;
            draw_rectangle(c.x, c.y, c.w, c.h, Color::from_rgba(f[0], f[1], f[2], 255));
        }
        let mut a = r.pad(30.0, 26.0);
        let t = a.cut_top(48.0, 2.0);
        text(&self.fonts.head, if lobby { "CO-OP LOBBY" } else { "PIXEL HORDE" }, t.x, t.y + 40.0, 46.0, p.text);
        let t = a.cut_top(18.0, 18.0);
        let sub = if lobby { "Your game. Pick what the group plays, then start." } else { "Hold the city. Every pixel is an enemy." };
        text(&self.fonts.mono, sub, t.x, t.y + 13.0, 14.0, p.dim);

        // pinned to the bottom: settings row, Start, co-op
        let foot = a.cut_bottom(36.0, 14.0);
        let start = a.cut_bottom(56.0, 20.0);
        let coop = a.cut_bottom(38.0, 16.0);

        // game mode
        let y = self.heading("Game mode", a.x, a.y);
        a.cut_top(y - a.y, 0.0);
        let mode = if self.camp_mode { 2 } else if self.real { 1 } else { 0 };
        match self.segmented(a.cut_top(40.0, 12.0), &["Random city", "Real city", "World campaign"], mode) {
            Some(0) => {
                self.real = false;
                self.camp_mode = false;
            }
            Some(1) => {
                self.real = true;
                self.camp_mode = false;
            }
            Some(2) => self.camp_mode = true,
            _ => {}
        }
        let saved_camp = if self.camp_mode { campaign::load() } else { None };
        let detail_h = if self.camp_mode {
            if saved_camp.is_some() { 150.0 } else { 104.0 }
        } else if self.real {
            if self.saved.is_empty() { 74.0 } else { 136.0 }
        } else {
            40.0
        };
        let mut d = a.cut_top(detail_h, 18.0);
        if self.camp_mode {
            while get_char_pressed().is_some() {}
            let blurb = "Start in New York, then pick cities on the globe. Near-fallen cities have bigger hordes. Civilians flown out of held cities buy research and troops; survivors march on. A lost city pays nothing. Lose three and the world's governments fire you.";
            let lines = wrap_lines(&self.fonts.mono, blurb, d.w, 13.0);
            for (i, l) in lines.iter().enumerate() {
                text(&self.fonts.mono, l, d.x, d.y + 13.0 + i as f32 * 18.0, 13.0, p.dim);
            }
            d.cut_top(lines.len() as f32 * 18.0 + 8.0, 0.0);
            let ready = self.pack.ready();
            let (maps, f) = match self.pack.get() {
                campaign::PackState::Fetching(i) if ready < CITIES.len() => (format!("City maps {}/{} · getting {}…", ready, CITIES.len(), CITIES[i].name), ready as f32 / CITIES.len() as f32),
                campaign::PackState::Failed(_) if ready < CITIES.len() => (format!("City maps {}/{} · the rest download when you reach them", ready, CITIES.len()), ready as f32 / CITIES.len() as f32),
                _ => (format!("City maps: all {} ready", CITIES.len()), 1.0),
            };
            let m = d.cut_top(30.0, 6.0);
            text(&self.fonts.mono, &maps, m.x, m.y + 13.0, 12.5, p.text);
            self.progress(Rect::new(m.x, m.y + 21.0, m.w, 4.0), f, if f >= 1.0 { p.good } else { p.accent });
            if let Some(c) = &saved_camp {
                let line = if c.fired() {
                    format!("fired after {} lost cities ({})", c.lost, FACTIONS[c.fac].name)
                } else if c.finished() {
                    format!("over, {} cities held ({})", c.held(), FACTIONS[c.fac].name)
                } else {
                    format!(
                        "day {} · next {} · {} · {} · {} civilians to spend",
                        c.fought as f32 * campaign::DAYS_PER_CITY,
                        c.city().name,
                        FACTIONS[c.fac].name,
                        DIFFS[c.diff].0,
                        c.points
                    )
                };
                let sr = d.cut_top(42.0, 0.0);
                rrect(sr, R_CTRL, alpha(p.accent, 0.1));
                let mut sr2 = sr.pad(12.0, 6.0);
                let nr = sr2.cut_right(132.0, 10.0);
                text(&self.fonts.mono, "SAVED CAMPAIGN", sr2.x, sr2.y + 11.0, 10.5, p.accent);
                let line = fit(&self.fonts.mono, &line, sr2.w, 12.5);
                text(&self.fonts.mono, &line, sr2.x, sr2.y + 27.0, 12.5, p.text);
                if self.button(nr, "New campaign", true, false) {
                    self.open_campaign(Some(Campaign::new(self.fac, self.diff)));
                    return;
                }
                if self.hot(nr) {
                    self.tip = Some("Starts over in New York with the faction and difficulty picked here. The saved campaign is lost.".into());
                }
            }
        } else if self.real {
            let fr = d.cut_top(40.0, 8.0);
            rrect(fr, R_CTRL, p.panel2);
            rrect_lines(fr, R_CTRL, 1.5, alpha(p.accent, 0.8));
            let show = if self.city.is_empty() { "Type a city, e.g. Rotterdam or Denver, CO" } else { &self.city };
            let caret = if (self.clock * 2.0) as i32 % 2 == 0 && !self.city.is_empty() { "|" } else { "" };
            text(&self.fonts.mono, &format!("{}{}", show, caret), fr.x + 14.0, fr.y + 26.0, 16.0, if self.city.is_empty() { p.dim } else { p.text });
            self.ui_rects.push(fr);
            while let Some(ch) = get_char_pressed() {
                if !ch.is_control() && self.city.len() < 60 {
                    self.city.push(ch);
                }
            }
            if is_key_pressed(KeyCode::Backspace) {
                self.city.pop();
            }
            let n = d.cut_top(18.0, 10.0);
            text(&self.fonts.mono, "Streets and buildings come from OpenStreetMap. Your HQ goes at the nearest airport.", n.x, n.y + 12.0, 12.0, p.dim);
            // cities downloaded before: one click to play again, no download
            if !self.saved.is_empty() {
                let y = self.heading("Played before", d.x, d.y);
                let mut flow = Flow::new(Rect::new(d.x, y, d.w, d.bottom() - y), 6.0);
                let mut pick = None;
                let mut del = None;
                for (i, m) in self.saved.iter().take(12).enumerate() {
                    let bw = text_w(&self.fonts.head, &m.query, 14.0) + 22.0;
                    let Some(cr) = flow.next(bw + 24.0, 26.0) else { break };
                    let r = Rect::new(cr.x, cr.y, bw, cr.h);
                    let rx = Rect::new(cr.x + bw, cr.y, 24.0, cr.h);
                    let (hot, hx) = (self.hot(r), self.hot(rx));
                    rrect(cr, 13.0, if hot { mix(p.panel2, p.text, 0.1) } else { p.panel2 });
                    if hx {
                        rrect(rx, 13.0, p.bad);
                    }
                    text(&self.fonts.head, &m.query, r.x + 12.0, r.y + 18.0, 14.0, p.text);
                    text(&self.fonts.mono, "×", rx.x + 7.0, rx.y + 18.0, 14.0, if hx { WHITE } else { p.dim });
                    self.ui_rects.push(cr);
                    if hot {
                        self.tip = Some(format!("{}: play again without downloading", m.name));
                    }
                    if hx {
                        self.tip = Some("Delete this downloaded city".into());
                    }
                    if is_mouse_button_pressed(MouseButton::Left) && !self.blocked {
                        if hot {
                            pick = Some(i);
                        } else if hx {
                            del = Some(i);
                        }
                    }
                }
                if let Some(i) = del {
                    storage::delete_map(&self.saved[i].path);
                    self.saved = storage::list_maps();
                } else if let Some(i) = pick {
                    self.city = self.saved[i].query.clone();
                    self.start();
                    return;
                }
            }
        } else {
            while get_char_pressed().is_some() {}
            wrap(&self.fonts.mono, "A new random city every game: river, airport, avenues and downtown all move around.", d.x, d.y + 13.0, d.w, 13.0, p.dim);
        }

        // faction
        let y = self.heading("Faction", a.x, a.y);
        a.cut_top(y - a.y, 0.0);
        let rows = a.cut_top(3.0 * 60.0 + 2.0 * 8.0, 18.0).rows(3, 8.0);
        for (i, f) in FACTIONS.iter().enumerate() {
            let cr = rows[i];
            let active = self.fac == i;
            let hot = self.hot(cr);
            let c = Color::from_rgba(f.col[0], f.col[1], f.col[2], 255);
            rrect(cr, R_CTRL, if active { mix(p.panel2, c, 0.16) } else if hot { mix(p.panel2, p.text, 0.06) } else { p.panel2 });
            rrect_lines(cr, R_CTRL, if active { 2.0 } else { 1.0 }, if active { c } else { p.line });
            // a little swarm of the faction's pixels
            for k in 0..16 {
                let q = vec2(cr.x + 14.0 + (k * 37 % 27) as f32, cr.y + 16.0 + (k * 13 % 29) as f32);
                draw_rectangle(q.x, q.y, 3.0, 3.0, c);
            }
            text(&self.fonts.head, f.name, cr.x + 56.0, cr.y + 22.0, 17.0, p.text);
            let lines = wrap_lines(&self.fonts.mono, f.blurb, cr.w - 70.0, 11.5);
            for (k, l) in lines.iter().take(2).enumerate() {
                text(&self.fonts.mono, l, cr.x + 56.0, cr.y + 39.0 + k as f32 * 14.0, 11.5, p.dim);
            }
            if hot && !self.blocked && is_mouse_button_pressed(MouseButton::Left) {
                self.fac = i;
            }
            self.ui_rects.push(cr);
        }

        // difficulty
        let y = self.heading("Difficulty", a.x, a.y);
        a.cut_top(y - a.y, 0.0);
        let names: Vec<&str> = DIFFS.iter().map(|d| d.0).collect();
        if let Some(i) = self.segmented(a.cut_top(38.0, 0.0), &names, self.diff) {
            self.diff = i;
        }

        // co-op with Steam friends
        if lobby {
            let c = coop.cols(2, 10.0);
            if self.button(c[0], "Invite Steam friends", true, true) {
                self.open_invites();
            }
            if self.button(c[1], "Leave lobby", true, false) {
                self.leave_coop();
                return;
            }
        } else {
            let have = self.steam.is_some();
            if self.button(coop, "Co-op lobby with Steam friends", have, false) {
                if let Some(st) = &self.steam {
                    st.create();
                    self.status = "Opening a Steam lobby".into();
                }
            }
            if self.hot(coop) {
                self.tip = Some(if have {
                    "Opens a co-op lobby and the Steam invite window. Friends who accept join you there; together you play any city or the world campaign.".into()
                } else {
                    "Steam is not running. Start Steam, then restart Pixel Horde to play co-op.".into()
                });
            }
        }
        if !self.status.is_empty() {
            let line = fit(&self.fonts.mono, &self.status, start.w, 13.0);
            text(&self.fonts.mono, &line, start.x, start.y - 6.0, 13.0, p.bad);
        }
        let can = self.camp_mode || !self.real || self.city.trim().len() > 1;
        let label = match (self.camp_mode, saved_camp.is_some(), lobby) {
            (true, true, _) => "Continue campaign",
            (true, false, true) => "Start campaign",
            _ => "Start",
        };
        if self.button_primary(start, label, can) || (can && !self.blocked && is_key_pressed(KeyCode::Enter)) {
            self.start();
            return;
        }
        if self.camp_mode && lobby && self.hot(start) {
            self.tip = Some("Opens your campaign for everybody. Spend civilians and pick the city on the globe, then all of you defend it together.".into());
        }
        if let Some(pr) = pr {
            self.players_panel(pr);
        }
        let fc = foot.cols(2, 10.0);
        if self.button(fc[0], "Settings", true, false) {
            self.open_settings();
        }
        if self.button(fc[1], if self.dark { "Light map" } else { "Dark map" }, true, false) {
            self.dark = !self.dark;
            self.save_settings();
        }
        // updater line, under the panel
        let st = self.updater.get();
        text(&self.fonts.mono, &self.updater.label(), r.x + 6.0, r.bottom() + 22.0, 12.5, p.dim);
        if let UpState::Ready(_) = st {
            if self.button_primary(Rect::new(r.right() - 150.0, r.bottom() + 6.0, 150.0, 28.0), "Restart now", true) {
                Updater::restart();
            }
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
            self.begin(d, c.seed.wrapping_mul(31).wrapping_add(c.fought as u64 * 16 + c.current as u64));
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

    /// Between cities: the globe to pick the next city on, unlocks, recruits and the way in.
    fn hub(&mut self) {
        let p = pal(self.dark);
        let (sw, sh) = (ui::sw(), ui::sh());
        let Some(mut c) = self.camp.clone() else {
            self.screen = Screen::Menu;
            return;
        };
        let before = (c.points, c.tech.len(), c.units.len(), c.current);
        // space
        clear_background(rgb(6, 9, 16));
        for k in 0..220u32 {
            let h = |s: u32| {
                let mut x = k.wrapping_mul(0x9E37_79B9) ^ s.wrapping_mul(0x85EB_CA6B);
                x ^= x >> 15;
                x = x.wrapping_mul(0x2C1B_3C6D);
                x ^= x >> 12;
                x = x.wrapping_mul(0x297A_2D39);
                x ^= x >> 15;
                (x >> 8) as f32 / (1u32 << 24) as f32
            };
            let tw = 0.35 + 0.35 * ((self.clock * (0.5 + h(3))) + h(4) * 9.0).sin();
            draw_circle(h(1) * sw, h(2) * sh, 0.6 + h(5) * 1.1, Color::new(0.8, 0.86, 1.0, tw * 0.7));
        }

        // two columns at the sides, the globe between them
        // inside the safe area, so on ultrawide screens the columns stay near the globe
        let hud = ui::hud();
        let colw = (hud.w * 0.25).clamp(300.0, 430.0);
        let pad = 16.0;
        let left = Rect::new(hud.x + pad, pad, colw, sh - 2.0 * pad);
        let right = Rect::new(hud.x + hud.w - pad - colw, pad, colw, sh - 2.0 * pad);
        let gc = vec2(sw / 2.0, sh / 2.0 + 14.0);
        let gr = ((hud.w - 2.0 * colw - 4.0 * pad) / 2.0).min(sh / 2.0 - 64.0).max(120.0);
        let m = self.mouse();
        let globe = self.globe.get_or_insert_with(globe::Globe::new);
        if self.globe_city != Some(c.current) {
            self.globe_city = Some(c.current);
            globe.focus(c.city().lat, c.city().lon);
        }
        let marks: Vec<globe::Mark> = CITIES
            .iter()
            .enumerate()
            .map(|(i, city)| {
                let s = &c.cities[i];
                let (col, label) = match s.fate {
                    Fate::Open => (mix(rgb(255, 196, 64), rgb(255, 64, 48), s.threat()), format!("{} · {:.0}d", city.name, s.days)),
                    Fate::Held => (rgb(64, 192, 87), format!("{} · held", city.name)),
                    Fate::Lost => (rgb(150, 70, 70), format!("{} · lost", city.name)),
                    Fate::Fallen => (rgb(110, 24, 24), format!("{} · fallen", city.name)),
                };
                globe::Mark { lat: city.lat, lon: city.lon, col, label, picked: i == c.current && !c.finished() }
            })
            .collect();
        let over_globe = !left.contains(m) && !right.contains(m);
        let (hover, clicked) = globe.draw(gc, gr, &marks, &self.fonts.head, over_globe, get_frame_time());
        if let Some(i) = clicked {
            c.select(i);
        }
        if let Some(i) = hover {
            self.tip = Some(self.city_tip(&c, i));
        }
        let title = "WORLD CAMPAIGN";
        text(&self.fonts.head, title, sw / 2.0 - text_w(&self.fonts.head, title, 28.0) / 2.0, 44.0, 28.0, p.text);
        let sub = format!("Day {} · {} cities held · {} lost of {} allowed", c.fought as f32 * campaign::DAYS_PER_CITY, c.held(), c.lost, campaign::MAX_LOST);
        text(&self.fonts.mono, &sub, sw / 2.0 - text_w(&self.fonts.mono, &sub, 13.0) / 2.0, 66.0, 13.0, p.dim);
        let hint = "Drag to turn the globe, scroll to zoom, click a city to pick it";
        text(&self.fonts.mono, hint, sw / 2.0 - text_w(&self.fonts.mono, hint, 12.0) / 2.0, sh - 22.0, 12.0, p.dim);

        // ---- left: the campaign and the picked city
        self.panel(left);
        let (x0, w) = (left.x + 20.0, left.w - 40.0);
        let mut y = left.y + 34.0;
        text(&self.fonts.head, FACTIONS[c.fac].name, x0, y, 20.0, p.text);
        y += 20.0;
        text(&self.fonts.mono, &format!("{} · {} flown out · {} kills", DIFFS[c.diff].0, c.flown, fmt_n(c.kills as usize)), x0, y, 12.0, p.dim);
        y += 18.0;
        // lost cities: three strikes
        text(&self.fonts.mono, "Cities lost", x0, y + 4.0, 12.0, p.dim);
        for k in 0..campaign::MAX_LOST {
            let on = k < c.lost;
            let r = Rect::new(x0 + 96.0 + k as f32 * 26.0, y - 8.0, 20.0, 16.0);
            draw_rectangle(r.x, r.y, r.w, r.h, if on { rgb(224, 49, 49) } else { p.panel2 });
        }
        y += 26.0;
        if c.finished() {
            self.camp_end(&c, x0, y, w);
        } else {
            let i = c.current;
            let city = c.city();
            let s = *c.state();
            let card = Rect::new(left.x + 10.0, y, left.w - 20.0, 196.0);
            draw_rectangle(card.x, card.y, card.w, card.h, p.panel2);
            draw_rectangle(card.x, card.y, 4.0, card.h, mix(rgb(255, 196, 64), rgb(255, 64, 48), s.threat()));
            let mut cy = card.y + 34.0;
            text(&self.fonts.head, city.name, x0, cy, 26.0, p.text);
            let ct = city.country();
            text(&self.fonts.mono, ct, card.x + card.w - 12.0 - text_w(&self.fonts.mono, ct, 12.0), cy - 4.0, 12.0, p.dim);
            cy += 26.0;
            text(&self.fonts.head, &format!("{:.0} days until it falls", s.days), x0, cy, 17.0, mix(rgb(255, 196, 64), rgb(255, 90, 70), s.threat()));
            cy += 22.0;
            text(&self.fonts.mono, &format!("{:.1}M people alive of {:.1}M", c.alive(i), city.pop), x0, cy, 12.5, p.text);
            cy += 18.0;
            // threat bar
            let bw = w;
            draw_rectangle(x0, cy, bw, 8.0, rgba(255, 255, 255, 0.08));
            draw_rectangle(x0, cy, bw * s.threat().max(0.03), 8.0, mix(rgb(255, 196, 64), rgb(255, 64, 48), s.threat()));
            cy += 26.0;
            text(&self.fonts.mono, &format!("Horde ×{:.2} of {}", c.horde_mult(), DIFFS[c.diff].0), x0, cy, 12.5, p.text);
            cy += 18.0;
            text(&self.fonts.mono, &format!("Hold out {} for the evac", mmss(c.hold_secs())), x0, cy, 12.5, p.text);
            cy += 18.0;
            let map = if campaign::have_map(city) { "map ready" } else { "map downloads first" };
            text(&self.fonts.mono, &format!("{} civilians on the map · {}", fmt_n(c.civilians_at(i)), map), x0, cy, 12.5, p.dim);
            y = card.y + card.h + 18.0;
            // every city, closest to falling first
            text(&self.fonts.head, "Cities", x0, y, 16.0, p.text);
            y += 8.0;
            let mut order: Vec<usize> = (0..CITIES.len()).collect();
            order.sort_by(|a, b| {
                let k = |i: usize| (c.cities[i].fate != Fate::Open, c.cities[i].days);
                let (ka, kb) = (k(*a), k(*b));
                ka.0.cmp(&kb.0).then(ka.1.total_cmp(&kb.1))
            });
            let by = left.y + left.h - 120.0;
            let rh = ((by - y - 8.0) / CITIES.len() as f32).clamp(14.0, 26.0);
            for j in order {
                let st = &c.cities[j];
                let rr = Rect::new(left.x + 10.0, y, left.w - 20.0, rh - 2.0);
                let hot = self.hot(rr) && st.fate == Fate::Open;
                if j == c.current || hot {
                    draw_rectangle(rr.x, rr.y, rr.w, rr.h, if j == c.current { mix(p.panel2, p.accent, 0.25) } else { p.panel2 });
                }
                let (col, right_s) = match st.fate {
                    Fate::Open => (p.text, format!("{:.0} days · {:.1}M", st.days, c.alive(j))),
                    Fate::Held => (rgb(64, 192, 87), "held".to_string()),
                    Fate::Lost => (rgb(200, 90, 90), "lost".to_string()),
                    Fate::Fallen => (p.dim, "fallen".to_string()),
                };
                let ty = rr.y + rh / 2.0 + 4.0;
                text(&self.fonts.mono, CITIES[j].name, rr.x + 10.0, ty, 12.5, col);
                text(&self.fonts.mono, &right_s, rr.x + rr.w - 10.0 - text_w(&self.fonts.mono, &right_s, 12.0), ty, 12.0, if st.fate == Fate::Open { p.dim } else { col });
                if hot {
                    self.tip = Some(self.city_tip(&c, j));
                    if is_mouse_button_pressed(MouseButton::Left) {
                        c.select(j);
                    }
                }
                y += rh;
            }
            if !self.status.is_empty() {
                wrap(&self.fonts.mono, &self.status, x0, by + 6.0, w, 12.0, rgb(250, 82, 82));
            }
            let go = self.button(Rect::new(x0, by + 30.0, w, 46.0), &format!("Defend {}", city.name), true, true) || is_key_pressed(KeyCode::Enter);
            if go {
                campaign::save(&c);
                self.camp = Some(c);
                self.start_city();
                return;
            }
        }
        if self.button(Rect::new(x0, left.y + left.h - 38.0, 130.0, 30.0), if self.sess.is_some() { "Lobby" } else { "Main menu" }, true, false) {
            campaign::save(&c);
            self.camp = Some(c);
            self.screen = Screen::Menu;
            return;
        }

        // ---- right: the tech tree and the army
        self.panel(right);
        let (x0, w) = (right.x + 20.0, right.w - 40.0);
        let mut y = right.y + 34.0;
        text(&self.fonts.head, "Tech tree", x0, y, 20.0, p.text);
        let pts = format!("{} RP", c.points);
        text(&self.fonts.head, &pts, x0 + w - text_w(&self.fonts.head, &pts, 22.0), y, 22.0, p.accent);
        y += 18.0;
        text(&self.fonts.mono, "Every civilian flown out is a research point.", x0, y, 11.5, p.dim);
        y += 12.0;
        const GROUPS: [&str; 5] = ["Turrets", "Support", "Abilities", "Research", "Turret upgrades"];
        const SHORT: [&str; 5] = ["Turrets", "Support", "Abilities", "Research", "Upgrades"];
        if let Some(k) = self.segmented(Rect::new(right.x + 10.0, y, right.w - 20.0, 30.0), &SHORT, self.tech_tab) {
            self.tech_tab = k;
        }
        y += 40.0;
        let list: Vec<campaign::Tech> = campaign::TECH.into_iter().filter(|t| t.group() == GROUPS[self.tech_tab.min(4)]).collect();
        let army_h = 150.0;
        let rh = ((right.y + right.h - army_h - y) / list.len().max(6) as f32).clamp(30.0, 50.0);
        for t in list {
            let rr = Rect::new(right.x + 10.0, y, right.w - 20.0, rh - 4.0);
            draw_rectangle(rr.x, rr.y, rr.w, rr.h, p.panel2);
            let fs = if rh < 38.0 { 12.5 } else { 14.0 };
            text(&self.fonts.head, t.name(), rr.x + 10.0, rr.y + rr.h / 2.0 + fs * 0.35, fs, if c.has_tech(t) { p.text } else { p.dim });
            let br = Rect::new(rr.x + rr.w - 112.0, rr.y + (rr.h - 26.0).max(0.0) / 2.0, 104.0, rr.h.min(26.0));
            let need = t.needs().filter(|n| !c.has_tech(*n));
            if c.has_tech(t) {
                self.button(br, "Unlocked", false, true);
            } else if let Some(n) = need {
                self.button(br, "Locked", false, false);
                if self.hot(rr) {
                    self.tip = Some(format!("{}: {} Needs {} first.", t.name(), t.tip(), n.name()));
                }
            } else if self.button(br, &format!("{} RP", t.cost()), c.can_buy(t) && !c.finished(), false) {
                c.buy(t);
            }
            if self.hot(rr) && need.is_none() {
                self.tip = Some(format!("{}: {}", t.name(), t.tip()));
            }
            y += rh;
        }
        // the army, at the bottom
        let mut y = right.y + right.h - army_h + 14.0;
        text(&self.fonts.head, "Your army", x0, y, 16.0, p.text);
        y += 10.0;
        let (rs, rn) = c.army(U::Rifle);
        let (ts, _) = c.army(U::Tank);
        let ar = Rect::new(right.x + 10.0, y, right.w - 20.0, 52.0);
        draw_rectangle(ar.x, ar.y, ar.w, ar.h, p.panel2);
        text(&self.fonts.mono, &format!("{} rifle squads ({} soldiers) · {} tanks", rs, rn, ts), x0, ar.y + 22.0, 12.5, p.text);
        text(&self.fonts.mono, "Survivors march on to the next city.", x0, ar.y + 40.0, 11.0, p.dim);
        y += ar.h + 8.0;
        let half = (right.w - 28.0) / 2.0;
        for (k, u) in [U::Rifle, U::Tank].into_iter().enumerate() {
            let cost = campaign::recruit_cost(u);
            let br = Rect::new(right.x + 10.0 + k as f32 * (half + 8.0), y, half, 34.0);
            let open = u == U::Rifle || c.has_tech(campaign::Tech::Unit(u));
            let label = if open { format!("+{} · {} RP", udef(u).name.to_lowercase(), cost) } else { format!("{}: locked", udef(u).name) };
            if self.button(br, &label, open && c.points >= cost && !c.finished(), false) {
                c.recruit(u);
            }
        }

        if (c.points, c.tech.len(), c.units.len(), c.current) != before {
            campaign::save(&c);
        }
        if c.current != before.3 {
            self.status.clear();
        }
        self.camp = Some(c);
    }

    /// What the globe's pins and the city list show on hover.
    fn city_tip(&self, c: &Campaign, i: usize) -> String {
        let city = &CITIES[i];
        let s = &c.cities[i];
        match s.fate {
            Fate::Open => format!(
                "{}, {}: {:.0} days until it falls, {:.1} million alive. Horde ×{:.2}, {} civilians on the map.",
                city.name,
                city.country(),
                s.days,
                c.alive(i),
                c.horde_mult_at(i),
                fmt_n(c.civilians_at(i))
            ),
            Fate::Held => format!("{}, {}: held until the evac.", city.name, city.country()),
            Fate::Lost => format!("{}, {}: lost. The HQ was overrun.", city.name, city.country()),
            Fate::Fallen => format!("{}, {}: fell to the horde before anyone came.", city.name, city.country()),
        }
    }

    /// The end of a campaign, in the left column: fired, or no city left to defend.
    fn camp_end(&mut self, c: &Campaign, x0: f32, y: f32, w: f32) {
        let p = pal(self.dark);
        let (head, body) = if c.fired() {
            (
                "You're fired.",
                format!(
                    "{} cities fell on your watch, and the world's governments have handed the war to someone else. You held {} and flew {} civilians out.",
                    campaign::MAX_LOST,
                    c.held(),
                    c.flown
                ),
            )
        } else {
            ("No cities left to defend.", format!("You held {} of the world's biggest cities and flew {} civilians out. The rest are the horde's.", c.held(), c.flown))
        };
        text(&self.fonts.head, head, x0, y + 30.0, 24.0, p.text);
        wrap(&self.fonts.mono, &body, x0, y + 60.0, w, 13.0, p.dim);
        if self.button(Rect::new(x0, y + 150.0, w, 46.0), "New campaign", true, true) {
            self.open_campaign(Some(Campaign::new(c.fac, c.diff)));
        }
    }

    /// Headless test hook: a campaign at city `level` with some troops and unlocks, on its screen or
    /// already playing a random city.
    pub fn test_campaign(&mut self, city: usize, play: bool) {
        let mut c = Campaign::with_seed(std::env::var("PH_FAC").ok().and_then(|s| s.parse().ok()).unwrap_or(0), 1, 42);
        c.current = city.min(CITIES.len() - 1);
        c.fought = 3;
        c.lost = 1;
        c.cities[0].fate = Fate::Held;
        c.cities[2].fate = Fate::Lost;
        c.cities[7].fate = Fate::Fallen;
        c.cities[7].days = 0.0;
        c.cities[c.current].fate = Fate::Open;
        c.points = 260;
        c.flown = 640;
        c.kills = 182_000;
        c.tech = vec![campaign::Tech::Build(S::Gun), campaign::Tech::Ability(A::Rescue), campaign::Tech::Research(R::Rescue), campaign::Tech::Research(R::Armour)];
        c.units = vec![(U::Rifle, 6), (U::Rifle, 4), (U::Tank, 1), (U::Rifle, 6)];
        self.camp = Some(c);
        self.screen = Screen::Hub;
        if play {
            self.in_camp = true;
            let d = campaign::load_map(&CITIES[city.min(CITIES.len() - 1)]).unwrap_or_else(|| crate::citygen::generate(42));
            self.begin(d, 1000 + city as u64);
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
        storage::save_settings(&storage::Settings {
            dark: self.dark,
            fac: self.fac,
            diff: self.diff,
            real: self.real,
            city: self.city.clone(),
            campaign: self.camp_mode,
            music: self.music_vol,
            sfx: self.sfx_vol,
            ui_scale: ui::user_scale(),
            hud_wide: ui::wide(),
            fullscreen: self.fullscreen,
            fps: self.show_fps,
            aa: self.aa,
            render: self.render_pct,
        });
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
            Some(c) => Game::new(d, c.fac, DIFFS[c.diff].1 * c.horde_mult(), seed),
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
        let aa = self.aa;
        let painter = *self.depth.get_or_insert_with(|| pick_depth(aa)) == render::DepthMode::Painter;
        self.scene = Some(Scene::new(&g, self.theme(), painter));
        self.game = Some(g);
        self.tool = Tool::None;
        self.selected = None;
        self.sel_struct = None;
        self.paused = false;
        self.acc = 0.0;
        self.screen = Screen::Play;
    }

    /// Starts the match on `d`. The co-op host sends the map (with the HQ where it was put) first.
    fn launch(&mut self, mut d: Desc, seed: u64) {
        let chosen = self.placing.take().is_some();
        if let Some(s) = self.sess.as_mut() {
            if s.is_host() {
                let camp = if self.in_camp { self.camp.as_ref().map(campaign::to_text) } else { None };
                d = s.start_game(&d, self.fac, self.diff, seed, camp.as_deref());
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
        self.banner = camp.map(|c| {
            let city = c.city();
            let line = format!("Day {:.0} · {:.0} days until it falls · hold out {} until the evac", c.fought as f32 * campaign::DAYS_PER_CITY, c.state().days, mmss(c.hold_secs()));
            (city.name.to_string(), city.country().to_string(), line, self.clock)
        });
        self.msg = match camp {
            // the banner names the city and its days; the line under it says where the HQ went
            Some(_) => (note, 6.0),
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
        let area = ui::hud();
        let r = Rect::new(area.x + (area.w - 760.0) / 2.0, 14.0, 760.0, 112.0);
        self.panel(r);
        let mut a = r.pad(24.0, 18.0);
        let mut b = a.cut_right(150.0, 20.0);
        if self.button(b.cut_top(34.0, 8.0), "Back", true, false) {
            self.placing = None;
            self.screen = if self.in_camp { Screen::Hub } else { Screen::Menu };
            self.in_camp = false;
        }
        if self.button_primary(b.cut_top(36.0, 0.0), "Start here", true) || (!self.blocked && is_key_pressed(KeyCode::Enter)) {
            self.place_go = true;
        }
        text(&self.fonts.head, "Choose your HQ location", a.x, a.y + 24.0, 24.0, p.text);
        let line = match &self.sess {
            Some(_) => "Click the map to move the HQ. Your co-op team starts there too.",
            None => "Click the map to move the HQ. The horde comes once you start.",
        };
        text(&self.fonts.mono, line, a.x, a.y + 50.0, 13.0, p.dim);
        text(&self.fonts.mono, "Drag to look around, wheel to zoom, F for the whole map.", a.x, a.y + 70.0, 13.0, p.dim);
        self.msg.1 -= get_frame_time();
        if self.msg.1 > 0.0 && !self.msg.0.is_empty() {
            let m = self.msg.0.clone();
            self.toast(r.bottom() + 12.0, &m, p.bad, 1.0);
        }
    }

    /// A message in a pill, centred in the HUD area with its top at `y`.
    fn toast(&self, y: f32, m: &str, c: Color, a: f32) {
        let p = pal(self.dark);
        let area = ui::hud();
        let tw = text_w(&self.fonts.head, m, 17.0);
        let r = Rect::new(area.x + (area.w - tw) / 2.0 - 20.0, y, tw + 40.0, 36.0);
        ui::shadow(r, 18.0, alpha(p.shadow, p.shadow.a * a));
        rrect(r, 18.0, alpha(Color::new(0.06, 0.07, 0.09, 1.0), 0.86 * a));
        draw_circle(r.x + 16.0, r.y + 18.0, 3.5, alpha(c, a));
        text(&self.fonts.head, m, r.x + 26.0, r.y + 24.0, 17.0, Color::new(1.0, 1.0, 1.0, a));
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
        let r = ui::hud().centered(560.0, 176.0);
        self.panel(r);
        let what = match (&self.camp, self.in_camp) {
            (Some(c), true) => c.city().name.to_string(),
            _ => self.city.trim().to_string(),
        };
        let mut a = r.pad(28.0, 24.0);
        let t = a.cut_top(30.0, 10.0);
        text(&self.fonts.head, &format!("Loading {}", what), t.x, t.y + 24.0, 24.0, p.text);
        ui::spinner(vec2(t.right() - 16.0, t.y + 14.0), 14.0, self.clock, p.accent);
        let l = a.cut_top(20.0, 0.0);
        let st = fit(&self.fonts.mono, &self.status, l.w, 14.0);
        text(&self.fonts.mono, &st, l.x, l.y + 14.0, 14.0, p.dim);
        if self.button(Rect::new(a.x, a.bottom() - 38.0, 130.0, 38.0), "Cancel", true, false) {
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
        if std::mem::take(&mut self.regfx) {
            let aa = self.aa;
            let painter = *self.depth.get_or_insert_with(|| pick_depth(aa)) == render::DepthMode::Painter;
            sc = Scene::new(&g, th, painter);
        }
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
        if let Some(s) = self.sel_struct {
            if s >= g.structs.len() || g.structs[s].dead {
                self.sel_struct = None;
            }
        }

        // camera
        let (sw, sh) = (screen_width(), screen_height());
        // the 3D view can be drawn at a lower resolution than the window (Settings > Graphics)
        let k = self.render_pct as f32 / 100.0;
        let (rw, rh) = ((sw * k).max(1.0) as u32, (sh * k).max(1.0) as u32);
        let aa = self.aa;
        let depth = *self.depth.get_or_insert_with(|| pick_depth(aa));
        match depth {
            render::DepthMode::Target(sample_count) => {
                if self.rt.as_ref().map(|r| (r.texture.width() as u32, r.texture.height() as u32)) != Some((rw, rh)) {
                    let rt = render_target_ex(rw, rh, RenderTargetParams { sample_count, depth: true });
                    rt.texture.set_filter(if self.render_pct < 100 { FilterMode::Linear } else { FilterMode::Nearest });
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
                sel_struct: self.sel_struct,
                time: self.clock,
                eye: cam.position.truncate(),
            },
            &mut labels,
        );
        let mw = self.pick(&g, self.px_mouse());
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

        // the HUD is in points
        ui::begin();
        self.blocked = self.settings_open;
        if self.placing.is_some() {
            self.place_hud();
        } else {
            self.hud(&mut g);
            self.city_banner();
        }
        if let (Some((s, c)), false) = (cursor_label, self.settings_open || self.over_ui()) {
            let m = self.mouse();
            let tw = text_w(&self.fonts.mono, &s, 14.0);
            let r = Rect::new(m.x + 18.0, m.y + 16.0, tw + 20.0, 26.0);
            rrect(r, 6.0, Color::new(0.04, 0.05, 0.07, 0.82));
            text(&self.fonts.mono, &s, r.x + 10.0, r.y + 18.0, 14.0, c);
        }
        self.overlays();
        ui::end();
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

    /// "DEFENDING TOKYO": a band across the top of the view when a campaign city starts. It opens
    /// out from the middle, holds a few seconds, then fades.
    fn city_banner(&mut self) {
        let Some((city, country, line, t0)) = self.banner.clone() else { return };
        let t = self.clock - t0;
        const LEN: f32 = 6.5;
        if t > LEN {
            self.banner = None;
            return;
        }
        let ease = |x: f32| 1.0 - (1.0 - x.clamp(0.0, 1.0)).powi(3);
        let a = ease(t / 0.45).min(1.0 - (t - (LEN - 1.0)).clamp(0.0, 1.0));
        let open = ease(t / 0.6);
        let p = pal(self.dark);
        let area = ui::hud();
        let amber = p.accent;
        let (cy, bh) = ((area.y + area.h * 0.24).max(area.y + 160.0), 136.0);
        let mid = area.x + area.w / 2.0;
        let bw = area.w * (0.3 + 0.7 * open);
        let x0 = mid - bw / 2.0;
        // the band, soft at both ends
        let fade = 160.0f32.min(bw / 4.0);
        let ink = Color::new(0.03, 0.04, 0.06, 0.8 * a);
        draw_rectangle(x0 + fade, cy - bh / 2.0, bw - 2.0 * fade, bh, ink);
        ui::hgrad(Rect::new(x0, cy - bh / 2.0, fade, bh), alpha(ink, 0.0), ink);
        ui::hgrad(Rect::new(x0 + bw - fade, cy - bh / 2.0, fade, bh), ink, alpha(ink, 0.0));
        // amber rules that run out from the centre, with a soft glow
        let rl = bw * 0.36 * open;
        for y in [cy - bh / 2.0 + 10.0, cy + bh / 2.0 - 10.0] {
            draw_line(mid - rl, y, mid + rl, y, 5.0, alpha(amber, 0.12 * a));
            draw_line(mid - rl, y, mid + rl, y, 1.5, alpha(amber, 0.9 * a));
            draw_rectangle(mid - rl - 3.0, y - 2.5, 5.0, 5.0, alpha(amber, a));
            draw_rectangle(mid + rl - 2.0, y - 2.5, 5.0, 5.0, alpha(amber, a));
        }
        // DEFENDING, letter spaced
        let tag = "D E F E N D I N G";
        let tw = text_w(&self.fonts.mono, tag, 14.0);
        text(&self.fonts.mono, tag, mid - tw / 2.0, cy - 30.0, 14.0, alpha(amber, a));
        let name = city.to_uppercase();
        let ns = 56.0;
        let nw = text_w(&self.fonts.head, &name, ns);
        let nx = mid - nw / 2.0 + (1.0 - ease(t / 0.7)) * 26.0;
        text(&self.fonts.head, &name, nx + 2.0, cy + 22.0, ns, Color::new(0.0, 0.0, 0.0, 0.5 * a));
        text(&self.fonts.head, &name, nx, cy + 20.0, ns, Color::new(1.0, 1.0, 1.0, a));
        let sub = if country.is_empty() { line } else { format!("{} · {}", country, line) };
        let lw = text_w(&self.fonts.mono, &sub, 14.0);
        text(&self.fonts.mono, &sub, mid - lw / 2.0, cy + 46.0, 14.0, Color::new(0.85, 0.87, 0.9, 0.9 * a));
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
                if d.turret && !d.ground && self.press.is_none() {
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
                if spots.len() == 1 {
                    let c = vec2(
                        spots[0].0 as f32 + d.w as f32 / 2.0,
                        spots[0].1 as f32 + d.h as f32 / 2.0,
                    );
                    if d.turret && d.range > 0.0 {
                        draw_circle_lines(c.x, c.y, g.turret_range(t, false), 0.4, rgba(255, 212, 59, 0.6));
                    }
                    if t == S::Rescue {
                        draw_circle_lines(c.x, c.y, RESCUE_REACH, 0.8, rgba(64, 192, 87, 0.7));
                    }
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
            Tool::None => {
                let i = self.sel_struct?;
                let s = &g.structs[i];
                match s.t {
                    S::Artillery => {
                        let r = def(S::Artillery).rad + 3.0 + mw.distance(s.center()) * 0.012;
                        draw_circle_lines(mw.x, mw.y, r, 0.6, rgba(255, 90, 70, 0.9));
                        Some(("Right-click to shell this spot".into(), WHITE))
                    }
                    S::Helipad => {
                        draw_circle_lines(mw.x, mw.y, 25.0 + 34.0, 0.6, rgba(255, 212, 59, 0.8));
                        Some(("Right-click to send the gunship here".into(), WHITE))
                    }
                    _ => None,
                }
            }
        }
    }

    /// The in-game HUD. Everything sits inside `ui::hud()`: the top bar along its top, the build
    /// bar along its bottom, abilities down its right side.
    fn hud(&mut self, g: &mut Game) {
        let p = pal(self.dark);
        let area = ui::hud();
        let scr = ui::screen();

        // ---- top bar: faction and stats on the left, controls on the right
        let top = Rect::new(area.x + 12.0, 10.0, area.w - 24.0, 52.0);
        self.panel(top);
        let mut row = top.pad(16.0, 0.0);
        let bs = row.cut_right(5.0 * 90.0 + 4.0 * 6.0, 16.0);
        let b = Rect::new(bs.x, top.y + 10.0, bs.w, 32.0).cols(5, 6.0);
        // stress test: drops a million more hostiles each press (F9 does the same)
        if !g.over && self.button(b[0], "Spawn 1M", true, false) {
            self.act(g, Cmd::Stress(STRESS_N));
        }
        if self.hot(b[0]) {
            self.tip = Some("Testing: one million more hostiles pour in from eight spots on the map [F9]".into());
        }
        if self.button(b[1], "Settings", true, false) {
            self.open_settings();
        }
        if self.button(b[2], if self.cam.top() { "3D view" } else { "Top view" }, true, false) {
            self.toggle_top();
        }
        if self.hot(b[2]) {
            self.tip = Some("Switch between the tilted 3D view and straight down [T]".into());
        }
        let paused = self.paused_now();
        if self.button(b[3], if paused { "Resume" } else { "Pause" }, true, paused) {
            self.toggle_pause();
        }
        if self.button(b[4], "Menu", true, false) {
            self.leave_game();
            return;
        }
        let f = g.faction();
        let fc = Color::from_rgba(f.col[0], f.col[1], f.col[2], 255);
        let fw = text_w(&self.fonts.head, f.name, 17.0) + 26.0;
        let fr = row.cut_left(fw, 14.0);
        rrect(Rect::new(fr.x, top.y + 20.0, 12.0, 12.0), 3.0, fc);
        text(&self.fonts.head, f.name, fr.x + 22.0, top.y + 32.0, 17.0, p.text);
        draw_rectangle(row.x - 8.0, top.y + 12.0, 1.0, top.h - 24.0, p.line);
        // stats, most important first; what doesn't fit is left out
        let evac = g.goal > 0.0;
        let mut stats: Vec<(&str, String, Color, Option<&str>)> = vec![
            ("SCRAP", format!("{:.0}", g.scrap), p.accent, None),
            ("AMMO", format!("{:.0}", g.ammo), if g.ammo < 20.0 { p.bad } else { p.text }, None),
            ("HOSTILES", fmt_n(g.hostiles()), p.text, None),
            ("CIVILIANS", fmt_n(g.alive_civs()), p.text, None),
        ];
        if evac {
            stats.push(("EVAC IN", mmss((g.goal - g.time).max(0.0)), p.warn, Some("Hold out until the evac flies everyone out")));
        }
        if g.income_mult < 0.999 {
            stats.push(("INCOME", format!("×{:.2}", g.income_mult), p.bad, Some("Too many rescued civilians are waiting in the base. Build more evac points to fly them out faster.")));
        }
        stats.extend([
            ("RESCUED", format!("{}", g.rescued), p.text, None),
            ("WAITING", format!("{}", g.waiting), p.text, None),
            ("FLOWN OUT", format!("{}", g.extracted), p.text, None),
            ("SCORE", format!("{:.0}", g.score), p.text, None),
        ]);
        let mut x = row.x;
        for (k, v, c, tip) in stats {
            let w = (text_w(&self.fonts.head, &v, 18.0).max(text_w(&self.fonts.mono, k, 10.0)) + 22.0).max(70.0);
            if x + w > row.right() + 22.0 {
                break;
            }
            text(&self.fonts.mono, k, x, top.y + 20.0, 10.0, p.dim);
            text(&self.fonts.head, &v, x, top.y + 40.0, 18.0, c);
            if let (Some(t), true) = (tip, self.hot(Rect::new(x, top.y, w, top.h))) {
                self.tip = Some(t.into());
            }
            x += w;
        }
        if self.show_fps {
            let s = format!("{:.0} fps · sim {:.1} ms", get_fps(), g.sim_ms);
            let tw = text_w(&self.fonts.mono, &s, 11.0);
            text(&self.fonts.mono, &s, scr.right() - tw - 10.0, scr.bottom() - 8.0, 11.0, alpha(p.dim, 0.8));
        }
        if let Some(s) = &self.sess {
            let names: Vec<String> = s.players.iter().map(|p| p.name.clone()).collect();
            text(&self.fonts.mono, &format!("Co-op: {}", names.join(", ")), top.x + 4.0, top.bottom() + 20.0, 12.0, p.dim);
        }

        // ---- build bar, centred along the bottom; wraps to two rows when the HUD is too narrow
        let items: Vec<Tool> = BUILD_ORDER
            .iter()
            .filter(|s| !(g.campaign && **s == S::Lab) && g.build_ok(**s))
            .map(|s| Tool::Build(*s))
            .chain([U::Rifle, U::Tank].into_iter().filter(|u| g.unit_ok(*u)).map(Tool::Unit))
            .collect();
        let (gap, ch) = (6.0, 60.0);
        let room = area.w - 24.0 - 16.0;
        let mut bw = 100.0f32;
        let n = items.len().max(1);
        if n as f32 * (bw + gap) - gap > room {
            bw = ((room + gap) / n as f32 - gap).max(98.0);
        }
        // as many rows as it takes, with the items shared evenly between them
        let nrows = n.div_ceil((((room + gap) / (bw + gap)).floor() as usize).clamp(1, n));
        let per_row = n.div_ceil(nrows);
        let bar_w = per_row as f32 * (bw + gap) - gap + 16.0;
        let bar_h = nrows as f32 * (ch + gap) - gap + 16.0;
        let bar = Rect::new(area.x + (area.w - bar_w) / 2.0, area.bottom() - bar_h - 12.0, bar_w, bar_h);
        self.panel(bar);
        self.bar_h = area.bottom() - bar.y;
        for (i, it) in items.iter().enumerate() {
            let (ri, ci) = (i / per_row, i % per_row);
            // the last row is centred
            let in_row = if ri + 1 == nrows { n - ri * per_row } else { per_row };
            let off = (per_row - in_row) as f32 * (bw + gap) / 2.0;
            let r = Rect::new(bar.x + 8.0 + off + ci as f32 * (bw + gap), bar.y + 8.0 + ri as f32 * (ch + gap), bw, ch);
            let (key, name, cost, col, tip, ok) = match it {
                Tool::Build(s) => {
                    let d = def(*s);
                    let c = if *s == S::Extractor { g.cost_at(*s, g.world.hq) } else { d.cost };
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
            rrect(r, R_CTRL, if active { mix(p.panel2, p.accent, 0.28) } else if hot { mix(p.panel2, p.text, 0.08) } else { p.panel2 });
            if active {
                rrect_lines(r, R_CTRL, 2.0, p.accent);
            }
            rrect(Rect::new(r.x + 7.0, r.y + 9.0, 4.0, r.h - 18.0), 2.0, col);
            self.keycap(r.right() - 21.0, r.y + 5.0, key);
            let nm = match it {
                Tool::Build(S::Gun) => "Gun turret",
                Tool::Build(S::Extractor) => "Extractor",
                Tool::Build(S::Plant) => "Ammo plant",
                Tool::Build(S::Silo) => "Missile silo",
                Tool::Build(S::Pad) => "Evac point",
                Tool::Build(S::Lab) => "Research",
                Tool::Build(S::Rescue) => "Rescue stn",
                Tool::Build(S::Flame) => "Flame",
                Tool::Build(S::Helium) => "Helium",
                Tool::Unit(U::Rifle) => "Rifles",
                _ => name,
            };
            let nm = fit(&self.fonts.head, nm, r.w - 20.0, 14.0);
            text(&self.fonts.head, &nm, r.x + 17.0, r.y + 33.0, 14.0, if ok { p.text } else { p.dim });
            text(&self.fonts.mono, &format!("${:.0}", cost), r.x + 17.0, r.y + 51.0, 12.5, if ok { p.accent } else { p.bad });
            self.ui_rects.push(r);
            if hot {
                self.tip = Some(format!("{} [{}]: {}", name, key.to_ascii_uppercase(), tip));
                if is_mouse_button_pressed(MouseButton::Left) {
                    self.tool = if active { Tool::None } else { *it };
                }
            }
        }

        // ---- abilities and research down the right side
        let aw = 248.0;
        let ax = area.right() - aw - 12.0;
        let mut ay = top.bottom() + 12.0;
        // cards get shorter when the screen is short
        let lab = !g.campaign && g.has(S::Lab);
        let lab_h = if lab { 44.0 + LAB_ONLY.len() as f32 * 30.0 } else { 0.0 };
        let room = bar.y - 12.0 - ay - lab_h;
        // campaign: only what the tech tree has unlocked
        let abilities: Vec<A> = ABILITIES.into_iter().filter(|a| g.ability_ok(*a)).collect();
        let card_h = ((room - 8.0 * (abilities.len().max(1) as f32 - 1.0)) / abilities.len().max(1) as f32).clamp(64.0, 88.0);
        for a in abilities {
            let d = adef(a);
            let r = Rect::new(ax, ay, aw, card_h);
            self.panel(r);
            let usable = g.can_use(a);
            let active = self.tool == Tool::Ability(a);
            let head = Rect::new(r.x, r.y, r.w, card_h - 30.0);
            let hot = self.hot(head);
            if active {
                rrect_lines(r, R_PANEL, 2.0, p.accent);
            } else if hot && usable {
                rrect(head.inset(3.0), R_CTRL, alpha(p.text, 0.05));
            }
            let fc = if usable { p.text } else { p.dim };
            let name = format!("{}{}", d.name, if g.upgraded(a) && a != A::Gunship { " +" } else { "" });
            text(&self.fonts.head, &name, r.x + 14.0, r.y + 23.0, 16.0, fc);
            self.keycap(r.right() - 26.0, r.y + 9.0, d.key);
            let mut line = format!("${:.0}", d.cost);
            if d.ammo > 0.0 {
                line.push_str(&format!(" + {:.0} ammo", d.ammo));
            }
            if a == A::Icbm {
                let (rdy, tot) = g.silos_ready();
                line = if tot == 0 { "Needs a missile silo".into() } else { format!("Silos ready {}/{}", rdy, tot) };
            }
            text(&self.fonts.mono, &line, r.x + 14.0, r.y + 41.0, 12.0, p.dim);
            let cd = g.cooldown(a);
            if cd > 0.0 {
                let pr = Rect::new(r.x + 14.0, r.y + 47.0, r.w - 28.0, 3.0);
                self.progress(pr, 1.0 - cd / d.cd, p.accent);
                let s = format!("{:.0}s", cd);
                let tw = text_w(&self.fonts.mono, &s, 12.0);
                text(&self.fonts.mono, &s, r.right() - 14.0 - tw, r.y + 41.0, 12.0, p.accent);
            }
            if hot {
                self.tip = Some(d.tip.to_string());
                if is_mouse_button_pressed(MouseButton::Left) && usable {
                    self.tool = if active { Tool::None } else { Tool::Ability(a) };
                }
            }
            let ur = Rect::new(r.x + 8.0, r.bottom() - 30.0, r.w - 16.0, 24.0);
            if let Some(rs) = ability_research(a) {
                self.research_button(g, rs, ur, "Upgrade");
            } else {
                text(&self.fonts.mono, "Hovers and fires for 20 s", ur.x + 8.0, ur.y + 16.0, 11.5, p.dim);
            }
            ay += card_h + 8.0;
        }
        // lab research that isn't tied to an ability
        if lab {
            let r = Rect::new(ax, ay, aw, lab_h - 8.0);
            self.panel(r);
            text(&self.fonts.head, "Research lab", r.x + 14.0, r.y + 22.0, 15.0, p.text);
            for (i, rs) in LAB_ONLY.iter().enumerate() {
                let ur = Rect::new(r.x + 8.0, r.y + 30.0 + i as f32 * 30.0, r.w - 16.0, 24.0);
                self.research_button(g, *rs, ur, "Research");
            }
        }

        // ---- selection, bottom left above the build bar
        if let Some(gi) = self.selected {
            let gr = &g.groups[gi];
            let n = gr.m.iter().filter(|m| m.alive).count();
            let s = format!(
                "{} · {} alive{} · right-click to move{}",
                udef(gr.kind).name,
                n,
                if gr.state == GState::Garrison { " · garrisoned" } else { "" },
                if gr.kind == U::Rifle { " (click a building to garrison)" } else { "" }
            );
            let r = Rect::new(area.x + 12.0, bar.y - 46.0, text_w(&self.fonts.mono, &s, 13.0) + 32.0, 34.0);
            self.panel(r);
            draw_circle(r.x + 14.0, r.y + 17.0, 3.5, udef(gr.kind).col);
            text(&self.fonts.mono, &s, r.x + 24.0, r.y + 22.0, 13.0, p.text);
        }

        if let Some(si) = self.sel_struct {
            let st = &g.structs[si];
            let (line, cease) = match st.t {
                S::Artillery => (
                    if st.target.is_some() {
                        format!("Artillery · firing · {:.0} ammo a shell · right-click to retarget", def(S::Artillery).ammo)
                    } else {
                        "Artillery · right-click anywhere on the map to open fire".to_string()
                    },
                    st.target.is_some(),
                ),
                _ => (
                    match g.pad_gunship(si) {
                        Some(gs) if gs.phase < 2 => format!("Heli pad · gunship out, {:.0} s left · right-click to move it", gs.t.max(0.0)),
                        Some(_) => "Heli pad · gunship returning".to_string(),
                        None if st.cd > 0.0 => format!("Heli pad · rearming {:.0} s", st.cd),
                        None => "Heli pad · gunship ready · right-click to send it".to_string(),
                    },
                    false,
                ),
            };
            let tw = text_w(&self.fonts.mono, &line, 13.0) + 24.0;
            let r = Rect::new(area.x + 12.0, bar.y - 46.0, tw + if cease { 110.0 } else { 0.0 }, 34.0);
            self.panel(r);
            text(&self.fonts.mono, &line, r.x + 12.0, r.y + 22.0, 13.0, p.text);
            if cease && self.button(Rect::new(r.x + tw, r.y + 4.0, 100.0, 26.0), "Cease fire", true, false) {
                self.act(g, Cmd::Cease(si as u32));
            }
        }

        // ---- banners, under the top bar
        let (msg, t) = if g.banner.1 > 0.0 { (g.banner.0.clone(), g.banner.1) } else { (self.msg.0.clone(), self.msg.1) };
        self.msg.1 -= get_frame_time();
        if t > 0.0 && !msg.is_empty() {
            self.toast(top.bottom() + 14.0, &msg, p.accent, t.min(1.0));
        }
        if self.paused_now() && !g.over && !self.settings_open {
            let r = area.centered(260.0, 76.0);
            ui::shadow(r, R_PANEL, p.shadow);
            rrect(r, R_PANEL, Color::new(0.04, 0.05, 0.07, 0.8));
            self.label_in(Rect::new(r.x, r.y + 8.0, r.w, 36.0), "PAUSED", 30.0, WHITE);
            self.label_in(Rect::new(r.x, r.y + 44.0, r.w, 20.0), "Space or P to resume", 12.0, rgb(200, 205, 214));
        }
        if g.over && self.in_camp {
            self.campaign_over(g);
        } else if g.over {
            self.backdrop();
            let r = area.centered(480.0, 210.0);
            self.panel(r);
            let mut a = r.pad(28.0, 26.0);
            let t = a.cut_top(32.0, 12.0);
            text(&self.fonts.head, "The HQ has fallen", t.x, t.y + 28.0, 30.0, p.text);
            let l = a.cut_top(18.0, 0.0);
            text(
                &self.fonts.mono,
                &format!("Score {:.0} · {} kills · {} flown out · {:.0} min", g.score, fmt_n(g.kills as usize), g.extracted, g.time / 60.0),
                l.x,
                l.y + 13.0,
                13.0,
                p.dim,
            );
            if self.button_primary(Rect::new(a.x, a.bottom() - 44.0, 200.0, 44.0), "Back to menu", true) {
                self.leave_game();
            }
        }
    }

    /// The end of a campaign city: held (bank civilians, carry the army on) or lost (try again).
    fn campaign_over(&mut self, g: &mut Game) {
        let p = pal(self.dark);
        let Some(c) = self.camp.clone() else { return };
        self.backdrop();
        let r = ui::hud().centered(560.0, 240.0);
        self.panel(r);
        let mut a = r.pad(28.0, 26.0);
        let t = a.cut_top(32.0, 14.0);
        let mut b = a.cut_bottom(44.0, 0.0);
        let squads = g.groups.iter().filter(|gr| !gr.dead).count();
        // what the campaign looks like after this city
        let mut next = c.clone();
        next.harvest(g);
        let army = if squads == 0 {
            "No troops survived.".to_string()
        } else {
            format!("{} squad{} march on.", squads, if squads == 1 { "" } else { "s" })
        };
        if g.won {
            text(&self.fonts.head, &format!("{} held", c.city().name), t.x, t.y + 28.0, 30.0, p.text);
            let l = a.cut_top(22.0, 4.0);
            text(&self.fonts.mono, &format!("{} civilians flown out: +{} to spend", g.extracted, g.extracted), l.x, l.y + 14.0, 14.0, p.accent);
        } else {
            text(&self.fonts.head, &format!("{} is lost", c.city().name), t.x, t.y + 28.0, 30.0, p.text);
            let left = campaign::MAX_LOST.saturating_sub(next.lost);
            let line = match left {
                0 => "That's three cities. The world's governments are letting you go.".to_string(),
                1 => "The HQ was overrun. Nothing to spend from here. One more lost city and you're fired.".to_string(),
                n => format!("The HQ was overrun. Nothing to spend from here. {} more lost cities and you're fired.", n),
            };
            let lines = wrap_lines(&self.fonts.mono, &line, a.w, 13.0);
            let l = a.cut_top(lines.len() as f32 * 17.0 + 4.0, 4.0);
            for (i, s) in lines.iter().enumerate() {
                text(&self.fonts.mono, s, l.x, l.y + 13.0 + i as f32 * 17.0, 13.0, p.bad);
            }
        }
        let l = a.cut_top(22.0, 2.0);
        text(&self.fonts.mono, &army, l.x, l.y + 14.0, 14.0, p.text);
        let fell: Vec<&str> = (0..CITIES.len()).filter(|i| c.cities[*i].fate == Fate::Open && next.cities[*i].fate == Fate::Fallen).map(|i| CITIES[i].name).collect();
        let world = if fell.is_empty() {
            format!("{} days pass. {} kills · score {:.0}", campaign::DAYS_PER_CITY, fmt_n(g.kills as usize), g.score)
        } else {
            format!("{} days pass. Fallen meanwhile: {}.", campaign::DAYS_PER_CITY, fell.join(", "))
        };
        let l = a.cut_top(20.0, 0.0);
        let world = fit(&self.fonts.mono, &world, l.w, 13.0);
        text(&self.fonts.mono, &world, l.x, l.y + 13.0, 13.0, p.dim);
        if self.client() {
            // the host carries on for everybody
            let hn = self.sess.as_ref().map(|s| s.host_name()).unwrap_or_default();
            ui::spinner(vec2(b.x + 12.0, b.y + 22.0), 10.0, self.clock, p.accent);
            text(&self.fonts.mono, &format!("Waiting for {} to carry on", hn), b.x + 34.0, b.y + 27.0, 13.0, p.text);
            if self.button(b.cut_right(150.0, 0.0), "Leave co-op", true, false) {
                self.leave_game();
            }
        } else if self.button_primary(b.cut_left(200.0, 0.0), "Continue", true) {
            campaign::save(&next);
            self.camp = Some(next);
            self.in_camp = false;
            self.end_round();
            self.screen = Screen::Hub;
        }
    }

    /// Co-op host: the city is over, so everybody goes back to the lobby.
    fn end_round(&mut self) {
        if let Some(s) = self.sess.as_mut() {
            if s.is_host() && s.in_game {
                s.end_game();
            }
        }
    }

    /// One research row: shows its state and starts it on click.
    fn research_button(&mut self, g: &mut Game, rs: R, ur: Rect, verb: &str) {
        let p = pal(self.dark);
        let d = rdef(rs);
        let mut prog = None;
        let (label, en) = if g.research.has(rs) {
            (format!("Done: {}", d.name), false)
        } else if g.campaign {
            ("Unlock between cities".to_string(), false)
        } else if let Some((busy, t)) = g.research.busy {
            if busy == rs {
                prog = Some(t);
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
        let done = g.research.has(rs);
        rrect(ur, 6.0, if hot && en { mix(p.panel2, p.accent, 0.22) } else { p.panel2 });
        if en {
            rrect_lines(ur, 6.0, 1.0, alpha(p.accent, 0.5));
        }
        if prog.is_some() {
            let k = 0.5 + 0.5 * (self.clock * 4.0).sin();
            rrect(Rect::new(ur.x, ur.bottom() - 3.0, ur.w, 3.0), 1.5, alpha(p.accent, 0.3 + 0.4 * k));
        }
        let label = fit(&self.fonts.mono, &label, ur.w - 16.0, 11.5);
        text(&self.fonts.mono, &label, ur.x + 8.0, ur.y + 16.0, 11.5, if en { p.accent } else if done { p.good } else { p.dim });
        if hot {
            self.tip = Some(format!("{}: {}", d.name, d.tip));
            if en && is_mouse_button_pressed(MouseButton::Left) {
                self.act(g, Cmd::Research(rs));
            }
        }
    }

    fn input(&mut self, g: &mut Game, sc: &mut Scene, mw: Vec2, dt: f32) {
        if self.settings_open {
            while get_char_pressed().is_some() {}
            return;
        }
        let ui = self.over_ui();
        // keyboard
        while let Some(ch) = get_char_pressed() {
            let ch = ch.to_ascii_lowercase();
            if let Some(s) = BUILD_ORDER.iter().find(|s| def(**s).key == ch && !(g.campaign && **s == S::Lab) && g.build_ok(**s) && self.placing.is_none()) {
                self.tool = if self.tool == Tool::Build(*s) {
                    Tool::None
                } else {
                    Tool::Build(*s)
                };
            } else if self.placing.is_some() && !"tkmfh".contains(ch) {
            } else if let Some(u) = [U::Rifle, U::Tank].iter().find(|u| udef(**u).key == ch && g.unit_ok(**u)) {
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
            self.sel_struct = None;
            self.press = None;
        }
        if is_key_pressed(KeyCode::F9) && self.placing.is_none() {
            self.act(g, Cmd::Stress(STRESS_N));
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
            let before = self.ground_at(self.px_mouse());
            let k = if wy > 0.0 { 1.0 / 1.15 } else { 1.15 };
            self.cam.dist = (self.cam.dist * k).clamp(20.0, 2600.0);
            let after = self.ground_at(self.px_mouse());
            self.cam.t += before - after;
        }
        let md = self.px_mouse() - self.last_m;
        let last = self.last_m;
        self.last_m = self.px_mouse();
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
            } else if let Some(si) = self.sel_struct {
                self.act(g, Cmd::Aim(si as u32, mw));
            } else if let Some(gi) = self.selected {
                self.act(g, Cmd::Order(gi as u32, mw));
            }
        }
        if is_mouse_button_pressed(MouseButton::Left) && !ui {
            self.press = Some((self.px_mouse(), mw));
            self.panning = false;
        }
        if is_mouse_button_down(MouseButton::Left) {
            if let Some((s0, _)) = self.press {
                if self.tool == Tool::None && (self.px_mouse() - s0).length() > 6.0 {
                    self.panning = true;
                }
                if self.panning && !alt {
                    // grab the ground: the point under the cursor stays under the cursor
                    self.cam.t += self.ground_at(last) - self.ground_at(self.px_mouse());
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
                    self.sel_struct = None;
                    if self.selected.is_none() {
                        self.sel_struct = g
                            .struct_at(mw)
                            .filter(|i| matches!(g.structs[*i].t, S::Artillery | S::Helipad));
                    }
                }
                Tool::Build(t) => {
                    let d = def(t);
                    if d.turret && !d.ground && w0.distance(mw) < 2.0 {
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

/// Finds a way of drawing the 3D view whose depth test works on this GPU (see `render::check_depth`)
/// and notes the result in data/gpu.txt. `PH_DEPTH=4|1|window` forces one.
fn pick_depth(aa: u8) -> render::DepthMode {
    use render::DepthMode::*;
    let forced = match std::env::var("PH_DEPTH").ok().as_deref() {
        Some("window" | "painter") => Some(Painter),
        Some(n) => n.parse().ok().map(Target),
        None => None,
    };
    if let Some(m) = forced {
        return m;
    }
    if aa == 2 {
        return Painter;
    }
    let tries: &[i32] = if aa == 1 || (std::env::var("PH_SHOT").is_ok() && std::env::var("PH_MSAA").is_err()) { &[1] } else { &[4, 1] };
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
/// `s` cut short with an ellipsis to fit in `w`.
fn fit(f: &Font, s: &str, w: f32, size: f32) -> String {
    if text_w(f, s, size) <= w {
        return s.to_string();
    }
    let mut t: String = s.to_string();
    while !t.is_empty() && text_w(f, &format!("{}…", t), size) > w {
        t.pop();
    }
    format!("{}…", t.trim_end())
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
            // PH_NETCAMP=<city>: a co-op campaign city instead
            if let Some(l) = std::env::var("PH_NETCAMP").ok().and_then(|v| v.parse::<usize>().ok()) {
                self.test_campaign(l, true);
            } else {
                self.start();
            }
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
        // a catching-up guest runs several ticks a frame, so log the first frame past each 100
        if exec / 100 > self.test_log / 100 {
            self.test_log = exec;
            eprintln!("{} tick {} checksum {:016x} structs {} hostiles {}{}", if host { "host" } else { "guest" }, exec, g.checksum(), g.structs.iter().filter(|s| !s.dead).count(), g.hostiles(), if s.desync { " DESYNC" } else { "" });
        }
        exec >= until
    }
}

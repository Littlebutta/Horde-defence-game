//! Screens, HUD and input: the start menu, the city loader, and the game itself.

use crate::campaign::{self, Campaign, City, Fate, POOL};
use crate::globe;
use crate::defs::*;
use crate::game::*;
use crate::net::{self, Cmd};
use crate::osm::{self, OsmMsg};
use crate::menubg::MenuBg;
use crate::music;
use crate::news::{self, News};
use crate::storage;
use crate::render::{self, text, text_w, Fonts, Scene, ViewInfo, WorldLabels};
use crate::ui::{self, alpha, mix, pal, rrect, rrect_lines, Flow, RectExt};
use crate::update::{UpState, Updater};
use crate::world::*;
use macroquad::prelude::*;

mod invite;
mod settings;
mod techtree;
mod war;
mod widgets;
mod workshop;
use war::{army_col, campaign_army_name, level_col, turns_s};
use widgets::{R_CTRL, R_PANEL};

#[derive(Clone, Copy, PartialEq)]
enum Tool {
    None,
    Build(S),
    Unit(U),
    Ability(A),
    /// the k-th deployed garage design (F1..F6)
    Custom(u8),
}

/// Something done on the campaign screen. In co-op, picking the city and going in are votes, and a
/// majority of the players makes the host carry them out; buying tech and recruiting are each
/// player's own, paid from their own research points.
#[derive(Clone, Copy, PartialEq)]
enum HubAct {
    Tech(campaign::Tech),
    Recruit(U),
    City(usize),
    Go,
    /// the next step of research into an army (shared by everybody, paid from its kills)
    Hunt(usize),
    /// research a garage part with research points
    ResearchPart(u16),
    /// buy a researched garage part with money
    BuyPart(u16),
}
impl HubAct {
    fn code(self) -> u32 {
        match self {
            HubAct::Tech(t) => campaign::TECH.iter().position(|x| *x == t).unwrap_or(0) as u32,
            HubAct::Recruit(u) => 1000 + (u == U::Tank) as u32,
            HubAct::City(i) => 2000 + i as u32,
            HubAct::Go => 3000,
            HubAct::Hunt(a) => 4000 + a as u32,
            HubAct::ResearchPart(i) => 5000 + i as u32,
            HubAct::BuyPart(i) => 6000 + i as u32,
        }
    }
    fn from_code(v: u32) -> Option<HubAct> {
        Some(match v {
            0..=999 => HubAct::Tech(*campaign::TECH.get(v as usize)?),
            1000 => HubAct::Recruit(U::Rifle),
            1001 => HubAct::Recruit(U::Tank),
            2000..=2999 => HubAct::City((v - 2000) as usize),
            3000 => HubAct::Go,
            4000..=4002 => HubAct::Hunt((v - 4000) as usize),
            5000..=5999 if ((v - 5000) as usize) < crate::garage::PARTS.len() => HubAct::ResearchPart((v - 5000) as u16),
            6000..=6999 if ((v - 6000) as usize) < crate::garage::PARTS.len() => HubAct::BuyPart((v - 6000) as u16),
            _ => return None,
        })
    }
    /// A player has one vote of each kind at a time: a city, ready to go. (Kind 0, buying, is no vote.)
    fn kind(self) -> u8 {
        match self {
            HubAct::Tech(_) | HubAct::Recruit(_) | HubAct::Hunt(_) | HubAct::ResearchPart(_) | HubAct::BuyPart(_) => 0,
            HubAct::City(_) => 1,
            HubAct::Go => 2,
        }
    }
}

#[derive(PartialEq, Debug)]
enum Screen {
    Menu,
    Loading,
    Play,
    /// the campaign between cities: unlocks, recruits and the next city
    Hub,
    /// the turret workshop (free builder from the menu, or the campaign's garage)
    Garage,
}

/// Pixel `k` of faction `i`'s little swarm on its menu card: chaos mixes all three colours.
fn swatch(i: usize, k: usize, c: Color) -> Color {
    if i == CHAOS {
        let f = FACTIONS[k % 3].col;
        Color::from_rgba(f[0], f[1], f[2], 255)
    } else {
        c
    }
}

/// Height of a faction card on the menu.
const FAC_ROW: f32 = 56.0;
const FAC_ROW_COMPACT: f32 = 46.0;

pub(crate) const DIFFS: [(&str, f32); 4] = [
    ("Easy", 0.6),
    ("Normal", 1.0),
    ("Hard", 1.6),
    ("Insane", 2.6),
];

pub struct App {
    screen: Screen,
    /// the garage while it's open
    workshop: Option<Box<workshop::Workshop>>,
    /// saved turret builds (data/turrets.txt)
    library: crate::garage::Library,
    fonts: Fonts,
    dark: bool,
    music: music::Music,
    /// music volume 0..=100, saved in settings
    music_vol: u32,
    /// effects volume 0..=100, saved in settings
    sfx_vol: u32,
    /// news broadcast volume 0..=100, saved in settings
    news_vol: u32,
    /// a match left running behind the menu (alone only): Resume or Abandon picks it up again
    suspended: bool,
    /// Esc this frame is free to open the menu (nothing modal took it)
    esc_free: bool,
    /// the volume to go back to when M unmutes
    music_prev: u32,
    fac: usize,
    diff: usize,
    real: bool,
    city: String,
    /// "World campaign" picked on the menu
    camp_mode: bool,
    /// asking before a new campaign erases the saved one
    confirm_new: bool,
    /// the turn the campaign screen's alien trails started drawing for, and when
    trail_t: Option<(u32, f32)>,
    /// bulletins acknowledged here (a co-op guest can't mark them in the host's campaign)
    told_local: u8,
    /// the last bulletin whose newscaster line was played: (campaign seed, bulletin bit)
    bulletin_played: Option<(u64, u8)>,
    camp: Option<Campaign>,
    /// the game being played (or loaded) is a campaign city
    in_camp: bool,
    pack: campaign::Pack,
    /// the campaign's globe, made the first time it's shown
    globe: Option<globe::Globe>,
    /// the city the globe last turned to
    globe_city: Option<usize>,
    /// the campaign's tech tree is open over the campaign screen
    tree_open: bool,
    /// which page of the tech tree is showing: 0 the main tree, then one per kind of garage part
    tree_tab: usize,
    /// co-op campaign screen, on the host: who voted for what, and for how many players
    votes: Vec<(usize, HubAct)>,
    vote_n: usize,
    /// a guest's copy of the host's campaign screen, as last parsed
    hub_text: String,
    /// headless co-op campaign screen test (`PH_NETHUB`): how far it got
    hub_test: u32,
    /// loading screen is waiting for the city pack instead of OpenStreetMap
    wait_pack: bool,
    status: String,
    loader: Option<osm::Loader>,
    game: Option<Game>,
    /// runs single-player ticks while the last frame is shown
    ticker: Ticker,
    /// ticks due to run once this frame is drawn, and ticks run since the horde was last redrawn
    due: u32,
    fresh_ticks: u32,
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
    /// co-op guest choosing: where they put their HQ, and whether they've sent it to the host
    my_spot: Option<Vec2>,
    spot_sent: bool,
    /// co-op: the map and seed of the game being played, to rebuild it once every HQ is placed
    coop_map: Option<(Desc, u64)>,
    /// set during a frame: move the HQ here / start the match
    place_at: Option<Vec2>,
    place_go: bool,
    /// headless tests start right away with the suggested HQ (`PH_PLACE=1` shows the choice)
    auto_place: bool,
    /// the "Defending <city>" banner at the start of a campaign city: (city, country, line, start)
    banner: Option<(String, String, String, f32)>,
    /// this match's news broadcasts and headline ticker
    news: Option<News>,
    /// the main menu's own headline ticker
    menu_news: News,
    /// the mouse is held inside the window (while a match is being played)
    grabbed: bool,
    /// the slider being dragged
    drag: Option<u32>,
    /// B-2 bomb run: where the right button went down
    rpress: Option<Vec2>,
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
/// Runs single-player simulation ticks on their own thread, between one frame being drawn and the
/// next starting, so a tick overlaps with the GPU drawing the frame and waiting for the screen
/// instead of landing on top of every third frame (which showed as a hitch). The frame takes the
/// game back first thing, waiting if the ticks are not done yet.
struct Ticker {
    tx: std::sync::mpsc::Sender<(Box<Game>, u32)>,
    rx: std::sync::mpsc::Receiver<(Box<Game>, u32)>,
    busy: bool,
}

impl Ticker {
    fn new() -> Self {
        let (tx, jobs) = std::sync::mpsc::channel::<(Box<Game>, u32)>();
        let (done, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("ticker".into())
            .spawn(move || {
                while let Ok((mut g, n)) = jobs.recv() {
                    for _ in 0..n {
                        g.tick();
                    }
                    if done.send((g, n)).is_err() {
                        break;
                    }
                }
            })
            .expect("ticker thread");
        Ticker { tx, rx, busy: false }
    }
    fn start(&mut self, g: Game, n: u32) {
        self.busy = true;
        self.tx.send((Box::new(g), n)).expect("ticker thread");
    }
    fn join(&mut self) -> Option<(Game, u32)> {
        if !self.busy {
            return None;
        }
        self.busy = false;
        let (g, n) = self.rx.recv().expect("ticker thread");
        Some((*g, n))
    }
}

/// Hostiles the "Spawn 5M" stress-test button (and F9) drops in.
const STRESS_N: u32 = 5_000_000;
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
        // closing the window comes to App::frame, which quits at once (see App::quit)
        prevent_quit();
        ui::set_user_scale(st.ui_scale);
        ui::set_wide(st.hud_wide);
        App {
            workshop: None,
            library: crate::garage::Library::load(),
            screen: Screen::Menu,
            fonts: render::load_fonts(),
            // the light map was removed: always dark
            dark: true,
            fac: st.fac,
            diff: st.diff,
            real: st.real,
            city: st.city,
            camp_mode: st.campaign,
            confirm_new: std::env::var("PH_CONFIRM").is_ok(),
            trail_t: None,
            told_local: 0,
            bulletin_played: None,
            music: music::Music::start(st.music, st.sfx, st.news),
            sfx_vol: st.sfx,
            news_vol: st.news,
            suspended: false,
            esc_free: false,
            music_vol: st.music,
            music_prev: if st.music > 0 { st.music } else { 70 },
            camp: None,
            in_camp: false,
            pack: campaign::Pack::start(),
            globe: None,
            globe_city: None,
            tree_open: false,
            tree_tab: 0,
            votes: Vec::new(),
            vote_n: 0,
            hub_text: String::new(),
            hub_test: 0,
            wait_pack: false,
            saved: storage::list_maps(),
            status: String::new(),
            loader: None,
            game: None,
            ticker: Ticker::new(),
            due: 0,
            fresh_ticks: 0,
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
            my_spot: None,
            spot_sent: false,
            coop_map: None,
            place_at: None,
            place_go: false,
            banner: None,
            news: None,
            menu_news: {
                let mut n = News::menu(macroquad::miniquad::date::now().to_bits());
                if let Some(x) = std::env::var("PH_NEWS").ok().and_then(|v| v.parse().ok()) {
                    n.preview(x);
                    n.since_news = f32::MAX;
                }
                n
            },
            grabbed: false,
            auto_place: ["PH_SHOT", "PH_NETTEST"].iter().any(|k| std::env::var(k).is_ok()) && std::env::var("PH_PLACE").is_err(),
            drag: None,
            rpress: None,
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
        &DARK
    }

    pub fn frame(&mut self) {
        self.join_ticks();
        let dt = get_frame_time().min(0.1);
        self.clock += dt;
        self.ui_rects.clear();
        self.tip = None;
        self.net_frame();
        self.music.ambience(self.screen == Screen::Play);
        self.sync_lobby();
        self.sync_hub();
        if is_key_pressed(KeyCode::F11) {
            self.set_fullscreen(!self.fullscreen);
        }
        // Alt+F4 and the window's close button quit at once, whatever is on screen
        let alt = is_key_down(KeyCode::LeftAlt) || is_key_down(KeyCode::RightAlt);
        if (alt && is_key_pressed(KeyCode::F4)) || is_quit_requested() {
            self.quit();
        }
        if self.screen == Screen::Play {
            // the game draws its 3D view in pixels and switches to points for the HUD itself
            self.play(dt);
        } else {
            ui::begin();
            self.blocked = self.settings_open || self.invite_open || self.confirm_new;
            match self.screen {
                Screen::Menu => self.menu(),
                Screen::Loading => self.loading(),
                Screen::Hub => self.hub(),
                Screen::Garage => self.workshop(),
                Screen::Play => {}
            }
            self.overlays();
            ui::end();
        }
        self.grab_mouse();
        // our own pointer, over everything (the system one is hidden)
        show_mouse(false);
        set_default_camera();
        gl_use_default_material();
        render::cursor(self.px_mouse());
    }

    /// While a match is being played the mouse can't leave the window; pausing (or the settings,
    /// the end of the match, any other screen) lets it go.
    fn grab_mouse(&mut self) {
        let over = self.game.as_ref().is_some_and(|g| g.over);
        let want = self.screen == Screen::Play && !self.paused_now() && !self.settings_open && !over && std::env::var("PH_SHOT").is_err();
        if want != self.grabbed {
            self.grabbed = want;
            set_cursor_grab(want);
        }
    }

    /// The click of a button being pressed.
    pub(super) fn click(&self) {
        self.music.play(music::Play { snd: music::Snd::Button, gain: 0.55, pan: 0.0, speed: 1.0, key: 0 });
    }

    /// What goes over any screen, in points: the settings dialog and the tooltip.
    fn overlays(&mut self) {
        self.blocked = false;
        if self.invite_open && self.screen != Screen::Play && !self.settings_open {
            self.tip = None;
            self.invites();
        }
        if self.confirm_new && matches!(self.screen, Screen::Menu | Screen::Hub) && !self.settings_open {
            self.tip = None;
            self.confirm_new_dialog();
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
                net::Note::Spots => self.spots_settled(),
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
                net::Note::Vote { player, act } => {
                    if let (Some(a), Screen::Hub) = (HubAct::from_code(act), &self.screen) {
                        if a.kind() == 0 {
                            // a guest buying with their own points: done straight away
                            if let Some(c) = self.camp.as_mut() {
                                Self::do_act(c, player, a);
                                campaign::save(c);
                            }
                        } else {
                            self.cast(player, a);
                        }
                    }
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
    /// The Menu button: playing alone, the match keeps waiting behind the menu (paused) until
    /// Resume or Abandon. In co-op everybody's game runs in lockstep, so it still ends it.
    fn open_menu(&mut self) {
        if self.sess.is_some() {
            self.leave_game();
            return;
        }
        self.suspended = true;
        self.paused = true;
        self.status.clear();
        self.screen = if self.in_camp { Screen::Hub } else { Screen::Menu };
    }
    /// Back into the match left behind the menu.
    fn resume_game(&mut self) {
        if self.game.is_none() || self.scene.is_none() {
            self.suspended = false;
            self.paused = false;
            return;
        }
        // research and parts bought on the campaign screen meanwhile are ready to use
        if let (true, Some(c), Some(g)) = (self.in_camp, self.camp.as_ref(), self.game.as_mut()) {
            c.refresh(g);
        }
        self.suspended = false;
        self.paused = false;
        self.status.clear();
        self.screen = Screen::Play;
    }
    /// Gives up the match left behind the menu. A campaign city counts as lost, and its end
    /// screen carries on to the next map; any other match just ends.
    fn abandon_game(&mut self) {
        self.suspended = false;
        self.paused = false;
        self.status.clear();
        if self.in_camp {
            if let Some(g) = self.game.as_mut() {
                g.over = true;
                g.won = false;
                self.screen = Screen::Play;
                return;
            }
        }
        self.game = None;
        self.in_camp = false;
        self.screen = Screen::Menu;
    }
    /// Resume / Abandon side by side in `r`, in place of the start button. Returns true when
    /// one was pressed (the screen has changed).
    fn suspended_buttons(&mut self, r: Rect, resume: &str, abandon: &str) -> bool {
        let c = r.cols(2, 10.0);
        if self.button_primary(c[0], resume, true) || (!self.blocked && is_key_pressed(KeyCode::Enter)) {
            self.resume_game();
            return true;
        }
        if self.button(c[1], abandon, true, false) {
            self.abandon_game();
            return true;
        }
        if self.hot(c[1]) {
            self.tip = Some(if self.in_camp {
                "Ends the battle as a loss: the city falls and the campaign moves on to the next map. Surviving troops pull out with you.".into()
            } else {
                "Ends this match and drops it.".into()
            });
        }
        false
    }
    /// Quits the game straight away (Quit game, Alt+F4, closing the window). Settings and the
    /// campaign are saved as they change, so there is nothing to lose here.
    pub(super) fn quit(&mut self) {
        self.save_settings();
        self.leave_coop();
        std::process::exit(0);
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
    /// The garage designs on the build bar: the deployed ones whose parts are all on this player's
    /// shelf, once the campaign has the Turret workshop (always outside it).
    fn custom_tools(&self, g: &Game) -> Vec<(u8, crate::garage::Build)> {
        if !g.build_ok(self.me(), S::Custom) {
            return Vec::new();
        }
        self.library
            .deployed()
            .enumerate()
            .filter(|(_, b)| b.ok() && g.parts_ok(self.me(), b))
            .map(|(k, b)| (k as u8, b.clone()))
            .collect()
    }
    /// Picks a build bar tool; a garage design is sent to the game first, so its cost and range
    /// show while placing.
    fn pick_tool(&mut self, g: &mut Game, t: Tool) {
        if let Tool::Custom(k) = t {
            if let Some((_, b)) = self.custom_tools(g).into_iter().find(|c| c.0 == k) {
                self.act(g, Cmd::design(&b));
            }
        }
        self.tool = t;
    }

    fn act(&mut self, g: &mut Game, c: Cmd) -> Option<&'static str> {
        if let Some(s) = self.sess.as_mut() {
            s.issue(c);
            return None;
        }
        let m = net::apply(g, 0, c);
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
                invite |= self.pressed(hot);
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
        if lob.mode != net::MODE_CAMPAIGN {
            let y = self.heading("Faction", a.x, a.y);
            a.cut_top(y - a.y, 0.0);
            let nf = FACTIONS.len();
            let rows = a.cut_top(nf as f32 * FAC_ROW + (nf - 1) as f32 * 6.0, 18.0).rows(nf, 6.0);
            for (i, f) in FACTIONS.iter().enumerate() {
                let cr = rows[i];
                let active = lob.fac as usize == i;
                let c = Color::from_rgba(f.col[0], f.col[1], f.col[2], 255);
                rrect(cr, R_CTRL, if active { mix(p.panel2, c, 0.16) } else { alpha(p.panel2, 0.5) });
                rrect_lines(cr, R_CTRL, if active { 2.0 } else { 1.0 }, if active { c } else { p.line });
                for k in 0..16 {
                    let q = vec2(cr.x + 14.0 + (k * 37 % 27) as f32, cr.y + 14.0 + (k * 13 % 29) as f32);
                    let c = swatch(i, k, c);
                    draw_rectangle(q.x, q.y, 3.0, 3.0, if active { c } else { alpha(c, 0.4) });
                }
                text(&self.fonts.head, f.name, cr.x + 56.0, cr.y + 21.0, 16.0, if active { p.text } else { p.dim });
                let lines = wrap_lines(&self.fonts.mono, f.blurb, cr.w - 70.0, 11.0);
                for (k, l) in lines.iter().take(2).enumerate() {
                    text(&self.fonts.mono, l, cr.x + 56.0, cr.y + 36.0 + k as f32 * 13.0, 11.0, p.dim);
                }
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

    /// Co-op campaign screen: the host shares it (with the votes) while on it; a guest follows the
    /// host onto it and back off.
    fn sync_hub(&mut self) {
        let Some(s) = self.sess.as_mut() else { return };
        if s.is_host() {
            let h = match (&self.screen, &self.camp) {
                (Screen::Hub, Some(c)) => Some(net::Hub { camp: campaign::to_text(c), votes: self.votes.iter().map(|(p, a)| (*p as u32, a.code())).collect() }),
                _ => None,
            };
            s.set_hub(h);
            return;
        }
        match (&s.hub, &self.screen) {
            (Some(h), Screen::Menu | Screen::Hub) => {
                if self.screen == Screen::Menu || h.camp != self.hub_text || self.camp.is_none() {
                    self.hub_text = h.camp.clone();
                    self.camp = Some(campaign::from_text(&h.camp));
                }
                self.screen = Screen::Hub;
            }
            (None, Screen::Hub) => {
                self.screen = Screen::Menu;
                self.camp = None;
            }
            _ => {}
        }
    }

    /// The campaign screen is shared with other players, so its actions are votes.
    fn hub_votes(&self) -> bool {
        self.sess.as_ref().is_some_and(|s| s.players.len() > 1)
    }
    /// The votes as everybody sees them.
    fn current_votes(&self) -> Vec<(usize, HubAct)> {
        match &self.sess {
            Some(s) if !s.is_host() => s.hub.iter().flat_map(|h| h.votes.iter()).filter_map(|(p, a)| Some((*p as usize, HubAct::from_code(*a)?))).collect(),
            _ => self.votes.clone(),
        }
    }
    /// Host: player `p` votes for `a` (voting for it again takes the vote back).
    fn cast(&mut self, p: usize, a: HubAct) {
        if a.kind() == 0 {
            return;
        }
        if let Some(i) = self.votes.iter().position(|v| *v == (p, a)) {
            self.votes.remove(i);
            return;
        }
        self.votes.retain(|(q, b)| !(*q == p && b.kind() == a.kind()));
        self.votes.push((p, a));
    }
    /// A campaign screen action: done right away alone, a vote in co-op. True when it's time to
    /// go and defend the picked city.
    fn hub_act(&mut self, c: &mut Campaign, a: HubAct) -> bool {
        // the battle left running behind the menu is still that city's
        if self.suspended && matches!(a, HubAct::City(_)) {
            self.status = format!("Resume or abandon the defense of {} first.", c.city().name);
            return false;
        }
        // a guest's purchases and votes go to the host, who keeps the campaign
        if self.client() {
            if let Some(s) = self.sess.as_mut() {
                s.vote(a.code());
            }
            return false;
        }
        if !self.hub_votes() || a.kind() == 0 {
            return Self::do_act(c, self.me(), a);
        }
        self.cast(0, a);
        false
    }
    /// Carries out a campaign screen action for player `p`.
    fn do_act(c: &mut Campaign, p: usize, a: HubAct) -> bool {
        match a {
            HubAct::Tech(t) => {
                c.buy(p, t);
            }
            HubAct::Recruit(u) => {
                c.recruit(p, u);
            }
            HubAct::City(i) => c.select(i),
            HubAct::Go => return true,
            HubAct::Hunt(a) => {
                c.fbuy(a);
            }
            HubAct::ResearchPart(i) => {
                c.research_part(p, i);
            }
            HubAct::BuyPart(i) => {
                c.buy_part(p, i);
            }
        }
        false
    }
    /// Host: carries out whatever more than half the players voted for. True for "go".
    fn tally(&mut self, c: &mut Campaign) -> bool {
        let n = self.sess.as_ref().map(|s| s.players.len()).unwrap_or(1);
        if n != self.vote_n {
            // somebody joined or left: the old votes no longer add up
            self.votes.clear();
            self.vote_n = n;
        }
        let win = self.votes.iter().map(|v| v.1).find(|a| self.votes.iter().filter(|v| v.1 == *a).count() * 2 > n);
        let Some(a) = win else { return false };
        self.votes.retain(|v| v.1.kind() != a.kind() && !(a.kind() == 1 && v.1 == HubAct::Go));
        Self::do_act(c, 0, a)
    }
    /// The coloured dots of the players voting for `a`, in the top right of `r`.
    fn vote_marks(&self, r: Rect, a: HubAct) {
        if !self.hub_votes() {
            return;
        }
        let mut k = 0;
        for (p, b) in self.current_votes() {
            if b == a {
                let c = vec2(r.right() - 8.0 - k as f32 * 10.0, r.y + 7.0);
                draw_circle(c.x, c.y, 3.6, PLAYER_COLS[p % 4]);
                draw_circle_lines(c.x, c.y, 3.6, 1.0, WHITE);
                k += 1;
            }
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
            Some(c) => (net::MODE_CAMPAIGN, c.fac, c.diff, format!("turn {}, {} cities held, next up {}", c.fought + 1, c.held(), c.city().name)),
            None => (net::MODE_CAMPAIGN, campaign::ZOMBIES, self.diff, "a new one, starting in New York".to_string()),
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
        // half the view's width in map cells. Effects fade out quickly past the edge of the view,
        // and as the camera pulls back: full up close, silent from about 1100 cells out.
        let r = (screen_width() / 2.0 / self.cam.scale()).max(10.0);
        let zoom = ((1100.0 - self.cam.dist) / 800.0).clamp(0.0, 1.0);
        let zoom = zoom * zoom;
        for (s, p) in cues {
            let off = p - self.cam.t;
            let near = (1.0 - (off.length() - r).max(0.0) / (0.6 * r)).clamp(0.0, 1.0);
            // a flame burst is keyed by its bunker so its end can cut the sound
            let key = (((p.x * 4.0) as i64 as u64) << 32 | ((p.y * 4.0) as i64 as u32 as u64)) | 1;
            if s == Sfx::FlameStop {
                self.music.stop(key);
                continue;
            }
            let pan = (off.dot(self.cam.right()) / r).clamp(-1.0, 1.0);
            let jitter = rand::gen_range(0.93, 1.07);
            if let Sfx::Custom(v) = s {
                // garage turrets: the game's own guns, pitched and filtered per barrel
                let snd = match v.base {
                    1 => music::Snd::Cannon,
                    2 => music::Snd::Railgun,
                    3 => music::Snd::ArtilleryShot,
                    4 => music::Snd::Flame,
                    5 => music::Snd::HeliGun,
                    _ => music::Snd::Gun,
                };
                // the gun barrels sit well under the cannon and railgun
                let gain = match v.base {
                    0 => 0.14,
                    3 => 0.35,
                    4 => 0.3,
                    5 => 0.3,
                    _ => 0.45,
                } * near
                    * near
                    * zoom;
                self.music.play_toned(music::Play { snd, gain, pan, speed: v.pitch * jitter, key: if v.base == 4 { key } else { 0 } }, v.tone);
                continue;
            }
            let (snd, gain, speed) = match s {
                Sfx::Cannon => (music::Snd::Cannon, 0.55, jitter),
                Sfx::Flame => (music::Snd::Flame, 0.3, jitter),
                Sfx::Railgun => (music::Snd::Railgun, 0.8, jitter),
                Sfx::ArtilleryShot => (music::Snd::ArtilleryShot, 0.55, jitter),
                Sfx::ArtilleryHit => (music::Snd::ArtilleryHit, 0.8, jitter),
                Sfx::Bomb => (music::Snd::ArtilleryHit, 0.9, 0.85 * jitter),
                Sfx::Nuke => (music::Snd::Nuke, 1.0, 1.0),
                Sfx::Gun => (music::Snd::Gun, 0.14, jitter),
                Sfx::HeliGun => (music::Snd::HeliGun, 0.35, jitter),
                Sfx::Rocket => (music::Snd::Cannon, 0.3, 1.45 * jitter),
                Sfx::BaseAttacked => (music::Snd::BaseAttacked, 0.8, 1.0),
                Sfx::FlameStop | Sfx::Custom(_) => continue,
            };
            // a nuke is heard anywhere on the map, and the alarm wherever you're looking
            let gain = match s {
                Sfx::Nuke => gain * near.max(0.7),
                Sfx::BaseAttacked => gain,
                _ => gain * near * near * zoom,
            };
            let pan = if s == Sfx::BaseAttacked { 0.0 } else { pan };
            let key = if s == Sfx::Flame { key } else { 0 };
            self.music.play(music::Play { snd, gain, pan, speed, key });
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
        self.menu_ticker(r);
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
            let blurb = "Start in New York, then pick one city a turn on the globe; the rest run down, and you can't save them all. Harder cities pay more. Aliens and orcs join the zombies a few turns in. Civilians flown out buy research and troops. Fail three defences and the world's governments fire you.";
            let lines = wrap_lines(&self.fonts.mono, blurb, d.w, 13.0);
            for (i, l) in lines.iter().enumerate() {
                text(&self.fonts.mono, l, d.x, d.y + 13.0 + i as f32 * 18.0, 13.0, p.dim);
            }
            d.cut_top(lines.len() as f32 * 18.0 + 8.0, 0.0);
            let cities: Vec<&City> = saved_camp.as_ref().map(|c| c.all().collect()).unwrap_or_else(|| vec![&POOL[0]]);
            self.pack.want(&cities);
            let n = cities.len();
            let ready = cities.iter().filter(|c| campaign::have_map(c)).count();
            let f = ready as f32 / n as f32;
            let maps = match self.pack.getting(cities.iter().copied()) {
                _ if ready == n => format!("City maps: all {} ready", n),
                Some(c) => format!("City maps {}/{} · getting {}…", ready, n, c.name),
                None => format!("City maps {}/{} · the rest download when you reach them", ready, n),
            };
            let m = d.cut_top(30.0, 6.0);
            text(&self.fonts.mono, &maps, m.x, m.y + 13.0, 12.5, p.text);
            self.progress(Rect::new(m.x, m.y + 21.0, m.w, 4.0), f, if f >= 1.0 { p.good } else { p.accent });
            if let Some(c) = &saved_camp {
                let line = if c.fired() {
                    format!("fired after {} failed defences", c.lost)
                } else if c.saved_world() {
                    "the world is saved".to_string()
                } else if c.finished() {
                    format!("over, {} cities held", c.held())
                } else {
                    format!("turn {} · next {} · {} · {} RP to spend", c.fought + 1, c.city().name, DIFFS[c.diff].0, c.points(0))
                };
                let sr = d.cut_top(42.0, 0.0);
                rrect(sr, R_CTRL, alpha(p.accent, 0.1));
                let mut sr2 = sr.pad(12.0, 6.0);
                let nr = sr2.cut_right(132.0, 10.0);
                text(&self.fonts.mono, "SAVED CAMPAIGN", sr2.x, sr2.y + 11.0, 10.5, p.accent);
                let line = fit(&self.fonts.mono, &line, sr2.w, 12.5);
                text(&self.fonts.mono, &line, sr2.x, sr2.y + 27.0, 12.5, p.text);
                if self.button(nr, "New campaign", true, false) {
                    self.confirm_new = true;
                }
                if self.hot(nr) {
                    self.tip = Some("Starts over in New York with the difficulty picked here. The saved campaign is erased.".into());
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
                    if self.pressed((hot || hx) && !self.blocked) {
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

        // faction: the campaign is always zombies, the others arrive on their own
        if !self.camp_mode {
            let y = self.heading("Faction", a.x, a.y);
            a.cut_top(y - a.y, 0.0);
            let nf = FACTIONS.len();
            // the difficulty picker needs its 20 + 38 points under the factions; when the mode's
            // details leave too little room the factions go two by two with one line of blurb each
            let full = nf as f32 * FAC_ROW + (nf - 1) as f32 * 6.0;
            let compact = a.h - 18.0 - 58.0 < full;
            let rows: Vec<Rect> = if compact {
                let g = a.cut_top(2.0 * FAC_ROW_COMPACT + 6.0, 14.0);
                g.rows(2, 6.0).into_iter().flat_map(|r| r.cols(2, 6.0)).collect()
            } else {
                a.cut_top(full, 18.0).rows(nf, 6.0)
            };
            for (i, f) in FACTIONS.iter().enumerate() {
                let cr = rows[i];
                let active = self.fac == i;
                let hot = self.hot(cr);
                let c = Color::from_rgba(f.col[0], f.col[1], f.col[2], 255);
                rrect(cr, R_CTRL, if active { mix(p.panel2, c, 0.16) } else if hot { mix(p.panel2, p.text, 0.06) } else { p.panel2 });
                rrect_lines(cr, R_CTRL, if active { 2.0 } else { 1.0 }, if active { c } else { p.line });
                // a little swarm of the faction's pixels
                for k in 0..16 {
                    let q = vec2(cr.x + 14.0 + (k * 37 % 27) as f32, cr.y + 14.0 + (k * 13 % 29) as f32);
                    draw_rectangle(q.x, q.y, 3.0, 3.0, swatch(i, k, c));
                }
                text(&self.fonts.head, f.name, cr.x + 56.0, cr.y + 21.0, 16.0, p.text);
                let lines = wrap_lines(&self.fonts.mono, f.blurb, cr.w - 70.0, 11.0);
                for (k, l) in lines.iter().take(if compact { 1 } else { 2 }).enumerate() {
                    let l = if compact && lines.len() > 1 { fit(&self.fonts.mono, &format!("{} {}", l, lines[1]), cr.w - 64.0, 11.0) } else { l.clone() };
                    text(&self.fonts.mono, &l, cr.x + 56.0, cr.y + 36.0 + k as f32 * 13.0, 11.0, p.dim);
                }
                if compact && hot {
                    self.tip = Some(f.blurb.into());
                }
                if self.pressed(hot && !self.blocked) {
                    self.fac = i;
                }
                self.ui_rects.push(cr);
            }
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
        if self.suspended && self.game.is_some() {
            let (resume, abandon) = if self.in_camp { ("Resume defense", "Abandon city") } else { ("Resume game", "Abandon game") };
            if self.suspended_buttons(start, resume, abandon) {
                return;
            }
        } else if self.button_primary(start, label, can) || (can && !self.blocked && is_key_pressed(KeyCode::Enter)) {
            self.start();
            return;
        }
        if self.camp_mode && lobby && self.hot(start) {
            self.tip = Some("Opens your campaign for everybody. Spend civilians and pick the city on the globe, then all of you defend it together.".into());
        }
        if let Some(pr) = pr {
            self.players_panel(pr);
        }
        let fc = foot.cols(3, 10.0);
        if self.button(fc[0], "Settings", true, false) {
            self.open_settings();
        }
        if self.button(fc[1], "Turret garage", true, false) {
            self.open_workshop(false);
            return;
        }
        if self.hot(fc[1]) {
            self.tip = Some("The free builder: every part on the shelf. Builds saved here can be imported in a campaign once its parts are researched.".into());
        }
        if self.button(fc[2], "Quit game", true, false) {
            self.quit();
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
        let c = c.unwrap_or_else(|| Campaign::new(self.diff));
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
        let test = std::env::var("PH_NETHUB").is_ok().then(|| crate::citygen::generate(42));
        if let Some(d) = campaign::load_map(city).or(test) {
            self.begin(d, c.seed.wrapping_mul(31).wrapping_add(c.fought as u64 * 16 + c.current as u64));
            return;
        }
        self.screen = Screen::Loading;
        self.pack.want(&[city]);
        if self.pack.pending(city) {
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
        // this campaign's maps, the city picked first
        let want: Vec<&City> = std::iter::once(c.city()).chain(c.all()).collect();
        self.pack.want(&want);
        let before = (c.crew.clone(), c.units.len(), c.current, c.ftier);
        // the tech tree covers the screen: nothing under it reacts
        let tree = self.tree_open && !c.finished();
        let news = c.bulletin(self.told_local).is_some();
        if tree || news {
            self.blocked = true;
        }
        // co-op: a guest sees the host's campaign and votes; the host carries out what wins
        let guest = self.client();
        if !guest && self.hub_votes() && self.tally(&mut c) {
            campaign::save(&c);
            self.camp = Some(c);
            self.votes.clear();
            self.start_city();
            return;
        }
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
        let marks: Vec<globe::Mark> = c
            .all()
            .enumerate()
            .map(|(i, city)| {
                let s = &c.cities[i];
                let army = c.army_at(i);
                let (col, label) = match s.fate {
                    Fate::Open => (level_col(c.level(i)), format!("{} · {}", city.name, turns_s(s.turns))),
                    Fate::Held => (rgb(64, 192, 87), format!("{} · saved", city.name)),
                    Fate::Lost | Fate::Fallen => (rgb(58, 60, 66), format!("{} · fallen", city.name)),
                };
                let open = s.fate == Fate::Open;
                let reticle = (open && army != campaign::ZOMBIES).then(|| army_col(army));
                let dots = if open { c.level(i) } else { 0 };
                globe::Mark { lat: city.lat, lon: city.lon, col, label, picked: i == c.current && !c.finished(), reticle, dots, dot_col: army_col(army) }
            })
            .collect();
        // the alien swarms that flew this turn trail purple from where they left
        let start = match self.trail_t {
            Some((turn, t)) if turn == c.fought => t,
            _ => {
                self.trail_t = Some((c.fought, self.clock));
                self.clock
            }
        };
        let trails: Vec<globe::Trail> = c
            .aliens
            .iter()
            .filter(|s| s.moved == c.fought)
            .filter_map(|s| {
                let (f, t) = (c.info(s.from?), c.info(s.city));
                Some(globe::Trail { from: (f.lat, f.lon), to: (t.lat, t.lon), prog: (self.clock - start - 0.6) / 2.8, col: army_col(campaign::ALIENS) })
            })
            .collect();
        let over_globe = !left.contains(m) && !right.contains(m) && !self.blocked;
        let (hover, clicked) = globe.draw(gc, gr, &marks, &trails, &self.fonts.head, over_globe, get_frame_time());
        if let Some(i) = clicked {
            self.hub_act(&mut c, HubAct::City(i));
        }
        if let Some(i) = hover {
            self.tip = Some(self.city_tip(&c, i));
        }
        let title = "WORLD CAMPAIGN";
        text(&self.fonts.head, title, sw / 2.0 - text_w(&self.fonts.head, title, 28.0) / 2.0, 44.0, 28.0, p.text);
        let sub = format!("Turn {} · {} saved · {} fallen · {} of {} failed defences", c.fought + 1, c.held(), c.fallen(), c.lost, campaign::MAX_LOST);
        text(&self.fonts.mono, &sub, sw / 2.0 - text_w(&self.fonts.mono, &sub, 13.0) / 2.0, 66.0, 13.0, p.dim);
        let n = self.sess.as_ref().map(|s| s.players.len()).unwrap_or(1);
        let vote_hint = format!("Co-op: the next city and going in are votes ({} of {} players carry them). Research is each player's own.", n / 2 + 1, n);
        let hint = if self.hub_votes() { vote_hint.as_str() } else { "One city a turn: the rest run down. Drag to turn the globe, scroll to zoom, click a city to pick it" };
        text(&self.fonts.mono, hint, sw / 2.0 - text_w(&self.fonts.mono, hint, 12.0) / 2.0, sh - 22.0, 12.0, p.dim);

        // ---- left: the campaign and the picked city
        self.panel(left);
        let (x0, w) = (left.x + 20.0, left.w - 40.0);
        let mut y = left.y + 34.0;
        text(&self.fonts.head, "The war", x0, y, 20.0, p.text);
        // the armies on Earth, in their colours
        let mut ax = x0 + w;
        for a in (0..3).rev() {
            if !c.present(a) {
                continue;
            }
            let name = campaign_army_name(a);
            let tw = text_w(&self.fonts.mono, name, 11.0);
            ax -= tw;
            text(&self.fonts.mono, name, ax, y - 2.0, 11.0, army_col(a));
            ax -= 12.0;
        }
        y += 20.0;
        text(&self.fonts.mono, &format!("{} · {} flown out · {} kills", DIFFS[c.diff].0, c.flown, fmt_n(c.kills as usize)), x0, y, 12.0, p.dim);
        y += 18.0;
        // failed defences: three strikes
        text(&self.fonts.mono, "Failed defences", x0, y + 4.0, 12.0, p.dim);
        for k in 0..campaign::MAX_LOST {
            let on = k < c.lost;
            let r = Rect::new(x0 + 124.0 + k as f32 * 26.0, y - 8.0, 20.0, 16.0);
            draw_rectangle(r.x, r.y, r.w, r.h, if on { rgb(224, 49, 49) } else { p.panel2 });
        }
        y += 26.0;
        if c.finished() {
            self.camp_end(&c, x0, y, w);
        } else {
            let i = c.current;
            let city = c.city();
            let s = *c.state();
            let army = c.army();
            let lvl = c.level(i);
            let card = Rect::new(left.x + 10.0, y, left.w - 20.0, 214.0);
            draw_rectangle(card.x, card.y, card.w, card.h, p.panel2);
            draw_rectangle(card.x, card.y, 4.0, card.h, level_col(lvl));
            let mut cy = card.y + 34.0;
            text(&self.fonts.head, city.name, x0, cy, 26.0, p.text);
            let ct = city.country();
            text(&self.fonts.mono, ct, card.x + card.w - 12.0 - text_w(&self.fonts.mono, ct, 12.0), cy - 4.0, 12.0, p.dim);
            cy += 26.0;
            let falls = if s.turns <= 1 { "Falls after this turn".to_string() } else { format!("Falls in {} turns", s.turns) };
            text(&self.fonts.head, &falls, x0, cy, 17.0, if s.turns <= 2 { rgb(255, 90, 70) } else { rgb(255, 196, 64) });
            cy += 20.0;
            let who = match army {
                campaign::ALIENS => {
                    let major = c.swarms_at(i).any(|k| c.aliens[k].major);
                    format!("{} alien swarm · eats 2 turns a turn", if major { "The main" } else { "A minor" })
                }
                campaign::ORCS => "Orc portals".to_string(),
                _ => "Zombie outbreak".to_string(),
            };
            text(&self.fonts.mono, &who, x0, cy, 12.5, army_col(army));
            cy += 18.0;
            text(&self.fonts.mono, &format!("{:.1}M people alive of {:.1}M", c.alive(i), city.pop), x0, cy, 12.5, p.text);
            cy += 16.0;
            // threat bar
            draw_rectangle(x0, cy, w, 8.0, rgba(255, 255, 255, 0.08));
            draw_rectangle(x0, cy, w * (lvl as f32 / 16.0).clamp(0.03, 1.0), 8.0, level_col(lvl));
            cy += 26.0;
            text(&self.fonts.head, &format!("Threat level {}", lvl), x0, cy, 15.0, level_col(lvl));
            let pay = format!("${} payout", fmt_n(c.city_pay(i) as usize));
            text(&self.fonts.mono, &pay, card.x + card.w - 12.0 - text_w(&self.fonts.mono, &pay, 12.5), cy, 12.5, p.good);
            cy += 18.0;
            text(&self.fonts.mono, &format!("Horde ×{:.2} of {} · hold out {}", c.horde_mult(), DIFFS[c.diff].0, mmss(c.hold_secs())), x0, cy, 12.5, p.text);
            cy += 18.0;
            let map = if campaign::have_map(city) { "map ready" } else { "map downloads first" };
            text(&self.fonts.mono, &format!("{} civilians on the map · {}", fmt_n(c.civilians_at(i)), map), x0, cy, 12.5, p.dim);
            y = card.y + card.h + 18.0;
            // the cities still standing, closest to falling first, then the rest
            text(&self.fonts.head, "Cities", x0, y, 16.0, p.text);
            let tally = format!("{} standing · {} saved · {} fallen", (0..c.cities.len()).filter(|j| c.open(*j)).count(), c.held(), c.fallen());
            text(&self.fonts.mono, &tally, x0 + w - text_w(&self.fonts.mono, &tally, 11.0), y, 11.0, p.dim);
            y += 8.0;
            let mut order: Vec<usize> = (0..c.cities.len()).collect();
            order.sort_by_key(|&j| {
                let st = &c.cities[j];
                (match st.fate {
                    Fate::Open => 0,
                    Fate::Held => 1,
                    _ => 2,
                }, st.turns, j)
            });
            let by = left.y + left.h - 120.0;
            let rh = 20.0f32.min((by - y - 8.0) / c.cities.len() as f32).max(15.0);
            let room = ((by - y - 4.0) / rh).max(1.0) as usize;
            let more = order.len().saturating_sub(room);
            for (n, j) in order.into_iter().enumerate() {
                if more > 0 && n + 1 >= room {
                    let rest = format!("+{} more saved or fallen", more + 1);
                    text(&self.fonts.mono, &rest, left.x + 20.0, y + rh / 2.0 + 4.0, 11.5, p.dim);
                    break;
                }
                let st = &c.cities[j];
                let rr = Rect::new(left.x + 10.0, y, left.w - 20.0, rh - 2.0);
                let hot = self.hot(rr) && st.fate == Fate::Open;
                if j == c.current || hot {
                    draw_rectangle(rr.x, rr.y, rr.w, rr.h, if j == c.current { mix(p.panel2, p.accent, 0.25) } else { p.panel2 });
                }
                let (col, right_s) = match st.fate {
                    Fate::Open => (p.text, format!("lvl {} · {}", c.level(j), turns_s(st.turns))),
                    Fate::Held => (rgb(64, 192, 87), "saved".to_string()),
                    Fate::Lost => (rgb(120, 122, 128), "lost".to_string()),
                    Fate::Fallen => (rgb(120, 122, 128), "fallen".to_string()),
                };
                let ty = rr.y + rh / 2.0 + 4.0;
                if st.fate == Fate::Open {
                    draw_rectangle(rr.x + 8.0, ty - 7.0, 5.0, 5.0, army_col(c.army_at(j)));
                }
                text(&self.fonts.mono, c.info(j).name, rr.x + 20.0, ty, 12.5, col);
                text(&self.fonts.mono, &right_s, rr.x + rr.w - 10.0 - text_w(&self.fonts.mono, &right_s, 12.0), ty, 12.0, if st.fate == Fate::Open { level_col(c.level(j)) } else { col });
                self.vote_marks(rr, HubAct::City(j));
                if hot {
                    self.tip = Some(self.city_tip(&c, j));
                    if self.pressed(true) {
                        self.hub_act(&mut c, HubAct::City(j));
                    }
                }
                y += rh;
            }
            if !self.status.is_empty() {
                wrap(&self.fonts.mono, &self.status, x0, by + 6.0, w, 12.0, rgb(250, 82, 82));
            }
            let gr = Rect::new(x0, by + 30.0, w, 46.0);
            if self.suspended && self.game.is_some() {
                self.camp = Some(c);
                self.suspended_buttons(gr, "Resume defense", "Abandon city");
                if self.screen != Screen::Hub {
                    return;
                }
                let Some(cc) = self.camp.take() else { return };
                c = cc;
            } else {
                let label = if self.hub_votes() { format!("Ready to defend {}", city.name) } else { format!("Defend {}", city.name) };
                let go = (self.button(gr, &label, true, true) || (!self.blocked && is_key_pressed(KeyCode::Enter))) && self.hub_act(&mut c, HubAct::Go);
                self.vote_marks(gr, HubAct::Go);
                if go {
                    campaign::save(&c);
                    self.camp = Some(c);
                    self.start_city();
                    return;
                }
            }
        }
        if guest {
            if self.button(Rect::new(x0, left.y + left.h - 38.0, 130.0, 30.0), "Leave co-op", true, false) {
                self.leave_coop();
                self.camp = None;
                self.screen = Screen::Menu;
                return;
            }
        } else if self.button(Rect::new(x0, left.y + left.h - 38.0, 130.0, 30.0), if self.sess.is_some() { "Lobby" } else { "Main menu" }, true, false) {
            self.votes.clear();
            campaign::save(&c);
            self.camp = Some(c);
            self.screen = Screen::Menu;
            return;
        }
        if self.button(Rect::new(x0 + 140.0, left.y + left.h - 38.0, 130.0, 30.0), "Quit game", true, false) {
            // a guest's screen shows the host's campaign: never saved here
            if !guest {
                campaign::save(&c);
            }
            self.quit();
        }

        // ---- right: research and the army
        self.research_column(&mut c, right);
        if tree && !news {
            self.blocked = self.settings_open || self.invite_open || self.confirm_new;
            self.tech_tree(&mut c);
        }
        if news {
            self.bulletin(&mut c);
        }

        if (&c.crew, c.units.len(), c.current, c.ftier) != (&before.0, before.1, before.2, before.3) && !guest {
            campaign::save(&c);
        }
        if c.current != before.2 {
            self.status.clear();
        }
        self.camp = Some(c);
    }

    /// What the globe's pins and the city list show on hover.
    fn city_tip(&self, c: &Campaign, i: usize) -> String {
        let city = c.info(i);
        let s = &c.cities[i];
        match s.fate {
            Fate::Open => {
                let who = match c.army_at(i) {
                    campaign::ALIENS => " Aliens are attacking: it loses two turns a turn, and they grow stronger every turn nobody fights them.",
                    campaign::ORCS => " Orc portals have opened here.",
                    _ => "",
                };
                format!(
                    "{}, {}: falls in {}, {:.1} million alive. Threat level {} (horde ×{:.2}), {} civilians on the map, ${} payout for holding it.{}",
                    city.name,
                    city.country(),
                    turns_s(s.turns),
                    c.alive(i),
                    c.level(i),
                    c.horde_mult_at(i),
                    fmt_n(c.civilians_at(i)),
                    fmt_n(c.city_pay(i) as usize),
                    who
                )
            }
            Fate::Held => format!("{}, {}: saved. You held it until the evac.", city.name, city.country()),
            Fate::Lost => format!("{}, {}: fallen. The HQ was overrun, and every horde grew stronger.", city.name, city.country()),
            Fate::Fallen => format!("{}, {}: fell before anyone came, and every horde grew stronger.", city.name, city.country()),
        }
    }

    /// The end of a campaign, in the left column: fired, the world saved, or no city left to defend.
    fn camp_end(&mut self, c: &Campaign, x0: f32, y: f32, w: f32) {
        let p = pal(self.dark);
        let (head, body) = if c.fired() {
            (
                "You're fired.",
                format!(
                    "{} defences failed on your watch, and the world's governments have handed the war to someone else. You saved {} cities and flew {} civilians out.",
                    campaign::MAX_LOST,
                    c.held(),
                    c.flown
                ),
            )
        } else if c.saved_world() {
            (
                "You saved the world.",
                format!(
                    "The zombies, the aliens and the orcs are gone. You saved {} cities and flew {} civilians out. And with your newfound knowledge of the unnatural, you've decided to dominate the world with your undead army.",
                    c.held(),
                    c.flown
                ),
            )
        } else {
            ("No cities left to defend.", format!("You saved {} of the world's biggest cities and flew {} civilians out. The rest are the hordes'.", c.held(), c.flown))
        };
        text(&self.fonts.head, head, x0, y + 30.0, 24.0, if c.saved_world() { p.good } else { p.text });
        let lines = wrap_lines(&self.fonts.mono, &body, w, 13.0);
        for (k, l) in lines.iter().enumerate() {
            text(&self.fonts.mono, l, x0, y + 60.0 + k as f32 * 18.0, 13.0, p.dim);
        }
        let by = y + 80.0 + lines.len() as f32 * 18.0;
        if self.button(Rect::new(x0, by, w, 46.0), "New campaign", !self.client(), true) {
            self.confirm_new = true;
        }
    }

    /// Headless test hook: a campaign at city `level` with some troops and unlocks, on its screen or
    /// already playing a random city.
    pub fn test_campaign(&mut self, city: usize, play: bool) {
        let mut c = Campaign::with_seed(1, 42);
        // a few turns in, with the aliens and orcs about (PH_TURNS sets how many turns)
        let turns = std::env::var("PH_TURNS").ok().and_then(|s| s.parse().ok()).unwrap_or(6);
        c.play_turns(turns);
        c.select(city);
        c.fkills = [140_000, 60_000, 9_000];
        // PH_TOLD=1: no bulletins over the screen
        if std::env::var("PH_TOLD").is_ok() {
            c.told = 0xff;
        }
        c.crew_mut(0).points = 260;
        // a co-op partner's own points
        c.crew_mut(1).points = 180;
        c.flown = 640;
        c.kills = 182_000;
        c.crew_mut(0).tech = vec![campaign::Tech::Build(S::Gun), campaign::Tech::Ability(A::Rescue), campaign::Tech::Research(R::Rescue), campaign::Tech::Research(R::Armour)];
        // PH_TREE=1: the tech tree open
        self.tree_open = std::env::var("PH_TREE").is_ok();
        // PH_TREE_TAB=n: that page of it
        self.tree_tab = std::env::var("PH_TREE_TAB").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
        c.units = vec![(U::Rifle, 6), (U::Rifle, 4), (U::Tank, 1), (U::Rifle, 6)];
        let c_city = c.city();
        self.camp = Some(c);
        self.screen = Screen::Hub;
        if play {
            self.in_camp = true;
            let d = campaign::load_map(c_city).unwrap_or_else(|| crate::citygen::generate(42));
            self.begin(d, 1000 + city as u64);
        }
    }

    /// Headless test hook (PH_RESUME): step `k` of leaving the match through the Menu button,
    /// into the garage and back, then Resume. Prints what happened at the end.
    pub fn resume_test(&mut self, k: u32) -> bool {
        match k {
            10 => self.open_menu(),
            14 => self.open_workshop(self.in_camp),
            18 => {
                if let Some(w) = self.workshop.take() {
                    self.screen = if w.campaign { Screen::Hub } else { Screen::Menu };
                }
            }
            22 => {
                eprintln!("resume test: behind the menu {:?}, game kept {}", self.screen, self.game.is_some());
                self.resume_game();
            }
            30 => {
                eprintln!("resume test: after Resume {:?}, game {}", self.screen, self.game.is_some());
                return true;
            }
            _ => {}
        }
        false
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
        self.join_ticks();
        self.game.as_mut()
    }
    /// Takes the game back from the ticker thread once the ticks it was given have run.
    fn join_ticks(&mut self) {
        if let Some((g, n)) = self.ticker.join() {
            self.game = Some(g);
            self.fresh_ticks += n;
        }
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
            news: self.news_vol,
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
        // a campaign city left earlier doesn't make this match a campaign one
        self.in_camp = false;
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
        self.my_spot = None;
        self.spot_sent = false;
        // PH_NETSPOT=dx,dy: in the co-op test the guest picks its HQ spot (see `net_test`)
        let pick_test = std::env::var("PH_NETSPOT").is_ok();
        if self.auto_place && !(pick_test && self.client()) {
            self.launch(d, seed);
            if let Some(s) = self.sess.as_mut().filter(|s| s.is_host() && !pick_test) {
                s.finish_spots();
            }
            return;
        }
        let g = self.make_game(d.clone(), seed);
        self.show(g, true);
        self.placing = Some((d, seed));
        self.msg = (String::new(), 0.0);
    }

    /// Co-op: every HQ spot is settled. A guest still choosing starts the match; anyone already in
    /// it (the host, a guest that skipped choosing) rebuilds the game around the final spots, which
    /// is safe because the clock only starts now.
    fn spots_settled(&mut self) {
        if self.screen != Screen::Play {
            return;
        }
        if let Some((d, seed)) = self.placing.clone() {
            if self.client() {
                self.launch(d, seed);
            }
            return;
        }
        if self.sess.as_ref().is_some_and(|s| s.exec == 0) {
            if let Some((d, seed)) = self.coop_map.clone() {
                let g = self.make_game(d, seed);
                self.show(g, false);
            }
        }
    }

    /// Where the local player's own HQ is (in co-op each player has one).
    /// This player's index in co-op (0: the host, or playing alone).
    fn me(&self) -> usize {
        self.sess.as_ref().map(|s| s.me).unwrap_or(0)
    }
    fn my_hq(&self, g: &Game) -> Vec2 {
        let me = self.sess.as_ref().map(|s| s.me).unwrap_or(0);
        match g.hqs.get(me).map(|&i| &g.structs[i]) {
            Some(s) if !s.dead => s.center(),
            _ => g.world.hq,
        }
    }

    fn make_game(&self, mut d: Desc, seed: u64) -> Game {
        let camp = if self.in_camp { self.camp.as_ref() } else { None };
        // co-op: the HQ spots the players picked (a guest still choosing sees their own pick)
        let mut spots = self.sess.as_ref().filter(|s| s.in_game).map(|s| s.spots.clone()).unwrap_or_default();
        if let (Some(s), Some(p)) = (&self.sess, self.my_spot) {
            if !s.spots_ready && s.me > 0 {
                if spots.len() <= s.me {
                    spots.resize(s.me + 1, None);
                }
                spots[s.me] = Some(p);
            }
        }
        d.extra_hqs = spots.iter().skip(1).flatten().copied().collect();
        let mut g = match camp {
            Some(c) => Game::new(d, c.army(), DIFFS[c.diff].1 * c.horde_mult(), seed),
            None => Game::new(d, self.fac, DIFFS[self.diff].1, seed),
        };
        // co-op: an HQ for every player
        let players = match &self.sess {
            Some(s) if s.in_game || !s.is_host() => s.game_players,
            Some(s) => s.players.len(),
            None => {
                g.relax_fields();
                1
            }
        };
        g.add_player_hqs_at(players, &spots);
        if let Some(c) = camp {
            c.apply(&mut g);
        }
        g
    }

    fn show(&mut self, g: Game, recentre: bool) {
        if recentre {
            self.cam = Orbit { t: self.my_hq(&g), dist: 0.0, yaw: 0.0, pitch: 0.95 };
            self.cam.set_scale((screen_height() / 260.0).max(1.5));
        }
        let aa = self.aa;
        let painter = *self.depth.get_or_insert_with(|| pick_depth(aa)) == render::DepthMode::Painter;
        self.scene = Some(Scene::new(&g, self.theme(), painter));
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
        let mut n = News::new(g.fac, seed);
        if let Some(x) = std::env::var("PH_NEWS").ok().and_then(|v| v.parse().ok()) {
            n.preview(x);
        }
        self.news = Some(n);
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
        self.coop_map = self.sess.is_some().then(|| (d.clone(), seed));
        // alone, the game built for the chosen spot is the one to play; the host rebuilds from the
        // map as sent, exactly like the guests do
        if !chosen || self.sess.is_some() || self.game.is_none() {
            let g = self.make_game(d, seed);
            self.show(g, !chosen);
        }
        let camp = if self.in_camp { self.camp.as_ref() } else { None };
        self.banner = camp.map(|c| {
            let city = c.city();
            let turns = c.state().turns;
            let line = format!("Turn {} · falls in {} turn{} · threat level {} · hold out {} until the evac", c.fought + 1, turns, if turns == 1 { "" } else { "s" }, c.level(c.current), mmss(c.hold_secs()));
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
        if self.client() {
            // a guest moves only their own HQ; the map stays the host's
            if self.spot_sent {
                return;
            }
            let Some((d, seed)) = self.placing.clone() else { return };
            self.my_spot = Some(p);
            let g = self.make_game(d, seed);
            self.show(g, false);
            return;
        }
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
        let guest = self.client();
        if guest {
            b.cut_top(42.0, 0.0);
        } else if self.button(b.cut_top(34.0, 8.0), "Back", true, false) {
            self.placing = None;
            self.screen = if self.in_camp { Screen::Hub } else { Screen::Menu };
            self.in_camp = false;
        }
        let label = if self.spot_sent { "Waiting…" } else { "Start here" };
        if (self.button_primary(b.cut_top(36.0, 0.0), label, !self.spot_sent) || (!self.blocked && is_key_pressed(KeyCode::Enter))) && !self.spot_sent {
            self.place_go = true;
        }
        text(&self.fonts.head, "Choose your HQ location", a.x, a.y + 24.0, 24.0, p.text);
        let host = self.sess.as_ref().map(|s| s.host_name()).unwrap_or_default();
        let line = match (&self.sess, guest, self.spot_sent) {
            (_, true, true) => "Waiting for the other players to choose their HQ spots.".to_string(),
            (_, true, false) => format!("Click the map to move your HQ. {} has set up theirs.", host),
            (Some(_), false, _) => "Click the map to move your HQ. Each of your team picks their own spot next.".to_string(),
            (None, ..) => "Click the map to move the HQ. The horde comes once you start.".to_string(),
        };
        text(&self.fonts.mono, &line, a.x, a.y + 50.0, 13.0, p.dim);
        text(&self.fonts.mono, "Drag to look around, wheel to zoom, F for the whole map.", a.x, a.y + 70.0, 13.0, p.dim);
        self.msg.1 -= get_frame_time();
        if self.msg.1 > 0.0 && !self.msg.0.is_empty() {
            let m = self.msg.0.clone();
            self.toast(r.bottom() + 12.0, &m, p.bad, 1.0);
        }
    }

    /// Co-op host: the guests are still picking their HQ spots, and the clock waits for them.
    fn spots_wait(&mut self) {
        let Some(s) = self.sess.as_ref().filter(|s| s.is_host() && s.in_game && !s.spots_ready) else { return };
        let names: Vec<String> = s.spots_missing().into_iter().filter_map(|k| s.players.get(k).map(|p| p.name.clone())).collect();
        let p = pal(self.dark);
        let area = ui::hud();
        let r = Rect::new(area.x + (area.w - 620.0) / 2.0, area.y + area.h * 0.3, 620.0, 96.0);
        self.panel(r);
        text(&self.fonts.head, "Waiting for your team to place their HQs", r.x + 22.0, r.y + 32.0, 20.0, p.text);
        let who = if names.is_empty() { "Almost there…".to_string() } else { format!("Still choosing: {}", names.join(", ")) };
        text(&self.fonts.mono, &fit(&self.fonts.mono, &who, r.w - 230.0, 13.0), r.x + 22.0, r.y + 60.0, 13.0, p.dim);
        let b = Rect::new(r.right() - 190.0, r.y + 46.0, 170.0, 34.0);
        if self.button(b, "Start without waiting", true, false) {
            if let Some(s) = self.sess.as_mut() {
                s.finish_spots();
            }
        }
        if self.hot(b) {
            self.tip = Some("Anyone who hasn't picked gets an HQ spot picked for them".into());
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
            let city = self.camp.as_ref().map(|c| c.city()).unwrap_or(&POOL[0]);
            if let Some(d) = campaign::have_map(city).then(|| campaign::load_map(city)).flatten() {
                self.wait_pack = false;
                self.begin(d, 7);
                return;
            }
            if !self.pack.pending(city) {
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
        // the war drum, louder the closer the view is to the nearest drummer
        let near = g.drummers.iter().filter(|d| !d.dead).map(|d| d.p.distance(self.cam.t)).fold(f32::MAX, f32::min);
        self.music.drum(if near < f32::MAX && !g.over { (1.0 - near / 700.0).clamp(0.3, 1.0) } else { 0.0 });
        if let (Some(n), None, false) = (self.news.as_mut(), &self.placing, g.over) {
            if let Some(army) = n.broadcast(g.time, dt) {
                self.music.news(army);
            }
        }
        // fixed 20 Hz simulation; in co-op, in lockstep with the host. Nothing runs while the HQ
        // spot is being chosen.
        if self.placing.is_some() {
        } else if let Some(s) = self.sess.as_mut() {
            let me = s.me;
            let mut said = Vec::new();
            let mut n = 0;
            if s.is_host() {
                if !s.paused && !g.over && s.spots_ready {
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
        } else {
            // alone, the ticks that came due last frame ran on the ticker thread while that frame
            // was shown (see `Ticker`); the ones due now run once this frame is drawn
            if std::mem::take(&mut self.fresh_ticks) > 0 {
                sc.update_units(&g);
            }
            if !self.paused && !g.over {
                self.acc += dt;
                // a huge horde can make one tick take most of a frame: then run one tick per frame
                // and let the game slow down a little, instead of catching up and freezing the screen
                let most = if g.sim_ms > 20.0 { 1 } else { 3 };
                while self.acc >= DT && self.due < most {
                    self.acc -= DT;
                    self.due += 1;
                }
                if self.due == most {
                    self.acc = 0.0;
                }
            }
        }
        if !sc.units_fresh {
            sc.update_units(&g);
        }
        // the horde glides from the last tick's picture to this one's until the next tick
        // (ticks about to run show next frame: until then the newest picture stands)
        let still = self.placing.is_some() || g.over || self.due > 0 || self.sess.as_ref().map_or(self.paused, |s| s.paused);
        sc.fade = if still { 1.0 } else { (self.acc / DT).clamp(0.0, 1.0) };
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
        // whose HQ is whose
        if let Some(s) = &self.sess {
            if g.hqs.len() > 1 {
                for (k, &i) in g.hqs.iter().enumerate() {
                    let st = &g.structs[i];
                    let name = s.players.get(k).map(|p| p.name.as_str()).unwrap_or("Player");
                    let label = if st.dead { format!("{}'s HQ (fallen)", name) } else { format!("{}'s HQ", name) };
                    labels.items.push((render::p3(st.center(), 16.0), label, PLAYER_COLS[k % 4], 14.0));
                }
            }
        }
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
            self.spots_wait();
        }
        if let (Some((s, c)), false) = (cursor_label, self.settings_open || self.over_ui()) {
            let m = self.mouse();
            let tw = text_w(&self.fonts.mono, &s, 14.0);
            let r = Rect::new(m.x + 18.0, m.y + 16.0, tw + 20.0, 26.0);
            rrect(r, 6.0, Color::new(0.04, 0.05, 0.07, 0.82));
            text(&self.fonts.mono, &s, r.x + 10.0, r.y + 18.0, 14.0, c);
        }
        // (Esc that just closed the settings doesn't also open the menu)
        let modal = self.settings_open;
        self.overlays();
        ui::end();
        self.esc_free = !modal;
        self.input(&mut g, &mut sc, mw, dt);
        if self.screen == Screen::Play {
            let due = std::mem::take(&mut self.due);
            if due > 0 && self.placing.is_none() && self.sess.is_none() {
                self.ticker.start(g, due);
            } else {
                self.game = Some(g);
            }
            self.scene = Some(sc);
        } else {
            self.due = 0;
            // the Menu button: the match waits behind the menu (and the garage, the campaign
            // screen) until Resume or Abandon
            if self.suspended {
                self.game = Some(g);
                self.scene = Some(sc);
            }
        }
        if self.placing.is_some() {
            if let Some(p) = self.place_at.take() {
                self.move_hq(p);
            }
            if std::mem::take(&mut self.place_go) {
                if self.client() {
                    // a guest sends their spot; the match starts once everybody has
                    let mine = self.my_spot.or_else(|| self.game.as_ref().map(|g| self.my_hq(g)));
                    if let (Some(s), Some(p)) = (self.sess.as_mut(), mine) {
                        s.send_spot(p);
                        self.spot_sent = true;
                    }
                } else {
                    let (d, seed) = self.placing.clone().unwrap();
                    self.launch(d, seed);
                }
            }
        }
        self.place_at = None;
        self.place_go = false;
    }

    /// The headline ticker: a thin dark strip under the top bar, `r` wide, where each headline
    /// scrolls across a few times like an old cinema marquee and then the strip goes away for a
    /// while. Returns how much room it takes (0 while it's hidden).
    fn news_ticker(&mut self, r: Rect) -> f32 {
        let Some(mut n) = self.news.take() else { return 0.0 };
        let h = self.ticker_strip(&mut n, r);
        self.news = Some(n);
        h
    }
    fn ticker_strip(&self, n: &mut News, r: Rect) -> f32 {
        let size = 12.0;
        let tag_w = 52.0;
        let lane = r.w - tag_w - 8.0;
        let tw = n.line().map_or(0.0, |l| text_w(&self.fonts.mono, l, size));
        let a = n.scroll(get_frame_time().min(0.1), lane, tw);
        let Some(line) = n.line() else { return 0.0 };
        if a <= 0.0 {
            return 0.0;
        }
        let live = n.since_news < news::LIVE;
        let x = n.offset(lane, tw);
        rrect(r, 5.0, Color::new(0.03, 0.035, 0.05, 0.74 * a));
        let tag = Rect::new(r.x + 3.0, r.y + 3.0, tag_w - 6.0, r.h - 6.0);
        let (tag_bg, tag_s) = if live { (rgb(200, 40, 40), "LIVE") } else { (rgb(150, 105, 30), "NEWS") };
        rrect(tag, 3.0, alpha(tag_bg, 0.9 * a));
        let ts = 10.0;
        let tsw = text_w(&self.fonts.mono, tag_s, ts);
        text(&self.fonts.mono, tag_s, tag.x + (tag.w - tsw) / 2.0, tag.y + tag.h / 2.0 + 3.5, ts, Color::new(1.0, 1.0, 1.0, a));
        // the text only shows inside the lane (scissor is in pixels, top left origin)
        let k = ui::draw_scale();
        let lx = r.x + tag_w;
        let gl = unsafe { get_internal_gl() }.quad_gl;
        gl.scissor(Some(((lx * k) as i32, (r.y * k) as i32, (lane * k) as i32, (r.h * k).ceil() as i32)));
        text(&self.fonts.mono, line, lx + x, r.y + r.h / 2.0 + 4.0, size, Color::new(1.0, 0.78, 0.42, 0.88 * a));
        let gl = unsafe { get_internal_gl() }.quad_gl;
        gl.scissor(None);
        r.h + 6.0
    }

    /// The main menu's headline ticker, along the bottom of the city beside the menu column.
    fn menu_ticker(&mut self, col: Rect) {
        let s = ui::screen();
        let left = col.right() + 32.0;
        let w = (s.right() - 24.0 - left).min(640.0);
        if w < 280.0 {
            return;
        }
        let r = Rect::new(s.right() - 24.0 - w, col.bottom() - 22.0, w, 22.0);
        let mut n = std::mem::replace(&mut self.menu_news, News::menu(1));
        self.ticker_strip(&mut n, r);
        self.menu_news = n;
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
        let tool = if let Tool::Custom(_) = self.tool { Tool::Build(S::Custom) } else { self.tool };
        match tool {
            Tool::Build(t) => {
                let d = def(t);
                // a turret on top of a bunker (with Turret mounts)
                if d.turret && !d.ground && self.press.is_none() {
                    if let Some(bi) = g.bunker_at(mw) {
                        let c = g.structs[bi].center();
                        let can = g.can_mount(t, bi);
                        let cost = g.cost_for(self.me(), t, c);
                        let afford = g.scrap >= cost;
                        ghost(c.x - d.w as f32 / 2.0, c.y - d.h as f32 / 2.0, d.w as f32, d.h as f32, def(S::Bunker).ht, can && afford);
                        let s = if !g.res(self.me()).mounts {
                            "Research turret mounts to build on bunkers".to_string()
                        } else if !can {
                            "This bunker already has a turret".to_string()
                        } else {
                            draw_circle_lines(c.x, c.y, g.turret_range(t, true, self.me()), 0.4, rgba(255, 212, 59, 0.6));
                            format!("On the bunker  ${:.0}  +60% range", cost)
                        };
                        return Some((s, if can && afford { WHITE } else { rgb(255, 140, 140) }));
                    }
                }
                // rooftop spot for turrets
                if d.turret && !d.ground && self.press.is_none() {
                    if let Some((b, p)) = g.roof_at(mw) {
                        let q = p;
                        let can = g.can_place_roof(t, b, p);
                        let cost = g.cost_for(self.me(), t, p);
                        let afford = g.scrap >= cost;
                        ghost(q.x - d.w as f32 / 2.0, q.y - d.h as f32 / 2.0, d.w as f32, d.h as f32, g.world.buildings[b].h, can && afford);
                        draw_circle_lines(q.x, q.y, g.turret_range(t, true, self.me()), 0.4, rgba(255, 212, 59, 0.6));
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
                    let cost = g.cost_for(self.me(), t, c);
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
                        draw_circle_lines(c.x, c.y, g.turret_range(t, false, self.me()), 0.4, rgba(255, 212, 59, 0.6));
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
                let s = if u == U::Rifle && g.bunker_at(mw).is_some() {
                    format!("${:.0}  man the bunker (+60% range)", d.cost)
                } else if roof.is_some() {
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
                        let r = if g.res(self.me()).nuke { 190.0 } else { 120.0 };
                        draw_circle(mw.x, mw.y, r, rgba(234, 67, 53, 0.08));
                        draw_circle_lines(mw.x, mw.y, r, 1.0, rgba(234, 67, 53, 0.8));
                        // the core where nothing survives; the blast weakens out to the edge
                        draw_circle(mw.x, mw.y, r * NUKE_CORE, rgba(234, 67, 53, 0.12));
                        draw_circle_lines(mw.x, mw.y, r * NUKE_CORE, 0.6, rgba(234, 67, 53, 0.6));
                    }
                    A::Rescue => {
                        let r = if g.res(self.me()).rescue { 90.0 } else { 60.0 };
                        draw_circle_lines(mw.x, mw.y, r, 0.6, rgba(64, 192, 87, 0.8));
                    }
                    A::Gunship => draw_circle_lines(mw.x, mw.y, 25.0 + 34.0, 0.6, blue),
                    A::Air => {
                        let start = self.rpress.or(self.press.map(|p| p.1)).filter(|w| w.distance(mw) > 8.0);
                        let n = if g.res(self.me()).air { 12 } else { 5 };
                        match start {
                            Some(s) => {
                                // the run as it will fly: a line from the start, a ring where each bomb lands
                                let e = s + (mw - s).clamp_length_max(AIR_RUN_MAX);
                                draw_circle(s.x, s.y, 1.6, rgba(255, 212, 59, 0.9));
                                draw_line(s.x, s.y, e.x, e.y, 1.0, rgba(255, 212, 59, 0.8));
                                for k in 0..n {
                                    let p = s.lerp(e, k as f32 / (n - 1) as f32);
                                    draw_circle_lines(p.x, p.y, 12.0, 0.4, rgba(255, 212, 59, 0.55));
                                }
                            }
                            None => draw_circle_lines(mw.x, mw.y, 12.0, 0.5, rgba(255, 212, 59, 0.6)),
                        }
                    }
                }
                let hint = match a {
                    A::Air if self.rpress.is_some() => "Let go to send the bomber along this line",
                    A::Air => "Right-click the start of the bomb run, hold and drag to its end",
                    _ => "Click the target",
                };
                Some((hint.to_string(), WHITE))
            }
            Tool::None | Tool::Custom(_) => {
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
        // stress test: drops five million more hostiles each press (F9 does the same); never in
        // a campaign city, where it would wreck the run
        if !g.over && !g.campaign && self.button(b[0], "Spawn 5M", true, false) {
            self.act(g, Cmd::Stress(STRESS_N));
        }
        if !g.campaign && self.hot(b[0]) {
            self.tip = Some("Testing: five million more hostiles pour in from eight spots on the map [F9]".into());
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
            self.open_menu();
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
            ("SCRAP", format!("{:.0}/{:.0}", g.scrap, g.scrap_cap), if g.scrap >= g.scrap_cap { p.warn } else { p.accent }, Some("Scrap stored / storage. Build scrap depots for more room.")),
            ("AMMO", format!("{:.0}/{:.0}", g.ammo, g.ammo_cap), if g.ammo < 20.0 { p.bad } else if g.ammo >= g.ammo_cap { p.warn } else { p.text }, Some("Ammo stored / storage. Build ammo depots for more room.")),
            ("HOSTILES", fmt_n(g.hostiles()), p.text, None),
            ("CIVILIANS", fmt_n(g.alive_civs()), p.text, None),
        ];
        if evac {
            stats.push(("EVAC IN", mmss((g.goal - g.time).max(0.0)), p.warn, Some("Hold out until the evac flies everyone out")));
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
        let tick_h = self.news_ticker(Rect::new(top.x, top.bottom() + 6.0, (area.w * 0.42).clamp(320.0, 620.0), 20.0));
        if let Some(s) = &self.sess {
            let names: Vec<String> = s.players.iter().map(|p| p.name.clone()).collect();
            text(&self.fonts.mono, &format!("Co-op: {}", names.join(", ")), top.x + 4.0, top.bottom() + 20.0 + tick_h, 12.0, p.dim);
        }

        // ---- build bar, centred along the bottom; wraps to two rows when the HUD is too narrow
        let items: Vec<Tool> = BUILD_ORDER
            .iter()
            .filter(|s| !(g.campaign && **s == S::Lab) && g.build_ok(self.me(), **s))
            .map(|s| Tool::Build(*s))
            .chain([U::Rifle, U::Tank].into_iter().filter(|u| g.unit_ok(self.me(), *u)).map(Tool::Unit))
            .chain(self.custom_tools(g).into_iter().map(|c| Tool::Custom(c.0)))
            .collect();
        let customs = self.custom_tools(g);
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
                Tool::Custom(k) => {
                    let b = &customs.iter().find(|c| c.0 == *k).unwrap().1;
                    let cost = crate::garage::spec(b).map_or(0.0, |sp| sp.cost);
                    let [r0, g0, b0] = b.paint[0];
                    (' ', "Custom turret", cost, Color::from_rgba(r0, g0, b0, 255), "", g.scrap >= cost)
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
            if let Tool::Custom(k) = it {
                let fk = format!("F{}", k + 1);
                let kr = Rect::new(r.right() - 25.0, r.y + 5.0, 20.0, 16.0);
                rrect(kr, 4.0, alpha(p.text, 0.1));
                let tw = text_w(&self.fonts.mono, &fk, 10.5);
                text(&self.fonts.mono, &fk, kr.x + (kr.w - tw) / 2.0, kr.y + 12.0, 10.5, p.dim);
            } else {
                self.keycap(r.right() - 21.0, r.y + 5.0, key);
            }
            let cname;
            let nm = match it {
                Tool::Custom(k) => {
                    let b = &customs.iter().find(|c| c.0 == *k).unwrap().1;
                    cname = if b.name.is_empty() { "Custom".to_string() } else { b.name.clone() };
                    cname.as_str()
                }
                Tool::Build(S::Gun) => "Gun turret",
                Tool::Build(S::Extractor) => "Extractor",
                Tool::Build(S::Plant) => "Ammo plant",
                Tool::Build(S::Silo) => "Missile silo",
                Tool::Build(S::Pad) => "Evac point",
                Tool::Build(S::Lab) => "Research",
                Tool::Build(S::Rescue) => "Rescue stn",
                Tool::Build(S::ScrapDepot) => "Scrapyard",
                Tool::Build(S::AmmoDepot) => "Ammo dump",
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
                self.tip = Some(match it {
                    Tool::Custom(k) => {
                        let b = &customs.iter().find(|c| c.0 == *k).unwrap().1;
                        let parts: Vec<&str> = b.part_ids().map(|i| crate::garage::part(i).name).collect();
                        let sp = crate::garage::spec(b);
                        format!(
                            "{} [F{}]: your garage design. {}. {:.0} damage a second, range {:.0}.",
                            nm,
                            k + 1,
                            parts.join(", "),
                            sp.as_ref().map_or(0.0, |s| s.dps()),
                            sp.as_ref().map_or(0.0, |s| s.range)
                        )
                    }
                    _ => format!("{} [{}]: {}", name, key.to_ascii_uppercase(), tip),
                });
                if self.pressed(true) {
                    self.pick_tool(g, if active { Tool::None } else { *it });
                }
            }
        }

        // ---- abilities and research down the right side
        let aw = 248.0;
        let ax = area.right() - aw - 12.0;
        let mut ay = top.bottom() + 12.0;
        ay += self.hq_card(g, Rect::new(ax, ay, aw, 78.0)) + 8.0;
        // cards get shorter when the screen is short
        let lab = !g.campaign && g.has(S::Lab);
        let lab_h = if lab { 44.0 + LAB_ONLY.len() as f32 * 30.0 } else { 0.0 };
        let room = bar.y - 12.0 - ay - lab_h;
        // campaign: only what the tech tree has unlocked
        let abilities: Vec<A> = ABILITIES.into_iter().filter(|a| g.ability_ok(self.me(), *a)).collect();
        let card_h = ((room - 8.0 * (abilities.len().max(1) as f32 - 1.0)) / abilities.len().max(1) as f32).clamp(64.0, 88.0);
        for a in abilities {
            let d = adef(a);
            let r = Rect::new(ax, ay, aw, card_h);
            self.panel(r);
            let usable = g.can_use(self.me(), a);
            let active = self.tool == Tool::Ability(a);
            let head = Rect::new(r.x, r.y, r.w, card_h - 30.0);
            let hot = self.hot(head);
            if active {
                rrect_lines(r, R_PANEL, 2.0, p.accent);
            } else if hot && usable {
                rrect(head.inset(3.0), R_CTRL, alpha(p.text, 0.05));
            }
            let fc = if usable { p.text } else { p.dim };
            let name = format!("{}{}", d.name, if g.upgraded(self.me(), a) && a != A::Gunship { " +" } else { "" });
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
                if self.pressed(usable) {
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
            self.toast(top.bottom() + 14.0 + tick_h, &msg, p.accent, t.min(1.0));
        }
        // the HQ under attack: a red alert under the banners while the hits keep coming
        let since = g.time - g.hq_hit.0;
        if since < 5.0 && g.hq_hit.1 as usize == self.me() && !g.over {
            let a = (5.0 - since).min(1.0);
            let r = Rect::new(area.x + (area.w - 360.0) / 2.0, top.bottom() + 58.0 + tick_h, 360.0, 58.0);
            let red = rgb(255, 82, 82);
            let pulse = 0.55 + 0.45 * (self.clock * 6.0).sin().abs();
            ui::shadow(r, R_CTRL, alpha(red, 0.28 * a * pulse));
            rrect(r, R_CTRL, alpha(rgb(28, 12, 14), 0.94 * a));
            rrect_lines(r, R_CTRL, 1.0, alpha(red, 0.75 * a));
            draw_rectangle(r.x, r.y + 8.0, 4.0, r.h - 16.0, alpha(red, a * pulse));
            // a warning triangle
            let (cx, cy) = (r.x + 30.0, r.y + 29.0);
            draw_triangle(vec2(cx, cy - 12.0), vec2(cx - 13.0, cy + 10.0), vec2(cx + 13.0, cy + 10.0), alpha(red, a * pulse));
            text(&self.fonts.head, "!", cx - 2.5, cy + 7.0, 15.0, alpha(rgb(28, 12, 14), a));
            text(&self.fonts.head, "HQ UNDER ATTACK", r.x + 54.0, r.y + 26.0, 19.0, alpha(WHITE, a));
            text(&self.fonts.mono, "Click or press H to look", r.x + 54.0, r.y + 44.0, 12.0, alpha(rgb(255, 180, 180), a));
            self.ui_rects.push(r);
            if self.pressed(self.hot(r)) {
                self.cam.t = self.my_hq(g);
            }
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

    /// The HQ's level, the logistics it gives (turrets and squads in the field / allowed) and the
    /// button for the next level. Returns its height.
    fn hq_card(&mut self, g: &mut Game, r: Rect) -> f32 {
        let p = pal(self.dark);
        let me = self.me();
        let lvl = g.hq_level(me);
        let (used, cap) = (g.logi_used(me), g.logi_cap(me));
        self.panel(r);
        text(&self.fonts.head, &format!("HQ · level {}", lvl), r.x + 14.0, r.y + 23.0, 16.0, p.text);
        // five pips for the levels
        for k in 0..HQ_MAX {
            let q = Rect::new(r.right() - 14.0 - (HQ_MAX - k) as f32 * 14.0, r.y + 13.0, 10.0, 10.0);
            rrect(q, 2.0, if k < lvl { p.accent } else { alpha(p.line, 1.0) });
        }
        let full = used >= cap;
        let logi = if lvl >= HQ_MAX { format!("Logistics {} · no limit", used) } else { format!("Logistics {} / {}", used, cap) };
        text(&self.fonts.mono, &logi, r.x + 14.0, r.y + 41.0, 12.0, if full { p.bad } else { p.dim });
        if lvl < HQ_MAX {
            let pr = Rect::new(r.x + 14.0, r.y + 47.0, r.w - 28.0, 3.0);
            self.progress(pr, (used as f32 / cap as f32).min(1.0), if full { p.bad } else { p.accent });
        }
        let br = Rect::new(r.x + 8.0, r.bottom() - 26.0, r.w - 16.0, 22.0);
        match g.hq_up_cost(me) {
            Some(cost) => {
                let ok = g.scrap >= cost && !g.over;
                if self.button(br, &format!("Upgrade to level {} · ${:.0}", lvl + 1, cost), ok, false) {
                    self.act(g, Cmd::UpgradeHq);
                }
                if self.hot(br) {
                    let next = if lvl + 1 >= HQ_MAX { "as many turrets and squads as you can pay for".to_string() } else { format!("{} turrets and squads", logi_cap(lvl + 1)) };
                    self.tip = Some(format!("A bigger HQ with more health and room for {}.", next));
                }
            }
            None => text(&self.fonts.mono, "Fully upgraded", br.x + 6.0, br.y + 15.0, 11.5, p.dim),
        }
        if self.hot(Rect::new(r.x, r.y, r.w, r.h - 28.0)) {
            self.tip = Some("Logistics: every turret, rifle squad (six soldiers) and tank you have out counts one. Upgrade the HQ to field more.".into());
        }
        r.h
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
            text(&self.fonts.mono, &format!("{} civilians flown out: +{} RP · +${}", g.extracted, g.extracted, fmt_n(c.city_pay(c.current) as usize)), l.x, l.y + 14.0, 14.0, p.accent);
        } else {
            text(&self.fonts.head, &format!("{} is lost", c.city().name), t.x, t.y + 28.0, 30.0, p.text);
            let left = campaign::MAX_LOST.saturating_sub(next.lost);
            let line = match left {
                0 => "That's three failed defences. The world's governments are letting you go.".to_string(),
                1 => "The city falls, and every horde grows stronger. Nothing to spend from here. One more failed defence and you're fired.".to_string(),
                n => format!("The city falls, and every horde grows stronger. Nothing to spend from here. {} more failed defences and you're fired.", n),
            };
            let lines = wrap_lines(&self.fonts.mono, &line, a.w, 13.0);
            let l = a.cut_top(lines.len() as f32 * 17.0 + 4.0, 4.0);
            for (i, s) in lines.iter().enumerate() {
                text(&self.fonts.mono, s, l.x, l.y + 13.0 + i as f32 * 17.0, 13.0, p.bad);
            }
        }
        let l = a.cut_top(22.0, 2.0);
        text(&self.fonts.mono, &army, l.x, l.y + 14.0, 14.0, p.text);
        let fell: Vec<&str> = (0..c.cities.len()).filter(|i| c.cities[*i].fate == Fate::Open && next.cities[*i].fate == Fate::Fallen).map(|i| c.info(i).name).collect();
        let army = g.fac.min(2);
        let fp = next.fpoints(army) as i64 - c.fpoints(army) as i64;
        let world = if fell.is_empty() {
            format!("A turn passes. {} kills: +{} FP against the {}", fmt_n(g.kills as usize), fp, campaign_army_name(army).to_lowercase())
        } else {
            format!("A turn passes. Fallen meanwhile: {}.", fell.join(", "))
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
        let (label, en) = if g.res(self.me()).has(rs) {
            (format!("Done: {}", d.name), false)
        } else if g.campaign {
            ("Unlock between cities".to_string(), false)
        } else if let Some((busy, t)) = g.res(self.me()).busy {
            if busy == rs {
                prog = Some(t);
                (format!("Researching… {:.0}s", t), false)
            } else {
                (format!("{}: {} ${:.0}", verb, d.name, d.cost), false)
            }
        } else if !g.has(S::Lab) {
            (format!("{}: needs research lab", verb), false)
        } else {
            (format!("{}: {} ${:.0}", verb, d.name, d.cost), g.can_research(self.me(), rs))
        };
        let hot = self.hot(ur);
        let done = g.res(self.me()).has(rs);
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
            if self.pressed(en) {
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
            if let Some(s) = BUILD_ORDER.iter().find(|s| def(**s).key == ch && !(g.campaign && **s == S::Lab) && g.build_ok(self.me(), **s) && self.placing.is_none()) {
                self.tool = if self.tool == Tool::Build(*s) {
                    Tool::None
                } else {
                    Tool::Build(*s)
                };
            } else if self.placing.is_some() && !"tkmfh".contains(ch) {
            } else if let Some(u) = [U::Rifle, U::Tank].iter().find(|u| udef(**u).key == ch && g.unit_ok(self.me(), **u)) {
                self.tool = Tool::Unit(*u);
            } else if let Some(a) = ABILITIES.iter().find(|a| adef(**a).key == ch) {
                if g.can_use(self.me(), *a) {
                    self.tool = Tool::Ability(*a);
                }
            } else {
                match ch {
                    't' => self.toggle_top(),
                    'm' => self.set_music(if self.music_vol > 0 { 0 } else { self.music_prev }),
                    'p' | ' ' => self.toggle_pause(),
                    'h' => self.cam.t = self.my_hq(g),
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
            // with nothing to drop, Esc opens the menu (alone only: in co-op the menu ends the match)
            let idle = self.tool == Tool::None && self.selected.is_none() && self.sel_struct.is_none();
            if idle && self.esc_free && self.sess.is_none() && self.placing.is_none() && !g.over {
                self.open_menu();
                return;
            }
            self.tool = Tool::None;
            self.selected = None;
            self.sel_struct = None;
            self.press = None;
        }
        let fkeys = [KeyCode::F1, KeyCode::F2, KeyCode::F3, KeyCode::F4, KeyCode::F5, KeyCode::F6];
        for (k, key) in fkeys.into_iter().enumerate() {
            let alt = is_key_down(KeyCode::LeftAlt) || is_key_down(KeyCode::RightAlt);
            if !alt && is_key_pressed(key) && self.placing.is_none() && self.custom_tools(g).iter().any(|c| c.0 as usize == k) {
                let t = Tool::Custom(k as u8);
                self.pick_tool(g, if self.tool == t { Tool::None } else { t });
            }
        }
        if is_key_pressed(KeyCode::F9) && self.placing.is_none() && !g.campaign {
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

        // B-2: right press marks where the bomb run starts, release sends it to where the mouse is
        let air = self.tool == Tool::Ability(A::Air);
        if air && is_mouse_button_pressed(MouseButton::Right) && !ui {
            self.rpress = Some(mw);
        }
        if let (true, Some(w0)) = (air && is_mouse_button_released(MouseButton::Right), self.rpress) {
            self.rpress = None;
            if self.act(g, Cmd::Ability(A::Air, w0, mw)) != Some(net::BAD_TARGET) {
                self.tool = Tool::None;
            }
        }
        if !air {
            self.rpress = None;
        }
        // right click: order the selected group, or drop the tool
        if is_mouse_button_pressed(MouseButton::Right) && !ui && !air {
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
            if let Tool::Custom(k) = self.tool {
                // the design goes ahead of the order, in case it changed since it was picked
                match self.custom_tools(g).into_iter().find(|c| c.0 == k) {
                    Some((_, b)) => {
                        self.act(g, Cmd::design(&b));
                    }
                    None => {
                        self.tool = Tool::None;
                        return;
                    }
                }
            }
            let tool = if let Tool::Custom(_) = self.tool { Tool::Build(S::Custom) } else { self.tool };
            match tool {
                Tool::Custom(_) => {}
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
                        if let Some(b) = g.bunker_at(mw) {
                            self.act(g, Cmd::Mount { t, b: b as u32 });
                            return;
                        }
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
                    let from = if a == A::Air { w0 } else { mw };
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
        // PH_NETHUB=1: both players vote on the campaign screen (city 5, a tech, then go)
        let in_game = s.in_game;
        if std::env::var("PH_NETHUB").is_ok() && !in_game {
            if host && s.players.len() >= 2 && self.screen == Screen::Menu && self.hub_test == 0 {
                let _ = s;
                self.test_campaign(3, false);
                self.hub_test = 1;
                return false;
            }
            if self.screen == Screen::Hub {
                let Some(c) = self.camp.clone() else { return false };
                let me = self.sess.as_ref().unwrap().me;
                let tech = campaign::TECH.into_iter().find(|t| c.can_buy(me, *t));
                self.hub_test += 1;
                let vote = |app: &mut App, a: HubAct| {
                    if host && a.kind() == 0 {
                        if let Some(c) = app.camp.as_mut() {
                            App::do_act(c, me, a);
                        }
                    } else if host {
                        app.cast(me, a);
                    } else if let Some(s) = app.sess.as_mut() {
                        s.vote(a.code());
                    }
                };
                if self.hub_test == 20 {
                    vote(self, HubAct::City(5));
                    if let Some(t) = tech {
                        vote(self, HubAct::Tech(t));
                    }
                }
                if self.hub_test > 60 && c.current == 5 && self.hub_test % 60 == 0 {
                    let crews: Vec<String> = c.crew.iter().map(|k| format!("{} tech/{} RP", k.tech.len(), k.points)).collect();
                    eprintln!("{} hub: city {} crews {}", if host { "host" } else { "guest" }, c.current, crews.join(", "));
                    vote(self, HubAct::Go);
                }
            }
            if self.screen != Screen::Play {
                return false;
            }
        }
        let s = self.sess.as_ref().unwrap();
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
        let off: Vec<f32> = std::env::var("PH_NETSPOT").unwrap_or_default().split(',').filter_map(|x| x.parse().ok()).collect();
        if let (false, true, false, [dx, dy]) = (host, self.placing.is_some(), self.spot_sent, off.as_slice()) {
            let Some(g) = self.game.as_ref() else { return false };
            let p = g.world.hq + vec2(*dx, *dy);
            self.move_hq(p);
            let mine = self.game.as_ref().map(|g| self.my_hq(g));
            if let (Some(s), Some(m)) = (self.sess.as_mut(), mine) {
                eprintln!("guest picks HQ at {:?} (asked {:?})", m, p);
                s.send_spot(m);
                self.spot_sent = true;
            }
            return false;
        }
        let s = self.sess.as_ref().unwrap();
        let exec = s.exec;
        let Some(g) = self.game.as_ref() else { return false };
        let hq = g.world.hq;
        let plan: &[(u64, bool, Cmd)] = &[
            (20, true, Cmd::Build { t: S::Gun, a: hq + vec2(24.0, 0.0), b: hq + vec2(24.0, 0.0) }),
            (25, false, Cmd::Build { t: S::Wall, a: hq + vec2(-40.0, 30.0), b: hq + vec2(-10.0, 30.0) }),
            (30, false, Cmd::Deploy(U::Rifle, hq + vec2(0.0, -40.0))),
            (40, true, Cmd::Ability(A::Rescue, hq + vec2(120.0, 60.0), hq + vec2(120.0, 60.0))),
            (60, false, Cmd::Build { t: S::Gun, a: hq + vec2(-24.0, 0.0), b: hq + vec2(-24.0, 0.0) }),
            (70, true, Cmd::Stress(300_000)),
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
            let hqs: Vec<String> = g.hqs.iter().map(|&i| format!("{:.0},{:.0}", g.structs[i].center().x, g.structs[i].center().y)).collect();
            eprintln!("{} tick {} checksum {:016x} structs {} hqs {} hostiles {}{}", if host { "host" } else { "guest" }, exec, g.checksum(), g.structs.iter().filter(|s| !s.dead).count(), hqs.join(" "), g.hostiles(), if s.desync { " DESYNC" } else { "" });
        }
        exec >= until
    }
}

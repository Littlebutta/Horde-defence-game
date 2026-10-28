//! The garage: a bright assembly hall, laid out like a rocket builder's, where turrets are put
//! together from parts on a launch pad, painted with a bucket, named, and saved to the build
//! library. The parts palette runs down the left in category tabs, the assembly stack stands
//! beside the pad, and the engineer's report and saved turrets are on the right. Opened from the
//! main menu it's a free builder with every part on the shelf; from the campaign screen parts
//! have to be researched (research points) and bought (money from cities held) before they go on.

use super::*;
use crate::garage::{self as parts, part, Build, Slot, PARTS, SLOTS, SLOT_KINDS};
use crate::render::custom::{self as rcustom, Pose};

/// The bucket's colours: field greys and greens, hazard colours, and a few loud ones.
const PAINTS: [[u8; 3]; 20] = [
    [96, 104, 92],
    [72, 82, 64],
    [128, 118, 88],
    [176, 168, 120],
    [70, 74, 80],
    [120, 126, 134],
    [190, 194, 200],
    [236, 238, 240],
    [36, 38, 42],
    [255, 196, 40],
    [240, 110, 30],
    [200, 50, 45],
    [120, 30, 40],
    [40, 90, 160],
    [90, 170, 230],
    [40, 140, 110],
    [120, 200, 90],
    [150, 90, 200],
    [230, 120, 180],
    [140, 90, 60],
];
const ZONE_NAMES: [&str; 3] = ["Body", "Head", "Barrel"];
/// The hall's own colours: pale steel and sky, a map-blue for the blueprint lines.
const HALL_TOP: Color = Color::new(0.93, 0.95, 0.97, 1.0);
const HALL_LOW: Color = Color::new(0.84, 0.87, 0.91, 1.0);
const STEEL_LT: Color = Color::new(0.70, 0.74, 0.79, 1.0);
const STEEL_MD: Color = Color::new(0.55, 0.60, 0.66, 1.0);
const PRINT: Color = Color::new(0.10, 0.45, 0.91, 1.0);

pub(super) struct Workshop {
    pub build: Build,
    /// opened from the campaign screen (parts have to be researched and bought)
    pub campaign: bool,
    /// which of the seven slots a picked part goes into
    slot: usize,
    /// which category tab the parts palette shows (index into `SLOT_KINDS`)
    tab: usize,
    scroll: f32,
    lib_scroll: f32,
    yaw: f32,
    pitch: f32,
    dist: f32,
    /// the turntable is being dragged round from this mouse position
    turning: Option<Vec2>,
    /// typing the turret's name
    naming: bool,
    /// the paint bucket is loaded with this colour
    bucket: Option<[u8; 3]>,
    /// the library entry being edited
    sel: Option<usize>,
    rt: Option<RenderTarget>,
    mat: Option<Material>,
    /// the demo shot on the turntable
    fire_t: f32,
    note: (String, f32),
}

impl Workshop {
    pub fn new(campaign: bool, build: Build, sel: Option<usize>) -> Workshop {
        Workshop {
            build,
            campaign,
            slot: 0,
            tab: 0,
            scroll: 0.0,
            lib_scroll: 0.0,
            yaw: 0.6,
            pitch: 0.42,
            dist: 30.0,
            turning: None,
            naming: false,
            bucket: None,
            sel,
            rt: None,
            mat: None,
            fire_t: 0.0,
            note: (String::new(), 0.0),
        }
    }
    /// The slot a part of kind `k` goes into when it's picked from the palette: the selected slot
    /// if it is that kind and empty, else the first empty one of that kind, else the selected or
    /// first one. Picking the same modifier twice so fills both of its slots.
    fn target(&self, k: Slot) -> usize {
        let of: Vec<usize> = (0..SLOTS.len()).filter(|n| SLOTS[*n] == k).collect();
        if SLOTS[self.slot] == k && self.build.parts[self.slot].is_none() {
            return self.slot;
        }
        if let Some(n) = of.iter().find(|n| self.build.parts[**n].is_none()) {
            return *n;
        }
        if SLOTS[self.slot] == k {
            self.slot
        } else {
            of[0]
        }
    }
    /// Puts the palette on slot `n`'s category.
    fn pick_slot(&mut self, n: usize) {
        self.slot = n;
        let t = SLOT_KINDS.iter().position(|k| *k == SLOTS[n]).unwrap_or(0);
        if t != self.tab {
            self.tab = t;
            self.scroll = 0.0;
        }
    }
}

/// What the player can do with a part right now.
#[derive(Clone, Copy, PartialEq)]
enum Stock {
    /// on the shelf: it can go on a turret
    Owned,
    /// researched; buy it for this much money
    Buy(u32),
    /// research it for this many points
    Research(u32),
    /// the weaker part of its family has to be researched first
    Locked,
}

impl App {
    /// Opens the garage: from the campaign screen with the campaign's parts, or the free builder.
    pub(super) fn open_workshop(&mut self, campaign: bool) {
        let (build, sel) = match self.library.builds.first() {
            Some(b) => (b.clone(), Some(0)),
            None => (Build::starter(), None),
        };
        self.workshop = Some(Box::new(Workshop::new(
            campaign && self.camp.is_some(),
            build,
            sel,
        )));
        self.screen = Screen::Garage;
    }

    fn stock(&self, c: Option<&Campaign>, i: u16) -> Stock {
        let Some(c) = c else { return Stock::Owned };
        let me = self.me();
        if c.part_owned(me, i) {
            Stock::Owned
        } else if c.part_known(me, i) {
            Stock::Buy(part(i).price())
        } else if c.part_open(me, i) {
            Stock::Research(part(i).research())
        } else {
            Stock::Locked
        }
    }

    /// Headless test hook (PH_GARAGE).
    pub fn open_garage_test(&mut self) {
        let camp = self.camp.is_some();
        self.open_workshop(camp);
    }
    /// One frame of the garage.
    pub(super) fn workshop(&mut self) {
        let Some(mut w) = self.workshop.take() else {
            self.screen = Screen::Menu;
            return;
        };
        let mut camp = if w.campaign { self.camp.take() } else { None };
        let before = camp.as_ref().map(|c| c.crew.clone());
        // the hall is always lit: the light palette, whatever the map uses
        let dark = std::mem::replace(&mut self.dark, false);
        let back = self.workshop_frame(&mut w, camp.as_mut());
        self.dark = dark;
        if let Some(c) = camp {
            // a guest's copy is the host's: theirs is saved there
            if before.as_ref() != Some(&c.crew) && !self.client() {
                campaign::save(&c);
            }
            self.camp = Some(c);
        }
        if back {
            self.library.save();
            self.screen = if w.campaign {
                Screen::Hub
            } else {
                Screen::Menu
            };
            self.workshop = None;
        } else {
            self.workshop = Some(w);
        }
    }

    /// Draws and runs the garage; true when the player leaves it.
    fn workshop_frame(&mut self, w: &mut Workshop, mut camp: Option<&mut Campaign>) -> bool {
        let p = pal(false);
        let dt = get_frame_time().min(0.1);
        let scr = ui::screen();
        let hud = ui::hud();
        let me = self.me();
        let spec = parts::spec_raw(&w.build);
        let problem = w.build.problem();

        // ---- layout, in points
        let top = Rect::new(hud.x + 12.0, 10.0, hud.w - 24.0, 52.0);
        let colw = (hud.w * 0.25).clamp(310.0, 390.0);
        let left = Rect::new(
            hud.x + 12.0,
            top.bottom() + 10.0,
            colw,
            scr.h - top.bottom() - 22.0,
        );
        let right = Rect::new(hud.right() - 12.0 - colw, left.y, colw, left.h);
        let stage = Rect::new(
            left.right() + 10.0,
            left.y,
            right.x - left.right() - 20.0,
            left.h,
        );
        // the assembly stack stands down the right of the hall, the paint shelf along its foot;
        // the pad is drawn in what's left so the turret stays in the middle of it
        let stack_w = (stage.w * 0.3).clamp(168.0, 210.0);
        let stack = Rect::new(stage.right() - stack_w - 10.0, stage.y + 10.0, stack_w, 0.0);
        let shelf_h = 96.0;
        let view = Rect::new(
            stage.x,
            stage.y,
            stage.w - stack_w - 14.0,
            stage.h - shelf_h - 14.0,
        );

        // ---- the turntable, rendered in pixels before the UI goes on top
        w.fire_t += dt;
        if w.turning.is_none() {
            w.yaw += dt * 0.25;
        }
        let flash = if problem.is_none() && w.fire_t % 2.4 < 0.12 {
            1.0
        } else {
            0.0
        };
        let pose = Pose {
            aim: std::f32::consts::FRAC_PI_4 * 3.0,
            aim2: std::f32::consts::FRAC_PI_4 * 3.0 + 0.4,
            rise: 1.0,
            arm: spec.reach * 0.5,
            flash,
            walk: 0.0,
            t: self.clock,
        };
        let k = ui::scale();
        let vp = (
            (view.x * k) as i32,
            (view.y * k) as i32,
            (view.w * k).max(8.0) as i32,
            (view.h * k).max(8.0) as i32,
        );
        let aa = self.aa;
        let depth = *self.depth.get_or_insert_with(|| pick_depth(aa));
        ui::end();
        match depth {
            render::DepthMode::Target(samples) => {
                if w.rt
                    .as_ref()
                    .map(|r| (r.texture.width() as i32, r.texture.height() as i32))
                    != Some((vp.2, vp.3))
                {
                    w.rt = Some(render_target_ex(
                        vp.2 as u32,
                        vp.3 as u32,
                        RenderTargetParams {
                            sample_count: samples,
                            depth: true,
                        },
                    ));
                }
                let mat = w.mat.get_or_insert_with(render::depth_material);
                rcustom::preview(
                    &w.build,
                    &spec,
                    pose,
                    w.yaw,
                    w.pitch,
                    w.dist,
                    w.rt.as_ref(),
                    vp,
                    Some(mat),
                );
            }
            render::DepthMode::Painter => {}
        }
        ui::begin();

        // ---- the hall around it
        self.garage_backdrop(scr, stage, view);
        if let Some(rt) = &w.rt {
            draw_texture_ex(
                &rt.texture,
                view.x,
                view.y,
                WHITE,
                DrawTextureParams {
                    dest_size: Some(vec2(view.w, view.h)),
                    flip_y: true,
                    ..Default::default()
                },
            );
        } else {
            // no depth buffer on this GPU: straight to the screen, far parts first
            ui::end();
            let sy = screen_height() as i32 - vp.1 - vp.3;
            rcustom::preview(
                &w.build,
                &spec,
                pose,
                w.yaw,
                w.pitch,
                w.dist,
                None,
                (vp.0, sy, vp.2, vp.3),
                None,
            );
            ui::begin();
        }

        // ---- the toolbar: title, the turret's name in the middle, the file buttons on the right
        self.panel(top);
        let title = "TURRET GARAGE";
        text(
            &self.fonts.head,
            title,
            top.x + 18.0,
            top.y + 24.0,
            19.0,
            p.text,
        );
        text(
            &self.fonts.mono,
            if w.campaign {
                "CAMPAIGN · RESEARCH AND BUY PARTS"
            } else {
                "FREE BUILDER · EVERY PART"
            },
            top.x + 18.0,
            top.y + 41.0,
            9.5,
            p.dim,
        );
        let tb = top.pad(10.0, 9.0);
        let btns = Rect::new(tb.right() - 4.0 * 94.0, tb.y, 4.0 * 94.0, tb.h).cols(4, 6.0);
        if self.button(btns[0], "New", true, false) {
            w.build = Build {
                name: "New turret".into(),
                ..Build::starter()
            };
            w.sel = None;
        }
        if self.button(btns[1], "Save copy", problem.is_none(), false) {
            let mut b = w.build.clone();
            let mut n = 2;
            while self
                .library
                .builds
                .iter()
                .any(|x| x.name.eq_ignore_ascii_case(&b.name))
            {
                b.name = format!(
                    "{} {}",
                    w.build.name.chars().take(20).collect::<String>(),
                    n
                );
                n += 1;
            }
            w.build = b.clone();
            w.sel = Some(self.library.put(b));
            self.library.save();
        }
        let save = self.button_primary(btns[2], "Save", problem.is_none());
        if save {
            let k = self.library.put(w.build.clone());
            w.sel = Some(k);
            self.library.save();
            w.note = (format!("Saved {}", w.build.name), 2.0);
        }
        let leave = self.button(btns[3], "Exit", true, false)
            || (!self.blocked
                && !w.naming
                && w.bucket.is_none()
                && is_key_pressed(KeyCode::Escape));
        if let Some(c) = camp.as_deref() {
            let s = format!("${}   {} RP", c.crew(me).money, c.points(me));
            let sw = text_w(&self.fonts.head, &s, 17.0);
            let chip = Rect::new(btns[0].x - 18.0 - sw - 24.0, tb.y, sw + 24.0, tb.h);
            rrect(chip, R_CTRL, mix(p.panel2, p.accent, 0.12));
            text(
                &self.fonts.head,
                &s,
                chip.x + 12.0,
                chip.y + chip.h / 2.0 + 6.0,
                17.0,
                p.accent,
            );
        }
        // the name plate
        let name_w = (top.w * 0.3).clamp(220.0, 360.0);
        let name_r = Rect::new(top.center().x - name_w / 2.0, tb.y, name_w, tb.h);
        let hot_name = self.hot(name_r);
        rrect(
            name_r,
            R_CTRL,
            if w.naming || hot_name {
                p.panel
            } else {
                p.panel2
            },
        );
        rrect_lines(
            name_r,
            R_CTRL,
            if w.naming { 2.0 } else { 1.0 },
            if w.naming { p.accent } else { p.line },
        );
        let caret = if w.naming && (self.clock * 2.0) as i32 % 2 == 0 {
            "|"
        } else {
            ""
        };
        let nm = fit(
            &self.fonts.head,
            &format!("{}{}", w.build.name, caret),
            name_r.w - 24.0,
            18.0,
        );
        let nw = text_w(&self.fonts.head, &nm, 18.0);
        text(
            &self.fonts.head,
            &nm,
            name_r.x + (name_r.w - nw) / 2.0,
            name_r.y + name_r.h / 2.0 + 6.5,
            18.0,
            p.text,
        );
        if hot_name && !w.naming {
            self.tip = Some("Click to rename this turret".into());
        }
        self.ui_rects.push(name_r);
        if self.pressed(hot_name) {
            w.naming = true;
        } else if is_mouse_button_pressed(MouseButton::Left) && !hot_name {
            w.naming = false;
        }
        if w.naming {
            while let Some(ch) = get_char_pressed() {
                if !ch.is_control() && !matches!(ch, ';' | '|') && w.build.name.chars().count() < 24
                {
                    w.build.name.push(ch);
                }
            }
            if is_key_pressed(KeyCode::Backspace) {
                w.build.name.pop();
            }
            if is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::Escape) {
                w.naming = false;
            }
        } else {
            while get_char_pressed().is_some() {}
        }

        // ---- left: the parts palette, category tabs down its edge and part tiles beside them
        self.panel(left);
        let mut a = left.pad(10.0, 10.0);
        let tabs = a.cut_left(50.0, 10.0);
        for (t, kind) in SLOT_KINDS.iter().enumerate() {
            let r = Rect::new(tabs.x, tabs.y + t as f32 * 58.0, 50.0, 52.0);
            let on = w.tab == t;
            let hot = self.hot(r);
            rrect(
                r,
                R_CTRL,
                if on {
                    mix(p.panel2, p.accent, 0.2)
                } else if hot {
                    mix(p.panel2, p.text, 0.07)
                } else {
                    p.panel2
                },
            );
            if on {
                rrect_lines(r, R_CTRL, 1.5, p.accent);
                draw_rectangle(r.x - 10.0, r.y + 10.0, 3.0, r.h - 20.0, p.accent);
            }
            part_icon(
                Rect::new(r.x + 10.0, r.y + 6.0, 30.0, 30.0),
                *kind,
                3,
                if on { p.accent } else { p.dim },
            );
            let lbl = match kind {
                Slot::Mount => "MOUNT",
                Slot::Head => "HEAD",
                Slot::Barrel => "BARREL",
                Slot::Mod => "MODS",
                Slot::Gear => "GEAR",
            };
            let lw = text_w(&self.fonts.mono, lbl, 8.5);
            text(
                &self.fonts.mono,
                lbl,
                r.x + (r.w - lw) / 2.0,
                r.bottom() - 6.0,
                8.5,
                if on { p.accent } else { p.dim },
            );
            if hot {
                self.tip = Some(format!(
                    "{}: {} parts",
                    kind.plural(),
                    PARTS.iter().filter(|q| q.slot() == *kind).count()
                ));
            }
            self.ui_rects.push(r);
            if self.pressed(hot) {
                w.tab = t;
                w.scroll = 0.0;
                let n = w.target(*kind);
                w.slot = n;
            }
        }
        let kind = SLOT_KINDS[w.tab];
        let into = w.target(kind);
        let hr = a.cut_top(34.0, 6.0);
        text(
            &self.fonts.head,
            kind.plural(),
            hr.x,
            hr.y + 17.0,
            17.0,
            p.text,
        );
        let into_s = match into {
            3 | 5 => format!("goes into {} 1", SLOTS[into].name().to_lowercase()),
            4 | 6 => format!("goes into {} 2", SLOTS[into].name().to_lowercase()),
            _ => "one per turret".to_string(),
        };
        text(&self.fonts.mono, &into_s, hr.x, hr.y + 31.0, 9.5, p.dim);
        if matches!(kind, Slot::Mod | Slot::Gear) {
            let er = Rect::new(hr.right() - 70.0, hr.y + 4.0, 70.0, 26.0);
            if self.button(er, "Empty", w.build.parts[into].is_some(), false) {
                w.build.parts[into] = None;
            }
            if self.hot(er) {
                self.tip = Some("Take the part out of the slot it would go into".into());
            }
        }
        let list = a;
        let mut ids: Vec<u16> = (0..PARTS.len() as u16)
            .filter(|i| part(*i).slot() == kind)
            .collect();
        ids.sort_by_key(|i| (part(*i).tier, part(*i).fam as u8, part(*i).lvl));
        let per = 2;
        let tw = (list.w - 8.0) / per as f32;
        let th = 92.0;
        let total = ids.len().div_ceil(per) as f32 * (th + 8.0);
        if self.hot(list) {
            let (_, wy) = mouse_wheel();
            w.scroll -= wy.signum() * (th + 8.0);
        }
        w.scroll = w.scroll.clamp(0.0, (total - list.h).max(0.0));
        let mut hover: Option<u16> = None;
        for (n, i) in ids.iter().copied().enumerate() {
            let r = Rect::new(
                list.x + (n % per) as f32 * (tw + 8.0),
                list.y + (n / per) as f32 * (th + 8.0) - w.scroll,
                tw,
                th,
            );
            // half-shown tiles at the edges are left out rather than clipped
            if r.y < list.y - 1.0 || r.bottom() > list.bottom() + 1.0 {
                continue;
            }
            let pt = part(i);
            let fitted = w.build.parts.iter().filter(|q| **q == Some(i)).count();
            let hot = self.hot(r);
            let st = self.stock(camp.as_deref(), i);
            let owned = st == Stock::Owned;
            if hot {
                ui::shadow(r, R_CTRL, p.shadow);
            }
            rrect(
                r,
                R_CTRL,
                if fitted > 0 {
                    mix(p.panel, p.accent, 0.14)
                } else if hot {
                    p.panel
                } else {
                    p.panel2
                },
            );
            rrect_lines(
                r,
                R_CTRL,
                if fitted > 0 { 1.5 } else { 1.0 },
                if fitted > 0 { p.accent } else { p.line },
            );
            // the icon in its little bay, the name beside it
            let ib = Rect::new(r.x + 8.0, r.y + 8.0, 36.0, 36.0);
            rrect(
                ib,
                6.0,
                if owned {
                    p.bg
                } else {
                    mix(p.bg, p.panel2, 0.6)
                },
            );
            part_icon(
                ib.inset(4.0),
                pt.slot(),
                pt.tier,
                if owned { tier_col(pt.tier) } else { p.dim },
            );
            let lines = wrap_lines(&self.fonts.head, pt.name, r.w - 58.0, 12.5);
            for (l, s) in lines.iter().take(2).enumerate() {
                text(
                    &self.fonts.head,
                    &fit(&self.fonts.head, s, r.w - 58.0, 12.5),
                    r.x + 50.0,
                    r.y + 20.0 + l as f32 * 15.0,
                    12.5,
                    if owned { p.text } else { p.dim },
                );
            }
            self.tier_pips(r.x + 8.0, r.y + 50.0, pt.tier);
            let wt = if pt.slot() == Slot::Mount {
                let mut b = Build::starter();
                b.parts = [Some(i), None, None, None, None, None, None];
                format!("load {:.0}", parts::spec_raw(&b).cap)
            } else {
                format!("wt {:.1}", pt.weight())
            };
            text(
                &self.fonts.mono,
                &format!("${:.0} · {}", pt.cost(), wt),
                r.x + 54.0,
                r.y + 55.0,
                9.5,
                p.dim,
            );
            if fitted > 0 {
                let s = if fitted > 1 {
                    format!("FITTED ×{}", fitted)
                } else {
                    "FITTED".to_string()
                };
                text(
                    &self.fonts.mono,
                    &s,
                    r.x + 8.0,
                    r.bottom() - 10.0,
                    9.5,
                    p.accent,
                );
            }
            // what it takes to get it, along the bottom
            let br = Rect::new(r.right() - 92.0, r.bottom() - 28.0, 84.0, 22.0);
            match st {
                Stock::Owned => {}
                Stock::Buy(price) => {
                    let can = camp.as_deref().is_some_and(|c| c.crew(me).money >= price);
                    if self.button(br, &format!("Buy ${}", price), can, false) {
                        if let Some(c) = camp.as_deref_mut() {
                            self.hub_act(c, HubAct::BuyPart(i));
                        }
                    }
                }
                Stock::Research(pts) => {
                    let can = camp.as_deref().is_some_and(|c| c.points(me) >= pts);
                    if self.button(br, &format!("Research {}", pts), can, false) {
                        if let Some(c) = camp.as_deref_mut() {
                            self.hub_act(c, HubAct::ResearchPart(i));
                        }
                    }
                }
                Stock::Locked => text(
                    &self.fonts.mono,
                    "LOCKED",
                    br.right() - 40.0,
                    br.y + 15.0,
                    9.5,
                    p.dim,
                ),
            }
            self.ui_rects.push(r);
            if hot && !(st != Stock::Owned && self.hot(br)) {
                hover = Some(i);
                let need = match st {
                    Stock::Locked => {
                        let prev = parts::needs(i).map(|n| part(n).name).unwrap_or("");
                        format!(" Research the {} first.", prev)
                    }
                    Stock::Research(n) => {
                        format!(" Research it for {} research points, then buy it.", n)
                    }
                    Stock::Buy(m) => format!(" Researched: buy it for ${}.", m),
                    Stock::Owned if matches!(kind, Slot::Mod | Slot::Gear) => {
                        " Fit two of the same and their effects stack.".to_string()
                    }
                    Stock::Owned => String::new(),
                };
                self.tip = Some(format!("{}{}", parts::tip(i), need));
                if self.pressed(true) {
                    if owned {
                        w.build.parts[into] = Some(i);
                        // the next pick of this kind goes into its other slot, if that's empty
                        let next = w.target(kind);
                        w.slot = next;
                    } else {
                        w.note = (
                            "Not on the shelf yet: research and buy it first".into(),
                            2.5,
                        );
                    }
                }
            }
        }
        self.ui_rects.push(list);

        // ---- the assembly stack beside the pad: one card a slot, top to bottom
        let row_h = 46.0;
        let stack = Rect::new(
            stack.x,
            stack.y,
            stack.w,
            30.0 + SLOTS.len() as f32 * (row_h + 5.0),
        );
        rrect(stack, R_PANEL, alpha(p.panel, 0.82));
        rrect_lines(stack, R_PANEL, 1.0, p.line);
        self.ui_rects.push(stack);
        text(
            &self.fonts.mono,
            "ASSEMBLY",
            stack.x + 12.0,
            stack.y + 19.0,
            10.0,
            p.dim,
        );
        // the spine the parts hang off
        draw_line(
            stack.x + 26.0,
            stack.y + 34.0,
            stack.x + 26.0,
            stack.bottom() - 26.0,
            2.0,
            alpha(PRINT, 0.35),
        );
        let mut rclear = None;
        for (n, slot) in SLOTS.iter().enumerate() {
            let r = Rect::new(
                stack.x + 6.0,
                stack.y + 28.0 + n as f32 * (row_h + 5.0),
                stack.w - 12.0,
                row_h,
            );
            let on = w.slot == n;
            let hot = self.hot(r);
            rrect(
                r,
                R_CTRL,
                if on {
                    mix(p.panel, p.accent, 0.18)
                } else if hot {
                    p.panel
                } else {
                    alpha(p.panel2, 0.95)
                },
            );
            if on {
                rrect_lines(r, R_CTRL, 1.5, p.accent);
            }
            let ic = Rect::new(r.x + 6.0, r.y + 7.0, 32.0, 32.0);
            draw_circle(ic.center().x, ic.center().y, 16.0, p.bg);
            let label = match n {
                3 => "MODIFIER 1",
                4 => "MODIFIER 2",
                5 => "ACCESSORY 1",
                6 => "ACCESSORY 2",
                _ => match slot {
                    Slot::Mount => "MOUNT",
                    Slot::Head => "HEAD",
                    _ => "BARREL",
                },
            };
            text(&self.fonts.mono, label, r.x + 46.0, r.y + 16.0, 8.5, p.dim);
            match w.build.parts[n] {
                Some(i) => {
                    let pt = part(i);
                    part_icon(ic.inset(6.0), *slot, pt.tier, tier_col(pt.tier));
                    text(
                        &self.fonts.head,
                        &fit(&self.fonts.head, pt.name, r.w - 52.0, 13.0),
                        r.x + 46.0,
                        r.y + 33.0,
                        13.0,
                        p.text,
                    );
                    // a matching pair stacks
                    let pair = match n {
                        3 => Some(4),
                        4 => Some(3),
                        5 => Some(6),
                        6 => Some(5),
                        _ => None,
                    };
                    if pair.is_some_and(|q| w.build.parts[q] == Some(i)) {
                        let s = "×2";
                        let sw = text_w(&self.fonts.mono, s, 9.5);
                        rrect(
                            Rect::new(r.right() - sw - 14.0, r.y + 5.0, sw + 8.0, 14.0),
                            4.0,
                            alpha(p.accent, 0.18),
                        );
                        text(
                            &self.fonts.mono,
                            s,
                            r.right() - sw - 10.0,
                            r.y + 15.5,
                            9.5,
                            p.accent,
                        );
                    }
                    if self.stock(camp.as_deref(), i) != Stock::Owned {
                        text(
                            &self.fonts.mono,
                            "NOT OWNED",
                            r.right() - 62.0,
                            r.y + 16.0,
                            8.5,
                            p.bad,
                        );
                    }
                    if hot {
                        self.tip = Some(format!("{} · right-click to take it off", parts::tip(i)));
                    }
                }
                None => {
                    part_icon(ic.inset(6.0), *slot, 0, alpha(p.dim, 0.4));
                    text(
                        &self.fonts.head,
                        if n < 3 { "Needed" } else { "Empty" },
                        r.x + 46.0,
                        r.y + 33.0,
                        13.0,
                        if n < 3 { p.bad } else { p.dim },
                    );
                }
            }
            self.ui_rects.push(r);
            if self.pressed(hot) {
                w.pick_slot(n);
            }
            if hot && n >= 3 && is_mouse_button_pressed(MouseButton::Right) {
                rclear = Some(n);
            }
        }
        if let Some(n) = rclear {
            w.build.parts[n] = None;
            self.click();
        }

        // dragging the turntable round, scrolling to zoom, the bucket
        let m = self.mouse();
        let on_stage =
            view.contains(m) && !self.blocked && !self.ui_rects.iter().any(|r| r.contains(m));
        if on_stage {
            let (_, wy) = mouse_wheel();
            if wy != 0.0 {
                w.dist = (w.dist * if wy > 0.0 { 0.9 } else { 1.1 }).clamp(14.0, 60.0);
            }
            if is_mouse_button_pressed(MouseButton::Left) {
                if let Some(col) = w.bucket {
                    // the bucket fills the part of the turret nearest the click
                    let mx = rcustom::preview_matrix(w.yaw, w.pitch, w.dist, view.w / view.h);
                    let zone = rcustom::zones(&spec, pose)
                        .iter()
                        .enumerate()
                        .filter_map(|(z, q)| {
                            let c = mx * q.extend(1.0);
                            (c.w > 0.01).then(|| {
                                let n = c.truncate() / c.w;
                                (
                                    z,
                                    vec2(
                                        view.x + (n.x + 1.0) / 2.0 * view.w,
                                        view.y + (1.0 - n.y) / 2.0 * view.h,
                                    )
                                    .distance(m),
                                )
                            })
                        })
                        .min_by(|a, b| a.1.total_cmp(&b.1))
                        .map(|z| z.0);
                    if let Some(z) = zone {
                        w.build.paint[z] = col;
                        self.click();
                    }
                } else {
                    w.turning = Some(m);
                }
            }
        }
        if let Some(m0) = w.turning {
            if is_mouse_button_down(MouseButton::Left) {
                w.yaw -= (m.x - m0.x) * 0.01;
                w.pitch = (w.pitch + (m.y - m0.y) * 0.006).clamp(0.08, 1.35);
                w.turning = Some(m);
            } else {
                w.turning = None;
            }
        }
        if (is_mouse_button_pressed(MouseButton::Right) && rclear.is_none())
            || (w.bucket.is_some() && is_key_pressed(KeyCode::Escape))
        {
            w.bucket = None;
        }

        // ---- the paint shelf along the foot of the hall
        let pr = Rect::new(
            stage.x + 10.0,
            stage.bottom() - shelf_h - 4.0,
            stage.w - 20.0,
            shelf_h,
        );
        self.panel(pr);
        let mut pa = pr.pad(14.0, 10.0);
        let hr = pa.cut_top(16.0, 8.0);
        text(
            &self.fonts.mono,
            "PAINT BUCKET",
            hr.x,
            hr.y + 11.0,
            10.0,
            p.dim,
        );
        let hint = if w.bucket.is_some() {
            "Click the body, head or barrel to fill it · right-click to put the bucket down"
        } else {
            "Pick a colour, then click the turret · drag to turn it, scroll to zoom"
        };
        text(
            &self.fonts.mono,
            &fit(&self.fonts.mono, hint, hr.w - 110.0, 10.0),
            hr.x + 104.0,
            hr.y + 11.0,
            10.0,
            if w.bucket.is_some() { p.accent } else { p.dim },
        );
        let zones = pa.cut_right(234.0, 14.0);
        let per = 10;
        let sz = ((pa.w - (per - 1) as f32 * 6.0) / per as f32).min(24.0);
        for (n, c) in PAINTS.iter().enumerate() {
            let r = Rect::new(
                pa.x + (n % per) as f32 * (sz + 6.0),
                pa.y + (n / per) as f32 * (sz + 6.0),
                sz,
                sz,
            );
            let col = rgb(c[0], c[1], c[2]);
            draw_circle(r.center().x, r.center().y, sz / 2.0, col);
            draw_circle_lines(r.center().x, r.center().y, sz / 2.0, 1.0, p.line);
            let hot = self.hot(r);
            if w.bucket == Some(*c) {
                draw_circle_lines(r.center().x, r.center().y, sz / 2.0 + 3.0, 2.0, p.accent);
            } else if hot {
                draw_circle_lines(r.center().x, r.center().y, sz / 2.0 + 2.0, 1.5, p.text);
            }
            self.ui_rects.push(r);
            if self.pressed(hot) {
                w.bucket = if w.bucket == Some(*c) { None } else { Some(*c) };
            }
        }
        // the three parts' colours: click one with the bucket to fill it too
        for (z, r) in zones.cols(3, 6.0).into_iter().enumerate() {
            let c = w.build.paint[z];
            let hot = self.hot(r);
            rrect(
                r,
                R_CTRL,
                if hot {
                    mix(p.panel2, p.text, 0.08)
                } else {
                    p.panel2
                },
            );
            rrect(
                Rect::new(r.x + 6.0, r.y + 6.0, r.w - 12.0, r.h - 28.0),
                5.0,
                rgb(c[0], c[1], c[2]),
            );
            let zw = text_w(&self.fonts.head, ZONE_NAMES[z], 12.0);
            text(
                &self.fonts.head,
                ZONE_NAMES[z],
                r.x + (r.w - zw) / 2.0,
                r.bottom() - 7.0,
                12.0,
                p.text,
            );
            self.ui_rects.push(r);
            if self.pressed(hot) {
                if let Some(col) = w.bucket {
                    w.build.paint[z] = col;
                } else {
                    w.bucket = Some(c);
                }
            }
        }

        // ---- right: the engineer's report, then the saved turrets
        self.panel(right);
        let mut a = right.pad(14.0, 12.0);
        let y = self.heading("Engineer's report", a.x, a.y);
        a.cut_top(y - a.y, 0.0);
        // hovering a part shows what it would change
        let alt = hover.map(|h| {
            let mut b = w.build.clone();
            b.parts[into] = Some(h);
            parts::spec_raw(&b)
        });
        let lines = parts::stat_lines(&spec);
        let alt_lines = alt.as_ref().map(parts::stat_lines);
        let half = (a.w - 10.0) / 2.0;
        let rows = lines.len().div_ceil(2);
        for (n, (k, v)) in lines.iter().enumerate() {
            let (cx, cy) = (
                a.x + (n / rows) as f32 * (half + 10.0),
                a.y + (n % rows) as f32 * 31.0,
            );
            text(
                &self.fonts.mono,
                &k.to_uppercase(),
                cx,
                cy + 9.0,
                9.0,
                p.dim,
            );
            let nv = alt_lines
                .as_ref()
                .and_then(|l| l.iter().find(|(kk, _)| kk == k))
                .map(|x| x.1.clone());
            match nv.filter(|nv| nv != v) {
                Some(nv) => {
                    text(&self.fonts.head, v, cx, cy + 25.0, 13.5, p.dim);
                    let vw = text_w(&self.fonts.head, v, 13.5);
                    text(
                        &self.fonts.head,
                        &format!(" → {}", nv),
                        cx + vw,
                        cy + 25.0,
                        13.5,
                        p.accent,
                    );
                }
                None => text(&self.fonts.head, v, cx, cy + 25.0, 13.5, p.text),
            }
        }
        a.cut_top(rows as f32 * 31.0 + 6.0, 0.0);
        // load bar
        let lt = a.cut_top(12.0, 4.0);
        text(&self.fonts.mono, "LOAD", lt.x, lt.y + 9.0, 9.0, p.dim);
        let ls = format!("{:.1} / {:.0}", spec.load, spec.cap);
        let lw = text_w(&self.fonts.mono, &ls, 9.5);
        text(
            &self.fonts.mono,
            &ls,
            lt.right() - lw,
            lt.y + 9.0,
            9.5,
            p.dim,
        );
        let lr = a.cut_top(8.0, 6.0);
        let f = spec.load / spec.cap.max(1.0);
        self.progress(
            lr,
            f.min(1.0),
            if f > 1.0 {
                p.bad
            } else if f > 0.85 {
                p.warn
            } else {
                p.good
            },
        );
        let status = match &problem {
            Some(s) => (s.clone(), p.bad),
            None if !self.build_usable_with(camp.as_deref(), &w.build) => {
                ("Has parts you don't own yet".into(), p.warn)
            }
            None => (format!("Ready · {}", aim_text(&spec)), p.good),
        };
        let sr = a.cut_top(26.0, 10.0);
        rrect(sr, R_CTRL, alpha(status.1, 0.1));
        draw_circle(sr.x + 12.0, sr.center().y, 4.0, status.1);
        text(
            &self.fonts.head,
            &fit(&self.fonts.head, &status.0, sr.w - 30.0, 13.0),
            sr.x + 24.0,
            sr.y + 17.5,
            13.0,
            status.1,
        );

        // the library: click to load, "Use" puts it on the build bar in a game
        let y = self.heading(
            &format!(
                "Saved turrets · on the build bar {}/{}",
                self.library.deployed().count(),
                parts::DEPLOY_MAX
            ),
            a.x,
            a.y,
        );
        a.cut_top(y - a.y, 0.0);
        let share = a.cut_bottom(30.0, 8.0).cols(3, 6.0);
        let list = a;
        let lh = 42.0;
        let total = self.library.builds.len() as f32 * lh;
        if self.hot(list) {
            let (_, wy) = mouse_wheel();
            w.lib_scroll -= wy.signum() * lh * 1.5;
        }
        w.lib_scroll = w.lib_scroll.clamp(0.0, (total - list.h).max(0.0));
        let mut load = None;
        let mut star = None;
        let mut del = None;
        for n in 0..self.library.builds.len() {
            let r = Rect::new(
                list.x,
                list.y + n as f32 * lh - w.lib_scroll,
                list.w,
                lh - 4.0,
            );
            if r.y < list.y - 1.0 || r.bottom() > list.bottom() + 1.0 {
                continue;
            }
            let b = &self.library.builds[n];
            let on = w.sel == Some(n);
            let usable = self.build_usable_with(camp.as_deref(), b);
            let hot = self.hot(r);
            rrect(
                r,
                R_CTRL,
                if on {
                    mix(p.panel2, p.accent, 0.18)
                } else if hot {
                    mix(p.panel2, p.text, 0.05)
                } else {
                    p.panel2
                },
            );
            if on {
                rrect_lines(r, R_CTRL, 1.0, alpha(p.accent, 0.7));
            }
            // paint chips
            for z in 0..3 {
                let c = b.paint[z];
                rrect(
                    Rect::new(r.x + 8.0 + z as f32 * 7.0, r.y + 9.0, 6.0, r.h - 18.0),
                    2.0,
                    rgb(c[0], c[1], c[2]),
                );
            }
            let price = parts::spec_raw(b).cost;
            text(
                &self.fonts.head,
                &fit(&self.fonts.head, &b.name, r.w - 154.0, 13.5),
                r.x + 36.0,
                r.y + 17.0,
                13.5,
                if usable { p.text } else { p.dim },
            );
            let line = if usable {
                format!("${:.0}", price)
            } else {
                format!("${:.0} · locked parts", price)
            };
            text(
                &self.fonts.mono,
                &line,
                r.x + 36.0,
                r.y + 31.0,
                10.0,
                if usable { p.dim } else { p.warn },
            );
            let ur = Rect::new(r.right() - 102.0, r.y + 6.0, 66.0, r.h - 12.0);
            let dr = Rect::new(r.right() - 32.0, r.y + 6.0, 26.0, r.h - 12.0);
            let dep = self.library.deploy[n];
            if self.button(ur, if dep { "In use" } else { "Use" }, true, dep) {
                star = Some(n);
            }
            if self.hot(ur) {
                self.tip = Some(if dep {
                    "On the build bar in a game. Click to take it off.".into()
                } else {
                    "Put it on the build bar in a game (in the campaign, once every part is owned)"
                        .into()
                });
            }
            if self.button(dr, "×", true, false) {
                del = Some(n);
            }
            if self.hot(dr) {
                self.tip = Some("Delete this saved turret".into());
            }
            self.ui_rects.push(r);
            if hot && !self.hot(ur) && !self.hot(dr) && self.pressed(true) {
                load = Some(n);
            }
        }
        if self.library.builds.is_empty() {
            text(
                &self.fonts.mono,
                "Nothing saved yet: Save puts this turret here",
                list.x,
                list.y + 14.0,
                10.5,
                p.dim,
            );
        }
        if let Some(n) = star {
            let on = !self.library.deploy[n];
            if on && self.library.deployed().count() >= parts::DEPLOY_MAX {
                w.note = (
                    format!("At most {} turrets can be in use", parts::DEPLOY_MAX),
                    2.5,
                );
            } else {
                self.library.deploy[n] = on;
                self.library.save();
            }
        }
        if let Some(n) = load {
            w.build = self.library.builds[n].clone();
            w.sel = Some(n);
        }
        if let Some(n) = del {
            self.library.remove(n);
            self.library.save();
            w.sel = w.sel.and_then(|s| {
                if s == n {
                    None
                } else if s > n {
                    Some(s - 1)
                } else {
                    Some(s)
                }
            });
        }
        // sharing
        if self.button(share[0], "Copy code", true, false) {
            macroquad::miniquad::window::clipboard_set(&w.build.code());
            w.note = (
                "Build code copied: paste it anywhere to share it".into(),
                2.5,
            );
        }
        if self.hot(share[0]) {
            self.tip = Some(
                "Copies this build as a line of text. Anyone can paste it into their garage."
                    .into(),
            );
        }
        if self.button(share[1], "Paste code", true, false) {
            let t = macroquad::miniquad::window::clipboard_get().unwrap_or_default();
            let n = self.library.import(&t);
            if n > 0 {
                if let Some(b) = t
                    .lines()
                    .filter_map(|l| Build::from_code(l.split_once('|').map(|x| x.1).unwrap_or(l)))
                    .find(|b| b.parts[0].is_some())
                {
                    w.sel = self.library.builds.iter().position(|x| x.name == b.name);
                    w.build = b;
                }
                self.library.save();
                w.note = (
                    format!("Imported {} build{}", n, if n == 1 { "" } else { "s" }),
                    2.5,
                );
            } else {
                w.note = ("No build code on the clipboard".into(), 2.5);
            }
        }
        if self.button(share[2], "Import files", true, false) {
            let n = self.library.import_files();
            self.library.save();
            w.note = if n > 0 {
                (format!("Imported {} builds", n), 2.5)
            } else {
                ("Put .turrets files (another player's turrets.txt, renamed) in the data folder or data/import".into(), 4.0)
            };
        }
        if self.hot(share[2]) {
            self.tip = Some("Imports every build from .turrets files in the data folder, and any .txt in data/import: another player's library or a backup.".into());
        }

        // a note across the bottom of the pad
        if w.note.1 > 0.0 {
            w.note.1 -= dt;
            let tw = text_w(&self.fonts.head, &w.note.0, 15.0) + 28.0;
            let r = Rect::new(view.center().x - tw / 2.0, view.bottom() - 42.0, tw, 32.0);
            ui::shadow(r, R_CTRL, p.shadow);
            rrect(r, R_CTRL, p.panel);
            rrect_lines(r, R_CTRL, 1.0, p.accent);
            text(
                &self.fonts.head,
                &w.note.0,
                r.x + 14.0,
                r.y + 21.0,
                15.0,
                p.text,
            );
        }
        // the bucket rides along with the pointer
        if let Some(c) = w.bucket {
            let m = self.mouse() + vec2(18.0, 14.0);
            draw_rectangle(m.x, m.y + 4.0, 16.0, 13.0, rgb(c[0], c[1], c[2]));
            draw_rectangle_lines(m.x, m.y + 4.0, 16.0, 13.0, 1.5, p.text);
            draw_line(m.x - 1.0, m.y + 4.0, m.x + 8.0, m.y - 3.0, 1.5, p.text);
            draw_line(m.x + 8.0, m.y - 3.0, m.x + 17.0, m.y + 4.0, 1.5, p.text);
        }
        leave
    }

    fn build_usable_with(&self, c: Option<&Campaign>, b: &Build) -> bool {
        c.is_none_or(|c| b.part_ids().all(|i| c.part_owned(self.me(), i)))
    }

    /// Strength, as five little bars.
    fn tier_pips(&self, x: f32, y: f32, tier: u8) {
        let p = pal(false);
        for k in 0..5u8 {
            let c = if k < tier {
                tier_col(k + 1)
            } else {
                alpha(p.text, 0.12)
            };
            draw_rectangle(x + k as f32 * 7.0, y, 5.0, 4.0, c);
        }
    }

    /// The assembly hall: pale steel walls, the great door open on the city seen from above, a
    /// service tower and crane by the pad, blueprint lines on the floor.
    fn garage_backdrop(&self, scr: Rect, stage: Rect, view: Rect) {
        ui::vgrad(scr, HALL_TOP, HALL_LOW);
        let floor_y = view.y + view.h * 0.7;
        let cx = view.center().x;
        // tall wall panels and girders
        let pw = 120.0;
        let mut x = stage.x - (stage.x % pw);
        while x < stage.right() {
            draw_line(
                x,
                stage.y,
                x,
                floor_y,
                1.0,
                Color::new(0.2, 0.3, 0.45, 0.07),
            );
            x += pw;
        }
        for k in 1..4 {
            let y = stage.y + (floor_y - stage.y) * k as f32 / 4.0;
            draw_rectangle(
                stage.x,
                y - 2.0,
                stage.w,
                4.0,
                Color::new(0.2, 0.3, 0.45, 0.05),
            );
        }
        // the great door, open on the city below
        let door = Rect::new(
            cx - view.w * 0.32,
            view.y + view.h * 0.1,
            view.w * 0.64,
            floor_y - view.y - view.h * 0.1,
        );
        city_view(door, self.clock);
        draw_rectangle_lines(
            door.x - 6.0,
            door.y - 6.0,
            door.w + 12.0,
            door.h + 6.0,
            6.0,
            STEEL_LT,
        );
        draw_rectangle(door.x - 14.0, door.y - 22.0, door.w + 28.0, 16.0, STEEL_MD);
        for k in 0..((door.w + 28.0) / 22.0) as i32 {
            draw_rectangle(
                door.x - 10.0 + k as f32 * 22.0,
                door.y - 18.0,
                10.0,
                8.0,
                STEEL_LT,
            );
        }
        // the floor, a few shades darker, with blueprint lines running to the pad
        ui::vgrad(
            Rect::new(stage.x, floor_y, stage.w, stage.bottom() - floor_y),
            Color::new(0.80, 0.83, 0.87, 1.0),
            Color::new(0.72, 0.76, 0.81, 1.0),
        );
        draw_line(stage.x, floor_y, stage.right(), floor_y, 2.0, STEEL_LT);
        for k in -8..=8 {
            let bx = cx + k as f32 * view.w * 0.14;
            draw_line(
                cx + (bx - cx) * 0.35,
                floor_y,
                bx,
                stage.bottom(),
                1.0,
                alpha(PRINT, 0.10),
            );
        }
        let mut d = 6.0;
        let mut y = floor_y + d;
        while y < stage.bottom() {
            draw_line(stage.x, y, stage.right(), y, 1.0, alpha(PRINT, 0.08));
            d *= 1.35;
            y += d;
        }
        // the service tower: a lattice of white steel up the left of the pad
        let tx = view.x + view.w * 0.14;
        let (t0, t1) = (tx - 18.0, tx + 18.0);
        let tt = view.y + view.h * 0.06;
        draw_rectangle(t0 - 3.0, tt, 6.0, floor_y - tt, STEEL_MD);
        draw_rectangle(t1 - 3.0, tt, 6.0, floor_y - tt, STEEL_MD);
        let mut y = tt;
        while y + 30.0 <= floor_y {
            draw_line(t0, y, t1, y + 30.0, 2.0, STEEL_LT);
            draw_line(t1, y, t0, y + 30.0, 2.0, STEEL_LT);
            draw_line(t0, y, t1, y, 2.0, STEEL_MD);
            y += 30.0;
        }
        // its crane arm across the top, a hook swinging over the pad
        let ay = tt + 4.0;
        draw_rectangle(t0 - 6.0, ay - 6.0, cx - t0 + view.w * 0.1, 10.0, STEEL_MD);
        let mut x = t1;
        while x < cx + view.w * 0.1 - 16.0 {
            draw_line(x, ay - 6.0, x + 12.0, ay + 4.0, 1.5, STEEL_LT);
            x += 16.0;
        }
        let hx = cx + (self.clock * 0.4).sin() * 2.0;
        draw_line(hx, ay + 4.0, hx, ay + 36.0, 1.5, STEEL_MD);
        draw_circle_lines(hx, ay + 42.0, 5.0, 2.0, STEEL_MD);
        // a warning lamp on top of the tower
        let blink = 0.5 + 0.5 * (self.clock * 3.0).sin();
        draw_circle(
            tx,
            tt - 6.0,
            4.0,
            Color::new(1.0, 0.45, 0.1, 0.6 + 0.4 * blink),
        );
        // ceiling lights
        let mut x = stage.x + 60.0;
        while x < stage.right() {
            draw_rectangle(
                x - 18.0,
                stage.y + 4.0,
                36.0,
                5.0,
                Color::new(1.0, 1.0, 1.0, 0.9),
            );
            draw_triangle(
                vec2(x - 18.0, stage.y + 9.0),
                vec2(x + 18.0, stage.y + 9.0),
                vec2(x, stage.y + 9.0 + view.h * 0.25),
                Color::new(1.0, 1.0, 1.0, 0.12),
            );
            x += 140.0;
        }
    }
}

/// The view out of the hall's door: the city from high above, the way the game's maps draw it
/// (pale land, white streets, parks and a river), with clouds drifting across.
fn city_view(r: Rect, t: f32) {
    draw_rectangle(r.x, r.y, r.w, r.h, rgb(232, 230, 222));
    // a little hash for the blocks
    let h = |a: i32, b: i32| -> f32 {
        let v = (a.wrapping_mul(374761393) ^ b.wrapping_mul(668265263)) as u32;
        let v = (v ^ (v >> 13)).wrapping_mul(1274126177);
        (v >> 8) as f32 / 16777216.0
    };
    let bs = 34.0;
    let (nx, ny) = ((r.w / bs) as i32 + 1, (r.h / bs) as i32 + 1);
    for j in 0..ny {
        for i in 0..nx {
            let x = r.x + i as f32 * bs + 3.0;
            let y = r.y + j as f32 * bs + 3.0;
            let w = (bs - 6.0).min(r.right() - x);
            let hh = (bs - 6.0).min(r.bottom() - y);
            if w <= 0.0 || hh <= 0.0 {
                continue;
            }
            let v = h(i, j);
            let c = if v < 0.14 {
                rgb(200, 230, 190)
            } else if v < 0.5 {
                rgb(240, 238, 232)
            } else {
                rgb(222, 219, 210)
            };
            draw_rectangle(x, y, w, hh, c);
            // buildings in the block
            if v >= 0.14 {
                for q in 0..3 {
                    let bw = 6.0 + h(i * 7 + q, j * 3) * 8.0;
                    let bx = x + 2.0 + h(i + q, j * 5) * (w - bw - 2.0).max(0.0);
                    let by = y + 2.0 + q as f32 * (hh - 4.0) / 3.0;
                    let bh = ((hh - 4.0) / 3.0 - 2.0).max(1.0);
                    if bx + bw <= x + w && by + bh <= y + hh {
                        draw_rectangle(bx, by, bw, bh, rgb(214, 210, 200));
                    }
                }
            }
        }
    }
    // a river winding down through it
    let mut y = r.y;
    while y < r.bottom() {
        let f = (y - r.y) / r.h;
        let x0 = r.x + r.w * (0.62 + 0.12 * (f * 4.0).sin());
        let wd = r.w * 0.07;
        let x1 = (x0 + wd).min(r.right());
        if x0 < r.right() {
            draw_rectangle(x0, y, x1 - x0, 4.0, rgb(170, 210, 240));
        }
        y += 4.0;
    }
    // two big roads
    draw_rectangle(r.x, r.y + r.h * 0.55, r.w, 5.0, rgb(255, 236, 170));
    draw_rectangle(r.x + r.w * 0.3, r.y, 5.0, r.h, rgb(255, 255, 255));
    // clouds, slow
    for k in 0..3 {
        let w = r.w * 0.28;
        let x = r.x + ((t * 6.0 + k as f32 * r.w * 0.45) % (r.w + w)) - w;
        let y = r.y + r.h * (0.2 + 0.25 * k as f32);
        let x0 = x.max(r.x);
        let x1 = (x + w).min(r.right());
        if x1 > x0 {
            draw_rectangle(x0, y, x1 - x0, r.h * 0.08, Color::new(1.0, 1.0, 1.0, 0.45));
        }
    }
    // the haze of height
    ui::vgrad(
        Rect::new(r.x, r.y, r.w, r.h * 0.4),
        Color::new(0.85, 0.92, 1.0, 0.65),
        Color::new(0.85, 0.92, 1.0, 0.0),
    );
}

/// Strength colours: map greens through to orange.
fn tier_col(t: u8) -> Color {
    match t {
        0 | 1 => rgb(64, 160, 90),
        2 => rgb(40, 150, 170),
        3 => rgb(26, 115, 232),
        4 => rgb(150, 90, 210),
        _ => rgb(232, 128, 0),
    }
}

/// A small line drawing of a kind of part in `r`: a mount's plinth, a head's sight, a barrel, a
/// round of ammunition for modifiers, a cog for accessories. Stronger parts get more detail.
fn part_icon(r: Rect, k: Slot, tier: u8, c: Color) {
    let (cx, cy, s) = (r.center().x, r.center().y, r.w.min(r.h) / 2.0);
    let lw = 1.8;
    match k {
        Slot::Mount => {
            let pts = [
                vec2(cx - s, cy + s * 0.7),
                vec2(cx + s, cy + s * 0.7),
                vec2(cx + s * 0.6, cy - s * 0.1),
                vec2(cx - s * 0.6, cy - s * 0.1),
            ];
            for i in 0..4 {
                draw_line(
                    pts[i].x,
                    pts[i].y,
                    pts[(i + 1) % 4].x,
                    pts[(i + 1) % 4].y,
                    lw,
                    c,
                );
            }
            draw_rectangle(cx - s * 0.3, cy - s * 0.6, s * 0.6, s * 0.5, c);
            if tier >= 3 {
                draw_line(
                    cx - s * 0.8,
                    cy + s * 0.3,
                    cx + s * 0.8,
                    cy + s * 0.3,
                    lw,
                    c,
                );
            }
        }
        Slot::Head => {
            draw_circle_lines(cx, cy, s * 0.7, lw, c);
            draw_line(cx - s, cy, cx + s, cy, lw * 0.8, c);
            draw_line(cx, cy - s, cx, cy + s, lw * 0.8, c);
            draw_circle(cx, cy, s * 0.15, c);
            if tier >= 3 {
                draw_circle_lines(cx, cy, s * 0.35, lw * 0.8, c);
            }
        }
        Slot::Barrel => {
            draw_rectangle(cx - s, cy - s * 0.35, s * 0.8, s * 0.7, c);
            let n = 1 + (tier as i32 / 2).min(2);
            for b in 0..n {
                let off = (b as f32 - (n - 1) as f32 / 2.0) * s * 0.3;
                draw_rectangle(cx - s * 0.2, cy + off - s * 0.08, s * 1.2, s * 0.16, c);
            }
        }
        Slot::Mod => {
            draw_rectangle(cx - s * 0.3, cy - s * 0.1, s * 0.6, s * 1.0, c);
            draw_triangle(
                vec2(cx - s * 0.3, cy - s * 0.1),
                vec2(cx + s * 0.3, cy - s * 0.1),
                vec2(cx, cy - s * 0.95),
                c,
            );
            if tier >= 3 {
                draw_line(
                    cx - s * 0.6,
                    cy + s * 0.5,
                    cx - s * 0.6,
                    cy - s * 0.4,
                    lw,
                    c,
                );
                draw_line(
                    cx + s * 0.6,
                    cy + s * 0.5,
                    cx + s * 0.6,
                    cy - s * 0.4,
                    lw,
                    c,
                );
            }
        }
        Slot::Gear => {
            draw_circle_lines(cx, cy, s * 0.55, lw * 1.2, c);
            draw_circle(cx, cy, s * 0.2, c);
            for t in 0..8 {
                let a = t as f32 / 8.0 * std::f32::consts::TAU;
                let (dx, dy) = (a.cos(), a.sin());
                draw_line(
                    cx + dx * s * 0.6,
                    cy + dy * s * 0.6,
                    cx + dx * s * 0.95,
                    cy + dy * s * 0.95,
                    lw * 1.4,
                    c,
                );
            }
        }
    }
}

/// How the head picks targets, in a few words.
fn aim_text(s: &parts::Spec) -> &'static str {
    use parts::Aim::*;
    match s.aim {
        Near => "shoots the nearest",
        Dense => "shoots the thickest crowd",
        Far => "shoots the furthest",
        Guard => "guards your buildings",
        Hq => "guards the HQ",
        Lead => "leads its targets",
        Scatter => "spreads its fire",
        Sweep => "sweeps round",
        Split => "nearest, then crowds",
        Smart => "picks the worst threat",
    }
}

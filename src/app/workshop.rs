//! The garage: a dim industrial workshop where turrets are put together from parts on a
//! turntable, painted with a bucket, named, and saved to the build library. Opened from the main
//! menu it's a free builder with every part on the shelf; from the campaign screen parts have to
//! be researched (research points) and bought (money from cities held) before they go on.

use super::*;
use crate::garage::{self as parts, part, Build, Slot, PARTS, SLOTS};
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

pub(super) struct Workshop {
    pub build: Build,
    /// opened from the campaign screen (parts have to be researched and bought)
    pub campaign: bool,
    /// which of the seven slots the parts list shows
    slot: usize,
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
        let back = self.workshop_frame(&mut w, camp.as_mut());
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
        let p = pal(self.dark);
        let dt = get_frame_time().min(0.1);
        let scr = ui::screen();
        let hud = ui::hud();
        let me = self.me();
        let spec = parts::spec_raw(&w.build);
        let problem = w.build.problem();

        // ---- layout, in points
        let top = Rect::new(hud.x + 16.0, 12.0, hud.w - 32.0, 56.0);
        let colw = (hud.w * 0.27).clamp(320.0, 400.0);
        let left = Rect::new(
            hud.x + 16.0,
            top.bottom() + 12.0,
            colw,
            scr.h - top.bottom() - 28.0,
        );
        let right = Rect::new(hud.right() - 16.0 - colw, left.y, colw, left.h);
        let mid = Rect::new(
            left.right() + 16.0,
            left.y,
            right.x - left.right() - 32.0,
            left.h,
        );
        let stage = Rect::new(mid.x, mid.y + 54.0, mid.w, mid.h - 54.0 - 128.0);

        // ---- the turntable, rendered in pixels before the UI goes on top
        w.fire_t += dt;
        if w.turning.is_none() {
            w.yaw += dt * 0.25;
        }
        let flash = if !problem.is_some() && w.fire_t % 2.4 < 0.12 {
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
            (stage.x * k) as i32,
            (stage.y * k) as i32,
            (stage.w * k).max(8.0) as i32,
            (stage.h * k).max(8.0) as i32,
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

        // ---- the workshop around it
        self.garage_backdrop(scr, stage);
        if let Some(rt) = &w.rt {
            draw_texture_ex(
                &rt.texture,
                stage.x,
                stage.y,
                WHITE,
                DrawTextureParams {
                    dest_size: Some(vec2(stage.w, stage.h)),
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

        // ---- top bar
        self.panel(top);
        let title = if w.campaign {
            "GARAGE"
        } else {
            "GARAGE · FREE BUILDER"
        };
        text(
            &self.fonts.head,
            title,
            top.x + 18.0,
            top.y + 36.0,
            24.0,
            p.text,
        );
        let sub = if w.campaign {
            "Research parts with research points, buy them with money from cities held."
        } else {
            "Every part on the shelf. Builds saved here show up in the campaign garage too."
        };
        text(
            &self.fonts.mono,
            sub,
            top.x + 18.0 + text_w(&self.fonts.head, title, 24.0) + 18.0,
            top.y + 34.0,
            12.0,
            p.dim,
        );
        let backr = Rect::new(top.right() - 120.0, top.y + 11.0, 104.0, 34.0);
        let leave = self.button(backr, "Back", true, false)
            || (!self.blocked
                && !w.naming
                && w.bucket.is_none()
                && is_key_pressed(KeyCode::Escape));
        if let Some(c) = camp.as_deref() {
            let s = format!("${}   {} RP", c.crew(me).money, c.points(me));
            let sw = text_w(&self.fonts.head, &s, 20.0);
            text(
                &self.fonts.head,
                &s,
                backr.x - 20.0 - sw,
                top.y + 35.0,
                20.0,
                p.accent,
            );
        }

        // ---- left: the build's slots, and the parts for the picked one
        self.panel(left);
        let mut a = left.pad(14.0, 12.0);
        let y = self.heading("Build", a.x, a.y);
        a.cut_top(y - a.y, 0.0);
        let row_h = 38.0;
        for (k, slot) in SLOTS.iter().enumerate() {
            let r = a.cut_top(row_h, 4.0);
            let on = w.slot == k;
            let hot = self.hot(r);
            rrect(
                r,
                R_CTRL,
                if on {
                    mix(p.panel2, p.accent, 0.22)
                } else if hot {
                    mix(p.panel2, p.text, 0.06)
                } else {
                    p.panel2
                },
            );
            if on {
                rrect_lines(r, R_CTRL, 1.5, p.accent);
            }
            let label = match k {
                3 => "Modifier 1",
                4 => "Modifier 2",
                5 => "Accessory 1",
                6 => "Accessory 2",
                _ => slot.name(),
            };
            text(
                &self.fonts.mono,
                &label.to_uppercase(),
                r.x + 10.0,
                r.y + 15.0,
                9.5,
                p.dim,
            );
            match w.build.parts[k] {
                Some(i) => {
                    let pt = part(i);
                    text(
                        &self.fonts.head,
                        &fit(&self.fonts.head, pt.name, r.w - 80.0, 14.0),
                        r.x + 10.0,
                        r.y + 31.0,
                        14.0,
                        p.text,
                    );
                    self.tier_pips(r.right() - 62.0, r.y + 22.0, pt.tier);
                    let ok = self.stock(camp.as_deref(), i) == Stock::Owned;
                    if !ok {
                        text(
                            &self.fonts.mono,
                            "not owned",
                            r.right() - 70.0,
                            r.y + 14.0,
                            9.5,
                            p.bad,
                        );
                    }
                }
                None => text(
                    &self.fonts.head,
                    if k < 3 { "— needed —" } else { "empty" },
                    r.x + 10.0,
                    r.y + 31.0,
                    14.0,
                    if k < 3 { p.bad } else { p.dim },
                ),
            }
            self.ui_rects.push(r);
            if self.pressed(hot) {
                w.slot = k;
                w.scroll = 0.0;
            }
        }
        a.cut_top(6.0, 0.0);
        let kind = SLOTS[w.slot];
        let y = self.heading(
            &format!(
                "{} · {}",
                kind.plural(),
                PARTS.iter().filter(|q| q.slot() == kind).count()
            ),
            a.x,
            a.y,
        );
        a.cut_top(y - a.y, 0.0);
        let list = a;
        let mut ids: Vec<Option<u16>> = (0..PARTS.len() as u16)
            .filter(|i| part(*i).slot() == kind)
            .map(Some)
            .collect();
        ids.sort_by_key(|i| i.map(|i| (part(i).tier, part(i).fam as u8, part(i).lvl)));
        if w.slot >= 3 {
            ids.insert(0, None);
        }
        let lh = 44.0;
        let total = ids.len() as f32 * lh;
        let hot_list = self.hot(list);
        if hot_list {
            let (_, wy) = mouse_wheel();
            w.scroll -= wy.signum() * lh * 1.5;
        }
        w.scroll = w.scroll.clamp(0.0, (total - list.h).max(0.0));
        let mut hover: Option<Option<u16>> = None;
        for (n, id) in ids.iter().enumerate() {
            let r = Rect::new(list.x, list.y + n as f32 * lh - w.scroll, list.w, lh - 4.0);
            if r.bottom() < list.y || r.y > list.bottom() {
                continue;
            }
            // half-shown rows at the edges are left out rather than clipped
            if r.y < list.y - 1.0 || r.bottom() > list.bottom() + 1.0 {
                continue;
            }
            let equipped = w.build.parts[w.slot] == *id;
            let hot = self.hot(r);
            let st = id.map_or(Stock::Owned, |i| self.stock(camp.as_deref(), i));
            rrect(
                r,
                R_CTRL,
                if equipped {
                    mix(p.panel2, p.accent, 0.3)
                } else if hot {
                    mix(p.panel2, p.text, 0.07)
                } else {
                    alpha(p.panel2, 0.6)
                },
            );
            let Some(i) = *id else {
                text(
                    &self.fonts.head,
                    "Empty slot",
                    r.x + 10.0,
                    r.y + 25.0,
                    14.0,
                    p.dim,
                );
                if hot {
                    hover = Some(None);
                }
                if self.pressed(hot) {
                    w.build.parts[w.slot] = None;
                }
                continue;
            };
            let pt = part(i);
            let fc = if st == Stock::Owned { p.text } else { p.dim };
            text(
                &self.fonts.head,
                &fit(&self.fonts.head, pt.name, r.w - 150.0, 14.0),
                r.x + 10.0,
                r.y + 18.0,
                14.0,
                fc,
            );
            self.tier_pips(r.x + 10.0, r.y + 26.0, pt.tier);
            let wt = if pt.slot() == Slot::Mount {
                let mut b = Build::starter();
                b.parts = [Some(i), None, None, None, None, None, None];
                format!("carries {:.0}", parts::spec_raw(&b).cap)
            } else {
                format!("wt {:.1}", pt.weight())
            };
            text(
                &self.fonts.mono,
                &format!("${:.0} · {}", pt.cost(), wt),
                r.x + 60.0,
                r.y + 33.0,
                10.5,
                p.dim,
            );
            // what it takes to get it, on the right
            let br = Rect::new(r.right() - 108.0, r.y + 7.0, 100.0, r.h - 14.0);
            match st {
                Stock::Owned => {
                    if equipped {
                        text(
                            &self.fonts.mono,
                            "FITTED",
                            br.x + 40.0,
                            br.y + 19.0,
                            10.5,
                            p.accent,
                        );
                    }
                }
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
                    "locked",
                    br.x + 46.0,
                    br.y + 19.0,
                    10.5,
                    p.dim,
                ),
            }
            if hot && !self.hot(br) {
                hover = Some(Some(i));
                let need = match st {
                    Stock::Locked => {
                        let prev = parts::needs(i).map(|n| part(n).name).unwrap_or("");
                        format!(" Research the {} first.", prev)
                    }
                    Stock::Research(n) => {
                        format!(" Research it for {} research points, then buy it.", n)
                    }
                    Stock::Buy(m) => format!(" Researched: buy it for ${}.", m),
                    Stock::Owned => String::new(),
                };
                self.tip = Some(format!("{}{}", parts::tip(i), need));
                if self.pressed(true) {
                    if st == Stock::Owned {
                        w.build.parts[w.slot] = Some(i);
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

        // ---- middle: name over the turntable, paint under it
        let name_r = Rect::new(mid.x + mid.w * 0.15, mid.y, mid.w * 0.7, 44.0);
        rrect(name_r, R_CTRL, alpha(p.panel, 0.85));
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
        let nm = format!("{}{}", w.build.name, caret);
        let nw = text_w(&self.fonts.head, &nm, 22.0);
        text(
            &self.fonts.head,
            &nm,
            name_r.x + (name_r.w - nw) / 2.0,
            name_r.y + 30.0,
            22.0,
            p.text,
        );
        if !w.naming {
            text(
                &self.fonts.mono,
                "click to rename",
                name_r.right() - 104.0,
                name_r.bottom() + 14.0,
                10.5,
                p.dim,
            );
        }
        self.ui_rects.push(name_r);
        if self.pressed(self.hot(name_r)) {
            w.naming = true;
        } else if is_mouse_button_pressed(MouseButton::Left) && !self.hot(name_r) {
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

        // dragging the turntable round, scrolling to zoom, the bucket
        let m = self.mouse();
        let on_stage =
            stage.contains(m) && !self.blocked && !self.ui_rects.iter().any(|r| r.contains(m));
        if on_stage {
            let (_, wy) = mouse_wheel();
            if wy != 0.0 {
                w.dist = (w.dist * if wy > 0.0 { 0.9 } else { 1.1 }).clamp(14.0, 60.0);
            }
            if is_mouse_button_pressed(MouseButton::Left) {
                if let Some(col) = w.bucket {
                    // the bucket fills the part of the turret nearest the click
                    let mx = rcustom::preview_matrix(w.yaw, w.pitch, w.dist, stage.w / stage.h);
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
                                        stage.x + (n.x + 1.0) / 2.0 * stage.w,
                                        stage.y + (1.0 - n.y) / 2.0 * stage.h,
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
        if is_mouse_button_pressed(MouseButton::Right)
            || (w.bucket.is_some() && is_key_pressed(KeyCode::Escape))
        {
            w.bucket = None;
        }

        // the paint shelf
        let pr = Rect::new(mid.x, stage.bottom() + 8.0, mid.w, 120.0);
        self.panel(pr);
        let mut pa = pr.pad(14.0, 10.0);
        let hr = pa.cut_top(16.0, 6.0);
        text(
            &self.fonts.mono,
            "PAINT BUCKET",
            hr.x,
            hr.y + 11.0,
            10.5,
            p.dim,
        );
        let hint = if w.bucket.is_some() {
            "Click the body, head or barrel to fill it · right-click to put the bucket down"
        } else {
            "Pick a colour, then click the turret"
        };
        text(
            &self.fonts.mono,
            hint,
            hr.x + 110.0,
            hr.y + 11.0,
            10.5,
            if w.bucket.is_some() { p.accent } else { p.dim },
        );
        let zones = pa.cut_right(170.0, 12.0);
        let per = 10;
        let sz = ((pa.w - (per - 1) as f32 * 6.0) / per as f32).min(30.0);
        for (n, c) in PAINTS.iter().enumerate() {
            let r = Rect::new(
                pa.x + (n % per) as f32 * (sz + 6.0),
                pa.y + (n / per) as f32 * (sz + 6.0),
                sz,
                sz,
            );
            let col = rgb(c[0], c[1], c[2]);
            rrect(r, 5.0, col);
            let hot = self.hot(r);
            if w.bucket == Some(*c) {
                rrect_lines(r.grow(2.0), 6.0, 2.0, p.accent);
            } else if hot {
                rrect_lines(r, 5.0, 1.5, p.text);
            }
            self.ui_rects.push(r);
            if self.pressed(hot) {
                w.bucket = if w.bucket == Some(*c) { None } else { Some(*c) };
            }
        }
        // the three parts' colours: click one with the bucket to fill it too
        for (z, r) in zones.rows(3, 4.0).into_iter().enumerate() {
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
                Rect::new(r.x + 6.0, r.y + 5.0, r.h - 10.0, r.h - 10.0),
                4.0,
                rgb(c[0], c[1], c[2]),
            );
            text(
                &self.fonts.head,
                ZONE_NAMES[z],
                r.x + r.h + 4.0,
                r.y + r.h / 2.0 + 5.0,
                13.0,
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

        // ---- right: what the build adds up to, and the library
        self.panel(right);
        let mut a = right.pad(14.0, 12.0);
        let y = self.heading("Performance", a.x, a.y);
        a.cut_top(y - a.y, 0.0);
        // hovering a part shows what it would change
        let alt = hover.map(|h| {
            let mut b = w.build.clone();
            b.parts[w.slot] = h;
            parts::spec_raw(&b)
        });
        let lines = parts::stat_lines(&spec);
        let alt_lines = alt.as_ref().map(parts::stat_lines);
        let half = (a.w - 10.0) / 2.0;
        let rows = lines.len().div_ceil(2);
        for (n, (k, v)) in lines.iter().enumerate() {
            let (cx, cy) = (
                a.x + (n / rows) as f32 * (half + 10.0),
                a.y + (n % rows) as f32 * 30.0,
            );
            text(
                &self.fonts.mono,
                &k.to_uppercase(),
                cx,
                cy + 10.0,
                9.5,
                p.dim,
            );
            let nv = alt_lines
                .as_ref()
                .and_then(|l| l.iter().find(|(kk, _)| kk == k))
                .map(|x| x.1.clone());
            match nv.filter(|nv| nv != v) {
                Some(nv) => {
                    text(&self.fonts.head, v, cx, cy + 26.0, 14.0, p.dim);
                    let vw = text_w(&self.fonts.head, v, 14.0);
                    text(
                        &self.fonts.head,
                        &format!(" → {}", nv),
                        cx + vw,
                        cy + 26.0,
                        14.0,
                        p.accent,
                    );
                }
                None => text(&self.fonts.head, v, cx, cy + 26.0, 14.0, p.text),
            }
        }
        a.cut_top(rows as f32 * 30.0 + 6.0, 0.0);
        // load bar
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
                p.accent
            },
        );
        let status = match &problem {
            Some(s) => (s.clone(), p.bad),
            None if !self.build_usable_with(camp.as_deref(), &w.build) => {
                ("Has parts you don't own yet".into(), p.warn)
            }
            None => (format!("Ready · {}", aim_text(&spec)), p.good),
        };
        let sr = a.cut_top(18.0, 8.0);
        text(
            &self.fonts.mono,
            &fit(&self.fonts.mono, &status.0, sr.w, 11.5),
            sr.x,
            sr.y + 13.0,
            11.5,
            status.1,
        );

        // save / new / share
        let r1 = a.cut_top(32.0, 6.0).cols(3, 6.0);
        if self.button(r1[0], "Save", problem.is_none(), true) {
            let k = self.library.put(w.build.clone());
            w.sel = Some(k);
            self.library.save();
            w.note = (format!("Saved {}", w.build.name), 2.0);
        }
        if self.button(r1[1], "Save copy", problem.is_none(), false) {
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
        if self.button(r1[2], "New", true, false) {
            w.build = Build {
                name: "New turret".into(),
                ..Build::starter()
            };
            w.sel = None;
        }
        let r2 = a.cut_top(32.0, 10.0).cols(3, 6.0);
        if self.button(r2[0], "Copy code", true, false) {
            macroquad::miniquad::window::clipboard_set(&w.build.code());
            w.note = (
                "Build code copied: paste it anywhere to share it".into(),
                2.5,
            );
        }
        if self.hot(r2[0]) {
            self.tip = Some(
                "Copies this build as a line of text. Anyone can paste it into their garage."
                    .into(),
            );
        }
        if self.button(r2[1], "Paste code", true, false) {
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
        if self.button(r2[2], "Import files", true, false) {
            let n = self.library.import_files();
            self.library.save();
            w.note = if n > 0 {
                (format!("Imported {} builds", n), 2.5)
            } else {
                ("Put .turrets files (another player's turrets.txt, renamed) in the data folder or data/import".into(), 4.0)
            };
        }
        if self.hot(r2[2]) {
            self.tip = Some("Imports every build from .turrets files in the data folder, and any .txt in data/import: another player's library or a backup.".into());
        }

        // the library: click to load, the star puts it on the build bar in a game
        let y = self.heading(
            &format!(
                "Saved turrets · \"Bar\" puts one on the build bar ({} max)",
                parts::DEPLOY_MAX
            ),
            a.x,
            a.y,
        );
        a.cut_top(y - a.y, 0.0);
        let list = a;
        let lh = 40.0;
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
                    mix(p.panel2, p.accent, 0.22)
                } else if hot {
                    mix(p.panel2, p.text, 0.06)
                } else {
                    alpha(p.panel2, 0.6)
                },
            );
            // paint chips
            for z in 0..3 {
                let c = b.paint[z];
                draw_rectangle(
                    r.x + 8.0 + z as f32 * 7.0,
                    r.y + 10.0,
                    6.0,
                    r.h - 20.0,
                    rgb(c[0], c[1], c[2]),
                );
            }
            let price = parts::spec_raw(b).cost;
            text(
                &self.fonts.head,
                &fit(&self.fonts.head, &b.name, r.w - 164.0, 14.0),
                r.x + 36.0,
                r.y + 17.0,
                14.0,
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
                10.5,
                if usable { p.dim } else { p.warn },
            );
            let sr = Rect::new(r.right() - 76.0, r.y + 6.0, 40.0, r.h - 12.0);
            let dr = Rect::new(r.right() - 32.0, r.y + 6.0, 26.0, r.h - 12.0);
            let dep = self.library.deploy[n];
            if self.button(sr, if dep { "Bar" } else { "bar" }, true, dep) {
                star = Some(n);
            }
            if self.hot(sr) {
                self.tip = Some(
                    "On the build bar in a game (in the campaign, once every part is owned)".into(),
                );
            }
            if self.button(dr, "×", true, false) {
                del = Some(n);
            }
            if self.hot(dr) {
                self.tip = Some("Delete this saved turret".into());
            }
            self.ui_rects.push(r);
            if hot && !self.hot(sr) && !self.hot(dr) && self.pressed(true) {
                load = Some(n);
            }
        }
        if let Some(n) = star {
            let on = !self.library.deploy[n];
            if on && self.library.deployed().count() >= parts::DEPLOY_MAX {
                w.note = (
                    format!("At most {} turrets go on the build bar", parts::DEPLOY_MAX),
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

        // a note across the bottom of the turntable
        if w.note.1 > 0.0 {
            w.note.1 -= dt;
            let tw = text_w(&self.fonts.head, &w.note.0, 15.0) + 28.0;
            let r = Rect::new(stage.center().x - tw / 2.0, stage.bottom() - 44.0, tw, 32.0);
            rrect(r, R_CTRL, alpha(p.panel, 0.92));
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
            draw_rectangle_lines(m.x, m.y + 4.0, 16.0, 13.0, 1.5, WHITE);
            draw_line(m.x - 1.0, m.y + 4.0, m.x + 8.0, m.y - 3.0, 1.5, WHITE);
            draw_line(m.x + 8.0, m.y - 3.0, m.x + 17.0, m.y + 4.0, 1.5, WHITE);
        }
        leave
    }

    fn build_usable_with(&self, c: Option<&Campaign>, b: &Build) -> bool {
        c.is_none_or(|c| b.part_ids().all(|i| c.part_owned(self.me(), i)))
    }

    /// Strength, as five little bars.
    fn tier_pips(&self, x: f32, y: f32, tier: u8) {
        let p = pal(self.dark);
        for k in 0..5u8 {
            let c = if k < tier {
                mix(p.accent, rgb(255, 196, 40), k as f32 / 4.0)
            } else {
                alpha(p.text, 0.12)
            };
            draw_rectangle(x + k as f32 * 9.0, y, 7.0, 4.0, c);
        }
    }

    /// The workshop itself: concrete walls, steel columns, a roll-up door behind the turntable,
    /// hazard stripes and work lamps.
    fn garage_backdrop(&self, scr: Rect, stage: Rect) {
        // walls and floor
        ui::vgrad(scr, rgb(22, 24, 28), rgb(12, 13, 16));
        let floor_y = stage.y + stage.h * 0.72;
        ui::vgrad(
            Rect::new(scr.x, floor_y, scr.w, scr.bottom() - floor_y),
            rgb(30, 31, 34),
            rgb(16, 17, 19),
        );
        // concrete wall panels
        let pw = 160.0;
        let mut x = scr.x - (scr.x % pw);
        while x < scr.right() {
            draw_line(x, 0.0, x, floor_y, 1.0, Color::new(1.0, 1.0, 1.0, 0.035));
            x += pw;
        }
        for k in 1..4 {
            let y = floor_y * k as f32 / 4.0;
            draw_line(
                scr.x,
                y,
                scr.right(),
                y,
                1.0,
                Color::new(1.0, 1.0, 1.0, 0.025),
            );
        }
        // the roll-up door behind the turntable
        let door = Rect::new(
            stage.center().x - stage.w * 0.34,
            stage.y + stage.h * 0.08,
            stage.w * 0.68,
            floor_y - stage.y - stage.h * 0.08,
        );
        draw_rectangle(door.x, door.y, door.w, door.h, rgb(40, 43, 48));
        let mut y = door.y + 8.0;
        while y < door.bottom() {
            draw_line(door.x, y, door.right(), y, 1.5, rgb(28, 30, 34));
            draw_line(door.x, y + 2.0, door.right(), y + 2.0, 1.0, rgb(52, 56, 62));
            y += 14.0;
        }
        // its hazard frame
        let stripe = |r: Rect, vertical: bool| {
            draw_rectangle(r.x, r.y, r.w, r.h, rgb(230, 180, 30));
            let n = ((if vertical { r.h } else { r.w }) / 14.0) as i32 + 1;
            for k in 0..n {
                let o = k as f32 * 14.0;
                if vertical {
                    draw_triangle(
                        vec2(r.x, r.y + o),
                        vec2(r.right(), r.y + o + 7.0),
                        vec2(r.x, r.y + o + 7.0),
                        rgb(26, 26, 28),
                    );
                } else {
                    draw_triangle(
                        vec2(r.x + o, r.y),
                        vec2(r.x + o + 7.0, r.y),
                        vec2(r.x + o + 7.0, r.bottom()),
                        rgb(26, 26, 28),
                    );
                }
            }
        };
        stripe(
            Rect::new(door.x - 10.0, door.y - 10.0, door.w + 20.0, 10.0),
            false,
        );
        stripe(Rect::new(door.x - 10.0, door.y, 10.0, door.h), true);
        stripe(Rect::new(door.right(), door.y, 10.0, door.h), true);
        // steel columns either side, an I-beam gantry across the top with a hook
        for cx in [stage.x + 18.0, stage.right() - 18.0] {
            draw_rectangle(cx - 9.0, 0.0, 18.0, floor_y, rgb(54, 58, 66));
            draw_rectangle(cx - 3.0, 0.0, 6.0, floor_y, rgb(40, 43, 50));
            for k in 0..((floor_y / 60.0) as i32) {
                draw_circle(cx, 20.0 + k as f32 * 60.0, 2.0, rgb(80, 86, 96));
            }
        }
        let gy = stage.y + 6.0;
        draw_rectangle(stage.x, gy, stage.w, 12.0, rgb(58, 62, 70));
        draw_rectangle(stage.x, gy + 4.0, stage.w, 4.0, rgb(40, 43, 50));
        let hx = stage.center().x + (self.clock * 0.4).sin() * 2.0;
        draw_line(hx, gy + 12.0, hx, gy + 42.0, 2.0, rgb(90, 94, 102));
        draw_circle_lines(hx, gy + 48.0, 6.0, 2.0, rgb(150, 150, 150));
        // work lamps and their light
        for lx in [stage.x + stage.w * 0.22, stage.right() - stage.w * 0.22] {
            draw_rectangle(lx - 14.0, gy + 12.0, 28.0, 8.0, rgb(30, 32, 36));
            draw_triangle(
                vec2(lx - 12.0, gy + 20.0),
                vec2(lx + 12.0, gy + 20.0),
                vec2(lx + (stage.center().x - lx) * 0.5, floor_y + 30.0),
                Color::new(1.0, 0.92, 0.7, 0.045),
            );
            draw_triangle(
                vec2(lx - 12.0, gy + 20.0),
                vec2(lx - 60.0 + (stage.center().x - lx) * 0.5, floor_y + 30.0),
                vec2(lx + 60.0 + (stage.center().x - lx) * 0.5, floor_y + 30.0),
                Color::new(1.0, 0.92, 0.7, 0.03),
            );
            draw_circle(lx, gy + 21.0, 4.0, rgb(255, 236, 180));
        }
        // a hazard line along the front of the floor
        stripe(Rect::new(scr.x, scr.bottom() - 8.0, scr.w, 8.0), false);
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

#[allow(dead_code)]
fn _slot_check(s: Slot) -> bool {
    s == Slot::Mount
}

//! The campaign screen's war room: research into the enemy armies, the bulletins when a new army
//! arrives or one is wiped out, and the check before a new campaign erases the old one. The
//! bulletins and the dialog share a look: chamfered dark panels with thin coloured edges,
//! slightly futuristic but grounded.

use super::{wrap_lines, App, HubAct};
use crate::campaign::{self, Bulletin, Campaign, ALIENS, FTIERS, ORCS, ZOMBIES};
use crate::defs::*;
use crate::render::{text, text_w};
use crate::ui::{self, alpha, mix, pal};
use macroquad::prelude::*;

/// Each army's colour on the campaign screen: zombie green, alien purple, orc orange.
pub(super) fn army_col(a: usize) -> Color {
    match a {
        ALIENS => rgb(190, 92, 255),
        ORCS => rgb(255, 140, 50),
        _ => rgb(92, 220, 110),
    }
}

pub(super) fn campaign_army_name(a: usize) -> &'static str {
    FACTIONS[a.min(2)].name
}

/// A city's threat level as a colour: amber when mild, red when hard.
pub(super) fn level_col(l: u32) -> Color {
    mix(rgb(255, 196, 64), rgb(255, 64, 48), ((l as f32 - 2.0) / 10.0).clamp(0.0, 1.0))
}

pub(super) fn turns_s(n: u32) -> String {
    if n == 1 { "1 turn".into() } else { format!("{} turns", n) }
}

/// The corners of `r` with its top-left and bottom-right corners cut off.
fn chamfer_pts(r: Rect, cut: f32) -> [Vec2; 6] {
    [
        vec2(r.x + cut, r.y),
        vec2(r.right(), r.y),
        vec2(r.right(), r.bottom() - cut),
        vec2(r.right() - cut, r.bottom()),
        vec2(r.x, r.bottom()),
        vec2(r.x, r.y + cut),
    ]
}

fn chamfer(r: Rect, cut: f32, fill: Color) {
    let pts = chamfer_pts(r, cut);
    let c = r.center();
    for k in 0..pts.len() {
        draw_triangle(c, pts[k], pts[(k + 1) % pts.len()], fill);
    }
}

fn chamfer_lines(r: Rect, cut: f32, th: f32, col: Color) {
    let pts = chamfer_pts(r, cut);
    for k in 0..pts.len() {
        let (a, b) = (pts[k], pts[(k + 1) % pts.len()]);
        draw_line(a.x, a.y, b.x, b.y, th, col);
    }
}

/// Short L-shaped brackets just outside the corners of `r`.
fn brackets(r: Rect, len: f32, col: Color) {
    let o = 6.0;
    let (l, t, rt, b) = (r.x - o, r.y - o, r.right() + o, r.bottom() + o);
    for (x, y, dx, dy) in [(l, t, 1.0, 1.0), (rt, t, -1.0, 1.0), (l, b, 1.0, -1.0), (rt, b, -1.0, -1.0)] {
        draw_line(x, y, x + dx * len, y, 2.0, col);
        draw_line(x, y, x, y + dy * len, 2.0, col);
    }
}

impl App {
    /// Research into the enemy armies, in the right column: for each army its points (from its
    /// kills), the steps bought and the next one. Returns the height used.
    pub(super) fn faction_research(&mut self, c: &mut Campaign, r: Rect) {
        let p = pal(self.dark);
        let (x0, w) = (r.x + 10.0, r.w - 20.0);
        let mut y = r.y;
        text(&self.fonts.head, "Know your enemy", x0, y + 14.0, 15.0, p.text);
        y += 22.0;
        for l in wrap_lines(&self.fonts.mono, "Kills of each army pay for research into it. Every step weakens it; the last wipes it out everywhere.", w, 11.0).iter().take(2) {
            text(&self.fonts.mono, l, x0, y + 10.0, 11.0, p.dim);
            y += 14.0;
        }
        y += 6.0;
        let card_h = ((r.bottom() - y) / 3.0 - 8.0).clamp(64.0, 104.0);
        // armies that haven't reached Earth yet stay secret
        let found: Vec<usize> = [ZOMBIES, ALIENS, ORCS].into_iter().filter(|&a| c.arrived(a)).collect();
        for a in found {
            let cr = Rect::new(r.x, y, r.w, card_h);
            let col = army_col(a);
            let here = c.arrived(a);
            chamfer(cr, 10.0, if here { p.panel2 } else { alpha(p.panel2, 0.5) });
            chamfer_lines(cr, 10.0, 1.0, alpha(col, if here { 0.55 } else { 0.2 }));
            draw_rectangle(cr.x + 1.0, cr.y + 12.0, 3.0, cr.h - 24.0, if here { col } else { alpha(col, 0.35) });
            let ix = cr.x + 14.0;
            let iw = cr.w - 28.0;
            text(&self.fonts.head, campaign_army_name(a), ix, cr.y + 22.0, 15.0, if here { p.text } else { p.dim });
            let pts = format!("{} FP", c.fpoints(a));
            text(&self.fonts.head, &pts, ix + iw - text_w(&self.fonts.head, &pts, 15.0), cr.y + 22.0, 15.0, if here { col } else { p.dim });
            // the steps, as segments
            let seg = (iw - (FTIERS - 1) as f32 * 4.0) / FTIERS as f32;
            for k in 0..FTIERS {
                let on = (k as u32) < c.ftier[a];
                let last = k == FTIERS - 1;
                let sr = Rect::new(ix + k as f32 * (seg + 4.0), cr.y + 30.0, seg, 5.0);
                draw_rectangle(sr.x, sr.y, sr.w, sr.h, if on { col } else if last { alpha(col, 0.22) } else { rgba(255, 255, 255, 0.08) });
            }
            let (line, line_col) = match (c.fblock(a), c.fnext(a)) {
                (Some(why), _) => (why.to_string(), p.dim),
                (None, Some(t)) => (format!("Next: {}", t.name), p.text),
                (None, None) => ("Wiped out".to_string(), p.dim),
            };
            let by = cr.bottom() - 30.0;
            let bw = 112.0;
            let text_w_room = iw - bw - 8.0;
            let line = super::fit(&self.fonts.mono, &line, text_w_room, 11.5);
            text(&self.fonts.mono, &line, ix, cr.y + 52.0, 11.5, line_col);
            if card_h >= 90.0 {
                let eff = if c.wiped(a) {
                    "Gone from the Earth.".to_string()
                } else if c.ftier[a] == 0 {
                    "No research yet.".to_string()
                } else {
                    format!("Hordes -{:.0}% size, -{:.0}% health and attack", (1.0 - c.size_mult(a)) * 100.0, (1.0 - c.potency(a)) * 100.0)
                };
                let eff = super::fit(&self.fonts.mono, &eff, text_w_room, 10.5);
                text(&self.fonts.mono, &eff, ix, cr.y + 68.0, 10.5, p.dim);
            }
            if let Some(t) = c.fnext(a).filter(|_| c.fblock(a).is_none()) {
                let br = Rect::new(ix + iw - bw, by, bw, 24.0);
                let label = format!("{} FP", t.cost);
                if self.button(br, &label, c.can_fbuy(a) && !c.finished(), c.can_fbuy(a)) {
                    self.hub_act(c, HubAct::Hunt(a));
                }
                if self.hot(cr) {
                    let wipe = if c.ftier[a] as usize == FTIERS - 1 { "" } else { " Hordes 10% smaller, units 7% weaker." };
                    self.tip = Some(format!("{}: {}{} {} FP; you have {}. One point for every {} {} killed.", t.name, t.tip, wipe, t.cost, c.fpoints(a), campaign::KILLS_PER_POINT[a], campaign_army_name(a).to_lowercase()));
                }
            }
            y += card_h + 8.0;
        }
    }

    /// The bulletin over the campaign screen when a new army arrives or one is wiped out, until
    /// it's acknowledged. The arrival plays the newscaster's line once. True while it's up.
    pub(super) fn bulletin(&mut self, c: &mut Campaign) -> bool {
        let Some(b) = c.bulletin(self.told_local) else { return false };
        let p = pal(self.dark);
        let key = (c.seed, b.bit());
        if self.bulletin_played != Some(key) {
            self.bulletin_played = Some(key);
            if let Bulletin::Arrived(a) = b {
                self.music.bulletin(a);
            }
        }
        let (army, kicker, head, body) = match b {
            Bulletin::Arrived(ALIENS) => {
                let at: Vec<&str> = c.aliens.iter().map(|s| c.info(s.city).name).collect();
                (
                    ALIENS,
                    "NEW THREAT DETECTED",
                    "ALIENS",
                    format!(
                        "Something came down out of the sky over {}. The swarm attacks one city at a time and eats through it twice as fast as the dead. Hold the city and it flies on to the next; take too long and it only grows stronger. If it takes a city, a second swarm splits off. Marked on the globe with a purple reticle.",
                        at.first().copied().unwrap_or("a city")
                    ),
                )
            }
            Bulletin::Arrived(_) => {
                let at: Vec<&str> = (0..c.cities.len()).filter(|&i| c.open(i) && c.army_at(i) == ORCS).map(|i| c.info(i).name).collect();
                (
                    ORCS,
                    "NEW THREAT DETECTED",
                    "ORCS",
                    format!(
                        "Portals have torn open in {}, and warbands from another dimension are pouring through. More portals will open in other cities without warning. Marked on the globe with an orange reticle.",
                        if at.is_empty() { "a city".to_string() } else { at.join(" and ") }
                    ),
                )
            }
            Bulletin::Wiped(ZOMBIES) => (
                ZOMBIES,
                "THE WORLD IS SAVED",
                "THE CURE WORKS",
                "The dead fall still in every street on Earth. The aliens are gone, the rifts are sealed, the plague is over. And with your newfound knowledge of the unnatural, you've decided to dominate the world with your undead army.".to_string(),
            ),
            Bulletin::Wiped(a) => (
                a,
                "THREAT ELIMINATED",
                if a == ALIENS { "ALIENS WIPED OUT" } else { "ORCS WIPED OUT" },
                if a == ALIENS {
                    "The hive mind has collapsed. Every alien swarm on Earth fell out of the sky at once. The zombies are still out there.".to_string()
                } else {
                    "The rift is sealed. Cut off from their dimension, the warbands have fallen. The zombies are still out there.".to_string()
                },
            ),
        };
        let col = army_col(army);
        self.backdrop();
        let (sw, sh) = (ui::sw(), ui::sh());
        let w = 620.0f32.min(sw - 40.0);
        let lines = wrap_lines(&self.fonts.mono, &body, w - 64.0, 13.5);
        let h = 214.0 + lines.len() as f32 * 19.0;
        let r = Rect::new((sw - w) / 2.0, (sh - h) / 2.0, w, h);
        chamfer(r, 18.0, Color::new(0.035, 0.05, 0.08, 0.97));
        // faint scanlines and a glow along the top edge
        let mut sy = r.y + 4.0;
        while sy < r.bottom() - 4.0 {
            draw_line(r.x + 2.0, sy, r.right() - 2.0, sy, 1.0, Color::new(1.0, 1.0, 1.0, 0.018));
            sy += 3.0;
        }
        draw_rectangle(r.x + 18.0, r.y, r.w - 18.0, 3.0, col);
        draw_rectangle(r.x + 18.0, r.y + 3.0, r.w - 18.0, 14.0, alpha(col, 0.08));
        chamfer_lines(r, 18.0, 1.2, alpha(col, 0.7));
        let pulse = (self.clock * 3.0).sin() * 0.5 + 0.5;
        brackets(r, 18.0, alpha(col, 0.45 + 0.35 * pulse));
        let x0 = r.x + 32.0;
        let tag = format!("PRIORITY BULLETIN · TURN {}", c.fought + 1);
        text(&self.fonts.mono, &tag, x0, r.y + 34.0, 11.0, col);
        let live = "LIVE";
        let lx = r.right() - 32.0 - text_w(&self.fonts.mono, live, 11.0);
        draw_circle(lx - 9.0, r.y + 30.0, 3.5, Color::new(1.0, 0.25, 0.2, 0.4 + 0.6 * pulse));
        text(&self.fonts.mono, live, lx, r.y + 34.0, 11.0, p.text);
        draw_line(x0, r.y + 46.0, r.right() - 32.0, r.y + 46.0, 1.0, alpha(col, 0.25));
        text(&self.fonts.mono, kicker, x0, r.y + 74.0, 13.0, p.dim);
        text(&self.fonts.head, head, x0, r.y + 112.0, 36.0, col);
        let mut y = r.y + 144.0;
        for l in &lines {
            text(&self.fonts.mono, l, x0, y, 13.5, p.text);
            y += 19.0;
        }
        let br = Rect::new(r.right() - 32.0 - 180.0, r.bottom() - 58.0, 180.0, 36.0);
        let was = self.blocked;
        self.blocked = self.settings_open || self.invite_open;
        let ok = self.button(br, "Acknowledged", true, true) || (!self.blocked && (is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::Escape) || is_key_pressed(KeyCode::Space)));
        self.blocked = was;
        if ok {
            self.told_local |= b.bit();
            if !self.client() {
                c.tell(b);
                campaign::save(c);
            }
        }
        true
    }

    /// Asks before a new campaign erases the saved one.
    pub(super) fn confirm_new_dialog(&mut self) {
        let p = pal(self.dark);
        self.backdrop();
        let (sw, sh) = (ui::sw(), ui::sh());
        let w = 480.0f32.min(sw - 40.0);
        let saved = campaign::load();
        let what = match &saved {
            Some(c) if !c.finished() => format!("Your saved campaign (turn {}, {} cities saved) will be erased. There's no undo.", c.fought + 1, c.held()),
            _ => "Your saved campaign will be erased. There's no undo.".to_string(),
        };
        let lines = wrap_lines(&self.fonts.mono, &what, w - 56.0, 13.0);
        let h = 150.0 + lines.len() as f32 * 18.0;
        let r = Rect::new((sw - w) / 2.0, (sh - h) / 2.0, w, h);
        let col = p.bad;
        chamfer(r, 14.0, Color::new(0.035, 0.05, 0.08, 0.98));
        draw_rectangle(r.x + 14.0, r.y, r.w - 14.0, 3.0, col);
        chamfer_lines(r, 14.0, 1.2, alpha(col, 0.6));
        let x0 = r.x + 28.0;
        text(&self.fonts.head, "Start a new campaign?", x0, r.y + 46.0, 24.0, p.text);
        let mut y = r.y + 76.0;
        for l in &lines {
            text(&self.fonts.mono, l, x0, y, 13.0, p.dim);
            y += 18.0;
        }
        let bw = (w - 56.0 - 12.0) / 2.0;
        let keep = Rect::new(x0, r.bottom() - 54.0, bw, 36.0);
        let go = Rect::new(x0 + bw + 12.0, r.bottom() - 54.0, bw, 36.0);
        if self.button(keep, "Keep it", true, false) || is_key_pressed(KeyCode::Escape) {
            self.confirm_new = false;
        }
        if self.button(go, "Erase and start over", true, true) {
            self.confirm_new = false;
            campaign::clear();
            self.trail_t = None;
            self.told_local = 0;
            self.open_campaign(Some(Campaign::new(self.diff)));
        }
    }
}

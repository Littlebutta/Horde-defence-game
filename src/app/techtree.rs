//! The campaign's research: the column on the campaign screen (points, the way into the tree,
//! research into the enemy armies, the army) and the tech tree itself, drawn as a tree of nodes over the whole screen. Each player has
//! their own points and their own tree.

use super::widgets::{R_CTRL, R_PANEL};
use super::{fit, App, HubAct, PLAYER_COLS};
use crate::campaign::{self, Campaign, Tech};
use crate::defs::*;
use crate::render::{text, text_w};
use crate::ui::{self, alpha, mix, pal, rrect, rrect_lines};
use macroquad::prelude::*;

const OWNED: Color = Color { r: 0.25, g: 0.75, b: 0.34, a: 1.0 };

/// The colour of a tech's branch of the game.
fn group_col(t: Tech) -> Color {
    match t.group() {
        "Turrets" => rgb(255, 196, 64),
        "Support" => rgb(77, 171, 247),
        "Abilities" => rgb(255, 107, 107),
        "Research" => rgb(177, 151, 252),
        _ => rgb(255, 146, 43),
    }
}

impl App {
    /// The right column of the campaign screen: this player's research points and money, the tree's
    /// way in, research into the enemy armies, and the army.
    pub(super) fn research_column(&mut self, c: &mut Campaign, right: Rect) {
        let p = pal(self.dark);
        let me = self.me();
        let coop = self.sess.as_ref().is_some_and(|s| s.players.len() > 1);
        if !(self.settings_open || self.invite_open) && !c.finished() && is_key_pressed(KeyCode::T) {
            self.tree_open = !self.tree_open;
        }
        self.panel(right);
        let (x0, w) = (right.x + 20.0, right.w - 40.0);
        let mut y = right.y + 34.0;
        text(&self.fonts.head, "Research", x0, y, 20.0, p.text);
        let pts = format!("{} RP", c.points(me));
        text(&self.fonts.head, &pts, x0 + w - text_w(&self.fonts.head, &pts, 22.0), y, 22.0, p.accent);
        y += 18.0;
        let line = if c.fought == 0 && c.crew(me).tech.is_empty() {
            "A grant to start with: enough for a first unlock, like the gun turret. After that, every civilian flown out is a research point."
        } else if coop {
            "Every civilian flown out from your evac points is one of your research points."
        } else {
            "Every civilian flown out is a research point."
        };
        for l in super::wrap_lines(&self.fonts.mono, line, w, 11.5).iter().take(2) {
            text(&self.fonts.mono, l, x0, y, 11.5, p.dim);
            y += 14.0;
        }
        // co-op: everybody's points
        if let Some(s) = self.sess.as_ref().filter(|_| coop) {
            let names: Vec<String> = s.players.iter().map(|pl| pl.name.clone()).collect();
            for (k, name) in names.iter().enumerate() {
                let cy = y + 6.0;
                draw_circle(x0 + 5.0, cy - 4.0, 4.0, PLAYER_COLS[k % 4]);
                let label = fit(&self.fonts.mono, name, w - 140.0, 11.5);
                text(&self.fonts.mono, &label, x0 + 16.0, cy, 11.5, if k == me { p.text } else { p.dim });
                let r = format!("{} RP · {} unlocked", c.points(k), c.crew(k).tech.len());
                text(&self.fonts.mono, &r, x0 + w - text_w(&self.fonts.mono, &r, 11.5), cy, 11.5, p.dim);
                y += 16.0;
            }
        }
        y += 6.0;
        let owned = c.crew(me).tech.len();
        let br = Rect::new(right.x + 10.0, y, right.w - 20.0, 44.0);
        if self.button(br, &format!("Open the tech tree (T) · {} of {} unlocked", owned, campaign::TECH.len()), !c.finished(), true) {
            self.tree_open = true;
        }
        y += 50.0;
        let gr = Rect::new(right.x + 10.0, y, right.w - 20.0, 32.0);
        let money = c.crew(me).money;
        if self.button(gr, &format!("Turret garage (G) · ${}", money), !c.finished(), false)
            || (!self.blocked && !self.tree_open && !c.finished() && is_key_pressed(KeyCode::G))
        {
            self.open_workshop(true);
        }
        if self.hot(gr) {
            self.tip = Some("Design your own turrets: research parts with RP, buy them with the money your cities pay.".into());
        }
        y += 54.0;

        // research into the enemy armies, between the tree and the army
        let army_h = 150.0;
        self.faction_research(c, Rect::new(right.x + 10.0, y - 16.0, right.w - 20.0, right.y + right.h - army_h - y + 4.0));

        // the army, at the bottom
        let mut y = right.y + right.h - army_h + 14.0;
        text(&self.fonts.head, if coop { "The army" } else { "Your army" }, x0, y, 16.0, p.text);
        y += 10.0;
        let (rs, rn) = c.army_size(U::Rifle);
        let (ts, _) = c.army_size(U::Tank);
        let ar = Rect::new(right.x + 10.0, y, right.w - 20.0, 52.0);
        draw_rectangle(ar.x, ar.y, ar.w, ar.h, p.panel2);
        text(&self.fonts.mono, &format!("{} rifle squad{} ({} soldiers) · {} tank{}", rs, if rs == 1 { "" } else { "s" }, rn, ts, if ts == 1 { "" } else { "s" }), x0, ar.y + 22.0, 12.5, p.text);
        // what doesn't fit half the HQ's logistics sits this city out
        let res = c.deployable(c.start_hq(0)).1.len();
        let note = if res == 0 {
            "Survivors march on to the next city.".to_string()
        } else {
            format!("{} in reserve: the army gets half the HQ's logistics.", if res == 1 { "1 squad waits".to_string() } else { format!("{} squads wait", res) })
        };
        let note = fit(&self.fonts.mono, &note, ar.w - 20.0, 11.0);
        text(&self.fonts.mono, &note, x0, ar.y + 40.0, 11.0, if res == 0 { p.dim } else { p.warn });
        y += ar.h + 8.0;
        let half = (right.w - 28.0) / 2.0;
        for (k, u) in [U::Rifle, U::Tank].into_iter().enumerate() {
            let cost = campaign::recruit_cost(u);
            let br = Rect::new(right.x + 10.0 + k as f32 * (half + 8.0), y, half, 34.0);
            let open = u == U::Rifle || c.has_tech(me, Tech::Unit(u));
            let label = if open { format!("+{} · {} RP", udef(u).name.to_lowercase(), cost) } else { format!("{}: locked", udef(u).name) };
            if self.button(br, &label, open && c.points(me) >= cost && !c.finished(), false) {
                self.hub_act(c, HubAct::Recruit(u));
            }
        }
    }

    /// The tech tree over the whole campaign screen: every unlock a node, each joined to the one it
    /// needs on its left. Click a node to unlock it.
    pub(super) fn tech_tree(&mut self, c: &mut Campaign) {
        let p = pal(self.dark);
        let me = self.me();
        if !self.blocked && is_key_pressed(KeyCode::Escape) {
            self.tree_open = false;
            return;
        }
        self.backdrop();
        let hud = ui::hud();
        let r = Rect::new(hud.x + 24.0, 20.0, hud.w - 48.0, ui::sh() - 40.0);
        // solid, so the campaign screen doesn't show through
        rrect(r, R_PANEL, Color { a: 1.0, ..mix(p.panel, BLACK, 0.25) });
        self.panel(r);
        let x0 = r.x + 28.0;
        text(&self.fonts.head, "TECH TREE", x0, r.y + 44.0, 26.0, p.text);
        let sub = "Unlock a tech with research points to open up the ones to its right.";
        text(&self.fonts.mono, sub, x0, r.y + 66.0, 12.5, p.dim);
        let close = Rect::new(r.right() - 128.0, r.y + 22.0, 104.0, 34.0);
        if self.button(close, "Close (T)", true, false) {
            self.tree_open = false;
        }
        let pts = format!("{} RP", c.points(me));
        text(&self.fonts.head, &pts, close.x - 24.0 - text_w(&self.fonts.head, &pts, 26.0), r.y + 48.0, 26.0, p.accent);
        if self.sess.as_ref().is_some_and(|s| s.players.len() > 1) {
            let mine = "YOUR TREE";
            text(&self.fonts.mono, mine, close.x - 24.0 - text_w(&self.fonts.mono, mine, 10.5), r.y + 66.0, 10.5, p.dim);
        }

        // the legend along the bottom
        let ly = r.bottom() - 22.0;
        let mut lx = x0;
        for (name, col) in [("Turrets", rgb(255, 196, 64)), ("Support", rgb(77, 171, 247)), ("Abilities", rgb(255, 107, 107)), ("Research", rgb(177, 151, 252)), ("Turret upgrades", rgb(255, 146, 43))] {
            draw_rectangle(lx, ly - 9.0, 10.0, 10.0, col);
            text(&self.fonts.mono, name, lx + 16.0, ly, 11.5, p.dim);
            lx += 34.0 + text_w(&self.fonts.mono, name, 11.5);
        }
        let states = "Green: unlocked · bright edge: ready to unlock · faded: needs the one on its left";
        text(&self.fonts.mono, states, r.right() - 28.0 - text_w(&self.fonts.mono, states, 11.5), ly, 11.5, p.dim);

        // the two halves of the tree side by side, each column as wide as the next
        let body = Rect::new(x0, r.y + 92.0, r.w - 56.0, r.h - 92.0 - 48.0);
        let (nodes, dims) = campaign::tree_layout();
        let gap = 48.0;
        let unit = (body.w - gap) / (dims[0].0 + dims[1].0) as f32;
        let nw = (unit - 30.0).min(210.0);
        let hx = [body.x, body.x + dims[0].0 as f32 * unit + gap];
        let rh: Vec<f32> = dims.iter().map(|d| (body.h / d.1.max(1.0)).min(72.0)).collect();
        let top: Vec<f32> = dims.iter().zip(&rh).map(|(d, h)| body.y + (body.h - d.1 * h) / 2.0).collect();
        let nh = (rh.iter().cloned().fold(f32::MAX, f32::min) * 0.74).clamp(30.0, 48.0);
        let at = |n: &campaign::Node| vec2(hx[n.half] + n.col as f32 * unit, top[n.half] + (n.row + 0.5) * rh[n.half]);
        let find = |t: Tech| nodes.iter().find(|n| n.tech == t);

        // the links first, so the nodes sit on top of them
        for n in &nodes {
            let Some(par) = n.tech.needs().and_then(find) else { continue };
            let (a, b) = (at(par), at(n));
            let (sx, ex) = (a.x + nw, b.x);
            let mx = ex - (unit - nw) / 2.0;
            let col = if c.has_tech(me, n.tech) {
                alpha(OWNED, 0.9)
            } else if c.has_tech(me, par.tech) {
                alpha(p.accent, 0.75)
            } else {
                alpha(p.line, 1.0)
            };
            let th = if c.has_tech(me, par.tech) { 2.0 } else { 1.2 };
            draw_line(sx, a.y, mx, a.y, th, col);
            draw_line(mx, a.y, mx, b.y, th, col);
            draw_line(mx, b.y, ex, b.y, th, col);
        }

        // co-op: who else has each node, as their coloured dots
        let others: Vec<(usize, Vec<Tech>)> = match self.sess.as_ref() {
            Some(s) if s.players.len() > 1 => (0..s.players.len()).filter(|k| *k != me).map(|k| (k, c.crew(k).tech.clone())).collect(),
            _ => Vec::new(),
        };
        let mut hovered = None;
        for n in &nodes {
            let t = n.tech;
            let q = at(n);
            let nr = Rect::new(q.x, q.y - nh / 2.0, nw, nh);
            let has = c.has_tech(me, t);
            let open = c.open_tech(me, t);
            let can = c.can_buy(me, t);
            let hot = self.hot(nr);
            let (fill, edge, ew) = if has {
                (mix(p.panel2, OWNED, 0.24), OWNED, 1.5)
            } else if can {
                (if hot { mix(p.panel2, p.accent, 0.18) } else { mix(p.panel2, p.accent, 0.07) }, p.accent, 1.8)
            } else if open {
                (if hot { mix(p.panel2, p.text, 0.06) } else { p.panel2 }, p.line, 1.0)
            } else {
                (alpha(p.panel2, 0.45), alpha(p.line, 0.6), 1.0)
            };
            if can {
                ui::shadow(nr, R_CTRL, alpha(p.accent, 0.25));
            }
            rrect(nr, R_CTRL, fill);
            rrect_lines(nr, R_CTRL, ew, edge);
            let gc = group_col(t);
            draw_rectangle(nr.x + 1.0, nr.y + 7.0, 3.0, nr.h - 14.0, if open || has { gc } else { alpha(gc, 0.4) });
            let dim = !(has || open);
            let name = fit(&self.fonts.head, t.name(), nw - 22.0, 13.5);
            let two = nh >= 36.0;
            let ny = if two { nr.y + nr.h / 2.0 - 2.0 } else { nr.y + nr.h / 2.0 + 4.5 };
            text(&self.fonts.head, &name, nr.x + 12.0, ny, 13.5, if dim { p.dim } else { p.text });
            if two {
                let (s, col) = if has {
                    ("UNLOCKED".to_string(), OWNED)
                } else if open {
                    (format!("{} RP", t.cost()), if can { p.accent } else { p.dim })
                } else {
                    (format!("{} RP · locked", t.cost()), alpha(p.dim, 0.8))
                };
                text(&self.fonts.mono, &s, nr.x + 12.0, nr.y + nr.h / 2.0 + 13.0, 11.0, col);
            }
            let mut k = 0;
            for (pl, tech) in &others {
                if tech.contains(&t) {
                    let d = vec2(nr.right() - 9.0 - k as f32 * 9.0, nr.y + 8.0);
                    draw_circle(d.x, d.y, 3.2, PLAYER_COLS[pl % 4]);
                    k += 1;
                }
            }
            if hot {
                hovered = Some(t);
                if self.pressed(can) {
                    self.hub_act(c, HubAct::Tech(t));
                }
            }
        }
        if let Some(t) = hovered {
            let state = if c.has_tech(me, t) {
                "Unlocked.".to_string()
            } else if let Some(n) = t.needs().filter(|n| !c.has_tech(me, *n)) {
                format!("Needs {} first. {} RP.", n.name(), t.cost())
            } else if c.can_buy(me, t) {
                format!("Click to unlock for {} RP.", t.cost())
            } else {
                format!("{} RP: {} more to go.", t.cost(), t.cost() - c.points(me))
            };
            self.tip = Some(format!("{}: {} {}", t.name(), t.tip(), state));
        }
    }
}

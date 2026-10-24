//! The controls every screen is built from: panels, buttons, segmented pickers, sliders, toggles and
//! tooltips. All sizes are in UI points (see `ui`). Each control records its rectangle in
//! `ui_rects`, so clicks on it don't reach the map underneath.

#![allow(dead_code)]

use super::App;
use crate::render::{text, text_w};
use crate::ui::{self, alpha, mix, pal, rrect, rrect_lines, RectExt};
use macroquad::prelude::*;

/// Corner radius of panels and of controls.
pub const R_PANEL: f32 = 12.0;
pub const R_CTRL: f32 = 8.0;

impl App {
    /// The mouse in UI points.
    pub(super) fn mouse(&self) -> Vec2 {
        ui::mouse()
    }
    /// The mouse in screen pixels, for picking on the map.
    pub(super) fn px_mouse(&self) -> Vec2 {
        let (x, y) = mouse_position();
        vec2(x, y)
    }
    /// The mouse is over `r` (and no dialog covers it).
    pub(super) fn hot(&self, r: Rect) -> bool {
        !self.blocked && r.contains(self.mouse())
    }
    pub(super) fn clicked(&self, r: Rect) -> bool {
        self.hot(r) && is_mouse_button_pressed(MouseButton::Left)
    }

    /// A floating panel with a soft shadow.
    pub(super) fn panel(&mut self, r: Rect) {
        let p = pal(self.dark);
        ui::shadow(r, R_PANEL, p.shadow);
        rrect(r, R_PANEL, p.panel);
        rrect_lines(r, R_PANEL, 1.0, p.line);
        self.ui_rects.push(r);
    }
    /// A panel with a title row; returns the area under the title.
    pub(super) fn card(&mut self, r: Rect, title: &str) -> Rect {
        self.panel(r);
        let p = pal(self.dark);
        let mut a = r.pad(18.0, 14.0);
        let t = a.cut_top(22.0, 10.0);
        text(&self.fonts.head, title, t.x, t.y + 17.0, 17.0, p.text);
        a
    }
    /// A small caps section heading at `y` (its top); returns the y below it.
    pub(super) fn heading(&self, s: &str, x: f32, y: f32) -> f32 {
        let p = pal(self.dark);
        text(&self.fonts.mono, &s.to_uppercase(), x, y + 11.0, 11.0, p.dim);
        y + 20.0
    }
    /// The shaded backdrop behind a dialog.
    pub(super) fn backdrop(&mut self) {
        let s = ui::screen();
        draw_rectangle(s.x, s.y, s.w, s.h, Color::new(0.0, 0.0, 0.0, 0.45));
        self.ui_rects.push(s);
    }

    /// A button; returns true when clicked.
    pub(super) fn button(&mut self, r: Rect, label: &str, enabled: bool, active: bool) -> bool {
        let p = pal(self.dark);
        let hot = self.hot(r) && enabled;
        let down = hot && is_mouse_button_down(MouseButton::Left);
        let bg = if active {
            mix(p.panel2, p.accent, if hot { 0.34 } else { 0.26 })
        } else if hot {
            mix(p.panel2, p.text, if down { 0.14 } else { 0.08 })
        } else {
            p.panel2
        };
        rrect(r, R_CTRL, bg);
        rrect_lines(r, R_CTRL, 1.0, if active { alpha(p.accent, 0.9) } else { p.line });
        let fc = if !enabled {
            alpha(p.dim, 0.7)
        } else {
            p.text
        };
        let size = (r.h * 0.42).clamp(12.0, 16.0);
        self.label_in(r, label, size, fc);
        self.ui_rects.push(r);
        hot && is_mouse_button_pressed(MouseButton::Left)
    }
    /// The main action of a screen: a filled accent button.
    pub(super) fn button_primary(&mut self, r: Rect, label: &str, enabled: bool) -> bool {
        let p = pal(self.dark);
        let hot = self.hot(r) && enabled;
        let down = hot && is_mouse_button_down(MouseButton::Left);
        if enabled {
            let base = if down { mix(p.accent, BLACK, 0.12) } else if hot { mix(p.accent, WHITE, 0.12) } else { p.accent };
            ui::shadow(r, R_CTRL, alpha(p.accent, 0.35));
            rrect(r, R_CTRL, base);
            // a light sheen on the top half
            let top = Rect::new(r.x + 2.0, r.y + 2.0, r.w - 4.0, r.h * 0.45);
            rrect(top, R_CTRL - 2.0, Color::new(1.0, 1.0, 1.0, 0.10));
        } else {
            rrect(r, R_CTRL, p.panel2);
            rrect_lines(r, R_CTRL, 1.0, p.line);
        }
        let fc = if enabled { p.on_accent } else { alpha(p.dim, 0.7) };
        let size = (r.h * 0.4).clamp(14.0, 22.0);
        self.label_in(r, label, size, fc);
        self.ui_rects.push(r);
        hot && is_mouse_button_pressed(MouseButton::Left)
    }
    /// A label centred in `r`, shrunk to fit.
    pub(super) fn label_in(&self, r: Rect, label: &str, size: f32, c: Color) {
        let mut size = size;
        let mut tw = text_w(&self.fonts.head, label, size);
        if tw > r.w - 12.0 && tw > 0.0 {
            size = (size * (r.w - 12.0) / tw).max(9.0);
            tw = text_w(&self.fonts.head, label, size);
        }
        text(&self.fonts.head, label, r.x + (r.w - tw) / 2.0, r.y + r.h / 2.0 + size * 0.34, size, c);
    }
    /// One of several choices side by side. Returns the one clicked.
    pub(super) fn segmented(&mut self, r: Rect, labels: &[&str], sel: usize) -> Option<usize> {
        let p = pal(self.dark);
        rrect(r, R_CTRL, p.panel2);
        rrect_lines(r, R_CTRL, 1.0, p.line);
        self.ui_rects.push(r);
        let mut out = None;
        for (i, c) in r.inset(3.0).cols(labels.len(), 3.0).into_iter().enumerate() {
            let hot = self.hot(c);
            if i == sel {
                rrect(c, R_CTRL - 2.0, p.accent);
            } else if hot {
                rrect(c, R_CTRL - 2.0, alpha(p.text, 0.07));
            }
            let fc = if i == sel { p.on_accent } else if hot { p.text } else { p.dim };
            self.label_in(c, labels[i], (r.h * 0.4).clamp(12.0, 16.0), fc);
            if hot && is_mouse_button_pressed(MouseButton::Left) {
                out = Some(i);
            }
        }
        out
    }
    /// An on/off switch with a label to its left, filling `r`. Returns true when flipped.
    pub(super) fn toggle(&mut self, r: Rect, label: &str, on: bool) -> bool {
        let p = pal(self.dark);
        let hot = self.hot(r);
        text(&self.fonts.head, label, r.x, r.y + r.h / 2.0 + 5.0, 15.0, p.text);
        let sw = Rect::new(r.right() - 44.0, r.y + (r.h - 24.0) / 2.0, 44.0, 24.0);
        rrect(sw, 12.0, if on { p.accent } else { mix(p.panel2, p.text, if hot { 0.2 } else { 0.12 }) });
        let k = if on { sw.right() - 12.0 } else { sw.x + 12.0 };
        draw_circle(k, sw.y + 12.0, 9.0, if on { p.on_accent } else { p.text });
        self.ui_rects.push(r);
        hot && is_mouse_button_pressed(MouseButton::Left)
    }
    /// A 0..=100 slider with its label and value, filling `r`. `id` tells the sliders apart while
    /// one is dragged. Returns the new value while it is being changed.
    pub(super) fn slider(&mut self, r: Rect, id: u32, label: &str, v: u32) -> Option<u32> {
        let p = pal(self.dark);
        let val = if v == 0 { "Off".to_string() } else { format!("{}%", v) };
        text(&self.fonts.head, label, r.x, r.y + 16.0, 15.0, p.text);
        let vw = text_w(&self.fonts.mono, &val, 13.0);
        text(&self.fonts.mono, &val, r.right() - vw, r.y + 16.0, 13.0, p.dim);
        let track = Rect::new(r.x + 9.0, r.bottom() - 15.0, r.w - 18.0, 6.0);
        let f = v as f32 / 100.0;
        rrect(track, 3.0, mix(p.panel2, p.text, 0.12));
        rrect(Rect::new(track.x, track.y, track.w * f, track.h), 3.0, p.accent);
        let knob = vec2(track.x + track.w * f, track.y + 3.0);
        let grab = Rect::new(r.x, r.bottom() - 28.0, r.w, 28.0);
        let hot = self.hot(grab);
        draw_circle(knob.x, knob.y, if hot || self.drag == Some(id) { 10.0 } else { 8.5 }, p.text);
        draw_circle(knob.x, knob.y, 4.0, p.accent);
        self.ui_rects.push(r);
        if hot && is_mouse_button_pressed(MouseButton::Left) {
            self.drag = Some(id);
        }
        if self.drag != Some(id) {
            // the wheel nudges it by 5
            if hot {
                let (_, wy) = mouse_wheel();
                if wy != 0.0 {
                    return Some((v as i32 + if wy > 0.0 { 5 } else { -5 }).clamp(0, 100) as u32);
                }
            }
            return None;
        }
        if !is_mouse_button_down(MouseButton::Left) {
            self.drag = None;
            return None;
        }
        let t = ((self.mouse().x - track.x) / track.w).clamp(0.0, 1.0);
        let nv = ((t * 100.0 / 5.0).round() * 5.0) as u32;
        (nv != v).then_some(nv)
    }
    /// A thin progress bar.
    pub(super) fn progress(&self, r: Rect, f: f32, c: Color) {
        let p = pal(self.dark);
        rrect(r, r.h / 2.0, mix(p.panel2, p.text, 0.1));
        if f > 0.0 {
            rrect(Rect::new(r.x, r.y, r.w * f.clamp(0.0, 1.0), r.h), r.h / 2.0, c);
        }
    }
    /// A key hint, like a keyboard keycap, with its top-left at `x`, `y`.
    pub(super) fn keycap(&self, x: f32, y: f32, k: char) {
        let p = pal(self.dark);
        let r = Rect::new(x, y, 16.0, 16.0);
        rrect(r, 4.0, alpha(p.text, 0.1));
        let s = k.to_ascii_uppercase().to_string();
        let tw = text_w(&self.fonts.mono, &s, 10.5);
        text(&self.fonts.mono, &s, r.x + (r.w - tw) / 2.0, r.y + 12.0, 10.5, p.dim);
    }

    /// The tooltip for `self.tip`, near the mouse and kept on screen.
    pub(super) fn tooltip(&self, t: &str) {
        let p = pal(self.dark);
        let m = self.mouse();
        let lines = super::wrap_lines(&self.fonts.mono, t, 300.0, 13.0);
        let w = lines.iter().map(|l| text_w(&self.fonts.mono, l, 13.0)).fold(0.0, f32::max) + 24.0;
        let h = 16.0 + lines.len() as f32 * 18.0;
        let s = ui::screen();
        let x = (m.x - w / 2.0).clamp(8.0, s.w - w - 8.0);
        let mut y = m.y - h - 18.0;
        if y < 8.0 {
            y = m.y + 26.0;
        }
        let r = Rect::new(x, y, w, h);
        ui::shadow(r, R_CTRL, p.shadow);
        rrect(r, R_CTRL, mix(p.panel2, p.bg, 0.2));
        rrect_lines(r, R_CTRL, 1.0, alpha(p.accent, 0.5));
        for (i, l) in lines.iter().enumerate() {
            text(&self.fonts.mono, l, x + 12.0, y + 21.0 + i as f32 * 18.0, 13.0, p.text);
        }
    }
}

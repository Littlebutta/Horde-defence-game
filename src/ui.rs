//! Layout for menus and the HUD, so they fit every window from 4:3 to 32:9 and portrait.
//!
//! Screens are laid out in *points*, not pixels. The UI scale is picked so the window is always at
//! least 1280 x 900 points (the size every screen is designed for); a bigger or denser window just
//! gets more room, never a squashed layout. `begin()` sets a 2D camera in points and `end()` goes
//! back to pixels (for 3D views and world labels). Text drawn with `render::text` stays sharp at any
//! scale because it is rasterised at the real pixel size.
//!
//! Inside UI code use `sw()`, `sh()`, `screen()` and `mouse()` from here instead of macroquad's
//! `screen_width()`, `screen_height()` and `mouse_position()`, which are in pixels.
//!
//! On ultrawide screens `hud()` is the centred part of the screen (21:9 at most, unless the player
//! picked the full-width HUD) where anything the player must keep an eye on belongs.

// Some helpers are here for screens still to come.
#![allow(dead_code)]

use macroquad::prelude::*;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};

/// The smallest size, in points, any window is laid out at.
pub const REF_W: f32 = 1280.0;
pub const REF_H: f32 = 900.0;
/// Widest the HUD area gets on an ultrawide screen, as width / height.
pub const HUD_ASPECT: f32 = 21.0 / 9.0;

/// The scale `render::text` draws at right now: the UI scale between `begin` and `end`, else 1.
static DRAW_SCALE: AtomicU32 = AtomicU32::new(0x3f80_0000);
/// The player's UI size, in percent of the automatic scale.
static USER_PCT: AtomicU32 = AtomicU32::new(100);
/// HUD spread over the whole width instead of the centred area.
static WIDE: AtomicBool = AtomicBool::new(false);

pub fn set_user_scale(pct: u32) {
    USER_PCT.store(pct.clamp(50, 200), Relaxed);
}
pub fn user_scale() -> u32 {
    USER_PCT.load(Relaxed)
}
pub fn set_wide(w: bool) {
    WIDE.store(w, Relaxed);
}
pub fn wide() -> bool {
    WIDE.load(Relaxed)
}

/// Pixels per point.
pub fn scale() -> f32 {
    let (w, h) = (screen_width().max(1.0), screen_height().max(1.0));
    let auto = (h / REF_H).min(w / REF_W);
    (auto * USER_PCT.load(Relaxed) as f32 / 100.0).clamp(0.35, 6.0)
}
/// The scale text is being drawn at (1 outside `begin`/`end`).
pub fn draw_scale() -> f32 {
    f32::from_bits(DRAW_SCALE.load(Relaxed))
}

/// From here on, draw in points.
pub fn begin() {
    let s = scale();
    DRAW_SCALE.store(s.to_bits(), Relaxed);
    let (w, h) = (screen_width() / s, screen_height() / s);
    // y grows downwards, like screen pixels
    set_camera(&Camera2D { target: vec2(w / 2.0, h / 2.0), zoom: vec2(2.0 / w, 2.0 / h), ..Default::default() });
}
/// Back to pixels.
pub fn end() {
    DRAW_SCALE.store(1f32.to_bits(), Relaxed);
    set_default_camera();
}

/// Screen width in points.
pub fn sw() -> f32 {
    screen_width() / scale()
}
/// Screen height in points.
pub fn sh() -> f32 {
    screen_height() / scale()
}
pub fn screen() -> Rect {
    Rect::new(0.0, 0.0, sw(), sh())
}
/// The mouse in points.
pub fn mouse() -> Vec2 {
    let (x, y) = mouse_position();
    vec2(x, y) / scale()
}
/// Where the HUD goes: the whole screen, or its centre 21:9 on wider screens.
pub fn hud() -> Rect {
    let s = screen();
    let most = s.h * HUD_ASPECT;
    if wide() || s.w <= most {
        s
    } else {
        Rect::new((s.w - most) / 2.0, 0.0, most, s.h)
    }
}
/// The screen is wider than the HUD area (ultrawide with the centred HUD).
pub fn ultrawide() -> bool {
    hud().w + 1.0 < sw()
}

// ------------------------------------------------------------------------------------------ layout

/// Slicing rectangles for layout. `cut_*` take a strip off one side of `self` and return it, so a
/// panel is laid out by cutting its rows and columns off in order:
///
/// ```ignore
/// let mut r = panel.inset(24.0);
/// let title = r.cut_top(40.0, 12.0);   // 40 tall, 12 gap below
/// let buttons = r.cut_bottom(44.0, 0.0);
/// for (i, c) in buttons.cols(3, 8.0).into_iter().enumerate() { ... }
/// ```
pub trait RectExt: Sized {
    fn inset(&self, d: f32) -> Rect;
    fn pad(&self, x: f32, y: f32) -> Rect;
    fn grow(&self, d: f32) -> Rect;
    fn cut_top(&mut self, h: f32, gap: f32) -> Rect;
    fn cut_bottom(&mut self, h: f32, gap: f32) -> Rect;
    fn cut_left(&mut self, w: f32, gap: f32) -> Rect;
    fn cut_right(&mut self, w: f32, gap: f32) -> Rect;
    /// `n` equal columns with `gap` between.
    fn cols(&self, n: usize, gap: f32) -> Vec<Rect>;
    /// `n` equal rows with `gap` between.
    fn rows(&self, n: usize, gap: f32) -> Vec<Rect>;
    /// Columns sized by weight.
    fn cols_w(&self, w: &[f32], gap: f32) -> Vec<Rect>;
    /// A `w` x `h` box at a spot inside: `ax`, `ay` 0 = left/top, 0.5 = centre, 1 = right/bottom.
    fn anchor(&self, ax: f32, ay: f32, w: f32, h: f32) -> Rect;
    /// A `w` x `h` box centred inside (no bigger than self).
    fn centered(&self, w: f32, h: f32) -> Rect;
    fn center(&self) -> Vec2;
}

impl RectExt for Rect {
    fn inset(&self, d: f32) -> Rect {
        self.pad(d, d)
    }
    fn pad(&self, x: f32, y: f32) -> Rect {
        Rect::new(self.x + x, self.y + y, (self.w - 2.0 * x).max(0.0), (self.h - 2.0 * y).max(0.0))
    }
    fn grow(&self, d: f32) -> Rect {
        Rect::new(self.x - d, self.y - d, self.w + 2.0 * d, self.h + 2.0 * d)
    }
    fn cut_top(&mut self, h: f32, gap: f32) -> Rect {
        let h = h.min(self.h);
        let r = Rect::new(self.x, self.y, self.w, h);
        let d = (h + gap).min(self.h);
        self.y += d;
        self.h -= d;
        r
    }
    fn cut_bottom(&mut self, h: f32, gap: f32) -> Rect {
        let h = h.min(self.h);
        let r = Rect::new(self.x, self.y + self.h - h, self.w, h);
        self.h -= (h + gap).min(self.h);
        r
    }
    fn cut_left(&mut self, w: f32, gap: f32) -> Rect {
        let w = w.min(self.w);
        let r = Rect::new(self.x, self.y, w, self.h);
        let d = (w + gap).min(self.w);
        self.x += d;
        self.w -= d;
        r
    }
    fn cut_right(&mut self, w: f32, gap: f32) -> Rect {
        let w = w.min(self.w);
        let r = Rect::new(self.x + self.w - w, self.y, w, self.h);
        self.w -= (w + gap).min(self.w);
        r
    }
    fn cols(&self, n: usize, gap: f32) -> Vec<Rect> {
        self.cols_w(&vec![1.0; n], gap)
    }
    fn rows(&self, n: usize, gap: f32) -> Vec<Rect> {
        let n = n.max(1);
        let h = (self.h - gap * (n - 1) as f32) / n as f32;
        (0..n).map(|i| Rect::new(self.x, self.y + i as f32 * (h + gap), self.w, h)).collect()
    }
    fn cols_w(&self, w: &[f32], gap: f32) -> Vec<Rect> {
        let total: f32 = w.iter().sum::<f32>().max(1e-6);
        let room = self.w - gap * (w.len().max(1) - 1) as f32;
        let mut x = self.x;
        w.iter()
            .map(|k| {
                let cw = room * k / total;
                let r = Rect::new(x, self.y, cw, self.h);
                x += cw + gap;
                r
            })
            .collect()
    }
    fn anchor(&self, ax: f32, ay: f32, w: f32, h: f32) -> Rect {
        Rect::new(self.x + (self.w - w) * ax, self.y + (self.h - h) * ay, w, h)
    }
    fn centered(&self, w: f32, h: f32) -> Rect {
        self.anchor(0.5, 0.5, w.min(self.w), h.min(self.h))
    }
    fn center(&self) -> Vec2 {
        vec2(self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
}

/// Lays boxes out left to right, wrapping to a new row when one doesn't fit.
pub struct Flow {
    area: Rect,
    gap: f32,
    x: f32,
    y: f32,
    row_h: f32,
}
impl Flow {
    pub fn new(area: Rect, gap: f32) -> Flow {
        Flow { area, gap, x: area.x, y: area.y, row_h: 0.0 }
    }
    /// The next `w` x `h` box, or None when it would fall below the area.
    pub fn next(&mut self, w: f32, h: f32) -> Option<Rect> {
        if self.x > self.area.x && self.x + w > self.area.right() + 0.5 {
            self.x = self.area.x;
            self.y += self.row_h + self.gap;
            self.row_h = 0.0;
        }
        if self.y + h > self.area.bottom() + 0.5 {
            return None;
        }
        let r = Rect::new(self.x, self.y, w.min(self.area.w), h);
        self.x += w + self.gap;
        self.row_h = self.row_h.max(h);
        Some(r)
    }
    /// How far down the boxes so far reach.
    pub fn bottom(&self) -> f32 {
        self.y + self.row_h
    }
}

// ------------------------------------------------------------------------------------------ colours

/// The UI palette. Map colours live in `world::Theme`; these are only for panels and controls.
pub struct Pal {
    /// screen background behind menus
    pub bg: Color,
    pub panel: Color,
    /// raised things on a panel: buttons, cards, fields
    pub panel2: Color,
    pub line: Color,
    pub text: Color,
    pub dim: Color,
    pub accent: Color,
    /// text on an accent fill
    pub on_accent: Color,
    pub good: Color,
    pub warn: Color,
    pub bad: Color,
    pub shadow: Color,
}
pub fn pal(dark: bool) -> Pal {
    if dark {
        Pal {
            bg: rgba(14, 16, 21, 1.0),
            panel: rgba(20, 23, 30, 0.93),
            panel2: rgba(36, 41, 51, 1.0),
            line: rgba(255, 255, 255, 0.09),
            text: rgba(234, 237, 242, 1.0),
            dim: rgba(146, 154, 168, 1.0),
            // a cool tactical cyan (Devin: the old amber didn't fit the rest of the UI)
            accent: rgba(66, 180, 222, 1.0),
            on_accent: rgba(4, 20, 28, 1.0),
            good: rgba(81, 207, 102, 1.0),
            warn: rgba(255, 212, 59, 1.0),
            bad: rgba(255, 99, 99, 1.0),
            shadow: rgba(0, 0, 0, 0.35),
        }
    } else {
        Pal {
            bg: rgba(232, 235, 240, 1.0),
            panel: rgba(255, 255, 255, 0.95),
            panel2: rgba(238, 241, 245, 1.0),
            line: rgba(20, 30, 50, 0.12),
            text: rgba(28, 31, 38, 1.0),
            dim: rgba(96, 102, 114, 1.0),
            accent: rgba(232, 128, 0, 1.0),
            on_accent: rgba(255, 255, 255, 1.0),
            good: rgba(43, 138, 62, 1.0),
            warn: rgba(230, 150, 0, 1.0),
            bad: rgba(224, 49, 49, 1.0),
            shadow: rgba(10, 20, 40, 0.16),
        }
    }
}

fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a)
}
pub fn mix(a: Color, b: Color, t: f32) -> Color {
    Color::new(a.r + (b.r - a.r) * t, a.g + (b.g - a.g) * t, a.b + (b.b - a.b) * t, a.a + (b.a - a.a) * t)
}
pub fn alpha(c: Color, a: f32) -> Color {
    Color::new(c.r, c.g, c.b, a)
}

// ------------------------------------------------------------------------------------------ shapes

/// A filled rectangle with rounded corners.
pub fn rrect(r: Rect, rad: f32, c: Color) {
    let rad = rad.min(r.w / 2.0).min(r.h / 2.0).max(0.0);
    if rad < 0.5 {
        draw_rectangle(r.x, r.y, r.w, r.h, c);
        return;
    }
    draw_rectangle(r.x + rad, r.y, r.w - 2.0 * rad, r.h, c);
    draw_rectangle(r.x, r.y + rad, rad, r.h - 2.0 * rad, c);
    draw_rectangle(r.x + r.w - rad, r.y + rad, rad, r.h - 2.0 * rad, c);
    for (cx, cy, rot) in [
        (r.x + rad, r.y + rad, 180.0),
        (r.x + r.w - rad, r.y + rad, 270.0),
        (r.x + r.w - rad, r.y + r.h - rad, 0.0),
        (r.x + rad, r.y + r.h - rad, 90.0),
    ] {
        quarter(cx, cy, rad, rot, c);
    }
}
/// A quarter disc (a pie slice from `rot` degrees, clockwise on screen).
fn quarter(cx: f32, cy: f32, rad: f32, rot: f32, c: Color) {
    const N: usize = 6;
    let a0 = rot.to_radians();
    let mut prev = vec2(cx + rad * a0.cos(), cy + rad * a0.sin());
    for k in 1..=N {
        let a = a0 + std::f32::consts::FRAC_PI_2 * k as f32 / N as f32;
        let p = vec2(cx + rad * a.cos(), cy + rad * a.sin());
        draw_triangle(vec2(cx, cy), prev, p, c);
        prev = p;
    }
}
/// The outline of a rounded rectangle.
pub fn rrect_lines(r: Rect, rad: f32, t: f32, c: Color) {
    let rad = rad.min(r.w / 2.0).min(r.h / 2.0).max(0.0);
    draw_rectangle(r.x + rad, r.y, r.w - 2.0 * rad, t, c);
    draw_rectangle(r.x + rad, r.y + r.h - t, r.w - 2.0 * rad, t, c);
    draw_rectangle(r.x, r.y + rad, t, r.h - 2.0 * rad, c);
    draw_rectangle(r.x + r.w - t, r.y + rad, t, r.h - 2.0 * rad, c);
    if rad >= 0.5 {
        for (cx, cy, rot) in [
            (r.x + rad, r.y + rad, 180.0),
            (r.x + r.w - rad, r.y + rad, 270.0),
            (r.x + r.w - rad, r.y + r.h - rad, 0.0),
            (r.x + rad, r.y + r.h - rad, 90.0),
        ] {
            draw_arc(cx, cy, 40, rad - t / 2.0, rot, t, 90.0, c);
        }
    }
}
/// A soft drop shadow under a box.
pub fn shadow(r: Rect, rad: f32, c: Color) {
    for (k, a) in [(10.0, 0.18), (6.0, 0.3), (3.0, 0.5)] {
        rrect(Rect::new(r.x - k * 0.5, r.y - k * 0.2 + 3.0, r.w + k, r.h + k), rad + k * 0.5, alpha(c, c.a * a));
    }
}
/// A top-to-bottom gradient.
pub fn vgrad(r: Rect, top: Color, bottom: Color) {
    use macroquad::models::{Mesh, Vertex};
    let v = |x: f32, y: f32, c: Color| Vertex::new(x, y, 0.0, 0.0, 0.0, c);
    let mesh = Mesh {
        vertices: vec![v(r.x, r.y, top), v(r.x + r.w, r.y, top), v(r.x + r.w, r.y + r.h, bottom), v(r.x, r.y + r.h, bottom)],
        indices: vec![0, 1, 2, 0, 2, 3],
        texture: None,
    };
    draw_mesh(&mesh);
}
/// A left-to-right gradient.
pub fn hgrad(r: Rect, left: Color, right: Color) {
    use macroquad::models::{Mesh, Vertex};
    let v = |x: f32, y: f32, c: Color| Vertex::new(x, y, 0.0, 0.0, 0.0, c);
    let mesh = Mesh {
        vertices: vec![v(r.x, r.y, left), v(r.x + r.w, r.y, right), v(r.x + r.w, r.y + r.h, right), v(r.x, r.y + r.h, left)],
        indices: vec![0, 1, 2, 0, 2, 3],
        texture: None,
    };
    draw_mesh(&mesh);
}
/// A spinner of fading dots around `c`.
pub fn spinner(c: Vec2, rad: f32, time: f32, col: Color) {
    for k in 0..12 {
        let a = time * 4.0 + k as f32 * 0.52;
        let p = c + vec2(a.cos(), a.sin()) * rad;
        draw_circle(p.x, p.y, rad * 0.15, alpha(col, k as f32 / 12.0));
    }
}

//! The campaign's globe: the Earth between cities, to spin around and pick the next city on.
//!
//! The land comes from Natural Earth's 1:50m land polygons (public domain), baked into
//! assets/land.bin by tools/bake_land.py and painted once into an equirectangular texture. The
//! sphere is drawn without the depth buffer (some GPUs' depth doesn't work, see `render`): it's
//! convex, so culling its far side is enough.

use crate::render::{painter_material, text, text_w};
use macroquad::prelude::*;

static LAND: &[u8] = include_bytes!("../assets/land.bin");
const TW: usize = 2048;
const TH: usize = 1024;
const FOVY: f32 = 0.6;
/// sphere rows and columns; rows are split into meshes small enough for one draw call
const ROWS: usize = 48;
const COLS: usize = 96;
const BAND: usize = 8;

/// A point on the unit globe. Longitude 0 faces +z, east is +x, north is +y.
pub fn dir(lat: f32, lon: f32) -> Vec3 {
    let (la, lo) = (lat.to_radians(), lon.to_radians());
    vec3(la.cos() * lo.sin(), la.sin(), la.cos() * lo.cos())
}

/// A city pin on the globe.
pub struct Mark {
    pub lat: f32,
    pub lon: f32,
    pub col: Color,
    pub label: String,
    /// the picked city: a ring around it and a bigger label
    pub picked: bool,
}

pub struct Globe {
    tex: Texture2D,
    mat: Material,
    bands: Vec<Mesh>,
    /// what the middle of the view looks at, in degrees
    pub lat: f32,
    pub lon: f32,
    pub zoom: f32,
    goal: Option<(f32, f32)>,
    drag: Option<(Vec2, f32)>,
}

impl Globe {
    pub fn new() -> Globe {
        let tex = Texture2D::from_rgba8(TW as u16, TH as u16, &paint());
        tex.set_filter(FilterMode::Linear);
        let mut bands = Vec::new();
        for r0 in (0..ROWS).step_by(BAND) {
            let rows = BAND.min(ROWS - r0);
            let mut vertices = Vec::new();
            for r in r0..=r0 + rows {
                let lat = 90.0 - 180.0 * r as f32 / ROWS as f32;
                for c in 0..=COLS {
                    let lon = -180.0 + 360.0 * c as f32 / COLS as f32;
                    let p = dir(lat, lon);
                    vertices.push(Vertex::new(p.x, p.y, p.z, c as f32 / COLS as f32, r as f32 / ROWS as f32, WHITE));
                }
            }
            let mut indices = Vec::new();
            let w = (COLS + 1) as u16;
            for r in 0..rows as u16 {
                for c in 0..COLS as u16 {
                    let (a, b, d, e) = (r * w + c, r * w + c + 1, (r + 1) * w + c, (r + 1) * w + c + 1);
                    indices.extend_from_slice(&[a, d, b, b, d, e]);
                }
            }
            bands.push(Mesh { vertices, indices, texture: Some(tex.clone()) });
        }
        Globe { tex, mat: painter_material(true), bands, lat: 25.0, lon: -40.0, zoom: 1.0, goal: None, drag: None }
    }

    /// Turns the globe to bring a point to the middle, smoothly.
    pub fn focus(&mut self, lat: f32, lon: f32) {
        self.goal = Some((lat.clamp(-60.0, 70.0), lon));
    }

    /// Draws the globe centred at `c` (UI points, inside `ui::begin`) with radius `r` at zoom 1, and its pins. `input`:
    /// whether the mouse is over the globe's part of the screen. Returns the pin under the mouse and
    /// the pin clicked, if any.
    pub fn draw(&mut self, c: Vec2, r: f32, marks: &[Mark], font: &Font, input: bool, dt: f32) -> (Option<usize>, Option<usize>) {
        let (sw, sh) = (crate::ui::sw(), crate::ui::sh());
        let rad = r * self.zoom;
        // turning: drag, wheel, or easing towards a picked city
        let m = crate::ui::mouse();
        if input {
            let wheel = mouse_wheel().1;
            if wheel != 0.0 {
                self.zoom = (self.zoom * if wheel > 0.0 { 1.12 } else { 1.0 / 1.12 }).clamp(0.7, 2.6);
            }
            if is_mouse_button_pressed(MouseButton::Left) {
                self.drag = Some((m, 0.0));
            }
        }
        let mut click = false;
        if let Some((last, moved)) = self.drag {
            let d = m - last;
            let k = 57.3 / rad.max(1.0);
            self.lon -= d.x * k;
            self.lat = (self.lat + d.y * k).clamp(-60.0, 70.0);
            self.drag = Some((m, moved + d.length()));
            if d.length() > 0.0 {
                self.goal = None;
            }
            if !is_mouse_button_down(MouseButton::Left) {
                click = moved + d.length() < 6.0;
                self.drag = None;
            }
        }
        if let Some((gl, go)) = self.goal {
            let dl = ((go - self.lon + 540.0).rem_euclid(360.0)) - 180.0;
            let f = 1.0 - (-dt * 4.0).exp();
            self.lon += dl * f;
            self.lat += (gl - self.lat) * f;
            if dl.abs() < 0.05 && (gl - self.lat).abs() < 0.05 {
                self.goal = None;
            }
        }
        self.lon = (self.lon + 540.0).rem_euclid(360.0) - 180.0;

        // a camera that shows the unit sphere `rad` pixels across, centred on `c`
        let t = rad / (sh / 2.0) * (FOVY / 2.0).tan();
        let dist = 1.0 / t.atan().sin();
        let view = dir(self.lat, self.lon);
        let cam = Camera3D {
            position: view * dist,
            target: Vec3::ZERO,
            up: vec3(0.0, 1.0, 0.0),
            fovy: FOVY,
            z_near: 0.05,
            z_far: dist + 2.0,
            ..Default::default()
        };
        // shift the projection so the globe's middle lands on `c`
        let shift = Mat4::from_translation(vec3((c.x / sw) * 2.0 - 1.0, 1.0 - (c.y / sh) * 2.0, 0.0));
        let mvp = shift * cam.matrix();

        // haze around the rim
        for k in 0..10 {
            let f = k as f32 / 10.0;
            draw_circle(c.x, c.y, rad * (1.0 + 0.05 * (1.0 - f)), Color::new(0.30, 0.55, 0.95, 0.018 + 0.012 * f));
        }

        // the sphere, lit from up and to the left of the viewer
        let right = vec3(0.0, 1.0, 0.0).cross(view).normalize_or_zero();
        let light = (view * 0.8 - right * 0.45 + vec3(0.0, 0.35, 0.0)).normalize();
        for b in &mut self.bands {
            for v in &mut b.vertices {
                let n = v.position;
                let l = 0.28 + 0.72 * n.dot(light).max(0.0);
                let rim = 1.0 - n.dot(view).max(0.0);
                let k = (l + rim * rim * 0.25).min(1.15);
                let ch = |x: f32| (x * 255.0).clamp(0.0, 255.0) as u8;
                v.color = [ch(k), ch(k), ch(k * 1.04), 255];
            }
        }
        push_camera_state();
        set_camera(&GlobeCam(mvp));
        gl_use_material(&self.mat);
        for b in &self.bands {
            draw_mesh(b);
        }
        gl_use_default_material();
        pop_camera_state();

        // pins
        let horizon = 1.0 / dist;
        let mut hover = None;
        let mut best = 18.0f32;
        let mut shown = Vec::new();
        for (i, mk) in marks.iter().enumerate() {
            let p = dir(mk.lat, mk.lon);
            let facing = p.dot(view);
            if facing < horizon + 0.02 {
                continue;
            }
            let q = mvp * (p * 1.004).extend(1.0);
            let s = vec2((q.x / q.w + 1.0) / 2.0 * sw, (1.0 - q.y / q.w) / 2.0 * sh);
            // pins at the rim fade out instead of popping
            let a = ((facing - horizon) / 0.15).clamp(0.0, 1.0);
            shown.push((i, s, a));
            let d = s.distance(m);
            if input && d < best {
                best = d;
                hover = Some(i);
            }
        }
        // the picked city's label first, so it keeps the best spot
        shown.sort_by_key(|(i, _, _)| !marks[*i].picked);
        let mut taken: Vec<Rect> = Vec::new();
        for &(i, s, a) in &shown {
            let mk = &marks[i];
            let hot = hover == Some(i);
            let size = if mk.picked { 7.0 } else { 5.0 } + if hot { 1.5 } else { 0.0 };
            draw_circle(s.x, s.y, size + 2.5, Color::new(0.0, 0.0, 0.0, 0.55 * a));
            draw_circle(s.x, s.y, size, Color::new(mk.col.r, mk.col.g, mk.col.b, a));
            if mk.picked {
                let pulse = (get_time() as f32 * 3.0).sin() * 0.5 + 0.5;
                draw_circle_lines(s.x, s.y, size + 6.0 + pulse * 4.0, 2.0, Color::new(1.0, 1.0, 1.0, 0.8 * a));
            }
            let fs = if mk.picked || hot { 17.0 } else { 13.0 };
            let tw = text_w(font, &mk.label, fs);
            // above the pin, or below, right or left when that's taken by another label
            let spots = [
                vec2(s.x - tw / 2.0, s.y - size - 9.0),
                vec2(s.x - tw / 2.0, s.y + size + fs + 4.0),
                vec2(s.x + size + 6.0, s.y + fs * 0.35),
                vec2(s.x - size - 6.0 - tw, s.y + fs * 0.35),
            ];
            let rect = |p: Vec2| Rect::new(p.x - 2.0, p.y - fs, tw + 4.0, fs + 4.0);
            let at = spots.iter().copied().find(|p| !taken.iter().any(|t: &Rect| t.overlaps(&rect(*p)))).unwrap_or(spots[0]);
            taken.push(rect(at));
            let (tx, ty) = (at.x, at.y);
            text(font, &mk.label, tx + 1.0, ty + 1.0, fs, Color::new(0.0, 0.0, 0.0, 0.7 * a));
            text(font, &mk.label, tx, ty, fs, Color::new(1.0, 1.0, 1.0, if mk.picked || hot { a } else { 0.8 * a }));
        }
        (hover, if click { hover } else { None })
    }
}

/// A fixed matrix as a camera, for the shifted globe projection.
struct GlobeCam(Mat4);
impl Camera for GlobeCam {
    fn matrix(&self) -> Mat4 {
        self.0
    }
    fn depth_enabled(&self) -> bool {
        false
    }
    fn render_pass(&self) -> Option<RenderPass> {
        None
    }
    fn viewport(&self) -> Option<(i32, i32, i32, i32)> {
        None
    }
}

/// The land rings from assets/land.bin: lon/lat in degrees.
fn rings() -> Vec<Vec<Vec2>> {
    let rd = |o: usize| u32::from_le_bytes([LAND[o], LAND[o + 1], LAND[o + 2], LAND[o + 3]]) as usize;
    let rs = |o: usize| i16::from_le_bytes([LAND[o], LAND[o + 1]]) as f32 / 100.0;
    let mut o = 4;
    (0..rd(0))
        .map(|_| {
            let n = rd(o);
            o += 4;
            let r = (0..n).map(|k| vec2(rs(o + k * 4), rs(o + k * 4 + 2))).collect();
            o += n * 4;
            r
        })
        .collect()
}

/// The Earth texture: sea, land, coastlines and a faint grid.
fn paint() -> Vec<u8> {
    let sea = [14u8, 34, 58];
    let deep = [9u8, 24, 44];
    let land = [62u8, 78, 66];
    let coast = [120u8, 150, 128];
    let mut img = vec![0u8; TW * TH * 4];
    let px = |p: Vec2| vec2((p.x + 180.0) / 360.0 * TW as f32, (90.0 - p.y) / 180.0 * TH as f32);
    let put = |img: &mut Vec<u8>, x: usize, y: usize, c: [u8; 3]| {
        let o = (y * TW + x) * 4;
        img[o..o + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
    };
    for y in 0..TH {
        let lat = 90.0 - 180.0 * (y as f32 + 0.5) / TH as f32;
        let k = (lat.abs() / 90.0).powf(1.5);
        let c = [0, 1, 2].map(|i| (sea[i] as f32 + (deep[i] as f32 - sea[i] as f32) * k) as u8);
        for x in 0..TW {
            put(&mut img, x, y, c);
        }
    }
    // a 30° grid on the sea
    for g in (-150..=180).step_by(30) {
        let x = (((g + 180) as f32 / 360.0 * TW as f32) as usize).min(TW - 1);
        for y in 0..TH {
            put(&mut img, x, y, [24, 52, 84]);
        }
    }
    for g in (-60..=60).step_by(30) {
        let y = (((90 - g) as f32 / 180.0 * TH as f32) as usize).min(TH - 1);
        for x in 0..TW {
            put(&mut img, x, y, [24, 52, 84]);
        }
    }
    let rings = rings();
    for r in &rings {
        let pts: Vec<Vec2> = r.iter().map(|p| px(*p)).collect();
        fill(&pts, |x, y| put(&mut img, x, y, land));
    }
    for r in &rings {
        for k in 0..r.len() {
            let (a, b) = (px(r[k]), px(r[(k + 1) % r.len()]));
            // ring edges along the map's border (Antarctica's base) aren't coast
            if (a.x - b.x).abs() > TW as f32 / 2.0 || (a.y > TH as f32 - 2.0 && b.y > TH as f32 - 2.0) {
                continue;
            }
            let n = (a.distance(b) * 1.5).ceil().max(1.0) as usize;
            for s in 0..=n {
                let p = a.lerp(b, s as f32 / n as f32);
                let (x, y) = (p.x as usize, p.y as usize);
                if x < TW && y < TH {
                    put(&mut img, x, y, coast);
                }
            }
        }
    }
    img
}

/// Scanline fill of a polygon in texture pixels.
fn fill(poly: &[Vec2], mut f: impl FnMut(usize, usize)) {
    let n = poly.len();
    let (y0, y1) = poly.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.y), b.max(p.y)));
    let mut xs: Vec<f32> = Vec::new();
    for y in (y0.floor().max(0.0) as usize)..=(y1.ceil() as usize).min(TH - 1) {
        let sy = y as f32 + 0.5;
        xs.clear();
        for i in 0..n {
            let (a, b) = (poly[i], poly[(i + n - 1) % n]);
            if (a.y > sy) != (b.y > sy) {
                xs.push(a.x + (sy - a.y) / (b.y - a.y) * (b.x - a.x));
            }
        }
        xs.sort_by(|a, b| a.total_cmp(b));
        for s in xs.chunks_exact(2) {
            let xa = (s[0] - 0.5).ceil().max(0.0) as usize;
            let xb = ((s[1] - 0.5).floor() as i64).min(TW as i64 - 1);
            for x in xa as i64..=xb {
                f(x as usize, y);
            }
        }
    }
}

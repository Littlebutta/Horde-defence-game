//! Drawing. The map (ground, roads, water, 2.5D city blocks) is baked into vertex meshes and only
//! rebuilt when something changes. The horde is one map-sized texture refreshed from the density grid
//! every tick, with a soft glow underneath that grows with the size of the crowd. Placeable player
//! buildings and army units keep their bold colours; everything else follows a Google Maps look.

use crate::defs::*;
use crate::game::*;
use crate::swarm::{TINT_DOWN, TINT_FLY, TINT_RAGE};
use crate::world::*;
use macroquad::models::{Mesh, Vertex};
use macroquad::prelude::*;
use rayon::prelude::*;
use std::sync::atomic::Ordering::Relaxed;

pub struct Fonts {
    pub head: Font,
    pub mono: Font,
}

pub fn load_fonts() -> Fonts {
    let mut head =
        load_ttf_font_from_bytes(include_bytes!("../assets/ChakraPetch-SemiBold.ttf")).unwrap();
    let mut mono =
        load_ttf_font_from_bytes(include_bytes!("../assets/IBMPlexMono-Medium.ttf")).unwrap();
    head.set_filter(FilterMode::Linear);
    mono.set_filter(FilterMode::Linear);
    Fonts { head, mono }
}

pub fn text(f: &Font, s: &str, x: f32, y: f32, size: f32, c: Color) {
    draw_text_ex(
        s,
        x,
        y,
        TextParams {
            font: Some(f),
            font_size: size as u16,
            color: c,
            ..Default::default()
        },
    );
}
pub fn text_w(f: &Font, s: &str, size: f32) -> f32 {
    measure_text(s, Some(f), size as u16, 1.0).width
}

// ------------------------------------------------------------------------------------------ meshes

/// Accumulates triangles into meshes of at most 65k vertices.
#[derive(Default)]
struct MB {
    out: Vec<Mesh>,
    v: Vec<Vertex>,
    i: Vec<u16>,
}
impl MB {
    fn room(&mut self, n: usize) {
        if self.v.len() + n > 9000 || self.i.len() + n * 3 > 4800 {
            self.flush();
        }
    }
    fn flush(&mut self) {
        if !self.v.is_empty() {
            self.out.push(Mesh {
                vertices: std::mem::take(&mut self.v),
                indices: std::mem::take(&mut self.i),
                texture: None,
            });
        }
    }
    fn vert(&mut self, p: Vec2, c: Color) -> u16 {
        self.v.push(Vertex::new(p.x, p.y, 0.0, 0.0, 0.0, c));
        (self.v.len() - 1) as u16
    }
    fn quad(&mut self, a: Vec2, b: Vec2, c: Vec2, d: Vec2, col: Color) {
        self.room(4);
        let (ia, ib, ic, id) = (
            self.vert(a, col),
            self.vert(b, col),
            self.vert(c, col),
            self.vert(d, col),
        );
        self.i.extend_from_slice(&[ia, ib, ic, ia, ic, id]);
    }
    fn poly(&mut self, pts: &[Vec2], col: Color) {
        if pts.len() < 3 {
            return;
        }
        if pts.len() > 60000 {
            return;
        }
        self.room(pts.len());
        let flat: Vec<f64> = pts.iter().flat_map(|p| [p.x as f64, p.y as f64]).collect();
        let Ok(tris) = earcutr::earcut(&flat, &[], 2) else {
            return;
        };
        let base = self.v.len() as u16;
        for p in pts {
            self.vert(*p, col);
        }
        for t in tris {
            self.i.push(base + t as u16);
        }
    }
    fn disc(&mut self, c: Vec2, r: f32, col: Color) {
        let n = if r > 6.0 { 16 } else { 10 };
        self.room(n + 1);
        let ic = self.vert(c, col);
        let first = self.v.len() as u16;
        for k in 0..n {
            let a = k as f32 / n as f32 * std::f32::consts::TAU;
            self.vert(c + vec2(a.cos(), a.sin()) * r, col);
        }
        for k in 0..n as u16 {
            self.i
                .extend_from_slice(&[ic, first + k, first + (k + 1) % n as u16]);
        }
    }
    fn line(&mut self, pts: &[Vec2], w: f32, col: Color) {
        let r = w / 2.0;
        for s in pts.windows(2) {
            let d = s[1] - s[0];
            let l = d.length();
            if l < 1e-4 {
                continue;
            }
            let n = vec2(-d.y, d.x) / l * r;
            self.quad(s[0] + n, s[1] + n, s[1] - n, s[0] - n, col);
        }
        if r > 0.8 {
            for p in pts {
                self.disc(*p, r, col);
            }
        }
    }
    fn dashed(&mut self, pts: &[Vec2], w: f32, dash: f32, gap: f32, col: Color) {
        let mut on = true;
        let mut left = dash;
        for s in pts.windows(2) {
            let (mut a, b) = (s[0], s[1]);
            loop {
                let rem = (b - a).length();
                if rem < 1e-4 {
                    break;
                }
                let step = rem.min(left);
                let nb = a + (b - a) / rem * step;
                if on {
                    self.line(&[a, nb], w, col);
                }
                left -= step;
                a = nb;
                if left <= 1e-4 {
                    on = !on;
                    left = if on { dash } else { gap };
                }
            }
        }
    }
    fn finish(mut self) -> Vec<Mesh> {
        self.flush();
        if std::env::var("PH_DEBUG").is_ok() {
            for m in &self.out {
                eprintln!("mesh {} v {} i", m.vertices.len(), m.indices.len());
            }
        }
        self.out
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

fn build_ground(w: &World, th: &Theme) -> Vec<Mesh> {
    let d = &w.desc;
    let mut mb = MB::default();
    let (wf, hf) = (W as f32, H as f32);
    mb.quad(
        vec2(0.0, 0.0),
        vec2(wf, 0.0),
        vec2(wf, hf),
        vec2(0.0, hf),
        th.land,
    );
    for p in &d.parks {
        mb.poly(p, th.park);
    }
    if let Some(a) = &d.airport {
        mb.poly(a, th.air);
    }
    for a in &d.aprons {
        mb.poly(a, th.apron);
    }
    for r in &d.runways {
        mb.line(&r.pts, r.w, th.runway);
        if r.w >= 10.0 {
            mb.dashed(&r.pts, 0.8, 8.0, 6.0, th.runway_mark);
        }
    }
    for p in &d.water {
        mb.poly(p, th.water_edge);
    }
    for l in &d.water_lines {
        mb.line(&l.pts, l.w + 2.0, th.water_edge);
    }
    for p in &d.water {
        let c = centroid(p);
        let inner: Vec<Vec2> = p.iter().map(|q| c + (*q - c) * 0.985).collect();
        mb.poly(&inner, th.water);
    }
    for l in &d.water_lines {
        mb.line(&l.pts, l.w, th.water);
    }
    // casings first, then fills, minor to major so big roads sit on top at junctions
    let mut roads: Vec<&Road> = d.roads.iter().collect();
    roads.sort_by_key(|r| r.rank);
    for r in &roads {
        mb.line(&r.pts, r.w + 1.4, th.casing);
    }
    for r in &roads {
        mb.line(
            &r.pts,
            r.w,
            if r.rank >= 3 { th.road_major } else { th.road },
        );
    }
    // flat base zone ground (the paved airport/base area) is a texture overlay, drawn later
    mb.finish()
}

/// City blocks in 2.5D: walls facing the viewer, then the roof lifted by the building's height.
/// Destroyed buildings become flat rubble.
fn build_buildings(g: &Game, th: &Theme) -> (Vec<Mesh>, Vec<Mesh>) {
    let tilt = g.tilt;
    let mut order: Vec<usize> = (0..g.world.buildings.len()).collect();
    order.sort_by(|&a, &b| {
        g.world.buildings[a]
            .y1
            .partial_cmp(&g.world.buildings[b].y1)
            .unwrap()
    });
    let mut mb = MB::default();
    let mut rubble = MB::default();
    let infected: std::collections::HashSet<usize> = g
        .hives
        .iter()
        .filter(|h| !h.dead)
        .flat_map(|h| h.infected.iter().copied())
        .collect();
    let hive_col = rgb(150, 70, 190);
    for bi in order {
        let b = &g.world.buildings[bi];
        if !b.alive {
            rubble.poly(&b.poly, th.rubble);
            continue;
        }
        let off = vec2(0.35 * b.h * tilt, -b.h * tilt);
        let inf = infected.contains(&bi);
        let ccw = b.area > 0.0;
        let n = b.poly.len();
        if tilt > 0.0 {
            for k in 0..n {
                let a = b.poly[k];
                let c = b.poly[(k + 1) % n];
                let e = c - a;
                let mut nrm = vec2(e.y, -e.x).normalize_or_zero();
                if !ccw {
                    nrm = -nrm;
                }
                if nrm.dot(off) >= 0.0 {
                    continue;
                }
                let shade = (0.5 + 0.5 * nrm.x).clamp(0.0, 1.0);
                let mut col = mix(th.side_a, th.side_b, shade);
                if inf {
                    col = mix(col, hive_col, 0.45);
                }
                mb.quad(a, c, c + off, a + off, col);
                // window bands on taller blocks
                if b.h > 9.0 && e.length() > 3.0 {
                    let floors = (b.h / 3.2) as i32;
                    for f in 1..floors {
                        let t = f as f32 / floors as f32;
                        let (p0, p1) = (a + off * t, c + off * t);
                        let inset = e.normalize() * 0.6;
                        mb.line(&[p0 + inset, p1 - inset], 0.35 * tilt.max(0.3), th.window);
                    }
                }
            }
        }
        let roof: Vec<Vec2> = b.poly.iter().map(|p| *p + off).collect();
        let mut col = if b.h > 15.0 { th.roof_tall } else { th.roof };
        if inf {
            col = mix(col, hive_col, 0.55);
        }
        let dmg = 1.0 - (b.hp / b.max).clamp(0.0, 1.0);
        if dmg > 0.05 {
            col = mix(col, th.rubble, dmg * 0.6);
        }
        mb.line(&[roof.as_slice(), &roof[..1]].concat(), 0.6, th.outline);
        mb.poly(&roof, col);
    }
    (mb.finish(), rubble.finish())
}

pub struct Scene {
    ground: Vec<Mesh>,
    bmesh: Vec<Mesh>,
    rubble: Vec<Mesh>,
    units: Texture2D,
    ubuf: Vec<u8>,
    glow: Texture2D,
    gbuf: Vec<u8>,
    zone: Texture2D,
    zbuf: Vec<u8>,
    pub dark: bool,
    btimer: f32,
    bsig: (usize, usize, i32),
    pub need_buildings: bool,
    pub need_zone: bool,
    pub units_fresh: bool,
}

const ZS: usize = 4; // zone overlay texel size in cells
const ZW: usize = W / ZS;
const ZH: usize = H / ZS;

impl Scene {
    pub fn new(g: &Game, th: &Theme) -> Scene {
        let units = Texture2D::from_rgba8(W as u16, H as u16, &vec![0u8; N * 4]);
        units.set_filter(FilterMode::Nearest);
        let glow = Texture2D::from_rgba8(CW as u16, CH as u16, &vec![0u8; CW * CH * 4]);
        glow.set_filter(FilterMode::Linear);
        let zone = Texture2D::from_rgba8(ZW as u16, ZH as u16, &vec![0u8; ZW * ZH * 4]);
        zone.set_filter(FilterMode::Linear);
        let (bmesh, rubble) = build_buildings(g, th);
        Scene {
            ground: build_ground(&g.world, th),
            bmesh,
            rubble,
            units,
            ubuf: vec![0; N * 4],
            glow,
            gbuf: vec![0; CW * CH * 4],
            zone,
            zbuf: vec![0; ZW * ZH * 4],
            dark: th.dark,
            btimer: 0.0,
            bsig: (0, 0, 0),
            need_buildings: false,
            need_zone: true,
            units_fresh: false,
        }
    }

    pub fn retheme(&mut self, g: &Game, th: &Theme) {
        self.ground = build_ground(&g.world, th);
        let (a, b) = build_buildings(g, th);
        self.bmesh = a;
        self.rubble = b;
        self.dark = th.dark;
        self.need_zone = true;
    }

    /// Refresh the horde texture and glow from this tick's density grid.
    pub fn update_units(&mut self, g: &Game) {
        let f = g.faction();
        let col = f.col;
        let deep = f.deep;
        let dens = &g.swarm.dens;
        let tint = &g.swarm.tint;
        let dark = self.dark;
        self.ubuf
            .par_chunks_mut(W * 4)
            .enumerate()
            .for_each(|(y, row)| {
                for x in 0..W {
                    let c = y * W + x;
                    let d = dens[c].load(Relaxed);
                    let t = tint[c].load(Relaxed);
                    let o = x * 4;
                    if d == 0 && t != TINT_DOWN {
                        row[o + 3] = 0;
                        continue;
                    }
                    let (mut r, mut gg, mut b) = (col[0] as f32, col[1] as f32, col[2] as f32);
                    let mut a = 255.0;
                    if t == TINT_DOWN && d == 0 {
                        r = deep[0] as f32;
                        gg = deep[1] as f32;
                        b = deep[2] as f32;
                        a = 150.0;
                    } else if t == TINT_FLY {
                        r += (255.0 - r) * 0.6;
                        gg += (255.0 - gg) * 0.6;
                        b += (255.0 - b) * 0.6;
                    } else if t >= TINT_RAGE {
                        let k = (t - TINT_RAGE) as f32 / 251.0;
                        r += (255.0 - r) * k;
                        gg *= 1.0 - 0.85 * k;
                        b *= 1.0 - 0.9 * k;
                    }
                    // crowded cells read brighter (dark map) or deeper (light map)
                    let k = ((d as f32 - 1.0) / 5.0).clamp(0.0, 1.0);
                    if dark {
                        r += (255.0 - r) * k * 0.35;
                        gg += (255.0 - gg) * k * 0.35;
                        b += (255.0 - b) * k * 0.35;
                    } else {
                        r *= 1.0 - 0.3 * k;
                        gg *= 1.0 - 0.3 * k;
                        b *= 1.0 - 0.3 * k;
                    }
                    row[o] = r as u8;
                    row[o + 1] = gg as u8;
                    row[o + 2] = b as u8;
                    row[o + 3] = a as u8;
                }
            });
        self.units.update_from_bytes(W as u32, H as u32, &self.ubuf);
        self.units_fresh = true;
        for i in 0..CW * CH {
            let n = g.swarm.coarse_at(i) as f32;
            let a = if n <= 0.0 {
                0.0
            } else {
                (n.sqrt() / 7.0).min(0.7)
            };
            self.gbuf[i * 4] = col[0];
            self.gbuf[i * 4 + 1] = col[1];
            self.gbuf[i * 4 + 2] = col[2];
            self.gbuf[i * 4 + 3] = (a * 255.0) as u8;
        }
        self.glow
            .update_from_bytes(CW as u32, CH as u32, &self.gbuf);
    }

    fn update_zone(&mut self, g: &Game, th: &Theme) {
        let zc = th.zone;
        let ze = th.zone_edge;
        let base = th.base;
        let rad = rgba(190, 230, 40, 0.30);
        let hive = rgba(170, 80, 220, 0.20);
        let w = &g.world;
        let zone_at = |x: usize, y: usize| -> bool {
            w.zone[(y * ZS + ZS / 2).min(H - 1) * W + (x * ZS + ZS / 2).min(W - 1)] == 1
        };
        for y in 0..ZH {
            for x in 0..ZW {
                let o = (y * ZW + x) * 4;
                let p = vec2((x * ZS) as f32 + 2.0, (y * ZS) as f32 + 2.0);
                let mut c = Color::new(0.0, 0.0, 0.0, 0.0);
                if zone_at(x, y) {
                    let edge = (x > 0 && !zone_at(x - 1, y))
                        || (x + 1 < ZW && !zone_at(x + 1, y))
                        || (y > 0 && !zone_at(x, y - 1))
                        || (y + 1 < ZH && !zone_at(x, y + 1));
                    c = if edge {
                        ze
                    } else {
                        mix(alpha(base, 0.0), zc, 1.0)
                    };
                }
                if w.irradiated_at(p) {
                    c = mix(c, rad, 0.85);
                }
                if g.hive_blocks(p) {
                    c = mix(c, hive, 0.8);
                }
                self.zbuf[o] = (c.r * 255.0) as u8;
                self.zbuf[o + 1] = (c.g * 255.0) as u8;
                self.zbuf[o + 2] = (c.b * 255.0) as u8;
                self.zbuf[o + 3] = (c.a * 255.0) as u8;
            }
        }
        self.zone
            .update_from_bytes(ZW as u32, ZH as u32, &self.zbuf);
    }

    pub fn maintain(&mut self, g: &mut Game, th: &Theme, dt: f32) {
        self.btimer -= dt;
        let inf: usize = g
            .hives
            .iter()
            .filter(|h| !h.dead)
            .map(|h| h.infected.len())
            .sum();
        let sig = (
            g.world.buildings.iter().filter(|b| b.alive).count(),
            inf,
            (g.tilt * 100.0) as i32,
        );
        if !g.dirty_buildings.is_empty() || sig != self.bsig || self.need_buildings {
            if self.btimer <= 0.0 {
                g.dirty_buildings.clear();
                self.bsig = sig;
                self.need_buildings = false;
                let (a, b) = build_buildings(g, th);
                self.bmesh = a;
                self.rubble = b;
                self.btimer = 0.3;
            }
        }
        // damage tint on roofs: refresh now and then while fighting
        if self.btimer < -2.0 && g.time > 0.0 {
            self.need_buildings = true;
        }
        if g.zone_dirty
            || self.need_zone
            || (g.time as i32 % 2 == 0 && !g.hives.is_empty() && self.btimer <= 0.0)
        {
            g.zone_dirty = false;
            self.need_zone = false;
            self.update_zone(g, th);
        }
    }
}

// ------------------------------------------------------------------------------------------ shapes

fn rot(v: Vec2, a: f32) -> Vec2 {
    let (s, c) = a.sin_cos();
    vec2(v.x * c - v.y * s, v.x * s + v.y * c)
}
fn fan(c: Vec2, pts: &[Vec2], a: f32, s: f32, col: Color) {
    let n = pts.len();
    let ctr = pts.iter().fold(Vec2::ZERO, |acc, p| acc + *p) / n as f32;
    let cc = c + rot(ctr * s, a);
    for k in 0..n {
        draw_triangle(
            cc,
            c + rot(pts[k] * s, a),
            c + rot(pts[(k + 1) % n] * s, a),
            col,
        );
    }
}
fn ellipse_pts(rx: f32, ry: f32, cx: f32, n: usize) -> Vec<Vec2> {
    (0..n)
        .map(|k| {
            let t = k as f32 / n as f32 * std::f32::consts::TAU;
            vec2(cx + t.cos() * rx, t.sin() * ry)
        })
        .collect()
}
fn rect_rot(c: Vec2, x0: f32, y0: f32, x1: f32, y1: f32, a: f32, s: f32, col: Color) {
    let p = [vec2(x0, y0), vec2(x1, y0), vec2(x1, y1), vec2(x0, y1)].map(|v| c + rot(v * s, a));
    draw_triangle(p[0], p[1], p[2], col);
    draw_triangle(p[0], p[2], p[3], col);
}

/// A detailed little helicopter seen from above at an angle: shadow on the ground, body lifted by altitude.
pub fn draw_heli(
    ground: Vec2,
    alt: f32,
    heading: f32,
    time: f32,
    gunship: bool,
    cargo: bool,
    tilt: f32,
    k: f32,
) {
    let s = 0.55 * k;
    let body = ground + vec2(0.35 * alt * tilt, -alt * tilt);
    // shadow
    let sh = Color::new(0.0, 0.0, 0.0, 0.22);
    fan(ground, &ellipse_pts(4.2, 2.3, 0.0, 12), heading, s, sh);
    rect_rot(ground, -9.0, -0.6, -2.0, 0.6, heading, s, sh);
    draw_circle(ground.x, ground.y, 6.0 * s, Color::new(0.0, 0.0, 0.0, 0.08));
    let (hull, dark, glass) = if gunship {
        (rgb(58, 66, 50), rgb(34, 38, 30), rgb(120, 200, 230))
    } else {
        (rgb(86, 104, 68), rgb(52, 62, 42), rgb(150, 215, 240))
    };
    if cargo {
        let cpos = ground + vec2(0.35 * alt * tilt * 0.45, -alt * tilt * 0.45);
        draw_line(body.x, body.y, cpos.x, cpos.y, 0.25, rgb(40, 40, 40));
        rect_rot(cpos, -1.6, -1.6, 1.6, 1.6, heading, s, rgb(160, 120, 70));
        rect_rot(cpos, -1.6, -0.2, 1.6, 0.2, heading, s, rgb(110, 80, 45));
    }
    // skids
    for side in [-2.3f32, 2.3] {
        rect_rot(body, -3.2, side - 0.2, 3.0, side + 0.2, heading, s, dark);
        rect_rot(
            body,
            -1.8,
            side.signum() * 1.4,
            -1.5,
            side,
            heading,
            s,
            dark,
        );
        rect_rot(body, 1.4, side.signum() * 1.4, 1.7, side, heading, s, dark);
    }
    // tail boom, fin, stabiliser
    rect_rot(body, -9.5, -0.55, -2.5, 0.55, heading, s, hull);
    rect_rot(body, -9.8, -2.0, -8.8, 2.0, heading, s, dark);
    if gunship {
        // stub wings with rocket pods
        rect_rot(body, -1.2, -4.6, 0.6, 4.6, heading, s, dark);
        for side in [-4.0f32, 4.0] {
            rect_rot(
                body,
                -1.6,
                side - 0.5,
                1.4,
                side + 0.5,
                heading,
                s,
                rgb(30, 30, 30),
            );
        }
    }
    // fuselage and cockpit
    fan(body, &ellipse_pts(4.3, 2.2, 0.0, 16), heading, s, dark);
    fan(body, &ellipse_pts(4.0, 1.9, 0.0, 16), heading, s, hull);
    fan(body, &ellipse_pts(1.7, 1.4, 2.3, 12), heading, s, glass);
    fan(
        body,
        &ellipse_pts(0.6, 0.5, 2.7, 8),
        heading,
        s,
        Color::new(1.0, 1.0, 1.0, 0.6),
    );
    // engine housing
    rect_rot(body, -1.8, -0.9, 0.6, 0.9, heading, s, dark);
    // main rotor: blur disc + two blades
    let rc = body + rot(vec2(-0.4, 0.0) * s, heading);
    draw_circle(rc.x, rc.y, 6.2 * s, Color::new(0.1, 0.1, 0.1, 0.12));
    let ra = time * 28.0;
    for k in 0..2 {
        let a = ra + k as f32 * std::f32::consts::PI;
        rect_rot(
            rc,
            -6.2,
            -0.28,
            6.2,
            0.28,
            a,
            s,
            Color::new(0.12, 0.12, 0.12, 0.75),
        );
        let _ = a;
    }
    draw_circle(rc.x, rc.y, 0.6 * s, rgb(25, 25, 25));
    // tail rotor
    let tr = body + rot(vec2(-9.3, 1.1) * s, heading);
    let ta = time * 40.0;
    let d = vec2(ta.cos(), ta.sin()) * 1.4 * s;
    draw_line(
        tr.x - d.x,
        tr.y - d.y,
        tr.x + d.x,
        tr.y + d.y,
        0.25,
        Color::new(0.1, 0.1, 0.1, 0.8),
    );
}

/// Fills any simple polygon (concave is fine).
fn fill_concave(pts: &[Vec2], col: Color) {
    let flat: Vec<f32> = pts.iter().flat_map(|p| [p.x, p.y]).collect();
    if let Ok(tris) = earcutr::earcut(&flat, &[], 2) {
        for t in tris.chunks(3) {
            draw_triangle(pts[t[0]], pts[t[1]], pts[t[2]], col);
        }
    }
}

/// A simple B-2: a flying wing with a saw-tooth trailing edge, outlined in yellow so it stands out.
pub fn draw_bomber(p: Vec2, heading: f32, tilt: f32, k: f32) {
    let s = 1.25 * k;
    let half = [
        vec2(6.0, 0.0),
        vec2(-1.4, 12.0),
        vec2(-2.7, 11.0),
        vec2(-0.9, 7.6),
        vec2(-3.0, 4.4),
        vec2(-1.3, 0.0),
    ];
    let mut shape: Vec<Vec2> = half.to_vec();
    for q in half[1..5].iter().rev() {
        shape.push(vec2(q.x, -q.y));
    }
    let alt = 26.0;
    let body = p + vec2(0.35 * alt * tilt, -alt * tilt);
    let place = |c: Vec2, grow: f32| -> Vec<Vec2> {
        shape
            .iter()
            .map(|v| {
                c + rot(
                    (*v - vec2(0.8, 0.0)) * s * grow + vec2(0.8, 0.0) * s,
                    heading,
                )
            })
            .collect()
    };
    fill_concave(&place(p, 1.0), Color::new(0.0, 0.0, 0.0, 0.2));
    fill_concave(&place(body, 1.13), rgb(255, 212, 59));
    fill_concave(&place(body, 1.0), rgb(44, 48, 56));
    // cockpit and engine humps
    fan(
        body,
        &ellipse_pts(1.5, 0.75, 3.0, 10),
        heading,
        s,
        rgb(78, 84, 96),
    );
    for side in [-2.4f32, 2.4] {
        fan(
            body,
            &ellipse_pts(1.7, 0.6, 0.2, 8)
                .iter()
                .map(|v| *v + vec2(0.0, side))
                .collect::<Vec<_>>(),
            heading,
            s,
            rgb(62, 68, 78),
        );
    }
}

/// A Google Maps style location pin standing on a point.
pub fn draw_pin(p: Vec2, size: f32, col: Color, inner: Color) {
    let head = p + vec2(0.0, -size * 1.6);
    draw_triangle(
        p,
        head + vec2(-size * 0.78, size * 0.45),
        head + vec2(size * 0.78, size * 0.45),
        col,
    );
    draw_circle(head.x, head.y, size, col);
    draw_circle(head.x, head.y, size * 0.42, inner);
}

fn arc_point(from: Vec2, to: Vec2, t: f32) -> Vec2 {
    let d = to - from;
    let n = vec2(-d.y, d.x).normalize_or_zero();
    let bulge = d.length() * 0.22;
    from + d * t + n * (4.0 * t * (1.0 - t)) * bulge * if n.y > 0.0 { -1.0 } else { 1.0 }
}

// ------------------------------------------------------------------------------------------ world pass

pub struct WorldLabels {
    pub items: Vec<(Vec2, String, Color, f32)>,
}

pub struct ViewInfo {
    pub tl: Vec2,
    pub br: Vec2,
    pub scale: f32,
    pub selected: Option<usize>,
    pub time: f32,
}

fn struct_box(x: f32, y: f32, w: f32, h: f32, ht: f32, tilt: f32, top: Color, side: Color) -> Vec2 {
    let off = vec2(0.35 * ht * tilt, -ht * tilt);
    if tilt > 0.0 {
        let front = mix(side, BLACK, 0.12);
        draw_triangle(
            vec2(x, y + h),
            vec2(x + w, y + h),
            vec2(x + w + off.x, y + h + off.y),
            front,
        );
        draw_triangle(
            vec2(x, y + h),
            vec2(x + w + off.x, y + h + off.y),
            vec2(x + off.x, y + h + off.y),
            front,
        );
        draw_triangle(
            vec2(x + w, y),
            vec2(x + w + off.x, y + off.y),
            vec2(x + w + off.x, y + h + off.y),
            side,
        );
        draw_triangle(
            vec2(x + w, y),
            vec2(x + w + off.x, y + h + off.y),
            vec2(x + w, y + h),
            side,
        );
    }
    draw_rectangle(x + off.x, y + off.y, w, h, top);
    off
}

fn bar(p: Vec2, w: f32, f: f32, col: Color) {
    draw_rectangle(p.x - w / 2.0, p.y, w, 0.9, Color::new(0.0, 0.0, 0.0, 0.55));
    draw_rectangle(p.x - w / 2.0, p.y, w * f.clamp(0.0, 1.0), 0.9, col);
}

pub fn draw_world(sc: &mut Scene, g: &Game, th: &Theme, v: &ViewInfo, labels: &mut WorldLabels) {
    for m in &sc.ground {
        draw_mesh(m);
    }
    for m in &sc.rubble {
        draw_mesh(m);
    }
    draw_texture_ex(
        &sc.zone,
        0.0,
        0.0,
        WHITE,
        DrawTextureParams {
            dest_size: Some(vec2(W as f32, H as f32)),
            ..Default::default()
        },
    );
    let inview = |p: Vec2, m: f32| {
        p.x > v.tl.x - m && p.y > v.tl.y - m && p.x < v.br.x + m && p.y < v.br.y + m
    };
    let tilt = g.tilt;
    let t = v.time;
    // markers (aircraft, pins) never shrink below a readable size on screen
    let mk = (2.2 / v.scale).max(1.0);

    // hordes on the street
    draw_texture_ex(
        &sc.glow,
        0.0,
        0.0,
        WHITE,
        DrawTextureParams {
            dest_size: Some(vec2(W as f32, H as f32)),
            ..Default::default()
        },
    );
    draw_texture_ex(
        &sc.units,
        0.0,
        0.0,
        WHITE,
        DrawTextureParams {
            dest_size: Some(vec2(W as f32, H as f32)),
            ..Default::default()
        },
    );

    // civilians
    let rim = th.civ_rim;
    for c in &g.civs {
        if c.st >= C_DEAD && c.st != C_TO_PAD && c.st != C_AT_PAD {
            continue;
        }
        if !inview(c.p, 2.0) {
            continue;
        }
        let r = match c.st {
            C_ESCORT => rgb(28, 126, 214),
            C_CAPTURED => rgb(230, 60, 30),
            _ => rim,
        };
        draw_rectangle(c.p.x - 0.95, c.p.y - 0.95, 1.9, 1.9, r);
        draw_rectangle(c.p.x - 0.6, c.p.y - 0.6, 1.2, 1.2, th.civ);
    }

    // rescue troops
    for q in &g.squads {
        if q.done {
            continue;
        }
        for m in &q.m {
            if m.alive && !m.home && inview(m.p, 2.0) {
                draw_circle(m.p.x, m.p.y, 1.25, WHITE);
                draw_circle(m.p.x, m.p.y, 0.9, rgb(47, 158, 68));
            }
        }
    }
    // army groups
    for (gi, gr) in g.groups.iter().enumerate() {
        if gr.dead || gr.state == GState::Garrison {
            continue;
        }
        let d = udef(gr.kind);
        let sel = v.selected == Some(gi);
        for m in &gr.m {
            if !m.alive || !inview(m.p, 4.0) {
                continue;
            }
            if sel {
                draw_circle_lines(
                    m.p.x,
                    m.p.y,
                    if gr.kind == U::Tank { 4.0 } else { 2.0 },
                    0.3,
                    rgb(255, 212, 59),
                );
            }
            match gr.kind {
                U::Rifle => {
                    draw_circle(m.p.x, m.p.y, 1.2, WHITE);
                    draw_circle(m.p.x, m.p.y, 0.85, d.col);
                }
                U::Tank => {
                    let a = if gr.target.distance(m.p) > 1.0 {
                        (gr.target - m.p).to_angle()
                    } else {
                        0.0
                    };
                    rect_rot(m.p, -2.4, -1.6, 2.4, 1.6, a, 1.0, rgb(20, 60, 110));
                    rect_rot(m.p, -2.1, -1.3, 2.1, 1.3, a, 1.0, d.col);
                    draw_circle(m.p.x, m.p.y, 1.0, rgb(20, 60, 110));
                    rect_rot(m.p, 0.0, -0.3, 3.4, 0.3, a, 1.0, rgb(20, 60, 110));
                }
            }
        }
        if sel {
            draw_circle_lines(
                gr.target.x,
                gr.target.y,
                3.0,
                0.3,
                alpha(rgb(255, 212, 59), 0.7),
            );
        }
    }

    // the city in 2.5D
    for m in &sc.bmesh {
        draw_mesh(m);
    }
    // x-ray of the horde through buildings so nothing hides behind a tower
    draw_texture_ex(
        &sc.units,
        0.0,
        0.0,
        Color::new(1.0, 1.0, 1.0, 0.28),
        DrawTextureParams {
            dest_size: Some(vec2(W as f32, H as f32)),
            ..Default::default()
        },
    );

    // garrisons
    for (gi, gr) in g.groups.iter().enumerate() {
        if gr.dead || gr.state != GState::Garrison {
            continue;
        }
        let b = gr.b as usize;
        let p = g.world.buildings[b].c + g.roof_off(b);
        let n = gr.m.iter().filter(|m| m.alive).count();
        if v.selected == Some(gi) {
            draw_circle_lines(p.x, p.y, 4.2, 0.4, rgb(255, 212, 59));
        }
        draw_circle(p.x, p.y, 3.0, WHITE);
        draw_circle(p.x, p.y, 2.4, udef(gr.kind).col);
        labels.items.push((p, format!("{}", n), WHITE, 12.0));
    }

    // player structures, back to front
    let mut order: Vec<usize> = (0..g.structs.len())
        .filter(|&i| !g.structs[i].dead)
        .collect();
    order.sort_by_key(|&i| g.structs[i].y + g.structs[i].h);
    for i in order {
        let s = &g.structs[i];
        let d = def(s.t);
        let base = if s.roof >= 0 {
            g.roof_off(s.roof as usize)
        } else {
            Vec2::ZERO
        };
        let (x, y) = (s.x as f32 + base.x, s.y as f32 + base.y);
        let (w, h) = (s.w as f32, s.h as f32);
        if !inview(vec2(x, y), 20.0) {
            continue;
        }
        if s.pending {
            draw_rectangle_lines(x, y, w, h, 0.4, alpha(d.top, 0.8));
            draw_rectangle(x, y, w, h, alpha(d.top, 0.15));
            continue;
        }
        let off = struct_box(x, y, w, h, d.ht, tilt, d.top, d.side);
        let top = vec2(x + off.x, y + off.y);
        let c = top + vec2(w / 2.0, h / 2.0);
        match s.t {
            S::Gun | S::Cannon => {
                let r = if s.t == S::Gun { 1.4 } else { 2.2 };
                draw_circle(c.x, c.y, r, mix(d.side, BLACK, 0.3));
                let dir = vec2(s.aim.cos(), s.aim.sin());
                let len = if s.t == S::Gun { 3.0 } else { 4.4 };
                draw_line(
                    c.x,
                    c.y,
                    c.x + dir.x * len,
                    c.y + dir.y * len,
                    if s.t == S::Gun { 0.6 } else { 1.0 },
                    rgb(40, 40, 40),
                );
                if g.time - s.last_fire < 0.08 {
                    draw_circle(
                        c.x + dir.x * len,
                        c.y + dir.y * len,
                        0.9,
                        rgb(255, 240, 170),
                    );
                }
            }
            S::Extractor => {
                draw_circle(c.x, c.y, 2.2, mix(d.side, BLACK, 0.2));
                let a = t * 2.0;
                for k in 0..3 {
                    let q = rot(vec2(1.9, 0.0), a + k as f32 * 2.094);
                    draw_line(c.x, c.y, c.x + q.x, c.y + q.y, 0.5, d.top);
                }
            }
            S::Plant => {
                draw_rectangle(top.x + 1.0, top.y + 1.0, 2.0, 2.0, mix(d.side, BLACK, 0.3));
                let puff = (t * 0.8).fract();
                draw_circle(
                    top.x + 2.0 + puff * 1.5,
                    top.y + 1.5 - puff * 4.0,
                    0.8 + puff,
                    Color::new(0.8, 0.8, 0.8, 0.5 * (1.0 - puff)),
                );
            }
            S::Silo => {
                let ready = s.cd <= 0.0;
                draw_circle(c.x, c.y, 3.2, rgb(90, 98, 108));
                draw_circle(
                    c.x,
                    c.y,
                    2.6,
                    if ready {
                        rgb(64, 192, 87)
                    } else {
                        rgb(201, 42, 42)
                    },
                );
                if !ready {
                    let f = s.cd / adef(A::Icbm).cd;
                    draw_arc(c.x, c.y, 24, 3.6, -90.0, 0.6, 360.0 * f, rgb(255, 212, 59));
                }
            }
            S::Pad => {
                draw_circle(c.x, c.y, 4.6, rgb(255, 255, 255));
                draw_circle_lines(c.x, c.y, 4.6, 0.5, rgb(255, 212, 59));
                for (a, b) in [
                    (vec2(-1.6, -2.2), vec2(-1.6, 2.2)),
                    (vec2(1.6, -2.2), vec2(1.6, 2.2)),
                    (vec2(-1.6, 0.0), vec2(1.6, 0.0)),
                ] {
                    draw_line(
                        c.x + a.x,
                        c.y + a.y,
                        c.x + b.x,
                        c.y + b.y,
                        0.7,
                        rgb(60, 66, 76),
                    );
                }
                if s.cd > 0.0 {
                    draw_arc(
                        c.x,
                        c.y,
                        24,
                        5.4,
                        -90.0,
                        0.6,
                        360.0 * (s.cd / PAD_COOLDOWN),
                        rgb(28, 126, 214),
                    );
                }
                let n = g
                    .civs
                    .iter()
                    .filter(|cv| cv.st == C_AT_PAD && cv.pad == i)
                    .count();
                if n > 0 {
                    labels
                        .items
                        .push((c + vec2(0.0, -7.0), format!("{} waiting", n), WHITE, 11.0));
                }
            }
            S::Outpost => {
                draw_line(c.x, c.y, c.x, c.y - 5.0, 0.35, rgb(40, 40, 40));
                draw_triangle(
                    vec2(c.x, c.y - 5.0),
                    vec2(c.x + 3.0, c.y - 4.2),
                    vec2(c.x, c.y - 3.4),
                    rgb(28, 126, 214),
                );
            }
            S::Lab => {
                draw_circle(c.x, c.y, 2.8, mix(d.top, WHITE, 0.35));
                if g.research.busy.is_some() {
                    draw_circle(c.x, c.y, 1.4 + (t * 3.0).sin().abs(), rgb(255, 255, 255));
                }
            }
            S::Hq => {
                draw_circle(c.x, c.y, 4.0, mix(d.top, WHITE, 0.25));
                labels
                    .items
                    .push((c + vec2(0.0, 1.2), "HQ".into(), WHITE, 14.0));
            }
            S::Wall => {}
        }
        if s.hp < s.max * 0.99 && s.t != S::Wall {
            bar(
                top + vec2(w / 2.0, -2.0),
                w.max(6.0),
                s.hp / s.max,
                if s.hp / s.max > 0.4 {
                    rgb(64, 192, 87)
                } else {
                    rgb(250, 82, 82)
                },
            );
        } else if s.t == S::Wall && s.hp < s.max * 0.6 {
            draw_rectangle(
                top.x,
                top.y,
                w,
                h,
                Color::new(0.0, 0.0, 0.0, 0.25 * (1.0 - s.hp / s.max)),
            );
        }
    }

    // alien hives
    for h in &g.hives {
        if h.dead || !inview(h.p, h.r + 10.0) {
            continue;
        }
        draw_circle_lines(h.p.x, h.p.y, h.r, 0.5, rgba(190, 100, 240, 0.7));
        let pulse = 1.0 + 0.06 * (t * 2.5).sin();
        let size = (5.0 + h.r * 0.06) * pulse;
        for k in 0..7 {
            let a = k as f32 * 0.9 + 0.3;
            let q = h.p + vec2(a.cos(), a.sin()) * size * 0.55;
            draw_circle(q.x, q.y, size * 0.55, rgb(120, 40, 160));
        }
        draw_circle(h.p.x, h.p.y, size * 0.8, rgb(160, 70, 200));
        draw_circle(h.p.x, h.p.y, size * 0.35, rgb(240, 170, 255));
        bar(
            h.p + vec2(0.0, -size - 3.0),
            14.0,
            h.hp / h.max,
            rgb(204, 93, 232),
        );
        for &b in &h.infected {
            let bc = g.world.buildings[b].c + g.roof_off(b);
            draw_line(h.p.x, h.p.y, bc.x, bc.y, 0.3, rgba(204, 93, 232, 0.35));
        }
    }
    // orc portals
    for p in &g.portals {
        if !inview(p.p, 20.0) {
            continue;
        }
        let r = 7.0 + (t * 3.0).sin();
        draw_circle(p.p.x, p.p.y, r + 1.5, rgba(255, 120, 30, 0.25));
        draw_circle(p.p.x, p.p.y, r, rgb(60, 20, 10));
        for k in 0..3 {
            draw_arc(
                p.p.x,
                p.p.y,
                24,
                r * (0.45 + k as f32 * 0.2),
                (t * 150.0 + k as f32 * 120.0) % 360.0,
                0.7,
                200.0,
                rgb(255, 146, 43),
            );
        }
        labels.items.push((
            p.p + vec2(0.0, -r - 3.0),
            format!("{:.0}s", p.t),
            rgb(255, 180, 120),
            11.0,
        ));
    }

    // effects
    for tr in &g.tracers {
        let (w, c) = match tr.kind {
            0 => (0.35, rgb(255, 236, 150)),
            1 => (0.8, rgb(255, 170, 80)),
            2 => (0.35, rgb(150, 240, 255)),
            4 => (0.7, rgb(200, 220, 255)),
            _ => (0.3, rgb(220, 235, 255)),
        };
        draw_line(tr.a.x, tr.a.y, tr.b.x, tr.b.y, w, c);
    }
    for b in &g.booms {
        let f = 1.0 - b.t / b.life;
        match b.kind {
            1 => {
                draw_circle(
                    b.p.x,
                    b.p.y,
                    b.r * (0.3 + 0.7 * f),
                    Color::new(1.0, 0.95, 0.85, 0.55 * (1.0 - f)),
                );
                draw_circle_lines(
                    b.p.x,
                    b.p.y,
                    b.r * (0.4 + 0.8 * f),
                    2.0,
                    Color::new(1.0, 0.6, 0.2, 0.8 * (1.0 - f)),
                );
            }
            2 => draw_circle_lines(
                b.p.x,
                b.p.y,
                b.r * (0.5 + f),
                0.8,
                Color::new(0.8, 0.45, 1.0, 1.0 - f),
            ),
            _ => {
                draw_circle(
                    b.p.x,
                    b.p.y,
                    b.r * (0.6 + 0.6 * f),
                    Color::new(1.0, 0.65, 0.25, 0.6 * (1.0 - f)),
                );
            }
        }
    }
    // ICBMs: a dashed route like a map directions line, a marker riding it, a red pin on the target
    for s in &g.strikes {
        if let Strike::Icbm {
            from,
            to,
            t: tt,
            dur,
            nuke,
        } = s
        {
            let f = tt / dur;
            let pts: Vec<Vec2> = (0..=40)
                .map(|k| arc_point(*from, *to, k as f32 / 40.0))
                .collect();
            let blue = rgb(66, 133, 244);
            for wv in pts.windows(2).step_by(2) {
                draw_line(wv[0].x, wv[0].y, wv[1].x, wv[1].y, 1.4, alpha(blue, 0.9));
            }
            let r = if *nuke { 190.0 } else { 120.0 };
            draw_circle(
                to.x,
                to.y,
                r,
                rgba(234, 67, 53, 0.10 + 0.05 * (t * 6.0).sin().abs()),
            );
            draw_circle_lines(to.x, to.y, r, 1.2, rgba(234, 67, 53, 0.8));
            draw_pin(
                *to,
                5.0 * mk,
                rgb(234, 67, 53),
                if *nuke {
                    rgb(255, 212, 59)
                } else {
                    rgb(165, 14, 14)
                },
            );
            let p = arc_point(*from, *to, f);
            let q = arc_point(*from, *to, (f + 0.01).min(1.0));
            let a = (q - p).to_angle();
            draw_circle(p.x, p.y, 3.4 * mk, WHITE);
            draw_circle(p.x, p.y, 2.6 * mk, blue);
            draw_triangle(
                p + rot(vec2(1.7, 0.0) * mk, a),
                p + rot(vec2(-1.1, 1.2) * mk, a),
                p + rot(vec2(-1.1, -1.2) * mk, a),
                WHITE,
            );
        }
    }
    for s in &g.strikes {
        if let Strike::Air {
            from,
            dir,
            pos,
            bombs,
            line,
            ..
        } = s
        {
            for b in bombs {
                if !b.2 {
                    draw_circle_lines(b.0.x, b.0.y, 3.0, 0.4, rgba(255, 212, 59, 0.6));
                }
            }
            draw_line(
                line.0.x,
                line.0.y,
                line.1.x,
                line.1.y,
                0.5,
                rgba(255, 212, 59, 0.35),
            );
            let p = *from + *dir * *pos;
            draw_bomber(p, dir.to_angle(), tilt.max(0.4), mk);
        }
    }
    for h in &g.helis {
        let f = (h.t / h.dur).clamp(0.0, 1.0);
        let ground = h.from.lerp(h.to, f);
        let alt = 14.0 * (f * 8.0).min(1.0) * ((1.0 - f) * 8.0).min(1.0) + 1.0;
        let cargo = matches!(h.kind, HeliKind::Deliver(_));
        draw_heli(
            ground,
            alt,
            (h.to - h.from).to_angle(),
            t,
            false,
            cargo,
            tilt.max(0.4),
            mk,
        );
    }
    for s in &g.gunships {
        let heading = if s.phase == 1 {
            s.ang + std::f32::consts::FRAC_PI_2
        } else {
            (s.center - s.p).to_angle()
        };
        if s.phase == 1 {
            draw_circle_lines(s.center.x, s.center.y, 25.0, 0.4, rgba(150, 240, 255, 0.35));
        }
        draw_heli(s.p, 16.0, heading, t, true, false, tilt.max(0.4), mk);
    }
}

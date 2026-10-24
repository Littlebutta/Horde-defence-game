//! Drawing, in 3D. The map lies on the ground plane (x, y in map cells) and heights go up, which
//! in this coordinate system is -z. The ground (roads, water, parks) is baked into flat meshes; city
//! blocks are real 3D prisms baked per map tile, so a destroyed building only rebuilds its own tile.
//! The horde stays 2D: one map-sized texture laid on the ground, refreshed from the density grid every
//! tick, with a soft glow under it that grows with the crowd. Placeable player buildings and army units
//! keep their bold colours; everything else follows a Google Maps look.
//!
//! Draw order: everything on the ground is drawn first with a material that ignores the depth test
//! but writes depth (so ground layers simply paint over each other), then everything with height is
//! drawn with normal depth testing, so buildings hide what is behind them.

use crate::defs::*;
use crate::game::*;
use crate::swarm::{TINT_DOWN, TINT_FLY, TINT_RAGE};
use crate::world::*;
use macroquad::miniquad::{BlendFactor, BlendState, BlendValue, Comparison, Equation, PipelineParams};
use macroquad::models::{Mesh, Vertex};
use macroquad::prelude::*;
use rayon::prelude::*;
use std::sync::atomic::Ordering::Relaxed;

pub struct Fonts {
    pub head: Font,
    pub mono: Font,
}

pub fn load_fonts() -> Fonts {
    let mut head = load_ttf_font_from_bytes(include_bytes!("../assets/ChakraPetch-SemiBold.ttf")).unwrap();
    let mut mono = load_ttf_font_from_bytes(include_bytes!("../assets/IBMPlexMono-Medium.ttf")).unwrap();
    head.set_filter(FilterMode::Linear);
    mono.set_filter(FilterMode::Linear);
    Fonts { head, mono }
}

pub fn text(f: &Font, s: &str, x: f32, y: f32, size: f32, c: Color) {
    draw_text_ex(s, x, y, TextParams { font: Some(f), font_size: size as u16, color: c, ..Default::default() });
}
pub fn text_w(f: &Font, s: &str, size: f32) -> f32 {
    measure_text(s, Some(f), size as u16, 1.0).width
}

/// A point `h` cells above the map point `p`.
#[inline]
pub fn p3(p: Vec2, h: f32) -> Vec3 {
    vec3(p.x, p.y, -h)
}

// ------------------------------------------------------------------------------------------ materials

const VS: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
varying lowp vec2 uv;
varying lowp vec4 color;
uniform mat4 Model;
uniform mat4 Projection;
void main() {
    gl_Position = Projection * Model * vec4(position, 1);
    color = color0 / 255.0;
    uv = texcoord;
}"#;
const FS: &str = r#"#version 100
varying lowp vec4 color;
varying lowp vec2 uv;
uniform sampler2D Texture;
void main() {
    gl_FragColor = color * texture2D(Texture, uv);
}"#;

/// Ground layers: no depth test (later layers paint over earlier ones) but depth is written, so
/// buildings drawn afterwards are correctly hidden behind or shown in front of the street.
pub fn flat_material() -> Material {
    let blend = BlendState::new(Equation::Add, BlendFactor::Value(BlendValue::SourceAlpha), BlendFactor::OneMinusValue(BlendValue::SourceAlpha));
    load_material(
        ShaderSource::Glsl { vertex: VS, fragment: FS },
        MaterialParams {
            pipeline_params: PipelineParams { depth_test: Comparison::Always, depth_write: true, color_blend: Some(blend), alpha_blend: Some(blend), ..Default::default() },
            ..Default::default()
        },
    )
    .unwrap()
}

// ------------------------------------------------------------------------------------------ meshes

/// Accumulates triangles into meshes small enough for one draw call each.
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
            self.out.push(Mesh { vertices: std::mem::take(&mut self.v), indices: std::mem::take(&mut self.i), texture: None });
        }
    }
    fn vert3(&mut self, p: Vec3, c: Color) -> u16 {
        self.v.push(Vertex::new(p.x, p.y, p.z, 0.0, 0.0, c));
        (self.v.len() - 1) as u16
    }
    fn vert(&mut self, p: Vec2, c: Color) -> u16 {
        self.vert3(vec3(p.x, p.y, 0.0), c)
    }
    fn quad3(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3, ca: Color, cb: Color) {
        self.room(4);
        let (ia, ib, ic, id) = (self.vert3(a, ca), self.vert3(b, ca), self.vert3(c, cb), self.vert3(d, cb));
        self.i.extend_from_slice(&[ia, ib, ic, ia, ic, id]);
    }
    fn quad(&mut self, a: Vec2, b: Vec2, c: Vec2, d: Vec2, col: Color) {
        self.room(4);
        let (ia, ib, ic, id) = (self.vert(a, col), self.vert(b, col), self.vert(c, col), self.vert(d, col));
        self.i.extend_from_slice(&[ia, ib, ic, ia, ic, id]);
    }
    /// Any simple polygon at height `h`.
    fn poly_at(&mut self, pts: &[Vec2], h: f32, col: Color) {
        if pts.len() < 3 || pts.len() > 1500 {
            return;
        }
        let flat: Vec<f64> = pts.iter().flat_map(|p| [p.x as f64, p.y as f64]).collect();
        let Ok(tris) = earcutr::earcut(&flat, &[], 2) else { return };
        self.room(pts.len());
        let base = self.v.len() as u16;
        for p in pts {
            self.vert3(p3(*p, h), col);
        }
        for t in tris {
            self.i.push(base + t as u16);
        }
    }
    fn poly(&mut self, pts: &[Vec2], col: Color) {
        if pts.len() > 1500 {
            // huge areas (lakes, forests): fan from the middle so they still show
            let c = centroid(pts);
            for k in 0..pts.len() {
                self.room(3);
                let (a, b, d) = (self.vert(c, col), self.vert(pts[k], col), self.vert(pts[(k + 1) % pts.len()], col));
                self.i.extend_from_slice(&[a, b, d]);
            }
            return;
        }
        self.poly_at(pts, 0.0, col);
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
            self.i.extend_from_slice(&[ic, first + k, first + (k + 1) % n as u16]);
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
    /// A lit box: footprint polygon `pts`, from height `z0` to `z1`.
    fn prism(&mut self, pts: &[Vec2], z0: f32, z1: f32, top: Color, side: Color) {
        let n = pts.len();
        let ccw = poly_area(pts) > 0.0;
        for k in 0..n {
            let a = pts[k];
            let b = pts[(k + 1) % n];
            let e = b - a;
            let mut nrm = vec2(e.y, -e.x).normalize_or_zero();
            if !ccw {
                nrm = -nrm;
            }
            let c = shade(side, nrm);
            self.quad3(p3(a, z0), p3(b, z0), p3(b, z1), p3(a, z1), mix(c, BLACK, 0.18), c);
        }
        self.poly_at(pts, z1, top);
    }
    fn finish(mut self) -> Vec<Mesh> {
        self.flush();
        self.out
    }
}

/// Wall colour for a wall facing `nrm`: lit from the north-west.
fn shade(c: Color, nrm: Vec2) -> Color {
    let l = vec2(-0.55, -0.83);
    let k = 0.62 + 0.38 * nrm.dot(l).max(0.0) - 0.12 * (-nrm.dot(l)).max(0.0);
    Color::new(c.r * k, c.g * k, c.b * k, c.a)
}

pub fn mix(a: Color, b: Color, t: f32) -> Color {
    Color::new(a.r + (b.r - a.r) * t, a.g + (b.g - a.g) * t, a.b + (b.b - a.b) * t, a.a + (b.a - a.a) * t)
}
fn alpha(c: Color, a: f32) -> Color {
    Color::new(c.r, c.g, c.b, a)
}

fn rect_pts(x: f32, y: f32, w: f32, h: f32) -> [Vec2; 4] {
    [vec2(x, y), vec2(x + w, y), vec2(x + w, y + h), vec2(x, y + h)]
}

fn build_ground(w: &World, th: &Theme) -> Vec<Mesh> {
    let d = &w.desc;
    let mut mb = MB::default();
    let (wf, hf) = (W as f32, H as f32);
    mb.quad(vec2(0.0, 0.0), vec2(wf, 0.0), vec2(wf, hf), vec2(0.0, hf), th.land);
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
        mb.line(&r.pts, r.w, if r.rank >= 3 { th.road_major } else { th.road });
    }
    mb.finish()
}

// ------------------------------------------------------------------------------------------ city tiles

const TILE: usize = 128;
const TW: usize = W.div_ceil(TILE);
const TH: usize = H.div_ceil(TILE);

/// What a building looked like when its tile was last baked.
#[derive(Clone, Copy, PartialEq)]
struct BLook {
    alive: bool,
    dmg: u8,
    infected: bool,
}

struct Tile {
    solid: Vec<Mesh>,
    rubble: Vec<Mesh>,
    members: Vec<usize>,
}

fn bake_tile(g: &Game, th: &Theme, members: &[usize], infected: &std::collections::HashSet<usize>) -> (Vec<Mesh>, Vec<Mesh>) {
    let mut mb = MB::default();
    let mut rubble = MB::default();
    let hive_col = rgb(150, 70, 190);
    for &bi in members {
        let b = &g.world.buildings[bi];
        if !b.alive {
            rubble.poly(&b.poly, th.rubble);
            continue;
        }
        let inf = infected.contains(&bi);
        let ccw = b.area > 0.0;
        let n = b.poly.len();
        let mut side = mix(th.side_a, th.side_b, 0.6);
        if inf {
            side = mix(side, hive_col, 0.45);
        }
        for k in 0..n {
            let a = b.poly[k];
            let c = b.poly[(k + 1) % n];
            let e = c - a;
            let mut nrm = vec2(e.y, -e.x).normalize_or_zero();
            if !ccw {
                nrm = -nrm;
            }
            let col = shade(side, nrm);
            // a little darker at street level, like ambient occlusion
            mb.quad3(p3(a, 0.0), p3(c, 0.0), p3(c, b.h), p3(a, b.h), mix(col, BLACK, 0.22), col);
            // window bands on taller blocks, nudged off the wall so they don't flicker
            if b.h > 7.0 && e.length() > 3.0 {
                let floors = (b.h / 3.2) as i32;
                let inset = e.normalize() * 0.7;
                let out = nrm * 0.06;
                for f in 1..floors {
                    let z = f as f32 * 3.2 - 1.6;
                    let (p0, p1) = (a + inset + out, c - inset + out);
                    mb.quad3(p3(p0, z - 0.45), p3(p1, z - 0.45), p3(p1, z + 0.45), p3(p0, z + 0.45), th.window, th.window);
                }
            }
        }
        let mut col = if b.h > 15.0 { th.roof_tall } else { th.roof };
        if inf {
            col = mix(col, hive_col, 0.55);
        }
        let dmg = 1.0 - (b.hp / b.max).clamp(0.0, 1.0);
        if dmg > 0.05 {
            col = mix(col, th.rubble, dmg * 0.6);
        }
        mb.poly_at(&b.poly, b.h, col);
    }
    (mb.finish(), rubble.finish())
}

pub struct Scene {
    ground: Vec<Mesh>,
    tiles: Vec<Tile>,
    looks: Vec<BLook>,
    units: Texture2D,
    ubuf: Vec<u8>,
    glow: Texture2D,
    gbuf: Vec<u8>,
    zone: Texture2D,
    zbuf: Vec<u8>,
    pub flat: Material,
    pub dark: bool,
    btimer: f32,
    ztimer: f32,
    pub need_zone: bool,
    pub units_fresh: bool,
}

const ZS: usize = 4; // zone overlay texel size in cells
const ZW: usize = W / ZS;
const ZH: usize = H / ZS;

fn look(g: &Game, b: usize, infected: &std::collections::HashSet<usize>) -> BLook {
    let bb = &g.world.buildings[b];
    BLook { alive: bb.alive, dmg: ((1.0 - (bb.hp / bb.max).clamp(0.0, 1.0)) * 5.0) as u8, infected: infected.contains(&b) }
}
fn infected_set(g: &Game) -> std::collections::HashSet<usize> {
    g.hives.iter().filter(|h| !h.dead).flat_map(|h| h.infected.iter().copied()).collect()
}

impl Scene {
    pub fn new(g: &Game, th: &Theme) -> Scene {
        let units = Texture2D::from_rgba8(W as u16, H as u16, &vec![0u8; N * 4]);
        units.set_filter(FilterMode::Nearest);
        let glow = Texture2D::from_rgba8(CW as u16, CH as u16, &vec![0u8; CW * CH * 4]);
        glow.set_filter(FilterMode::Linear);
        let zone = Texture2D::from_rgba8(ZW as u16, ZH as u16, &vec![0u8; ZW * ZH * 4]);
        zone.set_filter(FilterMode::Linear);
        let mut members = vec![Vec::new(); TW * TH];
        for (i, b) in g.world.buildings.iter().enumerate() {
            let tx = (b.c.x.max(0.0) as usize / TILE).min(TW - 1);
            let ty = (b.c.y.max(0.0) as usize / TILE).min(TH - 1);
            members[ty * TW + tx].push(i);
        }
        let inf = infected_set(g);
        let tiles = members
            .into_iter()
            .map(|m| {
                let (solid, rubble) = bake_tile(g, th, &m, &inf);
                Tile { solid, rubble, members: m }
            })
            .collect();
        Scene {
            ground: build_ground(&g.world, th),
            tiles,
            looks: (0..g.world.buildings.len()).map(|b| look(g, b, &inf)).collect(),
            units,
            ubuf: vec![0; N * 4],
            glow,
            gbuf: vec![0; CW * CH * 4],
            zone,
            zbuf: vec![0; ZW * ZH * 4],
            flat: flat_material(),
            dark: th.dark,
            btimer: 0.0,
            ztimer: 0.0,
            need_zone: true,
            units_fresh: false,
        }
    }

    pub fn retheme(&mut self, g: &Game, th: &Theme) {
        self.ground = build_ground(&g.world, th);
        let inf = infected_set(g);
        for t in &mut self.tiles {
            let (a, b) = bake_tile(g, th, &t.members, &inf);
            t.solid = a;
            t.rubble = b;
        }
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
        self.ubuf.par_chunks_mut(W * 4).enumerate().for_each(|(y, row)| {
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
            let a = if n <= 0.0 { 0.0 } else { (n.sqrt() / 7.0).min(0.7) };
            self.gbuf[i * 4] = col[0];
            self.gbuf[i * 4 + 1] = col[1];
            self.gbuf[i * 4 + 2] = col[2];
            self.gbuf[i * 4 + 3] = (a * 255.0) as u8;
        }
        self.glow.update_from_bytes(CW as u32, CH as u32, &self.gbuf);
    }

    fn update_zone(&mut self, g: &Game, th: &Theme) {
        let zc = th.zone;
        let ze = th.zone_edge;
        let rad = rgba(190, 230, 40, 0.30);
        let hive = rgba(170, 80, 220, 0.20);
        let w = &g.world;
        let zone_at = |x: usize, y: usize| -> bool { w.zone[(y * ZS + ZS / 2).min(H - 1) * W + (x * ZS + ZS / 2).min(W - 1)] == 1 };
        for y in 0..ZH {
            for x in 0..ZW {
                let o = (y * ZW + x) * 4;
                let p = vec2((x * ZS) as f32 + 2.0, (y * ZS) as f32 + 2.0);
                let mut c = Color::new(0.0, 0.0, 0.0, 0.0);
                if zone_at(x, y) {
                    let edge = (x > 0 && !zone_at(x - 1, y)) || (x + 1 < ZW && !zone_at(x + 1, y)) || (y > 0 && !zone_at(x, y - 1)) || (y + 1 < ZH && !zone_at(x, y + 1));
                    c = if edge { ze } else { zc };
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
        self.zone.update_from_bytes(ZW as u32, ZH as u32, &self.zbuf);
    }

    /// Rebakes only the tiles whose buildings changed (destroyed, damaged, infected).
    pub fn maintain(&mut self, g: &mut Game, th: &Theme, dt: f32) {
        self.btimer -= dt;
        self.ztimer -= dt;
        if self.btimer <= 0.0 {
            self.btimer = 0.35;
            g.dirty_buildings.clear();
            let inf = infected_set(g);
            if self.looks.len() != g.world.buildings.len() {
                self.looks = (0..g.world.buildings.len()).map(|b| look(g, b, &inf)).collect();
            }
            for t in &mut self.tiles {
                let mut dirty = false;
                for &b in &t.members {
                    let l = look(g, b, &inf);
                    if l != self.looks[b] {
                        self.looks[b] = l;
                        dirty = true;
                    }
                }
                if dirty {
                    let (a, b) = bake_tile(g, th, &t.members, &inf);
                    t.solid = a;
                    t.rubble = b;
                }
            }
        }
        if g.zone_dirty || self.need_zone || (!g.hives.is_empty() && self.ztimer <= 0.0) {
            self.ztimer = 2.0;
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
        draw_triangle(cc, c + rot(pts[k] * s, a), c + rot(pts[(k + 1) % n] * s, a), col);
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
#[allow(clippy::too_many_arguments)]
fn rect_rot(c: Vec2, x0: f32, y0: f32, x1: f32, y1: f32, a: f32, s: f32, col: Color) {
    let p = [vec2(x0, y0), vec2(x1, y0), vec2(x1, y1), vec2(x0, y1)].map(|v| c + rot(v * s, a));
    draw_triangle(p[0], p[1], p[2], col);
    draw_triangle(p[0], p[2], p[3], col);
}
fn fill_concave(pts: &[Vec2], col: Color) {
    let flat: Vec<f32> = pts.iter().flat_map(|p| [p.x, p.y]).collect();
    if let Ok(tris) = earcutr::earcut(&flat, &[], 2) {
        for t in tris.chunks(3) {
            draw_triangle(pts[t[0]], pts[t[1]], pts[t[2]], col);
        }
    }
}

/// Runs `f` with 2D drawing lifted `h` cells off the ground.
pub fn at_height(h: f32, f: impl FnOnce()) {
    let mut gl = unsafe { get_internal_gl() };
    gl.flush();
    gl.quad_gl.push_model_matrix(Mat4::from_translation(vec3(0.0, 0.0, -h)));
    f();
    let mut gl = unsafe { get_internal_gl() };
    gl.flush();
    gl.quad_gl.pop_model_matrix();
}

/// A detailed little helicopter: shadow on the ground, the machine itself at altitude.
#[allow(clippy::too_many_arguments)]
pub fn draw_heli(ground: Vec2, alt: f32, heading: f32, time: f32, gunship: bool, cargo: bool, k: f32) {
    let s = 0.55 * k;
    let sh = Color::new(0.0, 0.0, 0.0, 0.22);
    fan(ground, &ellipse_pts(4.2, 2.3, 0.0, 12), heading, s, sh);
    rect_rot(ground, -9.0, -0.6, -2.0, 0.6, heading, s, sh);
    draw_circle(ground.x, ground.y, 6.0 * s, Color::new(0.0, 0.0, 0.0, 0.08));
    let (hull, dark, glass) = if gunship { (rgb(58, 66, 50), rgb(34, 38, 30), rgb(120, 200, 230)) } else { (rgb(86, 104, 68), rgb(52, 62, 42), rgb(150, 215, 240)) };
    if cargo {
        let ch = alt * 0.45;
        draw_line_3d(p3(ground, alt), p3(ground, ch), rgb(40, 40, 40));
        at_height(ch, || {
            rect_rot(ground, -1.6, -1.6, 1.6, 1.6, heading, s, rgb(160, 120, 70));
            rect_rot(ground, -1.6, -0.2, 1.6, 0.2, heading, s, rgb(110, 80, 45));
        });
    }
    at_height(alt, || {
        let body = ground;
        for side in [-2.3f32, 2.3] {
            rect_rot(body, -3.2, side - 0.2, 3.0, side + 0.2, heading, s, dark);
        }
        rect_rot(body, -9.5, -0.55, -2.5, 0.55, heading, s, hull);
        rect_rot(body, -9.8, -2.0, -8.8, 2.0, heading, s, dark);
        if gunship {
            rect_rot(body, -1.2, -4.6, 0.6, 4.6, heading, s, dark);
            for side in [-4.0f32, 4.0] {
                rect_rot(body, -1.6, side - 0.5, 1.4, side + 0.5, heading, s, rgb(30, 30, 30));
            }
        }
        fan(body, &ellipse_pts(4.3, 2.2, 0.0, 16), heading, s, dark);
        fan(body, &ellipse_pts(4.0, 1.9, 0.0, 16), heading, s, hull);
        fan(body, &ellipse_pts(1.7, 1.4, 2.3, 12), heading, s, glass);
        fan(body, &ellipse_pts(0.6, 0.5, 2.7, 8), heading, s, Color::new(1.0, 1.0, 1.0, 0.6));
        rect_rot(body, -1.8, -0.9, 0.6, 0.9, heading, s, dark);
    });
    at_height(alt + 1.2 * k, || {
        let rc = ground + rot(vec2(-0.4, 0.0) * s, heading);
        draw_circle(rc.x, rc.y, 6.2 * s, Color::new(0.1, 0.1, 0.1, 0.12));
        let ra = time * 28.0;
        for b in 0..2 {
            rect_rot(rc, -6.2, -0.28, 6.2, 0.28, ra + b as f32 * std::f32::consts::PI, s, Color::new(0.12, 0.12, 0.12, 0.75));
        }
        draw_circle(rc.x, rc.y, 0.6 * s, rgb(25, 25, 25));
        let tr = ground + rot(vec2(-9.3, 1.1) * s, heading);
        let ta = time * 40.0;
        let d = vec2(ta.cos(), ta.sin()) * 1.4 * s;
        draw_line(tr.x - d.x, tr.y - d.y, tr.x + d.x, tr.y + d.y, 0.25 * k, Color::new(0.1, 0.1, 0.1, 0.8));
    });
}

/// A simple B-2: a flying wing with a saw-tooth trailing edge, outlined in yellow so it stands out.
pub fn draw_bomber(p: Vec2, heading: f32, k: f32) {
    let s = 1.25 * k;
    let half = [vec2(6.0, 0.0), vec2(-1.4, 12.0), vec2(-2.7, 11.0), vec2(-0.9, 7.6), vec2(-3.0, 4.4), vec2(-1.3, 0.0)];
    let mut shape: Vec<Vec2> = half.to_vec();
    for q in half[1..5].iter().rev() {
        shape.push(vec2(q.x, -q.y));
    }
    let place = |c: Vec2, grow: f32| -> Vec<Vec2> { shape.iter().map(|v| c + rot((*v - vec2(0.8, 0.0)) * s * grow + vec2(0.8, 0.0) * s, heading)).collect() };
    fill_concave(&place(p, 1.0), Color::new(0.0, 0.0, 0.0, 0.2));
    at_height(60.0, || {
        fill_concave(&place(p, 1.13), rgb(255, 212, 59));
        fill_concave(&place(p, 1.0), rgb(44, 48, 56));
        fan(p, &ellipse_pts(1.5, 0.75, 3.0, 10), heading, s, rgb(78, 84, 96));
        for side in [-2.4f32, 2.4] {
            fan(p, &ellipse_pts(1.7, 0.6, 0.2, 8).iter().map(|v| *v + vec2(0.0, side)).collect::<Vec<_>>(), heading, s, rgb(62, 68, 78));
        }
    });
}

/// A Google Maps style location pin, standing up on its point.
pub fn draw_pin(p: Vec2, size: f32, col: Color, inner: Color) {
    // the pin is a flat badge facing up, raised on a short stem so it reads from any angle
    draw_line_3d(p3(p, 0.0), p3(p, size * 1.6), col);
    at_height(size * 1.6, || {
        draw_circle(p.x, p.y, size, col);
        draw_circle(p.x, p.y, size * 0.42, inner);
    });
    draw_circle(p.x, p.y, size * 0.35, alpha(col, 0.5));
}

fn arc(from: Vec2, to: Vec2, t: f32) -> (Vec2, f32) {
    let d = to - from;
    (from + d * t, 4.0 * t * (1.0 - t) * (d.length() * 0.35).max(60.0))
}

// ------------------------------------------------------------------------------------------ world pass

pub struct WorldLabels {
    pub items: Vec<(Vec3, String, Color, f32)>,
}

pub struct ViewInfo {
    pub center: Vec2,
    pub radius: f32,
    pub scale: f32,
    pub selected: Option<usize>,
    pub time: f32,
}

fn bar(mb: &mut MB, p: Vec2, h: f32, w: f32, f: f32, col: Color) {
    let (x, y) = (p.x - w / 2.0, p.y);
    mb.quad3(p3(vec2(x, y), h), p3(vec2(x + w, y), h), p3(vec2(x + w, y + 0.9), h), p3(vec2(x, y + 0.9), h), Color::new(0.0, 0.0, 0.0, 0.55), Color::new(0.0, 0.0, 0.0, 0.55));
    let fw = w * f.clamp(0.0, 1.0);
    let h2 = h + 0.05;
    mb.quad3(p3(vec2(x, y), h2), p3(vec2(x + fw, y), h2), p3(vec2(x + fw, y + 0.9), h2), p3(vec2(x, y + 0.9), h2), col, col);
}

fn ring(mb: &mut MB, c: Vec2, r: f32, w: f32, col: Color) {
    let n = ((r * 1.5) as usize).clamp(16, 96);
    let pts: Vec<Vec2> = (0..=n).map(|k| c + vec2((k as f32 / n as f32 * std::f32::consts::TAU).cos(), (k as f32 / n as f32 * std::f32::consts::TAU).sin()) * r).collect();
    for s in pts.windows(2) {
        let d = (s[1] - s[0]).normalize_or_zero();
        let nn = vec2(-d.y, d.x) * w / 2.0;
        mb.quad(s[0] + nn, s[1] + nn, s[1] - nn, s[0] - nn, col);
    }
}

/// Everything that lies flat on the ground and changes every frame.
fn ground_dynamic(g: &Game, th: &Theme, v: &ViewInfo) -> MB {
    let mut mb = MB::default();
    let inview = |p: Vec2, m: f32| p.distance(v.center) < v.radius + m;
    let t = v.time;
    // civilians
    for c in &g.civs {
        if (c.st >= C_DEAD && c.st != C_TO_PAD && c.st != C_AT_PAD) || !inview(c.p, 2.0) {
            continue;
        }
        let r = match c.st {
            C_ESCORT => rgb(28, 126, 214),
            C_CAPTURED => rgb(230, 60, 30),
            _ => th.civ_rim,
        };
        mb.quad(c.p + vec2(-0.95, -0.95), c.p + vec2(0.95, -0.95), c.p + vec2(0.95, 0.95), c.p + vec2(-0.95, 0.95), r);
        mb.quad(c.p + vec2(-0.6, -0.6), c.p + vec2(0.6, -0.6), c.p + vec2(0.6, 0.6), c.p + vec2(-0.6, 0.6), th.civ);
    }
    // rescue troops
    for q in &g.squads {
        if q.done {
            continue;
        }
        for m in &q.m {
            if m.alive && !m.home && inview(m.p, 2.0) {
                mb.disc(m.p, 1.25, WHITE);
                mb.disc(m.p, 0.9, rgb(47, 158, 68));
            }
        }
    }
    // army on the street
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
                ring(&mut mb, m.p, if gr.kind == U::Tank { 4.0 } else { 2.0 }, 0.35, rgb(255, 212, 59));
            }
            match gr.kind {
                U::Rifle => {
                    mb.disc(m.p, 1.2, WHITE);
                    mb.disc(m.p, 0.85, d.col);
                }
                U::Tank => {
                    let a = if gr.target.distance(m.p) > 1.0 { (gr.target - m.p).to_angle() } else { 0.0 };
                    let box_ = |x0: f32, y0: f32, x1: f32, y1: f32| [vec2(x0, y0), vec2(x1, y0), vec2(x1, y1), vec2(x0, y1)].map(|q| m.p + rot(q, a));
                    let b = box_(-2.4, -1.6, 2.4, 1.6);
                    mb.quad(b[0], b[1], b[2], b[3], rgb(20, 60, 110));
                    let b = box_(-2.1, -1.3, 2.1, 1.3);
                    mb.quad(b[0], b[1], b[2], b[3], d.col);
                    mb.disc(m.p, 1.0, rgb(20, 60, 110));
                    let b = box_(0.0, -0.3, 3.4, 0.3);
                    mb.quad(b[0], b[1], b[2], b[3], rgb(20, 60, 110));
                }
            }
        }
        if sel {
            ring(&mut mb, gr.target, 3.0, 0.35, alpha(rgb(255, 212, 59), 0.7));
        }
    }
    // hive grounds and portals
    for h in &g.hives {
        if h.dead || !inview(h.p, h.r + 10.0) {
            continue;
        }
        ring(&mut mb, h.p, h.r, 0.6, rgba(190, 100, 240, 0.7));
        let size = 5.0 + h.r * 0.06;
        for k in 0..7 {
            let a = k as f32 * 0.9 + 0.3;
            mb.disc(h.p + vec2(a.cos(), a.sin()) * size * 0.9, size * 0.7, rgba(120, 40, 160, 0.8));
        }
        for &b in &h.infected {
            let bc = g.world.buildings[b].c;
            let d = (bc - h.p).normalize_or_zero();
            let n = vec2(-d.y, d.x) * 0.4;
            mb.quad(h.p + n, bc + n, bc - n, h.p - n, rgba(204, 93, 232, 0.4));
        }
    }
    for p in &g.portals {
        if !inview(p.p, 20.0) {
            continue;
        }
        let r = 7.0 + (t * 3.0).sin();
        mb.disc(p.p, r + 2.0, rgba(255, 120, 30, 0.25));
        mb.disc(p.p, r, rgb(60, 20, 10));
        for k in 0..3 {
            ring(&mut mb, p.p, r * (0.4 + k as f32 * 0.2), 0.6, alpha(rgb(255, 146, 43), 0.5 + 0.5 * ((t * 4.0 + k as f32).sin()).abs()));
        }
    }
    // explosions on the ground
    for b in &g.booms {
        let f = 1.0 - b.t / b.life;
        match b.kind {
            1 => mb.disc(b.p, b.r * (0.3 + 0.7 * f), Color::new(1.0, 0.95, 0.85, 0.45 * (1.0 - f))),
            2 => ring(&mut mb, b.p, b.r * (0.5 + f), 0.8, Color::new(0.8, 0.45, 1.0, 1.0 - f)),
            _ => mb.disc(b.p, b.r * (0.6 + 0.6 * f), Color::new(1.0, 0.65, 0.25, 0.6 * (1.0 - f))),
        }
    }
    // ICBM: a dashed route like a map directions line and the blast radius on the target
    for s in &g.strikes {
        match s {
            Strike::Icbm { from, to, nuke, .. } => {
                let r = if *nuke { 190.0 } else { 120.0 };
                mb.disc(*to, r, rgba(234, 67, 53, 0.10 + 0.05 * (t * 6.0).sin().abs()));
                ring(&mut mb, *to, r, 1.2, rgba(234, 67, 53, 0.8));
                mb.dashed(&[*from, *to], 1.4, 6.0, 5.0, alpha(rgb(66, 133, 244), 0.9));
            }
            Strike::Air { bombs, line, .. } => {
                for b in bombs {
                    if !b.2 {
                        ring(&mut mb, b.0, 3.0, 0.4, rgba(255, 212, 59, 0.6));
                    }
                }
                mb.line(&[line.0, line.1], 0.5, rgba(255, 212, 59, 0.35));
            }
        }
    }
    for s in &g.gunships {
        if s.phase == 1 {
            ring(&mut mb, s.center, 25.0, 0.4, rgba(150, 240, 255, 0.35));
        }
    }
    mb
}

/// Player structures and other things with height, rebuilt every frame (there are only hundreds).
fn solid_dynamic(g: &Game, v: &ViewInfo, labels: &mut WorldLabels) -> MB {
    let mut mb = MB::default();
    let inview = |p: Vec2, m: f32| p.distance(v.center) < v.radius + m;
    let t = v.time;
    for (i, s) in g.structs.iter().enumerate() {
        if s.dead {
            continue;
        }
        let d = def(s.t);
        let base = if s.roof >= 0 { g.world.buildings[s.roof as usize].h } else { 0.0 };
        let (x, y, w, h) = (s.x as f32, s.y as f32, s.w as f32, s.h as f32);
        let c = vec2(x + w / 2.0, y + h / 2.0);
        if !inview(c, 20.0) {
            continue;
        }
        if s.pending {
            // flown in by helicopter: a translucent ghost until it lands
            mb.prism(&rect_pts(x, y, w, h), base, base + 0.3, alpha(d.top, 0.3), alpha(d.top, 0.3));
            continue;
        }
        let top = base + d.ht;
        mb.prism(&rect_pts(x, y, w, h), base, top, d.top, d.side);
        let dark = mix(d.side, BLACK, 0.3);
        match s.t {
            S::Gun | S::Cannon => {
                let r = if s.t == S::Gun { 1.4 } else { 2.2 };
                let ring_pts: Vec<Vec2> = (0..10).map(|k| c + vec2((k as f32 * 0.628).cos(), (k as f32 * 0.628).sin()) * r).collect();
                mb.prism(&ring_pts, top, top + r * 0.8, dark, dark);
                let dir = vec2(s.aim.cos(), s.aim.sin());
                let n = vec2(-dir.y, dir.x) * if s.t == S::Gun { 0.3 } else { 0.5 };
                let len = if s.t == S::Gun { 3.4 } else { 4.8 };
                let bz = top + r * 0.5;
                mb.prism(&[c - n, c + dir * len - n, c + dir * len + n, c + n], bz, bz + 0.6, rgb(40, 40, 40), rgb(40, 40, 40));
            }
            S::Extractor => {
                let a = t * 2.0;
                for k in 0..3 {
                    let q = rot(vec2(1.0, 0.0), a + k as f32 * 2.094);
                    let n = vec2(-q.y, q.x) * 0.35;
                    mb.prism(&[c - n, c + q * 2.2 - n, c + q * 2.2 + n, c + n], top, top + 0.5, d.top, dark);
                }
            }
            S::Plant => {
                mb.prism(&rect_pts(x + 1.0, y + 1.0, 1.6, 1.6), top, top + 3.5, dark, dark);
            }
            S::Silo => {
                let ready = s.cd <= 0.0;
                let pts: Vec<Vec2> = (0..14).map(|k| c + vec2((k as f32 * 0.449).cos(), (k as f32 * 0.449).sin()) * 3.0).collect();
                mb.prism(&pts, top, top + 0.4, if ready { rgb(64, 192, 87) } else { rgb(201, 42, 42) }, rgb(90, 98, 108));
            }
            S::Pad => {
                let pts: Vec<Vec2> = (0..20).map(|k| c + vec2((k as f32 * 0.314).cos(), (k as f32 * 0.314).sin()) * 4.6).collect();
                mb.prism(&pts, top, top + 0.1, WHITE, rgb(255, 212, 59));
                for (a, b) in [(vec2(-1.6, -2.2), vec2(-1.6, 2.2)), (vec2(1.6, -2.2), vec2(1.6, 2.2)), (vec2(-1.6, -0.35), vec2(1.6, 0.35))] {
                    let (lo, hi) = (c + a.min(b) - vec2(0.35, 0.0), c + a.max(b) + vec2(0.35, 0.0));
                    mb.prism(&rect_pts(lo.x, lo.y, hi.x - lo.x, hi.y - lo.y), top + 0.1, top + 0.15, rgb(60, 66, 76), rgb(60, 66, 76));
                }
                if s.cd > 0.0 {
                    bar(&mut mb, c + vec2(0.0, -6.0), top + 0.5, 9.0, 1.0 - s.cd / PAD_COOLDOWN, rgb(28, 126, 214));
                }
                let n = g.civs.iter().filter(|cv| cv.st == C_AT_PAD && cv.pad == i).count();
                if n > 0 {
                    labels.items.push((p3(c, top + 6.0), format!("{} waiting", n), WHITE, 12.0));
                }
            }
            S::Outpost => {
                mb.prism(&rect_pts(c.x - 0.2, c.y - 0.2, 0.4, 0.4), top, top + 6.0, rgb(40, 40, 40), rgb(40, 40, 40));
                mb.prism(&rect_pts(c.x, c.y - 0.1, 3.0, 0.2), top + 4.6, top + 6.0, rgb(28, 126, 214), rgb(28, 126, 214));
            }
            S::Lab => {
                let pts: Vec<Vec2> = (0..12).map(|k| c + vec2((k as f32 * 0.524).cos(), (k as f32 * 0.524).sin()) * 2.8).collect();
                let busy = g.research.busy.is_some();
                mb.prism(&pts, top, top + 1.6 + if busy { (t * 3.0).sin().abs() } else { 0.0 }, mix(d.top, WHITE, 0.35), d.top);
            }
            S::Hq => {
                let pts: Vec<Vec2> = (0..12).map(|k| c + vec2((k as f32 * 0.524).cos(), (k as f32 * 0.524).sin()) * 3.6).collect();
                mb.prism(&pts, top, top + 3.0, mix(d.top, WHITE, 0.25), d.side);
                labels.items.push((p3(c, top + 5.0), "HQ".into(), WHITE, 14.0));
            }
            S::Wall => {}
        }
        if s.hp < s.max * 0.99 && s.t != S::Wall {
            let f = s.hp / s.max;
            bar(&mut mb, c + vec2(0.0, -h / 2.0 - 2.0), top + 1.0, w.max(6.0), f, if f > 0.4 { rgb(64, 192, 87) } else { rgb(250, 82, 82) });
        }
    }
    // troops on rooftops
    for (gi, gr) in g.groups.iter().enumerate() {
        if gr.dead || gr.state != GState::Garrison {
            continue;
        }
        let b = &g.world.buildings[gr.b as usize];
        let n = gr.m.iter().filter(|m| m.alive).count();
        let col = udef(gr.kind).col;
        let pts: Vec<Vec2> = (0..12).map(|k| b.c + vec2((k as f32 * 0.524).cos(), (k as f32 * 0.524).sin()) * 3.0).collect();
        let sel = v.selected == Some(gi);
        mb.prism(&pts, b.h, b.h + 0.4, if sel { rgb(255, 212, 59) } else { WHITE }, col);
        let pts: Vec<Vec2> = (0..12).map(|k| b.c + vec2((k as f32 * 0.524).cos(), (k as f32 * 0.524).sin()) * 2.3).collect();
        mb.prism(&pts, b.h + 0.4, b.h + 0.5, col, col);
        labels.items.push((p3(b.c, b.h + 3.0), format!("{}", n), WHITE, 12.0));
    }
    // hives: a living dome
    for h in &g.hives {
        if h.dead || !inview(h.p, 30.0) {
            continue;
        }
        let pulse = 1.0 + 0.06 * (t * 2.5).sin();
        let size = (5.0 + h.r * 0.06) * pulse;
        for (k, (r, z0, z1)) in [(size, 0.0, size * 0.5), (size * 0.75, size * 0.5, size * 0.9), (size * 0.45, size * 0.9, size * 1.15)].iter().enumerate() {
            let pts: Vec<Vec2> = (0..14).map(|j| h.p + vec2((j as f32 * 0.449).cos(), (j as f32 * 0.449).sin()) * *r).collect();
            let col = [rgb(120, 40, 160), rgb(160, 70, 200), rgb(240, 170, 255)][k];
            mb.prism(&pts, *z0, *z1, col, col);
        }
        bar(&mut mb, h.p + vec2(0.0, -size - 3.0), size * 1.4, 14.0, h.hp / h.max, rgb(204, 93, 232));
    }
    mb
}

pub fn draw_world(sc: &mut Scene, g: &Game, th: &Theme, v: &ViewInfo, labels: &mut WorldLabels) {
    let t = v.time;
    // markers (aircraft, pins) never shrink below a readable size on screen
    let mk = (2.2 / v.scale).max(1.0);
    let full = DrawTextureParams { dest_size: Some(vec2(W as f32, H as f32)), ..Default::default() };

    // ---- the ground: painted in order, writing depth
    gl_use_material(&sc.flat);
    for m in &sc.ground {
        draw_mesh(m);
    }
    for tl in &sc.tiles {
        for m in &tl.rubble {
            draw_mesh(m);
        }
    }
    draw_texture_ex(&sc.zone, 0.0, 0.0, WHITE, full.clone());
    draw_texture_ex(&sc.glow, 0.0, 0.0, WHITE, full.clone());
    draw_texture_ex(&sc.units, 0.0, 0.0, WHITE, full.clone());
    for m in ground_dynamic(g, th, v).finish() {
        draw_mesh(&m);
    }
    // shadows of aircraft are drawn with the aircraft below

    // ---- everything with height, depth tested
    gl_use_default_material();
    let near = |c: Vec2| c.distance(v.center) < v.radius + TILE as f32;
    for (k, tl) in sc.tiles.iter().enumerate() {
        let c = vec2(((k % TW) * TILE + TILE / 2) as f32, ((k / TW) * TILE + TILE / 2) as f32);
        if !near(c) {
            continue;
        }
        for m in &tl.solid {
            draw_mesh(m);
        }
    }
    for m in solid_dynamic(g, v, labels).finish() {
        draw_mesh(&m);
    }
    // portal labels
    for p in &g.portals {
        labels.items.push((p3(p.p, 10.0), format!("{:.0}s", p.t), rgb(255, 180, 120), 11.0));
    }
    // shots: lines from the shooter's height down to the target
    for tr in &g.tracers {
        let c = match tr.kind {
            0 => rgb(255, 236, 150),
            1 => rgb(255, 170, 80),
            2 => rgb(150, 240, 255),
            4 => rgb(200, 220, 255),
            _ => rgb(220, 235, 255),
        };
        draw_line_3d(p3(tr.a, tr.az), p3(tr.b, 0.3), c);
    }
    for b in &g.booms {
        if b.kind == 1 {
            let f = 1.0 - b.t / b.life;
            draw_sphere(p3(b.p, 0.0), b.r * (0.2 + 0.5 * f), None, Color::new(1.0, 0.7, 0.3, 0.35 * (1.0 - f)));
        }
    }
    // ICBMs riding their arc, pins on the targets
    for s in &g.strikes {
        if let Strike::Icbm { from, to, t: tt, dur, nuke } = s {
            draw_pin(*to, 5.0 * mk, rgb(234, 67, 53), if *nuke { rgb(255, 212, 59) } else { rgb(165, 14, 14) });
            let f = tt / dur;
            let blue = rgb(66, 133, 244);
            let mut prev = None;
            for k in 0..=24 {
                let ff = f * k as f32 / 24.0;
                let (p, h) = arc(*from, *to, ff);
                if let Some(q) = prev {
                    draw_line_3d(q, p3(p, h), alpha(blue, 0.7));
                }
                prev = Some(p3(p, h));
            }
            let (p, h) = arc(*from, *to, f);
            draw_circle(p.x, p.y, 2.2 * mk, Color::new(0.0, 0.0, 0.0, 0.2));
            at_height(h, || {
                draw_circle(p.x, p.y, 3.4 * mk, WHITE);
                draw_circle(p.x, p.y, 2.6 * mk, blue);
            });
        }
    }
    for s in &g.strikes {
        if let Strike::Air { from, dir, pos, .. } = s {
            draw_bomber(*from + *dir * *pos, dir.to_angle(), mk);
        }
    }
    for h in &g.helis {
        let f = (h.t / h.dur).clamp(0.0, 1.0);
        let ground = h.from.lerp(h.to, f);
        let alt = 30.0 * (f * 8.0).min(1.0) * ((1.0 - f) * 8.0).min(1.0) + 1.0;
        draw_heli(ground, alt, (h.to - h.from).to_angle(), t, false, matches!(h.kind, HeliKind::Deliver(_)), mk);
    }
    for s in &g.gunships {
        let heading = if s.phase == 1 { s.ang + std::f32::consts::FRAC_PI_2 } else { (s.center - s.p).to_angle() };
        draw_heli(s.p, 16.0 + 20.0, heading, t, true, false, mk);
    }
}

/// Ghost of a structure about to be placed (drawn by the app at ground or roof height).
pub fn ghost(x: f32, y: f32, w: f32, h: f32, z: f32, ok: bool) {
    let c = if ok { rgba(64, 192, 87, 0.55) } else { rgba(250, 82, 82, 0.55) };
    let mut mb = MB::default();
    mb.prism(&rect_pts(x, y, w, h), z, z + 1.5, c, c);
    for m in mb.finish() {
        draw_mesh(&m);
    }
}

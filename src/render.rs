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

mod air;
mod art;

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

/// Text at baseline `x`, `y`. Inside `ui::begin`/`ui::end` the glyphs are rasterised at the real
/// pixel size, so they stay sharp at any UI scale.
pub fn text(f: &Font, s: &str, x: f32, y: f32, size: f32, c: Color) {
    let k = crate::ui::draw_scale();
    let px = (size * k).round().max(1.0);
    draw_text_ex(s, x, y, TextParams { font: Some(f), font_size: px as u16, font_scale: size / px, color: c, ..Default::default() });
}
/// The width of `s` in the units it is drawn in (points inside `ui::begin`/`ui::end`).
pub fn text_w(f: &Font, s: &str, size: f32) -> f32 {
    let k = crate::ui::draw_scale();
    let px = (size * k).round().max(1.0);
    measure_text(s, Some(f), px as u16, 1.0).width * size / px
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

/// Solids: like `FS`, but surfaces tagged with u = 1 (fungus on infected blocks) throb with
/// `Time`, each at its own phase v.
const FS_PULSE: &str = r#"#version 100
precision mediump float;
varying lowp vec4 color;
varying mediump vec2 uv;
uniform sampler2D Texture;
uniform float Time;
void main() {
    vec4 c = color * texture2D(Texture, vec2(0.5, 0.5));
    if (uv.x > 0.5) {
        float p = 0.5 + 0.5 * sin(Time * 2.4 + uv.y);
        c.rgb = c.rgb * (0.55 + 0.65 * p) + vec3(0.16, 0.02, 0.24) * p;
    }
    gl_FragColor = c;
}"#;
const VS_PULSE: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
varying mediump vec2 uv;
varying lowp vec4 color;
uniform mat4 Model;
uniform mat4 Projection;
void main() {
    gl_Position = Projection * Model * vec4(position, 1);
    color = color0 / 255.0;
    uv = texcoord;
}"#;

/// Solid things with a working depth buffer: tested and written like the default material, plus
/// the fungus pulse.
pub fn depth_material() -> Material {
    solid_material(PipelineParams { depth_test: Comparison::LessOrEqual, depth_write: true, ..Default::default() })
}

fn solid_material(mut pp: PipelineParams) -> Material {
    let blend = BlendState::new(Equation::Add, BlendFactor::Value(BlendValue::SourceAlpha), BlendFactor::OneMinusValue(BlendValue::SourceAlpha));
    pp.color_blend = Some(blend);
    pp.alpha_blend = Some(blend);
    load_material(
        ShaderSource::Glsl { vertex: VS_PULSE, fragment: FS_PULSE },
        MaterialParams { pipeline_params: pp, uniforms: vec![UniformDesc::new("Time", UniformType::Float1)], ..Default::default() },
    )
    .unwrap()
}

/// Glowing line art over the scene: tested against depth (unless it doesn't work), never written.
pub fn glow_material(painter: bool) -> Material {
    let blend = BlendState::new(Equation::Add, BlendFactor::Value(BlendValue::SourceAlpha), BlendFactor::OneMinusValue(BlendValue::SourceAlpha));
    load_material(
        ShaderSource::Glsl { vertex: VS, fragment: FS },
        MaterialParams {
            pipeline_params: PipelineParams {
                depth_test: if painter { Comparison::Always } else { Comparison::LessOrEqual },
                depth_write: false,
                color_blend: Some(blend),
                alpha_blend: Some(blend),
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .unwrap()
}

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

/// For GPUs whose depth buffer can't be trusted (see `check_depth`): no depth test at all, so the
/// painter's order decides what covers what. `cull` hides the inside faces of closed shapes, which
/// is what makes one building look solid without a depth buffer.
pub fn painter_material(cull: bool) -> Material {
    solid_material(PipelineParams {
        depth_test: Comparison::Always,
        depth_write: false,
        cull_face: if cull { CULL } else { miniquad::CullFace::Nothing },
        ..Default::default()
    })
}
/// Which side the painter material drops: every solid face is wound to face outward (see
/// `MB::wall`); GL calls the inward ones back faces with this camera.
const CULL: miniquad::CullFace = miniquad::CullFace::Back;

// ------------------------------------------------------------------------------------------ meshes

/// Accumulates triangles into meshes small enough for one draw call each. With `split` on, every
/// object (`obj`) gets meshes of its own, tagged with where it stands, so they can be drawn far to
/// near.
#[derive(Default)]
struct MB {
    out: Vec<Mesh>,
    v: Vec<Vertex>,
    i: Vec<u16>,
    split: bool,
    anchor: Vec3,
    anchors: Vec<Vec3>,
    /// texture coordinates for the next vertices; u = 1 marks a pulsing surface, v is its phase
    uv: Vec2,
}
impl MB {
    fn split(on: bool) -> MB {
        MB { split: on, ..Default::default() }
    }
    /// Starts a new object at map point `at`; `lift` sorts it nearer than things at the same spot
    /// (a turret after the roof it stands on).
    fn obj(&mut self, at: Vec2, lift: f32) {
        if self.split {
            self.flush();
        }
        self.anchor = vec3(at.x, at.y, lift);
    }
    /// A wall from `a` to `b`, `z0` to `z1`, wound to face `nrm` (outward).
    fn wall(&mut self, a: Vec2, b: Vec2, z0: f32, z1: f32, nrm: Vec2, ca: Color, cb: Color) {
        // the triangles face the left of a->b; turn them around when outward is to the right
        let (a, b) = if vec2(-(b - a).y, (b - a).x).dot(nrm) < 0.0 { (b, a) } else { (a, b) };
        self.quad3(p3(a, z0), p3(b, z0), p3(b, z1), p3(a, z1), ca, cb);
    }
    fn room(&mut self, n: usize) {
        if self.v.len() + n > 9000 || self.i.len() + n * 3 > 4800 {
            self.flush();
        }
    }
    fn flush(&mut self) {
        clip_to_map(&mut self.v, &mut self.i);
        if !self.v.is_empty() {
            self.out.push(Mesh { vertices: std::mem::take(&mut self.v), indices: std::mem::take(&mut self.i), texture: None });
            self.anchors.push(self.anchor);
        }
    }
    fn vert3(&mut self, p: Vec3, c: Color) -> u16 {
        self.v.push(Vertex::new(p.x, p.y, p.z, self.uv.x, self.uv.y, c));
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
        if pts.len() < 3 || pts.len() > 60_000 {
            return;
        }
        let flat: Vec<f64> = pts.iter().flat_map(|p| [p.x as f64, p.y as f64]).collect();
        let Ok(tris) = earcutr::earcut(&flat, &[], 2) else { return };
        self.room(pts.len());
        let base = self.v.len() as u16;
        for p in pts {
            self.vert3(p3(*p, h), col);
        }
        // wound to face up (-z), like the walls face out
        for t in tris.chunks_exact(3) {
            let (a, b, c) = (pts[t[0]], pts[t[1]], pts[t[2]]);
            let up = (b - a).perp_dot(c - a) < 0.0;
            let (j, k) = if up { (t[1], t[2]) } else { (t[2], t[1]) };
            self.i.extend_from_slice(&[base + t[0] as u16, base + j as u16, base + k as u16]);
        }
    }
    fn poly(&mut self, pts: &[Vec2], col: Color) {
        if pts.len() > 1500 {
            // huge areas (a river's whole course through the region): only the part on the map.
            // These used to be drawn as a fan from their middle, which flooded half of Cairo.
            self.poly_at(&clip_ring(pts), 0.0, col);
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
            self.wall(a, b, z0, z1, nrm, mix(c, BLACK, 0.18), c);
        }
        self.poly_at(pts, z1, top);
    }
    fn finish(mut self) -> Vec<Mesh> {
        self.flush();
        self.out
    }
    fn finish_objs(mut self) -> (Vec<Mesh>, Vec<Vec3>) {
        self.flush();
        (self.out, self.anchors)
    }
}

/// Cuts a polygon to a little past the map's edges (Sutherland-Hodgman, one side at a time).
fn clip_ring(pts: &[Vec2]) -> Vec<Vec2> {
    let m = 4.0;
    let (lo, hi) = (-m, W.max(H) as f32 + m);
    let sides: [(usize, f32, bool); 4] = [(0, lo, true), (0, hi, false), (1, lo, true), (1, hi, false)];
    let mut ring = pts.to_vec();
    for (axis, at, keep_above) in sides {
        let inside = |p: Vec2| if keep_above { p[axis] >= at } else { p[axis] <= at };
        let mut out = Vec::with_capacity(ring.len());
        for k in 0..ring.len() {
            let (a, b) = (ring[k], ring[(k + 1) % ring.len()]);
            if inside(a) {
                out.push(a);
            }
            if inside(a) != inside(b) {
                out.push(a + (b - a) * ((at - a[axis]) / (b[axis] - a[axis])));
            }
        }
        ring = out;
        if ring.len() < 3 {
            return ring;
        }
    }
    ring
}

/// Cuts every triangle to the square map, so streets, rivers and blocks that run past the edge of
/// the map end cleanly on the border.
fn clip_to_map(v: &mut Vec<Vertex>, idx: &mut Vec<u16>) {
    let (w, h) = (W as f32, H as f32);
    let inside = |p: &Vertex| p.position.x >= 0.0 && p.position.y >= 0.0 && p.position.x <= w && p.position.y <= h;
    if v.iter().all(inside) {
        return;
    }
    let lerp = |a: &Vertex, b: &Vertex, t: f32| {
        let c = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
        Vertex {
            position: a.position.lerp(b.position, t),
            uv: a.uv.lerp(b.uv, t),
            color: [c(a.color[0], b.color[0]), c(a.color[1], b.color[1]), c(a.color[2], b.color[2]), c(a.color[3], b.color[3])],
            normal: a.normal,
        }
    };
    let mut nv: Vec<Vertex> = Vec::with_capacity(v.len());
    let mut ni: Vec<u16> = Vec::with_capacity(idx.len());
    let mut remap = vec![u16::MAX; v.len()];
    for t in idx.chunks_exact(3) {
        let tri = [&v[t[0] as usize], &v[t[1] as usize], &v[t[2] as usize]];
        if tri.iter().all(|p| inside(p)) {
            for &k in t {
                let k = k as usize;
                if remap[k] == u16::MAX {
                    remap[k] = nv.len() as u16;
                    nv.push(v[k]);
                }
                ni.push(remap[k]);
            }
            continue;
        }
        // Sutherland-Hodgman against the four map edges
        let mut poly: Vec<Vertex> = tri.iter().map(|p| **p).collect();
        for (axis, lim, keep_below) in [(0, 0.0, false), (0, w, true), (1, 0.0, false), (1, h, true)] {
            if poly.is_empty() {
                break;
            }
            let val = |p: &Vertex| if axis == 0 { p.position.x } else { p.position.y };
            let ok = |p: &Vertex| if keep_below { val(p) <= lim } else { val(p) >= lim };
            let mut out = Vec::with_capacity(poly.len() + 2);
            for k in 0..poly.len() {
                let (a, b) = (&poly[k], &poly[(k + 1) % poly.len()]);
                if ok(a) {
                    out.push(*a);
                }
                if ok(a) != ok(b) {
                    let t = (lim - val(a)) / (val(b) - val(a));
                    out.push(lerp(a, b, t));
                }
            }
            poly = out;
        }
        if poly.len() >= 3 {
            let base = nv.len() as u16;
            nv.extend_from_slice(&poly);
            for k in 1..poly.len() as u16 - 1 {
                ni.extend_from_slice(&[base, base + k, base + k + 1]);
            }
        }
    }
    *v = nv;
    *idx = ni;
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
    for i in &d.islands {
        mb.poly(i, th.land);
    }
    for p in d.parks.iter().filter(|p| d.islands.iter().any(|i| point_in(i, centroid(p)))) {
        mb.poly(p, th.park);
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
    /// where each `solid` mesh's building stands (painter mode has one building per mesh)
    anchors: Vec<Vec3>,
    rubble: Vec<Mesh>,
    members: Vec<usize>,
}

fn bake_tile(g: &Game, th: &Theme, members: &[usize], infected: &std::collections::HashSet<usize>, painter: bool) -> (Vec<Mesh>, Vec<Vec3>, Vec<Mesh>) {
    let mut mb = MB::split(painter);
    let mut rubble = MB::default();
    let hive_col = rgb(150, 70, 190);
    for &bi in members {
        let b = &g.world.buildings[bi];
        if !b.alive {
            rubble.poly(&b.poly, th.rubble);
            continue;
        }
        mb.obj(b.c, 0.0);
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
            mb.wall(a, c, 0.0, b.h, nrm, mix(col, BLACK, 0.15), col);
            // window bands on taller blocks, nudged off the wall so they don't flicker
            if b.h > 7.0 && e.length() > 3.0 {
                let floors = (b.h / 3.2) as i32;
                let inset = e.normalize() * 0.7;
                let out = nrm * 0.06;
                for f in 1..floors {
                    let z = f as f32 * 3.2 - 1.6;
                    let (p0, p1) = (a + inset + out, c - inset + out);
                    mb.wall(p0, p1, z - 0.45, z + 0.45, nrm, th.window, th.window);
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
        if inf {
            art::fungus(&mut mb, b, bi);
        }
    }
    let (solid, anchors) = mb.finish_objs();
    (solid, anchors, rubble.finish())
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
    /// the base zone's fill and outline
    zone_mesh: Vec<Mesh>,
    pub flat: Material,
    /// painter mode (no depth buffer): solid faces, culled, and everything else, not culled
    pub painter: bool,
    solid: Material,
    bare: Material,
    /// solids with depth testing (when the depth buffer works)
    depth: Material,
    /// glowing line art: depth tested (when it works) but not written, so strokes blend
    line_mat: Material,
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
    /// `painter`: the GPU's depth buffer doesn't work, draw solid things far to near instead.
    pub fn new(g: &Game, th: &Theme, painter: bool) -> Scene {
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
                let (solid, anchors, rubble) = bake_tile(g, th, &m, &inf, painter);
                Tile { solid, anchors, rubble, members: m }
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
            zone_mesh: Vec::new(),
            flat: flat_material(),
            painter,
            solid: painter_material(true),
            bare: painter_material(false),
            depth: depth_material(),
            line_mat: glow_material(painter),
            dark: th.dark,
            btimer: 0.0,
            ztimer: 0.0,
            need_zone: true,
            units_fresh: false,
        }
    }

    /// The material for solid 3D things drawn outside `draw_world` (placement ghosts).
    pub fn use_solid(&self) {
        if self.painter {
            gl_use_material(&self.solid);
        } else {
            gl_use_default_material();
        }
    }

    pub fn retheme(&mut self, g: &Game, th: &Theme) {
        self.ground = build_ground(&g.world, th);
        let inf = infected_set(g);
        for t in &mut self.tiles {
            (t.solid, t.anchors, t.rubble) = bake_tile(g, th, &t.members, &inf, self.painter);
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

    fn update_zone(&mut self, g: &Game, th: &Theme, base: bool) {
        if base {
            self.zone_mesh = zone_meshes(&g.world.zone, th);
        }
        let rad = rgba(190, 230, 40, 0.30);
        let hive = rgba(170, 80, 220, 0.20);
        let w = &g.world;
        for y in 0..ZH {
            for x in 0..ZW {
                let o = (y * ZW + x) * 4;
                let p = vec2((x * ZS) as f32 + 2.0, (y * ZS) as f32 + 2.0);
                let mut c = Color::new(0.0, 0.0, 0.0, 0.0);
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
                    (t.solid, t.anchors, t.rubble) = bake_tile(g, th, &t.members, &inf, self.painter);
                }
            }
        }
        if g.zone_dirty || self.need_zone || (!g.hives.is_empty() && self.ztimer <= 0.0) {
            self.ztimer = 2.0;
            let base = g.zone_dirty || self.need_zone;
            g.zone_dirty = false;
            self.need_zone = false;
            self.update_zone(g, th, base);
        }
    }
}

// ------------------------------------------------------------------------------------------ base zone

/// Contour grid spacing for the base zone outline, in cells.
const ZG: usize = 2;
const GW: usize = W / ZG + 1;
const GH: usize = H / ZG + 1;
/// The level of the blurred mask the outline follows: a little under half, so the line runs just
/// outside the perimeter wall instead of under it.
const ISO: f32 = 0.35;

/// The base zone as crisp vector shapes: the zone mask is sampled every `ZG` cells and blurred so
/// its stair steps round off, then marching squares traces the `ISO` line. The fill comes from
/// the same squares, so it meets the outline exactly; the outline is a mitred strip like a road.
fn zone_meshes(zone: &[u8], th: &Theme) -> Vec<Mesh> {
    let mut f: Vec<f32> = (0..GW * GH)
        .map(|k| {
            let (x, y) = ((k % GW * ZG).min(W - 1), (k / GW * ZG).min(H - 1));
            zone[y * W + x] as f32
        })
        .collect();
    // two box blurs, radius 2 grid points, make a soft hump around every corner
    let mut tmp = vec![0.0f32; GW * GH];
    for _ in 0..2 {
        tmp.par_chunks_mut(GW).enumerate().for_each(|(y, row)| {
            for x in 0..GW {
                let (a, b) = (x.saturating_sub(2), (x + 2).min(GW - 1));
                row[x] = (a..=b).map(|i| f[y * GW + i]).sum::<f32>() / (b - a + 1) as f32;
            }
        });
        f.par_chunks_mut(GW).enumerate().for_each(|(y, row)| {
            let (a, b) = (y.saturating_sub(2), (y + 2).min(GH - 1));
            for x in 0..GW {
                row[x] = (a..=b).map(|j| tmp[j * GW + x]).sum::<f32>() / (b - a + 1) as f32;
            }
        });
    }
    let fill = th.zone;
    let edge = Color { a: 1.0, ..mix(th.land, Color { a: 1.0, ..th.zone_edge }, (th.zone_edge.a + 0.25).min(1.0)) };
    let mut mb = MB::default();
    let at = |x: usize, y: usize| vec2((x * ZG) as f32, (y * ZG) as f32);
    // crossing point on the grid edge from point (x, y) towards +x (dir 0) or +y (dir 1)
    let cross = |x: usize, y: usize, dir: usize| -> Vec2 {
        let (x2, y2) = if dir == 0 { (x + 1, y) } else { (x, y + 1) };
        let (a, b) = (f[y * GW + x], f[y2 * GW + x2]);
        let t = ((ISO - a) / (b - a)).clamp(0.0, 1.0);
        at(x, y).lerp(at(x2, y2), t)
    };
    let eid = |x: usize, y: usize, dir: usize| ((y * GW + x) * 2 + dir) as u32;
    let mut links: std::collections::HashMap<u32, (Vec2, [u32; 2])> = std::collections::HashMap::new();
    let mut link = |a: (u32, Vec2), b: (u32, Vec2)| {
        for (s, o) in [(a, b.0), (b, a.0)] {
            let e = links.entry(s.0).or_insert((s.1, [u32::MAX; 2]));
            if e.1[0] == u32::MAX {
                e.1[0] = o;
            } else {
                e.1[1] = o;
            }
        }
    };
    for y in 0..GH - 1 {
        let mut run: Option<usize> = None;
        for x in 0..GW {
            let full = x < GW - 1 && [f[y * GW + x], f[y * GW + x + 1], f[(y + 1) * GW + x], f[(y + 1) * GW + x + 1]].iter().all(|v| *v >= ISO);
            if full {
                run.get_or_insert(x);
                continue;
            }
            // whole squares inside: one quad per run along the row
            if let Some(x0) = run.take() {
                mb.quad(at(x0, y), at(x, y), at(x, y + 1), at(x0, y + 1), fill);
            }
            if x == GW - 1 {
                break;
            }
            // corners clockwise, with the edge each one starts
            let corners = [(x, y, 0usize), (x + 1, y, 1), (x + 1, y + 1, 0), (x, y + 1, 1)];
            let ins: Vec<bool> = corners.iter().map(|(cx, cy, _)| f[cy * GW + cx] >= ISO).collect();
            if !ins.iter().any(|v| *v) {
                continue;
            }
            let mut poly = Vec::with_capacity(8);
            let mut xs: Vec<(u32, Vec2)> = Vec::with_capacity(4);
            for k in 0..4 {
                let (cx, cy, _) = corners[k];
                if ins[k] {
                    poly.push(at(cx, cy));
                }
                if ins[k] != ins[(k + 1) % 4] {
                    // the edge between corner k and k+1, named by its lower-left end
                    let (e, p) = match k {
                        0 => (eid(x, y, 0), cross(x, y, 0)),
                        1 => (eid(x + 1, y, 1), cross(x + 1, y, 1)),
                        2 => (eid(x, y + 1, 0), cross(x, y + 1, 0)),
                        _ => (eid(x, y, 1), cross(x, y, 1)),
                    };
                    poly.push(p);
                    xs.push((e, p));
                }
            }
            mb.room(poly.len());
            let base = mb.v.len() as u16;
            for p in &poly {
                mb.vert(*p, fill);
            }
            for k in 1..poly.len() as u16 - 1 {
                mb.i.extend_from_slice(&[base, base + k, base + k + 1]);
            }
            // outline pieces, paired the same way the fill polygon joins the corners
            match xs.len() {
                2 => link(xs[0], xs[1]),
                4 if ins[0] => {
                    link(xs[0], xs[1]);
                    link(xs[2], xs[3]);
                }
                4 => {
                    link(xs[1], xs[2]);
                    link(xs[3], xs[0]);
                }
                _ => {}
            }
        }
    }
    // chain the pieces into lines: open ones from their ends first, then the loops
    let mut seen: std::collections::HashSet<u32> = std::collections::HashSet::new();
    let mut starts: Vec<u32> = links.iter().filter(|(_, v)| v.1[1] == u32::MAX).map(|(k, _)| *k).collect();
    starts.sort_unstable();
    let mut rest: Vec<u32> = links.keys().copied().collect();
    rest.sort_unstable();
    starts.extend(rest);
    for s in starts {
        if seen.contains(&s) {
            continue;
        }
        let mut pts = Vec::new();
        let (mut prev, mut cur) = (u32::MAX, s);
        let mut closed = false;
        loop {
            seen.insert(cur);
            let (p, nb) = links[&cur];
            pts.push(p);
            let next = if nb[0] != prev { nb[0] } else { nb[1] };
            if next == u32::MAX {
                break;
            }
            if next == s {
                closed = true;
                break;
            }
            if seen.contains(&next) {
                break;
            }
            prev = cur;
            cur = next;
        }
        border_line(&mut mb, &smooth(&pts, closed), closed, 1.3, edge);
    }
    mb.finish()
}

/// One round of Chaikin corner cutting.
fn smooth(p: &[Vec2], closed: bool) -> Vec<Vec2> {
    let n = p.len();
    if n < 3 {
        return p.to_vec();
    }
    let mut out = Vec::with_capacity(n * 2);
    if !closed {
        out.push(p[0]);
    }
    let segs = if closed { n } else { n - 1 };
    for k in 0..segs {
        let (a, b) = (p[k], p[(k + 1) % n]);
        out.push(a.lerp(b, 0.25));
        out.push(a.lerp(b, 0.75));
    }
    if !closed {
        out.push(p[n - 1]);
    }
    out
}

/// A line `2 * r` wide with mitred joins, so a smooth curve stays one clean strip.
fn border_line(mb: &mut MB, p: &[Vec2], closed: bool, r: f32, col: Color) {
    let n = p.len();
    if n < 2 {
        return;
    }
    let seg_n = |k: usize| -> Vec2 {
        let d = (p[(k + 1) % n] - p[k]).normalize_or_zero();
        vec2(-d.y, d.x)
    };
    let off: Vec<Vec2> = (0..n)
        .map(|k| {
            let a = if k > 0 { seg_n(k - 1) } else if closed { seg_n(n - 1) } else { seg_n(0) };
            let b = if k + 1 < n || closed { seg_n(k) } else { seg_n(k - 1) };
            let m = (a + b).normalize_or_zero();
            m * r / m.dot(b).max(0.4)
        })
        .collect();
    let segs = if closed { n } else { n - 1 };
    for k in 0..segs {
        let j = (k + 1) % n;
        mb.quad(p[k] + off[k], p[j] + off[j], p[j] - off[j], p[k] - off[k], col);
    }
}

// ------------------------------------------------------------------------------------------ shapes

fn rot(v: Vec2, a: f32) -> Vec2 {
    let (s, c) = a.sin_cos();
    vec2(v.x * c - v.y * s, v.x * s + v.y * c)
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
    /// a selected structure (artillery or heli pad being aimed)
    pub sel_struct: Option<usize>,
    pub time: f32,
    /// the camera's ground position (painter mode draws far to near from it)
    pub eye: Vec2,
}

fn bar(mb: &mut MB, p: Vec2, h: f32, w: f32, f: f32, col: Color) {
    let (x, y) = (p.x - w / 2.0, p.y);
    // corners listed to face up, like roofs
    mb.quad3(p3(vec2(x, y), h), p3(vec2(x, y + 0.9), h), p3(vec2(x + w, y + 0.9), h), p3(vec2(x + w, y), h), Color::new(0.0, 0.0, 0.0, 0.55), Color::new(0.0, 0.0, 0.0, 0.55));
    let fw = w * f.clamp(0.0, 1.0);
    let h2 = h + 0.05;
    mb.quad3(p3(vec2(x, y), h2), p3(vec2(x, y + 0.9), h2), p3(vec2(x + fw, y + 0.9), h2), p3(vec2(x + fw, y), h2), col, col);
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
        if (c.st >= C_DEAD && c.st != C_TO_PAD && c.st != C_AT_PAD && c.st != C_TO_IDLE) || !inview(c.p, 2.0) {
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
            3 => {
                // flame: hot core in a wider orange lick
                mb.disc(b.p, b.r * (0.8 + 0.5 * f), Color::new(1.0, 0.38, 0.05, 0.7 * (1.0 - f * f)));
                mb.disc(b.p, b.r * (0.5 + 0.3 * f), Color::new(1.0, 0.72, 0.2, 0.85 * (1.0 - f)));
                mb.disc(b.p, b.r * 0.25, Color::new(1.0, 0.95, 0.7, 0.9 * (1.0 - f)));
            }
            4 => ring(&mut mb, b.p, b.r * (0.4 + f * 1.4), 0.6, Color::new(0.75, 0.95, 1.0, 1.0 - f)),
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
        if s.pad.is_some() && s.phase < 2 {
            ring(&mut mb, s.center, 3.0, 0.5, rgba(255, 212, 59, 0.8));
        }
    }
    // railgun slugs: a bright streak that fades
    for tr in &g.tracers {
        if tr.kind == 6 {
            let f = (tr.t / 0.4).clamp(0.0, 1.0);
            mb.line(&[tr.a, tr.b], 5.5 * f, Color::new(0.55, 0.85, 1.0, 0.35 * f));
            mb.line(&[tr.a, tr.b], 1.0 * f, Color::new(0.9, 0.98, 1.0, 0.9 * f));
        }
    }
    for (i, s) in g.structs.iter().enumerate() {
        if s.dead || s.pending {
            continue;
        }
        let c = s.center();
        match s.t {
            S::Helium if inview(c, 60.0) => {
                let r = g.turret_range(S::Helium, s.roof >= 0);
                mb.disc(c, r, rgba(165, 216, 255, 0.10 + 0.03 * (t * 1.3).sin()));
                ring(&mut mb, c, r, 0.5, rgba(200, 235, 255, 0.55));
                for k in 0..10 {
                    let a = k as f32 * 0.628 + t * 0.25;
                    let q = c + vec2(a.cos(), a.sin()) * r * (0.35 + 0.5 * ((k * 7 % 10) as f32 / 10.0));
                    let n = 0.9;
                    mb.line(&[q - vec2(n, 0.0), q + vec2(n, 0.0)], 0.25, rgba(230, 245, 255, 0.5));
                    mb.line(&[q - vec2(0.0, n), q + vec2(0.0, n)], 0.25, rgba(230, 245, 255, 0.5));
                }
            }
            S::Rail if s.charge > 0.0 => {
                if let Some(tg) = s.target {
                    let f = 1.0 - s.charge / RAIL_CHARGE;
                    let dir = (tg - c).normalize_or_zero();
                    let end = c + dir * g.turret_range(S::Rail, s.roof >= 0);
                    mb.dashed(&[c, end], 0.3 + 0.5 * f, 3.0, 2.0, Color::new(0.6, 0.9, 1.0, 0.25 + 0.5 * f));
                }
            }
            S::Artillery => {
                if let Some(tg) = s.target {
                    let sel = v.sel_struct == Some(i);
                    let col = if sel { rgba(255, 90, 70, 0.9) } else { rgba(255, 90, 70, 0.45) };
                    let r = def(S::Artillery).rad + 3.0 + tg.distance(c) * 0.012;
                    ring(&mut mb, tg, r, 0.6, col);
                    mb.line(&[tg - vec2(r * 0.5, 0.0), tg + vec2(r * 0.5, 0.0)], 0.4, col);
                    mb.line(&[tg - vec2(0.0, r * 0.5), tg + vec2(0.0, r * 0.5)], 0.4, col);
                    if sel {
                        mb.dashed(&[c, tg], 0.5, 5.0, 4.0, rgba(255, 90, 70, 0.4));
                    }
                }
            }
            _ => {}
        }
    }
    // burning napalm: dark scorch under a flickering orange pool
    for (k, n) in g.napalm.iter().enumerate() {
        if !inview(n.p, 6.0) {
            continue;
        }
        let f = (n.t / NAPALM_LIFE).clamp(0.0, 1.0);
        let fl = 0.75 + 0.25 * (t * 17.0 + k as f32 * 1.7).sin();
        mb.disc(n.p, n.r * 1.05, Color::new(0.12, 0.06, 0.02, 0.35 * f));
        mb.disc(n.p, n.r * (0.75 + 0.15 * fl), Color::new(1.0, 0.36, 0.05, 0.55 * f * fl));
        mb.disc(n.p, n.r * 0.4 * fl, Color::new(1.0, 0.8, 0.3, 0.6 * f));
    }
    // shell shadows
    for sh in &g.shells {
        let (p, _) = arc(sh.from, sh.to, sh.t / sh.dur);
        mb.disc(p, 0.9, Color::new(0.0, 0.0, 0.0, 0.25));
    }
    mb
}

/// Player structures and other things with height, rebuilt every frame (there are only hundreds).
fn solid_dynamic(g: &Game, v: &ViewInfo, labels: &mut WorldLabels, painter: bool) -> MB {
    let mut mb = MB::split(painter);
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
        // on a roof: drawn just after that building
        if s.roof >= 0 {
            mb.obj(g.world.buildings[s.roof as usize].c, 1.0);
        } else {
            mb.obj(c, 0.0);
        }
        if s.pending {
            // flown in by helicopter: a translucent ghost until it lands
            mb.prism(&rect_pts(x, y, w, h), base, base + 0.3, alpha(d.top, 0.3), alpha(d.top, 0.3));
            continue;
        }
        if v.sel_struct == Some(i) {
            art::ring_flat(&mut mb, c, w.max(h) / 2.0 + 1.5, 0.5, base + 0.05, rgb(255, 212, 59));
        }
        let top = art::structure(&mut mb, g, i, s, base, t, labels);
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
        mb.obj(b.c, 1.0);
        mb.prism(&pts, b.h, b.h + 0.4, if sel { rgb(255, 212, 59) } else { WHITE }, col);
        let pts: Vec<Vec2> = (0..12).map(|k| b.c + vec2((k as f32 * 0.524).cos(), (k as f32 * 0.524).sin()) * 2.3).collect();
        mb.prism(&pts, b.h + 0.4, b.h + 0.5, col, col);
        labels.items.push((p3(b.c, b.h + 3.0), format!("{}", n), WHITE, 12.0));
    }
    // hives: a living mound of pods
    for h in &g.hives {
        if h.dead || !inview(h.p, 30.0) {
            continue;
        }
        mb.obj(h.p, 0.0);
        let top = art::hive(&mut mb, h, t);
        let size = 5.0 + h.r * 0.06;
        bar(&mut mb, h.p + vec2(0.0, -size - 3.0), top + 1.0, 14.0, h.hp / h.max, rgb(204, 93, 232));
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
    for m in &sc.zone_mesh {
        draw_mesh(m);
    }
    draw_texture_ex(&sc.glow, 0.0, 0.0, WHITE, full.clone());
    draw_texture_ex(&sc.units, 0.0, 0.0, WHITE, full.clone());
    for m in ground_dynamic(g, th, v).finish() {
        draw_mesh(&m);
    }
    // shadows of aircraft are drawn with the aircraft below

    // ---- everything with height
    let near = |c: Vec2| c.distance(v.center) < v.radius + TILE as f32;
    let tile_c = |k: usize| vec2(((k % TW) * TILE + TILE / 2) as f32, ((k / TW) * TILE + TILE / 2) as f32);
    let order = (0..sc.tiles.len()).filter(|k| near(tile_c(*k)));
    let (dynamic, dyn_at) = solid_dynamic(g, v, labels, sc.painter).finish_objs();
    if sc.painter {
        // no depth buffer: every building and structure on its own, far to near, inside faces
        // culled. Each one is a closed box, so this covers correctly without depth.
        let fov = v.radius + 40.0;
        let mut objs: Vec<(f32, &Mesh)> = Vec::new();
        for k in order {
            let t = &sc.tiles[k];
            for (m, a) in t.solid.iter().zip(&t.anchors) {
                if a.truncate().distance(v.center) < fov {
                    objs.push((a.truncate().distance(v.eye) - a.z, m));
                }
            }
        }
        for (m, a) in dynamic.iter().zip(&dyn_at) {
            objs.push((a.truncate().distance(v.eye) - a.z, m));
        }
        objs.sort_by(|a, b| b.0.total_cmp(&a.0));
        sc.solid.set_uniform("Time", t);
        gl_use_material(&sc.solid);
        for (_, m) in objs {
            draw_mesh(m);
        }
        gl_use_material(&sc.bare);
    } else {
        sc.depth.set_uniform("Time", t);
        gl_use_material(&sc.depth);
        for k in order {
            for m in &sc.tiles[k].solid {
                draw_mesh(m);
            }
        }
        for m in &dynamic {
            draw_mesh(m);
        }
        gl_use_default_material();
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
            6 => rgb(230, 248, 255),
            _ => rgb(220, 235, 255),
        };
        draw_line_3d(p3(tr.a, tr.az), p3(tr.b, 0.3), c);
    }
    for b in &g.booms {
        if b.kind == 3 {
            // flame: a licking ball of fire above the ground puff
            let f = 1.0 - b.t / b.life;
            draw_sphere(p3(b.p, 0.8 + f * 1.5), b.r * (0.35 + 0.25 * f), None, Color::new(1.0, 0.55 + 0.3 * (1.0 - f), 0.15, 0.55 * (1.0 - f)));
        }
        if b.kind == 1 {
            let f = 1.0 - b.t / b.life;
            draw_sphere(p3(b.p, 0.0), b.r * (0.2 + 0.5 * f), None, Color::new(1.0, 0.7, 0.3, 0.35 * (1.0 - f)));
        }
    }
    // ICBMs riding their arc, pins on the targets
    let mut glow = air::Glow::default();
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
            let (q, qh) = arc(*from, *to, (f + 0.002).min(1.0));
            let dir = if f < 0.998 { p3(q, qh) - p3(p, h) } else { p3(p, h) - p3(arc(*from, *to, f - 0.002).0, arc(*from, *to, f - 0.002).1) };
            let trail: Vec<Vec3> = (0..=16).map(|k| {
                let ff = (f - 0.12 * (1.0 - k as f32 / 16.0)).max(0.0);
                let (p, h) = arc(*from, *to, ff);
                p3(p, h)
            }).collect();
            air::missile_trail(&mut glow, &trail, mk);
            air::missile(&mut glow, p3(p, h), dir, t, mk);
        }
    }
    for s in &g.strikes {
        if let Strike::Air { from, dir, pos, .. } = s {
            air::bomber(&mut glow, *from + *dir * *pos, dir.to_angle(), mk);
        }
    }
    // artillery shells on their lob
    for sh in &g.shells {
        let f = sh.t / sh.dur;
        let (p, hgt) = arc(sh.from, sh.to, f);
        let hgt = hgt * 0.5;
        at_height(hgt, || {
            draw_circle(p.x, p.y, 0.9 * mk, rgb(255, 200, 120));
            draw_circle(p.x, p.y, 0.55 * mk, rgb(60, 50, 40));
        });
    }
    // gunships parked on their heli pads
    for (i, s) in g.structs.iter().enumerate() {
        if s.t == S::Helipad && !s.dead && !s.pending && g.pad_gunship(i).is_none() {
            air::heli(&mut glow, s.center(), def(S::Helipad).ht + 1.0, -std::f32::consts::FRAC_PI_2, if s.cd > 0.0 { 0.0 } else { t }, true, false, mk.min(1.4));
        }
    }
    for h in &g.helis {
        let f = (h.t / h.dur).clamp(0.0, 1.0);
        let ground = h.from.lerp(h.to, f);
        let alt = 30.0 * (f * 8.0).min(1.0) * ((1.0 - f) * 8.0).min(1.0) + 1.0;
        air::heli(&mut glow, ground, alt, (h.to - h.from).to_angle(), t, false, matches!(h.kind, HeliKind::Deliver(_)), mk);
    }
    for s in &g.gunships {
        let heading = if s.phase == 1 { s.ang + std::f32::consts::FRAC_PI_2 } else { (s.center - s.p).to_angle() };
        air::heli(&mut glow, s.p, 16.0 + 20.0, heading, t, true, false, mk);
    }
    gl_use_material(&sc.line_mat);
    for m in glow.finish() {
        draw_mesh(&m);
    }
    gl_use_default_material();
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

// ------------------------------------------------------------------------------------------ GPU check

/// How the 3D view gets a working depth buffer on this GPU.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DepthMode {
    /// our own render target with depth, at this many MSAA samples
    Target(i32),
    /// no depth buffer at all: the window's framebuffer, solid things sorted and drawn far to near
    Painter,
}

/// What a little test scene came out as: Ok, NoDepth (far drawn over near) or Hidden (anything
/// depth-tested vanished), plus the centre pixel for the log.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DepthCheck {
    Ok,
    NoDepth,
    Hidden,
    Other,
}

/// Draws the game's draw order in miniature into a render target and reads it back: a street
/// painted with the ground material, then a near green box and a far red box over it. A working
/// depth buffer shows green. Some Windows drivers showed red (no depth test) or the street (every
/// depth-tested thing hidden), so the game tries each way of drawing until one shows green.
pub fn check_depth(samples: i32) -> (DepthCheck, [u8; 4]) {
    let rt = render_target_ex(32, 32, RenderTargetParams { sample_count: samples, depth: true });
    let flat = flat_material();
    let cam = Camera3D {
        position: vec3(0.0, 0.0, -100.0),
        target: vec3(0.0, 0.0, 0.0),
        up: vec3(0.0, -1.0, 0.0),
        fovy: 0.6,
        z_near: 5.0,
        z_far: 400.0,
        render_target: Some(rt.clone()),
        ..Default::default()
    };
    let quad = |z: f32, c: Color| Mesh {
        vertices: [(-40.0, -40.0), (40.0, -40.0), (40.0, 40.0), (-40.0, 40.0)].iter().map(|(x, y)| Vertex::new(*x, *y, z, 0.0, 0.0, c)).collect(),
        indices: vec![0, 1, 2, 0, 2, 3],
        texture: None,
    };
    set_camera(&cam);
    clear_background(BLACK);
    gl_use_material(&flat);
    draw_mesh(&quad(0.0, BLUE));
    gl_use_default_material();
    draw_mesh(&quad(-30.0, GREEN));
    draw_mesh(&quad(-10.0, RED));
    set_default_camera();
    unsafe {
        get_internal_gl().flush();
    }
    let img = rt.texture.get_texture_data();
    let k = (16 * 32 + 16) * 4;
    let px = [img.bytes[k], img.bytes[k + 1], img.bytes[k + 2], img.bytes[k + 3]];
    let r = match (px[0] > 128, px[1] > 128, px[2] > 128) {
        (false, true, false) => DepthCheck::Ok,
        (true, false, false) => DepthCheck::NoDepth,
        (false, false, true) => DepthCheck::Hidden,
        _ => DepthCheck::Other,
    };
    (r, px)
}

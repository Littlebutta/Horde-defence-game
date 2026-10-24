//! Aircraft and missiles as glowing line art: helicopters, the gunship and the B-2 in yellow, the
//! ICBM in red. Each stroke is a strip that is hot white-ish down the middle, solid at the core
//! and fades out to the sides, so the lines glow a little against the city.

use super::*;

pub const AIR: Color = rgb(255, 212, 59);
pub const MISSILE: Color = rgb(255, 72, 56);
const INK: Color = Color::new(0.05, 0.06, 0.09, 0.45);

/// Glowing strokes and soft fills, in a flat frame (origin + two axes, already scaled).
#[derive(Default)]
pub struct Glow {
    pub meshes: Vec<Mesh>,
    v: Vec<Vertex>,
    i: Vec<u16>,
    o: Vec3,
    ex: Vec3,
    ey: Vec3,
}

impl Glow {
    /// Draws from here on in the plane through `o` spanned by `ex`, `ey` (their length is the scale).
    pub fn frame(&mut self, o: Vec3, ex: Vec3, ey: Vec3) {
        self.o = o;
        self.ex = ex;
        self.ey = ey;
    }
    /// A flat frame at height `h` over `p`, nose along `heading`, `s` cells per unit.
    pub fn flat(&mut self, p: Vec2, h: f32, heading: f32, s: f32) {
        let (sn, cs) = heading.sin_cos();
        self.frame(p3(p, h), vec3(cs, sn, 0.0) * s, vec3(-sn, cs, 0.0) * s);
    }
    fn at(&self, q: Vec2) -> Vec3 {
        self.o + self.ex * q.x + self.ey * q.y
    }
    fn room(&mut self, nv: usize, ni: usize) {
        if self.v.len() + nv > 9000 || self.i.len() + ni > 4800 {
            self.flush();
        }
    }
    fn flush(&mut self) {
        if !self.v.is_empty() {
            self.meshes.push(Mesh { vertices: std::mem::take(&mut self.v), indices: std::mem::take(&mut self.i), texture: None });
        }
    }
    pub fn finish(mut self) -> Vec<Mesh> {
        self.flush();
        self.meshes
    }

    /// A glowing line through local points `pts`; `w` is the core width in local units.
    pub fn stroke(&mut self, pts: &[Vec2], closed: bool, w: f32, col: Color) {
        let m = pts.len();
        if m < 2 {
            return;
        }
        let segs = if closed { m } else { m - 1 };
        self.room(m * 5, segs * 24);
        let nrm = self.ex.cross(self.ey).normalize_or_zero();
        let scale = self.ex.length();
        let wp: Vec<Vec3> = pts.iter().map(|q| self.at(*q)).collect();
        let (core, halo) = (w * 0.5 * scale, w * 2.2 * scale);
        let hot = mix(col, WHITE, 0.5);
        let edge = alpha(col, 0.0);
        let base = self.v.len() as u16;
        for k in 0..m {
            let prev = if k > 0 { wp[k - 1] } else if closed { wp[m - 1] } else { wp[k] };
            let next = if k + 1 < m { wp[k + 1] } else if closed { wp[0] } else { wp[k] };
            let d0 = (wp[k] - prev).normalize_or_zero();
            let d1 = (next - wp[k]).normalize_or_zero();
            let t = (d0 + d1).normalize_or_zero();
            let t = if t == Vec3::ZERO { if d1 != Vec3::ZERO { d1 } else { d0 } } else { t };
            let side = nrm.cross(t).normalize_or_zero();
            let any = if d1 != Vec3::ZERO { d1 } else { d0 };
            let miter = 1.0 / side.dot(nrm.cross(any).normalize_or_zero()).max(0.5);
            for (off, c) in [(-halo, edge), (-core, col), (0.0, hot), (core, col), (halo, edge)] {
                let p = wp[k] + side * off * miter;
                self.v.push(Vertex::new(p.x, p.y, p.z, 0.0, 0.0, c));
            }
        }
        for k in 0..segs as u16 {
            let j = (k + 1) % m as u16;
            for b in 0..4u16 {
                let (a0, a1, b0, b1) = (base + k * 5 + b, base + k * 5 + b + 1, base + j * 5 + b, base + j * 5 + b + 1);
                self.i.extend_from_slice(&[a0, b0, b1, a0, b1, a1]);
            }
        }
    }

    /// A flat fill (any simple polygon) in local points.
    pub fn fill(&mut self, pts: &[Vec2], col: Color) {
        let flat: Vec<f32> = pts.iter().flat_map(|p| [p.x, p.y]).collect();
        let Ok(tris) = earcutr::earcut(&flat, &[], 2) else { return };
        self.room(pts.len(), tris.len());
        let base = self.v.len() as u16;
        for q in pts {
            let p = self.at(*q);
            self.v.push(Vertex::new(p.x, p.y, p.z, 0.0, 0.0, col));
        }
        self.i.extend(tris.iter().map(|t| base + *t as u16));
    }

    pub fn circle(&mut self, c: Vec2, r: f32, w: f32, col: Color) {
        let n = if r > 2.0 { 32 } else { 14 };
        self.stroke(&arc_pts(c, r, 0.0, std::f32::consts::TAU, n, false), true, w, col);
    }
}

/// Points on a circle from angle `a0` to `a1`.
fn arc_pts(c: Vec2, r: f32, a0: f32, a1: f32, n: usize, end: bool) -> Vec<Vec2> {
    let m = if end { n + 1 } else { n };
    (0..m).map(|k| {
        let a = a0 + (a1 - a0) * k as f32 / n as f32;
        c + vec2(a.cos(), a.sin()) * r
    }).collect()
}

/// `half` (the y >= 0 side, nose first) mirrored into a closed outline.
fn mirrored(half: &[Vec2]) -> Vec<Vec2> {
    let mut out = half.to_vec();
    for q in half.iter().rev() {
        if q.y.abs() > 1e-4 {
            out.push(vec2(q.x, -q.y));
        }
    }
    out
}

fn both(g: &mut Glow, pts: &[Vec2], closed: bool, w: f32, col: Color) {
    g.stroke(pts, closed, w, col);
    let m: Vec<Vec2> = pts.iter().map(|q| vec2(q.x, -q.y)).collect();
    g.stroke(&m, closed, w, col);
}

fn ground_shadow(pts: &[Vec2], at: Vec2, heading: f32, s: f32, a: f32) {
    let world: Vec<Vec2> = pts.iter().map(|q| at + rot(*q * s, heading)).collect();
    fill_concave(&world, Color::new(0.0, 0.0, 0.0, a));
}

// ------------------------------------------------------------------------------------------ helicopter

fn heli_body() -> Vec<Vec2> {
    mirrored(&[
        vec2(4.7, 0.0),
        vec2(4.5, 0.75),
        vec2(3.95, 1.35),
        vec2(3.0, 1.75),
        vec2(1.6, 1.95),
        vec2(0.0, 1.95),
        vec2(-1.4, 1.75),
        vec2(-2.4, 1.2),
        vec2(-3.3, 0.5),
        vec2(-9.0, 0.3),
        vec2(-9.6, 0.0),
    ])
}

/// A helicopter seen from above: fuselage, canopy, engine deck, tail, skids (or stub wings and
/// rocket pods on the gunship), a slung crate when it carries one, and a spinning rotor.
#[allow(clippy::too_many_arguments)]
pub fn heli(g: &mut Glow, ground: Vec2, alt: f32, heading: f32, time: f32, gunship: bool, cargo: bool, k: f32) {
    let s = 0.55 * k;
    let body = heli_body();
    ground_shadow(&body, ground, heading, s, 0.22);
    draw_circle(ground.x, ground.y, 6.6 * s, Color::new(0.0, 0.0, 0.0, 0.07));
    let w = 0.2;
    if cargo {
        // the crate on its sling, below the machine
        let ch = alt * 0.45;
        g.flat(ground, ch, heading, s);
        let crate_ = [vec2(-1.6, -1.6), vec2(1.6, -1.6), vec2(1.6, 1.6), vec2(-1.6, 1.6)];
        g.fill(&crate_, INK);
        g.stroke(&crate_, true, w, AIR);
        g.stroke(&[vec2(-1.6, -1.6), vec2(1.6, 1.6)], false, w * 0.7, AIR);
        g.stroke(&[vec2(-1.6, 1.6), vec2(1.6, -1.6)], false, w * 0.7, AIR);
        let a = p3(ground, alt);
        let b = p3(ground, ch);
        let side = vec3(-heading.sin(), heading.cos(), 0.0);
        // the sling itself is vertical, so draw it in a vertical frame
        g.frame(a, (b - a).normalize_or_zero(), side);
        g.stroke(&[vec2(0.0, 0.0), vec2((b - a).length(), 0.0)], false, 0.12 * s, AIR);
    }
    g.flat(ground, alt, heading, s);
    g.fill(&body, INK);
    g.stroke(&body, true, w, AIR);
    // canopy with its frame
    let canopy = mirrored(&[vec2(4.15, 0.0), vec2(3.9, 0.75), vec2(3.3, 1.25), vec2(2.3, 1.45), vec2(1.5, 1.4)]);
    g.stroke(&canopy, true, w * 0.8, AIR);
    g.stroke(&[vec2(1.5, 0.0), vec2(4.15, 0.0)], false, w * 0.5, AIR);
    // engine deck and exhausts
    let deck = mirrored(&[vec2(0.9, 0.0), vec2(0.8, 0.85), vec2(-2.0, 0.85), vec2(-2.4, 0.0)]);
    g.stroke(&deck, true, w * 0.7, AIR);
    both(g, &[vec2(-2.2, 0.55), vec2(-3.3, 0.9)], false, w * 0.7, AIR);
    // tailplane, fin and tail rotor
    both(g, &[vec2(-7.2, 0.3), vec2(-7.55, 1.95), vec2(-8.15, 1.95), vec2(-8.1, 0.3)], false, w * 0.8, AIR);
    let spin = (time * 40.0).sin();
    g.stroke(&[vec2(-9.3 - 1.1 * spin, 0.75), vec2(-9.3 + 1.1 * spin, 0.75)], false, w * 0.8, AIR);
    g.stroke(&[vec2(-9.3, 0.3), vec2(-9.3, 0.75)], false, w * 0.6, AIR);
    if gunship {
        // stub wings, rocket pods and the chin gun
        let wing = [vec2(0.7, 1.9), vec2(0.9, 4.0), vec2(-0.6, 4.0), vec2(-0.9, 1.85)];
        both(g, &wing, false, w * 0.9, AIR);
        for side in [-1.0f32, 1.0] {
            let pod: Vec<Vec2> = [vec2(1.8, 3.7), vec2(2.1, 4.0), vec2(2.1, 4.6), vec2(1.8, 4.9), vec2(-1.1, 4.9), vec2(-1.1, 3.7)].iter().map(|q| vec2(q.x, q.y * side)).collect();
            g.fill(&pod, INK);
            g.stroke(&pod, true, w * 0.8, AIR);
            for r in [3.95f32, 4.3, 4.65] {
                g.circle(vec2(1.75, r * side), 0.12, w * 0.5, AIR);
            }
        }
        g.circle(vec2(4.15, 0.0), 0.5, w * 0.7, AIR);
        g.stroke(&[vec2(4.6, 0.0), vec2(6.3, 0.0)], false, w * 0.9, AIR);
    } else {
        // skids
        for side in [-1.0f32, 1.0] {
            let skid = [vec2(-2.9, 2.45 * side), vec2(2.5, 2.45 * side), vec2(3.2, 2.15 * side)];
            g.stroke(&skid, false, w * 0.9, AIR);
            for x in [-1.6f32, 1.4] {
                g.stroke(&[vec2(x, 1.9 * side), vec2(x, 2.45 * side)], false, w * 0.6, AIR);
            }
        }
    }
    // main rotor: faint disc, blades with a little motion blur behind them, hub
    g.flat(ground, alt + 1.2 * k, heading, s);
    let rc = vec2(-0.4, 0.0);
    g.circle(rc, 6.5, w * 0.35, alpha(AIR, 0.35));
    let ra = time * 11.0;
    let blades = if gunship { 4 } else { 3 };
    for b in 0..blades {
        let a = ra + b as f32 / blades as f32 * std::f32::consts::TAU;
        let d = vec2(a.cos(), a.sin());
        let n = vec2(-d.y, d.x);
        let blade = [rc + d * 0.5 + n * 0.12, rc + d * 6.3 + n * 0.22, rc + d * 6.5, rc + d * 6.3 - n * 0.22, rc + d * 0.5 - n * 0.12];
        g.stroke(&blade, true, w * 0.7, AIR);
        g.stroke(&arc_pts(rc, 5.6, a - 0.55, a - 0.08, 8, true), false, w * 0.6, alpha(AIR, 0.35));
    }
    g.circle(rc, 0.55, w, AIR);
}

// ------------------------------------------------------------------------------------------ B-2

fn bomber_half() -> [Vec2; 6] {
    [vec2(6.0, 0.0), vec2(-1.4, 12.0), vec2(-2.7, 11.0), vec2(-0.9, 7.6), vec2(-3.0, 4.4), vec2(-1.3, 0.0)]
}

/// A B-2 flying wing: outline with its saw-tooth trailing edge, the raised spine and cockpit,
/// engine intakes and exhaust troughs, panel lines along the leading edge.
pub fn bomber(g: &mut Glow, p: Vec2, heading: f32, k: f32) {
    let s = 1.25 * k;
    let half = bomber_half();
    let mut shape: Vec<Vec2> = half.to_vec();
    for q in half[1..5].iter().rev() {
        shape.push(vec2(q.x, -q.y));
    }
    ground_shadow(&shape, p, heading, s, 0.2);
    g.flat(p, 60.0, heading, s);
    g.fill(&shape, INK);
    let w = 0.16;
    g.stroke(&shape, true, w, AIR);
    // spine and cockpit
    let spine = mirrored(&[vec2(5.0, 0.0), vec2(4.3, 1.0), vec2(3.2, 1.75), vec2(1.8, 2.2), vec2(0.4, 2.35), vec2(-0.6, 1.9), vec2(-1.1, 1.0), vec2(-1.2, 0.0)]);
    g.stroke(&spine, true, w * 0.8, AIR);
    g.stroke(&[vec2(3.9, -0.75), vec2(4.55, -0.25), vec2(4.55, 0.25), vec2(3.9, 0.75)], false, w * 0.8, AIR);
    // intakes on top and exhaust troughs behind them
    for side in [-1.0f32, 1.0] {
        g.stroke(&[vec2(2.0, 2.3 * side), vec2(2.4, 3.1 * side), vec2(1.9, 3.9 * side)], false, w * 0.8, AIR);
        g.stroke(&[vec2(0.2, 2.6 * side), vec2(-1.7, 3.2 * side)], false, w * 0.6, AIR);
        g.stroke(&[vec2(0.0, 3.6 * side), vec2(-1.8, 4.0 * side)], false, w * 0.6, AIR);
        // leading edge panel line and the elevons
        g.stroke(&[vec2(4.4, 1.1 * side), vec2(-1.2, 10.9 * side)], false, w * 0.5, alpha(AIR, 0.7));
    }
}

// ------------------------------------------------------------------------------------------ ICBM

/// An ICBM in red line art, pointing along its flight (`dir`, in 3D), with a flickering flame.
pub fn missile(g: &mut Glow, at: Vec3, dir: Vec3, time: f32, k: f32) {
    let s = 1.3 * k;
    let fwd = dir.normalize_or_zero();
    let mut side = fwd.cross(vec3(0.0, 0.0, 1.0)).normalize_or_zero();
    if side == Vec3::ZERO {
        side = vec3(1.0, 0.0, 0.0);
    }
    g.frame(at, fwd * s, side * s);
    let w = 0.2;
    let body = mirrored(&[
        vec2(4.8, 0.0),
        vec2(4.3, 0.3),
        vec2(3.5, 0.55),
        vec2(2.6, 0.62),
        vec2(-4.0, 0.62),
        vec2(-4.4, 0.45),
    ]);
    g.fill(&body, INK);
    g.stroke(&body, true, w, MISSILE);
    // stage joints and the re-entry vehicle's seam
    for x in [-1.4f32, 1.2] {
        g.stroke(&[vec2(x, -0.62), vec2(x, 0.62)], false, w * 0.6, MISSILE);
    }
    g.stroke(&[vec2(2.6, -0.62), vec2(2.6, 0.62)], false, w * 0.5, alpha(MISSILE, 0.7));
    // fins
    both(g, &[vec2(-2.6, 0.62), vec2(-3.9, 1.7), vec2(-4.5, 1.7), vec2(-4.2, 0.6)], false, w * 0.8, MISSILE);
    // flame
    let f = 0.75 + 0.25 * (time * 37.0).sin() * (time * 23.0).cos();
    let flame = mirrored(&[vec2(-4.4, 0.0), vec2(-4.5, 0.4), vec2(-5.6 - 0.6 * f, 0.5), vec2(-7.4 - 2.2 * f, 0.0)]);
    g.stroke(&flame, true, w * 0.8, rgb(255, 150, 60));
    g.stroke(&[vec2(-4.6, 0.0), vec2(-5.8 - 1.4 * f, 0.0)], false, w * 0.9, rgb(255, 230, 160));
}

/// The glowing trail an ICBM leaves behind it: `pts` from oldest to the missile.
pub fn missile_trail(g: &mut Glow, pts: &[Vec3], k: f32) {
    if pts.len() < 2 {
        return;
    }
    g.frame(Vec3::ZERO, vec3(1.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0));
    let n = pts.len();
    for i in 0..n - 1 {
        let (a, b) = (pts[i], pts[i + 1]);
        let f = (i + 1) as f32 / n as f32;
        // each piece in its own frame along the path, flat side up
        let fwd = b - a;
        let len = fwd.length();
        if len < 1e-4 {
            continue;
        }
        let mut side = fwd.cross(vec3(0.0, 0.0, 1.0)).normalize_or_zero();
        if side == Vec3::ZERO {
            side = vec3(1.0, 0.0, 0.0);
        }
        g.frame(a, fwd / len, side);
        g.stroke(&[vec2(0.0, 0.0), vec2(len, 0.0)], false, 0.5 * k * f, alpha(MISSILE, 0.55 * f));
    }
}

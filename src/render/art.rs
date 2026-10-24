//! Player structures, alien hives and the fungus on infected blocks, built from a small kit of
//! solids (bevelled boxes, sloped lofts, cylinders, domes, beams, lamps) so they sit next to the
//! city's shaded blocks instead of looking like flat coloured cubes. Every structure keeps its
//! colour as roof and trim, on a concrete or steel body.
//!
//! New structures: add a match arm in `structure` and build it from the helpers below; `generic`
//! is a decent default while a structure has no art of its own.

use super::*;

pub const CONCRETE: Color = rgb(164, 168, 174);
pub const CONCRETE_DARK: Color = rgb(118, 122, 130);
pub const STEEL: Color = rgb(78, 84, 94);
pub const STEEL_DARK: Color = rgb(46, 50, 58);
pub const GLASS: Color = rgb(96, 140, 176);

// ------------------------------------------------------------------------------------------ shapes

/// A regular polygon around `c`, starting at angle `a0`.
pub fn ngon(c: Vec2, r: f32, n: usize, a0: f32) -> Vec<Vec2> {
    (0..n).map(|k| c + vec2((a0 + k as f32 / n as f32 * std::f32::consts::TAU).cos(), (a0 + k as f32 / n as f32 * std::f32::consts::TAU).sin()) * r).collect()
}

/// A rectangle with its corners cut off, half-size `hw` x `hh`, turned by `a`.
pub fn chamfer(c: Vec2, hw: f32, hh: f32, cut: f32, a: f32) -> Vec<Vec2> {
    let k = cut.min(hw * 0.9).min(hh * 0.9);
    [vec2(hw, -hh + k), vec2(hw, hh - k), vec2(hw - k, hh), vec2(-hw + k, hh), vec2(-hw, hh - k), vec2(-hw, -hh + k), vec2(-hw + k, -hh), vec2(hw - k, -hh)].iter().map(|v| c + rot(*v, a)).collect()
}

/// A rectangle from local corners (`x0`, `y0`) to (`x1`, `y1`), turned by `a` around `c`.
pub fn rect(c: Vec2, x0: f32, y0: f32, x1: f32, y1: f32, a: f32) -> Vec<Vec2> {
    [vec2(x0, y0), vec2(x1, y0), vec2(x1, y1), vec2(x0, y1)].iter().map(|v| c + rot(*v, a)).collect()
}

/// `pts` moved toward their middle by `d` (exact for regular shapes, close enough for the rest).
pub fn shrink(pts: &[Vec2], d: f32) -> Vec<Vec2> {
    let m = pts.iter().fold(Vec2::ZERO, |a, p| a + *p) / pts.len() as f32;
    pts.iter().map(|p| *p + (m - *p).normalize_or_zero() * d).collect()
}

/// A colour lit for a face with outward normal `n` (in map space, -z up).
pub fn lit(c: Color, n: Vec3) -> Color {
    let up = (-n.z).clamp(0.0, 1.0);
    mix(shade(c, n.truncate().normalize_or_zero()), c, up)
}

/// A flat face (convex, any number of corners), wound to face away from `inside` and lit. Faces
/// that stand up get darker toward their foot, like the city's walls.
pub fn face(mb: &mut MB, pts: &[Vec3], inside: Vec3, col: Color) {
    let n = pts.len();
    if n < 3 {
        return;
    }
    let mid = pts.iter().fold(Vec3::ZERO, |a, p| a + *p) / n as f32;
    let mut nrm = (pts[1] - pts[0]).cross(pts[2] - pts[0]);
    if nrm.length_squared() < 1e-12 {
        nrm = (pts[2] - pts[0]).cross(pts[n - 1] - pts[0]);
    }
    let flip = nrm.dot(mid - inside) < 0.0;
    let nrm = if flip { -nrm } else { nrm }.normalize_or_zero();
    let c = lit(col, nrm);
    let (zlo, zhi) = pts.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(-p.z), b.max(-p.z)));
    let upright = nrm.z.abs() < 0.5 && zhi - zlo > 0.05;
    mb.room(n);
    let base = mb.v.len() as u16;
    for p in pts {
        let f = if upright { (-p.z - zlo) / (zhi - zlo) } else { 1.0 };
        mb.vert3(*p, mix(c, BLACK, 0.16 * (1.0 - f)));
    }
    for k in 1..n as u16 - 1 {
        if flip {
            mb.i.extend_from_slice(&[base, base + k + 1, base + k]);
        } else {
            mb.i.extend_from_slice(&[base, base + k, base + k + 1]);
        }
    }
}

/// A solid whose outline `lo` at height `z0` turns into `hi` at `z1` (same number of corners):
/// sloped bunker walls, tapered roofs, cones. `top` caps it.
pub fn loft(mb: &mut MB, lo: &[Vec2], hi: &[Vec2], z0: f32, z1: f32, top: Option<Color>, side: Color) {
    let n = lo.len();
    let m = lo.iter().chain(hi).fold(Vec2::ZERO, |a, p| a + *p) / (2 * n) as f32;
    let inside = p3(m, (z0 + z1) / 2.0);
    for k in 0..n {
        let j = (k + 1) % n;
        let q = [p3(lo[k], z0), p3(lo[j], z0), p3(hi[j], z1), p3(hi[k], z1)];
        if (hi[k] - hi[j]).length_squared() < 1e-8 {
            face(mb, &q[..3], inside, side);
        } else {
            face(mb, &q, inside, side);
        }
    }
    if let Some(t) = top {
        mb.poly_at(hi, z1, t);
    }
}

/// An upright solid on footprint `pts`.
pub fn block(mb: &mut MB, pts: &[Vec2], z0: f32, z1: f32, top: Color, side: Color) {
    loft(mb, pts, pts, z0, z1, Some(top), side);
}

pub fn cyl(mb: &mut MB, c: Vec2, r: f32, z0: f32, z1: f32, top: Color, side: Color) {
    let n = if r > 2.5 { 20 } else { 12 };
    block(mb, &ngon(c, r, n, 0.0), z0, z1, top, side);
}

/// A rounded dome of radius `r` and height `h` on `z0`.
pub fn dome(mb: &mut MB, c: Vec2, r: f32, z0: f32, h: f32, col: Color) {
    let n = if r > 2.5 { 20 } else { 12 };
    let rings = 4;
    let mut prev = ngon(c, r, n, 0.0);
    let mut pz = z0;
    for k in 1..=rings {
        let a = k as f32 / rings as f32 * std::f32::consts::FRAC_PI_2;
        let rr = r * a.cos();
        let z = z0 + h * a.sin();
        if k == rings {
            let tip = vec![c; n];
            loft(mb, &prev, &tip, pz, z, None, col);
        } else {
            let cur = ngon(c, rr, n, 0.0);
            loft(mb, &prev, &cur, pz, z, None, col);
            prev = cur;
        }
        pz = z;
    }
}

/// A square beam from `a` to `b` (pipes, barrels, booms), `hw` half thick, centred at height `z`.
pub fn beam(mb: &mut MB, a: Vec2, b: Vec2, hw: f32, z: f32, col: Color) {
    let d = (b - a).normalize_or_zero();
    let n = vec2(-d.y, d.x) * hw;
    block(mb, &[a - n, b - n, b + n, a + n], z - hw, z + hw, mix(col, WHITE, 0.12), col);
}

/// A slanted beam from `a` at height `za` to `b` at `zb` (legs, masts, braces).
pub fn strut(mb: &mut MB, a: Vec2, za: f32, b: Vec2, zb: f32, hw: f32, col: Color) {
    let lo = rect(a, -hw, -hw, hw, hw, 0.0);
    let hi = rect(b, -hw, -hw, hw, hw, 0.0);
    loft(mb, &lo, &hi, za, zb, Some(col), col);
}

/// A little light that sticks out of whatever it sits on.
pub fn lamp(mb: &mut MB, p: Vec2, z: f32, r: f32, col: Color) {
    block(mb, &ngon(p, r, 6, 0.0), z, z + r * 1.4, mix(col, WHITE, 0.35), col);
}

/// A rooftop air-con box with a fan grille.
pub fn vent(mb: &mut MB, p: Vec2, z: f32, s: f32, a: f32) {
    block(mb, &rect(p, -s, -s * 0.7, s, s * 0.7, a), z, z + s * 0.8, rgb(196, 200, 206), rgb(150, 155, 162));
    mb.poly_at(&ngon(p, s * 0.45, 8, 0.0), z + s * 0.8 + 0.02, STEEL_DARK);
}

/// A thin band of colour around a footprint (trim, window strips), standing just proud of it.
pub fn band(mb: &mut MB, pts: &[Vec2], z0: f32, z1: f32, col: Color) {
    let m = pts.iter().fold(Vec2::ZERO, |a, p| a + *p) / pts.len() as f32;
    let out: Vec<Vec2> = pts.iter().map(|p| *p + (*p - m).normalize_or_zero() * 0.06).collect();
    loft(mb, &out, &out, z0, z1, None, col);
}

/// A body in the structure's own colours on a concrete plinth: the default look, and the base
/// most structures build on. Returns the roof height.
#[allow(dead_code)]
pub fn generic(mb: &mut MB, d: &Def, c: Vec2, w: f32, h: f32, base: f32) -> f32 {
    let (hw, hh) = (w / 2.0, h / 2.0);
    let top = base + d.ht;
    plinth(mb, c, hw, hh, base);
    let body = chamfer(c, hw - 0.5, hh - 0.5, 0.8, 0.0);
    block(mb, &body, base + 0.5, top, mix(d.top, CONCRETE, 0.25), body_col(d));
    band(mb, &body, top - 0.7, top - 0.35, d.top);
    mb.poly_at(&shrink(&body, 0.45), top + 0.03, mix(d.top, BLACK, 0.12));
    top
}

/// The low concrete slab most structures stand on.
pub fn plinth(mb: &mut MB, c: Vec2, hw: f32, hh: f32, base: f32) {
    let lo = chamfer(c, hw, hh, 1.0, 0.0);
    let hi = chamfer(c, hw - 0.2, hh - 0.2, 0.9, 0.0);
    loft(mb, &lo, &hi, base, base + 0.5, Some(CONCRETE), CONCRETE_DARK);
}

/// Wall colour for a structure: its own colour, toned down toward concrete.
pub fn body_col(d: &Def) -> Color {
    mix(d.side, CONCRETE_DARK, 0.45)
}

/// Deterministic noise in 0..1.
pub fn hash(mut x: u32) -> f32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    (x & 0xffffff) as f32 / 16_777_216.0
}

// ------------------------------------------------------------------------------------------ structures

/// Draws structure `i` (alive, landed). `base` is the ground or roof it stands on; returns the
/// height of its top for the health bar.
pub fn structure(mb: &mut MB, g: &Game, i: usize, s: &Structure, base: f32, t: f32, labels: &mut WorldLabels) -> f32 {
    let d = def(s.t);
    let (w, h) = (s.w as f32, s.h as f32);
    let c = s.center();
    match s.t {
        S::Wall => wall(mb, d, c, w, h, base),
        S::Bunker => {
            let manned = g.groups.iter().filter(|gr| !gr.dead && gr.state == GState::Garrison && gr.sb == i as i32).map(|gr| gr.m.iter().filter(|m| m.alive).count()).sum::<usize>();
            let top = bunker(mb, d, c, w, h, base, manned > 0, t);
            if manned > 0 {
                labels.items.push((p3(c, top + 3.0), format!("{}", manned), WHITE, 12.0));
            }
            top
        }
        S::Gun => gun(mb, d, c, w, h, base, s.aim),
        S::Cannon => cannon(mb, d, c, w, h, base, s.aim),
        S::Extractor => extractor(mb, d, c, w, h, base, t),
        S::Plant => plant(mb, d, c, w, h, base, t),
        S::Silo => silo(mb, d, c, w, h, base, s.cd <= 0.0, t),
        S::Pad => {
            let top = pad(mb, c, w, h, base, s.cd <= 0.0, t);
            if s.cd > 0.0 {
                bar(mb, c + vec2(0.0, -h / 2.0 - 1.5), top + 0.5, 9.0, 1.0 - s.cd / PAD_COOLDOWN, rgb(28, 126, 214));
            }
            let n = g.civs.iter().filter(|cv| cv.st == C_AT_PAD && cv.pad == i).count();
            if n > 0 {
                labels.items.push((p3(c, top + 6.0), format!("{} waiting", n), WHITE, 12.0));
            }
            top
        }
        S::Outpost => outpost(mb, d, c, w, h, base, t),
        S::Lab => lab(mb, d, c, w, h, base, g.perks.iter().any(|k| k.research.busy.is_some()), t),
        S::Hq => {
            let top = hq(mb, d, c, w, h, base, t);
            labels.items.push((p3(c, top + 3.0), "HQ".into(), WHITE, 14.0));
            let n = g.civs.iter().filter(|cv| cv.st == C_AT_HQ && cv.p.distance(c) < 24.0).count();
            if n > 0 {
                labels.items.push((p3(c, top + 9.0), format!("{} awaiting evac", n), WHITE, 12.0));
            }
            top
        }
        S::ScrapDepot => scrap_depot(mb, d, c, w, h, base, t),
        S::AmmoDepot => ammo_depot(mb, d, c, w, h, base, t),
        S::Flame => flame(mb, d, c, w, h, base, s.aim, g.time - s.last_fire < 0.25, t),
        S::Rail => {
            let charge = if s.charge > 0.0 { 1.0 - s.charge / RAIL_CHARGE } else { 0.0 };
            rail(mb, d, c, w, h, base, s.aim, charge, g.time - s.last_fire, t)
        }
        S::Helium => helium(mb, d, c, w, h, base, t),
        S::Artillery => {
            let top = artillery(mb, d, c, w, h, base, s.aim, ((g.time - s.last_fire) / 0.5).clamp(0.0, 1.0));
            if s.target.is_none() {
                labels.items.push((p3(c, top + 3.0), "No target".into(), rgb(200, 205, 212), 11.0));
            }
            top
        }
        S::Helipad => {
            let home = g.pad_gunship(i).is_none();
            let top = helipad(mb, d, c, w, h, base, home && s.cd <= 0.0, t);
            if s.cd > 0.0 && home {
                bar(mb, c + vec2(0.0, -h / 2.0 - 1.5), top + 0.5, 9.0, 1.0 - s.cd / HELIPAD_COOLDOWN, rgb(255, 212, 59));
            }
            top
        }
        S::Rescue => {
            let top = rescue(mb, d, c, w, h, base, t);
            if s.cd > 0.0 && s.cd < RESCUE_EVERY {
                bar(mb, c + vec2(0.0, -h / 2.0 - 1.5), top + 0.5, 8.0, 1.0 - s.cd / RESCUE_EVERY, rgb(64, 192, 87));
            }
            top
        }
    }
}

/// A fenced scrap yard: heaps of salvage behind low concrete walls and a loading crane.
fn scrap_depot(mb: &mut MB, d: &Def, c: Vec2, w: f32, h: f32, base: f32, t: f32) -> f32 {
    let (hw, hh) = (w / 2.0, h / 2.0);
    plinth(mb, c, hw, hh, base);
    let z0 = base + 0.5;
    mb.poly_at(&chamfer(c, hw - 0.5, hh - 0.5, 0.8, 0.0), z0 + 0.02, mix(d.side, CONCRETE_DARK, 0.65));
    // walls on three sides, open toward +y
    for (x0, y0, x1, y1) in [(-hw + 0.4, -hh + 0.4, hw - 0.4, -hh + 0.8), (-hw + 0.4, -hh + 0.4, -hw + 0.8, hh - 0.4), (hw - 0.8, -hh + 0.4, hw - 0.4, hh - 0.4)] {
        block(mb, &rect(c, x0, y0, x1, y1, 0.0), z0, z0 + 1.1, CONCRETE, CONCRETE_DARK);
        band(mb, &rect(c, x0, y0, x1, y1, 0.0), z0 + 0.75, z0 + 0.95, d.top);
    }
    // salvage heaps: rusty cones with a few girders sticking out
    let rust = rgb(146, 104, 72);
    for (k, (x, y, r, ht)) in [(-1.2f32, -1.0f32, 1.5f32, 1.6f32), (1.3, -0.8, 1.2, 1.2), (0.0, 1.0, 1.1, 1.0)].iter().enumerate() {
        let p = c + vec2(*x, *y);
        let lo = ngon(p, *r, 9, k as f32);
        let hi = ngon(p + vec2(0.1, -0.1), r * 0.25, 9, k as f32);
        loft(mb, &lo, &hi, z0, z0 + ht, Some(mix(rust, WHITE, 0.1)), mix(rust, d.side, 0.3));
        strut(mb, p, z0 + ht * 0.4, p + rot(vec2(0.9, 0.0), k as f32 * 2.1), z0 + ht + 0.4, 0.07, STEEL);
    }
    // crane
    let m = c + vec2(hw - 1.4, hh - 1.4);
    strut(mb, m, z0, m, z0 + 3.4, 0.14, rgb(250, 204, 40));
    let arm = rot(vec2(-3.2, 0.0), 0.6 + (t * 0.3).sin() * 0.5);
    beam(mb, m, m + arm, 0.12, z0 + 3.3, rgb(250, 204, 40));
    strut(mb, m + arm, z0 + 1.6, m + arm, z0 + 3.3, 0.03, STEEL_DARK);
    block(mb, &rect(m + arm, -0.35, -0.35, 0.35, 0.35, 0.0), z0 + 1.3, z0 + 1.6, STEEL, STEEL_DARK);
    z0 + 3.4
}

/// An earth-covered ammunition bunker with blast doors, a hazard band and stacked crates.
fn ammo_depot(mb: &mut MB, d: &Def, c: Vec2, w: f32, h: f32, base: f32, t: f32) -> f32 {
    let (hw, hh) = (w / 2.0, h / 2.0);
    plinth(mb, c, hw, hh, base);
    let z0 = base + 0.5;
    let lo = chamfer(c + vec2(0.0, -0.6), hw - 0.4, hh - 1.2, 1.2, 0.0);
    let hi = chamfer(c + vec2(0.0, -0.6), hw - 1.6, hh - 2.2, 0.8, 0.0);
    let top = base + d.ht;
    let earth = rgb(112, 128, 84);
    loft(mb, &lo, &hi, z0, top, Some(mix(earth, WHITE, 0.08)), mix(earth, BLACK, 0.15));
    // concrete front with blast doors
    let front = rect(c, -hw + 1.2, hh - 2.0, hw - 1.2, hh - 1.6, 0.0);
    block(mb, &front, z0, top - 0.3, CONCRETE, CONCRETE_DARK);
    band(mb, &front, top - 0.9, top - 0.6, d.top);
    for k in [-1.0f32, 1.0] {
        block(mb, &rect(c + vec2(k * 0.85, hh - 1.6), -0.75, 0.0, 0.75, 0.1, 0.0), z0, z0 + 1.4, STEEL, STEEL_DARK);
    }
    vent(mb, c + vec2(0.0, -0.8), top, 0.45, 0.0);
    // crates on the apron
    for (k, x) in [-hw + 1.0, -hw + 1.8, hw - 1.4].iter().enumerate() {
        let p = c + vec2(*x, hh - 0.9);
        let z = z0 + if k == 1 { 0.0 } else { 0.0 };
        block(mb, &rect(p, -0.35, -0.3, 0.35, 0.3, 0.1 * k as f32), z, z + 0.55, rgb(120, 130, 80), rgb(84, 92, 56));
    }
    let blink = (t * 1.1).fract() < 0.3;
    lamp(mb, c + vec2(hw - 1.4, hh - 1.8), top - 0.3, 0.16, if blink { rgb(255, 170, 50) } else { rgb(110, 70, 20) });
    top
}

/// A squat armoured bunker with sloped walls, fuel tanks at the back and a flame nozzle that
/// swings to the target; the pilot light flares while it fires.
#[allow(clippy::too_many_arguments)]
fn flame(mb: &mut MB, d: &Def, c: Vec2, w: f32, h: f32, base: f32, aim: f32, firing: bool, t: f32) -> f32 {
    let (hw, hh) = (w / 2.0, h / 2.0);
    plinth(mb, c, hw, hh, base);
    let lo = chamfer(c, hw - 0.3, hh - 0.3, 1.4, 0.0);
    let hi = chamfer(c, hw - 1.1, hh - 1.1, 1.0, 0.0);
    let z1 = base + 1.7;
    loft(mb, &lo, &hi, base + 0.5, z1, Some(mix(CONCRETE, d.top, 0.1)), body_col(d));
    band(mb, &hi, z1 - 0.45, z1 - 0.22, d.top);
    // hazard stripes on the roof edge
    for k in 0..4 {
        let a = k as f32 * std::f32::consts::FRAC_PI_2;
        mb.poly_at(&rect(c, hw - 1.6, -0.25, hw - 1.25, 0.25, a), z1 + 0.02, rgb(255, 212, 59));
    }
    let fwd = vec2(aim.cos(), aim.sin());
    let side = vec2(-fwd.y, fwd.x);
    // fuel tanks ride on the turret's back
    cyl(mb, c, 1.5, z1, z1 + 0.3, STEEL, STEEL_DARK);
    let zh = z1 + 0.3;
    for k in [-0.55f32, 0.55] {
        let p = c - fwd * 0.6 + side * k;
        cyl(mb, p, 0.42, zh, zh + 1.0, mix(d.top, WHITE, 0.15), d.side);
        dome(mb, p, 0.42, zh + 1.0, 0.25, d.top);
    }
    let head = chamfer(c + fwd * 0.3, 0.9, 0.75, 0.35, aim);
    loft(mb, &head, &shrink(&head, 0.2), zh, zh + 0.7, Some(STEEL), STEEL_DARK);
    // nozzle
    let n0 = c + fwd * 0.9;
    beam(mb, n0, n0 + fwd * 2.3, 0.17, zh + 0.4, STEEL_DARK);
    beam(mb, n0 + fwd * 2.0, n0 + fwd * 2.6, 0.26, zh + 0.4, STEEL);
    let tip = n0 + fwd * 2.75;
    let flick = 0.5 + 0.5 * (t * 23.0).sin().abs();
    let pilot = if firing { mix(rgb(255, 150, 40), rgb(255, 240, 160), flick) } else { mix(rgb(70, 140, 255), rgb(170, 210, 255), flick) };
    lamp(mb, tip, zh + 0.25, if firing { 0.4 } else { 0.16 }, pilot);
    zh + 1.25
}

/// A round railgun mount: a turning housing with two long rails, field coils across them and a
/// core that glows brighter as the slug charges.
#[allow(clippy::too_many_arguments)]
fn rail(mb: &mut MB, d: &Def, c: Vec2, w: f32, h: f32, base: f32, aim: f32, charge: f32, since: f32, t: f32) -> f32 {
    let (hw, hh) = (w / 2.0, h / 2.0);
    plinth(mb, c, hw, hh, base);
    let r = hw.min(hh) - 0.4;
    let z1 = base + 1.5;
    loft(mb, &ngon(c, r, 18, 0.0), &ngon(c, r - 0.6, 18, 0.0), base + 0.5, z1, Some(mix(STEEL, d.top, 0.15)), body_col(d));
    band(mb, &ngon(c, r - 0.6, 18, 0.0), z1 - 0.4, z1 - 0.18, d.top);
    cyl(mb, c, 1.9, z1, z1 + 0.35, STEEL, STEEL_DARK);
    let zh = z1 + 0.35;
    let fwd = vec2(aim.cos(), aim.sin());
    let side = vec2(-fwd.y, fwd.x);
    // recoil after a shot
    let kick = (1.0 - (since / 0.6).clamp(0.0, 1.0)) * 0.8;
    let back = c - fwd * (0.7 + kick);
    let body = chamfer(back, 1.7, 1.3, 0.6, aim);
    loft(mb, &body, &shrink(&body, 0.25), zh, zh + 1.2, Some(d.top), mix(d.side, STEEL, 0.4));
    mb.poly_at(&rect(back, -1.0, -0.35, 0.6, 0.35, aim), zh + 1.22, STEEL_DARK);
    let zr = zh + 0.7;
    let (r0, r1) = (c + fwd * (0.4 - kick), c + fwd * (6.2 - kick));
    for k in [-0.45f32, 0.45] {
        beam(mb, r0 + side * k, r1 + side * k, 0.17, zr, rgb(206, 212, 218));
    }
    let core = mix(rgb(30, 70, 120), rgb(210, 248, 255), charge.max(0.15));
    beam(mb, r0, r1 - fwd * 0.6, 0.09, zr, core);
    for k in 0..3 {
        let p = r0 + fwd * (1.2 + k as f32 * 1.6);
        block(mb, &rect(p, -0.18, -0.72, 0.18, 0.72, aim), zr - 0.35, zr + 0.35, d.top, d.side);
    }
    // capacitor lights fill up with the charge
    for k in 0..4 {
        let on = charge > k as f32 / 4.0 || (charge == 0.0 && ((t * 0.8) as i32 % 4) == k);
        lamp(mb, back + rot(vec2(-1.2 + k as f32 * 0.5, 1.15), aim), zh + 0.5, 0.15, if on { rgb(150, 230, 255) } else { STEEL_DARK });
    }
    zh + 1.3
}

/// A frosted cryo tank in cooling rings on a steel skid, with a spray head turning on top.
fn helium(mb: &mut MB, d: &Def, c: Vec2, w: f32, h: f32, base: f32, t: f32) -> f32 {
    let (hw, hh) = (w / 2.0, h / 2.0);
    plinth(mb, c, hw, hh, base);
    let z0 = base + 0.5;
    block(mb, &chamfer(c, hw - 0.6, hh - 0.6, 0.8, 0.0), z0, z0 + 0.35, STEEL, STEEL_DARK);
    let frost = rgb(232, 240, 248);
    let r = hw.min(hh) - 1.0;
    let z1 = z0 + 0.35 + 3.0;
    cyl(mb, c, r, z0 + 0.35, z1, frost, mix(d.top, frost, 0.5));
    for k in 0..3 {
        let z = z0 + 0.9 + k as f32 * 0.9;
        band(mb, &ngon(c, r, 20, 0.0), z, z + 0.2, d.side);
    }
    dome(mb, c, r, z1, 0.9, frost);
    // feed pipes down the side
    for k in 0..3 {
        let a = k as f32 * 2.094 + 0.5;
        let o = vec2(a.cos(), a.sin());
        strut(mb, c + o * (r + 0.15), z0 + 0.4, c + o * (r + 0.15), z1 + 0.2, 0.09, STEEL);
    }
    let zt = z1 + 0.9;
    cyl(mb, c, 0.35, zt - 0.1, zt + 0.35, STEEL, STEEL_DARK);
    for k in 0..3 {
        let a = t * 1.6 + k as f32 * 2.094;
        let q = rot(vec2(1.1, 0.0), a);
        beam(mb, c, c + q, 0.08, zt + 0.25, STEEL);
        lamp(mb, c + q, zt + 0.15, 0.13, rgb(180, 230, 255));
    }
    zt + 0.4
}

/// A howitzer in a sandbagged ring: carriage and gun shield on a turntable, the barrel raised
/// toward the target and kicking back on each shot.
fn artillery(mb: &mut MB, d: &Def, c: Vec2, w: f32, h: f32, base: f32, aim: f32, recoil: f32) -> f32 {
    let (hw, hh) = (w / 2.0, h / 2.0);
    plinth(mb, c, hw, hh, base);
    let z0 = base + 0.5;
    let ring = chamfer(c, hw - 0.3, hh - 0.3, 2.2, 0.0);
    let sand = rgb(176, 156, 116);
    loft(mb, &ring, &shrink(&ring, 0.5), z0, z0 + 0.9, Some(mix(sand, WHITE, 0.1)), mix(sand, BLACK, 0.2));
    mb.poly_at(&shrink(&ring, 1.1), z0 + 0.03, CONCRETE_DARK);
    // shell crates by the wall
    for (x, y) in [(-hw + 1.6, hh - 1.7), (-hw + 2.6, hh - 1.7)] {
        block(mb, &rect(c, x - 0.4, y - 0.3, x + 0.4, y + 0.3, 0.0), z0, z0 + 0.5, rgb(120, 130, 80), rgb(84, 92, 56));
    }
    cyl(mb, c, 2.4, z0, z0 + 0.4, STEEL, STEEL_DARK);
    let zh = z0 + 0.4;
    let fwd = vec2(aim.cos(), aim.sin());
    let carriage = chamfer(c - fwd * 0.4, 1.9, 1.5, 0.5, aim);
    loft(mb, &carriage, &shrink(&carriage, 0.3), zh, zh + 0.9, Some(d.top), mix(d.side, STEEL, 0.3));
    // trails behind
    for k in [-0.9f32, 0.9] {
        let a = c - fwd * 1.5 + vec2(-fwd.y, fwd.x) * k;
        beam(mb, a, a - fwd * 2.2 + vec2(-fwd.y, fwd.x) * k * 0.6, 0.18, zh + 0.25, STEEL_DARK);
    }
    block(mb, &rect(c + fwd * 0.8, -0.18, -1.8, 0.18, 1.8, aim), zh + 0.5, zh + 1.9, mix(d.top, WHITE, 0.1), d.side);
    let kick = (1.0 - recoil) * 1.1;
    let b0 = c + fwd * (0.2 - kick);
    let b1 = c + fwd * (6.8 - kick);
    strut(mb, b0, zh + 1.1, b1, zh + 2.9, 0.3, STEEL_DARK);
    strut(mb, b0 - fwd * 0.6, zh + 0.95, b0 + fwd * 1.0, zh + 1.35, 0.5, STEEL);
    strut(mb, b1 - fwd * 0.5, zh + 2.75, b1 + fwd * 0.2, zh + 2.95, 0.4, STEEL);
    zh + 3.0
}

/// The gunship's own pad: a dark steel deck with a yellow ring and H, rearming crates at the
/// corners and edge lights that turn green when the gunship is ready.
#[allow(clippy::too_many_arguments)]
fn helipad(mb: &mut MB, d: &Def, c: Vec2, w: f32, h: f32, base: f32, ready: bool, t: f32) -> f32 {
    let (hw, hh) = (w / 2.0, h / 2.0);
    let deck = chamfer(c, hw, hh, 2.2, 0.0);
    loft(mb, &deck, &shrink(&deck, 0.3), base, base + d.ht, Some(d.top), CONCRETE_DARK);
    let z = base + d.ht;
    let r = hw.min(hh) - 1.4;
    let yellow = rgb(250, 204, 40);
    ring_flat(mb, c, r, 0.4, z + 0.02, yellow);
    for (x0, y0, x1, y1) in [(-1.9, -2.6, -1.1, 2.6), (1.1, -2.6, 1.9, 2.6), (-1.1, -0.4, 1.1, 0.4)] {
        mb.poly_at(&rect(c, x0, y0, x1, y1, 0.0), z + 0.04, yellow);
    }
    // rearming crates and a fuel drum in the corners
    for k in 0..4 {
        let a = k as f32 * std::f32::consts::FRAC_PI_2 + std::f32::consts::FRAC_PI_4;
        let p = c + vec2(a.cos(), a.sin()) * (r + 1.4);
        if k % 2 == 0 {
            block(mb, &rect(p, -0.45, -0.35, 0.45, 0.35, a), z, z + 0.55, rgb(120, 130, 80), rgb(84, 92, 56));
        } else {
            cyl(mb, p, 0.35, z, z + 0.7, rgb(200, 60, 50), rgb(150, 40, 34));
        }
    }
    let col = if ready { rgb(80, 230, 110) } else { rgb(255, 170, 50) };
    for k in 0..12 {
        let a = k as f32 / 12.0 * std::f32::consts::TAU;
        let on = ((t * 1.5 - k as f32 / 12.0).fract()) < 0.3;
        lamp(mb, c + vec2(a.cos(), a.sin()) * (r + 0.55), z, 0.16, if on { col } else { mix(col, BLACK, 0.55) });
    }
    z
}

/// A white field station with a green cross on the roof, a vehicle bay, a radio mast with a
/// blinking light and an ambulance parked by the door.
fn rescue(mb: &mut MB, d: &Def, c: Vec2, w: f32, h: f32, base: f32, t: f32) -> f32 {
    let (hw, hh) = (w / 2.0, h / 2.0);
    plinth(mb, c, hw, hh, base);
    let z0 = base + 0.5;
    let white = rgb(238, 241, 244);
    let wall_c = rgb(196, 202, 210);
    let body = chamfer(c + vec2(0.0, -0.8), hw - 0.6, hh - 1.4, 0.7, 0.0);
    let top = base + d.ht;
    block(mb, &body, z0, top, white, wall_c);
    band(mb, &body, top - 0.75, top - 0.45, d.top);
    // vehicle bay doors on the front
    for k in [-1.6f32, 1.6] {
        let p = c + vec2(k, hh - 2.35);
        block(mb, &rect(p, -1.1, -0.05, 1.1, 0.12, 0.0), z0, z0 + 1.6, STEEL, STEEL_DARK);
    }
    // the cross
    let cc = c + vec2(0.0, -0.8);
    mb.poly_at(&rect(cc, -2.2, -2.2, 2.2, 2.2, 0.0), top + 0.02, white);
    mb.poly_at(&rect(cc, -0.6, -1.8, 0.6, 1.8, 0.0), top + 0.04, d.top);
    mb.poly_at(&rect(cc, -1.8, -0.6, 1.8, 0.6, 0.0), top + 0.04, d.top);
    vent(mb, c + vec2(-hw + 1.6, -hh + 1.6), top, 0.5, 0.0);
    // radio mast
    let m = c + vec2(hw - 1.5, -hh + 1.5);
    strut(mb, m, top, m, top + 3.4, 0.07, STEEL);
    let blink = (t * 1.4).fract() < 0.4;
    lamp(mb, m, top + 3.4, 0.18, if blink { rgb(255, 70, 60) } else { rgb(110, 30, 30) });
    // ambulance on the apron
    let a = c + vec2(-hw + 2.2, hh - 0.9);
    block(mb, &rect(a, -1.3, -0.5, 1.3, 0.5, 0.0), z0, z0 + 0.9, white, wall_c);
    band(mb, &rect(a, -1.3, -0.5, 1.3, 0.5, 0.0), z0 + 0.4, z0 + 0.55, d.top);
    lamp(mb, a + vec2(0.6, 0.0), z0 + 0.9, 0.12, if blink { rgb(80, 160, 255) } else { rgb(255, 70, 60) });
    top
}

/// Jersey-barrier style concrete: sloped sides, a capping strip and hazard paint on the ends.
fn wall(mb: &mut MB, d: &Def, c: Vec2, w: f32, h: f32, base: f32) -> f32 {
    let (hw, hh) = (w / 2.0, h / 2.0);
    let top = base + d.ht;
    let lo = rect(c, -hw, -hh, hw, hh, 0.0);
    let mid = rect(c, -hw + 0.15, -hh + 0.15, hw - 0.15, hh - 0.15, 0.0);
    let hi = rect(c, -hw + 0.45, -hh + 0.45, hw - 0.45, hh - 0.45, 0.0);
    loft(mb, &lo, &mid, base, base + 0.35, None, d.side);
    loft(mb, &mid, &hi, base + 0.35, top, Some(d.top), mix(d.side, d.top, 0.3));
    mb.poly_at(&rect(c, -hw + 0.45, -0.18, hw - 0.45, 0.18, 0.0), top + 0.02, mix(d.top, WHITE, 0.2));
    top
}

/// A squat concrete bunker with sloped walls, a dark firing slit all the way round, and a
/// steel roof hatch that doubles as the seat for a mounted turret. Lit slits when manned.
#[allow(clippy::too_many_arguments)]
fn bunker(mb: &mut MB, d: &Def, c: Vec2, w: f32, h: f32, base: f32, manned: bool, t: f32) -> f32 {
    let (hw, hh) = (w / 2.0, h / 2.0);
    plinth(mb, c, hw, hh, base);
    let lo = chamfer(c, hw - 0.2, hh - 0.2, 1.3, 0.0);
    let mid = chamfer(c, hw - 0.55, hh - 0.55, 1.15, 0.0);
    let hi = chamfer(c, hw - 0.95, hh - 0.95, 1.0, 0.0);
    let z1 = base + d.ht;
    let wall = mix(CONCRETE, d.side, 0.35);
    let zs = base + 1.3;
    loft(mb, &lo, &mid, base + 0.5, zs, None, wall);
    // the firing slit, glowing faintly with muzzle flashes when troops are inside
    let flick = 0.5 + 0.5 * (t * 9.0).sin().abs();
    let slit = if manned { mix(rgb(40, 34, 20), rgb(255, 200, 90), 0.35 * flick) } else { rgb(28, 30, 32) };
    loft(mb, &shrink(&mid, 0.15), &shrink(&mid, 0.15), zs, zs + 0.35, None, slit);
    loft(mb, &mid, &hi, zs + 0.35, z1, Some(mix(CONCRETE, d.top, 0.5)), wall);
    band(mb, &hi, z1 - 0.3, z1 - 0.12, d.top);
    cyl(mb, c, 1.1, z1, z1 + 0.18, STEEL, STEEL_DARK);
    z1
}

/// A sandbagged concrete emplacement with a turning head and twin barrels.
fn gun(mb: &mut MB, d: &Def, c: Vec2, w: f32, h: f32, base: f32, aim: f32) -> f32 {
    let (hw, hh) = (w / 2.0, h / 2.0);
    plinth(mb, c, hw, hh, base);
    let lo = chamfer(c, hw - 0.35, hh - 0.35, 1.1, 0.0);
    let hi = chamfer(c, hw - 0.8, hh - 0.8, 0.9, 0.0);
    let z1 = base + 1.9;
    loft(mb, &lo, &hi, base + 0.5, z1, Some(mix(CONCRETE, d.top, 0.15)), body_col(d));
    band(mb, &hi, z1 - 0.45, z1 - 0.2, d.top);
    // turret ring and head, turning with the aim
    cyl(mb, c, 1.25, z1, z1 + 0.35, STEEL, STEEL_DARK);
    let zh = z1 + 0.35;
    let head = chamfer(c + rot(vec2(-0.2, 0.0), aim), 1.25, 1.05, 0.55, aim);
    let head_top = chamfer(c + rot(vec2(-0.25, 0.0), aim), 1.05, 0.85, 0.45, aim);
    loft(mb, &head, &head_top, zh, zh + 0.95, Some(d.top), mix(d.side, d.top, 0.4));
    let fwd = vec2(aim.cos(), aim.sin());
    let side = vec2(-fwd.y, fwd.x);
    for k in [-0.35f32, 0.35] {
        let a = c + fwd * 0.7 + side * k;
        beam(mb, a, a + fwd * 2.4, 0.13, zh + 0.5, STEEL_DARK);
        beam(mb, a + fwd * 2.2, a + fwd * 2.6, 0.19, zh + 0.5, STEEL);
    }
    mb.poly_at(&rect(c + rot(vec2(-0.6, 0.0), aim), -0.25, -0.5, 0.25, 0.5, aim), zh + 0.97, mix(d.top, BLACK, 0.3));
    zh + 1.0
}

/// A heavy octagonal casemate with a domed turret and a long barrel with a muzzle brake.
fn cannon(mb: &mut MB, d: &Def, c: Vec2, w: f32, h: f32, base: f32, aim: f32) -> f32 {
    let (hw, hh) = (w / 2.0, h / 2.0);
    plinth(mb, c, hw, hh, base);
    let lo = chamfer(c, hw - 0.3, hh - 0.3, 1.8, 0.0);
    let hi = chamfer(c, hw - 1.1, hh - 1.1, 1.4, 0.0);
    let z1 = base + 2.0;
    loft(mb, &lo, &hi, base + 0.5, z1, Some(mix(CONCRETE, d.top, 0.12)), body_col(d));
    band(mb, &hi, z1 - 0.5, z1 - 0.22, d.top);
    for k in 0..4 {
        let a = k as f32 * std::f32::consts::FRAC_PI_2 + std::f32::consts::FRAC_PI_4;
        lamp(mb, c + vec2(a.cos(), a.sin()) * (hw - 1.3), z1, 0.22, mix(d.top, WHITE, 0.2));
    }
    cyl(mb, c, 1.9, z1, z1 + 0.3, STEEL, STEEL_DARK);
    let zh = z1 + 0.3;
    let fwd = vec2(aim.cos(), aim.sin());
    let back = c - fwd * 0.35;
    let head = chamfer(back, 1.9, 1.6, 0.8, aim);
    let head_hi = chamfer(back - fwd * 0.2, 1.4, 1.15, 0.6, aim);
    loft(mb, &head, &head_hi, zh, zh + 1.1, Some(d.top), mix(d.side, d.top, 0.35));
    mb.poly_at(&chamfer(back - fwd * 0.6, 0.55, 0.55, 0.2, aim), zh + 1.13, STEEL_DARK);
    let m = c + fwd * 1.2;
    beam(mb, m, m + fwd * 4.2, 0.28, zh + 0.85, STEEL_DARK);
    beam(mb, m, m + fwd * 0.9, 0.4, zh + 0.85, STEEL);
    beam(mb, m + fwd * 3.9, m + fwd * 4.7, 0.38, zh + 0.85, STEEL);
    zh + 1.2
}

/// A drill rig on a teal pad: a lattice tower, a turning cutter head and two storage tanks.
fn extractor(mb: &mut MB, d: &Def, c: Vec2, w: f32, h: f32, base: f32, t: f32) -> f32 {
    let (hw, hh) = (w / 2.0, h / 2.0);
    plinth(mb, c, hw, hh, base);
    let z0 = base + 0.5;
    mb.poly_at(&chamfer(c, hw - 0.6, hh - 0.6, 0.8, 0.0), z0 + 0.02, mix(d.side, CONCRETE_DARK, 0.6));
    // two tanks on one side
    for k in [-1.5f32, 1.5] {
        let p = c + vec2(-hw + 1.6, k);
        cyl(mb, p, 1.15, z0, z0 + 2.0, mix(d.top, WHITE, 0.3), mix(d.side, d.top, 0.5));
        dome(mb, p, 1.15, z0 + 2.0, 0.45, mix(d.top, WHITE, 0.3));
        band(mb, &ngon(p, 1.15, 12, 0.0), z0 + 1.2, z0 + 1.45, STEEL);
    }
    // the rig
    let rc = c + vec2(0.9, 0.0);
    let zt = z0 + 4.2;
    for k in 0..4 {
        let a = std::f32::consts::FRAC_PI_4 + k as f32 * std::f32::consts::FRAC_PI_2;
        let foot = rc + vec2(a.cos(), a.sin()) * 1.9;
        let head = rc + vec2(a.cos(), a.sin()) * 0.7;
        strut(mb, foot, z0, head, zt, 0.14, d.side);
    }
    for z in [z0 + 1.4, z0 + 2.8] {
        let r = 1.9 - (z - z0) / 4.2 * 1.2;
        let sq = rect(rc, -r * 0.71, -r * 0.71, r * 0.71, r * 0.71, 0.0);
        for k in 0..4 {
            beam(mb, sq[k], sq[(k + 1) % 4], 0.08, z, d.top);
        }
    }
    block(mb, &rect(rc, -0.75, -0.75, 0.75, 0.75, 0.0), zt, zt + 0.6, d.top, d.side);
    // the cutter head turning at the foot of the rig
    let a = t * 2.0;
    cyl(mb, rc, 0.8, z0, z0 + 0.6, STEEL, STEEL_DARK);
    for k in 0..3 {
        let q = rot(vec2(1.0, 0.0), a + k as f32 * 2.094);
        beam(mb, rc, rc + q * 1.7, 0.18, z0 + 0.4, d.top);
    }
    lamp(mb, rc, zt + 0.6, 0.2, if (t * 2.0).fract() < 0.5 { rgb(255, 80, 60) } else { rgb(120, 30, 30) });
    zt + 0.8
}

/// A small factory: brick-coloured hall with a saw-tooth glass roof, a chimney and a loading bay.
fn plant(mb: &mut MB, d: &Def, c: Vec2, w: f32, h: f32, base: f32, t: f32) -> f32 {
    let (hw, hh) = (w / 2.0, h / 2.0);
    plinth(mb, c, hw, hh, base);
    let z0 = base + 0.5;
    let z1 = z0 + 2.0;
    let (x0, x1, y0, y1) = (c.x - hw + 0.6, c.x + hw - 1.6, c.y - hh + 0.6, c.y + hh - 0.6);
    block(mb, &[vec2(x0, y0), vec2(x1, y0), vec2(x1, y1), vec2(x0, y1)], z0, z1, mix(d.top, BLACK, 0.2), mix(d.side, CONCRETE_DARK, 0.25));
    band(mb, &[vec2(x0, y0), vec2(x1, y0), vec2(x1, y1), vec2(x0, y1)], z0 + 0.9, z0 + 1.3, mix(GLASS, BLACK, 0.2));
    // saw-tooth roof: each tooth slopes up to a glazed north face
    let teeth = 3;
    let step = (y1 - y0) / teeth as f32;
    for k in 0..teeth {
        let ya = y0 + k as f32 * step;
        let yb = ya + step;
        let zp = z1 + 1.1;
        let inside = p3(vec2((x0 + x1) / 2.0, (ya + yb) / 2.0), z1 + 0.3);
        face(mb, &[p3(vec2(x0, yb), z1), p3(vec2(x1, yb), z1), p3(vec2(x1, ya), zp), p3(vec2(x0, ya), zp)], inside, d.top);
        face(mb, &[p3(vec2(x0, ya), z1), p3(vec2(x1, ya), z1), p3(vec2(x1, ya), zp), p3(vec2(x0, ya), zp)], inside, GLASS);
        face(mb, &[p3(vec2(x0, ya), z1), p3(vec2(x0, yb), z1), p3(vec2(x0, ya), zp)], inside, d.side);
        face(mb, &[p3(vec2(x1, ya), z1), p3(vec2(x1, yb), z1), p3(vec2(x1, ya), zp)], inside, d.side);
    }
    // the chimney and its smoke-stained lip
    let ch = vec2(c.x + hw - 0.9, c.y - hh + 1.2);
    cyl(mb, ch, 0.55, z0, z1 + 3.2, STEEL_DARK, mix(d.side, CONCRETE_DARK, 0.4));
    band(mb, &ngon(ch, 0.55, 12, 0.0), z1 + 2.4, z1 + 2.7, rgb(230, 230, 230));
    let puff = (t * 0.7).fract();
    lamp(mb, ch + vec2(puff * 1.5, -puff), z1 + 3.4 + puff * 2.0, 0.3 + puff * 0.4, Color::new(0.55, 0.55, 0.58, 1.0));
    // crates of shells waiting by the bay
    for k in 0..3 {
        let p = vec2(c.x + hw - 0.9, c.y + hh - 1.0 - k as f32 * 0.9);
        block(mb, &rect(p, -0.35, -0.35, 0.35, 0.35, 0.0), z0, z0 + 0.55, rgb(120, 140, 80), rgb(88, 104, 58));
    }
    z1 + 1.2
}

/// A concrete launch pad: hazard-striped collar, sliding hatch doors that open when a missile is
/// ready, a control hut and status lamps.
#[allow(clippy::too_many_arguments)]
fn silo(mb: &mut MB, d: &Def, c: Vec2, w: f32, h: f32, base: f32, ready: bool, t: f32) -> f32 {
    let (hw, hh) = (w / 2.0, h / 2.0);
    plinth(mb, c, hw, hh, base);
    let z0 = base + 0.5;
    let pad = chamfer(c, hw - 0.4, hh - 0.4, 1.4, 0.0);
    block(mb, &pad, z0, z0 + 0.4, CONCRETE, CONCRETE_DARK);
    let z1 = z0 + 0.4;
    let hc = c + vec2(-0.6, 0.6);
    // collar: yellow and black segments
    let ring = ngon(hc, 2.9, 16, 0.0);
    let inside = p3(hc, z1 + 0.4);
    for k in 0..16 {
        let (a, b) = (ring[k], ring[(k + 1) % 16]);
        let col = if k % 2 == 0 { rgb(250, 200, 40) } else { rgb(40, 40, 44) };
        face(mb, &[p3(a, z1), p3(b, z1), p3(b, z1 + 0.8), p3(a, z1 + 0.8)], inside, col);
    }
    mb.poly_at(&ring, z1 + 0.8, CONCRETE);
    mb.poly_at(&ngon(hc, 2.3, 16, 0.0), z1 + 0.82, rgb(20, 22, 26));
    // the doors slide apart when it's ready
    let open = if ready { 1.0 } else { 0.0 };
    for sgn in [-1.0f32, 1.0] {
        let off = vec2(sgn * (1.15 + open * 1.9), 0.0);
        block(mb, &rect(hc + off, -1.15, -2.2, 1.15, 2.2, 0.0), z1 + 0.82, z1 + 1.05, d.top, d.side);
        mb.poly_at(&rect(hc + off, -0.15, -1.9, 0.15, 1.9, 0.0), z1 + 1.07, rgb(200, 60, 50));
    }
    if ready {
        dome(mb, hc, 0.9, z1 + 0.82, 1.0, rgb(220, 50, 40));
    }
    // control hut and antenna
    let hut = c + vec2(hw - 1.7, -hh + 1.7);
    block(mb, &chamfer(hut, 1.1, 1.0, 0.3, 0.0), z1, z1 + 1.5, rgb(236, 238, 241), d.side);
    band(mb, &chamfer(hut, 1.1, 1.0, 0.3, 0.0), z1 + 0.8, z1 + 1.1, GLASS);
    strut(mb, hut, z1 + 1.5, hut, z1 + 3.6, 0.06, STEEL);
    let blink = (t * 1.5).fract() < 0.5;
    lamp(mb, hut, z1 + 3.6, 0.18, if blink { rgb(255, 70, 60) } else { rgb(110, 30, 30) });
    let st = if ready { rgb(64, 220, 100) } else { rgb(230, 50, 50) };
    for p in [vec2(-hw + 1.3, -hh + 1.3), vec2(-hw + 1.3, hh - 1.3), vec2(hw - 1.3, hh - 1.3)] {
        lamp(mb, c + p, z1, 0.3, st);
    }
    z1 + 2.0
}

/// A helipad: raised deck, yellow ring, a big H and blinking edge lights. Green lights mean it
/// can fly the next group out.
fn pad(mb: &mut MB, c: Vec2, w: f32, h: f32, base: f32, ready: bool, t: f32) -> f32 {
    let (hw, hh) = (w / 2.0, h / 2.0);
    let deck = chamfer(c, hw, hh, 1.6, 0.0);
    loft(mb, &deck, &shrink(&deck, 0.25), base, base + 0.6, Some(rgb(70, 76, 84)), CONCRETE_DARK);
    let z = base + 0.6;
    let r = hw.min(hh) - 1.0;
    ring_flat(mb, c, r, 0.45, z + 0.02, rgb(250, 204, 40));
    mb.poly_at(&ngon(c, r - 0.25, 24, 0.0), z + 0.02, rgb(84, 90, 98));
    for (x0, y0, x1, y1) in [(-1.9, -2.6, -1.1, 2.6), (1.1, -2.6, 1.9, 2.6), (-1.1, -0.4, 1.1, 0.4)] {
        mb.poly_at(&rect(c, x0, y0, x1, y1, 0.0), z + 0.04, rgb(245, 246, 248));
    }
    let col = if ready { rgb(80, 230, 110) } else { rgb(255, 170, 50) };
    for k in 0..8 {
        let a = k as f32 / 8.0 * std::f32::consts::TAU + 0.39;
        let on = ((t * 1.2 - k as f32 / 8.0).fract()) < 0.35;
        lamp(mb, c + vec2(a.cos(), a.sin()) * (r + 0.6), z, 0.2, if on { col } else { mix(col, BLACK, 0.55) });
    }
    // windsock
    let ws = c + vec2(hw - 1.0, -hh + 1.0);
    strut(mb, ws, z, ws, z + 2.2, 0.05, STEEL);
    let sway = (t * 1.3).sin() * 0.3;
    beam(mb, ws, ws + rot(vec2(1.0, 0.0), 0.8 + sway), 0.15, z + 2.0, rgb(255, 120, 40));
    z
}

/// A flat ring lying at height `z`.
pub fn ring_flat(mb: &mut MB, c: Vec2, r: f32, w: f32, z: f32, col: Color) {
    let n = 32;
    for k in 0..n {
        let a0 = k as f32 / n as f32 * std::f32::consts::TAU;
        let a1 = (k + 1) as f32 / n as f32 * std::f32::consts::TAU;
        let (d0, d1) = (vec2(a0.cos(), a0.sin()), vec2(a1.cos(), a1.sin()));
        let q = [c + d0 * (r - w / 2.0), c + d1 * (r - w / 2.0), c + d1 * (r + w / 2.0), c + d0 * (r + w / 2.0)];
        mb.poly_at(&q, z, col);
    }
}

/// A forward command post: sloped concrete bunker with a blue roof, a turning radar and a mast.
fn outpost(mb: &mut MB, d: &Def, c: Vec2, w: f32, h: f32, base: f32, t: f32) -> f32 {
    let (hw, hh) = (w / 2.0, h / 2.0);
    plinth(mb, c, hw, hh, base);
    let z0 = base + 0.5;
    let lo = chamfer(c, hw - 0.5, hh - 0.5, 1.6, 0.0);
    let hi = chamfer(c, hw - 1.4, hh - 1.4, 1.2, 0.0);
    let z1 = z0 + 2.4;
    loft(mb, &lo, &hi, z0, z1, Some(mix(d.top, CONCRETE, 0.35)), body_col(d));
    band(mb, &hi, z1 - 0.7, z1 - 0.35, d.top);
    // gun slits
    for k in 0..4 {
        let a = k as f32 * std::f32::consts::FRAC_PI_2;
        let p = c + vec2(a.cos(), a.sin()) * (hw - 1.05);
        block(mb, &rect(p, -0.1, -0.7, 0.1, 0.7, a), z0 + 1.2, z0 + 1.45, STEEL_DARK, STEEL_DARK);
    }
    // radar: a turning dish on a short drum
    let rp = c + vec2(-1.2, -0.8);
    cyl(mb, rp, 0.7, z1, z1 + 0.6, STEEL, STEEL_DARK);
    let a = t * 1.6;
    let dish = rect(rp, -0.25, -1.4, 0.25, 1.4, a);
    let dish_hi = rect(rp + rot(vec2(0.35, 0.0), a), -0.1, -1.6, 0.1, 1.6, a);
    loft(mb, &dish, &dish_hi, z1 + 0.6, z1 + 1.5, Some(rgb(230, 234, 238)), rgb(200, 205, 212));
    // mast with a beacon
    let mp = c + vec2(1.5, 1.2);
    strut(mb, mp, z1, mp, z1 + 5.0, 0.1, STEEL);
    for z in [z1 + 2.0, z1 + 3.5] {
        beam(mb, mp - vec2(0.6, 0.0), mp + vec2(0.6, 0.0), 0.05, z, STEEL);
    }
    let on = (t * 1.1).fract() < 0.4;
    lamp(mb, mp, z1 + 5.0, 0.22, if on { rgb(255, 70, 60) } else { rgb(110, 30, 30) });
    vent(mb, c + vec2(1.0, -1.6), z1, 0.5, 0.0);
    z1 + 1.5
}

/// A modern lab: white walls, a violet glass band and a glass dome that glows while researching.
#[allow(clippy::too_many_arguments)]
fn lab(mb: &mut MB, d: &Def, c: Vec2, w: f32, h: f32, base: f32, busy: bool, t: f32) -> f32 {
    let (hw, hh) = (w / 2.0, h / 2.0);
    plinth(mb, c, hw, hh, base);
    let z0 = base + 0.5;
    let body = chamfer(c, hw - 0.6, hh - 0.6, 1.8, 0.0);
    let z1 = z0 + 2.6;
    block(mb, &body, z0, z1, rgb(232, 234, 238), rgb(214, 217, 222));
    band(mb, &body, z0 + 0.5, z0 + 1.0, mix(d.side, GLASS, 0.4));
    band(mb, &body, z0 + 1.5, z0 + 2.0, mix(d.side, GLASS, 0.4));
    mb.poly_at(&shrink(&body, 0.35), z1 + 0.02, rgb(190, 194, 200));
    let glow = if busy { 0.5 + 0.5 * (t * 3.0).sin() } else { 0.0 };
    let dc = c + vec2(-0.6, -0.4);
    cyl(mb, dc, 2.3, z1, z1 + 0.35, STEEL, STEEL_DARK);
    dome(mb, dc, 2.1, z1 + 0.35, 1.9, mix(mix(d.top, WHITE, 0.3), WHITE, glow * 0.5));
    for k in 0..3 {
        let a = k as f32 * 2.094 + 0.5;
        let p = dc + vec2(a.cos(), a.sin()) * 2.1;
        beam(mb, p, dc + (p - dc) * 0.15, 0.06, z1 + 0.35 + 1.3, STEEL);
    }
    let ant = c + vec2(hw - 1.6, hh - 1.6);
    strut(mb, ant, z1, ant, z1 + 2.6, 0.06, STEEL);
    lamp(mb, ant, z1 + 2.6, 0.2, mix(d.top, WHITE, glow * 0.6));
    vent(mb, c + vec2(hw - 1.7, -hh + 1.9), z1, 0.55, 0.0);
    z1 + 2.3
}

/// Headquarters: a glass podium, a stepped blue tower with mullion bands, a rooftop radome,
/// antennas and red aircraft lights.
fn hq(mb: &mut MB, d: &Def, c: Vec2, w: f32, h: f32, base: f32, t: f32) -> f32 {
    let (hw, hh) = (w / 2.0, h / 2.0);
    plinth(mb, c, hw, hh, base);
    let z0 = base + 0.5;
    let pod = chamfer(c, hw - 0.6, hh - 0.6, 2.0, 0.0);
    let z1 = z0 + 2.2;
    block(mb, &pod, z0, z1, rgb(200, 205, 212), CONCRETE_DARK);
    band(mb, &pod, z0 + 0.6, z0 + 1.6, mix(GLASS, d.top, 0.3));
    mb.poly_at(&shrink(&pod, 0.4), z1 + 0.02, rgb(150, 156, 164));
    let tw = chamfer(c, hw - 3.0, hh - 3.0, 1.4, 0.0);
    let z2 = z1 + 4.8;
    block(mb, &tw, z1, z2, mix(d.top, CONCRETE, 0.45), mix(d.side, GLASS, 0.45));
    for k in 0..4 {
        let z = z1 + 0.8 + k as f32 * 1.0;
        band(mb, &tw, z, z + 0.18, mix(d.side, WHITE, 0.55));
    }
    let crown = shrink(&tw, 0.5);
    block(mb, &crown, z2, z2 + 0.4, mix(d.top, CONCRETE, 0.6), mix(d.side, CONCRETE_DARK, 0.4));
    // radome and antennas on the roof
    let rd = c + vec2(-1.2, -1.0);
    cyl(mb, rd, 1.3, z2 + 0.4, z2 + 0.8, CONCRETE, CONCRETE_DARK);
    dome(mb, rd, 1.3, z2 + 0.8, 1.4, rgb(240, 242, 245));
    for (p, ht) in [(vec2(1.6, 1.4), 3.4), (vec2(2.2, -0.6), 2.4)] {
        let a = c + p;
        strut(mb, a, z2 + 0.4, a, z2 + 0.4 + ht, 0.08, STEEL);
        let on = ((t * 0.9) + ht).fract() < 0.35;
        lamp(mb, a, z2 + 0.4 + ht, 0.2, if on { rgb(255, 60, 50) } else { rgb(110, 28, 28) });
    }
    vent(mb, c + vec2(1.4, -1.8), z2 + 0.4, 0.5, 0.0);
    // a few cars parked on the podium edge
    for k in 0..3 {
        let p = c + vec2(-hw + 1.4, -2.0 + k as f32 * 1.6);
        block(mb, &rect(p, -0.45, -0.3, 0.45, 0.3, 0.0), z0, z0 + 0.35, [rgb(200, 60, 50), rgb(230, 230, 235), rgb(40, 44, 52)][k], STEEL_DARK);
    }
    z2 + 1.8
}

// ------------------------------------------------------------------------------------------ hives

/// An alien hive: a clump of lumpy pods around a big central mound, chitin spires, glowing pores
/// that pulse, all breathing slowly.
pub fn hive(mb: &mut MB, h: &Hive, t: f32) -> f32 {
    let pulse = 1.0 + 0.05 * (t * 2.5).sin();
    let size = (5.0 + h.r * 0.06) * pulse;
    let seed = (h.p.x as u32).wrapping_mul(73_856_093) ^ (h.p.y as u32).wrapping_mul(19_349_663);
    let skin = rgb(92, 32, 128);
    let skin_hi = rgb(150, 70, 196);
    // a skirt of ooze
    let ooze: Vec<Vec2> = (0..18).map(|k| {
        let a = k as f32 / 18.0 * std::f32::consts::TAU;
        h.p + vec2(a.cos(), a.sin()) * size * (1.25 + 0.25 * hash(seed + k))
    }).collect();
    loft(mb, &ooze, &shrink(&ooze, size * 0.3), 0.0, 0.5, Some(rgb(70, 24, 96)), rgb(60, 20, 84));
    // side pods
    for k in 0..6 {
        let a = k as f32 / 6.0 * std::f32::consts::TAU + hash(seed + 40 + k) * 0.8;
        let r = size * (0.32 + 0.18 * hash(seed + 50 + k));
        let p = h.p + vec2(a.cos(), a.sin()) * size * 0.85;
        mb.obj(p, 0.0);
        lumpy_dome(mb, p, r, 0.3, r * 1.1, skin, skin_hi, seed + k * 7, t);
    }
    mb.obj(h.p, 0.5);
    // the mound
    let top = lumpy_dome(mb, h.p, size * 0.8, 0.3, size * 1.15, skin, skin_hi, seed, t);
    // spires
    for k in 0..4 {
        let a = k as f32 * 1.7 + hash(seed + 90 + k) * 1.2;
        let p = h.p + vec2(a.cos(), a.sin()) * size * 0.4;
        let tip = p + vec2(a.cos(), a.sin()) * size * 0.25;
        let base = ngon(p, size * 0.12, 5, a);
        loft(mb, &base, &[tip; 5], size * 0.6, size * (1.5 + 0.3 * hash(seed + 95 + k)), None, rgb(60, 22, 80));
    }
    // a glowing mouth on top
    let glow = 0.5 + 0.5 * (t * 3.0).sin();
    mb.poly_at(&ngon(h.p, size * 0.22, 10, 0.0), top + 0.05, mix(rgb(200, 80, 255), rgb(255, 210, 255), glow));
    top
}

/// A dome with a wobbly outline and glowing pores; returns its top height.
#[allow(clippy::too_many_arguments)]
fn lumpy_dome(mb: &mut MB, c: Vec2, r: f32, z0: f32, h: f32, lo: Color, hi: Color, seed: u32, t: f32) -> f32 {
    let n = 14;
    let wob: Vec<f32> = (0..n).map(|k| 0.82 + 0.3 * hash(seed.wrapping_mul(31) + k as u32)).collect();
    let ring = |f: f32| -> Vec<Vec2> { (0..n).map(|k| c + vec2((k as f32 / n as f32 * std::f32::consts::TAU).cos(), (k as f32 / n as f32 * std::f32::consts::TAU).sin()) * r * f * wob[k]).collect() };
    let steps = [(1.0, 0.0), (0.92, 0.4), (0.7, 0.75), (0.38, 0.95)];
    for k in 0..steps.len() - 1 {
        let col = mix(lo, hi, k as f32 / 3.0);
        loft(mb, &ring(steps[k].0), &ring(steps[k + 1].0), z0 + h * steps[k].1, z0 + h * steps[k + 1].1, None, col);
    }
    let (f, z) = steps[3];
    let top = ring(f);
    loft(mb, &top, &vec![c; n], z0 + h * z, z0 + h, None, hi);
    // pores
    for k in 0..4 {
        let a = hash(seed + 200 + k) * std::f32::consts::TAU;
        let p = c + vec2(a.cos(), a.sin()) * r * 0.62;
        let g = 0.5 + 0.5 * (t * 2.8 + k as f32 * 1.3).sin();
        lamp(mb, p, z0 + h * 0.68, r * 0.09, mix(rgb(180, 60, 230), rgb(255, 190, 255), g));
    }
    z0 + h
}

// ------------------------------------------------------------------------------------------ fungus

/// Purple fungus over an infected block: blotches and veins over the roof, streaks running down
/// the walls. Drawn with the pulse flag on (texture u = 1), so it throbs in the shader; v is the
/// phase, so pulses ripple outward from the middle of the roof.
pub fn fungus(mb: &mut MB, b: &Building, bi: usize) {
    let seed = (bi as u32).wrapping_mul(2_654_435_761);
    let rnd = |k: u32| hash(seed ^ k.wrapping_mul(0x9e37_79b9));
    let pts = &b.poly;
    let n = pts.len();
    if n < 3 {
        return;
    }
    let (mut lo, mut hi) = (pts[0], pts[0]);
    for p in pts {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    let edge_dist = |p: Vec2| -> f32 {
        let mut best = f32::MAX;
        for k in 0..n {
            let (a, c) = (pts[k], pts[(k + 1) % n]);
            let e = c - a;
            let f = ((p - a).dot(e) / e.length_squared().max(1e-6)).clamp(0.0, 1.0);
            best = best.min(p.distance(a + e * f));
        }
        best
    };
    let mid = if point_in_poly(b.c, pts) { b.c } else { pts[0] };
    let mat = rgb(74, 22, 104);
    let dark = rgb(104, 34, 150);
    let vein = rgb(186, 84, 240);
    let core = rgb(240, 160, 255);
    let z = b.h;
    let phase = |p: Vec2| -p.distance(mid) * 0.45;
    let tag = |mb: &mut MB, p: Vec2| mb.uv = vec2(1.0, phase(p));
    let inside_at = |p: Vec2, r: f32| point_in_poly(p, pts) && edge_dist(p) > r;
    let wobbly = |p: Vec2, r: f32, m: usize, salt: u32| -> Vec<Vec2> {
        (0..m).map(|q| {
            let a = q as f32 / m as f32 * std::f32::consts::TAU;
            p + vec2(a.cos(), a.sin()) * r * (0.72 + 0.28 * rnd(salt * 31 + q as u32))
        }).collect()
    };
    // colonies: a mycelium mat, tendrils creeping out of it and clusters of little caps
    let area = b.area.abs();
    let colonies = ((area / 160.0) as u32).clamp(1, 5);
    let reach = (area / colonies as f32).sqrt() * 0.5;
    let mut salt = 0u32;
    for col in 0..colonies {
        let mut ctr = None;
        for k in 0..12 {
            let p = if col == 0 && k == 0 { mid } else { vec2(lo.x + (hi.x - lo.x) * rnd(col * 40 + k * 2), lo.y + (hi.y - lo.y) * rnd(col * 40 + k * 2 + 1)) };
            if inside_at(p, 0.6) {
                ctr = Some(p);
                break;
            }
        }
        let Some(ctr) = ctr else { continue };
        tag(mb, ctr);
        let mr = (reach * 0.55).min(edge_dist(ctr) * 0.95);
        salt += 1;
        mb.poly_at(&wobbly(ctr, mr, 14, salt), z + 0.04, mat);
        // tendrils: wandering, tapering veins
        let arms = 5 + (rnd(col + 70) * 4.0) as u32;
        for arm in 0..arms {
            let mut a = arm as f32 / arms as f32 * std::f32::consts::TAU + rnd(col * 16 + arm + 80) * 0.8;
            let mut p = ctr;
            let steps = 4 + (rnd(col * 16 + arm + 90) * 4.0) as u32;
            for st in 0..steps {
                a += (rnd(col * 256 + arm * 16 + st + 100) - 0.5) * 1.1;
                let q = p + vec2(a.cos(), a.sin()) * reach * 0.22;
                if !inside_at(q, 0.2) {
                    break;
                }
                let wdt = 0.26 * (1.0 - st as f32 / steps as f32) + 0.06;
                let d = (q - p).normalize_or_zero();
                let nn = vec2(-d.y, d.x) * wdt;
                tag(mb, q);
                mb.poly_at(&[p + nn, q + nn * 0.8, q - nn * 0.8, p - nn], z + 0.06, vein);
                p = q;
            }
        }
        // caps, thicker near the middle
        let caps = ((area / colonies as f32 / 7.0) as u32).clamp(5, 24);
        for k in 0..caps {
            let a = rnd(col * 512 + k * 3 + 300) * std::f32::consts::TAU;
            let dist = reach * rnd(col * 512 + k * 3 + 301).powf(1.6);
            let p = ctr + vec2(a.cos(), a.sin()) * dist;
            let r = (0.4 + 1.0 * rnd(col * 512 + k * 3 + 302)) * (1.0 - 0.5 * dist / reach.max(0.1));
            if !inside_at(p, r) {
                continue;
            }
            tag(mb, p);
            salt += 1;
            let rim = wobbly(p, r, 9, salt);
            mb.poly_at(&rim, z + 0.08, dark);
            mb.poly_at(&shrink(&rim, r * 0.3), z + 0.1, vein);
            mb.poly_at(&ngon(p, r * 0.28, 6, 0.0), z + 0.12, core);
        }
    }
    // streaks down the walls
    let ccw = b.area > 0.0;
    for e in 0..n {
        let (a, c) = (pts[e], pts[(e + 1) % n]);
        let len = a.distance(c);
        if len < 2.0 {
            continue;
        }
        let dir = (c - a) / len;
        let mut out = vec2(dir.y, -dir.x);
        if !ccw {
            out = -out;
        }
        let drips = ((len / 2.5) as u32).clamp(1, 10);
        for q in 0..drips {
            let s = rnd(e as u32 * 64 + q * 3 + 3000);
            if s < 0.35 {
                continue;
            }
            let at = a + dir * len * ((q as f32 + 0.2 + 0.6 * rnd(e as u32 * 64 + q * 3 + 3001)) / drips as f32);
            let wid = 0.25 + 0.4 * rnd(e as u32 * 64 + q * 3 + 3002);
            let drop = z * (0.2 + 0.65 * s);
            let p = at + out * 0.08;
            tag(mb, at);
            let inside = p3(at - out, z * 0.5);
            face(mb, &[p3(p - dir * wid, z), p3(p + dir * wid, z), p3(p + dir * wid * 0.3, z - drop), p3(p - dir * wid * 0.3, z - drop)], inside, vein);
            // a bulb where the drip ends
            let e = p + out * 0.02;
            let r = wid * 0.7;
            face(mb, &[p3(e - dir * r, z - drop + r), p3(e + dir * r, z - drop + r), p3(e + dir * r * 0.6, z - drop - r), p3(e - dir * r * 0.6, z - drop - r)], inside, core);
        }
    }
    mb.uv = Vec2::ZERO;
}

fn point_in_poly(p: Vec2, pts: &[Vec2]) -> bool {
    let mut inside = false;
    let n = pts.len();
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (pts[i], pts[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

//! Custom turrets from the garage, built from their parts: the mount sets the body, the head and
//! barrel sit on top in the player's paint, and accessories and modifiers only add a light or a
//! stripe, so a crowded base never turns into clutter. The garage draws the same model, big.

use super::art::*;
use super::*;
use crate::garage::{part, Build, Fam, Spec};

/// How a turret is posed this frame.
#[derive(Clone, Copy, Default)]
pub struct Pose {
    pub aim: f32,
    pub aim2: f32,
    /// pop-up mounts: 0 in the vault, 1 up
    pub rise: f32,
    /// crane arms: how far out the head is
    pub arm: f32,
    /// it fired a moment ago (muzzle glow)
    pub flash: f32,
    /// walkers: their stride
    pub walk: f32,
    pub t: f32,
}

fn col(c: [u8; 3]) -> Color {
    rgb(c[0], c[1], c[2])
}

/// The ammo stripe a modifier paints on the barrel.
fn stripe(f: Fam) -> Option<Color> {
    Some(match f {
        Fam::Incendiary => rgb(255, 120, 40),
        Fam::Cryo => rgb(120, 220, 255),
        Fam::Cluster | Fam::HighEx => rgb(250, 82, 82),
        Fam::Piercing => rgb(230, 230, 235),
        Fam::Volatile => rgb(200, 110, 255),
        Fam::Seeker => rgb(64, 192, 87),
        Fam::Overcharge => rgb(116, 192, 252),
        _ => return None,
    })
}

/// Draws a custom turret standing at `c` on `base`; returns the height of its top.
pub(super) fn turret(mb: &mut MB, b: &Build, sp: &Spec, c: Vec2, base: f32, pose: Pose) -> f32 {
    let body = col(b.paint[0]);
    let headc = col(b.paint[1]);
    let gun = col(b.paint[2]);
    let fam = |k: usize| b.parts[k].map(|i| part(i).fam);
    let mount = fam(0).unwrap_or(Fam::Ring);
    let side = mix(body, BLACK, 0.35);
    // the mount: returns where the head sits (centre, height)
    let (hc, hz) = match mount {
        Fam::Sandbag => {
            plinth(mb, c, 2.6, 2.6, base);
            let ring = ngon(c, 2.5, 10, 0.0);
            loft(
                mb,
                &ring,
                &shrink(&ring, 0.5),
                base + 0.5,
                base + 1.1,
                Some(mix(rgb(196, 180, 140), body, 0.4)),
                mix(rgb(150, 136, 100), body, 0.3),
            );
            cyl(mb, c, 1.0, base + 0.5, base + 1.2, STEEL, STEEL_DARK);
            (c, base + 1.2)
        }
        Fam::Pintle => {
            plinth(mb, c, 2.2, 2.2, base);
            cyl(mb, c, 0.9, base + 0.5, base + 0.9, body, side);
            cyl(mb, c, 0.35, base + 0.9, base + 2.0, STEEL, STEEL_DARK);
            (c, base + 2.0)
        }
        Fam::Casemate => {
            plinth(mb, c, 3.0, 3.0, base);
            let lo = chamfer(c, 2.85, 2.85, 1.2, pose.aim.min(99.0) * 0.0);
            let hi = chamfer(c, 2.3, 2.3, 1.0, 0.0);
            loft(
                mb,
                &lo,
                &hi,
                base + 0.5,
                base + 2.2,
                Some(mix(CONCRETE, body, 0.35)),
                mix(CONCRETE_DARK, body, 0.35),
            );
            band(mb, &hi, base + 1.7, base + 2.0, body);
            (c, base + 2.2)
        }
        Fam::Mast => {
            let top = base + sp.height;
            plinth(mb, c, 2.6, 2.6, base);
            for k in 0..4 {
                let a = k as f32 * std::f32::consts::FRAC_PI_2 + std::f32::consts::FRAC_PI_4;
                let foot = c + vec2(a.cos(), a.sin()) * 2.3;
                let head = c + vec2(a.cos(), a.sin()) * 0.9;
                strut(mb, foot, base + 0.5, head, top, 0.16, mix(STEEL, body, 0.3));
            }
            for f in [0.4f32, 0.7] {
                let z = base + 0.5 + (top - base - 0.5) * f;
                let r = 2.3 - 1.4 * f;
                ring_flat(mb, c, r, 0.18, z, mix(STEEL, body, 0.3));
            }
            let deck = chamfer(c, 1.4, 1.4, 0.4, 0.0);
            block(mb, &deck, top - 0.3, top, body, side);
            (c, top)
        }
        Fam::Popup => {
            plinth(mb, c, 3.0, 3.0, base);
            let lid = chamfer(c, 2.6, 2.6, 0.6, 0.0);
            block(
                mb,
                &lid,
                base + 0.5,
                base + 0.8,
                mix(STEEL, body, 0.4),
                STEEL_DARK,
            );
            mb.poly_at(
                &chamfer(c, 1.6, 1.6, 0.4, 0.0),
                base + 0.82,
                rgb(20, 22, 26),
            );
            // stripes round the hatch
            for k in 0..4 {
                let a = k as f32 * std::f32::consts::FRAC_PI_2;
                lamp(
                    mb,
                    c + vec2(a.cos(), a.sin()) * 2.1,
                    base + 0.8,
                    0.18,
                    rgb(255, 196, 40),
                );
            }
            let up = base + 0.8 + 1.6 * pose.rise;
            if pose.rise > 0.05 {
                cyl(mb, c, 1.2, base + 0.8, up, body, side);
            }
            (c, up)
        }
        Fam::Crane => {
            plinth(mb, c, 2.8, 2.8, base);
            let lo = chamfer(c, 2.0, 2.0, 0.7, 0.0);
            block(
                mb,
                &lo,
                base + 0.5,
                base + 1.6,
                mix(CONCRETE, body, 0.3),
                side,
            );
            cyl(mb, c, 0.8, base + 1.6, base + 2.4, body, side);
            let d = vec2(pose.aim.cos(), pose.aim.sin());
            let tip = c + d * (0.6 + pose.arm.min(16.0));
            strut(
                mb,
                c,
                base + 2.4,
                tip,
                base + 2.2,
                0.22,
                mix(rgb(255, 196, 40), body, 0.3),
            );
            beam(mb, c - d * 1.2, c, 0.35, base + 2.3, STEEL_DARK);
            (tip, base + 2.3)
        }
        Fam::Gyro => {
            plinth(mb, c, 2.8, 2.8, base);
            cyl(
                mb,
                c,
                2.0,
                base + 0.5,
                base + 1.3,
                mix(CONCRETE, body, 0.3),
                side,
            );
            let wob = (pose.t * 2.0).sin() * 0.15;
            ring_flat(mb, c, 1.9, 0.25, base + 1.6 + wob, mix(body, WHITE, 0.3));
            ring_flat(mb, c, 1.6, 0.2, base + 1.9 - wob, mix(body, WHITE, 0.2));
            (c, base + 1.8)
        }
        Fam::Twin => {
            plinth(mb, c, 3.0, 3.0, base);
            let lo = chamfer(c, 2.8, 2.2, 0.9, 0.0);
            let hi = chamfer(c, 2.4, 1.8, 0.7, 0.0);
            loft(
                mb,
                &lo,
                &hi,
                base + 0.5,
                base + 1.8,
                Some(mix(CONCRETE, body, 0.3)),
                side,
            );
            (c, base + 1.8)
        }
        Fam::Fortress => {
            plinth(mb, c, 3.0, 3.0, base);
            let lo = ngon(c, 3.0, 8, std::f32::consts::PI / 8.0);
            let hi = ngon(c, 2.6, 8, std::f32::consts::PI / 8.0);
            loft(
                mb,
                &lo,
                &hi,
                base + 0.5,
                base + 2.4,
                Some(mix(CONCRETE, body, 0.25)),
                mix(CONCRETE_DARK, body, 0.3),
            );
            for k in 0..8 {
                let a = k as f32 / 8.0 * std::f32::consts::TAU;
                let p = c + vec2(a.cos(), a.sin()) * 2.35;
                block(
                    mb,
                    &rect(p, -0.35, -0.35, 0.35, 0.35, a),
                    base + 2.4,
                    base + 2.85,
                    body,
                    side,
                );
            }
            (c, base + 2.6)
        }
        Fam::Walker => {
            let lift = base + 2.8 + (pose.walk * 6.0).sin().abs() * 0.25;
            for k in 0..4 {
                let a = k as f32 * std::f32::consts::FRAC_PI_2 + std::f32::consts::FRAC_PI_4;
                let step = (pose.walk * 6.0 + k as f32 * 1.6).sin() * 0.5;
                let d = vec2(a.cos(), a.sin());
                let foot = c + d * (3.0 + step);
                let knee = c + d * 2.2;
                strut(
                    mb,
                    c + d * 0.8,
                    lift - 0.4,
                    knee,
                    lift + 0.8,
                    0.18,
                    mix(STEEL, body, 0.3),
                );
                strut(
                    mb,
                    knee,
                    lift + 0.8,
                    foot,
                    base + 0.1,
                    0.15,
                    mix(STEEL_DARK, body, 0.2),
                );
            }
            let hull = chamfer(c, 1.5, 1.5, 0.5, pose.aim);
            block(mb, &hull, lift - 0.6, lift, body, side);
            (c, lift)
        }
        _ => {
            // ring mounts, and the look anything new starts from
            plinth(mb, c, 2.9, 2.9, base);
            let lo = chamfer(c, 2.6, 2.6, 1.0, 0.0);
            let hi = chamfer(c, 2.1, 2.1, 0.8, 0.0);
            let z1 = base + 1.8;
            loft(
                mb,
                &lo,
                &hi,
                base + 0.5,
                z1,
                Some(mix(CONCRETE, body, 0.2)),
                mix(body, CONCRETE_DARK, 0.4),
            );
            band(mb, &hi, z1 - 0.4, z1 - 0.18, body);
            cyl(mb, c, 1.25, z1, z1 + 0.3, STEEL, STEEL_DARK);
            (c, z1 + 0.3)
        }
    };
    // accessories: a light or a fitting each, never more
    let mut top = hz;
    for k in [5usize, 6] {
        let Some(f) = fam(k) else { continue };
        let back = hc - vec2(pose.aim.cos(), pose.aim.sin()) * 1.2;
        let sidev = vec2(-pose.aim.sin(), pose.aim.cos()) * if k == 5 { 0.9 } else { -0.9 };
        match f {
            Fam::Plates => {
                if mount != Fam::Walker && mount != Fam::Mast {
                    band(
                        mb,
                        &chamfer(c, 2.65, 2.65, 1.0, 0.0),
                        base + 0.6,
                        base + 1.1,
                        mix(STEEL_DARK, body, 0.2),
                    );
                }
            }
            Fam::Shield => ring_flat(
                mb,
                c,
                3.3,
                0.12,
                base + 0.15,
                rgba(120, 200, 255, 0.55 + 0.25 * (pose.t * 3.0).sin().abs()),
            ),
            Fam::Spotter => {
                strut(mb, back + sidev, hz, back + sidev, hz + 2.2, 0.06, STEEL);
                lamp(mb, back + sidev, hz + 2.2, 0.16, rgb(250, 82, 82));
            }
            Fam::Rangefinder | Fam::Stabiliser => {
                lamp(mb, hc + sidev * 1.1, hz + 0.6, 0.2, rgb(255, 230, 140))
            }
            Fam::Repair => lamp(mb, back + sidev, hz, 0.2, rgb(64, 192, 87)),
            Fam::Mines | Fam::Hopper | Fam::Recycler => block(
                mb,
                &rect(back + sidev, -0.4, -0.4, 0.4, 0.4, pose.aim),
                hz,
                hz + 0.5,
                mix(gun, rgb(80, 90, 70), 0.5),
                STEEL_DARK,
            ),
            Fam::Ciws => beam(
                mb,
                hc + sidev * 1.3,
                hc + sidev * 1.3 + vec2(pose.aim.cos(), pose.aim.sin()) * 1.2,
                0.1,
                hz + 0.3,
                STEEL_DARK,
            ),
            Fam::Cooling => lamp(mb, back + sidev, hz, 0.2, rgb(120, 220, 255)),
            _ => {}
        }
        top = top.max(hz);
    }
    let stripe = [3usize, 4].iter().find_map(|k| fam(*k).and_then(stripe));
    let twin = mount == Fam::Twin;
    let heads: Vec<(Vec2, f32)> = if twin {
        let s = vec2(-pose.aim.sin(), pose.aim.cos());
        vec![(hc + s * 1.2, pose.aim), (hc - s * 1.2, pose.aim2)]
    } else {
        vec![(hc, pose.aim)]
    };
    let scale = if twin { 0.75 } else { 1.0 };
    for (p, a) in heads {
        let t = head(
            mb,
            fam(1).unwrap_or(Fam::Iron),
            fam(2).unwrap_or(Fam::Rifle),
            p,
            hz,
            a,
            scale,
            headc,
            gun,
            stripe,
            pose,
        );
        top = top.max(t);
    }
    top
}

/// The head and its barrel; returns the top.
#[allow(clippy::too_many_arguments)]
fn head(
    mb: &mut MB,
    hf: Fam,
    bf: Fam,
    c: Vec2,
    z: f32,
    aim: f32,
    k: f32,
    hc: Color,
    gun: Color,
    stripe: Option<Color>,
    pose: Pose,
) -> f32 {
    let fwd = vec2(aim.cos(), aim.sin());
    let side = vec2(-fwd.y, fwd.x);
    let hs = mix(hc, BLACK, 0.3);
    let lens = match hf {
        Fam::Iron => rgb(60, 60, 60),
        Fam::Optic => rgb(116, 192, 252),
        Fam::Crowd => rgb(255, 146, 43),
        Fam::Far => rgb(165, 216, 255),
        Fam::Guard => rgb(64, 192, 87),
        Fam::LastLine => rgb(250, 82, 82),
        Fam::Lead => rgb(255, 212, 59),
        Fam::Scatter => rgb(204, 93, 232),
        Fam::Sweep => rgb(255, 240, 200),
        Fam::Split => rgb(56, 217, 169),
        _ => rgb(240, 248, 255),
    };
    // the head's own shape
    let hz = z + 0.9 * k;
    match hf {
        Fam::Crowd | Fam::Scatter | Fam::Overwatch => {
            cyl(mb, c, 1.15 * k, z, z + 0.35 * k, hs, hs);
            dome(mb, c, 1.15 * k, z + 0.35 * k, 0.8 * k, hc);
        }
        Fam::Guard | Fam::Split => {
            let hex = ngon(c, 1.2 * k, 6, aim);
            loft(mb, &hex, &shrink(&hex, 0.25 * k), z, hz, Some(hc), hs);
        }
        Fam::Sweep => cyl(mb, c, 1.1 * k, z, hz, hc, hs),
        _ => {
            let lo = chamfer(c - fwd * 0.2 * k, 1.25 * k, 1.0 * k, 0.5 * k, aim);
            let hi = chamfer(c - fwd * 0.3 * k, 1.0 * k, 0.8 * k, 0.4 * k, aim);
            loft(mb, &lo, &hi, z, hz, Some(hc), hs);
        }
    }
    lamp(
        mb,
        c + fwd * 0.7 * k + side * 0.45 * k,
        z + 0.45 * k,
        0.17 * k,
        lens,
    );
    if hf == Fam::Split {
        lamp(
            mb,
            c + fwd * 0.7 * k - side * 0.45 * k,
            z + 0.45 * k,
            0.17 * k,
            lens,
        );
    }
    if hf == Fam::Far {
        beam(
            mb,
            c - fwd * 0.4 * k + side * 0.7 * k,
            c + fwd * 1.3 * k + side * 0.7 * k,
            0.13 * k,
            hz + 0.05,
            STEEL_DARK,
        );
    }
    if hf == Fam::Lead {
        strut(
            mb,
            c - fwd * 0.6 * k,
            hz,
            c - fwd * 0.7 * k,
            hz + 1.0 * k,
            0.05,
            STEEL,
        );
    }
    // the barrel
    let bz = z + 0.5 * k;
    let m = c + fwd * 0.8 * k;
    let gd = mix(gun, BLACK, 0.25);
    let tip_glow = |mb: &mut MB, p: Vec2, zz: f32| {
        if pose.flash > 0.0 {
            lamp(mb, p, zz - 0.1, 0.2 * k, rgba(255, 220, 140, pose.flash));
        }
    };
    let len = match bf {
        Fam::Rifle => {
            beam(mb, m, m + fwd * 2.6 * k, 0.14 * k, bz, gd);
            beam(mb, m + fwd * 2.4 * k, m + fwd * 2.8 * k, 0.2 * k, bz, gun);
            2.8
        }
        Fam::Rotary => {
            cyl(
                mb,
                m + fwd * 0.3 * k,
                0.4 * k,
                bz - 0.4 * k,
                bz + 0.4 * k,
                gun,
                gd,
            );
            let spin = pose.t * 14.0 * pose.flash.max(0.1);
            for j in 0..3 {
                let a = spin + j as f32 * 2.09;
                let off = side * a.cos() * 0.2 * k;
                let zo = a.sin() * 0.2 * k;
                beam(mb, m + off, m + off + fwd * 2.3 * k, 0.08 * k, bz + zo, gd);
            }
            2.3
        }
        Fam::Shotgun => {
            beam(mb, m, m + fwd * 1.6 * k, 0.26 * k, bz, gd);
            beam(mb, m + fwd * 1.4 * k, m + fwd * 1.8 * k, 0.32 * k, bz, gun);
            1.8
        }
        Fam::Howitzer => {
            beam(mb, m, m + fwd * 3.4 * k, 0.24 * k, bz, gd);
            beam(mb, m, m + fwd * 0.8 * k, 0.34 * k, bz, gun);
            beam(mb, m + fwd * 3.2 * k, m + fwd * 3.8 * k, 0.33 * k, bz, gun);
            3.8
        }
        Fam::Mortar => {
            strut(mb, m, bz, m + fwd * 0.9 * k, bz + 1.6 * k, 0.32 * k, gd);
            lamp(mb, m + fwd * 0.9 * k, bz + 1.6 * k, 0.3 * k, gun);
            1.0
        }
        Fam::Rail => {
            for s in [-0.25f32, 0.25] {
                beam(
                    mb,
                    m + side * s * k,
                    m + side * s * k + fwd * 3.8 * k,
                    0.1 * k,
                    bz,
                    gd,
                );
            }
            beam(
                mb,
                m,
                m + fwd * 3.6 * k,
                0.05 * k,
                bz,
                rgba(150, 240, 255, 0.6 + 0.4 * pose.flash),
            );
            3.8
        }
        Fam::Laser => {
            beam(mb, m, m + fwd * 2.2 * k, 0.22 * k, bz, gd);
            lamp(
                mb,
                m + fwd * 2.3 * k,
                bz - 0.15 * k,
                0.18 * k,
                rgb(255, 80, 80),
            );
            2.4
        }
        Fam::Missile => {
            let pod = rect(m + fwd * 0.6 * k, -0.6 * k, -0.7 * k, 0.9 * k, 0.7 * k, aim);
            block(mb, &pod, bz - 0.4 * k, bz + 0.5 * k, gun, gd);
            for j in 0..4 {
                let p = m + fwd * 1.52 * k + side * ((j % 2) as f32 - 0.5) * 0.7 * k;
                lamp(
                    mb,
                    p,
                    bz + ((j / 2) as f32 - 0.5) * 0.4 * k,
                    0.12 * k,
                    rgb(30, 30, 34),
                );
            }
            1.5
        }
        Fam::Flamer => {
            cyl(
                mb,
                c - fwd * 0.9 * k - side * 0.6 * k,
                0.35 * k,
                z,
                z + 0.9 * k,
                rgb(240, 101, 46),
                rgb(160, 60, 20),
            );
            beam(mb, m, m + fwd * 1.8 * k, 0.16 * k, bz, gd);
            beam(mb, m + fwd * 1.6 * k, m + fwd * 2.0 * k, 0.24 * k, bz, gun);
            2.0
        }
        Fam::Tesla => {
            for j in 0..3 {
                let zz = hz + j as f32 * 0.35 * k;
                cyl(
                    mb,
                    c,
                    (0.55 - j as f32 * 0.08) * k,
                    zz,
                    zz + 0.25 * k,
                    mix(rgb(200, 150, 80), gun, 0.3),
                    rgb(150, 100, 50),
                );
            }
            dome(
                mb,
                c,
                0.45 * k,
                hz + 1.05 * k,
                0.45 * k,
                rgba(150, 200, 255, 0.85),
            );
            0.0
        }
        Fam::Flak => {
            for s in [-0.3f32, 0.3] {
                beam(
                    mb,
                    m + side * s * k,
                    m + side * s * k + fwd * 1.9 * k,
                    0.12 * k,
                    bz,
                    gd,
                );
            }
            1.9
        }
        Fam::Needle => {
            beam(mb, m, m + fwd * 3.6 * k, 0.07 * k, bz, gd);
            1.6
        }
        Fam::Sonic => {
            let dish = ngon(m + fwd * 0.9 * k, 0.9 * k, 10, aim);
            loft(
                mb,
                &shrink(&dish, 0.6 * k),
                &dish,
                bz - 0.4 * k,
                bz + 0.2 * k,
                Some(mix(gun, WHITE, 0.2)),
                gd,
            );
            1.0
        }
        _ => 2.0,
    };
    if let (Some(s), true) = (stripe, len > 1.2) {
        let p = m + fwd * len * 0.55 * k;
        beam(mb, p, p + fwd * 0.25 * k, 0.2 * k, bz, s);
    }
    if len > 0.5 {
        tip_glow(mb, m + fwd * len * k, bz);
    }
    let _ = hc;
    hz + if bf == Fam::Tesla { 1.5 * k } else { 0.0 }
}

/// A custom turret in a game.
pub(super) fn in_game(mb: &mut MB, g: &Game, s: &Structure, base: f32, t: f32) -> f32 {
    let Some(cu) = &s.cu else { return base };
    let Some(d) = g.designs.get(cu.d as usize) else {
        return base;
    };
    let pose = Pose {
        aim: s.aim,
        aim2: cu.aim2,
        rise: cu.rise,
        arm: cu.arm,
        flash: (1.0 - (g.time - s.last_fire) / 0.15).clamp(0.0, 1.0),
        walk: if cu.pos.distance(cu.home) > 0.3 {
            t
        } else {
            0.0
        },
        t,
    };
    turret(mb, &d.build, &d.spec, cu.pos, base, pose)
}

/// Where the garage's turntable stands on the (otherwise empty) map, well inside it so nothing
/// is clipped at its edge.
const STAGE: Vec2 = vec2(200.0, 200.0);

/// The garage camera: (eye, target) for a turn `yaw`, tilt `pitch` and distance `dist`.
pub fn preview_eye(yaw: f32, pitch: f32, dist: f32) -> (Vec3, Vec3) {
    let target = p3(STAGE, 2.4);
    (
        target
            + vec3(
                yaw.cos() * pitch.cos(),
                yaw.sin() * pitch.cos(),
                -pitch.sin(),
            ) * dist,
        target,
    )
}
pub fn preview_matrix(yaw: f32, pitch: f32, dist: f32, aspect: f32) -> Mat4 {
    let (eye, target) = preview_eye(yaw, pitch, dist);
    Mat4::perspective_rh_gl(0.55, aspect, 0.5, 400.0)
        * Mat4::look_at_rh(eye, target, vec3(0.0, 0.0, -1.0))
}
/// Points on the body, head and barrel, for the paint bucket.
pub fn zones(sp: &Spec, pose: Pose) -> [Vec3; 3] {
    let fwd = vec2(pose.aim.cos(), pose.aim.sin());
    [
        p3(STAGE + vec2(0.0, 0.0), sp.height * 0.5 + 0.4),
        p3(STAGE, sp.height + 0.9),
        p3(STAGE + fwd * 2.6, sp.height + 0.6),
    ]
}

/// Renders a build for the garage, big and lit on a turntable, into `rt` (or the screen inside
/// `viewport`, in pixels, without one). `mat` is a depth-tested material, or None on a GPU
/// without a working depth buffer (then far parts are drawn first).
#[allow(clippy::too_many_arguments)]
pub fn preview(
    b: &Build,
    sp: &Spec,
    pose: Pose,
    yaw: f32,
    pitch: f32,
    dist: f32,
    rt: Option<&RenderTarget>,
    viewport: (i32, i32, i32, i32),
    mat: Option<&Material>,
) {
    let c = STAGE;
    let mut mb = MB::split(mat.is_none());
    mb.obj(c, 0.0);
    // the turntable
    mb.prism(
        &art::ngon(c, 6.5, 40, 0.0),
        -0.4,
        0.0,
        rgb(58, 62, 68),
        rgb(36, 38, 42),
    );
    art::ring_flat(&mut mb, c, 6.2, 0.15, 0.02, rgba(255, 196, 40, 0.65));
    for k in 0..12 {
        let a = k as f32 / 12.0 * std::f32::consts::TAU;
        mb.line(
            &[
                c + vec2(a.cos(), a.sin()) * 4.2,
                c + vec2(a.cos(), a.sin()) * 5.6,
            ],
            0.08,
            rgba(255, 255, 255, 0.10),
        );
    }
    mb.obj(c, 0.5);
    turret(&mut mb, b, sp, c, 0.0, pose);
    let (eye, target) = preview_eye(yaw, pitch, dist);
    let aspect = viewport.2 as f32 / viewport.3.max(1) as f32;
    let cam = Camera3D {
        position: eye,
        target,
        up: vec3(0.0, 0.0, -1.0),
        fovy: 0.55,
        aspect: Some(aspect),
        z_near: 0.5,
        z_far: 400.0,
        render_target: rt.cloned(),
        viewport: if rt.is_some() { None } else { Some(viewport) },
        ..Default::default()
    };
    set_camera(&cam);
    if rt.is_some() {
        clear_background(Color::new(0.0, 0.0, 0.0, 0.0));
    }
    let (meshes, anchors) = mb.finish_objs();
    match mat {
        Some(m) => {
            m.set_uniform("Time", pose.t);
            gl_use_material(m);
            for me in &meshes {
                draw_mesh(me);
            }
        }
        None => {
            // no depth buffer: far things first
            let mut order: Vec<(f32, usize)> = anchors
                .iter()
                .enumerate()
                .map(|(i, a)| ((*a - eye).length(), i))
                .collect();
            order.sort_by(|a, b| b.0.total_cmp(&a.0));
            gl_use_default_material();
            for (_, i) in order {
                draw_mesh(&meshes[i]);
            }
        }
    }
    gl_use_default_material();
    set_default_camera();
}

//! Random city generator. Produces the same vector description as the OpenStreetMap loader.

use crate::world::*;
use macroquad::prelude::{vec2, Vec2};

fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Poly {
    vec![vec2(x0, y0), vec2(x1, y0), vec2(x1, y1), vec2(x0, y1)]
}

pub fn generate(seed: u64) -> Desc {
    let mut r = Rng::new(seed);
    let (wf, hf) = (W as f32, H as f32);
    let mut d = Desc {
        name: format!("Generated city #{}", seed % 100000),
        mpc: 3.0,
        ..Default::default()
    };

    // airport somewhere away from the exact edge, size varies
    let aw = r.range(300.0, 380.0);
    let ah = r.range(260.0, 340.0);
    let side = r.below(5);
    let (ax0, ay0) = match side {
        0 => (r.range(90.0, 180.0), r.range(200.0, hf - ah - 200.0)),
        1 => (
            wf - aw - r.range(90.0, 180.0),
            r.range(200.0, hf - ah - 200.0),
        ),
        2 => (r.range(200.0, wf - aw - 200.0), r.range(90.0, 180.0)),
        3 => (
            r.range(200.0, wf - aw - 200.0),
            hf - ah - r.range(90.0, 180.0),
        ),
        _ => (
            r.range(wf * 0.3, wf * 0.7 - aw),
            r.range(hf * 0.3, hf * 0.7 - ah),
        ),
    };
    let (ax1, ay1) = (ax0 + aw, ay0 + ah);
    d.airport = Some(rect(ax0, ay0, ax1, ay1));
    let horizontal_runways = r.f() < 0.5;
    if horizontal_runways {
        d.runways.push(Line {
            pts: vec![vec2(ax0 + 30.0, ay0 + 55.0), vec2(ax1 - 25.0, ay0 + 55.0)],
            w: 16.0,
        });
        d.runways.push(Line {
            pts: vec![vec2(ax0 + 35.0, ay1 - 45.0), vec2(ax1 - 30.0, ay1 - 45.0)],
            w: 14.0,
        });
        for x in [ax0 + 40.0, ax1 - 40.0] {
            d.runways.push(Line {
                pts: vec![vec2(x, ay0 + 55.0), vec2(x, ay1 - 45.0)],
                w: 6.0,
            });
        }
        d.runways.push(Line {
            pts: vec![
                vec2(ax0 + 40.0, (ay0 + ay1) / 2.0),
                vec2(ax1 - 40.0, (ay0 + ay1) / 2.0),
            ],
            w: 6.0,
        });
    } else {
        d.runways.push(Line {
            pts: vec![vec2(ax0 + 55.0, ay0 + 30.0), vec2(ax0 + 55.0, ay1 - 25.0)],
            w: 16.0,
        });
        d.runways.push(Line {
            pts: vec![vec2(ax1 - 45.0, ay0 + 35.0), vec2(ax1 - 45.0, ay1 - 30.0)],
            w: 14.0,
        });
        for y in [ay0 + 40.0, ay1 - 40.0] {
            d.runways.push(Line {
                pts: vec![vec2(ax0 + 55.0, y), vec2(ax1 - 45.0, y)],
                w: 6.0,
            });
        }
        d.runways.push(Line {
            pts: vec![
                vec2((ax0 + ax1) / 2.0, ay0 + 40.0),
                vec2((ax0 + ax1) / 2.0, ay1 - 40.0),
            ],
            w: 6.0,
        });
    }
    let hq = vec2(
        (ax0 + ax1) / 2.0 + r.range(-30.0, 30.0),
        (ay0 + ay1) / 2.0 + r.range(-20.0, 20.0),
    );
    d.aprons
        .push(rect(hq.x - 85.0, hq.y - 60.0, hq.x + 85.0, hq.y + 60.0));
    d.hq = hq;
    let in_air = |x: f32, y: f32, m: f32| x > ax0 - m && x < ax1 + m && y > ay0 - m && y < ay1 + m;

    // river, sometimes
    if r.f() < 0.75 {
        let vertical = r.f() < 0.5;
        let base = r.range(0.15, 0.85);
        let (a1, a2, p1, p2) = (
            r.range(30.0, 70.0),
            r.range(10.0, 25.0),
            r.range(120.0, 200.0),
            r.range(40.0, 70.0),
        );
        let mut pts = Vec::new();
        let mut t = -10.0;
        while t <= wf + 10.0 {
            let o = a1 * (t / p1).sin() + a2 * (t / p2).sin();
            pts.push(if vertical {
                vec2(base * wf + o, t)
            } else {
                vec2(t, base * hf + o)
            });
            t += 8.0;
        }
        d.water_lines.push(Line {
            pts,
            w: r.range(26.0, 42.0),
        });
    }
    // a lake or two
    for _ in 0..r.below(3) {
        let c = vec2(r.range(100.0, wf - 100.0), r.range(100.0, hf - 100.0));
        if in_air(c.x, c.y, 80.0) {
            continue;
        }
        let (rx, ry) = (r.range(25.0, 60.0), r.range(20.0, 45.0));
        let poly: Poly = (0..24)
            .map(|k| {
                let a = k as f32 / 24.0 * std::f32::consts::TAU;
                let w = 1.0 + 0.15 * (a * 3.0 + seed as f32).sin();
                c + vec2(a.cos() * rx * w, a.sin() * ry * w)
            })
            .collect();
        d.water.push(poly);
    }

    // street grid with random spacing
    struct S {
        p: f32,
        w: f32,
        big: bool,
    }
    let mut xs = Vec::new();
    let mut ys = Vec::new();
    let (sp0, sp1) = (r.range(48.0, 60.0), r.range(80.0, 110.0));
    let avenue_every = 2 + r.below(3);
    let mut p = r.range(4.0, 12.0);
    let mut i = 0;
    while p < wf {
        let big = i % avenue_every == 1;
        let w = if big {
            r.range(16.0, 20.0)
        } else {
            r.range(10.0, 13.0)
        };
        xs.push(S { p, w, big });
        p += w + r.range(sp0, sp1);
        i += 1;
    }
    let mut p = r.range(4.0, 12.0);
    let mut i = 0;
    while p < hf {
        let big = i % avenue_every == 0;
        let w = if big {
            r.range(16.0, 20.0)
        } else {
            r.range(10.0, 13.0)
        };
        ys.push(S { p, w, big });
        p += w + r.range(sp0 * 0.9, sp1 * 0.9);
        i += 1;
    }
    let rank = |big: bool| if big { 4 } else { 1 };
    for s in &xs {
        let x = s.p + s.w / 2.0;
        if x > ax0 - 8.0 && x < ax1 + 8.0 {
            d.roads.push(Road {
                pts: vec![vec2(x, 0.0), vec2(x, ay0 - 14.0)],
                w: s.w,
                rank: rank(s.big),
            });
            d.roads.push(Road {
                pts: vec![vec2(x, ay1 + 14.0), vec2(x, hf)],
                w: s.w,
                rank: rank(s.big),
            });
        } else {
            d.roads.push(Road {
                pts: vec![vec2(x, 0.0), vec2(x, hf)],
                w: s.w,
                rank: rank(s.big),
            });
        }
    }
    for s in &ys {
        let y = s.p + s.w / 2.0;
        if y > ay0 - 8.0 && y < ay1 + 8.0 {
            d.roads.push(Road {
                pts: vec![vec2(0.0, y), vec2(ax0 - 14.0, y)],
                w: s.w,
                rank: rank(s.big),
            });
            d.roads.push(Road {
                pts: vec![vec2(ax1 + 14.0, y), vec2(wf, y)],
                w: s.w,
                rank: rank(s.big),
            });
        } else {
            d.roads.push(Road {
                pts: vec![vec2(0.0, y), vec2(wf, y)],
                w: s.w,
                rank: rank(s.big),
            });
        }
    }
    // ring road around the airport and access roads in to the apron
    d.roads.push(Road {
        pts: vec![
            vec2(ax0 - 12.0, ay0 - 12.0),
            vec2(ax1 + 12.0, ay0 - 12.0),
            vec2(ax1 + 12.0, ay1 + 12.0),
            vec2(ax0 - 12.0, ay1 + 12.0),
            vec2(ax0 - 12.0, ay0 - 12.0),
        ],
        w: 14.0,
        rank: 3,
    });
    let gates = 1 + r.below(3);
    let entries = [
        (vec2(ax0 - 12.0, hq.y), vec2(hq.x - 85.0, hq.y)),
        (vec2(ax1 + 12.0, hq.y), vec2(hq.x + 85.0, hq.y)),
        (vec2(hq.x, ay1 + 12.0), vec2(hq.x, hq.y + 60.0)),
        (vec2(hq.x, ay0 - 12.0), vec2(hq.x, hq.y - 60.0)),
    ];
    let start = r.below(4);
    for k in 0..gates {
        let (a, b) = entries[(start + k) % 4];
        d.roads.push(Road {
            pts: vec![a, b],
            w: 14.0,
            rank: 3,
        });
    }
    // diagonal boulevards
    for _ in 0..r.below(3) {
        let a = vec2(r.range(0.0, wf), 0.0);
        let b = vec2(r.range(0.0, wf), hf);
        // skip ones that would cut through the airport
        let mut hits = false;
        for k in 0..=40 {
            let q = a.lerp(b, k as f32 / 40.0);
            if in_air(q.x, q.y, 20.0) {
                hits = true;
                break;
            }
        }
        if !hits {
            d.roads.push(Road {
                pts: vec![a, b],
                w: r.range(14.0, 18.0),
                rank: 3,
            });
        }
    }

    // blocks and lots
    let downtown = vec2(r.range(wf * 0.2, wf * 0.8), r.range(hf * 0.2, hf * 0.8));
    let park_chance = r.range(0.06, 0.14);
    for i in 0..xs.len().saturating_sub(1) {
        for j in 0..ys.len().saturating_sub(1) {
            let (bx0, bx1) = (xs[i].p + xs[i].w, xs[i + 1].p);
            let (by0, by1) = (ys[j].p + ys[j].w, ys[j + 1].p);
            if bx1 - bx0 < 14.0 || by1 - by0 < 14.0 {
                continue;
            }
            let cx = (bx0 + bx1) / 2.0;
            let cy = (by0 + by1) / 2.0;
            if in_air(cx, cy, 40.0)
                || in_air(bx0, by0, 16.0)
                || in_air(bx1, by1, 16.0)
                || in_air(bx0, by1, 16.0)
                || in_air(bx1, by0, 16.0)
            {
                continue;
            }
            if r.f() < park_chance {
                d.parks
                    .push(rect(bx0 + 2.0, by0 + 2.0, bx1 - 2.0, by1 - 2.0));
                continue;
            }
            let dd = vec2(cx, cy).distance(downtown);
            let tall = (1.0 - dd / 520.0).max(0.0);
            let mut lots = Vec::new();
            split(
                &mut r,
                bx0 + 3.0,
                by0 + 3.0,
                bx1 - 3.0,
                by1 - 3.0,
                0,
                &mut lots,
            );
            for l in lots {
                let g = 1.0 + r.f() * 1.5;
                let (x0, y0, x1, y1) = (l[0] + g, l[1] + g, l[2] - g, l[3] - g);
                if x1 - x0 < 5.0 || y1 - y0 < 5.0 {
                    continue;
                }
                let tower = if r.f() < 0.6 { 1.0 } else { 0.4 };
                let h = 3.0 + r.f() * 3.0 + tall * tall * (10.0 + r.f() * 32.0) * tower;
                let k = r.f();
                let poly = if k < 0.18 && x1 - x0 > 12.0 && y1 - y0 > 12.0 {
                    let cx = x0 + (x1 - x0) * r.range(0.45, 0.65);
                    let cy = y0 + (y1 - y0) * r.range(0.45, 0.65);
                    vec![
                        vec2(x0, y0),
                        vec2(x1, y0),
                        vec2(x1, cy),
                        vec2(cx, cy),
                        vec2(cx, y1),
                        vec2(x0, y1),
                    ]
                } else if k < 0.26 && x1 - x0 > 10.0 {
                    let c = ((x1 - x0) / 4.0).min(4.0);
                    vec![
                        vec2(x0 + c, y0),
                        vec2(x1 - c, y0),
                        vec2(x1, y0 + c),
                        vec2(x1, y1 - c),
                        vec2(x1 - c, y1),
                        vec2(x0 + c, y1),
                        vec2(x0, y1 - c),
                        vec2(x0, y0 + c),
                    ]
                } else {
                    rect(x0, y0, x1, y1)
                };
                d.buildings.push(BDesc { poly, h });
            }
        }
    }
    d.note = "HQ set up at the city airport.".into();
    d
}

fn split(r: &mut Rng, x0: f32, y0: f32, x1: f32, y1: f32, dep: u32, out: &mut Vec<[f32; 4]>) {
    let (w, h) = (x1 - x0, y1 - y0);
    if (w < 32.0 && h < 32.0) || dep > 5 {
        out.push([x0, y0, x1, y1]);
        return;
    }
    if w >= h {
        let m = x0 + w * r.range(0.35, 0.65);
        split(r, x0, y0, m, y1, dep + 1, out);
        split(r, m, y0, x1, y1, dep + 1, out);
    } else {
        let m = y0 + h * r.range(0.35, 0.65);
        split(r, x0, y0, x1, m, dep + 1, out);
        split(r, x0, m, x1, y1, dep + 1, out);
    }
}

#[allow(dead_code)]
pub fn dummy(_: Vec2) {}

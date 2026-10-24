//! Real cities from OpenStreetMap. Everything runs on a background thread with the game's own HTTPS
//! (rustls), so it works without Windows' TLS settings or a local proxy.
//!
//! 1. Find the place (Nominatim, falling back to Photon).
//! 2. Find the nearest airport; the HQ goes there.
//! 3. Download streets, buildings, water, parks and the airport in 2×2 tiles, trying several Overpass
//!    mirrors in turn, and project it all onto the map grid at 4 m per cell.

use crate::world::*;
use macroquad::prelude::{vec2, Vec2};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

pub enum OsmMsg {
    Status(String),
    Done(Box<Desc>),
    Fail(String),
}

pub struct Loader {
    pub rx: Receiver<OsmMsg>,
}

const MPC: f32 = 4.0;
const MIRRORS: [&str; 4] = [
    "https://overpass-api.de/api/interpreter",
    "https://overpass.private.coffee/api/interpreter",
    "https://maps.mail.ru/osm/tools/overpass/api/interpreter",
    "https://overpass.kumi.systems/api/interpreter",
];

/// Loads a city in the background; the HQ goes at an airport up to `air_m` metres away.
pub fn start(query: String, air_m: u32) -> Loader {
    let (tx, rx) = channel();
    std::thread::spawn(move || match load(&query, &tx, 2, air_m) {
        Ok(d) => {
            let _ = tx.send(OsmMsg::Done(Box::new(d)));
        }
        Err(e) => {
            let _ = tx.send(OsmMsg::Fail(e));
        }
    });
    Loader { rx }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(180))
        .user_agent(concat!(
            "PixelHorde/",
            env!("CARGO_PKG_VERSION"),
            " (github.com/Littlebutta/Horde-defence-game)"
        ))
        .build()
}

fn enc(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                o.push(b as char)
            }
            b' ' => o.push('+'),
            _ => o.push_str(&format!("%{:02X}", b)),
        }
    }
    o
}

fn geocode(a: &ureq::Agent, q: &str) -> Result<(f64, f64, String), String> {
    let url = format!(
        "https://nominatim.openstreetmap.org/search?format=json&limit=1&q={}",
        enc(q)
    );
    if let Ok(r) = a.get(&url).call() {
        if let Ok(j) = r.into_json::<Value>() {
            if let Some(f) = j.as_array().and_then(|v| v.first()) {
                let lat = f["lat"].as_str().and_then(|s| s.parse().ok());
                let lon = f["lon"].as_str().and_then(|s| s.parse().ok());
                if let (Some(lat), Some(lon)) = (lat, lon) {
                    let name = f["display_name"]
                        .as_str()
                        .unwrap_or(q)
                        .split(',')
                        .take(2)
                        .collect::<Vec<_>>()
                        .join(",");
                    return Ok((lat, lon, name));
                }
            }
        }
    }
    let url = format!("https://photon.komoot.io/api/?limit=1&q={}", enc(q));
    let j: Value = a
        .get(&url)
        .call()
        .map_err(|e| format!("could not look up the place ({})", short(&e.to_string())))?
        .into_json()
        .map_err(|e| e.to_string())?;
    let f = &j["features"][0];
    let c = &f["geometry"]["coordinates"];
    match (c[1].as_f64(), c[0].as_f64()) {
        (Some(lat), Some(lon)) => Ok((
            lat,
            lon,
            f["properties"]["name"].as_str().unwrap_or(q).to_string(),
        )),
        _ => Err(format!("no place called \"{}\" was found", q)),
    }
}

fn overpass(a: &ureq::Agent, q: &str, tx: &Sender<OsmMsg>, what: &str) -> Result<Value, String> {
    let mut last = String::new();
    for round in 0..2 {
        for m in MIRRORS {
            let host = m.split('/').nth(2).unwrap_or(m);
            let _ = tx.send(OsmMsg::Status(format!(
                "{} from {}{}",
                what,
                host,
                if round > 0 { " (retry)" } else { "" }
            )));
            match a.post(m).send_form(&[("data", q)]) {
                Ok(r) => match serde_json::from_reader::<_, Value>(r.into_reader()) {
                    Ok(v) if v["elements"].is_array() => return Ok(v),
                    Ok(_) => last = format!("{}: unexpected reply", host),
                    Err(e) => last = format!("{}: {}", host, short(&e.to_string())),
                },
                Err(e) => last = format!("{}: {}", host, short(&e.to_string())),
            }
        }
        std::thread::sleep(Duration::from_secs(3));
    }
    Err(format!("map download failed ({})", last))
}

fn short(s: &str) -> String {
    s.chars().take(80).collect()
}

struct Proj {
    lat0: f64,
    lon0: f64,
    kx: f64,
    ky: f64,
}
impl Proj {
    fn p(&self, lat: f64, lon: f64) -> Vec2 {
        vec2(
            ((lon - self.lon0) * self.kx) as f32,
            ((self.lat0 - lat) * self.ky) as f32,
        )
    }
}

fn geom(e: &Value, pr: &Proj) -> Vec<Vec2> {
    e["geometry"]
        .as_array()
        .map(|g| {
            g.iter()
                .filter_map(|n| Some(pr.p(n["lat"].as_f64()?, n["lon"].as_f64()?)))
                .collect()
        })
        .unwrap_or_default()
}

fn closed(mut v: Vec<Vec2>) -> Option<Poly> {
    if v.len() < 4 || v[0].distance(*v.last().unwrap()) > 0.01 {
        return None;
    }
    v.pop();
    Some(v)
}

/// Joins the outer ways of a multipolygon into closed rings.
fn rings(e: &Value, pr: &Proj) -> Vec<Poly> {
    let mut parts: Vec<Vec<Vec2>> = Vec::new();
    for m in e["members"].as_array().into_iter().flatten() {
        if m["type"] != "way" || m["role"].as_str().unwrap_or("outer") == "inner" {
            continue;
        }
        let g = geom(m, pr);
        if g.len() >= 2 {
            parts.push(g);
        }
    }
    let mut out = Vec::new();
    while let Some(mut ring) = parts.pop() {
        loop {
            if ring.len() > 3 && ring[0].distance(*ring.last().unwrap()) < 0.01 {
                break;
            }
            let end = *ring.last().unwrap();
            let Some(k) = parts
                .iter()
                .position(|p| p[0].distance(end) < 0.01 || p.last().unwrap().distance(end) < 0.01)
            else {
                break;
            };
            let mut p = parts.swap_remove(k);
            if p[0].distance(end) >= 0.01 {
                p.reverse();
            }
            ring.extend(p.into_iter().skip(1));
        }
        if let Some(r) = closed(ring) {
            out.push(r);
        }
    }
    out
}

fn num(s: &str) -> Option<f32> {
    s.trim()
        .trim_end_matches('m')
        .trim()
        .replace(',', ".")
        .parse()
        .ok()
}

fn road_style(h: &str) -> Option<(f32, u8)> {
    Some(match h {
        "motorway" | "trunk" => (6.0, 4),
        "primary" => (5.0, 4),
        "secondary" => (4.5, 3),
        "tertiary" => (4.0, 3),
        "motorway_link" | "trunk_link" | "primary_link" | "secondary_link" | "tertiary_link" => {
            (3.5, 2)
        }
        "residential" | "unclassified" | "road" => (3.5, 1),
        "living_street" | "pedestrian" => (3.0, 1),
        "service" => (2.5, 0),
        _ => return None,
    })
}

/// Downloads a city in `tiles`×`tiles` pieces (more pieces = smaller Overpass queries). The HQ goes
/// at the nearest airport within `air_m` metres, else in the middle of town.
pub fn load(query: &str, tx: &Sender<OsmMsg>, tiles: usize, air_m: u32) -> Result<Desc, String> {
    let a = agent();
    let _ = tx.send(OsmMsg::Status(format!("Looking up {}", query)));
    let (lat, lon, name) = geocode(&a, query)?;

    // nearest airport
    let _ = tx.send(OsmMsg::Status("Finding the nearest airport".into()));
    let q = format!("[out:json][timeout:60];(way[\"aeroway\"=\"aerodrome\"](around:{air_m},{lat},{lon});relation[\"aeroway\"=\"aerodrome\"](around:{air_m},{lat},{lon}););out tags center;");
    let mut air: Option<(f64, f64, String)> = None;
    if let Ok(j) = overpass(&a, &q, tx, "Finding the nearest airport") {
        let mut best = f64::MAX;
        for e in j["elements"].as_array().into_iter().flatten() {
            let (Some(alat), Some(alon)) =
                (e["center"]["lat"].as_f64(), e["center"]["lon"].as_f64())
            else {
                continue;
            };
            let t = &e["tags"];
            let mut d = ((alat - lat) * 110.5).hypot((alon - lon) * 111.3 * lat.to_radians().cos());
            // real airports beat airstrips and heliports
            if t["iata"].is_null() && t["icao"].is_null() {
                d += 12.0;
            }
            if t["aerodrome:type"] == "military" || t["aerodrome"] == "private" {
                d += 6.0;
            }
            if d < best {
                best = d;
                air = Some((
                    alat,
                    alon,
                    t["name"].as_str().unwrap_or("the airport").to_string(),
                ));
            }
        }
    }
    // centre the map so the airport and as much city as possible are both on it
    let half = W as f64 * MPC as f64 / 2.0;
    let (clat, clon) = match &air {
        Some((alat, alon, _)) => {
            let dy = (lat - alat) * 110_540.0;
            let dx = (lon - alon) * 111_320.0 * alat.to_radians().cos();
            let dist = dx.hypot(dy).max(1.0);
            let k = (dist.min(half * 0.62)) / dist;
            (alat + (lat - alat) * k, alon + (lon - alon) * k)
        }
        None => (lat, lon),
    };
    let kx = 111_320.0 * clat.to_radians().cos() / MPC as f64;
    let ky = 110_540.0 / MPC as f64;
    let pr = Proj {
        lat0: clat + half / 110_540.0,
        lon0: clon - half / (111_320.0 * clat.to_radians().cos()),
        kx,
        ky,
    };
    let (s, w) = (clat - half / 110_540.0, pr.lon0);
    let (n, e) = (pr.lat0, clon + half / (111_320.0 * clat.to_radians().cos()));

    let mut d = Desc {
        name: name.clone(),
        mpc: MPC,
        ..Default::default()
    };
    let mut seen: HashSet<(bool, u64)> = HashSet::new();
    let mut aerodromes: Vec<Poly> = Vec::new();
    let nt = tiles.max(1);
    for ty in 0..nt {
        for tx_ in 0..nt {
            let ts = s + (n - s) * ty as f64 / nt as f64;
            let tn = s + (n - s) * (ty + 1) as f64 / nt as f64;
            let tw = w + (e - w) * tx_ as f64 / nt as f64;
            let te = w + (e - w) * (tx_ + 1) as f64 / nt as f64;
            let q = format!(
                "[out:json][timeout:170][bbox:{ts:.6},{tw:.6},{tn:.6},{te:.6}];(way[\"highway\"];way[\"building\"];way[\"natural\"=\"water\"];relation[\"natural\"=\"water\"];way[\"waterway\"~\"^(river|canal)$\"];way[\"landuse\"~\"^(grass|forest|recreation_ground|cemetery|meadow|village_green)$\"];way[\"leisure\"~\"^(park|garden|pitch|golf_course)$\"];way[\"aeroway\"~\"^(aerodrome|apron|runway|taxiway)$\"];relation[\"aeroway\"=\"aerodrome\"];);out geom qt;"
            );
            let k = ty * nt + tx_ + 1;
            let j = overpass(&a, &q, tx, &format!("Downloading map tile {}/{}", k, nt * nt))?;
            for el in j["elements"].as_array().into_iter().flatten() {
                let id = el["id"].as_u64().unwrap_or(0);
                let rel = el["type"] == "relation";
                if !seen.insert((rel, id)) {
                    continue;
                }
                let t = &el["tags"];
                if rel {
                    if t["natural"] == "water" {
                        d.water.extend(rings(el, &pr));
                    } else if t["aeroway"] == "aerodrome" {
                        aerodromes.extend(rings(el, &pr));
                    }
                    continue;
                }
                let g = geom(el, &pr);
                if g.len() < 2 {
                    continue;
                }
                if let Some(h) = t["highway"].as_str() {
                    if t["area"] == "yes" {
                        continue;
                    }
                    if let Some((wd, rank)) = road_style(h) {
                        d.roads.push(Road {
                            pts: g,
                            w: wd,
                            rank,
                        });
                    }
                } else if !t["building"].is_null() {
                    if let Some(p) = closed(g) {
                        let m = t["height"]
                            .as_str()
                            .and_then(num)
                            .or_else(|| {
                                t["building:levels"].as_str().and_then(num).map(|l| l * 3.2)
                            })
                            .unwrap_or(5.0 + (id % 7) as f32 * 1.5);
                        d.buildings.push(BDesc {
                            poly: p,
                            h: (m * 0.5).clamp(2.0, 60.0),
                        });
                    }
                } else if t["natural"] == "water" {
                    if let Some(p) = closed(g) {
                        d.water.push(p);
                    }
                } else if let Some(ww) = t["waterway"].as_str() {
                    d.water_lines.push(Line {
                        pts: g,
                        w: if ww == "river" { 9.0 } else { 4.0 },
                    });
                } else if let Some(ae) = t["aeroway"].as_str() {
                    match ae {
                        "aerodrome" => aerodromes.extend(closed(g)),
                        "apron" => d.aprons.extend(closed(g)),
                        "runway" => d.runways.push(Line { pts: g, w: 12.0 }),
                        _ => d.runways.push(Line { pts: g, w: 4.0 }),
                    }
                } else if let Some(p) = closed(g) {
                    d.parks.push(p);
                }
            }
        }
    }
    if d.roads.is_empty() {
        return Err("no streets came back for that area".into());
    }
    // HQ: the airport (its biggest apron if it has one), otherwise the middle of the map
    let mid = vec2(W as f32 / 2.0, H as f32 / 2.0);
    let target = match &air {
        Some((alat, alon, _)) => pr.p(*alat, *alon),
        None => mid,
    };
    let inside =
        |p: Vec2| p.x > 40.0 && p.y > 40.0 && p.x < W as f32 - 40.0 && p.y < H as f32 - 40.0;
    let airport = aerodromes
        .into_iter()
        .filter(|p| inside(centroid(p)))
        .min_by(|a, b| {
            centroid(a)
                .distance(target)
                .partial_cmp(&centroid(b).distance(target))
                .unwrap()
        });
    let mut hq = target;
    if let Some(ap) = &airport {
        let mut best = 0.0;
        for a in &d.aprons {
            let c = centroid(a);
            let ar = poly_area(a).abs();
            if ar > best && point_in(ap, c) {
                best = ar;
                hq = c;
            }
        }
        if best == 0.0 {
            hq = centroid(ap);
        }
    }
    if !inside(hq) {
        hq = mid;
    }
    d.airport = airport;
    d.hq = hq;
    d.note = match &air {
        Some((_, _, an)) if d.airport.is_some() => format!("HQ set up at {}.", an),
        _ => "No airport nearby, so the HQ is in the middle of town.".into(),
    };
    let mut by_name: HashMap<&str, usize> = HashMap::new();
    by_name.insert("roads", d.roads.len());
    let _ = tx.send(OsmMsg::Status(format!(
        "Building {} ({} streets, {} buildings)",
        name,
        d.roads.len(),
        d.buildings.len()
    )));
    Ok(d)
}

fn point_in(poly: &[Vec2], p: Vec2) -> bool {
    let mut inside = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + n - 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

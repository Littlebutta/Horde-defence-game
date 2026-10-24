//! Everything the game saves lives in a `data` folder next to PixelHorde.exe: settings and every
//! real city that has been downloaded, so it can be played again instantly without the internet.

use crate::world::*;
use macroquad::prelude::{vec2, Vec2};
use std::path::PathBuf;

pub fn data_dir() -> PathBuf {
    let base = std::env::current_exe().ok().and_then(|p| p.parent().map(|p| p.to_path_buf())).unwrap_or_else(|| PathBuf::from("."));
    let d = base.join("data");
    let _ = std::fs::create_dir_all(d.join("maps"));
    d
}

// ------------------------------------------------------------------------------------------ settings

#[derive(Clone)]
pub struct Settings {
    pub dark: bool,
    pub fac: usize,
    pub diff: usize,
    pub real: bool,
    pub city: String,
}
impl Default for Settings {
    fn default() -> Self {
        Settings { dark: true, fac: 0, diff: 1, real: false, city: String::new() }
    }
}

pub fn load_settings() -> Settings {
    let mut s = Settings::default();
    let Ok(t) = std::fs::read_to_string(data_dir().join("settings.txt")) else { return s };
    for line in t.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        let v = v.trim();
        match k.trim() {
            "dark" => s.dark = v == "1",
            "faction" => s.fac = v.parse().unwrap_or(0).min(2),
            "difficulty" => s.diff = v.parse().unwrap_or(1).min(3),
            "real" => s.real = v == "1",
            "city" => s.city = v.to_string(),
            _ => {}
        }
    }
    s
}

pub fn save_settings(s: &Settings) {
    let t = format!("dark={}\nfaction={}\ndifficulty={}\nreal={}\ncity={}\n", s.dark as u8, s.fac, s.diff, s.real as u8, s.city.replace('\n', " "));
    let _ = std::fs::write(data_dir().join("settings.txt"), t);
}

// ------------------------------------------------------------------------------------------ maps

const MAGIC: &[u8; 8] = b"PHMAP\0\0\x01";

pub struct SavedMap {
    pub query: String,
    pub name: String,
    pub path: PathBuf,
    pub when: u64,
}

fn slug(s: &str) -> String {
    let mut o: String = s.trim().to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { '-' }).collect();
    while o.contains("--") {
        o = o.replace("--", "-");
    }
    o.trim_matches('-').chars().take(60).collect()
}

pub struct Wr(pub Vec<u8>);
impl Wr {
    pub fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub fn str(&mut self, s: &str) {
        self.u32(s.len() as u32);
        self.0.extend_from_slice(s.as_bytes());
    }
    pub fn pts(&mut self, p: &[Vec2]) {
        self.u32(p.len() as u32);
        for q in p {
            self.f32(q.x);
            self.f32(q.y);
        }
    }
}
pub struct Rd<'a>(pub &'a [u8], pub usize);
impl Rd<'_> {
    pub fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.0.get(self.1..self.1 + n)?;
        self.1 += n;
        Some(s)
    }
    pub fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    pub fn f32(&mut self) -> Option<f32> {
        Some(f32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    pub fn str(&mut self) -> Option<String> {
        let n = self.u32()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).ok()
    }
    pub fn pts(&mut self) -> Option<Vec<Vec2>> {
        let n = self.u32()? as usize;
        if n > 50_000_000 {
            return None;
        }
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push(vec2(self.f32()?, self.f32()?));
        }
        Some(v)
    }
    pub fn list<T>(&mut self, mut f: impl FnMut(&mut Self) -> Option<T>) -> Option<Vec<T>> {
        let n = self.u32()? as usize;
        let mut v = Vec::with_capacity(n.min(1_000_000));
        for _ in 0..n {
            v.push(f(self)?);
        }
        Some(v)
    }
}

pub fn encode(query: &str, d: &Desc) -> Vec<u8> {
    let mut w = Wr(MAGIC.to_vec());
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    w.u32((now >> 32) as u32);
    w.u32(now as u32);
    w.str(query);
    w.str(&d.name);
    w.str(&d.note);
    w.f32(d.mpc);
    w.f32(d.hq.x);
    w.f32(d.hq.y);
    w.u32(d.roads.len() as u32);
    for r in &d.roads {
        w.pts(&r.pts);
        w.f32(r.w);
        w.u32(r.rank as u32);
    }
    w.u32(d.buildings.len() as u32);
    for b in &d.buildings {
        w.pts(&b.poly);
        w.f32(b.h);
    }
    for polys in [&d.water, &d.parks, &d.aprons] {
        w.u32(polys.len() as u32);
        for p in polys {
            w.pts(p);
        }
    }
    for lines in [&d.water_lines, &d.runways] {
        w.u32(lines.len() as u32);
        for l in lines {
            w.pts(&l.pts);
            w.f32(l.w);
        }
    }
    match &d.airport {
        Some(a) => {
            w.u32(1);
            w.pts(a);
        }
        None => w.u32(0),
    }
    w.0
}

fn header(b: &[u8]) -> Option<(u64, String, String, Rd<'_>)> {
    if b.get(..8)? != MAGIC {
        return None;
    }
    let mut r = Rd(b, 8);
    let when = ((r.u32()? as u64) << 32) | r.u32()? as u64;
    let query = r.str()?;
    let name = r.str()?;
    Some((when, query, name, r))
}

pub fn decode(b: &[u8]) -> Option<Desc> {
    let (_, _, name, mut r) = header(b)?;
    let note = r.str()?;
    let mpc = r.f32()?;
    let hq = vec2(r.f32()?, r.f32()?);
    let roads = r.list(|r| Some(Road { pts: r.pts()?, w: r.f32()?, rank: r.u32()? as u8 }))?;
    let buildings = r.list(|r| Some(BDesc { poly: r.pts()?, h: r.f32()? }))?;
    let water = r.list(|r| r.pts())?;
    let parks = r.list(|r| r.pts())?;
    let aprons = r.list(|r| r.pts())?;
    let water_lines = r.list(|r| Some(Line { pts: r.pts()?, w: r.f32()? }))?;
    let runways = r.list(|r| Some(Line { pts: r.pts()?, w: r.f32()? }))?;
    let airport = if r.u32()? == 1 { Some(r.pts()?) } else { None };
    Some(Desc { name, note, mpc, roads, buildings, water, water_lines, parks, airport, aprons, runways, hq })
}

pub fn save_map(query: &str, d: &Desc) {
    let p = data_dir().join("maps").join(format!("{}.phmap", slug(query)));
    let _ = std::fs::write(p, encode(query, d));
}

/// Downloaded cities, most recently saved first.
pub fn list_maps() -> Vec<SavedMap> {
    let mut v = Vec::new();
    let Ok(rd) = std::fs::read_dir(data_dir().join("maps")) else { return v };
    for e in rd.flatten() {
        let path = e.path();
        if path.extension().and_then(|x| x.to_str()) != Some("phmap") {
            continue;
        }
        // only the header is needed for the list
        let Ok(mut f) = std::fs::File::open(&path) else { continue };
        let mut buf = vec![0u8; 4096];
        use std::io::Read;
        let n = f.read(&mut buf).unwrap_or(0);
        buf.truncate(n);
        if let Some((when, query, name, _)) = header(&buf) {
            v.push(SavedMap { query, name, path, when });
        }
    }
    v.sort_by(|a, b| b.when.cmp(&a.when));
    v
}

pub fn load_map(path: &std::path::Path) -> Option<Desc> {
    decode(&std::fs::read(path).ok()?)
}

pub fn find_map(query: &str) -> Option<PathBuf> {
    let p = data_dir().join("maps").join(format!("{}.phmap", slug(query)));
    p.exists().then_some(p)
}

pub fn delete_map(path: &std::path::Path) {
    let _ = std::fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn map_roundtrip() {
        let d = crate::citygen::generate(5);
        let b = encode("Test town", &d);
        let e = decode(&b).unwrap();
        assert_eq!(e.roads.len(), d.roads.len());
        assert_eq!(e.buildings.len(), d.buildings.len());
        assert_eq!(e.hq, d.hq);
        assert_eq!(e.airport.is_some(), d.airport.is_some());
        assert_eq!(header(&b).unwrap().1, "Test town");
    }
}

//! The world campaign: hold ten of the world's biggest cities in a row, smallest to largest.
//!
//! Troops that survive a city march on to the next one; scrap and buildings don't. Civilians flown
//! out are the campaign's currency: between cities they unlock the same research the lab sells for
//! scrap in a normal game, or recruit more troops.
//!
//! The ten maps are downloaded from OpenStreetMap once, by the `cities` GitHub Actions workflow
//! (`PH_FETCH`), and published as the `cities` release. The game fetches that pack from GitHub in the
//! background when it starts, so a campaign city never waits on OpenStreetMap. If a map is missing
//! anyway, it falls back to downloading the city live.

use crate::defs::*;
use crate::game::Game;
use crate::storage::{self, data_dir};
use crate::world::*;
use macroquad::prelude::vec2;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub struct City {
    pub name: &'static str,
    /// what the map loader looks up
    pub query: &'static str,
    /// map file name, plain ASCII so it works as a release asset name
    pub file: &'static str,
}

/// The ten largest urban areas (UN World Urbanization Prospects), played smallest first.
pub const CITIES: [City; 10] = [
    City { name: "Osaka", query: "Osaka, Japan", file: "osaka" },
    City { name: "Dhaka", query: "Dhaka, Bangladesh", file: "dhaka" },
    City { name: "Beijing", query: "Beijing, China", file: "beijing" },
    City { name: "Mumbai", query: "Mumbai, India", file: "mumbai" },
    City { name: "Cairo", query: "Cairo, Egypt", file: "cairo" },
    City { name: "Mexico City", query: "Mexico City, Mexico", file: "mexico-city" },
    City { name: "São Paulo", query: "São Paulo, Brazil", file: "sao-paulo" },
    City { name: "Shanghai", query: "Shanghai, China", file: "shanghai" },
    City { name: "Delhi", query: "Delhi, India", file: "delhi" },
    City { name: "Tokyo", query: "Tokyo, Japan", file: "tokyo" },
];

/// Civilians it costs to recruit troops between cities.
pub fn recruit_cost(u: U) -> u32 {
    match u {
        U::Rifle => 30,
        U::Tank => 100,
    }
}

/// How much bigger the horde is in each city than on the chosen difficulty: ×1.3 per city.
pub fn horde_mult(level: usize) -> f32 {
    1.3f32.powi(level as i32)
}

/// Seconds to hold a city before the evac: 7 minutes, plus 30 s per city.
pub fn hold_secs(level: usize) -> f32 {
    (7.0 + 0.5 * level as f32) * 60.0
}

#[derive(Clone)]
pub struct Campaign {
    pub fac: usize,
    pub diff: usize,
    /// the next city to fight in; CITIES.len() once the campaign is won
    pub level: usize,
    /// civilians to spend
    pub points: u32,
    pub research: Vec<R>,
    /// surviving troops: (kind, soldiers) per squad
    pub units: Vec<(U, u32)>,
    pub flown: u32,
    pub kills: u64,
}

impl Campaign {
    pub fn new(fac: usize, diff: usize) -> Campaign {
        Campaign { fac, diff, level: 0, points: 0, research: Vec::new(), units: Vec::new(), flown: 0, kills: 0 }
    }
    pub fn finished(&self) -> bool {
        self.level >= CITIES.len()
    }
    pub fn city(&self) -> &'static City {
        &CITIES[self.level.min(CITIES.len() - 1)]
    }
    pub fn has(&self, r: R) -> bool {
        self.research.contains(&r)
    }
    pub fn buy_research(&mut self, r: R) -> bool {
        let c = rdef(r).civs;
        if self.has(r) || self.points < c {
            return false;
        }
        self.points -= c;
        self.research.push(r);
        true
    }
    pub fn recruit(&mut self, u: U) -> bool {
        let c = recruit_cost(u);
        if self.points < c {
            return false;
        }
        self.points -= c;
        self.units.push((u, udef(u).n as u32));
        true
    }
    /// (squads, soldiers) of one kind of troops.
    pub fn army(&self, u: U) -> (usize, u32) {
        let v: Vec<u32> = self.units.iter().filter(|x| x.0 == u).map(|x| x.1).collect();
        (v.len(), v.iter().sum())
    }

    /// Sets a fresh game up as this campaign's next city: research, the hold timer and the army.
    pub fn apply(&self, g: &mut Game) {
        g.campaign = true;
        g.goal = hold_secs(self.level);
        for r in &self.research {
            g.complete_research(*r);
        }
        let hq = g.world.hq;
        for (i, (u, n)) in self.units.iter().enumerate() {
            // fan the squads out around the HQ
            let ang = i as f32 * 2.399_963;
            let mut placed = false;
            for k in 0..6 {
                let r = 30.0 + (i / 10) as f32 * 6.0 + k as f32 * 8.0;
                let at = hq + vec2(libm::cosf(ang), libm::sinf(ang)) * r;
                if g.spawn_group(*u, *n as usize, at) {
                    placed = true;
                    break;
                }
            }
            if !placed {
                g.spawn_group(*u, *n as usize, hq + vec2(0.0, 20.0));
            }
        }
    }

    /// After a city is held: bank the civilians, keep the survivors and move on.
    pub fn harvest(&mut self, g: &Game) {
        self.points += g.extracted;
        self.flown += g.extracted;
        self.kills += g.kills;
        self.units = g
            .groups
            .iter()
            .filter(|gr| !gr.dead)
            .map(|gr| (gr.kind, gr.m.iter().filter(|m| m.alive).count() as u32))
            .filter(|x| x.1 > 0)
            .collect();
        self.level += 1;
    }
}

// ------------------------------------------------------------------------------------------ saving

fn save_path() -> PathBuf {
    data_dir().join("campaign.txt")
}

pub fn load() -> Option<Campaign> {
    Some(from_text(&std::fs::read_to_string(save_path()).ok()?))
}

pub fn save(c: &Campaign) {
    let _ = std::fs::write(save_path(), to_text(c));
}

fn from_text(t: &str) -> Campaign {
    let mut c = Campaign::new(0, 1);
    for line in t.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        let v = v.trim();
        match k.trim() {
            "faction" => c.fac = v.parse().unwrap_or(0).min(FACTIONS.len() - 1),
            "difficulty" => c.diff = v.parse().unwrap_or(1).min(3),
            "level" => c.level = v.parse().unwrap_or(0).min(CITIES.len()),
            "points" => c.points = v.parse().unwrap_or(0),
            "flown" => c.flown = v.parse().unwrap_or(0),
            "kills" => c.kills = v.parse().unwrap_or(0),
            "research" => {
                c.research = v.split(',').filter_map(|x| x.trim().parse::<usize>().ok()).filter_map(|i| RESEARCH.get(i).copied()).collect();
            }
            "units" => {
                c.units = v
                    .split(',')
                    .filter_map(|x| {
                        let x = x.trim();
                        let u = match x.get(..1)? {
                            "r" => U::Rifle,
                            "t" => U::Tank,
                            _ => return None,
                        };
                        Some((u, x[1..].parse().ok()?))
                    })
                    .collect();
            }
            _ => {}
        }
    }
    c
}

fn to_text(c: &Campaign) -> String {
    let research: Vec<String> = c.research.iter().filter_map(|r| RESEARCH.iter().position(|x| x == r)).map(|i| i.to_string()).collect();
    let units: Vec<String> = c.units.iter().map(|(u, n)| format!("{}{}", if *u == U::Rifle { 'r' } else { 't' }, n)).collect();
    format!(
        "faction={}\ndifficulty={}\nlevel={}\npoints={}\nflown={}\nkills={}\nresearch={}\nunits={}\n",
        c.fac,
        c.diff,
        c.level,
        c.points,
        c.flown,
        c.kills,
        research.join(","),
        units.join(",")
    )
}

// ------------------------------------------------------------------------------------------ maps

fn map_dir() -> PathBuf {
    let d = data_dir().join("campaign");
    let _ = std::fs::create_dir_all(&d);
    d
}
/// Megacity airports are mostly far out of town, so the campaign holds the city itself: only an
/// airport close enough to share the map with downtown gets the HQ.
pub const AIRPORT_M: u32 = 4_000;
/// Bumped when the maps are made differently, so old downloads are fetched again.
/// v3: islands in rivers and lakes are land.
const PACK: u32 = 3;

pub fn file_name(c: &City) -> String {
    format!("{}-v{}.phmap", c.file, PACK)
}
pub fn map_path(c: &City) -> PathBuf {
    map_dir().join(file_name(c))
}
/// The previous pack's map, still played until the new one has downloaded.
fn old_path(c: &City) -> PathBuf {
    map_dir().join(format!("{}-v{}.phmap", c.file, PACK - 1))
}
pub fn have_map(c: &City) -> bool {
    map_path(c).exists() || old_path(c).exists()
}
pub fn load_map(c: &City) -> Option<Desc> {
    storage::load_map(&map_path(c)).or_else(|| storage::load_map(&old_path(c)))
}
pub fn save_map(c: &City, d: &Desc) {
    let _ = std::fs::write(map_path(c), storage::encode(c.query, d));
}

#[derive(Clone, PartialEq)]
pub enum PackState {
    Fetching(usize),
    Done,
    Failed(String),
}

/// Downloads the pre-built city pack from the `cities` GitHub release in the background.
pub struct Pack {
    pub state: Arc<Mutex<PackState>>,
}

impl Pack {
    pub fn start() -> Pack {
        let state = Arc::new(Mutex::new(PackState::Fetching(0)));
        let st = state.clone();
        std::thread::spawn(move || {
            let a = ureq::AgentBuilder::new()
                .timeout(std::time::Duration::from_secs(120))
                .user_agent(concat!("PixelHorde/", env!("CARGO_PKG_VERSION")))
                .build();
            let mut err = None;
            for (i, c) in CITIES.iter().enumerate() {
                if map_path(c).exists() {
                    continue;
                }
                *st.lock().unwrap() = PackState::Fetching(i);
                let url = format!("https://github.com/{}/releases/download/cities/{}.z", crate::update::REPO, file_name(c));
                let got = a.get(&url).call().map_err(|e| e.to_string()).and_then(|r| {
                    let mut z = Vec::new();
                    r.into_reader().read_to_end(&mut z).map_err(|e| e.to_string())?;
                    let b = miniz_oxide::inflate::decompress_to_vec(&z).map_err(|_| "damaged download".to_string())?;
                    storage::decode(&b).ok_or("damaged map")?;
                    Ok(b)
                });
                match got {
                    Ok(b) => {
                        let _ = std::fs::write(map_path(c), b);
                    }
                    // an older map still plays; the new one is tried again next time
                    Err(_) if old_path(c).exists() => {}
                    Err(e) => err = Some(e.chars().take(60).collect::<String>()),
                }
            }
            *st.lock().unwrap() = match err {
                Some(e) => PackState::Failed(e),
                None => PackState::Done,
            };
        });
        Pack { state }
    }
    pub fn get(&self) -> PackState {
        self.state.lock().unwrap().clone()
    }
    pub fn busy(&self) -> bool {
        matches!(self.get(), PackState::Fetching(_))
    }
    pub fn ready(&self) -> usize {
        CITIES.iter().filter(|c| have_map(c)).count()
    }
}

/// `PH_FETCH=<dir>`: downloads every campaign city from OpenStreetMap into `<dir>` as compressed
/// maps for the `cities` release. Run by the GitHub workflow, not by players. Returns false if any
/// city failed.
pub fn fetch_all(out: &Path) -> bool {
    let _ = std::fs::create_dir_all(out);
    let only: Option<String> = std::env::var("PH_FETCH_ONLY").ok().filter(|s| !s.trim().is_empty());
    let mut ok = true;
    for c in CITIES.iter() {
        if only.as_ref().is_some_and(|o| !o.split(',').any(|x| x.trim().eq_ignore_ascii_case(c.name))) {
            continue;
        }
        let mut done = false;
        for attempt in 1..=3 {
            eprintln!("== {} (attempt {})", c.query, attempt);
            let (tx, rx) = std::sync::mpsc::channel();
            let printer = std::thread::spawn(move || {
                for m in rx {
                    if let crate::osm::OsmMsg::Status(s) = m {
                        eprintln!("   {}", s);
                    }
                }
            });
            let r = crate::osm::load(c.query, &tx, 3, AIRPORT_M).map(|mut d| {
                d.name = c.name.to_string();
                if d.airport.is_none() {
                    d.note = "HQ set up in the middle of town.".into();
                }
                d
            });
            drop(tx);
            let _ = printer.join();
            match r {
                Ok(d) => {
                    let b = storage::encode(c.query, &d);
                    let z = miniz_oxide::deflate::compress_to_vec(&b, 9);
                    let p = out.join(format!("{}.z", file_name(c)));
                    if std::fs::write(&p, &z).is_ok() {
                        eprintln!(
                            "   saved {} ({} streets, {} buildings, {:.1} MB, {:.1} MB packed). {}",
                            p.display(),
                            d.roads.len(),
                            d.buildings.len(),
                            b.len() as f64 / 1e6,
                            z.len() as f64 / 1e6,
                            d.note
                        );
                        done = true;
                        break;
                    }
                }
                Err(e) => eprintln!("   failed: {}", e),
            }
            std::thread::sleep(std::time::Duration::from_secs(20));
        }
        ok &= done;
    }
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    fn veteran() -> Campaign {
        let mut c = Campaign::new(2, 1);
        c.level = 3;
        c.research = vec![R::Armour, R::Barrels, R::Rescue];
        c.units = vec![(U::Rifle, 6), (U::Tank, 1), (U::Rifle, 2)];
        c
    }

    /// Carried troops arrive, survivors carry on, and a campaign game stays deterministic.
    #[test]
    fn carry_over() {
        let c = veteran();
        let mk = || {
            let mut g = Game::new(crate::citygen::generate(3), c.fac, horde_mult(c.level), 1003);
            c.apply(&mut g);
            g
        };
        let (mut a, mut b) = (mk(), mk());
        assert_eq!(a.groups.len(), 3);
        assert!(a.research.armour && a.research.barrels && a.research.rescue && !a.research.nuke);
        assert!(!a.can_research(R::Nuke));
        for _ in 0..400 {
            a.tick();
            b.tick();
        }
        assert_eq!(a.checksum(), b.checksum());
        let mut next = c.clone();
        a.extracted = 77;
        next.harvest(&a);
        assert_eq!(next.level, 4);
        assert_eq!(next.points, 77);
        let alive: usize = a.groups.iter().filter(|g| !g.dead).count();
        assert_eq!(next.units.len(), alive);
    }

    #[test]
    fn hold_timer_wins() {
        let mut g = Game::new(crate::citygen::generate(4), 1, 1.0, 5);
        g.campaign = true;
        g.goal = 3.0;
        for _ in 0..80 {
            g.tick();
        }
        assert!(g.won && g.over);
    }

    #[test]
    fn save_roundtrip() {
        let mut c = veteran();
        c.points = 321;
        c.kills = 1_234_567;
        let d = from_text(&to_text(&c));
        assert_eq!((d.fac, d.diff, d.level, d.points, d.kills), (2, 1, 3, 321, 1_234_567));
        assert_eq!(d.research, c.research);
        assert_eq!(d.units, c.units);
    }

    #[test]
    fn shop() {
        let mut c = Campaign::new(0, 1);
        c.points = 200;
        assert!(c.buy_research(R::Rescue));
        assert!(!c.buy_research(R::Rescue));
        assert!(c.recruit(U::Rifle));
        assert!(!c.recruit(U::Tank));
        assert_eq!(c.points, 200 - rdef(R::Rescue).civs - recruit_cost(U::Rifle));
        assert_eq!(c.army(U::Rifle), (1, 6));
    }
}



//! The world campaign: the outbreak is spreading through the world's biggest cities, and you are
//! the one the governments send in. It starts in New York. Between cities a globe shows where the
//! horde stands: every city has days until it falls and civilians still alive, and you pick where
//! to go next. A city close to falling has a bigger horde.
//!
//! Hold a city until the evac and the civilians you flew out are yours to spend on the same
//! research the lab sells for scrap in a normal game, or on recruits. Lose one and you get nothing
//! from it; lose three and the world's governments fire you. Troops that survive march on to the
//! next city; scrap and buildings don't.
//!
//! The maps are downloaded from OpenStreetMap once, by the `cities` GitHub Actions workflow
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
    pub lat: f32,
    pub lon: f32,
    /// metro population, millions (UN World Urbanization Prospects, 2023)
    pub pop: f32,
}

/// New York, where it starts, and the ten largest urban areas.
pub const CITIES: [City; 11] = [
    City { name: "New York", query: "Manhattan, New York, USA", file: "new-york", lat: 40.75, lon: -73.99, pop: 19.6 },
    City { name: "Osaka", query: "Osaka, Japan", file: "osaka", lat: 34.69, lon: 135.50, pop: 19.0 },
    City { name: "Dhaka", query: "Dhaka, Bangladesh", file: "dhaka", lat: 23.81, lon: 90.41, pop: 23.2 },
    City { name: "Beijing", query: "Beijing, China", file: "beijing", lat: 39.90, lon: 116.40, pop: 21.8 },
    City { name: "Mumbai", query: "Mumbai, India", file: "mumbai", lat: 19.08, lon: 72.88, pop: 21.3 },
    City { name: "Cairo", query: "Cairo, Egypt", file: "cairo", lat: 30.04, lon: 31.24, pop: 22.6 },
    City { name: "Mexico City", query: "Mexico City, Mexico", file: "mexico-city", lat: 19.43, lon: -99.13, pop: 22.5 },
    City { name: "São Paulo", query: "São Paulo, Brazil", file: "sao-paulo", lat: -23.55, lon: -46.63, pop: 22.6 },
    City { name: "Shanghai", query: "Shanghai, China", file: "shanghai", lat: 31.23, lon: 121.47, pop: 29.2 },
    City { name: "Delhi", query: "Delhi, India", file: "delhi", lat: 28.61, lon: 77.21, pop: 33.8 },
    City { name: "Tokyo", query: "Tokyo, Japan", file: "tokyo", lat: 35.68, lon: 139.69, pop: 37.2 },
];

impl City {
    /// The country, from the lookup ("Cairo, Egypt" -> "Egypt").
    pub fn country(&self) -> &'static str {
        self.query.rsplit(", ").next().unwrap_or("")
    }
}

/// Civilians it costs to recruit troops between cities.
pub fn recruit_cost(u: U) -> u32 {
    match u {
        U::Rifle => 30,
        U::Tank => 100,
    }
}

/// Lost cities before the governments fire you.
pub const MAX_LOST: u32 = 3;
/// Days that pass for every city you fight in (the fight, the flight, the clean-up).
pub const DAYS_PER_CITY: f32 = 10.0;
/// A city this many days or more from falling has the mildest horde.
const CALM_DAYS: f32 = 90.0;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Fate {
    /// still standing, can be defended
    Open,
    /// you held it until the evac
    Held,
    /// you fought there and the HQ fell
    Lost,
    /// the horde took it before anyone came
    Fallen,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct CityState {
    /// days until the city falls by itself
    pub days: f32,
    /// days it had when the campaign started, for how many people are left
    pub start: f32,
    /// days lost per campaign day: how fast the horde moves there
    pub rate: f32,
    pub fate: Fate,
}

impl CityState {
    /// Share of the people still alive: falls as the city nears its end.
    pub fn share(&self) -> f32 {
        match self.fate {
            Fate::Fallen => 0.0,
            _ => 0.08 + 0.92 * (self.days / self.start).clamp(0.0, 1.0).powf(0.8),
        }
    }
    /// 0 for a quiet city, 1 for one about to fall.
    pub fn threat(&self) -> f32 {
        (1.0 - self.days / CALM_DAYS).clamp(0.0, 1.0)
    }
}

/// A small fixed generator so a campaign's world is the same everywhere it is loaded.
fn hash(seed: u64, k: u64) -> f32 {
    let mut x = seed ^ k.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    x ^= x >> 33;
    (x >> 40) as f32 / (1u64 << 24) as f32
}

#[derive(Clone)]
pub struct Campaign {
    pub fac: usize,
    pub diff: usize,
    /// picks the cities' days and speeds
    pub seed: u64,
    pub cities: Vec<CityState>,
    /// the city picked to defend next
    pub current: usize,
    /// cities fought in so far, held or lost
    pub fought: u32,
    pub lost: u32,
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
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(1);
        Campaign::with_seed(fac, diff, seed)
    }
    pub fn with_seed(fac: usize, diff: usize, seed: u64) -> Campaign {
        let cities = (0..CITIES.len())
            .map(|i| {
                // New York is where it starts: the horde is already in the streets
                let days = if i == 0 { 40.0 } else { 30.0 + 90.0 * hash(seed, i as u64) };
                CityState { days, start: days, rate: 0.7 + 0.8 * hash(seed, 100 + i as u64), fate: Fate::Open }
            })
            .collect();
        Campaign { fac, diff, seed, cities, current: 0, fought: 0, lost: 0, points: 0, research: Vec::new(), units: Vec::new(), flown: 0, kills: 0 }
    }
    /// Fired after three lost cities.
    pub fn fired(&self) -> bool {
        self.lost >= MAX_LOST
    }
    pub fn open(&self, i: usize) -> bool {
        self.cities.get(i).is_some_and(|c| c.fate == Fate::Open)
    }
    /// Over: fired, or no city left standing to defend.
    pub fn finished(&self) -> bool {
        self.fired() || !(0..CITIES.len()).any(|i| self.open(i))
    }
    pub fn held(&self) -> usize {
        self.cities.iter().filter(|c| c.fate == Fate::Held).count()
    }
    pub fn city(&self) -> &'static City {
        &CITIES[self.current.min(CITIES.len() - 1)]
    }
    pub fn state(&self) -> &CityState {
        &self.cities[self.current.min(CITIES.len() - 1)]
    }
    /// Millions of people still alive in city `i`.
    pub fn alive(&self, i: usize) -> f32 {
        CITIES[i].pop * self.cities[i].share()
    }
    /// How much bigger the horde is in city `i` than on the chosen difficulty: from ×0.8 for a
    /// quiet city to ×3.2 for one about to fall, and 6% more for every city fought before.
    pub fn horde_mult_at(&self, i: usize) -> f32 {
        let t = self.cities[i].threat();
        (0.8 + 2.4 * t.powf(1.2)) * 1.06f32.powi(self.fought as i32)
    }
    pub fn horde_mult(&self) -> f32 {
        self.horde_mult_at(self.current)
    }
    /// Seconds to hold out before the evac: 7 minutes, 20 s more per city fought, at most 11.
    pub fn hold_secs(&self) -> f32 {
        (7.0 * 60.0 + 20.0 * self.fought as f32).min(11.0 * 60.0)
    }
    /// Civilians on the map in city `i`: one for every ~1,700 people still alive there.
    pub fn civilians_at(&self, i: usize) -> usize {
        (self.alive(i) * 600.0).clamp(1500.0, 18000.0) as usize
    }
    /// The open city closest to falling.
    pub fn most_urgent(&self) -> Option<usize> {
        (0..CITIES.len()).filter(|i| self.open(*i)).min_by(|a, b| self.cities[*a].days.total_cmp(&self.cities[*b].days))
    }
    pub fn select(&mut self, i: usize) {
        if self.open(i) {
            self.current = i;
        }
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

    /// Sets a fresh game up as the chosen city: its people, research, the hold timer and the army.
    /// Everything read here is saved by `to_text`, so co-op guests set up the same game.
    pub fn apply(&self, g: &mut Game) {
        g.campaign = true;
        g.goal = self.hold_secs();
        g.set_civilians(self.civilians_at(self.current));
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

    /// After a city: held, bank the civilians; lost, nothing. Survivors march on either way, the
    /// world moves on `DAYS_PER_CITY` days, and the most urgent city is picked next.
    pub fn harvest(&mut self, g: &Game) {
        let cur = self.current;
        if g.won {
            self.points += g.extracted;
            self.flown += g.extracted;
            self.cities[cur].fate = Fate::Held;
        } else {
            self.cities[cur].fate = Fate::Lost;
            self.cities[cur].days = 0.0;
            self.lost += 1;
        }
        self.kills += g.kills;
        self.units = g
            .groups
            .iter()
            .filter(|gr| !gr.dead)
            .map(|gr| (gr.kind, gr.m.iter().filter(|m| m.alive).count() as u32))
            .filter(|x| x.1 > 0)
            .collect();
        self.fought += 1;
        for c in self.cities.iter_mut().filter(|c| c.fate == Fate::Open) {
            c.days -= DAYS_PER_CITY * c.rate;
            if c.days <= 0.0 {
                c.days = 0.0;
                c.fate = Fate::Fallen;
            }
        }
        if let Some(i) = self.most_urgent() {
            self.current = i;
        }
    }
}

// ------------------------------------------------------------------------------------------ saving

fn save_path() -> PathBuf {
    data_dir().join("campaign.txt")
}

pub fn load() -> Option<Campaign> {
    let t = std::fs::read_to_string(save_path()).ok()?;
    // saves from before the globe (one fixed city order) start over
    t.contains("cities=").then(|| from_text(&t))
}

pub fn save(c: &Campaign) {
    let _ = std::fs::write(save_path(), to_text(c));
}

pub fn from_text(t: &str) -> Campaign {
    let field = |k: &str| t.lines().find_map(|l| l.split_once('=').filter(|(a, _)| a.trim() == k).map(|(_, v)| v.trim().to_string()));
    let num = |k: &str| field(k).and_then(|v| v.parse::<u64>().ok());
    let mut c = Campaign::with_seed(
        num("faction").unwrap_or(0).min(FACTIONS.len() as u64 - 1) as usize,
        num("difficulty").unwrap_or(1).min(3) as usize,
        num("seed").unwrap_or(1),
    );
    c.current = num("current").unwrap_or(0).min(CITIES.len() as u64 - 1) as usize;
    c.fought = num("fought").unwrap_or(0) as u32;
    c.lost = num("lost").unwrap_or(0) as u32;
    c.points = num("points").unwrap_or(0) as u32;
    c.flown = num("flown").unwrap_or(0) as u32;
    c.kills = num("kills").unwrap_or(0);
    if let Some(v) = field("cities") {
        for (s, x) in c.cities.iter_mut().zip(v.split(',')) {
            let p: Vec<&str> = x.split(':').collect();
            if let [d, st, r, f] = p[..] {
                s.days = d.parse().unwrap_or(s.days);
                s.start = st.parse().unwrap_or(s.start);
                s.rate = r.parse().unwrap_or(s.rate);
                s.fate = match f {
                    "h" => Fate::Held,
                    "l" => Fate::Lost,
                    "f" => Fate::Fallen,
                    _ => Fate::Open,
                };
            }
        }
    }
    if let Some(v) = field("research") {
        c.research = v.split(',').filter_map(|x| x.trim().parse::<usize>().ok()).filter_map(|i| RESEARCH.get(i).copied()).collect();
    }
    if let Some(v) = field("units") {
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
    c
}

/// The whole campaign as text: the save file, and what the co-op host sends its guests. Floats are
/// written exactly, so both sides build the same game.
pub fn to_text(c: &Campaign) -> String {
    let research: Vec<String> = c.research.iter().filter_map(|r| RESEARCH.iter().position(|x| x == r)).map(|i| i.to_string()).collect();
    let units: Vec<String> = c.units.iter().map(|(u, n)| format!("{}{}", if *u == U::Rifle { 'r' } else { 't' }, n)).collect();
    let cities: Vec<String> = c
        .cities
        .iter()
        .map(|s| {
            let f = match s.fate {
                Fate::Open => "o",
                Fate::Held => "h",
                Fate::Lost => "l",
                Fate::Fallen => "f",
            };
            format!("{:?}:{:?}:{:?}:{}", s.days, s.start, s.rate, f)
        })
        .collect();
    format!(
        "faction={}\ndifficulty={}\nseed={}\ncurrent={}\nfought={}\nlost={}\npoints={}\nflown={}\nkills={}\ncities={}\nresearch={}\nunits={}\n",
        c.fac,
        c.diff,
        c.seed,
        c.current,
        c.fought,
        c.lost,
        c.points,
        c.flown,
        c.kills,
        cities.join(","),
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
        let mut c = Campaign::with_seed(2, 1, 77);
        c.fought = 3;
        c.current = 4;
        c.research = vec![R::Armour, R::Barrels, R::Rescue];
        c.units = vec![(U::Rifle, 6), (U::Tank, 1), (U::Rifle, 2)];
        c
    }

    /// Carried troops arrive, survivors carry on, and a campaign game stays deterministic.
    #[test]
    fn carry_over() {
        let c = veteran();
        let mk = || {
            let mut g = Game::new(crate::citygen::generate(3), c.fac, c.horde_mult(), 1003);
            c.apply(&mut g);
            g
        };
        let (mut a, mut b) = (mk(), mk());
        assert_eq!(a.groups.len(), 3);
        assert_eq!(a.civs.len(), c.civilians_at(4));
        assert!(a.research.armour && a.research.barrels && a.research.rescue && !a.research.nuke);
        assert!(!a.can_research(R::Nuke));
        for _ in 0..400 {
            a.tick();
            b.tick();
        }
        assert_eq!(a.checksum(), b.checksum());
        let mut next = c.clone();
        a.extracted = 77;
        a.won = true;
        next.harvest(&a);
        assert_eq!(next.fought, 4);
        assert_eq!(next.points, 77);
        assert_eq!(next.cities[4].fate, Fate::Held);
        assert_ne!(next.current, 4);
        let alive: usize = a.groups.iter().filter(|g| !g.dead).count();
        assert_eq!(next.units.len(), alive);
    }

    /// A lost city pays nothing, and the third one ends the campaign.
    #[test]
    fn fired_after_three() {
        let mut c = Campaign::with_seed(0, 1, 5);
        let mut g = Game::new(crate::citygen::generate(4), 0, 1.0, 5);
        g.extracted = 500;
        for k in 0..3 {
            assert!(!c.finished());
            c.harvest(&g);
            assert_eq!(c.lost, k + 1);
        }
        assert_eq!(c.points, 0);
        assert!(c.fired() && c.finished());
    }

    /// Cities run out of time while you're elsewhere, and a city near its end is harder.
    #[test]
    fn world_moves_on() {
        let mut c = Campaign::with_seed(0, 1, 9);
        let g = {
            let mut g = Game::new(crate::citygen::generate(4), 0, 1.0, 5);
            g.won = true;
            g
        };
        let d0 = c.cities[3].days;
        c.harvest(&g);
        assert!(c.cities[3].days < d0);
        assert!(c.alive(3) < CITIES[3].pop);
        for _ in 0..30 {
            if c.finished() {
                break;
            }
            c.harvest(&g);
        }
        assert!(c.finished());
        assert!(c.cities.iter().any(|s| s.fate == Fate::Fallen));
        let mut a = Campaign::with_seed(0, 1, 9);
        a.cities[2].days = 5.0;
        a.cities[3].days = 80.0;
        assert!(a.horde_mult_at(2) > 2.0 * a.horde_mult_at(3));
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
        c.lost = 2;
        c.cities[6].fate = Fate::Fallen;
        c.cities[2].days = 13.371_337;
        let d = from_text(&to_text(&c));
        assert_eq!((d.fac, d.diff, d.seed, d.current, d.fought, d.lost, d.points, d.kills), (2, 1, 77, 4, 3, 2, 321, 1_234_567));
        assert_eq!(d.cities, c.cities);
        assert_eq!(d.research, c.research);
        assert_eq!(d.units, c.units);
        assert_eq!(to_text(&d), to_text(&c));
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

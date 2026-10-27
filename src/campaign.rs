//! The world campaign: the zombie outbreak is spreading through the world's biggest cities, and
//! you are the one the governments send in. It starts in New York. Between cities a globe shows
//! where the war stands, XCOM style: every city has turns until it falls, its own threat level,
//! civilians still alive and pay for holding it, and you can only defend one a turn. The cities you
//! don't pick run down; when one falls the hordes everywhere else grow stronger and another city
//! joins the war. Harder cities pay more.
//!
//! A few turns in, the aliens and the orcs arrive. Alien swarms fly from city to city, attacking
//! one at a time and eating through it fast; hold the city and they fly on. Orcs open portals in
//! cities at random. Kills of each army buy research into it, which weakens it and in the end
//! wipes it out everywhere. Wipe out all three and the world is saved.
//!
//! Hold a city until the evac and the civilians you flew out are yours to spend on the tech tree
//! or on recruits. Fail three defences and the world's governments fire you. Troops that survive
//! march on to the next city; scrap and buildings don't.
//!
//! The maps are downloaded from OpenStreetMap once, by the `cities` GitHub Actions workflow
//! (`PH_FETCH`), and published as the `cities` release. The game fetches that pack from GitHub in the
//! background when it starts, so a campaign city never waits on OpenStreetMap. If a map is missing
//! anyway, it falls back to downloading the city live.

use crate::defs::*;
use crate::game::{Allowed, Game};
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

/// Well-known cities a campaign picks from. New York, where every campaign starts, comes first.
/// Saves store indices into it, so new cities go at the end.
pub const POOL: [City; 40] = [
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
    City { name: "London", query: "London, United Kingdom", file: "london", lat: 51.51, lon: -0.13, pop: 9.6 },
    City { name: "Paris", query: "Paris, France", file: "paris", lat: 48.86, lon: 2.35, pop: 11.3 },
    City { name: "Los Angeles", query: "Los Angeles, USA", file: "los-angeles", lat: 34.05, lon: -118.24, pop: 12.5 },
    City { name: "Chicago", query: "Chicago, USA", file: "chicago", lat: 41.88, lon: -87.63, pop: 8.9 },
    City { name: "Rio de Janeiro", query: "Rio de Janeiro, Brazil", file: "rio-de-janeiro", lat: -22.91, lon: -43.17, pop: 13.8 },
    City { name: "Buenos Aires", query: "Buenos Aires, Argentina", file: "buenos-aires", lat: -34.60, lon: -58.38, pop: 15.6 },
    City { name: "Moscow", query: "Moscow, Russia", file: "moscow", lat: 55.76, lon: 37.62, pop: 12.7 },
    City { name: "Istanbul", query: "Fatih, Istanbul, Turkey", file: "istanbul", lat: 41.01, lon: 28.98, pop: 15.8 },
    City { name: "Seoul", query: "Seoul, South Korea", file: "seoul", lat: 37.57, lon: 126.98, pop: 10.0 },
    City { name: "Hong Kong", query: "Central, Hong Kong", file: "hong-kong", lat: 22.28, lon: 114.16, pop: 7.6 },
    City { name: "Singapore", query: "Downtown Core, Singapore", file: "singapore", lat: 1.29, lon: 103.85, pop: 6.1 },
    City { name: "Bangkok", query: "Bangkok, Thailand", file: "bangkok", lat: 13.76, lon: 100.50, pop: 11.2 },
    City { name: "Jakarta", query: "Jakarta, Indonesia", file: "jakarta", lat: -6.18, lon: 106.83, pop: 11.2 },
    City { name: "Manila", query: "Manila, Philippines", file: "manila", lat: 14.60, lon: 120.98, pop: 14.9 },
    City { name: "Sydney", query: "Sydney, Australia", file: "sydney", lat: -33.87, lon: 151.21, pop: 5.1 },
    City { name: "Lagos", query: "Lagos Island, Lagos, Nigeria", file: "lagos", lat: 6.45, lon: 3.39, pop: 15.9 },
    City { name: "Nairobi", query: "Nairobi, Kenya", file: "nairobi", lat: -1.29, lon: 36.82, pop: 5.3 },
    City { name: "Johannesburg", query: "Johannesburg, South Africa", file: "johannesburg", lat: -26.20, lon: 28.05, pop: 6.2 },
    City { name: "Cape Town", query: "Cape Town, South Africa", file: "cape-town", lat: -33.92, lon: 18.42, pop: 4.9 },
    City { name: "Dubai", query: "Downtown Dubai, Dubai, United Arab Emirates", file: "dubai", lat: 25.20, lon: 55.27, pop: 3.0 },
    City { name: "Rome", query: "Rome, Italy", file: "rome", lat: 41.90, lon: 12.50, pop: 4.3 },
    City { name: "Berlin", query: "Berlin, Germany", file: "berlin", lat: 52.52, lon: 13.40, pop: 3.6 },
    City { name: "Madrid", query: "Madrid, Spain", file: "madrid", lat: 40.42, lon: -3.70, pop: 6.8 },
    City { name: "Toronto", query: "Toronto, Canada", file: "toronto", lat: 43.65, lon: -79.38, pop: 6.4 },
    City { name: "San Francisco", query: "San Francisco, USA", file: "san-francisco", lat: 37.77, lon: -122.42, pop: 3.3 },
    City { name: "Lima", query: "Lima, Peru", file: "lima", lat: -12.05, lon: -77.04, pop: 11.2 },
    City { name: "Tehran", query: "Tehran, Iran", file: "tehran", lat: 35.69, lon: 51.39, pop: 9.5 },
    City { name: "Washington", query: "Washington, D.C., USA", file: "washington", lat: 38.90, lon: -77.04, pop: 5.4 },
    City { name: "Barcelona", query: "Barcelona, Spain", file: "barcelona", lat: 41.39, lon: 2.17, pop: 5.7 },
];

/// Cities in a campaign: New York and ten more from the pool.
pub const CAMPAIGN_CITIES: usize = 11;
/// No two campaign cities closer than this, so each campaign spreads over the world.
const SPREAD_KM: f32 = 1_000.0;

impl City {
    pub fn km_to(&self, o: &City) -> f32 {
        let (a, b) = ((self.lat.to_radians(), self.lon.to_radians()), (o.lat.to_radians(), o.lon.to_radians()));
        let c = a.0.sin() * b.0.sin() + a.0.cos() * b.0.cos() * (a.1 - b.1).cos();
        6371.0 * c.clamp(-1.0, 1.0).acos()
    }
    /// The country, from the lookup ("Cairo, Egypt" -> "Egypt").
    pub fn country(&self) -> &'static str {
        self.query.rsplit(", ").next().unwrap_or("")
    }
}

// ------------------------------------------------------------------------------------------ tech tree

/// Something the campaign's tech tree unlocks with research points (civilians flown out).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tech {
    Build(S),
    Unit(U),
    Ability(A),
    Research(R),
    Up(Up),
}

/// What every campaign starts with.
pub const START_BUILDS: [S; 5] = [S::Wall, S::Bunker, S::Extractor, S::Pad, S::Rescue];

/// The whole tree, in the order the campaign screen lists it. Saves store indices into it, so new entries go at the end.
pub const TECH: [Tech; 32] = [
    Tech::Build(S::Gun),
    Tech::Build(S::Cannon),
    Tech::Build(S::Flame),
    Tech::Build(S::Rail),
    Tech::Build(S::Helium),
    Tech::Build(S::Artillery),
    Tech::Build(S::Outpost),
    Tech::Build(S::Helipad),
    Tech::Build(S::Silo),
    Tech::Unit(U::Tank),
    Tech::Ability(A::Rescue),
    Tech::Ability(A::Air),
    Tech::Ability(A::Gunship),
    Tech::Research(R::Rescue),
    Tech::Research(R::Carpet),
    Tech::Research(R::Nuke),
    Tech::Research(R::Armour),
    Tech::Research(R::Presses),
    Tech::Research(R::Barrels),
    Tech::Up(Up::GunRapid),
    Tech::Up(Up::GunAp),
    Tech::Up(Up::CannonAuto),
    Tech::Up(Up::CannonSplash),
    Tech::Up(Up::FlameNapalm),
    Tech::Up(Up::FlameReach),
    Tech::Up(Up::RailCaps),
    Tech::Up(Up::RailWide),
    Tech::Up(Up::HeliumReach),
    Tech::Up(Up::HeliumHull),
    Tech::Up(Up::ArtyRapid),
    Tech::Up(Up::ArtyHe),
    Tech::Research(R::Mounts),
];

/// Turrets that shoot ammo: the first one unlocked brings the ammo plant with it.
fn shoots(t: S) -> bool {
    matches!(t, S::Gun | S::Cannon | S::Flame | S::Rail | S::Artillery)
}

impl Tech {
    pub fn name(self) -> &'static str {
        match self {
            Tech::Build(s) => def(s).name,
            Tech::Unit(u) => udef(u).name,
            Tech::Ability(a) => adef(a).name,
            Tech::Research(r) => rdef(r).name,
            Tech::Up(u) => updef(u).name,
        }
    }
    pub fn tip(self) -> String {
        match self {
            Tech::Build(S::Silo) => format!("{} Unlocks the ICBM.", def(S::Silo).tip),
            Tech::Build(s) if shoots(s) => format!("{} The first turret also unlocks the ammo plant.", def(s).tip),
            Tech::Build(s) => def(s).tip.to_string(),
            Tech::Unit(u) => format!("{}: a squad you can call in from the build bar.", udef(u).name),
            Tech::Ability(a) => adef(a).tip.to_string(),
            Tech::Research(r) => rdef(r).tip.to_string(),
            Tech::Up(u) => updef(u).tip.to_string(),
        }
    }
    /// Research points it costs: three times the first campaign's prices, which one good city
    /// could nearly pay off.
    pub fn cost(self) -> u32 {
        3 * match self {
            Tech::Build(s) => match s {
                S::Gun => 60,
                S::Cannon => 140,
                S::Flame => 120,
                S::Rail => 260,
                S::Helium => 160,
                S::Artillery => 320,
                S::Outpost => 100,
                S::Helipad => 260,
                S::Silo => 380,
                _ => 0,
            },
            Tech::Unit(_) => 150,
            Tech::Ability(a) => match a {
                A::Rescue => 60,
                A::Air => 150,
                A::Gunship => 220,
                A::Icbm => 0,
            },
            Tech::Research(r) => rdef(r).civs,
            Tech::Up(u) => updef(u).cost,
        }
    }
    /// The heading it's listed under.
    pub fn group(self) -> &'static str {
        match self {
            Tech::Build(s) if shoots(s) || s == S::Helium => "Turrets",
            Tech::Build(_) | Tech::Unit(_) => "Support",
            Tech::Ability(_) | Tech::Research(R::Rescue | R::Carpet | R::Nuke) => "Abilities",
            Tech::Research(_) => "Research",
            Tech::Up(_) => "Turret upgrades",
        }
    }
    /// What has to be unlocked first, if anything: its parent in the tree.
    pub fn needs(self) -> Option<Tech> {
        match self {
            Tech::Up(u) => Some(Tech::Build(updef(u).on)),
            Tech::Build(S::Cannon | S::Flame) => Some(Tech::Build(S::Gun)),
            Tech::Build(S::Rail | S::Artillery) => Some(Tech::Build(S::Cannon)),
            Tech::Build(S::Silo) => Some(Tech::Ability(A::Air)),
            Tech::Build(S::Helipad) => Some(Tech::Ability(A::Gunship)),
            Tech::Unit(U::Tank) => Some(Tech::Research(R::Armour)),
            Tech::Ability(A::Gunship) => Some(Tech::Ability(A::Air)),
            Tech::Research(R::Rescue) => Some(Tech::Ability(A::Rescue)),
            Tech::Research(R::Carpet) => Some(Tech::Ability(A::Air)),
            Tech::Research(R::Nuke) => Some(Tech::Build(S::Silo)),
            Tech::Research(R::Presses | R::Barrels | R::Mounts) => Some(Tech::Build(S::Gun)),
            _ => None,
        }
    }
    /// What it opens up, in tree order.
    pub fn children(self) -> Vec<Tech> {
        TECH.into_iter().filter(|t| t.needs() == Some(self)).collect()
    }
}

/// Where a tech sits in the drawn tree: which of the two halves, its column (how deep it is) and
/// its row (in leaf slots, from the top of its half; parents sit level with the middle of their
/// children).
#[derive(Clone, Copy, Debug)]
pub struct Node {
    pub tech: Tech,
    pub half: usize,
    pub col: usize,
    pub row: f32,
}

/// Empty rows left between one root's branch and the next.
pub const TREE_GAP: f32 = 0.6;

/// The whole tech tree laid out left to right, roots first: the branches are split between two
/// halves of about the same number of rows. Also returns each half's (columns, rows).
pub fn tree_layout() -> (Vec<Node>, [(usize, f32); 2]) {
    fn leaves(t: Tech) -> usize {
        t.children().into_iter().map(leaves).sum::<usize>().max(1)
    }
    fn place(t: Tech, half: usize, col: usize, row: &mut f32, out: &mut Vec<Node>) -> f32 {
        let kids = t.children();
        let y = if kids.is_empty() {
            let y = *row;
            *row += 1.0;
            y
        } else {
            let ys: Vec<f32> = kids.into_iter().map(|k| place(k, half, col + 1, row, out)).collect();
            (ys[0] + ys[ys.len() - 1]) / 2.0
        };
        out.push(Node { tech: t, half, col, row: y });
        y
    }
    let roots: Vec<Tech> = TECH.into_iter().filter(|t| t.needs().is_none()).collect();
    let total: usize = roots.iter().map(|r| leaves(*r)).sum();
    let mut out = Vec::new();
    let mut rows = [0.0f32; 2];
    let (mut half, mut done) = (0, 0);
    for r in roots {
        if half == 0 && done * 2 >= total {
            half = 1;
        }
        if rows[half] > 0.0 {
            rows[half] += TREE_GAP;
        }
        place(r, half, 0, &mut rows[half], &mut out);
        done += leaves(r);
    }
    let cols = |h: usize| out.iter().filter(|n| n.half == h).map(|n| n.col + 1).max().unwrap_or(1);
    let dims = [(cols(0), rows[0]), (cols(1), rows[1])];
    (out, dims)
}

/// Research points it costs to recruit troops between cities.
pub fn recruit_cost(u: U) -> u32 {
    match u {
        U::Rifle => 60,
        U::Tank => 200,
    }
}

/// Failed defences before the governments fire you.
pub const MAX_LOST: u32 = 3;
/// The armies, as indices into `FACTIONS`. Zombies are everywhere from the start; aliens and orcs
/// turn up a few turns in.
pub const ZOMBIES: usize = 0;
pub const ALIENS: usize = 1;
pub const ORCS: usize = 2;
/// Every city that falls (or is lost) makes the hordes everywhere else this much stronger.
const DOOM: f32 = 1.08;
/// An alien swarm grows this much stronger for every turn nobody fights it.
const ALIEN_GROWTH: f32 = 0.2;
/// Turns an alien attack takes off its city each turn (zombies and orcs take one).
const ALIEN_BITE: u32 = 2;
/// At most this many minor alien swarms at once.
const MAX_MINOR: usize = 3;
/// Chance each turn that the orcs open portals in one more city.
const ORC_SPREAD: f32 = 0.4;
/// Chance that a city added to the map later already has orcs in it.
const ORC_NEW_CITY: f32 = 0.3;
/// Kills of each army that make one point of research into it.
pub const KILLS_PER_POINT: [u64; 3] = [1000, 1000, 250];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Fate {
    /// still standing, can be defended
    Open,
    /// you held it until the evac
    Held,
    /// you fought there and the HQ fell (or you abandoned it)
    Lost,
    /// the horde took it before anyone came
    Fallen,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct CityState {
    /// turns until the city falls by itself
    pub turns: u32,
    /// turns it had when it joined the campaign, for how many people are left
    pub start: u32,
    /// how hard its horde is by itself; grows by `ramp` every turn
    pub heat: f32,
    pub ramp: f32,
    /// how crowded it still is: more or fewer civilians on the map than its size says
    pub civ: f32,
    /// orc portals have opened here
    pub orcs: bool,
    pub fate: Fate,
}

impl CityState {
    /// A city's own numbers, from the campaign's seed and its place in the list. Each one differs
    /// in all four ways, so no two campaigns play the same order.
    fn fresh(seed: u64, slot: usize) -> CityState {
        let k = 1000 + slot as u64 * 8;
        let turns = 3 + (hash(seed, k) * 8.0) as u32;
        CityState {
            turns,
            start: turns,
            heat: 0.9 + 1.2 * hash(seed, k + 1),
            ramp: 0.04 + 0.16 * hash(seed, k + 2),
            civ: 0.5 + 1.1 * hash(seed, k + 3),
            orcs: false,
            fate: Fate::Open,
        }
    }
    /// Share of the people still alive: falls as the city nears its end.
    pub fn share(&self) -> f32 {
        match self.fate {
            Fate::Fallen | Fate::Lost => 0.0,
            _ => 0.25 + 0.75 * (self.turns as f32 / self.start.max(1) as f32).clamp(0.0, 1.0),
        }
    }
}

/// An alien swarm on the move: it attacks one city at a time and flies on when that city is
/// held or taken. Minor swarms (spawned when the aliens take a city) die when their city is held.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Swarm {
    pub city: usize,
    /// the city it flew in from, for the trail on the globe
    pub from: Option<usize>,
    /// ×horde: grows every turn it's left alone
    pub power: f32,
    pub major: bool,
    /// the turn (cities fought) it last moved on
    pub moved: u32,
}

/// One step of research into an army.
pub struct FTech {
    pub name: &'static str,
    pub tip: &'static str,
    pub cost: u32,
}

/// Steps on each army's research path; the last wipes the army out everywhere.
pub const FTIERS: usize = 5;
/// Each step before the last makes the army's hordes this much smaller...
const F_SIZE: f32 = 0.10;
/// ...and its units this much weaker (health and attack).
const F_STATS: f32 = 0.07;

pub const FTECH: [[FTech; FTIERS]; 3] = [
    [
        FTech { name: "Necrotic tissue study", tip: "Autopsies show where the dead are weakest.", cost: 40 },
        FTech { name: "Pheromone jamming", tip: "Scrambles the scent that draws the dead together.", cost: 80 },
        FTech { name: "Outbreak containment", tip: "Quarantine teams cut new outbreaks off early.", cost: 130 },
        FTech { name: "Prion inhibitor", tip: "A spray that slows the infection in the bitten.", cost: 200 },
        FTech { name: "Global cure", tip: "The cure, everywhere at once. Ends the zombie plague, and with it the war. Needs the aliens and orcs gone first.", cost: 320 },
    ],
    [
        FTech { name: "Chitin analysis", tip: "Finds the soft joints in their shells.", cost: 40 },
        FTech { name: "Hive signal jamming", tip: "Muddles the signal that coordinates the drops.", cost: 80 },
        FTech { name: "Xeno-toxin rounds", tip: "Ammunition laced with something they can't digest.", cost: 130 },
        FTech { name: "Orbital tracking", tip: "Satellites follow their ships between cities.", cost: 200 },
        FTech { name: "Hive-mind collapse", tip: "Severs the hive mind. Every alien swarm on Earth dies at once.", cost: 320 },
    ],
    [
        FTech { name: "Portal harmonics", tip: "Learns the hum that comes before a portal opens.", cost: 40 },
        FTech { name: "Warband psychology", tip: "Their chiefs hold them together; snipers know who to hit.", cost: 80 },
        FTech { name: "Blood-rage suppressant", tip: "Gas that takes the edge off their fury.", cost: 130 },
        FTech { name: "Rift stabilisers", tip: "Keeps portals small while they're open.", cost: 200 },
        FTech { name: "Seal the rift", tip: "Closes their dimension's door for good. Every orc on Earth is cut off and falls.", cost: 320 },
    ],
];

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

/// The pool in this campaign's own seeded order (New York left out): where its cities come from.
fn pool_order(seed: u64) -> Vec<usize> {
    let mut order: Vec<usize> = (1..POOL.len()).collect();
    order.sort_by(|a, b| hash(seed, 500 + *a as u64).total_cmp(&hash(seed, 500 + *b as u64)));
    order
}

/// New York, then cities from the pool in a seeded order, skipping any too close to one already in.
pub fn pick_cities(seed: u64) -> Vec<usize> {
    let order = pool_order(seed);
    let mut picks = vec![0];
    for &i in &order {
        if picks.len() < CAMPAIGN_CITIES && picks.iter().all(|&p| POOL[p].km_to(&POOL[i]) >= SPREAD_KM) {
            picks.push(i);
        }
    }
    // a crowded draw fills up with whatever is left
    for &i in &order {
        if picks.len() < CAMPAIGN_CITIES && !picks.contains(&i) {
            picks.push(i);
        }
    }
    picks
}

/// One player's side of a campaign: their research points and what they have unlocked. In co-op
/// every player earns points from the civilians flown out of their own evac points and builds up
/// their own tech tree; alone there is just the one.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Crew {
    pub points: u32,
    pub tech: Vec<Tech>,
    /// money for the garage, paid for every city held
    pub money: u32,
}
static NO_CREW: Crew = Crew { points: 0, tech: Vec::new(), money: 0 };

/// Players a campaign keeps a crew for (co-op's limit).
pub const CREWS: usize = 4;

/// News for the campaign screen's bulletin.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Bulletin {
    /// a new army has reached Earth
    Arrived(usize),
    /// research wiped an army out
    Wiped(usize),
}
impl Bulletin {
    pub fn bit(self) -> u8 {
        match self {
            Bulletin::Arrived(a) => 1 << a,
            Bulletin::Wiped(a) => 8 << a,
        }
    }
}

#[derive(Clone)]
pub struct Campaign {
    /// the army the campaign is about: always the zombies
    pub fac: usize,
    pub diff: usize,
    /// picks the cities' numbers, when the other armies arrive and where they go
    pub seed: u64,
    /// this campaign's cities, indices into `POOL`; New York first, cities added as others fall
    pub picks: Vec<usize>,
    pub cities: Vec<CityState>,
    /// the city picked to defend next
    pub current: usize,
    /// cities fought in so far, held or lost: the campaign's turns
    pub fought: u32,
    pub lost: u32,
    /// each player's research points and tech tree unlocks, host first
    pub crew: Vec<Crew>,
    /// surviving troops: (kind, soldiers) per squad
    pub units: Vec<(U, u32)>,
    pub flown: u32,
    pub kills: u64,
    /// the alien swarms flying from city to city
    pub aliens: Vec<Swarm>,
    /// kills of each army, which pay for research into it
    pub fkills: [u64; 3],
    /// points spent on each army's research, and the steps bought
    pub fspent: [u32; 3],
    pub ftier: [u32; 3],
    /// bulletins already shown (see `Bulletin::bit`)
    pub told: u8,
}

impl Campaign {
    pub fn new(diff: usize) -> Campaign {
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(1);
        Campaign::with_seed(diff, seed)
    }
    pub fn with_seed(diff: usize, seed: u64) -> Campaign {
        Campaign::with_picks(diff, seed, pick_cities(seed))
    }
    fn with_picks(diff: usize, seed: u64, picks: Vec<usize>) -> Campaign {
        let cities = (0..picks.len())
            .map(|i| {
                let mut s = CityState::fresh(seed, i);
                if i == 0 {
                    // New York is where it starts: a middling city, not about to fall
                    s.turns = 4;
                    s.start = 4;
                    s.heat = 1.3;
                    s.civ = 1.0;
                }
                s
            })
            .collect();
        Campaign {
            fac: ZOMBIES,
            diff,
            seed,
            picks,
            cities,
            current: 0,
            fought: 0,
            lost: 0,
            crew: vec![Crew::default()],
            units: Vec::new(),
            flown: 0,
            kills: 0,
            aliens: Vec::new(),
            fkills: [0; 3],
            fspent: [0; 3],
            ftier: [0; 3],
            told: 0,
        }
    }
    /// Fired after three failed defences.
    pub fn fired(&self) -> bool {
        self.lost >= MAX_LOST
    }
    pub fn open(&self, i: usize) -> bool {
        self.cities.get(i).is_some_and(|c| c.fate == Fate::Open)
    }
    /// All three armies wiped out by research.
    pub fn saved_world(&self) -> bool {
        (0..3).all(|a| self.wiped(a))
    }
    /// Over: fired, the world saved, or no city left standing to defend.
    pub fn finished(&self) -> bool {
        self.fired() || self.saved_world() || !(0..self.cities.len()).any(|i| self.open(i))
    }
    pub fn held(&self) -> usize {
        self.cities.iter().filter(|c| c.fate == Fate::Held).count()
    }
    /// Cities the hordes have taken, fought for or not.
    pub fn fallen(&self) -> usize {
        self.cities.iter().filter(|c| matches!(c.fate, Fate::Fallen | Fate::Lost)).count()
    }
    /// The campaign's city `i`.
    pub fn info(&self, i: usize) -> &'static City {
        &POOL[self.picks[i.min(self.picks.len() - 1)]]
    }
    /// Every city in the campaign, in its own order.
    pub fn all(&self) -> impl Iterator<Item = &'static City> + '_ {
        self.picks.iter().map(|&p| &POOL[p])
    }
    pub fn city(&self) -> &'static City {
        self.info(self.current)
    }
    pub fn state(&self) -> &CityState {
        &self.cities[self.current.min(self.cities.len() - 1)]
    }
    /// Millions of people still alive in city `i`.
    pub fn alive(&self, i: usize) -> f32 {
        self.info(i).pop * self.cities[i].share()
    }

    // ---- the other armies

    /// The turn (cities fought) each army reaches Earth: zombies from the start, the aliens and
    /// orcs some time in the first six turns, in a seeded order.
    pub fn arrives(&self, a: usize) -> u32 {
        let first = 2 + (hash(self.seed, 41) * 3.0) as u32;
        let second = (first + 1 + (hash(self.seed, 42) * 2.0) as u32).min(6);
        let aliens_first = hash(self.seed, 43) < 0.5;
        match a {
            ALIENS if aliens_first => first,
            ORCS if !aliens_first => first,
            ALIENS | ORCS => second,
            _ => 0,
        }
    }
    pub fn arrived(&self, a: usize) -> bool {
        self.fought >= self.arrives(a)
    }
    pub fn wiped(&self, a: usize) -> bool {
        self.ftier[a.min(2)] as usize >= FTIERS
    }
    /// The army is on Earth and not wiped out yet.
    pub fn present(&self, a: usize) -> bool {
        self.arrived(a) && !self.wiped(a)
    }
    /// Swarm indices attacking city `i`.
    pub fn swarms_at(&self, i: usize) -> impl Iterator<Item = usize> + '_ {
        (0..self.aliens.len()).filter(move |&k| self.aliens[k].city == i)
    }
    /// Who is attacking city `i`: aliens if a swarm is there, orcs if their portals are, zombies otherwise.
    pub fn army_at(&self, i: usize) -> usize {
        if self.swarms_at(i).next().is_some() {
            ALIENS
        } else if self.cities.get(i).is_some_and(|c| c.orcs) && self.present(ORCS) {
            ORCS
        } else {
            ZOMBIES
        }
    }
    /// The battle's army in the city picked.
    pub fn army(&self) -> usize {
        self.army_at(self.current)
    }

    // ---- difficulty

    /// How much bigger the horde is in city `i` than on the chosen difficulty: the city's own
    /// heat, how far the campaign has come, how many cities have fallen, a swarm's strength and
    /// what research has taken off its army.
    pub fn horde_mult_at(&self, i: usize) -> f32 {
        let a = self.army_at(i);
        let swarm = self.swarms_at(i).map(|k| self.aliens[k].power).fold(1.0f32, f32::max);
        self.cities[i].heat * self.progress() * self.doom() * swarm * self.size_mult(a)
    }
    /// The campaign's part of the horde's size: ×0.55 at the start, 10% more for every city held.
    pub fn progress(&self) -> f32 {
        0.55 * 1.10f32.powi(self.held() as i32)
    }
    /// ×1.08 for every city lost or fallen.
    pub fn doom(&self) -> f32 {
        DOOM.powi(self.fallen() as i32)
    }
    pub fn horde_mult(&self) -> f32 {
        self.horde_mult_at(self.current)
    }
    /// The difficulty of city `i` as a whole number (×0.25 of horde each): what the city list
    /// shows, and how many of its army roam around it on the globe.
    pub fn level(&self, i: usize) -> u32 {
        ((self.horde_mult_at(i) * crate::app::DIFFS[self.diff.min(3)].1 * 4.0).round() as u32).max(1)
    }
    /// Money paid to every player for holding city `i`: the harder the city, the more.
    pub fn city_pay(&self, i: usize) -> u32 {
        150 * self.level(i)
    }
    /// Research steps bought against army `a` that weaken it (the last one wipes it out instead).
    fn steps(&self, a: usize) -> u32 {
        self.ftier[a.min(2)].min(FTIERS as u32 - 1)
    }
    /// Its hordes' size after research.
    pub fn size_mult(&self, a: usize) -> f32 {
        1.0 - F_SIZE * self.steps(a) as f32
    }
    /// Its units' health and attack after research.
    pub fn potency(&self, a: usize) -> f32 {
        1.0 - F_STATS * self.steps(a) as f32
    }
    /// Seconds to hold out before the evac: 7 minutes, 20 s more per city fought, at most 11.
    pub fn hold_secs(&self) -> f32 {
        (7.0 * 60.0 + 20.0 * self.fought as f32).min(11.0 * 60.0)
    }
    /// Civilians on the map in city `i`: about one for every ~1,700 people still alive there, more
    /// or fewer depending on the city.
    pub fn civilians_at(&self, i: usize) -> usize {
        (self.alive(i) * 600.0 * self.cities[i].civ).clamp(1200.0, 20000.0) as usize
    }
    /// The open city closest to falling.
    pub fn most_urgent(&self) -> Option<usize> {
        (0..self.cities.len()).filter(|i| self.open(*i)).min_by_key(|i| self.cities[*i].turns)
    }
    pub fn select(&mut self, i: usize) {
        if self.open(i) {
            self.current = i;
        }
    }

    // ---- research

    /// Player `p`'s research points and unlocks.
    pub fn crew(&self, p: usize) -> &Crew {
        self.crew.get(p).unwrap_or(&NO_CREW)
    }
    pub fn crew_mut(&mut self, p: usize) -> &mut Crew {
        let p = p.min(CREWS - 1);
        if self.crew.len() <= p {
            self.crew.resize(p + 1, Crew::default());
        }
        &mut self.crew[p]
    }
    pub fn points(&self, p: usize) -> u32 {
        self.crew(p).points
    }
    pub fn has_tech(&self, p: usize, t: Tech) -> bool {
        self.crew(p).tech.contains(&t)
    }
    /// Not unlocked yet, but what it needs is: it can be bought once there are points for it.
    pub fn open_tech(&self, p: usize, t: Tech) -> bool {
        !self.has_tech(p, t) && t.needs().is_none_or(|n| self.has_tech(p, n))
    }
    pub fn can_buy(&self, p: usize, t: Tech) -> bool {
        self.open_tech(p, t) && self.points(p) >= t.cost()
    }
    /// Player `p` unlocks `t` with their own points.
    pub fn buy(&mut self, p: usize, t: Tech) -> bool {
        if !self.can_buy(p, t) {
            return false;
        }
        let c = self.crew_mut(p);
        c.points -= t.cost();
        c.tech.push(t);
        true
    }
    /// Points for research into army `a`, from its kills.
    pub fn fpoints(&self, a: usize) -> u32 {
        ((self.fkills[a] / KILLS_PER_POINT[a]) as u32).saturating_sub(self.fspent[a])
    }
    /// The next step of research into army `a`, if any are left.
    pub fn fnext(&self, a: usize) -> Option<&'static FTech> {
        FTECH[a].get(self.ftier[a] as usize)
    }
    /// Why army `a`'s next step can't be bought yet, if it can't (points aside).
    pub fn fblock(&self, a: usize) -> Option<&'static str> {
        if !self.arrived(a) {
            Some("Not on Earth yet")
        } else if self.wiped(a) {
            Some("Wiped out")
        } else if a == ZOMBIES && self.ftier[a] as usize == FTIERS - 1 && !(self.wiped(ALIENS) && self.wiped(ORCS)) {
            Some("Needs the aliens and orcs gone first")
        } else {
            None
        }
    }
    pub fn can_fbuy(&self, a: usize) -> bool {
        self.fblock(a).is_none() && self.fnext(a).is_some_and(|t| self.fpoints(a) >= t.cost)
    }
    /// Buys the next step against army `a` (shared by everybody in co-op). The last one wipes it out.
    pub fn fbuy(&mut self, a: usize) -> bool {
        if !self.can_fbuy(a) {
            return false;
        }
        self.fspent[a] += self.fnext(a).map(|t| t.cost).unwrap_or(0);
        self.ftier[a] += 1;
        if self.wiped(a) {
            match a {
                ALIENS => self.aliens.clear(),
                ORCS => self.cities.iter_mut().for_each(|c| c.orcs = false),
                _ => {}
            }
        }
        true
    }
    /// The next bulletin the campaign screen should show, if any, leaving out those in `seen`
    /// as well as those already told.
    pub fn bulletin(&self, seen: u8) -> Option<Bulletin> {
        let all = [Bulletin::Arrived(ALIENS), Bulletin::Arrived(ORCS), Bulletin::Wiped(ALIENS), Bulletin::Wiped(ORCS), Bulletin::Wiped(ZOMBIES)];
        all.into_iter().find(|b| {
            (self.told | seen) & b.bit() == 0
                && match *b {
                    Bulletin::Arrived(a) => self.arrived(a) && self.fought > 0,
                    Bulletin::Wiped(a) => self.wiped(a),
                }
        })
    }
    pub fn tell(&mut self, b: Bulletin) {
        self.told |= b.bit();
    }

    /// What player `p`'s tech tree lets them build, recruit and use in a city.
    pub fn allowed(&self, p: usize) -> Allowed {
        let mut a = Allowed { builds: START_BUILDS.to_vec(), units: vec![U::Rifle], abilities: Vec::new() };
        // storage is never locked away: scrap depots from the start, ammo depots with the plant
        a.builds.push(S::ScrapDepot);
        for t in &self.crew(p).tech {
            match *t {
                Tech::Build(s) => {
                    a.builds.push(s);
                    if shoots(s) && !a.builds.contains(&S::Plant) {
                        a.builds.push(S::Plant);
                        a.builds.push(S::AmmoDepot);
                    }
                    if s == S::Silo {
                        a.abilities.push(A::Icbm);
                    }
                }
                Tech::Unit(u) => a.units.push(u),
                Tech::Ability(x) => a.abilities.push(x),
                _ => {}
            }
        }
        a
    }
    /// Player `p` recruits a squad for everybody's army with their own points.
    pub fn recruit(&mut self, p: usize, u: U) -> bool {
        let c = recruit_cost(u);
        if self.points(p) < c || (u != U::Rifle && !self.has_tech(p, Tech::Unit(u))) {
            return false;
        }
        self.crew_mut(p).points -= c;
        self.units.push((u, udef(u).n as u32));
        true
    }
    /// (squads, soldiers) of one kind of troops.
    pub fn army_size(&self, u: U) -> (usize, u32) {
        let v: Vec<u32> = self.units.iter().filter(|x| x.0 == u).map(|x| x.1).collect();
        (v.len(), v.iter().sum())
    }

    /// Sets a fresh game up as the chosen city: its people, research, the hold timer and the army.
    /// Everything read here is saved by `to_text`, so co-op guests set up the same game.
    pub fn apply(&self, g: &mut Game) {
        g.campaign = true;
        g.goal = self.hold_secs();
        g.potency = self.potency(g.fac.min(2));
        g.set_civilians(self.civilians_at(self.current));
        // every player gets their own unlocks and upgrades
        for o in 0..g.perks.len() {
            g.perks[o].allowed = Some(self.allowed(o));
            for t in &self.crew(o).tech {
                match *t {
                    Tech::Research(r) => g.complete_research(o, r),
                    Tech::Up(u) => g.perks[o].ups.push(u),
                    _ => {}
                }
            }
        }
        // the starter bases keep only what their players have unlocked
        for i in 0..g.structs.len() {
            if !g.structs[i].dead && !g.build_ok(g.structs[i].owner as usize, g.structs[i].t) {
                g.kill_struct(i);
            }
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

    /// After a city, one turn of the war. Held: each player banks the civilians they flew out and
    /// the city's pay; lost: nothing. Survivors march on either way. Then every other city loses a
    /// turn (two under an alien attack) and grows harder, cities that run out fall and new ones
    /// join, the aliens fly on and the orcs spread, the other armies may arrive, and the most
    /// urgent city is picked next.
    pub fn harvest(&mut self, g: &Game) {
        let cur = self.current;
        self.fkills[g.fac.min(2)] += g.kills;
        if g.won {
            let pay = self.city_pay(cur);
            // each player's research points are the civilians flown out from their own evac points
            for (o, k) in g.perks.iter().enumerate() {
                let c = self.crew_mut(o);
                c.points += k.flown;
                c.money += pay;
            }
            self.flown += g.extracted;
            self.cities[cur].fate = Fate::Held;
        } else {
            self.cities[cur].fate = Fate::Lost;
            self.cities[cur].turns = 0;
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
        self.advance(cur, g.won);
    }

    /// Plays `n` turns without the battles, holding three cities in four: for tests and the
    /// screenshot hooks.
    pub fn play_turns(&mut self, n: u32) {
        for k in 0..n {
            if self.finished() {
                return;
            }
            let cur = self.current;
            let held = k % 4 != 3;
            if held {
                self.cities[cur].fate = Fate::Held;
            } else {
                self.cities[cur].fate = Fate::Lost;
                self.cities[cur].turns = 0;
                self.lost += 1;
            }
            self.fought += 1;
            self.advance(cur, held);
        }
    }

    /// The world's turn after fighting in city `cur` (held or not).
    fn advance(&mut self, cur: usize, held: bool) {
        let turn = self.fought;
        let mut fell = Vec::new();
        if !held {
            fell.push(cur);
        }
        for i in 0..self.cities.len() {
            if !self.open(i) {
                continue;
            }
            let bite = if self.swarms_at(i).next().is_some() { ALIEN_BITE } else { 1 };
            let c = &mut self.cities[i];
            c.turns = c.turns.saturating_sub(bite);
            c.heat += c.ramp;
            if c.turns == 0 {
                c.fate = Fate::Fallen;
                fell.push(i);
            }
        }
        // swarms nobody fought grow
        for s in self.aliens.iter_mut().filter(|s| s.city != cur) {
            s.power += ALIEN_GROWTH;
        }
        // the city fought: the aliens there fly on if it was held (minor swarms die), and so does
        // any swarm whose city fell
        self.aliens_leave(cur, held, turn);
        for &i in fell.iter().filter(|i| **i != cur) {
            self.aliens_leave(i, false, turn);
        }
        for _ in &fell {
            self.add_city();
        }
        // orcs open portals somewhere new now and then, and always somewhere
        let orc_city = (0..self.cities.len()).any(|i| self.open(i) && self.cities[i].orcs);
        if self.present(ORCS) && turn > self.arrives(ORCS) && (!orc_city || hash(self.seed, 7000 + turn as u64) < ORC_SPREAD) {
            self.orcs_to(1, 7100 + turn as u64);
        }
        // the other armies arrive
        if turn == self.arrives(ORCS) && self.present(ORCS) {
            self.orcs_to(2, 7200);
        }
        if turn == self.arrives(ALIENS) && self.present(ALIENS) {
            if let Some(i) = self.pick_open(7300, None) {
                self.aliens.push(Swarm { city: i, from: None, power: 1.0, major: true, moved: turn });
            }
        }
        if let Some(i) = self.most_urgent() {
            self.current = i;
        }
    }

    /// Orc portals open in `n` more cities that have none (and no aliens).
    fn orcs_to(&mut self, n: usize, k: u64) {
        for j in 0..n as u64 {
            let free: Vec<usize> = (0..self.cities.len()).filter(|&i| self.open(i) && !self.cities[i].orcs && self.swarms_at(i).next().is_none()).collect();
            if free.is_empty() {
                return;
            }
            let i = free[((hash(self.seed, k + j) * free.len() as f32) as usize).min(free.len() - 1)];
            self.cities[i].orcs = true;
        }
    }

    /// An open city with no swarm in it, picked by the seed; with `near`, one of the three closest to it.
    fn pick_open(&self, k: u64, near: Option<usize>) -> Option<usize> {
        let mut free: Vec<usize> = (0..self.cities.len()).filter(|&i| self.open(i) && self.swarms_at(i).next().is_none()).collect();
        if let Some(n) = near {
            let from = self.info(n);
            free.sort_by(|a, b| from.km_to(self.info(*a)).total_cmp(&from.km_to(self.info(*b))));
            free.truncate(3);
        }
        if free.is_empty() {
            return None;
        }
        Some(free[((hash(self.seed, k) * free.len() as f32) as usize).min(free.len() - 1)])
    }

    /// The swarms in city `i` after a turn: held, the major one flies to a city nearby and minor
    /// ones die; taken, every swarm flies on and the major one leaves a minor swarm behind to roam.
    fn aliens_leave(&mut self, i: usize, held: bool, turn: u32) {
        let here: Vec<usize> = self.swarms_at(i).collect();
        let mut spawn = false;
        let mut gone = Vec::new();
        for (n, &k) in here.iter().enumerate() {
            let major = self.aliens[k].major;
            if held && !major {
                gone.push(k);
                continue;
            }
            spawn |= !held && major;
            match self.pick_open(7400 + turn as u64 * 16 + n as u64, Some(i)) {
                Some(to) => {
                    let s = &mut self.aliens[k];
                    s.from = Some(i);
                    s.city = to;
                    s.moved = turn;
                }
                None => gone.push(k),
            }
        }
        gone.sort_unstable();
        for k in gone.into_iter().rev() {
            self.aliens.remove(k);
        }
        let minors = self.aliens.iter().filter(|s| !s.major).count();
        if spawn && minors < MAX_MINOR {
            if let Some(to) = self.pick_open(7600 + turn as u64, Some(i)) {
                self.aliens.push(Swarm { city: to, from: Some(i), power: 0.8, major: false, moved: turn });
            }
        }
    }

    /// Another city from the pool joins the war (if any are left), as far from the others as it can be.
    fn add_city(&mut self) {
        let order = pool_order(self.seed);
        let spare: Vec<usize> = order.into_iter().filter(|i| !self.picks.contains(i)).collect();
        let far = |i: &usize| self.picks.iter().all(|&p| POOL[p].km_to(&POOL[*i]) >= SPREAD_KM / 2.0);
        let Some(&next) = spare.iter().find(|i| far(i)).or(spare.first()) else { return };
        let slot = self.cities.len();
        let mut s = CityState::fresh(self.seed, slot);
        s.orcs = self.present(ORCS) && hash(self.seed, 7700 + slot as u64) < ORC_NEW_CITY;
        self.picks.push(next);
        self.cities.push(s);
    }
}

// ------------------------------------------------------------------------------------------ saving

fn save_path() -> PathBuf {
    data_dir().join("campaign.txt")
}

/// The save format: campaigns from before turns (version 1, with days) start over.
const VERSION: u64 = 2;

pub fn load() -> Option<Campaign> {
    let t = std::fs::read_to_string(save_path()).ok()?;
    t.lines().any(|l| l.trim() == format!("version={}", VERSION)).then(|| from_text(&t))
}

pub fn save(c: &Campaign) {
    let _ = std::fs::write(save_path(), to_text(c));
}

/// Throws the saved campaign away.
pub fn clear() {
    let _ = std::fs::remove_file(save_path());
}

pub fn from_text(t: &str) -> Campaign {
    let field = |k: &str| t.lines().find_map(|l| l.split_once('=').filter(|(a, _)| a.trim() == k).map(|(_, v)| v.trim().to_string()));
    let num = |k: &str| field(k).and_then(|v| v.parse::<u64>().ok());
    let three = |k: &str| -> [u64; 3] {
        let v: Vec<u64> = field(k).unwrap_or_default().split(',').filter_map(|x| x.trim().parse().ok()).collect();
        [0, 1, 2].map(|i| v.get(i).copied().unwrap_or(0))
    };
    let picks: Vec<usize> = match field("picks") {
        Some(v) => v.split(',').filter_map(|x| x.trim().parse::<usize>().ok()).filter(|i| *i < POOL.len()).collect(),
        None => (0..CAMPAIGN_CITIES).collect(),
    };
    let picks = if picks.is_empty() { vec![0] } else { picks };
    let mut c = Campaign::with_picks(num("difficulty").unwrap_or(1).min(3) as usize, num("seed").unwrap_or(1), picks);
    c.fought = num("fought").unwrap_or(0) as u32;
    c.lost = num("lost").unwrap_or(0) as u32;
    // the host's crew, then any co-op players' (points1=, tech1=, money1=, ...)
    for p in 0..CREWS {
        let k = |name: &str| if p == 0 { name.to_string() } else { format!("{}{}", name, p) };
        let (pts, tech) = (num(&k("points")), field(&k("tech")));
        if p > 0 && pts.is_none() && tech.is_none() {
            continue;
        }
        let money = num(&k("money"));
        let cr = c.crew_mut(p);
        cr.points = pts.unwrap_or(0) as u32;
        cr.money = money.unwrap_or(0) as u32;
        if let Some(v) = tech {
            cr.tech = v.split(',').filter_map(|x| x.trim().parse::<usize>().ok()).filter_map(|i| TECH.get(i).copied()).collect();
        }
    }
    c.flown = num("flown").unwrap_or(0) as u32;
    c.kills = num("kills").unwrap_or(0);
    c.fkills = three("fkills");
    c.fspent = three("fspent").map(|x| x as u32);
    c.ftier = three("ftier").map(|x| (x as u32).min(FTIERS as u32));
    c.told = num("told").unwrap_or(0) as u8;
    if let Some(v) = field("cities") {
        for (s, x) in c.cities.iter_mut().zip(v.split(',')) {
            let p: Vec<&str> = x.split(':').collect();
            if let [tu, st, he, ra, cv, o, f] = p[..] {
                s.turns = tu.parse().unwrap_or(s.turns);
                s.start = st.parse().unwrap_or(s.start);
                s.heat = he.parse().unwrap_or(s.heat);
                s.ramp = ra.parse().unwrap_or(s.ramp);
                s.civ = cv.parse().unwrap_or(s.civ);
                s.orcs = o == "1";
                s.fate = match f {
                    "h" => Fate::Held,
                    "l" => Fate::Lost,
                    "f" => Fate::Fallen,
                    _ => Fate::Open,
                };
            }
        }
    }
    c.current = num("current").unwrap_or(0).min(c.cities.len() as u64 - 1) as usize;
    if let Some(v) = field("aliens") {
        c.aliens = v
            .split(',')
            .filter_map(|x| {
                let p: Vec<&str> = x.trim().split(':').collect();
                let [city, from, power, major, moved] = p[..] else { return None };
                let city: usize = city.parse().ok()?;
                (city < c.cities.len()).then_some(())?;
                Some(Swarm { city, from: from.parse().ok().filter(|f: &usize| *f < c.cities.len()), power: power.parse().ok()?, major: major == "1", moved: moved.parse().ok()? })
            })
            .collect();
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
    let join = |v: &mut dyn Iterator<Item = String>| v.collect::<Vec<_>>().join(",");
    let picks = join(&mut c.picks.iter().map(|i| i.to_string()));
    let tech_of = |cr: &Crew| -> String { cr.tech.iter().filter_map(|t| TECH.iter().position(|x| x == t)).map(|i| i.to_string()).collect::<Vec<_>>().join(",") };
    let mut others = String::new();
    for (p, cr) in c.crew.iter().enumerate().skip(1) {
        others += &format!("points{}={}\ntech{}={}\nmoney{}={}\n", p, cr.points, p, tech_of(cr), p, cr.money);
    }
    let units = join(&mut c.units.iter().map(|(u, n)| format!("{}{}", if *u == U::Rifle { 'r' } else { 't' }, n)));
    let cities = join(&mut c.cities.iter().map(|s| {
        let f = match s.fate {
            Fate::Open => "o",
            Fate::Held => "h",
            Fate::Lost => "l",
            Fate::Fallen => "f",
        };
        format!("{}:{}:{:?}:{:?}:{:?}:{}:{}", s.turns, s.start, s.heat, s.ramp, s.civ, s.orcs as u8, f)
    }));
    let aliens = join(&mut c.aliens.iter().map(|s| format!("{}:{}:{:?}:{}:{}", s.city, s.from.map(|f| f as i64).unwrap_or(-1), s.power, s.major as u8, s.moved)));
    let three = |v: [u64; 3]| format!("{},{},{}", v[0], v[1], v[2]);
    format!(
        "version={}\ndifficulty={}\nseed={}\npicks={}\ncurrent={}\nfought={}\nlost={}\npoints={}\nmoney={}\nflown={}\nkills={}\ncities={}\naliens={}\nfkills={}\nfspent={}\nftier={}\ntold={}\ntech={}\nunits={}\n{}",
        VERSION,
        c.diff,
        c.seed,
        picks,
        c.current,
        c.fought,
        c.lost,
        c.crew(0).points,
        c.crew(0).money,
        c.flown,
        c.kills,
        cities,
        aliens,
        three(c.fkills),
        three(c.fspent.map(|x| x as u64)),
        three(c.ftier.map(|x| x as u64)),
        c.told,
        tech_of(c.crew(0)),
        units,
        others
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

/// Maps downloaded from the city pack at once.
const PACK_WORKERS: usize = 3;

#[derive(Default)]
struct PackQueue {
    /// pool indices waiting, the most wanted first
    queue: std::collections::VecDeque<usize>,
    active: Vec<usize>,
    /// not in the pack (or no connection): these come from OpenStreetMap instead
    failed: Vec<usize>,
}

/// Downloads campaign maps from the `cities` GitHub release in the background, a few at once, the
/// ones asked for most recently first.
pub struct Pack {
    q: Arc<(Mutex<PackQueue>, std::sync::Condvar)>,
}

impl Pack {
    pub fn start() -> Pack {
        let q: Arc<(Mutex<PackQueue>, std::sync::Condvar)> = Arc::default();
        for _ in 0..PACK_WORKERS {
            let q = q.clone();
            std::thread::spawn(move || {
                let a = ureq::AgentBuilder::new()
                    .timeout(std::time::Duration::from_secs(120))
                    .user_agent(concat!("PixelHorde/", env!("CARGO_PKG_VERSION")))
                    .build();
                loop {
                    let i = {
                        let mut g = q.0.lock().unwrap();
                        loop {
                            if let Some(i) = g.queue.pop_front() {
                                g.active.push(i);
                                break i;
                            }
                            g = q.1.wait(g).unwrap();
                        }
                    };
                    let c = &POOL[i];
                    let url = format!("https://github.com/{}/releases/download/cities/{}.z", crate::update::REPO, file_name(c));
                    let got = a.get(&url).call().map_err(|e| e.to_string()).and_then(|r| {
                        let mut z = Vec::new();
                        r.into_reader().read_to_end(&mut z).map_err(|e| e.to_string())?;
                        let b = miniz_oxide::inflate::decompress_to_vec(&z).map_err(|_| "damaged download".to_string())?;
                        storage::decode(&b).ok_or("damaged map")?;
                        Ok(b)
                    });
                    let ok = got.is_ok_and(|b| std::fs::write(map_path(c), b).is_ok());
                    let mut g = q.0.lock().unwrap();
                    g.active.retain(|x| *x != i);
                    if !ok {
                        g.failed.push(i);
                    }
                }
            });
        }
        let p = Pack { q };
        p.want(&[&POOL[0]]);
        p
    }
    /// Queues these maps ahead of everything else, in this order.
    pub fn want(&self, cities: &[&'static City]) {
        let mut g = self.q.0.lock().unwrap();
        for c in cities.iter().rev() {
            let Some(i) = POOL.iter().position(|p| std::ptr::eq(p, *c)) else { continue };
            if map_path(c).exists() || g.active.contains(&i) || g.failed.contains(&i) {
                continue;
            }
            g.queue.retain(|x| *x != i);
            g.queue.push_front(i);
        }
        self.q.1.notify_all();
    }
    /// Still on its way from the pack.
    pub fn pending(&self, c: &City) -> bool {
        let g = self.q.0.lock().unwrap();
        POOL.iter().position(|p| std::ptr::eq(p, c)).is_some_and(|i| g.queue.contains(&i) || g.active.contains(&i))
    }
    /// The first of these still downloading, if any.
    pub fn getting<'a>(&self, cities: impl IntoIterator<Item = &'a City>) -> Option<&'a City> {
        cities.into_iter().find(|c| self.pending(c))
    }
}

/// `PH_FETCH=<dir>`: downloads campaign cities from OpenStreetMap into `<dir>` as compressed maps
/// for the `cities` release. Run by the GitHub workflow, not by players. `PH_FETCH_ONLY` names
/// cities, `PH_FETCH_HAVE` lists files already in the release, which are skipped, and
/// `PH_FETCH_PART=k/n` takes every n-th of the rest from the k-th, so n jobs split the work.
/// Returns false if any city failed.
pub fn fetch_all(out: &Path) -> bool {
    let _ = std::fs::create_dir_all(out);
    let env = |k: &str| std::env::var(k).ok().filter(|s| !s.trim().is_empty());
    let only = env("PH_FETCH_ONLY");
    let have = env("PH_FETCH_HAVE").unwrap_or_default();
    let part = env("PH_FETCH_PART").and_then(|p| {
        let (k, n) = p.split_once('/')?;
        Some((k.trim().parse::<usize>().ok()?, n.trim().parse::<usize>().ok()?.max(1)))
    });
    let mut ok = true;
    // the cities still to fetch, then this job's share of them, so the jobs split the work evenly
    let todo: Vec<&City> = POOL
        .iter()
        .filter(|c| match &only {
            Some(o) => o.split(',').any(|x| x.trim().eq_ignore_ascii_case(c.name)),
            None => !have.split_whitespace().any(|h| h == format!("{}.z", file_name(c))),
        })
        .collect();
    for (i, c) in todo.into_iter().enumerate() {
        if part.is_some_and(|(k, n)| i % n != k % n) {
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
        let mut c = Campaign::with_seed(1, 77);
        c.fought = 3;
        c.current = 4;
        c.crew_mut(0).tech = vec![Tech::Research(R::Armour), Tech::Build(S::Gun), Tech::Research(R::Barrels), Tech::Ability(A::Rescue), Tech::Research(R::Rescue), Tech::Up(Up::GunAp)];
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
        let r = a.res(0);
        assert!(r.armour && r.barrels && r.rescue && !r.nuke);
        assert!(!a.can_research(0, R::Nuke));
        assert!(a.build_ok(0, S::Gun) && a.build_ok(0, S::Plant) && a.build_ok(0, S::Pad) && !a.build_ok(0, S::Cannon) && !a.build_ok(0, S::Lab));
        assert!(a.unit_ok(0, U::Rifle) && !a.unit_ok(0, U::Tank));
        assert!(!a.can_use(0, A::Air) && a.has_up(0, Up::GunAp) && a.dmg_mult(S::Gun, 0) > 1.0);
        assert!(a.structs.iter().all(|s| s.dead || a.build_ok(0, s.t)));
        for _ in 0..400 {
            a.tick();
            b.tick();
        }
        assert_eq!(a.checksum(), b.checksum());
        let mut next = c.clone();
        a.extracted = 77;
        a.perks[0].flown = 77;
        a.won = true;
        next.harvest(&a);
        assert_eq!(next.fought, 4);
        assert_eq!(next.points(0), 77);
        assert_eq!(next.cities[4].fate, Fate::Held);
        assert_ne!(next.current, 4);
        let alive: usize = a.groups.iter().filter(|g| !g.dead).count();
        assert_eq!(next.units.len(), alive);
    }

    /// A lost city pays nothing, and the third one ends the campaign.
    #[test]
    fn fired_after_three() {
        let mut c = Campaign::with_seed(1, 5);
        let mut g = Game::new(crate::citygen::generate(4), 0, 1.0, 5);
        g.extracted = 500;
        g.perks[0].flown = 500;
        for k in 0..3 {
            assert!(!c.finished());
            c.harvest(&g);
            assert_eq!(c.lost, k + 1);
        }
        assert_eq!(c.points(0), 0);
        assert!(c.fired() && c.finished());
    }

    /// Cities run out of turns while you're elsewhere; fallen ones make the rest harder and new
    /// cities join; you can't save them all.
    #[test]
    fn world_moves_on() {
        let mut c = Campaign::with_seed(1, 9);
        let g = {
            let mut g = Game::new(crate::citygen::generate(4), 0, 1.0, 5);
            g.won = true;
            g
        };
        let other = (1..c.cities.len()).find(|i| *i != c.current).unwrap();
        let (t0, h0, m0) = (c.cities[other].turns, c.cities[other].heat, c.horde_mult_at(other));
        c.harvest(&g);
        assert_eq!(c.cities[other].turns, t0 - 1);
        assert!(c.cities[other].heat > h0 && c.horde_mult_at(other) > m0);
        assert!(c.crew(0).money > 0, "a held city pays");
        let mut held = 0;
        for _ in 0..40 {
            if c.finished() {
                break;
            }
            c.harvest(&g);
            held += 1;
        }
        assert!(c.cities.iter().any(|s| s.fate == Fate::Fallen), "some fall while you're elsewhere");
        assert!(c.cities.len() > CAMPAIGN_CITIES, "fallen cities are replaced");
        assert!(c.doom() > 1.0);
        assert_eq!(c.held(), held + 1);
        // cities differ: turns, heat, ramp and civilians all vary
        let c = Campaign::with_seed(1, 9);
        for f in [|s: &CityState| s.turns as f32, |s: &CityState| s.heat, |s: &CityState| s.ramp, |s: &CityState| s.civ] {
            let v: Vec<f32> = c.cities[1..].iter().map(f).collect();
            let (lo, hi) = v.iter().fold((f32::MAX, f32::MIN), |(a, b), x| (a.min(*x), b.max(*x)));
            assert!(hi > lo * 1.3, "{:?}", v);
        }
        // a harder city pays more
        let (easy, hard) = (1..c.cities.len()).fold((1, 1), |(e, h), i| (if c.level(i) < c.level(e) { i } else { e }, if c.level(i) > c.level(h) { i } else { h }));
        assert!(c.city_pay(hard) > c.city_pay(easy));
    }

    /// The aliens and the orcs both arrive by turn six, with a bulletin each, in every campaign.
    #[test]
    fn armies_arrive() {
        for seed in 0..60 {
            let mut c = Campaign::with_seed(1, seed);
            assert!(!c.present(ALIENS) && !c.present(ORCS));
            assert_eq!(c.bulletin(0), None);
            let (a, o) = (c.arrives(ALIENS), c.arrives(ORCS));
            assert!(a >= 2 && o >= 2 && a <= 6 && o <= 6 && a != o, "{} {}", a, o);
            c.play_turns(6);
            if c.finished() {
                continue;
            }
            assert!(c.present(ALIENS) && c.present(ORCS));
            assert!(!c.aliens.is_empty(), "seed {}", seed);
            assert!((0..c.cities.len()).any(|i| c.open(i) && c.army_at(i) == ORCS), "seed {}", seed);
            let b = c.bulletin(0).unwrap();
            c.tell(b);
            let b2 = c.bulletin(0).unwrap();
            assert_ne!(b, b2);
            assert_eq!(c.bulletin(b2.bit()), None);
        }
    }

    /// The alien swarm attacks one city, eats it twice as fast, grows when left alone, flies on
    /// when the city is held and leaves a minor swarm behind when it takes one.
    #[test]
    fn aliens_fly_on() {
        let mut c = Campaign::with_seed(1, 3);
        c.play_turns(c.arrives(ALIENS));
        let k = c.aliens.iter().position(|s| s.major).unwrap();
        let at = c.aliens[k].city;
        assert_eq!(c.army_at(at), ALIENS);
        // left alone a turn: two turns gone, stronger
        let elsewhere = (0..c.cities.len()).find(|i| c.open(*i) && *i != at && c.swarms_at(*i).next().is_none()).unwrap();
        c.select(elsewhere);
        let (t0, p0) = (c.cities[at].turns, c.aliens[k].power);
        let mut won = Game::new(crate::citygen::generate(4), 0, 1.0, 5);
        won.won = true;
        c.harvest(&won);
        if c.open(at) {
            assert_eq!(c.cities[at].turns, t0.saturating_sub(2));
            assert!(c.aliens[k].power > p0);
            // held: the major swarm flies on to another city, trailing from this one
            c.select(at);
            won.fac = ALIENS;
            won.kills = 5000;
            c.harvest(&won);
            let s = c.aliens.iter().find(|s| s.major).unwrap();
            assert_ne!(s.city, at);
            assert_eq!((s.from, s.moved), (Some(at), c.fought));
            assert_eq!(c.fkills[ALIENS], 5000);
        }
        // taken: it flies on and a minor swarm splits off
        let mut c = Campaign::with_seed(1, 3);
        c.play_turns(c.arrives(ALIENS));
        let at = c.aliens[0].city;
        c.select(at);
        let lost = Game::new(crate::citygen::generate(4), 1, 1.0, 5);
        c.harvest(&lost);
        assert_eq!(c.cities[at].fate, Fate::Lost);
        assert!(c.aliens.iter().any(|s| !s.major) && c.aliens.iter().all(|s| s.city != at));
        // a minor swarm dies when its city is held
        let m = c.aliens.iter().find(|s| !s.major).unwrap().city;
        c.select(m);
        c.harvest(&won);
        assert!(c.aliens.iter().all(|s| s.major || s.city != m));
    }

    /// Kills buy research into an army; each step weakens it and the last wipes it out. The
    /// zombies go last, and wiping them out saves the world.
    #[test]
    fn hunt_them_down() {
        let mut c = Campaign::with_seed(1, 8);
        c.fkills[ZOMBIES] = 10_000_000;
        assert!(c.can_fbuy(ZOMBIES) && !c.can_fbuy(ALIENS), "the aliens aren't here yet");
        let m0 = c.horde_mult_at(0);
        assert!(c.fbuy(ZOMBIES));
        assert!(c.potency(ZOMBIES) < 1.0 && c.horde_mult_at(0) < m0);
        let mut g = Game::new(crate::citygen::generate(4), 0, 1.0, 5);
        c.apply(&mut g);
        assert!(g.stats().hp < FACTIONS[0].stats.hp);
        while c.fbuy(ZOMBIES) {}
        assert_eq!(c.ftier[ZOMBIES], FTIERS as u32 - 1);
        assert_eq!(c.fblock(ZOMBIES), Some("Needs the aliens and orcs gone first"));
        c.play_turns(7);
        c.lost = 0;
        c.fkills = [10_000_000; 3];
        for a in [ALIENS, ORCS] {
            while c.fbuy(a) {}
            assert!(c.wiped(a));
        }
        assert!(c.aliens.is_empty() && (0..c.cities.len()).all(|i| c.army_at(i) == ZOMBIES));
        assert!(!c.saved_world() && c.fbuy(ZOMBIES));
        assert!(c.saved_world() && c.finished());
        assert_eq!(c.bulletin(0), Some(Bulletin::Arrived(ALIENS)));
        let d = from_text(&to_text(&c));
        assert!(d.saved_world());
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

    /// Every campaign starts in New York and spreads over the world; different seeds, different cities.
    #[test]
    fn city_picks() {
        let mut seen = std::collections::HashSet::new();
        for seed in 0..40 {
            let c = Campaign::with_seed(1, seed);
            assert_eq!(c.picks.len(), CAMPAIGN_CITIES);
            assert_eq!(c.picks[0], 0);
            let mut p = c.picks.clone();
            p.sort();
            p.dedup();
            assert_eq!(p.len(), CAMPAIGN_CITIES);
            for (k, a) in c.all().enumerate() {
                assert!(c.all().skip(k + 1).all(|b| a.km_to(b) >= SPREAD_KM), "{} too close", a.name);
            }
            seen.extend(c.picks);
        }
        assert!(seen.len() > 30);
        // an old save, from before the pool, keeps its eleven cities
        let old = from_text("seed=3\n");
        assert_eq!(old.picks, (0..CAMPAIGN_CITIES).collect::<Vec<_>>());
        assert!((POOL[0].km_to(&POOL[11]) - 5570.0).abs() < 60.0);
    }

    #[test]
    fn save_roundtrip() {
        let mut c = veteran();
        c.crew_mut(0).points = 321;
        c.crew_mut(2).points = 45;
        c.crew_mut(2).tech = vec![Tech::Build(S::Flame), Tech::Ability(A::Air)];
        c.kills = 1_234_567;
        c.lost = 2;
        c.crew_mut(2).money = 900;
        c.play_turns(7);
        c.cities[2].heat = 13.371_337;
        c.fkills = [5, 6, 7];
        c.fspent = [1, 0, 2];
        c.ftier = [1, 0, 1];
        c.told = 6;
        assert!(!c.aliens.is_empty());
        let d = from_text(&to_text(&c));
        assert_eq!((d.fac, d.diff, d.seed, d.current, d.fought, d.lost, d.points(0), d.kills), (0, 1, 77, c.current, c.fought, c.lost, 321, 1_234_567));
        assert_eq!((d.aliens.clone(), d.fkills, d.fspent, d.ftier, d.told), (c.aliens.clone(), c.fkills, c.fspent, c.ftier, c.told));
        assert_eq!(d.cities, c.cities);
        assert_eq!(d.picks, c.picks);
        assert_eq!(d.crew, c.crew);
        assert_eq!(d.units, c.units);
        assert_eq!(to_text(&d), to_text(&c));
    }

    #[test]
    fn shop() {
        let mut c = Campaign::new(1);
        c.crew_mut(0).points = 900;
        assert!(!c.buy(0, Tech::Up(Up::GunRapid)), "needs the gun turret first");
        assert!(c.buy(0, Tech::Build(S::Gun)));
        assert!(!c.buy(0, Tech::Build(S::Gun)));
        assert!(c.allowed(0).builds.contains(&S::Plant), "the first turret brings the ammo plant");
        assert!(c.buy(0, Tech::Up(Up::GunRapid)));
        assert!(c.recruit(0, U::Rifle));
        assert!(!c.recruit(0, U::Tank));
        assert!(!c.buy(1, Tech::Build(S::Gun)), "the other player has no points of their own");
        assert_eq!(c.points(0), 900 - Tech::Build(S::Gun).cost() - Tech::Up(Up::GunRapid).cost() - recruit_cost(U::Rifle));
        assert_eq!(c.army_size(U::Rifle), (1, 6));
    }

    /// Co-op: each player gets their own unlocks and upgrades in the city, and earns research
    /// points from the civilians flown out of their own evac points.
    #[test]
    fn coop_crews() {
        let mut c = Campaign::with_seed(1, 21);
        c.crew_mut(0).tech = vec![Tech::Build(S::Gun), Tech::Up(Up::GunAp), Tech::Research(R::Armour)];
        c.crew_mut(1).tech = vec![Tech::Build(S::Flame)];
        let mut g = Game::new(crate::citygen::generate(6), c.fac, c.horde_mult(), 77);
        g.add_player_hqs(2);
        c.apply(&mut g);
        assert_eq!(g.perks.len(), 2);
        assert!(g.build_ok(0, S::Gun) && !g.build_ok(0, S::Flame));
        assert!(g.build_ok(1, S::Flame) && !g.build_ok(1, S::Gun));
        assert!(g.dmg_mult(S::Gun, 0) > 1.0 && g.dmg_mult(S::Gun, 1) == 1.0);
        assert!(g.armour(S::Wall, 0) > g.armour(S::Wall, 1));
        // the second player's starter base is theirs: its guns go (they have none unlocked), their evac point stays
        assert!(g.structs.iter().filter(|s| s.owner == 1 && !s.dead).any(|s| s.t == S::Pad));
        assert!(g.structs.iter().filter(|s| s.owner == 1 && !s.dead).all(|s| g.build_ok(1, s.t)));
        // building as a player makes it theirs
        g.scrap = 1e6;
        g.actor = 1;
        let hq1 = g.structs[g.hqs[1]].center();
        let placed = (0..400).any(|k| {
            let (x, y) = (hq1.x as i32 - 30 + (k % 20) * 3, hq1.y as i32 - 30 + (k / 20) * 3);
            g.place(S::Flame, x, y)
        });
        assert!(placed);
        assert!(!g.place(S::Gun, hq1.x as i32 + 20, hq1.y as i32 + 20), "player 1 has no gun turret unlocked");
        g.actor = 0;
        assert_eq!(g.structs.last().unwrap().owner, 1);
        g.perks[0].flown = 30;
        g.perks[1].flown = 12;
        g.extracted = 42;
        g.won = true;
        c.harvest(&g);
        assert_eq!((c.points(0), c.points(1), c.flown), (30, 12, 42));
    }

    /// The drawn tree: every tech once, each to the right of the one it needs, nothing overlapping.
    #[test]
    fn tree_layout_fits() {
        let (nodes, dims) = tree_layout();
        assert_eq!(nodes.len(), TECH.len());
        for n in &nodes {
            if let Some(p) = n.tech.needs() {
                let par = nodes.iter().find(|m| m.tech == p).unwrap();
                assert_eq!((par.half, par.col + 1), (n.half, n.col), "{:?}", n.tech);
            }
            for m in &nodes {
                if m.tech != n.tech && m.half == n.half && m.col == n.col {
                    assert!((m.row - n.row).abs() >= 0.99, "{:?} and {:?} overlap", m.tech, n.tech);
                }
            }
            assert!(n.row >= 0.0 && n.row < dims[n.half].1 && n.col < dims[n.half].0);
        }
        assert!(nodes.iter().any(|n| n.half == 1));
    }
}

//! Tunable game data: buildings, units, abilities, factions, research and colour themes.

use crate::swarm::{Behaviour, FactionStats};
use macroquad::prelude::Color;

pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0)
}
pub const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum S {
    Wall,
    Gun,
    Cannon,
    Extractor,
    Plant,
    Silo,
    Pad,
    Outpost,
    Lab,
    Hq,
    Flame,
    Rail,
    Helium,
    Artillery,
    Helipad,
    Rescue,
    ScrapDepot,
    AmmoDepot,
    Bunker,
    /// a turret built in the garage (its parts are in `Structure::cu`)
    Custom,
}
pub const BUILD_ORDER: [S; 18] = [
    S::Wall,
    S::Bunker,
    S::Gun,
    S::Cannon,
    S::Flame,
    S::Rail,
    S::Helium,
    S::Artillery,
    S::Extractor,
    S::Plant,
    S::ScrapDepot,
    S::AmmoDepot,
    S::Silo,
    S::Pad,
    S::Rescue,
    S::Helipad,
    S::Outpost,
    S::Lab,
];
/// Seconds a heli pad's gunship circles its target, and its turnaround once home.
pub const HELIPAD_FLIGHT: f32 = 20.0;
pub const HELIPAD_COOLDOWN: f32 = 35.0;
/// How often a rescue station sends out a squad, and how far it looks for civilians.
pub const RESCUE_EVERY: f32 = 25.0;
pub const RESCUE_REACH: f32 = 380.0;
/// Storage without depots, and what each depot adds.
pub const SCRAP_CAP: f32 = 2500.0;
pub const AMMO_CAP: f32 = 1500.0;
pub const DEPOT_SCRAP: f32 = 2500.0;
pub const DEPOT_AMMO: f32 = 2000.0;
/// How long a flame bunker sprays, how long its napalm burns and how hard.
pub const FLAME_SPRAY: f32 = 1.0;
pub const NAPALM_LIFE: f32 = 1.6;
pub const NAPALM_DPS: f32 = 14.0;
/// Seconds a railgun charges before it fires.
pub const RAIL_CHARGE: f32 = 1.2;
/// Scrap an ammo plant burns each second (for 6 ammo, or 10 with Shell presses).
pub const PLANT_SCRAP: f32 = 5.0;

/// HQ levels: what upgrading to each level costs, and how many turrets and squads (logistics) each
/// level supports. Level 5 is as good as unlimited.
pub const HQ_MAX: u8 = 5;
pub const HQ_UP_COST: [f32; 6] = [0.0, 0.0, 1000.0, 2000.0, 3500.0, 6000.0];
pub const LOGI_CAP: [u32; 6] = [0, 15, 25, 40, 60, 100_000];
/// HQ health grows by this share of its base for every level above 1.
pub const HQ_HP_PER_LEVEL: f32 = 0.3;
pub fn logi_cap(level: u8) -> u32 {
    LOGI_CAP[level.clamp(1, HQ_MAX) as usize]
}

/// Seconds a structure takes to build once placed (scaffolding goes up, then sinks away).
pub fn build_time(t: S) -> f32 {
    match t {
        S::Hq => 0.0,
        S::Wall => 1.0,
        _ if def(t).w >= 9 => 3.0,
        _ => 2.5,
    }
}
/// How long the scaffolding takes to sink into the ground once a build is done.
pub const SCAFFOLD_SINK: f32 = 0.7;

pub struct Def {
    pub key: char,
    pub name: &'static str,
    pub w: i32,
    pub h: i32,
    pub hp: f32,
    pub cost: f32,
    pub ht: f32,
    pub top: Color,
    pub side: Color,
    pub range: f32,
    pub rate: f32,
    pub dmg: f32,
    pub rad: f32,
    pub ammo: f32,
    pub bdmg: f32,
    pub turret: bool,
    /// turrets that can't go on a rooftop
    pub ground: bool,
    pub zone: bool,
    pub tip: &'static str,
}

const BASE: Def = Def {
    key: ' ',
    name: "",
    w: 7,
    h: 7,
    hp: 300.0,
    cost: 100.0,
    ht: 3.0,
    top: rgb(200, 200, 200),
    side: rgb(120, 120, 120),
    range: 0.0,
    rate: 0.0,
    dmg: 0.0,
    rad: 0.0,
    ammo: 0.0,
    bdmg: 0.0,
    turret: false,
    ground: false,
    zone: false,
    tip: "",
};

pub fn def(s: S) -> &'static Def {
    static WALL: Def = Def { key: '1', name: "Wall", w: 3, h: 3, hp: 300.0, cost: 3.0, ht: 1.6, top: rgb(139, 147, 159), side: rgb(93, 100, 112), tip: "Drag across a street to seal it. Corners that touch still block. Hordes chew through when there is no other way.", ..BASE };
    static GUN: Def = Def { key: '2', name: "Gun turret", w: 5, h: 5, hp: 220.0, cost: 90.0, ht: 3.0, top: rgb(255, 212, 59), side: rgb(184, 147, 31), range: 46.0, rate: 0.16, dmg: 14.0, rad: 1.3, ammo: 1.0, turret: true, tip: "Fast, 1 ammo per shot. Click a rooftop for +60% range (it falls with the building). Base price inside the base zone; outside it costs more the further from the HQ it goes and is flown in by helicopter.", ..BASE };
    static CANNON: Def = Def { key: '3', name: "Cannon", w: 7, h: 7, hp: 420.0, cost: 240.0, ht: 3.5, top: rgb(255, 146, 43), side: rgb(184, 100, 15), range: 80.0, rate: 1.4, dmg: 70.0, rad: 5.0, ammo: 6.0, bdmg: 6.0, turret: true, tip: "Slow splash damage, 6 ammo per shell. Rooftops give +60% range. Base price inside the base zone; outside it costs more the further from the HQ it goes.", ..BASE };
    static EXT: Def = Def {
        key: '4',
        name: "Scrap extractor",
        hp: 300.0,
        cost: 120.0,
        ht: 3.0,
        top: rgb(56, 217, 169),
        side: rgb(18, 134, 106),
        zone: true,
        tip: "Makes 2.5 scrap per second. Base zone only.",
        ..BASE
    };
    static PLANT: Def = Def {
        key: '5',
        name: "Ammo plant",
        hp: 300.0,
        cost: 160.0,
        ht: 4.0,
        top: rgb(198, 154, 116),
        side: rgb(134, 96, 63),
        zone: true,
        tip: "Turns 5 scrap into 6 ammo every second. Base zone only.",
        ..BASE
    };
    static SILO: Def = Def {
        key: '6',
        name: "Missile silo",
        w: 9,
        h: 9,
        hp: 800.0,
        cost: 1200.0,
        ht: 5.0,
        top: rgb(241, 243, 245),
        side: rgb(154, 163, 173),
        zone: true,
        tip: "Unlocks the ICBM. Each silo has its own 75 s cooldown. Base zone only.",
        ..BASE
    };
    static PAD: Def = Def { key: '7', name: "Evac point", w: 11, h: 11, hp: 500.0, cost: 200.0, ht: 0.5, top: rgb(248, 249, 250), side: rgb(173, 181, 189), zone: true, tip: "Rescued civilians walk to the nearest pad. A pad flies up to 16 out at a time (+100 score, +20 scrap, +10 ammo each), then needs 30 s before its next flight, so build several.", ..BASE };
    static OUTPOST: Def = Def {
        key: 'o',
        name: "Outpost",
        w: 9,
        h: 9,
        hp: 700.0,
        cost: 400.0,
        ht: 4.0,
        top: rgb(116, 192, 252),
        side: rgb(24, 100, 171),
        zone: true,
        tip: "Place near the edge of the base zone to push the zone out around it.",
        ..BASE
    };
    static LAB: Def = Def {
        key: 'l',
        name: "Research lab",
        w: 9,
        h: 9,
        hp: 600.0,
        cost: 800.0,
        ht: 4.5,
        top: rgb(204, 93, 232),
        side: rgb(130, 40, 160),
        zone: true,
        tip: "Unlocks ability upgrades. Use the Upgrade buttons on the ability panel.",
        ..BASE
    };
    static HQ: Def = Def {
        key: ' ',
        name: "HQ",
        w: 15,
        h: 15,
        hp: 6000.0,
        cost: 0.0,
        ht: 7.0,
        top: rgb(77, 171, 247),
        side: rgb(24, 100, 171),
        ..BASE
    };
    static FLAME: Def = Def { key: 'b', name: "Flame bunker", w: 6, h: 6, hp: 1600.0, cost: 280.0, ht: 2.4, top: rgb(240, 101, 46), side: rgb(150, 52, 20), range: 20.0, rate: 1.8, dmg: 3.0, rad: 2.6, ammo: 5.0, bdmg: 0.0, turret: true, ground: true, tip: "Very tough, short range, burns right up to its walls. Sprays fire for a second, leaving burning napalm, then pauses; it won't hit a spot again until the napalm there is out. 5 ammo a spray. Ground only.", ..BASE };
    static RAIL: Def = Def { key: 'n', name: "Railgun", w: 7, h: 7, hp: 520.0, cost: 750.0, ht: 4.0, top: rgb(116, 192, 252), side: rgb(35, 90, 150), range: 120.0, rate: 3.5, dmg: 400.0, rad: 2.4, ammo: 30.0, turret: true, tip: "Picks the line through the most of the horde, leading their movement, charges for a moment, then a slug destroys everything in a wide straight line to the end of its range. 30 ammo a shot. Rooftops give +60% range.", ..BASE };
    static HELIUM: Def = Def { key: 'i', name: "Helium tower", w: 6, h: 6, hp: 450.0, cost: 380.0, ht: 4.5, top: rgb(165, 216, 255), side: rgb(70, 130, 190), range: 30.0, turret: true, tip: "Sprays liquid helium around it: everything in the field moves at 40% speed. Needs no ammo. Rooftops give +60% range.", ..BASE };
    static ARTY: Def = Def { key: 'y', name: "Artillery", w: 9, h: 9, hp: 1000.0, cost: 1800.0, ht: 3.0, top: rgb(134, 142, 150), side: rgb(73, 80, 87), range: 0.0, rate: 4.0, dmg: 140.0, rad: 10.0, ammo: 45.0, bdmg: 25.0, turret: true, ground: true, tip: "Lobs heavy shells anywhere on the map. Click it, then right-click a target; it keeps firing there until you pick another spot or cease fire. 45 ammo a shell.", ..BASE };
    static HELIPAD: Def = Def { key: 'u', name: "Heli pad", w: 11, h: 11, hp: 800.0, cost: 1500.0, ht: 0.6, top: rgb(73, 80, 87), side: rgb(40, 44, 50), zone: true, tip: "Keeps a gunship of its own. Click the pad, then right-click anywhere: the gunship flies there for free and fires for 20 s (right-click again to move it), then returns to rearm for 35 s. Base zone only.", ..BASE };
    static RESCUE: Def = Def { key: 'j', name: "Rescue station", w: 9, h: 9, hp: 600.0, cost: 500.0, ht: 3.2, top: rgb(105, 219, 124), side: rgb(43, 138, 62), zone: true, tip: "Every 25 s sends a rescue team to the biggest group of civilians it can reach safely and walks them back to the base. Base zone only.", ..BASE };
    static SDEPOT: Def = Def { key: '0', name: "Scrap depot", w: 7, h: 7, hp: 500.0, cost: 250.0, ht: 2.6, top: rgb(56, 217, 169), side: rgb(18, 134, 106), zone: true, tip: "Stores 2500 more scrap (you start with room for 2500). Base zone only.", ..BASE };
    static BUNKER: Def = Def { key: '-', name: "Bunker", w: 7, h: 7, hp: 1400.0, cost: 150.0, ht: 2.4, top: rgb(150, 156, 122), side: rgb(92, 98, 72), tip: "A concrete bunker. Right-click it with a squad selected to send them in: they fire from the slits with +60% range and are safe until it falls, when 3 in 4 of them die. Any number of troops fit. Research turret mounts to put a turret on top for the rooftop range bonus.", ..BASE };
    static CUSTOM: Def = Def { key: ' ', name: "Custom turret", w: 6, h: 6, hp: 300.0, cost: 0.0, ht: 2.6, top: rgb(150, 160, 140), side: rgb(90, 96, 86), range: 40.0, turret: true, tip: "A turret you built in the garage. Any of them can go on a rooftop or a bunker: the higher the roof and the taller its mount, the further it reaches.", ..BASE };
    static ADEPOT: Def = Def { key: 'v', name: "Ammo depot", w: 7, h: 7, hp: 500.0, cost: 250.0, ht: 2.6, top: rgb(198, 154, 116), side: rgb(134, 96, 63), zone: true, tip: "Stores 2000 more ammo (you start with room for 1500). Base zone only.", ..BASE };
    match s {
        S::ScrapDepot => &SDEPOT,
        S::AmmoDepot => &ADEPOT,
        S::Bunker => &BUNKER,
        S::Custom => &CUSTOM,
        S::Flame => &FLAME,
        S::Rail => &RAIL,
        S::Helium => &HELIUM,
        S::Artillery => &ARTY,
        S::Helipad => &HELIPAD,
        S::Rescue => &RESCUE,
        S::Wall => &WALL,
        S::Gun => &GUN,
        S::Cannon => &CANNON,
        S::Extractor => &EXT,
        S::Plant => &PLANT,
        S::Silo => &SILO,
        S::Pad => &PAD,
        S::Outpost => &OUTPOST,
        S::Lab => &LAB,
        S::Hq => &HQ,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum U {
    Rifle,
    Tank,
}
pub struct UDef {
    pub key: char,
    pub name: &'static str,
    pub cost: f32,
    pub n: usize,
    pub hp: f32,
    pub spd: f32,
    pub range: f32,
    pub rate: f32,
    pub dmg: f32,
    pub rad: f32,
    pub ammo: f32,
    pub bdmg: f32,
    pub col: Color,
    pub tip: &'static str,
}
pub fn udef(u: U) -> &'static UDef {
    static RIFLE: UDef = UDef { key: '8', name: "Rifle squad", cost: 60.0, n: 6, hp: 30.0, spd: 14.0, range: 24.0, rate: 0.5, dmg: 9.0, rad: 1.0, ammo: 0.0, bdmg: 0.0, col: rgb(28, 126, 214), tip: "Six soldiers march from the HQ and hold where you click, spread out in a line. They need no ammo. Click a building or a bunker to garrison it (+60% range). Select units with left click, send them with right click." };
    static TANK: UDef = UDef { key: '9', name: "Tank", cost: 300.0, n: 1, hp: 600.0, spd: 18.0, range: 44.0, rate: 1.1, dmg: 45.0, rad: 3.5, ammo: 2.0, bdmg: 4.0, col: rgb(24, 100, 171), tip: "Drives from the HQ and shells the thickest part of the horde. Tough, but each shell costs 2 ammo. Left click to select, right click to move." };
    match u {
        U::Rifle => &RIFLE,
        U::Tank => &TANK,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum A {
    Rescue,
    Air,
    Gunship,
    Icbm,
}
pub const ABILITIES: [A; 4] = [A::Rescue, A::Air, A::Gunship, A::Icbm];
pub struct ADef {
    pub key: char,
    pub name: &'static str,
    pub cost: f32,
    pub ammo: f32,
    pub cd: f32,
    pub tip: &'static str,
}
pub fn adef(a: A) -> &'static ADef {
    static RESCUE: ADef = ADef { key: 'r', name: "Rescue squad", cost: 80.0, ammo: 0.0, cd: 6.0, tip: "Sends troops to the spot you click. Each soldier walks up to civilians one by one and escorts them back to the base, where they walk to an evac point." };
    static AIR: ADef = ADef { key: 'q', name: "Airstrike", cost: 250.0, ammo: 0.0, cd: 18.0, tip: "A B-2 bomber drops 5 bombs spread evenly along a line. Right-click where the run starts, hold and drag to where it ends, and let go to send it. Flattens buildings." };
    static GUNSHIP: ADef = ADef { key: 'g', name: "Gunship", cost: 300.0, ammo: 150.0, cd: 40.0, tip: "A helicopter gunship flies out and circles the spot you click for 20 seconds, firing at anything in range." };
    static ICBM: ADef = ADef { key: 'e', name: "ICBM", cost: 1500.0, ammo: 400.0, cd: 75.0, tip: "$1500 + 400 ammo. Launches from the nearest ready silo and erases everything in a huge radius. Every silo has its own cooldown, so more silos means more launches." };
    match a {
        A::Rescue => &RESCUE,
        A::Air => &AIR,
        A::Gunship => &GUNSHIP,
        A::Icbm => &ICBM,
    }
}

/// Research topics. In a normal game the research lab sells them for scrap; in the campaign they are
/// unlocked between cities with civilians flown out.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum R {
    Rescue,
    Carpet,
    Nuke,
    Armour,
    Presses,
    Barrels,
    Mounts,
}
pub const RESEARCH: [R; 7] = [R::Rescue, R::Carpet, R::Nuke, R::Armour, R::Presses, R::Barrels, R::Mounts];
/// Lab topics that aren't tied to an ability (those show on the ability panel instead).
pub const LAB_ONLY: [R; 4] = [R::Armour, R::Presses, R::Barrels, R::Mounts];
pub struct RDef {
    pub name: &'static str,
    pub tip: &'static str,
    /// scrap at the research lab
    pub cost: f32,
    /// civilians in the campaign
    pub civs: u32,
}
pub fn rdef(r: R) -> &'static RDef {
    static RESCUE: RDef = RDef { name: "Bigger squads", tip: "Rescue squads send 18 troops and gather up to 120 civilians from a wider area", cost: 600.0, civs: 120 };
    static CARPET: RDef = RDef { name: "Carpet bombing", tip: "Airstrikes drop 12 bombs along the run instead of 5", cost: 900.0, civs: 200 };
    static NUKE: RDef = RDef { name: "Nuclear warhead", tip: "Bigger ICBM blast that leaves radiation you can't build or send troops into for 90 s", cost: 2500.0, civs: 450 };
    static ARMOUR: RDef = RDef { name: "Reinforced armour", tip: "Walls and turrets get 75% more health", cost: 700.0, civs: 150 };
    static PRESSES: RDef = RDef { name: "Shell presses", tip: "Ammo plants make 10 ammo a second instead of 6 from the same scrap", cost: 800.0, civs: 180 };
    static BARRELS: RDef = RDef { name: "Long barrels", tip: "Turrets reach 25% further", cost: 1000.0, civs: 220 };
    static MOUNTS: RDef = RDef { name: "Turret mounts", tip: "A turret can be built on top of a bunker, where it gets the rooftop range bonus (+60%)", cost: 600.0, civs: 140 };
    match r {
        R::Rescue => &RESCUE,
        R::Carpet => &CARPET,
        R::Nuke => &NUKE,
        R::Armour => &ARMOUR,
        R::Presses => &PRESSES,
        R::Barrels => &BARRELS,
        R::Mounts => &MOUNTS,
    }
}
/// Campaign-only turret upgrades, two for each kind of turret, bought with research points.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Up {
    GunRapid,
    GunAp,
    CannonAuto,
    CannonSplash,
    FlameNapalm,
    FlameReach,
    RailCaps,
    RailWide,
    HeliumReach,
    HeliumHull,
    ArtyRapid,
    ArtyHe,
    RifleDrill,
    RifleAp,
    RifleGrenade,
    RifleRocket,
}
/// The rifle squad upgrades, in the order they unlock (each needs the one before).
pub const RIFLE_UPS: [Up; 4] = [Up::RifleDrill, Up::RifleAp, Up::RifleGrenade, Up::RifleRocket];
/// Rocket riflemen: how hard a rocket hits and how wide it bursts (kept small on purpose).
pub const ROCKET_DMG: f32 = 32.0;
pub const ROCKET_RAD: f32 = 2.2;
pub struct UpDef {
    pub name: &'static str,
    pub tip: &'static str,
    /// the turret it upgrades (rifle squad upgrades: the HQ, which trains them)
    pub on: S,
    /// research points
    pub cost: u32,
}
pub fn updef(u: Up) -> UpDef {
    let (name, tip, on, cost) = match u {
        Up::GunRapid => ("Rapid fire", "Gun turrets fire 40% faster", S::Gun, 120),
        Up::GunAp => ("Armour-piercing rounds", "Gun turret bullets do 50% more damage", S::Gun, 140),
        Up::CannonAuto => ("Autoloader", "Cannons reload 30% faster", S::Cannon, 160),
        Up::CannonSplash => ("Heavy shells", "Cannon shells blast a 40% wider area", S::Cannon, 180),
        Up::FlameNapalm => ("Napalm mix", "Flame bunkers burn 50% hotter", S::Flame, 150),
        Up::FlameReach => ("High-pressure nozzles", "Flame bunkers reach 30% further", S::Flame, 150),
        Up::RailCaps => ("Capacitor banks", "Railguns recharge 35% faster", S::Rail, 220),
        Up::RailWide => ("Wide slugs", "Railgun shots cut a 50% wider path", S::Rail, 220),
        Up::HeliumReach => ("Bigger balloons", "Helium towers slow the horde over a 35% wider area", S::Helium, 160),
        Up::HeliumHull => ("Armoured tanks", "Helium towers have twice the health", S::Helium, 120),
        Up::ArtyRapid => ("Gun crews", "Artillery fires 30% faster", S::Artillery, 220),
        Up::ArtyHe => ("High explosive", "Artillery shells blast a 40% wider area and hit 30% harder", S::Artillery, 240),
        Up::RifleDrill => ("Marksman drills", "Riflemen fire 30% faster", S::Hq, 90),
        Up::RifleAp => ("Battle rifles", "Riflemen hit 60% harder and reach 15% further", S::Hq, 140),
        Up::RifleGrenade => ("Rifle grenades", "Rifle shots burst in a small blast that also hits the units around the target", S::Hq, 200),
        Up::RifleRocket => ("Shoulder rockets", "Riflemen fire rockets: much harder hits in a small blast that also damages buildings, at a slower rate", S::Hq, 300),
    };
    UpDef { name, tip, on, cost }
}

/// The upgrade that has to be bought before this one, for upgrades that come in a chain.
pub fn up_after(u: Up) -> Option<Up> {
    let k = RIFLE_UPS.iter().position(|x| *x == u)?;
    (k > 0).then(|| RIFLE_UPS[k - 1])
}

/// The research that upgrades an ability, if any.
pub fn ability_research(a: A) -> Option<R> {
    match a {
        A::Rescue => Some(R::Rescue),
        A::Air => Some(R::Carpet),
        A::Icbm => Some(R::Nuke),
        A::Gunship => None,
    }
}

pub struct Faction {
    pub name: &'static str,
    pub stats: FactionStats,
    pub sight: u32,
    pub col: [u8; 3],
    pub deep: [u8; 3],
    pub reward: f32,
    pub blurb: &'static str,
}
pub const FACTIONS: [Faction; 4] = [
    Faction { name: "Zombies", stats: FactionStats { mode: Behaviour::Zombie, spd: 9.0, hp: 12.0, atk: 4.0, jit: 8.0 }, sight: 600, col: [92, 220, 110], deep: [20, 120, 40], reward: 0.04, blurb: "Small outbreaks that spread by eating civilians. Attack anything in sight, then rush your HQ." },
    Faction { name: "Aliens", stats: FactionStats { mode: Behaviour::Alien, spd: 24.0, hp: 6.0, atk: 3.0, jit: 14.0 }, sight: 700, col: [222, 120, 255], deep: [120, 30, 170], reward: 0.03, blurb: "Small drops all over the city. Straight for the HQ, killing anyone in the way." },
    Faction { name: "Orcs", stats: FactionStats { mode: Behaviour::Orc, spd: 8.0, hp: 50.0, atk: 14.0, jit: 5.0 }, sight: 900, col: [255, 140, 50], deep: [170, 50, 0], reward: 0.2, blurb: "A few portals pour out huge waves that destroy everything they see." },
    // the three armies at once: these stats only stand in where the game needs one army's numbers
    // (aiming ahead, troops in a fight); every unit moves by its own army's
    Faction { name: "Chaos", stats: FactionStats { mode: Behaviour::Alien, spd: 14.0, hp: 20.0, atk: 7.0, jit: 9.0 }, sight: 900, col: [235, 235, 245], deep: [110, 110, 120], reward: 0.08, blurb: "All three at once: outbreaks, bug drops and orc portals, each army in its own colour." },
];
/// The faction index of a chaos game.
pub const CHAOS: usize = 3;

/// Map colours. Dark is the default; both follow Google Maps' palettes.
pub struct Theme {
    pub bg: Color,
    pub land: Color,
    pub base: Color,
    pub park: Color,
    pub water: Color,
    pub water_edge: Color,
    pub air: Color,
    pub apron: Color,
    pub runway: Color,
    pub runway_mark: Color,
    pub road: Color,
    pub road_major: Color,
    pub casing: Color,
    pub rubble: Color,
    pub roof: Color,
    pub roof_tall: Color,
    pub side_a: Color,
    pub side_b: Color,
    pub outline: Color,
    pub window: Color,
    pub zone: Color,
    pub zone_edge: Color,
    pub civ: Color,
    pub civ_rim: Color,
    pub dark: bool,
}
pub const DARK: Theme = Theme {
    bg: rgb(23, 26, 31),
    land: rgb(36, 40, 46),
    base: rgb(44, 50, 60),
    park: rgb(30, 54, 40),
    water: rgb(18, 40, 62),
    water_edge: rgb(28, 56, 84),
    air: rgb(42, 46, 54),
    apron: rgb(52, 57, 66),
    runway: rgb(66, 71, 80),
    runway_mark: rgba(220, 224, 230, 0.7),
    road: rgb(64, 69, 78),
    road_major: rgb(84, 88, 96),
    casing: rgb(28, 31, 36),
    rubble: rgb(72, 64, 54),
    roof: rgb(92, 100, 116),
    roof_tall: rgb(104, 114, 134),
    side_a: rgb(66, 72, 86),
    side_b: rgb(88, 96, 112),
    outline: rgb(34, 37, 44),
    window: rgba(255, 214, 140, 0.22),
    // amber, so the base never reads as water
    zone: rgba(255, 192, 40, 0.06),
    zone_edge: rgba(252, 180, 40, 0.75),
    civ: rgb(255, 255, 255),
    civ_rim: rgba(0, 0, 0, 0.9),
    dark: true,
};

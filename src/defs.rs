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
}
pub const BUILD_ORDER: [S; 9] = [
    S::Wall,
    S::Gun,
    S::Cannon,
    S::Extractor,
    S::Plant,
    S::Silo,
    S::Pad,
    S::Outpost,
    S::Lab,
];

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
    zone: false,
    tip: "",
};

pub fn def(s: S) -> &'static Def {
    static WALL: Def = Def { key: '1', name: "Wall", w: 3, h: 3, hp: 300.0, cost: 3.0, ht: 1.6, top: rgb(139, 147, 159), side: rgb(93, 100, 112), tip: "Drag across a street to seal it. Corners that touch still block. Hordes chew through when there is no other way.", ..BASE };
    static GUN: Def = Def { key: '2', name: "Gun turret", w: 5, h: 5, hp: 220.0, cost: 90.0, ht: 3.0, top: rgb(255, 212, 59), side: rgb(184, 147, 31), range: 46.0, rate: 0.16, dmg: 14.0, rad: 1.3, ammo: 1.0, turret: true, tip: "Fast, 1 ammo per shot. Click a rooftop for +60% range (it falls with the building). Costs more the further from the HQ it goes; outside the base zone it is flown in by helicopter.", ..BASE };
    static CANNON: Def = Def { key: '3', name: "Cannon", w: 7, h: 7, hp: 420.0, cost: 240.0, ht: 3.5, top: rgb(255, 146, 43), side: rgb(184, 100, 15), range: 80.0, rate: 1.4, dmg: 70.0, rad: 5.0, ammo: 6.0, bdmg: 6.0, turret: true, tip: "Slow splash damage, 6 ammo per shell. Rooftops give +60% range. Costs more the further from the HQ it goes.", ..BASE };
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
        tip: "Turns 3 scrap into 12 ammo every second. Base zone only.",
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
    static PAD: Def = Def { key: '7', name: "Extraction pad", w: 11, h: 11, hp: 500.0, cost: 200.0, ht: 0.5, top: rgb(248, 249, 250), side: rgb(173, 181, 189), zone: true, tip: "Rescued civilians walk to the nearest pad. A pad flies up to 16 out at a time (+100 score, +20 scrap, +10 ammo each), then needs 30 s before its next flight, so build several. A crowded base slows your income.", ..BASE };
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
    match s {
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
    static RIFLE: UDef = UDef { key: '8', name: "Rifle squad", cost: 60.0, n: 6, hp: 30.0, spd: 14.0, range: 24.0, rate: 0.5, dmg: 9.0, rad: 1.0, ammo: 0.25, bdmg: 0.0, col: rgb(28, 126, 214), tip: "Six soldiers march from the HQ and hold where you click. Click a building to garrison it (+60% range). Select units with left click, send them with right click." };
    static TANK: UDef = UDef { key: '9', name: "Tank", cost: 300.0, n: 1, hp: 600.0, spd: 18.0, range: 44.0, rate: 1.1, dmg: 45.0, rad: 3.5, ammo: 2.0, bdmg: 4.0, col: rgb(24, 100, 171), tip: "Drives from the HQ and shells the horde. Tough, but each shell costs 2 ammo. Left click to select, right click to move." };
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
    static RESCUE: ADef = ADef { key: 'r', name: "Rescue squad", cost: 80.0, ammo: 0.0, cd: 6.0, tip: "Sends troops to the spot you click. Each soldier walks up to civilians one by one and escorts them back to the base, where they walk to an extraction pad." };
    static AIR: ADef = ADef { key: 'q', name: "Airstrike", cost: 250.0, ammo: 0.0, cd: 18.0, tip: "A bomber crosses the map and drops a line of bombs through the spot you click. Flattens buildings." };
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
}
pub const RESEARCH: [R; 6] = [R::Rescue, R::Carpet, R::Nuke, R::Armour, R::Presses, R::Barrels];
/// Lab topics that aren't tied to an ability (those show on the ability panel instead).
pub const LAB_ONLY: [R; 3] = [R::Armour, R::Presses, R::Barrels];
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
    static CARPET: RDef = RDef { name: "Carpet bombing", tip: "Airstrikes drop 12 bombs; drag on the map to aim the bomb run", cost: 900.0, civs: 200 };
    static NUKE: RDef = RDef { name: "Nuclear warhead", tip: "Bigger ICBM blast that leaves radiation you can't build or send troops into for 90 s", cost: 2500.0, civs: 450 };
    static ARMOUR: RDef = RDef { name: "Reinforced armour", tip: "Walls and turrets get 75% more health", cost: 700.0, civs: 150 };
    static PRESSES: RDef = RDef { name: "Shell presses", tip: "Ammo plants make 20 ammo a second instead of 12 from the same scrap", cost: 800.0, civs: 180 };
    static BARRELS: RDef = RDef { name: "Long barrels", tip: "Turrets reach 25% further", cost: 1000.0, civs: 220 };
    match r {
        R::Rescue => &RESCUE,
        R::Carpet => &CARPET,
        R::Nuke => &NUKE,
        R::Armour => &ARMOUR,
        R::Presses => &PRESSES,
        R::Barrels => &BARRELS,
    }
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
pub const FACTIONS: [Faction; 3] = [
    Faction { name: "Zombies", stats: FactionStats { mode: Behaviour::Zombie, spd: 9.0, hp: 12.0, atk: 4.0, jit: 8.0 }, sight: 600, col: [92, 220, 110], deep: [20, 120, 40], reward: 0.04, blurb: "Small outbreaks that spread by eating civilians. Attack anything in sight, then rush your HQ." },
    Faction { name: "Alien bugs", stats: FactionStats { mode: Behaviour::Alien, spd: 20.0, hp: 6.0, atk: 3.0, jit: 14.0 }, sight: 700, col: [222, 120, 255], deep: [120, 30, 170], reward: 0.03, blurb: "Small drops all over the city. Straight for the HQ, killing anyone in the way." },
    Faction { name: "Orcs", stats: FactionStats { mode: Behaviour::Orc, spd: 8.0, hp: 50.0, atk: 14.0, jit: 5.0 }, sight: 900, col: [255, 140, 50], deep: [170, 50, 0], reward: 0.2, blurb: "A few portals pour out huge waves that destroy everything they see." },
];

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
    zone: rgba(77, 171, 247, 0.09),
    zone_edge: rgba(116, 192, 252, 0.6),
    civ: rgb(255, 255, 255),
    civ_rim: rgba(0, 0, 0, 0.9),
    dark: true,
};
pub const LIGHT: Theme = Theme {
    bg: rgb(12, 15, 19),
    land: rgb(232, 234, 229),
    base: rgb(226, 231, 238),
    park: rgb(201, 232, 195),
    water: rgb(159, 208, 243),
    water_edge: rgb(134, 194, 236),
    air: rgb(225, 228, 233),
    apron: rgb(211, 215, 221),
    runway: rgb(188, 193, 201),
    runway_mark: rgba(255, 255, 255, 0.9),
    road: rgb(255, 255, 255),
    road_major: rgb(255, 255, 255),
    casing: rgb(205, 209, 216),
    rubble: rgb(205, 191, 169),
    roof: rgb(243, 244, 246),
    roof_tall: rgb(232, 237, 245),
    side_a: rgb(160, 168, 180),
    side_b: rgb(198, 204, 214),
    outline: rgb(201, 205, 212),
    window: rgba(90, 110, 140, 0.16),
    zone: rgba(77, 171, 247, 0.10),
    zone_edge: rgba(24, 100, 171, 0.6),
    civ: rgb(255, 255, 255),
    civ_rim: rgba(30, 34, 40, 0.9),
    dark: false,
};

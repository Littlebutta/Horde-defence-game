//! Custom turrets: the garage's parts catalogue, what a build of parts adds up to, and the builds
//! the player has saved.
//!
//! A build is a mount (the body), a head (its sensors and targeting brain), a barrel (what it
//! fires), up to two modifiers (how its shots behave) and up to two accessories (gear bolted on).
//! Parts come in families that each work differently, and every family comes in a few strengths
//! (tiers 1 to 5) that cost more the stronger they are. A turret costs the sum of its parts.
//!
//! The catalogue is append-only: builds and campaign saves name parts by `id`, and co-op games
//! send builds as text, so a part's id and behaviour never change once it shipped.

use crate::storage::data_dir;
use std::fmt::Write as _;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slot {
    Mount,
    Head,
    Barrel,
    Mod,
    Gear,
}
pub const SLOT_KINDS: [Slot; 5] = [Slot::Mount, Slot::Head, Slot::Barrel, Slot::Mod, Slot::Gear];
/// The seven places on a build: mount, head, barrel, two modifiers, two accessories.
pub const SLOTS: [Slot; 7] = [
    Slot::Mount,
    Slot::Head,
    Slot::Barrel,
    Slot::Mod,
    Slot::Mod,
    Slot::Gear,
    Slot::Gear,
];

impl Slot {
    pub fn name(self) -> &'static str {
        match self {
            Slot::Mount => "Mount",
            Slot::Head => "Head",
            Slot::Barrel => "Barrel",
            Slot::Mod => "Modifier",
            Slot::Gear => "Accessory",
        }
    }
    pub fn plural(self) -> &'static str {
        match self {
            Slot::Mount => "Mounts",
            Slot::Head => "Heads",
            Slot::Barrel => "Barrels",
            Slot::Mod => "Modifiers",
            Slot::Gear => "Accessories",
        }
    }
}

/// A family of parts: one way of working, in a few strengths.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fam {
    // mounts
    Sandbag,
    Pintle,
    Ring,
    Casemate,
    Mast,
    Popup,
    Crane,
    Gyro,
    Twin,
    Fortress,
    Walker,
    // heads
    Iron,
    Optic,
    Crowd,
    Far,
    Guard,
    LastLine,
    Lead,
    Scatter,
    Sweep,
    Split,
    Overwatch,
    // barrels
    Rifle,
    Rotary,
    Shotgun,
    Howitzer,
    Mortar,
    Rail,
    Laser,
    Missile,
    Flamer,
    Tesla,
    Flak,
    Needle,
    Sonic,
    // modifiers
    Incendiary,
    Cryo,
    Cluster,
    Piercing,
    HighEx,
    TwinLink,
    Ricochet,
    Overcharge,
    Drum,
    LightLoad,
    Seeker,
    Volatile,
    HairTrigger,
    // accessories
    Rangefinder,
    Plates,
    Repair,
    Hopper,
    Stabiliser,
    Mines,
    Ciws,
    Spotter,
    Shield,
    Recycler,
    Cooling,
}

pub struct Part {
    pub id: &'static str,
    pub name: &'static str,
    pub fam: Fam,
    /// strength, 1..=5: price, weight and research all grow with it
    pub tier: u8,
    /// 0, 1, 2: which of its family's strengths this is
    pub lvl: u8,
}

const fn p(id: &'static str, name: &'static str, fam: Fam, tier: u8, lvl: u8) -> Part {
    Part {
        id,
        name,
        fam,
        tier,
        lvl,
    }
}

/// Every part there is. Append only (see the module notes).
pub static PARTS: &[Part] = &[
    // ---- mounts
    p("m.sandbag", "Sandbag nest", Fam::Sandbag, 1, 0),
    p("m.pintle1", "Pintle post", Fam::Pintle, 1, 0),
    p("m.pintle2", "Swivel pintle", Fam::Pintle, 2, 1),
    p("m.ring1", "Ring mount", Fam::Ring, 1, 0),
    p("m.ring2", "Heavy ring", Fam::Ring, 2, 1),
    p("m.ring3", "Bastion ring", Fam::Ring, 3, 2),
    p("m.case1", "Pillbox casemate", Fam::Casemate, 2, 0),
    p("m.case2", "Concrete casemate", Fam::Casemate, 3, 1),
    p("m.case3", "Citadel casemate", Fam::Casemate, 4, 2),
    p("m.mast1", "Lattice mast", Fam::Mast, 2, 0),
    p("m.mast2", "Signal tower", Fam::Mast, 3, 1),
    p("m.popup1", "Pop-up silo", Fam::Popup, 3, 0),
    p("m.popup2", "Vault riser", Fam::Popup, 4, 1),
    p("m.crane1", "Crane arm", Fam::Crane, 3, 0),
    p("m.crane2", "Gantry boom", Fam::Crane, 4, 1),
    p("m.gyro1", "Gyro platform", Fam::Gyro, 4, 0),
    p("m.gyro2", "Gimbal cradle", Fam::Gyro, 5, 1),
    p("m.twin1", "Twin cradle", Fam::Twin, 4, 0),
    p("m.twin2", "Hydra cradle", Fam::Twin, 5, 1),
    p("m.fort", "Fortress keep", Fam::Fortress, 5, 0),
    p("m.walk1", "Strider legs", Fam::Walker, 3, 0),
    p("m.walk2", "Stalker legs", Fam::Walker, 4, 1),
    // ---- heads
    p("h.iron", "Iron sights", Fam::Iron, 1, 0),
    p("h.optic1", "Optic block", Fam::Optic, 1, 0),
    p("h.optic2", "Prism scope", Fam::Optic, 2, 1),
    p("h.crowd1", "Crowd counter", Fam::Crowd, 2, 0),
    p("h.crowd2", "Swarm tracker", Fam::Crowd, 3, 1),
    p("h.far1", "Long scope", Fam::Far, 2, 0),
    p("h.far2", "Horizon scope", Fam::Far, 3, 1),
    p("h.guard1", "Sentry brain", Fam::Guard, 2, 0),
    p("h.guard2", "Warden array", Fam::Guard, 3, 1),
    p("h.guard3", "Aegis array", Fam::Guard, 4, 2),
    p("h.last1", "Last-line logic", Fam::LastLine, 2, 0),
    p("h.last2", "Citadel logic", Fam::LastLine, 3, 1),
    p("h.lead1", "Ballistic computer", Fam::Lead, 3, 0),
    p("h.lead2", "Oracle computer", Fam::Lead, 4, 1),
    p("h.scat1", "Scatter brain", Fam::Scatter, 2, 0),
    p("h.scat2", "Saturation logic", Fam::Scatter, 3, 1),
    p("h.sweep1", "Sweeper drum", Fam::Sweep, 3, 0),
    p("h.sweep2", "Lighthouse drum", Fam::Sweep, 4, 1),
    p("h.split1", "Twin-eye", Fam::Split, 3, 0),
    p("h.split2", "Compound eye", Fam::Split, 4, 1),
    p("h.over", "Overwatch core", Fam::Overwatch, 5, 0),
    // ---- barrels
    p("b.rifle1", "Rifle barrel", Fam::Rifle, 1, 0),
    p("b.rifle2", "Battle rifle", Fam::Rifle, 2, 1),
    p("b.rifle3", "Marksman barrel", Fam::Rifle, 3, 2),
    p("b.rot1", "Rotary cannon", Fam::Rotary, 2, 0),
    p("b.rot2", "Buzzsaw rotary", Fam::Rotary, 3, 1),
    p("b.rot3", "Vulcan rotary", Fam::Rotary, 4, 2),
    p("b.shot1", "Scattergun", Fam::Shotgun, 1, 0),
    p("b.shot2", "Riot gun", Fam::Shotgun, 2, 1),
    p("b.shot3", "Breacher", Fam::Shotgun, 3, 2),
    p("b.how1", "Field gun", Fam::Howitzer, 2, 0),
    p("b.how2", "Howitzer", Fam::Howitzer, 3, 1),
    p("b.how3", "Siege howitzer", Fam::Howitzer, 4, 2),
    p("b.mort1", "Mortar tube", Fam::Mortar, 2, 0),
    p("b.mort2", "Heavy mortar", Fam::Mortar, 3, 1),
    p("b.rail1", "Coil driver", Fam::Rail, 3, 0),
    p("b.rail2", "Rail driver", Fam::Rail, 4, 1),
    p("b.rail3", "Mass driver", Fam::Rail, 5, 2),
    p("b.laser1", "Laser emitter", Fam::Laser, 3, 0),
    p("b.laser2", "Burn lance", Fam::Laser, 4, 1),
    p("b.msl1", "Rocket pod", Fam::Missile, 3, 0),
    p("b.msl2", "Missile rack", Fam::Missile, 4, 1),
    p("b.msl3", "Swarm launcher", Fam::Missile, 5, 2),
    p("b.flame1", "Flame projector", Fam::Flamer, 2, 0),
    p("b.flame2", "Dragon nozzle", Fam::Flamer, 3, 1),
    p("b.tesla1", "Tesla coil", Fam::Tesla, 3, 0),
    p("b.tesla2", "Storm coil", Fam::Tesla, 4, 1),
    p("b.tesla3", "Thunderhead", Fam::Tesla, 5, 2),
    p("b.flak", "Flak burster", Fam::Flak, 2, 0),
    p("b.needle1", "Needle gun", Fam::Needle, 3, 0),
    p("b.needle2", "Lance needler", Fam::Needle, 4, 1),
    p("b.sonic", "Sonic projector", Fam::Sonic, 4, 0),
    // ---- modifiers
    p("x.fire1", "Incendiary rounds", Fam::Incendiary, 1, 0),
    p("x.fire2", "Phosphor rounds", Fam::Incendiary, 2, 1),
    p("x.fire3", "Thermite rounds", Fam::Incendiary, 3, 2),
    p("x.cryo1", "Cryo rounds", Fam::Cryo, 2, 0),
    p("x.cryo2", "Nitrogen rounds", Fam::Cryo, 3, 1),
    p("x.clus1", "Shrapnel casing", Fam::Cluster, 2, 0),
    p("x.clus2", "Cluster casing", Fam::Cluster, 3, 1),
    p("x.clus3", "Submunitions", Fam::Cluster, 4, 2),
    p("x.ap1", "AP cores", Fam::Piercing, 1, 0),
    p("x.ap2", "Tungsten cores", Fam::Piercing, 2, 1),
    p("x.ap3", "Uranium cores", Fam::Piercing, 3, 2),
    p("x.he1", "HE filler", Fam::HighEx, 1, 0),
    p("x.he2", "Thermobaric filler", Fam::HighEx, 2, 1),
    p("x.twin", "Twin-link feed", Fam::TwinLink, 3, 0),
    p("x.rico1", "Ricochet tips", Fam::Ricochet, 2, 0),
    p("x.rico2", "Pinball tips", Fam::Ricochet, 3, 1),
    p("x.over1", "Overcharger", Fam::Overcharge, 3, 0),
    p("x.over2", "Siege capacitor", Fam::Overcharge, 4, 1),
    p("x.drum1", "Drum magazine", Fam::Drum, 1, 0),
    p("x.drum2", "Saddle drums", Fam::Drum, 2, 1),
    p("x.drum3", "Belt box", Fam::Drum, 3, 2),
    p("x.light1", "Light loads", Fam::LightLoad, 1, 0),
    p("x.light2", "Reduced charges", Fam::LightLoad, 2, 1),
    p("x.seek1", "Seeker rounds", Fam::Seeker, 3, 0),
    p("x.seek2", "Smart rounds", Fam::Seeker, 4, 1),
    p("x.vol1", "Volatile mix", Fam::Volatile, 4, 0),
    p("x.vol2", "Antimatter trace", Fam::Volatile, 5, 1),
    p("x.hair1", "Hair trigger", Fam::HairTrigger, 2, 0),
    p("x.hair2", "Rapid cycler", Fam::HairTrigger, 3, 1),
    // ---- accessories
    p("g.range1", "Rangefinder", Fam::Rangefinder, 1, 0),
    p("g.range2", "Laser rangefinder", Fam::Rangefinder, 2, 1),
    p("g.range3", "Lidar mast", Fam::Rangefinder, 3, 2),
    p("g.plate1", "Armour plates", Fam::Plates, 1, 0),
    p("g.plate2", "Composite plates", Fam::Plates, 2, 1),
    p("g.plate3", "Reactive plates", Fam::Plates, 3, 2),
    p("g.rep1", "Repair kit", Fam::Repair, 2, 0),
    p("g.rep2", "Repair drone", Fam::Repair, 3, 1),
    p("g.rep3", "Nanite bath", Fam::Repair, 4, 2),
    p("g.hop1", "Ammo hopper", Fam::Hopper, 1, 0),
    p("g.hop2", "Autoloader", Fam::Hopper, 2, 1),
    p("g.stab1", "Gyro stabiliser", Fam::Stabiliser, 2, 0),
    p("g.stab2", "Servo package", Fam::Stabiliser, 3, 1),
    p("g.mine1", "Mine layer", Fam::Mines, 2, 0),
    p("g.mine2", "Mine thrower", Fam::Mines, 3, 1),
    p("g.mine3", "Minefield array", Fam::Mines, 4, 2),
    p("g.ciws1", "Close-in guns", Fam::Ciws, 3, 0),
    p("g.ciws2", "Point-defence array", Fam::Ciws, 4, 1),
    p("g.spot1", "Spotter uplink", Fam::Spotter, 3, 0),
    p("g.spot2", "Command uplink", Fam::Spotter, 4, 1),
    p("g.shield1", "Shield emitter", Fam::Shield, 3, 0),
    p("g.shield2", "Barrier emitter", Fam::Shield, 4, 1),
    p("g.shield3", "Aegis emitter", Fam::Shield, 5, 2),
    p("g.recyc", "Scrap recycler", Fam::Recycler, 4, 0),
    p("g.cool1", "Cooling jacket", Fam::Cooling, 2, 0),
    p("g.cool2", "Cryo radiator", Fam::Cooling, 3, 1),
];

/// Parts a new campaign starts with in the garage, researched and bought.
pub const STARTER: [&str; 9] = [
    "m.sandbag",
    "m.ring1",
    "h.iron",
    "h.optic1",
    "b.rifle1",
    "b.shot1",
    "x.ap1",
    "g.range1",
    "g.plate1",
];

/// The part a family's first strength needs researched before it, which makes each slot's parts
/// one research tree: families with none are its roots (the starter parts and a few cheap basics).
fn fam_after(f: Fam) -> Option<&'static str> {
    use Fam::*;
    Some(match f {
        Sandbag | Ring | Iron | Optic | Rifle | Shotgun | Piercing | Incendiary | HighEx | Drum
        | LightLoad | Rangefinder | Plates | Hopper => return None,
        // mounts
        Pintle => "m.sandbag",
        Mast => "m.ring1",
        Casemate => "m.ring2",
        Twin => "m.ring3",
        Popup => "m.case1",
        Fortress => "m.case3",
        Crane => "m.mast1",
        Gyro => "m.pintle2",
        Walker => "m.pintle2",
        // heads
        Crowd => "h.iron",
        Guard => "h.iron",
        Scatter => "h.crowd1",
        Sweep => "h.scat1",
        LastLine => "h.guard1",
        Far => "h.optic1",
        Split => "h.far1",
        Lead => "h.optic2",
        Overwatch => "h.lead2",
        // barrels
        Rotary => "b.rifle1",
        Needle => "b.rifle2",
        Rail => "b.rifle3",
        Laser => "b.rail1",
        Tesla => "b.laser1",
        Sonic => "b.tesla1",
        Flamer => "b.shot1",
        Flak => "b.shot1",
        Howitzer => "b.flak",
        Mortar => "b.how1",
        Missile => "b.mort1",
        // modifiers
        Ricochet => "x.ap1",
        Seeker => "x.rico1",
        Overcharge => "x.ap2",
        Cryo => "x.fire1",
        Cluster => "x.he1",
        Volatile => "x.clus2",
        HairTrigger => "x.drum1",
        TwinLink => "x.drum2",
        // accessories
        Stabiliser => "g.range1",
        Cooling => "g.stab1",
        Spotter => "g.range2",
        Repair => "g.plate1",
        Shield => "g.rep2",
        Mines => "g.hop1",
        Recycler => "g.mine2",
        Ciws => "g.hop2",
    })
}

/// The part that has to be researched before part `i`: the weaker part of its family, or for a
/// family's first part the one its branch grows from.
pub fn needs(i: u16) -> Option<u16> {
    let pt = part(i);
    if pt.lvl > 0 {
        return PARTS.iter().position(|q| q.fam == pt.fam && q.lvl + 1 == pt.lvl).map(|j| j as u16);
    }
    fam_after(pt.fam).and_then(find)
}

/// Where a part sits in its slot's research tree: column (how deep) and row (in leaf slots,
/// parents level with the middle of their children).
#[derive(Clone, Copy, Debug)]
pub struct TreeNode {
    pub part: u16,
    pub col: usize,
    pub row: f32,
}

/// One slot's parts as a tree, left to right, roots first. Also returns its (columns, rows).
pub fn part_tree(slot: Slot) -> (Vec<TreeNode>, (usize, f32)) {
    fn kids(i: u16) -> Vec<u16> {
        (0..PARTS.len() as u16).filter(|&j| needs(j) == Some(i)).collect()
    }
    fn place(i: u16, col: usize, row: &mut f32, out: &mut Vec<TreeNode>) -> f32 {
        let ks = kids(i);
        let y = if ks.is_empty() {
            let y = *row;
            *row += 1.0;
            y
        } else {
            let ys: Vec<f32> = ks.into_iter().map(|k| place(k, col + 1, row, out)).collect();
            (ys[0] + ys[ys.len() - 1]) / 2.0
        };
        out.push(TreeNode { part: i, col, row: y });
        y
    }
    let mut out = Vec::new();
    let mut rows = 0.0f32;
    for i in (0..PARTS.len() as u16).filter(|&i| part(i).slot() == slot && needs(i).is_none()) {
        if rows > 0.0 {
            rows += 0.5;
        }
        place(i, 0, &mut rows, &mut out);
    }
    let cols = out.iter().map(|n| n.col + 1).max().unwrap_or(1);
    (out, (cols, rows))
}

pub fn find(id: &str) -> Option<u16> {
    PARTS.iter().position(|p| p.id == id).map(|i| i as u16)
}
pub fn part(i: u16) -> &'static Part {
    &PARTS[(i as usize).min(PARTS.len() - 1)]
}

impl Fam {
    pub fn slot(self) -> Slot {
        use Fam::*;
        match self {
            Sandbag | Pintle | Ring | Casemate | Mast | Popup | Crane | Gyro | Twin | Fortress
            | Walker => Slot::Mount,
            Iron | Optic | Crowd | Far | Guard | LastLine | Lead | Scatter | Sweep | Split
            | Overwatch => Slot::Head,
            Rifle | Rotary | Shotgun | Howitzer | Mortar | Rail | Laser | Missile | Flamer
            | Tesla | Flak | Needle | Sonic => Slot::Barrel,
            Incendiary | Cryo | Cluster | Piercing | HighEx | TwinLink | Ricochet | Overcharge
            | Drum | LightLoad | Seeker | Volatile | HairTrigger => Slot::Mod,
            Rangefinder | Plates | Repair | Hopper | Stabiliser | Mines | Ciws | Spotter
            | Shield | Recycler | Cooling => Slot::Gear,
        }
    }
    /// What the family does, in a sentence or two.
    pub fn blurb(self) -> &'static str {
        use Fam::*;
        match self {
            Sandbag => "Cheap and light. Barely any armour and a lazy turn, but it gets a gun on the street.",
            Pintle => "A bare post on a swivel: whips round faster than anything else, but it can't carry much or take a beating.",
            Ring => "The all-rounder. A steel ring that turns at a fair pace and carries a fair load.",
            Casemate => "Thick concrete facing away from your HQ. It can only fire across its front (140°), but it shrugs off the horde and hits 10% harder. Ground only.",
            Mast => "A tall lattice tower: sees and shoots much further, but it turns slowly and is fragile. Ground only.",
            Popup => "Sinks into an armoured vault when there's nothing to shoot and takes a fifth of the damage while down. Needs a moment to rise before the first shot.",
            Crane => "The head rides on a crane arm that swings out toward whatever it's shooting, reaching past its own range.",
            Gyro => "Gyroscopic cradle: fires while still turning onto the target and steadies the aim.",
            Twin => "Carries two heads and two barrels that pick different targets. Doubles their price and weight.",
            Fortress => "A squat keep that takes enormous punishment and mends itself. Turns like a ship. Ground only.",
            Walker => "Walks out to meet the horde, up to a leash from where it was built, and steps back home when the fight is over. The horde can't block it, but crowds around its legs wear it down.",
            Iron => "Plain sights: shoots at the nearest thing, not very accurately.",
            Optic => "Good glass: the nearest target, accurately, from a bit further away.",
            Crowd => "Counts heads and shoots into the thickest part of the horde in range. Best with splash.",
            Far => "Shoots the furthest hostiles it can reach, thinning a wave before it arrives.",
            Guard => "Watches your buildings and walls: shoots whatever is closest to them first.",
            LastLine => "Shoots whatever is closest to reaching the HQ (by the horde's own path), wherever it is in range.",
            Lead => "Works out where the horde is going and aims ahead of it, so slow shells and charged shots land on target.",
            Scatter => "Never shoots the same patch twice in a row: spreads its fire across the whole front.",
            Sweep => "Spins its head round and round, firing at whatever crosses its sights. Pairs well with beams and flame.",
            Split => "Splits its attention: every other shot goes at the nearest hostile, the rest into the thickest crowd.",
            Overwatch => "Weighs every group in range by size and how close it is to the HQ and fires at the worst threat. Turns fast.",
            Rifle => "A single heavy round per shot, straight to the target. Uses little ammo.",
            Rotary => "Spins up while it keeps firing, getting several times faster. Inaccurate, hungry, terrifying.",
            Shotgun => "A cone of pellets at short range. Shreds whatever is in front of it.",
            Howitzer => "Shells that fly for a moment and burst on landing.",
            Mortar => "Lobs heavy bombs a long way in a high arc. Can't hit anything close, and isn't accurate.",
            Rail => "Charges, then fires a slug that cuts through everything along the line to the end of its range.",
            Laser => "Holds a burning beam on the horde for a second and a half, dragging it along their line. Uses ammo for the power cells.",
            Missile => "A salvo of rockets that steer onto their target as they arrive.",
            Flamer => "Sprays a cone of fire for a moment, leaving burning ground.",
            Tesla => "A bolt that jumps from hostile to hostile down the crowd.",
            Flak => "Bursts a ring of shrapnel around the target instead of one blast.",
            Needle => "A tungsten needle that punches through its target and everything behind it for a long way.",
            Sonic => "A cone of sound that pounds everything in front of it and leaves them staggering slowly. No ammo.",
            Incendiary => "Hits leave the ground burning for a couple of seconds. A little more ammo per shot.",
            Cryo => "Hits leave a frost patch: the horde moves at 40% speed in it for a few seconds.",
            Cluster => "Every hit scatters bomblets around it.",
            Piercing => "Harder hits on a smaller spot, and they crack buildings.",
            HighEx => "Wider blasts, a little weaker.",
            TwinLink => "Every shot is followed by a second at a neighbouring target. Twice the ammo.",
            Ricochet => "Hits glance off to the next hostile nearby at 60% power.",
            Overcharge => "Charges every shot first: much harder hits, a bit slower.",
            Drum => "Empties a burst of quick shots, then takes a long time to reload.",
            LightLoad => "Weaker charges: far less ammo per shot for a little less punch and reach.",
            Seeker => "Shots never miss: they find the nearest hostile where they land.",
            Volatile => "Every hit sets off a second blast that grows with how packed the horde is where it lands.",
            HairTrigger => "Fires sooner, but sprays more.",
            Rangefinder => "Reaches further.",
            Plates => "More armour.",
            Repair => "Mends the turret a little every second.",
            Hopper => "Less ammo per shot.",
            Stabiliser => "Much steadier aim and a quicker turn.",
            Mines => "Every few seconds tosses a mine onto the horde nearby. It goes off when they walk over it.",
            Ciws => "Small guns of its own that shred anything that gets right up to the turret, wherever the main gun is pointing.",
            Spotter => "Shares its targeting with custom turrets nearby: they reach further and fire faster.",
            Shield => "A shield that soaks up damage before the armour does. It comes back 8 s after the last hit.",
            Recycler => "Every hit earns back a little scrap for the hostiles it caught.",
            Cooling => "Reloads faster.",
        }
    }
    /// Scrap price at tier 1 (see `Part::cost`).
    fn base_cost(self) -> f32 {
        use Fam::*;
        match self {
            Sandbag => 10.0,
            Pintle => 18.0,
            Ring => 20.0,
            Casemate => 32.0,
            Mast => 28.0,
            Popup => 30.0,
            Crane => 28.0,
            Gyro => 30.0,
            Twin => 34.0,
            Fortress => 40.0,
            Walker => 40.0,
            Iron => 10.0,
            Optic => 15.0,
            Crowd | Far | Guard | LastLine | Scatter => 16.0,
            Lead | Sweep | Split => 18.0,
            Overwatch => 22.0,
            Rifle => 45.0,
            Rotary => 40.0,
            Shotgun => 40.0,
            Howitzer => 50.0,
            Mortar => 55.0,
            Rail => 70.0,
            Laser => 60.0,
            Missile => 60.0,
            Flamer => 55.0,
            Tesla => 55.0,
            Flak => 50.0,
            Needle => 45.0,
            Sonic => 40.0,
            Incendiary | Cryo | Piercing | HighEx | LightLoad | HairTrigger => 14.0,
            Cluster | Ricochet | Drum => 16.0,
            TwinLink | Overcharge | Seeker => 20.0,
            Volatile => 22.0,
            Rangefinder | Plates | Hopper | Cooling => 12.0,
            Repair | Stabiliser | Mines => 15.0,
            Ciws | Spotter | Shield | Recycler => 20.0,
        }
    }
    /// Load at tier 1 (a mount carries what its `cap` allows).
    fn base_weight(self) -> f32 {
        match self.slot() {
            Slot::Mount => 0.0,
            Slot::Head => 2.0,
            Slot::Barrel => match self {
                Fam::Rifle | Fam::Needle | Fam::Shotgun => 3.0,
                Fam::Rotary | Fam::Flamer | Fam::Tesla | Fam::Flak | Fam::Sonic => 4.0,
                _ => 5.0,
            },
            Slot::Mod => 1.0,
            Slot::Gear => match self {
                Fam::Plates | Fam::Shield | Fam::Ciws | Fam::Mines => 2.0,
                _ => 1.0,
            },
        }
    }
}

/// Price and weight grow with tier.
const TIER_COST: [f32; 6] = [1.0, 1.0, 1.7, 2.7, 4.0, 5.8];
const TIER_WEIGHT: [f32; 6] = [1.0, 1.0, 1.3, 1.6, 2.0, 2.4];
/// Research points a part needs in the campaign, by tier.
const TIER_RESEARCH: [u32; 6] = [0, 30, 70, 130, 210, 320];

impl Part {
    pub fn slot(&self) -> Slot {
        self.fam.slot()
    }
    /// Scrap it adds to a turret's price.
    pub fn cost(&self) -> f32 {
        (self.fam.base_cost() * TIER_COST[self.tier as usize]).round()
    }
    pub fn weight(&self) -> f32 {
        (self.fam.base_weight() * TIER_WEIGHT[self.tier as usize] * 2.0).round() / 2.0
    }
    /// Campaign research points to unlock it.
    pub fn research(&self) -> u32 {
        TIER_RESEARCH[self.tier as usize]
    }
    /// Campaign money to buy it for the garage once researched.
    pub fn price(&self) -> u32 {
        ((self.cost() * 3.0 / 10.0).round() * 10.0) as u32
    }
}

// ------------------------------------------------------------------------------------------ specs

/// What the turret fires.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fire {
    Bullet,
    Pellets,
    Shell,
    Mortar,
    Rail,
    Laser,
    Missile,
    Flame,
    Tesla,
    Flak,
    Needle,
    Sonic,
}
/// How the head picks what to shoot.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Aim {
    Near,
    Dense,
    Far,
    Guard,
    Hq,
    Lead,
    Scatter,
    Sweep,
    Split,
    Smart,
}

/// The sound a turret's shot makes: one of the game's own effects, pitched and filtered.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Voice {
    /// 0 gun, 1 cannon, 2 railgun, 3 artillery shot, 4 flame, 5 heli gun
    pub base: u8,
    pub pitch: f32,
    /// 0 plain, 1 muffled (low-pass), 2 thin and buzzy (high-pass)
    pub tone: u8,
}

/// Everything a build adds up to. Ranges are in map cells, times in seconds.
#[derive(Clone, Debug)]
pub struct Spec {
    pub hp: f32,
    pub range: f32,
    pub min_range: f32,
    pub reload: f32,
    pub dmg: f32,
    pub rad: f32,
    pub bdmg: f32,
    pub ammo: f32,
    /// radians a second it turns
    pub traverse: f32,
    /// casemates: how far either side of straight ahead it can fire (PI: all round)
    pub arc: f32,
    /// how far shots stray, in cells per 50 cells of distance
    pub spread: f32,
    pub fire: Fire,
    pub aim: Aim,
    pub pellets: u8,
    pub cone: f32,
    /// shots per trigger pull, and the time between them
    pub burst: u8,
    pub burst_gap: f32,
    /// seconds a shot takes to fly 50 cells (0: it hits at once)
    pub travel: f32,
    /// seconds of charging before each shot
    pub charge: f32,
    /// rotary barrels: how many times faster it fires once spun up (1: no spin)
    pub spin: f32,
    pub chain: u8,
    pub pierce: f32,
    pub homing: bool,
    /// lasers, flamers: seconds the beam or spray is held
    pub hold: f32,
    pub napalm: f32,
    pub cryo: f32,
    pub cluster: u8,
    pub bounce: u8,
    pub twin_link: bool,
    pub volatile: f32,
    pub regen: f32,
    pub shield: f32,
    /// spotter uplink: range bonus it gives custom turrets near it (0: none)
    pub uplink: f32,
    pub mines: f32,
    pub ciws: f32,
    pub recycle: f32,
    /// crane arm: how far past its own range the head swings out
    pub reach: f32,
    /// pop-up mount: seconds to rise (0: no vault)
    pub rise: f32,
    pub twin_mount: bool,
    /// walker legs: how far from home it walks (0: stays put)
    pub leash: f32,
    pub fire_on_move: bool,
    pub ground: bool,
    /// how tall the mount stands (art only)
    pub height: f32,
    pub cost: f32,
    pub load: f32,
    pub cap: f32,
    pub voice: Voice,
}

impl Spec {
    fn empty() -> Spec {
        Spec {
            hp: 0.0,
            range: 40.0,
            min_range: 0.0,
            reload: 0.5,
            dmg: 10.0,
            rad: 1.2,
            bdmg: 0.0,
            ammo: 1.0,
            traverse: 3.0,
            arc: std::f32::consts::PI,
            spread: 0.0,
            fire: Fire::Bullet,
            aim: Aim::Near,
            pellets: 1,
            cone: 0.0,
            burst: 1,
            burst_gap: 0.1,
            travel: 0.0,
            charge: 0.0,
            spin: 1.0,
            chain: 0,
            pierce: 0.0,
            homing: false,
            hold: 0.0,
            napalm: 0.0,
            cryo: 0.0,
            cluster: 0,
            bounce: 0,
            twin_link: false,
            volatile: 0.0,
            regen: 0.0,
            shield: 0.0,
            uplink: 0.0,
            mines: 0.0,
            ciws: 0.0,
            recycle: 0.0,
            reach: 0.0,
            rise: 0.0,
            twin_mount: false,
            leash: 0.0,
            fire_on_move: false,
            ground: false,
            height: 1.8,
            cost: 0.0,
            load: 0.0,
            cap: 0.0,
            voice: Voice {
                base: 0,
                pitch: 1.0,
                tone: 0,
            },
        }
    }

    /// Damage a single trigger pull can do, roughly, per second: for comparing builds.
    pub fn dps(&self) -> f32 {
        let per = match self.fire {
            Fire::Pellets => self.dmg * self.pellets as f32 * 0.6,
            Fire::Laser | Fire::Flame => self.dmg * self.hold / 0.05 * 0.5,
            Fire::Tesla => self.dmg * (1 + self.chain) as f32 * 0.8,
            Fire::Flak => self.dmg * 6.0 * 0.5,
            Fire::Rail | Fire::Needle => self.dmg * 3.0,
            _ => self.dmg,
        } * (1.0
            + 0.35 * self.cluster as f32
            + 0.5 * self.bounce as f32
            + if self.twin_link { 1.0 } else { 0.0 });
        let shots = self.burst as f32 * if self.twin_mount { 2.0 } else { 1.0 };
        let cycle = self.reload / self.spin.max(1.0).sqrt()
            + self.charge
            + self.burst_gap * (self.burst as f32 - 1.0);
        per * shots / cycle.max(0.05)
    }

    /// Ammo used per second of steady fire.
    pub fn ammo_rate(&self) -> f32 {
        let shots = self.burst as f32
            * if self.twin_mount { 2.0 } else { 1.0 }
            * if self.twin_link { 2.0 } else { 1.0 };
        let cycle = self.reload / self.spin.max(1.0).sqrt()
            + self.charge
            + self.burst_gap * (self.burst as f32 - 1.0);
        self.ammo * shots / cycle.max(0.05)
    }
}

const PI: f32 = std::f32::consts::PI;

fn pick3(lvl: u8, v: [f32; 3]) -> f32 {
    v[(lvl as usize).min(2)]
}

/// What one part does to a spec.
fn apply(pt: &Part, s: &mut Spec) {
    use Fam::*;
    let l = pt.lvl;
    let k = |v: [f32; 3]| pick3(l, v);
    match pt.fam {
        // ---- mounts: body health, turning, load and how it stands
        Sandbag => {
            s.hp = 150.0;
            s.traverse = 2.4;
            s.cap = 9.0;
            s.height = 1.0;
        }
        Pintle => {
            s.hp = k([120.0, 180.0, 0.0]);
            s.traverse = k([7.0, 9.0, 0.0]);
            s.cap = k([8.0, 10.0, 0.0]);
            s.height = 1.6;
        }
        Ring => {
            s.hp = k([280.0, 420.0, 600.0]);
            s.traverse = k([3.0, 3.4, 3.8]);
            s.cap = k([13.0, 17.0, 22.0]);
            s.height = 1.9;
        }
        Casemate => {
            s.hp = k([800.0, 1150.0, 1600.0]);
            s.traverse = 2.0;
            s.arc = 70f32.to_radians();
            s.cap = k([18.0, 23.0, 28.0]);
            s.dmg *= 1.1;
            s.ground = true;
            s.height = 2.2;
        }
        Mast => {
            s.hp = k([170.0, 250.0, 0.0]);
            s.traverse = 2.0;
            s.cap = k([10.0, 13.0, 0.0]);
            s.range *= k([1.3, 1.45, 0.0]);
            s.ground = true;
            s.height = k([6.0, 8.0, 0.0]);
        }
        Popup => {
            s.hp = k([420.0, 600.0, 0.0]);
            s.traverse = 3.2;
            s.cap = k([17.0, 21.0, 0.0]);
            s.rise = k([0.7, 0.45, 0.0]);
            s.height = 1.6;
        }
        Crane => {
            s.hp = k([320.0, 450.0, 0.0]);
            s.traverse = 2.4;
            s.cap = k([15.0, 19.0, 0.0]);
            s.reach = k([10.0, 16.0, 0.0]);
            s.height = 2.4;
        }
        Gyro => {
            s.hp = k([440.0, 620.0, 0.0]);
            s.traverse = k([5.0, 6.5, 0.0]);
            s.cap = k([19.0, 25.0, 0.0]);
            s.fire_on_move = true;
            s.height = 2.0;
        }
        Twin => {
            s.hp = k([520.0, 760.0, 0.0]);
            s.traverse = k([3.0, 3.6, 0.0]);
            s.cap = k([28.0, 36.0, 0.0]);
            s.twin_mount = true;
            s.height = 2.0;
        }
        Fortress => {
            s.hp = 2800.0;
            s.traverse = 1.5;
            s.cap = 34.0;
            s.regen += 6.0;
            s.ground = true;
            s.height = 2.6;
        }
        Walker => {
            s.hp = k([360.0, 520.0, 0.0]);
            s.traverse = 3.4;
            s.cap = k([14.0, 18.0, 0.0]);
            s.leash = k([22.0, 34.0, 0.0]);
            s.ground = true;
            s.height = 3.2;
        }
        // ---- heads: how it picks targets
        Iron => {
            s.aim = Aim::Near;
            s.spread += 2.0;
        }
        Optic => {
            s.aim = Aim::Near;
            s.spread += k([0.6, 0.2, 0.0]);
            s.range *= k([1.08, 1.15, 1.0]);
        }
        Crowd => {
            s.aim = Aim::Dense;
            s.spread += k([1.0, 0.5, 0.0]);
        }
        Far => {
            s.aim = Aim::Far;
            s.range *= k([1.12, 1.2, 1.0]);
            s.spread += k([0.8, 0.4, 0.0]);
        }
        Guard => {
            s.aim = Aim::Guard;
            s.traverse *= k([1.1, 1.2, 1.35]);
            s.spread += k([1.0, 0.6, 0.2]);
        }
        LastLine => {
            s.aim = Aim::Hq;
            s.spread += k([1.0, 0.5, 0.0]);
            s.range *= k([1.0, 1.08, 1.0]);
        }
        Lead => {
            s.aim = Aim::Lead;
            s.spread += k([0.5, 0.1, 0.0]);
        }
        Scatter => {
            s.aim = Aim::Scatter;
            s.spread += k([1.2, 0.6, 0.0]);
            s.traverse *= k([1.2, 1.4, 1.0]);
        }
        Sweep => {
            s.aim = Aim::Sweep;
            s.traverse *= k([1.6, 2.2, 1.0]);
            s.spread += k([1.0, 0.5, 0.0]);
        }
        Split => {
            s.aim = Aim::Split;
            s.spread += k([0.8, 0.3, 0.0]);
            s.traverse *= k([1.2, 1.4, 1.0]);
        }
        Overwatch => {
            s.aim = Aim::Smart;
            s.traverse *= 1.5;
            s.range *= 1.1;
        }
        // ---- barrels: what it fires (sets the base numbers the rest scale)
        Rifle => {
            s.fire = Fire::Bullet;
            s.range = k([44.0, 50.0, 58.0]) * s.range / 40.0;
            s.reload = k([0.24, 0.2, 0.2]);
            s.dmg *= k([1.3, 1.8, 2.5]);
            s.rad = 1.2;
            s.ammo = 1.0;
            s.spread += 0.5;
            s.voice = Voice {
                base: 0,
                pitch: k([1.12, 1.0, 0.88]),
                tone: 0,
            };
        }
        Rotary => {
            s.fire = Fire::Bullet;
            s.range = k([40.0, 44.0, 48.0]) * s.range / 40.0;
            s.reload = 0.32;
            s.spin = k([4.0, 5.0, 6.0]);
            s.dmg *= k([0.7, 0.9, 1.2]);
            s.rad = 1.1;
            s.ammo = 0.5;
            s.spread += 2.0;
            s.voice = Voice {
                base: 5,
                pitch: k([1.25, 1.12, 1.0]),
                tone: 0,
            };
        }
        Shotgun => {
            s.fire = Fire::Pellets;
            s.range = k([24.0, 28.0, 32.0]) * s.range / 40.0;
            s.reload = k([0.9, 0.85, 0.8]);
            s.pellets = k([6.0, 8.0, 10.0]) as u8;
            s.cone = 0.5;
            s.dmg *= k([1.0, 1.3, 1.7]);
            s.rad = 1.4;
            s.ammo = k([2.0, 2.5, 3.0]);
            s.voice = Voice {
                base: 1,
                pitch: k([1.6, 1.5, 1.4]),
                tone: 2,
            };
        }
        Howitzer => {
            s.fire = Fire::Shell;
            s.range = k([70.0, 80.0, 90.0]) * s.range / 40.0;
            s.reload = k([1.5, 1.6, 1.7]);
            s.dmg *= k([6.0, 9.0, 13.0]);
            s.rad = k([4.5, 5.5, 6.5]);
            s.bdmg += 6.0;
            s.ammo = k([5.0, 7.0, 10.0]);
            s.travel = 0.6;
            s.spread += 1.0;
            s.voice = Voice {
                base: 1,
                pitch: k([1.05, 0.92, 0.8]),
                tone: 0,
            };
        }
        Mortar => {
            s.fire = Fire::Mortar;
            s.range = k([110.0, 130.0, 0.0]) * s.range / 40.0;
            s.min_range = 18.0;
            s.reload = k([2.6, 3.0, 0.0]);
            s.dmg *= k([9.0, 14.0, 0.0]);
            s.rad = k([7.0, 9.0, 0.0]);
            s.bdmg += 10.0;
            s.ammo = k([8.0, 12.0, 0.0]);
            s.travel = 1.0;
            s.spread += 5.0;
            s.voice = Voice {
                base: 3,
                pitch: k([1.2, 1.0, 0.0]),
                tone: 1,
            };
        }
        Rail => {
            s.fire = Fire::Rail;
            s.range = k([100.0, 115.0, 130.0]) * s.range / 40.0;
            s.charge += k([0.9, 0.8, 0.7]);
            s.reload = k([3.0, 3.0, 3.2]);
            s.dmg *= k([22.0, 32.0, 45.0]);
            s.rad = k([1.8, 2.2, 2.6]);
            s.ammo = k([18.0, 24.0, 32.0]);
            s.voice = Voice {
                base: 2,
                pitch: k([1.18, 1.0, 0.85]),
                tone: 0,
            };
        }
        Laser => {
            s.fire = Fire::Laser;
            s.range = k([60.0, 70.0, 0.0]) * s.range / 40.0;
            s.reload = 2.0;
            s.hold = 1.5;
            s.dmg *= k([0.6, 0.9, 0.0]);
            s.rad = 1.2;
            s.ammo = 6.0;
            s.voice = Voice {
                base: 2,
                pitch: k([1.8, 1.6, 0.0]),
                tone: 2,
            };
        }
        Missile => {
            s.fire = Fire::Missile;
            s.range = k([90.0, 100.0, 110.0]) * s.range / 40.0;
            s.reload = k([3.5, 3.5, 3.2]);
            s.burst = k([4.0, 6.0, 8.0]) as u8;
            s.burst_gap = 0.12;
            s.dmg *= k([4.5, 5.5, 6.5]);
            s.rad = k([3.5, 4.0, 4.5]);
            s.ammo = 3.0;
            s.travel = 1.1;
            s.homing = true;
            s.spread += 3.0;
            s.voice = Voice {
                base: 3,
                pitch: k([1.6, 1.45, 1.3]),
                tone: 2,
            };
        }
        Flamer => {
            s.fire = Fire::Flame;
            s.range = k([22.0, 28.0, 0.0]) * s.range / 40.0;
            s.reload = 1.6;
            s.hold = 0.8;
            s.dmg *= k([0.3, 0.42, 0.0]);
            s.rad = 2.4;
            s.napalm += 12.0;
            s.ammo = k([4.0, 5.0, 0.0]);
            s.voice = Voice {
                base: 4,
                pitch: k([1.1, 0.9, 0.0]),
                tone: 0,
            };
        }
        Tesla => {
            s.fire = Fire::Tesla;
            s.range = k([34.0, 38.0, 42.0]) * s.range / 40.0;
            s.reload = k([1.2, 1.1, 1.0]);
            s.chain = k([4.0, 6.0, 9.0]) as u8;
            s.dmg *= k([4.0, 5.5, 7.5]);
            s.rad = 1.6;
            s.ammo = k([2.0, 2.5, 3.0]);
            s.voice = Voice {
                base: 2,
                pitch: k([2.3, 2.1, 1.9]),
                tone: 2,
            };
        }
        Flak => {
            s.fire = Fire::Flak;
            s.range = 60.0 * s.range / 40.0;
            s.reload = 1.0;
            s.dmg *= 2.0;
            s.rad = 2.2;
            s.ammo = 3.0;
            s.travel = 0.4;
            s.voice = Voice {
                base: 1,
                pitch: 1.3,
                tone: 0,
            };
        }
        Needle => {
            s.fire = Fire::Needle;
            s.range = k([56.0, 64.0, 0.0]) * s.range / 40.0;
            s.reload = 0.55;
            s.pierce = k([20.0, 28.0, 0.0]);
            s.dmg *= k([3.5, 5.0, 0.0]);
            s.rad = 1.0;
            s.ammo = 2.0;
            s.voice = Voice {
                base: 0,
                pitch: k([1.7, 1.55, 0.0]),
                tone: 2,
            };
        }
        Sonic => {
            s.fire = Fire::Sonic;
            s.range = 30.0 * s.range / 40.0;
            s.reload = 1.3;
            s.cone = 0.9;
            s.dmg *= 2.6;
            s.rad = 2.0;
            s.ammo = 0.0;
            s.cryo += 1.5;
            s.voice = Voice {
                base: 4,
                pitch: 0.55,
                tone: 1,
            };
        }
        // ---- modifiers
        Incendiary => {
            s.napalm += k([5.0, 8.0, 12.0]);
            s.ammo *= 1.2;
        }
        Cryo => s.cryo += k([3.0, 5.0, 0.0]),
        Cluster => s.cluster += k([3.0, 4.0, 6.0]) as u8,
        Piercing => {
            s.dmg *= k([1.25, 1.4, 1.6]);
            s.rad *= 0.8;
            s.bdmg += k([3.0, 5.0, 8.0]);
        }
        HighEx => {
            s.rad *= k([1.35, 1.6, 1.0]);
            s.dmg *= 0.9;
        }
        TwinLink => s.twin_link = true,
        Ricochet => s.bounce += k([1.0, 2.0, 0.0]) as u8,
        Overcharge => {
            s.charge += 0.5;
            s.dmg *= k([1.8, 2.4, 1.0]);
            s.reload *= 1.2;
        }
        Drum => {
            s.burst = s.burst.max(1) + k([2.0, 4.0, 7.0]) as u8;
            s.burst_gap = s.burst_gap.min(0.08);
            s.reload *= k([2.2, 2.6, 3.0]);
        }
        LightLoad => {
            s.ammo *= k([0.65, 0.45, 1.0]);
            s.dmg *= 0.85;
            s.range *= 0.95;
        }
        Seeker => {
            s.homing = true;
            s.spread *= k([0.3, 0.0, 0.0]);
        }
        Volatile => s.volatile += k([0.6, 1.0, 0.0]),
        HairTrigger => {
            s.reload *= k([0.8, 0.7, 1.0]);
            s.spread = s.spread * 1.5 + 0.5;
        }
        // ---- accessories
        Rangefinder => s.range *= k([1.1, 1.18, 1.26]),
        Plates => s.hp *= k([1.4, 1.8, 2.3]),
        Repair => s.regen += k([1.5, 3.0, 6.0]),
        Hopper => s.ammo *= k([0.8, 0.65, 1.0]),
        Stabiliser => {
            s.spread *= 0.4;
            s.traverse *= k([1.5, 1.8, 1.0]);
        }
        Mines => s.mines = k([12.0, 9.0, 6.0]),
        Ciws => s.ciws = k([9.0, 12.0, 0.0]),
        Spotter => s.uplink = k([0.1, 0.15, 0.0]),
        Shield => s.shield += k([150.0, 300.0, 550.0]),
        Recycler => s.recycle = 0.02,
        Cooling => s.reload *= k([0.88, 0.8, 1.0]),
    }
}

// ------------------------------------------------------------------------------------------ builds

/// A turret design: its parts by slot (`SLOTS`), its name and its paint (mount, head, barrel).
#[derive(Clone, Debug, PartialEq)]
pub struct Build {
    pub name: String,
    pub parts: [Option<u16>; 7],
    pub paint: [[u8; 3]; 3],
}

pub const DEFAULT_PAINT: [[u8; 3]; 3] = [[96, 104, 92], [176, 168, 120], [70, 74, 80]];

impl Build {
    pub fn starter() -> Build {
        let f = |id| find(id);
        Build {
            name: "Workhorse".into(),
            parts: [
                f("m.ring1"),
                f("h.optic1"),
                f("b.rifle1"),
                None,
                None,
                None,
                None,
            ],
            paint: DEFAULT_PAINT,
        }
    }
    pub fn part_ids(&self) -> impl Iterator<Item = u16> + '_ {
        self.parts.iter().flatten().copied()
    }
    /// Text that names it exactly: what is saved, shared and sent to co-op partners.
    pub fn code(&self) -> String {
        let mut s = String::new();
        let name: String = self
            .name
            .chars()
            .filter(|c| !matches!(c, ';' | '\n' | '\r' | '|'))
            .take(24)
            .collect();
        s.push_str(name.trim());
        for p in &self.parts {
            s.push(';');
            s.push_str(p.map(|i| part(i).id).unwrap_or("-"));
        }
        for c in &self.paint {
            let _ = write!(s, ";{:02x}{:02x}{:02x}", c[0], c[1], c[2]);
        }
        s
    }
    pub fn from_code(t: &str) -> Option<Build> {
        let f: Vec<&str> = t.trim().split(';').collect();
        if f.len() < 4 {
            return None;
        }
        let mut b = Build {
            name: f[0].chars().take(24).collect(),
            parts: [None; 7],
            paint: DEFAULT_PAINT,
        };
        for (k, slot) in SLOTS.iter().enumerate() {
            let Some(id) = f.get(k + 1) else { break };
            b.parts[k] = find(id).filter(|i| part(*i).slot() == *slot);
        }
        for k in 0..3 {
            if let Some(h) = f.get(8 + k).filter(|h| h.len() == 6) {
                let v = u32::from_str_radix(h, 16).unwrap_or(0);
                b.paint[k] = [(v >> 16) as u8, (v >> 8) as u8, v as u8];
            }
        }
        if b.name.trim().is_empty() {
            b.name = "Unnamed".into();
        }
        Some(b)
    }
    /// What's wrong with it, if anything: it needs a mount, a head and a barrel, no family twice,
    /// and no more weight than the mount carries.
    pub fn problem(&self) -> Option<String> {
        for (k, what) in [(0, "a mount"), (1, "a head"), (2, "a barrel")] {
            if self.parts[k].is_none() {
                return Some(format!("Needs {}", what));
            }
        }
        let fams: Vec<Fam> = self.part_ids().map(|i| part(i).fam).collect();
        for (i, f) in fams.iter().enumerate() {
            if fams[..i].contains(f) {
                return Some(format!("Two {} parts", fam_name(*f)));
            }
        }
        let s = spec_raw(self);
        if s.load > s.cap + 1e-3 {
            return Some(format!("Too heavy: load {:.1} of {:.0}", s.load, s.cap));
        }
        None
    }
    pub fn ok(&self) -> bool {
        self.problem().is_none()
    }
}

/// The family's name for messages ("Two Rifle parts").
pub fn fam_name(f: Fam) -> String {
    format!("{:?}", f)
}

/// The spec of a complete, legal build.
pub fn spec(b: &Build) -> Option<Spec> {
    b.ok().then(|| spec_raw(b))
}

/// The spec of any build, legal or not (the garage shows it while it's being put together).
/// Parts apply in a fixed order: mount, barrel (it sets the base numbers), head, then the rest.
pub fn spec_raw(b: &Build) -> Spec {
    let mut s = Spec::empty();
    let order = [0usize, 2, 1, 3, 4, 5, 6];
    for k in order {
        if let Some(i) = b.parts[k] {
            apply(part(i), &mut s);
        }
    }
    let twin = s.twin_mount;
    for (k, i) in b.parts.iter().enumerate() {
        let Some(i) = *i else { continue };
        let pt = part(i);
        // a twin cradle carries two heads and two barrels
        let n = if twin && (k == 1 || k == 2) { 2.0 } else { 1.0 };
        s.cost += pt.cost() * n;
        s.load += pt.weight() * n;
    }
    if s.fire_on_move {
        s.spread *= 0.5;
    }
    s.hp = s.hp.max(60.0);
    s.reload = s.reload.max(0.08);
    s
}

/// The numbers the garage lists for a spec, as (label, value) pairs.
pub fn stat_lines(s: &Spec) -> Vec<(&'static str, String)> {
    let mut v = vec![
        ("Price", format!("${:.0}", s.cost)),
        ("Load", format!("{:.1} / {:.0}", s.load, s.cap)),
        ("Armour", format!("{:.0}", s.hp)),
        (
            "Range",
            if s.min_range > 0.0 {
                format!("{:.0}-{:.0}", s.min_range, s.range)
            } else {
                format!("{:.0}", s.range)
            },
        ),
        (
            "Damage",
            format!(
                "{:.0}{}",
                s.dmg,
                if s.pellets > 1 {
                    format!(" x{}", s.pellets)
                } else {
                    String::new()
                }
            ),
        ),
        ("Blast", format!("{:.1}", s.rad)),
        ("Reload", format!("{:.2} s", s.reload)),
        (
            "Ammo/shot",
            if s.ammo <= 0.0 {
                "none".into()
            } else {
                format!("{:.1}", s.ammo)
            },
        ),
        ("Turn", format!("{:.0}°/s", s.traverse.to_degrees())),
        ("Spread", format!("{:.1}", s.spread)),
        ("Firepower", format!("{:.0}/s", s.dps())),
        ("Ammo use", format!("{:.1}/s", s.ammo_rate())),
    ];
    let mut extra = |k: &'static str, val: String| v.push((k, val));
    if s.burst > 1 {
        extra("Burst", format!("{} shots", s.burst));
    }
    if s.charge > 0.0 {
        extra("Charge", format!("{:.1} s", s.charge));
    }
    if s.spin > 1.0 {
        extra("Spin-up", format!("x{:.0}", s.spin));
    }
    if s.chain > 0 {
        extra("Jumps", format!("{}", s.chain));
    }
    if s.napalm > 0.0 {
        extra("Burning", format!("{:.0}/s", s.napalm));
    }
    if s.cryo > 0.0 {
        extra("Frost", format!("{:.1} s", s.cryo));
    }
    if s.cluster > 0 {
        extra("Bomblets", format!("{}", s.cluster));
    }
    if s.bounce > 0 {
        extra("Ricochets", format!("{}", s.bounce));
    }
    if s.regen > 0.0 {
        extra("Repair", format!("{:.1}/s", s.regen));
    }
    if s.shield > 0.0 {
        extra("Shield", format!("{:.0}", s.shield));
    }
    if s.leash > 0.0 {
        extra("Leash", format!("{:.0}", s.leash));
    }
    if s.reach > 0.0 {
        extra("Arm reach", format!("+{:.0}", s.reach));
    }
    if s.arc < PI - 0.01 {
        extra(
            "Field of fire",
            format!("{:.0}°", (s.arc * 2.0).to_degrees()),
        );
    }
    v
}

/// A part's tooltip: what it does and what it costs.
pub fn tip(i: u16) -> String {
    let pt = part(i);
    let mut s = format!(
        "{} (tier {} {}). {}",
        pt.name,
        pt.tier,
        pt.slot().name().to_lowercase(),
        pt.fam.blurb()
    );
    let _ = write!(s, " ${:.0} a turret, weight {:.1}.", pt.cost(), pt.weight());
    s
}

// ------------------------------------------------------------------------------------------ library

/// The player's saved builds, shared by the free builder and every campaign (data/turrets.txt).
/// Up to `DEPLOY_MAX` of them are marked to go on the build bar in a game.
#[derive(Clone, Default)]
pub struct Library {
    pub builds: Vec<Build>,
    pub deploy: Vec<bool>,
}
pub const DEPLOY_MAX: usize = 6;
pub const LIBRARY_MAX: usize = 60;

fn library_path() -> std::path::PathBuf {
    data_dir().join("turrets.txt")
}

impl Library {
    pub fn load() -> Library {
        let mut l = Library::default();
        if let Ok(t) = std::fs::read_to_string(library_path()) {
            l.read(&t);
        }
        if l.builds.is_empty() {
            l.builds.push(Build::starter());
            l.deploy.push(true);
        }
        l
    }
    fn read(&mut self, t: &str) {
        for line in t.lines() {
            let (dep, code) = match line.split_once('|') {
                Some((d, c)) => (d.trim() == "1", c),
                None => (false, line),
            };
            if let Some(b) = Build::from_code(code) {
                self.builds.push(b);
                self.deploy
                    .push(dep && self.deployed().count() < DEPLOY_MAX);
            }
        }
    }
    pub fn save(&self) {
        let mut s = String::new();
        for (b, d) in self.builds.iter().zip(&self.deploy) {
            let _ = writeln!(s, "{}|{}", if *d { 1 } else { 0 }, b.code());
        }
        let _ = std::fs::write(library_path(), s);
    }
    pub fn deployed(&self) -> impl Iterator<Item = &Build> {
        self.builds
            .iter()
            .zip(&self.deploy)
            .filter(|(_, d)| **d)
            .map(|(b, _)| b)
    }
    /// Saves `b` over the build of the same name, or as a new one. Returns its index.
    pub fn put(&mut self, b: Build) -> usize {
        if let Some(i) = self
            .builds
            .iter()
            .position(|x| x.name.eq_ignore_ascii_case(&b.name))
        {
            self.builds[i] = b;
            return i;
        }
        if self.builds.len() >= LIBRARY_MAX {
            self.builds.remove(0);
            self.deploy.remove(0);
        }
        self.builds.push(b);
        self.deploy.push(self.deployed().count() < DEPLOY_MAX);
        self.builds.len() - 1
    }
    pub fn remove(&mut self, i: usize) {
        if i < self.builds.len() {
            self.builds.remove(i);
            self.deploy.remove(i);
        }
    }
    /// Imports every build found in `text` (codes, one a line, or a whole turrets file). Returns
    /// how many came in.
    pub fn import(&mut self, text: &str) -> usize {
        let mut n = 0;
        for line in text.lines() {
            let code = line.split_once('|').map(|(_, c)| c).unwrap_or(line);
            if let Some(b) = Build::from_code(code).filter(|b| b.parts[0].is_some()) {
                self.put(b);
                n += 1;
            }
        }
        n
    }
    /// Builds in other turret files dropped next to the game (data/*.turrets or data/import/*):
    /// another player's library, or a backup.
    pub fn import_files(&mut self) -> usize {
        let mut n = 0;
        let mut dirs = vec![data_dir(), data_dir().join("import")];
        dirs.dedup();
        for d in dirs {
            let Ok(rd) = std::fs::read_dir(&d) else {
                continue;
            };
            let mut files: Vec<_> = rd.flatten().map(|e| e.path()).collect();
            files.sort();
            for f in files {
                let name = f.file_name().and_then(|s| s.to_str()).unwrap_or("");
                let ok =
                    name.ends_with(".turrets") || (d.ends_with("import") && name.ends_with(".txt"));
                if ok && f != library_path() {
                    if let Ok(t) = std::fs::read_to_string(&f) {
                        n += self.import(&t);
                    }
                }
            }
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_slot_has_twenty_parts() {
        for s in SLOT_KINDS {
            let n = PARTS.iter().filter(|p| p.slot() == s).count();
            assert!(n >= 20, "{:?} has {}", s, n);
        }
    }
    #[test]
    fn ids_unique_and_starters_exist() {
        for (i, p) in PARTS.iter().enumerate() {
            assert!(PARTS[..i].iter().all(|q| q.id != p.id), "{}", p.id);
            assert!((1..=5).contains(&p.tier));
        }
        for s in STARTER {
            assert!(find(s).is_some(), "{}", s);
        }
    }
    #[test]
    fn cost_grows_with_tier_in_a_family() {
        for a in PARTS {
            for b in PARTS {
                if a.fam == b.fam && a.tier < b.tier {
                    assert!(a.cost() < b.cost(), "{} {}", a.id, b.id);
                }
            }
        }
    }
    #[test]
    fn code_round_trips() {
        let mut b = Build::starter();
        b.parts[3] = find("x.fire2");
        b.parts[5] = find("g.plate1");
        b.name = "Big; Bertha".into();
        b.paint[1] = [1, 2, 250];
        let c = b.code();
        let back = Build::from_code(&c).unwrap();
        assert_eq!(back.parts, b.parts);
        assert_eq!(back.paint, b.paint);
        assert_eq!(back.name, "Big Bertha");
    }
    #[test]
    fn starter_build_is_legal_and_close_to_a_gun_turret() {
        let s = spec(&Build::starter()).unwrap();
        assert!(s.cost > 60.0 && s.cost < 130.0, "{}", s.cost);
        assert!(s.range > 40.0 && s.range < 60.0);
    }
    #[test]
    fn overweight_and_doubled_families_are_refused() {
        let mut b = Build::starter();
        b.parts[0] = find("m.sandbag");
        b.parts[2] = find("b.rail3");
        assert!(b.problem().unwrap().starts_with("Too heavy"));
        let mut b = Build::starter();
        b.parts[3] = find("x.fire1");
        b.parts[4] = find("x.fire2");
        assert!(b.problem().unwrap().starts_with("Two"));
    }
    #[test]
    fn every_part_fits_a_heavy_mount() {
        for (i, p) in PARTS.iter().enumerate() {
            let mut b = Build::starter();
            b.parts[0] = find("m.fort");
            let k = match p.slot() {
                Slot::Mount => 0,
                Slot::Head => 1,
                Slot::Barrel => 2,
                Slot::Mod => 3,
                Slot::Gear => 5,
            };
            b.parts[k] = Some(i as u16);
            let s = spec_raw(&b);
            assert!(
                s.dps().is_finite() && s.cost > 0.0 && s.range > 5.0,
                "{}",
                p.id
            );
        }
    }

    /// Every part sits once in its own slot's research tree, after a part of the same slot.
    #[test]
    fn part_trees_cover_everything() {
        for i in 0..PARTS.len() as u16 {
            if let Some(n) = needs(i) {
                assert_eq!(part(n).slot(), part(i).slot(), "{}", part(i).id);
            }
            if part(i).lvl == 0 {
                if let Some(id) = fam_after(part(i).fam) {
                    assert!(find(id).is_some(), "{} needs a missing part {}", part(i).id, id);
                }
            }
        }
        let mut seen = 0;
        for s in SLOT_KINDS {
            let (nodes, (cols, rows)) = part_tree(s);
            for n in &nodes {
                assert_eq!(part(n.part).slot(), s);
                assert!(n.col < cols && n.row < rows);
                for m in &nodes {
                    if m.part != n.part && m.col == n.col {
                        assert!((m.row - n.row).abs() >= 0.99, "{} and {} overlap", part(m.part).id, part(n.part).id);
                    }
                }
            }
            seen += nodes.len();
        }
        assert_eq!(seen, PARTS.len());
    }
}

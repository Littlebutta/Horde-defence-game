//! The news: the announcer's broadcasts (once or twice a match, played by `music`) and the
//! headline ticker, a thin strip under the top bar that scrolls each joke headline across a few
//! times like an old cinema marquee, then goes away for a while. Both only talk about the army
//! in the game (all three in chaos). Nothing here touches the sim.

use crate::defs::CHAOS;

/// Devin's headlines, indexed like `FACTIONS`: zombies, alien bugs, orcs.
pub const HEADLINES: [&[&str]; 3] = [
    &[
        "Toilet paper futures hit record high as nation prepares for third consecutive week indoors.",
        "Local man tries to lick 6-foot distance marker to prove a point; immediately quarantined.",
        "Zoom fatigue officially reclassified as a major depressive state.",
        "Introverts declare this the greatest era in human history.",
        "Experts remind public that \"social distancing\" does not mean living in a treehouse.",
        "Man who hoarded 4,000 pounds of dried beans now facing severe bean-induced existential dread.",
        "Scientists confirm hand sanitizer cannot cure bad decision-making.",
        "Cat demands explanation for why humans are home twenty-four hours a day.",
        "Local gym converts treadmills into hamster wheels for trapped residents.",
        "Rumor that drinking bleach cures virus debunked; sales of bleach remain inexplicably high anyway.",
        "Neighbors finally learn each other's names via passive-aggressive notes taped to doors.",
        "Baking artisan banana bread becomes leading cause of kitchen fires nationwide.",
        "CDC advises against hugging strangers, pets, or inanimate objects resembling loved ones.",
        "Streaming service CEO thanks plague for record-breaking quarterly profits.",
        "Local conspiracy theorist claims virus is just a plot to make people wash their hands.",
        "Virtual happy hours continue despite everyone secretly hating video calls.",
        "Quarantine beard growth reaches alarming, uncharted evolutionary milestones.",
        "Stay-at-home order extended; national pants elastic industry sees an unprecedented market boom.",
        "Dog population suffers from extreme emotional burnout due to twenty-four-hour owner attention.",
        "World health officials remind citizens that wearing a mask on your chin is a fashion statement, not protection.",
        "Local man attempts to bake sourdough bread from scratch; accidentally summons an ancient deity.",
    ],
    &[
        "Government reassures public that glowing green lights in the sky are just swamp gas and extreme swamp enthusiasm.",
        "Area 51 gift shop reports record sales of 'I survived the non-existent alien invasion' t-shirts.",
        "Local man claims he was abducted by aliens; turns out he just fell asleep behind a fast-food dumpster.",
        "NASA urges public to stop pointing laser pointers at Jupiter; interstellar neighbors are not amused.",
        "Authorities insist flying saucers are actually weather balloons shaped like advanced aerodynamic discs.",
        "Intergalactic ambassador denied entry at customs for lacking a valid passport and green card.",
        "Cult leader deeply disappointed as promised mothership fails to arrive; blames bad Wi-Fi connection.",
        "Astronomers discover radio signal from space; turns out to be a heavily compressed spam email from the Andromeda galaxy.",
        "Local police arrest teenager for shining a green flashlight at neighbors and shouting \"we come in peace\".",
        "Pentagon confirms no motherships are parked over major cities; clouds are just acting dramatic today.",
        "Conspiracy theorists claim the moon is an alien space station; NASA replies: please stop looking at the moon, it is getting self-conscious.",
        "Extraterrestrial denial hotline jammed with callers reporting alien cats stealing their trash.",
        "Scientists clarify that crop circles are just terrible geometric work by local farmers after a night at the pub.",
        "Local man tries to pay his taxes in galactic credits; IRS remains thoroughly unimpressed.",
        "Rumor that aliens have landed in the Midwest proven false; it is just normal regional behavior.",
        "Interstellar tourism board ranks Earth as a low-interest hazard zone; advises travelers not to land.",
        "Local news anchor accidentally refers to the Prime Minister as a lizard person live on air; forced to apologize to lizards.",
        "Telecommunication companies blame recent dropped calls on interstellar interference rather than terrible infrastructure.",
        "UFO sightings spike dramatically following the release of a low-budget science fiction movie.",
        "Government reminds citizens that Martian real estate offers are illegal pyramid schemes.",
        "Local psychic predicts alien landing tomorrow; cancels appointment due to unforeseen circumstances.",
    ],
    &[
        "City council officially renames brutal orc pillaging to \"aggressive cultural exchange\".",
        "Local realtor insists giant spike-filled war camps are actually rustic outdoor aesthetic features.",
        "City guards confirm war drums echoing across the valley are just extremely rhythmic thunder.",
        "Mayor assures citizens that green-skinned giants breaking down the gates are merely oversized landscape architects.",
        "Blacksmiths report record-breaking demand for pitchforks, but note it is strictly for very large gardening projects.",
        "Citizens advised to ignore the smell of burning timber and war paint; it is just seasonal barbecue season.",
        "Goblin scouts spotted roaming the downtown area; authorities claim they are just very short, enthusiastic tourists.",
        "Major toll bridge completely destroyed by siege engines; transit department blames unexpected structural settling.",
        "Local tavern owner renames house specialty \"orc slop\" to attract adventurous foodies.",
        "Government denies claims of an invading horde, stating the army of ten thousand snarling beasts is just a local hiking club.",
        "Wolf-rider tracks found in city park; animal control officially blames rogue stray dogs.",
        "University history department notes that being conquered by orcs builds strong character and endurance.",
        "Massive battle axe found embedded in city hall front door; tourism board claims it is a piece of modern public art.",
        "Residents urged not to panic when hearing blood-curdling war cries; just yell back to assert dominance.",
        "Local economy booms as demand for barricades, board nails, and heavy plate armor skyrockets.",
        "Royal palace issues decree stating that the horde is a figment of everyone's collective imagination.",
        "Orc chieftain releases public statement complaining about high real estate prices in the valley.",
        "City gates heavily reinforced with solid steel; mayor insists it is just part of a routine aesthetic upgrade.",
        "Public reminded that feeding wild orcs only encourages them to return for more pillaging.",
        "Weather forecast predicts a ninety percent chance of falling debris, stray arrows, and complete denial of danger.",
        "Local hero volunteers to negotiate with the orc warlord; seen ten minutes later running very fast in the opposite direction.",
    ],
];

/// how many times each headline crosses the strip before the next one
pub const PASSES: u32 = 3;
/// marquee speed, UI points a second
pub const SPEED: f32 = 70.0;
/// quiet seconds between headlines, and before the first one
const QUIET: f32 = 40.0;
const FIRST: f32 = 15.0;
/// the strip fades in and out over this long
pub const FADE: f32 = 0.6;

/// A broadcast goes out this far into the match (game seconds), then a second one this much
/// later (Devin: hear them once, maybe twice a match).
const FIRST_NEWS: (f32, f32) = (150.0, 300.0);
const SECOND_NEWS: (f32, f32) = (720.0, 1080.0);
/// how long the ticker shows LIVE once a broadcast starts
pub const LIVE: f32 = 17.0;

/// One match's news.
pub struct News {
    rng: u64,
    /// the headlines still to show this round, as (army, line)
    order: Vec<(usize, usize)>,
    armies: Vec<usize>,
    /// the headline crossing now, and how far it has scrolled (points)
    pub cur: Option<(usize, usize)>,
    pub x: f32,
    /// seconds until the next headline comes on
    wait: f32,
    /// game time of the next broadcast, and how many have gone out
    next_news: f32,
    pub aired: u32,
    /// seconds since the last broadcast began
    pub since_news: f32,
}

impl News {
    pub fn new(fac: usize, seed: u64) -> News {
        let armies = if fac == CHAOS { vec![0, 1, 2] } else { vec![fac.min(2)] };
        let mut n = News { rng: seed | 1, order: Vec::new(), armies, cur: None, x: 0.0, wait: FIRST, next_news: 0.0, aired: 0, since_news: f32::MAX };
        n.next_news = n.range(FIRST_NEWS);
        n
    }
    fn rand(&mut self) -> u64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }
    fn range(&mut self, (a, b): (f32, f32)) -> f32 {
        a + (self.rand() % 10_000) as f32 / 10_000.0 * (b - a)
    }
    fn refill(&mut self) {
        let armies = self.armies.clone();
        self.order = armies.iter().flat_map(|&a| (0..HEADLINES[a].len()).map(move |l| (a, l))).collect();
        for i in (1..self.order.len()).rev() {
            let j = (self.rand() % (i as u64 + 1)) as usize;
            self.order.swap(i, j);
        }
    }
    /// Test hook (PH_NEWS=<points>): a headline already that far along, as if a broadcast had just begun.
    pub fn preview(&mut self, x: f32) {
        self.refill();
        self.cur = self.order.pop();
        self.x = x;
        self.since_news = 0.0;
    }
    pub fn line(&self) -> Option<&'static str> {
        self.cur.map(|(a, l)| HEADLINES[a][l])
    }
    /// How far the current headline has to scroll to cross a strip `w` wide `PASSES` times.
    fn run(w: f32, tw: f32) -> f32 {
        (w + tw) * PASSES as f32
    }
    /// Moves the marquee on by `dt` seconds for a strip `w` points wide, the current headline
    /// being `tw` points long. Returns the strip's opacity (0 = hidden).
    pub fn scroll(&mut self, dt: f32, w: f32, tw: f32) -> f32 {
        match self.cur {
            None => {
                self.wait -= dt;
                if self.wait <= 0.0 {
                    if self.order.is_empty() {
                        self.refill();
                    }
                    self.cur = self.order.pop();
                    self.x = -FADE * SPEED;
                }
                0.0
            }
            Some(_) => {
                self.x += dt * SPEED;
                let end = Self::run(w, tw);
                if self.x > end + FADE * SPEED {
                    self.cur = None;
                    self.wait = QUIET;
                    return 0.0;
                }
                let fin = (self.x / (FADE * SPEED) + 1.0).clamp(0.0, 1.0);
                let fout = ((end + FADE * SPEED - self.x) / (FADE * SPEED)).clamp(0.0, 1.0);
                fin.min(fout)
            }
        }
    }
    /// Where the headline's left edge sits, from the strip's left, while it crosses again and
    /// again (it enters at the right edge).
    pub fn offset(&self, w: f32, tw: f32) -> f32 {
        let x = self.x.clamp(0.0, Self::run(w, tw));
        w - x % (w + tw)
    }
    /// Called each frame with the match's game time: the army a broadcast should go out about now.
    pub fn broadcast(&mut self, time: f32, dt: f32) -> Option<usize> {
        self.since_news += dt;
        if self.aired >= 2 || time < self.next_news {
            return None;
        }
        self.aired += 1;
        self.next_news = time + self.range(SECOND_NEWS) - FIRST_NEWS.0;
        self.since_news = 0.0;
        let k = (self.rand() % self.armies.len() as u64) as usize;
        Some(self.armies[k])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn at_most_two_broadcasts_about_the_right_army() {
        let mut n = News::new(2, 7);
        let mut aired = Vec::new();
        let mut t = 0.0;
        while t < 3600.0 {
            if let Some(a) = n.broadcast(t, 0.5) {
                aired.push((t, a));
            }
            t += 0.5;
        }
        assert_eq!(aired.len(), 2);
        assert!(aired.iter().all(|(_, a)| *a == 2));
        assert!(aired[0].0 >= FIRST_NEWS.0 && aired[1].0 - aired[0].0 >= SECOND_NEWS.0 - FIRST_NEWS.0);
    }
    #[test]
    fn headline_crosses_three_times_then_rests() {
        let mut n = News::new(CHAOS, 3);
        let (w, tw) = (500.0, 800.0);
        let mut shown = 0.0;
        let mut t = 0.0;
        while n.cur.is_none() {
            n.scroll(0.1, w, tw);
            t += 0.1;
            assert!(t < FIRST + 1.0);
        }
        while n.cur.is_some() {
            if n.scroll(0.1, w, tw) > 0.0 {
                shown += 0.1;
            }
        }
        let want = (w + tw) * PASSES as f32 / SPEED;
        assert!((shown - want).abs() < 2.0 * FADE + 0.5, "{shown} {want}");
    }
}

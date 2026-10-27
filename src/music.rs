//! Sound: Devin's music tracks as a shuffled playlist, the battle ambience under a game, the
//! sound effects and the news broadcasts. Everything is embedded in the exe and plays on one
//! audio thread (rodio, with symphonia's pure-Rust MP3 decoder); the game only sends it volumes and effects to play.
//! Neither the music nor the ambience loops cleanly on its own, so each one crossfades into its
//! next play.

use std::io::Cursor;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source};

static TRACKS: [&[u8]; 6] = [
    include_bytes!("../assets/music/horde-defense.mp3"),
    include_bytes!("../assets/music/approaching-danger.mp3"),
    include_bytes!("../assets/music/night-watch-protocol.mp3"),
    include_bytes!("../assets/music/night-watch-protocol-2.mp3"),
    include_bytes!("../assets/music/siege-pulse.mp3"),
    include_bytes!("../assets/music/night-watch-pulse.mp3"),
];
static AMBIENCE: [&[u8]; 1] = [include_bytes!("../assets/sfx/ambience.mp3")];
/// Devin's war drum, looped while a war-drum orc is on the map.
static DRUM: &[u8] = include_bytes!("../assets/sfx/orc-wardrum.mp3");
const DRUM_GAIN: f32 = 0.8;
/// seconds the drum takes to fade in or out
const DRUM_FADE: f32 = 0.8;

/// The news broadcast, from Devin's recordings: an opener, a report about the army in the game
/// (indexed like `FACTIONS`: zombies, aliens, orcs) and a closer, over the newsroom bed.
static OPENERS: [&[u8]; 3] = [
    include_bytes!("../assets/news/opener-1.mp3"),
    include_bytes!("../assets/news/opener-2.mp3"),
    include_bytes!("../assets/news/opener-3.mp3"),
];
static REPORTS: [[&[u8]; 5]; 3] = [
    [
        include_bytes!("../assets/news/zombie-1.mp3"),
        include_bytes!("../assets/news/zombie-2.mp3"),
        include_bytes!("../assets/news/zombie-3.mp3"),
        include_bytes!("../assets/news/zombie-4.mp3"),
        include_bytes!("../assets/news/zombie-5.mp3"),
    ],
    [
        include_bytes!("../assets/news/alien-1.mp3"),
        include_bytes!("../assets/news/alien-2.mp3"),
        include_bytes!("../assets/news/alien-3.mp3"),
        include_bytes!("../assets/news/alien-4.mp3"),
        include_bytes!("../assets/news/alien-5.mp3"),
    ],
    [
        include_bytes!("../assets/news/orc-1.mp3"),
        include_bytes!("../assets/news/orc-2.mp3"),
        include_bytes!("../assets/news/orc-3.mp3"),
        include_bytes!("../assets/news/orc-4.mp3"),
        include_bytes!("../assets/news/orc-5.mp3"),
    ],
];
static CLOSERS: [&[u8]; 3] = [
    include_bytes!("../assets/news/closer-1.mp3"),
    include_bytes!("../assets/news/closer-2.mp3"),
    include_bytes!("../assets/news/closer-3.mp3"),
];
static NEWSROOM: &[u8] = include_bytes!("../assets/news/newsroom.mp3");
/// The campaign's bulletins when a new army reaches Earth (Devin's recordings), indexed like
/// `FACTIONS`; the zombies are there from the start.
static ARRIVALS: [Option<&[u8]>; 3] = [None, Some(include_bytes!("../assets/news/alien-arrive.mp3")), Some(include_bytes!("../assets/news/orc-arrive.mp3"))];
/// a bulletin plays over the quiet campaign screen, a little louder than a broadcast in battle
const BULLETIN_GAIN: f32 = 1.5;
/// the announcer sits under the game (Devin: a little louder than at first, with its own
/// volume setting), the newsroom bed lower still
const NEWS_GAIN: f32 = 0.55;
const BED_GAIN: f32 = 0.16;
/// the bed fades in this long before the opener and out after the closer
const BED_LEAD: f32 = 1.2;
const BED_FADE: f32 = 1.5;

/// The sound effects, by the name of the file Devin made them for.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Snd {
    Cannon,
    ArtilleryShot,
    ArtilleryHit,
    Flame,
    Railgun,
    Gun,
    HeliGun,
    Nuke,
    Button,
    BaseAttacked,
}
const SNDS: usize = 10;
static SFX: [&[u8]; SNDS] = [
    include_bytes!("../assets/sfx/cannon-shell.wav"),
    include_bytes!("../assets/sfx/artillery-shot.mp3"),
    include_bytes!("../assets/sfx/artillery-explosion.mp3"),
    include_bytes!("../assets/sfx/flame-bunker.mp3"),
    include_bytes!("../assets/sfx/railgun.wav"),
    include_bytes!("../assets/sfx/gun-turret.mp3"),
    include_bytes!("../assets/sfx/helicopter-fire.mp3"),
    include_bytes!("../assets/sfx/icbm-explosion.mp3"),
    include_bytes!("../assets/sfx/button.mp3"),
    include_bytes!("../assets/sfx/base-attacked.mp3"),
];
/// at most this many copies of one effect play at once, and a new copy waits this long after the
/// last one, so a hundred cannons firing together stay a rumble instead of a wall of noise
const VOICES: [usize; SNDS] = [5, 3, 5, 3, 3, 5, 3, 2, 2, 1];
const GAP_MS: [u64; SNDS] = [70, 120, 90, 250, 150, 70, 200, 300, 40, 20_000];

/// seconds each music crossfade takes; the end of a track fades out while the next fades in
const FADE: f32 = 6.0;
const AMB_FADE: f32 = 2.5;
/// the music never plays at full output: it sits under the game
const MAX_GAIN: f32 = 0.8;
const AMB_GAIN: f32 = 0.35;
const SFX_GAIN: f32 = 0.9;

pub struct Play {
    pub snd: Snd,
    /// 0..1 before the effects volume
    pub gain: f32,
    /// -1 left .. 1 right
    pub pan: f32,
    /// playback speed: below 1 is deeper and longer
    pub speed: f32,
    /// nonzero: a later `stop` with the same key fades this copy out (a flame burst ending)
    pub key: u64,
}

enum Cmd {
    Play(Play),
    Stop(u64),
    /// a news broadcast about army 0..3 (see `REPORTS`)
    News(usize),
    /// the campaign bulletin for army 1..3 arriving (see `ARRIVALS`)
    Bulletin(usize),
}
/// seconds a stopped effect takes to fade out
const STOP_FADE: f32 = 0.15;

pub struct Music {
    /// volumes 0..=100, read by the audio thread
    vol: Arc<AtomicU32>,
    sfx_vol: Arc<AtomicU32>,
    news_vol: Arc<AtomicU32>,
    /// the battle ambience plays while this is set (a game is on screen)
    amb: Arc<AtomicBool>,
    /// the war drum's loudness, 0..=1000 (0: not playing)
    drum: Arc<AtomicU32>,
    tx: Option<mpsc::Sender<Cmd>>,
}

impl Music {
    pub fn start(vol: u32, sfx: u32, news: u32) -> Music {
        let mut m = Music {
            vol: Arc::new(AtomicU32::new(vol.min(100))),
            sfx_vol: Arc::new(AtomicU32::new(sfx.min(100))),
            news_vol: Arc::new(AtomicU32::new(news.min(100))),
            amb: Arc::new(AtomicBool::new(false)),
            drum: Arc::new(AtomicU32::new(0)),
            tx: None,
        };
        // headless test runs and the city fetcher stay silent (PH_SOUND=1 turns sound back on)
        let quiet = ["PH_SHOT", "PH_FETCH", "PH_NETTEST"].iter().any(|k| std::env::var(k).is_ok()) && std::env::var("PH_SOUND").is_err();
        if !quiet {
            let (tx, rx) = mpsc::channel();
            let (v, s, n, a, d) = (m.vol.clone(), m.sfx_vol.clone(), m.news_vol.clone(), m.amb.clone(), m.drum.clone());
            if std::thread::Builder::new().name("audio".into()).spawn(move || player(v, s, n, a, d, rx)).is_ok() {
                m.tx = Some(tx);
            }
        }
        m
    }
    pub fn set_volume(&self, vol: u32) {
        self.vol.store(vol.min(100), Ordering::Relaxed);
    }
    pub fn set_sfx_volume(&self, vol: u32) {
        self.sfx_vol.store(vol.min(100), Ordering::Relaxed);
    }
    pub fn set_news_volume(&self, vol: u32) {
        self.news_vol.store(vol.min(100), Ordering::Relaxed);
    }
    pub fn ambience(&self, on: bool) {
        self.amb.store(on, Ordering::Relaxed);
    }
    /// The war drum loops at this loudness (0..1) while it's above 0.
    pub fn drum(&self, gain: f32) {
        self.drum.store((gain.clamp(0.0, 1.0) * 1000.0) as u32, Ordering::Relaxed);
    }
    pub fn play(&self, p: Play) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Cmd::Play(p));
        }
    }
    /// Starts a news broadcast about `army` (0 zombies, 1 aliens, 2 orcs). It is cut short
    /// when the game ends (the ambience goes off).
    pub fn news(&self, army: usize) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Cmd::News(army.min(2)));
        }
    }
    /// Plays the campaign screen's bulletin that `army` has arrived, over the newsroom bed.
    pub fn bulletin(&self, army: usize) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Cmd::Bulletin(army.min(2)));
        }
    }
    /// Fades out the effects started with this key.
    pub fn stop(&self, key: u64) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Cmd::Stop(key));
        }
    }
}

// ------------------------------------------------------------------------------- playlists

struct Playing {
    sink: Sink,
    len: f32,
}

/// A track's length in seconds. MP3s without a length header are decoded once to count it.
fn length(data: &'static [u8]) -> f32 {
    let Ok(dec) = Decoder::new(Cursor::new(data)) else { return 0.0 };
    if let Some(d) = dec.total_duration() {
        return d.as_secs_f32();
    }
    let per_sec = (dec.sample_rate() * dec.channels() as u32) as f32;
    dec.count() as f32 / per_sec
}

fn start_track(handle: &OutputStreamHandle, data: &'static [u8], len: f32) -> Option<Playing> {
    let dec = Decoder::new(Cursor::new(data)).ok()?;
    let sink = Sink::try_new(handle).ok()?;
    sink.set_volume(0.0);
    sink.append(dec);
    Some(Playing { sink, len })
}

/// Tracks played one after another in shuffled rounds, each crossfading into the next.
struct Playlist {
    tracks: &'static [&'static [u8]],
    fade: f32,
    order: Vec<usize>,
    last: usize,
    lens: Vec<f32>,
    cur: Option<Playing>,
    old: Option<Playing>,
    /// 0..1, eases the whole list in and out when it's switched on or off
    level: f32,
}

impl Playlist {
    fn new(tracks: &'static [&'static [u8]], fade: f32) -> Playlist {
        let lens = tracks.iter().map(|t| length(t)).collect();
        Playlist { tracks, fade, order: Vec::new(), last: usize::MAX, lens, cur: None, old: None, level: 0.0 }
    }
    fn next(&mut self, rng: &mut u64) -> usize {
        if self.order.is_empty() {
            // shuffle a fresh round, never repeating the track that just played
            self.order = (0..self.tracks.len()).collect();
            for i in (1..self.order.len()).rev() {
                *rng ^= *rng << 13;
                *rng ^= *rng >> 7;
                *rng ^= *rng << 17;
                self.order.swap(i, (*rng % (i as u64 + 1)) as usize);
            }
            if self.order.len() > 1 && self.order.last() == Some(&self.last) {
                let n = self.order.len();
                self.order.swap(0, n - 1);
            }
        }
        self.last = self.order.pop().unwrap();
        self.last
    }
    /// Fade gain of a track: ramps in over the first `fade` seconds and out over the last.
    fn shape(&self, p: &Playing) -> f32 {
        let t = p.sink.get_pos().as_secs_f32();
        let fin = (t / self.fade).min(1.0);
        let fout = if p.len > 0.0 { ((p.len - t) / self.fade).clamp(0.0, 1.0) } else { 1.0 };
        fin.min(fout)
    }
    /// One step of the audio thread: `on` plays the list, `gain` is its volume.
    fn update(&mut self, handle: &OutputStreamHandle, rng: &mut u64, on: bool, gain: f32, dt: f32) {
        self.level = if on { (self.level + dt / self.fade).min(1.0) } else { (self.level - dt / self.fade).max(0.0) };
        if self.level <= 0.0 {
            // fully off: stop, and start from a fresh track next time
            self.cur = None;
            self.old = None;
            return;
        }
        match &self.cur {
            None => {
                let i = self.next(rng);
                self.cur = start_track(handle, self.tracks[i], self.lens[i]);
            }
            Some(c) => {
                // start the next track as this one begins to fade out
                let left = c.len - c.sink.get_pos().as_secs_f32();
                if c.sink.empty() || (c.len > 0.0 && left <= self.fade) {
                    self.old = self.cur.take();
                    let i = self.next(rng);
                    self.cur = start_track(handle, self.tracks[i], self.lens[i]);
                }
            }
        }
        for p in self.old.iter().chain(self.cur.iter()) {
            // an equal-power curve keeps the crossfade from dipping in the middle
            let f = self.shape(p);
            p.sink.set_volume(gain * self.level * (f * std::f32::consts::FRAC_PI_2).sin());
        }
        if self.old.as_ref().is_some_and(|o| o.sink.empty()) {
            self.old = None;
        }
    }
}

// ------------------------------------------------------------------------------------ news

/// A broadcast on the air: the announcer's lines queued one after another on `voice`, the
/// newsroom bed easing in under them and out once they're done.
struct News {
    voice: Sink,
    bed: Sink,
    /// seconds since it started
    t: f32,
    /// set when the voice has finished (or the game ended): seconds since
    done: Option<f32>,
    /// a campaign bulletin: plays on outside a game, a little louder
    bulletin: bool,
}

fn pick(rng: &mut u64, n: usize) -> usize {
    *rng ^= *rng << 13;
    *rng ^= *rng >> 7;
    *rng ^= *rng << 17;
    (*rng % n as u64) as usize
}

fn start_news(handle: &OutputStreamHandle, rng: &mut u64, army: usize) -> Option<News> {
    let voice = Sink::try_new(handle).ok()?;
    let bed = Sink::try_new(handle).ok()?;
    voice.set_volume(0.0);
    voice.append(rodio::source::Zero::<f32>::new(1, 44100).take_duration(Duration::from_secs_f32(BED_LEAD)));
    let parts = [OPENERS[pick(rng, OPENERS.len())], REPORTS[army][pick(rng, 5)], CLOSERS[pick(rng, CLOSERS.len())]];
    for (i, b) in parts.into_iter().enumerate() {
        if i > 0 {
            voice.append(rodio::source::Zero::<f32>::new(1, 44100).take_duration(Duration::from_secs_f32(0.35)));
        }
        voice.append(Decoder::new(Cursor::new(b)).ok()?);
    }
    bed.set_volume(0.0);
    bed.append(Decoder::new(Cursor::new(NEWSROOM)).ok()?);
    Some(News { voice, bed, t: 0.0, done: None, bulletin: false })
}

fn start_bulletin(handle: &OutputStreamHandle, army: usize) -> Option<News> {
    let clip = ARRIVALS.get(army).copied().flatten()?;
    let voice = Sink::try_new(handle).ok()?;
    let bed = Sink::try_new(handle).ok()?;
    voice.set_volume(0.0);
    voice.append(rodio::source::Zero::<f32>::new(1, 44100).take_duration(Duration::from_secs_f32(BED_LEAD)));
    voice.append(Decoder::new(Cursor::new(clip)).ok()?);
    bed.set_volume(0.0);
    bed.append(Decoder::new(Cursor::new(NEWSROOM)).ok()?);
    Some(News { voice, bed, t: 0.0, done: None, bulletin: true })
}

impl News {
    /// One step: `on` is false once the game is over. Returns false when it's finished.
    fn update(&mut self, on: bool, gain: f32, dt: f32) -> bool {
        self.t += dt;
        let on = on || self.bulletin;
        let gain = if self.bulletin { gain * BULLETIN_GAIN } else { gain };
        if self.done.is_none() && (self.voice.empty() || !on) {
            self.done = Some(0.0);
        }
        let mut bed = (self.t / BED_LEAD).min(1.0);
        let mut voice = 1.0;
        if let Some(d) = self.done.as_mut() {
            *d += dt;
            bed = bed.min(1.0 - *d / BED_FADE);
            voice = (1.0 - *d / 0.3).max(0.0);
            if bed <= 0.0 {
                return false;
            }
        }
        self.voice.set_volume(gain * NEWS_GAIN * voice);
        self.bed.set_volume(gain * BED_GAIN * bed.max(0.0));
        true
    }
}

// --------------------------------------------------------------------------------- effects

/// A decoded effect, shared by every copy that plays it.
struct Clip {
    /// interleaved stereo
    data: Arc<[f32]>,
    rate: u32,
    playing: Arc<AtomicUsize>,
}

fn decode(bytes: &'static [u8]) -> Option<Clip> {
    let dec = Decoder::new(Cursor::new(bytes)).ok()?;
    let (ch, rate) = (dec.channels() as usize, dec.sample_rate());
    let s: Vec<f32> = dec.convert_samples().collect();
    let data: Vec<f32> = if ch == 1 {
        s.iter().flat_map(|v| [*v, *v]).collect()
    } else {
        s.chunks_exact(ch).flat_map(|f| [f[0], f[1]]).collect()
    };
    Some(Clip { data: data.into(), rate, playing: Arc::new(AtomicUsize::new(0)) })
}

/// One playing copy of a clip, panned; counts itself in `playing` while it lives.
struct Voice {
    data: Arc<[f32]>,
    i: usize,
    rate: u32,
    gl: f32,
    gr: f32,
    playing: Arc<AtomicUsize>,
    /// set when the effect is stopped; it then fades out over STOP_FADE
    stopped: Arc<AtomicBool>,
    fade: f32,
}
impl Drop for Voice {
    fn drop(&mut self) {
        self.playing.fetch_sub(1, Ordering::Relaxed);
    }
}
impl Iterator for Voice {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        let v = *self.data.get(self.i)?;
        let g = if self.i % 2 == 0 { self.gl } else { self.gr };
        if self.stopped.load(Ordering::Relaxed) {
            self.fade -= 1.0 / (STOP_FADE * self.rate as f32 * 2.0);
            if self.fade <= 0.0 {
                return None;
            }
        }
        self.i += 1;
        Some(v * g * self.fade)
    }
}
impl Source for Voice {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        2
    }
    fn sample_rate(&self) -> u32 {
        self.rate
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

fn player(vol: Arc<AtomicU32>, sfx_vol: Arc<AtomicU32>, news_vol: Arc<AtomicU32>, amb: Arc<AtomicBool>, drum_gain: Arc<AtomicU32>, rx: mpsc::Receiver<Cmd>) {
    // no audio device: no sound, the game carries on
    let Ok((_stream, handle)) = OutputStream::try_default() else { return };
    let mut rng = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1) | 1;
    let mut music = Playlist::new(&TRACKS, FADE);
    let mut ambience = Playlist::new(&AMBIENCE, AMB_FADE);
    let clips: Vec<Option<Clip>> = SFX.iter().map(|b| decode(b)).collect();
    let mut last_play = [Instant::now() - Duration::from_secs(1); SNDS];
    let mut keyed: Vec<(u64, Arc<AtomicBool>)> = Vec::new();
    let mut news: Option<News> = None;
    // the war drum: one endless loop, eased in and out
    let mut drum: Option<Sink> = None;
    let mut drum_level = 0.0f32;
    let mut t = Instant::now();
    loop {
        let dt = t.elapsed().as_secs_f32().min(0.5);
        t = Instant::now();
        // perceived loudness is roughly the square of the gain, so the volume settings are squared
        let sq = |a: &AtomicU32| {
            let v = a.load(Ordering::Relaxed) as f32 / 100.0;
            v * v
        };
        let (mv, sv, nv) = (sq(&vol), sq(&sfx_vol), sq(&news_vol));
        music.update(&handle, &mut rng, true, mv * MAX_GAIN, dt);
        let in_game = amb.load(Ordering::Relaxed);
        ambience.update(&handle, &mut rng, in_game && sv > 0.0, sv * AMB_GAIN, dt);
        if news.as_mut().is_some_and(|n| !n.update(in_game, nv, dt)) {
            news = None;
        }
        let want = if in_game { drum_gain.load(Ordering::Relaxed) as f32 / 1000.0 } else { 0.0 };
        drum_level = if want > drum_level { (drum_level + dt / DRUM_FADE).min(want) } else { (drum_level - dt / DRUM_FADE).max(want) };
        if drum_level > 0.0 && drum.is_none() {
            drum = Decoder::new(Cursor::new(DRUM)).ok().and_then(|dec| {
                let sink = Sink::try_new(&handle).ok()?;
                sink.set_volume(0.0);
                sink.append(dec.repeat_infinite());
                Some(sink)
            });
        }
        if drum_level <= 0.0 {
            drum = None;
        }
        if let Some(d) = &drum {
            d.set_volume(drum_level * sv * DRUM_GAIN);
        }
        // effects queued since the last step; wait for the next one for up to 50 ms
        let deadline = t + Duration::from_millis(50);
        while let Ok(cmd) = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            let p = match cmd {
                Cmd::Play(p) => p,
                Cmd::Stop(key) => {
                    for (k, s) in &keyed {
                        if *k == key {
                            s.store(true, Ordering::Relaxed);
                        }
                    }
                    continue;
                }
                Cmd::News(army) => {
                    if news.is_none() && nv > 0.0 {
                        news = start_news(&handle, &mut rng, army);
                    }
                    continue;
                }
                Cmd::Bulletin(army) => {
                    if nv > 0.0 {
                        news = start_bulletin(&handle, army);
                    }
                    continue;
                }
            };
            // forget keys whose voice has finished
            keyed.retain(|(_, s)| Arc::strong_count(s) > 1);
            let k = p.snd as usize;
            let Some(c) = &clips[k] else { continue };
            let now = Instant::now();
            let g = p.gain.clamp(0.0, 1.0) * sv * SFX_GAIN;
            if g < 0.003 || c.playing.load(Ordering::Relaxed) >= VOICES[k] || now - last_play[k] < Duration::from_millis(GAP_MS[k]) {
                continue;
            }
            last_play[k] = now;
            c.playing.fetch_add(1, Ordering::Relaxed);
            let pan = p.pan.clamp(-1.0, 1.0);
            let stopped = Arc::new(AtomicBool::new(false));
            if p.key != 0 {
                keyed.push((p.key, stopped.clone()));
            }
            let v = Voice {
                stopped,
                fade: 1.0,
                data: c.data.clone(),
                i: 0,
                rate: c.rate,
                gl: g * (1.0 - pan.max(0.0) * 0.7),
                gr: g * (1.0 + pan.min(0.0) * 0.7),
                playing: c.playing.clone(),
            };
            let _ = handle.play_raw(v.speed(p.speed.clamp(0.3, 3.0)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tracks_decode_with_a_length() {
        for t in TRACKS.iter().chain(AMBIENCE.iter()) {
            let len = length(t);
            assert!(len > 20.0, "{len}");
        }
    }
    #[test]
    fn stopped_voice_fades_out() {
        let c = decode(SFX[Snd::Flame as usize]).unwrap();
        let stopped = Arc::new(AtomicBool::new(false));
        c.playing.fetch_add(1, Ordering::Relaxed);
        let mut v = Voice { data: c.data.clone(), i: 0, rate: c.rate, gl: 1.0, gr: 1.0, playing: c.playing.clone(), stopped: stopped.clone(), fade: 1.0 };
        for _ in 0..1000 {
            v.next().unwrap();
        }
        stopped.store(true, Ordering::Relaxed);
        let rest = v.count();
        let fade = (STOP_FADE * c.rate as f32 * 2.0) as usize;
        assert!(rest <= fade + 1 && rest + 1000 < c.data.len(), "{rest}");
        assert_eq!(c.playing.load(Ordering::Relaxed), 0);
    }
    #[test]
    fn news_decodes() {
        let all = OPENERS.iter().chain(REPORTS.iter().flatten()).chain(CLOSERS.iter()).chain(ARRIVALS.iter().flatten());
        for b in all {
            let len = length(b);
            assert!(len > 0.5 && len < 20.0, "{len}");
        }
        assert!(length(NEWSROOM) > 60.0);
    }
    #[test]
    fn effects_decode() {
        for b in SFX {
            let c = decode(b).expect("decodes");
            assert!(c.data.len() > 4_000);
        }
    }
}

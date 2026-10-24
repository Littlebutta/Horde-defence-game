//! Sound: Devin's music tracks as a shuffled playlist, the battle ambience under a game, and the
//! weapon sound effects. Everything is embedded in the exe and plays on one audio thread (rodio,
//! with symphonia's pure-Rust MP3 decoder); the game only sends it volumes and effects to play.
//! Neither the music nor the ambience loops cleanly on its own, so each one crossfades into its
//! next play.

use std::io::Cursor;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source};

static TRACKS: [&[u8]; 4] = [
    include_bytes!("../assets/music/horde-defense.mp3"),
    include_bytes!("../assets/music/approaching-danger.mp3"),
    include_bytes!("../assets/music/night-watch-protocol.mp3"),
    include_bytes!("../assets/music/night-watch-protocol-2.mp3"),
];
static AMBIENCE: [&[u8]; 1] = [include_bytes!("../assets/sfx/ambience.mp3")];

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
}
const SNDS: usize = 8;
static SFX: [&[u8]; SNDS] = [
    include_bytes!("../assets/sfx/cannon-shell.wav"),
    include_bytes!("../assets/sfx/artillery-shot.mp3"),
    include_bytes!("../assets/sfx/artillery-explosion.mp3"),
    include_bytes!("../assets/sfx/flame-bunker.mp3"),
    include_bytes!("../assets/sfx/railgun.wav"),
    include_bytes!("../assets/sfx/gun-turret.mp3"),
    include_bytes!("../assets/sfx/helicopter-fire.mp3"),
    include_bytes!("../assets/sfx/icbm-explosion.mp3"),
];
/// at most this many copies of one effect play at once, and a new copy waits this long after the
/// last one, so a hundred cannons firing together stay a rumble instead of a wall of noise
const VOICES: [usize; SNDS] = [5, 3, 5, 3, 3, 5, 3, 2];
const GAP_MS: [u64; SNDS] = [70, 120, 90, 250, 150, 70, 200, 300];

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
}
/// seconds a stopped effect takes to fade out
const STOP_FADE: f32 = 0.15;

pub struct Music {
    /// volumes 0..=100, read by the audio thread
    vol: Arc<AtomicU32>,
    sfx_vol: Arc<AtomicU32>,
    /// the battle ambience plays while this is set (a game is on screen)
    amb: Arc<AtomicBool>,
    tx: Option<mpsc::Sender<Cmd>>,
}

impl Music {
    pub fn start(vol: u32, sfx: u32) -> Music {
        let mut m = Music {
            vol: Arc::new(AtomicU32::new(vol.min(100))),
            sfx_vol: Arc::new(AtomicU32::new(sfx.min(100))),
            amb: Arc::new(AtomicBool::new(false)),
            tx: None,
        };
        // headless test runs and the city fetcher stay silent (PH_SOUND=1 turns sound back on)
        let quiet = ["PH_SHOT", "PH_FETCH", "PH_NETTEST"].iter().any(|k| std::env::var(k).is_ok()) && std::env::var("PH_SOUND").is_err();
        if !quiet {
            let (tx, rx) = mpsc::channel();
            let (v, s, a) = (m.vol.clone(), m.sfx_vol.clone(), m.amb.clone());
            if std::thread::Builder::new().name("audio".into()).spawn(move || player(v, s, a, rx)).is_ok() {
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
    pub fn ambience(&self, on: bool) {
        self.amb.store(on, Ordering::Relaxed);
    }
    pub fn play(&self, p: Play) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Cmd::Play(p));
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

fn player(vol: Arc<AtomicU32>, sfx_vol: Arc<AtomicU32>, amb: Arc<AtomicBool>, rx: mpsc::Receiver<Cmd>) {
    // no audio device: no sound, the game carries on
    let Ok((_stream, handle)) = OutputStream::try_default() else { return };
    let mut rng = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1) | 1;
    let mut music = Playlist::new(&TRACKS, FADE);
    let mut ambience = Playlist::new(&AMBIENCE, AMB_FADE);
    let clips: Vec<Option<Clip>> = SFX.iter().map(|b| decode(b)).collect();
    let mut last_play = [Instant::now() - Duration::from_secs(1); SNDS];
    let mut keyed: Vec<(u64, Arc<AtomicBool>)> = Vec::new();
    let mut t = Instant::now();
    loop {
        let dt = t.elapsed().as_secs_f32().min(0.5);
        t = Instant::now();
        // perceived loudness is roughly the square of the gain, so the volume settings are squared
        let sq = |a: &AtomicU32| {
            let v = a.load(Ordering::Relaxed) as f32 / 100.0;
            v * v
        };
        let (mv, sv) = (sq(&vol), sq(&sfx_vol));
        music.update(&handle, &mut rng, true, mv * MAX_GAIN, dt);
        ambience.update(&handle, &mut rng, amb.load(Ordering::Relaxed) && sv > 0.0, sv * AMB_GAIN, dt);
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
    fn effects_decode() {
        for b in SFX {
            let c = decode(b).expect("decodes");
            assert!(c.data.len() > 10_000);
        }
    }
}

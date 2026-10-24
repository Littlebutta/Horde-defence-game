//! Background music: Devin's tracks, embedded in the exe, played as a shuffled playlist.
//! The tracks don't loop on their own, so each one crossfades into the next. Playback runs on its
//! own thread (rodio streams the MP3s), so the game only sets the volume.

use std::io::Cursor;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rodio::{Decoder, OutputStream, Sink, Source};

static TRACKS: [&[u8]; 4] = [
    include_bytes!("../assets/music/horde-defense.mp3"),
    include_bytes!("../assets/music/approaching-danger.mp3"),
    include_bytes!("../assets/music/night-watch-protocol.mp3"),
    include_bytes!("../assets/music/night-watch-protocol-2.mp3"),
];
/// seconds each crossfade takes; the end of a track fades out while the next fades in
const FADE: f32 = 6.0;
/// the music never plays at full output: it sits under the game
const MAX_GAIN: f32 = 0.8;

pub struct Music {
    /// volume 0..=100, read by the player thread
    vol: Arc<AtomicU32>,
}

impl Music {
    pub fn start(vol: u32) -> Music {
        let v = Arc::new(AtomicU32::new(vol.min(100)));
        // headless test runs and the city fetcher stay silent
        let quiet = ["PH_SHOT", "PH_FETCH", "PH_NETTEST"].iter().any(|k| std::env::var(k).is_ok());
        if !quiet {
            let v2 = v.clone();
            let _ = std::thread::Builder::new().name("music".into()).spawn(move || player(v2));
        }
        Music { vol: v }
    }
    pub fn set_volume(&self, vol: u32) {
        self.vol.store(vol.min(100), Ordering::Relaxed);
    }
}

struct Playing {
    sink: Sink,
    len: f32,
}

fn play(handle: &rodio::OutputStreamHandle, i: usize) -> Option<Playing> {
    let dec = Decoder::new(Cursor::new(TRACKS[i])).ok()?;
    let len = dec.total_duration().map(|d| d.as_secs_f32()).unwrap_or(0.0);
    let sink = Sink::try_new(handle).ok()?;
    sink.set_volume(0.0);
    sink.append(dec);
    Some(Playing { sink, len })
}

/// Fade gain of a track: ramps in over the first FADE seconds and out over the last FADE.
fn fade(p: &Playing) -> f32 {
    let t = p.sink.get_pos().as_secs_f32();
    let fin = (t / FADE).min(1.0);
    let fout = if p.len > 0.0 { ((p.len - t) / FADE).clamp(0.0, 1.0) } else { 1.0 };
    fin.min(fout)
}

fn player(vol: Arc<AtomicU32>) {
    // no audio device: no music, the game carries on
    let Ok((_stream, handle)) = OutputStream::try_default() else { return };
    let mut rng = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1) | 1;
    let mut order: Vec<usize> = Vec::new();
    let mut last = usize::MAX;
    let mut next = move || {
        if order.is_empty() {
            // shuffle a fresh round, never repeating the track that just played
            order = (0..TRACKS.len()).collect();
            for i in (1..order.len()).rev() {
                rng ^= rng << 13;
                rng ^= rng >> 7;
                rng ^= rng << 17;
                order.swap(i, (rng % (i as u64 + 1)) as usize);
            }
            if order.last() == Some(&last) {
                order.swap(0, TRACKS.len() - 1);
            }
        }
        last = order.pop().unwrap();
        last
    };
    let mut cur = play(&handle, next());
    let mut old: Option<Playing> = None;
    loop {
        // perceived loudness is roughly the square of the gain, so the slider is squared
        let v = vol.load(Ordering::Relaxed) as f32 / 100.0;
        let master = v * v * MAX_GAIN;
        // a track that can't be decoded is skipped
        let Some(c) = &cur else {
            cur = play(&handle, next());
            std::thread::sleep(Duration::from_millis(200));
            continue;
        };
        // start the next track as this one begins to fade out
        let left = c.len - c.sink.get_pos().as_secs_f32();
        if c.sink.empty() || (c.len > 0.0 && left <= FADE) {
            old = cur.take();
            cur = play(&handle, next());
        }
        for p in old.iter().chain(cur.iter()) {
            // an equal-power curve keeps the crossfade from dipping in the middle
            let f = fade(p);
            p.sink.set_volume(master * (f * std::f32::consts::FRAC_PI_2).sin());
        }
        if old.as_ref().is_some_and(|o| o.sink.empty()) {
            old = None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tracks_decode_with_a_length() {
        for t in TRACKS {
            let d = Decoder::new(Cursor::new(t)).unwrap();
            let len = d.total_duration().expect("length").as_secs_f32();
            assert!(len > 60.0, "{len}");
            eprintln!("{len:.1}s at {} Hz", d.sample_rate());
        }
    }
}

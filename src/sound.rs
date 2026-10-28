//! Typewriter sounds: a Hermes Precisa 305, recorded by Joseph Sardin and
//! released CC0 (see `assets/sounds/PROVENANCE.md`).
//!
//! Every key gets one of eight strikes cut from real typing, played a
//! touch faster or slower and louder or softer than the last, so a run
//! of the same letter never sounds like a sample repeating. The space
//! bar, the carriage return and the margin bell are their own
//! recordings.
//!
//! Playback is a small mixer in the audio callback: strikes overlap the
//! way they do when you type fast. The output stream only exists while
//! sounds are on, so a silent app holds no audio device open.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Sound {
    Key,
    Space,
    Return,
    /// The bell a few characters before the right margin.
    Bell,
}

const KEYS: [&[u8]; 8] = [
    include_bytes!("../assets/sounds/key-1.wav"),
    include_bytes!("../assets/sounds/key-2.wav"),
    include_bytes!("../assets/sounds/key-3.wav"),
    include_bytes!("../assets/sounds/key-4.wav"),
    include_bytes!("../assets/sounds/key-5.wav"),
    include_bytes!("../assets/sounds/key-6.wav"),
    include_bytes!("../assets/sounds/key-7.wav"),
    include_bytes!("../assets/sounds/key-8.wav"),
];
const SPACE: &[u8] = include_bytes!("../assets/sounds/space.wav");
const RETURN: &[u8] = include_bytes!("../assets/sounds/return.wav");
const BELL: &[u8] = include_bytes!("../assets/sounds/bell.wav");

/// Overall level. The recordings are normalised; a typewriter in the
/// next room is the goal, not one strapped to your head.
const VOLUME: f32 = 0.45;
/// Strikes sounding at once. More than this and the oldest is dropped.
const MAX_VOICES: usize = 24;

/// A 16-bit PCM WAV as samples in -1..1, with its sample rate.
pub fn decode(wav: &[u8]) -> (u32, Vec<f32>) {
    let le16 = |o: usize| u16::from_le_bytes([wav[o], wav[o + 1]]);
    let le32 = |o: usize| u32::from_le_bytes([wav[o], wav[o + 1], wav[o + 2], wav[o + 3]]);
    let mut rate = 48_000;
    let mut channels = 1usize;
    let mut at = 12;
    while at + 8 <= wav.len() {
        let id = &wav[at..at + 4];
        let len = le32(at + 4) as usize;
        let body = at + 8;
        if id == b"fmt " {
            channels = le16(body + 2).max(1) as usize;
            rate = le32(body + 4);
        } else if id == b"data" {
            let end = (body + len).min(wav.len());
            let samples = wav[body..end]
                .chunks_exact(2 * channels)
                .map(|frame| {
                    // Mix down to mono.
                    frame
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|s| i16::from_le_bytes(*s) as f32 / 32768.0)
                        .sum::<f32>()
                        / channels as f32
                })
                .collect();
            return (rate, samples);
        }
        at = body + len + (len & 1);
    }
    (rate, Vec::new())
}

struct Clip {
    rate: u32,
    samples: Arc<[f32]>,
}

impl Clip {
    fn new(wav: &[u8]) -> Self {
        let (rate, samples) = decode(wav);
        Self {
            rate,
            samples: samples.into(),
        }
    }
}

/// One sound playing.
struct Voice {
    samples: Arc<[f32]>,
    /// Position in source samples; fractional, since the source rate,
    /// the device rate and the pitch all differ.
    pos: f32,
    step: f32,
    gain: f32,
}

/// Mix every voice into an interleaved output buffer, dropping the ones
/// that have finished.
fn mix(voices: &mut Vec<Voice>, out: &mut [f32], channels: usize) {
    for frame in out.chunks_mut(channels.max(1)) {
        let mut sum = 0.0;
        for v in voices.iter_mut() {
            let i = v.pos as usize;
            if let (Some(&a), b) = (v.samples.get(i), v.samples.get(i + 1)) {
                let t = v.pos - i as f32;
                sum += (a + (b.copied().unwrap_or(0.0) - a) * t) * v.gain;
            }
            v.pos += v.step;
        }
        let s = sum.clamp(-1.0, 1.0);
        frame.iter_mut().for_each(|o| *o = s);
    }
    voices.retain(|v| (v.pos as usize) < v.samples.len());
}

/// The sound bank and an open output stream.
pub struct Player {
    _stream: cpal::Stream,
    tx: Sender<Voice>,
    device_rate: f32,
    keys: Vec<Clip>,
    space: Clip,
    ret: Clip,
    bell: Clip,
    /// Which key strike played last, and a counter for the variation.
    last_key: usize,
    strikes: u64,
}

impl Player {
    /// Open the default output device. `None` when there is no audio
    /// output to be had, which the caller reports and survives.
    pub fn open() -> Option<Self> {
        let device = cpal::default_host().default_output_device()?;
        let supported = device.default_output_config().ok()?;
        let config: cpal::StreamConfig = supported.config();
        let channels = config.channels as usize;
        let device_rate = config.sample_rate as f32;

        let (tx, rx): (Sender<Voice>, Receiver<Voice>) = mpsc::channel();
        let mut voices: Vec<Voice> = Vec::with_capacity(MAX_VOICES);
        let stream = device
            .build_output_stream::<f32, _, _>(
                config,
                move |out: &mut [f32], _| {
                    while let Ok(v) = rx.try_recv() {
                        if voices.len() >= MAX_VOICES {
                            voices.remove(0);
                        }
                        voices.push(v);
                    }
                    mix(&mut voices, out, channels);
                },
                |e| eprintln!("audio: {e}"),
                None,
            )
            .ok()?;
        stream.play().ok()?;

        Some(Self {
            _stream: stream,
            tx,
            device_rate,
            keys: KEYS.iter().map(|w| Clip::new(w)).collect(),
            space: Clip::new(SPACE),
            ret: Clip::new(RETURN),
            bell: Clip::new(BELL),
            last_key: 0,
            strikes: 0,
        })
    }

    pub fn play(&mut self, sound: Sound) {
        self.strikes = self.strikes.wrapping_add(1);
        let r = mix64(self.strikes);
        // Two values in -1..1 for this strike's pitch and level.
        let a = ((r >> 11) & 0xFFFF) as f32 / 32768.0 - 1.0;
        let b = ((r >> 33) & 0xFFFF) as f32 / 32768.0 - 1.0;
        let (clip, pitch, level) = match sound {
            Sound::Key => {
                // Never the same strike twice running.
                let mut i = (r % self.keys.len() as u64) as usize;
                if i == self.last_key {
                    i = (i + 1) % self.keys.len();
                }
                self.last_key = i;
                (&self.keys[i], 1.0 + a * 0.035, 0.9 + b * 0.12)
            }
            Sound::Space => (&self.space, 1.0 + a * 0.02, 0.85),
            Sound::Return => (&self.ret, 1.0, 0.8),
            Sound::Bell => (&self.bell, 1.0, 0.5),
        };
        let _ = self.tx.send(Voice {
            samples: clip.samples.clone(),
            pos: 0.0,
            step: clip.rate as f32 / self.device_rate * pitch,
            gain: VOLUME * level,
        });
    }
}

/// SplitMix64, for strike-to-strike variation.
fn mix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_recording_decodes() {
        for wav in KEYS.iter().chain([SPACE, RETURN, BELL].iter()) {
            let (rate, samples) = decode(wav);
            assert_eq!(rate, 48_000);
            assert!(samples.len() > 1_000);
            assert!(samples.iter().all(|s| (-1.0..=1.0).contains(s)));
            let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            assert!(peak > 0.5, "normalised");
        }
    }

    #[test]
    fn key_strikes_are_short_and_start_on_the_hit() {
        for wav in KEYS {
            let (rate, samples) = decode(wav);
            assert!(samples.len() < rate as usize / 5, "under 200ms");
            // The loudest moment is within the first 20ms: the click
            // must not lag the key.
            let loudest = samples
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
                .map(|(i, _)| i)
                .unwrap_or(0);
            assert!(loudest < rate as usize / 50);
        }
    }

    #[test]
    fn voices_mix_and_finish() {
        let clip: Arc<[f32]> = vec![0.5; 4].into();
        let mut voices = vec![
            Voice {
                samples: clip.clone(),
                pos: 0.0,
                step: 1.0,
                gain: 1.0,
            },
            Voice {
                samples: clip,
                pos: 0.0,
                step: 1.0,
                gain: 1.0,
            },
        ];
        let mut out = vec![0.0; 4]; // two stereo frames
        mix(&mut voices, &mut out, 2);
        assert_eq!(
            out,
            vec![1.0, 1.0, 1.0, 1.0],
            "summed, clamped, both channels"
        );
        assert_eq!(voices.len(), 2, "still playing");
        let mut out = vec![0.0; 8];
        mix(&mut voices, &mut out, 2);
        assert!(voices.is_empty(), "finished voices are dropped");
        assert_eq!(&out[4..], &[0.0; 4], "silence after the end");
    }
}

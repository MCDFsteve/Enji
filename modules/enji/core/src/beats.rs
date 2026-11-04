//! Where the beat of the music falls, worked out from the samples as they
//! are decoded. How strongly the spectrum rises at each moment (spectral
//! flux) shows the attacks of the drums; the delay at which that signal
//! repeats best is the tempo, and the offset at which its peaks line up is
//! the phase. A beat is told when it is due, by prediction: it can come
//! ahead of the sound by `lead` seconds, for whatever shows it to land on
//! the beat, and it keeps coming through a bar the drums leave out.

/// Samples of each spectrum, and how far apart they are taken.
const WINDOW: usize = 1024;
const HOP: usize = 512;
/// The spectrum above this does not help to find the beat.
const HIGHEST_HZ: f64 = 6000.0;
/// How much of the music the tempo is read from.
const MEMORY_SECONDS: f64 = 8.0;
/// How much must be heard before a tempo is believed.
const SETTLE_SECONDS: f64 = 3.0;
/// The tempi looked for; above this the beat is taken at half time, the
/// pulse people move to.
const SLOWEST_BPM: f64 = 62.0;
const FASTEST_BPM: f64 = 165.0;
/// The tempo preferred when the music fits several.
const LIKELIEST_BPM: f64 = 105.0;
/// How often the tempo and phase are read again, in hops.
const READ_EVERY: u64 = 8;
/// Samples below this loudness are silence.
const SILENCE: f32 = 0.004;
/// How clearly the music must repeat for its beat to be followed.
const CLEAR: f32 = 0.10;

/// A beat, due `lead` seconds from when it is told.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Beat {
    /// How strong it is, from 0 to 1: how hard the music hits, for how loud
    /// it is now.
    pub strength: f32,
    /// The tempo it follows.
    pub bpm: f32,
}

pub struct Beats {
    hop_seconds: f64,
    lead_hops: f64,
    /// The latest samples, and how many came since the last spectrum.
    samples: Vec<f32>,
    fresh: usize,
    hann: Vec<f32>,
    cos: Vec<f32>,
    sin: Vec<f32>,
    reversed: Vec<u32>,
    real: Vec<f32>,
    imaginary: Vec<f32>,
    /// Log magnitudes of the last two spectra, the older first.
    spectra: [Vec<f32>; 2],
    bins: usize,
    /// How strongly the spectrum rose at each hop, a ring.
    rises: Vec<f32>,
    /// Hops heard.
    at: u64,
    /// The loudest the music has been lately, and how loud it is now.
    loudest: f32,
    loud: f32,
    /// The beat's period in hops, 0 until known, and how clear it is.
    period: f64,
    clarity: f32,
    /// A different period heard, and how many readings in a row.
    other: f64,
    other_for: u32,
    /// The hop the next beat falls on.
    next: f64,
    /// How far from it the peaks line up, and how many readings in a row.
    moved: f64,
    moved_for: u32,
}

impl Beats {
    /// For samples at `rate` per second, telling each beat `lead` seconds
    /// ahead of it.
    pub fn new(rate: u32, lead: f64) -> Self {
        let rate = f64::from(rate.max(1));
        let hop_seconds = HOP as f64 / rate;
        let bits = WINDOW.trailing_zeros();
        let tau = std::f32::consts::TAU;
        let bins = (((HIGHEST_HZ / rate) * WINDOW as f64) as usize).clamp(8, WINDOW / 2);
        Beats {
            hop_seconds,
            lead_hops: lead / hop_seconds,
            samples: vec![0.0; WINDOW],
            fresh: 0,
            hann: (0..WINDOW).map(|i| 0.5 - 0.5 * (tau * i as f32 / WINDOW as f32).cos()).collect(),
            cos: (0..WINDOW / 2).map(|i| (tau * i as f32 / WINDOW as f32).cos()).collect(),
            sin: (0..WINDOW / 2).map(|i| (tau * i as f32 / WINDOW as f32).sin()).collect(),
            reversed: (0..WINDOW as u32).map(|i| i.reverse_bits() >> (32 - bits)).collect(),
            real: vec![0.0; WINDOW],
            imaginary: vec![0.0; WINDOW],
            spectra: [vec![0.0; bins], vec![0.0; bins]],
            bins,
            rises: vec![0.0; (MEMORY_SECONDS / hop_seconds) as usize],
            at: 0,
            loudest: 0.0,
            loud: 0.0,
            period: 0.0,
            clarity: 0.0,
            other: 0.0,
            other_for: 0,
            next: 0.0,
            moved: 0.0,
            moved_for: 0,
        }
    }

    /// The tempo followed now; 0 until one is known.
    pub fn bpm(&self) -> f64 {
        if self.period > 0.0 { 60.0 / (self.period * self.hop_seconds) } else { 0.0 }
    }

    /// Hear `sample`; the beat due next, when it is time to tell it.
    pub fn hear(&mut self, sample: f32) -> Option<Beat> {
        let at = (self.at as usize * HOP + self.fresh) % WINDOW;
        self.samples[at] = sample;
        self.fresh += 1;
        if self.fresh < HOP {
            return None;
        }
        self.fresh = 0;
        self.hop()
    }

    fn hop(&mut self) -> Option<Beat> {
        let rise = self.spectrum();
        let place = self.at as usize % self.rises.len();
        self.rises[place] = rise;
        self.at += 1;
        if self.at.is_multiple_of(READ_EVERY) && self.at as f64 * self.hop_seconds >= SETTLE_SECONDS {
            self.read();
        }
        if self.period <= 0.0 || self.clarity < CLEAR || self.loud < SILENCE {
            return None;
        }
        if (self.at as f64) + self.lead_hops < self.next {
            return None;
        }
        // Behind, after a silence or a seek: on to the next one to come.
        let late = ((self.at as f64 + self.lead_hops - self.next) / self.period).floor();
        self.next += (late + 1.0) * self.period;
        Some(Beat { strength: self.strength(), bpm: self.bpm() as f32 })
    }

    /// How strongly the music's spectrum rose since two hops ago.
    fn spectrum(&mut self) -> f32 {
        let start = (self.at as usize * HOP + HOP) % WINDOW;
        let mut energy = 0.0f32;
        for i in 0..WINDOW {
            let sample = self.samples[(start + i) % WINDOW];
            energy += sample * sample;
            let at = self.reversed[i] as usize;
            self.real[at] = sample * self.hann[i];
            self.imaginary[at] = 0.0;
        }
        let loud = (energy / WINDOW as f32).sqrt();
        self.loud = loud;
        // A few seconds to forget the loudest part.
        self.loudest = (self.loudest * 0.9995).max(loud);
        self.transform();
        let [older, newer] = &mut self.spectra;
        let mut rise = 0.0f32;
        for bin in 1..self.bins {
            let power = self.real[bin] * self.real[bin] + self.imaginary[bin] * self.imaginary[bin];
            let level = (1.0 + 40.0 * power.sqrt()).ln();
            let up = level - older[bin];
            if up > 0.0 {
                // The drums' low end counts the most.
                rise += if bin < 8 { up * 3.0 } else { up };
            }
            older[bin] = newer[bin];
            newer[bin] = level;
        }
        rise / self.bins as f32
    }

    /// The Fourier transform of `real` and `imaginary`, in place; they hold
    /// the samples in bit-reversed order.
    fn transform(&mut self) {
        let mut size = 2;
        while size <= WINDOW {
            let (half, step) = (size / 2, WINDOW / size);
            for start in (0..WINDOW).step_by(size) {
                for i in 0..half {
                    let (c, s) = (self.cos[i * step], -self.sin[i * step]);
                    let (a, b) = (start + i, start + i + half);
                    let (re, im) = (self.real[b] * c - self.imaginary[b] * s, self.real[b] * s + self.imaginary[b] * c);
                    self.real[b] = self.real[a] - re;
                    self.imaginary[b] = self.imaginary[a] - im;
                    self.real[a] += re;
                    self.imaginary[a] += im;
                }
            }
            size *= 2;
        }
    }

    /// How strongly the spectrum rose `back` hops ago.
    fn rise(&self, back: usize) -> f32 {
        let len = self.rises.len();
        self.rises[(self.at as usize + len * 2 - 1 - back) % len]
    }

    /// The rises heard, the oldest first, around their mean.
    fn heard(&self) -> Vec<f32> {
        let len = self.rises.len().min(self.at as usize);
        let mut heard: Vec<f32> = (0..len).rev().map(|back| self.rise(back)).collect();
        let mean = heard.iter().sum::<f32>() / len.max(1) as f32;
        for rise in &mut heard {
            *rise = (*rise - mean).max(0.0);
        }
        heard
    }

    /// Read the tempo and the phase again from what was heard.
    fn read(&mut self) {
        let heard = self.heard();
        let Some((period, clarity)) = self.tempo(&heard) else { return };
        self.clarity = self.clarity * 0.7 + clarity * 0.3;
        if self.period <= 0.0 {
            self.period = period;
        } else if (period / self.period - 1.0).abs() < 0.04 {
            self.period += (period - self.period) * 0.2;
            self.other_for = 0;
        } else if self.other > 0.0 && (period / self.other - 1.0).abs() < 0.04 {
            // Changed for good, not for a moment.
            self.other_for += 1;
            if self.other_for >= 12 {
                self.period = period;
                self.other_for = 0;
            }
        } else {
            self.other = period;
            self.other_for = 0;
        }
        self.align(&heard);
    }

    /// The period that repeats best, in hops, and how clearly.
    fn tempo(&self, heard: &[f32]) -> Option<(f64, f32)> {
        let shortest = (60.0 / FASTEST_BPM / self.hop_seconds) as usize;
        let longest = (60.0 / SLOWEST_BPM / self.hop_seconds) as usize + 1;
        if heard.len() < longest * 3 {
            return None;
        }
        let silence: f32 = heard.iter().map(|rise| rise * rise).sum();
        if silence <= 0.0 {
            return None;
        }
        let repeats = |lag: usize| -> f32 {
            let pairs = heard.len().saturating_sub(lag).max(1);
            heard[lag.min(heard.len())..].iter().zip(heard).map(|(a, b)| a * b).sum::<f32>() / pairs as f32
        };
        let energy = silence / heard.len() as f32;
        let (mut best, mut best_score, mut best_repeat) = (0usize, 0.0f32, 0.0f32);
        for lag in shortest..=longest {
            let bpm = 60.0 / (lag as f64 * self.hop_seconds);
            let off = (bpm / LIKELIEST_BPM).log2();
            let prior = (-0.5 * (off / 0.8) * (off / 0.8)).exp() as f32;
            // A true period repeats at twice its length too, and its eighth
            // notes at half of it; a period one and a half beats long, on
            // the same grid, does not.
            let repeat = repeats(lag);
            let score = prior * (repeat + 0.5 * repeats(lag * 2) + 0.4 * repeats(lag / 2));
            if score > best_score {
                (best, best_score, best_repeat) = (lag, score, repeat);
            }
        }
        if best == 0 {
            return None;
        }
        // Finer than a hop: where it repeats as many periods on as the
        // memory holds, over that many.
        let periods = (heard.len() * 2 / 3 / best).max(1);
        let far = best * periods;
        let peak = (far.saturating_sub(periods)..=far + periods).filter(|&lag| lag < heard.len()).max_by(|&a, &b| repeats(a).total_cmp(&repeats(b)));
        let period = peak.map_or(best as f64, |lag| lag as f64 / periods as f64);
        Some((period, best_repeat / energy))
    }

    /// Pull the next beat towards where the peaks line up.
    fn align(&mut self, heard: &[f32]) {
        let period = self.period;
        if period <= 0.0 {
            return;
        }
        let beats = ((heard.len() as f64 / period) as usize).min(10);
        let line_up = |back: f64| -> f32 {
            (0..beats)
                .map(|beat| {
                    let at = heard.len() as f64 - 1.0 - back - beat as f64 * period;
                    if at < 1.0 { return 0.0 }
                    let at = at as usize;
                    // The latest beats count the most.
                    (heard[at] + 0.5 * (heard[at - 1] + heard[(at + 1).min(heard.len() - 1)])) * 0.92f32.powi(beat as i32)
                })
                .sum()
        };
        let offsets = period.ceil() as usize;
        let Some(best) = (0..offsets).max_by(|&a, &b| line_up(a as f64).total_cmp(&line_up(b as f64))) else { return };
        // A hop's spectrum shows what began half a window before it.
        let anchor = self.at as f64 - 1.0 - best as f64 - 1.0;
        if self.next <= 0.0 {
            self.next = anchor + period * ((self.at as f64 - anchor) / period).ceil();
            return;
        }
        let mut off = (anchor - self.next) % period;
        if off > period / 2.0 {
            off -= period;
        } else if off < -period / 2.0 {
            off += period;
        }
        if off.abs() < period * 0.12 {
            self.next += off * 0.25;
            self.moved_for = 0;
            return;
        }
        // Far from where the beat was: it moves only if that lasts, or a
        // fill would throw it onto the off-beat.
        self.moved_for = if (off - self.moved).abs() < period * 0.12 { self.moved_for + 1 } else { 0 };
        self.moved = off;
        if self.moved_for >= 16 {
            self.next += off;
            self.moved_for = 0;
        }
    }

    /// How hard the latest beat hit against how the music hits lately, and
    /// how loud it is against the loudest it has been.
    fn strength(&self) -> f32 {
        let back = (self.period as usize).max(1);
        let hit = (0..back / 4 + 1).map(|at| self.rise(at)).fold(0.0f32, f32::max);
        let len = self.rises.len().min(self.at as usize).max(1);
        let usual = (0..len).map(|at| self.rise(at)).sum::<f32>() / len as f32;
        let hard = (hit / (usual * 3.0).max(1e-6)).min(1.0);
        let loud = if self.loudest > 0.0 { (self.loud / self.loudest).min(1.0) } else { 0.0 };
        (hard * 0.6 + loud * 0.4) * self.clarity.min(0.5) * 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 44100;

    /// Kicks at `bpm`, a bass note under them all the time and some noise.
    fn song(bpm: f64, seconds: f64) -> Vec<f32> {
        let mut noise = 12345u32;
        (0..(seconds * f64::from(RATE)) as usize)
            .map(|i| {
                let t = i as f64 / f64::from(RATE);
                let since = t % (60.0 / bpm);
                let kick = (std::f64::consts::TAU * 70.0 * since).sin() * (-since * 30.0).exp();
                let bass = (std::f64::consts::TAU * 55.0 * t).sin() * 0.2;
                noise = noise.wrapping_mul(1664525).wrapping_add(1013904223);
                let hiss = (f64::from(noise >> 8) / f64::from(1 << 24) - 0.5) * 0.05;
                (kick * 0.7 + bass + hiss) as f32
            })
            .collect()
    }

    /// When each beat is told, in seconds.
    fn told(beats: &mut Beats, samples: &[f32]) -> Vec<(f64, Beat)> {
        samples.iter().enumerate().filter_map(|(i, &s)| beats.hear(s).map(|beat| (i as f64 / f64::from(RATE), beat))).collect()
    }

    #[test]
    fn it_finds_the_tempo_of_a_steady_beat() {
        for bpm in [80.0, 100.0, 128.0, 140.0] {
            let mut beats = Beats::new(RATE, 0.0);
            let heard = told(&mut beats, &song(bpm, 20.0));
            // Kicks alone say nothing of the half time: either is the beat.
            let heard_bpm = beats.bpm();
            let fits = [bpm, bpm / 2.0].iter().any(|one| (heard_bpm / one - 1.0).abs() < 0.03);
            assert!(fits, "{bpm} heard as {heard_bpm}");
            assert!(heard.len() as f64 > 20.0 * heard_bpm / 60.0 * 0.7, "{bpm}: only {} beats", heard.len());
        }
    }

    #[test]
    fn a_fast_beat_is_followed_at_half_time() {
        let mut beats = Beats::new(RATE, 0.0);
        told(&mut beats, &song(190.0, 20.0));
        assert!((beats.bpm() / 95.0 - 1.0).abs() < 0.03, "heard as {}", beats.bpm());
    }

    #[test]
    fn beats_fall_on_the_kicks_and_lead_comes_early() {
        let bpm = 120.0;
        for lead in [0.0, 0.1] {
            let mut beats = Beats::new(RATE, lead);
            let heard = told(&mut beats, &song(bpm, 24.0));
            let late: Vec<f64> = heard.iter().filter(|(at, _)| *at > 12.0).map(|(at, _)| {
                // Where the beat it tells lands, against the nearest kick.
                let due = at + lead;
                let off = due % 0.5;
                if off > 0.25 { off - 0.5 } else { off }
            }).collect();
            let mean = late.iter().map(|off| off.abs()).sum::<f64>() / late.len() as f64;
            assert!(mean < 0.04, "lead {lead}: {mean} seconds from the kicks");
        }
    }

    #[test]
    fn it_keeps_the_beat_through_a_bar_without_drums() {
        let bpm = 120.0;
        let mut samples = song(bpm, 24.0);
        // Two seconds of the bass note alone.
        for i in (12 * RATE as usize)..(14 * RATE as usize) {
            let t = i as f64 / f64::from(RATE);
            samples[i] = ((std::f64::consts::TAU * 55.0 * t).sin() * 0.2) as f32;
        }
        let mut beats = Beats::new(RATE, 0.0);
        let heard = told(&mut beats, &samples);
        assert!(heard.iter().filter(|(at, _)| (12.0..14.0).contains(at)).count() >= 3, "the beat stopped");
    }

    #[test]
    fn silence_has_no_beat() {
        let mut beats = Beats::new(RATE, 0.0);
        assert!(told(&mut beats, &vec![0.0; RATE as usize * 10]).is_empty());
    }

    #[test]
    fn noise_has_no_steady_beat_to_follow() {
        let mut beats = Beats::new(RATE, 0.0);
        let mut noise = 99u32;
        let samples: Vec<f32> = (0..RATE as usize * 12)
            .map(|_| {
                noise = noise.wrapping_mul(1664525).wrapping_add(1013904223);
                (f64::from(noise >> 8) / f64::from(1 << 24) - 0.5) as f32 * 0.4
            })
            .collect();
        let heard = told(&mut beats, &samples);
        assert!(heard.len() < 10, "{} beats in noise", heard.len());
    }
}

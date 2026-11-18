//! Whole-track loudness facts, not a playback meter or an automatic gain controller.
//! Look up one decision before play. Only run `measure` / `content_key` on the
//! caller's bounded background worker, on a successfully finished, non-preview source.

use super::{Stream, TrackDecoder};
use crate::store;
use ebur128::{Channel, EbuR128, Mode};
use serde_json::{Value, json};
use sha1::{Digest, Sha1};
use std::{
    collections::VecDeque,
    fs,
    io::{self, Cursor, Read, Write},
    path::Path,
    sync::{Arc, atomic::{AtomicBool, Ordering}},
};
use symphonia::core::{audio::{Channels, SampleBuffer, SignalSpec}, errors::Error};

pub const TARGET_LUFS: f32 = -18.0;
const CEILING_DB: f32 = -1.0;
const MAX_ENTRIES: usize = 2048;
const MAX_FILE_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Level {
    /// Track gain at the conventional ReplayGain reference (89 dB); not a LUFS reading.
    ReplayGain(f32),
    Integrated(f32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Info {
    level: Level,
    sample_peak_dbfs: Option<f32>,
    true_peak_dbtp: Option<f32>,
}

impl Info {
    pub fn source(&self) -> &'static str {
        match self.level { Level::ReplayGain(_) => "replaygain", Level::Integrated(_) => "measured" }
    }

    pub fn integrated_lufs(&self) -> Option<f32> {
        match self.level { Level::Integrated(lufs) => Some(lufs), Level::ReplayGain(_) => None }
    }

    pub fn sample_peak_dbfs(&self) -> Option<f32> { self.sample_peak_dbfs }
    pub fn true_peak_dbtp(&self) -> Option<f32> { self.true_peak_dbtp }

    /// Apply the same safety ceiling to built-in and plugin gain decisions. A
    /// sample peak is a lower bound on the true peak, never permission to boost.
    pub fn limit_gain_db(&self, requested: f32) -> f32 {
        let requested = if requested.is_finite() { requested.clamp(-120.0, 60.0) } else { 0.0 };
        let ceiling = self.true_peak_dbtp.map_or(0.0, |peak| CEILING_DB - peak);
        let ceiling = self.sample_peak_dbfs.map_or(ceiling, |peak| ceiling.min(CEILING_DB - peak));
        requested.min(ceiling)
    }

    pub fn gain_db(&self) -> f32 {
        self.limit_gain_db(match self.level {
            Level::Integrated(lufs) => TARGET_LUFS - lufs,
            Level::ReplayGain(db) => db,
        })
    }

    /// REPLAYGAIN_TRACK_PEAK has no reliable indication of oversampling.
    /// Treat it as a linear sample peak, never as measured dBTP. Ignore album tags.
    pub(crate) fn replaygain(gain: &str, linear_peak: Option<&str>) -> Option<Self> {
        let gain = gain.trim();
        if !gain.is_ascii() || gain.len() > 48 { return None; }
        let number = if gain.to_ascii_lowercase().ends_with("db") { gain[..gain.len() - 2].trim_end() } else { gain };
        let db: f32 = number.parse().ok()?;
        if !(-60.0..=60.0).contains(&db) { return None; }
        let peak = linear_peak.and_then(|s| s.trim().parse::<f64>().ok()).and_then(peak_db);
        Some(Self { level: Level::ReplayGain(db), sample_peak_dbfs: peak, true_peak_dbtp: None })
    }

    pub(crate) fn to_json(self) -> Value {
        json!({"source": self.source(), "level": match self.level { Level::ReplayGain(x) | Level::Integrated(x) => x },
               "sample_peak_dbfs": self.sample_peak_dbfs, "true_peak_dbtp": self.true_peak_dbtp})
    }

    pub(crate) fn from_json(v: &Value) -> Option<Self> {
        let number = v["level"].as_f64()? as f32;
        let peak = |v: &Value| -> Option<Option<f32>> {
            if v.is_null() { return Some(None); }
            let db = v.as_f64()? as f32;
            (-200.0..=60.0).contains(&db).then_some(Some(db))
        };
        let sample_peak_dbfs = peak(&v["sample_peak_dbfs"])?;
        let true_peak_dbtp = peak(&v["true_peak_dbtp"])?;
        let level = match v["source"].as_str()? {
            "replaygain" if (-60.0..=60.0).contains(&number) && true_peak_dbtp.is_none() => Level::ReplayGain(number),
            "measured" if (-71.0..=60.0).contains(&number) && sample_peak_dbfs.is_some() => Level::Integrated(number),
            _ => return None,
        };
        Some(Self { level, sample_peak_dbfs, true_peak_dbtp })
    }
}

fn peak_db(amplitude: f64) -> Option<f32> {
    let db = (20.0 * amplitude.log10()) as f32;
    (-200.0..=60.0).contains(&db).then_some(db)
}

/// No paths, signed CDN URLs, song names or login information in source keys.
/// Even a caller-supplied key is digested once more before going on disk.
fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(2 * bytes.len());
    for &b in bytes {
        text.push(char::from(DIGITS[(b >> 4) as usize]));
        text.push(char::from(DIGITS[(b & 15) as usize]));
    }
    text
}

pub(crate) fn hex_digest(bytes: &[u8]) -> String { hex(&Sha1::digest(bytes)) }

pub fn online_key(id: i64, data: &Value) -> Option<String> {
    if id <= 0 || !data["freeTrialInfo"].is_null() || data["id"].as_i64().is_some_and(|actual| actual != id) {
        return None;
    }
    // Matching size, bitrate and codec identify an encoding, not its master.
    // Without a content identifier the producer must use a strong HTTP validator
    // scoped to its resource, or a digest after it has received the complete bytes.
    let md5 = data["md5"].as_str()?.trim();
    if md5.len() != 32 || !md5.bytes().all(|c| c.is_ascii_hexdigit()) || md5.bytes().all(|c| c == b'0') {
        return None;
    }
    let variant = json!([id, "md5", md5.to_ascii_lowercase()]);
    Some(format!("netease-v1:{}", hex_digest(variant.to_string().as_bytes())))
}

/// Last resort for a complete source with no trustworthy version identifier.
/// Never hash a growing stream or hold its mutex for the duration of the hash.
pub fn content_key(stream: &Stream, cancel: &AtomicBool) -> Option<String> {
    let bytes = stream.complete_bytes()?;
    let mut hash = Sha1::new();
    for chunk in bytes.chunks(256 * 1024) {
        if cancel.load(Ordering::Relaxed) { return None; }
        hash.update(chunk);
    }
    (!cancel.load(Ordering::Relaxed)).then(|| format!("bytes-v1:{}", hex(&hash.finalize())))
}

/// A fixed number of records, oldest update first, including after a restart.
/// The caller owns one writer; playback lookups never perform filesystem I/O.
#[derive(Clone, Default)]
pub struct Cache {
    entries: VecDeque<(String, Info)>,
}

impl Cache {
    pub fn get(&self, key: &str) -> Option<Info> {
        let digest = hex_digest(key.as_bytes());
        self.entries.iter().rev().find(|(k, _)| *k == digest).map(|(_, info)| *info)
    }

    pub fn insert(&mut self, key: String, info: Info) {
        if !key.is_empty() { self.insert_digest(hex_digest(key.as_bytes()), info); }
    }

    fn insert_digest(&mut self, digest: String, info: Info) {
        if let Some(index) = self.entries.iter().position(|(k, _)| k == &digest) { self.entries.remove(index); }
        if self.entries.len() == MAX_ENTRIES { self.entries.pop_front(); }
        self.entries.push_back((digest, info));
    }

    pub fn load() -> Self { Self::read(&store::dir().join("loudness.json")).unwrap_or_default() }

    fn read(path: &Path) -> Option<Self> {
        let mut bytes = Vec::new();
        fs::File::open(path).ok()?.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes).ok()?;
        if bytes.len() as u64 > MAX_FILE_BYTES { return None; }
        let root: Value = serde_json::from_slice(&bytes).ok()?;
        if root["version"].as_u64() != Some(1) { return None; }
        let mut cache = Self::default();
        let rows = root["entries"].as_array()?;
        for row in &rows[rows.len().saturating_sub(MAX_ENTRIES)..] {
            if let Some(key) = row.get(0).and_then(Value::as_str)
                && key.len() == 40 && key.bytes().all(|c| c.is_ascii_hexdigit())
                && let Some(info) = row.get(1).and_then(Info::from_json)
            {
                cache.insert_digest(key.to_ascii_lowercase(), info);
            }
        }
        Some(cache)
    }

    pub fn save(&self) -> io::Result<()> {
        self.write(&store::dir().join("loudness.json"))
    }

    fn write(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() { fs::create_dir_all(dir)?; }
        // One bounded side file even across crashes. Lock writers, not readers;
        // a second process must not truncate the temporary file under a rename.
        let lock = fs::OpenOptions::new().create(true).write(true).truncate(false).open(path.with_extension("lock"))?;
        lock.lock()?;
        let temp = path.with_extension("partial");
        let result = (|| {
            let rows: Vec<_> = self.entries.iter().map(|(key, info)| json!([key, info.to_json()])).collect();
            let mut file = fs::File::create(&temp)?;
            file.write_all(&serde_json::to_vec(&json!({"version": 1, "entries": rows}))?)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temp, path)
        })();
        if result.is_err() { let _ = fs::remove_file(temp); }
        result
    }
}

/// `Cursor<Bytes>` is seekable, knows its exact length, and never competes
/// with playback for the growing buffer or its cursor.
struct Bytes(Arc<Vec<u8>>);
impl AsRef<[u8]> for Bytes {
    fn as_ref(&self) -> &[u8] { self.0.as_slice() }
}

/// Skip only complete leading ID3v2 tags; don't scan a picture for audio sync.
fn after_id3(mut bytes: &[u8]) -> Option<&[u8]> {
    while bytes.starts_with(b"ID3") {
        let header = bytes.get(..10)?;
        if !matches!(header[3], 2..=4) || header[6..].iter().any(|b| b & 0x80 != 0) { return None; }
        let size = header[6..].iter().fold(0usize, |n, &b| (n << 7) | usize::from(b));
        let footer = if header[3] == 4 && header[5] & 0x10 != 0 { 10 } else { 0 };
        bytes = bytes.get(10 + size + footer..)?;
    }
    Some(bytes)
}

/// A raw stream has no container end marker. Its demuxer both scans past bad
/// sync and represents a torn final frame with the same UnexpectedEof as a
/// clean end. Account for the bytes of *every* packet it actually delivered.
struct RawFrames<'a> {
    remaining: &'a [u8],
    adts: bool,
    first: bool,
    counted: bool,
}

impl RawFrames<'_> {
    fn accept(&mut self, packet: &[u8]) -> Option<()> {
        if self.adts {
            let h = self.remaining.get(..7)?;
            // Symphonia 0.5's ADTS demuxer recognizes 0xfff1 and removes seven bytes.
            let len = usize::from(h[3] & 3) << 11 | usize::from(h[4]) << 3 | usize::from(h[5] >> 5);
            if h[..2] != [0xff, 0xf1] || len != 7 + packet.len() { return None; }
            self.remaining = &self.remaining[7..];
        } else if self.first && !self.remaining.starts_with(packet) {
            // MP3 omits a leading Xing/Info/VBRI metadata frame from its output.
            let prefix = &self.remaining[..self.remaining.len().min(4096 + packet.len())];
            let at = prefix.windows(packet.len()).position(|w| w == packet)?;
            let header = &prefix[..at];
            if let Some(tag) = header.windows(4).position(|w| w == b"Xing" || w == b"Info") {
                let flags = u32::from_be_bytes(header.get(tag + 4..tag + 8)?.try_into().ok()?);
                self.counted = flags & 1 != 0;
            } else if header.windows(4).any(|w| w == b"VBRI") {
                self.counted = true;
            } else { return None; }
            self.remaining = &self.remaining[at..];
        }
        self.first = false;
        self.remaining = self.remaining.strip_prefix(packet)?;
        Some(())
    }

    fn complete(&self) -> bool {
        let mut tail = self.remaining;
        if tail.len() >= 128 && tail[tail.len() - 128..].starts_with(b"TAG") { tail = &tail[..tail.len() - 128]; }
        if tail.is_empty() { return true; }
        let Some(footer) = tail.len().checked_sub(32).map(|n| &tail[n..]) else { return false };
        if !footer.starts_with(b"APETAGEX") { return false; }
        let size = u32::from_le_bytes(footer[12..16].try_into().unwrap()) as usize;
        size >= 32 && (size == tail.len() || (tail.starts_with(b"APETAGEX") && size == tail.len() - 32))
    }
}

/// The RIFF reader synthesizes a speaker mask from just a channel count when
/// WAVEFORMATEXTENSIBLE is missing, and repairs incorrect explicit masks. Do
/// not use either guess when deciding which channel is LFE or surround.
fn known_layout(payload: &[u8], channels: Channels) -> bool {
    if channels.count() <= 2 { return true; }
    if payload.starts_with(b"FORM") { return false; } // AIFF supplies only a channel count.
    if !payload.starts_with(b"RIFF") { return true; }
    let Some(mut chunks) = payload.get(12..) else { return false };
    while let Some(header) = chunks.get(..8) {
        let len = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
        chunks = &chunks[8..];
        let Some(data) = chunks.get(..len) else { return false };
        if &header[..4] == b"fmt " {
            return data.len() >= 40 && data.starts_with(&0xfffeu16.to_le_bytes())
                && u32::from_le_bytes(data[20..24].try_into().unwrap()) == channels.bits();
        }
        let Some(rest) = chunks.get(len.saturating_add(len % 2)..) else { return false };
        chunks = rest;
    }
    false
}

fn channel_map(mask: Channels) -> Option<Vec<Channel>> {
    // Symphonia interleaves in mask order. Never use EbuR128's default
    // six-channel ordering for an unknown layout or ignore surround channels.
    mask.iter().map(|c| Some(match c {
        Channels::FRONT_LEFT => Channel::Left,
        Channels::FRONT_RIGHT => Channel::Right,
        Channels::FRONT_CENTRE => Channel::Center,
        Channels::LFE1 | Channels::LFE2 => Channel::Unused,
        Channels::REAR_LEFT if mask.contains(Channels::SIDE_LEFT) => Channel::Mp135,
        Channels::REAR_RIGHT if mask.contains(Channels::SIDE_RIGHT) => Channel::Mm135,
        Channels::REAR_LEFT => Channel::LeftSurround,
        Channels::REAR_RIGHT => Channel::RightSurround,
        Channels::SIDE_LEFT => Channel::Mp090,
        Channels::SIDE_RIGHT => Channel::Mm090,
        Channels::REAR_CENTRE => Channel::Mp180,
        _ => return None,
    })).collect()
}

/// Synchronous CPU work: no network, no wait for a producer, no `Decoded`
/// playback meter/beat writes. The caller admits at most one worker. A
/// successful receipt (even without Content-Length) is necessary, not sufficient:
/// decoding must also finish, preserve layout and account for the full track.
pub fn measure(stream: &Stream, expected_seconds: f64, cancel: &AtomicBool) -> Option<Info> {
    if cancel.load(Ordering::Relaxed) { return None; }
    let bytes = stream.complete_bytes()?;
    let payload = after_id3(&bytes)?;
    let source = Cursor::new(Bytes(Arc::clone(&bytes)));
    let mut input = TrackDecoder::open(Box::new(source), true).ok()?;
    let sized_form = payload.starts_with(b"RIFF") || payload.starts_with(b"FORM");
    if sized_form {
        let len = payload.get(4..8)?.try_into().ok()?;
        let size = if payload.starts_with(b"RIFF") { u32::from_le_bytes(len) } else { u32::from_be_bytes(len) };
        if u64::from(size) + 8 > payload.len() as u64 { return None; }
    }
    let mp4 = payload.get(4..8).is_some_and(|w| [b"ftyp", b"moov", b"mdat", b"free", b"skip", b"wide", b"moof", b"styp"].iter().any(|tag| w == tag.as_slice()));
    let mut raw = if sized_form || mp4 || payload.starts_with(b"OggS") || payload.starts_with(b"fLaC") {
        None
    } else {
        use symphonia::core::codecs::{CODEC_TYPE_AAC, CODEC_TYPE_MP1, CODEC_TYPE_MP2, CODEC_TYPE_MP3};
        let adts = input.params.codec == CODEC_TYPE_AAC;
        if !adts && !matches!(input.params.codec, CODEC_TYPE_MP1 | CODEC_TYPE_MP2 | CODEC_TYPE_MP3) { return None; }
        Some(RawFrames { remaining: payload, adts, first: true, counted: false })
    };
    let mut analysis: Option<(SignalSpec, EbuR128, SampleBuffer<f32>)> = None;
    let mut frames = 0u64;
    let mut packet_end = None;
    loop {
        if cancel.load(Ordering::Relaxed) { return None; }
        let packet = match input.format.next_packet() {
            Ok(packet) => packet,
            Err(Error::IoError(err)) if err.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(_) => return None,
        };
        if packet.track_id() != input.track { continue; }
        if packet.buf().is_empty() || packet_end.is_some_and(|at| packet.ts() > at) { return None; }
        packet_end = Some(packet.ts().checked_add(packet.dur())?);
        if let Some(extents) = &mut raw { extents.accept(packet.buf())?; }
        // Unlike playback, never conceal a corrupt packet or continue through a reset.
        let decoded = input.codec.decode(&packet).ok()?;
        let spec = *decoded.spec();
        let n = spec.channels.count();
        let count = decoded.frames();
        if !(1..=8).contains(&n) || !(8_000..=384_000).contains(&spec.rate) || count.checked_mul(n)? > 2 * 1024 * 1024 {
            return None;
        }
        if count == 0 { continue; }
        if analysis.is_none() {
            if !known_layout(payload, spec.channels) { return None; }
            let mut meter = EbuR128::new(n as u32, spec.rate, Mode::I | Mode::TRUE_PEAK | Mode::HISTOGRAM).ok()?;
            meter.set_channel_map(&channel_map(spec.channels)?).ok()?;
            analysis = Some((spec, meter, SampleBuffer::new(count as u64, spec)));
        }
        let (first, meter, pcm) = analysis.as_mut()?;
        if spec != *first { return None; }
        if pcm.capacity() < count * n { *pcm = SampleBuffer::new(count as u64, spec); }
        pcm.copy_interleaved_ref(decoded);
        let samples = pcm.samples();
        if samples.iter().any(|s| !s.is_finite()) { return None; }
        meter.add_frames_f32(samples).ok()?;
        frames = frames.checked_add(count as u64)?;
    }
    let (spec, mut meter, _) = analysis?;
    let verified = input.codec.finalize().verify_ok;
    if verified == Some(false) || cancel.load(Ordering::Relaxed) { return None; }
    // n_frames is in the container's time base; MP4 need not use a PCM
    // frame as a timestamp unit. Do not accept a short final packet as EOF.
    if raw.as_ref().is_some_and(|r| !r.complete()) { return None; }
    // For ADTS or MP3 without a frame-count tag Symphonia *estimates* the
    // duration from bitrate and the file size, including a possible fragment.
    // It is not independent evidence. Ogg only publishes n_frames after EOS.
    let counted = raw.as_ref().is_none_or(|r| r.counted);
    let declared = input.params.n_frames.filter(|&v| counted && v > 0);
    if let Some(ticks) = declared {
        if packet_end? < input.params.start_ts.checked_add(ticks)? { return None; }
        let pcm = input.params.time_base.map_or(ticks as f64, |b| {
            ticks as f64 * f64::from(b.numer) * f64::from(spec.rate) / f64::from(b.denom)
        });
        if frames as f64 + 0.5 < pcm { return None; }
    } else if verified != Some(true) && raw.is_none() {
        return None; // A missing container end/length cannot be proven by rounded catalogue time.
    }
    let seconds = frames as f64 / f64::from(spec.rate);
    if expected_seconds.is_finite() && expected_seconds > 0.0 {
        // Catalogue times may be rounded or include a codec's priming/padding.
        // A different edit, an undeclared trial or a partial download is not this track.
        if (seconds - expected_seconds).abs() > 0.5 { return None; }
    } else if declared.is_none() && verified != Some(true) {
        // A raw fragment ending on a packet boundary looks just like clean EOF.
        return None;
    }
    let lufs = meter.loudness_global().ok()? as f32;
    if !(-71.0..=60.0).contains(&lufs) { return None; } // includes silence with no gated blocks
    let channels = spec.channels.count();
    // Flush the FIR tail *after* taking integrated loudness. The 2x/4x
    // interpolators retain at most 24 input frames. Peaks include the LFE.
    let has_true_peak = spec.rate < 192_000;
    if has_true_peak { meter.add_frames_f32(&[0.0; 32 * 8][..32 * channels]).ok()?; }
    let (mut sample, mut tp) = (0.0f64, 0.0f64);
    for ch in 0..channels as u32 {
        sample = sample.max(meter.sample_peak(ch).ok()?);
        if has_true_peak { tp = tp.max(meter.true_peak(ch).ok()?); }
    }
    // At >=192 kHz this library returns sample_peak unchanged as true_peak.
    // Keep those sources sample-peak-only, rather than publishing fictitious dBTP.
    let sample_peak_dbfs = Some(peak_db(sample)?);
    let true_peak_dbtp = if has_true_peak { Some(peak_db(tp)?) } else { None };
    (!cancel.load(Ordering::Relaxed)).then_some(Info { level: Level::Integrated(lufs), sample_peak_dbfs, true_peak_dbtp })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(rate: u32, frames: u32, mut sample: impl FnMut(u32) -> f64) -> Vec<u8> {
        // Two identical PCM16 channels. The quantization is part of the fixture.
        let mut bytes = Vec::with_capacity(44 + frames as usize * 4);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + frames * 4).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&rate.to_le_bytes());
        bytes.extend_from_slice(&(rate * 4).to_le_bytes());
        bytes.extend_from_slice(&4u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&(frames * 4).to_le_bytes());
        for frame in 0..frames {
            let pcm = (sample(frame) * 32_768.0).round() as i16;
            bytes.extend_from_slice(&pcm.to_le_bytes());
            bytes.extend_from_slice(&pcm.to_le_bytes());
        }
        bytes
    }

    fn received(bytes: &[u8]) -> Stream {
        let stream = Stream::new(None);
        stream.push(bytes);
        stream.finish();
        stream
    }

    #[test]
    fn integrated_and_intersample_peaks_are_measured_from_the_whole_track() {
        let cancel = AtomicBool::new(false);
        let sine = |n: u32| (f64::from(n) * std::f64::consts::TAU * 1_000.0 / 48_000.0).sin() * 10.0f64.powf(-23.0 / 20.0);
        let full = received(&wav(48_000, 96_000, sine));
        let level = measure(&full, 2.0, &cancel).unwrap();
        assert_eq!(level.source(), "measured");
        assert!((level.integrated_lufs().unwrap() + 23.0).abs() < 0.2);
        assert!((level.sample_peak_dbfs().unwrap() + 23.0).abs() < 0.02);
        assert!((level.gain_db() - 5.0).abs() < 0.2);
        // Three seconds of silence at the end must not dilute the gated result
        // like whole-file RMS would (mixed transition blocks can shift it slightly).
        let gated = measure(&received(&wav(48_000, 240_000, |n| if n < 96_000 { sine(n) } else { 0.0 })), 5.0, &cancel).unwrap();
        assert!((gated.integrated_lufs().unwrap() - level.integrated_lufs().unwrap()).abs() < 0.6);
        let quarter_rate = |n: u32| 0.8 * if n % 4 < 2 { std::f64::consts::FRAC_1_SQRT_2 } else { -std::f64::consts::FRAC_1_SQRT_2 };
        let inter = measure(&received(&wav(48_000, 48_000, quarter_rate)), 1.0, &cancel).unwrap();
        let tp = inter.true_peak_dbtp().unwrap();
        assert!(tp > inter.sample_peak_dbfs().unwrap() + 2.0, "sample {:?}, true {tp}", inter.sample_peak_dbfs());
        assert!(inter.limit_gain_db(40.0) + tp <= -0.999);
        let high = measure(&received(&wav(192_000, 192_000, quarter_rate)), 1.0, &cancel).unwrap();
        assert!(high.sample_peak_dbfs().is_some() && high.true_peak_dbtp().is_none());
        assert_eq!(high.limit_gain_db(10.0), 0.0);
    }

    #[test]
    fn replaygain_and_plugin_requests_never_promote_sample_peaks_to_dbtp() {
        for peak in [None, Some("0.5"), Some("NaN")] {
            let info = Info::replaygain(" +8.0 dB ", peak).unwrap();
            assert_eq!(info.source(), "replaygain");
            assert_eq!(info.integrated_lufs(), None);
            assert_eq!(info.true_peak_dbtp(), None);
            assert_eq!(info.gain_db(), 0.0);
            assert_eq!(info.limit_gain_db(35.0), 0.0);
            assert_eq!(info.limit_gain_db(-3.0), -3.0);
        }
        assert_eq!(Info::replaygain("7", Some("1")).unwrap().gain_db(), -1.0);
        assert!(Info::replaygain("NaN", None).is_none());
        assert!(Info::from_json(&json!({"source":"replaygain","level":-6.0,"true_peak_dbtp":-9.0})).is_none());
        let limited = Info { level: Level::Integrated(-32.0), sample_peak_dbfs: Some(-3.0), true_peak_dbtp: Some(-1.5) };
        assert_eq!(limited.gain_db(), 0.5);
        assert_eq!(limited.limit_gain_db(55.0), 0.5);
        assert_eq!(limited.limit_gain_db(f32::INFINITY), 0.0);
    }

    #[test]
    fn a_successful_transport_eof_is_not_proof_of_a_complete_audio_file() {
        let cancel = AtomicBool::new(false);
        let mut wav = wav(48_000, 48_000, |n| (f64::from(n) * 0.2).sin() / 2.0);
        let full = received(&wav);
        assert!(measure(&full, 30.0, &cancel).is_none(), "a short edit/trial is not the full song");
        assert!(measure(&full, 1.0, &AtomicBool::new(true)).is_none());
        wav.truncate(wav.len() - 1);
        assert!(measure(&received(&wav), 1.0, &cancel).is_none(), "one missing byte, even with no HTTP length");
        // Outer RIFF size is valid, but the inner data chunk still advertises a full frame.
        let size = (wav.len() as u32 - 8).to_le_bytes();
        wav[4..8].copy_from_slice(&size);
        assert!(measure(&received(&wav), 1.0, &cancel).is_none());
        assert!(measure(&received(b"not a song"), 0.0, &cancel).is_none());
        let body = [0xff, 0xf1, 2, 3, 4, 5, 6];
        let mut raw = RawFrames { remaining: &body, first: true, adts: false, counted: false };
        raw.accept(&body[..4]).unwrap();
        assert!(!raw.complete(), "a torn final raw frame is not clean EOF");
    }

    #[test]
    fn version_keys_distinguish_equal_size_masters_and_reject_trials() {
        let a = "0123456789abcdeffedcba9876543210";
        let b = "1123456789abcdeffedcba9876543210";
        let mut data = json!({"id":42, "size":900_000, "br":320_000, "type":"mp3", "level":"high", "url":"https://cdn.invalid/a?token=secret"});
        assert!(online_key(42, &data).is_none());
        for bad in ["unknown", "00000000000000000000000000000000"] {
            data["md5"] = json!(bad);
            assert!(online_key(42, &data).is_none());
        }
        data["md5"] = json!(a);
        let old = online_key(42, &data).unwrap();
        data["md5"] = json!(a.to_uppercase());
        data["url"] = json!("https://other.invalid/?token=changed");
        assert_eq!(online_key(42, &data).as_deref(), Some(old.as_str()));
        data["md5"] = json!(b);
        assert_ne!(online_key(42, &data).unwrap(), old);
        assert!(online_key(43, &data).is_none());
        data["freeTrialInfo"] = json!({"start": 0, "end": 30});
        assert!(online_key(42, &data).is_none());
        let cancel = AtomicBool::new(false);
        assert_ne!(content_key(&received(b"masterA"), &cancel), content_key(&received(b"masterB"), &cancel));
        assert!(content_key(&received(b"masterA"), &AtomicBool::new(true)).is_none());
    }

    #[test]
    fn disk_cache_is_bounded_and_never_writes_source_paths() {
        let root = std::env::temp_dir().join(format!("enji-loudness-cache-{}-{:016x}", std::process::id(), rand::random::<u64>()));
        let path = root.join("loudness.json");
        let private = "/private/Music/recording.flac";
        let info = Info::replaygain("-4", Some("0.5")).unwrap();
        let mut cache = Cache::default();
        cache.insert(private.to_string(), info);
        for n in 0..MAX_ENTRIES - 1 { cache.insert(n.to_string(), info); }
        cache.insert(private.to_string(), info);
        cache.insert("new".into(), info);
        assert_eq!(cache.entries.len(), MAX_ENTRIES);
        cache.write(&path).unwrap();
        cache.write(&path).unwrap(); // replace an existing complete generation
        assert!(!fs::read_to_string(&path).unwrap().contains(private));
        let loaded = Cache::read(&path).unwrap();
        assert_eq!(loaded.get(private), Some(info));
        assert_eq!(loaded.get("0"), None);
        assert_eq!(loaded.get("new"), Some(info));
        fs::write(&path, vec![b' '; MAX_FILE_BYTES as usize + 1]).unwrap();
        assert!(Cache::read(&path).is_none());
        fs::remove_dir_all(root).unwrap();
    }
}

//! Playback on a thread of its own, which owns the audio output. A song
//! plays while it downloads: reading waits for the bytes not here yet. The
//! thread reports where the song is and when it has ended.

use crate::beats::{Beat, Beats};
#[cfg(target_os = "tvos")]
use crate::tv_audio::Output;
#[cfg(target_os = "android")]
use crate::usb_audio::{Exclusive, Plugged};
#[cfg(target_os = "watchos")]
use crate::watch_audio::Output;
use rodio::{Sink, Source};
use symphonia::core::{
    audio::SampleBuffer,
    codecs::{Decoder as Codec, DecoderOptions},
    formats::{FormatOptions, FormatReader, SeekMode, SeekTo},
    io::{MediaSource, MediaSourceStream},
    meta::MetadataOptions,
    probe::Hint,
    units::Time,
};
use std::{
    io::{Read, Seek, SeekFrom},
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc, Arc, Condvar, Mutex,
    },
    time::{Duration, Instant},
};

/// How often a USB DAC is looked for, while one is wanted.
const LOOK_AGAIN: Duration = Duration::from_secs(2);

/// How loud the music being played is: the RMS of the samples decoded
/// last, from 0 to about 1, and of their low end (below about 150 Hz,
/// where the drums are). Kept as the bits of an `f32`, read by plugins.
static LEVEL: AtomicU32 = AtomicU32::new(0);
static BASS: AtomicU32 = AtomicU32::new(0);

/// Whether anything listens for the beat: finding it takes some work.
static FOLLOW_BEAT: AtomicBool = AtomicBool::new(false);
/// Beats told so far, and the last one's strength and tempo as the bits of
/// an `f32`.
static BEATS: AtomicU32 = AtomicU32::new(0);
static BEAT_STRENGTH: AtomicU32 = AtomicU32::new(0);
static BEAT_BPM: AtomicU32 = AtomicU32::new(0);
/// How long before the beat lands it is told: for a pulse to rise to its
/// peak on the beat, after the way through a plugin to the screen.
const BEAT_LEAD: f64 = 0.12;

/// Find the beat of what plays, or stop.
pub fn follow_beat(on: bool) {
    FOLLOW_BEAT.store(on, Ordering::Relaxed);
}

/// How many beats were told so far, and the last one.
pub fn beat() -> (u32, Beat) {
    let beat = Beat {
        strength: f32::from_bits(BEAT_STRENGTH.load(Ordering::Relaxed)),
        bpm: f32::from_bits(BEAT_BPM.load(Ordering::Relaxed)),
    };
    (BEATS.load(Ordering::Relaxed), beat)
}

/// The loudness of the music being played and of its low end, before the
/// volume.
pub fn levels() -> (f32, f32) {
    (f32::from_bits(LEVEL.load(Ordering::Relaxed)), f32::from_bits(BASS.load(Ordering::Relaxed)))
}

/// The system's audio output, as rodio plays through it.
#[cfg(not(any(target_os = "tvos", target_os = "watchos")))]
struct Shared {
    _stream: rodio::OutputStream,
    handle: rodio::OutputStreamHandle,
}

/// Where a song plays. A DAC of its own leaves no room for the shared
/// output, which would keep it mixing.
#[cfg(not(any(target_os = "tvos", target_os = "watchos")))]
struct Output {
    shared: Option<Shared>,
    /// A USB DAC is to be used alone.
    #[cfg(target_os = "android")]
    wanted: bool,
    /// The USB output plugged in, while `wanted`.
    #[cfg(target_os = "android")]
    usb: Option<Plugged>,
    #[cfg(target_os = "android")]
    exclusive: Option<Exclusive>,
}

#[cfg(not(any(target_os = "tvos", target_os = "watchos")))]
impl Output {
    fn open() -> Result<Output, String> {
        let mut output = Output {
            shared: None,
            #[cfg(target_os = "android")]
            wanted: false,
            #[cfg(target_os = "android")]
            usb: None,
            #[cfg(target_os = "android")]
            exclusive: None,
        };
        output.share()?;
        Ok(output)
    }

    /// The shared output, opened if it is not.
    fn share(&mut self) -> Result<&rodio::OutputStreamHandle, String> {
        if self.shared.is_none() {
            let (_stream, handle) = rodio::OutputStream::try_default().map_err(|e| e.to_string())?;
            self.shared = Some(Shared { _stream, handle });
        }
        Ok(&self.shared.as_ref().expect("opened above").handle)
    }

    /// A sink for a song of `rate`: on the DAC if it can take that rate.
    fn sink(&mut self, rate: u32) -> Result<Sink, String> {
        #[cfg(target_os = "android")]
        if let Some(sink) = self.exclusive_sink(rate) {
            return Ok(sink);
        }
        #[cfg(not(target_os = "android"))]
        let _ = rate;
        Sink::try_new(self.share()?).map_err(|e| e.to_string())
    }
}

/// Elsewhere no app can have a USB DAC to itself.
#[cfg(not(target_os = "android"))]
impl Output {
    fn set_exclusive(&mut self, _: bool) -> bool {
        false
    }
    fn moved(&mut self) -> bool {
        false
    }
    fn rest(&self) {}
    fn wake(&self) {}
    fn report(&self, _: &mut Status, _: bool) {}
}

#[cfg(target_os = "android")]
impl Output {
    fn exclusive_sink(&mut self, rate: u32) -> Option<Sink> {
        if self.exclusive.as_ref().is_some_and(|open| open.lost() || open.rate() != rate) {
            self.exclusive = None;
        }
        if self.exclusive.is_none() {
            self.usb = if self.wanted { Plugged::find() } else { None };
            let usb = self.usb.as_ref().filter(|usb| usb.takes(rate))?;
            // While the shared output plays, the DAC keeps mixing.
            self.shared = None;
            self.exclusive = Exclusive::open(usb, rate);
        }
        Some(self.exclusive.as_ref()?.sink())
    }

    /// Whether what plays should move for this.
    fn set_exclusive(&mut self, on: bool) -> bool {
        self.wanted = on;
        self.usb = if on { Plugged::find() } else { None };
        self.exclusive.take().is_some() || self.usb.is_some()
    }

    /// Whether what plays should move: its DAC went, or one came.
    fn moved(&mut self) -> bool {
        if !self.wanted {
            return false;
        }
        let found = Plugged::find();
        let changed = self.usb.as_ref().map(Plugged::id) != found.as_ref().map(Plugged::id);
        self.usb = found;
        let moved = changed || self.exclusive.as_ref().is_some_and(Exclusive::lost);
        if moved {
            self.exclusive = None;
        }
        moved
    }

    fn rest(&self) {
        self.exclusive.iter().for_each(Exclusive::rest);
    }

    fn wake(&self) {
        self.exclusive.iter().for_each(Exclusive::wake);
    }

    fn report(&self, status: &mut Status, playing: bool) {
        status.usb_plugged = self.usb.is_some();
        status.usb_refused = playing && self.usb.is_some() && self.exclusive.is_none();
        status.usb_format = self.exclusive.as_ref().map_or_else(String::new, Exclusive::describe);
    }
}

/// Whether this system can give a USB DAC to one app.
pub fn exclusive_available() -> bool {
    #[cfg(target_os = "android")]
    return crate::usb_audio::available();
    #[cfg(not(target_os = "android"))]
    false
}

/// A song as it downloads.
#[derive(Clone)]
pub struct Stream {
    inner: Arc<(Mutex<Buffer>, Condvar)>,
}

#[derive(Default)]
struct Buffer {
    data: Vec<u8>,
    total: Option<u64>,
    done: bool,
    /// Another song started: readers stop.
    cancelled: bool,
}

impl Stream {
    pub fn new(total: Option<u64>) -> Self {
        let buffer = Buffer { total, data: Vec::with_capacity(total.unwrap_or(0) as usize), ..Buffer::default() };
        Self { inner: Arc::new((Mutex::new(buffer), Condvar::new())) }
    }

    fn change(&self, change: impl FnOnce(&mut Buffer)) {
        change(&mut self.inner.0.lock().unwrap_or_else(|e| e.into_inner()));
        self.inner.1.notify_all();
    }

    pub fn push(&self, chunk: &[u8]) {
        self.change(|b| b.data.extend_from_slice(chunk));
    }
    pub fn finish(&self) {
        self.change(|b| b.done = true);
    }
    pub fn cancel(&self) {
        self.change(|b| b.cancelled = true);
    }
    pub fn cancelled(&self) -> bool {
        self.inner.0.lock().unwrap_or_else(|e| e.into_inner()).cancelled
    }

    fn reader(&self) -> Reader {
        Reader { stream: self.clone(), at: 0 }
    }

    fn total(&self) -> Option<u64> {
        self.inner.0.lock().unwrap_or_else(|e| e.into_inner()).total
    }
}

struct Reader {
    stream: Stream,
    at: u64,
}

impl Read for Reader {
    fn read(&mut self, into: &mut [u8]) -> std::io::Result<usize> {
        let (lock, arrived) = &*self.stream.inner;
        let mut buffer = lock.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if buffer.cancelled {
                return Ok(0);
            }
            let have = buffer.data.len() as u64;
            if self.at < have {
                let from = self.at as usize;
                let n = into.len().min(buffer.data.len() - from);
                into[..n].copy_from_slice(&buffer.data[from..from + n]);
                self.at += n as u64;
                return Ok(n);
            }
            if buffer.done {
                return Ok(0);
            }
            buffer = arrived.wait_timeout(buffer, Duration::from_millis(200)).unwrap_or_else(|e| e.into_inner()).0;
        }
    }
}

impl Seek for Reader {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        let (lock, arrived) = &*self.stream.inner;
        let mut buffer = lock.lock().unwrap_or_else(|e| e.into_inner());
        let end = loop {
            if let Some(total) = buffer.total {
                break total;
            }
            if buffer.done || buffer.cancelled {
                break buffer.data.len() as u64;
            }
            if !matches!(to, SeekFrom::End(_)) {
                break 0;
            }
            buffer = arrived.wait_timeout(buffer, Duration::from_millis(200)).unwrap_or_else(|e| e.into_inner()).0;
        };
        let target = match to {
            SeekFrom::Start(at) => at as i64,
            SeekFrom::Current(by) => self.at as i64 + by,
            SeekFrom::End(by) => end as i64 + by,
        };
        if target < 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "before the start"));
        }
        self.at = target as u64;
        Ok(self.at)
    }
}

impl MediaSource for Reader {
    fn is_seekable(&self) -> bool {
        true
    }

    /// The length, which symphonia's MP4 reader needs: without it, it
    /// never finds the end of an AAC song.
    fn byte_len(&self) -> Option<u64> {
        self.stream.total()
    }
}

/// A song's samples as symphonia decodes them, for the output. A packet
/// that does not decode is skipped; past a few in a row the song ends.
struct Decoded {
    format: Box<dyn FormatReader>,
    codec: Box<dyn Codec>,
    track: u32,
    channels: u16,
    rate: u32,
    total: Option<Duration>,
    samples: Vec<f32>,
    at: usize,
    /// Samples to drop after a seek, to land where it was asked.
    skip: usize,
    /// The low-pass filter of the low end, as it was after the last sample.
    low: f32,
    /// What follows the beat, while something listens for it.
    beats: Option<Beats>,
}

impl Decoded {
    fn open(stream: &Stream) -> Result<Self, String> {
        let source = MediaSourceStream::new(Box::new(stream.reader()), Default::default());
        let probed = symphonia::default::get_probe()
            .format(&Hint::new(), source, &FormatOptions { enable_gapless: true, ..Default::default() }, &MetadataOptions::default())
            .map_err(|e| e.to_string())?;
        let format = probed.format;
        let track = format
            .tracks()
            .iter()
            .find(|track| track.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
            .ok_or("没有音轨")?;
        let (track_id, params) = (track.id, track.codec_params.clone());
        let codec = symphonia::default::get_codecs()
            .make(&params, &DecoderOptions::default())
            .map_err(|e| e.to_string())?;
        let rate = params.sample_rate.unwrap_or(44_100);
        let total = params.n_frames.map(|frames| Duration::from_secs_f64(frames as f64 / f64::from(rate)));
        let mut decoded = Self {
            format,
            codec,
            track: track_id,
            channels: params.channels.map_or(2, |channels| channels.count() as u16),
            rate,
            total,
            samples: Vec::new(),
            at: 0,
            skip: 0,
            low: 0.0,
            beats: None,
        };
        // The first packet tells the real layout, which a stream may not
        // have said in its header.
        decoded.refill();
        Ok(decoded)
    }

    /// The next packet of the track decoded; false at the end.
    /// The loudness of the samples just decoded, and of their low end.
    fn meter(&mut self) {
        let channels = self.channels.max(1) as usize;
        let frames = self.samples.len() / channels;
        if frames == 0 {
            return;
        }
        let alpha = 1.0 - (-2.0 * std::f32::consts::PI * 150.0 / self.rate.max(1) as f32).exp();
        if !FOLLOW_BEAT.load(Ordering::Relaxed) {
            self.beats = None;
        } else if self.beats.is_none() {
            self.beats = Some(Beats::new(self.rate, BEAT_LEAD));
        }
        let (mut all, mut low) = (0.0f32, 0.0f32);
        for frame in self.samples.chunks_exact(channels) {
            let mono = frame.iter().sum::<f32>() / channels as f32;
            self.low += alpha * (mono - self.low);
            all += mono * mono;
            low += self.low * self.low;
            if let Some(beat) = self.beats.as_mut().and_then(|beats| beats.hear(mono)) {
                BEAT_STRENGTH.store(beat.strength.to_bits(), Ordering::Relaxed);
                BEAT_BPM.store(beat.bpm.to_bits(), Ordering::Relaxed);
                BEATS.fetch_add(1, Ordering::Relaxed);
            }
        }
        LEVEL.store((all / frames as f32).sqrt().to_bits(), Ordering::Relaxed);
        BASS.store((low / frames as f32).sqrt().to_bits(), Ordering::Relaxed);
    }

    fn refill(&mut self) -> bool {
        let mut failures = 0;
        loop {
            let packet = match self.format.next_packet() {
                Ok(packet) => packet,
                Err(_) => return false,
            };
            if packet.track_id() != self.track {
                continue;
            }
            match self.codec.decode(&packet) {
                Ok(audio) => {
                    let spec = *audio.spec();
                    let mut buffer = SampleBuffer::<f32>::new(audio.capacity() as u64, spec);
                    buffer.copy_interleaved_ref(audio);
                    self.channels = spec.channels.count() as u16;
                    self.rate = spec.rate;
                    self.samples = buffer.samples().to_vec();
                    self.meter();
                    self.at = self.skip.min(self.samples.len());
                    self.skip -= self.at;
                    if self.at < self.samples.len() {
                        return true;
                    }
                }
                Err(symphonia::core::errors::Error::DecodeError(_)) if failures < 8 => failures += 1,
                Err(symphonia::core::errors::Error::ResetRequired) => self.codec.reset(),
                Err(_) => return false,
            }
        }
    }
}

impl Iterator for Decoded {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.at >= self.samples.len() && !self.refill() {
            return None;
        }
        let sample = self.samples[self.at];
        self.at += 1;
        Some(sample)
    }
}

impl Source for Decoded {
    /// One span to the end: a song keeps its rate and channels. rodio
    /// starts its resampler afresh at each span, so a span per packet put a
    /// click at every packet, tens a second, heard as a low buzz, and after
    /// the one-sample span between packets swapped left and right.
    fn current_frame_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        self.channels
    }
    fn sample_rate(&self) -> u32 {
        self.rate
    }
    fn total_duration(&self) -> Option<Duration> {
        self.total
    }
    fn try_seek(&mut self, at: Duration) -> Result<(), rodio::source::SeekError> {
        let time = Time::new(at.as_secs(), f64::from(at.subsec_nanos()) / 1e9);
        let seeked = self
            .format
            .seek(SeekMode::Accurate, SeekTo::Time { time, track_id: Some(self.track) })
            .map_err(|e| rodio::source::SeekError::Other(Box::new(e)))?;
        self.codec.reset();
        self.samples.clear();
        // The beat is found again from where it lands.
        self.beats = None;
        self.at = 0;
        self.skip = seeked.required_ts.saturating_sub(seeked.actual_ts) as usize * usize::from(self.channels);
        Ok(())
    }
}

enum Command {
    Play(u64, Stream),
    Pause,
    Resume,
    Seek(f64),
    Volume(f32),
    Exclusive(bool),
    Stop,
}

#[derive(Debug, Clone, Default)]
pub struct Status {
    /// Seconds into the song.
    pub position: f64,
    pub playing: bool,
    /// The song of this token has played to its end.
    pub ended: Option<u64>,
    pub error: Option<String>,
    /// A USB output is plugged in, as far as anything wanted one.
    pub usb_plugged: bool,
    /// It is plugged in, and the song plays without it.
    pub usb_refused: bool,
    /// How the USB DAC plays now that the song has it alone; empty otherwise.
    pub usb_format: String,
}

pub struct Audio {
    commands: mpsc::Sender<Command>,
    status: Arc<Mutex<Status>>,
}

impl Audio {
    pub fn start(volume: f32, exclusive: bool) -> Self {
        let (commands, receive) = mpsc::channel();
        let status = Arc::new(Mutex::new(Status::default()));
        let shared = status.clone();
        let _ = std::thread::Builder::new()
            .name("enji-audio".into())
            .spawn(move || run(receive, shared, volume, exclusive));
        Self { commands, status }
    }

    pub fn play(&self, token: u64, stream: Stream) {
        let _ = self.commands.send(Command::Play(token, stream));
    }
    pub fn pause(&self) {
        let _ = self.commands.send(Command::Pause);
    }
    pub fn resume(&self) {
        let _ = self.commands.send(Command::Resume);
    }
    pub fn seek(&self, seconds: f64) {
        let _ = self.commands.send(Command::Seek(seconds));
    }
    pub fn volume(&self, volume: f32) {
        let _ = self.commands.send(Command::Volume(volume));
    }
    pub fn stop(&self) {
        let _ = self.commands.send(Command::Stop);
    }
    /// Whether a USB DAC is to be used alone, the song playing moving at once.
    pub fn exclusive(&self, on: bool) {
        let _ = self.commands.send(Command::Exclusive(on));
    }

    /// Seconds into the song, as often as words change.
    pub fn position(&self) -> f64 {
        self.status.lock().unwrap_or_else(|e| e.into_inner()).position
    }

    /// A song plays now.
    pub fn playing(&self) -> bool {
        self.status.lock().unwrap_or_else(|e| e.into_inner()).playing
    }

    /// What the thread last reported; an ended song is reported once.
    pub fn status(&self) -> Status {
        let mut status = self.status.lock().unwrap_or_else(|e| e.into_inner());
        let now = status.clone();
        status.ended = None;
        status.error = None;
        now
    }
}

/// `stream` playing on `output` from its start.
fn start(output: &mut Output, stream: &Stream, volume: f32) -> Result<Sink, String> {
    let source = Decoded::open(stream).map_err(|e| format!("无法解码这首歌：{e}"))?;
    let sink = output.sink(source.rate)?;
    sink.set_volume(volume);
    sink.append(source);
    Ok(sink)
}

/// The song of `old` carrying on where it was, on what `output` gives now.
fn moved_on(output: &mut Output, old: Sink, stream: &Stream, volume: f32) -> Result<Sink, String> {
    let (at, paused) = (old.get_pos(), old.is_paused());
    drop(old);
    let sink = start(output, stream, volume)?;
    sink.pause();
    let _ = sink.try_seek(at);
    if paused {
        output.rest();
    } else {
        sink.play();
    }
    Ok(sink)
}

fn run(commands: mpsc::Receiver<Command>, status: Arc<Mutex<Status>>, mut volume: f32, exclusive: bool) {
    let report = |f: &dyn Fn(&mut Status)| f(&mut status.lock().unwrap_or_else(|e| e.into_inner()));
    let mut output = match Output::open() {
        Ok(found) => found,
        Err(e) => {
            let message = format!("没有可用的音频输出：{e}");
            report(&|s| s.error = Some(message.clone()));
            return;
        }
    };
    let _ = output.set_exclusive(exclusive);
    let mut sink: Option<Sink> = None;
    let mut current: Option<u64> = None;
    // What plays, to start it again on another output.
    let mut song: Option<Stream> = None;
    let mut looked = Instant::now();
    loop {
        let mut moving = false;
        match commands.recv_timeout(Duration::from_millis(30)) {
            Ok(Command::Play(token, stream)) => {
                sink = None;
                current = None;
                match start(&mut output, &stream, volume) {
                    Ok(next) => {
                        sink = Some(next);
                        current = Some(token);
                        song = Some(stream);
                    }
                    Err(message) => report(&|s| s.error = Some(message.clone())),
                }
            }
            Ok(Command::Pause) => {
                sink.iter().for_each(Sink::pause);
                output.rest();
            }
            Ok(Command::Resume) => {
                output.wake();
                sink.iter().for_each(Sink::play);
            }
            Ok(Command::Seek(seconds)) => {
                if let Some(sink) = &sink {
                    // A seek waits for the output to read on.
                    output.wake();
                    let _ = sink.try_seek(Duration::from_secs_f64(seconds.max(0.0)));
                    if sink.is_paused() {
                        output.rest();
                    }
                }
            }
            Ok(Command::Volume(level)) => {
                volume = level;
                sink.iter().for_each(|sink| sink.set_volume(level));
            }
            Ok(Command::Exclusive(on)) => {
                moving = output.set_exclusive(on);
            }
            Ok(Command::Stop) => {
                sink = None;
                current = None;
                song = None;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        if looked.elapsed() >= LOOK_AGAIN {
            looked = Instant::now();
            moving |= output.moved();
        }
        if moving
            && current.is_some()
            && let (Some(old), Some(stream)) = (sink.take(), &song)
        {
            match moved_on(&mut output, old, stream, volume) {
                Ok(next) => sink = Some(next),
                Err(message) => report(&|s| s.error = Some(message.clone())),
            }
        }
        let mut now = status.lock().unwrap_or_else(|e| e.into_inner());
        output.report(&mut now, current.is_some());
        match &sink {
            Some(sink) => {
                now.position = sink.get_pos().as_secs_f64();
                now.playing = !sink.is_paused() && !sink.empty();
                if sink.empty()
                    && let Some(token) = current.take()
                {
                    now.ended = Some(token);
                }
            }
            None => {
                now.position = 0.0;
                now.playing = false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Decoded, Stream};
    use rodio::source::UniformSourceIterator;

    /// Two seconds of a 440 Hz tone at 44.1 kHz as a WAV, the right channel
    /// half as loud as the left.
    fn tone() -> Vec<u8> {
        let frames = 88_200u32;
        let mut wav = Vec::new();
        let mut put = |bytes: &[u8]| wav.extend_from_slice(bytes);
        put(b"RIFF");
        put(&(36 + frames * 4).to_le_bytes());
        put(b"WAVEfmt ");
        put(&16u32.to_le_bytes());
        put(&1u16.to_le_bytes());
        put(&2u16.to_le_bytes());
        put(&44_100u32.to_le_bytes());
        put(&(44_100u32 * 4).to_le_bytes());
        put(&4u16.to_le_bytes());
        put(&16u16.to_le_bytes());
        put(b"data");
        put(&(frames * 4).to_le_bytes());
        for i in 0..frames {
            let left = (i as f32 / 44_100.0 * 440.0 * std::f32::consts::TAU).sin() * 16_000.0;
            put(&(left as i16).to_le_bytes());
            put(&((left / 2.0) as i16).to_le_bytes());
        }
        wav
    }

    #[test]
    fn a_resampled_song_keeps_its_channels_and_has_no_clicks() {
        let wav = tone();
        let stream = Stream::new(Some(wav.len() as u64));
        stream.push(&wav);
        stream.finish();
        let decoded = Decoded::open(&stream).expect("a WAV decodes");
        let mut out = Vec::new();
        for sample in UniformSourceIterator::<_, f32>::new(decoded, 2, 48_000) {
            out.push(sample);
        }
        let left: Vec<f32> = out.chunks_exact(2).map(|frame| frame[0]).collect();
        let swapped = out.chunks_exact(2).filter(|frame| (frame[1] - frame[0] / 2.0).abs() > 0.01).count();
        let clicks = left.windows(3).filter(|w| (w[2] - 2.0 * w[1] + w[0]).abs() > 0.02).count();
        assert!(left.len() > 90_000, "{} frames", left.len());
        assert_eq!((swapped, clicks), (0, 0));
    }
}

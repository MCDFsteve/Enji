//! Playback on a thread of its own, which owns the audio output. A song
//! plays while it downloads: reading waits for the bytes not here yet. The
//! thread reports where the song is and when it has ended.

use rodio::{OutputStream, Sink, Source};
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
        atomic::{AtomicU32, Ordering},
        mpsc, Arc, Condvar, Mutex,
    },
    time::Duration,
};

/// How loud the music being played is: the RMS of the samples decoded
/// last, from 0 to about 1, and of their low end (below about 150 Hz,
/// where the drums are). Kept as the bits of an `f32`, read by plugins.
static LEVEL: AtomicU32 = AtomicU32::new(0);
static BASS: AtomicU32 = AtomicU32::new(0);

/// The loudness of the music being played and of its low end, before the
/// volume.
pub fn levels() -> (f32, f32) {
    (f32::from_bits(LEVEL.load(Ordering::Relaxed)), f32::from_bits(BASS.load(Ordering::Relaxed)))
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
        let (mut all, mut low) = (0.0f32, 0.0f32);
        for frame in self.samples.chunks_exact(channels) {
            let mono = frame.iter().sum::<f32>() / channels as f32;
            self.low += alpha * (mono - self.low);
            all += mono * mono;
            low += self.low * self.low;
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
    fn current_frame_len(&self) -> Option<usize> {
        Some(self.samples.len().saturating_sub(self.at).max(1))
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
}

pub struct Audio {
    commands: mpsc::Sender<Command>,
    status: Arc<Mutex<Status>>,
}

impl Audio {
    pub fn start(volume: f32) -> Self {
        let (commands, receive) = mpsc::channel();
        let status = Arc::new(Mutex::new(Status::default()));
        let shared = status.clone();
        let _ = std::thread::Builder::new()
            .name("enji-audio".into())
            .spawn(move || run(receive, shared, volume));
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

    /// What the thread last reported; an ended song is reported once.
    pub fn status(&self) -> Status {
        let mut status = self.status.lock().unwrap_or_else(|e| e.into_inner());
        let now = status.clone();
        status.ended = None;
        status.error = None;
        now
    }
}

fn run(commands: mpsc::Receiver<Command>, status: Arc<Mutex<Status>>, mut volume: f32) {
    let report = |f: &dyn Fn(&mut Status)| f(&mut status.lock().unwrap_or_else(|e| e.into_inner()));
    let (_stream, output) = match OutputStream::try_default() {
        Ok(found) => found,
        Err(e) => {
            let message = format!("没有可用的音频输出：{e}");
            report(&|s| s.error = Some(message.clone()));
            return;
        }
    };
    let mut sink: Option<Sink> = None;
    let mut current: Option<u64> = None;
    loop {
        match commands.recv_timeout(Duration::from_millis(100)) {
            Ok(Command::Play(token, stream)) => {
                sink = None;
                current = None;
                match Decoded::open(&stream) {
                    Ok(source) => match Sink::try_new(&output) {
                        Ok(next) => {
                            next.set_volume(volume);
                            next.append(source);
                            sink = Some(next);
                            current = Some(token);
                        }
                        Err(e) => {
                            let message = e.to_string();
                            report(&|s| s.error = Some(message.clone()));
                        }
                    },
                    Err(e) => {
                        let message = format!("无法解码这首歌：{e}");
                        report(&|s| s.error = Some(message.clone()));
                    }
                }
            }
            Ok(Command::Pause) => sink.iter().for_each(Sink::pause),
            Ok(Command::Resume) => sink.iter().for_each(Sink::play),
            Ok(Command::Seek(seconds)) => {
                if let Some(sink) = &sink {
                    let _ = sink.try_seek(Duration::from_secs_f64(seconds.max(0.0)));
                }
            }
            Ok(Command::Volume(level)) => {
                volume = level;
                sink.iter().for_each(|sink| sink.set_volume(level));
            }
            Ok(Command::Stop) => {
                sink = None;
                current = None;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        let mut now = status.lock().unwrap_or_else(|e| e.into_inner());
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


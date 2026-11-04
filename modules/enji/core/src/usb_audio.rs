//! A USB DAC to itself on Android 14 and later. The system normally mixes
//! everything at one rate of its own; here it is asked to run the DAC at the
//! song's rate and to leave the samples as they are, and a stream of that
//! very format plays them.

/// How the samples of a stream are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    I16,
    /// Three bytes each.
    I24,
    I32,
    F32,
}

impl Encoding {
    fn bytes(self) -> usize {
        match self {
            Encoding::I16 => 2,
            Encoding::I24 => 3,
            Encoding::I32 | Encoding::F32 => 4,
        }
    }

    /// How much it is preferred: the more bits the better, as a lossy song
    /// decodes to more than sixteen.
    fn rank(self) -> u8 {
        match self {
            Encoding::I32 => 3,
            Encoding::I24 => 2,
            Encoding::I16 => 1,
            Encoding::F32 => 0,
        }
    }

    fn bits(self) -> u8 {
        match self {
            Encoding::I16 => 16,
            Encoding::I24 => 24,
            Encoding::I32 | Encoding::F32 => 32,
        }
    }
}

/// `samples` written into `out`, as many as it holds, silence once they end.
/// A song's own samples come back to the integers they were decoded from:
/// the scale is a power of two, which a float keeps exactly.
fn fill(encoding: Encoding, samples: &mut impl Iterator<Item = f32>, out: &mut [u8]) {
    let scaled = |sample: f32, bits: u32| {
        let top = f64::from(1u32 << (bits - 1));
        (f64::from(sample) * top).round().clamp(-top, top - 1.0) as i32
    };
    for slot in out.chunks_exact_mut(encoding.bytes()) {
        let sample = samples.next().unwrap_or(0.0);
        match encoding {
            Encoding::I16 => slot.copy_from_slice(&(scaled(sample, 16) as i16).to_le_bytes()),
            Encoding::I24 => slot.copy_from_slice(&scaled(sample, 24).to_le_bytes()[..3]),
            Encoding::I32 => slot.copy_from_slice(&scaled(sample, 32).to_le_bytes()),
            Encoding::F32 => slot.copy_from_slice(&sample.to_le_bytes()),
        }
    }
}

/// What a DAC can take, and whether the system leaves it untouched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Format {
    rate: u32,
    encoding: Encoding,
    untouched: bool,
}

/// The format of `offered` to play a song of `rate` in: one of that rate, so
/// nothing resamples it, untouched if there is one.
fn best(offered: &[Format], rate: u32) -> Option<usize> {
    offered
        .iter()
        .enumerate()
        .filter(|(_, format)| format.rate == rate)
        .max_by_key(|(_, format)| (format.untouched, format.encoding.rank()))
        .map(|(at, _)| at)
}

#[cfg(target_os = "android")]
pub use android::{available, Exclusive, Plugged};

#[cfg(target_os = "android")]
mod android {
    use super::{best, fill, Encoding, Format};
    use jni::{
        objects::{GlobalRef, JObject, JObjectArray, JValue},
        JNIEnv, JavaVM,
    };
    use rodio::{
        dynamic_mixer::{mixer, DynamicMixer, DynamicMixerController},
        Sink,
    };
    use std::{
        ffi::c_void,
        ptr,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        time::Duration,
    };

    const CHANNELS: u16 = 2;
    /// `Build.VERSION_CODES.UPSIDE_DOWN_CAKE`: what asks for a mixer came then.
    const FIRST_SDK: i32 = 34;
    const GET_DEVICES_OUTPUTS: i32 = 2;
    const TYPE_USB_DEVICE: i32 = 11;
    const TYPE_USB_HEADSET: i32 = 22;
    const USAGE_MEDIA: i32 = 1;
    const MIXER_BEHAVIOR_BIT_PERFECT: i32 = 1;
    const ENCODING_PCM_16BIT: i32 = 2;
    const ENCODING_PCM_FLOAT: i32 = 4;
    const ENCODING_PCM_24BIT_PACKED: i32 = 21;
    const ENCODING_PCM_32BIT: i32 = 22;

    const AAUDIO_FORMAT_PCM_I16: i32 = 1;
    const AAUDIO_FORMAT_PCM_FLOAT: i32 = 2;
    const AAUDIO_FORMAT_PCM_I24_PACKED: i32 = 3;
    const AAUDIO_FORMAT_PCM_I32: i32 = 4;
    const AAUDIO_CALLBACK_RESULT_CONTINUE: i32 = 0;

    type DataCallback = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, i32) -> i32;
    type ErrorCallback = unsafe extern "C" fn(*mut c_void, *mut c_void, i32);

    #[link(name = "aaudio")]
    unsafe extern "C" {
        fn AAudio_createStreamBuilder(builder: *mut *mut c_void) -> i32;
        fn AAudioStreamBuilder_setDeviceId(builder: *mut c_void, device: i32);
        fn AAudioStreamBuilder_setSampleRate(builder: *mut c_void, rate: i32);
        fn AAudioStreamBuilder_setChannelCount(builder: *mut c_void, channels: i32);
        fn AAudioStreamBuilder_setFormat(builder: *mut c_void, format: i32);
        fn AAudioStreamBuilder_setDataCallback(builder: *mut c_void, callback: DataCallback, context: *mut c_void);
        fn AAudioStreamBuilder_setErrorCallback(builder: *mut c_void, callback: ErrorCallback, context: *mut c_void);
        fn AAudioStreamBuilder_openStream(builder: *mut c_void, stream: *mut *mut c_void) -> i32;
        fn AAudioStreamBuilder_delete(builder: *mut c_void) -> i32;
        fn AAudioStream_requestStart(stream: *mut c_void) -> i32;
        fn AAudioStream_requestPause(stream: *mut c_void) -> i32;
        fn AAudioStream_requestStop(stream: *mut c_void) -> i32;
        fn AAudioStream_close(stream: *mut c_void) -> i32;
    }

    fn message(error: impl std::fmt::Display) -> String {
        error.to_string()
    }

    /// `work` on this thread's way into Java, with the app's context. An
    /// exception it left, such as a method this system does not have, is its
    /// failure and no one else's.
    fn java<T>(work: impl FnOnce(&mut JNIEnv, &JObject) -> jni::errors::Result<T>) -> Result<T, String> {
        let context = ndk_context::android_context();
        // SAFETY: the host gave the VM and a global reference to the context
        // when it started, and both last as long as the process.
        let (vm, app) = unsafe { (JavaVM::from_raw(context.vm().cast()).map_err(message)?, JObject::from_raw(context.context().cast())) };
        let mut env = vm.attach_current_thread().map_err(message)?;
        let result = work(&mut env, &app);
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_clear();
        }
        result.map_err(message)
    }

    fn sdk(env: &mut JNIEnv) -> jni::errors::Result<i32> {
        env.get_static_field("android/os/Build$VERSION", "SDK_INT", "I")?.i()
    }

    fn manager<'a>(env: &mut JNIEnv<'a>, app: &JObject) -> jni::errors::Result<JObject<'a>> {
        let name = env.new_string("audio")?;
        env.call_method(app, "getSystemService", "(Ljava/lang/String;)Ljava/lang/Object;", &[JValue::Object(&name)])?.l()
    }

    /// What asks for a mixer plays for: the media a stream plays by default.
    fn media<'a>(env: &mut JNIEnv<'a>) -> jni::errors::Result<JObject<'a>> {
        let builder = env.new_object("android/media/AudioAttributes$Builder", "()V", &[])?;
        let builder = env
            .call_method(&builder, "setUsage", "(I)Landroid/media/AudioAttributes$Builder;", &[JValue::Int(USAGE_MEDIA)])?
            .l()?;
        env.call_method(&builder, "build", "()Landroid/media/AudioAttributes;", &[])?.l()
    }

    /// Whether this system can give an app a USB DAC.
    pub fn available() -> bool {
        java(|env, _| sdk(env)).is_ok_and(|level| level >= FIRST_SDK)
    }

    /// The USB output plugged in and what it can take: only then does a song
    /// have a format to ask for.
    pub struct Plugged {
        id: i32,
        device: GlobalRef,
        offered: Vec<(Format, GlobalRef)>,
    }

    impl Plugged {
        /// The first USB output among the system's, if there is one.
        pub fn find() -> Option<Plugged> {
            java(|env, app| {
                if sdk(env)? < FIRST_SDK {
                    return Ok(None);
                }
                let manager = manager(env, app)?;
                let devices = env
                    .call_method(&manager, "getDevices", "(I)[Landroid/media/AudioDeviceInfo;", &[JValue::Int(GET_DEVICES_OUTPUTS)])?
                    .l()?;
                let devices = JObjectArray::from(devices);
                for at in 0..env.get_array_length(&devices)? {
                    let device = env.get_object_array_element(&devices, at)?;
                    let kind = env.call_method(&device, "getType", "()I", &[])?.i()?;
                    if kind == TYPE_USB_DEVICE || kind == TYPE_USB_HEADSET {
                        return plugged(env, &manager, &device).map(Some);
                    }
                }
                Ok(None)
            })
            .ok()
            .flatten()
        }

        pub fn id(&self) -> i32 {
            self.id
        }

        /// Whether it can take a song of `rate` as it is.
        pub fn takes(&self, rate: u32) -> bool {
            self.offered.iter().any(|(format, _)| format.rate == rate)
        }
    }

    fn plugged(env: &mut JNIEnv, manager: &JObject, device: &JObject) -> jni::errors::Result<Plugged> {
        let id = env.call_method(device, "getId", "()I", &[])?.i()?;
        let list = env
            .call_method(manager, "getSupportedMixerAttributes", "(Landroid/media/AudioDeviceInfo;)Ljava/util/List;", &[JValue::Object(device)])?
            .l()?;
        let mut offered = Vec::new();
        for at in 0..env.call_method(&list, "size", "()I", &[])?.i()? {
            let mixer = env.call_method(&list, "get", "(I)Ljava/lang/Object;", &[JValue::Int(at)])?.l()?;
            let behavior = env.call_method(&mixer, "getMixerBehavior", "()I", &[])?.i()?;
            let format = env.call_method(&mixer, "getFormat", "()Landroid/media/AudioFormat;", &[])?.l()?;
            let encoding = env.call_method(&format, "getEncoding", "()I", &[])?.i()?;
            let rate = env.call_method(&format, "getSampleRate", "()I", &[])?.i()?;
            let channels = env.call_method(&format, "getChannelCount", "()I", &[])?.i()?;
            let encoding = match encoding {
                ENCODING_PCM_16BIT => Encoding::I16,
                ENCODING_PCM_24BIT_PACKED => Encoding::I24,
                ENCODING_PCM_32BIT => Encoding::I32,
                ENCODING_PCM_FLOAT => Encoding::F32,
                _ => continue,
            };
            if channels == i32::from(CHANNELS) {
                let format = Format { rate: rate as u32, encoding, untouched: behavior == MIXER_BEHAVIOR_BIT_PERFECT };
                offered.push((format, env.new_global_ref(&mixer)?));
            }
        }
        Ok(Plugged { id, device: env.new_global_ref(device)?, offered })
    }

    /// What the stream's thread reads the song from.
    struct Feed {
        mixer: DynamicMixer<f32>,
        encoding: Encoding,
        lost: Arc<AtomicBool>,
    }

    /// The system asks for the next samples.
    unsafe extern "C" fn data(_: *mut c_void, context: *mut c_void, audio: *mut c_void, frames: i32) -> i32 {
        // SAFETY: `context` is the feed the stream was opened with, which
        // outlives it, and only this thread reads it while it runs. `audio`
        // holds `frames` frames of the stream's format.
        unsafe {
            let feed = &mut *context.cast::<Feed>();
            let bytes = frames as usize * usize::from(CHANNELS) * feed.encoding.bytes();
            fill(feed.encoding, &mut feed.mixer, std::slice::from_raw_parts_mut(audio.cast::<u8>(), bytes));
        }
        AAUDIO_CALLBACK_RESULT_CONTINUE
    }

    /// The stream is gone, most often with the DAC: it is closed from the
    /// audio thread, which looks now and then.
    unsafe extern "C" fn failed(_: *mut c_void, context: *mut c_void, _: i32) {
        // SAFETY: as in `data`; this only reads the flag behind the feed.
        unsafe { (*context.cast::<Feed>()).lost.store(true, Ordering::Relaxed) };
    }

    /// The DAC with a stream of one format open on it.
    pub struct Exclusive {
        format: Format,
        device: GlobalRef,
        stream: *mut c_void,
        feed: *mut Feed,
        controller: Arc<DynamicMixerController<f32>>,
        lost: Arc<AtomicBool>,
    }

    impl Exclusive {
        /// A stream at `rate` on `usb`, if it can take that rate.
        pub fn open(usb: &Plugged, rate: u32) -> Option<Exclusive> {
            let formats: Vec<Format> = usb.offered.iter().map(|(format, _)| *format).collect();
            let (format, attributes) = &usb.offered[best(&formats, rate)?];
            let asked = java(|env, app| {
                let manager = manager(env, app)?;
                let media = media(env)?;
                env.call_method(
                    &manager,
                    "setPreferredMixerAttributes",
                    "(Landroid/media/AudioAttributes;Landroid/media/AudioDeviceInfo;Landroid/media/AudioMixerAttributes;)Z",
                    &[JValue::Object(&media), JValue::Object(usb.device.as_obj()), JValue::Object(attributes.as_obj())],
                )?
                .z()
            });
            if !asked.unwrap_or(false) {
                return None;
            }
            let opened = Self::stream(usb, *format);
            if opened.is_none() {
                release(&usb.device);
            }
            opened
        }

        fn stream(usb: &Plugged, format: Format) -> Option<Exclusive> {
            let (controller, source) = mixer::<f32>(CHANNELS, format.rate);
            let lost = Arc::new(AtomicBool::new(false));
            let feed = Box::into_raw(Box::new(Feed { mixer: source, encoding: format.encoding, lost: lost.clone() }));
            let audio_format = match format.encoding {
                Encoding::I16 => AAUDIO_FORMAT_PCM_I16,
                Encoding::I24 => AAUDIO_FORMAT_PCM_I24_PACKED,
                Encoding::I32 => AAUDIO_FORMAT_PCM_I32,
                Encoding::F32 => AAUDIO_FORMAT_PCM_FLOAT,
            };
            let mut stream = ptr::null_mut();
            // SAFETY: each call gets what its documentation asks for. The
            // builder is deleted once the stream is open, and the feed lives
            // until the stream is closed. A stream plays media as music
            // unless told otherwise, which is what the mixer was asked for.
            let opened = unsafe {
                let mut builder = ptr::null_mut();
                if AAudio_createStreamBuilder(&mut builder) != 0 {
                    drop(Box::from_raw(feed));
                    return None;
                }
                AAudioStreamBuilder_setDeviceId(builder, usb.id);
                AAudioStreamBuilder_setSampleRate(builder, format.rate as i32);
                AAudioStreamBuilder_setChannelCount(builder, i32::from(CHANNELS));
                AAudioStreamBuilder_setFormat(builder, audio_format);
                AAudioStreamBuilder_setDataCallback(builder, data, feed.cast());
                AAudioStreamBuilder_setErrorCallback(builder, failed, feed.cast());
                let result = AAudioStreamBuilder_openStream(builder, &mut stream);
                AAudioStreamBuilder_delete(builder);
                result == 0 && AAudioStream_requestStart(stream) == 0
            };
            if !opened {
                // SAFETY: the stream, if it opened, never started.
                unsafe {
                    if !stream.is_null() {
                        AAudioStream_close(stream);
                    }
                    drop(Box::from_raw(feed));
                }
                return None;
            }
            Some(Exclusive { format, device: usb.device.clone(), stream, feed, controller, lost })
        }

        pub fn rate(&self) -> u32 {
            self.format.rate
        }

        /// A sink playing through the stream.
        pub fn sink(&self) -> Sink {
            let (sink, queue) = Sink::new_idle();
            self.controller.add(queue);
            sink
        }

        /// Whether the stream died, as it does with its DAC.
        pub fn lost(&self) -> bool {
            self.lost.load(Ordering::Relaxed)
        }

        /// Lets other apps have the DAC while nothing plays. The sink says
        /// where it is at its next tick, which takes the stream running.
        pub fn rest(&self) {
            std::thread::sleep(Duration::from_millis(20));
            // SAFETY: the stream is open until this is dropped.
            unsafe { AAudioStream_requestPause(self.stream) };
        }

        pub fn wake(&self) {
            // SAFETY: the stream is open until this is dropped.
            unsafe { AAudioStream_requestStart(self.stream) };
        }

        /// How the DAC is playing, as a person reads it.
        pub fn describe(&self) -> String {
            let rate = f64::from(self.format.rate) / 1000.0;
            format!("{rate} kHz · {} bit", self.format.encoding.bits())
        }
    }

    impl Drop for Exclusive {
        fn drop(&mut self) {
            // SAFETY: closing waits for the stream's thread, and nothing
            // reads the feed after that.
            unsafe {
                AAudioStream_requestStop(self.stream);
                AAudioStream_close(self.stream);
                drop(Box::from_raw(self.feed));
            }
            release(&self.device);
        }
    }

    /// The DAC back to the system.
    fn release(device: &GlobalRef) {
        let _ = java(|env, app| {
            let manager = manager(env, app)?;
            let media = media(env)?;
            env.call_method(
                &manager,
                "clearPreferredMixerAttributes",
                "(Landroid/media/AudioAttributes;Landroid/media/AudioDeviceInfo;)Z",
                &[JValue::Object(&media), JValue::Object(device.as_obj())],
            )?
            .z()
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{best, fill, Encoding, Format};

    fn format(rate: u32, encoding: Encoding, untouched: bool) -> Format {
        Format { rate, encoding, untouched }
    }

    #[test]
    fn sixteen_bit_samples_come_back_as_they_were() {
        let song: Vec<i16> = vec![0, 1, -1, 12_345, -12_345, i16::MAX, i16::MIN];
        let mut samples = song.iter().map(|sample| f32::from(*sample) / 32_768.0);
        let mut out = vec![0; song.len() * 2];
        fill(Encoding::I16, &mut samples, &mut out);
        let back: Vec<i16> = out.chunks_exact(2).map(|two| i16::from_le_bytes([two[0], two[1]])).collect();
        assert_eq!(back, song);
    }

    #[test]
    fn twenty_four_bit_samples_come_back_in_every_wider_encoding() {
        let song: Vec<i32> = vec![0, 1, -1, 8_388_607, -8_388_608, 1_234_567, -7_654_321];
        let float = |sample: i32| sample as f32 / 8_388_608.0;
        let mut out = vec![0; song.len() * 3];
        fill(Encoding::I24, &mut song.iter().copied().map(float), &mut out);
        let packed: Vec<i32> = out.chunks_exact(3).map(|three| i32::from_le_bytes([0, three[0], three[1], three[2]]) >> 8).collect();
        assert_eq!(packed, song);
        let mut out = vec![0; song.len() * 4];
        fill(Encoding::I32, &mut song.iter().copied().map(float), &mut out);
        let wide: Vec<i32> = out.chunks_exact(4).map(|four| i32::from_le_bytes([four[0], four[1], four[2], four[3]]) >> 8).collect();
        assert_eq!(wide, song);
    }

    #[test]
    fn a_full_sample_stays_in_range_and_the_rest_is_silence() {
        let mut out = vec![0xFF; 6];
        fill(Encoding::I16, &mut [1.5f32].into_iter(), &mut out);
        assert_eq!(out, [0xFF, 0x7F, 0, 0, 0, 0]);
    }

    #[test]
    fn a_song_takes_its_own_rate_untouched_with_the_most_bits() {
        let offered = [
            format(44_100, Encoding::I16, true),
            format(44_100, Encoding::I32, false),
            format(44_100, Encoding::I24, true),
            format(48_000, Encoding::I32, true),
        ];
        assert_eq!(best(&offered, 44_100), Some(2));
        assert_eq!(best(&offered, 48_000), Some(3));
        assert_eq!(best(&offered, 96_000), None);
    }
}

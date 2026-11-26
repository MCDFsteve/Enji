//! The audio output on iOS. `rodio`'s default `OutputStream` keeps its
//! `RemoteIO` audio unit running with silence while paused, which makes iOS
//! report the app as still playing on the Dynamic Island, Lock Screen, and
//! Control Center. Controlling `RemoteIO` directly stops the hardware unit on
//! pause while keeping `AVAudioSession` active so system controls stay visible
//! and reflect the paused state.

use objc2_avf_audio::{AVAudioSession, AVAudioSessionCategoryPlayback};
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
};

const CHANNELS: u16 = 2;
const RATE: u32 = 48_000;

const fn code(name: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*name)
}

#[repr(C)]
struct ComponentDescription {
    kind: u32,
    sub_kind: u32,
    manufacturer: u32,
    flags: u32,
    flags_mask: u32,
}

#[repr(C)]
struct StreamDescription {
    sample_rate: f64,
    format: u32,
    format_flags: u32,
    bytes_per_packet: u32,
    frames_per_packet: u32,
    bytes_per_frame: u32,
    channels: u32,
    bits_per_channel: u32,
    reserved: u32,
}

#[repr(C)]
struct Buffer {
    channels: u32,
    bytes: u32,
    data: *mut c_void,
}

#[repr(C)]
struct BufferList {
    count: u32,
    buffers: [Buffer; 1],
}

type Render = unsafe extern "C" fn(*mut c_void, *mut u32, *const c_void, u32, u32, *mut BufferList) -> i32;

#[repr(C)]
struct RenderCallback {
    render: Render,
    context: *mut c_void,
}

const OUTPUT: u32 = code(b"auou");
const REMOTE_IO: u32 = code(b"rioc");
const APPLE: u32 = code(b"appl");
const LINEAR_PCM: u32 = code(b"lpcm");
/// Floats, packed.
const FLOAT_PACKED: u32 = 1 | 8;
const STREAM_FORMAT: u32 = 8;
const SET_RENDER_CALLBACK: u32 = 23;
const IS_RUNNING: u32 = 2001;
const GLOBAL_SCOPE: u32 = 0;
const INPUT_SCOPE: u32 = 1;

#[link(name = "AudioToolbox", kind = "framework")]
unsafe extern "C" {
    fn AudioComponentFindNext(after: *mut c_void, description: *const ComponentDescription) -> *mut c_void;
    fn AudioComponentInstanceNew(component: *mut c_void, instance: *mut *mut c_void) -> i32;
    fn AudioComponentInstanceDispose(instance: *mut c_void) -> i32;
    fn AudioUnitSetProperty(unit: *mut c_void, property: u32, scope: u32, element: u32, data: *const c_void, size: u32) -> i32;
    fn AudioUnitGetProperty(unit: *mut c_void, property: u32, scope: u32, element: u32, data: *mut c_void, size: *mut u32) -> i32;
    fn AudioUnitInitialize(unit: *mut c_void) -> i32;
    fn AudioUnitUninitialize(unit: *mut c_void) -> i32;
    fn AudioOutputUnitStart(unit: *mut c_void) -> i32;
    fn AudioOutputUnitStop(unit: *mut c_void) -> i32;
}

struct RenderContext {
    mixer: DynamicMixer<f32>,
    muted: Arc<AtomicBool>,
}

/// The system asks for the next samples; silence once the mixer has none or
/// while muted during a paused seek.
unsafe extern "C" fn render(context: *mut c_void, _: *mut u32, _: *const c_void, _: u32, frames: u32, list: *mut BufferList) -> i32 {
    // SAFETY: `context` points to a heap-allocated `RenderContext` that stays
    // valid until `Output::drop` stops and disposes the unit, and the unit
    // invokes this callback from one render thread at a time.
    unsafe {
        if context.is_null() || list.is_null() || (*list).count == 0 {
            return 0;
        }
        let ctx = &mut *context.cast::<RenderContext>();
        let buffer = &(*list).buffers[0];
        if buffer.data.is_null() {
            return 0;
        }
        let count = (frames as usize * usize::from(CHANNELS)).min(buffer.bytes as usize / size_of::<f32>());
        let samples = std::slice::from_raw_parts_mut(buffer.data.cast::<f32>(), count);
        let muted = ctx.muted.load(Ordering::Acquire);
        for sample in samples {
            let next = ctx.mixer.next().unwrap_or(0.0);
            *sample = if muted { 0.0 } else { next };
        }
    }
    0
}

/// The iOS audio output. Owned by the audio thread (`enji-audio`) for its
/// lifetime; the `RemoteIO` unit stays initialized and is started/stopped with
/// playback so iOS sees the paused state cleanly.
pub struct Output {
    controller: Arc<DynamicMixerController<f32>>,
    unit: *mut c_void,
    context: *mut RenderContext,
    muted: Arc<AtomicBool>,
}

impl Output {
    pub fn open() -> Result<Output, String> {
        let (controller, source) = mixer::<f32>(CHANNELS, RATE);
        let muted = Arc::new(AtomicBool::new(false));
        let context = Box::into_raw(Box::new(RenderContext {
            mixer: source,
            muted: muted.clone(),
        }));
        let description = ComponentDescription {
            kind: OUTPUT,
            sub_kind: REMOTE_IO,
            manufacturer: APPLE,
            flags: 0,
            flags_mask: 0,
        };
        let bytes = u32::from(CHANNELS) * 4;
        let format = StreamDescription {
            sample_rate: f64::from(RATE),
            format: LINEAR_PCM,
            format_flags: FLOAT_PACKED,
            bytes_per_packet: bytes,
            frames_per_packet: 1,
            bytes_per_frame: bytes,
            channels: u32::from(CHANNELS),
            bits_per_channel: 32,
            reserved: 0,
        };
        let callback = RenderCallback {
            render,
            context: context.cast::<c_void>(),
        };
        // SAFETY: all AudioToolbox calls follow their C ABI contracts; on any
        // setup error we dispose the created unit (if any) and reclaim `context`.
        unsafe {
            let component = AudioComponentFindNext(ptr::null_mut(), &description);
            if component.is_null() {
                drop(Box::from_raw(context));
                return Err("没有音频输出组件".into());
            }
            let mut unit = ptr::null_mut();
            if let Err(err) = check(AudioComponentInstanceNew(component, &mut unit)) {
                drop(Box::from_raw(context));
                return Err(err);
            }
            let size = |bytes: usize| bytes as u32;
            let setup = check(AudioUnitSetProperty(
                unit,
                STREAM_FORMAT,
                INPUT_SCOPE,
                0,
                ptr::from_ref(&format).cast(),
                size(size_of::<StreamDescription>()),
            ))
            .and_then(|()| {
                check(AudioUnitSetProperty(
                    unit,
                    SET_RENDER_CALLBACK,
                    INPUT_SCOPE,
                    0,
                    ptr::from_ref(&callback).cast(),
                    size(size_of::<RenderCallback>()),
                ))
            })
            .and_then(|()| check(AudioUnitInitialize(unit)));
            if let Err(err) = setup {
                let _ = AudioComponentInstanceDispose(unit);
                drop(Box::from_raw(context));
                return Err(err);
            }
            Ok(Output {
                controller,
                unit,
                context,
                muted,
            })
        }
    }

    /// A sink playing through this output.
    pub fn sink(&mut self, _rate: u32) -> Result<Sink, String> {
        let (sink, queue) = Sink::new_idle();
        self.controller.add(queue);
        Ok(sink)
    }

    /// Queries the underlying `RemoteIO` unit rather than caching state, so
    /// system interruptions that stop the unit stay reflected accurately.
    fn is_running(&self) -> bool {
        let mut running: u32 = 0;
        let mut size = size_of::<u32>() as u32;
        // SAFETY: `self.unit` is valid until `Drop`; `running` and `size` match
        // the `UInt32` property `kAudioOutputUnitProperty_IsRunning`.
        let status = unsafe {
            AudioUnitGetProperty(
                self.unit,
                IS_RUNNING,
                GLOBAL_SCOPE,
                0,
                ptr::from_mut(&mut running).cast(),
                &mut size,
            )
        };
        status == 0 && running != 0
    }

    /// Ensures the playback `AVAudioSession` is active and starts the `RemoteIO`
    /// hardware unit so audio renders.
    pub fn wake(&self) -> bool {
        if self.is_running() {
            return true;
        }
        // SAFETY: `AVAudioSession` methods are thread-safe to configure before
        // starting the output unit, ensuring recovery after interruptions.
        unsafe {
            let session = AVAudioSession::sharedInstance();
            if let Some(category) = AVAudioSessionCategoryPlayback {
                let _ = session.setCategory_error(category);
            }
            let _ = session.setActive_error(true);
            AudioOutputUnitStart(self.unit) == 0
        }
    }

    /// Stops the `RemoteIO` hardware unit while keeping `AVAudioSession`
    /// active so Lock Screen / Dynamic Island controls show paused state.
    pub fn rest(&self) {
        if self.is_running() {
            // SAFETY: `self.unit` is valid until `Drop`.
            unsafe {
                let _ = AudioOutputUnitStop(self.unit);
            }
        }
    }

    /// Silences render output without stopping the unit (used while seeking a
    /// paused `Sink`).
    pub fn mute(&self, on: bool) {
        self.muted.store(on, Ordering::Release);
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        // SAFETY: stop and dispose the unit before freeing `self.context` so
        // no render callback can access freed memory.
        unsafe {
            let _ = AudioOutputUnitStop(self.unit);
            let _ = AudioUnitUninitialize(self.unit);
            let _ = AudioComponentInstanceDispose(self.unit);
            drop(Box::from_raw(self.context));
        }
    }
}

fn check(status: i32) -> Result<(), String> {
    if status == 0 {
        Ok(())
    } else {
        Err(format!("音频输出出错（{status}）"))
    }
}

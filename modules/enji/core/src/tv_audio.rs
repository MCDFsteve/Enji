//! The audio output of an Apple TV. The output rodio plays through has no
//! tvOS, so here the system's remote I/O unit asks for samples itself, from
//! the same mixer rodio plays through elsewhere.

use rodio::{
    dynamic_mixer::{mixer, DynamicMixer, DynamicMixerController},
    Sink,
};
use std::{ffi::c_void, ptr, sync::Arc};

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
const INPUT_SCOPE: u32 = 1;

#[link(name = "AudioToolbox", kind = "framework")]
unsafe extern "C" {
    fn AudioComponentFindNext(after: *mut c_void, description: *const ComponentDescription) -> *mut c_void;
    fn AudioComponentInstanceNew(component: *mut c_void, instance: *mut *mut c_void) -> i32;
    fn AudioUnitSetProperty(unit: *mut c_void, property: u32, scope: u32, element: u32, data: *const c_void, size: u32) -> i32;
    fn AudioUnitInitialize(unit: *mut c_void) -> i32;
    fn AudioOutputUnitStart(unit: *mut c_void) -> i32;
}

/// The system asks for the next samples; silence once the mixer has none.
unsafe extern "C" fn render(context: *mut c_void, _: *mut u32, _: *const c_void, _: u32, frames: u32, list: *mut BufferList) -> i32 {
    // SAFETY: `context` is the mixer `Output::open` leaked for the unit, and
    // the unit calls this from one thread at a time. `list` holds one buffer
    // of `frames` interleaved frames in the format the unit was given.
    unsafe {
        let mixer = &mut *context.cast::<DynamicMixer<f32>>();
        let buffer = &(*list).buffers[0];
        let samples = std::slice::from_raw_parts_mut(buffer.data.cast::<f32>(), frames as usize * usize::from(CHANNELS));
        for sample in samples {
            *sample = mixer.next().unwrap_or(0.0);
        }
    }
    0
}

/// The running output. It lives as long as the app, as the audio thread
/// that opens it does.
pub struct Output {
    controller: Arc<DynamicMixerController<f32>>,
}

impl Output {
    pub fn open() -> Result<Output, String> {
        let (controller, source) = mixer::<f32>(CHANNELS, RATE);
        let context = Box::into_raw(Box::new(source)).cast::<c_void>();
        let description = ComponentDescription { kind: OUTPUT, sub_kind: REMOTE_IO, manufacturer: APPLE, flags: 0, flags_mask: 0 };
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
        let callback = RenderCallback { render, context };
        // SAFETY: each call gets what its documentation asks for, and the
        // unit copies the format and the callback it is given.
        unsafe {
            let component = AudioComponentFindNext(ptr::null_mut(), &description);
            if component.is_null() {
                return Err("没有音频输出组件".into());
            }
            let mut unit = ptr::null_mut();
            check(AudioComponentInstanceNew(component, &mut unit))?;
            let size = |bytes: usize| bytes as u32;
            check(AudioUnitSetProperty(unit, STREAM_FORMAT, INPUT_SCOPE, 0, ptr::from_ref(&format).cast(), size(size_of::<StreamDescription>())))?;
            check(AudioUnitSetProperty(unit, SET_RENDER_CALLBACK, INPUT_SCOPE, 0, ptr::from_ref(&callback).cast(), size(size_of::<RenderCallback>())))?;
            check(AudioUnitInitialize(unit))?;
            check(AudioOutputUnitStart(unit))?;
        }
        Ok(Output { controller })
    }

    /// A sink playing through this output.
    pub fn sink(&self) -> Result<Sink, String> {
        let (sink, queue) = Sink::new_idle();
        self.controller.add(queue);
        Ok(sink)
    }
}

fn check(status: i32) -> Result<(), String> {
    if status == 0 { Ok(()) } else { Err(format!("音频输出出错（{status}）")) }
}

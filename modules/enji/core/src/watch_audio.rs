//! The audio output of an Apple Watch. watchOS ships `AVFAudio` rather than
//! `AudioToolbox`, so `AVAudioEngine` pulls samples through an
//! `AVAudioSourceNode` from the same `rodio` mixer used elsewhere.

use block2::RcBlock;
use objc2::{AnyThread, rc::Retained, runtime::Bool};
use objc2_avf_audio::{
    AVAudioEngine, AVAudioFormat, AVAudioFrameCount, AVAudioSession,
    AVAudioSessionCategoryPlayback, AVAudioSourceNode,
};
use objc2_core_audio_types::{AudioBuffer, AudioBufferList, AudioTimeStamp};
use rodio::{
    Sink,
    dynamic_mixer::{DynamicMixerController, mixer},
};
use std::{
    ptr::{self, NonNull},
    sync::{Arc, Mutex},
};

const CHANNELS: u16 = 2;
const RATE: u32 = 48_000;

/// The running output. It lives as long as the audio thread that opens it.
pub struct Output {
    controller: Arc<DynamicMixerController<f32>>,
    _engine: Retained<AVAudioEngine>,
    _source: Retained<AVAudioSourceNode>,
}

impl Output {
    pub fn open() -> Result<Output, String> {
        let (controller, source) = mixer::<f32>(CHANNELS, RATE);
        let mixer = Mutex::new(source);

        // Activate a direct playback session so Apple Watch plays through the
        // paired headphones or its built-in speaker without forcing a route
        // picker sheet before the first note.
        unsafe {
            let session = AVAudioSession::sharedInstance();
            if let Some(category) = AVAudioSessionCategoryPlayback {
                let _ = session.setCategory_error(category);
            }
            let _ = session.setActive_error(true);
        }

        let format = unsafe {
            AVAudioFormat::initStandardFormatWithSampleRate_channels(
                AVAudioFormat::alloc(),
                f64::from(RATE),
                u32::from(CHANNELS),
            )
        }
        .ok_or_else(|| String::from("没有可用的音频输出格式"))?;

        let render = RcBlock::new(
            move |is_silence: NonNull<Bool>,
                  _timestamp: NonNull<AudioTimeStamp>,
                  frame_count: AVAudioFrameCount,
                  output_data: NonNull<AudioBufferList>|
                  -> i32 {
                let frames = frame_count as usize;
                let mut silent = true;
                // SAFETY: `AVAudioSourceNode` passes a valid `AudioBufferList`
                // with `mNumberBuffers` initialized buffers sized for
                // `frame_count` frames, and a valid `is_silence` flag pointer.
                unsafe {
                    let list = output_data.as_ptr();
                    let count = (*list).mNumberBuffers as usize;
                    let buffers = std::slice::from_raw_parts_mut(
                        ptr::addr_of_mut!((*list).mBuffers).cast::<AudioBuffer>(),
                        count,
                    );
                    if count >= 2 && !buffers[0].mData.is_null() && !buffers[1].mData.is_null() {
                        let left =
                            std::slice::from_raw_parts_mut(buffers[0].mData.cast::<f32>(), frames);
                        let right =
                            std::slice::from_raw_parts_mut(buffers[1].mData.cast::<f32>(), frames);
                        if let Ok(mut mix) = mixer.try_lock() {
                            for i in 0..frames {
                                let l = mix.next().unwrap_or(0.0);
                                let r = mix.next().unwrap_or(0.0);
                                if l != 0.0 || r != 0.0 {
                                    silent = false;
                                }
                                left[i] = l;
                                right[i] = r;
                            }
                        } else {
                            left.fill(0.0);
                            right.fill(0.0);
                        }
                        for extra in &mut buffers[2..] {
                            if !extra.mData.is_null() {
                                let samples = extra.mDataByteSize as usize / size_of::<f32>();
                                std::slice::from_raw_parts_mut(extra.mData.cast::<f32>(), samples)
                                    .fill(0.0);
                            }
                        }
                    } else if count == 1 && !buffers[0].mData.is_null() {
                        let ch = buffers[0].mNumberChannels.max(1) as usize;
                        let out = std::slice::from_raw_parts_mut(
                            buffers[0].mData.cast::<f32>(),
                            frames * ch,
                        );
                        if let Ok(mut mix) = mixer.try_lock() {
                            for frame in out.chunks_exact_mut(ch) {
                                let l = mix.next().unwrap_or(0.0);
                                let r = mix.next().unwrap_or(0.0);
                                if l != 0.0 || r != 0.0 {
                                    silent = false;
                                }
                                if ch >= 2 {
                                    frame[0] = l;
                                    frame[1] = r;
                                    frame[2..].fill(0.0);
                                } else {
                                    frame[0] = (l + r) * 0.5;
                                }
                            }
                        } else {
                            out.fill(0.0);
                        }
                    }
                    *is_silence.as_ptr() = Bool::new(silent);
                }
                0
            },
        );

        let source_node = unsafe {
            AVAudioSourceNode::initWithFormat_renderBlock(
                AVAudioSourceNode::alloc(),
                &format,
                ptr::from_ref(&*render).cast_mut(),
            )
        };

        let engine = unsafe { AVAudioEngine::new() };
        unsafe {
            engine.attachNode(&source_node);
            let main_mixer = engine.mainMixerNode();
            engine.connect_to_format(&source_node, &main_mixer, Some(&format));
            engine
                .startAndReturnError()
                .map_err(|err| format!("音频输出出错（{err}）"))?;
        }

        Ok(Output {
            controller,
            _engine: engine,
            _source: source_node,
        })
    }

    /// A sink playing through this output.
    pub fn sink(&mut self, _rate: u32) -> Result<Sink, String> {
        let (sink, queue) = Sink::new_idle();
        self.controller.add(queue);
        Ok(sink)
    }
}

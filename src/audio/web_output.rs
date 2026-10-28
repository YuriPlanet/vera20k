//! Browser audio output: one `AudioContext` fed from one rodio mixer that the
//! music and SFX players share.
//!
//! This replaces CPAL 0.17's WebAudio host (`host/webaudio/mod.rs`), which
//! glitches on a busy main thread. That host keeps only two buffers in
//! flight and refills each one from the previous buffer's `ended` event. With
//! the SFX player's ~10 ms period, any main-thread task longer than about one
//! period starves the device. Its schedule cursor also never resyncs after a
//! stall, so the late buffers start together and play on top of each other.
//!
//! Here blocks stay short, so the mixer still admits a new Player every
//! [`BLOCK_FRAMES`] and the `Limit=1` cues that `sfx_buffer_frames` protects
//! still render before the next pump stops them. Several blocks are queued
//! ahead to ride out long frames. A cursor that falls behind the context
//! clock restarts just ahead of it: a stall leaves a gap, never an overlap.

use std::cell::RefCell;
use std::num::NonZero;

use rodio::mixer::{Mixer, MixerSource};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{AudioBuffer, AudioContext, AudioScheduledSourceNode};

/// Frames per scheduled block, about 11 ms at 44.1/48 kHz.
const BLOCK_FRAMES: usize = 512;
/// Blocks queued ahead of the context clock, about 64 ms of main-thread
/// stall the output survives without a gap.
const BLOCKS_AHEAD: usize = 6;
/// Ring of reusable buffers. More than [`BLOCKS_AHEAD`] + 1, so a buffer is
/// rewritten only after its node has finished playing it.
const BUFFER_RING: usize = BLOCKS_AHEAD + 2;
/// How far ahead of the clock a restarted schedule begins.
const RESTART_MARGIN_SECS: f64 = 0.01;
const CHANNELS: usize = 2;

thread_local! {
    static OUTPUT: RefCell<Option<WebOutput>> = const { RefCell::new(None) };
}

/// Handle to the shared browser output. The output lives for the page, so
/// dropping a handle does not stop playback.
pub struct WebAudioSink {
    mixer: Mixer,
}

impl WebAudioSink {
    /// Open the shared output on first use. Call from a user gesture so the
    /// browser lets the context run.
    pub fn open() -> Result<Self, JsValue> {
        OUTPUT.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot.is_none() {
                let mut output = WebOutput::new()?;
                output.top_up();
                *slot = Some(output);
            }
            let mixer = slot.as_ref().expect("output opened above").mixer.clone();
            Ok(Self { mixer })
        })
    }

    pub fn mixer(&self) -> &Mixer {
        &self.mixer
    }
}

struct WebOutput {
    ctx: AudioContext,
    mixer: Mixer,
    source: MixerSource,
    buffers: Vec<AudioBuffer>,
    next_buffer: usize,
    /// Context time at which the next block starts.
    next_start: f64,
    block_secs: f64,
    interleaved: Vec<f32>,
    planar: Vec<f32>,
    on_ended: Closure<dyn FnMut()>,
}

impl WebOutput {
    fn new() -> Result<Self, JsValue> {
        let ctx = AudioContext::new()?;
        let sample_rate = ctx.sample_rate();
        let rate = NonZero::new(sample_rate as u32)
            .ok_or_else(|| JsValue::from_str("AudioContext reports a zero sample rate"))?;
        let (mixer, source) = rodio::mixer::mixer(NonZero::new(CHANNELS as u16).unwrap(), rate);
        let buffers = (0..BUFFER_RING)
            .map(|_| ctx.create_buffer(CHANNELS as u32, BLOCK_FRAMES as u32, sample_rate))
            .collect::<Result<Vec<_>, _>>()?;
        // The gesture that creates the context also lets it start; resume in
        // case the browser created it suspended anyway.
        let _ = ctx.resume();
        log::info!(
            "Browser audio output: {BLOCK_FRAMES}-frame blocks, {BLOCKS_AHEAD} ahead at {sample_rate} Hz"
        );
        Ok(Self {
            ctx,
            mixer,
            source,
            buffers,
            next_buffer: 0,
            next_start: 0.0,
            block_secs: BLOCK_FRAMES as f64 / f64::from(sample_rate),
            interleaved: vec![0.0; BLOCK_FRAMES * CHANNELS],
            planar: vec![0.0; BLOCK_FRAMES],
            on_ended: Closure::new(|| {
                OUTPUT.with(|slot| {
                    if let Some(output) = slot.borrow_mut().as_mut() {
                        output.top_up();
                    }
                });
            }),
        })
    }

    /// Schedule blocks until [`BLOCKS_AHEAD`] are queued past the clock.
    fn top_up(&mut self) {
        let now = self.ctx.current_time();
        if self.next_start < now {
            self.next_start = now + RESTART_MARGIN_SECS;
        }
        let horizon = now + BLOCKS_AHEAD as f64 * self.block_secs;
        while self.next_start < horizon {
            if let Err(error) = self.schedule_block() {
                log::error!("Browser audio block failed: {error:?}");
                return;
            }
            self.next_start += self.block_secs;
        }
    }

    fn schedule_block(&mut self) -> Result<(), JsValue> {
        for sample in &mut self.interleaved {
            *sample = self.source.next().unwrap_or(0.0);
        }
        let buffer = &self.buffers[self.next_buffer];
        self.next_buffer = (self.next_buffer + 1) % BUFFER_RING;
        for channel in 0..CHANNELS {
            for (frame, sample) in self.planar.iter_mut().enumerate() {
                *sample = self.interleaved[frame * CHANNELS + channel];
            }
            buffer.copy_to_channel(&self.planar, channel as i32)?;
        }
        let node = self.ctx.create_buffer_source()?;
        node.set_buffer(Some(buffer));
        node.connect_with_audio_node(&self.ctx.destination())?;
        AudioScheduledSourceNode::set_onended(&node, Some(self.on_ended.as_ref().unchecked_ref()));
        node.start_with_when(self.next_start)
    }
}

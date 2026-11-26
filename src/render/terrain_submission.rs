//! Bounded command submission for native destination edits and their fences.
//!
//! wgpu-core 27.0.3 encodes each render pass plus its resource transitions in
//! separate HAL command buffers (`command/render.rs:1875,2262`). The pinned
//! Metal backend allows 4096 outstanding buffers (`metal/adapter.rs:30`) and
//! blocks when that pool is exhausted. Finishing a large unsubmitted frame can
//! therefore deadlock before the application reaches `Queue::submit`.
//!
//! Submit only at closed-pass boundaries. The same queue preserves order and
//! subsequent passes load the same attachments, so this does not change any
//! pixel, depth, object order or presentation boundary. See
//! <https://github.com/gfx-rs/wgpu/issues/5738> and
//! <https://github.com/gfx-rs/wgpu/pull/7858> for the backend constraints.
//!
//! wgpu-core 30 records passes and encodes them at `CommandEncoder::finish`
//! (`command/mod.rs` `encode_commands`), so each submission here also carries
//! the driver encoding of the passes before it. The performance panel counts that
//! time as handing the frame to the driver, not as recording draw calls.

use crate::util::time::{Duration, Instant};

/// Keep substantial headroom for wgpu's internal buffers and the fixed frame
/// passes outside native-object replay. This is a scheduling bound, not a
/// claim that the backend exposes a portable command-buffer limit.
/// Closed passes per command buffer on a GPU with its own memory. Windows
/// judges each command buffer on its own against its two-second timeout, so
/// a frame that takes longer survives as long as no one buffer does.
pub(crate) const CLOSED_PASSES_PER_SUBMISSION: usize = 128;
/// The same on a GPU that draws from the system's memory
/// (`gpu::shared_memory_graphics`): its passes take many times longer, and an
/// Intel HD 620 lost its device to that timeout seconds into every match. A
/// frame of short buffers stays slow, but the game keeps its device and the
/// log keeps telling why the frame is slow.
pub(crate) const SHARED_MEMORY_PASSES_PER_SUBMISSION: usize = 8;

pub(super) struct PassSubmission {
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// Marks each split-off command buffer's span for the GPU time of the
    /// frame; times nothing until the frame owner hands it the GPU's timer.
    pub(super) timer: crate::render::gpu_timer::GpuTimer,
    pending_passes: usize,
    /// Closed passes a command buffer holds before it is submitted
    /// ([`CLOSED_PASSES_PER_SUBMISSION`] unless the device is told otherwise).
    passes_per_submission: usize,
    submissions: usize,
    /// Wall time spent finishing and submitting this frame's bounded batches.
    submit_time: Duration,
    /// Every pass closed this frame: replay pass-count tests read it, and the
    /// frame owner logs it (`presentation::render::FramePassLog`), since the
    /// pass count is what a weak GPU pays for most.
    closed_passes: usize,
}

impl PassSubmission {
    pub(super) fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        Self {
            device: device.clone(),
            queue: queue.clone(),
            timer: Default::default(),
            pending_passes: 0,
            passes_per_submission: CLOSED_PASSES_PER_SUBMISSION,
            submissions: 0,
            submit_time: Duration::ZERO,
            closed_passes: 0,
        }
    }

    /// How many closed passes a command buffer holds before it is submitted.
    pub(super) fn set_passes_per_submission(&mut self, passes: usize) {
        self.passes_per_submission = passes.max(1);
    }

    /// Called by the existing per-frame target preparation owner. Previously
    /// submitted commands retain their own resources until the queue retires
    /// them; only these observation/scheduling counters reset here.
    pub(super) fn reset_frame(&mut self) {
        self.pending_passes = 0;
        self.submissions = 0;
        self.submit_time = Duration::ZERO;
        self.closed_passes = 0;
    }

    /// Wall time this frame's bounded submissions took so far.
    pub(super) fn submit_time(&self) -> Duration {
        self.submit_time
    }

    pub(super) fn submissions(&self) -> usize {
        self.submissions
    }

    pub(super) fn closed_passes(&self) -> usize {
        self.closed_passes
    }

    /// The caller has ended every pass counted here. Account for each small
    /// replay group immediately, rather than after an unbounded list of waves.
    pub(super) fn note_closed_passes(&mut self, encoder: &mut wgpu::CommandEncoder, passes: usize) {
        self.pending_passes += passes;
        self.closed_passes += passes;
        if self.pending_passes < self.passes_per_submission {
            return;
        }
        let next = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Native object replay continuation"),
            });
        let mut completed = std::mem::replace(encoder, next);
        self.timer.end(&mut completed);
        self.timer.begin(encoder);
        let started = Instant::now();
        self.queue.submit([completed.finish()]);
        self.submit_time += started.elapsed();
        self.pending_passes = 0;
        self.submissions += 1;
    }
}

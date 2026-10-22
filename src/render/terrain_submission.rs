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

/// Keep substantial headroom for wgpu's internal buffers and the fixed frame
/// passes outside native-object replay. This is a scheduling bound, not a
/// claim that the backend exposes a portable command-buffer limit.
const CLOSED_PASSES_PER_SUBMISSION: usize = 128;

pub(super) struct PassSubmission {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pending_passes: usize,
    submissions: usize,
    /// Every pass closed this frame, for replay pass-count tests.
    #[cfg(test)]
    closed_passes: usize,
}

impl PassSubmission {
    pub(super) fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        Self {
            device: device.clone(),
            queue: queue.clone(),
            pending_passes: 0,
            submissions: 0,
            #[cfg(test)]
            closed_passes: 0,
        }
    }

    /// Called by the existing per-frame target preparation owner. Previously
    /// submitted commands retain their own resources until the queue retires
    /// them; only these observation/scheduling counters reset here.
    pub(super) fn reset_frame(&mut self) {
        self.pending_passes = 0;
        self.submissions = 0;
        #[cfg(test)]
        {
            self.closed_passes = 0;
        }
    }

    #[cfg(test)]
    pub(super) fn submissions(&self) -> usize {
        self.submissions
    }

    #[cfg(test)]
    pub(super) fn closed_passes(&self) -> usize {
        self.closed_passes
    }

    /// The caller has ended every pass counted here. Account for each small
    /// replay group immediately, rather than after an unbounded list of waves.
    pub(super) fn note_closed_passes(&mut self, encoder: &mut wgpu::CommandEncoder, passes: usize) {
        self.pending_passes += passes;
        #[cfg(test)]
        {
            self.closed_passes += passes;
        }
        if self.pending_passes < CLOSED_PASSES_PER_SUBMISSION {
            return;
        }
        let next = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Native object replay continuation"),
            });
        let completed = std::mem::replace(encoder, next);
        self.queue.submit([completed.finish()]);
        self.pending_passes = 0;
        self.submissions += 1;
    }
}

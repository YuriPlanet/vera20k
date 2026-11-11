//! The GPU time of VERA's own frames, for the Performance Stats panel's GPU
//! row: how long the GPU worked on each frame, and so its share of the time
//! between frames. Other programs' GPU work is not in it.
//!
//! Timestamp queries ([`wgpu::Features::TIMESTAMP_QUERY`]) mark the start and
//! the end of each command buffer a frame submits: the frame's own and each
//! one [`PassSubmission`](super::terrain_draw) splits off. A frame's GPU time
//! is the sum of those spans, so a GPU that waits for the CPU between two of
//! them does not count the wait. The spans leave out wgpu's staged buffer and
//! texture writes, which run just before each buffer, and the few buffers
//! submitted outside a frame's encoders, such as loading repaints.
//!
//! Apple GPUs take timestamps only at pass boundaries, and Metal writes none
//! for a pass with no work (wgpu-hal 30.0.1 `metal/command.rs`, `enter_blit`;
//! <https://github.com/gpuweb/gpuweb/issues/2046#issuecomment-1205793680>).
//! Each mark is therefore a compute pass of one invocation that stores a word,
//! with the timestamp at its end. A frame's timestamps resolve and map back
//! after the GPU finishes it; [`SLOTS`] frames can be on their way at once, and
//! a frame that finds no slot free goes untimed. Where the adapter has no
//! timestamp queries or compute shaders (WebGL2), the timer does nothing and
//! the panel leaves out its GPU row.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// Frames whose timestamps can be on their way back at once.
const SLOTS: usize = 6;
/// Timestamps per frame: a start and an end for each of up to 128 command
/// buffers, many more than `PassSubmission` splits a large battle frame into.
const QUERIES_PER_SLOT: u32 = 256;
/// One frame's resolved timestamps, in bytes. A multiple of
/// `wgpu::QUERY_RESOLVE_BUFFER_ALIGNMENT`, so each slot's offset is one too.
const SLOT_BYTES: u64 = QUERIES_PER_SLOT as u64 * wgpu::QUERY_SIZE as u64;
/// A span longer than this comes from a misread timestamp and drops its frame.
const LONGEST_SPAN_NS: f64 = 1e9;

const MARK_SHADER: &str = "
@group(0) @binding(0) var<storage, read_write> mark: u32;

@compute @workgroup_size(1)
fn main() {
    mark = 1u;
}
";

/// The readback states of a slot's mapping.
const MAPPING: u8 = 0;
const MAPPED: u8 = 1;
const FAILED: u8 = 2;

/// Shared by the frame owner and the terrain renderer's split submissions;
/// the default times nothing.
#[derive(Clone, Default)]
pub(crate) struct GpuTimer(Option<Arc<Timer>>);

struct Timer {
    /// Nanoseconds per timestamp tick.
    period_ns: f64,
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: Vec<wgpu::Buffer>,
    mark: wgpu::ComputePipeline,
    mark_group: wgpu::BindGroup,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    active: bool,
    /// The frame being recorded, from `begin_frame` to `end_frame`.
    frame: Option<OpenFrame>,
    slots: [Slot; SLOTS],
    /// Frames read back since the panel last took them, and their GPU time.
    frames: u32,
    busy_ns: f64,
}

struct OpenFrame {
    slot: usize,
    /// Timestamps written so far.
    used: u32,
    /// Whether the last timestamp started a span.
    open: bool,
    /// The frame ran out of timestamps, so its spans would undercount.
    overflow: bool,
}

#[derive(Default)]
enum Slot {
    #[default]
    Free,
    Recording,
    Reading {
        used: u32,
        mapped: Arc<AtomicU8>,
    },
}

impl GpuTimer {
    /// The features `GpuContext` requests for the timer: timestamp queries
    /// where the adapter has them and the compute shaders that mark them.
    pub(crate) fn features(adapter: &wgpu::Adapter) -> wgpu::Features {
        let compute = adapter
            .get_downlevel_capabilities()
            .flags
            .contains(wgpu::DownlevelFlags::COMPUTE_SHADERS);
        if compute {
            adapter.features() & wgpu::Features::TIMESTAMP_QUERY
        } else {
            wgpu::Features::empty()
        }
    }

    /// A timer for `device`, which does nothing unless the device was made
    /// with [`GpuTimer::features`].
    pub(crate) fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return Self::default();
        }
        let queries = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("GPU timer"),
            ty: wgpu::QueryType::Timestamp,
            count: SLOTS as u32 * QUERIES_PER_SLOT,
        });
        let resolve = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("GPU timer resolve"),
            size: SLOTS as u64 * SLOT_BYTES,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = (0..SLOTS)
            .map(|_| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("GPU timer readback"),
                    size: SLOT_BYTES,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            })
            .collect();
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("GPU timer mark"),
            source: wgpu::ShaderSource::Wgsl(MARK_SHADER.into()),
        });
        let mark = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("GPU timer mark"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let word = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("GPU timer mark word"),
            size: 4,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let mark_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("GPU timer mark"),
            layout: &mark.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: word.as_entire_binding(),
            }],
        });
        Self(Some(Arc::new(Timer {
            period_ns: f64::from(queue.get_timestamp_period()),
            queries,
            resolve,
            readback,
            mark,
            mark_group,
            state: Mutex::new(State::default()),
        })))
    }

    /// Time frames while the panel shows. Hiding it drops what was measured,
    /// so showing it again starts afresh.
    pub(crate) fn set_active(&self, active: bool) {
        let Some(timer) = &self.0 else {
            return;
        };
        let mut guard = timer.lock();
        let state = &mut *guard;
        if state.active == active {
            return;
        }
        // Frames that came back while hidden are not counted.
        timer.collect(state);
        state.active = active;
        state.drop_frame();
        state.frames = 0;
        state.busy_ns = 0.0;
    }

    /// Start the frame's first span at the top of `encoder`, its first
    /// encoder, before any pass.
    pub(crate) fn begin_frame(&self, encoder: &mut wgpu::CommandEncoder) {
        let Some(timer) = &self.0 else {
            return;
        };
        let mut guard = timer.lock();
        let state = &mut *guard;
        // A frame dropped before its submission leaves its slot behind.
        state.drop_frame();
        if !state.active {
            return;
        }
        timer.collect(state);
        let Some(slot) = state
            .slots
            .iter()
            .position(|slot| matches!(slot, Slot::Free))
        else {
            return;
        };
        state.slots[slot] = Slot::Recording;
        state.frame = Some(OpenFrame {
            slot,
            used: 0,
            open: false,
            overflow: false,
        });
        timer.begin(state, encoder);
    }

    /// Start a span at the top of the frame's next encoder, after an earlier
    /// one was split off and submitted.
    pub(crate) fn begin(&self, encoder: &mut wgpu::CommandEncoder) {
        if let Some(timer) = &self.0 {
            timer.begin(&mut timer.lock(), encoder);
        }
    }

    /// End the span of `encoder`, just before it is finished and submitted.
    pub(crate) fn end(&self, encoder: &mut wgpu::CommandEncoder) {
        if let Some(timer) = &self.0 {
            timer.end(&mut timer.lock(), encoder);
        }
    }

    /// End the frame's last span in `encoder`, its last encoder, and read the
    /// frame's timestamps back once the GPU has run it. Call just before the
    /// submission, since the readback starts with it.
    pub(crate) fn end_frame(&self, encoder: &mut wgpu::CommandEncoder) {
        let Some(timer) = &self.0 else {
            return;
        };
        let mut guard = timer.lock();
        let state = &mut *guard;
        timer.end(state, encoder);
        let Some(frame) = state.frame.take() else {
            return;
        };
        if frame.overflow || frame.used == 0 {
            state.slots[frame.slot] = Slot::Free;
            return;
        }
        let first = frame.slot as u32 * QUERIES_PER_SLOT;
        let offset = frame.slot as u64 * SLOT_BYTES;
        let bytes = u64::from(frame.used) * u64::from(wgpu::QUERY_SIZE);
        let readback = &timer.readback[frame.slot];
        encoder.resolve_query_set(
            &timer.queries,
            first..first + frame.used,
            &timer.resolve,
            offset,
        );
        encoder.copy_buffer_to_buffer(&timer.resolve, offset, readback, 0, bytes);
        let mapped = Arc::new(AtomicU8::new(MAPPING));
        let flag = Arc::clone(&mapped);
        encoder.map_buffer_on_submit(readback, wgpu::MapMode::Read, 0..bytes, move |result| {
            let state = if result.is_ok() { MAPPED } else { FAILED };
            flag.store(state, Ordering::Release);
        });
        state.slots[frame.slot] = Slot::Reading {
            used: frame.used,
            mapped,
        };
    }

    /// The mean GPU time of the frames read back since the last call, in
    /// milliseconds. `None` while none came back, and always where the GPU
    /// cannot be timed.
    pub(crate) fn take_frame_ms(&self) -> Option<f32> {
        let timer = self.0.as_ref()?;
        let mut guard = timer.lock();
        let state = &mut *guard;
        timer.collect(state);
        let frames = std::mem::take(&mut state.frames);
        let busy_ns = std::mem::take(&mut state.busy_ns);
        (frames > 0).then(|| (busy_ns / f64::from(frames) / 1e6) as f32)
    }
}

impl Timer {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn begin(&self, state: &mut State, encoder: &mut wgpu::CommandEncoder) {
        let Some(frame) = state.frame.as_mut() else {
            return;
        };
        if frame.open || frame.overflow {
            return;
        }
        if frame.used + 2 > QUERIES_PER_SLOT {
            frame.overflow = true;
            return;
        }
        self.mark(encoder, frame.slot as u32 * QUERIES_PER_SLOT + frame.used);
        frame.used += 1;
        frame.open = true;
    }

    fn end(&self, state: &mut State, encoder: &mut wgpu::CommandEncoder) {
        let Some(frame) = state.frame.as_mut().filter(|frame| frame.open) else {
            return;
        };
        self.mark(encoder, frame.slot as u32 * QUERIES_PER_SLOT + frame.used);
        frame.used += 1;
        frame.open = false;
    }

    /// A pass that does a little work, so the GPU writes timestamp `index`
    /// once everything before it has run.
    fn mark(&self, encoder: &mut wgpu::CommandEncoder, index: u32) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("GPU timer mark"),
            timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
                query_set: &self.queries,
                beginning_of_pass_write_index: None,
                end_of_pass_write_index: Some(index),
            }),
        });
        pass.set_pipeline(&self.mark);
        pass.set_bind_group(0, &self.mark_group, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }

    /// Add up the frames whose timestamps have mapped back, while timing,
    /// and free their slots.
    fn collect(&self, state: &mut State) {
        for (index, slot) in state.slots.iter_mut().enumerate() {
            let Slot::Reading { used, mapped } = slot else {
                continue;
            };
            match mapped.load(Ordering::Acquire) {
                MAPPING => continue,
                MAPPED => {
                    let buffer = &self.readback[index];
                    let bytes = u64::from(*used) * u64::from(wgpu::QUERY_SIZE);
                    let busy = buffer
                        .get_mapped_range(..bytes)
                        .ok()
                        .and_then(|stamps| frame_busy_ns(&stamps, self.period_ns));
                    buffer.unmap();
                    if let Some(busy) = busy.filter(|_| state.active) {
                        state.frames += 1;
                        state.busy_ns += busy;
                    }
                }
                _ => {}
            }
            *slot = Slot::Free;
        }
    }
}

impl State {
    /// Forget a frame that has not reached its readback.
    fn drop_frame(&mut self) {
        if let Some(frame) = self.frame.take() {
            self.slots[frame.slot] = Slot::Free;
        }
    }
}

/// The sum of a frame's start-to-end spans in `stamps`, its resolved
/// timestamps, in nanoseconds. `None` when a timestamp is unwritten or
/// misread: zero, Metal's all-ones error value, or a span running backwards
/// or longer than a second.
fn frame_busy_ns(stamps: &[u8], period_ns: f64) -> Option<f64> {
    let mut busy = 0.0;
    for span in stamps.chunks_exact(16) {
        let (start, end) = span.split_at(8);
        let start = u64::from_ne_bytes(start.try_into().ok()?);
        let end = u64::from_ne_bytes(end.try_into().ok()?);
        let ns = end.checked_sub(start)? as f64 * period_ns;
        if start == 0 || ns > LONGEST_SPAN_NS {
            return None;
        }
        busy += ns;
    }
    Some(busy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::gpu::block_on;

    fn bytes(stamps: &[u64]) -> Vec<u8> {
        stamps
            .iter()
            .flat_map(|stamp| stamp.to_ne_bytes())
            .collect()
    }

    #[test]
    fn a_frame_sums_its_spans_and_drops_a_misread_timestamp() {
        // Two command buffers of 2 ms and 1 ms; the 5 ms between them, while
        // the GPU waited for the second, is not GPU time.
        let split = bytes(&[1_000_000, 3_000_000, 8_000_000, 9_000_000]);
        assert_eq!(frame_busy_ns(&split, 1.0), Some(3_000_000.0));
        assert_eq!(frame_busy_ns(&bytes(&[10, 20]), 2.5), Some(25.0));
        for misread in [[20, 10], [0, 10], [10, u64::MAX]] {
            assert_eq!(frame_busy_ns(&bytes(&misread), 1.0), None, "{misread:?}");
        }
    }

    #[test]
    #[ignore = "requires a wgpu adapter with timestamp queries; run explicitly"]
    fn a_split_frame_reads_back_its_gpu_time() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: None,
            ..Default::default()
        }))
        .expect("the GPU timer test requires a wgpu adapter");
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("GPU timer test device"),
            required_features: GpuTimer::features(&adapter),
            ..Default::default()
        }))
        .expect("request the GPU timer test device");
        let timer = GpuTimer::new(&device, &queue);
        assert!(
            timer.0.is_some(),
            "{:?} has no timestamp queries",
            adapter.get_info()
        );
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("GPU timer test target"),
            size: wgpu::Extent3d {
                width: 512,
                height: 512,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let clear = |encoder: &mut wgpu::CommandEncoder| {
            let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("GPU timer test clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLUE),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        };
        // A frame drawn before the panel shows goes untimed.
        let mut hidden = device.create_command_encoder(&Default::default());
        timer.begin_frame(&mut hidden);
        clear(&mut hidden);
        timer.end_frame(&mut hidden);
        queue.submit([hidden.finish()]);
        timer.set_active(true);
        for _ in 0..3 {
            // A frame that `PassSubmission` split in two.
            let mut first = device.create_command_encoder(&Default::default());
            timer.begin_frame(&mut first);
            clear(&mut first);
            timer.end(&mut first);
            queue.submit([first.finish()]);
            let mut last = device.create_command_encoder(&Default::default());
            timer.begin(&mut last);
            clear(&mut last);
            timer.end_frame(&mut last);
            queue.submit([last.finish()]);
        }
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the GPU runs the frames");
        let ms = timer
            .take_frame_ms()
            .expect("the frames' timestamps come back");
        eprintln!(
            "GPU timer test: {ms} ms per frame on {:?}",
            adapter.get_info()
        );
        assert!(ms > 0.0 && ms < 100.0, "{ms} ms");
        assert_eq!(timer.take_frame_ms(), None, "each frame counts once");
    }
}

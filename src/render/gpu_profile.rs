//! Optional, nonblocking queue-timeline sampling for hardware diagnostics.
//! A span may include GPU idle gaps between this frame's internal submissions;
//! it is not a hardware utilization counter or a sum of shader execution time.

use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

const SAMPLE_EVERY: u64 = 4;
pub(super) const FEATURES: wgpu::Features =
    wgpu::Features::TIMESTAMP_QUERY.union(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS);

struct Slot {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
    pending: Option<(Receiver<Result<(), wgpu::BufferAsyncError>>, bool)>,
}

pub(crate) struct GpuProfile {
    slots: Vec<Slot>,
    frame: u64,
    period_ns: f64,
    samples_ms: Vec<f64>,
    skipped: u64,
    errors: u64,
    next_report: Instant,
}

impl GpuProfile {
    pub(super) fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let enabled = device.features().contains(FEATURES);
        log::info!(target: "switch::perf",
            "GPU timestamp sampling enabled={enabled} period_ns={} every={SAMPLE_EVERY} frames; scope=queue_timeline includes_submission_gaps=true",
            queue.get_timestamp_period());
        let slots = if enabled {
            (0..3)
                .map(|_| Slot {
                    queries: device.create_query_set(&wgpu::QuerySetDescriptor {
                        label: Some("Hardware frame timestamps"),
                        ty: wgpu::QueryType::Timestamp,
                        count: 2,
                    }),
                    resolve: device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("Hardware timestamp resolve"),
                        size: 16,
                        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                        mapped_at_creation: false,
                    }),
                    readback: device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("Hardware timestamp readback"),
                        size: 16,
                        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                        mapped_at_creation: false,
                    }),
                    pending: None,
                })
                .collect()
        } else {
            Vec::new()
        };
        Self {
            slots,
            frame: 0,
            period_ns: queue.get_timestamp_period() as f64,
            samples_ms: Vec::new(),
            skipped: 0,
            errors: 0,
            next_report: Instant::now() + Duration::from_secs(10),
        }
    }

    pub(crate) fn begin(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        active: bool,
    ) -> Option<usize> {
        if self.slots.is_empty() {
            return None;
        }
        if !active {
            self.samples_ms.clear();
            self.skipped = 0;
            self.errors = 0;
            self.frame = 0;
            self.next_report = Instant::now() + Duration::from_secs(10);
            for slot in &mut self.slots {
                if let Some((_, collect)) = &mut slot.pending {
                    *collect = false;
                }
            }
        }
        // Poll never waits for a GPU submission. Full slots drop a sample.
        if self.slots.iter().any(|slot| slot.pending.is_some()) {
            if let Err(error) = device.poll(wgpu::PollType::Poll) {
                log::warn!(target: "switch::perf", "GPU timestamp poll: {error}");
                self.errors += 1;
            }
        }
        for slot in &mut self.slots {
            let Some((receiver, collect)) = &slot.pending else {
                continue;
            };
            match receiver.try_recv() {
                Ok(Ok(())) => {
                    let data = slot.readback.slice(..).get_mapped_range();
                    let start = u64::from_ne_bytes(data[..8].try_into().unwrap());
                    let end = u64::from_ne_bytes(data[8..16].try_into().unwrap());
                    if *collect {
                        if start != 0 && end > start {
                            self.samples_ms
                                .push((end - start) as f64 * self.period_ns / 1_000_000.0);
                        } else {
                            self.errors += 1;
                        }
                    }
                    drop(data);
                    slot.readback.unmap();
                    slot.pending = None;
                }
                Ok(Err(_)) | Err(mpsc::TryRecvError::Disconnected) => {
                    self.errors += 1;
                    slot.pending = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if !active {
            return None;
        }
        let now = Instant::now();
        if now >= self.next_report {
            self.samples_ms.sort_by(f64::total_cmp);
            let n = self.samples_ms.len();
            let mean = self.samples_ms.iter().sum::<f64>() / n.max(1) as f64;
            let percentile = |fraction: f64| {
                self.samples_ms
                    .get(((n as f64 * fraction).ceil() as usize).saturating_sub(1))
                    .copied()
                    .unwrap_or(0.0)
            };
            log::info!(target: "switch::perf",
                "GPU queue span samples={n} mean_ms={mean:.3} p50_ms={:.3} p95_ms={:.3} max_ms={:.3} skipped={} errors={}; scene=battlefield includes_submission_gaps=true",
                percentile(0.5), percentile(0.95), self.samples_ms.last().copied().unwrap_or(0.0),
                self.skipped, self.errors);
            self.samples_ms.clear();
            self.skipped = 0;
            self.errors = 0;
            self.next_report = now + Duration::from_secs(10);
        }
        self.frame += 1;
        if self.frame % SAMPLE_EVERY != 0 {
            return None;
        }
        let Some(index) = self.slots.iter().position(|slot| slot.pending.is_none()) else {
            self.skipped += 1;
            return None;
        };
        encoder.write_timestamp(&self.slots[index].queries, 0);
        Some(index)
    }

    pub(crate) fn end(&mut self, encoder: &mut wgpu::CommandEncoder, index: usize) {
        let slot = &mut self.slots[index];
        encoder.write_timestamp(&slot.queries, 1);
        encoder.resolve_query_set(&slot.queries, 0..2, &slot.resolve, 0);
        encoder.copy_buffer_to_buffer(&slot.resolve, 0, &slot.readback, 0, 16);
        let (send, receive) = mpsc::channel();
        encoder.map_buffer_on_submit(&slot.readback, wgpu::MapMode::Read, .., move |result| {
            let _ = send.send(result);
        });
        slot.pending = Some((receive, true));
    }
}

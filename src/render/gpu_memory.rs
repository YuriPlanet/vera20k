//! Horizon's shared CPU/GPU heap and live wgpu resource accounting.

use std::sync::mpsc::{self, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

#[repr(C)]
#[derive(Default)]
struct MemorySnapshot {
    process_total: u64,
    process_used: u64,
    heap_arena: u64,
    heap_used: u64,
    heap_free: u64,
    heap_capacity: u64,
    heap_top_free: u64,
    region_reserved: u64,
    region_used: u64,
    region_peak: u64,
    region_fallbacks: u64,
    total_result: u32,
    used_result: u32,
}

unsafe extern "C" {
    fn platform_memory_snapshot(snapshot: *mut MemorySnapshot);
    #[cfg(feature = "horizon-frame-profile")]
    fn platform_perf_poll();
    #[cfg(feature = "horizon-frame-profile")]
    fn platform_perf_thread_name(name: *const std::ffi::c_char);
}

pub(super) struct Monitor {
    stop: Sender<()>,
    worker: Option<JoinHandle<()>>,
}

impl Monitor {
    pub(super) fn start(device: &wgpu::Device) -> Option<Self> {
        let device = device.clone();
        let (stop, stopped) = mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("gpu-memory".into())
            .stack_size(128 * 1024)
            .spawn(move || {
                #[cfg(feature = "horizon-frame-profile")]
                unsafe {
                    platform_perf_thread_name(c"gpu-memory".as_ptr())
                };
                let mib = |bytes: u64| bytes as f64 / (1024.0 * 1024.0);
                let mut heap_peak = 0;
                let mut gpu_peak = 0;
                loop {
                    let mut memory = MemorySnapshot::default();
                    unsafe { platform_memory_snapshot(&mut memory) };
                    let gpu = device.get_internal_counters().hal;
                    let textures = gpu.texture_memory.read().max(0) as u64;
                    let buffers = gpu.buffer_memory.read().max(0) as u64;
                    let gpu_used = textures + buffers;
                    heap_peak = heap_peak.max(memory.heap_used);
                    gpu_peak = gpu_peak.max(gpu_used);
                    log::info!(
                        target: "switch::memory",
                        "process committed={:.1}/{:.1} MiB (GetInfo results={:#x}/{:#x}); \
                         malloc used={:.1} MiB arena={:.1}/{:.1} MiB reusable={:.1} MiB \
                         top_free={:.1} MiB sampled_peak={:.1} MiB; \
                         NVK backing used={:.1}/{:.1} MiB peak={:.1} MiB \
                         malloc_fallbacks={}; GPU resources={:.1} MiB \
                         textures={:.1} MiB buffers={:.1} MiB allocations={} \
                         sampled_peak={:.1} MiB (GPU resources overlap backing/malloc)",
                        mib(memory.process_used),
                        mib(memory.process_total),
                        memory.used_result,
                        memory.total_result,
                        mib(memory.heap_used),
                        mib(memory.heap_arena),
                        mib(memory.heap_capacity),
                        mib(memory.heap_free),
                        mib(memory.heap_top_free),
                        mib(heap_peak),
                        mib(memory.region_used),
                        mib(memory.region_reserved),
                        mib(memory.region_peak),
                        memory.region_fallbacks,
                        mib(gpu_used),
                        mib(textures),
                        mib(buffers),
                        gpu.memory_allocations.read(),
                        mib(gpu_peak),
                    );
                    #[cfg(feature = "horizon-frame-profile")]
                    unsafe {
                        platform_perf_poll()
                    };
                    // Publish the last sample even if the render thread hangs.
                    log::logger().flush();
                    match stopped.recv_timeout(Duration::from_secs(2)) {
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        _ => break,
                    }
                }
            });
        match worker {
            Ok(worker) => Some(Self {
                stop,
                worker: Some(worker),
            }),
            Err(error) => {
                log::warn!("GPU memory monitor unavailable: {error}");
                None
            }
        }
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

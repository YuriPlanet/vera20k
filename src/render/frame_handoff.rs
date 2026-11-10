//! Hands each finished frame to the driver from its own thread.
//!
//! Encoding the recorded passes (`CommandEncoder::finish`), submitting them
//! and presenting take milliseconds on a busy frame and need nothing from the
//! game. One thread does them here, so the next frame's simulation tick runs
//! beside them instead of after them.
//!
//! Only the order of GPU work has to stay the same. wgpu applies a
//! `Queue::write_*` at the next `submit`, ahead of that submission's command
//! buffers, so a write made for the next frame must not overtake the frame
//! still on its way (<https://docs.rs/wgpu/30.0.1/wgpu/struct.Queue.html#method.write_buffer>).
//! [`HandoffQueue`] makes every use of the GPU context's queue wait for that
//! frame first, and the context's surface calls do the same.
//!
//! The browser has no thread to give this to and hands the frame over inline.

use std::any::Any;
use std::ops::Deref;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError};

use crate::util::time::{Duration, Instant};

type Panic = Box<dyn Any + Send>;

struct Shared {
    /// True from a job's send until it finished; every wait's fast path.
    outstanding: AtomicBool,
    /// A job's panic, kept for the next wait to raise on the waiting thread.
    panic: Mutex<Option<Panic>>,
    changed: Condvar,
    /// Nanoseconds spent blocked in a wait since the owner last took them.
    waited_ns: AtomicU64,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Option<Panic>> {
        self.panic.lock().unwrap_or_else(PoisonError::into_inner)
    }

    #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
    fn finish(&self, panic: Option<Panic>) {
        let mut slot = self.lock();
        match panic {
            // Left outstanding, so the next wait takes the slow path and sees it.
            Some(panic) => *slot = Some(panic),
            None => self.outstanding.store(false, Ordering::Release),
        }
        drop(slot);
        self.changed.notify_all();
    }

    fn block(&self) {
        let started = Instant::now();
        let mut slot = self.lock();
        let panic = loop {
            if let Some(panic) = slot.take() {
                self.outstanding.store(false, Ordering::Release);
                break Some(panic);
            }
            if !self.outstanding.load(Ordering::Acquire) {
                break None;
            }
            slot = self
                .changed
                .wait(slot)
                .unwrap_or_else(PoisonError::into_inner);
        };
        drop(slot);
        self.waited_ns
            .fetch_add(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
        if let Some(panic) = panic {
            std::panic::resume_unwind(panic);
        }
    }
}

/// Waits for the frame on its way to the driver.
#[derive(Clone)]
pub(crate) struct HandoffWaiter(Arc<Shared>);

impl HandoffWaiter {
    /// Return once no frame is on its way. A panic on the hand-off thread
    /// continues here.
    pub(crate) fn wait(&self) {
        if self.0.outstanding.load(Ordering::Acquire) {
            self.0.block();
        }
    }

    /// The time waits spent blocked since the last call.
    pub(crate) fn take_waited(&self) -> Duration {
        Duration::from_nanos(self.0.waited_ns.swap(0, Ordering::Relaxed))
    }
}

/// The GPU context's command queue. Reaching it waits for the frame on its way
/// to the driver, so what the caller does with it stays behind that frame.
///
/// A plain `wgpu::Queue` cloned from this skips the wait: only code that runs
/// during drawing, after `GpuContext::acquire_frame` has waited, may keep one.
pub struct HandoffQueue {
    queue: wgpu::Queue,
    handoff: HandoffWaiter,
}

impl HandoffQueue {
    pub(crate) fn new(queue: wgpu::Queue, handoff: HandoffWaiter) -> Self {
        Self { queue, handoff }
    }
}

impl Deref for HandoffQueue {
    type Target = wgpu::Queue;

    fn deref(&self) -> &wgpu::Queue {
        self.handoff.wait();
        &self.queue
    }
}

type Job = Box<dyn FnOnce() + Send>;

struct Worker {
    jobs: std::sync::mpsc::Sender<Job>,
    thread: std::thread::JoinHandle<()>,
}

/// Owns the hand-off thread, which runs one job at a time.
pub(crate) struct FrameHandoff {
    shared: Arc<Shared>,
    /// `None` runs each job inline: the browser, or a failed thread start.
    worker: Option<Worker>,
}

impl FrameHandoff {
    pub(crate) fn new() -> Self {
        let shared = Arc::new(Shared {
            outstanding: AtomicBool::new(false),
            panic: Mutex::new(None),
            changed: Condvar::new(),
            waited_ns: AtomicU64::new(0),
        });
        let worker = Self::start(&shared);
        Self { shared, worker }
    }

    #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
    fn start(_shared: &Arc<Shared>) -> Option<Worker> {
        None
    }

    #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
    fn start(shared: &Arc<Shared>) -> Option<Worker> {
        let (jobs, inbox) = std::sync::mpsc::channel::<Job>();
        let finished = Arc::clone(shared);
        let thread = std::thread::Builder::new()
            .name("frame-handoff".into())
            .spawn(move || {
                for job in inbox {
                    let run = std::panic::AssertUnwindSafe(|| run_job(job));
                    finished.finish(std::panic::catch_unwind(run).err());
                }
            });
        match thread {
            Ok(thread) => Some(Worker { jobs, thread }),
            Err(error) => {
                log::warn!("No frame hand-off thread ({error}); handing frames over inline");
                None
            }
        }
    }

    pub(crate) fn waiter(&self) -> HandoffWaiter {
        HandoffWaiter(Arc::clone(&self.shared))
    }

    /// Run `job` on the hand-off thread, after any job before it.
    pub(crate) fn send(&self, job: impl FnOnce() + wgpu::WasmNotSend + 'static) {
        self.waiter().wait();
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        if let Some(worker) = &self.worker {
            self.shared.outstanding.store(true, Ordering::Release);
            worker
                .jobs
                .send(Box::new(job))
                .expect("the frame hand-off thread outlives its sender");
            return;
        }
        job();
    }
}

impl Drop for FrameHandoff {
    fn drop(&mut self) {
        if let Some(Worker { jobs, thread }) = self.worker.take() {
            // Closing the channel ends the thread once it finished its job.
            drop(jobs);
            let _ = thread.join();
        }
    }
}

/// Objective-C objects a job autoreleases outside wgpu's own pools would
/// otherwise wait for this thread to end.
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
fn run_job(job: Job) {
    off_main_thread(job);
}

/// Run GPU work on a thread other than the main one: on macOS inside an
/// autorelease pool, so the Objective-C objects it autoreleases outside
/// wgpu's own pools go when it ends, not when the thread does.
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
pub(crate) fn off_main_thread<R>(work: impl FnOnce() -> R) -> R {
    #[cfg(target_os = "macos")]
    return objc2::rc::autoreleasepool(|_| work());
    #[cfg(not(target_os = "macos"))]
    work()
}

/// The submission of a frame handed over; asking for it waits for that frame.
pub(crate) struct FrameSubmission {
    index: Arc<OnceLock<wgpu::SubmissionIndex>>,
    handoff: HandoffWaiter,
}

impl FrameSubmission {
    pub(crate) fn new(index: Arc<OnceLock<wgpu::SubmissionIndex>>, handoff: HandoffWaiter) -> Self {
        Self { index, handoff }
    }

    pub(crate) fn index(&self) -> wgpu::SubmissionIndex {
        self.handoff.wait();
        self.index
            .get()
            .expect("a finished hand-off has its submission")
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    #[test]
    fn a_wait_returns_only_after_the_frame_on_its_way() {
        let handoff = FrameHandoff::new();
        let (release, held) = mpsc::channel::<()>();
        let done = Arc::new(AtomicBool::new(false));
        let finished = Arc::clone(&done);
        handoff.send(move || {
            held.recv().unwrap();
            finished.store(true, Ordering::SeqCst);
        });
        assert!(!done.load(Ordering::SeqCst));

        let waiter = handoff.waiter();
        let waiting = std::thread::spawn(move || {
            waiter.wait();
        });
        std::thread::sleep(std::time::Duration::from_millis(30));
        assert!(!waiting.is_finished(), "the job is still running");
        release.send(()).unwrap();
        waiting.join().unwrap();
        assert!(done.load(Ordering::SeqCst));
        assert!(handoff.waiter().take_waited() > Duration::ZERO);
    }

    #[test]
    fn a_panic_on_the_thread_continues_at_the_next_wait_only() {
        let handoff = FrameHandoff::new();
        handoff.send(|| panic!("the driver went away"));
        let waiter = handoff.waiter();
        let raised = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| waiter.wait()))
            .expect_err("the job's panic reaches the waiting thread");
        assert_eq!(raised.downcast_ref::<&str>(), Some(&"the driver went away"));
        waiter.wait();
        handoff.send(|| {});
        handoff.waiter().wait();
    }

    #[test]
    fn dropping_the_owner_finishes_the_frame_on_its_way() {
        let handoff = FrameHandoff::new();
        let done = Arc::new(AtomicBool::new(false));
        let finished = Arc::clone(&done);
        handoff.send(move || {
            std::thread::sleep(std::time::Duration::from_millis(30));
            finished.store(true, Ordering::SeqCst);
        });
        drop(handoff);
        assert!(done.load(Ordering::SeqCst));
    }

    /// A write made after the hand-off stays behind the frame even though the
    /// frame has not reached `Queue::submit` yet: `wgpu` would otherwise apply
    /// the write ahead of that frame's commands.
    #[test]
    #[ignore = "requires a wgpu adapter"]
    fn a_later_queue_write_stays_behind_the_frame_on_its_way() {
        use crate::render::gpu::block_on;

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: None,
            ..Default::default()
        }))
        .expect("the regression requires a GPU adapter");
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
            .expect("request the regression device");
        let source = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 4,
            usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let seen = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 4,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        queue.write_buffer(&source, 0, &[1, 1, 1, 1]);
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(&source, 0, &seen, 0, 4);

        let handoff = FrameHandoff::new();
        let ordered = HandoffQueue::new(queue.clone(), handoff.waiter());
        let (release, held) = mpsc::channel::<()>();
        let frame_queue = queue.clone();
        handoff.send(move || {
            held.recv().unwrap();
            frame_queue.submit([encoder.finish()]);
        });
        let writer = {
            let source = source.clone();
            std::thread::spawn(move || {
                ordered.write_buffer(&source, 0, &[2, 2, 2, 2]);
                ordered.submit([]);
            })
        };
        std::thread::sleep(std::time::Duration::from_millis(30));
        release.send(()).unwrap();
        writer.join().unwrap();

        seen.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        assert_eq!(seen.slice(..).get_mapped_range().unwrap()[..], [1, 1, 1, 1]);
    }
}

//! The client's owner of the simulation (docs/sim-thread.md, phase 1).
//!
//! The host lives as long as the process, like the simulation thread it is
//! meant to become. It holds the installed scenario's [`SimRuntime`], if
//! any, and the process-lifetime state a tick reaches outside the world.
//!
//! Phase 3 lends the runtime to the simulation thread for one job
//! ([`SimHost::lend`]), publishing a presentation frame as it goes: a copy
//! of the world as the job found it. While the runtime is away, reads see
//! that frame, and any write first takes the runtime back, waiting for the
//! job to end; the job's result then waits in the host for
//! [`SimHost::take_job_result`]. Back home, the frame is dropped and reads
//! see the live world again, so a tick that is not lent copies nothing.
//!
//! A frame-loop pass never waits for a lent job: the host collects it only
//! once it has ended ([`SimHost::pass_may_run`]), and the simulation thread
//! wakes the event loop when it ends ([`SimHost::set_wake`]). The browser's
//! main thread may never wait at all, and its simulation thread is a Web
//! Worker (`web::sim_worker`).
//!
//! Reads take [`SimHost::runtime`]. A write names its route with
//! [`SimWrite`], so every place that changes the simulation outside
//! presentation is explicit and found by one search. The later phases turn
//! each route into its own operation: commands become submissions, the tick
//! moves to the simulation thread, and the few routes that must stay
//! synchronous are the ones still named here.

use std::any::Any;
use std::sync::{Arc, Condvar, PoisonError};

use crate::app::presentation::frame::PresentationFrame;
use crate::sim::command_queue::{FrameChecksumState, MultiplayerChecksumHistory};
use crate::sim::runtime::SimRuntime;
use crate::util::sync::Mutex;
use crate::util::time::{Duration, Instant};

/// What a lent job returns, for its owner to downcast.
pub(crate) type SimJobResult = Box<dyn Any + Send>;
/// Work the simulation thread runs on the lent runtime.
pub(crate) type SimJob = Box<dyn FnOnce(&mut SimRuntime) -> SimJobResult + Send>;
type Returned = (SimRuntime, std::thread::Result<SimJobResult>);
/// Wakes the frame loop once a lent job has ended.
pub(crate) type SimWake = Arc<dyn Fn() + Send + Sync>;

/// Why a caller writes the simulation. Each route is a family of writes that
/// a later phase handles the same way when the simulation runs elsewhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SimWrite {
    /// Local player input: commands, planning nodes, options requests, and
    /// the voice and flash effects an order or selection has at input time.
    Command,
    /// Debug tools, cheats and the trainer.
    Debug,
    /// Installing, replacing, restoring and retiring the scenario.
    Lifecycle,
    /// The frame loop's tick, its waits and scenario pause service.
    Tick,
    /// Network session bookkeeping: departures and the EventClass queues.
    Network,
    /// Presentation handoff state that is neither saved nor hashed.
    Presentation,
    /// The runtime's bound rules, not the world.
    Rules,
}

/// The installed simulation, if any, and the state a tick needs beyond it.
#[derive(Default)]
pub(crate) struct SimHost {
    /// `None` while uninstalled or lent.
    runtime: Option<SimRuntime>,
    /// Whether the runtime is on the simulation thread.
    lent: bool,
    /// The last job's result, once the runtime is back.
    job_result: Option<SimJobResult>,
    /// Jobs lent, and those a write took back before
    /// [`Self::take_job_result`] asked for them, since the last report.
    lent_jobs: u32,
    early_returns: u32,
    /// The simulation thread, started by the first [`Self::lend`] (in the
    /// browser, by the first pass of a scenario: [`Self::pass_may_run`]).
    worker: Option<SimWorker>,
    /// A lent job's return, once [`Self::pass_may_run`] found it ended.
    ended: Option<Returned>,
    /// The world without its objects as the frame found it
    /// ([`SimRuntime::presentation_copy`]), which drawing reads beside the
    /// frame's records while the live world ticks elsewhere. Copy-on-write
    /// state (the terrain cells, the resources) stays shared with the live
    /// world until that changes. Release builds keep it only while the
    /// runtime is lent; debug builds take one with every frame, so every
    /// draw reads the frame as a lent tick's draw does.
    presentation: Option<Arc<SimRuntime>>,
    /// The last world copy's world, which the next copy brings up to date
    /// instead of copying the world whole.
    spare: Option<crate::sim::world::Simulation>,
    /// Time spent capturing frames and bringing the world copy up to date,
    /// and how many of each, since the last report.
    snapshot_costs: std::cell::Cell<SnapshotCosts>,
    /// What the last lent job cost this thread: copying the frame when the
    /// job left ([`Self::lend`]), kept in `copy_cost` meanwhile, then the
    /// wait for the job and the frame's drop when it came back. `None`
    /// until one has come back in this scenario.
    lend_cost: Option<Duration>,
    copy_cost: Duration,
    /// Called by the simulation thread when a job ends, so the frame loop
    /// finishes it without waiting for its next redraw.
    wake: Option<SimWake>,
    /// What drawing reads (`presentation::frame`): captured from the home
    /// world on its first read after a write, and at [`Self::lend`] for the
    /// world the lent tick starts from. Every write route empties it.
    frame: std::cell::OnceCell<Arc<PresentationFrame>>,
    /// Global B04474 and its comparison timer (`FrameChecksumState`):
    /// process state that launch and load retain, written and compared only
    /// inside a multiplayer tick.
    frame_checksums: FrameChecksumState,
}

impl SimHost {
    /// The installed scenario's runtime, for reading: the published frame
    /// while the runtime is lent.
    pub(crate) fn runtime(&self) -> Option<&SimRuntime> {
        if self.lent {
            return self.presentation.as_deref();
        }
        self.runtime.as_ref()
    }

    /// The live runtime while it is home; `None` while a tick has it. For
    /// diagnostics that read what drawing may not (objects' paths, the path
    /// grid): they show nothing beside a lent tick.
    pub(crate) fn home_runtime(&self) -> Option<&SimRuntime> {
        self.runtime.as_ref()
    }

    pub(crate) fn is_installed(&self) -> bool {
        self.lent || self.runtime.is_some()
    }

    /// Whether a job holds the runtime on the simulation thread.
    pub(crate) fn is_lent(&self) -> bool {
        self.lent
    }

    /// Whether ordinary frames may tick on the simulation thread
    /// ([`sim_thread_available`]). The threaded browser build lends nothing
    /// until its worker runs; the other browser builds have none.
    pub(crate) fn thread_enabled(&self) -> bool {
        #[cfg(all(
            feature = "wasm-threads",
            target_arch = "wasm32",
            target_os = "unknown"
        ))]
        return sim_thread_available() && self.worker.as_ref().is_some_and(|worker| worker.running);
        #[cfg(not(all(
            feature = "wasm-threads",
            target_arch = "wasm32",
            target_os = "unknown"
        )))]
        sim_thread_available()
    }

    /// The start of a frame-loop pass: whether it may go on. A pass goes on
    /// only once a lent tick has ended; until then it draws the frame
    /// published for the tick, and the pass the tick's end wakes
    /// ([`Self::set_wake`]) finishes it. With `wait`, a desktop pass goes on
    /// anyway and waits for the tick (`sim_tick::finish_threaded_frame`), for
    /// runs whose pictures must each follow a finished tick. The browser's
    /// main thread traps if it waits on another thread, so there `wait`
    /// changes nothing; there this also starts the simulation worker for the
    /// first scenario that can use it.
    pub(crate) fn pass_may_run(&mut self, wait: bool) -> bool {
        #[cfg(all(
            feature = "wasm-threads",
            target_arch = "wasm32",
            target_os = "unknown"
        ))]
        if let Some(worker) = &mut self.worker {
            worker.running |= worker.ready.load(std::sync::atomic::Ordering::Acquire);
        } else if self.is_installed()
            && sim_thread_available()
            && crate::web::sim_worker::available()
        {
            self.worker = Some(SimWorker::spawn());
        }
        self.job_ended() || (wait && !BROWSER_THREADS)
    }

    /// Have the simulation thread call `wake` whenever a lent job ends.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn set_wake(&mut self, wake: SimWake) {
        self.wake = Some(wake);
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn has_wake(&self) -> bool {
        self.wake.is_some()
    }

    /// Whether no lent job is still running, without waiting for one. An
    /// ended job's runtime waits in the host, still lent: reads keep seeing
    /// the job's frame until a write or [`Self::take_job_result`] takes it
    /// back, which then need not wait.
    fn job_ended(&mut self) -> bool {
        if !self.lent || self.ended.is_some() {
            return true;
        }
        let worker = self.worker.as_ref().expect("a lent runtime has a worker");
        self.ended = worker.returned.take();
        // A worker's panic aborts it, so its tick never comes back.
        #[cfg(all(
            feature = "wasm-threads",
            target_arch = "wasm32",
            target_os = "unknown"
        ))]
        if self.ended.is_none() {
            crate::web::sim_worker::raise_worker_panic();
        }
        self.ended.is_some()
    }

    /// Run `job` on the simulation thread with the runtime, publishing the
    /// world as it is now. Reads serve that frame until a write takes the
    /// runtime back. Returns false, running nothing, when no runtime is
    /// installed.
    pub(crate) fn lend(&mut self, job: SimJob) -> bool {
        self.reclaim(true);
        if self.runtime.is_none() {
            return false;
        }
        self.lent_jobs += 1;
        if self.lent_jobs == EARLY_RETURN_REPORT_JOBS {
            self.report_early_returns();
        }
        // The frame drawn while the tick runs is the world as it finds it.
        let copy_started = Instant::now();
        self.capture_frame(true);
        self.copy_cost = copy_started.elapsed();
        let runtime = self.runtime.take().expect("installed runtime");
        self.worker
            .get_or_insert_with(SimWorker::spawn)
            .jobs
            .put(Some((runtime, job, self.wake.clone())));
        self.lent = true;
        true
    }

    /// Log how many lent jobs a write took back early, then start counting
    /// again. Such a job ran while the main thread waited for it, so the two
    /// did not overlap; common ones are worth finding.
    fn report_early_returns(&mut self) {
        if self.lent_jobs > 0 {
            let costs = self.snapshot_costs.take();
            log::info!(
                "Simulation thread: {} of the last {} ticks were taken back early; \
                 frame capture {:.3} ms mean over {}, world copy {:.3} ms mean over {}",
                self.early_returns,
                self.lent_jobs,
                costs.capture_ms(),
                costs.captures,
                costs.copy_ms(),
                costs.copies,
            );
        }
        self.lent_jobs = 0;
        self.early_returns = 0;
    }

    /// Wait for a lent runtime to come back. A panic in the job resumes here.
    /// `early` when the caller is not the one collecting the job's result.
    fn reclaim(&mut self, early: bool) {
        if !self.lent {
            return;
        }
        self.lent = false;
        self.early_returns += u32::from(early);
        let wait_started = Instant::now();
        let (runtime, result) = match self.ended.take() {
            Some(returned) => returned,
            None => self
                .worker
                .as_ref()
                .expect("a lent runtime has a worker")
                .wait(),
        };
        let waited = wait_started.elapsed();
        self.runtime = Some(runtime);
        // The tick moved the world past the frame it found, which has done
        // its work: reads see the live world again.
        let drop_started = Instant::now();
        self.forget_frame();
        self.lend_cost = Some(self.copy_cost + waited + drop_started.elapsed());
        match result {
            Ok(result) => self.job_result = Some(result),
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }

    /// The last lent job's result, waiting for the job if it still runs.
    pub(crate) fn take_job_result(&mut self) -> Option<SimJobResult> {
        self.reclaim(false);
        self.job_result.take()
    }

    /// Mutable access to the installed runtime for one write route; a lent
    /// runtime comes back first.
    pub(crate) fn write_sim(&mut self, _route: SimWrite) -> Option<&mut SimRuntime> {
        self.reclaim(true);
        self.forget_frame();
        self.runtime.as_mut()
    }

    /// Drop the frame and the world copy: the world may change. The copy's
    /// world is kept for the next copy to bring up to date
    /// (`Simulation::presentation_copy`).
    fn forget_frame(&mut self) {
        self.frame = std::cell::OnceCell::new();
        if let Some(copy) = self.presentation.take()
            && let Ok(copy) = Arc::try_unwrap(copy)
        {
            self.spare = Some(copy.simulation);
        }
    }

    /// Capture the frame of the home world before a draw, and in debug
    /// builds the world copy too, so every draw reads a lent tick's view of
    /// the world. A lent runtime keeps the frame its tick found.
    pub(crate) fn ensure_frame(&mut self) {
        if !self.lent {
            self.capture_frame(cfg!(debug_assertions));
        }
    }

    /// Capture the frame of the home runtime, and with `world` the copy of
    /// the world drawing reads beside it.
    fn capture_frame(&mut self, world: bool) {
        let _ = self.frame();
        if world
            && self.presentation.is_none()
            && let Some(runtime) = self.runtime.as_mut()
        {
            let viewer = runtime.simulation.session().current_house();
            let reuse = self.spare.take();
            let started = Instant::now();
            self.presentation = Some(Arc::new(runtime.presentation_copy(viewer, reuse)));
            let mut costs = self.snapshot_costs.get();
            costs.copy += started.elapsed();
            costs.copies += 1;
            self.snapshot_costs.set(costs);
        }
    }

    /// What drawing reads of the objects: the frame the lent tick started
    /// from, else the home world's, captured on its first read since the
    /// last write.
    pub(crate) fn frame(&self) -> Option<&PresentationFrame> {
        if self.lent {
            return self.frame.get().map(|frame| &**frame);
        }
        let runtime = self.runtime.as_ref()?;
        Some(self.frame.get_or_init(|| {
            let started = Instant::now();
            let frame = Arc::new(PresentationFrame::capture(runtime));
            let mut costs = self.snapshot_costs.get();
            costs.capture += started.elapsed();
            costs.captures += 1;
            self.snapshot_costs.set(costs);
            frame
        }))
    }

    /// What the last lent job cost this thread, to weigh against the tick
    /// it freed this thread from: the frame's copy and drop, and any wait
    /// for a job that was not back when this thread needed the world. A
    /// simulation thread that runs slower than this one, on a busy machine
    /// or a slower core, shows up in that wait. `None` before the first job
    /// of a scenario is back.
    pub(crate) fn lend_cost(&self) -> Option<Duration> {
        self.lend_cost
    }

    /// The world drawing reads beside [`Self::frame`]: its copy without the
    /// objects while one stands for the current world, else the live world.
    /// The objects are the frame's.
    pub(crate) fn presentation(&self) -> Option<&SimRuntime> {
        self.presentation.as_deref().or(self.runtime.as_ref())
    }

    /// The [`SimWrite::Tick`] route: the installed runtime and the checksum
    /// owner its tick borrows at the native queue boundary, together.
    pub(crate) fn tick_parts(&mut self) -> Option<(&mut SimRuntime, &mut FrameChecksumState)> {
        self.reclaim(true);
        self.forget_frame();
        let runtime = self.runtime.as_mut()?;
        Some((runtime, &mut self.frame_checksums))
    }

    /// The multiplayer checksum ring the tick records and compares.
    pub(crate) fn checksum_history(&self) -> &MultiplayerChecksumHistory {
        self.frame_checksums.checksum_history()
    }

    /// Install `runtime` in place of any current one.
    pub(crate) fn install(&mut self, runtime: Option<SimRuntime>) {
        // A job's result belongs to the world it ran on.
        self.reclaim(true);
        self.report_early_returns();
        self.job_result = None;
        self.lend_cost = None;
        self.runtime = runtime;
        self.forget_frame();
        self.spare = None;
    }

    /// Remove and return the installed runtime.
    pub(crate) fn take_runtime(&mut self) -> Option<SimRuntime> {
        self.reclaim(true);
        self.report_early_returns();
        self.job_result = None;
        self.lend_cost = None;
        self.forget_frame();
        self.spare = None;
        self.runtime.take()
    }
}

/// A question about the live world a draw asks every frame (a cursor's
/// preview, a hover): answered while the world is home and the answer kept
/// in `kept`, which the draws beside a lent tick read (docs/sim-thread.md).
pub(crate) fn home_answer<T: Clone>(
    host: &SimHost,
    kept: &std::cell::RefCell<T>,
    ask: impl FnOnce() -> T,
) -> T {
    if host.is_lent() {
        return kept.borrow().clone();
    }
    let answer = ask();
    *kept.borrow_mut() = answer.clone();
    answer
}

/// What the presentation snapshots cost this thread since the last report
/// ([`SimHost::report_early_returns`]).
#[derive(Clone, Copy, Debug, Default)]
struct SnapshotCosts {
    capture: Duration,
    captures: u32,
    copy: Duration,
    copies: u32,
}

impl SnapshotCosts {
    fn capture_ms(self) -> f64 {
        self.capture.as_secs_f64() * 1000.0 / f64::from(self.captures.max(1))
    }

    fn copy_ms(self) -> f64 {
        self.copy.as_secs_f64() * 1000.0 / f64::from(self.copies.max(1))
    }
}

/// The threaded browser build, whose main thread may never wait for the
/// simulation thread (docs/sim-thread.md, "The browser").
const BROWSER_THREADS: bool = cfg!(all(
    feature = "wasm-threads",
    target_arch = "wasm32",
    target_os = "unknown"
));

/// How many lent jobs pass between two log lines on early returns.
const EARLY_RETURN_REPORT_JOBS: u32 = 1024;

/// The simulation thread: runs each lent job and sends the runtime back.
/// It ends when the host drops it.
struct SimWorker {
    /// The next job and who to wake when it ends; `None` ends the thread.
    jobs: Arc<Handoff<Option<LentJob>>>,
    returned: Arc<Handoff<Returned>>,
    /// Browser: set by the Web Worker once it runs, which can take a moment
    /// after [`Self::spawn`] or never happen.
    #[cfg(all(
        feature = "wasm-threads",
        target_arch = "wasm32",
        target_os = "unknown"
    ))]
    ready: Arc<std::sync::atomic::AtomicBool>,
    /// Browser: `ready` as the last pass found it, so one pass's frames
    /// agree on whether they can be lent.
    #[cfg(all(
        feature = "wasm-threads",
        target_arch = "wasm32",
        target_os = "unknown"
    ))]
    running: bool,
}

impl SimWorker {
    #[cfg(not(all(
        feature = "wasm-threads",
        target_arch = "wasm32",
        target_os = "unknown"
    )))]
    fn spawn() -> Self {
        let (jobs, returned) = (Arc::new(Handoff::default()), Arc::new(Handoff::default()));
        let (job_queue, send_back) = (Arc::clone(&jobs), Arc::clone(&returned));
        std::thread::Builder::new()
            .name("simulation".into())
            // The main thread's size: a tick recurses as deep there.
            .stack_size(8 << 20)
            .spawn(move || run_jobs(&job_queue, &send_back))
            .expect("start the simulation thread");
        Self { jobs, returned }
    }

    /// The browser has no `std::thread`: the thread is a Web Worker sharing
    /// the engine's memory, with the main thread's stack size.
    #[cfg(all(
        feature = "wasm-threads",
        target_arch = "wasm32",
        target_os = "unknown"
    ))]
    fn spawn() -> Self {
        let (jobs, returned) = (Arc::new(Handoff::default()), Arc::new(Handoff::default()));
        let (job_queue, send_back) = (Arc::clone(&jobs), Arc::clone(&returned));
        let ready = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let started = Arc::clone(&ready);
        crate::web::sim_worker::spawn(Box::new(move || {
            started.store(true, std::sync::atomic::Ordering::Release);
            run_jobs(&job_queue, &send_back);
        }));
        Self {
            jobs,
            returned,
            ready,
            running: false,
        }
    }

    /// Wait for the lent runtime to come back.
    fn wait(&self) -> Returned {
        // A wait on another thread traps on the browser's main thread, so
        // the rare write that takes a tick back there spins until it ends.
        #[cfg(all(
            feature = "wasm-threads",
            target_arch = "wasm32",
            target_os = "unknown"
        ))]
        loop {
            if let Some(returned) = self.returned.take() {
                return returned;
            }
            crate::web::sim_worker::raise_worker_panic();
            std::hint::spin_loop();
        }
        #[cfg(not(all(
            feature = "wasm-threads",
            target_arch = "wasm32",
            target_os = "unknown"
        )))]
        self.returned.wait()
    }
}

impl Drop for SimWorker {
    fn drop(&mut self) {
        self.jobs.put(None);
    }
}

type LentJob = (SimRuntime, SimJob, Option<SimWake>);

/// The simulation thread's loop.
fn run_jobs(jobs: &Handoff<Option<LentJob>>, returned: &Handoff<Returned>) {
    while let Some((mut runtime, job, wake)) = jobs.wait() {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| job(&mut runtime)));
        returned.put((runtime, result));
        if let Some(wake) = wake {
            wake();
        }
    }
}

/// One value at a time between the main thread and the simulation thread.
/// Its lock spins instead of waiting on the browser's main thread
/// (`util::sync`); a standard channel's sender may wait there on a lock of
/// its own. Only the simulation thread and desktop's main thread
/// [`Self::wait`].
struct Handoff<T> {
    value: Mutex<Option<T>>,
    filled: Condvar,
}

impl<T> Default for Handoff<T> {
    fn default() -> Self {
        Self {
            value: Mutex::new(None),
            filled: Condvar::new(),
        }
    }
}

impl<T> Handoff<T> {
    fn put(&self, value: T) {
        *self.value.lock().unwrap_or_else(PoisonError::into_inner) = Some(value);
        self.filled.notify_one();
    }

    /// The value, if one was put since the last take.
    fn take(&self) -> Option<T> {
        self.value
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }

    /// The next value, waiting for it.
    fn wait(&self) -> T {
        let mut value = self.value.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if let Some(value) = value.take() {
                return value;
            }
            value = self
                .filled
                .wait(value)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
}

/// Whether ordinary frames can tick on the simulation thread: in every
/// desktop build, and in the browser only in its threaded build, once the
/// worker runs ([`SimHost::thread_enabled`]) and when the page asks for it
/// ([`set_browser_sim_thread`]). Unit tests tick inline too: they read the
/// world straight after a frame, and the host's own tests cover lending.
fn sim_thread_available() -> bool {
    (!cfg!(all(feature = "wasm", target_arch = "wasm32")) || BROWSER_THREADS)
        && !cfg!(test)
        && browser_sim_thread() != Some("0")
}

/// The page's setting for the browser's experimental simulation worker
/// (`web::set_simulation_thread`): `0`, the default, keeps it off; `1` lends
/// a frame when that saves the main thread time; `always` lends every frame
/// the worker can take. The first read fixes it. Desktop has no setting.
fn browser_sim_thread() -> Option<&'static str> {
    #[cfg(all(
        feature = "wasm-threads",
        target_arch = "wasm32",
        target_os = "unknown"
    ))]
    return Some(BROWSER_SIM_THREAD.get_or_init(|| "0".to_owned()).as_str());
    #[cfg(not(all(
        feature = "wasm-threads",
        target_arch = "wasm32",
        target_os = "unknown"
    )))]
    None
}

/// The page's simulation-thread setting (`web::set_simulation_thread`).
#[cfg(all(
    feature = "wasm-threads",
    target_arch = "wasm32",
    target_os = "unknown"
))]
static BROWSER_SIM_THREAD: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Set the page's simulation-thread setting before the engine first reads
/// it; a later call changes nothing.
#[cfg(all(
    feature = "wasm-threads",
    target_arch = "wasm32",
    target_os = "unknown"
))]
pub(crate) fn set_browser_sim_thread(value: &str) {
    let _ = BROWSER_SIM_THREAD.set(value.to_owned());
}

/// Whether a frame the browser's worker can take goes to it whatever its
/// published frame costs the main thread: the page's `always`, for
/// exercising the worker. Otherwise a frame goes only when that saves the
/// main thread time (`sim_tick::advance_in_game_runtime_mode`).
pub(crate) fn sim_thread_always() -> bool {
    sim_thread_available() && browser_sim_thread() == Some("always")
}

// docs/sim-thread.md moves the host onto its own thread; it must stay Send.
const _: () = {
    const fn assert_send<T: Send>() {}
    assert_send::<SimHost>();
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::lockstep::{ChecksumMismatch, FrameInfo, FrameInfoCompareGate};
    use crate::sim::world::Simulation;

    #[test]
    fn a_lent_runtime_serves_reads_from_the_frame_until_a_write_takes_it_back() {
        let mut host = SimHost::default();
        host.install(Some(SimRuntime::from_simulation(Simulation::new())));
        host.write_sim(SimWrite::Tick)
            .unwrap()
            .simulation
            .session_mut_for_test()
            .set_tick_for_test(7);
        // Lending publishes the world as the job finds it.
        assert!(host.lend(Box::new(|runtime| {
            runtime
                .simulation
                .session_mut_for_test()
                .set_tick_for_test(8);
            Box::new(41_u32)
        })));
        assert!(host.is_installed());
        // Whether or not the job has finished, reads see the frame.
        assert_eq!(host.runtime().unwrap().simulation.session().tick(), 7);
        let live = host.write_sim(SimWrite::Command).unwrap();
        assert_eq!(live.simulation.session().tick(), 8);
        assert!(!host.is_lent());
        assert_eq!((host.lent_jobs, host.early_returns), (1, 1));
        // Back home, the job's frame is gone and drawing reads the live world.
        assert_eq!(host.presentation().unwrap().simulation.session().tick(), 8);
        // What it cost is known from then on, until another scenario.
        assert!(host.lend_cost().is_some());
        let result = host.take_job_result().unwrap();
        assert_eq!(*result.downcast::<u32>().unwrap(), 41);
        assert!(host.take_job_result().is_none());
        // A job its owner collects is not an early return.
        assert!(host.lend(Box::new(|_| Box::new(()))));
        assert!(host.take_job_result().is_some());
        assert_eq!((host.lent_jobs, host.early_returns), (2, 1));
    }

    #[test]
    fn a_pass_finds_a_running_job_without_waiting_and_an_ended_one_waits_lent() {
        let mut host = SimHost::default();
        host.install(Some(SimRuntime::from_simulation(Simulation::new())));
        host.write_sim(SimWrite::Tick)
            .unwrap()
            .simulation
            .session_mut_for_test()
            .set_tick_for_test(7);
        let (go, wait) = std::sync::mpsc::channel::<()>();
        assert!(host.lend(Box::new(move |runtime| {
            wait.recv().unwrap();
            runtime
                .simulation
                .session_mut_for_test()
                .set_tick_for_test(8);
            Box::new(41_u32)
        })));
        // Still running: the call returns at once.
        assert!(!host.job_ended());
        go.send(()).unwrap();
        while !host.job_ended() {
            std::thread::yield_now();
        }
        // Ended, but still lent: reads see the job's frame until the pass
        // collects its result, which needs no wait and is not early.
        assert!(host.is_lent());
        assert_eq!(host.runtime().unwrap().simulation.session().tick(), 7);
        let result = host.take_job_result().unwrap();
        assert_eq!(*result.downcast::<u32>().unwrap(), 41);
        assert_eq!(host.runtime().unwrap().simulation.session().tick(), 8);
        assert_eq!((host.lent_jobs, host.early_returns), (1, 0));
    }

    #[test]
    fn a_pass_goes_on_only_once_the_job_ended_and_its_end_wakes_the_loop() {
        let mut host = SimHost::default();
        host.install(Some(SimRuntime::from_simulation(Simulation::new())));
        let woken = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let counter = Arc::clone(&woken);
        host.set_wake(Arc::new(move || {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }));
        let (go, wait) = std::sync::mpsc::channel::<()>();
        assert!(host.lend(Box::new(move |_| {
            wait.recv().unwrap();
            Box::new(())
        })));
        // Running: a pass passes beside it, unless it is told to wait.
        assert!(!host.pass_may_run(false));
        assert_eq!(woken.load(std::sync::atomic::Ordering::SeqCst), 0);
        go.send(()).unwrap();
        while woken.load(std::sync::atomic::Ordering::SeqCst) == 0 {
            std::thread::yield_now();
        }
        assert!(host.pass_may_run(false));
        assert!(host.take_job_result().is_some());
        assert_eq!((host.lent_jobs, host.early_returns), (1, 0));
    }

    #[test]
    fn a_new_scenario_drops_the_last_job_result_and_a_job_panic_reaches_the_caller() {
        let mut host = SimHost::default();
        host.install(Some(SimRuntime::from_simulation(Simulation::new())));
        assert!(host.lend(Box::new(|_| Box::new(()))));
        host.install(Some(SimRuntime::from_simulation(Simulation::new())));
        assert!(host.take_job_result().is_none());

        assert!(host.lend(Box::new(|_| panic!("tick failed"))));
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            host.take_job_result();
        }));
        assert!(caught.is_err());
        // The runtime came back with the panic.
        assert!(host.runtime().is_some());
        host.install(None);
        assert!(!host.lend(Box::new(|_| Box::new(()))));
    }

    #[test]
    fn frame_zero_resets_the_checksum_ring_and_installs_do_not() {
        let mut host = SimHost::default();
        assert_eq!(host.frame_checksums.compare_gate(), None);
        host.frame_checksums
            .record_frame_checksum(0x1_05, 0xdead_beef);
        assert_eq!(host.checksum_history().get(0x05), 0xdead_beef);
        assert_eq!(host.checksum_history().get(0x2_05), 0xdead_beef);
        assert_eq!(
            host.frame_checksums.compare_gate(),
            Some(FrameInfoCompareGate::new(0x1_05, 0))
        );
        // Installing a scenario retains the process ring, as a launch does.
        host.install(Some(SimRuntime::from_simulation(Simulation::new())));
        assert_eq!(host.checksum_history().get(0x05), 0xdead_beef);
        assert_eq!(
            host.frame_checksums.compare_gate(),
            Some(FrameInfoCompareGate::new(0x1_05, 0))
        );

        host.frame_checksums.record_frame_checksum(0, 0x1234_5678);
        assert_eq!(host.checksum_history().get(0), 0);
        assert_eq!(host.checksum_history().get(0x05), 0);
        assert_eq!(
            host.frame_checksums.compare_gate(),
            Some(FrameInfoCompareGate::new(0, 256))
        );
        host.frame_checksums.record_frame_checksum(1, 0x1234_5678);
        assert_eq!(host.checksum_history().get(1), 0x1234_5678);
    }

    #[test]
    fn frame_info_comparison_starts_at_frame_256_and_reads_current_checksum() {
        let mut host = SimHost::default();
        host.frame_checksums.record_frame_checksum(0, 1);
        host.frame_checksums.record_frame_checksum(255, 0x1122_3344);
        let info = |frame, checksum| FrameInfo {
            house_id: 2,
            event_frame: frame,
            checksum,
            timing_word: 0,
            delay: 0,
        };
        assert_eq!(
            host.frame_checksums.compare_frame_info(info(255, 0), 255),
            Ok(false)
        );

        host.frame_checksums.record_frame_checksum(256, 0x5566_7788);
        assert_eq!(
            host.frame_checksums
                .compare_frame_info(info(256, 0x5566_7788), 256),
            Ok(true)
        );
        assert_eq!(
            host.frame_checksums
                .compare_frame_info(info(256, 0x99), 256),
            Err(ChecksumMismatch {
                house_id: 2,
                event_frame: 256,
                delay: 0,
                history_index: 0,
                local_checksum: 0x5566_7788,
                remote_checksum: 0x99,
            })
        );
    }

    #[test]
    fn first_nonzero_frame_opens_comparison_without_startup_grace() {
        let mut host = SimHost::default();
        host.frame_checksums.record_frame_checksum(73, 0x1234_5678);
        let info = FrameInfo {
            house_id: 1,
            event_frame: 73,
            checksum: 0x1234_5678,
            timing_word: 0,
            delay: 0,
        };
        assert_eq!(host.frame_checksums.compare_frame_info(info, 73), Ok(true));
    }

    #[test]
    fn nonzero_checksum_restart_uses_frame_plus_256_as_timer_period() {
        let mut host = SimHost::default();
        host.frame_checksums.record_frame_checksum(100, 0x1234_5678);
        host.frame_checksums.reset_frame_checksum_history(100);
        assert_eq!(host.checksum_history().get(100), 0);
        assert_eq!(
            host.frame_checksums.compare_gate(),
            Some(FrameInfoCompareGate::new(100, 356))
        );
        let info = |frame, checksum| FrameInfo {
            house_id: 1,
            event_frame: frame,
            checksum,
            timing_word: 0,
            delay: 0,
        };
        assert_eq!(
            host.frame_checksums.compare_frame_info(info(455, 1), 455),
            Ok(false)
        );
        host.frame_checksums.record_frame_checksum(456, 0x8765_4321);
        assert_eq!(
            host.frame_checksums
                .compare_frame_info(info(456, 0x8765_4321), 456),
            Ok(true)
        );
    }
}

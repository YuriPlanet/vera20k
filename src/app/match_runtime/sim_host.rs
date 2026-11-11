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
//! Reads take [`SimHost::runtime`]. A write names its route with
//! [`SimWrite`], so every place that changes the simulation outside
//! presentation is explicit and found by one search. The later phases turn
//! each route into its own operation: commands become submissions, the tick
//! moves to the simulation thread, and the few routes that must stay
//! synchronous are the ones still named here.

use std::any::Any;
use std::sync::{Arc, mpsc};

use crate::sim::command_queue::{FrameChecksumState, MultiplayerChecksumHistory};
use crate::sim::runtime::SimRuntime;
use crate::util::time::{Duration, Instant};

/// What a lent job returns, for its owner to downcast.
pub(crate) type SimJobResult = Box<dyn Any + Send>;
/// Work the simulation thread runs on the lent runtime.
pub(crate) type SimJob = Box<dyn FnOnce(&mut SimRuntime) -> SimJobResult + Send>;
type Returned = (SimRuntime, std::thread::Result<SimJobResult>);

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
    /// The simulation thread, started by the first [`Self::lend`].
    worker: Option<SimWorker>,
    /// The presentation frame (phase 2): the world as a lent job found it.
    /// Drawing reads it while the live world is ticking elsewhere.
    /// Copy-on-write state (the terrain cells, the resources) stays shared
    /// with the live world until that changes. It lives only while the
    /// runtime is lent, apart from the `VERA20K_PRESENTATION_FRAME` opt-in.
    presentation: Option<Arc<SimRuntime>>,
    /// What the last lent job cost this thread: copying the frame when the
    /// job left ([`Self::lend`]), kept in `copy_cost` meanwhile, then the
    /// wait for the job and the frame's drop when it came back. `None`
    /// until one has come back in this scenario.
    lend_cost: Option<Duration>,
    copy_cost: Duration,
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

    pub(crate) fn is_installed(&self) -> bool {
        self.lent || self.runtime.is_some()
    }

    /// Whether a job holds the runtime on the simulation thread.
    pub(crate) fn is_lent(&self) -> bool {
        self.lent
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
        let copy_started = Instant::now();
        self.publish_presentation();
        self.copy_cost = copy_started.elapsed();
        let runtime = self.runtime.take().expect("installed runtime");
        self.worker
            .get_or_insert_with(SimWorker::spawn)
            .jobs
            .send((runtime, job))
            .expect("simulation thread stopped");
        self.lent = true;
        true
    }

    /// Log how many lent jobs a write took back early, then start counting
    /// again. Such a job ran while the main thread waited for it, so the two
    /// did not overlap; common ones are worth finding.
    fn report_early_returns(&mut self) {
        if self.lent_jobs > 0 {
            log::info!(
                "Simulation thread: {} of the last {} ticks were taken back early",
                self.early_returns,
                self.lent_jobs
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
        let (runtime, result) = self
            .worker
            .as_ref()
            .expect("a lent runtime has a worker")
            .returned
            .recv()
            .expect("simulation thread stopped");
        let waited = wait_started.elapsed();
        self.runtime = Some(runtime);
        // The job's frame has done its work: reads see the live world again,
        // and nothing shares its storage or its resources.
        let drop_started = Instant::now();
        self.presentation = None;
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
    pub(crate) fn write_sim(&mut self, route: SimWrite) -> Option<&mut SimRuntime> {
        self.reclaim(true);
        if matches!(route, SimWrite::Rules | SimWrite::Lifecycle) {
            // These may rebind the shared resources (`resources_mut`), which
            // an opted-in frame would still share.
            self.presentation = None;
        }
        self.runtime.as_mut()
    }

    /// Publish the live world as the presentation frame: [`Self::lend`]
    /// does, and so does every tick under the `VERA20K_PRESENTATION_FRAME`
    /// opt-in.
    pub(crate) fn publish_presentation(&mut self) {
        if self.lent {
            return;
        }
        self.presentation = self
            .runtime
            .as_ref()
            .map(|runtime| Arc::new(runtime.clone()));
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

    /// What drawing reads: the published frame, else the live world.
    pub(crate) fn presentation(&self) -> Option<&SimRuntime> {
        self.presentation.as_deref().or(self.runtime.as_ref())
    }

    /// The [`SimWrite::Tick`] route: the installed runtime and the checksum
    /// owner its tick borrows at the native queue boundary, together.
    pub(crate) fn tick_parts(&mut self) -> Option<(&mut SimRuntime, &mut FrameChecksumState)> {
        self.reclaim(true);
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
        self.presentation = None;
        self.lend_cost = None;
        self.runtime = runtime;
    }

    /// Remove and return the installed runtime.
    pub(crate) fn take_runtime(&mut self) -> Option<SimRuntime> {
        self.reclaim(true);
        self.report_early_returns();
        self.job_result = None;
        self.presentation = None;
        self.lend_cost = None;
        self.runtime.take()
    }
}

/// How many lent jobs pass between two log lines on early returns.
const EARLY_RETURN_REPORT_JOBS: u32 = 1024;

/// The simulation thread: runs each lent job and sends the runtime back.
/// It ends when the host drops its sender.
struct SimWorker {
    jobs: mpsc::Sender<(SimRuntime, SimJob)>,
    returned: mpsc::Receiver<Returned>,
}

impl SimWorker {
    fn spawn() -> Self {
        let (jobs, job_queue) = mpsc::channel::<(SimRuntime, SimJob)>();
        let (send_back, returned) = mpsc::channel::<Returned>();
        std::thread::Builder::new()
            .name("simulation".into())
            // The main thread's size: a tick recurses as deep there.
            .stack_size(8 << 20)
            .spawn(move || {
                for (mut runtime, job) in job_queue {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        job(&mut runtime)
                    }));
                    if send_back.send((runtime, result)).is_err() {
                        break;
                    }
                }
            })
            .expect("start the simulation thread");
        Self { jobs, returned }
    }
}

/// Whether every tick publishes a presentation frame for drawing to read,
/// with or without the simulation thread: the phase 2 aid, opted into with
/// `VERA20K_PRESENTATION_FRAME=1`. The thread needs no such frame: it
/// publishes its own when it takes a tick ([`SimHost::lend`]).
pub(crate) fn presentation_frames_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| env_flag("VERA20K_PRESENTATION_FRAME"))
}

/// Whether ordinary offline frames tick on the simulation thread: yes,
/// unless `VERA20K_SIM_THREAD=0` turns it off. The browser build has no
/// thread to lend to and always ticks inline. Unit tests tick inline too:
/// they read the world straight after a frame, and the host's own tests
/// cover lending.
pub(crate) fn sim_thread_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| {
        !cfg!(all(feature = "wasm", target_arch = "wasm32"))
            && !cfg!(test)
            && sim_thread_wanted(std::env::var_os("VERA20K_SIM_THREAD").as_deref())
    })
}

/// What `VERA20K_SIM_THREAD` asks for: only `0` turns the thread off.
fn sim_thread_wanted(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_none_or(|value| value != "0")
}

/// Whether a frame the thread can take goes to it whatever its published
/// frame costs the main thread: `VERA20K_SIM_THREAD=always`, for diagnosis
/// and for exercising the thread. By default it goes only when that saves
/// the main thread time (`sim_tick::advance_in_game_runtime_mode`).
pub(crate) fn sim_thread_always() -> bool {
    static ALWAYS: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ALWAYS.get_or_init(|| {
        sim_thread_enabled()
            && sim_thread_always_wanted(std::env::var_os("VERA20K_SIM_THREAD").as_deref())
    })
}

fn sim_thread_always_wanted(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_some_and(|value| value == "always")
}

fn env_flag(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|value| value == "1")
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
    fn the_simulation_thread_is_on_unless_the_variable_is_zero() {
        use std::ffi::OsStr;
        assert!(sim_thread_wanted(None));
        assert!(sim_thread_wanted(Some(OsStr::new("1"))));
        assert!(!sim_thread_wanted(Some(OsStr::new("0"))));
        // `always` is on too, and alone skips the cost comparison.
        assert!(sim_thread_wanted(Some(OsStr::new("always"))));
        assert!(sim_thread_always_wanted(Some(OsStr::new("always"))));
        assert!(!sim_thread_always_wanted(Some(OsStr::new("1"))));
        assert!(!sim_thread_always_wanted(None));
    }

    #[test]
    fn a_published_frame_keeps_the_tick_it_saw_while_the_world_moves_on() {
        let mut host = SimHost::default();
        host.install(Some(SimRuntime::from_simulation(Simulation::new())));
        host.write_sim(SimWrite::Tick)
            .unwrap()
            .simulation
            .session_mut_for_test()
            .set_tick_for_test(7);
        host.publish_presentation();
        host.write_sim(SimWrite::Tick)
            .unwrap()
            .simulation
            .session_mut_for_test()
            .set_tick_for_test(8);
        assert_eq!(host.presentation().unwrap().simulation.session().tick(), 7);
        assert_eq!(host.runtime().unwrap().simulation.session().tick(), 8);
        host.install(None);
        assert!(host.presentation().is_none());
    }

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

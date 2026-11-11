//! The client's owner of the simulation (docs/sim-thread.md, phase 1).
//!
//! The host lives as long as the process, like the simulation thread it is
//! meant to become. It holds the installed scenario's [`SimRuntime`], if
//! any, and the process-lifetime state a tick reaches outside the world.
//!
//! Phase 3 lends the runtime to the simulation thread for one job
//! ([`SimHost::lend`]). While it is away, reads see the published
//! presentation frame, and any write first takes it back, waiting for the
//! job to end; the job's result then waits in the host for
//! [`SimHost::take_job_result`].
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
    /// The simulation thread, started by the first [`Self::lend`].
    worker: Option<SimWorker>,
    /// The presentation frame (phase 2): the world as the last published
    /// tick left it. Drawing reads it while the live world is ticking
    /// elsewhere. Copy-on-write state (the terrain cells, the resources)
    /// stays shared with the live world until that changes.
    presentation: Option<Arc<SimRuntime>>,
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

    /// Run `job` on the simulation thread with the runtime. Reads serve the
    /// published frame until a write takes the runtime back. Returns false,
    /// running nothing, when no runtime is installed.
    pub(crate) fn lend(&mut self, job: SimJob) -> bool {
        self.reclaim();
        if self.runtime.is_none() {
            return false;
        }
        if self.presentation.is_none() {
            self.publish_presentation();
        }
        let runtime = self.runtime.take().expect("installed runtime");
        self.worker
            .get_or_insert_with(SimWorker::spawn)
            .jobs
            .send((runtime, job))
            .expect("simulation thread stopped");
        self.lent = true;
        true
    }

    /// Wait for a lent runtime to come back. A panic in the job resumes here.
    fn reclaim(&mut self) {
        if !self.lent {
            return;
        }
        self.lent = false;
        let (runtime, result) = self
            .worker
            .as_ref()
            .expect("a lent runtime has a worker")
            .returned
            .recv()
            .expect("simulation thread stopped");
        self.runtime = Some(runtime);
        match result {
            Ok(result) => self.job_result = Some(result),
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }

    /// The last lent job's result, waiting for the job if it still runs.
    pub(crate) fn take_job_result(&mut self) -> Option<SimJobResult> {
        self.reclaim();
        self.job_result.take()
    }

    /// Mutable access to the installed runtime for one write route; a lent
    /// runtime comes back first.
    pub(crate) fn write_sim(&mut self, route: SimWrite) -> Option<&mut SimRuntime> {
        self.reclaim();
        if matches!(route, SimWrite::Rules | SimWrite::Lifecycle) {
            // These may rebind the shared resources (`resources_mut`).
            self.presentation = None;
        }
        self.runtime.as_mut()
    }

    /// Publish the live world as the presentation frame.
    pub(crate) fn publish_presentation(&mut self) {
        if self.lent {
            return;
        }
        self.presentation = self
            .runtime
            .as_ref()
            .map(|runtime| Arc::new(runtime.clone()));
    }

    /// What drawing reads: the published frame, else the live world.
    pub(crate) fn presentation(&self) -> Option<&SimRuntime> {
        self.presentation.as_deref().or(self.runtime.as_ref())
    }

    /// The [`SimWrite::Tick`] route: the installed runtime and the checksum
    /// owner its tick borrows at the native queue boundary, together.
    pub(crate) fn tick_parts(&mut self) -> Option<(&mut SimRuntime, &mut FrameChecksumState)> {
        self.reclaim();
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
        self.reclaim();
        self.job_result = None;
        self.presentation = None;
        self.runtime = runtime;
    }

    /// Remove and return the installed runtime.
    pub(crate) fn take_runtime(&mut self) -> Option<SimRuntime> {
        self.reclaim();
        self.job_result = None;
        self.presentation = None;
        self.runtime.take()
    }
}

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

/// Whether ticks publish a presentation frame: on the simulation thread
/// (phase 4), or opted into with `VERA20K_PRESENTATION_FRAME=1`.
pub(crate) fn presentation_frames_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var_os("VERA20K_PRESENTATION_FRAME").is_some_and(|value| value == "1")
    })
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
        host.publish_presentation();
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
        let result = host.take_job_result().unwrap();
        assert_eq!(*result.downcast::<u32>().unwrap(), 41);
        assert!(host.take_job_result().is_none());
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

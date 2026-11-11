//! The client's owner of the simulation (docs/sim-thread.md, phase 1).
//!
//! The host lives as long as the process, like the simulation thread it is
//! meant to become. It holds the installed scenario's [`SimRuntime`], if
//! any, and the process-lifetime state a tick reaches outside the world.
//!
//! Reads take [`SimHost::runtime`]. A write names its route with
//! [`SimWrite`], so every place that changes the simulation outside
//! presentation is explicit and found by one search. The later phases turn
//! each route into its own operation: commands become submissions, the tick
//! moves to the simulation thread, and the few routes that must stay
//! synchronous are the ones still named here.

use crate::sim::command_queue::{FrameChecksumState, MultiplayerChecksumHistory};
use crate::sim::runtime::SimRuntime;

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
    runtime: Option<SimRuntime>,
    /// The presentation frame (phase 2): the world as the last published
    /// tick left it. Drawing reads it while the live world is ticking
    /// elsewhere. Copy-on-write state (the terrain cells, the resources)
    /// stays shared with the live world until that changes.
    presentation: Option<std::sync::Arc<SimRuntime>>,
    /// Global B04474 and its comparison timer (`FrameChecksumState`):
    /// process state that launch and load retain, written and compared only
    /// inside a multiplayer tick.
    frame_checksums: FrameChecksumState,
}

impl SimHost {
    /// The installed scenario's runtime, for reading.
    pub(crate) fn runtime(&self) -> Option<&SimRuntime> {
        self.runtime.as_ref()
    }

    pub(crate) fn is_installed(&self) -> bool {
        self.runtime.is_some()
    }

    /// Mutable access to the installed runtime for one write route.
    pub(crate) fn write_sim(&mut self, route: SimWrite) -> Option<&mut SimRuntime> {
        if matches!(route, SimWrite::Rules | SimWrite::Lifecycle) {
            // These may rebind the shared resources (`resources_mut`).
            self.presentation = None;
        }
        self.runtime.as_mut()
    }

    /// Publish the live world as the presentation frame.
    pub(crate) fn publish_presentation(&mut self) {
        self.presentation = self
            .runtime
            .as_ref()
            .map(|runtime| std::sync::Arc::new(runtime.clone()));
    }

    /// What drawing reads: the published frame, else the live world.
    pub(crate) fn presentation(&self) -> Option<&SimRuntime> {
        self.presentation.as_deref().or(self.runtime.as_ref())
    }

    /// The [`SimWrite::Tick`] route: the installed runtime and the checksum
    /// owner its tick borrows at the native queue boundary, together.
    pub(crate) fn tick_parts(&mut self) -> Option<(&mut SimRuntime, &mut FrameChecksumState)> {
        let runtime = self.runtime.as_mut()?;
        Some((runtime, &mut self.frame_checksums))
    }

    /// The multiplayer checksum ring the tick records and compares.
    pub(crate) fn checksum_history(&self) -> &MultiplayerChecksumHistory {
        self.frame_checksums.checksum_history()
    }

    /// Install `runtime` in place of any current one.
    pub(crate) fn install(&mut self, runtime: Option<SimRuntime>) {
        self.presentation = None;
        self.runtime = runtime;
    }

    /// Remove and return the installed runtime.
    pub(crate) fn take_runtime(&mut self) -> Option<SimRuntime> {
        self.presentation = None;
        self.runtime.take()
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

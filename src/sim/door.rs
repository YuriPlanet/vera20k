//! Shared Techno `DoorClass` at instance `+0x350`.
//!
//! Original constructor4A50F0, predicates4A5110/4A5130/4A51B0/4A51D0,
//! Open4A51F0, Close4A5240, Reverse4A5290 and Finish4A5360. Gates,
//! factory Unload and Unit transport doors all consume this one object.
//! Native execution: anytown_damage/unit_unlimbo factory continuation.

use crate::sim::timer::CdTimer;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub(crate) enum DoorPhase {
    #[default]
    ClosedStable,
    Opening,
    OpenStable,
    Closing,
}

/// The live timer and direction bytes own gameplay predicates. The native
/// double at +0 is a nominal duration used by drawing progress4A52F0;
/// presentation progress and the timer's unused auxiliary word remain outside
/// this gameplay owner. Constructor4A50F0 initializes neither of those words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct DoorClass {
    timer: CdTimer,
    total_ticks: i32,
    phase: DoorPhase,
}

impl Default for DoorClass {
    fn default() -> Self {
        Self::at_frame(0)
    }
}

impl DoorClass {
    pub(crate) const fn at_frame(frame: u32) -> Self {
        Self {
            timer: CdTimer::started(frame as i32, 0),
            total_ticks: 0,
            phase: DoorPhase::ClosedStable,
        }
    }

    pub(crate) const fn phase(self) -> DoorPhase {
        self.phase
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) const fn timer_fields(self) -> (i32, i32, i32) {
        (
            self.timer.start_frame(),
            self.timer.duration(),
            self.total_ticks,
        )
    }

    ///4A51F0/4A5240 always arm a transition, including zero duration. A
    /// request for the already stable direction is the sole no-op. Due and
    /// stable are separate: TechnoAI6FA5BE..D6 calls Finish at its own slot.
    pub(crate) fn open(&mut self, ticks: u32, frame: u32) {
        if self.phase != DoorPhase::OpenStable {
            self.start(ticks, frame, DoorPhase::Opening);
        }
    }

    pub(crate) fn close(&mut self, ticks: u32, frame: u32) {
        if self.phase != DoorPhase::ClosedStable {
            self.start(ticks, frame, DoorPhase::Closing);
        }
    }

    fn start(&mut self, ticks: u32, frame: u32, phase: DoorPhase) {
        self.total_ticks = ticks as i32;
        self.timer.start(frame as i32, ticks as i32);
        self.phase = phase;
    }

    ///4A5290 retains the original anchor, replaces the duration with total
    /// minus the live remainder, and flips the direction. It does not finish
    /// a now-due transition itself.
    pub(crate) fn reverse(&mut self, frame: u32) {
        let phase = match self.phase {
            DoorPhase::Opening => DoorPhase::Closing,
            DoorPhase::Closing => DoorPhase::Opening,
            _ => return,
        };
        let duration = self
            .total_ticks
            .wrapping_sub(self.timer.remaining(frame as i32));
        self.timer = CdTimer::from_raw(self.timer.start_frame(), duration);
        self.phase = phase;
    }

    /// Drawing progress `0x004A52F0`: the share of the transition done,
    /// `(total - remaining) / total` from the live timer and the total its
    /// Open or Close armed (a Reverse keeps the total), or 1.0 for a stable
    /// door or a zero total. Presentation only.
    pub(crate) fn draw_progress(self, frame: u32) -> f64 {
        if !matches!(self.phase, DoorPhase::Opening | DoorPhase::Closing) || self.total_ticks == 0 {
            return 1.0;
        }
        let done = self
            .total_ticks
            .wrapping_sub(self.timer.remaining(frame as i32));
        f64::from(done) / f64::from(self.total_ticks)
    }

    ///4A5150 followed by4A5360. Finish changes only the active byte;
    /// duration, total and anchor survive stable state and snapshot restore.
    pub(crate) fn advance(&mut self, frame: u32) {
        if self.timer.expired(frame as i32) {
            self.phase = match self.phase {
                DoorPhase::Opening => DoorPhase::OpenStable,
                DoorPhase::Closing => DoorPhase::ClosedStable,
                stable => stable,
            };
        }
    }
}

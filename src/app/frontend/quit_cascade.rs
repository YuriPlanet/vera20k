//! Graceful main-menu quit wait (presentation/teardown only).
//!
//! The original (`Main_PrepareSession @ 0x0052D9A0` case 7, `0x0052E764..
//! 0x0052E816`) runs this teardown as a blocking spin; the port spreads it
//! across frames so the winit event loop keeps turning:
//! - Theme `Stop(1)` starts its own 1000 ms stream fade (the app does that
//!   when the wait starts);
//! - the wait holds while the music still plays and a voice is active, up to a
//!   ceiling (`0x0052E79C..0x0052E7E4`);
//! - then Theme `Stop(0)` (`0x0052E7EC`) and the exit. `Palette__FadeTo
//!   @ 0x004A3C30` (`0x0052E805`) returns at once on a primary surface that
//!   is not 8-bit (`0x004A3C48..0x004A3C4E`), so there is no fade to black.
//!
//! ## Dependency rules
//! - App layer; depends only on std. No render/sim/audio type deps (the app
//!   applies the result to Theme and the event loop), so it stays
//!   headless-testable.

use std::time::Instant;

/// Safety ceiling on the trailing-voice wait: `0xBB8` buckets of
/// `timeGetTime() >> 4` (`0x006C8C40`), 3000 × 16 ms (`0x0052E796`). The
/// Theme fade ends the wait first, so this is reached only if audio never
/// reports done.
const WAIT_CEILING_MS: u64 = 0xBB8 * 16; // 48_000 ms

#[derive(Debug, Clone)]
pub(crate) struct QuitCascade {
    started_at: Instant,
}

impl QuitCascade {
    /// Begin the wait once Theme's `Stop(1)` fade has been requested.
    pub(crate) fn start(now: Instant) -> Self {
        Self { started_at: now }
    }

    /// Whether the wait is over. `waiting` is its loop condition: Theme
    /// `Still_Playing` and `VoxClass::PumpAndCheckActive` both hold.
    pub(crate) fn finished(&self, now: Instant, waiting: bool) -> bool {
        let elapsed_ms = now.duration_since(self.started_at).as_millis() as u64;
        !waiting || elapsed_ms >= WAIT_CEILING_MS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// The wait lasts while the music and a voice both play, and no longer
    /// than the ceiling.
    #[test]
    fn the_wait_ends_when_either_side_stops_or_at_the_ceiling() {
        let t0 = Instant::now();
        let cascade = QuitCascade::start(t0);
        assert!(!cascade.finished(t0 + Duration::from_millis(900), true));
        assert!(cascade.finished(t0 + Duration::from_millis(1_000), false));
        assert!(cascade.finished(t0 + Duration::from_millis(WAIT_CEILING_MS), true));
    }
}

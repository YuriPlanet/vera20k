//! Static `0x71C`: the animated SDWRNANM "WARNING" monitor in the right panel.
//!
//! gamemd.exe gives control `0x71C` of the dialogs listed at
//! `0x0060A7CF..0x0060A8F5` (among them main menu `0xE2`, Single Player
//! `0x100`, Movies & Credits `0x101`, movie list `0x129` and Options `0xD5`)
//! static kind 4 with the SDWRNANM.SHP image (`0x006038C7..0x00603A00`) through
//! the SHELL2 palette path (`0x00603776`). Its initialization
//! (`0x0060A982..0x0060AA0B`) stamps GetTickCount and arms a 1 ms timer
//! (`0x00603240` returns 1). The first WM_TIMER after that millisecond
//! invalidates the static, stores the SHP frame count and re-arms the timer at
//! that many milliseconds (91 for SDWRNANM, `0x00615C49..0x00615D0F`); later
//! timers only invalidate. Every WM_PAINT draws the current frame and then
//! advances it modulo the stored count (`0x006159EB..0x00615A17`), so frame 0
//! is drawn until the first timer has fired.
//!
//! A dialog's first-paint slide runs inside its first WM_PAINT
//! (`0x00612690 -> 0x00608260 -> 0x006071E0`: `Sleep(0x1E)` steps with no
//! message dispatch), so no WM_TIMER reaches the static until the slide ends:
//! the monitor shows frame 0 during the slide and its first timer fires right
//! after it.
//!
//! The frame is centered in the static's window along an axis where the window
//! is larger (`0x0061595E..0x0061597E`).

use std::time::{Duration, Instant};

/// Control id of the monitor static in every dialog that has one.
pub(crate) const WARNING_MONITOR_CONTROL: u16 = 0x071C;

/// Static kind-4 initial timer (`0x00603240` returns 1 ms for `0x71C`).
const STARTUP_TIMER: Duration = Duration::from_millis(1);

/// Retained, paint-driven animation state for one `0x71C` static instance.
/// A new dialog instance starts from `Default` (frame 0, timer unarmed).
///
/// [`WarningMonitor::paint`] returns the frame a recomposition draws; a
/// timer-driven frame advances only once [`WarningMonitor::commit_presented`]
/// confirms that the recomposition reached the screen, so a dropped frame is
/// drawn again instead of being skipped.
#[derive(Debug, Clone, Default)]
pub(crate) struct WarningMonitor {
    started: Option<Instant>,
    /// Next timer deadline once the frame-count timer is armed.
    next_timer: Option<Instant>,
    /// Frame drawn by the last presented paint.
    displayed: usize,
    /// Frame the next timer-driven paint draws.
    next_frame: usize,
    /// A timer fired and its paint has not been presented yet.
    dirty: bool,
    /// Timer-driven frame drawn by the latest recomposition.
    pending: Option<usize>,
}

impl WarningMonitor {
    /// Frame to draw at `now` for an SHP with `frames` frames, or `None` when
    /// the image is missing. `timers` is false while the dialog's first-paint
    /// slide runs: the static then repaints its current frame and no timer is
    /// delivered.
    pub(crate) fn paint(&mut self, now: Instant, frames: usize, timers: bool) -> Option<usize> {
        self.pending = None;
        if frames == 0 {
            return None;
        }
        let started = *self.started.get_or_insert(now);
        if !timers {
            return Some(self.displayed % frames);
        }
        let startup_elapsed = now.saturating_duration_since(started) > STARTUP_TIMER;
        let timer_due = match self.next_timer {
            // Before the first WM_TIMER every paint shows frame 0.
            None => startup_elapsed,
            Some(deadline) => now >= deadline,
        };
        if timer_due {
            let interval = frame_count_interval(frames);
            self.next_timer = Some(match self.next_timer {
                // The first WM_TIMER re-arms the timer from its handler.
                None => now + interval,
                // A periodic timer: missed periods coalesce into one WM_TIMER.
                Some(deadline) => {
                    let periods = now.duration_since(deadline).as_nanos() / interval.as_nanos() + 1;
                    deadline + interval * u32::try_from(periods).unwrap_or(u32::MAX)
                }
            });
            self.dirty = true;
        }
        if self.dirty {
            let frame = self.next_frame % frames;
            self.pending = Some(frame);
            Some(frame)
        } else {
            Some(self.displayed % frames)
        }
    }

    /// The latest recomposition was presented: the native paint advances the
    /// frame after drawing it.
    pub(crate) fn commit_presented(&mut self) {
        if let Some(frame) = self.pending.take() {
            self.displayed = frame;
            self.next_frame = frame + 1;
            self.dirty = false;
        }
    }
}

/// After the first timer the static re-arms at the SHP frame count, read as
/// milliseconds (`0x00615CF8..0x00615D0F`).
fn frame_count_interval(frames: usize) -> Duration {
    Duration::from_millis(frames as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SDWRNANM_FRAMES: usize = 91;

    fn step(monitor: &mut WarningMonitor, now: Instant) -> usize {
        let frame = monitor.paint(now, SDWRNANM_FRAMES, true).expect("frames");
        monitor.commit_presented();
        frame
    }

    #[test]
    fn frame_zero_is_drawn_before_and_at_the_first_timer() {
        let t0 = Instant::now();
        let mut monitor = WarningMonitor::default();
        assert_eq!(step(&mut monitor, t0), 0);
        assert_eq!(step(&mut monitor, t0 + Duration::from_millis(1)), 0);
        // First timer: frame 0 again, then one frame per 91 ms.
        assert_eq!(step(&mut monitor, t0 + Duration::from_millis(16)), 0);
        assert_eq!(step(&mut monitor, t0 + Duration::from_millis(100)), 0);
        assert_eq!(step(&mut monitor, t0 + Duration::from_millis(107)), 1);
        assert_eq!(step(&mut monitor, t0 + Duration::from_millis(198)), 2);
    }

    #[test]
    fn unpresented_timer_paint_is_redrawn_without_advancing() {
        let t0 = Instant::now();
        let mut monitor = WarningMonitor::default();
        assert_eq!(step(&mut monitor, t0), 0);
        assert_eq!(step(&mut monitor, t0 + Duration::from_millis(5)), 0);
        // Frame 1 is drawn but never presented: the next recomposition
        // repeats it instead of skipping to frame 2.
        assert_eq!(
            monitor.paint(t0 + Duration::from_millis(96), SDWRNANM_FRAMES, true),
            Some(1)
        );
        assert_eq!(step(&mut monitor, t0 + Duration::from_millis(110)), 1);
        assert_eq!(step(&mut monitor, t0 + Duration::from_millis(187)), 2);
    }

    #[test]
    fn late_paints_keep_the_periodic_timer_cadence() {
        let t0 = Instant::now();
        let mut monitor = WarningMonitor::default();
        step(&mut monitor, t0);
        // First timer at 2 ms re-arms at 93 ms; paints arriving 10 ms late
        // still see timers at 93, 184, 275 ms, and a stall until 560 ms
        // coalesces the missed periods into one advance (next timer 639 ms).
        assert_eq!(step(&mut monitor, t0 + Duration::from_millis(2)), 0);
        assert_eq!(step(&mut monitor, t0 + Duration::from_millis(103)), 1);
        assert_eq!(step(&mut monitor, t0 + Duration::from_millis(183)), 1);
        assert_eq!(step(&mut monitor, t0 + Duration::from_millis(184)), 2);
        assert_eq!(step(&mut monitor, t0 + Duration::from_millis(560)), 3);
        assert_eq!(step(&mut monitor, t0 + Duration::from_millis(638)), 3);
        assert_eq!(step(&mut monitor, t0 + Duration::from_millis(639)), 4);
    }

    #[test]
    fn startup_timer_matches_the_native_getter() {
        // 0x00603240 for 0x71C, executed by
        // tools/storage_oracle/shell_static_timers.py.
        let cases = crate::ui::shell::static_reveal::tests::native_static_timer_cases();
        for dialog in [0xE2, 0x100, 0x101, 0x129, 0xD5] {
            let case = cases
                .iter()
                .find(|case| {
                    case["dialog_id"] == dialog
                        && case["control_id"] == u64::from(WARNING_MONITOR_CONTROL)
                })
                .expect("monitor case");
            assert_eq!(
                case["kind4_startup_ms"].as_u64(),
                Some(STARTUP_TIMER.as_millis() as u64),
                "dialog {dialog:#x}"
            );
        }
    }

    #[test]
    fn missing_image_draws_nothing() {
        let mut monitor = WarningMonitor::default();
        assert_eq!(monitor.paint(Instant::now(), 0, true), None);
    }

    #[test]
    fn frames_wrap_at_the_shp_frame_count() {
        let t0 = Instant::now();
        let mut monitor = WarningMonitor::default();
        step(&mut monitor, t0);
        // Timer paints every 91 ms after the startup millisecond.
        let frames: Vec<usize> = (0..93)
            .map(|tick| step(&mut monitor, t0 + Duration::from_millis(2 + tick * 91)))
            .collect();
        assert_eq!(&frames[..3], &[0, 1, 2]);
        assert_eq!(&frames[89..], &[89, 90, 0, 1]);
    }

    #[test]
    fn the_slide_holds_frame_zero_and_the_first_timer_fires_after_it() {
        let t0 = Instant::now();
        let mut monitor = WarningMonitor::default();
        // Slide paints (no timers) keep frame 0 however long they run.
        for ms in [0, 16, 300, 540] {
            let frame = monitor.paint(t0 + Duration::from_millis(ms), SDWRNANM_FRAMES, false);
            monitor.commit_presented();
            assert_eq!(frame, Some(0));
        }
        // The pending startup timer fires at once after the slide, then one
        // frame per 91 ms.
        assert_eq!(step(&mut monitor, t0 + Duration::from_millis(556)), 0);
        assert_eq!(step(&mut monitor, t0 + Duration::from_millis(646)), 0);
        assert_eq!(step(&mut monitor, t0 + Duration::from_millis(647)), 1);
    }
}

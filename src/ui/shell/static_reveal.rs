//! Paint-driven state for native kind-1 owner-draw shell statics.
//!
//! A Win32 timer invalidates the child surface, but the reveal count advances
//! only after that child actually paints. VERA20k continuously recomposes the
//! parent, so this render-agnostic state separates the internal count, the last
//! successfully presented count, and one dirty-paint generation. The app layer
//! acknowledges that generation only after swapchain presentation.

use std::time::{Duration, Instant};

/// Native kind-1 timer interval (`0x1e` milliseconds).
pub(crate) const KIND1_TIMER_INTERVAL: Duration = Duration::from_millis(30);
/// Native kind-1 highlight trail used by the active shell statics.
pub(crate) const KIND1_HIGHLIGHT_RANGE: u32 = 8;

#[derive(Debug, Clone)]
pub(crate) struct Kind1StaticReveal {
    phase: RevealPhase,
    next_generation: u64,
}

#[derive(Debug, Clone)]
enum RevealPhase {
    Waiting,
    Running(RunningReveal),
}

#[derive(Debug, Clone)]
struct RunningReveal {
    internal_count: u32,
    displayed_count: Option<u32>,
    target_count: u32,
    next_timer_at: Option<Instant>,
    dirty: Option<DirtyPaint>,
}

#[derive(Debug, Clone, Copy)]
struct DirtyPaint {
    count: u32,
    generation: u64,
}

/// The count/range the Path-A renderer must use for one child paint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Kind1RevealWindow {
    pub count: u32,
    pub range: u32,
}

/// Opaque one-use acknowledgement for one encoded dirty generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Kind1RevealReceipt {
    generation: u64,
}

/// What a whole-window recomposition should display for this retained child.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind1PaintWindow {
    /// The child is waiting for the shell SHOW completion message.
    Hidden,
    /// Repaint the last successfully presented child pixels without advancing.
    Retained(Kind1RevealWindow),
    /// Encode the dirty count and return its post-present acknowledgement.
    Due {
        window: Kind1RevealWindow,
        receipt: Kind1RevealReceipt,
    },
}

impl Default for Kind1StaticReveal {
    fn default() -> Self {
        Self {
            phase: RevealPhase::Waiting,
            next_generation: 1,
        }
    }
}

impl Kind1StaticReveal {
    /// Recreate the native child-equivalent waiting state.
    pub(crate) fn reset_waiting(&mut self) {
        self.phase = RevealPhase::Waiting;
    }

    /// Start once from Waiting. The count-1 paint is immediately dirty.
    pub(crate) fn start(&mut self, text: &str, now: Instant) -> bool {
        if !matches!(self.phase, RevealPhase::Waiting) {
            return false;
        }
        let Ok(utf16_units) = u32::try_from(text.encode_utf16().count()) else {
            return false;
        };
        let Some(target_count) = utf16_units
            .checked_add(1)
            .and_then(|value| value.checked_add(KIND1_HIGHLIGHT_RANGE))
        else {
            return false;
        };
        let generation = self.allocate_generation();
        self.phase = RevealPhase::Running(RunningReveal {
            internal_count: 1,
            displayed_count: None,
            target_count,
            next_timer_at: now.checked_add(KIND1_TIMER_INTERVAL),
            dirty: Some(DirtyPaint {
                count: 1,
                generation,
            }),
        });
        true
    }

    /// Deliver/coalesce timer invalidation without changing the reveal count.
    pub(crate) fn poll_timer(&mut self, now: Instant) -> bool {
        let needs_generation = {
            let RevealPhase::Running(running) = &mut self.phase else {
                return false;
            };
            let Some(deadline) = running.next_timer_at else {
                return false;
            };
            if now < deadline {
                return false;
            }
            running.next_timer_at = Some(next_deadline_after(deadline, now));
            running.dirty.is_none()
        };

        if needs_generation {
            let generation = self.allocate_generation();
            let RevealPhase::Running(running) = &mut self.phase else {
                unreachable!("phase cannot change while allocating a generation");
            };
            running.dirty = Some(DirtyPaint {
                count: running.internal_count,
                generation,
            });
        }
        true
    }

    /// Return the hidden, retained, or dirty child content for this frame.
    pub(crate) fn paint_window(&self) -> Kind1PaintWindow {
        let RevealPhase::Running(running) = &self.phase else {
            return Kind1PaintWindow::Hidden;
        };
        if let Some(dirty) = running.dirty {
            return Kind1PaintWindow::Due {
                window: Kind1RevealWindow {
                    count: dirty.count,
                    range: KIND1_HIGHLIGHT_RANGE,
                },
                receipt: Kind1RevealReceipt {
                    generation: dirty.generation,
                },
            };
        }
        running
            .displayed_count
            .map_or(Kind1PaintWindow::Hidden, |count| {
                Kind1PaintWindow::Retained(Kind1RevealWindow {
                    count,
                    range: KIND1_HIGHLIGHT_RANGE,
                })
            })
    }

    /// Commit one dirty child paint after its encoded frame successfully presents.
    pub(crate) fn record_presented(&mut self, receipt: Kind1RevealReceipt) -> bool {
        let RevealPhase::Running(running) = &mut self.phase else {
            return false;
        };
        let Some(dirty) = running.dirty else {
            return false;
        };
        if dirty.generation != receipt.generation {
            return false;
        }

        running.dirty = None;
        running.displayed_count = Some(dirty.count);
        running.internal_count = running.internal_count.saturating_add(1);
        if running.internal_count >= running.target_count {
            running.internal_count = running.target_count;
            running.next_timer_at = None;
        }
        true
    }

    /// Internal completion with the final invalidated paint still retained.
    pub(crate) fn is_terminal_persistent(&self) -> bool {
        let RevealPhase::Running(running) = &self.phase else {
            return false;
        };
        running.internal_count == running.target_count
            && running.displayed_count == running.target_count.checked_sub(1)
            && running.dirty.is_none()
            && running.next_timer_at.is_none()
    }

    #[cfg(test)]
    fn target_count(&self) -> Option<u32> {
        match &self.phase {
            RevealPhase::Waiting => None,
            RevealPhase::Running(running) => Some(running.target_count),
        }
    }

    fn allocate_generation(&mut self) -> u64 {
        let generation = self.next_generation;
        self.next_generation = self.next_generation.checked_add(1).unwrap_or(1);
        generation
    }
}

/// A kind-1 static whose dirty paint is committed by the frame loop once the
/// recomposition that drew it has been presented.
#[derive(Debug, Clone, Default)]
pub(crate) struct PresentedKind1Static {
    reveal: Kind1StaticReveal,
    pending: Option<Kind1RevealReceipt>,
}

impl PresentedKind1Static {
    /// The shell SHOW completion starts the reveal once (`0x4EC -> 0x4EE`).
    pub(crate) fn start(&mut self, text: &str, now: Instant) -> bool {
        self.reveal.start(text, now)
    }

    /// Reveal window for this recomposition, `None` while the child is hidden.
    pub(crate) fn paint(&mut self, now: Instant) -> Option<Kind1RevealWindow> {
        self.reveal.poll_timer(now);
        match self.reveal.paint_window() {
            Kind1PaintWindow::Hidden => {
                self.pending = None;
                None
            }
            Kind1PaintWindow::Retained(window) => {
                self.pending = None;
                Some(window)
            }
            Kind1PaintWindow::Due { window, receipt } => {
                self.pending = Some(receipt);
                Some(window)
            }
        }
    }

    /// The latest recomposition was presented.
    pub(crate) fn commit_presented(&mut self) {
        if let Some(receipt) = self.pending.take() {
            self.reveal.record_presented(receipt);
        }
    }

    /// The reveal ran to completion and its final paint is on screen.
    pub(crate) fn is_terminal(&self) -> bool {
        self.pending.is_none() && self.reveal.is_terminal_persistent()
    }
}

fn next_deadline_after(deadline: Instant, now: Instant) -> Instant {
    let overdue = now.duration_since(deadline);
    let intervals = overdue.as_nanos() / KIND1_TIMER_INTERVAL.as_nanos() + 1;
    let Ok(intervals) = u32::try_from(intervals) else {
        return now.checked_add(KIND1_TIMER_INTERVAL).unwrap_or(deadline);
    };
    deadline
        .checked_add(KIND1_TIMER_INTERVAL * intervals)
        .unwrap_or_else(|| now.checked_add(KIND1_TIMER_INTERVAL).unwrap_or(deadline))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn due(reveal: &Kind1StaticReveal) -> (Kind1RevealWindow, Kind1RevealReceipt) {
        let Kind1PaintWindow::Due { window, receipt } = reveal.paint_window() else {
            panic!("expected dirty paint");
        };
        (window, receipt)
    }

    #[test]
    fn waiting_is_hidden_and_timer_is_inert() {
        let mut reveal = Kind1StaticReveal::default();
        let now = Instant::now();
        assert_eq!(reveal.paint_window(), Kind1PaintWindow::Hidden);
        assert!(!reveal.poll_timer(now + Duration::from_secs(1)));
        assert!(!reveal.is_terminal_persistent());
    }

    /// Native getters executed under Unicorn by
    /// `tools/storage_oracle/shell_static_timers.py`.
    pub(crate) fn native_static_timer_cases() -> Vec<serde_json::Value> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tools/storage_oracle/shell_static_timers.json"
        ))
        .unwrap();
        fixture["cases"].as_array().unwrap().clone()
    }

    #[test]
    fn heading_reveal_parameters_match_the_native_getters() {
        // Heading 0x694 of the right-panel dialogs: kind 1 (0x00602490),
        // interval 0x00600CA0, step 0x006015E0 (the reveal advances by one),
        // range 0x00601D20.
        let cases = native_static_timer_cases();
        for dialog in [0xE2, 0x100, 0x101, 0x129, 0xD5] {
            let case = cases
                .iter()
                .find(|case| case["dialog_id"] == dialog && case["control_id"] == 0x694)
                .expect("heading case");
            assert_eq!(case["kind1"], 1, "dialog {dialog:#x}");
            assert_eq!(
                case["interval_ms"].as_u64(),
                Some(KIND1_TIMER_INTERVAL.as_millis() as u64)
            );
            assert_eq!(case["step"], 1);
            assert_eq!(
                case["range"].as_u64(),
                Some(u64::from(KIND1_HIGHLIGHT_RANGE))
            );
        }
    }

    #[test]
    fn start_uses_exact_utf16_target_and_is_single_shot() {
        let mut reveal = Kind1StaticReveal::default();
        let now = Instant::now();
        assert!(reveal.start("A😀B", now));
        // A + surrogate pair + B = four UTF-16 units.
        assert_eq!(reveal.target_count(), Some(4 + 1 + 8));
        assert!(!reveal.start("replacement", now));
        assert_eq!(due(&reveal).0.count, 1);
    }

    #[test]
    fn timer_without_present_never_advances_or_replaces_dirty_generation() {
        let mut reveal = Kind1StaticReveal::default();
        let now = Instant::now();
        assert!(reveal.start("abc", now));
        let first = due(&reveal);
        assert!(reveal.poll_timer(now + Duration::from_millis(95)));
        let after_timers = due(&reveal);
        assert_eq!(after_timers, first);
        assert_eq!(after_timers.0.count, 1);
    }

    #[test]
    fn presented_count_is_retained_until_the_next_timer_invalidation() {
        let mut reveal = Kind1StaticReveal::default();
        let now = Instant::now();
        assert!(reveal.start("abc", now));
        let (_, first) = due(&reveal);
        assert!(reveal.record_presented(first));
        assert_eq!(
            reveal.paint_window(),
            Kind1PaintWindow::Retained(Kind1RevealWindow { count: 1, range: 8 })
        );
        assert!(!reveal.poll_timer(now + Duration::from_millis(29)));
        assert!(reveal.poll_timer(now + Duration::from_millis(30)));
        assert_eq!(due(&reveal).0.count, 2);
    }

    #[test]
    fn stale_duplicate_and_reset_receipts_never_advance() {
        let mut reveal = Kind1StaticReveal::default();
        let now = Instant::now();
        assert!(reveal.start("abc", now));
        let (_, old_receipt) = due(&reveal);
        assert!(reveal.record_presented(old_receipt));
        assert!(!reveal.record_presented(old_receipt));

        assert!(reveal.poll_timer(now + Duration::from_millis(30)));
        let (_, current_receipt) = due(&reveal);
        assert!(!reveal.record_presented(old_receipt));
        assert_eq!(due(&reveal).1, current_receipt);

        reveal.reset_waiting();
        assert_eq!(reveal.paint_window(), Kind1PaintWindow::Hidden);
        assert!(!reveal.record_presented(current_receipt));
        assert!(reveal.start("abc", now + Duration::from_secs(1)));
        assert_ne!(due(&reveal).1, current_receipt);
        assert!(!reveal.record_presented(current_receipt));
    }

    #[test]
    fn main_menu_retains_count_17_after_internal_count_reaches_18() {
        let mut reveal = Kind1StaticReveal::default();
        let start = Instant::now();
        assert!(reveal.start("Main Menu", start));
        assert_eq!(reveal.target_count(), Some(18));

        for count in 1..=17 {
            let (window, receipt) = due(&reveal);
            assert_eq!(window.count, count);
            assert!(reveal.record_presented(receipt));
            if count < 17 {
                assert!(reveal.poll_timer(start + KIND1_TIMER_INTERVAL * count));
            }
        }

        assert!(reveal.is_terminal_persistent());
        assert_eq!(
            reveal.paint_window(),
            Kind1PaintWindow::Retained(Kind1RevealWindow {
                count: 17,
                range: 8,
            })
        );
        assert!(!reveal.poll_timer(start + Duration::from_secs(10)));
    }
}

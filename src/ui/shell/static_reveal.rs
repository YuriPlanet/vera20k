//! Paint-driven state for native kind-1 owner-draw shell statics.
//!
//! A Win32 timer invalidates the child surface, but the reveal count advances
//! only after that child actually paints. VERA20k continuously recomposes the
//! parent, so this render-agnostic state separates the internal count, the last
//! successfully presented count, and one dirty-paint generation. The app layer
//! acknowledges that generation only after swapchain presentation.

use std::time::{Duration, Instant};

/// Native kind-1 timer interval of the right-panel heading `0x694`
/// (`0x1e` milliseconds).
pub(crate) const KIND1_TIMER_INTERVAL: Duration = Duration::from_millis(30);
/// Native kind-1 highlight trail of the right-panel heading `0x694`.
pub(crate) const KIND1_HIGHLIGHT_RANGE: u32 = 8;

/// One kind-1 static's reveal parameters: timer interval (`0x00600CA0`),
/// count step per paint (`0x006015E0`) and highlight range (`0x00601D20`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Kind1Params {
    pub interval: Duration,
    pub step: u32,
    pub range: u32,
}

/// Heading `0x694` of the right-panel shell dialogs.
pub(crate) const HEADING_KIND1: Kind1Params = Kind1Params {
    interval: KIND1_TIMER_INTERVAL,
    step: 1,
    range: KIND1_HIGHLIGHT_RANGE,
};

/// Status line `0x695` of the right-panel shell dialogs outside a network
/// session.
pub(crate) const STATUS_LINE_KIND1: Kind1Params = Kind1Params {
    interval: Duration::from_millis(15),
    step: 3,
    range: 16,
};

#[derive(Debug, Clone)]
pub(crate) struct Kind1StaticReveal {
    params: Kind1Params,
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

/// The heading's parameters ([`HEADING_KIND1`]).
impl Default for Kind1StaticReveal {
    fn default() -> Self {
        Self::with_params(HEADING_KIND1)
    }
}

impl Kind1StaticReveal {
    pub(crate) fn with_params(params: Kind1Params) -> Self {
        Self {
            params,
            phase: RevealPhase::Waiting,
            next_generation: 1,
        }
    }

    pub(crate) fn params(&self) -> Kind1Params {
        self.params
    }

    /// Recreate the native child-equivalent waiting state.
    pub(crate) fn reset_waiting(&mut self) {
        self.phase = RevealPhase::Waiting;
    }

    /// `0x4EE` has started the reveal (the native `+0xA8` flag); it stays set
    /// after the last paint.
    pub(crate) fn is_started(&self) -> bool {
        matches!(self.phase, RevealPhase::Running(_))
    }

    /// Text replaced while started (`0x4B2`): kill the timer and start again
    /// from count 1 (`0x00611C8A..0x00611CAF`).
    pub(crate) fn restart(&mut self, text: &str, now: Instant) -> bool {
        self.phase = RevealPhase::Waiting;
        self.start(text, now)
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
            .and_then(|value| value.checked_add(self.params.range))
        else {
            return false;
        };
        let generation = self.allocate_generation();
        self.phase = RevealPhase::Running(RunningReveal {
            internal_count: 1,
            displayed_count: None,
            target_count,
            next_timer_at: now.checked_add(self.params.interval),
            dirty: Some(DirtyPaint {
                count: 1,
                generation,
            }),
        });
        true
    }

    /// Deliver/coalesce timer invalidation without changing the reveal count.
    pub(crate) fn poll_timer(&mut self, now: Instant) -> bool {
        let interval = self.params.interval;
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
            running.next_timer_at = Some(next_deadline_after(deadline, now, interval));
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
                    range: self.params.range,
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
                    range: self.params.range,
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

        // WM_PAINT draws, then advances the count while it is below
        // `units + 1 + range` and kills the timer once it gets there
        // (`0x00615B0D..0x00615B49`); a later paint only redraws.
        running.dirty = None;
        running.displayed_count = Some(dirty.count);
        if running.internal_count < running.target_count {
            running.internal_count = running.internal_count.saturating_add(self.params.step);
            if running.internal_count >= running.target_count {
                running.next_timer_at = None;
            }
        }
        true
    }

    /// An invalidation outside the timer (a hover message reaching the status
    /// line, `0x00615EF7`): the next paint draws the current count, which
    /// then advances as any paint does.
    pub(crate) fn invalidate(&mut self) {
        let needs_generation = matches!(
            &self.phase,
            RevealPhase::Running(running) if running.dirty.is_none()
        );
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
    }

    /// Internal completion with the final invalidated paint still retained.
    pub(crate) fn is_terminal_persistent(&self) -> bool {
        let RevealPhase::Running(running) = &self.phase else {
            return false;
        };
        running.internal_count >= running.target_count
            && running.displayed_count.is_some()
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
/// recomposition that drew it has been presented. It owns the static's text
/// (native `+0x28`). Built with its native parameters ([`HEADING_KIND1`],
/// [`STATUS_LINE_KIND1`]).
#[derive(Debug, Clone)]
pub(crate) struct PresentedKind1Static {
    reveal: Kind1StaticReveal,
    pending: Option<Kind1RevealReceipt>,
    text: String,
}

impl PresentedKind1Static {
    pub(crate) fn new(params: Kind1Params) -> Self {
        Self {
            reveal: Kind1StaticReveal::with_params(params),
            pending: None,
            text: String::new(),
        }
    }

    /// A new dialog instance's static: hidden, no text, same parameters.
    pub(crate) fn reset(&mut self) {
        *self = Self::new(self.reveal.params());
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    #[cfg(test)]
    pub(crate) fn params(&self) -> Kind1Params {
        self.reveal.params()
    }

    /// `0x4B2`: replace the text. A started reveal whose text changed starts
    /// again from count 1; before the SHOW completion the text is only stored.
    pub(crate) fn set_text(&mut self, text: &str, now: Instant) {
        if self.text == text {
            return;
        }
        self.text.clear();
        self.text.push_str(text);
        if self.reveal.is_started() {
            self.pending = None;
            self.reveal.restart(text, now);
        }
    }

    /// A paint outside the timer. A hover message (`0x4B2`) restores the
    /// saved background and invalidates the static besides replacing the
    /// text (`0x00615EF7..0x00615F75`), so it repaints — and a running reveal
    /// advances — on every hover message; a static shown again repaints at
    /// its count, which after the last timer paint is the target.
    pub(crate) fn repaint(&mut self) {
        self.reveal.invalidate();
    }

    /// The shell SHOW completion starts the reveal once with the current text
    /// (`0x4EC -> 0x0060AA60 -> 0x4EE`).
    pub(crate) fn start(&mut self, now: Instant) -> bool {
        self.reveal.start(&self.text, now)
    }

    /// The SHOW completion of a dialog that may show again: `0x4EE` starts
    /// only a static that has not started (`0x00615FDB`); a started one
    /// repaints at its count as its dialog shows.
    pub(crate) fn show(&mut self, now: Instant) {
        if !self.start(now) {
            self.repaint();
        }
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

    /// [`Self::paint`] with the static's text.
    pub(crate) fn paint_text(&mut self, now: Instant) -> Option<StaticPaint<'_>> {
        let window = self.paint(now)?;
        Some(StaticPaint {
            text: &self.text,
            window,
        })
    }
}

/// One shown static in a recomposition: its text and reveal window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StaticPaint<'a> {
    pub text: &'a str,
    pub window: Kind1RevealWindow,
}

/// Heading `0x694` and status line `0x695` of a family dialog that another
/// dialog hides or covers without destroying (`0x6B` under `0x105`, `0x105`
/// under the seed browser), so they outlive the other dialog's run: text,
/// started flag and count stay, and its SHOW completion repaints what
/// already started ([`PresentedKind1Static::show`]).
#[derive(Debug, Clone)]
pub(crate) struct DialogStatics {
    heading: PresentedKind1Static,
    /// Holds the last hover help.
    status_line: PresentedKind1Static,
}

impl Default for DialogStatics {
    fn default() -> Self {
        Self {
            heading: PresentedKind1Static::new(HEADING_KIND1),
            status_line: PresentedKind1Static::new(STATUS_LINE_KIND1),
        }
    }
}

impl DialogStatics {
    /// The SHOW completion with the heading the dialog holds.
    pub(crate) fn show(&mut self, heading: &str, now: Instant) {
        self.heading.set_text(heading, now);
        self.heading.show(now);
        self.status_line.show(now);
    }

    /// A hover message to the status line (`0x4B2`): a changed help restarts
    /// a started reveal, and every message repaints the line
    /// (`0x00615EF7`). Returns whether the help changed.
    pub(crate) fn hover(&mut self, help: &str, now: Instant) -> bool {
        let changed = self.status_line.text() != help;
        self.status_line.set_text(help, now);
        self.status_line.repaint();
        changed
    }

    /// The dialog shows again (`ShowWindow`) or another dialog stops
    /// covering it: both statics repaint at their counts. An entry slide
    /// blits the top panel over the heading, while the status line, outside
    /// the blits and validate-only while the slide runs (`0x00606800` through
    /// `0x00601360`), keeps those pixels (retail `rmg-cancel.png`).
    pub(crate) fn shown_again(&mut self) {
        self.heading.repaint();
        self.status_line.repaint();
    }

    pub(crate) fn paint_heading(&mut self, now: Instant) -> Option<StaticPaint<'_>> {
        self.heading.paint_text(now)
    }

    pub(crate) fn paint_status_line(&mut self, now: Instant) -> Option<StaticPaint<'_>> {
        self.status_line.paint_text(now)
    }

    pub(crate) fn commit_presented(&mut self) {
        self.heading.commit_presented();
        self.status_line.commit_presented();
    }

    /// Both reveals ran to completion and their last paints are on screen.
    pub(crate) fn is_terminal(&self) -> bool {
        self.heading.is_terminal() && self.status_line.is_terminal()
    }
}

fn next_deadline_after(deadline: Instant, now: Instant, interval: Duration) -> Instant {
    let overdue = now.duration_since(deadline);
    let intervals = overdue.as_nanos() / interval.as_nanos() + 1;
    let Ok(intervals) = u32::try_from(intervals) else {
        return now.checked_add(interval).unwrap_or(deadline);
    };
    deadline
        .checked_add(interval * intervals)
        .unwrap_or_else(|| now.checked_add(interval).unwrap_or(deadline))
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

    fn native_params(dialog: u64, control: u64) -> Kind1Params {
        let cases = native_static_timer_cases();
        let case = cases
            .iter()
            .find(|case| case["dialog_id"] == dialog && case["control_id"] == control)
            .unwrap_or_else(|| panic!("case {dialog:#x}/{control:#x}"));
        assert_eq!(case["kind1"], 1, "{dialog:#x}/{control:#x} is kind 1");
        let value = |key: &str| case[key].as_u64().unwrap();
        Kind1Params {
            interval: Duration::from_millis(value("interval_ms")),
            step: value("step") as u32,
            range: value("range") as u32,
        }
    }

    #[test]
    fn heading_and_status_line_parameters_match_the_native_getters() {
        // Heading 0x694 and status line 0x695: kind 1 (0x00602490), interval
        // 0x00600CA0, step 0x006015E0, range 0x00601D20, executed natively.
        for dialog in [0xE2, 0x100, 0x101, 0x129, 0xD5, 0x102, 0xB7, 0x6B, 0x105] {
            assert_eq!(native_params(dialog, 0x694), HEADING_KIND1, "{dialog:#x}");
            assert_eq!(
                native_params(dialog, 0x695),
                STATUS_LINE_KIND1,
                "{dialog:#x}"
            );
        }
    }

    fn present(reveal: &mut PresentedKind1Static, now: Instant) -> Option<Kind1RevealWindow> {
        let window = reveal.paint(now);
        reveal.commit_presented();
        window
    }

    #[test]
    fn status_line_paints_every_third_unit_and_stops_at_its_target() {
        let t0 = Instant::now();
        let mut status = PresentedKind1Static::new(STATUS_LINE_KIND1);
        status.set_text("Hello", t0);
        // Hidden until the SHOW completion starts it.
        assert_eq!(status.paint(t0), None);
        assert!(status.start(t0));
        // "Hello": target 5 + 1 + 16 = 22; counts 1, 4, ..., 19 are painted.
        let counts: Vec<u32> = (0..8)
            .map(|tick| {
                present(&mut status, t0 + Duration::from_millis(15 * tick))
                    .expect("started")
                    .count
            })
            .collect();
        assert_eq!(counts, [1, 4, 7, 10, 13, 16, 19, 19]);
        assert!(status.is_terminal());
        assert_eq!(
            status.paint(t0 + Duration::from_secs(1)),
            Some(Kind1RevealWindow {
                count: 19,
                range: 16
            })
        );
    }

    #[test]
    fn hover_messages_repaint_the_status_line_and_advance_a_running_reveal() {
        let t0 = Instant::now();
        let mut status = PresentedKind1Static::new(STATUS_LINE_KIND1);
        status.set_text("Hello", t0);
        assert!(status.start(t0));
        assert_eq!(present(&mut status, t0).map(|w| w.count), Some(1));
        // A hover paint between timer ticks draws the current count and then
        // advances it, like any paint.
        status.repaint();
        assert_eq!(
            present(&mut status, t0 + Duration::from_millis(5)).map(|w| w.count),
            Some(4)
        );
        assert_eq!(
            present(&mut status, t0 + Duration::from_millis(15)).map(|w| w.count),
            Some(7)
        );
        for tick in 2..8 {
            present(&mut status, t0 + Duration::from_millis(15 * tick));
        }
        assert!(status.is_terminal());
        let finished = status.paint(t0 + Duration::from_secs(1)).expect("shown");
        assert_eq!(finished.count, 19);
        // After the last timer paint a hover paint draws the final count
        // (22 >= target: every unit plain) and stops advancing there.
        status.repaint();
        assert_eq!(
            present(&mut status, t0 + Duration::from_secs(1)).map(|w| w.count),
            Some(22)
        );
        status.repaint();
        assert_eq!(
            present(&mut status, t0 + Duration::from_secs(2)).map(|w| w.count),
            Some(22)
        );
        assert!(status.is_terminal());
    }

    #[test]
    fn changed_text_restarts_a_started_reveal_but_not_an_unstarted_one() {
        let t0 = Instant::now();
        let mut status = PresentedKind1Static::new(STATUS_LINE_KIND1);
        status.set_text("A", t0);
        status.set_text("B", t0);
        assert_eq!(status.paint(t0), None);
        assert!(status.start(t0));
        assert_eq!(status.text(), "B");
        for tick in 0..8 {
            present(&mut status, t0 + Duration::from_millis(15 * tick));
        }
        assert!(status.is_terminal());
        // The same text does not restart; a different one starts at count 1.
        status.set_text("B", t0 + Duration::from_millis(200));
        assert!(status.is_terminal());
        status.set_text("Other", t0 + Duration::from_millis(200));
        assert_eq!(
            present(&mut status, t0 + Duration::from_millis(200)).map(|w| w.count),
            Some(1)
        );
        assert!(!status.is_terminal());
        // Leaving every control clears the text, which restarts it too.
        status.set_text("", t0 + Duration::from_millis(215));
        assert_eq!(status.text(), "");
        assert_eq!(
            present(&mut status, t0 + Duration::from_millis(215)).map(|w| w.count),
            Some(1)
        );
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

    /// Paint and present both statics once: (heading, status line) counts.
    fn present_dialog(statics: &mut DialogStatics, now: Instant) -> (Option<u32>, Option<u32>) {
        let heading = statics.paint_heading(now).map(|shown| shown.window.count);
        let status = statics
            .paint_status_line(now)
            .map(|shown| shown.window.count);
        statics.commit_presented();
        (heading, status)
    }

    #[test]
    fn dialog_statics_start_once_and_repaint_when_the_dialog_shows_again() {
        let t0 = Instant::now();
        let mut statics = DialogStatics::default();
        // Help written before the SHOW completion is only stored.
        assert!(statics.hover("Help", t0));
        assert_eq!(present_dialog(&mut statics, t0), (None, None));
        statics.show("Choose Map", t0);
        assert_eq!(present_dialog(&mut statics, t0), (Some(1), Some(1)));
        let mut now = t0;
        while !statics.is_terminal() {
            now += Duration::from_millis(15);
            present_dialog(&mut statics, now);
        }
        // "Choose Map" (10 units): the last timer paint drew 18 of 19;
        // "Help": 19 of 21 (step 3).
        assert_eq!(present_dialog(&mut statics, now), (Some(18), Some(19)));
        // Shown again after another dialog hid it, or uncovered: both repaint
        // at their counts, which after the last timer paint is the target.
        statics.shown_again();
        assert_eq!(present_dialog(&mut statics, now), (Some(19), Some(22)));
        // The SHOW completion after an entry slide repaints without a restart.
        statics.show("Choose Map", now);
        assert_eq!(present_dialog(&mut statics, now), (Some(19), Some(22)));
        assert!(statics.is_terminal());
    }
}

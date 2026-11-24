//! Paint-driven state for native kind-1 owner-draw shell statics.
//!
//! A Win32 timer invalidates the child surface, but the reveal count advances
//! only after that child actually paints. VERA20k continuously recomposes the
//! parent, so this render-agnostic state separates the internal count, the last
//! successfully presented count and one dirty paint. The app layer commits the
//! dirty paint only after swapchain presentation.

use vera20k_gui::time::{Duration, Instant};

/// Native kind-1 timer interval of the right-panel heading `0x694`
/// (`0x1e` milliseconds).
pub const KIND1_TIMER_INTERVAL: Duration = Duration::from_millis(30);

/// One kind-1 static's reveal parameters: timer interval (`0x00600CA0`),
/// count step per paint (`0x006015E0`) and highlight range (`0x00601D20`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Kind1Params {
    pub interval: Duration,
    pub step: u32,
    pub range: u32,
}

/// Heading `0x694` of the right-panel shell dialogs.
pub const HEADING_KIND1: Kind1Params = Kind1Params {
    interval: KIND1_TIMER_INTERVAL,
    step: 1,
    range: 8,
};

/// Status line `0x695` of the right-panel shell dialogs outside a network
/// session.
pub const STATUS_LINE_KIND1: Kind1Params = Kind1Params {
    interval: Duration::from_millis(15),
    step: 3,
    range: 16,
};

/// The count/range the Path-A renderer must use for one child paint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Kind1RevealWindow {
    pub count: u32,
    pub range: u32,
}

/// A started reveal (`0x4EE` set the native `+0xA8` flag; it stays set after
/// the last paint).
#[derive(Debug, Clone)]
struct RunningReveal {
    internal_count: u32,
    displayed_count: Option<u32>,
    target_count: u32,
    next_timer_at: Option<Instant>,
    /// The count an invalidation asks the next paint to draw.
    dirty: Option<u32>,
}

/// A kind-1 static whose dirty paint is committed by the frame loop once the
/// recomposition that drew it has been presented. It owns the static's text
/// (native `+0x28`). Built with its native parameters ([`HEADING_KIND1`],
/// [`STATUS_LINE_KIND1`]).
#[derive(Debug, Clone)]
pub struct PresentedKind1Static {
    params: Kind1Params,
    running: Option<RunningReveal>,
    /// The latest recomposition drew the dirty paint.
    pending: bool,
    text: String,
}

impl PresentedKind1Static {
    pub fn new(params: Kind1Params) -> Self {
        Self {
            params,
            running: None,
            pending: false,
            text: String::new(),
        }
    }

    /// A new dialog instance's static: hidden, no text, same parameters.
    pub fn reset(&mut self) {
        *self = Self::new(self.params);
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    #[cfg(test)]
    pub fn params(&self) -> Kind1Params {
        self.params
    }

    /// `0x4B2`: replace the text. A started reveal whose text changed kills
    /// its timer and starts again from count 1 (`0x00611C8A..0x00611CAF`);
    /// before the SHOW completion the text is only stored.
    pub fn set_text(&mut self, text: &str, now: Instant) {
        if self.text == text {
            return;
        }
        self.text.clear();
        self.text.push_str(text);
        if self.running.take().is_some() {
            self.start(now);
        }
    }

    /// A paint outside the timer. A hover message (`0x4B2`) restores the
    /// saved background and invalidates the static besides replacing the
    /// text (`0x00615EF7..0x00615F75`), so it repaints — and a running reveal
    /// advances — on every hover message; a static shown again repaints at
    /// its count, which after the last timer paint is the target.
    pub fn repaint(&mut self) {
        if let Some(running) = self.running.as_mut() {
            running.dirty.get_or_insert(running.internal_count);
        }
    }

    /// The shell SHOW completion starts the reveal once with the current text
    /// (`0x4EC -> 0x0060AA60 -> 0x4EE`); the count-1 paint is immediately
    /// dirty. The target is the text's UTF-16 units + 1 + the range.
    pub fn start(&mut self, now: Instant) -> bool {
        if self.running.is_some() {
            return false;
        }
        let Some(target_count) = u32::try_from(self.text.encode_utf16().count())
            .ok()
            .and_then(|units| units.checked_add(1 + self.params.range))
        else {
            return false;
        };
        self.pending = false;
        self.running = Some(RunningReveal {
            internal_count: 1,
            displayed_count: None,
            target_count,
            next_timer_at: now.checked_add(self.params.interval),
            dirty: Some(1),
        });
        true
    }

    /// The SHOW completion of a dialog that may show again: `0x4EE` starts
    /// only a static that has not started (`0x00615FDB`); a started one
    /// repaints at its count as its dialog shows.
    pub fn show(&mut self, now: Instant) {
        if !self.start(now) {
            self.repaint();
        }
    }

    /// Reveal window for this recomposition, `None` while the child is hidden.
    /// A due timer invalidates the static without changing its count.
    pub fn paint(&mut self, now: Instant) -> Option<Kind1RevealWindow> {
        if let Some(running) = self.running.as_mut()
            && let Some(deadline) = running.next_timer_at
            && now >= deadline
        {
            running.next_timer_at = Some(next_deadline_after(deadline, now, self.params.interval));
            running.dirty.get_or_insert(running.internal_count);
        }
        self.paint_undelivered()
    }

    /// [`Self::paint`] with no timer delivered: the dirty count, else the last
    /// presented one.
    fn paint_undelivered(&mut self) -> Option<Kind1RevealWindow> {
        let running = self.running.as_ref()?;
        self.pending = running.dirty.is_some();
        let count = running.dirty.or(running.displayed_count)?;
        Some(Kind1RevealWindow {
            count,
            range: self.params.range,
        })
    }

    /// The latest recomposition was presented. Returns whether it showed a
    /// new step of the reveal. WM_PAINT draws, then advances the count while
    /// it is below `units + 1 + range` and kills the timer once it gets there
    /// (`0x00615B0D..0x00615B49`); a later paint only redraws.
    pub fn commit_presented(&mut self) -> bool {
        if !std::mem::take(&mut self.pending) {
            return false;
        }
        let Some(running) = self.running.as_mut() else {
            return false;
        };
        let Some(count) = running.dirty.take() else {
            return false;
        };
        running.displayed_count = Some(count);
        if running.internal_count < running.target_count {
            running.internal_count = running.internal_count.saturating_add(self.params.step);
            if running.internal_count >= running.target_count {
                running.next_timer_at = None;
            }
        }
        true
    }

    /// The reveal ran to completion and its final paint is on screen.
    pub fn is_terminal(&self) -> bool {
        !self.pending
            && self.running.as_ref().is_some_and(|running| {
                running.internal_count >= running.target_count
                    && running.displayed_count.is_some()
                    && running.dirty.is_none()
                    && running.next_timer_at.is_none()
            })
    }

    /// [`Self::paint`] with the static's text. While a slide runs no timer
    /// reaches the static (`0x006071E0` sleeps without dispatching):
    /// `timers` is false and only an invalidation paints a new count.
    pub fn paint_text(&mut self, now: Instant, timers: bool) -> Option<StaticPaint<'_>> {
        let window = if timers {
            self.paint(now)?
        } else {
            self.paint_undelivered()?
        };
        Some(StaticPaint {
            text: &self.text,
            window,
        })
    }
}

/// One shown static in a recomposition: its text and reveal window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaticPaint<'a> {
    pub text: &'a str,
    pub window: Kind1RevealWindow,
}

/// A [`DialogStatics`] recomposition; `None` where nothing paints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogStaticsPaint<'a> {
    pub heading: Option<StaticPaint<'a>>,
    pub status_line: Option<StaticPaint<'a>>,
}

/// Heading `0x694` and status line `0x695` of a family dialog that another
/// dialog hides or covers without destroying (`0x6B` under `0x105`, `0x105`
/// under the seed browser), so they outlive the other dialog's run: text,
/// started flag and count stay. A hidden dialog shown again keeps its last
/// status-line paint through its entry slide (retail `rmg-cancel.png`), and
/// its SHOW completion repaints what already started
/// ([`PresentedKind1Static::show`]).
#[derive(Debug, Clone)]
pub struct DialogStatics {
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
    pub fn show(&mut self, heading: &str, now: Instant) {
        self.heading.set_text(heading, now);
        self.heading.show(now);
        self.status_line.show(now);
    }

    /// A hover message to the status line (`0x4B2`): a changed help restarts
    /// a started reveal, and every message repaints the line
    /// (`0x00615EF7`). Returns whether the help changed.
    pub fn hover(&mut self, help: &str, now: Instant) -> bool {
        let changed = self.status_line.text() != help;
        self.status_line.set_text(help, now);
        self.status_line.repaint();
        changed
    }

    /// Another dialog stops covering this one without a slide (the seed
    /// browser over `0x105` closes): both statics repaint at their counts.
    pub fn uncovered(&mut self) {
        self.heading.repaint();
        self.status_line.repaint();
    }

    /// Both statics for this recomposition; the frame loop commits them
    /// after present. While a slide runs the engine blits the top panel over
    /// the heading, and the status line, outside the blits and validate-only
    /// (`0x00606800` through `0x00601360`), gets no timer and keeps its last
    /// paint ([`PresentedKind1Static::paint_text`]).
    pub fn paint(&mut self, now: Instant, sliding: bool) -> DialogStaticsPaint<'_> {
        let heading = if sliding {
            None
        } else {
            self.heading.paint_text(now, true)
        };
        DialogStaticsPaint {
            heading,
            status_line: self.status_line.paint_text(now, !sliding),
        }
    }

    /// Returns whether the presented frame showed a new status-line step.
    pub fn commit_presented(&mut self) -> bool {
        self.heading.commit_presented();
        self.status_line.commit_presented()
    }

    /// Both reveals ran to completion and their last paints are on screen.
    pub fn is_terminal(&self) -> bool {
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

#[cfg(all(test, tests_ui))]
pub mod tests {
    use super::*;

    fn native_params(dialog: u64, control: u64) -> Kind1Params {
        let cases = crate::ui::shell::native_fixtures::native_static_timer_cases();
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
        let now = Instant::now();
        let mut reveal = PresentedKind1Static::new(HEADING_KIND1);
        // A + surrogate pair + B = four UTF-16 units: target 4 + 1 + 8 = 13,
        // so the last timer paint draws 12.
        reveal.set_text("A😀B", now);
        assert!(reveal.start(now));
        assert!(!reveal.start(now));
        let mut last = 0;
        for tick in 0..20 {
            last = present(&mut reveal, now + KIND1_TIMER_INTERVAL * tick)
                .unwrap()
                .count;
        }
        assert_eq!(last, 12);
        assert!(reveal.is_terminal());
    }

    #[test]
    fn an_unpresented_paint_is_drawn_again_without_advancing() {
        let now = Instant::now();
        let mut reveal = PresentedKind1Static::new(HEADING_KIND1);
        reveal.set_text("abc", now);
        assert!(reveal.start(now));
        assert_eq!(reveal.paint(now).map(|w| w.count), Some(1));
        assert_eq!(
            reveal
                .paint(now + Duration::from_millis(95))
                .map(|w| w.count),
            Some(1)
        );
        assert!(reveal.commit_presented());
        // The presented count is retained until the next timer.
        assert_eq!(
            present(&mut reveal, now + Duration::from_millis(100)).map(|w| w.count),
            Some(1)
        );
        assert_eq!(
            present(&mut reveal, now + Duration::from_millis(120)).map(|w| w.count),
            Some(2)
        );
    }

    #[test]
    fn main_menu_retains_count_17_after_internal_count_reaches_18() {
        let start = Instant::now();
        let mut reveal = PresentedKind1Static::new(HEADING_KIND1);
        reveal.set_text("Main Menu", start);
        assert!(reveal.start(start));
        for count in 1..=17 {
            let now = start + KIND1_TIMER_INTERVAL * (count - 1);
            assert_eq!(present(&mut reveal, now).map(|w| w.count), Some(count));
        }
        assert!(reveal.is_terminal());
        assert_eq!(
            reveal.paint(start + Duration::from_secs(10)),
            Some(Kind1RevealWindow {
                count: 17,
                range: 8,
            })
        );
    }

    /// Paint and present both statics once: (heading, status line) counts.
    fn present_dialog(statics: &mut DialogStatics, now: Instant) -> (Option<u32>, Option<u32>) {
        present_dialog_sliding(statics, now, false)
    }

    fn present_dialog_sliding(
        statics: &mut DialogStatics,
        now: Instant,
        sliding: bool,
    ) -> (Option<u32>, Option<u32>) {
        let shown = statics.paint(now, sliding);
        let counts = (
            shown.heading.map(|shown| shown.window.count),
            shown.status_line.map(|shown| shown.window.count),
        );
        statics.commit_presented();
        counts
    }

    /// The frame loop hears of a status-line step only when a present showed
    /// one, so the draw limit eases just for those.
    #[test]
    fn commit_reports_only_a_new_status_line_step() {
        let t0 = Instant::now();
        let mut statics = DialogStatics::default();
        statics.hover("Help", t0);
        statics.show("Choose Map", t0);
        statics.paint(t0, false);
        assert!(statics.commit_presented());
        statics.paint(t0, false);
        assert!(!statics.commit_presented(), "no timer came due");
        statics.paint(t0 + Duration::from_millis(15), false);
        assert!(statics.commit_presented());
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
        // Shown again after another dialog hid it: the entry slide keeps the
        // status line's last paint, and the SHOW completion repaints both at
        // their counts, which after the last timer paint is the target.
        assert_eq!(
            present_dialog_sliding(&mut statics, now, true),
            (None, Some(19))
        );
        statics.show("Choose Map", now);
        assert_eq!(present_dialog(&mut statics, now), (Some(19), Some(22)));
        assert!(statics.is_terminal());
        // Uncovered without a slide: the same repaint at once.
        statics.uncovered();
        assert_eq!(present_dialog(&mut statics, now), (Some(19), Some(22)));
    }

    #[test]
    fn no_timer_reaches_the_status_line_while_a_slide_runs() {
        let t0 = Instant::now();
        let mut statics = DialogStatics::default();
        statics.show("Choose Map", t0);
        statics.hover("Create a random battlefield.", t0);
        assert_eq!(present_dialog(&mut statics, t0), (Some(1), Some(1)));
        // A slide paints no heading, and the status line keeps its last paint
        // however many timer deadlines pass.
        let mut now = t0;
        for _ in 0..30 {
            now += Duration::from_millis(15);
            assert_eq!(
                present_dialog_sliding(&mut statics, now, true),
                (None, Some(1))
            );
        }
        // After the slide both reveals go on from their counts.
        now += Duration::from_millis(15);
        assert_eq!(present_dialog(&mut statics, now), (Some(2), Some(4)));
    }
}

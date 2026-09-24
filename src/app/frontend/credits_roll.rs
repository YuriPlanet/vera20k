//! Show_Credits `0x004C3E30`, reached from Movies & Credits View Credits.
//!
//! Lines parsed from CREDITSMD.TXT (`ui::movies_credits_shell::parse_credits`)
//! scroll up two pixels per frame on a black screen. Frames are paced on an
//! absolute schedule of two 16 ms wall-clock buckets (`DAT_00820770 = 2`); any
//! queued keyboard/mouse event ends the current wait early, is consumed, and is
//! ignored unless it is a bare Escape press (`Keyboard::Get() == 0x1B`), which
//! ends the roll. The roll also ends when every line has scrolled off. Each
//! line is painted in GAME.FNT, RGB(255,255,128), aligned in a 520 px box
//! centered on the screen (`0x004C3D00`), and the top and bottom 32 rows are
//! faded in the 16-bit domain (presented through `SurfaceEffects`).

use std::collections::VecDeque;

use crate::render::batch::SpriteInstance;
use crate::render::bit_font::BitFont;
use crate::render::shell_paint::TEXT_DEPTH;
use crate::ui::movies_credits_shell::{CreditLine, CreditsLayoutFlags};

/// Width of the per-line text box (`0x208`), centered on the screen.
pub(crate) const CREDITS_BOX_WIDTH: i32 = 0x208;
/// Rows faded at each screen edge.
pub(crate) const CREDITS_FADE_ROWS: u32 = 32;
/// RGB(255,255,128) (`ECX = 0x0080FFFF` for `0x006211D0`).
pub(crate) const CREDITS_TEXT_RGB: [f32; 3] = [1.0, 1.0, 128.0 / 255.0];
/// 16 ms buckets per frame.
const FRAME_BUCKETS: u64 = 2;
/// Per-frame upward scroll in pixels.
const SCROLL_STEP: i32 = 2;
/// A line is deleted once its y is `<= -0x15` (`while (-0x15 < y)`).
const DELETE_BELOW: i32 = -0x15;

/// One queued `Keyboard` entry as the credits loop sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CreditsInput {
    /// A bare Escape key press: code `0x1B` with no Shift/Ctrl/Alt bits.
    Escape,
    /// Any other key press/release or mouse button event.
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Scrolling,
    /// Post-loop hold (`0x004C47AA..`): frames continue with nothing drawn.
    /// `waiting` is set after an iteration until its `2 * frame` wait ends.
    Hold {
        waiting: bool,
    },
    Done,
}

#[derive(Debug, Clone)]
struct RollLine {
    text: String,
    y: i32,
    flags: CreditsLayoutFlags,
    width: i32,
}

/// Native-shaped credits state machine. The app owns wall time and input; this
/// type owns the frame counters, the line list and the exit decision.
#[derive(Debug, Clone)]
pub(crate) struct CreditsRoll {
    lines: Vec<RollLine>,
    phase: Phase,
    /// Frames produced (`iStack_b8` / `iVar19`).
    frame: u64,
    start_bucket: u64,
    /// `c` in the hold arithmetic; wraps to 0 after 200 while `k < 16`.
    counter_c: i32,
    counter_k: i32,
    pending: VecDeque<CreditsInput>,
    /// Diagnostic shell capture only: the roll is pinned at one frame.
    capture_hold: bool,
}

impl CreditsRoll {
    /// Start the roll at `now_bucket` (a 16 ms wall-clock bucket). The first
    /// frame is produced immediately; an empty file goes straight to the hold.
    pub(crate) fn new(lines: Vec<CreditLine>, font: &BitFont, now_bucket: u64) -> Self {
        let lines = lines
            .into_iter()
            .map(|line| RollLine {
                width: font.text_width(&line.text) as i32,
                text: line.text,
                y: line.y,
                flags: line.flags,
            })
            .collect::<Vec<_>>();
        let mut roll = Self {
            phase: if lines.is_empty() {
                Phase::Hold { waiting: false }
            } else {
                Phase::Scrolling
            },
            lines,
            frame: 0,
            start_bucket: now_bucket,
            counter_c: 0,
            counter_k: 0,
            pending: VecDeque::new(),
            capture_hold: false,
        };
        if roll.phase == Phase::Scrolling {
            roll.produce_frame();
        }
        roll
    }

    /// Queue one keyboard/mouse event (`Keyboard::Put`).
    pub(crate) fn push_input(&mut self, input: CreditsInput) {
        if self.phase != Phase::Done {
            self.pending.push_back(input);
        }
    }

    pub(crate) fn finished(&self) -> bool {
        self.phase == Phase::Done
    }

    /// Frames produced so far.
    pub(crate) fn frame(&self) -> u64 {
        self.frame
    }

    /// Diagnostic shell capture: produce scrolling frames up to `target`
    /// without waiting for wall time, then pin the roll there. The produced
    /// state is identical to reaching `target` on schedule with no input.
    pub(crate) fn hold_at_frame_for_capture(&mut self, target: u64) {
        while self.phase == Phase::Scrolling && self.frame < target && !self.lines.is_empty() {
            self.produce_frame();
        }
        self.capture_hold = true;
    }

    fn frame_due(&self, now_bucket: u64) -> bool {
        now_bucket.saturating_sub(self.start_bucket) >= FRAME_BUCKETS * self.frame
    }

    /// One scrolling frame: counters, then scroll from the last line to the
    /// first, deleting at most the first line that crosses the top.
    fn produce_frame(&mut self) {
        self.frame += 1;
        self.counter_c += 1;
        if self.counter_c > 200 && self.counter_k < 16 {
            self.counter_k += 1;
            self.counter_c = 0;
        }
        for index in (0..self.lines.len()).rev() {
            self.lines[index].y -= SCROLL_STEP;
            if self.lines[index].y <= DELETE_BELOW {
                self.lines.remove(index);
                break;
            }
        }
    }

    /// Advance to wall-clock bucket `now_bucket`. Returns whether the visible
    /// frame changed.
    pub(crate) fn advance(&mut self, now_bucket: u64) -> bool {
        if self.capture_hold {
            return false;
        }
        let mut changed = false;
        loop {
            match self.phase {
                Phase::Done => return changed,
                Phase::Scrolling => {
                    // The wait after each frame ends on schedule or on a
                    // pending entry, which is then consumed.
                    let input = self.pending.front().copied();
                    if input.is_none() && !self.frame_due(now_bucket) {
                        return changed;
                    }
                    if let Some(input) = input {
                        self.pending.pop_front();
                        if input == CreditsInput::Escape {
                            self.enter_hold();
                            continue;
                        }
                    }
                    if self.lines.is_empty() {
                        self.enter_hold();
                        continue;
                    }
                    self.produce_frame();
                    changed = true;
                }
                Phase::Hold { waiting } => {
                    // The hold never consumes input: any pending entry makes
                    // every remaining wait return immediately.
                    if !self.pending.is_empty() {
                        self.phase = Phase::Done;
                        continue;
                    }
                    if waiting && !self.frame_due(now_bucket) {
                        return changed;
                    }
                    if self.counter_c > 0xBB {
                        self.phase = Phase::Done;
                        continue;
                    }
                    self.hold_iteration();
                    self.phase = Phase::Hold { waiting: true };
                }
            }
        }
    }

    /// Leave the scroll loop; the hold's `while (c <= 0xBB)` is checked
    /// before each iteration.
    fn enter_hold(&mut self) {
        self.phase = Phase::Hold { waiting: false };
    }

    /// One hold iteration: skip `c` from 48..149 to 150, then advance the
    /// frame and `c`; the caller waits for the new frame's slot.
    fn hold_iteration(&mut self) {
        if self.counter_c > 47 && self.counter_c < 150 {
            self.counter_c = 150;
        }
        self.frame += 1;
        self.counter_c += 1;
    }

    /// Text sprites for the current frame (`0x004C3D00` placement).
    pub(crate) fn instances(&self, font: &BitFont, screen_w: i32) -> Vec<SpriteInstance> {
        let box_x = credits_box_x(screen_w);
        let mut out = Vec::new();
        for line in &self.lines {
            let x = line_x(box_x, line.width, line.flags);
            out.extend(font.build_text(
                &line.text,
                x as f32,
                line.y as f32,
                1.0,
                TEXT_DEPTH,
                CREDITS_TEXT_RGB,
                [0.0, 0.0],
            ));
        }
        out
    }
}

/// Left edge of the 520 px text box every line is drawn and clipped in
/// (`0x004C3D46..0x004C3D6B`: rect `((W - 0x208) / 2, y, 0x208, ...)`).
pub(crate) fn credits_box_x(screen_w: i32) -> i32 {
    (screen_w - CREDITS_BOX_WIDTH) / 2
}

/// A running Show_Credits: the roll plus the ScoreVolume it saved at
/// `0x004C42B3` and restores at `0x004C484A`.
pub(crate) struct CreditsRollSession {
    pub(crate) roll: CreditsRoll,
    pub(crate) saved_score_volume: f32,
    /// Text of the last frame drawn while the application was active. While
    /// it is inactive nothing is drawn or copied to the screen
    /// (`0x004C3D8A`, `0x004C46AA`), so that frame stays up.
    pub(crate) last_drawn: Vec<SpriteInstance>,
}

/// Line x inside the 520 px box: left at the box, centered at
/// `box + ((520 - w) + 1) / 2`, right at `box + 520 - w - 1`.
fn line_x(box_x: i32, width: i32, flags: CreditsLayoutFlags) -> i32 {
    if flags.centered() {
        box_x + (CREDITS_BOX_WIDTH - width + 1) / 2
    } else if flags.right() {
        box_x + CREDITS_BOX_WIDTH - width - 1
    } else {
        box_x
    }
}

/// Wall time in native 16 ms buckets (`timeGetTime() >> 4`).
pub(crate) fn wall_bucket(wall_ms: u64) -> u64 {
    wall_ms >> 4
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str, y: i32) -> CreditLine {
        CreditLine {
            text: text.into(),
            column: 7,
            y,
            flags: CreditsLayoutFlags(0x168),
        }
    }

    fn font() -> BitFont {
        crate::render::bit_font::tests::make_test_font(&[(u16::from(b'A'), 6)], 4)
    }

    fn ys(roll: &CreditsRoll) -> Vec<i32> {
        roll.lines.iter().map(|line| line.y).collect()
    }

    #[test]
    fn first_frame_scrolls_immediately_then_waits_two_buckets() {
        let mut roll = CreditsRoll::new(vec![line("A", 602)], &font(), 100);
        assert_eq!(ys(&roll), [600]);
        assert!(!roll.advance(101));
        assert!(roll.advance(102));
        assert_eq!(ys(&roll), [598]);
        // Absolute schedule: a late wake catches up frame by frame.
        assert!(roll.advance(108));
        assert_eq!(ys(&roll), [592]);
    }

    #[test]
    fn scroll_rate_matches_the_timed_retail_capture() {
        // Retail YR (cnc-ddraw, 800x600), 38 screenshot pairs timed by file
        // modification time over 178 s of scrolling: 62.499 px/s, i.e. one
        // 2 px step every 32.001 ms (docs/research/shell/
        // 2026-09-24-movies-and-credits-evidence.md). Two 16 ms buckets per
        // frame at 2 px per frame is 62.5 px/s.
        let mut roll = CreditsRoll::new(vec![line("A", 20_000)], &font(), 0);
        let start = ys(&roll)[0];
        let buckets_per_second = 1000 / 16;
        roll.advance(178 * buckets_per_second);
        let travelled = start - ys(&roll)[0];
        let seconds = f64::from(178 * buckets_per_second as i32) * 16.0 / 1000.0;
        let rate = f64::from(travelled) / seconds;
        assert!((rate - 62.499).abs() < 0.1, "{rate} px/s");
    }

    #[test]
    fn other_input_ends_one_wait_early_and_escape_ends_the_roll() {
        let mut roll = CreditsRoll::new(vec![line("A", 602)], &font(), 0);
        roll.push_input(CreditsInput::Other);
        assert!(roll.advance(0));
        assert_eq!(ys(&roll), [598]);
        roll.push_input(CreditsInput::Escape);
        roll.push_input(CreditsInput::Other);
        roll.advance(0);
        assert!(roll.finished(), "the pending release collapses the hold");
    }

    #[test]
    fn escape_without_further_input_runs_the_hold_on_schedule() {
        let mut roll = CreditsRoll::new(vec![line("A", 602)], &font(), 0);
        roll.push_input(CreditsInput::Escape);
        roll.advance(0);
        assert!(!roll.finished());
        // c == 1 after one frame: 86 - 1 = 85 hold iterations remain.
        roll.advance(2 * 86 - 1);
        assert!(!roll.finished());
        roll.advance(2 * 86);
        assert!(roll.finished());
    }

    #[test]
    fn at_most_one_line_is_deleted_per_frame_and_earlier_siblings_skip_the_step() {
        let mut roll = CreditsRoll::new(vec![line("A", -17), line("A", -17)], &font(), 0);
        // First frame: the later sibling reaches -19 (> -21), both step.
        assert_eq!(ys(&roll), [-19, -19]);
        roll.advance(2);
        // Second frame: the later sibling crosses and is deleted; the earlier
        // one is not decremented this frame.
        assert_eq!(ys(&roll), [-19]);
        roll.advance(4);
        assert_eq!(roll.lines.len(), 0);
    }

    #[test]
    fn alignment_matches_the_native_line_painter() {
        let box_x = (800 - CREDITS_BOX_WIDTH) / 2;
        assert_eq!(box_x, 140);
        assert_eq!(line_x(box_x, 100, CreditsLayoutFlags(0x168)), 140 + 210);
        assert_eq!(line_x(box_x, 101, CreditsLayoutFlags(0x168)), 140 + 210);
        assert_eq!(line_x(box_x, 100, CreditsLayoutFlags(0x268)), 140 + 419);
        assert_eq!(line_x(box_x, 100, CreditsLayoutFlags(0x68)), 140);
        // 640 wide: the box starts left of the screen.
        assert_eq!((640 - CREDITS_BOX_WIDTH) / 2, 60);
    }
}

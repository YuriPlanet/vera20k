//! Data-driven first-paint slide eligibility + frame schedule (contract C11).
//!
//! Every allow-listed front-end shell dialog (main menu `0xE2`, single player
//! `0x100`, skirmish setup `0x102`, …) plays a one-shot controls-reveal slide on
//! its OWN first paint — not a screen-edge crossfade. Each owner-draw control's
//! chrome-SHP (SDBTNANM) frame index advances on a staggered 30 ms-per-frame
//! schedule; controls are never repositioned. This module owns the two
//! render-agnostic halves of that behaviour:
//!   * **eligibility data** — the dialog-id allow-list (`is_slide_eligible`) and
//!     per-dialog animated owner-draw-button count (`slot_count_for`), and
//!   * **the frame schedule** — [`ShellFrameWave`], the SDBTNANM frame sweep with
//!     the verified tick cadence and loop bound.
//!
//! Render-agnostic: depends only on [`DialogId`] + `std` (no sim/render/assets),
//! honouring the `ui/` layering rule. The app layer (`app::frontend::shell_transition`) maps
//! the showing screen to a `DialogId`, drives the wave each frame, and plays the
//! start cue (`GUIMoveInSound`, stock `MenuSlideIn`); the stock-empty end cue
//! (`ShellButtonSlideSound`) stays silent.

use std::time::{Duration, Instant};

use super::descriptor::DialogId;

/// One animation tick per 30 ms, advancing exactly one frame (never skipped).
pub(crate) const WAVE_TICK_MS: u32 = 30;
/// Active-retail dialog `0xE2` presents exactly ticks `0..13`.
pub(crate) const MAIN_MENU_ENTRY_FRAME_COUNT: u8 = 14;
pub(crate) const MAIN_MENU_TERMINAL_TICK: u8 = MAIN_MENU_ENTRY_FRAME_COUNT - 1;
/// Extra ticks after the last schedule entry so the ramp completes. The loop
/// bound is `max(schedule entry) + WAVE_TAIL_TICKS`.
pub(crate) const WAVE_TAIL_TICKS: u32 = 6;
/// Linear ramp length (delta 0..=5 inclusive => 6 steps).
pub(crate) const WAVE_RAMP_STEPS: i32 = 6;

/// SDBTNANM frame constants per button group. Each tuple is
/// `(held_before, ramp_base, held_after)`. With `dir() = -1` on slide-IN, the IN
/// ramp counts DOWN from `base`; the held terminals are distinct constants, not
/// `base`. Slide-OUT uses `dir() = +1` (ramp counts UP).
/// Group A = regular owner-draw button cell (SDBTNANM 10→5, settle 1).
/// Group B = the "second cell group" (SDBTNANM 16→11, settle 0) — not yet wired
/// by any consumer.
pub(crate) struct WaveFrames {
    pub before: i32,
    pub base: i32,
    pub after: i32,
}
/// SHOW: hold 10 → ramp 10,9,8,7,6,5 → settle 1.
pub(crate) const GROUP_A_IN: WaveFrames = WaveFrames {
    before: 10,
    base: 10,
    after: 1,
};
/// CLOSE: hold 1 → ramp 5,6,7,8,9,10 → settle 10.
pub(crate) const GROUP_A_OUT: WaveFrames = WaveFrames {
    before: 1,
    base: 5,
    after: 10,
};
/// SHOW: hold 10 → ramp 16,15,14,13,12,11 → settle 0.
pub(crate) const GROUP_B_IN: WaveFrames = WaveFrames {
    before: 10,
    base: 16,
    after: 0,
};
/// CLOSE: hold 0 → ramp 11,12,13,14,15,16 → settle 10.
pub(crate) const GROUP_B_OUT: WaveFrames = WaveFrames {
    before: 0,
    base: 11,
    after: 10,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WaveDirection {
    SlideIn,
    /// Slide-OUT (close) ramp run by the teardown slide (`0x00608070` ->
    /// `0x006071E0` with `DL = 0`): the same schedule with the OUT frames.
    SlideOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ButtonGroup {
    A,
    /// The "second cell group" (SDBTNANM 16→11). Modeled for completeness; not
    /// yet wired by any shell renderer (all current shells use group A).
    #[allow(dead_code)]
    B,
}

impl WaveDirection {
    /// Frame multiplier: -1 on slide-in (frames count DOWN, e.g. 10→5), +1 on
    /// slide-out (frames count UP). Matches the original ramp direction (-1 on
    /// show / +1 on close).
    fn dir(self) -> i32 {
        match self {
            WaveDirection::SlideIn => -1,
            WaveDirection::SlideOut => 1,
        }
    }
}

// --- Data-driven slide eligibility + per-dialog slot count (allow-list) -------

/// A slide-eligible front-end shell dialog that has a renderer here, plus its
/// animated owner-draw button count `N`. `N` = the dialog's visible/enabled
/// owner-draw button children (the SDBTNANM cells) — statics/headings do not
/// animate. `N` sets the per-cell stagger length and therefore the loop bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ShellSlideSpec {
    pub dialog_id: u16,
    pub slot_count: u32,
}

/// Front-end shell dialogs we render today, with their animated slot counts.
/// `0xE2` main menu: five regular schedule entries; Exit shares Options' fifth
/// entry. `0x100` single player and `0x101` Movies & Credits: 4 owner-draw
/// buttons each. `0x102` skirmish setup: Start Game / Choose Map / Back (3
/// right-panel buttons). `0x129` movie list: Play Movie / Back (both dialogs
/// are in the full-screen allow-list at `0x0060C63A` / `0x0060C645`).
pub(crate) const RENDERED_SHELL_SLIDES: &[ShellSlideSpec] = &[
    ShellSlideSpec {
        dialog_id: 0x00E2,
        slot_count: 5,
    },
    ShellSlideSpec {
        dialog_id: 0x0100,
        slot_count: 4,
    },
    ShellSlideSpec {
        dialog_id: 0x0101,
        slot_count: 4,
    },
    ShellSlideSpec {
        dialog_id: 0x0129,
        slot_count: 2,
    },
    ShellSlideSpec {
        dialog_id: 0x0102,
        slot_count: 3,
    },
];

/// Front-end shell dialog ids that slide on first paint (the eligibility
/// allow-list, scoped to the front-end shells). The rendered shells
/// (`RENDERED_SHELL_SLIDES`) plus the front-end dialogs documented as
/// allow-listed but not yet rendered here (`0x94`/`0x6B` per
/// `docs/research/skirmish-ui/SHELL_FIRST_PAINT_SLIDE_GENERIC_TRIGGER_GHIDRA_REPORT.md`
/// §3); those slide automatically once a renderer maps to them and gains a
/// `RENDERED_SHELL_SLIDES` slot count. The original's full list
/// (`0x0060C540`, 55 ids) also marks Options `0xD5` and its children, Load
/// `0xB7`, Score `0x108`, the in-game menu dialogs (`0xB5`, `0xB6`, `0xB8`,
/// `0xBBA`, `0xBBB`) and the LAN/WOL setup dialogs; none of them slides here
/// yet. Message boxes (`0x120` confirm, `0xCE` body-ok) are not in it.
pub(crate) const SHELL_SLIDE_ALLOW_LIST: &[u16] =
    &[0x00E2, 0x0094, 0x006B, 0x0100, 0x0101, 0x0102, 0x0129];

/// Whether a dialog plays the first-paint controls-reveal slide.
pub(crate) fn is_slide_eligible(id: DialogId) -> bool {
    SHELL_SLIDE_ALLOW_LIST.contains(&id.0)
}

/// Animated owner-draw button count for a rendered shell dialog (`N`, the stagger
/// length). `None` for an allow-listed dialog that has no renderer here yet — the
/// app layer only ever drives the slide for dialogs it actually paints.
pub(crate) fn slot_count_for(id: DialogId) -> Option<u32> {
    RENDERED_SHELL_SLIDES
        .iter()
        .find(|s| s.dialog_id == id.0)
        .map(|s| s.slot_count)
}

// --- Frame schedule (the wave) -----------------------------------------------

const MAIN_MENU_GROUP_A_ENTRY_TICKS: &[(u16, i32)] = &[
    (0x0683, 1),
    (0x0684, 2),
    (0x0578, 3),
    (0x0686, 4),
    (0x055C, 5),
    (0x03EE, 5),
];

/// Schedule entry tick of a `0xE2` button by resource id.
fn main_menu_entry_tick(resource_id: u16) -> Option<i32> {
    MAIN_MENU_GROUP_A_ENTRY_TICKS
        .iter()
        .find_map(|&(id, tick)| (id == resource_id).then_some(tick))
}

#[derive(Debug, Clone)]
enum WaveClock {
    Compatibility {
        last_step_at: Instant,
        tick: u32,
    },
    PresentedMainMenu {
        generation: u64,
        tick: u8,
        phase: PresentedPhase,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PresentedPhase {
    Armed,
    Ready,
    WaitingUntil(Instant),
    Completing,
    Poisoned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PresentedPoll {
    Acquire,
    WaitUntil(Instant),
    Complete,
    Poisoned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MainMenuEntryPaintFrame {
    generation: u64,
    tick: u8,
}

impl MainMenuEntryPaintFrame {
    pub(crate) fn generation(self) -> u64 {
        self.generation
    }

    pub(crate) fn tick(self) -> u8 {
        self.tick
    }

    pub(crate) fn sdbtnanm_frame(self, resource_id: u16, group: ButtonGroup) -> Option<usize> {
        Some(frame_for_tick(
            i32::from(self.tick),
            main_menu_entry_tick(resource_id)?,
            group,
            WaveDirection::SlideIn,
        ))
    }
}

/// Single-use proof that one exact generation/tick was encoded through the
/// final main-menu presenter. Deliberately neither `Clone` nor `Copy`.
#[derive(Debug)]
pub(crate) struct MainMenuEntryPresentToken {
    generation: u64,
    tick: u8,
}

impl MainMenuEntryPresentToken {
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn tick(&self) -> u8 {
        self.tick
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PresentedCommitError {
    CompatibilityMode,
    NotReady,
    GenerationMismatch,
    TickMismatch,
}

impl std::fmt::Display for PresentedCommitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::CompatibilityMode => "present token used with compatibility wave",
            Self::NotReady => "present token committed outside the ready phase",
            Self::GenerationMismatch => "present token generation mismatch",
            Self::TickMismatch => "present token tick mismatch",
        };
        f.write_str(message)
    }
}

impl std::error::Error for PresentedCommitError {}

/// Per-dialog first-paint slide state. `None` (Idle) → `Some(running)` →
/// `Some(complete)` mirrors the original's `1→2→3` slide state machine: the wave
/// is created on the dialog's entry edge, advances one frame per 30 ms tick, and
/// signals completion via [`ShellFrameWave::is_complete`].
#[derive(Debug, Clone)]
pub(crate) struct ShellFrameWave {
    clock: WaveClock,
    /// number of animated control slots (N).
    #[allow(dead_code)]
    slot_count: u32,
    /// inclusive loop bound = max(schedule entry) + WAVE_TAIL_TICKS.
    total_ticks: u32,
    direction: WaveDirection,
}

impl ShellFrameWave {
    pub(crate) fn new_first_paint_slide(slot_count: u32, now: Instant) -> Self {
        Self {
            clock: WaveClock::Compatibility {
                last_step_at: now,
                tick: 0,
            },
            slot_count,
            total_ticks: Self::total_ticks_for(slot_count),
            direction: WaveDirection::SlideIn,
        }
    }

    /// The teardown slide of a shown dialog (`0x00608070`): the same schedule
    /// and loop bound as the entry slide, frames counting up to the empty slot.
    pub(crate) fn new_slide_out(slot_count: u32, now: Instant) -> Self {
        Self {
            direction: WaveDirection::SlideOut,
            ..Self::new_first_paint_slide(slot_count, now)
        }
    }

    pub(crate) fn new_presented_main_menu(generation: u64) -> Self {
        assert_ne!(generation, 0, "main-menu wave generation must be nonzero");
        Self {
            clock: WaveClock::PresentedMainMenu {
                generation,
                tick: 0,
                phase: PresentedPhase::Armed,
            },
            slot_count: 5,
            total_ticks: u32::from(MAIN_MENU_ENTRY_FRAME_COUNT),
            direction: WaveDirection::SlideIn,
        }
    }

    /// Loop bound (total frames) for `N` animated button slots. Replicates the
    /// schedule-array build: the N button slots take entry ticks `1..=N` (index
    /// `s` ← `s+1`), and a fixed radar-open anchor slot is written at entry tick
    /// `N+3`, which the max-scan always picks as the largest schedule entry. The
    /// loop then runs `max + WAVE_TAIL_TICKS` ticks, so total = `N + 3 + 6 =
    /// N + 9`. (The radar group itself only DRAWS when its `+0xD5`-family gate is
    /// set, but its schedule slot is written unconditionally, so the bound is a
    /// pure function of N for every shell.)
    fn total_ticks_for(slot_count: u32) -> u32 {
        let max_entry = slot_count + 3;
        max_entry + WAVE_TAIL_TICKS
    }

    /// Entry tick for a control slot (the stagger): slot 0 enters at tick 1.
    fn entry_tick(slot: u32) -> i32 {
        slot as i32 + 1
    }

    pub(crate) fn is_complete(&self) -> bool {
        match self.clock {
            WaveClock::Compatibility { tick, .. } => tick >= self.total_ticks,
            WaveClock::PresentedMainMenu { .. } => false,
        }
    }

    /// Current tick of a compatibility-clock wave.
    pub(crate) fn compatibility_tick(&self) -> Option<u32> {
        match self.clock {
            WaveClock::Compatibility { tick, .. } => Some(tick),
            WaveClock::PresentedMainMenu { .. } => None,
        }
    }

    /// Shell capture only: keep a compatibility-clock wave at its current
    /// tick (the next step never becomes due).
    pub(crate) fn hold_for_capture(&mut self) {
        if let WaveClock::Compatibility { last_step_at, .. } = &mut self.clock {
            *last_step_at = Instant::now() + Duration::from_secs(3600);
        }
    }

    /// Advance at most ONE tick per call, only once >= 30 ms has elapsed.
    /// Never collapses multiple indices (faithful to one-frame-per-Sleep).
    pub(crate) fn advance(&mut self, now: Instant) {
        let step = Duration::from_millis(u64::from(WAVE_TICK_MS));
        let WaveClock::Compatibility { last_step_at, tick } = &mut self.clock else {
            return;
        };
        if *tick < self.total_ticks && now.duration_since(*last_step_at) >= step {
            *tick += 1;
            *last_step_at += step;
        }
    }

    /// Frame index for an SDBTNANM button at the current tick.
    /// 4-case: held-before / linear ramp (base + delta*dir) / held-after.
    /// Terminal frames are DISTINCT constants, not `base` (verified from binary).
    pub(crate) fn sdbtnanm_frame(&self, slot: u32, group: ButtonGroup) -> usize {
        let WaveClock::Compatibility { tick, .. } = self.clock else {
            panic!("slot-index frame lookup is invalid for a presented main-menu wave");
        };
        frame_for_tick(tick as i32, Self::entry_tick(slot), group, self.direction)
    }

    /// Frame of a main-menu `0xE2` button (by resource id) on a
    /// compatibility-clock wave, with the 0xE2 schedule (Exit shares Options'
    /// entry tick).
    pub(crate) fn main_menu_sdbtnanm_frame(
        &self,
        resource_id: u16,
        group: ButtonGroup,
    ) -> Option<usize> {
        let WaveClock::Compatibility { tick, .. } = self.clock else {
            return None;
        };
        Some(frame_for_tick(
            tick as i32,
            main_menu_entry_tick(resource_id)?,
            group,
            self.direction,
        ))
    }

    pub(crate) fn activate_after_acquire(&mut self) -> bool {
        let WaveClock::PresentedMainMenu { phase, .. } = &mut self.clock else {
            return false;
        };
        if *phase != PresentedPhase::Armed {
            return false;
        }
        *phase = PresentedPhase::Ready;
        true
    }

    pub(crate) fn poll_presented(&mut self, now: Instant) -> Option<PresentedPoll> {
        let WaveClock::PresentedMainMenu { tick, phase, .. } = &mut self.clock else {
            return None;
        };
        Some(match *phase {
            PresentedPhase::Armed | PresentedPhase::Ready => PresentedPoll::Acquire,
            PresentedPhase::WaitingUntil(deadline) if now < deadline => {
                PresentedPoll::WaitUntil(deadline)
            }
            PresentedPhase::WaitingUntil(_) if *tick < MAIN_MENU_TERMINAL_TICK => {
                *tick += 1;
                *phase = PresentedPhase::Ready;
                PresentedPoll::Acquire
            }
            PresentedPhase::WaitingUntil(_) => {
                *phase = PresentedPhase::Completing;
                PresentedPoll::Complete
            }
            PresentedPhase::Completing => PresentedPoll::Complete,
            PresentedPhase::Poisoned => PresentedPoll::Poisoned,
        })
    }

    pub(crate) fn current_main_menu_frame(&self) -> Option<MainMenuEntryPaintFrame> {
        let WaveClock::PresentedMainMenu {
            generation,
            tick,
            phase: PresentedPhase::Ready,
        } = self.clock
        else {
            return None;
        };
        Some(MainMenuEntryPaintFrame { generation, tick })
    }

    pub(crate) fn mint_present_token(
        &self,
        frame: MainMenuEntryPaintFrame,
    ) -> Option<MainMenuEntryPresentToken> {
        (self.current_main_menu_frame() == Some(frame)).then_some(MainMenuEntryPresentToken {
            generation: frame.generation,
            tick: frame.tick,
        })
    }

    pub(crate) fn record_presented(
        &mut self,
        token: MainMenuEntryPresentToken,
        now: Instant,
    ) -> Result<(), PresentedCommitError> {
        let WaveClock::PresentedMainMenu {
            generation,
            tick,
            phase,
        } = &mut self.clock
        else {
            return Err(PresentedCommitError::CompatibilityMode);
        };
        if *phase != PresentedPhase::Ready {
            return Err(PresentedCommitError::NotReady);
        }
        if token.generation != *generation {
            return Err(PresentedCommitError::GenerationMismatch);
        }
        if token.tick != *tick {
            return Err(PresentedCommitError::TickMismatch);
        }
        *phase = PresentedPhase::WaitingUntil(now + Duration::from_millis(u64::from(WAVE_TICK_MS)));
        Ok(())
    }

    pub(crate) fn poison_presented(&mut self) {
        if let WaveClock::PresentedMainMenu { phase, .. } = &mut self.clock {
            *phase = PresentedPhase::Poisoned;
        }
    }

    pub(crate) fn presented_wake_deadline(&self) -> Option<Instant> {
        let WaveClock::PresentedMainMenu {
            phase: PresentedPhase::WaitingUntil(deadline),
            ..
        } = self.clock
        else {
            return None;
        };
        Some(deadline)
    }

    pub(crate) fn presented_generation(&self) -> Option<u64> {
        let WaveClock::PresentedMainMenu { generation, .. } = self.clock else {
            return None;
        };
        Some(generation)
    }

    pub(crate) fn is_presented_completing(&self, generation: u64) -> bool {
        matches!(
            self.clock,
            WaveClock::PresentedMainMenu {
                generation: actual,
                phase: PresentedPhase::Completing,
                ..
            } if actual == generation
        )
    }

    pub(crate) fn is_presented_poisoned(&self) -> bool {
        matches!(
            self.clock,
            WaveClock::PresentedMainMenu {
                phase: PresentedPhase::Poisoned,
                ..
            }
        )
    }

    #[cfg(test)]
    fn compatibility_tick_for_test(&self) -> u32 {
        let WaveClock::Compatibility { tick, .. } = self.clock else {
            panic!("expected compatibility wave");
        };
        tick
    }

    #[cfg(test)]
    fn add_compatibility_ticks_for_test(&mut self, amount: u32) {
        let WaveClock::Compatibility { tick, .. } = &mut self.clock else {
            panic!("expected compatibility wave");
        };
        *tick += amount;
    }
}

fn frame_for_tick(
    tick: i32,
    entry_tick: i32,
    group: ButtonGroup,
    direction: WaveDirection,
) -> usize {
    let f = match (group, direction) {
        (ButtonGroup::A, WaveDirection::SlideIn) => GROUP_A_IN,
        (ButtonGroup::A, WaveDirection::SlideOut) => GROUP_A_OUT,
        (ButtonGroup::B, WaveDirection::SlideIn) => GROUP_B_IN,
        (ButtonGroup::B, WaveDirection::SlideOut) => GROUP_B_OUT,
    };
    let delta = tick - entry_tick;
    let frame = if delta < 0 {
        f.before
    } else if delta < WAVE_RAMP_STEPS {
        f.base + delta * direction.dir()
    } else {
        f.after
    };
    frame.max(0) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn total_ticks_is_max_schedule_plus_tail() {
        // N=5 buttons => max entry N+3=8, total = 8 + 6 = 14 (= N+9). The
        // radar-open anchor slot (entry tick N+3) is the max, not the last
        // button (entry tick N).
        let w = ShellFrameWave::new_first_paint_slide(5, Instant::now());
        assert_eq!(w.total_ticks, 5 + 3 + WAVE_TAIL_TICKS);
        assert_eq!(w.total_ticks, 5 + 9);
    }

    #[test]
    fn slide_out_ramps_every_button_up_to_the_empty_slot() {
        // 0x006071E0 with DL = 0: before the ramp frame 1, then 5..10, then 10;
        // same stagger (slot s enters at tick s + 1) and loop bound as SHOW.
        let t0 = Instant::now();
        let mut out = ShellFrameWave::new_slide_out(4, t0);
        assert_eq!(out.total_ticks, 13);
        let mut frames = Vec::new();
        for tick in 0..=13u64 {
            out.advance(t0 + Duration::from_millis(30 * tick));
            frames.push((
                out.sdbtnanm_frame(0, ButtonGroup::A),
                out.sdbtnanm_frame(3, ButtonGroup::A),
            ));
        }
        assert_eq!(frames[0], (1, 1));
        assert_eq!(frames[1], (5, 1));
        assert_eq!(frames[4], (8, 5));
        assert_eq!(frames[6], (10, 7));
        assert_eq!(frames[10], (10, 10));
        assert!(out.is_complete());
    }

    #[test]
    fn main_menu_slide_out_uses_the_0xe2_schedule() {
        let t0 = Instant::now();
        let mut out = ShellFrameWave::new_slide_out(5, t0);
        assert_eq!(out.total_ticks, u32::from(MAIN_MENU_ENTRY_FRAME_COUNT));
        for tick in 1..=5u64 {
            out.advance(t0 + Duration::from_millis(30 * tick));
        }
        // Tick 5: Single Player (entry 1) ramped 4 steps, Options and Exit
        // (entry 5) just started.
        assert_eq!(
            out.main_menu_sdbtnanm_frame(0x0683, ButtonGroup::A),
            Some(9)
        );
        assert_eq!(
            out.main_menu_sdbtnanm_frame(0x055C, ButtonGroup::A),
            Some(5)
        );
        assert_eq!(
            out.main_menu_sdbtnanm_frame(0x03EE, ButtonGroup::A),
            Some(5)
        );
        assert_eq!(out.main_menu_sdbtnanm_frame(0x1234, ButtonGroup::A), None);
    }

    #[test]
    fn total_ticks_matches_native_table() {
        // Binary loop bound per N (= total frames): N=3→12, N=4→13, N=6→15.
        for (n, total) in [(3, 12), (4, 13), (6, 15)] {
            let w = ShellFrameWave::new_first_paint_slide(n, Instant::now());
            assert_eq!(w.total_ticks, total, "N={n}");
        }
    }

    #[test]
    fn advance_steps_one_frame_per_30ms_and_never_collapses() {
        let t0 = Instant::now();
        let mut w = ShellFrameWave::new_first_paint_slide(4, t0);
        w.advance(t0 + Duration::from_millis(29));
        assert_eq!(w.compatibility_tick_for_test(), 0);
        w.advance(t0 + Duration::from_millis(30));
        assert_eq!(w.compatibility_tick_for_test(), 1);
        // A 1-second gap must still advance only ONE index (no catch-up).
        w.advance(t0 + Duration::from_millis(1030));
        assert_eq!(w.compatibility_tick_for_test(), 2);
    }

    #[test]
    fn group_a_slide_in_holds_10_ramps_10_to_5_then_holds_1() {
        let t0 = Instant::now();
        let mut w = ShellFrameWave::new_first_paint_slide(3, t0);
        // slot 1 enters at tick 2; before that it holds at the "before" terminal = 10.
        assert_eq!(w.sdbtnanm_frame(1, ButtonGroup::A), 10);
        for _ in 0..2 {
            w.add_compatibility_ticks_for_test(1);
        } // tick = 2 => delta 0 => base 10
        assert_eq!(w.sdbtnanm_frame(1, ButtonGroup::A), 10);
        w.add_compatibility_ticks_for_test(5); // delta 5 => last ramp step
        assert_eq!(w.sdbtnanm_frame(1, ButtonGroup::A), 5);
        w.add_compatibility_ticks_for_test(3); // held "after" terminal
        assert_eq!(w.sdbtnanm_frame(1, ButtonGroup::A), 1);
    }

    #[test]
    fn group_b_slide_in_holds_10_ramps_16_to_11_then_holds_0() {
        let t0 = Instant::now();
        let mut w = ShellFrameWave::new_first_paint_slide(3, t0);
        assert_eq!(w.sdbtnanm_frame(0, ButtonGroup::B), 10); // before-entry (slot 0 enters tick 1)
        w.add_compatibility_ticks_for_test(1); // delta 0 => base 16
        assert_eq!(w.sdbtnanm_frame(0, ButtonGroup::B), 16);
        w.add_compatibility_ticks_for_test(5); // delta 5 => 11
        assert_eq!(w.sdbtnanm_frame(0, ButtonGroup::B), 11);
        w.add_compatibility_ticks_for_test(3); // held "after" = 0
        assert_eq!(w.sdbtnanm_frame(0, ButtonGroup::B), 0);
    }

    #[test]
    fn rendered_shells_are_all_eligible() {
        for spec in RENDERED_SHELL_SLIDES {
            assert!(
                is_slide_eligible(DialogId(spec.dialog_id)),
                "rendered shell {:#06x} must be on the allow-list",
                spec.dialog_id
            );
            assert_eq!(
                slot_count_for(DialogId(spec.dialog_id)),
                Some(spec.slot_count)
            );
        }
    }

    #[test]
    fn main_menu_uses_five_regular_schedule_entries() {
        assert_eq!(slot_count_for(DialogId(0x00E2)), Some(5));
    }

    #[test]
    fn main_menu_resource_schedule_is_exact_for_ticks_zero_through_thirteen() {
        let expected = [
            (0x0683, [10, 10, 9, 8, 7, 6, 5, 1, 1, 1, 1, 1, 1, 1]),
            (0x0684, [10, 10, 10, 9, 8, 7, 6, 5, 1, 1, 1, 1, 1, 1]),
            (0x0578, [10, 10, 10, 10, 9, 8, 7, 6, 5, 1, 1, 1, 1, 1]),
            (0x0686, [10, 10, 10, 10, 10, 9, 8, 7, 6, 5, 1, 1, 1, 1]),
            (0x055C, [10, 10, 10, 10, 10, 10, 9, 8, 7, 6, 5, 1, 1, 1]),
            (0x03EE, [10, 10, 10, 10, 10, 10, 9, 8, 7, 6, 5, 1, 1, 1]),
        ];
        for (resource_id, frames) in expected {
            for (tick, expected_frame) in frames.into_iter().enumerate() {
                let frame = MainMenuEntryPaintFrame {
                    generation: 1,
                    tick: tick as u8,
                };
                assert_eq!(
                    frame.sdbtnanm_frame(resource_id, ButtonGroup::A),
                    Some(expected_frame),
                    "resource {resource_id:#06x}, tick {tick}"
                );
            }
        }
    }

    #[test]
    fn presented_wave_advances_only_after_matching_present_and_deadline() {
        let start = Instant::now();
        let mut wave = ShellFrameWave::new_presented_main_menu(7);
        assert_eq!(wave.poll_presented(start), Some(PresentedPoll::Acquire));
        assert_eq!(wave.current_main_menu_frame(), None);
        assert!(wave.activate_after_acquire());
        let frame0 = wave.current_main_menu_frame().expect("tick 0");
        assert_eq!((frame0.generation(), frame0.tick()), (7, 0));
        let token0 = wave.mint_present_token(frame0).expect("token 0");
        wave.record_presented(token0, start).expect("accept tick 0");
        assert_eq!(
            wave.poll_presented(start + Duration::from_millis(29)),
            Some(PresentedPoll::WaitUntil(start + Duration::from_millis(30)))
        );
        assert_eq!(
            wave.poll_presented(start + Duration::from_millis(30)),
            Some(PresentedPoll::Acquire)
        );
        assert_eq!(wave.current_main_menu_frame().map(|f| f.tick()), Some(1));

        let frame1 = wave.current_main_menu_frame().expect("tick 1");
        wave.record_presented(
            wave.mint_present_token(frame1).expect("token 1"),
            start + Duration::from_millis(30),
        )
        .expect("accept tick 1");
        assert_eq!(
            wave.poll_presented(start + Duration::from_secs(1)),
            Some(PresentedPoll::Acquire)
        );
        assert_eq!(wave.current_main_menu_frame().map(|f| f.tick()), Some(2));
        assert_eq!(
            wave.poll_presented(start + Duration::from_secs(1)),
            Some(PresentedPoll::Acquire),
            "a stall must not release a second unpresented tick"
        );
        assert_eq!(wave.current_main_menu_frame().map(|f| f.tick()), Some(2));
    }

    #[test]
    fn presented_wave_rejects_stale_wrong_and_double_tokens() {
        let start = Instant::now();
        let mut wave = ShellFrameWave::new_presented_main_menu(9);
        assert!(wave.activate_after_acquire());
        assert_eq!(
            wave.record_presented(
                MainMenuEntryPresentToken {
                    generation: 8,
                    tick: 0,
                },
                start,
            ),
            Err(PresentedCommitError::GenerationMismatch)
        );
        assert_eq!(
            wave.record_presented(
                MainMenuEntryPresentToken {
                    generation: 9,
                    tick: 1,
                },
                start,
            ),
            Err(PresentedCommitError::TickMismatch)
        );
        wave.record_presented(
            MainMenuEntryPresentToken {
                generation: 9,
                tick: 0,
            },
            start,
        )
        .expect("matching token");
        assert_eq!(
            wave.record_presented(
                MainMenuEntryPresentToken {
                    generation: 9,
                    tick: 0,
                },
                start,
            ),
            Err(PresentedCommitError::NotReady)
        );
    }

    #[test]
    fn terminal_tick_holds_then_completes_without_a_tick_fourteen() {
        let start = Instant::now();
        let mut wave = ShellFrameWave::new_presented_main_menu(11);
        assert!(wave.activate_after_acquire());
        let mut accepted_at = start;
        for expected_tick in 0..=MAIN_MENU_TERMINAL_TICK {
            let frame = wave.current_main_menu_frame().expect("ready frame");
            assert_eq!(frame.tick(), expected_tick);
            let token = wave.mint_present_token(frame).expect("matching token");
            wave.record_presented(token, accepted_at)
                .expect("accepted present");
            if expected_tick < MAIN_MENU_TERMINAL_TICK {
                accepted_at += Duration::from_millis(30);
                assert_eq!(
                    wave.poll_presented(accepted_at),
                    Some(PresentedPoll::Acquire)
                );
            }
        }
        assert_eq!(
            wave.poll_presented(accepted_at + Duration::from_millis(29)),
            Some(PresentedPoll::WaitUntil(
                accepted_at + Duration::from_millis(30)
            ))
        );
        assert_eq!(
            wave.poll_presented(accepted_at + Duration::from_millis(30)),
            Some(PresentedPoll::Complete)
        );
        assert!(wave.is_presented_completing(11));
        assert_eq!(wave.current_main_menu_frame(), None);
    }

    #[test]
    fn poisoned_presented_wave_exposes_no_frame_or_wake() {
        let mut wave = ShellFrameWave::new_presented_main_menu(13);
        assert!(wave.activate_after_acquire());
        wave.poison_presented();
        assert!(wave.is_presented_poisoned());
        assert_eq!(
            wave.poll_presented(Instant::now()),
            Some(PresentedPoll::Poisoned)
        );
        assert_eq!(wave.current_main_menu_frame(), None);
        assert_eq!(wave.presented_wake_deadline(), None);
    }

    #[test]
    fn modal_and_in_game_dialogs_do_not_slide() {
        // Modal confirm/body-ok and the in-game Options dialog are excluded.
        for id in [0x0120u16, 0x00CE, 0x0BBB] {
            assert!(!is_slide_eligible(DialogId(id)), "{id:#06x} must not slide");
            assert_eq!(slot_count_for(DialogId(id)), None);
        }
    }

    #[test]
    fn allow_listed_but_unrendered_dialogs_have_no_slot_count() {
        // 0x94/0x6B are eligible per research but have no renderer yet, so
        // the app layer never drives them; slot count is therefore unknown.
        for id in [0x0094u16, 0x006B] {
            assert!(is_slide_eligible(DialogId(id)));
            assert_eq!(slot_count_for(DialogId(id)), None);
        }
    }
}

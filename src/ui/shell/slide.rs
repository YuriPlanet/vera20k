//! Data-driven first-paint slide eligibility + frame schedule (contract C11).
//!
//! Every allow-listed front-end shell dialog (main menu `0xE2`, single player
//! `0x100`, skirmish setup `0x102`, …) slides in on its own first paint and
//! slides out when it closes. The slide engine `0x006071E0` animates the
//! dialog's right-panel column on a 30 ms tick: the SDBTNANM frame of every tile
//! row (buttons and empty tiles), and on Skirmish the map button and the top
//! panel's warning display. Nothing moves. This module owns the two
//! render-agnostic halves of that behaviour:
//!   * **eligibility data** — the dialog-id allow-list (`is_slide_eligible`) and
//!     each rendered dialog's slide column (`slide_spec_for`), and
//!   * **the frame schedule** — [`ShellFrameWave`] over a [`SlideColumn`]: the
//!     SDBTNANM frame of every right-panel tile row, the map button and the top
//!     panel per tick, with the native cadence and loop bound, pinned to an
//!     executed run of `0x006071E0` (`tools/storage_oracle/shell_slide_engine.py`).
//!
//! Render-agnostic: depends only on `ui::shell` + `std` (no sim/render/assets),
//! honouring the `ui/` layering rule. The app layer (`app::frontend::shell_transition`) maps
//! the showing screen to a `DialogId`, drives the wave each frame, and plays the
//! start cue (`GUIMoveInSound`, stock `MenuSlideIn`); the stock-empty end cue
//! (`ShellButtonSlideSound`) stays silent.

use std::time::{Duration, Instant};

use super::descriptor::DialogId;
use super::geom::RightPanelRects;

/// One animation tick per 30 ms, advancing exactly one frame (never skipped).
pub(crate) const WAVE_TICK_MS: u32 = 30;
/// Extra ticks after the last schedule entry so the ramp completes. The loop
/// bound is `max(schedule entry) + WAVE_TAIL_TICKS` (`0x006076A4`).
pub(crate) const WAVE_TAIL_TICKS: u32 = 6;
/// Linear ramp length (delta 0..=5 inclusive => 6 steps).
pub(crate) const WAVE_RAMP_STEPS: i32 = 6;

/// SDBTNANM frame constants per button group. Each tuple is
/// `(held_before, ramp_base, held_after)`. With `dir() = -1` on slide-IN, the IN
/// ramp counts DOWN from `base`; the held terminals are distinct constants, not
/// `base`. Slide-OUT uses `dir() = +1` (ramp counts UP).
/// Group A = a button row (SDBTNANM 10→5, settle 1).
/// Group B = an empty tile row (SDBTNANM 16→11, settle 0, the plate).
struct WaveFrames {
    before: i32,
    base: i32,
    after: i32,
}
/// SHOW: hold 10 → ramp 10,9,8,7,6,5 → settle 1.
const GROUP_A_IN: WaveFrames = WaveFrames {
    before: 10,
    base: 10,
    after: 1,
};
/// CLOSE: hold 1 → ramp 5,6,7,8,9,10 → settle 10.
const GROUP_A_OUT: WaveFrames = WaveFrames {
    before: 1,
    base: 5,
    after: 10,
};
/// SHOW: hold 10 → ramp 16,15,14,13,12,11 → settle 0.
const GROUP_B_IN: WaveFrames = WaveFrames {
    before: 10,
    base: 16,
    after: 0,
};
/// CLOSE: hold 0 → ramp 11,12,13,14,15,16 → settle 10.
const GROUP_B_OUT: WaveFrames = WaveFrames {
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
enum ButtonGroup {
    /// A row holding a button.
    A,
    /// An empty tile row: its shutter opens onto the plate or closes over it.
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

// --- Data-driven slide eligibility + per-dialog slide column -----------------

/// What `0x006071E0` animates for one rendered dialog besides its tile rows:
/// the visible top buttons (counted by `0x0060A180` through the `0x00608CD0`
/// list), the bottom button (counted by `0x0060A250` through the `0x00609730`
/// list) and the record flags set at creation (`0x00622820`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SlideDialogSpec {
    pub dialog_id: u16,
    pub top_buttons: u32,
    pub bottom_button: bool,
    /// Record `+0xD6`: the map button takes the column's first tile row.
    pub map_button: bool,
    /// Record `+0xD5`: the top panel, whose warning display moves last.
    pub top_panel: bool,
}

impl SlideDialogSpec {
    /// The column at a resolution with `rows` right-panel tile rows.
    pub(crate) fn column(self, rows: u32) -> SlideColumn {
        SlideColumn {
            rows,
            top_buttons: self.top_buttons,
            bottom_button: self.bottom_button,
            map_button: self.map_button,
            top_panel: self.top_panel,
        }
    }
}

/// Front-end shell dialogs rendered here. `0xE2`: Single Player, Internet,
/// Network, Movies & Credits and Options on top, Exit (`0x3EE`) at the bottom.
/// `0x100` and `0x101`: three page buttons, Main Menu (`0x686`) at the bottom.
/// `0x129`: Play Movie, then Back (`0x686`). `0x102`: Start Game and Choose Map,
/// then Back (`0x5C0`), with the map button and the top panel. `0x94`: only
/// Back (`0x686`); its one top-list button, Load `0x40E`, stays hidden
/// (`0x0052F05C`), and `0x0060A180` counts visible buttons only. `0xB7` from
/// Single Player: Load (`0x40F`, counted even while disabled), then Back
/// (`0x686`).
pub(crate) const RENDERED_SHELL_SLIDES: &[SlideDialogSpec] = &[
    SlideDialogSpec {
        dialog_id: 0x00E2,
        top_buttons: 5,
        bottom_button: true,
        map_button: false,
        top_panel: false,
    },
    SlideDialogSpec {
        dialog_id: 0x0100,
        top_buttons: 3,
        bottom_button: true,
        map_button: false,
        top_panel: false,
    },
    SlideDialogSpec {
        dialog_id: 0x0101,
        top_buttons: 3,
        bottom_button: true,
        map_button: false,
        top_panel: false,
    },
    SlideDialogSpec {
        dialog_id: 0x0129,
        top_buttons: 1,
        bottom_button: true,
        map_button: false,
        top_panel: false,
    },
    SlideDialogSpec {
        dialog_id: 0x0102,
        top_buttons: 2,
        bottom_button: true,
        map_button: true,
        top_panel: true,
    },
    SlideDialogSpec {
        dialog_id: 0x0094,
        top_buttons: 0,
        bottom_button: true,
        map_button: false,
        top_panel: false,
    },
    SlideDialogSpec {
        dialog_id: 0x00B7,
        top_buttons: 1,
        bottom_button: true,
        map_button: false,
        top_panel: false,
    },
];

/// Front-end shell dialog ids that slide on first paint (the eligibility
/// allow-list, scoped to the front-end shells). The rendered shells
/// (`RENDERED_SHELL_SLIDES`) plus the front-end dialogs documented as
/// allow-listed but not yet rendered here (`0x6B` per
/// `docs/research/skirmish-ui/SHELL_FIRST_PAINT_SLIDE_GENERIC_TRIGGER_GHIDRA_REPORT.md`
/// §3); it slides once a renderer maps to it and it gains a
/// `RENDERED_SHELL_SLIDES` entry. The original's full list
/// (`0x0060C540`, 55 ids) also marks Options `0xD5` and its children, Score
/// `0x108`, the in-game menu dialogs (`0xB5`, `0xB6`, `0xB8`, `0xBBA`,
/// `0xBBB`) and the LAN/WOL setup dialogs; none of them slides here yet.
/// `0xB7` slides only outside a suspended game (`0x00612690`), which is the
/// only place it is rendered as a family page. Message boxes (`0x120`
/// confirm, `0xCE` body-ok) are not in it.
pub(crate) const SHELL_SLIDE_ALLOW_LIST: &[u16] = &[
    0x00E2, 0x0094, 0x006B, 0x00B7, 0x0100, 0x0101, 0x0102, 0x0129,
];

/// Whether a dialog plays the first-paint controls-reveal slide.
pub(crate) fn is_slide_eligible(id: DialogId) -> bool {
    SHELL_SLIDE_ALLOW_LIST.contains(&id.0)
}

/// The slide column spec of a rendered shell dialog. `None` for an
/// allow-listed dialog that has no renderer here yet — the app layer only ever
/// drives the slide for dialogs it actually paints.
pub(crate) fn slide_spec_for(id: DialogId) -> Option<SlideDialogSpec> {
    RENDERED_SHELL_SLIDES
        .iter()
        .find(|spec| spec.dialog_id == id.0)
        .copied()
}

// --- Frame schedule (the wave) -----------------------------------------------

/// A dialog's slide column at one resolution (`0x006071E0`). The schedule array
/// (`0x00607646..0x006076A4`) holds one entry per regular tile row (`c + 1`),
/// the map button at 0 and the top panel at `rows + 3`, so the loop runs
/// `max + 6 = rows + 9` ticks whatever the flags. The regular rows are
/// `rows - 1` (all `rows` without a bottom button) and start one tile lower
/// with the map button; the first `top_buttons` of them are buttons (group A),
/// the rest empty tiles (group B). The bottom button is drawn over the last
/// tile row with its own entry at `rows`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SlideColumn {
    /// Right-panel tile rows (`[0x00B0FA20]`, from `0x0072EE88`).
    pub rows: u32,
    pub top_buttons: u32,
    pub bottom_button: bool,
    pub map_button: bool,
    pub top_panel: bool,
}

/// One SDBTNANM draw of the slide engine: the panel tile row (0 = the first
/// tile under the top panel) and the frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ColumnDraw {
    pub row: u32,
    pub frame: usize,
}

/// One top-panel draw of the slide engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PanelArt {
    /// SDTP.SHP at the panel top: 0 = the warning housing, 1 = the open top.
    Sdtp(usize),
    /// SDWRNTMP.SHP at the panel top: the warning display moving (0..=5).
    Warning(usize),
    /// SDMPBTN.SHP one tile above the column: 0 = the map button in place,
    /// 6 = only its rail left.
    MapButton(usize),
}

/// Top-left of a column row's SDBTNANM frame `shape_w` wide: right-aligned
/// over the row's tile, as `0x006071E0` draws it (the executed positions in
/// `tools/storage_oracle/shell_slide_engine.json`).
pub(crate) fn column_draw_origin(panel: RightPanelRects, row: u32, shape_w: i32) -> (i32, i32) {
    (
        panel.tile.x + panel.tile.w - shape_w,
        panel.tile.y + row as i32 * panel.tile.h,
    )
}

/// Top-left of a top-panel shape of `shape_w` x `shape_h`: SDTP and SDWRNTMP
/// at the panel top; SDMPBTN right-aligned with its bottom on the first tile
/// row's bottom, where the steady map button also sits.
pub(crate) fn panel_art_origin(
    panel: RightPanelRects,
    art: PanelArt,
    shape_w: i32,
    shape_h: i32,
) -> (i32, i32) {
    match art {
        PanelArt::Sdtp(_) | PanelArt::Warning(_) => (panel.top.x, panel.top.y),
        PanelArt::MapButton(_) => (
            panel.tile.x + panel.tile.w - shape_w,
            panel.tile.y + panel.tile.h - shape_h,
        ),
    }
}

impl SlideColumn {
    /// Loop bound: the top panel's entry `rows + 3` plus the ramp tail.
    pub(crate) fn total_ticks(self) -> u32 {
        self.rows + 3 + WAVE_TAIL_TICKS
    }

    /// The SDBTNANM draws of `tick` in the engine's order (later on top): the
    /// regular rows top to bottom, then the bottom button.
    pub(crate) fn button_draws(self, tick: u32, direction: WaveDirection) -> Vec<ColumnDraw> {
        let tick = tick as i32;
        let first = u32::from(self.map_button);
        let regular = self.rows.saturating_sub(u32::from(self.bottom_button));
        let mut draws: Vec<ColumnDraw> = (0..regular)
            .map(|c| {
                let group = if c < self.top_buttons {
                    ButtonGroup::A
                } else {
                    ButtonGroup::B
                };
                ColumnDraw {
                    row: first + c,
                    frame: frame_for_tick(tick, c as i32 + 1, group, direction),
                }
            })
            .collect();
        if self.bottom_button {
            draws.push(ColumnDraw {
                row: self.rows - 1,
                frame: frame_for_tick(tick, self.rows as i32, ButtonGroup::A, direction),
            });
        }
        draws
    }

    /// The top-panel draws of `tick` in the engine's order (later on top). The
    /// map button's entry is tick 0 (`0x0060775C`, `0x00607985`); the top panel
    /// shows SDTP 0 (in) or 1 (out) until its entry at `rows + 3`, then SDTP 1
    /// with the warning display over it (`0x0060787F..0x00607935`). A map
    /// button closed by a slide-out is drawn under the top panel (`0x0060777F`).
    pub(crate) fn panel_art(self, tick: u32, direction: WaveDirection) -> Vec<PanelArt> {
        let entering = direction == WaveDirection::SlideIn;
        let tick = tick as i32;
        let map = self.map_button.then(|| {
            let frame = match (entering, tick < WAVE_RAMP_STEPS) {
                (true, true) => 6 - tick,
                (true, false) => 0,
                (false, true) => 1 + tick,
                (false, false) => 6,
            };
            PanelArt::MapButton(frame as usize)
        });
        let map_under = !entering && tick >= WAVE_RAMP_STEPS;
        let mut art = Vec::new();
        if map_under {
            art.extend(map);
        }
        if self.top_panel {
            let delta = tick - (self.rows as i32 + 3);
            if delta < 0 {
                art.push(PanelArt::Sdtp(usize::from(!entering)));
            } else {
                art.push(PanelArt::Sdtp(1));
                let frame = if entering { 5 - delta } else { delta };
                art.push(PanelArt::Warning(frame.clamp(0, 5) as usize));
            }
        }
        if !map_under {
            art.extend(map);
        }
        art
    }
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
    column: SlideColumn,
}

impl MainMenuEntryPaintFrame {
    pub(crate) fn generation(self) -> u64 {
        self.generation
    }

    pub(crate) fn tick(self) -> u8 {
        self.tick
    }

    /// The column's SDBTNANM draws on this entry tick.
    pub(crate) fn button_draws(self) -> Vec<ColumnDraw> {
        self.column
            .button_draws(u32::from(self.tick), WaveDirection::SlideIn)
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
    column: SlideColumn,
    direction: WaveDirection,
}

impl ShellFrameWave {
    pub(crate) fn new_first_paint_slide(column: SlideColumn, now: Instant) -> Self {
        Self {
            clock: WaveClock::Compatibility {
                last_step_at: now,
                tick: 0,
            },
            column,
            direction: WaveDirection::SlideIn,
        }
    }

    /// The teardown slide of a shown dialog (`0x00608070`): the same schedule
    /// and loop bound as the entry slide, frames counting up to the empty slot.
    pub(crate) fn new_slide_out(column: SlideColumn, now: Instant) -> Self {
        Self {
            direction: WaveDirection::SlideOut,
            ..Self::new_first_paint_slide(column, now)
        }
    }

    pub(crate) fn new_presented_main_menu(generation: u64, column: SlideColumn) -> Self {
        assert_ne!(generation, 0, "main-menu wave generation must be nonzero");
        Self {
            clock: WaveClock::PresentedMainMenu {
                generation,
                tick: 0,
                phase: PresentedPhase::Armed,
            },
            column,
            direction: WaveDirection::SlideIn,
        }
    }

    fn total_ticks(&self) -> u32 {
        self.column.total_ticks()
    }

    /// Last tick a presented `0xE2` entry shows.
    fn presented_terminal_tick(&self) -> u8 {
        u8::try_from(self.total_ticks() - 1).expect("slide fits a u8 tick")
    }

    pub(crate) fn is_complete(&self) -> bool {
        match self.clock {
            WaveClock::Compatibility { tick, .. } => tick >= self.total_ticks(),
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
        let total = self.total_ticks();
        let WaveClock::Compatibility { last_step_at, tick } = &mut self.clock else {
            return;
        };
        if *tick < total && now.duration_since(*last_step_at) >= step {
            *tick += 1;
            *last_step_at += step;
        }
    }

    /// The column's SDBTNANM draws on the current tick of a compatibility-clock
    /// wave, in draw order. 4-case per row: held-before / linear ramp (base +
    /// delta*dir) / held-after; the terminals are distinct constants.
    pub(crate) fn button_draws(&self) -> Vec<ColumnDraw> {
        let WaveClock::Compatibility { tick, .. } = self.clock else {
            panic!("column draws of a presented main-menu wave come from its paint frame");
        };
        self.column.button_draws(tick, self.direction)
    }

    /// The top-panel draws on the current tick of a compatibility-clock wave.
    pub(crate) fn panel_art(&self) -> Vec<PanelArt> {
        let WaveClock::Compatibility { tick, .. } = self.clock else {
            return Vec::new();
        };
        self.column.panel_art(tick, self.direction)
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
        let terminal = self.presented_terminal_tick();
        let WaveClock::PresentedMainMenu { tick, phase, .. } = &mut self.clock else {
            return None;
        };
        Some(match *phase {
            PresentedPhase::Armed | PresentedPhase::Ready => PresentedPoll::Acquire,
            PresentedPhase::WaitingUntil(deadline) if now < deadline => {
                PresentedPoll::WaitUntil(deadline)
            }
            PresentedPhase::WaitingUntil(_) if *tick < terminal => {
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
        Some(MainMenuEntryPaintFrame {
            generation,
            tick,
            column: self.column,
        })
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
    use crate::ui::shell::geom;

    // Retail SHP canvas sizes, as the oracle's shape headers carry them.
    const SDBTNANM_SIZE: (i32, i32) = (156, 42);
    const SDTP_SIZE: (i32, i32) = (168, 199);
    const SDWRNTMP_SIZE: (i32, i32) = (168, 177);
    const SDMPBTN_SIZE: (i32, i32) = (156, 84);

    fn spec(dialog_id: u16) -> SlideDialogSpec {
        slide_spec_for(DialogId(dialog_id)).expect("rendered dialog")
    }

    fn main_menu_column() -> SlideColumn {
        spec(0x00E2).column(9)
    }

    /// Screen draw of one column row at a resolution through the painters'
    /// position helper, as `CC_Draw_Shape` receives it: `[x, y, frame]`.
    fn button_draw_at(screen: (i32, i32), draw: ColumnDraw) -> [i64; 3] {
        let panel = geom::right_panel_rects(screen.0, screen.1);
        let (x, y) = column_draw_origin(panel, draw.row, SDBTNANM_SIZE.0);
        [i64::from(x), i64::from(y), draw.frame as i64]
    }

    fn art_at(screen: (i32, i32), art: PanelArt) -> (String, i64, i64, i64) {
        let panel = geom::right_panel_rects(screen.0, screen.1);
        let (name, frame, (w, h)) = match art {
            PanelArt::Sdtp(frame) => ("SDTP", frame, SDTP_SIZE),
            PanelArt::Warning(frame) => ("SDWRNTMP", frame, SDWRNTMP_SIZE),
            PanelArt::MapButton(frame) => ("SDMPBTN", frame, SDMPBTN_SIZE),
        };
        let (x, y) = panel_art_origin(panel, art, w, h);
        (name.into(), frame as i64, i64::from(x), i64::from(y))
    }

    /// `0x006071E0` executed under Unicorn for the family dialogs' button
    /// counts and flags at three resolutions, both directions
    /// (`tools/storage_oracle/shell_slide_engine.py`).
    #[test]
    fn column_schedule_matches_the_executed_slide_engine() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tools/storage_oracle/shell_slide_engine.json"
        ))
        .unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 36);
        for case in cases {
            let dialog = case["dialog"].as_str().unwrap();
            let dialog_id = u16::from_str_radix(dialog.trim_start_matches("0x"), 16).unwrap();
            let screen = (
                case["width"].as_i64().unwrap() as i32,
                case["height"].as_i64().unwrap() as i32,
            );
            let direction = match case["direction"].as_str().unwrap() {
                "in" => WaveDirection::SlideIn,
                _ => WaveDirection::SlideOut,
            };
            let rows = case["rows"].as_u64().unwrap() as u32;
            let label = format!("{dialog} {}x{} {direction:?}", screen.0, screen.1);
            assert_eq!(
                rows,
                geom::right_panel_rects(screen.0, screen.1).tile_count as u32,
                "{label}: [0xB0FA20] is the panel's tile rows"
            );
            let column = spec(dialog_id).column(rows);
            let ticks = case["ticks"].as_array().unwrap();
            assert_eq!(
                ticks.len() as u32,
                column.total_ticks(),
                "{label}: loop bound"
            );
            for (tick, native) in ticks.iter().enumerate() {
                let buttons: Vec<[i64; 3]> = column
                    .button_draws(tick as u32, direction)
                    .into_iter()
                    .map(|draw| button_draw_at(screen, draw))
                    .collect();
                let expected: Vec<[i64; 3]> = native["buttons"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|d| {
                        let d = d.as_array().unwrap();
                        [
                            d[0].as_i64().unwrap(),
                            d[1].as_i64().unwrap(),
                            d[2].as_i64().unwrap(),
                        ]
                    })
                    .collect();
                assert_eq!(buttons, expected, "{label}: button draws on tick {tick}");
                let art: Vec<_> = column
                    .panel_art(tick as u32, direction)
                    .into_iter()
                    .map(|art| art_at(screen, art))
                    .collect();
                let expected: Vec<_> = native["art"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|a| {
                        let a = a.as_array().unwrap();
                        (
                            a[0].as_str().unwrap().to_string(),
                            a[1].as_i64().unwrap(),
                            a[2].as_i64().unwrap(),
                            a[3].as_i64().unwrap(),
                        )
                    })
                    .collect();
                assert_eq!(art, expected, "{label}: panel art on tick {tick}");
            }
            let end = if direction == WaveDirection::SlideIn {
                "0x4EC"
            } else {
                "0x4ED"
            };
            assert_eq!(case["end_message"], end, "{label}: end message");
        }
    }

    #[test]
    fn a_compatibility_wave_draws_the_column_of_its_current_tick() {
        let t0 = Instant::now();
        let column = spec(0x0129).column(9);
        let mut wave = ShellFrameWave::new_slide_out(column, t0);
        assert_eq!(
            wave.button_draws(),
            column.button_draws(0, WaveDirection::SlideOut)
        );
        for tick in 1..=4u64 {
            wave.advance(t0 + Duration::from_millis(30 * tick));
        }
        assert_eq!(wave.compatibility_tick(), Some(4));
        assert_eq!(
            wave.button_draws(),
            column.button_draws(4, WaveDirection::SlideOut)
        );
        assert!(
            wave.panel_art().is_empty(),
            "0x129 has no top panel or map button"
        );
        for tick in 5..=column.total_ticks() as u64 {
            wave.advance(t0 + Duration::from_millis(30 * tick));
        }
        assert!(wave.is_complete());
    }

    #[test]
    fn advance_steps_one_frame_per_30ms_and_never_collapses() {
        let t0 = Instant::now();
        let mut w = ShellFrameWave::new_first_paint_slide(spec(0x0100).column(9), t0);
        w.advance(t0 + Duration::from_millis(29));
        assert_eq!(w.compatibility_tick_for_test(), 0);
        w.advance(t0 + Duration::from_millis(30));
        assert_eq!(w.compatibility_tick_for_test(), 1);
        // A 1-second gap must still advance only ONE index (no catch-up).
        w.advance(t0 + Duration::from_millis(1030));
        assert_eq!(w.compatibility_tick_for_test(), 2);
    }

    #[test]
    fn rendered_shells_are_all_eligible() {
        for spec in RENDERED_SHELL_SLIDES {
            assert!(
                is_slide_eligible(DialogId(spec.dialog_id)),
                "rendered shell {:#06x} must be on the allow-list",
                spec.dialog_id
            );
            assert_eq!(slide_spec_for(DialogId(spec.dialog_id)), Some(*spec));
        }
    }

    #[test]
    fn presented_wave_advances_only_after_matching_present_and_deadline() {
        let start = Instant::now();
        let mut wave = ShellFrameWave::new_presented_main_menu(7, main_menu_column());
        assert_eq!(wave.poll_presented(start), Some(PresentedPoll::Acquire));
        assert_eq!(wave.current_main_menu_frame(), None);
        assert!(wave.activate_after_acquire());
        let frame0 = wave.current_main_menu_frame().expect("tick 0");
        assert_eq!((frame0.generation(), frame0.tick()), (7, 0));
        assert_eq!(
            frame0.button_draws(),
            main_menu_column().button_draws(0, WaveDirection::SlideIn)
        );
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
        let mut wave = ShellFrameWave::new_presented_main_menu(9, main_menu_column());
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
    fn terminal_tick_holds_then_completes_after_the_loop_bound() {
        // 0xE2 at 800x600: 9 tile rows, ticks 0..=17.
        let start = Instant::now();
        let mut wave = ShellFrameWave::new_presented_main_menu(11, main_menu_column());
        assert!(wave.activate_after_acquire());
        let terminal = main_menu_column().total_ticks() as u8 - 1;
        assert_eq!(terminal, 17);
        let mut accepted_at = start;
        for expected_tick in 0..=terminal {
            let frame = wave.current_main_menu_frame().expect("ready frame");
            assert_eq!(frame.tick(), expected_tick);
            let token = wave.mint_present_token(frame).expect("matching token");
            wave.record_presented(token, accepted_at)
                .expect("accepted present");
            if expected_tick < terminal {
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
        let mut wave = ShellFrameWave::new_presented_main_menu(13, main_menu_column());
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
        // Message boxes are not in 0x0060C540's list; the in-game Options
        // dialog 0xBBB is, but has no slide here yet.
        for id in [0x0120u16, 0x00CE, 0x0BBB] {
            assert!(!is_slide_eligible(DialogId(id)), "{id:#06x} must not slide");
            assert_eq!(slide_spec_for(DialogId(id)), None);
        }
    }

    #[test]
    fn allow_listed_but_unrendered_dialogs_have_no_column() {
        // 0x6B is eligible per research but has no renderer yet, so the app
        // layer never drives it.
        assert!(is_slide_eligible(DialogId(0x006B)));
        assert_eq!(slide_spec_for(DialogId(0x006B)), None);
    }
}

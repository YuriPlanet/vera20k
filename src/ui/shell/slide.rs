//! Data-driven first-paint slide eligibility + frame schedule (contract C11).
//!
//! Every allow-listed front-end shell dialog (main menu `0xE2`, single player
//! `0x100`, skirmish setup `0x102`, …) slides in on its own first paint and
//! slides out when it closes. The slide engine `0x006071E0` animates the
//! dialog's right-panel column on a 30 ms tick: the SDBTNANM frame of every tile
//! row (buttons and empty tiles), and on Skirmish the map button and the top
//! panel's warning display. Nothing moves. This module owns the two
//! render-agnostic halves of that behaviour:
//!   * **eligibility data** — each rendered dialog's slide column
//!     (`slide_spec_for`), and
//!   * **the frame schedule** — [`ShellFrameWave`] over a [`SlideColumn`]: the
//!     SDBTNANM frame of every right-panel tile row, the map button and the top
//!     panel per tick, with the native cadence and loop bound, pinned to an
//!     executed run of `0x006071E0` (`tools/storage_oracle/shell_slide_engine.py`).
//!
//! Render-agnostic: depends only on `ui::shell` + `std` (no sim/render/assets),
//! honouring the `ui/` layering rule. The app layer (`app::frontend::shell_transition`) maps
//! the showing screen to a `DialogResourceId`, drives the wave each frame, and plays the
//! start cue (`GUIMoveInSound`, stock `MenuSlideIn`) and the end cue
//! (`ShellButtonSlideSound`, empty in stock rules).

use crate::util::time::{Duration, Instant};

use super::descriptor::DialogResourceId;
use super::geom::RightPanelRects;

/// One animation tick per 30 ms, advancing exactly one frame (never skipped).
pub(crate) const WAVE_TICK_MS: u32 = 30;
/// Extra ticks after the last schedule entry so the ramp completes. The loop
/// bound is `max(schedule entry) + WAVE_TAIL_TICKS` (`0x006076A4`).
pub(crate) const WAVE_TAIL_TICKS: u32 = 6;
/// Linear ramp length (delta 0..=5 inclusive => 6 steps).
pub(crate) const WAVE_RAMP_STEPS: i32 = 6;

/// SDBTNANM frames of one button group and direction: held before the row's
/// entry, the ramp `base + delta * dir` (-1 on slide-in, +1 on slide-out) and
/// held after; the terminals are distinct constants, not `base`.
/// Group A = a button row (SHOW: 10 → 10..5 → 1; CLOSE: 1 → 5..10 → 10).
/// Group B = an empty tile row (SHOW: 10 → 16..11 → 0, the plate; CLOSE: 0 →
/// 11..16 → 10).
struct WaveFrames {
    before: i32,
    base: i32,
    after: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WaveDirection {
    SlideIn,
    /// Slide-OUT (close) ramp run by the teardown slide (`0x00608070` ->
    /// `0x006071E0` with `DL = 0`): the same schedule with the OUT frames.
    SlideOut,
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
    /// Campaign score `0x103`/`0xBC7`: the compositor draws SCRT frames then
    /// SCRBK instead of a top panel. These dialogs have no `+0xD5`/`+0xD6`.
    pub campaign_score: bool,
}

impl SlideDialogSpec {
    /// A dialog with `top_buttons` and its bottom button, no record flags.
    const fn new(dialog_id: u16, top_buttons: u32) -> Self {
        Self {
            dialog_id,
            top_buttons,
            bottom_button: true,
            map_button: false,
            top_panel: false,
            campaign_score: false,
        }
    }

    const fn map_button(self) -> Self {
        Self {
            map_button: true,
            ..self
        }
    }

    const fn top_panel(self) -> Self {
        Self {
            top_panel: true,
            ..self
        }
    }

    const fn campaign_score(self) -> Self {
        Self {
            campaign_score: true,
            ..self
        }
    }

    /// The column at a resolution with `rows` right-panel tile rows.
    pub(crate) fn column(self, rows: u32) -> SlideColumn {
        SlideColumn {
            rows,
            top_buttons: self.top_buttons,
            bottom_button: self.bottom_button,
            button_above_back: false,
            map_button: self.map_button,
            top_panel: self.top_panel,
            campaign_score: self.campaign_score,
        }
    }
}

/// Front-end shell dialogs rendered here, each with its top-list buttons and
/// one bottom button. `0xE2`: Single Player, Internet, Network, Movies &
/// Credits and Options, then Exit (`0x3EE`). `0x100`/`0x101`: three page
/// buttons, then Main Menu (`0x686`). `0x129`: Play Movie, then Back. `0x102`:
/// Start Game and Choose Map, then Back (`0x5C0`). `0x94`: only Back; its Load
/// `0x40E` stays hidden (`0x0052F05C`) and `0x0060A180` counts visible buttons
/// only. `0xB7`: Load (`0x40F`, counted even while disabled). `0xD5`: Keyboard
/// (`0x5CE`) and Network (`0x5CD`). `0x10E`: six. `0xA3`: Back only; Assign and
/// Reset All sit on the left. `0x6B`: Use Map and Create Random Map (`0x6C5`,
/// `0x583`, counted even while disabled), the top panel (`0x00622915`). `0x108`:
/// only Continue (`0x6D1`, `0x00609A71`). `0x103`: only Continue (`0x423`), its
/// compositor drawing SCRT then SCRBK (`+0xD7`, `0x006229CE`). LAN (`0x0060A180`/
/// `0x0060A250`, flags `0x006228E4..0x0062298A`): `0xBB`'s Observe `0x6CE` is
/// hidden; `0xBC`/`0xBD` and WOL setup `0xC2` (Start, Choose Map, Page) and
/// `0xC9` (Accept, Page) have the top panel and map button. WOL login `0x10F`:
/// Login, Forget Account (disabled, counted) and Manage Account. Custom lobby
/// `0x113`: eight (`0x00608CD0`) and the top panel; its runtime visibility is
/// not yet observed. WOL Options `0x2BC`: OK and Sound Options; its Sound
/// Options `0xD6`: Back only. `0x112`: Yes; `0xFE`: OK (created disabled,
/// counted); `0x117`: Cancel only; `0x122`: Yes; `0x116`: Create, Join, Add,
/// Page and Remove; `0x114`: Play Buddy, Clan, Tournament, Forget
/// (template-disabled, counted) and Manage (executed slide records). `0xBC7`
/// (LAN Cooperative campaign end): `0x103`'s record, Continue `0x6D1`. `0x105`
/// (random map): Use Map, Load, Save and Delete Map, and the top panel
/// (`0x0062290E`). VERA's Mods page: Use This Mod and Open Mods Folder.
pub(crate) const RENDERED_SHELL_SLIDES: &[SlideDialogSpec] = &[
    SlideDialogSpec::new(0x00BB, 2),
    SlideDialogSpec::new(0x00BC, 2).map_button().top_panel(),
    SlideDialogSpec::new(0x00BD, 1).map_button().top_panel(),
    SlideDialogSpec::new(0x00C2, 3).map_button().top_panel(),
    SlideDialogSpec::new(0x00C9, 2).map_button().top_panel(),
    SlideDialogSpec::new(0x00E2, 5),
    SlideDialogSpec::new(0x0100, 3),
    SlideDialogSpec::new(0x0101, 3),
    SlideDialogSpec::new(0x0129, 1),
    SlideDialogSpec::new(0x0102, 2).map_button().top_panel(),
    SlideDialogSpec::new(0x0094, 0),
    SlideDialogSpec::new(0x00B7, 1),
    SlideDialogSpec::new(0x00D5, 2),
    SlideDialogSpec::new(0x010E, 6),
    SlideDialogSpec::new(0x010F, 3),
    SlideDialogSpec::new(0x0113, 8).top_panel(),
    SlideDialogSpec::new(
        crate::ui::mods_shell::DIALOG.0,
        crate::ui::mods_shell::PAGE.stacked.len() as u32,
    ),
    SlideDialogSpec::new(0x02BC, 2),
    SlideDialogSpec::new(0x0112, 1),
    SlideDialogSpec::new(0x0117, 0),
    SlideDialogSpec::new(0x0122, 1),
    SlideDialogSpec::new(0x0116, 5),
    SlideDialogSpec::new(0x0114, 5),
    SlideDialogSpec::new(0x00FE, 1),
    SlideDialogSpec::new(0x00D6, 0),
    SlideDialogSpec::new(0x00A3, 0),
    SlideDialogSpec::new(0x006B, 2).top_panel(),
    SlideDialogSpec::new(0x0108, 0),
    SlideDialogSpec::new(0x0103, 0).campaign_score(),
    SlideDialogSpec::new(0x0BC7, 0).campaign_score(),
    SlideDialogSpec::new(0x0105, 4).top_panel(),
];

/// The slide column spec of a dialog that plays the first-paint
/// controls-reveal slide: every front-end shell in `RENDERED_SHELL_SLIDES`.
/// The original's full list (`0x0060C540`, 55 ids) also marks the in-game menu
/// dialogs (`0xB5`, `0xB6`, `0xB8`, `0xBBA`, `0xBBB`) and the remaining WOL
/// setup dialogs. `0xB7` slides only outside a suspended game (`0x00612690`),
/// which is the only place it is rendered as a family page. Message boxes
/// (`0x120` confirm, `0xCE` body-ok) are not in it.
pub(crate) fn slide_spec_for(id: DialogResourceId) -> Option<SlideDialogSpec> {
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
    /// An action in the physical slot immediately above Back.
    pub button_above_back: bool,
    /// Right-panel tile rows (`[0x00B0FA20]`, from `0x0072EE88`).
    pub rows: u32,
    pub top_buttons: u32,
    pub bottom_button: bool,
    pub map_button: bool,
    pub top_panel: bool,
    /// Campaign score's SCRT/SCRBK compositor (see `SlideDialogSpec`).
    pub campaign_score: bool,
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
    /// Campaign score `SCRT` frame at the dialog origin (+80 at 800+).
    Scrt(usize),
    /// Campaign score `SCRBK` at the dialog origin. `0x72E9F0` already painted
    /// it under the dialog, so a renderer needs no second draw; the fixture
    /// still records the native call for the slide-out tail.
    ScrBackdrop,
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
        // Campaign score art is placed from the dialog origin, not the panel.
        PanelArt::Scrt(_) | PanelArt::ScrBackdrop => (0, 0),
    }
}

impl SlideColumn {
    /// Loop bound: the top panel's entry `rows + 3` plus the ramp tail.
    pub(crate) fn total_ticks(self) -> u32 {
        self.rows + 3 + WAVE_TAIL_TICKS
    }

    /// The SDBTNANM draws of `tick` in the engine's order (later on top): the
    /// regular rows top to bottom, then the bottom button.
    /// The engine's start-tick table (`rows + 3` ints, `0x006075A0..`):
    /// row `c < rows` starts at `c + 1`; slot `rows` (the map button's) is 0,
    /// `rows + 1` (the top panel's) is `rows + 3` and `rows + 2` is 0. Top
    /// buttons past the panel (`0x113`'s eight at 640x480) read those slots.
    fn entry_tick(self, c: u32) -> i32 {
        let rows = self.rows;
        match c {
            c if c < rows => c as i32 + 1,
            c if c == rows + 1 => rows as i32 + 3,
            _ => 0,
        }
    }

    pub(crate) fn button_draws(self, tick: u32, direction: WaveDirection) -> Vec<ColumnDraw> {
        let tick = tick as i32;
        let first = u32::from(self.map_button);
        // Top buttons claim their rows even where the bottom button already
        // has the last one, and past the panel: `0x10E`'s six top buttons at
        // 640x480 (six rows) draw row 5 twice, `0x113`'s eight draw rows 6
        // and 7 below it, as the executed engine does.
        let regular = self
            .rows
            .saturating_sub(u32::from(self.bottom_button))
            .max(self.top_buttons);
        let mut draws: Vec<ColumnDraw> = (0..regular)
            .map(|c| ColumnDraw {
                row: first + c,
                frame: frame_for_tick(
                    tick,
                    self.entry_tick(c),
                    c < self.top_buttons
                        || (self.button_above_back && first + c == self.rows.saturating_sub(2)),
                    direction,
                ),
            })
            .collect();
        if self.bottom_button {
            draws.push(ColumnDraw {
                row: self.rows - 1,
                frame: frame_for_tick(tick, self.rows as i32, true, direction),
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
        // Campaign score `0x103`/`0xBC7`: the compositor draws the SCRT
        // shutter 5..0 on SHOW and 0..5 on CLOSE, then falls back to SCRBK at
        // the dialog origin for the rest of the teardown
        // (`tools/storage_oracle/shell_slide_engine.json`).
        if self.campaign_score {
            return if entering {
                vec![PanelArt::Scrt(
                    (WAVE_RAMP_STEPS - 1 - tick.min(WAVE_RAMP_STEPS - 1)) as usize,
                )]
            } else if tick < WAVE_RAMP_STEPS {
                vec![PanelArt::Scrt(tick as usize)]
            } else {
                vec![PanelArt::ScrBackdrop]
            };
        }
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

/// One slide of a shell dialog: the entry slide-in on its first paint or the
/// teardown slide-out (`0x006071E0`). Ticks `0..total_ticks` show one 30 ms
/// step each, never skipped; the slide ends when the tick reaches the loop
/// bound, so the bound itself is never drawn.
#[derive(Debug, Clone)]
pub(crate) struct ShellFrameWave {
    last_step_at: Instant,
    tick: u32,
    column: SlideColumn,
    direction: WaveDirection,
}

impl ShellFrameWave {
    pub(crate) fn new(column: SlideColumn, direction: WaveDirection, now: Instant) -> Self {
        Self {
            last_step_at: now,
            tick: 0,
            column,
            direction,
        }
    }

    /// Every tick has been shown.
    pub(crate) fn is_complete(&self) -> bool {
        self.tick >= self.column.total_ticks()
    }

    pub(crate) fn tick(&self) -> u32 {
        self.tick
    }

    /// Shell capture only: keep the wave at its current tick (the next step
    /// never becomes due).
    pub(crate) fn hold_for_capture(&mut self) {
        self.last_step_at = Instant::now() + Duration::from_secs(3600);
    }

    /// Advance at most ONE tick per call, only once >= 30 ms has elapsed.
    /// Never collapses multiple indices (faithful to one-frame-per-Sleep).
    pub(crate) fn advance(&mut self, now: Instant) {
        let step = Duration::from_millis(u64::from(WAVE_TICK_MS));
        if !self.is_complete() && now.duration_since(self.last_step_at) >= step {
            self.tick += 1;
            self.last_step_at += step;
        }
    }

    /// The column's SDBTNANM draws on the current tick, in draw order.
    pub(crate) fn button_draws(&self) -> Vec<ColumnDraw> {
        self.column.button_draws(self.tick, self.direction)
    }

    /// The top-panel draws on the current tick.
    pub(crate) fn panel_art(&self) -> Vec<PanelArt> {
        self.column.panel_art(self.tick, self.direction)
    }
}

/// One row's SDBTNANM frame on `tick`: held before its entry, the six-step
/// ramp, then held after.
fn frame_for_tick(tick: i32, entry_tick: i32, button: bool, direction: WaveDirection) -> usize {
    let f = match (button, direction) {
        (true, WaveDirection::SlideIn) => WaveFrames {
            before: 10,
            base: 10,
            after: 1,
        },
        (true, WaveDirection::SlideOut) => WaveFrames {
            before: 1,
            base: 5,
            after: 10,
        },
        (false, WaveDirection::SlideIn) => WaveFrames {
            before: 10,
            base: 16,
            after: 0,
        },
        (false, WaveDirection::SlideOut) => WaveFrames {
            before: 0,
            base: 11,
            after: 10,
        },
    };
    let dir = match direction {
        WaveDirection::SlideIn => -1,
        WaveDirection::SlideOut => 1,
    };
    let delta = tick - entry_tick;
    let frame = if delta < 0 {
        f.before
    } else if delta < WAVE_RAMP_STEPS {
        f.base + delta * dir
    } else {
        f.after
    };
    frame.max(0) as usize
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;
    use crate::ui::shell::geom;

    // Retail SHP canvas sizes, as the oracle's shape headers carry them.
    const SDBTNANM_SIZE: (i32, i32) = (156, 42);
    const SDTP_SIZE: (i32, i32) = (168, 199);
    const SDWRNTMP_SIZE: (i32, i32) = (168, 177);
    const SDMPBTN_SIZE: (i32, i32) = (156, 84);

    fn spec(dialog_id: u16) -> SlideDialogSpec {
        slide_spec_for(DialogResourceId(dialog_id)).expect("rendered dialog")
    }

    /// Screen draw of one column row at a resolution through the painters'
    /// position helper, as `CC_Draw_Shape` receives it: `[x, y, frame]`.
    fn button_draw_at(screen: (i32, i32), draw: ColumnDraw) -> [i64; 3] {
        let panel = geom::right_panel_rects(screen.0, screen.1);
        let cell = panel.row_cell(draw.row as i32, SDBTNANM_SIZE.0, SDBTNANM_SIZE.1);
        [i64::from(cell.x), i64::from(cell.y), draw.frame as i64]
    }

    fn art_at(screen: (i32, i32), art: PanelArt) -> (String, i64, i64, i64) {
        let panel = geom::right_panel_rects(screen.0, screen.1);
        let (name, frame, (x, y)) = match art {
            PanelArt::Sdtp(frame) => {
                let origin = panel_art_origin(panel, art, SDTP_SIZE.0, SDTP_SIZE.1);
                ("SDTP", frame, origin)
            }
            PanelArt::Warning(frame) => {
                let origin = panel_art_origin(panel, art, SDWRNTMP_SIZE.0, SDWRNTMP_SIZE.1);
                ("SDWRNTMP", frame, origin)
            }
            PanelArt::MapButton(frame) => {
                let origin = panel_art_origin(panel, art, SDMPBTN_SIZE.0, SDMPBTN_SIZE.1);
                ("SDMPBTN", frame, origin)
            }
            // `RadarBackground 0x0072E920`: SCRT at the dialog origin, plus
            // 0x50 from 800 pixels; SCRBK stays at the dialog origin.
            PanelArt::Scrt(frame) => {
                let (ox, oy) = geom::dialog_origin(screen.0, screen.1);
                let x = ox + if screen.0 >= 800 { 80 } else { 0 };
                ("xSCRT", frame, (x, oy))
            }
            PanelArt::ScrBackdrop => {
                let (ox, oy) = geom::dialog_origin(screen.0, screen.1);
                ("xSCRBK", 0, (ox, oy))
            }
        };
        (name.into(), frame as i64, i64::from(x), i64::from(y))
    }

    /// `0x006071E0` executed under Unicorn for the family dialogs' button
    /// counts and flags at three resolutions, both directions
    /// (`tools/storage_oracle/shell_slide_engine.py`).
    #[test]
    fn column_schedule_matches_the_executed_slide_engine() {
        let fixture: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/shell_slide_engine.json",
        ))
        .unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 132);
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
    fn a_wave_draws_the_column_of_its_current_tick() {
        let t0 = Instant::now();
        let column = spec(0x0129).column(9);
        let mut wave = ShellFrameWave::new(column, WaveDirection::SlideOut, t0);
        assert_eq!(
            wave.button_draws(),
            column.button_draws(0, WaveDirection::SlideOut)
        );
        for tick in 1..=4u64 {
            wave.advance(t0 + Duration::from_millis(30 * tick));
        }
        assert_eq!(wave.tick(), 4);
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
        let mut w = ShellFrameWave::new(spec(0x0100).column(9), WaveDirection::SlideIn, t0);
        w.advance(t0 + Duration::from_millis(29));
        assert_eq!(w.tick(), 0);
        w.advance(t0 + Duration::from_millis(30));
        assert_eq!(w.tick(), 1);
        // A 1-second gap must still advance only ONE index (no catch-up).
        w.advance(t0 + Duration::from_millis(1030));
        assert_eq!(w.tick(), 2);
    }

    #[test]
    fn modal_and_in_game_dialogs_do_not_slide() {
        // Message boxes are not in 0x0060C540's list; the in-game Options
        // dialog 0xBBB is, but has no slide here yet.
        for id in [0x0120u16, 0x00CE, 0x0BBB] {
            assert_eq!(
                slide_spec_for(DialogResourceId(id)),
                None,
                "{id:#06x} must not slide"
            );
        }
    }
}

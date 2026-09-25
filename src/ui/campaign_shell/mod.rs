//! Campaign selection dialog `0x94` (Single Player -> New Campaign).
//!
//! gamemd.exe reaches it from Single Player result `0x688` (state 8,
//! `0x0052DF05`): the art loader `0x0072D9A0` loads the faction emblems and
//! the background, `0x00622650` creates `0x94` with proc `0x0052EC00`, and the
//! state loops until the proc writes its result. Back (`0x686`) writes -1 and
//! state 1 recreates the Single Player page. Releasing the mouse on the emblem
//! it was pressed on writes that campaign's index (`0x0046CC90` by name:
//! `0x6EA` -> "all1", `0x6EC` -> "sov1") and stores the difficulty slider into
//! OptionsClass `Difficulty` (`[0x00A8EB64]`, `0x0052F2C3`).
//!
//! Hovering an emblem starts its animation (`0x4D3`, one frame per 100 ms from
//! `0x006033F0`) and, after 500 ms on it, its faction voice (the state loop at
//! `0x0052DFAF`). The static notifies the dialog of every painted frame
//! (`0x4D8`, `0x00615A34`); the wrap to frame 0 stops it (`0x0052F3C8`).

use std::time::{Duration, Instant};

use crate::ui::main_menu_dialogs::options::{
    TrackbarPress, trackbar_position_from_x, trackbar_press,
};
use crate::ui::shell::descriptor::DialogId;
use crate::ui::shell::geom::{RectPx, center_offset, dlu_rect};
use crate::ui::shell::menu_page::{self, MenuPageButtonSpec, MenuPageLayout, MenuPageSpec};

pub const CAMPAIGN_DIALOG: DialogId = DialogId(0x0094);
/// Allied emblem static (FSALG.SHP, kind 4).
pub const ALLIED_EMBLEM: u16 = 0x06EA;
/// Soviet emblem static (FSSLG.SHP, kind 4).
pub const SOVIET_EMBLEM: u16 = 0x06EC;
/// Difficulty trackbar `msctls_trackbar32`.
pub const DIFFICULTY_SLIDER: u16 = 0x050F;
/// Static the slider's `WM_HSCROLL` writes the difficulty name into.
pub const DIFFICULTY_VALUE: u16 = 0x0670;
pub const BACK_BUTTON: u16 = 0x0686;

/// Slider positions `0..=2` (`TBM_SETRANGE 0x20000`, `0x0052F105`).
pub const DIFFICULTY_MAX: u8 = 2;
/// Difficulty names by slider position (table `0x00822774`).
pub const DIFFICULTY_LABEL_KEYS: [&str; 3] = ["TXT_EASY", "TXT_NORMAL", "TXT_HARD"];
/// Delay before a hovered emblem's voice plays (`0x0052ED9C`).
pub const HOVER_VOICE_DELAY: Duration = Duration::from_millis(500);
/// Emblem animation step (`0x006033F0` for `0x94`'s emblems).
pub const EMBLEM_FRAME_INTERVAL: Duration = Duration::from_millis(100);

/// RT_DIALOG `0x94` right panel: only Back. Load `0x40E` and the campaign list
/// `0x455` stay hidden while `[0x00A8F7AD]` is clear (`0x0052F05C`), which
/// nothing in retail sets.
pub const CAMPAIGN_PAGE: MenuPageSpec = MenuPageSpec {
    dialog: CAMPAIGN_DIALOG,
    title_key: "GUI:CampaignMenu",
    stacked: &[],
    back: MenuPageButtonSpec {
        id: BACK_BUTTON,
        dlu_top: 253,
        csf_key: "GUI:Back",
        tooltip_key: "STT:CampaignButtonBack",
        result: Some(-1),
    },
};

/// The two campaigns `0x94` offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CampaignSide {
    Allied,
    Soviet,
}

impl CampaignSide {
    pub const ALL: [CampaignSide; 2] = [CampaignSide::Allied, CampaignSide::Soviet];

    pub fn emblem(self) -> u16 {
        match self {
            CampaignSide::Allied => ALLIED_EMBLEM,
            CampaignSide::Soviet => SOVIET_EMBLEM,
        }
    }

    pub fn from_emblem(id: u16) -> Option<Self> {
        Self::ALL.into_iter().find(|side| side.emblem() == id)
    }

    /// `[Battles]` campaign name the proc looks up (`0x00825C68`, `0x00825C6C`).
    pub fn campaign_name(self) -> &'static str {
        match self {
            CampaignSide::Allied => "all1",
            CampaignSide::Soviet => "sov1",
        }
    }

    /// Sound the hover schedules (`0x0052EFCE..0x0052EFF1`).
    pub fn voice(self) -> &'static str {
        match self {
            CampaignSide::Allied => "AlliedCampaignSelect",
            CampaignSide::Soviet => "SovietCampaignSelect",
        }
    }

    /// Caption static under the emblem (`0x7A7`, `0x7A8`) and its text, set on
    /// `0x497` (`0x0052F07F..0x0052F0DF`). It is also the emblem's status help.
    pub fn caption_key(self) -> &'static str {
        match self {
            CampaignSide::Allied => "STT:AlliedCampaignIcon",
            CampaignSide::Soviet => "STT:SovietCampaignIcon",
        }
    }

    fn index(self) -> usize {
        match self {
            CampaignSide::Allied => 0,
            CampaignSide::Soviet => 1,
        }
    }
}

/// Scenario difficulties a campaign starts with, by slider position: the
/// `0x0052E527` switch (table `0x0052EBE4`) writes Scenario `+0x610` and
/// `+0x60C` (easy 0, normal 1, hard 2). Positions 3 and 4 exist in the table
/// but the slider range stops at 2.
pub fn scenario_difficulties(position: u8) -> (u8, u8) {
    match position {
        0 => (2, 0),
        1 => (2, 1),
        2 => (1, 1),
        3 => (0, 1),
        _ => (0, 2),
    }
}

/// Dialog `0x94` geometry. The right panel, heading, monitor, status line and
/// Back follow the menu page rules. From 800 pixels wide, the emblems,
/// captions, slider and labels take fixed pixel rectangles
/// (`ShellDialog__GetControlOverrideRect` `0x00608500`, applied by
/// `0x0060AF50` with the half-margins beyond 800x600); narrower screens keep
/// the template rectangles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CampaignLayout {
    pub page: MenuPageLayout,
    pub allied: RectPx,
    pub soviet: RectPx,
    pub allied_caption: RectPx,
    pub soviet_caption: RectPx,
    pub slider: RectPx,
    pub difficulty_label: RectPx,
    pub difficulty_value: RectPx,
}

impl CampaignLayout {
    pub fn emblem(&self, side: CampaignSide) -> RectPx {
        match side {
            CampaignSide::Allied => self.allied,
            CampaignSide::Soviet => self.soviet,
        }
    }

    pub fn caption(&self, side: CampaignSide) -> RectPx {
        match side {
            CampaignSide::Allied => self.allied_caption,
            CampaignSide::Soviet => self.soviet_caption,
        }
    }

    /// The emblem whose window contains `(x, y)`.
    pub fn emblem_at(&self, x: i32, y: i32) -> Option<CampaignSide> {
        CampaignSide::ALL
            .into_iter()
            .find(|side| self.emblem(*side).contains(x, y))
    }
}

/// Shells at least this wide take the fixed control rectangles
/// (`[0x007F5BE4]`).
const OVERRIDE_MIN_WIDTH: u32 = 800;

pub fn compute_layout(screen_w: u32, screen_h: u32) -> CampaignLayout {
    let page = menu_page::compute_layout(&CAMPAIGN_PAGE, screen_w, screen_h);
    if screen_w < OVERRIDE_MIN_WIDTH {
        return CampaignLayout {
            page,
            allied: dlu_rect(125, 34, 174, 87),
            soviet: dlu_rect(125, 216, 173, 86),
            allied_caption: dlu_rect(4, 123, 415, 13),
            soviet_caption: dlu_rect(3, 304, 420, 13),
            slider: dlu_rect(137, 140, 140, 14),
            difficulty_label: dlu_rect(137, 167, 75, 12),
            difficulty_value: dlu_rect(203, 167, 75, 12),
        };
    }
    let dx = center_offset(screen_w as i32, 800);
    let dy = center_offset(screen_h as i32, 600);
    let at = |x: i32, y: i32, w: i32, h: i32| RectPx::new(x + dx, y + dy, w, h);
    // 0x00608564..0x006086CF.
    CampaignLayout {
        page,
        allied: at(141, 58, 348, 136),
        soviet: at(141, 364, 348, 136),
        allied_caption: at(10, 198, 610, 20),
        soviet_caption: at(10, 505, 610, 20),
        slider: at(177, 265, 272, 22),
        difficulty_label: at(177, 291, 141, 20),
        difficulty_value: at(309, 291, 141, 20),
    }
}

/// One emblem static's kind-4 animation (`+0x98` frame, `+0xA8` running).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EmblemAnimation {
    /// Frame the next paint draws (`+0x98`).
    next_frame: usize,
    /// Frame on screen: the last one painted.
    shown_frame: usize,
    /// Timer running, with the next step due at this time.
    next_step_at: Option<Instant>,
}

impl EmblemAnimation {
    pub fn shown_frame(&self) -> usize {
        self.shown_frame
    }

    pub fn running(&self) -> bool {
        self.next_step_at.is_some()
    }

    /// `0x4D5` with frame 0 then `0x4D4`: reset, repaint, stop.
    fn reset(&mut self) {
        *self = Self::default();
    }

    /// `0x4D3`: start the timer unless it already runs (`0x00615E40`).
    fn start(&mut self, now: Instant) {
        if self.next_step_at.is_none() {
            self.next_step_at = Some(now + EMBLEM_FRAME_INTERVAL);
        }
    }

    /// Paint every due timer step. Returns true when the painted frame
    /// wrapped the counter to 0: the static reports `0x4D8` with frame 0 and
    /// the dialog stops it (`0x4D4`), leaving the last frame on screen.
    fn advance(&mut self, now: Instant, frame_count: usize) -> bool {
        while let Some(due) = self.next_step_at {
            if now < due || frame_count == 0 {
                return false;
            }
            self.shown_frame = self.next_frame;
            self.next_frame = (self.next_frame + 1) % frame_count;
            self.next_step_at = Some(due + EMBLEM_FRAME_INTERVAL);
            if self.next_frame == 0 {
                self.next_step_at = None;
                return true;
            }
        }
        false
    }
}

/// What a release on the dialog did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CampaignRelease {
    None,
    /// The proc wrote the campaign's result (`0x0052F26B`).
    Selected(CampaignSide),
}

/// One `0x94` instance: the emblem under the cursor (`[0x00A8F7A8]`), the
/// captured press (`[0x00A8F7B0]`), the pending hover voice (`[0x00825C20]`
/// with its due time `[0x00A8F7A0]`), the emblem animations and the slider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CampaignShellState {
    hovered: Option<CampaignSide>,
    pressed: Option<CampaignSide>,
    pending_voice: Option<(CampaignSide, Instant)>,
    emblems: [EmblemAnimation; 2],
    /// Slider position `0..=DIFFICULTY_MAX`.
    pub difficulty: u8,
    /// Thumb captured by a press on it (`TrackBar_ProcessMouse` `0x0061D950`).
    slider_captured: bool,
}

impl CampaignShellState {
    /// `0x497`: the slider starts at OptionsClass `Difficulty` (0..=4, the
    /// trackbar clamps it into its range) and both emblems at frame 0. The
    /// posted range change already sends `WM_HSCROLL`, so the value static
    /// names the position from the first paint.
    pub fn open(options_difficulty: i32) -> Self {
        Self {
            hovered: None,
            pressed: None,
            pending_voice: None,
            emblems: [EmblemAnimation::default(); 2],
            difficulty: options_difficulty.clamp(0, i32::from(DIFFICULTY_MAX)) as u8,
            slider_captured: false,
        }
    }

    /// The difficulty name the value static `0x670` shows.
    pub fn difficulty_label_key(&self) -> &'static str {
        DIFFICULTY_LABEL_KEYS[usize::from(self.difficulty)]
    }

    pub fn slider_captured(&self) -> bool {
        self.slider_captured
    }

    /// A left press at slider-local `(x, y)` in a `width` x `height` trackbar.
    /// Returns whether the position changed (`WM_HSCROLL` and GenericClick).
    pub fn slider_press(&mut self, x: i32, y: i32, width: i32, height: i32) -> bool {
        match trackbar_press(self.difficulty, x, y, width, height, 0, DIFFICULTY_MAX) {
            TrackbarPress::Ignored => false,
            TrackbarPress::Capture => {
                self.slider_captured = true;
                false
            }
            TrackbarPress::Jump(position) => self.set_difficulty(position),
        }
    }

    /// Mouse movement with the button held: a captured thumb follows it.
    pub fn slider_drag(&mut self, x: i32, width: i32) -> bool {
        if !self.slider_captured {
            return false;
        }
        self.set_difficulty(trackbar_position_from_x(x, width, 0, DIFFICULTY_MAX))
    }

    /// Release, or movement without the button: the capture ends.
    pub fn slider_release(&mut self) {
        self.slider_captured = false;
    }

    pub fn hovered(&self) -> Option<CampaignSide> {
        self.hovered
    }

    pub fn pressed(&self) -> Option<CampaignSide> {
        self.pressed
    }

    pub fn emblem(&self, side: CampaignSide) -> &EmblemAnimation {
        &self.emblems[side.index()]
    }

    fn emblem_mut(&mut self, side: CampaignSide) -> &mut EmblemAnimation {
        &mut self.emblems[side.index()]
    }

    /// `WM_NCHITTEST` (`0x0052ED60`). `ChildWindowFromPointEx` returns the
    /// dialog itself over its background, so what matters is the emblem under
    /// the cursor, if any. With no press captured, a change of target stops
    /// the old emblem (`0x4D5` frame 0, `0x4D4`) and forgets the queued voice;
    /// a newly hovered emblem animates (`0x4D3`) and queues its voice 500 ms
    /// out.
    pub fn pointer_moved(&mut self, emblem_under: Option<CampaignSide>, now: Instant) {
        if self.pressed.is_some() || emblem_under == self.hovered {
            return;
        }
        if let Some(old) = self.hovered.take() {
            self.emblem_mut(old).reset();
        }
        self.pending_voice = None;
        if let Some(side) = emblem_under {
            self.pending_voice = Some((side, now + HOVER_VOICE_DELAY));
            self.emblem_mut(side).start(now);
            self.hovered = Some(side);
        }
    }

    /// `WM_LBUTTONDOWN` (`0x0052EEFA`): a press on an emblem captures the mouse
    /// and plays `GUIMainButtonSound` (`Rules +0x188`). Returns whether it did.
    pub fn pointer_down(&mut self, emblem_under: Option<CampaignSide>) -> bool {
        let Some(side) = emblem_under else {
            return false;
        };
        self.pressed = Some(side);
        true
    }

    /// `WM_LBUTTONUP` (`0x0052F1C0`): releasing on the pressed emblem selects
    /// its campaign; releasing on another emblem moves the highlight there;
    /// anywhere else clears it. The capture ends either way.
    pub fn pointer_up(
        &mut self,
        emblem_under: Option<CampaignSide>,
        now: Instant,
    ) -> CampaignRelease {
        let pressed = self.pressed.take();
        if let Some(old) = self.hovered.take() {
            self.emblem_mut(old).reset();
        }
        match emblem_under {
            Some(side) if pressed == Some(side) => CampaignRelease::Selected(side),
            Some(side) => {
                self.emblem_mut(side).start(now);
                self.hovered = Some(side);
                CampaignRelease::None
            }
            None => CampaignRelease::None,
        }
    }

    /// The voice to play at once on a selection: the pending hover voice, if
    /// any (`0x0052F333`).
    pub fn take_selection_voice(&mut self) -> Option<CampaignSide> {
        self.pending_voice.take().map(|(side, _)| side)
    }

    /// The state loop's hover voice (`0x0052DFAF`): once due, play it once.
    pub fn take_due_voice(&mut self, now: Instant) -> Option<CampaignSide> {
        let (side, due) = self.pending_voice?;
        if now < due {
            return None;
        }
        self.pending_voice = None;
        Some(side)
    }

    /// Step the running emblem animations; the hovered emblem's wrap stops it.
    /// Returns whether any frame changed.
    pub fn advance_emblems(&mut self, now: Instant, frame_counts: [usize; 2]) -> bool {
        let mut changed = false;
        for side in CampaignSide::ALL {
            let before = self.emblem(side).shown_frame;
            let running = self.emblem(side).running();
            self.emblem_mut(side)
                .advance(now, frame_counts[side.index()]);
            changed |= running && self.emblem(side).shown_frame != before;
        }
        changed
    }

    /// The next time something on the dialog changes without input.
    pub fn next_deadline(&self) -> Option<Instant> {
        let voice = self.pending_voice.map(|(_, due)| due);
        let emblems = self.emblems.iter().filter_map(|emblem| emblem.next_step_at);
        voice.into_iter().chain(emblems).min()
    }

    /// A position change sends `WM_HSCROLL` (`0x0061E672..0x0061E6AF`), whose
    /// handler (`0x0052EC59`) names the new position. Returns whether it
    /// changed.
    fn set_difficulty(&mut self, position: u8) -> bool {
        let position = position.min(DIFFICULTY_MAX);
        let changed = position != self.difficulty;
        self.difficulty = position;
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t0() -> Instant {
        Instant::now()
    }

    #[test]
    fn wide_screens_take_the_fixed_control_rects_centred() {
        let layout = compute_layout(800, 600);
        assert_eq!(layout.allied, RectPx::new(141, 58, 348, 136));
        assert_eq!(layout.slider, RectPx::new(177, 265, 272, 22));
        assert_eq!(layout.difficulty_value, RectPx::new(309, 291, 141, 20));
        let wide = compute_layout(1024, 768);
        assert_eq!(wide.soviet, RectPx::new(141 + 112, 364 + 84, 348, 136));
        let small = compute_layout(640, 480);
        assert_eq!(small.slider, dlu_rect(137, 140, 140, 14));
    }

    #[test]
    fn layout_puts_back_on_the_last_row_and_hit_tests_the_emblems() {
        let layout = compute_layout(800, 600);
        assert_eq!(
            layout.page.button_rect(BACK_BUTTON),
            Some(RectPx::new(644, 535, 156, 42))
        );
        assert!(
            layout.page.buttons.len() == 1,
            "only Back is on the right panel"
        );
        assert_eq!(
            layout.emblem_at(layout.allied.x + 1, layout.allied.y + 1),
            Some(CampaignSide::Allied)
        );
        assert_eq!(
            layout.emblem_at(layout.soviet.x + 1, layout.soviet.y + 1),
            Some(CampaignSide::Soviet)
        );
        assert_eq!(
            layout.emblem_at(layout.slider.x + 1, layout.slider.y + 1),
            None
        );
    }

    #[test]
    fn slider_starts_at_the_options_difficulty_clamped_to_its_range() {
        assert_eq!(CampaignShellState::open(1).difficulty, 1);
        assert_eq!(CampaignShellState::open(4).difficulty, 2);
        assert_eq!(CampaignShellState::open(-3).difficulty, 0);
        assert_eq!(
            CampaignShellState::open(1).difficulty_label_key(),
            "TXT_NORMAL"
        );
    }

    #[test]
    fn slider_presses_jump_beside_the_thumb_and_capture_on_it() {
        // 272x22 at 800x600: thumbs at x 1, 130 and 260 (executed 178/307/437
        // on screen), positions 0 below x 94, 1 below 180, else 2.
        let mut state = CampaignShellState::open(1);
        assert!(
            !state.slider_press(40, 3, 272, 22),
            "above the admitted strip"
        );
        assert!(state.slider_press(40, 10, 272, 22));
        assert_eq!(state.difficulty, 0);
        assert!(!state.slider_captured());
        assert!(
            !state.slider_press(5, 10, 272, 22),
            "on the thumb at position 0"
        );
        assert!(state.slider_captured());
        assert!(state.slider_drag(200, 272));
        assert_eq!(state.difficulty_label_key(), "TXT_HARD");
        state.slider_release();
        assert!(!state.slider_drag(20, 272));
    }

    #[test]
    fn hovering_an_emblem_queues_its_voice_after_half_a_second() {
        let start = t0();
        let mut state = CampaignShellState::open(1);
        state.pointer_moved(Some(CampaignSide::Allied), start);
        assert_eq!(state.hovered(), Some(CampaignSide::Allied));
        assert_eq!(
            state.take_due_voice(start + Duration::from_millis(499)),
            None
        );
        assert_eq!(
            state.take_due_voice(start + Duration::from_millis(500)),
            Some(CampaignSide::Allied)
        );
        assert_eq!(
            state.take_due_voice(start + Duration::from_secs(2)),
            None,
            "plays once"
        );
    }

    #[test]
    fn leaving_the_emblem_before_the_delay_cancels_the_voice() {
        let start = t0();
        let mut state = CampaignShellState::open(1);
        state.pointer_moved(Some(CampaignSide::Allied), start);
        // Another child or the dialog background: stop and forget.
        state.pointer_moved(None, start + Duration::from_millis(200));
        assert_eq!(state.hovered(), None);
        assert_eq!(state.take_due_voice(start + Duration::from_secs(1)), None);
        assert!(!state.emblem(CampaignSide::Allied).running());
    }

    #[test]
    fn a_hovered_emblem_plays_once_at_100_ms_a_frame_and_keeps_its_last_frame() {
        let start = t0();
        let mut state = CampaignShellState::open(1);
        state.pointer_moved(Some(CampaignSide::Allied), start);
        let frames = [4, 4];
        assert!(!state.advance_emblems(start + Duration::from_millis(99), frames));
        assert_eq!(state.emblem(CampaignSide::Allied).shown_frame(), 0);
        for (ms, frame) in [(100, 0), (200, 1), (300, 2), (400, 3)] {
            state.advance_emblems(start + Duration::from_millis(ms), frames);
            assert_eq!(
                state.emblem(CampaignSide::Allied).shown_frame(),
                frame,
                "at {ms} ms"
            );
        }
        assert!(
            !state.emblem(CampaignSide::Allied).running(),
            "the wrap stops it"
        );
        state.advance_emblems(start + Duration::from_secs(5), frames);
        assert_eq!(state.emblem(CampaignSide::Allied).shown_frame(), 3);
        // Leaving resets it to frame 0 (0x4D5).
        state.pointer_moved(None, start + Duration::from_secs(6));
        assert_eq!(state.emblem(CampaignSide::Allied).shown_frame(), 0);
    }

    #[test]
    fn releasing_on_the_pressed_emblem_selects_its_campaign() {
        let start = t0();
        let mut state = CampaignShellState::open(2);
        state.pointer_moved(Some(CampaignSide::Soviet), start);
        assert!(state.pointer_down(Some(CampaignSide::Soviet)));
        assert_eq!(
            state.pointer_up(
                Some(CampaignSide::Soviet),
                start + Duration::from_millis(50)
            ),
            CampaignRelease::Selected(CampaignSide::Soviet)
        );
        assert_eq!(state.take_selection_voice(), Some(CampaignSide::Soviet));
        assert_eq!(CampaignSide::Soviet.campaign_name(), "sov1");
    }

    #[test]
    fn a_press_dragged_elsewhere_selects_nothing() {
        let start = t0();
        let mut state = CampaignShellState::open(1);
        assert!(state.pointer_down(Some(CampaignSide::Allied)));
        // While captured, hover changes are ignored.
        state.pointer_moved(Some(CampaignSide::Soviet), start);
        assert_eq!(state.hovered(), None);
        assert_eq!(
            state.pointer_up(Some(CampaignSide::Soviet), start),
            CampaignRelease::None
        );
        assert_eq!(
            state.hovered(),
            Some(CampaignSide::Soviet),
            "highlight moves there"
        );
        assert!(state.pressed().is_none());
        assert!(!state.pointer_down(None), "only emblems capture");
    }

    #[test]
    fn slider_positions_map_to_scenario_difficulties() {
        assert_eq!(scenario_difficulties(0), (2, 0));
        assert_eq!(scenario_difficulties(1), (2, 1));
        assert_eq!(scenario_difficulties(2), (1, 1));
    }
}

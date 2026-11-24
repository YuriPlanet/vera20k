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

use crate::util::time::{Duration, Instant};

use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::geom::{RectPx, ShellScale, center_offset};
use crate::ui::shell::layout_data;
use crate::ui::shell::menu_page::{self, MenuPageButtonSpec, MenuPageLayout, MenuPageSpec};
use vera20k_gui::{NodeKind, NodeSpec, Prop, Value};

pub const CAMPAIGN_DIALOG: DialogResourceId = DialogResourceId(0x0094);
/// Allied emblem static (FSALG.SHP, kind 4).
pub const ALLIED_EMBLEM: u16 = 0x06EA;
/// Boot Camp emblem static (FSBCLG.SHP, kind 4). Original RA2 only; the
/// control id `0x6EB` sits between the Allied and Soviet emblems in dialog
/// `0x94`, but YR leaves it unused (`gamemd.exe 0x0052EE78` still handles it).
pub const BOOTCAMP_EMBLEM: u16 = 0x06EB;
/// Soviet emblem static (FSSLG.SHP, kind 4).
pub const SOVIET_EMBLEM: u16 = 0x06EC;
/// Difficulty trackbar `msctls_trackbar32`.
pub const DIFFICULTY_SLIDER: u16 = 0x050F;
pub const BACK_BUTTON: u16 = 0x0686;

/// Slider positions `0..=2` (`TBM_SETRANGE 0x20000`, `0x0052F105`).
pub const DIFFICULTY_MAX: i32 = 2;
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
    back: MenuPageButtonSpec::new(BACK_BUTTON, 253, "GUI:Back", "STT:CampaignButtonBack")
        .returning(-1),
};

/// `0x94`'s nodes: Back, the emblems and the difficulty slider, each with
/// its status help. An emblem takes a press like a button: the proc
/// captures it (`0x0052EEFA`) and a release on it selects its campaign
/// (`0x0052F26B`). The slider is a shell trackbar without the value plaque:
/// the value static `0x670` names the position instead.
pub fn nodes() -> NodeSpec {
    // Original RA2 has a third Boot Camp emblem (0x6EB). Include it always;
    // `CampaignLayout::control_rect` hides it from hit-testing under YR, where
    // the campaign dialog offers only Allied and Soviet.
    let emblems = CampaignSide::ALL_RA2.map(|side| {
        NodeSpec::new(NodeKind::Button)
            .id(side.emblem().into())
            .tooltip(side.caption_key())
    });
    let slider = NodeSpec::new(NodeKind::Trackbar)
        .id(DIFFICULTY_SLIDER.into())
        .tooltip("STT:CampaignSliderDifficulty")
        .with(Prop::Max, Value::Int(DIFFICULTY_MAX))
        .with(Prop::ShowValue, Value::Bool(false));
    CAMPAIGN_PAGE.nodes().children(emblems).child(slider)
}

/// The campaigns `0x94` offers. Original RA2 has three: Boot Camp, Allied and
/// Soviet (`gamemd.exe 0x0052EFCE..0x0052EFF1` resolves all three select
/// voices). VERA enables Boot Camp only under `-ra2`, since the YR campaign
/// menu has no boot camp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CampaignSide {
    Allied,
    Soviet,
    BootCamp,
}

impl CampaignSide {
    /// The two emblems YR shows. Kept as the default so YR behaviour and the
    /// existing diagnostics/tests are unchanged.
    pub const ALL: [CampaignSide; 2] = [CampaignSide::Allied, CampaignSide::Soviet];
    /// All three emblems, including Boot Camp (original RA2 only).
    pub const ALL_RA2: [CampaignSide; 3] = [
        CampaignSide::BootCamp,
        CampaignSide::Allied,
        CampaignSide::Soviet,
    ];

    /// The emblems active for the current launch mode.
    pub fn active() -> &'static [CampaignSide] {
        if crate::util::ra2_launch::enabled() {
            &Self::ALL_RA2
        } else {
            &Self::ALL
        }
    }

    pub fn emblem(self) -> u16 {
        match self {
            CampaignSide::Allied => ALLIED_EMBLEM,
            CampaignSide::Soviet => SOVIET_EMBLEM,
            CampaignSide::BootCamp => BOOTCAMP_EMBLEM,
        }
    }

    pub fn from_emblem(id: u16) -> Option<Self> {
        Self::ALL_RA2.into_iter().find(|side| side.emblem() == id)
    }

    /// `[Battles]` campaign name the proc looks up (`0x00825C68`, `0x00825C6C`).
    pub fn campaign_name(self) -> &'static str {
        match self {
            CampaignSide::Allied => "all1",
            CampaignSide::Soviet => "sov1",
            CampaignSide::BootCamp => "tut1",
        }
    }

    /// Sound the hover schedules (`0x0052EFCE..0x0052EFF1`).
    pub fn voice(self) -> &'static str {
        match self {
            CampaignSide::Allied => "AlliedCampaignSelect",
            CampaignSide::Soviet => "SovietCampaignSelect",
            CampaignSide::BootCamp => "BootCampSelect",
        }
    }

    /// Caption static under the emblem (`0x7A7`, `0x7A8`) and its text, set on
    /// `0x497` (`0x0052F07F..0x0052F0DF`). It is also the emblem's status help.
    pub fn caption_key(self) -> &'static str {
        match self {
            CampaignSide::Allied => "STT:AlliedCampaignIcon",
            CampaignSide::Soviet => "STT:SovietCampaignIcon",
            CampaignSide::BootCamp => "STT:CampaignAnimTutorial",
        }
    }

    fn index(self) -> usize {
        match self {
            CampaignSide::Allied => 0,
            CampaignSide::Soviet => 1,
            CampaignSide::BootCamp => 2,
        }
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
    /// Boot Camp emblem. Only meaningful under `-ra2`; in YR it is a copy of
    /// the Allied rect so code that reads it unconditionally stays safe.
    pub bootcamp: RectPx,
    pub allied_caption: RectPx,
    pub soviet_caption: RectPx,
    pub bootcamp_caption: RectPx,
    pub slider: RectPx,
    pub difficulty_label: RectPx,
    pub difficulty_value: RectPx,
    /// The mapping `fit_window` stretched the authored dialog
    /// with; [`compute_layout`] always reports `RETAIL_SCALE`. Paint and
    /// hit-testing both read their rects through this.
    pub scale: ShellScale,
}

impl CampaignLayout {
    pub fn emblem(&self, side: CampaignSide) -> RectPx {
        match side {
            CampaignSide::Allied => self.allied,
            CampaignSide::Soviet => self.soviet,
            CampaignSide::BootCamp => self.bootcamp,
        }
    }

    pub fn caption(&self, side: CampaignSide) -> RectPx {
        match side {
            CampaignSide::Allied => self.allied_caption,
            CampaignSide::Soviet => self.soviet_caption,
            CampaignSide::BootCamp => self.bootcamp_caption,
        }
    }

    /// The window of node `id` (see [`nodes`]). Boot Camp has no window under
    /// YR, which offers only the two campaign emblems.
    pub fn control_rect(&self, id: u16) -> Option<RectPx> {
        if id == BOOTCAMP_EMBLEM && !crate::util::ra2_launch::enabled() {
            return None;
        }
        match CampaignSide::from_emblem(id) {
            Some(side) => Some(self.emblem(side)),
            None if id == DIFFICULTY_SLIDER => Some(self.slider),
            None => self.page.button_rect(id),
        }
    }
}

/// Shells at least this wide take the fixed control rectangles
/// (`[0x007F5BE4]`).
const OVERRIDE_MIN_WIDTH: u32 = 800;

/// Original RA2 `game.exe` dialog `0x94` has a pixel override installed by
/// `ShellDialog__GetControlOverrideRect` (RA2 `0x001E64A0`), just like YR's
/// `0x00608500`. The RT_DIALOG template only carries the small DLU statics;
/// reading those directly puts the emblems/slider dozens of pixels too high.
/// These are the values RA2 writes at runtime:
///   0x6EA Allied (30,26,570,135), 0x6EB Boot Camp (82,187,468,108),
///   0x6EC Soviet (98,298,444,149), 0x50F slider (177,509,272,21),
///   0x670 difficulty value (309,476,141,20).
/// Captions are not separate overridden statics in RA2; the proc draws them
/// directly under each emblem, so they sit at the emblem's lower edge.
fn compute_ra2_layout(page: MenuPageLayout, screen_w: u32, screen_h: u32) -> CampaignLayout {
    let dx = if screen_w < OVERRIDE_MIN_WIDTH {
        0
    } else {
        center_offset(screen_w as i32, 800)
    };
    let dy = if screen_w < OVERRIDE_MIN_WIDTH {
        0
    } else {
        center_offset(screen_h as i32, 600)
    };
    let at = |x: i32, y: i32, w: i32, h: i32| RectPx::new(x + dx, y + dy, w, h);
    CampaignLayout {
        page,
        allied: at(30, 26, 570, 135),
        bootcamp: at(82, 187, 468, 108),
        soviet: at(98, 298, 444, 149),
        allied_caption: at(30, 161, 570, 20),
        bootcamp_caption: at(82, 295, 468, 20),
        soviet_caption: at(98, 447, 444, 20),
        slider: at(177, 509, 272, 21),
        difficulty_label: at(177, 476, 141, 20),
        difficulty_value: at(309, 476, 141, 20),
        scale: crate::ui::shell::geom::RETAIL_SCALE,
    }
}

pub fn compute_layout(screen_w: u32, screen_h: u32) -> CampaignLayout {
    let page = menu_page::compute_layout(&CAMPAIGN_PAGE, screen_w, screen_h);
    // Original RA2 installs its own control override rects for dialog 0x94;
    // the YR layout_data table does not describe them. Use the native RA2
    // geometry when launched with -ra2.
    if crate::util::ra2_launch::enabled() {
        return compute_ra2_layout(page, screen_w, screen_h);
    }
    // : 0x00608564..0x006086CF.
    let section = if screen_w < OVERRIDE_MIN_WIDTH {
        "0x94"
    } else {
        "0x94.Wide"
    };
    let rects = layout_data::place(
        section,
        crate::ui::shell::geom::RETAIL_SCALE,
        screen_w as i32,
        screen_h as i32,
    );
    let rect = |id| rects.get(id).unwrap_or_default();
    CampaignLayout {
        page,
        allied: rect(ALLIED_EMBLEM),
        soviet: rect(SOVIET_EMBLEM),
        // YR has no Boot Camp; keep a valid copy so unconditional readers stay
        // safe, but  hides the node from hit-testing.
        bootcamp: rect(ALLIED_EMBLEM),
        allied_caption: rect(0x7A7),
        soviet_caption: rect(0x7A8),
        bootcamp_caption: rect(0x7A7),
        slider: rect(DIFFICULTY_SLIDER),
        difficulty_label: rect(0xFF00),
        difficulty_value: rect(0xFF01),
        scale: crate::ui::shell::geom::RETAIL_SCALE,
    }
}

vera20k_gui::scaled_rects!(CampaignLayout {
    [scale]
    page, allied, soviet, bootcamp, allied_caption, soviet_caption, bootcamp_caption,
    slider, difficulty_label, difficulty_value
});

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

    /// One due `WM_TIMER` paints the next frame and advances the counter
    /// (`0x006159FC`); missed periods coalesce into that one message. When
    /// the counter wraps to 0 the static reports `0x4D8` with frame 0 and the
    /// dialog stops it (`0x4D4`), leaving the last painted frame on screen
    /// until the static repaints.
    fn advance(&mut self, now: Instant, frame_count: usize) {
        let Some(due) = self.next_step_at else {
            return;
        };
        if now < due || frame_count == 0 {
            return;
        }
        self.shown_frame = self.next_frame;
        self.next_frame = (self.next_frame + 1) % frame_count;
        if self.next_frame == 0 {
            self.next_step_at = None;
            return;
        }
        let mut next = due + EMBLEM_FRAME_INTERVAL;
        while next <= now {
            next += EMBLEM_FRAME_INTERVAL;
        }
        self.next_step_at = Some(next);
    }
}

/// One `0x94` instance: the emblem under the cursor (`[0x00A8F7A8]`), the
/// pending hover voice (`[0x00825C20]` with its due time `[0x00A8F7A0]`), the
/// emblem animations and the slider position its `WM_HSCROLL` names. The
/// press capture (`[0x00A8F7B0]`) and the slider's hold are the gui nodes'.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CampaignShellState {
    hovered: Option<CampaignSide>,
    pending_voice: Option<(CampaignSide, Instant)>,
    emblems: [EmblemAnimation; 3],
    /// Slider position `0..=DIFFICULTY_MAX`.
    difficulty: i32,
}

impl CampaignShellState {
    /// `0x497`: the slider starts at OptionsClass `Difficulty` (0..=4, the
    /// trackbar clamps it into its range) and both emblems at frame 0. The
    /// posted range change already sends `WM_HSCROLL`, so the value static
    /// names the position from the first paint.
    pub fn open(options_difficulty: i32) -> Self {
        Self {
            hovered: None,
            pending_voice: None,
            emblems: [EmblemAnimation::default(); 3],
            difficulty: options_difficulty.clamp(0, DIFFICULTY_MAX),
        }
    }

    /// Slider position `0..=DIFFICULTY_MAX` (`TBM_GETPOS`).
    pub fn difficulty(&self) -> i32 {
        self.difficulty
    }

    /// The difficulty name the value static `0x670` shows.
    pub fn difficulty_label_key(&self) -> &'static str {
        DIFFICULTY_LABEL_KEYS[self.difficulty as usize]
    }

    pub fn hovered(&self) -> Option<CampaignSide> {
        self.hovered
    }

    pub fn emblem(&self, side: CampaignSide) -> &EmblemAnimation {
        &self.emblems[side.index()]
    }

    fn emblem_mut(&mut self, side: CampaignSide) -> &mut EmblemAnimation {
        &mut self.emblems[side.index()]
    }

    /// `WM_NCHITTEST` (`0x0052ED60`), which the host sends only while no
    /// press holds the mouse. `ChildWindowFromPointEx` returns the dialog
    /// itself over its background, so what matters is the emblem under the
    /// cursor, if any. A change of target stops the old emblem (`0x4D5`
    /// frame 0, `0x4D4`) and forgets the queued voice; a newly hovered emblem
    /// animates (`0x4D3`) and queues its voice 500 ms out.
    pub fn pointer_moved(&mut self, emblem_under: Option<CampaignSide>, now: Instant) {
        if emblem_under == self.hovered {
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

    /// Released on the pressed emblem (`0x0052F26B`): the proc selects its
    /// campaign and stops the hovered emblem, which stays hovered
    /// (`0x0052F276..0x0052F2A3`).
    pub fn select(&mut self) {
        if let Some(hovered) = self.hovered {
            self.emblem_mut(hovered).reset();
        }
    }

    /// Any other `WM_LBUTTONUP` reaching the dialog (`0x0052F1CF`): with an
    /// emblem's capture, or over the background or an emblem. Releasing on
    /// an emblem stops the hovered one and restarts the one under the cursor
    /// from frame 0 (`0x0052F34C..0x0052F38D`); anywhere else stops and
    /// clears the hover (`0x0052F31F`, `0x0052F395`). The queued voice is
    /// kept.
    pub fn pointer_up(&mut self, emblem_under: Option<CampaignSide>, now: Instant) {
        match emblem_under {
            Some(side) => {
                if let Some(old) = self.hovered.take() {
                    self.emblem_mut(old).reset();
                }
                self.emblem_mut(side).reset();
                self.emblem_mut(side).start(now);
                self.hovered = Some(side);
            }
            None => {
                if let Some(old) = self.hovered.take() {
                    self.emblem_mut(old).reset();
                }
            }
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
        // 0x0052DFAF plays it only once the time is past the due time.
        if now <= due {
            return None;
        }
        self.pending_voice = None;
        Some(side)
    }

    /// Step the running emblem animations; the hovered emblem's wrap stops it.
    /// Returns whether any frame changed.
    pub fn advance_emblems(&mut self, now: Instant, frame_counts: [usize; 3]) -> bool {
        let mut changed = false;
        for side in CampaignSide::active() {
            let before = self.emblem(*side).shown_frame;
            let running = self.emblem(*side).running();
            self.emblem_mut(*side)
                .advance(now, frame_counts[side.index()]);
            changed |= running && self.emblem(*side).shown_frame != before;
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
    pub fn set_difficulty(&mut self, position: i32) -> bool {
        let position = position.clamp(0, DIFFICULTY_MAX);
        let changed = position != self.difficulty;
        self.difficulty = position;
        changed
    }
}

#[cfg(all(test, tests_ui))]
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
        assert_eq!(
            small.slider,
            crate::ui::shell::geom::dlu_rect(137, 140, 140, 14)
        );
    }

    #[test]
    fn layout_puts_back_on_the_last_row() {
        let layout = compute_layout(800, 600);
        assert_eq!(
            layout.page.button_rect(BACK_BUTTON),
            Some(RectPx::new(644, 535, 156, 42))
        );
        assert!(
            layout.page.buttons.len() == 1,
            "only Back is on the right panel"
        );
    }

    #[test]
    fn slider_starts_at_the_options_difficulty_clamped_to_its_range() {
        assert_eq!(CampaignShellState::open(1).difficulty(), 1);
        assert_eq!(CampaignShellState::open(4).difficulty(), 2);
        assert_eq!(CampaignShellState::open(-3).difficulty(), 0);
        assert_eq!(
            CampaignShellState::open(1).difficulty_label_key(),
            "TXT_NORMAL"
        );
    }

    /// The page's nodes on a stretched 1600x1200 window, the slider at 1.
    fn page() -> (vera20k_gui::Gui, CampaignLayout) {
        use crate::ui::shell::page as shell_page;
        let scale = crate::ui::shell::geom::shell_scale(1600, 1200);
        let layout = crate::ui::shell::geom::fit_window(1600, 1200, compute_layout);
        let mut gui = vera20k_gui::Gui::new(16);
        let root = gui.create(NodeKind::Dialog, None).unwrap();
        let dialog = gui
            .push_dialog(root, vera20k_gui::DialogKind::Page)
            .unwrap();
        gui.set_dialog_scale(dialog, scale);
        shell_page::ensure_built(&mut gui, root, nodes);
        shell_page::sync_rects(&mut gui, root, |id| layout.control_rect(id));
        shell_page::set(&mut gui, root, DIFFICULTY_SLIDER, Prop::Pos, Value::Int(1));
        (gui, layout)
    }

    fn click(gui: &mut vera20k_gui::Gui, (x, y): (i32, i32), up: Option<(i32, i32)>) {
        use vera20k_gui::{InputEvent, MouseButton::Left};
        gui.input(InputEvent::PointerMove { x, y }, 0);
        gui.input(InputEvent::PointerDown { x, y, button: Left }, 0);
        let (x, y) = up.unwrap_or((x, y));
        gui.input(InputEvent::PointerMove { x, y }, 0);
        gui.input(InputEvent::PointerUp { x, y, button: Left }, 0);
    }

    fn events(gui: &mut vera20k_gui::Gui) -> Vec<crate::ui::shell::page::PageEvent> {
        let events = gui.drain_events();
        crate::ui::shell::page::page_events(gui, events)
    }

    #[test]
    fn an_emblem_selects_on_a_release_over_its_own_press() {
        use crate::ui::shell::page::{PageCue, PageEvent};
        let (mut gui, layout) = page();
        let centre = |r: RectPx| (r.x + r.w / 2, r.y + r.h / 2);
        click(&mut gui, centre(layout.soviet), None);
        assert_eq!(
            events(&mut gui),
            [
                PageEvent::Cue(PageCue::Button),
                PageEvent::Activated(SOVIET_EMBLEM)
            ]
        );
        click(&mut gui, centre(layout.allied), Some(centre(layout.soviet)));
        assert_eq!(events(&mut gui), [PageEvent::Cue(PageCue::Button)]);
    }

    #[test]
    fn a_stretched_slider_presses_and_drags_on_its_authored_track() {
        use crate::ui::shell::page::PageEvent;
        // The 272x22 slider at 177,265 doubled. Authored, its thumbs sit at
        // x 1, 130 and 260 (executed 178/307/437 on screen) and a press
        // jumps to 0 below x 94; only the strip above y 4 takes a press.
        let (mut gui, _) = page();
        let at = |x: i32, y: i32| (2 * (177 + x), 2 * (265 + y));
        click(&mut gui, at(40, 3), None);
        let slid = |value, final_| PageEvent::Slid {
            id: DIFFICULTY_SLIDER,
            value,
            final_,
        };
        assert_eq!(
            events(&mut gui),
            [slid(1, true)],
            "above the admitted strip"
        );
        click(&mut gui, at(40, 10), Some(at(200, 10)));
        assert_eq!(
            events(&mut gui),
            [slid(0, false), slid(0, true)],
            "a rail press does not drag"
        );
        click(&mut gui, at(5, 10), Some(at(200, 10)));
        assert_eq!(events(&mut gui), [slid(2, false), slid(2, true)]);
    }

    #[test]
    fn hovering_an_emblem_queues_its_voice_after_half_a_second() {
        let start = t0();
        let mut state = CampaignShellState::open(1);
        state.pointer_moved(Some(CampaignSide::Allied), start);
        assert_eq!(state.hovered(), Some(CampaignSide::Allied));
        assert_eq!(
            state.take_due_voice(start + Duration::from_millis(500)),
            None
        );
        assert_eq!(
            state.take_due_voice(start + Duration::from_millis(501)),
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
        let frames = [4, 4, 4];
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
    fn a_release_on_another_emblem_moves_the_highlight_there() {
        let start = t0();
        let mut state = CampaignShellState::open(1);
        state.pointer_up(Some(CampaignSide::Soviet), start);
        assert_eq!(state.hovered(), Some(CampaignSide::Soviet));
        assert!(state.emblem(CampaignSide::Soviet).running());
    }

    #[test]
    fn a_selection_keeps_the_emblem_hovered_so_moving_on_it_queues_no_voice() {
        let start = t0();
        let mut state = CampaignShellState::open(1);
        state.pointer_moved(Some(CampaignSide::Allied), start);
        state.advance_emblems(start + Duration::from_millis(100), [5, 5, 5]);
        let at = start + Duration::from_millis(150);
        state.select();
        assert_eq!(state.take_selection_voice(), Some(CampaignSide::Allied));
        assert_eq!(state.hovered(), Some(CampaignSide::Allied));
        assert_eq!(state.emblem(CampaignSide::Allied).shown_frame(), 0);
        assert!(!state.emblem(CampaignSide::Allied).running());
        state.pointer_moved(Some(CampaignSide::Allied), at);
        assert_eq!(state.take_due_voice(at + Duration::from_secs(1)), None);
        assert!(!state.emblem(CampaignSide::Allied).running());
    }

    #[test]
    fn an_uncaptured_release_on_the_hovered_emblem_restarts_it() {
        let start = t0();
        let mut state = CampaignShellState::open(1);
        state.pointer_moved(Some(CampaignSide::Soviet), start);
        for ms in [100, 200] {
            state.advance_emblems(start + Duration::from_millis(ms), [5, 5, 5]);
        }
        assert_eq!(state.emblem(CampaignSide::Soviet).shown_frame(), 1);
        let at = start + Duration::from_millis(250);
        state.pointer_up(Some(CampaignSide::Soviet), at);
        assert_eq!(state.emblem(CampaignSide::Soviet).shown_frame(), 0);
        state.advance_emblems(at + Duration::from_millis(100), [5, 5, 5]);
        assert_eq!(state.emblem(CampaignSide::Soviet).shown_frame(), 0);
        assert!(
            state
                .take_due_voice(start + Duration::from_millis(501))
                .is_some(),
            "the queued voice survives the release"
        );
    }

    #[test]
    fn a_stalled_timer_paints_one_step() {
        let start = t0();
        let mut state = CampaignShellState::open(1);
        state.pointer_moved(Some(CampaignSide::Allied), start);
        state.advance_emblems(start + Duration::from_millis(450), [5, 5, 5]);
        assert_eq!(state.emblem(CampaignSide::Allied).shown_frame(), 0);
        assert_eq!(
            state.next_deadline(),
            Some(start + Duration::from_millis(500))
        );
    }
}

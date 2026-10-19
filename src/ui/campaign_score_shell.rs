//! Campaign-victory score page: the reachable retail dialog `0x103`.
//!
//! Campaign victory reaches `0x6C8E80` from `0x685960`; that body selects
//! dialog `0x103` and the dialog proc `0x6C8FF0`. Its `RT_DIALOG` template is
//! the campaign score page, not the skirmish results dialog `0x108`. The
//! control ids, captions, template DLU rectangles and style bits below come
//! from retail `gamemd.exe`'s `RT_DIALOG 0x103`. The tooltip keys and kind-1
//! reveal parameters were read by executing the existing
//! `shell_help_keys.help_label` and `shell_static_timers.run_getter` helpers
//! against that retail executable. The control windows are the executed
//! WM_INITDIALOG relayout (`shell_relayout.py`), not the template rects: at
//! 800x600 the text statics move 50 pixels up and `0x72B` takes the
//! `0x00608500` art override.
//!
//! The page opens with its statics hidden and the emblem timer unarmed; the
//! shell slide's SHOW completion (`0x4EC`) starts both and plays
//! `ScoreAnimSound`. The app supplies already-resolved display strings. This
//! module owns no score calculation, campaign progression or asset lookup.

use std::time::{Duration, Instant};

use crate::ui::shell::descriptor::DialogId;
use crate::ui::shell::geom::{self, RectPx, RightPanelRects, ShellScale, child_window};
use crate::ui::shell::layout::status_line_rect;
use crate::ui::shell::menu_page::{self, MenuPageButtonSpec, MenuPageSpec};
use crate::ui::shell::static_reveal::{Kind1Params, Kind1RevealWindow, PresentedKind1Static};

pub const CAMPAIGN_SCORE_DIALOG: DialogId = DialogId(0x0103);

pub const CONTINUE_BUTTON: u16 = 0x0423;
pub const TITLE_STATIC: u16 = 0x0694;
pub const STATUS_HELP_STATIC: u16 = 0x0695;
pub const PAR_TIME_LABEL_STATIC: u16 = 0x06EF;
pub const YOUR_TIME_LABEL_STATIC: u16 = 0x06F0;
pub const PAR_TIME_VALUE_STATIC: u16 = 0x06F1;
pub const YOUR_TIME_VALUE_STATIC: u16 = 0x06F2;
pub const RANK_STATIC: u16 = 0x06F3;
pub const COMMENDATION_STATIC: u16 = 0x06F4;
pub const ENEMIES_DESTROYED_LABEL_STATIC: u16 = 0x069F;
pub const ENEMIES_DESTROYED_VALUE_STATIC: u16 = 0x06F6;
pub const SCORE_ART_REGION_STATIC: u16 = 0x072B;

/// Retail `gamemd.exe` SHA-256 `3e81a61775d2745d1dabe397325ef663cd994ffc194da4e998e3bf5d2d308600`:
/// `0x006033F0` returns 100 ms for dialog `0x103` / static `0x72B`.
const CAMPAIGN_SCORE_EMBLEM_INTERVAL: Duration = Duration::from_millis(100);
/// `0x006C93C9..0x006C93E1` handles its frame-zero `0x4D8` by selecting SCRA
/// frame 29 again.
const CAMPAIGN_SCORE_EMBLEM_WRAP_FRAME: usize = 29;

/// Shared shell metadata for the right-edge heading, status help and bottom
/// Continue button. The campaign dialog's Continue control has its own id and
/// tooltip; the application reacts to that control id rather than a guessed
/// dialog return value.
pub const CAMPAIGN_SCORE_PAGE: MenuPageSpec = MenuPageSpec {
    dialog: CAMPAIGN_SCORE_DIALOG,
    title_key: "GUI:StandaloneScore",
    stacked: &[],
    back: MenuPageButtonSpec {
        id: CONTINUE_BUTTON,
        dlu_top: 346,
        csf_key: "GUI:Continue",
        tooltip_key: "STT:StandScoreButtonReturn",
        result: None,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CampaignScoreControlKind {
    Button,
    Static,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CampaignScoreAction {
    Continue,
}

/// Executed kind-1 timing from `0x602490`, `0x600CA0`, `0x6015E0` and
/// `0x601D20` for the campaign page's controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CampaignScoreRevealSpec {
    pub interval_ms: u64,
    pub step: u32,
    pub highlight_range: u32,
}

const TEXT_REVEAL: CampaignScoreRevealSpec = CampaignScoreRevealSpec {
    interval_ms: 30,
    step: 1,
    highlight_range: 8,
};

const LONG_TEXT_REVEAL: CampaignScoreRevealSpec = CampaignScoreRevealSpec {
    interval_ms: 15,
    step: 3,
    highlight_range: 16,
};

/// One control from retail `RT_DIALOG 0x103`. `template_dlu` preserves the
/// resource's raw 6x13 dialog units; `native_style` is its original Win32
/// style word. A `None` reveal marks controls which do not use kind-1 text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CampaignScoreControlSpec {
    pub id: u16,
    pub kind: CampaignScoreControlKind,
    pub template_dlu: RectPx,
    pub native_style: u32,
    pub caption_key: Option<&'static str>,
    pub tooltip_key: Option<&'static str>,
    pub action: Option<CampaignScoreAction>,
    pub reveal: Option<CampaignScoreRevealSpec>,
}

/// Controls are in the order stored in the retail `RT_DIALOG 0x103` template.
/// `0x72B` is the kind-4 static whose client rect `0x00608500` sizes to the
/// current SCRA frame; its 100 ms interval is `ShellStatic__AnimationInterval`
/// `0x006033F0`'s `0x103`/`0x72B` branch. The art itself is owned by the
/// renderer (`SCRT` panel plus the `SCRA` sequence), not by this layout module.
pub const CAMPAIGN_SCORE_CONTROLS: [CampaignScoreControlSpec; 12] = [
    CampaignScoreControlSpec {
        id: CONTINUE_BUTTON,
        kind: CampaignScoreControlKind::Button,
        template_dlu: RectPx::new(425, 346, 108, 23),
        native_style: 0x5000_000B,
        caption_key: Some("GUI:Continue"),
        tooltip_key: Some("STT:StandScoreButtonReturn"),
        action: Some(CampaignScoreAction::Continue),
        reveal: None,
    },
    CampaignScoreControlSpec {
        id: PAR_TIME_LABEL_STATIC,
        kind: CampaignScoreControlKind::Static,
        template_dlu: RectPx::new(125, 266, 55, 10),
        native_style: 0x5000_0001,
        caption_key: Some("GUI:ParTime"),
        tooltip_key: None,
        action: None,
        reveal: Some(TEXT_REVEAL),
    },
    CampaignScoreControlSpec {
        id: YOUR_TIME_LABEL_STATIC,
        kind: CampaignScoreControlKind::Static,
        template_dlu: RectPx::new(242, 266, 55, 10),
        native_style: 0x5000_0001,
        caption_key: Some("GUI:YourTime"),
        tooltip_key: None,
        action: None,
        reveal: Some(TEXT_REVEAL),
    },
    CampaignScoreControlSpec {
        id: PAR_TIME_VALUE_STATIC,
        kind: CampaignScoreControlKind::Static,
        template_dlu: RectPx::new(125, 277, 55, 10),
        native_style: 0x5000_0001,
        caption_key: Some("GUI:Blank"),
        tooltip_key: Some("STT:StandScoreParTime"),
        action: None,
        reveal: Some(TEXT_REVEAL),
    },
    CampaignScoreControlSpec {
        id: YOUR_TIME_VALUE_STATIC,
        kind: CampaignScoreControlKind::Static,
        template_dlu: RectPx::new(242, 277, 55, 10),
        native_style: 0x5000_0001,
        caption_key: Some("GUI:Blank"),
        tooltip_key: Some("STT:StandScoreYourTime"),
        action: None,
        reveal: Some(TEXT_REVEAL),
    },
    CampaignScoreControlSpec {
        id: RANK_STATIC,
        kind: CampaignScoreControlKind::Static,
        template_dlu: RectPx::new(85, 35, 257, 10),
        native_style: 0x5000_0000,
        caption_key: Some("GUI:Blank"),
        tooltip_key: Some("STT:StandScoreTitle"),
        action: None,
        reveal: Some(TEXT_REVEAL),
    },
    CampaignScoreControlSpec {
        id: COMMENDATION_STATIC,
        kind: CampaignScoreControlKind::Static,
        template_dlu: RectPx::new(85, 50, 257, 40),
        native_style: 0x5000_0000,
        caption_key: Some("GUI:Blank"),
        tooltip_key: Some("STT:StandScoreMessage"),
        action: None,
        reveal: Some(LONG_TEXT_REVEAL),
    },
    CampaignScoreControlSpec {
        id: ENEMIES_DESTROYED_LABEL_STATIC,
        kind: CampaignScoreControlKind::Static,
        template_dlu: RectPx::new(85, 100, 110, 10),
        native_style: 0x5000_0000,
        caption_key: Some("GUI:HeadsTaken"),
        tooltip_key: None,
        action: None,
        reveal: Some(TEXT_REVEAL),
    },
    CampaignScoreControlSpec {
        id: ENEMIES_DESTROYED_VALUE_STATIC,
        kind: CampaignScoreControlKind::Static,
        template_dlu: RectPx::new(200, 100, 142, 10),
        native_style: 0x5000_0000,
        caption_key: Some("GUI:Blank"),
        tooltip_key: Some("STT:StandScoreKills"),
        action: None,
        reveal: Some(TEXT_REVEAL),
    },
    CampaignScoreControlSpec {
        id: STATUS_HELP_STATIC,
        kind: CampaignScoreControlKind::Static,
        template_dlu: RectPx::new(10, 357, 303, 12),
        native_style: 0x5000_0200,
        caption_key: Some("GUI:Blank"),
        tooltip_key: None,
        action: None,
        reveal: Some(LONG_TEXT_REVEAL),
    },
    CampaignScoreControlSpec {
        id: SCORE_ART_REGION_STATIC,
        kind: CampaignScoreControlKind::Static,
        template_dlu: RectPx::new(72, 115, 282, 141),
        native_style: 0x5000_0007,
        caption_key: None,
        tooltip_key: None,
        action: None,
        reveal: None,
    },
    CampaignScoreControlSpec {
        id: TITLE_STATIC,
        kind: CampaignScoreControlKind::Static,
        template_dlu: RectPx::new(425, 1, 108, 10),
        native_style: 0x5002_0001,
        caption_key: Some("GUI:StandaloneScore"),
        tooltip_key: None,
        action: None,
        reveal: Some(TEXT_REVEAL),
    },
];

/// Already-resolved text from the campaign victory path. The app owns all
/// value formatting and sends these strings to the page unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CampaignScoreScreenModel {
    /// Campaign side index (`Scenario+0x34B8`): 0 allies, 1 Soviet, 2 Yuri.
    /// It selects the `0x72B` art window's 220- or 230-pixel height, the
    /// `0x00608500` x by one pixel, and the small/large asset tables.
    pub side: i32,
    pub par_time: String,
    pub your_time: String,
    pub enemies_destroyed: String,
    pub rank: String,
    pub commendation: String,
}

/// CSF strings resolved by the app for the fixed labels and the Continue
/// status line. The status text is installed only when the controller reports
/// the corresponding hover/help action.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CampaignScoreTexts {
    pub title: String,
    pub continue_label: String,
    pub continue_status: String,
    pub par_time_label: String,
    pub your_time_label: String,
    pub enemies_destroyed_label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CampaignScoreControlRect {
    pub id: u16,
    pub rect: RectPx,
}

/// Geometry for the campaign score dialog at one window size. The control
/// windows are the executed WM_INITDIALOG relayout of `0x103`
/// (`tools/storage_oracle/shell_relayout.py`): the authored template at
/// 640x480, its upper 50 pixels at 800x600 and the right-panel anchors of
/// `0x0060B1D0`/`0x0060B350`/`0x0060B550`. The `0x72B` art window is the
/// `0x00608500` override, not the template rect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CampaignScoreLayout {
    pub scale: ShellScale,
    pub screen: RectPx,
    /// `SCRBK` at its native SHP size: 472x448 at width 640, 632x568 else.
    /// Its top-left is the native dialog origin `[0x00B0FC1C]`.
    pub background: RectPx,
    /// `SCRT` frame 0: 472x448, centred in `SCRBK` (x + 80 from width 800).
    pub panel: RectPx,
    pub right_panel: RightPanelRects,
    pub lower_strip: RectPx,
    pub title: RectPx,
    pub status_help: RectPx,
    pub continue_button: RectPx,
    pub controls: [CampaignScoreControlRect; CAMPAIGN_SCORE_CONTROLS.len()],
}

/// Root window width the campaign score page was authored for.
const SCORE_PAGE_W: i32 = 800;
/// Root window height the campaign score page was authored for.
const SCORE_PAGE_H: i32 = 600;
/// `0x00601360`'s template rect for status line `0x695`; `0x0060B550`
/// anchors its converted 456x21 window to `dx + 10` and the bottom edge.
const STATUS_LINE_DLU: RectPx = RectPx::new(10, 357, 303, 12);
/// `0x0060B950` moves `0x103`'s eight text statics up by
/// `10 - (600 - 480) / 2` when the screen is at least 800 wide.
const TEXT_UPPER_SHIFT: i32 = 50;

impl CampaignScoreLayout {
    /// Retail window placement for the allied side (side 0).
    pub fn compute_layout(screen_w: u32, screen_h: u32) -> Self {
        Self::compute_layout_for_side(screen_w, screen_h, 0)
    }

    /// Retail window placement for one campaign side.
    pub fn compute_layout_for_side(screen_w: u32, screen_h: u32, side: i32) -> Self {
        let page = menu_page::compute_layout(&CAMPAIGN_SCORE_PAGE, screen_w, screen_h);
        let screen_w = screen_w as i32;
        let screen_h = screen_h as i32;
        let continue_button = page
            .button_rect(CONTINUE_BUTTON)
            .expect("campaign score page lays out Continue");
        let status_help = status_line_rect(STATUS_LINE_DLU, screen_w, screen_h);
        let (origin_x, origin_y) = geom::dialog_origin(screen_w, screen_h);
        let small = screen_w == 640;
        let background = RectPx::new(
            origin_x,
            origin_y,
            if small { 472 } else { 632 },
            if small { 448 } else { 568 },
        );
        let panel = RectPx::new(
            origin_x + if screen_w >= 800 { 80 } else { 0 },
            origin_y,
            472,
            448,
        );
        let art = campaign_score_art_rect(screen_w, screen_h, side);
        let text_shift = if screen_w >= 800 { TEXT_UPPER_SHIFT } else { 0 };
        let controls = std::array::from_fn(|index| {
            let spec = CAMPAIGN_SCORE_CONTROLS[index];
            let rect = match spec.id {
                TITLE_STATIC => page.title,
                STATUS_HELP_STATIC => status_help,
                CONTINUE_BUTTON => continue_button,
                SCORE_ART_REGION_STATIC => art,
                _ => {
                    let template = spec.template_dlu;
                    let rect = child_window(template.x, template.y, template.w, template.h);
                    RectPx::new(rect.x, rect.y - text_shift, rect.w, rect.h)
                }
            };
            CampaignScoreControlRect { id: spec.id, rect }
        });
        Self {
            scale: page.scale,
            screen: page.screen,
            background,
            panel,
            right_panel: page.right_panel,
            lower_strip: page.lower_strip,
            title: page.title,
            status_help,
            continue_button,
            controls,
        }
    }

    /// Uniformly scale the 800x600 campaign score page into the supplied
    /// window. This is the app's fill-window path; it keeps the large art.
    pub fn compute_responsive_layout(screen_w: u32, screen_h: u32) -> Self {
        Self::compute_responsive_layout_for_side(screen_w, screen_h, 0)
    }

    /// Uniformly scale the 800x600 page for one campaign side.
    pub fn compute_responsive_layout_for_side(screen_w: u32, screen_h: u32, side: i32) -> Self {
        let base = Self::compute_layout_for_side(SCORE_PAGE_W as u32, SCORE_PAGE_H as u32, side);
        let scale = ShellScale::from_screen(screen_w as i32, screen_h as i32);
        base.scaled(scale)
    }

    fn scaled(mut self, scale: ShellScale) -> Self {
        self.scale = scale;
        self.screen = RectPx::new(0, 0, scale.window().w, scale.window().h);
        self.title = scale.scale_rect(self.title);
        self.status_help = scale.scale_rect(self.status_help);
        self.continue_button = scale.scale_rect(self.continue_button);
        self.background = scale.scale_rect(self.background);
        self.panel = scale.scale_rect(self.panel);
        self.right_panel = scale.scale_panel(self.right_panel);
        self.lower_strip = scale.scale_rect(self.lower_strip);
        for control in &mut self.controls {
            control.rect = scale.scale_rect(control.rect);
        }
        self
    }

    pub fn control_rect(&self, id: u16) -> Option<RectPx> {
        self.controls
            .iter()
            .find(|control| control.id == id)
            .map(|control| control.rect)
    }
}

/// `0x00608500`'s fixed window for dialog `0x103`'s `0x72B` static. The
/// campaign side selects x 104/103 at width 800+ and 24/23 below it; the
/// 230-pixel Soviet/Yuri art starts 12 pixels higher. `0x0060AF50` applies
/// the rect and keeps the centred dialog origin.
fn campaign_score_art_rect(screen_w: i32, screen_h: i32, side: i32) -> RectPx {
    let (origin_x, origin_y) = geom::dialog_origin(screen_w, screen_h);
    // `0x00608500` tests `DAT_007f5be4` (800), not the 640 asset switch.
    let compact = screen_w < 800;
    let (x, y, height) = match (side == 0, compact) {
        (true, true) => (24, 142, 220),
        (true, false) => (104, 142, 220),
        (false, true) => (23, 130, 230),
        (false, false) => (103, 130, 230),
    };
    RectPx::new(origin_x + x, origin_y + y, 424, height)
}

/// Module-level retail layout entry point for callers which keep page layouts
/// alongside their sibling shell functions.
pub fn compute_layout(screen_w: u32, screen_h: u32) -> CampaignScoreLayout {
    CampaignScoreLayout::compute_layout(screen_w, screen_h)
}

/// Module-level retail layout entry point for one campaign side.
pub fn compute_layout_for_side(screen_w: u32, screen_h: u32, side: i32) -> CampaignScoreLayout {
    CampaignScoreLayout::compute_layout_for_side(screen_w, screen_h, side)
}

/// Module-level responsive layout entry point.
pub fn compute_responsive_layout(screen_w: u32, screen_h: u32) -> CampaignScoreLayout {
    CampaignScoreLayout::compute_responsive_layout(screen_w, screen_h)
}

/// Module-level responsive layout entry point for one campaign side.
pub fn compute_responsive_layout_for_side(
    screen_w: u32,
    screen_h: u32,
    side: i32,
) -> CampaignScoreLayout {
    CampaignScoreLayout::compute_responsive_layout_for_side(screen_w, screen_h, side)
}

fn reveal_params(spec: CampaignScoreRevealSpec) -> Kind1Params {
    Kind1Params {
        interval: Duration::from_millis(spec.interval_ms),
        step: spec.step,
        range: spec.highlight_range,
    }
}

/// A resolved campaign score static with native kind-1 paint/reveal state.
#[derive(Debug, Clone)]
pub struct CampaignScoreStatic {
    pub id: u16,
    reveal_spec: CampaignScoreRevealSpec,
    reveal: PresentedKind1Static,
}

impl CampaignScoreStatic {
    fn new(
        spec: &CampaignScoreControlSpec,
        text: &str,
        reveal_spec: CampaignScoreRevealSpec,
        now: Instant,
    ) -> Self {
        let mut reveal = PresentedKind1Static::new(reveal_params(reveal_spec));
        reveal.set_text(text, now);
        Self {
            id: spec.id,
            reveal_spec,
            reveal,
        }
    }

    pub fn text(&self) -> &str {
        self.reveal.text()
    }

    pub fn reveal_spec(&self) -> CampaignScoreRevealSpec {
        self.reveal_spec
    }

    /// Reveal window for this composition. The app acknowledges a presented
    /// frame through [`CampaignScorePage::commit_presented`].
    pub(crate) fn paint(&mut self, now: Instant) -> Option<Kind1RevealWindow> {
        self.reveal.paint(now)
    }
}

/// The open campaign victory score dialog `0x103`.
#[derive(Debug, Clone)]
pub struct CampaignScorePage {
    pub model: CampaignScoreScreenModel,
    pub texts: CampaignScoreTexts,
    statics: Vec<CampaignScoreStatic>,
    emblem_animation: Option<CampaignScoreEmblemAnimation>,
    /// The native SHOW completion (`0x4EC`) has started reveals and the
    /// kind-4 timer. False until then, including a page whose slide was
    /// unavailable.
    presentation_started: bool,
}

/// The campaign `0x72B` SCRA static's kind-4 timer and frame. The timer is
/// presentation-only and belongs to this page instance; dropping/reopening
/// the page drops/restarts its animation epoch with the art.
#[derive(Debug, Clone)]
struct CampaignScoreEmblemAnimation {
    frame_count: usize,
    frame: usize,
    next_tick_at: Instant,
}

impl CampaignScoreEmblemAnimation {
    fn new(frame_count: usize, now: Instant) -> Option<Self> {
        (frame_count > 0).then_some(Self {
            frame_count,
            frame: 0,
            next_tick_at: now + CAMPAIGN_SCORE_EMBLEM_INTERVAL,
        })
    }

    /// A due native `WM_TIMER` advances exactly one frame; overdue timer
    /// periods coalesce instead of jumping through unpainted frames. On the
    /// SCRA wrap to zero the parent immediately selects frame 29 again.
    fn poll(&mut self, now: Instant) -> usize {
        if now >= self.next_tick_at {
            let next = (self.frame + 1) % self.frame_count;
            self.frame = if next == 0 && self.frame_count > CAMPAIGN_SCORE_EMBLEM_WRAP_FRAME {
                CAMPAIGN_SCORE_EMBLEM_WRAP_FRAME
            } else {
                next
            };
            let periods = now.duration_since(self.next_tick_at).as_nanos()
                / CAMPAIGN_SCORE_EMBLEM_INTERVAL.as_nanos()
                + 1;
            self.next_tick_at +=
                CAMPAIGN_SCORE_EMBLEM_INTERVAL * u32::try_from(periods).unwrap_or(u32::MAX);
        }
        self.frame
    }

    fn next_tick_at(&self) -> Instant {
        self.next_tick_at
    }
}

impl CampaignScorePage {
    /// Install the app-resolved values into the native `0x103` controls.
    pub fn open(model: CampaignScoreScreenModel, texts: CampaignScoreTexts, now: Instant) -> Self {
        let mut statics = Vec::with_capacity(10);
        for spec in CAMPAIGN_SCORE_CONTROLS {
            let Some(reveal) = spec.reveal else {
                continue;
            };
            let text: &str = match spec.id {
                TITLE_STATIC => &texts.title,
                STATUS_HELP_STATIC => "",
                PAR_TIME_LABEL_STATIC => &texts.par_time_label,
                YOUR_TIME_LABEL_STATIC => &texts.your_time_label,
                PAR_TIME_VALUE_STATIC => &model.par_time,
                YOUR_TIME_VALUE_STATIC => &model.your_time,
                RANK_STATIC => &model.rank,
                COMMENDATION_STATIC => &model.commendation,
                ENEMIES_DESTROYED_LABEL_STATIC => &texts.enemies_destroyed_label,
                ENEMIES_DESTROYED_VALUE_STATIC => &model.enemies_destroyed,
                _ => continue,
            };
            statics.push(CampaignScoreStatic::new(&spec, text, reveal, now));
        }
        Self {
            model,
            texts,
            statics,
            emblem_animation: None,
            presentation_started: false,
        }
    }

    /// Native SHOW completion (`0x4EC`): start every kind-1 static and the
    /// kind-4 emblem timer. Idempotent, so a repeated completion or a
    /// wave-less fallback cannot restart a running reveal.
    pub fn start_presentation(&mut self, frame_count: usize, now: Instant) {
        if self.presentation_started {
            return;
        }
        self.presentation_started = true;
        self.start_reveals(now);
        self.start_emblem_animation(frame_count, now);
    }

    pub fn presentation_started(&self) -> bool {
        self.presentation_started
    }

    /// Geometry this page instance uses at the live window size. The retail
    /// path is unscaled; the app's fill-window path scales the 800x600 page.
    pub fn layout(&self, screen_w: u32, screen_h: u32, fills_window: bool) -> CampaignScoreLayout {
        if fills_window {
            CampaignScoreLayout::compute_responsive_layout_for_side(
                screen_w,
                screen_h,
                self.model.side,
            )
        } else {
            CampaignScoreLayout::compute_layout_for_side(screen_w, screen_h, self.model.side)
        }
    }

    /// Start the campaign emblem's native 100 ms kind-4 timer when the SCRA
    /// frames have been loaded for control `0x72B`.
    pub fn start_emblem_animation(&mut self, frame_count: usize, now: Instant) {
        self.emblem_animation = CampaignScoreEmblemAnimation::new(frame_count, now);
    }

    /// Current SCRA frame to draw after applying any due native timer tick.
    /// `None` means the page has no loaded SCRA sequence.
    pub fn poll_emblem_animation(&mut self, now: Instant) -> Option<usize> {
        self.emblem_animation
            .as_mut()
            .map(|animation| animation.poll(now))
    }

    /// Read the last timer-selected SCRA frame without advancing its clock.
    pub fn emblem_animation_frame(&self) -> Option<usize> {
        self.emblem_animation
            .as_ref()
            .map(|animation| animation.frame)
    }

    /// Next native timer deadline while the SCRA kind-4 animation is active.
    pub fn next_emblem_animation_tick(&self) -> Option<Instant> {
        self.emblem_animation
            .as_ref()
            .map(CampaignScoreEmblemAnimation::next_tick_at)
    }

    pub fn static_text(&self, control_id: u16) -> Option<&str> {
        self.statics
            .iter()
            .find(|static_text| static_text.id == control_id)
            .map(CampaignScoreStatic::text)
    }

    pub fn statics(&self) -> &[CampaignScoreStatic] {
        &self.statics
    }

    pub fn statics_mut(&mut self) -> &mut [CampaignScoreStatic] {
        &mut self.statics
    }

    /// Apply the common dialog's hover/help text to static `0x695`.
    pub fn set_status_help(&mut self, text: &str, now: Instant) {
        if let Some(status) = self
            .statics
            .iter_mut()
            .find(|static_text| static_text.id == STATUS_HELP_STATIC)
        {
            status.reveal.set_text(text, now);
        }
    }

    /// The shell's SHOW completion starts each kind-1 static.
    pub fn start_reveals(&mut self, now: Instant) {
        for static_text in &mut self.statics {
            static_text.reveal.start(now);
        }
    }

    /// The latest recomposition was presented by the renderer.
    pub fn commit_presented(&mut self) {
        for static_text in &mut self.statics {
            static_text.reveal.commit_presented();
        }
    }

    pub fn reveals_terminal(&self) -> bool {
        self.statics
            .iter()
            .all(|static_text| static_text.reveal.is_terminal())
    }
}

impl CampaignScoreControlSpec {
    pub fn for_id(id: u16) -> Option<&'static Self> {
        CAMPAIGN_SCORE_CONTROLS
            .iter()
            .find(|control| control.id == id)
    }

    pub fn action_for_id(id: u16) -> Option<CampaignScoreAction> {
        Self::for_id(id).and_then(|control| control.action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(id: u16) -> &'static CampaignScoreControlSpec {
        CampaignScoreControlSpec::for_id(id).unwrap_or_else(|| panic!("0x103/{id:#x}"))
    }

    #[test]
    fn control_contract_matches_campaign_template_and_help_lookup() {
        assert_eq!(CAMPAIGN_SCORE_CONTROLS.len(), 12);
        assert_eq!(
            spec(CONTINUE_BUTTON).template_dlu,
            RectPx::new(425, 346, 108, 23)
        );
        assert_eq!(spec(CONTINUE_BUTTON).caption_key, Some("GUI:Continue"));
        assert_eq!(
            spec(CONTINUE_BUTTON).tooltip_key,
            Some("STT:StandScoreButtonReturn")
        );
        assert_eq!(
            CampaignScoreControlSpec::action_for_id(CONTINUE_BUTTON),
            Some(CampaignScoreAction::Continue)
        );
        assert_eq!(spec(TITLE_STATIC).caption_key, Some("GUI:StandaloneScore"));
        assert_eq!(spec(PAR_TIME_LABEL_STATIC).caption_key, Some("GUI:ParTime"));
        assert_eq!(
            spec(YOUR_TIME_LABEL_STATIC).caption_key,
            Some("GUI:YourTime")
        );
        assert_eq!(
            spec(ENEMIES_DESTROYED_LABEL_STATIC).caption_key,
            Some("GUI:HeadsTaken")
        );
        assert_eq!(
            spec(PAR_TIME_VALUE_STATIC).tooltip_key,
            Some("STT:StandScoreParTime")
        );
        assert_eq!(
            spec(YOUR_TIME_VALUE_STATIC).tooltip_key,
            Some("STT:StandScoreYourTime")
        );
        assert_eq!(spec(RANK_STATIC).tooltip_key, Some("STT:StandScoreTitle"));
        assert_eq!(
            spec(COMMENDATION_STATIC).tooltip_key,
            Some("STT:StandScoreMessage")
        );
        assert_eq!(
            spec(ENEMIES_DESTROYED_VALUE_STATIC).tooltip_key,
            Some("STT:StandScoreKills")
        );
        assert_eq!(spec(SCORE_ART_REGION_STATIC).native_style, 0x5000_0007);
        assert_eq!(spec(SCORE_ART_REGION_STATIC).reveal, None);
    }

    #[test]
    fn campaign_emblem_timer_advances_one_frame_and_native_wrap_selects_29() {
        let opened = Instant::now();
        let mut page = CampaignScorePage::open(
            CampaignScoreScreenModel::default(),
            CampaignScoreTexts::default(),
            opened,
        );
        assert_eq!(page.poll_emblem_animation(opened), None);
        page.start_emblem_animation(0, opened);
        assert_eq!(page.poll_emblem_animation(opened), None);
        assert_eq!(page.next_emblem_animation_tick(), None);

        page.start_emblem_animation(44, opened);
        assert_eq!(page.emblem_animation_frame(), Some(0));
        assert_eq!(
            page.next_emblem_animation_tick(),
            Some(opened + Duration::from_millis(100))
        );
        assert_eq!(
            page.poll_emblem_animation(opened + Duration::from_millis(99)),
            Some(0)
        );
        assert_eq!(
            page.poll_emblem_animation(opened + Duration::from_millis(100)),
            Some(1)
        );

        for tick in 2..=43 {
            let at = opened + Duration::from_millis(tick * 100);
            assert_eq!(page.poll_emblem_animation(at), Some(tick as usize));
        }
        assert_eq!(
            page.poll_emblem_animation(opened + Duration::from_millis(4_400)),
            Some(29)
        );
        assert_eq!(
            page.poll_emblem_animation(opened + Duration::from_millis(4_500)),
            Some(30)
        );

        // A stalled event loop coalesces to one WM_TIMER callback.
        let mut stalled = CampaignScorePage::open(
            CampaignScoreScreenModel::default(),
            CampaignScoreTexts::default(),
            opened,
        );
        stalled.start_emblem_animation(44, opened);
        assert_eq!(
            stalled.poll_emblem_animation(opened + Duration::from_millis(5_000)),
            Some(1)
        );
        assert_eq!(stalled.emblem_animation_frame(), Some(1));
        assert_eq!(
            stalled.next_emblem_animation_tick(),
            Some(opened + Duration::from_millis(5_100))
        );
    }

    #[test]
    fn campaign_emblem_animation_restarts_with_a_new_page_epoch() {
        let opened = Instant::now();
        let mut page = CampaignScorePage::open(
            CampaignScoreScreenModel::default(),
            CampaignScoreTexts::default(),
            opened,
        );
        page.start_emblem_animation(44, opened);
        assert_eq!(
            page.poll_emblem_animation(opened + Duration::from_millis(800)),
            Some(1)
        );

        let reopened = opened + Duration::from_secs(20);
        let mut reopened_page = CampaignScorePage::open(
            CampaignScoreScreenModel::default(),
            CampaignScoreTexts::default(),
            reopened,
        );
        reopened_page.start_emblem_animation(44, reopened);
        assert_eq!(reopened_page.emblem_animation_frame(), Some(0));
        assert_eq!(
            reopened_page.poll_emblem_animation(reopened + Duration::from_millis(99)),
            Some(0)
        );
    }

    #[test]
    fn campaign_emblem_timer_remains_scheduled_after_text_reveals_finish() {
        let opened = Instant::now();
        let mut page = CampaignScorePage::open(
            CampaignScoreScreenModel::default(),
            CampaignScoreTexts::default(),
            opened,
        );
        page.start_emblem_animation(44, opened);
        page.start_reveals(opened);

        for elapsed_ms in (0..10_000).step_by(15) {
            let now = opened + Duration::from_millis(elapsed_ms);
            page.poll_emblem_animation(now);
            for static_text in page.statics_mut() {
                static_text.paint(now);
            }
            page.commit_presented();
            if page.reveals_terminal() {
                assert!(
                    page.next_emblem_animation_tick()
                        .is_some_and(|due| due > now)
                );
                return;
            }
        }
        panic!("campaign score text reveals did not reach their terminal state");
    }

    /// The reveal groups are the executed `0x103` entries of
    /// `tools/storage_oracle/shell_static_timers.py` (kind-1 classifier, timer
    /// interval, step and highlight range).
    #[test]
    fn reveal_groups_match_executed_0x103_static_timers() {
        let fixture: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/shell_static_timers.json",
        ))
        .unwrap();
        for spec in CAMPAIGN_SCORE_CONTROLS {
            let Some(reveal) = spec.reveal else {
                continue;
            };
            let case = fixture["cases"]
                .as_array()
                .unwrap()
                .iter()
                .find(|case| case["dialog_id"] == 0x103 && case["control_id"] == spec.id)
                .unwrap_or_else(|| panic!("no 0x103 timer case for {:#x}", spec.id));
            assert_eq!(case["kind1"], 1, "{:#x}", spec.id);
            assert_eq!(case["interval_ms"], reveal.interval_ms, "{:#x}", spec.id);
            assert_eq!(case["step"], reveal.step, "{:#x}", spec.id);
            assert_eq!(case["range"], reveal.highlight_range, "{:#x}", spec.id);
        }
        // 0x72B is the only kind-4 static; `ShellStatic__AnimationInterval`
        // 0x006033F0 selects 100 ms for dialog 0x103's 0x72B.
        let art_case = fixture["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["dialog_id"] == 0x103 && case["control_id"] == 0x72B)
            .expect("0x103 0x72B timer case");
        assert_eq!(art_case["kind1"], 0);
        assert_eq!(art_case["kind4_interval_ms"], 100);
        assert_eq!(spec(SCORE_ART_REGION_STATIC).reveal, None);
        assert_eq!(CAMPAIGN_SCORE_EMBLEM_INTERVAL, Duration::from_millis(100));
    }

    #[test]
    fn presentation_start_is_single_shot_and_arms_reveals_and_timer() {
        let opened = Instant::now();
        let mut page = CampaignScorePage::open(
            CampaignScoreScreenModel::default(),
            CampaignScoreTexts::default(),
            opened,
        );
        assert!(!page.presentation_started());
        assert_eq!(page.emblem_animation_frame(), None);
        page.start_presentation(44, opened);
        assert!(page.presentation_started());
        assert_eq!(page.emblem_animation_frame(), Some(0));
        let deadline = page.next_emblem_animation_tick();
        assert_eq!(deadline, Some(opened + Duration::from_millis(100)));
        // A repeated SHOW completion must not restart the epoch.
        page.start_presentation(44, opened + Duration::from_secs(5));
        assert_eq!(page.next_emblem_animation_tick(), deadline);
    }

    #[test]
    fn layout_uses_executed_campaign_relayout_and_native_art_window() {
        let layout = compute_layout(800, 600);
        let shared = menu_page::compute_layout(&CAMPAIGN_SCORE_PAGE, 800, 600);
        assert_eq!(layout.title, shared.title);
        assert_eq!(layout.title, RectPx::new(635, 9, 163, 18));
        assert_eq!(layout.status_help, RectPx::new(10, 578, 456, 21));
        assert_eq!(
            layout.continue_button,
            shared.button_rect(CONTINUE_BUTTON).unwrap()
        );
        assert_eq!(layout.continue_button, RectPx::new(644, 535, 156, 42));
        assert_eq!(
            layout.control_rect(PAR_TIME_LABEL_STATIC),
            Some(RectPx::new(188, 382, 84, 17))
        );
        assert_eq!(
            layout.control_rect(PAR_TIME_VALUE_STATIC),
            Some(RectPx::new(188, 400, 84, 17))
        );
        assert_eq!(
            layout.control_rect(RANK_STATIC),
            Some(RectPx::new(128, 7, 387, 17))
        );
        assert_eq!(
            layout.control_rect(COMMENDATION_STATIC),
            Some(RectPx::new(128, 31, 387, 66))
        );
        assert_eq!(
            layout.control_rect(SCORE_ART_REGION_STATIC),
            Some(RectPx::new(104, 142, 424, 220))
        );
        assert_eq!(layout.background, RectPx::new(0, 0, 632, 568));
        assert_eq!(layout.panel, RectPx::new(80, 0, 472, 448));
        assert_eq!(layout.control_rect(0xFFFF), None);
    }

    /// Every `0x103` child window is the executed WM_INITDIALOG relayout
    /// (`tools/storage_oracle/shell_relayout.py`), for both campaign sides.
    #[test]
    fn control_windows_match_executed_0x103_relayout_at_each_retail_size() {
        use crate::ui::shell::layout::tests::executed_child_window_for_side as executed;
        for (width, height) in [(640, 480), (800, 600), (1024, 768)] {
            for side in [0, 1] {
                let layout = compute_layout_for_side(width, height, side);
                for spec in CAMPAIGN_SCORE_CONTROLS {
                    assert_eq!(
                        layout.control_rect(spec.id),
                        Some(executed(0x103, spec.id, width as i32, height as i32, side)),
                        "{width}x{height} side {side} control {:#x}",
                        spec.id
                    );
                }
            }
        }
    }

    /// `SCRBK`/`SCRT` and the `0x72B` override follow `0x006C8E80`,
    /// `0x00608500` and `0x0060727D` at the two retail shell sizes.
    #[test]
    fn art_windows_follow_the_native_score_composition() {
        for (width, height, background, panel, art) in [
            (
                640,
                480,
                RectPx::new(0, 0, 472, 448),
                RectPx::new(0, 0, 472, 448),
                RectPx::new(24, 142, 424, 220),
            ),
            (
                800,
                600,
                RectPx::new(0, 0, 632, 568),
                RectPx::new(80, 0, 472, 448),
                RectPx::new(104, 142, 424, 220),
            ),
        ] {
            let layout = compute_layout_for_side(width, height, 0);
            assert_eq!(layout.background, background, "{width}");
            assert_eq!(layout.panel, panel, "{width}");
            assert_eq!(
                layout.control_rect(SCORE_ART_REGION_STATIC),
                Some(art),
                "{width}"
            );
        }
        assert_eq!(
            compute_layout_for_side(800, 600, 1).control_rect(SCORE_ART_REGION_STATIC),
            Some(RectPx::new(103, 130, 424, 230))
        );
        assert_eq!(
            compute_layout_for_side(640, 480, 2).control_rect(SCORE_ART_REGION_STATIC),
            Some(RectPx::new(23, 130, 424, 230))
        );
        // 0x00608500 switches at 800, not at the 640-wide asset branch.
        for (width, x) in [(641, 24), (700, 24), (799, 24), (800, 104)] {
            assert_eq!(
                compute_layout_for_side(width, 600, 0)
                    .control_rect(SCORE_ART_REGION_STATIC)
                    .map(|rect| rect.x),
                Some(x),
                "{width}"
            );
        }
    }

    #[test]
    fn responsive_layout_scales_the_800x600_page() {
        let base = compute_layout_for_side(800, 600, 0);
        let same = compute_responsive_layout_for_side(800, 600, 0);
        assert_eq!(same, base);

        let large = compute_responsive_layout(1600, 1200);
        assert_eq!(
            large.control_rect(PAR_TIME_LABEL_STATIC),
            Some(RectPx::new(376, 764, 168, 34))
        );
        assert_eq!(large.background, RectPx::new(0, 0, 1264, 1136));
        assert_eq!(large.panel, RectPx::new(160, 0, 944, 896));
        let shared = menu_page::compute_responsive_layout(&CAMPAIGN_SCORE_PAGE, 1600, 1200);
        assert_eq!(large.title, shared.title);
        assert_eq!(large.status_help, shared.status_help);
        assert_eq!(
            large.continue_button,
            shared.button_rect(CONTINUE_BUTTON).unwrap()
        );
    }

    #[test]
    fn page_binds_only_resolved_values_and_uses_native_reveal_groups() {
        let model = CampaignScoreScreenModel {
            side: 0,
            par_time: "00:10:00".into(),
            your_time: "00:08:31".into(),
            enemies_destroyed: "17".into(),
            rank: "Major".into(),
            commendation: "Mission commendation".into(),
        };
        let texts = CampaignScoreTexts {
            title: "Mission Score".into(),
            continue_label: "Continue".into(),
            continue_status: "Continue to the next scenario.".into(),
            par_time_label: "Par Time".into(),
            your_time_label: "Your Time".into(),
            enemies_destroyed_label: "Enemies Destroyed".into(),
        };
        let now = Instant::now();
        let mut page = CampaignScorePage::open(model, texts, now);

        assert_eq!(page.static_text(TITLE_STATIC), Some("Mission Score"));
        assert_eq!(page.static_text(PAR_TIME_VALUE_STATIC), Some("00:10:00"));
        assert_eq!(page.static_text(YOUR_TIME_VALUE_STATIC), Some("00:08:31"));
        assert_eq!(page.static_text(RANK_STATIC), Some("Major"));
        assert_eq!(
            page.static_text(COMMENDATION_STATIC),
            Some("Mission commendation")
        );
        assert_eq!(page.static_text(ENEMIES_DESTROYED_VALUE_STATIC), Some("17"));
        assert_eq!(page.static_text(STATUS_HELP_STATIC), Some(""));
        assert_eq!(page.statics().len(), 10);
        assert_eq!(
            spec(COMMENDATION_STATIC).reveal,
            Some(CampaignScoreRevealSpec {
                interval_ms: 15,
                step: 3,
                highlight_range: 16,
            })
        );
        assert_eq!(spec(RANK_STATIC).reveal, Some(TEXT_REVEAL));

        page.set_status_help(
            "Continue to the next scenario.",
            now + Duration::from_millis(1),
        );
        assert_eq!(
            page.static_text(STATUS_HELP_STATIC),
            Some("Continue to the next scenario.")
        );
        page.start_reveals(now);
        assert!(
            page.statics_mut()
                .iter_mut()
                .all(|static_text| static_text.paint(now).is_some())
        );
        assert!(!page.reveals_terminal());
        page.commit_presented();
    }
}

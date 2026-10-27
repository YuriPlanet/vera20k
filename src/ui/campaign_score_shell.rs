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
//!
//! The same module drives the LAN Cooperative campaign-end dialog `0xBC7`
//! (`CampaignEndScoreClass vt+4` `0x00471420`, proc `0x00470D30`): it loads
//! the same side art, slides the same way and animates `0x72B` on the same
//! timer, with its own controls, monitor `0x71C` and score cells. Its
//! template, kind-1 groups, help keys and relayout were executed the same
//! way; its body statics keep their template windows at every retail size.

use crate::util::time::{Duration, Instant};

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

/// Retail `gamemd.exe`:
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
    /// A static the dialog fills once and paints whole (no kind-1 reveal),
    /// such as `0xBC7`'s score cells.
    Text,
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

/// The LAN Cooperative campaign-end dialog `0xBC7`
/// (`CampaignEndScoreClass vt+4` `0x00471420`, proc `0x00470D30`).
pub const CAMPAIGN_END_DIALOG: DialogId = DialogId(0x0BC7);
pub const CAMPAIGN_END_CONTINUE: u16 = 0x06D1;
pub const CAMPAIGN_END_NAME: u16 = 0x0733;
pub const CAMPAIGN_END_NUMBER: u16 = 0x0735;
pub const CAMPAIGN_END_DIFFICULTY: u16 = 0x0732;
pub const CAMPAIGN_END_TIME: u16 = 0x03EA;
/// First place's name and score, then second place's (`0x00470F73..`).
pub const CAMPAIGN_END_PLACES: [(u16, u16); 2] = [(0x073A, 0x073B), (0x0739, 0x0738)];
/// The right-panel warning monitor static.
pub const CAMPAIGN_END_MONITOR: u16 = 0x071C;

pub const CAMPAIGN_END_PAGE: MenuPageSpec = MenuPageSpec {
    dialog: CAMPAIGN_END_DIALOG,
    title_key: "GUI:CampaignResults",
    stacked: &[],
    back: MenuPageButtonSpec {
        id: CAMPAIGN_END_CONTINUE,
        dlu_top: 346,
        csf_key: "GUI:Continue",
        tooltip_key: "STT:MPScoreButtonContinue",
        result: None,
    },
};

/// `0xBC7`'s kind-1 groups, executed from the same getters as `0x103`'s.
const PLACE_LABEL_REVEAL: CampaignScoreRevealSpec = CampaignScoreRevealSpec {
    interval_ms: 15,
    step: 1,
    highlight_range: 8,
};
const PLACE_REVEAL: CampaignScoreRevealSpec = CampaignScoreRevealSpec {
    interval_ms: 60,
    step: 1,
    highlight_range: 32,
};
const HEADER_REVEAL: CampaignScoreRevealSpec = CampaignScoreRevealSpec {
    interval_ms: 30,
    step: 1,
    highlight_range: 16,
};

const fn end_control(
    id: u16,
    kind: CampaignScoreControlKind,
    template_dlu: RectPx,
    native_style: u32,
    caption_key: Option<&'static str>,
    tooltip_key: Option<&'static str>,
    reveal: Option<CampaignScoreRevealSpec>,
) -> CampaignScoreControlSpec {
    CampaignScoreControlSpec {
        id,
        kind,
        template_dlu,
        native_style,
        caption_key,
        tooltip_key,
        action: if id == CAMPAIGN_END_CONTINUE {
            Some(CampaignScoreAction::Continue)
        } else {
            None
        },
        reveal,
    }
}

/// Retail `RT_DIALOG 0xBC7` in template order. The score cells
/// `0x781..0x78A` share `0xBC6`'s table `0x0081B1EC` and are not kind-1.
#[rustfmt::skip]
pub const CAMPAIGN_END_CONTROLS: [CampaignScoreControlSpec; 30] = {
    use CampaignScoreControlKind::{Button, Static, Text};
    const BLANK: Option<&str> = Some("GUI:Blank");
    [
        end_control(0x06D1, Button, RectPx::new(425, 130, 108, 23), 0x5000_000B, Some("GUI:Continue"), Some("STT:MPScoreButtonContinue"), None),
        end_control(0x0736, Static, RectPx::new(113, 232, 74, 10), 0x5000_0001, Some("GUI:FirstPlace"), None, Some(PLACE_LABEL_REVEAL)),
        end_control(0x0737, Static, RectPx::new(233, 232, 68, 10), 0x5000_0001, Some("GUI:SecondPlace"), None, Some(PLACE_LABEL_REVEAL)),
        end_control(0x073A, Static, RectPx::new(123, 242, 55, 10), 0x5000_0001, BLANK, Some("STT:MPScoreLabelPlayer"), Some(PLACE_REVEAL)),
        end_control(0x0739, Static, RectPx::new(240, 242, 55, 10), 0x5000_0001, BLANK, Some("STT:MPScoreLabelPlayer"), Some(PLACE_REVEAL)),
        end_control(0x0733, Static, RectPx::new(83, 4, 257, 10), 0x5000_0001, BLANK, Some("STT:MPScoreLabelCampaignName"), Some(TEXT_REVEAL)),
        end_control(0x0735, Static, RectPx::new(83, 19, 64, 10), 0x5000_0000, BLANK, Some("STT:MPCampEndCampaignNumber"), Some(TEXT_REVEAL)),
        end_control(STATUS_HELP_STATIC, Static, RectPx::new(10, 356, 303, 12), 0x5000_0200, BLANK, None, Some(LONG_TEXT_REVEAL)),
        end_control(SCORE_ART_REGION_STATIC, Static, RectPx::new(70, 84, 282, 141), 0x5000_0007, None, None, None),
        end_control(TITLE_STATIC, Static, RectPx::new(425, 1, 108, 10), 0x5002_0001, Some("GUI:CampaignResults"), None, Some(TEXT_REVEAL)),
        end_control(0x0732, Static, RectPx::new(167, 19, 87, 10), 0x5000_0001, BLANK, Some("STT:MPScoreLabelDifficulty"), Some(TEXT_REVEAL)),
        end_control(0x03EA, Static, RectPx::new(257, 19, 80, 10), 0x5000_0002, BLANK, Some("STT:MPCampEndTime"), Some(TEXT_REVEAL)),
        end_control(0x0738, Static, RectPx::new(240, 254, 55, 10), 0x5000_0001, BLANK, Some("STT:MPScoreLabelScore"), Some(PLACE_REVEAL)),
        end_control(0x073B, Static, RectPx::new(123, 254, 55, 10), 0x5000_0001, BLANK, Some("STT:MPScoreLabelScore"), Some(PLACE_REVEAL)),
        end_control(0x073C, Static, RectPx::new(83, 31, 49, 10), 0x5000_0000, Some("GUI:Player"), None, Some(HEADER_REVEAL)),
        end_control(0x073D, Static, RectPx::new(149, 31, 41, 10), 0x5000_0002, Some("GUI:Kills"), None, Some(HEADER_REVEAL)),
        end_control(0x073E, Static, RectPx::new(202, 31, 41, 10), 0x5000_0002, Some("GUI:Losses"), None, Some(HEADER_REVEAL)),
        end_control(0x073F, Static, RectPx::new(247, 31, 41, 10), 0x5000_0002, Some("GUI:Built"), None, Some(HEADER_REVEAL)),
        end_control(0x0740, Static, RectPx::new(294, 31, 41, 10), 0x5000_0002, Some("GUI:Score"), None, Some(HEADER_REVEAL)),
        end_control(0x0781, Text, RectPx::new(83, 40, 68, 10), 0x5000_0000, BLANK, None, None),
        end_control(0x0782, Text, RectPx::new(83, 51, 68, 10), 0x5000_0000, BLANK, None, None),
        end_control(0x0783, Text, RectPx::new(149, 40, 37, 10), 0x5000_0002, BLANK, None, None),
        end_control(0x0784, Text, RectPx::new(149, 51, 37, 10), 0x5000_0002, BLANK, None, None),
        end_control(0x0785, Text, RectPx::new(197, 40, 39, 10), 0x5000_0002, BLANK, None, None),
        end_control(0x0786, Text, RectPx::new(197, 51, 39, 10), 0x5000_0002, BLANK, None, None),
        end_control(0x0787, Text, RectPx::new(252, 40, 34, 10), 0x5000_0002, BLANK, None, None),
        end_control(0x0788, Text, RectPx::new(252, 51, 34, 10), 0x5000_0002, BLANK, None, None),
        end_control(0x0789, Text, RectPx::new(293, 40, 41, 8), 0x5000_0002, BLANK, None, None),
        end_control(0x078A, Text, RectPx::new(293, 51, 41, 8), 0x5000_0002, BLANK, None, None),
        end_control(CAMPAIGN_END_MONITOR, Static, RectPx::new(449, 29, 61, 33), 0x5000_0007, None, None, None),
    ]
};

/// The dialogs this module drives. Both load the side's SCRBK/SCRT/SCRA
/// (`0x0072D300`), slide with SCRT frames and animate `0x72B` every
/// 100 ms, and both handle `0x72B`'s frame-zero `0x4D8` by selecting frame
/// 29 (`0x006C93C9`, `0x00470E8E`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CampaignScoreDialog {
    /// Campaign victory StandaloneScore `0x103`.
    #[default]
    Victory,
    /// The LAN co-op campaign end `0xBC7`.
    CampaignEnd,
}

impl CampaignScoreDialog {
    pub const fn id(self) -> DialogId {
        self.page().dialog
    }

    pub const fn page(self) -> &'static MenuPageSpec {
        match self {
            Self::Victory => &CAMPAIGN_SCORE_PAGE,
            Self::CampaignEnd => &CAMPAIGN_END_PAGE,
        }
    }

    pub fn controls(self) -> &'static [CampaignScoreControlSpec] {
        match self {
            Self::Victory => &CAMPAIGN_SCORE_CONTROLS,
            Self::CampaignEnd => &CAMPAIGN_END_CONTROLS,
        }
    }

    pub const fn continue_button(self) -> u16 {
        self.page().back.id
    }

    pub fn control(self, id: u16) -> Option<&'static CampaignScoreControlSpec> {
        self.controls().iter().find(|control| control.id == id)
    }

    /// `0x0060B950` moves `0x103`'s text statics up from 800 pixels wide;
    /// `0xBC7`'s keep their template windows at every retail size.
    const fn text_shift(self, screen_w: i32) -> i32 {
        match self {
            Self::Victory if screen_w >= 800 => TEXT_UPPER_SHIFT,
            _ => 0,
        }
    }
}

/// Already-resolved text from the campaign victory path. The app owns all
/// value formatting and sends these strings to the page unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CampaignScoreScreenModel {
    /// Campaign side index (`Scenario+0x34B8`): 0 allies, 1 Soviet, 2 Yuri.
    /// It selects the `0x72B` art window's 220- or 230-pixel height, the
    /// `0x00608500` x by one pixel, and the small/large asset tables.
    pub side: i32,
    /// Whether the elapsed time exceeded the par time at `0x6C9231`, the same
    /// comparison that selects the over-par rank strings and that
    /// `ScoreDialog_Handle_ScoreThemeA` uses to pick the theme.
    pub over_par: bool,
    /// The theme dialog `0x103` starts on its `WM_INIT`: the side's
    /// `CampaignScore.UnderParTheme` / `.OverParTheme` under Ares, `None` for
    /// gamemd's literal `SCORE`.
    pub theme: Option<String>,
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

/// The texts `0xBC7`'s init (`0x00470D30`, `WM_INITDIALOG`) writes, resolved
/// by the app: the campaign name `0x733`, `GUI:CampaignLabel` over the
/// campaign's number `0x735`, `GUI:DifficultyString` `0x732`, the time
/// `0x3EA`, and the two partners' rows as `0x00471420` sorted them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CampaignEndModel {
    /// Scenario `+0x34B8`, which picks the score art (`0x0072D300`).
    pub side: i32,
    pub campaign_name: String,
    pub campaign_number: String,
    pub difficulty: String,
    pub time: String,
    pub rows: Vec<crate::ui::score_shell::ScoreRow>,
}

/// One of `0xBC7`'s plain score cells (`0x781..0x78A`): its text and the
/// row colour `0x498` set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CampaignScoreCell {
    pub id: u16,
    pub text: String,
    pub rgb: [u8; 3],
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
    pub warning_monitor: RectPx,
    pub controls: Vec<CampaignScoreControlRect>,
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
        Self::compute_dialog_layout(CampaignScoreDialog::Victory, screen_w, screen_h, side)
    }

    /// Retail window placement of either dialog for one campaign side.
    pub fn compute_dialog_layout(
        dialog: CampaignScoreDialog,
        screen_w: u32,
        screen_h: u32,
        side: i32,
    ) -> Self {
        let page = menu_page::compute_layout(dialog.page(), screen_w, screen_h);
        let screen_w = screen_w as i32;
        let screen_h = screen_h as i32;
        let continue_button = page
            .button_rect(dialog.continue_button())
            .expect("campaign score page lays out Continue");
        let status_dlu = dialog
            .control(STATUS_HELP_STATIC)
            .map_or(STATUS_LINE_DLU, |spec| spec.template_dlu);
        let status_help = status_line_rect(status_dlu, screen_w, screen_h);
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
        let text_shift = dialog.text_shift(screen_w);
        let continue_id = dialog.continue_button();
        let controls = dialog
            .controls()
            .iter()
            .map(|spec| {
                let rect = match spec.id {
                    TITLE_STATIC => page.title,
                    STATUS_HELP_STATIC => status_help,
                    id if id == continue_id => continue_button,
                    SCORE_ART_REGION_STATIC => art,
                    CAMPAIGN_END_MONITOR => page.warning_monitor,
                    _ => {
                        let template = spec.template_dlu;
                        let rect = child_window(template.x, template.y, template.w, template.h);
                        RectPx::new(rect.x, rect.y - text_shift, rect.w, rect.h)
                    }
                };
                CampaignScoreControlRect { id: spec.id, rect }
            })
            .collect();
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
            warning_monitor: page.warning_monitor,
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
        Self::compute_responsive_dialog_layout(
            CampaignScoreDialog::Victory,
            screen_w,
            screen_h,
            side,
        )
    }

    /// Uniformly scale either dialog's 800x600 page for one campaign side.
    pub fn compute_responsive_dialog_layout(
        dialog: CampaignScoreDialog,
        screen_w: u32,
        screen_h: u32,
        side: i32,
    ) -> Self {
        let base =
            Self::compute_dialog_layout(dialog, SCORE_PAGE_W as u32, SCORE_PAGE_H as u32, side);
        let scale = ShellScale::from_screen(screen_w as i32, screen_h as i32);
        base.scaled(scale)
    }

    fn scaled(mut self, scale: ShellScale) -> Self {
        self.scale = scale;
        self.screen = RectPx::new(0, 0, scale.window().w, scale.window().h);
        self.title = scale.scale_rect(self.title);
        self.status_help = scale.scale_rect(self.status_help);
        self.continue_button = scale.scale_rect(self.continue_button);
        self.warning_monitor = scale.scale_rect(self.warning_monitor);
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
    /// The colour a `0x498` set, else the shell's yellow.
    pub rgb: Option<[u8; 3]>,
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
            rgb: None,
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
    pub dialog: CampaignScoreDialog,
    pub model: CampaignScoreScreenModel,
    pub texts: CampaignScoreTexts,
    statics: Vec<CampaignScoreStatic>,
    cells: Vec<CampaignScoreCell>,
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
            dialog: CampaignScoreDialog::Victory,
            model,
            texts,
            statics,
            cells: Vec::new(),
            emblem_animation: None,
            presentation_started: false,
        }
    }

    /// `0xBC7`'s `WM_INITDIALOG` (`0x00470D30`): the campaign texts, then
    /// per row its five cells (table `0x0081B1EC`) and, for the first two
    /// rows, the place's name and score, each in the row's colour. Statics
    /// it does not write keep their template captions, resolved by
    /// `caption`.
    pub fn open_campaign_end(
        model: CampaignEndModel,
        caption: impl Fn(&'static str) -> String,
        now: Instant,
    ) -> Self {
        let dialog = CampaignScoreDialog::CampaignEnd;
        let mut statics = Vec::new();
        for spec in dialog.controls() {
            let Some(reveal) = spec.reveal else {
                continue;
            };
            let place =
                CAMPAIGN_END_PLACES
                    .iter()
                    .zip(&model.rows)
                    .find_map(|(&(name, score), row)| {
                        (spec.id == name)
                            .then(|| row.name.clone())
                            .or_else(|| (spec.id == score).then(|| row.score.to_string()))
                            .map(|text| (text, row.rgb))
                    });
            let (text, rgb) = match (spec.id, place) {
                (_, Some((text, rgb))) => (text, Some(rgb)),
                (STATUS_HELP_STATIC, _) => (String::new(), None),
                (CAMPAIGN_END_NAME, _) => (model.campaign_name.clone(), None),
                (CAMPAIGN_END_NUMBER, _) => (model.campaign_number.clone(), None),
                (CAMPAIGN_END_DIFFICULTY, _) => (model.difficulty.clone(), None),
                (CAMPAIGN_END_TIME, _) => (model.time.clone(), None),
                _ => (spec.caption_key.map(&caption).unwrap_or_default(), None),
            };
            let mut static_text = CampaignScoreStatic::new(spec, &text, reveal, now);
            static_text.rgb = rgb;
            statics.push(static_text);
        }
        let mut cells = Vec::new();
        for (row, ids) in model
            .rows
            .iter()
            .zip(crate::ui::coop_score_shell::ROW_CELLS)
        {
            let values = [
                row.name.clone(),
                row.kills.to_string(),
                row.losses.to_string(),
                row.built.to_string(),
                row.score.to_string(),
            ];
            for (id, text) in ids.into_iter().zip(values) {
                cells.push(CampaignScoreCell {
                    id,
                    text,
                    rgb: row.rgb,
                });
            }
        }
        Self {
            dialog,
            model: CampaignScoreScreenModel {
                side: model.side,
                ..Default::default()
            },
            texts: CampaignScoreTexts::default(),
            statics,
            cells,
            emblem_animation: None,
            presentation_started: false,
        }
    }

    /// `0xBC7`'s score cells; empty on `0x103`.
    pub fn cells(&self) -> &[CampaignScoreCell] {
        &self.cells
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
            CampaignScoreLayout::compute_responsive_dialog_layout(
                self.dialog,
                screen_w,
                screen_h,
                self.model.side,
            )
        } else {
            CampaignScoreLayout::compute_dialog_layout(
                self.dialog,
                screen_w,
                screen_h,
                self.model.side,
            )
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

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(id: u16) -> &'static CampaignScoreControlSpec {
        CampaignScoreDialog::Victory
            .control(id)
            .unwrap_or_else(|| panic!("0x103/{id:#x}"))
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
    fn page_binds_only_resolved_values_and_uses_native_reveal_groups() {
        let model = CampaignScoreScreenModel {
            side: 0,
            par_time: "00:10:00".into(),
            your_time: "00:08:31".into(),
            enemies_destroyed: "17".into(),
            rank: "Major".into(),
            commendation: "Mission commendation".into(),
            ..Default::default()
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

    /// `0xBC7`'s executed WM_INITDIALOG relayout (side 0): the right-panel
    /// anchors and the `0x72B` override move with the screen while the body
    /// statics keep their template windows.
    #[test]
    fn campaign_end_windows_match_the_executed_0xbc7_relayout() {
        let cases: [(u32, u32, [(u16, [i32; 4]); 6]); 2] = [
            (
                800,
                600,
                [
                    (CAMPAIGN_END_CONTINUE, [644, 535, 156, 42]),
                    (STATUS_HELP_STATIC, [10, 578, 456, 21]),
                    (SCORE_ART_REGION_STATIC, [104, 142, 424, 220]),
                    (TITLE_STATIC, [635, 9, 163, 18]),
                    (CAMPAIGN_END_MONITOR, [670, 47, 93, 55]),
                    (0x0789, [440, 65, 63, 14]),
                ],
            ),
            (
                1024,
                768,
                [
                    (CAMPAIGN_END_CONTINUE, [756, 619, 156, 42]),
                    (STATUS_HELP_STATIC, [122, 662, 456, 21]),
                    (SCORE_ART_REGION_STATIC, [216, 226, 424, 220]),
                    (TITLE_STATIC, [747, 93, 163, 18]),
                    (CAMPAIGN_END_MONITOR, [782, 131, 93, 55]),
                    (0x0733, [125, 7, 387, 17]),
                ],
            ),
        ];
        for (width, height, expected) in cases {
            let layout = CampaignScoreLayout::compute_dialog_layout(
                CampaignScoreDialog::CampaignEnd,
                width,
                height,
                0,
            );
            for (id, [x, y, w, h]) in expected {
                assert_eq!(
                    layout.control_rect(id),
                    Some(RectPx::new(x, y, w, h)),
                    "{width}x{height} {id:#x}"
                );
            }
        }
    }

    /// `0x00470D30`'s init: each row's cells in its colour, the first two
    /// rows' places, and the template captions elsewhere.
    #[test]
    fn campaign_end_binds_rows_to_cells_and_places() {
        let row = |name: &str, rgb, score| crate::ui::score_shell::ScoreRow {
            name: name.into(),
            rgb,
            kills: 7,
            losses: 2,
            built: 30,
            score,
        };
        let model = CampaignEndModel {
            side: 1,
            campaign_name: "Soviet Campaign".into(),
            campaign_number: "Campaign 2".into(),
            difficulty: "Difficulty: Medium".into(),
            time: "01:02:03".into(),
            rows: vec![
                row("Host", [10, 20, 30], 900),
                row("Guest", [40, 50, 60], 400),
            ],
        };
        let page =
            CampaignScorePage::open_campaign_end(model, |key| key.to_owned(), Instant::now());
        assert_eq!(page.dialog, CampaignScoreDialog::CampaignEnd);
        assert_eq!(page.static_text(CAMPAIGN_END_NAME), Some("Soviet Campaign"));
        assert_eq!(page.static_text(CAMPAIGN_END_TIME), Some("01:02:03"));
        assert_eq!(page.static_text(0x0736), Some("GUI:FirstPlace"));
        assert_eq!(page.static_text(TITLE_STATIC), Some("GUI:CampaignResults"));
        let place = |id| {
            page.statics()
                .iter()
                .find(|text| text.id == id)
                .map(|text| (text.text().to_owned(), text.rgb))
        };
        assert_eq!(place(0x073A), Some(("Host".into(), Some([10, 20, 30]))));
        assert_eq!(place(0x073B), Some(("900".into(), Some([10, 20, 30]))));
        assert_eq!(place(0x0739), Some(("Guest".into(), Some([40, 50, 60]))));
        assert_eq!(place(0x0738), Some(("400".into(), Some([40, 50, 60]))));
        let cells: Vec<_> = page
            .cells()
            .iter()
            .map(|cell| (cell.id, cell.text.as_str(), cell.rgb))
            .collect();
        assert_eq!(
            cells[..5],
            [
                (0x0781, "Host", [10, 20, 30]),
                (0x0783, "7", [10, 20, 30]),
                (0x0785, "2", [10, 20, 30]),
                (0x0787, "30", [10, 20, 30]),
                (0x0789, "900", [10, 20, 30]),
            ]
        );
        assert_eq!(cells[5], (0x0782, "Guest", [40, 50, 60]));
    }
}

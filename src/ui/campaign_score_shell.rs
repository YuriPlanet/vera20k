//! Campaign-victory score page: the reachable retail dialog `0x103`.
//!
//! Campaign victory reaches `0x6C8E80` from `0x685960`; that body selects
//! dialog `0x103` and the dialog proc `0x6C8FF0`. Its `RT_DIALOG` template is
//! the campaign score page, not the skirmish results dialog `0x108`. The
//! control ids, captions and style bits below come from retail `gamemd.exe`'s
//! `RT_DIALOG 0x103`; the templates are `shell_layout.ini`'s `[0x103]`. The
//! tooltip keys and kind-1 reveal parameters were read by executing the
//! existing `shell_help_keys.help_label` and `shell_static_timers.run_getter`
//! helpers against that retail executable. The control windows are the
//! executed WM_INITDIALOG relayout (`shell_relayout.py`): at 800x600 the text
//! statics move 50 pixels up and `0x72B` takes the `0x00608500` art override.
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

use crate::ui::score_shell::{ScoreRow, ScoreStatic, ScoreStatics};
use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::geom::ShellAlign;
use crate::ui::shell::geom::{self, RectPx, fit_window};
use crate::ui::shell::layout::RectTable;
use crate::ui::shell::layout_data;
use crate::ui::shell::menu_page::{self, MenuPageButtonSpec, MenuPageLayout, MenuPageSpec};
use crate::ui::shell::static_reveal::{HEADING_KIND1, Kind1Params, STATUS_LINE_KIND1};

pub const CAMPAIGN_SCORE_DIALOG: DialogResourceId = DialogResourceId(0x0103);

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
    back: MenuPageButtonSpec::new(
        CONTINUE_BUTTON,
        346,
        "GUI:Continue",
        "STT:StandScoreButtonReturn",
    ),
};

/// One kind-1 static or the Continue button of a template: its id, alignment
/// (the low two bits of its Win32 style), template caption, status help and
/// kind-1 reveal (`None` for Continue).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CampaignScoreControlSpec {
    pub id: u16,
    pub align: ShellAlign,
    pub caption_key: Option<&'static str>,
    pub tooltip_key: Option<&'static str>,
    pub reveal: Option<Kind1Params>,
}

/// Executed kind-1 timing from `0x602490`, `0x600CA0`, `0x6015E0` and
/// `0x601D20`: the short texts take the heading's, the commendation and the
/// status line the status line's.
const TEXT_REVEAL: Option<Kind1Params> = Some(HEADING_KIND1);
const LONG_TEXT_REVEAL: Option<Kind1Params> = Some(STATUS_LINE_KIND1);

const fn kind1(interval_ms: u64, range: u32) -> Option<Kind1Params> {
    Some(Kind1Params {
        interval: Duration::from_millis(interval_ms),
        step: 1,
        range,
    })
}

const fn control(
    id: u16,
    align: ShellAlign,
    caption_key: Option<&'static str>,
    tooltip_key: Option<&'static str>,
    reveal: Option<Kind1Params>,
) -> CampaignScoreControlSpec {
    CampaignScoreControlSpec {
        id,
        align,
        caption_key,
        tooltip_key,
        reveal,
    }
}

const L: ShellAlign = ShellAlign::NONE;
const C: ShellAlign = ShellAlign::H_CENTER;
const R: ShellAlign = ShellAlign::H_RIGHT;
const BLANK: Option<&str> = Some("GUI:Blank");

/// `0x103`'s Continue and kind-1 statics in template order. `0x72B` is the
/// kind-4 static whose client rect `0x00608500` sizes to the current SCRA
/// frame; its 100 ms interval is `ShellStatic__AnimationInterval`
/// `0x006033F0`'s `0x103`/`0x72B` branch.
#[rustfmt::skip]
pub const CAMPAIGN_SCORE_CONTROLS: [CampaignScoreControlSpec; 11] = [
    control(CONTINUE_BUTTON, L, Some("GUI:Continue"), Some("STT:StandScoreButtonReturn"), None),
    control(PAR_TIME_LABEL_STATIC, C, Some("GUI:ParTime"), None, TEXT_REVEAL),
    control(YOUR_TIME_LABEL_STATIC, C, Some("GUI:YourTime"), None, TEXT_REVEAL),
    control(PAR_TIME_VALUE_STATIC, C, BLANK, Some("STT:StandScoreParTime"), TEXT_REVEAL),
    control(YOUR_TIME_VALUE_STATIC, C, BLANK, Some("STT:StandScoreYourTime"), TEXT_REVEAL),
    control(RANK_STATIC, L, BLANK, Some("STT:StandScoreTitle"), TEXT_REVEAL),
    control(COMMENDATION_STATIC, L, BLANK, Some("STT:StandScoreMessage"), LONG_TEXT_REVEAL),
    control(ENEMIES_DESTROYED_LABEL_STATIC, L, Some("GUI:HeadsTaken"), None, TEXT_REVEAL),
    control(ENEMIES_DESTROYED_VALUE_STATIC, L, BLANK, Some("STT:StandScoreKills"), TEXT_REVEAL),
    control(STATUS_HELP_STATIC, L, BLANK, None, LONG_TEXT_REVEAL),
    control(TITLE_STATIC, C, Some("GUI:StandaloneScore"), None, TEXT_REVEAL),
];

/// The LAN Cooperative campaign-end dialog `0xBC7`
/// (`CampaignEndScoreClass vt+4` `0x00471420`, proc `0x00470D30`).
pub const CAMPAIGN_END_DIALOG: DialogResourceId = DialogResourceId(0x0BC7);
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
    back: MenuPageButtonSpec::new(
        CAMPAIGN_END_CONTINUE,
        346,
        "GUI:Continue",
        "STT:MPScoreButtonContinue",
    ),
};

/// `0xBC7`'s kind-1 groups, executed from the same getters as `0x103`'s.
const PLACE_LABEL_REVEAL: Option<Kind1Params> = Some(Kind1Params {
    interval: Duration::from_millis(15),
    step: 1,
    range: 8,
});
const PLACE_REVEAL: Option<Kind1Params> = kind1(60, 32);
const HEADER_REVEAL: Option<Kind1Params> = kind1(30, 16);

/// Retail `RT_DIALOG 0xBC7`'s Continue and kind-1 statics in template order.
/// Its score cells `0x781..0x78A` share `0xBC6`'s table `0x0081B1EC` and are
/// not kind-1 ([`CampaignScoreCell`]).
#[rustfmt::skip]
pub const CAMPAIGN_END_CONTROLS: [CampaignScoreControlSpec; 18] = [
    control(0x06D1, L, Some("GUI:Continue"), Some("STT:MPScoreButtonContinue"), None),
    control(0x0736, C, Some("GUI:FirstPlace"), None, PLACE_LABEL_REVEAL),
    control(0x0737, C, Some("GUI:SecondPlace"), None, PLACE_LABEL_REVEAL),
    control(0x073A, C, BLANK, Some("STT:MPScoreLabelPlayer"), PLACE_REVEAL),
    control(0x0739, C, BLANK, Some("STT:MPScoreLabelPlayer"), PLACE_REVEAL),
    control(0x0733, C, BLANK, Some("STT:MPScoreLabelCampaignName"), TEXT_REVEAL),
    control(0x0735, L, BLANK, Some("STT:MPCampEndCampaignNumber"), TEXT_REVEAL),
    control(STATUS_HELP_STATIC, L, BLANK, None, LONG_TEXT_REVEAL),
    control(TITLE_STATIC, C, Some("GUI:CampaignResults"), None, TEXT_REVEAL),
    control(0x0732, C, BLANK, Some("STT:MPScoreLabelDifficulty"), TEXT_REVEAL),
    control(0x03EA, R, BLANK, Some("STT:MPCampEndTime"), TEXT_REVEAL),
    control(0x0738, C, BLANK, Some("STT:MPScoreLabelScore"), PLACE_REVEAL),
    control(0x073B, C, BLANK, Some("STT:MPScoreLabelScore"), PLACE_REVEAL),
    control(0x073C, L, Some("GUI:Player"), None, HEADER_REVEAL),
    control(0x073D, R, Some("GUI:Kills"), None, HEADER_REVEAL),
    control(0x073E, R, Some("GUI:Losses"), None, HEADER_REVEAL),
    control(0x073F, R, Some("GUI:Built"), None, HEADER_REVEAL),
    control(0x0740, R, Some("GUI:Score"), None, HEADER_REVEAL),
];

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
    pub const fn id(self) -> DialogResourceId {
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

    /// The controls as nodes: Continue presses, and each static with status
    /// help only hovers.
    pub fn nodes(self) -> vera20k_gui::NodeSpec {
        use vera20k_gui::{NodeKind, NodeSpec};
        let node = |c: &CampaignScoreControlSpec| match c.tooltip_key {
            _ if c.id == self.continue_button() => Some(
                NodeSpec::new(NodeKind::Button)
                    .id(i32::from(c.id))
                    .label(c.caption_key.unwrap_or_default()),
            ),
            help => help.map(|key| menu_page::hover_target(c.id, key)),
        };
        NodeSpec::new(NodeKind::Dialog).children(self.controls().iter().filter_map(node))
    }

    pub fn control(self, id: u16) -> Option<&'static CampaignScoreControlSpec> {
        self.controls().iter().find(|control| control.id == id)
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
    pub rows: Vec<ScoreRow>,
}

/// One of `0xBC7`'s plain score cells (`0x781..0x78A`): its text, alignment
/// and the row colour `0x498` set. They paint whole, with no reveal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CampaignScoreCell {
    pub id: u16,
    pub text: String,
    pub align: ShellAlign,
    pub rgb: [u8; 3],
}

/// Geometry for a campaign score dialog at one window size. The control
/// windows are the executed WM_INITDIALOG relayout of `0x103`
/// (`tools/storage_oracle/shell_relayout.py`): the authored template at
/// 640x480, its upper 50 pixels at 800x600 and the right-panel anchors of
/// `0x0060B1D0`/`0x0060B350`/`0x0060B550`. The `0x72B` art window is the
/// `0x00608500` override, not the template rect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CampaignScoreLayout {
    pub page: MenuPageLayout,
    /// `SCRBK` at its native SHP size: 472x448 at width 640, 632x568 else.
    /// Its top-left is the native dialog origin `[0x00B0FC1C]`.
    pub background: RectPx,
    /// `SCRT` frame 0: 472x448, centred in `SCRBK` (x + 80 from width 800).
    pub panel: RectPx,
    /// The body statics, the art window `0x72B` and the heading.
    pub controls: RectTable,
}

/// `0x0060B950` moves `0x103`'s eight text statics up by
/// `10 - (600 - 480) / 2` when the screen is at least 800 wide.
const TEXT_UPPER_SHIFT: i32 = 50;

/// The SCRBK rectangle in the unscaled shell. The background overlay
/// (`0x0072E730`) uses the dialog origin, and `0x0072D460` selects the
/// small score art at width 640.
pub(crate) fn campaign_score_background_rect(screen_w: i32, screen_h: i32) -> RectPx {
    let (x, y) = geom::dialog_origin(screen_w, screen_h);
    let (w, h) = if screen_w == 640 {
        (472, 448)
    } else {
        (632, 568)
    };
    RectPx::new(x, y, w, h)
}

impl CampaignScoreLayout {
    /// Retail window placement of either dialog for one campaign side.
    pub fn compute(dialog: CampaignScoreDialog, screen_w: u32, screen_h: u32, side: i32) -> Self {
        let (w, h) = (screen_w as i32, screen_h as i32);
        let section = match dialog {
            CampaignScoreDialog::Victory => "0x103",
            CampaignScoreDialog::CampaignEnd => "0xBC7",
        };
        let (page, placed) = layout_data::menu_page(
            dialog.page(),
            section,
            crate::ui::shell::geom::RETAIL_SCALE,
            w,
            h,
        );
        let shift = match dialog {
            CampaignScoreDialog::Victory if w >= 800 => TEXT_UPPER_SHIFT,
            _ => 0,
        };
        let mut controls: RectTable = placed
            .iter()
            .map(|(id, r)| (id, RectPx::new(r.x, r.y - shift, r.w, r.h)))
            .collect();
        controls.insert(TITLE_STATIC, page.title);
        controls.insert(STATUS_HELP_STATIC, page.status_help);
        controls.insert(SCORE_ART_REGION_STATIC, campaign_score_art_rect(w, h, side));
        let background = campaign_score_background_rect(w, h);
        let panel_x = background.x + if w >= 800 { 80 } else { 0 };
        Self {
            panel: RectPx::new(panel_x, background.y, 472, 448),
            page,
            background,
            controls,
        }
    }

    pub fn control_rect(&self, id: u16) -> Option<RectPx> {
        match id {
            CAMPAIGN_END_MONITOR => Some(self.page.warning_monitor),
            id => self.controls.get(id).or_else(|| self.page.button_rect(id)),
        }
    }
}

/// `0x00608500`'s fixed window for dialog `0x103`'s `0x72B` static. The
/// campaign side selects x 104/103 at width 800+ and 24/23 below it; the
/// 230-pixel Soviet/Yuri art starts 12 pixels higher. `0x0060AF50` applies
/// the rect and keeps the centred dialog origin.
fn campaign_score_art_rect(screen_w: i32, screen_h: i32, side: i32) -> RectPx {
    let (origin_x, origin_y) = geom::dialog_origin(screen_w, screen_h);
    // `0x00608500` tests `DAT_007f5be4` (800), not the 640 asset switch.
    let x = if screen_w < 800 { 24 } else { 104 };
    let (x, y, height) = if side == 0 {
        (x, 142, 220)
    } else {
        (x - 1, 130, 230)
    };
    RectPx::new(origin_x + x, origin_y + y, 424, height)
}

vera20k_gui::scaled_rects!(CampaignScoreLayout {
    page,
    background,
    panel,
    controls
});

/// The open campaign score dialog, `0x103` or `0xBC7`.
#[derive(Debug, Clone)]
pub struct CampaignScorePage {
    pub dialog: CampaignScoreDialog,
    /// Scenario `+0x34B8`, which picks the art window (`0x00608500`).
    pub side: i32,
    pub statics: ScoreStatics,
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
}

impl CampaignScorePage {
    /// The init of `dialog`: each kind-1 static gets the text and colour
    /// `value` gives its id, else its template caption resolved by `caption`.
    fn open_dialog(
        dialog: CampaignScoreDialog,
        side: i32,
        value: impl Fn(u16) -> Option<(String, Option<[u8; 3]>)>,
        caption: impl Fn(&'static str) -> String,
        now: Instant,
    ) -> Self {
        let statics = dialog.controls().iter().filter_map(|spec| {
            let reveal = spec.reveal?;
            let (text, rgb) = match spec.id {
                STATUS_HELP_STATIC => (String::new(), None),
                id => value(id)
                    .unwrap_or_else(|| (spec.caption_key.map(&caption).unwrap_or_default(), None)),
            };
            Some(ScoreStatic::new(
                spec.id, spec.align, rgb, reveal, &text, now,
            ))
        });
        Self {
            dialog,
            side,
            statics: ScoreStatics(statics.collect()),
            cells: Vec::new(),
            emblem_animation: None,
            presentation_started: false,
        }
    }

    /// Install the app-resolved values into the native `0x103` controls; the
    /// labels keep their template captions, resolved by `caption`.
    pub fn open(
        model: &CampaignScoreScreenModel,
        caption: impl Fn(&'static str) -> String,
        now: Instant,
    ) -> Self {
        let value = |id| {
            let text = match id {
                PAR_TIME_VALUE_STATIC => &model.par_time,
                YOUR_TIME_VALUE_STATIC => &model.your_time,
                RANK_STATIC => &model.rank,
                COMMENDATION_STATIC => &model.commendation,
                ENEMIES_DESTROYED_VALUE_STATIC => &model.enemies_destroyed,
                _ => return None,
            };
            Some((text.clone(), None))
        };
        Self::open_dialog(
            CampaignScoreDialog::Victory,
            model.side,
            value,
            caption,
            now,
        )
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
        let value =
            |id| {
                let place = CAMPAIGN_END_PLACES.iter().zip(&model.rows).find_map(
                    |(&(name, score), row)| {
                        let text = (id == name).then(|| row.name.clone());
                        let text = text.or_else(|| (id == score).then(|| row.score.to_string()));
                        text.map(|text| (text, Some(row.rgb)))
                    },
                );
                let text = match id {
                    _ if place.is_some() => return place,
                    CAMPAIGN_END_NAME => &model.campaign_name,
                    CAMPAIGN_END_NUMBER => &model.campaign_number,
                    CAMPAIGN_END_DIFFICULTY => &model.difficulty,
                    CAMPAIGN_END_TIME => &model.time,
                    _ => return None,
                };
                Some((text.clone(), None))
            };
        let dialog = CampaignScoreDialog::CampaignEnd;
        let mut page = Self::open_dialog(dialog, model.side, value, caption, now);
        let rows = model
            .rows
            .iter()
            .zip(crate::ui::coop_score_shell::ROW_CELLS);
        for (row, ids) in rows {
            for (column, (id, text)) in ids.into_iter().zip(row.cells()).enumerate() {
                let align = if column == 0 { L } else { R };
                page.cells.push(CampaignScoreCell {
                    id,
                    text,
                    align,
                    rgb: row.rgb,
                });
            }
        }
        page
    }

    /// `0xBC7`'s score cells; empty on `0x103`.
    pub fn cells(&self) -> &[CampaignScoreCell] {
        &self.cells
    }

    /// Native SHOW completion (`0x4EC`): start every kind-1 static and the
    /// kind-4 emblem timer when the SCRA frames have been loaded for control
    /// `0x72B`. Idempotent, so a repeated completion or a wave-less fallback
    /// cannot restart a running reveal.
    pub fn start_presentation(&mut self, frame_count: usize, now: Instant) {
        if self.presentation_started {
            return;
        }
        self.presentation_started = true;
        self.statics.start(now);
        self.emblem_animation = (frame_count > 0).then_some(CampaignScoreEmblemAnimation {
            frame_count,
            frame: 0,
            next_tick_at: now + CAMPAIGN_SCORE_EMBLEM_INTERVAL,
        });
    }

    pub fn presentation_started(&self) -> bool {
        self.presentation_started
    }

    /// Geometry this page instance uses at the live window size. The retail
    /// path is unscaled; the app's fill-window path scales the 800x600 page.
    pub fn layout(&self, screen_w: u32, screen_h: u32, fills_window: bool) -> CampaignScoreLayout {
        let layout = |w, h| CampaignScoreLayout::compute(self.dialog, w, h, self.side);
        if fills_window {
            fit_window(screen_w, screen_h, layout)
        } else {
            layout(screen_w, screen_h)
        }
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
            .map(|animation| animation.next_tick_at)
    }
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;

    fn layout(width: u32, height: u32, side: i32) -> CampaignScoreLayout {
        CampaignScoreLayout::compute(CampaignScoreDialog::Victory, width, height, side)
    }

    fn open(at: Instant) -> CampaignScorePage {
        let model = CampaignScoreScreenModel::default();
        CampaignScorePage::open(&model, |key| key.to_owned(), at)
    }

    #[test]
    fn campaign_emblem_timer_advances_one_frame_and_native_wrap_selects_29() {
        let opened = Instant::now();
        let ms = |ms| opened + Duration::from_millis(ms);
        let mut page = open(opened);
        assert_eq!(page.poll_emblem_animation(opened), None);
        page.start_presentation(44, opened);
        assert_eq!(page.emblem_animation_frame(), Some(0));
        assert_eq!(page.next_emblem_animation_tick(), Some(ms(100)));
        assert_eq!(page.poll_emblem_animation(ms(99)), Some(0));
        assert_eq!(page.poll_emblem_animation(ms(100)), Some(1));
        for tick in 2..=43 {
            assert_eq!(
                page.poll_emblem_animation(ms(tick * 100)),
                Some(tick as usize)
            );
        }
        assert_eq!(page.poll_emblem_animation(ms(4_400)), Some(29));
        assert_eq!(page.poll_emblem_animation(ms(4_500)), Some(30));

        // A stalled event loop coalesces to one WM_TIMER callback.
        let mut stalled = open(opened);
        stalled.start_presentation(44, opened);
        assert_eq!(stalled.poll_emblem_animation(ms(5_000)), Some(1));
        assert_eq!(stalled.next_emblem_animation_tick(), Some(ms(5_100)));
        // Without SCRA frames there is no timer.
        let mut bare = open(opened);
        bare.start_presentation(0, opened);
        assert_eq!(bare.next_emblem_animation_tick(), None);
    }

    #[test]
    fn campaign_emblem_timer_remains_scheduled_after_text_reveals_finish() {
        let opened = Instant::now();
        let mut page = open(opened);
        page.start_presentation(44, opened);
        for elapsed_ms in (0..10_000).step_by(15) {
            let now = opened + Duration::from_millis(elapsed_ms);
            page.poll_emblem_animation(now);
            for static_text in &mut page.statics.0 {
                static_text.paint(now);
            }
            page.statics.commit_presented();
            if page.statics.is_terminal() {
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
        let cases = fixture["cases"].as_array().unwrap();
        let case = |dialog: u16, id: u16| {
            cases
                .iter()
                .find(|case| case["dialog_id"] == dialog && case["control_id"] == id)
                .unwrap_or_else(|| panic!("no {dialog:#x} timer case for {id:#x}"))
        };
        for spec in &CAMPAIGN_SCORE_CONTROLS {
            let Some(reveal) = spec.reveal else {
                continue;
            };
            let case = case(0x103, spec.id);
            assert_eq!(case["kind1"], 1, "{:#x}", spec.id);
            assert_eq!(case["interval_ms"], reveal.interval.as_millis() as u64);
            assert_eq!(case["step"], reveal.step, "{:#x}", spec.id);
            assert_eq!(case["range"], reveal.range, "{:#x}", spec.id);
        }
        // 0x72B is the only kind-4 static; `ShellStatic__AnimationInterval`
        // 0x006033F0 selects 100 ms for dialog 0x103's 0x72B.
        let art_case = case(0x103, SCORE_ART_REGION_STATIC);
        assert_eq!(art_case["kind1"], 0);
        assert_eq!(art_case["kind4_interval_ms"], 100);
        assert_eq!(CAMPAIGN_SCORE_EMBLEM_INTERVAL, Duration::from_millis(100));
    }

    #[test]
    fn presentation_start_is_single_shot_and_arms_reveals_and_timer() {
        let opened = Instant::now();
        let mut page = open(opened);
        assert!(!page.presentation_started());
        assert_eq!(page.emblem_animation_frame(), None);
        page.start_presentation(44, opened);
        assert!(page.presentation_started());
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
        use crate::ui::shell::native_fixtures::executed_child_window_for_side as executed;
        let ids = CAMPAIGN_SCORE_CONTROLS.iter().map(|spec| spec.id);
        let ids: Vec<u16> = ids.chain([SCORE_ART_REGION_STATIC]).collect();
        for (width, height) in [(640, 480), (800, 600), (1024, 768)] {
            for side in [0, 1] {
                let layout = layout(width, height, side);
                for &id in &ids {
                    assert_eq!(
                        layout.control_rect(id),
                        Some(executed(0x103, id, width as i32, height as i32, side)),
                        "{width}x{height} side {side} control {id:#x}",
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
            let layout = layout(width, height, 0);
            assert_eq!(layout.background, background, "{width}");
            assert_eq!(layout.panel, panel, "{width}");
            assert_eq!(
                layout.control_rect(SCORE_ART_REGION_STATIC),
                Some(art),
                "{width}"
            );
        }
        assert_eq!(
            layout(800, 600, 1).control_rect(SCORE_ART_REGION_STATIC),
            Some(RectPx::new(103, 130, 424, 230))
        );
        assert_eq!(
            layout(640, 480, 2).control_rect(SCORE_ART_REGION_STATIC),
            Some(RectPx::new(23, 130, 424, 230))
        );
        // 0x00608500 switches at 800, not at the 640-wide asset branch.
        for (width, x) in [(641, 24), (700, 24), (799, 24), (800, 104)] {
            assert_eq!(
                layout(width, 600, 0)
                    .control_rect(SCORE_ART_REGION_STATIC)
                    .map(|rect| rect.x),
                Some(x),
                "{width}"
            );
        }
    }

    #[test]
    fn page_binds_only_resolved_values_to_its_statics() {
        let model = CampaignScoreScreenModel {
            par_time: "00:10:00".into(),
            your_time: "00:08:31".into(),
            enemies_destroyed: "17".into(),
            rank: "Major".into(),
            commendation: "Mission commendation".into(),
            ..Default::default()
        };
        let page = CampaignScorePage::open(&model, |key| key.to_owned(), Instant::now());
        let text = |id| page.statics.text(id);
        assert_eq!(text(TITLE_STATIC), Some("GUI:StandaloneScore"));
        assert_eq!(text(PAR_TIME_LABEL_STATIC), Some("GUI:ParTime"));
        assert_eq!(text(PAR_TIME_VALUE_STATIC), Some("00:10:00"));
        assert_eq!(text(YOUR_TIME_VALUE_STATIC), Some("00:08:31"));
        assert_eq!(text(RANK_STATIC), Some("Major"));
        assert_eq!(text(COMMENDATION_STATIC), Some("Mission commendation"));
        assert_eq!(text(ENEMIES_DESTROYED_VALUE_STATIC), Some("17"));
        assert_eq!(text(STATUS_HELP_STATIC), Some(""));
        assert_eq!(page.statics.0.len(), 10);
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
            let layout =
                CampaignScoreLayout::compute(CampaignScoreDialog::CampaignEnd, width, height, 0);
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
        assert_eq!(
            page.statics.text(CAMPAIGN_END_NAME),
            Some("Soviet Campaign")
        );
        assert_eq!(page.statics.text(CAMPAIGN_END_TIME), Some("01:02:03"));
        assert_eq!(page.statics.text(0x0736), Some("GUI:FirstPlace"));
        assert_eq!(page.statics.text(TITLE_STATIC), Some("GUI:CampaignResults"));
        let place = |id| {
            page.statics
                .0
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

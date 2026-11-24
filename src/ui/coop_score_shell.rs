//! The LAN Cooperative result dialog `0xBC6` between missions: the loop
//! `CampaignScoreClass vt+4` (`0x0046FC00`) and its proc
//! `Dialog0xBC6__DlgProc` (`0x0046DFD0`).
//!
//! Controls, captions, styles and help keys come from retail
//! `RT_DIALOG 0xBC6`; the windows are the executed relayout
//! (`tools/storage_oracle/lan_lobby.json` `setup_layouts`). Body controls
//! keep their resource places; the heading, buttons, preview and map texts
//! are right-anchored like BC's. The status statics `0x5C8`/`0x70A`/`0x70B`,
//! the AI rows and the final-mission frame `0x71C` are template-hidden.
//!
//! The app supplies resolved texts and a per-frame view of the round, which
//! `MatchSession` owns; this leaf keeps only the dialog's interaction.

use crate::ui::lan_shell::LanChatState;
use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::geom::RectPx;
use crate::ui::shell::geom::ShellAlign;
use crate::ui::shell::layout::RectTable;
use crate::ui::shell::layout_data;
use crate::ui::shell::menu_page::{self, MenuPageButtonSpec, MenuPageLayout, MenuPageSpec};
use vera20k_gui::shell::trackbar::TrackbarRange;

pub const COOP_SCORE_DIALOG: DialogResourceId = DialogResourceId(0x0BC6);
pub const CONTINUE: u16 = 0x06D1;
pub const QUIT: u16 = 0x05C0;
pub const TIME: u16 = 0x03EA;
pub const NEXT_MISSION: u16 = 0x077E;
pub const MODE_LABEL: u16 = 0x06EC;
pub const MAP_LABEL: u16 = 0x05A8;
pub const PREVIEW: u16 = 0x0468;
pub const DESCRIPTION: u16 = 0x072F;
pub const DIFFICULTY: u16 = 0x0732;
pub const CHAT: u16 = 0x0540;
pub const CHAT_LOG: u16 = 0x053F;
/// Per human slot (rows 0 and 1): name combo (`0x0046F5E0`), ready icon,
/// flag, country (`0x004E37D0`), colour (`0x004E41D0`) and start
/// (`0x004E4E60`).
pub const NAMES: [u16; 2] = [0x0411, 0x0412];
pub const READY_ICONS: [u16; 2] = [0x06D2, 0x06D3];
pub const FLAGS: [u16; 2] = [0x06DA, 0x06DB];
pub const COUNTRIES: [u16; 2] = [0x06A1, 0x0510];
pub const COLORS: [u16; 2] = [0x06A2, 0x0522];
pub const STARTS: [u16; 2] = [0x06A3, 0x06A4];
/// The score rows' cells, Player/Kills/Losses/Built/Score, from the tail's
/// table `0x0081B1EC`.
pub const ROW_CELLS: [[u16; 5]; 2] = [
    [0x0781, 0x0783, 0x0785, 0x0787, 0x0789],
    [0x0782, 0x0784, 0x0786, 0x0788, 0x078A],
];

/// The right-panel heading and the two buttons (`0x0060B000` Continue,
/// `0x0060B350` Quit).
pub const COOP_SCORE_PAGE: MenuPageSpec = MenuPageSpec {
    dialog: COOP_SCORE_DIALOG,
    title_key: "GUI:CampaignResults",
    stacked: &[MenuPageButtonSpec::new(
        CONTINUE,
        149,
        "GUI:Continue",
        "STT:MPCampScoreContinue",
    )],
    back: MenuPageButtonSpec::new(QUIT, 346, "GUI:Quit", "STT:MPCampScoreQuit"),
};

/// One visible body control of `RT_DIALOG 0xBC6`: id, caption key (statics
/// with fixed text), help key and Win32 STATIC alignment (`SS_CENTER` 1,
/// `SS_RIGHT` 2). The windows are `shell_layout.ini`'s.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoopScoreControlSpec {
    pub id: u16,
    pub caption_key: Option<&'static str>,
    pub help_key: Option<&'static str>,
    pub align: ShellAlign,
}

const fn control(
    id: u16,
    caption_key: Option<&'static str>,
    help_key: Option<&'static str>,
    align: ShellAlign,
) -> CoopScoreControlSpec {
    CoopScoreControlSpec {
        id,
        caption_key,
        help_key,
        align,
    }
}

/// The template's unnamed `GUI:PreviousMission` static (id -1).
pub const PREVIOUS_MISSION: u16 = 0xFFFF;

const L: ShellAlign = ShellAlign::NONE;
const C: ShellAlign = ShellAlign::H_CENTER;
const R: ShellAlign = ShellAlign::H_RIGHT;

/// The visible body controls in template order.
pub const COOP_SCORE_CONTROLS: [CoopScoreControlSpec; 38] = [
    control(0x069D, Some("GUI:Player"), None, L),
    control(0x069E, Some("GUI:Losses"), None, C),
    control(0x069F, Some("GUI:Kills"), None, C),
    control(0x06A0, Some("GUI:Built"), None, C),
    control(0x078B, Some("GUI:Score"), None, C),
    control(0x0781, None, Some("STT:MPScoreLabelPlayer"), L),
    control(0x0785, None, Some("STT:MPScoreLabelLosses"), R),
    control(0x0783, None, Some("STT:MPScoreLabelKills"), R),
    control(0x0787, None, Some("STT:MPScoreLabelBuilt"), R),
    control(0x0789, None, Some("STT:MPScoreLabelScore"), R),
    control(0x0782, None, Some("STT:MPScoreLabelPlayer"), L),
    control(0x0786, None, Some("STT:MPScoreLabelLosses"), R),
    control(0x0784, None, Some("STT:MPScoreLabelKills"), R),
    control(0x0788, None, Some("STT:MPScoreLabelBuilt"), R),
    control(0x078A, None, Some("STT:MPScoreLabelScore"), R),
    control(TIME, None, Some("STT:MPScoreLabelTime"), R),
    control(PREVIOUS_MISSION, Some("GUI:PreviousMission"), None, L),
    control(0x079B, Some("GUI:NextMission"), None, L),
    control(NEXT_MISSION, None, None, L),
    control(0x0411, None, Some("STT:MPScoreLabelPlayer"), L),
    control(0x06D2, None, None, L),
    control(0x06A1, None, Some("STT:HostComboCountry"), L),
    control(0x06DA, None, Some("STT:HostPictureFlag"), L),
    control(0x06A2, None, Some("STT:HostComboColor"), L),
    control(0x0412, None, Some("STT:MPScoreLabelPlayer"), L),
    control(0x06D3, None, None, L),
    control(0x0510, None, Some("STT:HostComboCountry"), L),
    control(0x06DB, None, Some("STT:HostPictureFlag"), L),
    control(0x0522, None, Some("STT:HostComboColor"), L),
    control(CHAT_LOG, None, Some("STT:HostEditOutput"), L),
    control(CHAT, None, Some("STT:HostEditInput"), L),
    control(DIFFICULTY, None, Some("STT:MPScoreLabelDifficulty"), L),
    control(0x0798, Some("TXT_EASY"), None, L),
    control(0x0799, Some("TXT_NORMAL"), None, L),
    control(0x079A, Some("TXT_HARD"), None, L),
    control(0x06A3, None, None, L),
    control(0x06A4, None, None, L),
    control(DESCRIPTION, None, None, L),
];

/// The controls `0x0046DE20` hides on the final mission (`can_continue`
/// false): Continue, the slider, every combo, the description, the next
/// mission texts, the preview and the map name.
pub fn hidden_on_final_mission(id: u16) -> bool {
    matches!(
        id,
        CONTINUE | DIFFICULTY | DESCRIPTION | NEXT_MISSION | 0x079B | PREVIEW | MAP_LABEL
    ) || NAMES.contains(&id)
        || COUNTRIES.contains(&id)
        || COLORS.contains(&id)
        || STARTS.contains(&id)
}

/// `0x0046DE20`'s slider range: Easy, Normal, Hard.
pub const DIFFICULTY_RANGE: TrackbarRange = TrackbarRange::new(0, 2, 1);

#[derive(Debug, Clone)]
pub struct CoopScoreLayout {
    pub page: MenuPageLayout,
    controls: RectTable,
}

impl CoopScoreLayout {
    pub fn new(width: i32, height: i32, responsive: bool) -> Self {
        let scale = if responsive {
            crate::ui::shell::geom::shell_scale(width, height)
        } else {
            crate::ui::shell::geom::RETAIL_SCALE
        };
        let (page, controls) =
            layout_data::menu_page(&COOP_SCORE_PAGE, "0xBC6", scale, width, height);
        Self { page, controls }
    }

    pub fn control_rect(&self, id: u16) -> Option<RectPx> {
        self.controls.get(id).or_else(|| self.page.button_rect(id))
    }
}

/// A control's status help (`STT:` key), from the template table.
pub fn help_key(id: u16) -> Option<&'static str> {
    COOP_SCORE_PAGE
        .buttons()
        .find(|button| button.id == id)
        .map(|button| button.tooltip_key)
        .or_else(|| {
            COOP_SCORE_CONTROLS
                .iter()
                .find(|spec| spec.id == id)
                .and_then(|spec| spec.help_key)
        })
}

/// What the dialog asks of its owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoopScoreAction {
    Continue,
    Quit,
    Chat(Vec<u16>),
}

/// The right-panel heading `0x694` and the status line `0x695`, statics
/// that take no pointer.
pub const TITLE: u16 = 0x0694;
pub const STATUS: u16 = 0x0695;

/// Whether body control `id` shows: the final mission hides the
/// continuation and the flags (`0x0046DE20`), and an empty slot its row.
pub fn shown(id: u16, can_continue: bool, slots: [bool; 2]) -> bool {
    if !can_continue && (hidden_on_final_mission(id) || FLAGS.contains(&id)) {
        return false;
    }
    let row = [NAMES, READY_ICONS, FLAGS, COUNTRIES, COLORS, STARTS]
        .iter()
        .find_map(|ids| ids.iter().position(|each| *each == id));
    row.is_none_or(|row| slots[row])
}

/// The page's buttons and body controls as nodes, the buttons first, the
/// body in the order it paints (no two body controls overlap, so it is
/// also the order a press finds them): the heading and status line, the
/// statics, the score cells, each slot's ready icon, flag and lists
/// (disabled: BC6 only shows them), Difficulty (disabled, no plaque), the
/// chat edit `0x540` and its log `0x53F`, the preview. Each but the heading
/// and status line hovers with its help.
pub fn nodes() -> vera20k_gui::NodeSpec {
    use vera20k_gui::{NodeKind, NodeSpec, Prop, Value};
    let node = |kind, id: u16| NodeSpec::new(kind).id(id.into());
    let hover = |kind, id: u16| node(kind, id).tooltip(help_key(id).unwrap_or_default());
    let align =
        |spec: NodeSpec, align: ShellAlign| spec.with(Prop::Align, Value::Int(align.0.into()));
    let spec_align = |id: u16| {
        COOP_SCORE_CONTROLS
            .iter()
            .find(|spec| spec.id == id)
            .map_or(ShellAlign::NONE, |spec| spec.align)
    };
    let statics = COOP_SCORE_CONTROLS
        .iter()
        .filter(|spec| {
            spec.caption_key.is_some() || matches!(spec.id, TIME | NEXT_MISSION | DESCRIPTION)
        })
        .map(move |spec| align(hover(NodeKind::Text, spec.id), spec.align));
    let labels =
        [MODE_LABEL, MAP_LABEL].map(|id| align(hover(NodeKind::Text, id), ShellAlign::H_CENTER));
    let cells = ROW_CELLS
        .into_iter()
        .flatten()
        .map(move |id| align(hover(NodeKind::Text, id), spec_align(id)));
    let slots = (0..2).flat_map(move |row| {
        let combo = |id| {
            hover(NodeKind::Combo, id)
                .with(Prop::Enabled, Value::Bool(false))
                .with(Prop::Selected, Value::Int(0))
        };
        [
            hover(NodeKind::Image, READY_ICONS[row]),
            hover(NodeKind::Image, FLAGS[row]),
            combo(NAMES[row]),
            combo(COUNTRIES[row]),
            combo(COLORS[row]),
            combo(STARTS[row]),
        ]
    });
    let range = DIFFICULTY_RANGE;
    let difficulty = hover(NodeKind::Trackbar, DIFFICULTY)
        .with(Prop::Enabled, Value::Bool(false))
        .with(Prop::ShowValue, Value::Bool(false))
        .with(Prop::Min, Value::Int(range.min))
        .with(Prop::Max, Value::Int(range.max))
        .with(Prop::Step, Value::Int(range.step));
    let inset = vera20k_gui::Rect::new(2, 0, 2, 0);
    let log = hover(NodeKind::List, CHAT_LOG)
        .with(Prop::RowCount, Value::Int(0))
        .with(Prop::TextInset, Value::Rect(inset))
        .with(Prop::NoSelectionFill, Value::Bool(true));
    COOP_SCORE_PAGE
        .nodes()
        .child(align(node(NodeKind::Text, TITLE), ShellAlign::PANEL_TITLE))
        .child(node(NodeKind::Text, STATUS))
        .children(statics)
        .children(labels)
        .children(cells)
        .children(slots)
        .child(difficulty)
        .child(hover(NodeKind::Edit, CHAT))
        .child(log)
        .child(menu_page::hover_target(
            PREVIEW,
            help_key(PREVIEW).unwrap_or_default(),
        ))
}

/// BC6's own state: the chat transcript (the buttons, the chat edit and
/// the log's scroll are nodes).
#[derive(Debug, Clone, Default)]
pub struct CoopScoreState {
    pub chat: LanChatState,
}

/// The texts the tail `0x0046EE17` writes once: this mission's rows
/// (`0x0046DA70`, sorted by `0x0046DC90`), the time `0x3EA`, the next
/// mission `0x77E`, the game type `0x6EC`, the map name `0x5A8` and the
/// description `0x72F`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoopScoreModel {
    pub rows: Vec<crate::ui::score_shell::ScoreRow>,
    pub time: String,
    pub next_mission: String,
    pub mode_label: String,
    pub map_label: String,
    pub description: String,
    /// Scenario `+0x34B8`, which picks the score palette (`0x0072D300`).
    pub side: i32,
    /// The next mission's shell map, for the preview `0x468`.
    pub map_index: Option<usize>,
    pub can_continue: bool,
}

/// The open dialog: its fixed texts, its interaction, and what the
/// continuation needs from the finished match.
#[derive(Debug, Clone)]
pub struct CoopScorePage {
    pub model: CoopScoreModel,
    pub ui: CoopScoreState,
    /// GameExit's arm: Victory or Defeat.
    pub victory: bool,
    /// After the last stage's win, the campaign-end dialog `0xBC7` that
    /// Quit opens (`+0xA4`), resolved while the Houses were resident.
    pub campaign_end: Option<crate::ui::campaign_score_shell::CampaignEndModel>,
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;
    use crate::ui::skirmish_shell::COMBO_FACE_H;

    #[test]
    fn controls_follow_the_executed_bc6_relayout() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/lan_lobby.json",
        ))
        .unwrap();
        let mut checked = 0;
        for case in corpus["setup_layouts"].as_array().unwrap() {
            if case["dialog"] != 0xBC6 {
                continue;
            }
            let layout = CoopScoreLayout::new(
                case["width"].as_i64().unwrap() as i32,
                case["height"].as_i64().unwrap() as i32,
                false,
            );
            for child in case["children"].as_array().unwrap() {
                let id = child["id"].as_u64().unwrap();
                let id = if id == u64::from(u32::MAX) {
                    PREVIOUS_MISSION
                } else {
                    id as u16
                };
                let Some(actual) = layout.control_rect(id) else {
                    continue;
                };
                let a = child["rect"].as_array().unwrap();
                let mut native = RectPx::new(
                    a[0].as_i64().unwrap() as i32,
                    a[1].as_i64().unwrap() as i32,
                    a[2].as_i64().unwrap() as i32,
                    a[3].as_i64().unwrap() as i32,
                );
                if child["class"] == "ComboBox" {
                    native.h = COMBO_FACE_H;
                }
                assert_eq!(
                    actual, native,
                    "{}x{} control {id:#x}",
                    case["width"], case["height"]
                );
                checked += 1;
            }
            assert!(layout.control_rect(0x695).is_none());
            let status = case["children"]
                .as_array()
                .unwrap()
                .iter()
                .find(|child| child["id"] == 0x695)
                .unwrap()["rect"]
                .clone();
            let r = layout.page.status_help;
            assert_eq!(status, serde_json::json!([r.x, r.y, r.w, r.h]));
            let title = case["children"]
                .as_array()
                .unwrap()
                .iter()
                .find(|child| child["id"] == 0x694)
                .unwrap()["rect"]
                .clone();
            let t = layout.page.title;
            assert_eq!(title, serde_json::json!([t.x, t.y, t.w, t.h]));
        }
        assert!(checked > 80, "both sizes' visible controls: {checked}");
    }

    #[test]
    fn help_keys_follow_the_template() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/lan_lobby.json",
        ))
        .unwrap();
        let mut checked = 0;
        for case in corpus["setup_help"].as_array().unwrap() {
            if case["dialog"] != 0xBC6 {
                continue;
            }
            let control = case["control"].as_u64().unwrap() as u16;
            assert_eq!(help_key(control), case["key"].as_str(), "{case}");
            checked += 1;
        }
        assert!(checked > 10);
    }

    #[test]
    fn the_final_mission_hides_continuation() {
        assert!(hidden_on_final_mission(CONTINUE));
        assert!(hidden_on_final_mission(COUNTRIES[1]));
        assert!(!hidden_on_final_mission(QUIT));
        assert!(!hidden_on_final_mission(ROW_CELLS[0][4]));
    }
}

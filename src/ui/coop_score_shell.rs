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
use crate::ui::shell::button::ShellButtonInteraction;
use crate::ui::shell::descriptor::{AnchorRule, DialogId, LOW_HEADING_ANCHOR, MAP_TEXT_ANCHOR};
use crate::ui::shell::geom::{RectPx, child_window};
use crate::ui::shell::layout::{anchor_rect, status_line_rect};
use crate::ui::shell::menu_page::{self, MenuPageButtonSpec, MenuPageLayout, MenuPageSpec};
use crate::ui::shell::trackbar::TrackbarRange;

pub const COOP_SCORE_DIALOG: DialogId = DialogId(0x0BC6);
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
    stacked: &[MenuPageButtonSpec {
        id: CONTINUE,
        dlu_top: 149,
        csf_key: "GUI:Continue",
        tooltip_key: "STT:MPCampScoreContinue",
        result: None,
    }],
    back: MenuPageButtonSpec {
        id: QUIT,
        dlu_top: 346,
        csf_key: "GUI:Quit",
        tooltip_key: "STT:MPCampScoreQuit",
        result: None,
    },
};

/// Win32 STATIC alignment (`SS_RIGHT` is style 2, `SS_CENTER` 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoopScoreAlign {
    Left,
    Center,
    Right,
}

/// One visible body control of `RT_DIALOG 0xBC6`: id, template DLU,
/// caption key (statics with fixed text), help key and text alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoopScoreControlSpec {
    pub id: u16,
    pub dlu: RectPx,
    pub caption_key: Option<&'static str>,
    pub help_key: Option<&'static str>,
    pub align: CoopScoreAlign,
}

const fn control(
    id: u16,
    dlu: RectPx,
    caption_key: Option<&'static str>,
    help_key: Option<&'static str>,
    align: CoopScoreAlign,
) -> CoopScoreControlSpec {
    CoopScoreControlSpec {
        id,
        dlu,
        caption_key,
        help_key,
        align,
    }
}

/// The template's unnamed `GUI:PreviousMission` static (id -1).
pub const PREVIOUS_MISSION: u16 = 0xFFFF;

use CoopScoreAlign::{Center, Left, Right};

/// The visible body controls in template order. Combos keep their dropped
/// height here; the face is the shared 24-pixel combo face.
pub const COOP_SCORE_CONTROLS: [CoopScoreControlSpec; 38] = [
    control(
        0x069D,
        RectPx::new(38, 34, 50, 10),
        Some("GUI:Player"),
        None,
        Left,
    ),
    control(
        0x069E,
        RectPx::new(158, 34, 46, 10),
        Some("GUI:Losses"),
        None,
        Center,
    ),
    control(
        0x069F,
        RectPx::new(106, 34, 38, 10),
        Some("GUI:Kills"),
        None,
        Center,
    ),
    control(
        0x06A0,
        RectPx::new(218, 34, 46, 10),
        Some("GUI:Built"),
        None,
        Center,
    ),
    control(
        0x078B,
        RectPx::new(278, 34, 46, 10),
        Some("GUI:Score"),
        None,
        Center,
    ),
    control(
        0x0781,
        RectPx::new(37, 50, 62, 10),
        None,
        Some("STT:MPScoreLabelPlayer"),
        Left,
    ),
    control(
        0x0785,
        RectPx::new(161, 50, 38, 10),
        None,
        Some("STT:MPScoreLabelLosses"),
        Right,
    ),
    control(
        0x0783,
        RectPx::new(103, 50, 38, 10),
        None,
        Some("STT:MPScoreLabelKills"),
        Right,
    ),
    control(
        0x0787,
        RectPx::new(219, 50, 38, 10),
        None,
        Some("STT:MPScoreLabelBuilt"),
        Right,
    ),
    control(
        0x0789,
        RectPx::new(277, 50, 40, 10),
        None,
        Some("STT:MPScoreLabelScore"),
        Right,
    ),
    control(
        0x0782,
        RectPx::new(37, 64, 62, 10),
        None,
        Some("STT:MPScoreLabelPlayer"),
        Left,
    ),
    control(
        0x0786,
        RectPx::new(161, 64, 38, 10),
        None,
        Some("STT:MPScoreLabelLosses"),
        Right,
    ),
    control(
        0x0784,
        RectPx::new(103, 64, 38, 10),
        None,
        Some("STT:MPScoreLabelKills"),
        Right,
    ),
    control(
        0x0788,
        RectPx::new(219, 64, 38, 10),
        None,
        Some("STT:MPScoreLabelBuilt"),
        Right,
    ),
    control(
        0x078A,
        RectPx::new(277, 64, 40, 10),
        None,
        Some("STT:MPScoreLabelScore"),
        Right,
    ),
    control(
        TIME,
        RectPx::new(318, 5, 89, 10),
        None,
        Some("STT:MPScoreLabelTime"),
        Right,
    ),
    control(
        PREVIOUS_MISSION,
        RectPx::new(19, 6, 66, 10),
        Some("GUI:PreviousMission"),
        None,
        Left,
    ),
    control(
        0x079B,
        RectPx::new(34, 87, 54, 10),
        Some("GUI:NextMission"),
        None,
        Left,
    ),
    control(NEXT_MISSION, RectPx::new(96, 87, 71, 10), None, None, Left),
    control(
        0x0411,
        RectPx::new(73, 103, 100, 74),
        None,
        Some("STT:MPScoreLabelPlayer"),
        Left,
    ),
    control(0x06D2, RectPx::new(53, 103, 18, 12), None, None, Left),
    control(
        0x06A1,
        RectPx::new(222, 103, 78, 74),
        None,
        Some("STT:HostComboCountry"),
        Left,
    ),
    control(
        0x06DA,
        RectPx::new(185, 103, 32, 12),
        None,
        Some("STT:HostPictureFlag"),
        Left,
    ),
    control(
        0x06A2,
        RectPx::new(312, 103, 25, 73),
        None,
        Some("STT:HostComboColor"),
        Left,
    ),
    control(
        0x0412,
        RectPx::new(73, 120, 100, 74),
        None,
        Some("STT:MPScoreLabelPlayer"),
        Left,
    ),
    control(0x06D3, RectPx::new(53, 120, 18, 12), None, None, Left),
    control(
        0x0510,
        RectPx::new(222, 120, 78, 74),
        None,
        Some("STT:HostComboCountry"),
        Left,
    ),
    control(
        0x06DB,
        RectPx::new(185, 120, 32, 12),
        None,
        Some("STT:HostPictureFlag"),
        Left,
    ),
    control(
        0x0522,
        RectPx::new(312, 120, 25, 73),
        None,
        Some("STT:HostComboColor"),
        Left,
    ),
    control(
        CHAT_LOG,
        RectPx::new(7, 227, 407, 98),
        None,
        Some("STT:HostEditOutput"),
        Left,
    ),
    control(
        CHAT,
        RectPx::new(7, 328, 407, 12),
        None,
        Some("STT:HostEditInput"),
        Left,
    ),
    control(
        DIFFICULTY,
        RectPx::new(122, 159, 177, 13),
        None,
        Some("STT:MPScoreLabelDifficulty"),
        Left,
    ),
    control(
        0x0798,
        RectPx::new(112, 145, 38, 8),
        Some("TXT_EASY"),
        None,
        Left,
    ),
    control(
        0x0799,
        RectPx::new(197, 145, 50, 8),
        Some("TXT_NORMAL"),
        None,
        Left,
    ),
    control(
        0x079A,
        RectPx::new(277, 145, 40, 8),
        Some("TXT_HARD"),
        None,
        Left,
    ),
    control(0x06A3, RectPx::new(349, 103, 25, 73), None, None, Left),
    control(0x06A4, RectPx::new(349, 120, 25, 73), None, None, Left),
    control(DESCRIPTION, RectPx::new(7, 182, 407, 39), None, None, Left),
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
pub const DIFFICULTY_RANGE: TrackbarRange = TrackbarRange {
    min: 0,
    max: 2,
    step: 1,
};

/// The combo face height every shell combo paints.
const COMBO_FACE_H: i32 = 24;

#[derive(Debug, Clone)]
pub struct CoopScoreLayout {
    pub page: MenuPageLayout,
    controls: Vec<(u16, RectPx)>,
}

impl CoopScoreLayout {
    pub fn new(width: i32, height: i32, responsive: bool) -> Self {
        let mut page = if responsive {
            menu_page::compute_responsive_layout(&COOP_SCORE_PAGE, width as u32, height as u32)
        } else {
            menu_page::compute_layout(&COOP_SCORE_PAGE, width as u32, height as u32)
        };
        let scale = page.scale;
        let (aw, ah) = if responsive {
            (800, 600)
        } else {
            (width, height)
        };
        page.title = scale.scale_rect(anchor_rect(
            LOW_HEADING_ANCHOR,
            RectPx::new(425, 1, 107, 10),
            aw,
            ah,
        ));
        page.status_help = scale.scale_rect(status_line_rect(RectPx::new(1, 356, 316, 12), aw, ah));
        let mut controls: Vec<(u16, RectPx)> = COOP_SCORE_CONTROLS
            .iter()
            .map(|spec| {
                let d = spec.dlu;
                let mut window = child_window(d.x, d.y, d.w, d.h);
                if NAMES.contains(&spec.id)
                    || COUNTRIES.contains(&spec.id)
                    || COLORS.contains(&spec.id)
                    || STARTS.contains(&spec.id)
                {
                    window.h = COMBO_FACE_H;
                }
                (spec.id, scale.scale_rect(window))
            })
            .collect();
        // 0x0060B1D0 moves the preview; it keeps its created window size.
        let created = child_window(432, 23, 96, 69);
        let mut preview = anchor_rect(
            AnchorRule::RightAnchor,
            RectPx::new(432, 23, 96, 69),
            aw,
            ah,
        );
        preview.w = created.w;
        preview.h = created.h;
        controls.push((PREVIEW, scale.scale_rect(preview)));
        controls.push((
            MODE_LABEL,
            scale.scale_rect(anchor_rect(
                MAP_TEXT_ANCHOR,
                RectPx::new(434, 103, 90, 10),
                aw,
                ah,
            )),
        ));
        controls.push((
            MAP_LABEL,
            scale.scale_rect(anchor_rect(
                MAP_TEXT_ANCHOR,
                RectPx::new(434, 116, 90, 20),
                aw,
                ah,
            )),
        ));
        Self { page, controls }
    }

    pub fn control_rect(&self, id: u16) -> Option<RectPx> {
        self.controls
            .iter()
            .find(|(candidate, _)| *candidate == id)
            .map(|(_, rect)| *rect)
            .or_else(|| self.page.button_rect(id))
    }

    /// The topmost control under the pointer, for hover help and presses.
    pub fn control_at(&self, x: i32, y: i32) -> Option<u16> {
        self.page
            .buttons
            .iter()
            .map(|button| button.id)
            .chain(self.controls.iter().map(|(id, _)| *id))
            .find(|id| {
                self.control_rect(*id)
                    .is_some_and(|rect| rect.contains(x, y))
            })
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

/// BC6's interaction state: the chat, the pressed button and focus.
#[derive(Debug, Clone)]
pub struct CoopScoreState {
    pub chat: LanChatState,
    pub buttons: ShellButtonInteraction<u16>,
}

impl Default for CoopScoreState {
    fn default() -> Self {
        let mut chat = LanChatState::default();
        chat.edit.focused = true;
        Self {
            chat,
            buttons: ShellButtonInteraction::default(),
        }
    }
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
    /// A won mission's carry-over record (`0x004C6180`), saved before the
    /// world retired.
    pub saved: Option<crate::sim::campaign_carry_over::CampaignCarryOver>,
}

#[cfg(test)]
mod tests {
    use super::*;

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

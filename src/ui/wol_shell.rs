//! Westwood Online welcome dialog `0x10E` (Main Menu -> Internet).
//!
//! `0xE2`'s Internet (`0x684`) writes 2; `Main__PrepareSession` state 2
//! (`0x0052DD57`) sets GameMode 4 and state 0x10 runs `WOL_Main`
//! `0x0077B2A0`, which draws the empty shell backdrop and runs `0x10E` with
//! proc `0x007988C0` (`0x00798DE0`; the variant `0x11C` only when the WOLAPI
//! `Options` registry bit 1 is set, which a retail install without WOL never
//! has). The page needs no network: its Main Menu leaves WOL (result 0,
//! `0x0052E28E` recreates `0xE2`), and every WOL action first creates the
//! WOLAPI COM object (`0x00786390`), which fails without the Westwood online
//! components and shows `TXT_APIMISSING` in the message box `0xD0`, then
//! returns to `0xE2` (-2).

use crate::ui::shell::descriptor::DialogId;
use crate::ui::shell::geom::{RectPx, dlu_rect};
use crate::ui::shell::layout::status_line_rect;
use crate::ui::shell::menu_page::{self, MenuPageButtonSpec, MenuPageLayout, MenuPageSpec};

pub const WOL_WELCOME_DIALOG: DialogId = DialogId(0x010E);
pub const QUICK_MATCH: u16 = 0x06E0;
pub const QUICK_COOP: u16 = 0x0771;
pub const CUSTOM_MATCH: u16 = 0x06E1;
pub const PLAY_BUDDY: u16 = 0x06E2;
pub const MY_INFORMATION: u16 = 0x06E4;
/// "Community": opens `HKLM\Software\Westwood\Yuri's Revenge\URL` `Community`
/// in a browser (`0x0077DC90`); without the key nothing happens.
pub const COMMUNITY: u16 = 0x055F;
pub const MAIN_MENU: u16 = 0x0686;

const fn button(
    id: u16,
    dlu_top: i32,
    csf_key: &'static str,
    tooltip_key: &'static str,
) -> MenuPageButtonSpec {
    MenuPageButtonSpec {
        id,
        dlu_top,
        csf_key,
        tooltip_key,
        result: None,
    }
}

/// RT_DIALOG `0x10E` as a family page: six top buttons (`0x00608CD0`, all
/// visible) snapped by their template tops, Main Menu on the bottom row
/// (`0x00609730`). Status help from `0x006040B0`.
pub const WOL_WELCOME_PAGE: MenuPageSpec = MenuPageSpec {
    dialog: WOL_WELCOME_DIALOG,
    title_key: "GUI:WOLWelcome",
    stacked: &[
        button(
            QUICK_MATCH,
            116,
            "GUI:QuickMatch",
            "STT:WOLWelcomeQuickMatch",
        ),
        button(QUICK_COOP, 144, "GUI:QuickCoop", "STT:QuickCoop"),
        button(
            CUSTOM_MATCH,
            172,
            "GUI:CustomMatch",
            "STT:WOLWelcomeCustomMatch",
        ),
        button(PLAY_BUDDY, 200, "GUI:PlayBuddy", "STT:WOLBuddy"),
        button(
            MY_INFORMATION,
            228,
            "GUI:MyInformation",
            "STT:WOLWelcomeMyInformation",
        ),
        button(
            COMMUNITY,
            256,
            "GUI:YuriWebSite",
            "STT:MainButtonYuriWebSite",
        ),
    ],
    back: MenuPageButtonSpec {
        id: MAIN_MENU,
        dlu_top: 284,
        csf_key: "GUI:MainMenu",
        tooltip_key: "STT:WOLWelcomeBack",
        result: Some(0),
    },
};

/// The buttons in template order, which is their Z-order: a hit test finds
/// the first. At 640x480 Community and Main Menu share the last row, and
/// Main Menu, created first, takes the pointer.
pub const WOL_BUTTON_Z_ORDER: [u16; 7] = [
    QUICK_MATCH,
    CUSTOM_MATCH,
    PLAY_BUDDY,
    MAIN_MENU,
    MY_INFORMATION,
    QUICK_COOP,
    COMMUNITY,
];

/// What a `0x10E` button leads to on a machine without WOLAPI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WolWelcomeAction {
    /// Quick Match, Quick Co-op, Custom Match, Buddy List and My Information
    /// (modes 1, 2, 4, 5, 6; `0x00798C39`): `0x10E` closes, the WOLAPI object
    /// cannot be created and `TXT_APIMISSING` shows (`0x00785711`).
    ApiMissing,
    /// Community: no registry URL, nothing happens and the page stays.
    Community,
    /// Main Menu (result 0): `0x10E` closes and `0xE2` is recreated.
    MainMenu,
}

pub fn action_for_control(id: u16) -> Option<WolWelcomeAction> {
    match id {
        QUICK_MATCH | QUICK_COOP | CUSTOM_MATCH | PLAY_BUDDY | MY_INFORMATION => {
            Some(WolWelcomeAction::ApiMissing)
        }
        COMMUNITY => Some(WolWelcomeAction::Community),
        MAIN_MENU => Some(WolWelcomeAction::MainMenu),
        _ => None,
    }
}

/// Text alignment of a kind-0 static (`SS_LEFT` / `SS_CENTER` / `SS_RIGHT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaticAlign {
    Left,
    Center,
    Right,
}

/// A text static of the left side: id (status help), caption key, template
/// rect and alignment. Kind 0: painted at once, without a reveal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WolTextStatic {
    pub id: Option<u16>,
    pub key: &'static str,
    pub dlu: (i32, i32, i32, i32),
    pub align: StaticAlign,
}

const fn text(
    id: Option<u16>,
    key: &'static str,
    dlu: (i32, i32, i32, i32),
    align: StaticAlign,
) -> WolTextStatic {
    WolTextStatic {
        id,
        key,
        dlu,
        align,
    }
}

/// The ladder statistics and the icon glossary. With no nickname the stats
/// fill (`0x007989AA`) is skipped, so the values keep `GUI:UnknownStats`;
/// `0x6EF` and `0x797` stay empty.
pub const WOL_TEXT_STATICS: [WolTextStatic; 19] = [
    text(
        None,
        "GUI:IconGlossary",
        (60, 167, 308, 13),
        StaticAlign::Center,
    ),
    text(
        None,
        "GUI:LadderWins",
        (87, 63, 115, 12),
        StaticAlign::Right,
    ),
    text(
        None,
        "GUI:LadderLosses",
        (87, 79, 115, 12),
        StaticAlign::Right,
    ),
    text(
        None,
        "GUI:Disconnects",
        (87, 95, 115, 12),
        StaticAlign::Right,
    ),
    text(
        None,
        "GUI:LadderRank",
        (87, 111, 115, 12),
        StaticAlign::Right,
    ),
    text(
        None,
        "GUI:LadderPoints",
        (87, 127, 115, 12),
        StaticAlign::Right,
    ),
    text(
        Some(0x06F0),
        "GUI:UnknownStats",
        (214, 63, 110, 12),
        StaticAlign::Left,
    ),
    text(
        Some(0x06F1),
        "GUI:UnknownStats",
        (214, 79, 110, 12),
        StaticAlign::Left,
    ),
    text(
        Some(0x0701),
        "GUI:UnknownStats",
        (214, 95, 110, 12),
        StaticAlign::Left,
    ),
    text(
        Some(0x06F4),
        "GUI:UnknownStats",
        (214, 111, 110, 12),
        StaticAlign::Left,
    ),
    text(
        Some(0x06F5),
        "GUI:UnknownStats",
        (214, 127, 110, 12),
        StaticAlign::Left,
    ),
    text(
        Some(0x0797),
        "GUI:Blank",
        (95, 143, 229, 12),
        StaticAlign::Center,
    ),
    text(
        Some(0x074C),
        "GUI:NormalGameIconDesc",
        (89, 200, 257, 12),
        StaticAlign::Left,
    ),
    text(
        Some(0x074E),
        "GUI:ClanGameIconDesc",
        (89, 215, 257, 12),
        StaticAlign::Left,
    ),
    text(
        Some(0x074F),
        "GUI:ResolutionLimitIconDesc",
        (89, 230, 257, 12),
        StaticAlign::Left,
    ),
    text(
        Some(0x0750),
        "GUI:ObserverAllowedIconDesc",
        (89, 245, 257, 12),
        StaticAlign::Left,
    ),
    text(
        Some(0x0753),
        "GUI:PasswordIconDesc",
        (89, 260, 257, 12),
        StaticAlign::Left,
    ),
    text(
        None,
        "GUI:LatencyIconDesc",
        (103, 275, 243, 12),
        StaticAlign::Left,
    ),
    text(
        None,
        "GUI:BadgesOfHonor",
        (113, 290, 221, 12),
        StaticAlign::Left,
    ),
];

/// A kind-2 image static the proc's `0x497` sets (`0x00603D30`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WolIcon {
    pub id: u16,
    pub dlu: (i32, i32, i32, i32),
    pub file: &'static str,
}

const fn icon(id: u16, dlu: (i32, i32, i32, i32), file: &'static str) -> WolIcon {
    WolIcon { id, dlu, file }
}

/// The glossary icons: template rect and PCX. `0x79F` is shifted one pixel
/// right by `0x0060C002`.
pub const WOL_ICONS: [WolIcon; 13] = [
    icon(0x0746, (67, 200, 18, 12), "gt18.pcx"),
    icon(0x0748, (67, 215, 18, 12), "wolclan.pcx"),
    icon(0x0749, (67, 230, 18, 12), "wolreslk.pcx"),
    icon(0x074A, (67, 245, 18, 12), "wolob.pcx"),
    icon(0x0752, (67, 260, 18, 12), "wolpriv.pcx"),
    icon(0x075C, (67, 275, 10, 12), "pingg.pcx"),
    icon(0x075D, (79, 275, 10, 12), "pingy.pcx"),
    icon(0x075E, (91, 275, 10, 12), "pingr.pcx"),
    icon(0x079E, (67, 290, 6, 12), "number1.pcx"),
    icon(0x079F, (72, 290, 6, 12), "number0.pcx"),
    icon(0x07A0, (77, 290, 6, 12), "number0.pcx"),
    icon(0x07A1, (84, 290, 12, 12), "cooperat.pcx"),
    icon(0x07A2, (96, 290, 12, 12), "comchief.pcx"),
];

/// Status help of the stats values and `0x797` (`0x006040B0`).
fn static_help(id: u16) -> Option<&'static str> {
    Some(match id {
        0x06F0 => "STT:WOLMyInformationWins",
        0x06F1 => "STT:WOLMyInformationLosses",
        0x0701 => "STT:WOLMyInformationDisconnects",
        0x06F4 => "STT:WOLMyInformationRank",
        0x06F5 => "STT:WOLMyInformationPoints",
        0x0797 => "STT:WelcomeUpdate",
        _ => return None,
    })
}

/// A template child window: the 6x13 conversion one pixel wider and taller,
/// as every family child measures (`0x0060C42E` keeps the template place).
fn child_window((x, y, w, h): (i32, i32, i32, i32)) -> RectPx {
    let rect = dlu_rect(x, y, w, h);
    RectPx::new(rect.x, rect.y, rect.w + 1, rect.h + 1)
}

/// The two group boxes (`BS_GROUPBOX`): ladder stats and icon glossary.
pub const WOL_GROUP_BOXES: [(i32, i32, i32, i32); 2] = [(81, 52, 255, 106), (61, 186, 289, 126)];

/// `0x10E` at the current screen: the family right panel, and the left-side
/// children at their template places (`0x0060C0C0` keeps them at every size).
#[derive(Debug, Clone)]
pub struct WolWelcomeLayout {
    pub page: MenuPageLayout,
    /// Status line `0x695`: the template is 316 DLU wide here.
    pub status_help: RectPx,
    pub texts: Vec<(WolTextStatic, RectPx)>,
    pub icons: Vec<(&'static str, RectPx)>,
    pub group_boxes: [RectPx; 2],
}

pub fn compute_layout(screen_w: u32, screen_h: u32) -> WolWelcomeLayout {
    let page = menu_page::compute_layout(&WOL_WELCOME_PAGE, screen_w, screen_h);
    let status_help = status_line_rect(
        RectPx::new(2, 355, 316, 12),
        screen_w as i32,
        screen_h as i32,
    );
    let texts = WOL_TEXT_STATICS
        .iter()
        .map(|text| (*text, child_window(text.dlu)))
        .collect();
    let icons = WOL_ICONS
        .iter()
        .map(|icon| {
            let mut window = child_window(icon.dlu);
            if icon.id == 0x079F {
                window.x += 1;
            }
            (icon.file, window)
        })
        .collect();
    WolWelcomeLayout {
        page,
        status_help,
        texts,
        icons,
        group_boxes: WOL_GROUP_BOXES.map(child_window),
    }
}

impl WolWelcomeLayout {
    /// The page's buttons in hit-test order.
    pub fn buttons_in_z_order(&self) -> Vec<crate::ui::shell::menu_page::MenuPageButtonRect> {
        WOL_BUTTON_Z_ORDER
            .iter()
            .filter_map(|id| self.page.buttons.iter().find(|button| button.id == *id))
            .copied()
            .collect()
    }

    /// Status help of the child under `(x, y)`: the buttons' keys, then the
    /// stats values and `0x797`; other children have none. The statics come
    /// before the group boxes in the template, so they are hit first.
    pub fn status_help_key(&self, x: i32, y: i32) -> Option<&'static str> {
        if let Some(button) = self
            .buttons_in_z_order()
            .into_iter()
            .find(|button| button.rect.contains(x, y))
        {
            return WOL_WELCOME_PAGE
                .button(button.id)
                .map(|spec| spec.tooltip_key);
        }
        self.texts
            .iter()
            .find(|(_, rect)| rect.contains(x, y))
            .and_then(|(text, _)| text.id.and_then(static_help))
    }
}

/// The `TXT_APIMISSING` message box `0xD0`: one OK button, created modal
/// (mode 2, no slide) over the empty shell backdrop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiMissingBox {
    pub body: String,
    pub ok: String,
}

/// One visit to Westwood Online: the shuffle flag `WOL_Main` saved on entry
/// (`0x0077B30C`), the status help the last hit test wrote, and the message
/// box once a WOL action has failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WolWelcomeState {
    pub saved_shuffle: bool,
    pub status_help: Option<&'static str>,
    pub api_missing: Option<ApiMissingBox>,
}

impl WolWelcomeState {
    pub fn open(saved_shuffle: bool) -> Self {
        Self {
            saved_shuffle,
            status_help: None,
            api_missing: None,
        }
    }
}

/// A kind-2 image static centres its image only along an axis where the
/// window is larger (`0x00615831..0x006158F3`).
pub fn centred_image_origin(window: RectPx, image_w: i32, image_h: i32) -> (i32, i32) {
    let dx = if window.w > image_w {
        (window.w - image_w) / 2
    } else {
        0
    };
    let dy = if window.h > image_h {
        (window.h - image_h) / 2
    } else {
        0
    };
    (window.x + dx, window.y + dy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn right_panel_matches_the_executed_relayout() {
        let layout = compute_layout(800, 600);
        let rows: Vec<(u16, RectPx)> = layout
            .page
            .buttons
            .iter()
            .map(|button| (button.id, button.rect))
            .collect();
        assert_eq!(
            rows,
            [
                (QUICK_MATCH, RectPx::new(644, 199, 156, 42)),
                (QUICK_COOP, RectPx::new(644, 241, 156, 42)),
                (CUSTOM_MATCH, RectPx::new(644, 283, 156, 42)),
                (PLAY_BUDDY, RectPx::new(644, 325, 156, 42)),
                (MY_INFORMATION, RectPx::new(644, 367, 156, 42)),
                (COMMUNITY, RectPx::new(644, 409, 156, 42)),
                (MAIN_MENU, RectPx::new(644, 535, 156, 42)),
            ]
        );
        assert_eq!(layout.status_help, RectPx::new(10, 578, 475, 21));
    }

    #[test]
    fn left_children_keep_their_template_places() {
        let layout = compute_layout(800, 600);
        assert_eq!(layout.group_boxes[0], RectPx::new(122, 85, 384, 173));
        assert_eq!(layout.group_boxes[1], RectPx::new(92, 302, 435, 206));
        let wins = layout
            .texts
            .iter()
            .find(|(text, _)| text.key == "GUI:LadderWins")
            .unwrap();
        assert_eq!(wins.1, RectPx::new(131, 102, 174, 21));
        // The executed kind-2 placement: gt18 at (108, 328), the second
        // number0 (0x79F, +1 x) at (110, 473).
        let (_, gt18) = layout.icons[0];
        assert_eq!(centred_image_origin(gt18, 14, 14), (108, 328));
        let (_, number0) = layout.icons[9];
        assert_eq!(centred_image_origin(number0, 8, 16), (110, 473));
        // 1024x768 leaves them at their 800x600 places.
        let wide = compute_layout(1024, 768);
        assert_eq!(wide.group_boxes, layout.group_boxes);
    }

    #[test]
    fn status_help_covers_buttons_and_stats_values() {
        let layout = compute_layout(800, 600);
        assert_eq!(
            layout.status_help_key(720, 220),
            Some("STT:WOLWelcomeQuickMatch")
        );
        assert_eq!(layout.status_help_key(720, 556), Some("STT:WOLWelcomeBack"));
        assert_eq!(
            layout.status_help_key(330, 110),
            Some("STT:WOLMyInformationWins")
        );
        assert_eq!(
            layout.status_help_key(200, 110),
            None,
            "the label has no id"
        );
        assert_eq!(layout.status_help_key(400, 300), None);
    }

    #[test]
    fn main_menu_takes_the_shared_last_row_at_640() {
        let layout = compute_layout(640, 480);
        let community = layout.page.button_rect(COMMUNITY).unwrap();
        assert_eq!(layout.page.button_rect(MAIN_MENU), Some(community));
        let first = layout
            .buttons_in_z_order()
            .into_iter()
            .find(|button| button.rect.contains(community.x + 5, community.y + 5))
            .unwrap();
        assert_eq!(first.id, MAIN_MENU);
    }

    #[test]
    fn wol_actions_need_the_missing_api_except_community_and_main_menu() {
        for id in [
            QUICK_MATCH,
            QUICK_COOP,
            CUSTOM_MATCH,
            PLAY_BUDDY,
            MY_INFORMATION,
        ] {
            assert_eq!(action_for_control(id), Some(WolWelcomeAction::ApiMissing));
        }
        assert_eq!(
            action_for_control(COMMUNITY),
            Some(WolWelcomeAction::Community)
        );
        assert_eq!(
            action_for_control(MAIN_MENU),
            Some(WolWelcomeAction::MainMenu)
        );
    }
}

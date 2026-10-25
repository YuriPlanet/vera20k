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
use crate::ui::shell::geom::{
    RectPx, SHELL_BASE_H, SHELL_BASE_W, ShellScale, child_window, dlu_rect,
};
use crate::ui::shell::layout::status_line_rect;
use crate::ui::shell::menu_page::{
    self, MenuPageButtonRect, MenuPageButtonSpec, MenuPageLayout, MenuPageSpec,
};

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
/// The custom lobby `0x113`'s heading `0x694` (`GUI:CustomMatch`).
pub const WOL_LOBBY_TITLE_KEY: &str = "GUI:CustomMatch";

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

/// The live service currently offers custom games. Keep the other entry points
/// on the legacy page until their online flows are ready.
pub const WOL_ONLINE_WELCOME_PAGE: MenuPageSpec = MenuPageSpec {
    stacked: &[button(
        CUSTOM_MATCH,
        116,
        "GUI:CustomMatch",
        "STT:WOLWelcomeCustomMatch",
    )],
    ..WOL_WELCOME_PAGE
};

/// The buttons in template order, which is their Z-order: a hit test finds
/// the first. At 640x480 Community and Main Menu share the last row: Main
/// Menu, created first, takes the pointer, and Community paints over it.
pub const WOL_BUTTON_Z_ORDER: [u16; 7] = [
    QUICK_MATCH,
    CUSTOM_MATCH,
    PLAY_BUDDY,
    MAIN_MENU,
    MY_INFORMATION,
    QUICK_COOP,
    COMMUNITY,
];

/// The WOL mode `[0xA8B244]` a `0x10E` button writes (`0x007988C0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WolMode {
    QuickMatch,
    QuickCoop,
    CustomMatch,
    PlayBuddy,
    MyInformation,
}

impl WolMode {
    /// The native value of `[0xA8B244]`.
    pub fn native(self) -> i32 {
        match self {
            Self::QuickMatch => 1,
            Self::QuickCoop => 2,
            Self::CustomMatch => 4,
            Self::PlayBuddy => 5,
            Self::MyInformation => 6,
        }
    }

    /// `WOL_RunMain`'s login category for the mode (`0x0077B3AA..0x0077B3C8`):
    /// 0 for My Information (it ensures the API itself), 1 otherwise here
    /// (2 belongs to mode 3, which `0x10E` does not offer).
    pub fn needs_login(self) -> bool {
        self != Self::MyInformation
    }
}

/// What a `0x10E` button does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WolWelcomeAction {
    /// Quick Match, Quick Co-op, Custom Match, Buddy List and My Information
    /// (`0x00798C39`): the mode is written and `0x10E` closes; the WOL flow
    /// then first creates the WOLAPI object (`0x00786390`).
    Mode(WolMode),
    /// Community: no registry URL, nothing happens and the page stays.
    Community,
    /// Main Menu (result 0): `0x10E` closes and `0xE2` is recreated.
    MainMenu,
}

pub fn action_for_control(id: u16) -> Option<WolWelcomeAction> {
    Some(match id {
        QUICK_MATCH => WolWelcomeAction::Mode(WolMode::QuickMatch),
        QUICK_COOP => WolWelcomeAction::Mode(WolMode::QuickCoop),
        CUSTOM_MATCH => WolWelcomeAction::Mode(WolMode::CustomMatch),
        PLAY_BUDDY => WolWelcomeAction::Mode(WolMode::PlayBuddy),
        MY_INFORMATION => WolWelcomeAction::Mode(WolMode::MyInformation),
        COMMUNITY => WolWelcomeAction::Community,
        MAIN_MENU => WolWelcomeAction::MainMenu,
        _ => return None,
    })
}

/// Text alignment of a kind-0 static (`SS_LEFT` / `SS_CENTER` / `SS_RIGHT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaticAlign {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WolTextCaption {
    Csf(&'static str),
    Literal(&'static str),
}

use WolTextCaption::{Csf, Literal};

/// A text static of the left side: id (status help), caption, template
/// rect and alignment. Kind 0: painted at once, without a reveal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WolTextStatic {
    pub id: Option<u16>,
    pub caption: WolTextCaption,
    pub dlu: (i32, i32, i32, i32),
    pub align: StaticAlign,
}

const fn text(
    id: Option<u16>,
    caption: WolTextCaption,
    dlu: (i32, i32, i32, i32),
    align: StaticAlign,
) -> WolTextStatic {
    WolTextStatic {
        id,
        caption,
        dlu,
        align,
    }
}

/// The ladder statistics and the icon glossary at their template captions.
/// With a nickname, init fills the heading `0x6EF`, the five values and
/// `0x797` from the persona (`0x007989AA` -> `0x0077A9F0`); without one the
/// values keep `GUI:UnknownStats` and `0x6EF` and `0x797` stay empty.
pub const WOL_TEXT_STATICS: [WolTextStatic; 20] = [
    // The persona heading: centred (`0x50020001`), empty caption.
    text(
        Some(0x06EF),
        Csf(""),
        (86, 41, 254, 12),
        StaticAlign::Center,
    ),
    text(
        None,
        Csf("GUI:IconGlossary"),
        (60, 167, 308, 13),
        StaticAlign::Center,
    ),
    text(
        None,
        Csf("GUI:LadderWins"),
        (87, 63, 115, 12),
        StaticAlign::Right,
    ),
    text(
        None,
        Csf("GUI:LadderLosses"),
        (87, 79, 115, 12),
        StaticAlign::Right,
    ),
    text(
        None,
        Csf("GUI:Disconnects"),
        (87, 95, 115, 12),
        StaticAlign::Right,
    ),
    text(
        None,
        Csf("GUI:LadderRank"),
        (87, 111, 115, 12),
        StaticAlign::Right,
    ),
    text(
        None,
        Csf("GUI:LadderPoints"),
        (87, 127, 115, 12),
        StaticAlign::Right,
    ),
    text(
        Some(0x06F0),
        Csf("GUI:UnknownStats"),
        (214, 63, 110, 12),
        StaticAlign::Left,
    ),
    text(
        Some(0x06F1),
        Csf("GUI:UnknownStats"),
        (214, 79, 110, 12),
        StaticAlign::Left,
    ),
    text(
        Some(0x0701),
        Csf("GUI:UnknownStats"),
        (214, 95, 110, 12),
        StaticAlign::Left,
    ),
    text(
        Some(0x06F4),
        Csf("GUI:UnknownStats"),
        (214, 111, 110, 12),
        StaticAlign::Left,
    ),
    text(
        Some(0x06F5),
        Csf("GUI:UnknownStats"),
        (214, 127, 110, 12),
        StaticAlign::Left,
    ),
    text(
        Some(0x0797),
        Csf("GUI:Blank"),
        (95, 143, 229, 12),
        StaticAlign::Center,
    ),
    text(
        Some(0x074C),
        Csf("GUI:NormalGameIconDesc"),
        (89, 200, 257, 12),
        StaticAlign::Left,
    ),
    text(
        Some(0x074E),
        Csf("GUI:ClanGameIconDesc"),
        (89, 215, 257, 12),
        StaticAlign::Left,
    ),
    text(
        Some(0x074F),
        Csf("GUI:ResolutionLimitIconDesc"),
        (89, 230, 257, 12),
        StaticAlign::Left,
    ),
    text(
        Some(0x0750),
        Csf("GUI:ObserverAllowedIconDesc"),
        (89, 245, 257, 12),
        StaticAlign::Left,
    ),
    text(
        Some(0x0753),
        Csf("GUI:PasswordIconDesc"),
        (89, 260, 257, 12),
        StaticAlign::Left,
    ),
    text(
        None,
        Csf("GUI:LatencyIconDesc"),
        (103, 275, 243, 12),
        StaticAlign::Left,
    ),
    text(
        None,
        Csf("GUI:BadgesOfHonor"),
        (113, 290, 221, 12),
        StaticAlign::Left,
    ),
];

/// VERA's ids for the online welcome's lobby counts (`GET /v1/stats`,
/// `services/lobby/protocol.md`); no native control uses them.
const LOBBY_STATS_HEADING: u16 = 0x7F00;
const LOBBY_STATS_BODY: u16 = 0x7F01;
pub const LOBBY_STATS_IDS: [u16; 2] = [LOBBY_STATS_HEADING, LOBBY_STATS_BODY];

/// The lobby's public counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
pub struct WolLobbyStats {
    pub online: u32,
    pub in_game: u32,
    pub open_games: u32,
    pub games_24h: u32,
    pub games_total: u32,
}

impl WolLobbyStats {
    /// The texts of [`LOBBY_STATS_IDS`]; they stay blank until counts arrive.
    pub fn texts(self) -> [(u16, String); 2] {
        let count =
            |n: u32, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
        [
            (LOBBY_STATS_HEADING, "RIGHT NOW".to_owned()),
            (
                LOBBY_STATS_BODY,
                format!(
                    "{} online, {} in games, {}.\n{} in the last 24 hours, {} in all.",
                    count(self.online, "player", "players"),
                    self.in_game,
                    count(self.open_games, "open game", "open games"),
                    count(self.games_24h, "game", "games"),
                    self.games_total,
                ),
            ),
        ]
    }
}

const ONLINE_WELCOME_TEXT: [WolTextStatic; 8] = [
    text(
        None,
        Literal("VERA20k Online"),
        (42, 50, 330, 18),
        StaticAlign::Left,
    ),
    text(
        None,
        Literal(
            "Custom games for VERA20k players.\nMeet in German Bunker to create or join a game.",
        ),
        (42, 80, 330, 34),
        StaticAlign::Left,
    ),
    text(
        None,
        Literal("GET STARTED"),
        (42, 134, 330, 12),
        StaticAlign::Left,
    ),
    text(
        None,
        Literal(
            "Select Custom Match, enter a username and press OK.\nUse Create Game, or select an available game and Join.",
        ),
        (42, 152, 330, 36),
        StaticAlign::Left,
    ),
    text(
        None,
        Literal("PLAY AGAINST AI"),
        (42, 206, 330, 12),
        StaticAlign::Left,
    ),
    text(
        None,
        Literal(
            "Create a game and choose an AI difficulty in an open\nplayer slot before pressing Start.",
        ),
        (42, 224, 330, 36),
        StaticAlign::Left,
    ),
    text(
        Some(LOBBY_STATS_HEADING),
        Literal(""),
        (42, 278, 330, 12),
        StaticAlign::Left,
    ),
    text(
        Some(LOBBY_STATS_BODY),
        Literal(""),
        (42, 296, 330, 36),
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

/// The two group boxes (`BS_GROUPBOX`): ladder stats and icon glossary.
pub const WOL_GROUP_BOXES: [(i32, i32, i32, i32); 2] = [(81, 52, 255, 106), (61, 186, 289, 126)];

/// `0x10E` at the current screen: the family right panel, and the left-side
/// children at their template places (`0x0060C0C0` keeps them at every size).
#[derive(Debug, Clone)]
pub struct WolWelcomeLayout {
    spec: &'static MenuPageSpec,
    pub page: MenuPageLayout,
    /// Status line `0x695`: the template is 316 DLU wide here.
    pub status_help: RectPx,
    pub texts: Vec<(WolTextStatic, RectPx)>,
    pub icons: Vec<(&'static str, RectPx)>,
    pub group_boxes: Vec<RectPx>,
}

pub fn compute_layout(
    screen_w: u32,
    screen_h: u32,
    spec: &'static MenuPageSpec,
) -> WolWelcomeLayout {
    let mut page = menu_page::compute_layout(spec, screen_w, screen_h);
    // Account statistics and the old icon glossary belong to the legacy
    // information entry, which the normal online page does not offer.
    let information = spec.button(MY_INFORMATION).is_some();
    // The heading template is 108x11 DLU here (the family's is 108x10): the
    // relayout keeps the family place and the window is one row taller.
    page.title.h = dlu_rect(423, 4, 108, 11).h + 1;
    let status_help = status_line_rect(
        RectPx::new(2, 355, 316, 12),
        screen_w as i32,
        screen_h as i32,
    );
    let statics = if information {
        &WOL_TEXT_STATICS[..]
    } else {
        &ONLINE_WELCOME_TEXT[..]
    };
    let texts = statics
        .iter()
        .map(|text| {
            let (x, y, w, h) = text.dlu;
            let mut rect = child_window(x, y, w, h);
            if !information {
                rect.w = rect.w.min((page.right_panel.top.x - rect.x - 24).max(0));
            }
            (*text, rect)
        })
        .collect();
    let icons = WOL_ICONS
        .iter()
        .filter(|_| information)
        .map(|icon| {
            let (x, y, w, h) = icon.dlu;
            let mut window = child_window(x, y, w, h);
            if icon.id == 0x079F {
                window.x += 1;
            }
            (icon.file, window)
        })
        .collect();
    WolWelcomeLayout {
        spec,
        page,
        status_help,
        texts,
        icons,
        group_boxes: WOL_GROUP_BOXES
            .into_iter()
            .filter(|_| information)
            .map(|(x, y, w, h)| child_window(x, y, w, h))
            .collect(),
    }
}

/// The responsive variant of [`compute_layout`]: the verified 800x600 `0x10E`
/// scaled to fit the window, keeping its shape. Identity at the base size.
pub fn compute_responsive_layout(
    screen_w: u32,
    screen_h: u32,
    spec: &'static MenuPageSpec,
) -> WolWelcomeLayout {
    let scale = ShellScale::from_screen(screen_w as i32, screen_h as i32);
    let base = compute_layout(SHELL_BASE_W as u32, SHELL_BASE_H as u32, spec);
    let mut page = menu_page::compute_responsive_layout(spec, screen_w, screen_h);
    // The heading override is authored 11 DLU tall, so it stretches from the
    // overridden base rect rather than the family's 10.
    page.title = scale.scale_rect(base.page.title);
    WolWelcomeLayout {
        spec,
        page,
        status_help: scale.scale_rect(base.status_help),
        texts: base
            .texts
            .iter()
            .map(|(text, window)| (*text, scale.scale_rect(*window)))
            .collect(),
        icons: base
            .icons
            .iter()
            .map(|(file, window)| (*file, scale.scale_rect(*window)))
            .collect(),
        group_boxes: base
            .group_boxes
            .into_iter()
            .map(|window| scale.scale_rect(window))
            .collect(),
    }
}

impl WolWelcomeLayout {
    pub fn page_spec(&self) -> &'static MenuPageSpec {
        self.spec
    }

    /// The page's buttons in hit-test order.
    pub fn buttons_in_z_order(&self) -> Vec<MenuPageButtonRect> {
        WOL_BUTTON_Z_ORDER
            .iter()
            .filter_map(|id| self.page.buttons.iter().find(|button| button.id == *id))
            .copied()
            .collect()
    }

    /// The buttons whose faces show. Siblings do not clip each other and
    /// paint down the Z-order, so where two share a row (Community and Main
    /// Menu at 640x480) the later one covers the earlier, unless the earlier
    /// is pressed and repaints alone (inferred from Win32 paint order;
    /// uncaptured).
    pub fn painted_buttons(&self, pressed: Option<u16>) -> Vec<MenuPageButtonRect> {
        let order = self.buttons_in_z_order();
        let shows = |index: usize, button: &MenuPageButtonRect| {
            if pressed == Some(button.id) {
                return true;
            }
            let row_pressed = order
                .iter()
                .any(|other| other.rect == button.rect && pressed == Some(other.id));
            !row_pressed
                && !order[index + 1..]
                    .iter()
                    .any(|other| other.rect == button.rect)
        };
        order
            .iter()
            .enumerate()
            .filter(|(index, button)| shows(*index, button))
            .map(|(_, button)| *button)
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
            return self.spec.button(button.id).map(|spec| spec.tooltip_key);
        }
        self.texts
            .iter()
            .find(|(_, rect)| rect.contains(x, y))
            .and_then(|(text, _)| text.id.and_then(static_help))
    }
}

/// A WOL message box from `0x005E2CD0`, modal (mode 2, no slide) over the
/// empty shell backdrop: style 0 is `0xD0` with OK (id 1); style 4 is
/// `0x126` with Yes (6) and No (7). Its loop (`0x007759E0`) dispatches
/// without `IsDialogMessage`, so it takes no keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WolMessageBox {
    pub body: String,
    pub buttons: WolBoxButtons,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WolBoxButtons {
    Ok(String),
    YesNo { yes: String, no: String },
}

impl WolMessageBox {
    pub fn dialog(&self) -> DialogId {
        DialogId(match self.buttons {
            WolBoxButtons::Ok(_) => 0x00D0,
            WolBoxButtons::YesNo { .. } => 0x0126,
        })
    }
}

/// `0xC4` OK button id 1, `0x126` Yes 6 / No 7.
pub const BOX_OK: u16 = 1;
pub const BOX_YES: u16 = 6;
pub const BOX_NO: u16 = 7;
/// The wait box `0xC4`'s Disconnect and WOL Check Status.
pub const WAIT_DISCONNECT: u16 = 0x058D;
pub const WAIT_CHECK_STATUS: u16 = 0x058E;
pub const WAIT_DIALOG: DialogId = DialogId(0x00C4);

/// The wait box `0xC4` (`0x00792BE0`, proc `0x00792CF0`): created hidden
/// (fShow 0); its init arms timer `0x45` for `[0x84A248]` = 7000 ms, whose
/// tick shows it and kills the timer. A result before then never shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WolWaitBox {
    /// `0x5B2`'s text, set by `0x4B2` after creation.
    pub text: String,
    pub disconnect: String,
    pub check_status: String,
    pub opened_at: std::time::Instant,
    pub shown: bool,
}

/// `[0x84A248]`, read only at `0x00792D93`.
pub const WAIT_REVEAL: std::time::Duration = std::time::Duration::from_millis(7000);

impl WolWaitBox {
    pub fn reveal_at(&self) -> std::time::Instant {
        self.opened_at + WAIT_REVEAL
    }

    /// The timer tick: true when this call showed the box.
    pub fn poll(&mut self, now: std::time::Instant) -> bool {
        if !self.shown && now >= self.reveal_at() {
            self.shown = true;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::shell::geom::centred_in_window;

    #[test]
    fn welcome_buttons_write_the_native_modes() {
        let modes: Vec<i32> = [
            QUICK_MATCH,
            QUICK_COOP,
            CUSTOM_MATCH,
            PLAY_BUDDY,
            MY_INFORMATION,
        ]
        .into_iter()
        .map(|id| match action_for_control(id) {
            Some(WolWelcomeAction::Mode(mode)) => mode.native(),
            other => panic!("{id:#x}: {other:?}"),
        })
        .collect();
        assert_eq!(modes, [1, 2, 4, 5, 6]);
        assert!(!WolMode::MyInformation.needs_login());
        assert!(WolMode::CustomMatch.needs_login());
        assert_eq!(
            action_for_control(COMMUNITY),
            Some(WolWelcomeAction::Community)
        );
        assert_eq!(
            action_for_control(MAIN_MENU),
            Some(WolWelcomeAction::MainMenu)
        );
    }

    #[test]
    fn online_welcome_only_paints_and_hits_custom_match_and_main_menu() {
        let layout = compute_layout(800, 600, &WOL_ONLINE_WELCOME_PAGE);
        let controls: Vec<_> = layout
            .painted_buttons(None)
            .iter()
            .map(|button| button.id)
            .collect();
        assert_eq!(controls, [CUSTOM_MATCH, MAIN_MENU]);
        assert!(!layout.texts.is_empty());
        assert!(layout.icons.is_empty() && layout.group_boxes.is_empty());
        assert!(layout.texts.iter().all(|(text, _)| {
            matches!(text.caption, Literal(_))
                && text.id.is_none_or(|id| LOBBY_STATS_IDS.contains(&id))
        }));
        let first = layout.page.button_rect(CUSTOM_MATCH).unwrap();
        assert_eq!(first, RectPx::new(644, 199, 156, 42));
        assert_eq!(
            layout.status_help_key(first.x + 5, first.y + 5),
            Some("STT:WOLWelcomeCustomMatch")
        );
        let legacy = compute_layout(800, 600, &WOL_WELCOME_PAGE);
        for control in [
            QUICK_MATCH,
            QUICK_COOP,
            PLAY_BUDDY,
            MY_INFORMATION,
            COMMUNITY,
        ] {
            assert_eq!(layout.page.button_rect(control), None);
        }
        let old_buddy = legacy.page.button_rect(PLAY_BUDDY).unwrap();
        assert!(
            !layout
                .buttons_in_z_order()
                .iter()
                .any(|button| { button.rect.contains(old_buddy.x + 5, old_buddy.y + 5) })
        );
        assert_eq!(
            layout.status_help_key(old_buddy.x + 5, old_buddy.y + 5),
            None
        );
        let scaled = compute_responsive_layout(1600, 1200, &WOL_ONLINE_WELCOME_PAGE);
        assert_eq!(scaled.painted_buttons(None).len(), 2);
        assert!(!scaled.texts.is_empty());
        assert!(scaled.icons.is_empty() && scaled.group_boxes.is_empty());
        assert_eq!(
            scaled.page.button_rect(CUSTOM_MATCH),
            Some(RectPx::new(1288, 398, 312, 84))
        );
    }

    #[test]
    fn lobby_stats_read_as_counted_phrases() {
        let stats = WolLobbyStats {
            online: 1,
            in_game: 0,
            open_games: 2,
            games_24h: 1,
            games_total: 40,
        };
        assert_eq!(
            stats.texts()[1].1,
            "1 player online, 0 in games, 2 open games.\n1 game in the last 24 hours, 40 in all."
        );
    }

    #[test]
    fn wait_box_shows_once_its_timer_fires() {
        let opened_at = std::time::Instant::now();
        let mut wait = WolWaitBox {
            text: String::new(),
            disconnect: String::new(),
            check_status: String::new(),
            opened_at,
            shown: false,
        };
        assert!(!wait.poll(opened_at + WAIT_REVEAL - std::time::Duration::from_millis(1)));
        assert!(wait.poll(opened_at + WAIT_REVEAL));
        assert!(wait.shown);
        assert!(!wait.poll(opened_at + WAIT_REVEAL * 2));
    }

    #[test]
    fn right_panel_matches_the_executed_relayout() {
        let layout = compute_layout(800, 600, &WOL_WELCOME_PAGE);
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
        // The executed relayout of heading 0x694 (dlu 423,4,108,11).
        assert_eq!(layout.page.title, RectPx::new(635, 9, 163, 19));
        assert_eq!(
            compute_layout(640, 480, &WOL_WELCOME_PAGE).page.title,
            RectPx::new(475, 9, 163, 19)
        );
        assert_eq!(
            compute_layout(1024, 768, &WOL_WELCOME_PAGE).page.title,
            RectPx::new(747, 93, 163, 19)
        );
    }

    #[test]
    fn left_children_keep_their_template_places() {
        let layout = compute_layout(800, 600, &WOL_WELCOME_PAGE);
        assert_eq!(layout.group_boxes[0], RectPx::new(122, 85, 384, 173));
        assert_eq!(layout.group_boxes[1], RectPx::new(92, 302, 435, 206));
        let wins = layout
            .texts
            .iter()
            .find(|(text, _)| text.caption == Csf("GUI:LadderWins"))
            .unwrap();
        assert_eq!(wins.1, RectPx::new(131, 102, 174, 21));
        // The executed kind-2 placement: gt18 at (108, 328), the second
        // number0 (0x79F, +1 x) at (110, 473).
        let (_, gt18) = layout.icons[0];
        assert_eq!(centred_in_window(gt18, 14, 14), (108, 328));
        let (_, number0) = layout.icons[9];
        assert_eq!(centred_in_window(number0, 8, 16), (110, 473));
        // 1024x768 leaves them at their 800x600 places.
        let wide = compute_layout(1024, 768, &WOL_WELCOME_PAGE);
        assert_eq!(wide.group_boxes, layout.group_boxes);
    }

    #[test]
    fn responsive_layout_stretches_the_page_and_the_left_side() {
        let base = compute_layout(800, 600, &WOL_WELCOME_PAGE);
        // Identity at the authored size, where the retail layout is used.
        let same = compute_responsive_layout(800, 600, &WOL_WELCOME_PAGE);
        assert_eq!(same.page.title, base.page.title);
        assert_eq!(same.status_help, base.status_help);
        assert_eq!(same.group_boxes, base.group_boxes);
        let scale = ShellScale::from_screen(1600, 1200);
        let scaled = compute_responsive_layout(1600, 1200, &WOL_WELCOME_PAGE);
        assert_eq!(scaled.page.scale, scale);
        assert_eq!(scaled.page.title, RectPx::new(1270, 18, 326, 38));
        assert_eq!(scaled.status_help, RectPx::new(20, 1156, 950, 42));
        assert_eq!(scaled.group_boxes[0], RectPx::new(244, 170, 768, 346));
        assert_eq!(scaled.group_boxes[1], RectPx::new(184, 604, 870, 412));
        assert_eq!(
            base.texts
                .iter()
                .map(|(text, window)| (*text, scale.scale_rect(*window)))
                .collect::<Vec<_>>(),
            scaled.texts
        );
        assert_eq!(
            base.icons
                .iter()
                .map(|(file, window)| (*file, scale.scale_rect(*window)))
                .collect::<Vec<_>>(),
            scaled.icons
        );
        let wins = scaled
            .texts
            .iter()
            .find(|(text, _)| text.caption == Csf("GUI:LadderWins"))
            .unwrap();
        assert_eq!(wins.1, RectPx::new(262, 204, 348, 42));
        assert_eq!(
            scaled.page.button_rect(MAIN_MENU),
            Some(RectPx::new(1288, 1070, 312, 84))
        );
        assert_eq!(scaled.texts.len(), base.texts.len());
        assert_eq!(scaled.icons.len(), base.icons.len());
    }

    #[test]
    fn status_help_covers_buttons_and_stats_values() {
        let layout = compute_layout(800, 600, &WOL_WELCOME_PAGE);
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
        let layout = compute_layout(640, 480, &WOL_WELCOME_PAGE);
        let community = layout.page.button_rect(COMMUNITY).unwrap();
        assert_eq!(layout.page.button_rect(MAIN_MENU), Some(community));
        let first = layout
            .buttons_in_z_order()
            .into_iter()
            .find(|button| button.rect.contains(community.x + 5, community.y + 5))
            .unwrap();
        assert_eq!(first.id, MAIN_MENU);
        // One face shows on the shared row: Community's, painted later, or
        // a pressed Main Menu's.
        let shown = |pressed| -> Vec<u16> {
            layout
                .painted_buttons(pressed)
                .iter()
                .filter(|button| button.rect == community)
                .map(|button| button.id)
                .collect()
        };
        assert_eq!(shown(None), [COMMUNITY]);
        assert_eq!(shown(Some(MAIN_MENU)), [MAIN_MENU]);
        assert_eq!(layout.painted_buttons(None).len(), 6);
        assert_eq!(
            compute_layout(800, 600, &WOL_WELCOME_PAGE)
                .painted_buttons(None)
                .len(),
            7
        );
    }
}

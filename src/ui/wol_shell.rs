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

use vera20k_gui::{NodeKind, NodeSpec, Prop, Value};

use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::geom::RectPx;
use crate::ui::shell::geom::ShellAlign;
use crate::ui::shell::layout::RectTable;
use crate::ui::shell::layout_data;
use crate::ui::shell::menu_page::{
    MenuPageButtonRect, MenuPageButtonSpec, MenuPageLayout, MenuPageSpec,
};
use crate::ui::shell::modal::ShellBox;

pub const WOL_WELCOME_DIALOG: DialogResourceId = DialogResourceId(0x010E);
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

const fn button(id: u16, top: i32, key: &'static str, help: &'static str) -> MenuPageButtonSpec {
    MenuPageButtonSpec::new(id, top, key, help)
}

/// RT_DIALOG `0x10E` as a family page: six top buttons (`0x00608CD0`, all
/// visible) snapped by their template tops, Main Menu on the bottom row
/// (`0x00609730`). Status help from `0x006040B0`.
#[rustfmt::skip]
pub const WOL_WELCOME_PAGE: MenuPageSpec = MenuPageSpec {
    dialog: WOL_WELCOME_DIALOG,
    title_key: "GUI:WOLWelcome",
    stacked: &[
        button(QUICK_MATCH, 116, "GUI:QuickMatch", "STT:WOLWelcomeQuickMatch"),
        button(QUICK_COOP, 144, "GUI:QuickCoop", "STT:QuickCoop"),
        button(CUSTOM_MATCH, 172, "GUI:CustomMatch", "STT:WOLWelcomeCustomMatch"),
        button(PLAY_BUDDY, 200, "GUI:PlayBuddy", "STT:WOLBuddy"),
        button(MY_INFORMATION, 228, "GUI:MyInformation", "STT:WOLWelcomeMyInformation"),
        button(COMMUNITY, 256, "GUI:YuriWebSite", "STT:MainButtonYuriWebSite"),
    ],
    back: MenuPageButtonSpec::new(MAIN_MENU, 284, "GUI:MainMenu", "STT:WOLWelcomeBack").returning(0),
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
    #[cfg(test)]
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

/// A kind-0 text static of the left side, painted at once without a reveal:
/// its control id (the id-less captions get [`CAPTION_IDS`] + n), caption
/// key, alignment (`SS_LEFT` / `SS_CENTER` / `SS_RIGHT`) and status help
/// (`0x006040B0`). The rects are `shell_layout.ini`'s.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WolTextStatic {
    pub id: u16,
    pub key: &'static str,
    pub align: ShellAlign,
    pub help: Option<&'static str>,
}

/// The ids given to the captions the template leaves without one.
pub const CAPTION_IDS: u16 = 0xFF00;
const L: ShellAlign = ShellAlign::NONE;
const C: ShellAlign = ShellAlign::H_CENTER;
const R: ShellAlign = ShellAlign::H_RIGHT;

const fn text(
    id: u16,
    key: &'static str,
    align: ShellAlign,
    help: Option<&'static str>,
) -> WolTextStatic {
    WolTextStatic {
        id,
        key,
        align,
        help,
    }
}

/// The ladder statistics and the icon glossary at their template captions.
/// With a nickname, init fills the heading `0x6EF`, the five values and
/// `0x797` from the persona (`0x007989AA` -> `0x0077A9F0`); without one the
/// values keep `GUI:UnknownStats` and `0x6EF` and `0x797` stay empty.
#[rustfmt::skip]
pub const WOL_TEXT_STATICS: [WolTextStatic; 20] = [
    // The persona heading: centred (`0x50020001`), empty caption.
    text(0x06EF, "", C, None),
    text(CAPTION_IDS, "GUI:IconGlossary", C, None),
    text(CAPTION_IDS + 1, "GUI:LadderWins", R, None),
    text(CAPTION_IDS + 2, "GUI:LadderLosses", R, None),
    text(CAPTION_IDS + 3, "GUI:Disconnects", R, None),
    text(CAPTION_IDS + 4, "GUI:LadderRank", R, None),
    text(CAPTION_IDS + 5, "GUI:LadderPoints", R, None),
    text(0x06F0, "GUI:UnknownStats", L, Some("STT:WOLMyInformationWins")),
    text(0x06F1, "GUI:UnknownStats", L, Some("STT:WOLMyInformationLosses")),
    text(0x0701, "GUI:UnknownStats", L, Some("STT:WOLMyInformationDisconnects")),
    text(0x06F4, "GUI:UnknownStats", L, Some("STT:WOLMyInformationRank")),
    text(0x06F5, "GUI:UnknownStats", L, Some("STT:WOLMyInformationPoints")),
    text(0x0797, "GUI:Blank", C, Some("STT:WelcomeUpdate")),
    text(0x074C, "GUI:NormalGameIconDesc", L, None),
    text(0x074E, "GUI:ClanGameIconDesc", L, None),
    text(0x074F, "GUI:ResolutionLimitIconDesc", L, None),
    text(0x0750, "GUI:ObserverAllowedIconDesc", L, None),
    text(0x0753, "GUI:PasswordIconDesc", L, None),
    text(CAPTION_IDS + 6, "GUI:LatencyIconDesc", L, None),
    text(CAPTION_IDS + 7, "GUI:BadgesOfHonor", L, None),
];

/// `0x10E`'s widgets: the buttons in Z-order with their status help, then
/// the statics (before the group boxes in the template, so hit first; only
/// the stats values and `0x797` have help). `filled` holds the persona
/// texts init wrote, by control id; the rest keep their template caption.
pub fn spec(filled: &[(u16, String)]) -> NodeSpec {
    let buttons = WOL_BUTTON_Z_ORDER
        .iter()
        .filter_map(|id| WOL_WELCOME_PAGE.button(*id));
    let button = |b: &MenuPageButtonSpec| button_node(b.id).tooltip(b.tooltip_key);
    let texts = WOL_TEXT_STATICS.iter().map(|text| {
        let node = NodeSpec::new(NodeKind::Text)
            .id(text.id.into())
            .with(Prop::Align, Value::Int(text.align.0.into()));
        let node = match text.help {
            Some(help) => node.tooltip(help),
            None => node,
        };
        match filled.iter().find(|(id, _)| *id == text.id) {
            Some((_, value)) => node.with(Prop::Text, Value::Text(value.clone())),
            // An empty template caption is no label: the persona heading
            // stays blank without a nickname.
            None if text.key.is_empty() => node,
            None => node.label(text.key),
        }
    });
    // The group boxes' frames and the glossary's icons, which take nothing.
    let group_boxes = GROUP_BOXES.map(|id| NodeSpec::new(NodeKind::Text).id(id.into()));
    let icons = WOL_ICONS.map(|icon| NodeSpec::new(NodeKind::Image).id(icon.id.into()));
    NodeSpec::new(NodeKind::Dialog)
        .children(buttons.map(button))
        .children(texts)
        .children(group_boxes)
        .children(icons)
}

/// A kind-2 image static the proc's `0x497` sets (`0x00603D30`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WolIcon {
    pub id: u16,
    pub file: &'static str,
}

const fn icon(id: u16, file: &'static str) -> WolIcon {
    WolIcon { id, file }
}

/// The glossary icons and their PCX. `0x79F` is shifted one pixel right by
/// `0x0060C002`.
#[rustfmt::skip]
pub const WOL_ICONS: [WolIcon; 13] = [
    icon(0x0746, "gt18.pcx"),
    icon(0x0748, "wolclan.pcx"),
    icon(0x0749, "wolreslk.pcx"),
    icon(0x074A, "wolob.pcx"),
    icon(0x0752, "wolpriv.pcx"),
    icon(0x075C, "pingg.pcx"),
    icon(0x075D, "pingy.pcx"),
    icon(0x075E, "pingr.pcx"),
    icon(0x079E, "number1.pcx"),
    icon(0x079F, "number0.pcx"),
    icon(0x07A0, "number0.pcx"),
    icon(0x07A1, "cooperat.pcx"),
    icon(0x07A2, "comchief.pcx"),
];

/// The two group boxes (`BS_GROUPBOX`): ladder stats and icon glossary.
pub const GROUP_BOXES: [u16; 2] = [0xFF10, 0xFF11];

/// `0x10E` at the current screen: the family right panel, and the left-side
/// children at their template places (`0x0060C0C0` keeps them at every size).
#[derive(Debug, Clone)]
pub struct WolWelcomeLayout {
    pub page: MenuPageLayout,
    pub rects: RectTable,
}

pub fn compute_layout(screen_w: u32, screen_h: u32) -> WolWelcomeLayout {
    let (w, h) = (screen_w as i32, screen_h as i32);
    let (page, mut rects) = layout_data::menu_page(
        &WOL_WELCOME_PAGE,
        "0x10E",
        crate::ui::shell::geom::RETAIL_SCALE,
        w,
        h,
    );
    if let Some(rect) = rects.get(0x079F) {
        rects.insert(0x079F, rect.translate(1, 0));
    }
    WolWelcomeLayout { page, rects }
}

vera20k_gui::scaled_rects!(WolWelcomeLayout { page, rects });

impl WolWelcomeLayout {
    pub fn page_spec(&self) -> &'static MenuPageSpec {
        &WOL_WELCOME_PAGE
    }

    /// Where control `id` of [`spec`] sits.
    pub fn rect_of(&self, id: u16) -> Option<RectPx> {
        self.page.button_rect(id).or_else(|| self.rects.get(id))
    }

    /// The glossary icons' PCX and windows.
    pub fn icons(&self) -> impl Iterator<Item = (&'static str, RectPx)> + '_ {
        WOL_ICONS
            .iter()
            .filter_map(|icon| Some((icon.file, self.rects.get(icon.id)?)))
    }

    pub fn group_boxes(&self) -> impl Iterator<Item = RectPx> + '_ {
        GROUP_BOXES.iter().filter_map(|id| self.rects.get(*id))
    }

    /// The buttons whose faces show, in Z-order. Siblings do not clip each
    /// other and paint down the Z-order, so where two share a row (Community
    /// and Main Menu at 640x480) the later one covers the earlier, unless the
    /// earlier is pressed and repaints alone (inferred from Win32 paint
    /// order; uncaptured).
    pub fn painted_buttons(&self, pressed: Option<u16>) -> Vec<MenuPageButtonRect> {
        let order: Vec<MenuPageButtonRect> = WOL_BUTTON_Z_ORDER
            .iter()
            .filter_map(|id| self.page.buttons.iter().find(|button| button.id == *id))
            .copied()
            .collect();
        let shows = |index: usize, button: &MenuPageButtonRect| {
            let row = |other: &MenuPageButtonRect| other.rect == button.rect;
            pressed == Some(button.id)
                || (!order
                    .iter()
                    .any(|other| row(other) && pressed == Some(other.id))
                    && !order[index + 1..].iter().any(row))
        };
        let shown = order
            .iter()
            .enumerate()
            .filter(|(index, button)| shows(*index, button));
        shown.map(|(_, button)| *button).collect()
    }
}

fn button_node(id: u16) -> NodeSpec {
    NodeSpec::new(NodeKind::Button).id(id.into())
}

/// A WOL message box from `0x005E2CD0`, modal (mode 2, no slide) over the
/// empty shell backdrop: style 0 is `0xD0` with OK (id 1); style 4 is
/// `0x126` with Yes (6) and No (7) on the CE/120 rects. Its loop
/// (`0x007759E0`) dispatches without `IsDialogMessage`, so it takes no keys.
pub fn message_box(body: String, ok: String) -> ShellBox {
    ShellBox::new(DialogResourceId(0x00D0), body).button(BOX_OK, ok)
}

/// [`message_box`]'s style 4: Yes and No.
pub fn yes_no_box(body: String, yes: String, no: String) -> ShellBox {
    ShellBox::new(DialogResourceId(0x0126), body)
        .button(BOX_YES, yes)
        .button(BOX_NO, no)
}

/// `0xC4` OK button id 1, `0x126` Yes 6 / No 7.
pub const BOX_OK: u16 = 1;
pub const BOX_YES: u16 = 6;
pub const BOX_NO: u16 = 7;
/// The wait box `0xC4`'s Disconnect and WOL Check Status.
pub const WAIT_DISCONNECT: u16 = 0x058D;
pub const WAIT_CHECK_STATUS: u16 = 0x058E;
pub const WAIT_DIALOG: DialogResourceId = DialogResourceId(0x00C4);

/// The wait box `0xC4` (`0x00792BE0`, proc `0x00792CF0`): created hidden
/// (fShow 0); its init arms timer `0x45` for `[0x84A248]` = 7000 ms, whose
/// tick shows it and kills the timer. A result before then never shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WolWaitBox {
    /// `0x5B2`'s text (set by `0x4B2` after creation), Disconnect and WOL
    /// Check Status.
    pub shell_box: ShellBox,
    pub opened_at: crate::util::time::Instant,
    pub shown: bool,
}

/// `[0x84A248]`, read only at `0x00792D93`.
pub const WAIT_REVEAL: std::time::Duration = std::time::Duration::from_millis(7000);

impl WolWaitBox {
    pub fn new(text: String, disconnect: String, check_status: String) -> Self {
        let shell_box = ShellBox::new(WAIT_DIALOG, text)
            .button(WAIT_DISCONNECT, disconnect)
            .button(WAIT_CHECK_STATUS, check_status)
            .template(crate::ui::shell::modal::WAIT_TEMPLATE);
        let opened_at = crate::util::time::Instant::now();
        Self {
            shell_box,
            opened_at,
            shown: false,
        }
    }

    pub fn reveal_at(&self) -> crate::util::time::Instant {
        self.opened_at + WAIT_REVEAL
    }

    /// The timer tick: true when this call showed the box.
    pub fn poll(&mut self, now: crate::util::time::Instant) -> bool {
        if !self.shown && now >= self.reveal_at() {
            self.shown = true;
            return true;
        }
        false
    }
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;
    use crate::ui::shell::geom::{centred_in_window, fit_window};

    #[test]
    fn wol_boxes_scale_with_the_quit_box_and_keep_their_hits() {
        let message = yes_no_box("Question".into(), "Yes".into(), "No".into());
        let quit = ShellBox::message("Q".into(), "OK".into(), "Cancel".into(), String::new());
        let wait = WolWaitBox::new("Wait".into(), "D".into(), "C".into()).shell_box;
        for (w, h) in [(640, 480), (760, 600), (2560, 1600), (1200, 1600)] {
            let scale = crate::ui::shell::geom::shell_scale(w, h);
            let parts = message.parts(w, h, scale);
            let native = quit.parts(w, h, scale);
            assert_eq!((parts.dialog, parts.body), (native.dialog, native.body));
            let no = parts.rect_of(BOX_NO).unwrap();
            assert_eq!((parts.button(0), no), (native.button(0), native.button(1)));
            let center = (no.x + no.w / 2, no.y + no.h / 2);
            assert!(!parts.button(0).contains(center.0, center.1));
            let wait = wait.parts(w, h, scale);
            assert_eq!(wait.dialog, native.dialog);
            let check = wait.rect_of(WAIT_CHECK_STATUS).unwrap();
            let disconnect = wait.rect_of(WAIT_DISCONNECT).unwrap();
            assert!(!disconnect.contains(check.x + check.w / 2, check.y + check.h / 2));
        }
        let large = message.parts(2560, 1600, crate::ui::shell::geom::shell_scale(2560, 1600));
        assert_eq!(large.dialog, RectPx::new(456, 323, 1202, 869));
        let retail = message_box("Notice".into(), "OK".into()).parts(
            800,
            600,
            crate::ui::shell::geom::RETAIL_SCALE,
        );
        assert_eq!((retail.dialog.w, retail.dialog.h), (451, 326));
    }

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
    fn wait_box_shows_once_its_timer_fires() {
        let mut wait = WolWaitBox::new(String::new(), String::new(), String::new());
        let opened_at = wait.opened_at;
        assert!(!wait.poll(opened_at + WAIT_REVEAL - std::time::Duration::from_millis(1)));
        assert!(wait.poll(opened_at + WAIT_REVEAL));
        assert!(wait.shown);
        assert!(!wait.poll(opened_at + WAIT_REVEAL * 2));
    }

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
        assert_eq!(layout.page.status_help, RectPx::new(10, 578, 475, 21));
        // The executed relayout of heading 0x694 (dlu 423,4,108,11).
        assert_eq!(layout.page.title, RectPx::new(635, 9, 163, 19));
        assert_eq!(
            compute_layout(640, 480).page.title,
            RectPx::new(475, 9, 163, 19)
        );
        assert_eq!(
            compute_layout(1024, 768).page.title,
            RectPx::new(747, 93, 163, 19)
        );
    }

    #[test]
    fn left_children_keep_their_template_places() {
        let layout = compute_layout(800, 600);
        let boxes: Vec<_> = layout.group_boxes().collect();
        assert_eq!(
            boxes,
            [
                RectPx::new(122, 85, 384, 173),
                RectPx::new(92, 302, 435, 206)
            ]
        );
        assert_eq!(
            layout.rect_of(CAPTION_IDS + 1),
            Some(RectPx::new(131, 102, 174, 21))
        );
        // The executed kind-2 placement: gt18 at (108, 328), the second
        // number0 (0x79F, +1 x) at (110, 473).
        let icons: Vec<_> = layout.icons().collect();
        assert_eq!(centred_in_window(icons[0].1, 14, 14), (108, 328));
        assert_eq!(centred_in_window(icons[9].1, 8, 16), (110, 473));
        // 1024x768 leaves them at their 800x600 places.
        let wide = compute_layout(1024, 768);
        assert!(wide.group_boxes().eq(layout.group_boxes()));
    }

    #[test]
    fn responsive_layout_stretches_the_page_and_the_left_side() {
        let base = compute_layout(800, 600);
        // Identity at the authored size, where the retail layout is used.
        let same = fit_window(800, 600, compute_layout);
        assert_eq!(same.page.title, base.page.title);
        assert_eq!(same.rects, base.rects);
        let scale = crate::ui::shell::geom::shell_scale(1600, 1200);
        let scaled = fit_window(1600, 1200, compute_layout);
        assert_eq!(scaled.page.scale, scale);
        assert_eq!(scaled.page.title, RectPx::new(1270, 18, 326, 38));
        assert_eq!(scaled.page.status_help, RectPx::new(20, 1156, 950, 42));
        let boxes: Vec<_> = scaled.group_boxes().collect();
        assert_eq!(
            boxes,
            [
                RectPx::new(244, 170, 768, 346),
                RectPx::new(184, 604, 870, 412)
            ]
        );
        assert_eq!(
            scaled.rect_of(CAPTION_IDS + 1),
            Some(RectPx::new(262, 204, 348, 42))
        );
        assert_eq!(
            scaled.page.button_rect(MAIN_MENU),
            Some(RectPx::new(1288, 1070, 312, 84))
        );
    }

    /// The control under `(x, y)` once the page's widgets are on the
    /// layout, and the status help it writes.
    fn hover(layout: &WolWelcomeLayout, x: i32, y: i32) -> (Option<u16>, Option<String>) {
        use crate::ui::shell::page;
        use vera20k_gui::{DialogKind, Gui, InputEvent};
        let mut gui = Gui::new(64);
        let root = gui.create(NodeKind::Dialog, None).unwrap();
        gui.push_dialog(root, DialogKind::Page).unwrap();
        page::ensure_built(&mut gui, root, || spec(&[]));
        page::sync_rects(&mut gui, root, |id| layout.rect_of(id));
        gui.input(InputEvent::PointerMove { x, y }, 0);
        let hovered = gui.hovered();
        let id = hovered.and_then(|node| gui.id_of(node)).map(|id| id as u16);
        (id, page::hovered_help(&gui, hovered))
    }

    #[test]
    fn status_help_covers_buttons_and_stats_values() {
        let layout = compute_layout(800, 600);
        let help = |x, y| hover(&layout, x, y).1;
        assert_eq!(help(720, 220).as_deref(), Some("STT:WOLWelcomeQuickMatch"));
        assert_eq!(help(720, 556).as_deref(), Some("STT:WOLWelcomeBack"));
        assert_eq!(help(330, 110).as_deref(), Some("STT:WOLMyInformationWins"));
        assert_eq!(help(200, 110), None, "the label has no id");
        assert_eq!(help(400, 300), None);
    }

    #[test]
    fn init_fills_the_persona_texts_over_the_template_captions() {
        let node = |filled: &[(u16, String)], id: i32| {
            let spec = spec(filled);
            let props = |n: &NodeSpec| n.props.contains(&(Prop::Id, Value::Int(id)));
            spec.children.into_iter().find(|n| props(n)).unwrap().props
        };
        let wins = node(&[(0x06F0, "17".into())], 0x06F0);
        assert!(wins.contains(&(Prop::Text, Value::Text("17".into()))));
        assert!(!wins.iter().any(|(prop, _)| *prop == Prop::Label));
        let unknown = node(&[], 0x06F0);
        assert!(unknown.contains(&(Prop::Label, Value::Text("GUI:UnknownStats".into()))));
        // The heading's empty template caption is no label.
        assert!(
            !node(&[], 0x06EF)
                .iter()
                .any(|(prop, _)| *prop == Prop::Label)
        );
    }

    #[test]
    fn main_menu_takes_the_shared_last_row_at_640() {
        let layout = compute_layout(640, 480);
        let community = layout.page.button_rect(COMMUNITY).unwrap();
        assert_eq!(layout.page.button_rect(MAIN_MENU), Some(community));
        let first = hover(&layout, community.x + 5, community.y + 5).0;
        assert_eq!(first, Some(MAIN_MENU));
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
        assert_eq!(compute_layout(800, 600).painted_buttons(None).len(), 7);
    }
}

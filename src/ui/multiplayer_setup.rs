//! Network setup presentation: LAN host BC (005DE520) and guest BD
//! (005E1CA0), WOL host C2 (0078F7F0) and guest C9 (00791B10). The WOL pages
//! are supersets of the LAN ones with the same control ids (resources C2/C9).
//! MatchSession owns admitted participants, options, readiness and map status.
//! This leaf keeps only control interaction and emits commands for that owner.

use std::time::Instant;

use crate::ui::lan_shell::LanChatState;
use crate::ui::shell::button::ShellButtonInteraction;
use crate::ui::shell::combobox::{ComboPopup, ComboPopupInteraction};
use crate::ui::shell::descriptor::{DialogId, LOW_HEADING_ANCHOR, MAP_TEXT_ANCHOR};
use crate::ui::shell::geom::{RectPx, ShellScale, child_window};
use crate::ui::shell::layout::anchor_rect;
use crate::ui::shell::menu_page::{self, MenuPageButtonSpec, MenuPageLayout, MenuPageSpec};
use crate::ui::shell::trackbar::{TrackbarHold, TrackbarRange};
use crate::ui::skirmish_shell::{SkirmishStatics, SkirmishValidationModalState};

pub(crate) const START: u16 = 0x588;
pub(crate) const ACCEPT: u16 = 0x59F;
pub(crate) const BACK: u16 = 2;
pub(crate) const CHOOSE_MAP: u16 = 0x5AA;
pub(crate) const CHAT: u16 = 0x540;
pub(crate) const CHAT_LOG: u16 = 0x53F;
pub(crate) const SPEED: u16 = 0x529;
pub(crate) const CREDITS: u16 = 0x402;
pub(crate) const UNITS: u16 = 0x54F;
pub(crate) const DIFFICULTY: u16 = 0x732;
pub(crate) const CHECKBOXES: [u16; 5] = [0x54E, 0x3E8, 0x54D, 0x69A, 0x69D];
pub(crate) const COUNTRIES: [u16; 8] = [0x6A1, 0x510, 0x513, 0x51E, 0x514, 0x51F, 0x520, 0x521];
pub(crate) const COLORS: [u16; 8] = [0x6A2, 0x522, 0x523, 0x524, 0x525, 0x526, 0x527, 0x528];
pub(crate) const STARTS: [u16; 8] = [0x6A3, 0x6A4, 0x6A5, 0x6A6, 0x6A7, 0x6A8, 0x6AA, 0x6AB];
/// WOL-only C2/C9 controls: Page (modal 0xCF), the emote button and the
/// per-row ping statics 0x754+row (helper 004E34A0).
pub(crate) const PAGE: u16 = 0x5C2;
pub(crate) const EMOTE: u16 = 0x3ED;
pub(crate) const PING_ICONS: u16 = 0x754;

/// The service whose setup dialog this is. Every shared helper keeps one
/// code path and branches on GameMode 3/4 like the native helpers do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SetupFlavor {
    Lan,
    Wol,
}

/// One of the four setup dialogs: BC/BD (LAN) or C2/C9 (WOL).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct SetupDialog {
    pub(crate) host: bool,
    pub(crate) flavor: SetupFlavor,
}
impl SetupDialog {
    pub(crate) const fn page(self) -> &'static MenuPageSpec {
        match (self.flavor, self.host) {
            (SetupFlavor::Lan, true) => &HOST_PAGE,
            (SetupFlavor::Lan, false) => &GUEST_PAGE,
            (SetupFlavor::Wol, true) => &WOL_HOST_PAGE,
            (SetupFlavor::Wol, false) => &WOL_GUEST_PAGE,
        }
    }
    pub(crate) const fn wol(self) -> bool {
        matches!(self.flavor, SetupFlavor::Wol)
    }
}

/// WOL__SetPlayerPing 007A0EA0: icon -1/0/1/2 at thresholds 0/300/500, art
/// pingg/pingy/pingr.pcx through 004E34A0. Negative pings show no image.
pub(crate) const fn ping_icon(ping: i32) -> Option<u8> {
    if ping < 0 {
        None
    } else if ping < 300 {
        Some(0)
    } else if ping < 500 {
        Some(1)
    } else {
        Some(2)
    }
}

pub(crate) const HOST_PAGE: MenuPageSpec = MenuPageSpec {
    dialog: DialogId(0xBC),
    title_key: "GUI:HostScreen",
    stacked: &[
        MenuPageButtonSpec {
            id: START,
            dlu_top: 147,
            csf_key: "GUI:StartGame",
            tooltip_key: "STT:HostButtonGo",
            result: None,
        },
        MenuPageButtonSpec {
            id: CHOOSE_MAP,
            dlu_top: 176,
            csf_key: "GUI:ChooseMap",
            tooltip_key: "STT:HostButtonChooseMap",
            result: None,
        },
    ],
    back: MenuPageButtonSpec {
        id: BACK,
        dlu_top: 346,
        csf_key: "GUI:Back",
        tooltip_key: "STT:HostButtonBack",
        result: None,
    },
};
pub(crate) const GUEST_PAGE: MenuPageSpec = MenuPageSpec {
    dialog: DialogId(0xBD),
    title_key: "GUI:JoinScreen",
    stacked: &[MenuPageButtonSpec {
        id: ACCEPT,
        dlu_top: 149,
        csf_key: "GUI:Accept",
        tooltip_key: "STT:GuestButtonAccept",
        result: None,
    }],
    back: MenuPageButtonSpec {
        id: BACK,
        dlu_top: 346,
        csf_key: "GUI:Back",
        tooltip_key: "STT:GuestButtonBack",
        result: None,
    },
};

const WOL_BACK: MenuPageButtonSpec = MenuPageButtonSpec {
    id: BACK,
    dlu_top: 257,
    csf_key: "GUI:Back",
    tooltip_key: "STT:WOLNewGameCancel",
    result: None,
};
pub(crate) const WOL_HOST_PAGE: MenuPageSpec = MenuPageSpec {
    dialog: DialogId(0xC2),
    title_key: "GUI:HostScreen",
    stacked: &[
        MenuPageButtonSpec {
            id: START,
            dlu_top: 156,
            csf_key: "GUI:StartGame",
            tooltip_key: "STT:HostButtonGo",
            result: None,
        },
        MenuPageButtonSpec {
            id: CHOOSE_MAP,
            dlu_top: 183,
            csf_key: "GUI:ChooseMap",
            tooltip_key: "STT:HostButtonChooseMap",
            result: None,
        },
        MenuPageButtonSpec {
            id: PAGE,
            dlu_top: 210,
            csf_key: "GUI:PageButton",
            tooltip_key: "STT:WOLPageButton",
            result: None,
        },
    ],
    back: WOL_BACK,
};
pub(crate) const WOL_GUEST_PAGE: MenuPageSpec = MenuPageSpec {
    dialog: DialogId(0xC9),
    title_key: "GUI:JoinScreen",
    stacked: &[
        MenuPageButtonSpec {
            id: ACCEPT,
            dlu_top: 156,
            csf_key: "GUI:Accept",
            tooltip_key: "STT:GuestButtonAccept",
            result: None,
        },
        MenuPageButtonSpec {
            id: PAGE,
            dlu_top: 183,
            csf_key: "GUI:PageButton",
            tooltip_key: "STT:WOLPageButton",
            result: None,
        },
    ],
    back: WOL_BACK,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SetupColumn {
    Player,
    Country,
    Color,
    Start,
    Team,
}
impl SetupColumn {
    pub(crate) const ALL: [Self; 5] = [
        Self::Player,
        Self::Country,
        Self::Color,
        Self::Start,
        Self::Team,
    ];
    pub(crate) fn control(self, row: usize) -> u16 {
        match self {
            Self::Player => 0x411 + row as u16,
            Self::Country => COUNTRIES[row],
            Self::Color => COLORS[row],
            Self::Start => STARTS[row],
            Self::Team => 0x76D + row as u16,
        }
    }
    pub(crate) fn decode(control: u16) -> Option<(Self, usize)> {
        Self::ALL.into_iter().find_map(|column| {
            (0..8)
                .find(|row| column.control(*row) == control)
                .map(|row| (column, row))
        })
    }
    /// Explicit 4DE overrides from shared setup initialization4E37D0 etc.
    pub(crate) fn visible_rows(self) -> usize {
        match self {
            Self::Country => 7,
            Self::Color | Self::Start | Self::Team => 9,
            Self::Player => 3,
        }
    }
}

/// Read-only control projection, rebuilt from the session; native item-data is
/// passed back unchanged so the session owner can validate a requested mutation.
#[derive(Debug, Clone)]
pub(crate) struct SetupChoice {
    pub(crate) value: i32,
    pub(crate) text: String,
    pub(crate) swatch: Option<usize>,
}
#[derive(Debug, Clone)]
pub(crate) struct SetupCombo {
    pub(crate) id: u16,
    pub(crate) enabled: bool,
    pub(crate) visible: bool,
    pub(crate) selected: Option<usize>,
    pub(crate) choices: Vec<SetupChoice>,
}
#[derive(Debug, Clone)]
pub(crate) struct SetupTrackbar {
    pub(crate) id: u16,
    pub(crate) value: i32,
    pub(crate) range: TrackbarRange,
    pub(crate) enabled: bool,
    pub(crate) visible: bool,
}
impl SetupTrackbar {
    // Co-op5C21D0 sends7324AC(0), so the shared native trackbar runs with no
    // value-plaque reserve; ordinary options retain the49-pixel plaque.
    fn reserve(&self) -> i32 {
        if self.id == DIFFICULTY {
            0
        } else {
            crate::ui::shell::trackbar::PLAQUE_RESERVE
        }
    }
    pub(crate) fn thumb_left(&self, rect: RectPx, scale: ShellScale) -> i32 {
        crate::ui::shell::trackbar::thumb_left_at(
            self.value - self.range.min,
            rect,
            self.reserve(),
            self.range.max - self.range.min,
            scale,
        )
    }
    pub(crate) fn pointer_value(&self, x: i32, rect: RectPx, scale: ShellScale) -> i32 {
        let track = crate::ui::shell::trackbar::authored_track(rect, (x, rect.y), scale);
        self.range.stepped(
            self.range.min
                + crate::ui::shell::trackbar::trackbar_position_from_x(
                    track.local_x,
                    track.width,
                    self.reserve(),
                    self.range.max - self.range.min,
                ),
        )
    }
    pub(crate) fn press(
        &self,
        x: i32,
        y: i32,
        rect: RectPx,
        scale: ShellScale,
    ) -> crate::ui::shell::trackbar::TrackbarPress {
        use crate::ui::shell::trackbar::{TrackbarPress, authored_track, trackbar_press};
        let track = authored_track(rect, (x, y), scale);
        match trackbar_press(
            self.value - self.range.min,
            track.local_x,
            track.local_y,
            track.width,
            track.height,
            self.reserve(),
            self.range.max - self.range.min,
        ) {
            TrackbarPress::Jump(value) => {
                TrackbarPress::Jump(self.range.stepped(self.range.min + value))
            }
            other => other,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SetupRowIcon {
    pub(crate) country: Option<String>,
    pub(crate) host: bool,
    pub(crate) accepted: bool,
}
#[derive(Debug, Clone)]
pub(crate) struct SetupView {
    pub(crate) dialog: SetupDialog,
    pub(crate) host: bool,
    pub(crate) locked: bool,
    pub(crate) accept_enabled: bool,
    pub(crate) start_requested: bool,
    pub(crate) combos: Vec<SetupCombo>,
    pub(crate) trackbars: Vec<SetupTrackbar>,
    pub(crate) checks: [bool; 5],
    pub(crate) row_icons: [Option<SetupRowIcon>; 8],
    /// C2/C9 ping statics; always empty on BC/BD.
    pub(crate) ping_icons: [Option<u8>; 8],
    pub(crate) mode_label: String,
    pub(crate) map_label: String,
    pub(crate) selected_map_index: Option<usize>,
    pub(crate) cooperative: bool,
    pub(crate) cooperative_description: Option<String>,
}
impl SetupView {
    pub(crate) fn page(&self) -> &'static MenuPageSpec {
        self.dialog.page()
    }
    pub(crate) fn combo(&self, id: u16) -> Option<&SetupCombo> {
        self.combos.iter().find(|combo| combo.id == id)
    }
    pub(crate) fn enabled(&self, id: u16) -> bool {
        if matches!(id, CHAT | CHAT_LOG) || (id == EMOTE && self.dialog.wol()) {
            return true;
        }
        if self.locked {
            return false;
        }
        match id {
            BACK | CHAT | CHAT_LOG => true,
            START | CHOOSE_MAP => self.host,
            PAGE => self.dialog.wol(),
            ACCEPT => !self.host && self.accept_enabled,
            id if CHECKBOXES.contains(&id) => self.host,
            id => {
                self.combo(id)
                    .is_some_and(|combo| combo.visible && combo.enabled)
                    || self
                        .trackbars
                        .iter()
                        .any(|bar| bar.id == id && bar.visible && bar.enabled)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SetupAction {
    Start,
    Accept,
    Back,
    ChooseMap,
    Select {
        control: u16,
        value: i32,
    },
    Toggle {
        control: u16,
    },
    Trackbar {
        control: u16,
        value: i32,
    },
    Chat(Vec<u16>),
    /// C2/C9 0x3ED sends the chat edit's text as an action message.
    Emote,
    Page,
}

#[derive(Debug, Clone)]
pub(crate) struct MultiplayerSetupState {
    pub(crate) chat: LanChatState,
    pub(crate) notice: Option<SkirmishValidationModalState>,
    pub(crate) buttons: ShellButtonInteraction<u16>,
    pub(crate) popup: ComboPopupInteraction<u16>,
    pub(crate) trackbar_hold: Option<TrackbarHold<u16>>,
    pub(crate) statics: SkirmishStatics,
    pub(crate) shown: bool,
    pub(crate) focused_control: u16,
    focus_started: Instant,
    accept_flash_started: Option<Instant>,
}
impl Default for MultiplayerSetupState {
    fn default() -> Self {
        Self {
            chat: {
                let mut chat = LanChatState::default();
                chat.edit.focused = true;
                chat
            },
            notice: None,
            buttons: ShellButtonInteraction::default(),
            popup: ComboPopupInteraction::default(),
            trackbar_hold: None,
            statics: SkirmishStatics::default(),
            shown: false,
            focused_control: CHAT,
            focus_started: Instant::now(),
            accept_flash_started: None,
        }
    }
}
impl MultiplayerSetupState {
    pub(crate) fn accept_highlight(&mut self, enabled: bool, now: Instant) -> bool {
        if !enabled {
            self.accept_flash_started = None;
            return false;
        }
        crate::ui::shell::button::flash_phase(*self.accept_flash_started.get_or_insert(now), now)
    }
    pub(crate) fn popup_geometry(
        &self,
        view: &SetupView,
        layout: &MultiplayerSetupLayout,
    ) -> Option<ComboPopup> {
        if view.locked {
            return None;
        }
        let id = self.popup.control?;
        let combo = view
            .combo(id)
            .filter(|combo| combo.visible && combo.enabled)?;
        let (column, _) = SetupColumn::decode(id)?;
        Some(ComboPopup::new(
            layout.control_rect(id)?,
            layout.page.scale,
            combo.choices.len(),
            column.visible_rows(),
            self.popup.top_index,
        ))
    }
    pub(crate) fn caret_visible(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.focus_started).as_secs() % 2 == 0
    }
    pub(crate) fn focus_chat(&mut self) {
        if self.focused_control != CHAT {
            self.focus_started = Instant::now();
        }
        self.focused_control = CHAT;
        self.chat.edit.focused = true;
    }
    pub(crate) fn cancel_pointer(&mut self) {
        self.buttons = ShellButtonInteraction::default();
        self.popup.cancel_pointer();
        self.trackbar_hold = None;
        self.chat.list.scroll.cancel();
    }
}

#[derive(Debug, Clone)]
pub(crate) struct MultiplayerSetupLayout {
    pub(crate) page: MenuPageLayout,
    pub(crate) combos: [[RectPx; 5]; 8],
    pub(crate) flags: [RectPx; 8],
    pub(crate) acceptance: [RectPx; 8],
    /// C2/C9 ping statics and emote button; `None` on BC/BD.
    pub(crate) ping_icons: Option<[RectPx; 8]>,
    pub(crate) emote: Option<RectPx>,
    pub(crate) chat: RectPx,
    pub(crate) chat_log: RectPx,
    pub(crate) checkboxes: [RectPx; 5],
    pub(crate) trackbars: [(u16, RectPx); 4],
    pub(crate) labels: Vec<(&'static str, RectPx)>,
    pub(crate) map_preview: RectPx,
    pub(crate) cooperative_description: RectPx,
    pub(crate) mode_label: RectPx,
    pub(crate) map_label: RectPx,
}
impl MultiplayerSetupLayout {
    pub(crate) fn new(dialog: SetupDialog, width: i32, height: i32, responsive: bool) -> Self {
        let host = dialog.host;
        let spec = dialog.page();
        let mut page = if responsive {
            menu_page::compute_responsive_layout(spec, width as u32, height as u32)
        } else {
            menu_page::compute_layout(spec, width as u32, height as u32)
        };
        let scale = page.scale;
        let rect = |x, y, w, h| scale.scale_rect(RectPx::new(x, y, w, h));
        let (aw, ah) = if responsive {
            (800, 600)
        } else {
            (width, height)
        };
        // Resource templates of the anchored statics (BC/BD vs C2/C9).
        let (title, preview_x, label_x, map_label_y) = match (dialog.flavor, host) {
            (SetupFlavor::Lan, _) => (RectPx::new(425, 1, 108, 10), 429, 431, 116),
            (SetupFlavor::Wol, true) => (RectPx::new(424, 4, 108, 11), 432, 434, 118),
            (SetupFlavor::Wol, false) => (RectPx::new(424, 4, 108, 11), 433, 435, 118),
        };
        page.title = scale.scale_rect(anchor_rect(LOW_HEADING_ANCHOR, title, aw, ah));
        // Original BC/BD relayout60C4A0→60C0C0. Body controls stay at
        // resource coordinates; only the sidebar is anchored to screen size.
        page.status_help = scale.scale_rect(crate::ui::shell::layout::status_line_rect(
            RectPx::new(2, 355, 410, 12),
            aw,
            ah,
        ));
        let combos = std::array::from_fn(|row| {
            std::array::from_fn(|column| {
                let (x, w) = [(49, 100), (201, 78), (294, 27), (334, 25), (374, 25)][column];
                let window = child_window(x, 22 + row as i32 * 16, w, 74);
                scale.scale_rect(RectPx::new(window.x, window.y, window.w, 24))
            })
        });
        // C9's option block sits 1-2 px lower than C2/BC/BD in its resource.
        let wol_guest = dialog.wol() && !host;
        let option_y: [i32; 4] = if wol_guest {
            [254, 281, 309, 336]
        } else {
            [252, 280, 307, 335]
        };
        // BC/BD and C2 differ by one pixel in the credits/unit rows.
        let (credit_y, unit_y) = match (dialog.flavor, host) {
            (SetupFlavor::Lan, _) => (279, 306),
            (SetupFlavor::Wol, _) => (option_y[1], option_y[2]),
        };
        let (label_speed_y, label_credit_y, label_unit_y) = if wol_guest {
            (254, 281, 309)
        } else {
            (252, 280, 307)
        };
        let labels = [
            ("GUI:Players", 74, 11, 147, 17),
            ("GUI:Side", 302, 11, 111, 17),
            ("GUI:Color", 440, 11, 64, 17),
            ("GUI:StartPosition", 503, 11, 52, 17),
            ("GUI:Team", 560, 11, 52, 17),
            ("GUI:GameSpeed", 302, label_speed_y, 93, 17),
            ("GUI:Credits", 302, label_credit_y, 91, 17),
            ("GUI:UnitCount", 302, label_unit_y, 91, 17),
        ]
        .into_iter()
        .map(|(s, x, y, w, h)| (s, rect(x, y, w, h)))
        .collect();
        // C2's sixth row icons sit at 164 (template 101), C9's at 166.
        let icon_y = |row: usize| {
            if dialog.wol() && host && row == 5 {
                164
            } else {
                36 + row as i32 * 26
            }
        };
        let checkbox_x = if dialog.wol() { 72 } else { 71 };
        let build_off_ally_width = match (dialog.flavor, host) {
            (SetupFlavor::Lan, true) => 225,
            (SetupFlavor::Lan, false) => 234,
            (SetupFlavor::Wol, true) => 256,
            (SetupFlavor::Wol, false) => 261,
        };
        let (chat_y, chat_w, chat_log_w) = match (dialog.flavor, host) {
            (SetupFlavor::Lan, _) => (533, 609, 610),
            (SetupFlavor::Wol, true) => (533, 580, 612),
            (SetupFlavor::Wol, false) => (535, 580, 612),
        };
        Self {
            page,
            combos,
            flags: std::array::from_fn(|row| rect(240, 36 + row as i32 * 26, 49, 21)),
            acceptance: std::array::from_fn(|row| {
                if dialog.wol() {
                    rect(50, icon_y(row), 16, 21)
                } else {
                    rect(42, icon_y(row), 28, 21)
                }
            }),
            ping_icons: dialog
                .wol()
                .then(|| std::array::from_fn(|row| rect(27, icon_y(row), 16, 21))),
            emote: dialog.wol().then(|| rect(596, 533, 27, 24)),
            // The NewEdit paints inside its one-pixel resource frame.
            chat: rect(12, chat_y + 1, chat_w - 2, 19),
            chat_log: rect(11, 369, chat_log_w, 160),
            checkboxes: [
                rect(checkbox_x, option_y[0], 151, 17),
                rect(checkbox_x, option_y[1], 151, 17),
                rect(checkbox_x, option_y[2], 151, 17),
                rect(checkbox_x, option_y[3], 151, 17),
                rect(302, option_y[3], build_off_ally_width, 17),
            ],
            trackbars: [
                (SPEED, rect(404, option_y[0], 129, 21)),
                (CREDITS, rect(404, credit_y, 129, 21)),
                (UNITS, rect(404, unit_y, 129, 21)),
                (
                    DIFFICULTY,
                    rect(180, 215, 266, if dialog.wol() && host { 21 } else { 22 }),
                ),
            ],
            labels,
            cooperative_description: rect(60, 98, 540, 143),
            map_preview: scale.scale_rect(anchor_rect(
                crate::ui::shell::descriptor::AnchorRule::RightAnchor,
                RectPx::new(preview_x, 23, 96, 69),
                aw,
                ah,
            )),
            mode_label: scale.scale_rect(anchor_rect(
                MAP_TEXT_ANCHOR,
                RectPx::new(label_x, 103, 90, 10),
                aw,
                ah,
            )),
            map_label: scale.scale_rect(anchor_rect(
                MAP_TEXT_ANCHOR,
                RectPx::new(label_x, map_label_y, 90, 20),
                aw,
                ah,
            )),
        }
    }
    pub(crate) fn control_rect(&self, id: u16) -> Option<RectPx> {
        if let Some((column, row)) = SetupColumn::decode(id) {
            return Some(self.combos[row][column as usize]);
        }
        if let Some(index) = CHECKBOXES.iter().position(|candidate| *candidate == id) {
            return Some(self.checkboxes[index]);
        }
        if let Some((_, rect)) = self
            .trackbars
            .iter()
            .find(|(candidate, _)| *candidate == id)
        {
            return Some(*rect);
        }
        if let Some(icons) = &self.ping_icons
            && (PING_ICONS..PING_ICONS + 8).contains(&id)
        {
            return Some(icons[usize::from(id - PING_ICONS)]);
        }
        match id {
            CHAT => Some(self.chat),
            CHAT_LOG => Some(self.chat_log),
            EMOTE => self.emote,
            _ => self.page.button_rect(id),
        }
    }
    pub(crate) fn control_at(&self, view: &SetupView, x: i32, y: i32) -> Option<u16> {
        view.combos
            .iter()
            .filter(|combo| combo.visible)
            .map(|combo| combo.id)
            .chain(
                view.trackbars
                    .iter()
                    .filter(|bar| bar.visible)
                    .map(|bar| bar.id),
            )
            .chain(CHECKBOXES)
            .chain([CHAT, CHAT_LOG])
            .chain(self.emote.map(|_| EMOTE))
            .chain(self.page.buttons.iter().map(|button| button.id))
            .find(|id| {
                self.control_rect(*id)
                    .is_some_and(|rect| rect.contains(x, y))
            })
    }
}

/// OwnerDraw__GetTooltipStringLabel6040B0 BC/BD and C2/C9 branches. Guest
/// controls retain their own help even while disabled; no invented fallback.
pub(crate) fn tooltip_key(dialog: SetupDialog, control: u16) -> Option<String> {
    let host = dialog.host;
    if dialog.wol() {
        match control {
            BACK => return Some("STT:WOLNewGameCancel".into()),
            PAGE => return Some("STT:WOLPageButton".into()),
            EMOTE => return Some("STT:EmoteButton".into()),
            _ => {}
        }
    }
    let suffix = if let Some((column, _)) = SetupColumn::decode(control) {
        match column {
            SetupColumn::Player => "ComboPlayer",
            SetupColumn::Country => "ComboCountry",
            SetupColumn::Color => "ComboColor",
            SetupColumn::Start => "ComboStart",
            SetupColumn::Team => "ComboTeam",
        }
    } else {
        match control {
            SPEED => "SliderSpeed",
            CREDITS => "SliderCredits",
            UNITS => "SliderUnit",
            0x3E8 => "CBoxRedeploys",
            0x54E => "CBoxShortGame",
            0x69A => "CBoxSWAllowed",
            0x69D => "CBoxBuildOffAlly",
            0x54D => "CBoxCrates",
            CHAT => "EditInput",
            CHAT_LOG => "EditOutput",
            START => "ButtonGo",
            ACCEPT => "ButtonAccept",
            CHOOSE_MAP => "ButtonChooseMap",
            BACK => "ButtonBack",
            0x468 => "MapThumbnail",
            0x5A8 => "LabelScenario",
            0x6D2..=0x6D9 => "PictureAcceptance",
            0x6DA..=0x6E1 => "PictureFlag",
            _ => return None,
        }
    };
    Some(format!(
        "STT:{}{}",
        if host { "Host" } else { "Guest" },
        suffix
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accept_prompt_flash_starts_off_stops_and_restarts_at_its_own_phase() {
        let mut ui = MultiplayerSetupState::default();
        let now = Instant::now();
        assert!(!ui.accept_highlight(true, now));
        assert!(ui.accept_highlight(true, now + std::time::Duration::from_millis(1000)));
        assert!(!ui.accept_highlight(false, now + std::time::Duration::from_millis(1500)));
        assert!(!ui.accept_highlight(true, now + std::time::Duration::from_millis(1600)));
        assert!(ui.accept_highlight(true, now + std::time::Duration::from_millis(2600)));
    }

    #[test]
    fn setup_controls_follow_original_resources_and_executed_relayout() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/lan_lobby.json",
        ))
        .unwrap();
        for case in corpus["setup_layouts"].as_array().unwrap() {
            let dialog = SetupDialog {
                host: case["dialog"] == 0xBC,
                flavor: SetupFlavor::Lan,
            };
            let layout = MultiplayerSetupLayout::new(
                dialog,
                case["width"].as_i64().unwrap() as i32,
                case["height"].as_i64().unwrap() as i32,
                false,
            );
            for child in case["children"].as_array().unwrap() {
                let id = child["id"].as_u64().unwrap() as u16;
                let Some(actual) = layout.control_rect(id) else {
                    continue;
                };
                // Co-op controls are repositioned by their active mode later;
                // this corpus is the common resource relayout only.
                if id == DIFFICULTY {
                    continue;
                }
                let a = child["rect"].as_array().unwrap();
                let mut native = RectPx::new(
                    a[0].as_i64().unwrap() as i32,
                    a[1].as_i64().unwrap() as i32,
                    a[2].as_i64().unwrap() as i32,
                    a[3].as_i64().unwrap() as i32,
                );
                if SetupColumn::decode(id).is_some() {
                    native.h = 24;
                }
                if id == CHAT {
                    native = RectPx::new(native.x + 1, native.y + 1, native.w - 2, native.h - 2);
                }
                assert_eq!(actual, native, "dialog{} control{id:#x}", case["dialog"]);
            }
        }
    }

    #[test]
    fn setup_help_uses_original_host_and_guest_control_tables() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/lan_lobby.json",
        ))
        .unwrap();
        for case in corpus["setup_help"].as_array().unwrap() {
            assert_eq!(
                tooltip_key(
                    SetupDialog {
                        host: case["dialog"] == 0xBC,
                        flavor: SetupFlavor::Lan,
                    },
                    case["control"].as_u64().unwrap() as u16
                )
                .as_deref(),
                case["key"].as_str(),
                "{case}"
            );
        }
    }

    /// C2/C9 executed relayout and help tables
    /// (`tools/storage_oracle/wol_shell.json`).
    #[test]
    fn wol_setup_controls_and_help_follow_the_executed_c2_c9_tables() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/wol_shell.json",
        ))
        .unwrap();
        let parse_id = |key: &str| u16::from_str_radix(key.trim_start_matches("0x"), 16).ok();
        for (key, host) in [("0xC2", true), ("0xC9", false)] {
            let dialog = SetupDialog {
                host,
                flavor: SetupFlavor::Wol,
            };
            let mut checked = 0;
            for (size, case) in corpus["layouts"][key]["sizes"].as_object().unwrap() {
                let (width, height) = size.split_once('x').unwrap();
                let layout = MultiplayerSetupLayout::new(
                    dialog,
                    width.parse().unwrap(),
                    height.parse().unwrap(),
                    false,
                );
                for (id, child) in case["controls"].as_object().unwrap() {
                    let Some(id) = parse_id(id) else {
                        continue;
                    };
                    let Some(actual) = layout.control_rect(id) else {
                        continue;
                    };
                    if id == DIFFICULTY {
                        continue;
                    }
                    let a = child["rect"].as_array().unwrap();
                    let mut native = RectPx::new(
                        a[0].as_i64().unwrap() as i32,
                        a[1].as_i64().unwrap() as i32,
                        a[2].as_i64().unwrap() as i32,
                        a[3].as_i64().unwrap() as i32,
                    );
                    if SetupColumn::decode(id).is_some() {
                        native.h = 24;
                    }
                    if id == CHAT {
                        native =
                            RectPx::new(native.x + 1, native.y + 1, native.w - 2, native.h - 2);
                    }
                    assert_eq!(actual, native, "{key} {size} control {id:#x}");
                    checked += 1;
                }
            }
            assert!(checked > 100, "{key}: {checked} controls compared");
            for (id, help) in corpus["help"][key].as_object().unwrap() {
                let Some(id) = parse_id(id) else {
                    continue;
                };
                assert_eq!(
                    tooltip_key(dialog, id).as_deref(),
                    help.as_str(),
                    "{key} control {id:#x}"
                );
            }
        }
    }

    #[test]
    fn setup_retains_native_row_identity_across_columns() {
        for row in 0..8 {
            for column in SetupColumn::ALL {
                assert_eq!(
                    SetupColumn::decode(column.control(row)),
                    Some((column, row))
                );
            }
        }
        assert_eq!(SetupColumn::decode(0x6A9), None);
    }
}

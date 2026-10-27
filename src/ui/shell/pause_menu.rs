//! In-game menu (scenario state 1), original4F10E0 and callback4F11B0.
//! 4F10E1..4F1130 picks the template by GameMode: B5 (BEF9D0) for modes0/5,
//! FF (BFAB00) for WOL while Session+0x300 (set by MPlayer_Defeated4FC205 for
//! the local House) is clear, and BBA (C019C8) for every other mode. Top
//! buttons enrol in60B000 (B5609253, BBA609291, FF60954B); Resume686/675
//! uses60B350 (FF609A25). FF's group box, trackbars and statics take the
//! ordinary60B7A0 placement. App owns action dispatch.

use super::geom::{self, RectPx};
use super::in_game_shell::{self, InGameShellLayout};
use super::trackbar::TrackbarRange;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum PauseMenuButton {
    GameControls,
    Load,
    Save,
    Delete,
    Abort,
    Resume,
    /// VERA relay control, independent of the native local Options dialog.
    SharedPause,
}

impl PauseMenuButton {
    pub const NATIVE: [Self; 6] = [
        Self::GameControls,
        Self::Load,
        Self::Save,
        Self::Delete,
        Self::Abort,
        Self::Resume,
    ];
    pub const ALL: [Self; 7] = [
        Self::GameControls,
        Self::Load,
        Self::Save,
        Self::Delete,
        Self::Abort,
        Self::Resume,
        Self::SharedPause,
    ];
}

/// The RT_DIALOG4F10E0 begins for the running mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PauseMenuTemplate {
    /// B5: campaign and skirmish.
    #[default]
    Offline,
    /// BBA: LAN, and WOL once the local player is defeated.
    Network,
    /// FF: WOL with the Internet Game Controls sliders.
    Internet,
}

impl PauseMenuTemplate {
    /// `0x004F10E1..0x004F1130`: GameMode0/5 take B5; mode4 takes FF unless
    /// Session+0x300 is set, then BBA; every other mode takes BBA.
    pub const fn select(offline: bool, wol: bool, local_defeated: bool) -> Self {
        if offline {
            Self::Offline
        } else if wol && !local_defeated {
            Self::Internet
        } else {
            Self::Network
        }
    }

    /// The template's buttons in resource order of their sidebar rows, Resume
    /// last.
    pub const fn buttons(self) -> &'static [PauseMenuButton] {
        match self {
            Self::Offline => &PauseMenuButton::NATIVE,
            Self::Network | Self::Internet => &[
                PauseMenuButton::GameControls,
                PauseMenuButton::Abort,
                PauseMenuButton::Resume,
            ],
        }
    }

    pub fn control(self, button: PauseMenuButton) -> Option<&'static PauseMenuControl> {
        let table: &[PauseMenuControl] = match self {
            Self::Offline => &B5_BUTTONS,
            Self::Network => &BBA_BUTTONS,
            Self::Internet => &FF_BUTTONS,
        };
        let index = self.buttons().iter().position(|b| *b == button)?;
        Some(&table[index])
    }

    pub const fn title(self) -> PauseMenuControl {
        match self {
            Self::Offline | Self::Network => PauseMenuControl {
                id: 0x694,
                csf_key: "GUI:GameOptions",
                help: None,
                dlu_rect: RectPx::new(425, 1, 108, 10),
            },
            Self::Internet => PauseMenuControl {
                id: 0x694,
                csf_key: "GUI:OptionsInternet",
                help: None,
                dlu_rect: RectPx::new(423, 4, 108, 11),
            },
        }
    }
}

/// One retained owner-draw state per button; eligibility is supplied by the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PauseMenuButtonState {
    pub pressed: bool,
    pub highlighted: bool,
    pub enabled: bool,
}

pub type PauseMenuInteraction = super::button::ShellButtonInteraction<PauseMenuButton>;

impl Default for PauseMenuButtonState {
    fn default() -> Self {
        Self {
            pressed: false,
            highlighted: false,
            enabled: true,
        }
    }
}

/// A template control: id, CSF caption, executed `0x006040B0` help key and
/// resource DLU rect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PauseMenuControl {
    pub id: u16,
    pub csf_key: &'static str,
    pub help: Option<&'static str>,
    pub dlu_rect: RectPx,
}

const fn button(
    id: u16,
    csf_key: &'static str,
    help: &'static str,
    dlu_rect: RectPx,
) -> PauseMenuControl {
    PauseMenuControl {
        id,
        csf_key,
        help: Some(help),
        dlu_rect,
    }
}

/// B5 in [`PauseMenuButton::NATIVE`] order.
const B5_BUTTONS: [PauseMenuControl; 6] = [
    button(
        0x521,
        "GUI:GameControls",
        "STT:IGSingleOptButtonControls",
        RectPx::new(425, 122, 108, 23),
    ),
    button(
        0x51e,
        "GUI:LoadGame",
        "STT:IGSingleOptButtonLoad",
        RectPx::new(425, 149, 108, 23),
    ),
    button(
        0x51f,
        "GUI:SaveGame",
        "STT:IGSingleOptButtonSave",
        RectPx::new(425, 176, 108, 23),
    ),
    button(
        0x520,
        "GUI:DeleteGame",
        "STT:IGSingleOptButtonDelete",
        RectPx::new(425, 203, 108, 23),
    ),
    button(
        0x522,
        "GUI:AbortMission",
        "STT:IGSingleOptButtonAbort",
        RectPx::new(425, 230, 108, 23),
    ),
    button(
        0x686,
        "GUI:ResumeMission",
        "STT:IGSingleOptButtonBack",
        RectPx::new(425, 346, 108, 23),
    ),
];

/// BBA (C019C8, five controls): Game Controls, Abort, Resume.
const BBA_BUTTONS: [PauseMenuControl; 3] = [
    button(
        0x521,
        "GUI:GameControls",
        "STT:IGMultiOptButtonControls",
        RectPx::new(425, 122, 108, 23),
    ),
    button(
        0x522,
        "GUI:AbortMission",
        "STT:IGMultiOptButtonAbort",
        RectPx::new(425, 149, 108, 23),
    ),
    button(
        0x686,
        "GUI:ResumeMission",
        "STT:IGMultiOptButtonResume",
        RectPx::new(425, 346, 108, 23),
    ),
];

/// FF (BFAB00, twelve controls): Resume is control675, not686.
const FF_BUTTONS: [PauseMenuControl; 3] = [
    button(
        0x521,
        "GUI:GameControls",
        "STT:OptionInternetGameControls",
        RectPx::new(424, 128, 108, 23),
    ),
    button(
        0x522,
        "GUI:AbortMission",
        "STT:OptionInternetAbortMission",
        RectPx::new(424, 154, 108, 23),
    ),
    button(
        0x675,
        "GUI:ResumeMission",
        "STT:IGMultiOptButtonResume",
        RectPx::new(425, 346, 108, 23),
    ),
];

pub const PAUSE_MENU_FOOTER: PauseMenuControl = PauseMenuControl {
    id: 0x695,
    csf_key: "GUI:Blank",
    help: None,
    dlu_rect: RectPx::new(6, 355, 303, 12),
};

/// FF's two `msctls_trackbar32` children.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum InternetSlider {
    /// 0x3EF, GUI:Connection: position `3 - LatencyFudge`.
    Connection,
    /// 0x529, GUI:GameSpeed: position `6 - GameSpeed`.
    Speed,
}

impl InternetSlider {
    pub const ALL: [Self; 2] = [Self::Connection, Self::Speed];

    /// Init4F1720 sends `TBM_SETRANGE (1, MAKELONG(0, n))`: 0..3 and 0..6. It
    /// never sends `0x4AC`, so both keep the default value plaque.
    pub const fn range(self) -> TrackbarRange {
        match self {
            Self::Connection => TrackbarRange::new(0, 3, 1),
            Self::Speed => TrackbarRange::new(0, 6, 1),
        }
    }

    pub const fn help(self) -> &'static str {
        match self {
            Self::Connection => "STT:OptionInternetLatency",
            Self::Speed => "STT:OptionInternetSpeedSlider",
        }
    }

    /// The left caption static (template id -1).
    pub const fn caption(self) -> (&'static str, &'static str) {
        match self {
            Self::Connection => ("GUI:Connection", "Connection"),
            Self::Speed => ("GUI:GameSpeed", "Game Speed"),
        }
    }

    /// The right static (0x672 / 0x671): the position's label from the
    /// tables at 0x00823FD4 and 0x00822730 (callback4F11D9..4F1250). Init's
    /// `TBM_SETRANGE` already notifies (61E609..61E6AF), so the template
    /// caption `GUI:Better` / `GUI:Faster` never shows.
    pub const fn value_caption(self, position: i32) -> (&'static str, &'static str) {
        match (self, position) {
            (Self::Connection, 0) => ("TXT_WORST_CONNECTION", "Worst"),
            (Self::Connection, 1) => ("TXT_POOR_CONNECTION", "Poor"),
            (Self::Connection, 2) => ("TXT_GOOD_CONNECTION", "Good"),
            (Self::Connection, _) => ("TXT_BEST_CONNECTION", "Best"),
            (Self::Speed, 0) => ("TXT_SLOWEST", "Slowest"),
            (Self::Speed, 1) => ("TXT_SLOWER", "Slower"),
            (Self::Speed, 2) => ("TXT_SLOW", "Slow"),
            (Self::Speed, 3) => ("TXT_MEDIUM", "Medium"),
            (Self::Speed, 4) => ("TXT_FAST", "Fast"),
            (Self::Speed, 5) => ("TXT_FASTER", "Faster"),
            (Self::Speed, _) => ("TXT_FASTEST", "Fastest"),
        }
    }
}

/// FF's ordinary children, each placed by60B7A0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InternetControlsLayout {
    /// The `BS_GROUPBOX` captioned GUI:InternetGameControls.
    pub group: RectPx,
    pub sliders: [RectPx; 2],
    pub captions: [RectPx; 2],
    pub values: [RectPx; 2],
}

impl InternetControlsLayout {
    pub fn slider_at(&self, x: i32, y: i32) -> Option<InternetSlider> {
        InternetSlider::ALL
            .into_iter()
            .find(|slider| self.sliders[*slider as usize].contains(x, y))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PauseMenuLayout {
    pub template: PauseMenuTemplate,
    buttons: [Option<RectPx>; 7],
    pub title: RectPx,
    pub footer: RectPx,
    pub internet: Option<InternetControlsLayout>,
}

impl PauseMenuLayout {
    pub fn button(&self, button: PauseMenuButton) -> Option<RectPx> {
        self.buttons[button as usize]
    }

    /// The template's buttons with their rectangles.
    pub fn buttons(&self) -> impl Iterator<Item = (PauseMenuButton, RectPx)> + '_ {
        self.template
            .buttons()
            .iter()
            .chain(std::iter::once(&PauseMenuButton::SharedPause))
            .filter_map(|button| Some((*button, self.button(*button)?)))
    }

    /// Add one relay control in the next unused sidebar row. Native controls,
    /// including the local Resume button, keep their original rectangles.
    pub fn enable_shared_pause(&mut self) {
        if self.template == PauseMenuTemplate::Offline {
            return;
        }
        let controls = self.button(PauseMenuButton::GameControls).unwrap();
        let abort = self.button(PauseMenuButton::Abort).unwrap();
        self.buttons[PauseMenuButton::SharedPause as usize] = Some(RectPx::new(
            abort.x,
            abort.y + (abort.y - controls.y),
            abort.w,
            abort.h,
        ));
    }
}

/// Layout uses the same loaded-art geometry as the parent compositor. Control
/// rectangles are physical pixels; no battlefield camera applies.
pub fn pause_menu_layout(
    width: i32,
    height: i32,
    shell: InGameShellLayout,
    button_size: [i32; 2],
    template: PauseMenuTemplate,
) -> PauseMenuLayout {
    let mut buttons = [None; 7];
    for button in template.buttons() {
        let control = template.control(*button).expect("template button");
        let dlu = control.dlu_rect;
        buttons[*button as usize] = Some(if *button == PauseMenuButton::Resume {
            shell.lower_button_rect(width, button_size)
        } else {
            shell.button_rect(geom::dlu_rect(dlu.x, dlu.y, dlu.w, dlu.h), button_size)
        });
    }
    let ordinary =
        |x, y, w, h| in_game_shell::ordinary_child_rect(geom::dlu_rect(x, y, w, h), width, height);
    let internet = (template == PauseMenuTemplate::Internet).then(|| InternetControlsLayout {
        group: ordinary(58, 118, 301, 68),
        sliders: [ordinary(134, 136, 140, 13), ordinary(134, 158, 140, 13)],
        captions: [ordinary(61, 136, 67, 13), ordinary(61, 158, 67, 13)],
        values: [ordinary(278, 136, 78, 13), ordinary(278, 158, 78, 13)],
    });
    PauseMenuLayout {
        template,
        buttons,
        title: in_game_shell::right_panel_title_rect(width),
        footer: in_game_shell::status_line_rect(height),
        internet,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::shell::in_game_shell::InGameShellSizes;

    fn geometry() -> serde_json::Value {
        serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/in_game_shell_geometry.json",
        ))
        .unwrap()
    }

    fn shell_for(fixture: &serde_json::Value, case: &serde_json::Value) -> InGameShellLayout {
        let width = case["width"].as_i64().unwrap() as i32;
        let height = case["height"].as_i64().unwrap() as i32;
        let assets = &fixture["assets"][case["theme"].as_str().unwrap()];
        let size = |address: &str| {
            [
                assets[address]["width"].as_i64().unwrap() as i32,
                assets[address]["height"].as_i64().unwrap() as i32,
            ]
        };
        InGameShellLayout::new(
            width,
            height,
            InGameShellSizes {
                background_small: size("00b0fad4"),
                background_medium: size("00b0fac8"),
                background_large: size("00b0fa50"),
                credits: size("00b0fb08"),
                top: size("00b0f9e0"),
                radar: size("00b0fa68"),
                side1: size("00b0fa70"),
                side2: size("00b0fafc"),
                side3: size("00b0fa8c"),
                addon: size("00b0fa48"),
                bottom_spacer: size("00b0fa3c"),
                left_cap: size("00b0fa90"),
                button_background: size("00b0faa8"),
                right_cap: size("00b0fabc"),
            },
        )
        .unwrap()
    }

    #[test]
    fn pause_rows_and_resume_follow_native_shell_geometry_at_each_stock_resolution() {
        let fixture = geometry();
        for case in fixture["cases"].as_array().unwrap() {
            let width = case["width"].as_i64().unwrap() as i32;
            let height = case["height"].as_i64().unwrap() as i32;
            let shell = shell_for(&fixture, case);
            let native_side3_y = case["rects"]["00b0fc4c"][1].as_i64().unwrap() as i32;
            let resume = RectPx::new(width - 147, native_side3_y - 25, 125, 25);
            let row = |row: i32| RectPx::new(width - 147, 227 + row * 25, 125, 25);

            let b5 = pause_menu_layout(width, height, shell, [125, 25], PauseMenuTemplate::Offline);
            for (index, button) in PauseMenuButton::ALL[..5].iter().enumerate() {
                assert_eq!(b5.button(*button), Some(row(index as i32)));
            }
            assert_eq!(b5.button(PauseMenuButton::Resume), Some(resume));
            assert_eq!(b5.title, RectPx::new(width - 165, 2, 162, 16));
            assert_eq!(b5.footer, RectPx::new(10, height - 21, 455, 20));
            assert!(b5.internet.is_none());
            let mut offline = b5;
            offline.enable_shared_pause();
            assert_eq!(offline, b5);

            // BBA (425,122)/(425,149) and FF (424,128)/(424,154) both land on
            // the first two60B000 rows; Load/Save/Delete are absent.
            for template in [PauseMenuTemplate::Network, PauseMenuTemplate::Internet] {
                let layout = pause_menu_layout(width, height, shell, [125, 25], template);
                assert_eq!(layout.button(PauseMenuButton::GameControls), Some(row(0)));
                assert_eq!(layout.button(PauseMenuButton::Abort), Some(row(1)));
                assert_eq!(layout.button(PauseMenuButton::Resume), Some(resume));
                let mut shared = layout;
                shared.enable_shared_pause();
                assert_eq!(shared.button(PauseMenuButton::SharedPause), Some(row(2)));
                assert_eq!(shared.button(PauseMenuButton::Resume), Some(resume));
                for absent in [
                    PauseMenuButton::Load,
                    PauseMenuButton::Save,
                    PauseMenuButton::Delete,
                ] {
                    assert_eq!(layout.button(absent), None);
                }
                assert_eq!(layout.title, b5.title);
                assert_eq!(layout.footer, b5.footer);
                assert_eq!(
                    layout.internet.is_some(),
                    template == PauseMenuTemplate::Internet
                );
            }
        }
    }

    #[test]
    fn template_follows_game_mode_and_local_defeat() {
        use PauseMenuTemplate::*;
        assert_eq!(PauseMenuTemplate::select(true, false, false), Offline);
        assert_eq!(PauseMenuTemplate::select(true, false, true), Offline);
        assert_eq!(PauseMenuTemplate::select(false, false, false), Network);
        assert_eq!(PauseMenuTemplate::select(false, true, false), Internet);
        assert_eq!(PauseMenuTemplate::select(false, true, true), Network);
        assert_eq!(Internet.control(PauseMenuButton::Resume).unwrap().id, 0x675);
        assert_eq!(Network.control(PauseMenuButton::Resume).unwrap().id, 0x686);
        assert_eq!(Internet.title().csf_key, "GUI:OptionsInternet");
        assert!(Network.control(PauseMenuButton::Save).is_none());
    }

    /// Resource controls, help keys and ordinary-child placements of BBA and
    /// FF, from tools/storage_oracle/pause_menu_layout.json.
    #[test]
    fn network_templates_match_the_retail_resources_and_native_placements() {
        let fixture: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/pause_menu_layout.json",
        ))
        .unwrap();
        for (template, dialog) in [
            (PauseMenuTemplate::Network, 0xBBA),
            (PauseMenuTemplate::Internet, 0xFF),
        ] {
            let controls = fixture["dialogs"][format!("{dialog:#x}")]["controls"]
                .as_array()
                .unwrap();
            let find = |id: u16| {
                controls
                    .iter()
                    .find(|c| c["control_id"] == u64::from(id))
                    .unwrap_or_else(|| panic!("{dialog:#x} has {id:#x}"))
            };
            for button in template.buttons() {
                let control = template.control(*button).unwrap();
                let native = find(control.id);
                assert_eq!(native["caption"], control.csf_key, "{dialog:#x}");
                assert_eq!(native["help"].as_str(), control.help, "{dialog:#x}");
                let r = control.dlu_rect;
                assert_eq!(native["dlu_rect"], serde_json::json!([r.x, r.y, r.w, r.h]));
            }
            assert_eq!(find(0x694)["caption"], template.title().csf_key);
            // Every control is a button, a title/footer static or FF's
            // Internet Game Controls group.
            let expected = if template == PauseMenuTemplate::Internet {
                12
            } else {
                5
            };
            assert_eq!(controls.len(), expected);
        }
        let ff = &fixture["dialogs"]["0xff"];
        for slider in InternetSlider::ALL {
            let id = match slider {
                InternetSlider::Connection => 0x3EF,
                InternetSlider::Speed => 0x529,
            };
            let native = ff["controls"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["control_id"] == id)
                .unwrap();
            assert_eq!(native["help"].as_str(), Some(slider.help()));
        }
        for case in ff["cases"].as_array().unwrap() {
            let width = case["width"].as_i64().unwrap() as i32;
            let height = case["height"].as_i64().unwrap() as i32;
            let native: Vec<i32> = case["rect"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_i64().unwrap() as i32)
                .collect();
            let native = RectPx::new(native[0], native[1], native[2], native[3]);
            let shell = shell_for(&geometry(), &geometry()["cases"][0]);
            let layout =
                pause_menu_layout(width, height, shell, [125, 25], PauseMenuTemplate::Internet)
                    .internet
                    .unwrap();
            let dlu: Vec<i32> = case["dlu_rect"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_i64().unwrap() as i32)
                .collect();
            let ours = match (case["control_id"].as_u64().unwrap(), dlu[0], dlu[1]) {
                (0x3EF, ..) => layout.sliders[0],
                (0x529, ..) => layout.sliders[1],
                (0x672, ..) => layout.values[0],
                (0x671, ..) => layout.values[1],
                (0xFFFF, 58, _) => layout.group,
                (0xFFFF, 61, 136) => layout.captions[0],
                (0xFFFF, 61, 158) => layout.captions[1],
                _ => continue,
            };
            assert_eq!(ours, native, "{case}");
        }
    }
}

//! Offline pause dialog B5: original resource VA0x00BEF9D0, eight controls.
//! 4F10E0 selects B5 for offline modes0/5. 609253..60928C enrolls the five
//! upper buttons in60B000; Resume686 uses60B350. App owns action dispatch.

use super::geom::{self, RectPx};
use super::in_game_shell::InGameShellLayout;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum PauseMenuButton {
    GameControls,
    Load,
    Save,
    Delete,
    Abort,
    Resume,
}

impl PauseMenuButton {
    pub const ALL: [Self; 6] = [
        Self::GameControls,
        Self::Load,
        Self::Save,
        Self::Delete,
        Self::Abort,
        Self::Resume,
    ];

    pub fn descriptor(self) -> &'static PauseMenuControl {
        &PAUSE_MENU_BUTTONS[self as usize]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PauseMenuControl {
    pub id: u16,
    pub csf_key: &'static str,
    pub dlu_rect: RectPx,
}

pub const PAUSE_MENU_BUTTONS: [PauseMenuControl; 6] = [
    PauseMenuControl {
        id: 0x521,
        csf_key: "GUI:GameControls",
        dlu_rect: RectPx::new(425, 122, 108, 23),
    },
    PauseMenuControl {
        id: 0x51e,
        csf_key: "GUI:LoadGame",
        dlu_rect: RectPx::new(425, 149, 108, 23),
    },
    PauseMenuControl {
        id: 0x51f,
        csf_key: "GUI:SaveGame",
        dlu_rect: RectPx::new(425, 176, 108, 23),
    },
    PauseMenuControl {
        id: 0x520,
        csf_key: "GUI:DeleteGame",
        dlu_rect: RectPx::new(425, 203, 108, 23),
    },
    PauseMenuControl {
        id: 0x522,
        csf_key: "GUI:AbortMission",
        dlu_rect: RectPx::new(425, 230, 108, 23),
    },
    PauseMenuControl {
        id: 0x686,
        csf_key: "GUI:ResumeMission",
        dlu_rect: RectPx::new(425, 346, 108, 23),
    },
];
pub const PAUSE_MENU_TITLE: PauseMenuControl = PauseMenuControl {
    id: 0x694,
    csf_key: "GUI:GameOptions",
    dlu_rect: RectPx::new(425, 1, 108, 10),
};
pub const PAUSE_MENU_FOOTER: PauseMenuControl = PauseMenuControl {
    id: 0x695,
    csf_key: "GUI:Blank",
    dlu_rect: RectPx::new(6, 355, 303, 12),
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PauseMenuLayout {
    pub buttons: [RectPx; 6],
    pub title: RectPx,
    pub footer: RectPx,
}

/// Layout uses the same loaded-art geometry as the parent compositor. Control
/// rectangles are physical pixels; no egui scale or battlefield camera applies.
pub fn pause_menu_layout(
    width: i32,
    height: i32,
    shell: InGameShellLayout,
    button_size: [i32; 2],
) -> PauseMenuLayout {
    let buttons = PauseMenuButton::ALL.map(|button| {
        let dlu = button.descriptor().dlu_rect;
        let raw = geom::dlu_rect(dlu.x, dlu.y, dlu.w, dlu.h);
        if button == PauseMenuButton::Resume {
            // Original60B3FC..60B41C: lower anchor is SIDE3.y, not viewportbottom.
            RectPx::new(
                width - 147,
                shell.side3.y - button_size[1],
                button_size[0],
                button_size[1],
            )
        } else {
            shell.button_rect(raw, button_size)
        }
    });
    PauseMenuLayout {
        buttons,
        // Original60B1D0 active branch: no launcher title's +7 final adjustment.
        title: RectPx::new(width - 165, 2, 162, 16),
        // Original60B550 active branch retains the resource455x20 canvas,
        // places its left at10 and its bottom one pixel above the parent bottom.
        footer: RectPx::new(10, height - 21, 455, 20),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::shell::in_game_shell::InGameShellSizes;

    #[test]
    fn pause_rows_and_resume_follow_native_shell_geometry_at_each_stock_resolution() {
        let fixture: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/in_game_shell_geometry.json",
        ))
        .unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let width = case["width"].as_i64().unwrap() as i32;
            let height = case["height"].as_i64().unwrap() as i32;
            let assets = &fixture["assets"][case["theme"].as_str().unwrap()];
            let size = |address: &str| {
                [
                    assets[address]["width"].as_i64().unwrap() as i32,
                    assets[address]["height"].as_i64().unwrap() as i32,
                ]
            };
            let shell = InGameShellLayout::new(
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
            .unwrap();
            let layout = pause_menu_layout(width, height, shell, [125, 25]);
            for (row, rect) in layout.buttons[..5].iter().enumerate() {
                assert_eq!(
                    *rect,
                    RectPx::new(width - 147, 227 + row as i32 * 25, 125, 25)
                );
            }
            let native_side3_y = case["rects"]["00b0fc4c"][1].as_i64().unwrap() as i32;
            assert_eq!(
                layout.buttons[PauseMenuButton::Resume as usize],
                RectPx::new(width - 147, native_side3_y - 25, 125, 25)
            );
            assert_eq!(layout.title, RectPx::new(width - 165, 2, 162, 16));
            assert_eq!(layout.footer, RectPx::new(10, height - 21, 455, 20));
        }
    }

    #[test]
    fn b5_resource_has_six_buttons_and_two_statics() {
        assert_eq!(
            PauseMenuButton::ALL.map(|b| b.descriptor().id),
            [0x521, 0x51e, 0x51f, 0x520, 0x522, 0x686]
        );
        assert_eq!(
            PauseMenuButton::ALL.map(|b| b.descriptor().csf_key),
            [
                "GUI:GameControls",
                "GUI:LoadGame",
                "GUI:SaveGame",
                "GUI:DeleteGame",
                "GUI:AbortMission",
                "GUI:ResumeMission"
            ]
        );
        assert_eq!(PAUSE_MENU_TITLE.id, 0x694);
        assert_eq!(PAUSE_MENU_FOOTER.dlu_rect, RectPx::new(6, 355, 303, 12));
    }
}

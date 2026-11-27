//! Initial main-menu shell control identity and result lookups.
//!
//! The buttons are `vera20k-gui` nodes built from `MAIN_MENU_PAGE` (captions,
//! status help); this module keeps the control identity and the
//! action/result-code tables the activated control id maps through.

use crate::ui::shell::controller::DialogController;
use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::menu_page;

/// The main-menu dialog resource.
pub const MAIN_MENU_DIALOG: DialogResourceId = DialogResourceId(0x00E2);

/// Browser launchers select the edition outside the engine, and Android
/// cannot restart itself through the desktop restart path.
pub(crate) const HAS_EDITION_SWITCH: bool = !cfg!(any(
    target_os = "android",
    all(target_arch = "wasm32", target_os = "unknown")
));
/// VERA-only id, outside the retail controls on this dialog.
pub(super) const SWITCH_EDITION: u16 = 0x7FA0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainMenuControlId {
    SinglePlayer0x683,
    WwOnline0x684,
    Network0x578,
    MoviesAndCredits0x686,
    Options0x55c,
    SwitchEdition,
    ExitGame0x3ee,
}

impl MainMenuControlId {
    /// The Win32 control resource id this identity stands for. The controller
    /// works in raw resource ids; the app maps back via [`Self::from_resource_id`].
    pub fn resource_id(self) -> u16 {
        match self {
            Self::SinglePlayer0x683 => 0x0683,
            Self::WwOnline0x684 => 0x0684,
            Self::Network0x578 => 0x0578,
            Self::MoviesAndCredits0x686 => 0x0686,
            Self::Options0x55c => 0x055C,
            Self::SwitchEdition => SWITCH_EDITION,
            Self::ExitGame0x3ee => 0x03EE,
        }
    }

    /// Inverse of [`Self::resource_id`]; `None` for an unknown id.
    pub fn from_resource_id(id: u16) -> Option<Self> {
        Some(match id {
            0x0683 => Self::SinglePlayer0x683,
            0x0684 => Self::WwOnline0x684,
            0x0578 => Self::Network0x578,
            0x0686 => Self::MoviesAndCredits0x686,
            0x055C => Self::Options0x55c,
            SWITCH_EDITION if HAS_EDITION_SWITCH => Self::SwitchEdition,
            0x03EE => Self::ExitGame0x3ee,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainMenuShellAction {
    None,
    SinglePlayer,
    WwOnline,
    Network,
    MoviesAndCredits,
    Options,
    SwitchEdition,
    ExitGame,
}

#[derive(Debug, Clone, Default)]
pub struct MainMenuShellState {
    /// Process-wide like `0x00A8F7B8`: a new 0xE2 keeps the typed letters.
    pub(crate) typed_codes: super::MainMenuTypedCodes,
}

/// The 0xE2 button held down, read from the controller's live 0xE2 instance.
/// `None` while no 0xE2 is live: after its destruction, and after a re-entry
/// until the first pointer message creates the new instance.
pub fn pressed_control(controller: &DialogController) -> Option<MainMenuControlId> {
    menu_page::pressed_id(controller, MAIN_MENU_DIALOG)
        .and_then(MainMenuControlId::from_resource_id)
}

/// The 0xE2 button under the cursor; it drives the bottom-left status line.
/// 0xE2 buttons have no hover frame: the focus-flash byte the paint path reads
/// is only toggled by a timer armed for a network-dialog control that does not
/// exist on this dialog.
pub fn hovered_control(controller: &DialogController) -> Option<MainMenuControlId> {
    menu_page::hovered_id(controller, MAIN_MENU_DIALOG)
        .and_then(MainMenuControlId::from_resource_id)
}

pub fn action_for_control(id: MainMenuControlId) -> MainMenuShellAction {
    match id {
        MainMenuControlId::SinglePlayer0x683 => MainMenuShellAction::SinglePlayer,
        MainMenuControlId::WwOnline0x684 => MainMenuShellAction::WwOnline,
        MainMenuControlId::Network0x578 => MainMenuShellAction::Network,
        MainMenuControlId::MoviesAndCredits0x686 => MainMenuShellAction::MoviesAndCredits,
        MainMenuControlId::Options0x55c => MainMenuShellAction::Options,
        MainMenuControlId::SwitchEdition => MainMenuShellAction::SwitchEdition,
        MainMenuControlId::ExitGame0x3ee => MainMenuShellAction::ExitGame,
    }
}

pub fn return_code_for_action(action: MainMenuShellAction) -> Option<i32> {
    match action {
        MainMenuShellAction::None | MainMenuShellAction::SwitchEdition => None,
        MainMenuShellAction::SinglePlayer => Some(1),
        MainMenuShellAction::WwOnline => Some(2),
        MainMenuShellAction::Network => Some(3),
        MainMenuShellAction::MoviesAndCredits => Some(4),
        MainMenuShellAction::Options => Some(5),
        MainMenuShellAction::ExitGame => Some(6),
    }
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;
    use crate::ui::main_menu_shell::compute_layout;
    use vera20k_gui::{InputEvent, MouseButton};

    /// 0xE2's live instance with its button nodes on the 800x600 layout.
    fn main_menu() -> DialogController {
        let page = compute_layout(800, 600).page();
        let mut c = DialogController::default();
        c.ensure_active(MAIN_MENU_DIALOG);
        menu_page::sync_nodes(
            &mut c,
            MAIN_MENU_DIALOG,
            || super::super::layout::nodes(false),
            |id| page.button_rect(id),
            &[],
        );
        c
    }

    fn down(c: &mut DialogController, x: i32, y: i32) {
        c.on_input(InputEvent::PointerDown {
            x,
            y,
            button: MouseButton::Left,
        });
    }

    fn pressed(c: &DialogController) -> Option<u16> {
        menu_page::pressed_id(c, MAIN_MENU_DIALOG)
    }

    #[test]
    fn button_actions_preserve_return_codes() {
        assert_eq!(
            return_code_for_action(action_for_control(MainMenuControlId::SinglePlayer0x683)),
            Some(1)
        );
        assert_eq!(
            return_code_for_action(action_for_control(MainMenuControlId::WwOnline0x684)),
            Some(2)
        );
        assert_eq!(
            return_code_for_action(action_for_control(MainMenuControlId::Network0x578)),
            Some(3)
        );
        assert_eq!(
            return_code_for_action(action_for_control(MainMenuControlId::MoviesAndCredits0x686)),
            Some(4)
        );
        assert_eq!(
            return_code_for_action(action_for_control(MainMenuControlId::Options0x55c)),
            Some(5)
        );
        assert_eq!(
            return_code_for_action(action_for_control(MainMenuControlId::ExitGame0x3ee)),
            Some(6)
        );
    }

    #[test]
    fn controller_hits_main_menu_buttons_by_geometry() {
        // Real layout geometry routed through the shared controller. SinglePlayer
        // cell (644,199,156,42); Exit cell (644,535,156,42). The flush-right cell's
        // exclusive right edge (x=800), the 632..644 gutter, and above-top all miss;
        // statics are no nodes, so the title and monitor never register as hits.
        let mut c = main_menu();
        down(&mut c, 700, 210);
        assert_eq!(
            pressed(&c),
            Some(MainMenuControlId::SinglePlayer0x683.resource_id())
        );
        if HAS_EDITION_SWITCH {
            down(&mut c, 700, 493);
            assert_eq!(pressed(&c), Some(SWITCH_EDITION));
            assert_eq!(
                action_for_control(MainMenuControlId::from_resource_id(SWITCH_EDITION).unwrap()),
                MainMenuShellAction::SwitchEdition
            );
        }
        down(&mut c, 700, 535);
        assert_eq!(
            pressed(&c),
            Some(MainMenuControlId::ExitGame0x3ee.resource_id())
        );
        down(&mut c, 800, 203);
        assert_eq!(pressed(&c), None);
        down(&mut c, 640, 210);
        assert_eq!(pressed(&c), None);
        down(&mut c, 700, 198);
        assert_eq!(pressed(&c), None);
        down(&mut c, 700, 577);
        assert_eq!(pressed(&c), None);
    }

    #[test]
    fn paint_state_follows_the_live_0xe2_instance_only() {
        let single_player = Some(MainMenuControlId::SinglePlayer0x683);
        let mut c = main_menu();
        c.on_input(InputEvent::PointerMove { x: 700, y: 210 });
        down(&mut c, 700, 210);
        // A message box over 0xE2 leaves 0xE2's own press and hover painted.
        c.push(DialogResourceId(0x0120));
        assert_eq!(pressed_control(&c), single_player);
        assert_eq!(hovered_control(&c), single_player);
        // Destroying 0xE2 under the box clears them for the next instance.
        c.restart_instance(MAIN_MENU_DIALOG);
        assert_eq!(pressed_control(&c), None);
        assert_eq!(hovered_control(&c), None);
        assert_eq!(c.top_id(), Some(DialogResourceId(0x0120)));
        // Back from Single Player: 0x100's hover is never read as 0xE2's.
        c.reset_to(DialogResourceId(0x0100));
        c.on_input(InputEvent::PointerMove { x: 700, y: 210 });
        assert_eq!(hovered_control(&c), None);
    }
}

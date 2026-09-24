//! Initial main-menu shell control identity and CSF/result lookups.
//!
//! Hit-testing and the press-must-match-release gesture moved to the shared
//! `ui::shell::controller::DialogController` (substrate Slice 2); this module
//! keeps the control identity, the CSF/tooltip keys, and the action/result-code
//! tables that the controller's activated-control id maps through.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainMenuControlId {
    SinglePlayer0x683,
    WwOnline0x684,
    Network0x578,
    MoviesAndCredits0x686,
    Options0x55c,
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
    ExitGame,
}

#[derive(Debug, Clone, Default)]
pub struct MainMenuShellState {
    pub pressed_owner_draw_button: Option<MainMenuControlId>,
    /// Control under the cursor right now, if any. Drives the bottom-left
    /// tooltip status line. Dialog 0xE2 buttons have no hover frame change:
    /// the focus-flash byte the paint path reads is only toggled by a timer
    /// armed for a network-dialog control that does not exist on this dialog,
    /// so hovering produces no visual change (default frame stays selected).
    pub hovered_owner_draw_button: Option<MainMenuControlId>,
    /// Retained, paint-driven kind-1 state for title static 0x694.
    pub(crate) title_reveal: crate::ui::shell::static_reveal::Kind1StaticReveal,
}

pub fn action_for_control(id: MainMenuControlId) -> MainMenuShellAction {
    match id {
        MainMenuControlId::SinglePlayer0x683 => MainMenuShellAction::SinglePlayer,
        MainMenuControlId::WwOnline0x684 => MainMenuShellAction::WwOnline,
        MainMenuControlId::Network0x578 => MainMenuShellAction::Network,
        MainMenuControlId::MoviesAndCredits0x686 => MainMenuShellAction::MoviesAndCredits,
        MainMenuControlId::Options0x55c => MainMenuShellAction::Options,
        MainMenuControlId::ExitGame0x3ee => MainMenuShellAction::ExitGame,
    }
}

pub fn return_code_for_action(action: MainMenuShellAction) -> Option<i32> {
    match action {
        MainMenuShellAction::None => None,
        MainMenuShellAction::SinglePlayer => Some(1),
        MainMenuShellAction::WwOnline => Some(2),
        MainMenuShellAction::Network => Some(3),
        MainMenuShellAction::MoviesAndCredits => Some(4),
        MainMenuShellAction::Options => Some(5),
        MainMenuShellAction::ExitGame => Some(6),
    }
}

pub fn csf_key_for_control(id: MainMenuControlId) -> &'static str {
    match id {
        MainMenuControlId::SinglePlayer0x683 => "GUI:SinglePlayer",
        MainMenuControlId::WwOnline0x684 => "GUI:WWOnline",
        MainMenuControlId::Network0x578 => "GUI:Network",
        MainMenuControlId::MoviesAndCredits0x686 => "GUI:MoviesAndCredits",
        MainMenuControlId::Options0x55c => "GUI:Options",
        MainMenuControlId::ExitGame0x3ee => "GUI:ExitGame",
    }
}

/// CSF key for the bottom-left hover-tooltip status line, looked up per
/// control when the cursor is over a main-menu owner-draw button.
pub fn tooltip_csf_key_for_control(id: MainMenuControlId) -> &'static str {
    match id {
        MainMenuControlId::SinglePlayer0x683 => "STT:MainButtonSinglePlayer",
        MainMenuControlId::WwOnline0x684 => "STT:MainButtonWWOnline",
        MainMenuControlId::Network0x578 => "STT:MainButtonNetwork",
        MainMenuControlId::MoviesAndCredits0x686 => "STT:MainButtonMovies",
        MainMenuControlId::Options0x55c => "STT:MainButtonOptions",
        MainMenuControlId::ExitGame0x3ee => "STT:MainButtonExitGamemd",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::main_menu_shell::compute_layout;
    use crate::ui::shell::controller::DialogController;
    use crate::ui::shell::descriptor::DialogId;
    use crate::ui::shell::layout::LaidOutControl;

    /// Adapt the laid-out main-menu buttons into the controller's button-only feed.
    fn button_feed(
        layout: &crate::ui::main_menu_shell::MainMenuShellLayout,
    ) -> Vec<LaidOutControl> {
        layout
            .buttons
            .iter()
            .map(|b| LaidOutControl {
                id: b.id.resource_id(),
                rect: b.rect,
            })
            .collect()
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
        // statics are never fed, so the title and monitor never register as hits.
        let layout = compute_layout(800, 600);
        let feed = button_feed(&layout);
        let mut c = DialogController::default();
        c.ensure_active(DialogId(0x00E2), false);
        c.on_pointer_down(700, 210, &feed);
        assert_eq!(
            c.pressed(),
            Some(MainMenuControlId::SinglePlayer0x683.resource_id())
        );
        c.on_pointer_down(700, 535, &feed);
        assert_eq!(
            c.pressed(),
            Some(MainMenuControlId::ExitGame0x3ee.resource_id())
        );
        c.on_pointer_down(800, 203, &feed);
        assert_eq!(c.pressed(), None);
        c.on_pointer_down(640, 210, &feed);
        assert_eq!(c.pressed(), None);
        c.on_pointer_down(700, 198, &feed);
        assert_eq!(c.pressed(), None);
        c.on_pointer_down(700, 577, &feed);
        assert_eq!(c.pressed(), None);
    }

    #[test]
    fn controller_hits_unscaled_large_screen_button_rects() {
        let layout = compute_layout(1024, 768);
        let feed = button_feed(&layout);
        let mut c = DialogController::default();
        c.ensure_active(DialogId(0x00E2), false);
        c.on_pointer_down(760, 300, &feed);
        assert_eq!(
            c.pressed(),
            Some(MainMenuControlId::SinglePlayer0x683.resource_id())
        );
        c.on_pointer_down(809, 255, &feed);
        assert_eq!(c.pressed(), None);
    }

    #[test]
    fn controller_release_must_match_pressed_button() {
        let layout = compute_layout(800, 600);
        let feed = button_feed(&layout);
        let mut c = DialogController::default();
        c.ensure_active(DialogId(0x00E2), false);
        // Press SinglePlayer, release over the WW Online row -> no fire.
        c.on_pointer_down(700, 210, &feed);
        assert_eq!(c.on_pointer_up(700, 250, &feed), None);
        // Press and release SinglePlayer -> the activated id maps to its action.
        c.on_pointer_down(700, 210, &feed);
        let activated = c.on_pointer_up(700, 210, &feed);
        assert_eq!(
            activated
                .and_then(MainMenuControlId::from_resource_id)
                .map(action_for_control),
            Some(MainMenuShellAction::SinglePlayer)
        );
    }
}

//! Dialog 0x100 Single Player control identity and dialog-proc results.
//!
//! The buttons are `vera20k-gui` nodes built from the page table in `layout`
//! (`ui::shell::menu_page`), which also gives the labels, status help and
//! results.

use super::layout::SINGLE_PLAYER_PAGE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinglePlayerControlId {
    NewCampaign0x688,
    LoadSavedGame0x689,
    Skirmish0x579,
    MainMenu0x686,
}

impl SinglePlayerControlId {
    /// The Win32 control resource id this identity stands for.
    pub fn resource_id(self) -> u16 {
        match self {
            Self::NewCampaign0x688 => 0x0688,
            Self::LoadSavedGame0x689 => 0x0689,
            Self::Skirmish0x579 => 0x0579,
            Self::MainMenu0x686 => 0x0686,
        }
    }

    /// Inverse of [`Self::resource_id`]; `None` for an unknown id.
    pub fn from_resource_id(id: u16) -> Option<Self> {
        Some(match id {
            0x0688 => Self::NewCampaign0x688,
            0x0689 => Self::LoadSavedGame0x689,
            0x0579 => Self::Skirmish0x579,
            0x0686 => Self::MainMenu0x686,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinglePlayerShellAction {
    None,
    NewCampaign,
    LoadSavedGame,
    Skirmish,
    MainMenu,
}

/// Runtime state the page table cannot carry: the `0x497` Load Saved Game
/// enable refresh, which the button node follows.
#[derive(Debug, Clone, Default)]
pub struct SinglePlayerShellState {
    pub load_saved_game_enabled: bool,
}

pub fn action_for_control(id: SinglePlayerControlId) -> SinglePlayerShellAction {
    match id {
        SinglePlayerControlId::NewCampaign0x688 => SinglePlayerShellAction::NewCampaign,
        SinglePlayerControlId::LoadSavedGame0x689 => SinglePlayerShellAction::LoadSavedGame,
        SinglePlayerControlId::Skirmish0x579 => SinglePlayerShellAction::Skirmish,
        SinglePlayerControlId::MainMenu0x686 => SinglePlayerShellAction::MainMenu,
    }
}

pub fn return_code_for_action(action: SinglePlayerShellAction) -> Option<i32> {
    let id = match action {
        SinglePlayerShellAction::None => return None,
        SinglePlayerShellAction::NewCampaign => SinglePlayerControlId::NewCampaign0x688,
        SinglePlayerShellAction::LoadSavedGame => SinglePlayerControlId::LoadSavedGame0x689,
        SinglePlayerShellAction::Skirmish => SinglePlayerControlId::Skirmish0x579,
        SinglePlayerShellAction::MainMenu => SinglePlayerControlId::MainMenu0x686,
    };
    SINGLE_PLAYER_PAGE
        .button(id.resource_id())
        .and_then(|button| button.result)
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;
    use crate::ui::shell::controller::DialogController;
    use crate::ui::shell::menu_page;
    use vera20k_gui::{InputEvent, MouseButton};

    const DIALOG: crate::ui::shell::descriptor::DialogResourceId = SINGLE_PLAYER_PAGE.dialog;

    /// 0x100's button nodes on the 800x600 layout, Load Saved Game enabled
    /// or not (`0x497`).
    fn sync(c: &mut DialogController, load_enabled: bool) {
        let layout = crate::ui::single_player_shell::compute_layout(800, 600);
        menu_page::sync_nodes(
            c,
            DIALOG,
            || SINGLE_PLAYER_PAGE.nodes(),
            |id| layout.button_rect(id),
            &[(
                SinglePlayerControlId::LoadSavedGame0x689.resource_id(),
                load_enabled,
            )],
        );
    }

    fn single_player(load_enabled: bool) -> DialogController {
        let mut c = DialogController::default();
        c.ensure_active(DIALOG);
        sync(&mut c, load_enabled);
        c
    }

    fn input(c: &mut DialogController, event: InputEvent) -> Option<u16> {
        c.on_input(event);
        let events = c.drain_events();
        crate::ui::shell::page::page_events(c.gui(), events)
            .into_iter()
            .find_map(|event| match event {
                crate::ui::shell::page::PageEvent::Activated(id) => Some(id),
                _ => None,
            })
    }

    fn down(c: &mut DialogController, x: i32, y: i32) {
        input(
            c,
            InputEvent::PointerDown {
                x,
                y,
                button: MouseButton::Left,
            },
        );
    }

    fn up(c: &mut DialogController, x: i32, y: i32) -> Option<u16> {
        input(
            c,
            InputEvent::PointerUp {
                x,
                y,
                button: MouseButton::Left,
            },
        )
    }

    fn hover(c: &mut DialogController, x: i32, y: i32) {
        input(c, InputEvent::PointerMove { x, y });
    }

    #[test]
    fn command_results_match_dialog_proc_0x52d640() {
        assert_eq!(
            return_code_for_action(action_for_control(SinglePlayerControlId::NewCampaign0x688)),
            Some(8)
        );
        assert_eq!(
            return_code_for_action(action_for_control(
                SinglePlayerControlId::LoadSavedGame0x689
            )),
            Some(9)
        );
        assert_eq!(
            return_code_for_action(action_for_control(SinglePlayerControlId::Skirmish0x579)),
            Some(0x0B)
        );
        assert_eq!(
            return_code_for_action(action_for_control(SinglePlayerControlId::MainMenu0x686)),
            Some(0x12)
        );
    }

    #[test]
    fn controller_hits_dialog_0x100_buttons_by_geometry() {
        let mut c = single_player(true);
        down(&mut c, 639, 204);
        assert_eq!(menu_page::pressed_id(&c, DIALOG), None);
        down(&mut c, 644, 204);
        assert_eq!(
            menu_page::pressed_id(&c, DIALOG),
            Some(SinglePlayerControlId::NewCampaign0x688.resource_id())
        );
        down(&mut c, 644, 290);
        assert_eq!(
            menu_page::pressed_id(&c, DIALOG),
            Some(SinglePlayerControlId::Skirmish0x579.resource_id())
        );
        down(&mut c, 644, 540);
        assert_eq!(
            menu_page::pressed_id(&c, DIALOG),
            Some(SinglePlayerControlId::MainMenu0x686.resource_id())
        );
    }

    #[test]
    fn controller_disabled_load_saved_game_suppresses_press_but_still_hovers() {
        let load = SinglePlayerControlId::LoadSavedGame0x689.resource_id();
        // Disabled (no saves): press suppressed, no action emitted...
        let mut c = single_player(false);
        down(&mut c, 639, 248);
        assert_eq!(menu_page::pressed_id(&c, DIALOG), None);
        assert_eq!(up(&mut c, 639, 248), None);
        hover(&mut c, 639, 248);
        assert_eq!(menu_page::hovered_id(&c, DIALOG), None);
        // ...but the disabled button still hover-tracks at its native boundary.
        hover(&mut c, 644, 248);
        assert_eq!(menu_page::hovered_id(&c, DIALOG), Some(load));
        // Enabled: press-and-release fires Load Saved Game.
        sync(&mut c, true);
        down(&mut c, 644, 248);
        let activated = up(&mut c, 644, 248);
        assert_eq!(
            activated
                .and_then(SinglePlayerControlId::from_resource_id)
                .map(action_for_control),
            Some(SinglePlayerShellAction::LoadSavedGame)
        );
    }
}

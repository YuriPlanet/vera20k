//! Dialog 0x100 Single Player control identity and dialog-proc results.
//!
//! Hit-testing, press/hover and the Load Saved Game disabled guard live in the
//! shared `ui::shell::controller::DialogController`; labels, status help and
//! results come from the page table in `layout`.

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

/// Runtime state the page table cannot carry. Press/hover live in the shared
/// controller; this is only the `0x497` Load Saved Game enable refresh.
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
        .map(|button| button.result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::shell::controller::DialogController;
    use crate::ui::shell::descriptor::DialogId;
    use crate::ui::shell::layout::LaidOutControl;
    use crate::ui::single_player_shell::compute_layout;

    fn button_feed(layout: &crate::ui::shell::menu_page::MenuPageLayout) -> Vec<LaidOutControl> {
        layout
            .buttons
            .iter()
            .map(|b| LaidOutControl {
                id: b.id,
                rect: b.rect,
            })
            .collect()
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
    fn every_page_button_maps_to_a_typed_control() {
        for button in SINGLE_PLAYER_PAGE.buttons() {
            let id = SinglePlayerControlId::from_resource_id(button.id).expect("typed control");
            assert_eq!(id.resource_id(), button.id);
        }
    }

    #[test]
    fn status_help_keys_match_dialog_0x100_control_mapping() {
        let key = |id: SinglePlayerControlId| {
            SINGLE_PLAYER_PAGE
                .button(id.resource_id())
                .map(|button| button.tooltip_key)
        };
        assert_eq!(
            key(SinglePlayerControlId::NewCampaign0x688),
            Some("STT:SingleButtonNewCampaign")
        );
        assert_eq!(
            key(SinglePlayerControlId::LoadSavedGame0x689),
            Some("STT:SingleButtonLoadSavedGame")
        );
        assert_eq!(
            key(SinglePlayerControlId::Skirmish0x579),
            Some("STT:SingleButtonSkirmish")
        );
        assert_eq!(
            key(SinglePlayerControlId::MainMenu0x686),
            Some("STT:SingleButtonBack")
        );
    }

    #[test]
    fn controller_hits_dialog_0x100_buttons_by_geometry() {
        let layout = compute_layout(800, 600);
        let feed = button_feed(&layout);
        let mut c = DialogController::default();
        c.ensure_active(DialogId(0x0100), false);
        c.on_pointer_down(639, 204, &feed);
        assert_eq!(c.pressed(), None);
        c.on_pointer_down(644, 204, &feed);
        assert_eq!(
            c.pressed(),
            Some(SinglePlayerControlId::NewCampaign0x688.resource_id())
        );
        c.on_pointer_down(644, 290, &feed);
        assert_eq!(
            c.pressed(),
            Some(SinglePlayerControlId::Skirmish0x579.resource_id())
        );
        c.on_pointer_down(644, 540, &feed);
        assert_eq!(
            c.pressed(),
            Some(SinglePlayerControlId::MainMenu0x686.resource_id())
        );
    }

    #[test]
    fn controller_disabled_load_saved_game_suppresses_press_but_still_hovers() {
        let layout = compute_layout(800, 600);
        let feed = button_feed(&layout);
        let load = SinglePlayerControlId::LoadSavedGame0x689.resource_id();
        let mut c = DialogController::default();
        c.ensure_active(DialogId(0x0100), false);
        // Disabled (no saves): press suppressed, no action emitted...
        c.set_disabled(load, true);
        c.on_pointer_down(639, 248, &feed);
        assert_eq!(c.pressed(), None);
        assert_eq!(c.on_pointer_up(639, 248, &feed), None);
        c.on_pointer_move(639, 248, &feed);
        assert_eq!(c.hovered(), None);
        // ...but the disabled button still hover-tracks at its native boundary.
        c.on_pointer_move(644, 248, &feed);
        assert_eq!(c.hovered(), Some(load));
        // Enabled: press-and-release fires Load Saved Game.
        c.set_disabled(load, false);
        c.on_pointer_down(644, 248, &feed);
        let activated = c.on_pointer_up(644, 248, &feed);
        assert_eq!(
            activated
                .and_then(SinglePlayerControlId::from_resource_id)
                .map(action_for_control),
            Some(SinglePlayerShellAction::LoadSavedGame)
        );
    }
}

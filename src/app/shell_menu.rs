//! VERA's Esc menu: Esc on a front-end page asks, over that page, for Options,
//! Exit Game or Cancel, and offers Full Screen, Performance Stats and Language
//! from the Options page and a Set Window Size button, all of which take
//! effect at once.
//!
//! gamemd has no such dialog: most of its shell dialogs ignore IDCANCEL, and
//! Skirmish and the network rooms take it as Back. The menu is made of retail
//! pieces: the three-button message box `0x121` on the Allied dialog art,
//! launcher Options `0xD5` and the Main Menu's Exit Game quit. Each page
//! answers whether Esc is free, so its own boxes keep the key; the page's
//! widgets then see it first, so an open drop-down takes it and closes.

use winit::event::KeyEvent;
use winit::keyboard::{KeyCode, PhysicalKey};

use super::{App, AppState};
use crate::app::frontend::shell_layout;
use crate::app::frontend::shell_transition;
use crate::app::frontend::state::{ShellMessageBox, ShellMessageBoxPurpose};
use crate::app::input::precedence::ScreenPage;
use crate::ui::main_menu_dialogs::{OptionsDialogState, shell_menu};
use crate::ui::shell::controller::gui_input;
use crate::ui::shell::keyboard::KeyboardParent;

impl App {
    /// An Esc press on `screen`. True when the key was used: the page's
    /// widgets took it (an open drop-down closes, without a cue), or it
    /// opened the menu.
    pub(super) fn shell_menu_key(
        state: &mut AppState,
        event: &KeyEvent,
        screen: ScreenPage,
    ) -> bool {
        if event.physical_key != PhysicalKey::Code(KeyCode::Escape)
            || !event.state.is_pressed()
            || event.repeat
            || state.frontend.shell_message_box.is_some()
            || shell_transition::current_shell_slide_target(state).is_none()
            || !Self::page_leaves_escape(state, screen)
        {
            return false;
        }
        // My Information's lists stay open under the menu: its Esc never
        // reached them.
        let my_info = screen == ScreenPage::Wol
            && matches!(
                state.frontend.wol.as_ref().map(|visit| &visit.page),
                Some(super::shell_wol::WolPage::MyInformation(_))
            );
        let escape =
            gui_input(Some(KeyCode::Escape), false, event.text.as_deref()).filter(|_| !my_info);
        let controller = &mut state.frontend.shell_controller;
        if escape.is_some_and(|escape| controller.on_input(escape)) {
            return true;
        }
        // A focused edit reported the Escape as its cancel; the menu takes it.
        controller.discard_cancelled();
        Self::open_shell_menu(state);
        true
    }

    /// Movies, credits, Options, the Keyboard page, score screens and a match
    /// keep their own Esc.
    fn page_leaves_escape(state: &AppState, screen: ScreenPage) -> bool {
        let chooser_open = state
            .frontend
            .skirmish_shell_state
            .choose_map_modal
            .is_some();
        match screen {
            ScreenPage::MainMenu
            | ScreenPage::SinglePlayer
            | ScreenPage::MoviesAndCredits
            | ScreenPage::MovieList
            | ScreenPage::Mods
            | ScreenPage::Campaign => true,
            ScreenPage::LoadSavedGame => Self::load_saved_game_leaves_escape(state),
            ScreenPage::LanLobby => crate::app::shell_lan::leaves_escape(state),
            ScreenPage::NetworkSetup if !chooser_open => {
                crate::app::input::multiplayer_setup::leaves_escape(state)
            }
            ScreenPage::Skirmish | ScreenPage::NetworkSetup => Self::skirmish_leaves_escape(state),
            ScreenPage::Wol => Self::wol_leaves_escape(state),
            _ => false,
        }
    }

    fn open_shell_menu(state: &mut AppState) {
        // The box's art; without it nothing could draw or close the menu.
        Self::ensure_skirmish_shell_chrome(state);
        if state.frontend.skirmish_shell_chrome.is_none() {
            return;
        }
        let csf = |key: &str, fallback: &str| state.csf_label(key, fallback);
        let shell_box = shell_menu(&csf);
        let settings = Box::new(Self::launcher_options_from_state(state));
        Self::open_shell_message_box(
            state,
            shell_box,
            ShellMessageBoxPurpose::ShellMenu(settings),
        );
    }

    /// The pointer over the menu, for its settings: `pressed` is the left
    /// button's new state, `None` a move. True when an open list took a press,
    /// as a click outside the list only closes it.
    pub(super) fn shell_menu_pointer(state: &mut AppState, pressed: Option<bool>) -> bool {
        let size = shell_layout::shell_screen_size(state);
        let Some(settings) = state
            .frontend
            .shell_message_box
            .as_mut()
            .and_then(ShellMessageBox::menu_settings_mut)
        else {
            return false;
        };
        let layout = shell_layout::shell_menu_settings_layout(size, &state.renderer.bit_font);
        let (x, y) = state.match_state.input.cursor_px();
        let list_open = settings.shell_list_open();
        match pressed {
            Some(true) => settings.shell_mouse_down(x, y, &layout),
            Some(false) => settings.shell_mouse_up(x, y, &layout),
            None => settings.shell_mouse_move(x, y, &layout),
        }
        Self::run_shell_menu_settings(state);
        list_open && pressed == Some(true)
    }

    /// Esc closes the menu's open list before the menu.
    pub(super) fn close_shell_menu_list(state: &mut AppState) -> bool {
        if !Self::shell_menu_settings(state).is_some_and(|settings| settings.shell_close_list()) {
            return false;
        }
        Self::run_shell_menu_settings(state);
        true
    }

    fn shell_menu_settings(state: &mut AppState) -> Option<&mut OptionsDialogState> {
        state
            .frontend
            .shell_message_box
            .as_mut()
            .and_then(ShellMessageBox::menu_settings_mut)
    }

    /// What the settings did takes effect through the Options page's own
    /// path. Unlike that page, the menu keeps no accept to write them out at,
    /// so a language is written as it is picked.
    fn run_shell_menu_settings(state: &mut AppState) {
        let Some(settings) = Self::shell_menu_settings(state) else {
            return;
        };
        let output = settings.drain_output();
        if Self::run_launcher_options_events(state, output.cues, output.events) {
            let csf = |key: &str, fallback: &str| state.csf_label(key, fallback);
            let shell_box = shell_menu(&csf);
            let option_labels = Self::launcher_options_labels(state);
            if let Some(message_box) = state.frontend.shell_message_box.as_mut() {
                message_box.shell_box = shell_box;
                if let Some(settings) = message_box.menu_settings_mut() {
                    settings.set_labels(option_labels);
                }
            }
            crate::app::persistence::options::persist_options_profile(state);
        }
    }

    /// The menu's box shows what the window is. A change is written once the
    /// window has made it, which is what the box and the profile follow.
    pub(super) fn shell_menu_follow_full_screen(
        state: &mut AppState,
        full_screen: bool,
        changed: bool,
    ) {
        let Some(settings) = Self::shell_menu_settings(state) else {
            return;
        };
        settings.sync_full_screen(full_screen);
        if changed {
            crate::app::persistence::options::persist_options_profile(state);
        }
    }

    /// The stats' own hotkey works with the menu up.
    pub(crate) fn shell_menu_follow_performance_stats(state: &mut AppState, shown: bool) {
        if let Some(settings) = Self::shell_menu_settings(state) {
            settings.sync_performance_stats(shown);
        }
    }

    /// The menu has closed with `answer`: 0 is Options, 2 Exit Game, 1 Cancel
    /// or Esc. The page slides out for Options as it does for any button of
    /// its own, and slides back in when Options closes.
    pub(super) fn answer_shell_menu(state: &mut AppState, answer: usize) {
        match answer {
            0 => match shell_transition::current_shell_slide_target(state) {
                Some(kind) => {
                    Self::leave_shell_dialog(state, kind, App::open_launcher_options_dialog);
                }
                None => Self::open_launcher_options_dialog(state),
            },
            2 => {
                Self::prepare_shell_terminal_exit(state);
                Self::confirm_quit(state);
            }
            _ => {}
        }
    }

    /// A match is launching under the menu, because a room's host started it.
    /// The menu and the Options it opened end here, Options applying as a
    /// terminated pump does.
    pub(crate) fn close_shell_menu_at_launch(state: &mut AppState) {
        if state
            .frontend
            .keyboard_dialog
            .as_ref()
            .is_some_and(|dialog| dialog.parent == KeyboardParent::Launcher)
        {
            crate::app::input::keyboard::prepare_terminal_exit(state);
        }
        Self::close_launcher_options_terminal(state);
        if state
            .frontend
            .shell_message_box
            .as_ref()
            .is_some_and(ShellMessageBox::keeps_page)
        {
            Self::close_shell_message_box_from_controller(state);
        }
    }
}

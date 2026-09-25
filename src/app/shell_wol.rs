//! Main Menu -> Internet: `WOL_Main` (`0x0077B2A0`) with the Westwood Online
//! welcome `0x10E` and, when a WOL action cannot load WOLAPI, the
//! `TXT_APIMISSING` box `0xD0`. Both ways out return to a new `0xE2`
//! (`0x0052E28E`).

use std::time::Instant;

use super::{App, AppState};
use crate::app::frontend::shell_transition::ShellExitThen;
use crate::app::shell_route::ShellRoute;
use crate::ui::shell::layout::LaidOutControl;
use crate::ui::wol_shell::{
    ApiMissingBox, WOL_WELCOME_DIALOG, WolWelcomeAction, WolWelcomeLayout, WolWelcomeState,
    action_for_control, compute_layout,
};

/// The message box's controller identity while it owns input.
const API_MISSING_DIALOG: crate::ui::shell::descriptor::DialogId =
    crate::ui::shell::descriptor::DialogId(0x00D0);

impl App {
    /// State 2 (`0x0052DD57`) then `WOL_Main`: the lobby music takes over
    /// (`0x0077B2D7..0x0077B31D`) and `0x10E` is created after the empty
    /// backdrop (`0x00798DE0`).
    pub(super) fn open_wol_welcome_page(state: &mut AppState) {
        Self::enter_shell_window_mode(state);
        crate::app::frontend::main_menu_shell_render::clear_ra2ts_movie_session(state);
        crate::app::frontend::shell_transition::invalidate_main_menu_dialog_instance(state);
        // The message box paints from the skirmish chrome atlas.
        let _ = Self::ensure_skirmish_shell_chrome(state);
        Self::ensure_wol_welcome_art(state);
        let now_ms =
            crate::app::match_runtime::sim_tick::monotonic_frame_pacer_ms(state, Instant::now());
        let lobby_music = state.persistence.options_profile.wol_lobby_music != 0;
        let saved_shuffle = state.audio.enter_wol_lobby_music(lobby_music, now_ms);
        state.frontend.wol_welcome = Some(WolWelcomeState::open(saved_shuffle));
        state.frontend.shell_route = ShellRoute::WolWelcome;
        state
            .frontend
            .shell_controller
            .reset_to(WOL_WELCOME_DIALOG, false);
    }

    /// `0x0072C7E0` loads the page's art (the background only at 800 wide).
    pub(crate) fn ensure_wol_welcome_art(state: &mut AppState) -> bool {
        if state.frontend.wol_welcome_art.is_some() {
            return true;
        }
        let Some(assets) = state.process_assets.manager() else {
            return false;
        };
        let icons: Vec<&str> = crate::ui::wol_shell::WOL_ICONS
            .iter()
            .map(|icon| icon.file)
            .collect();
        state.frontend.wol_welcome_art =
            crate::render::main_menu_shell_chrome::build_wol_welcome_art(
                &state.renderer.gpu,
                &state.renderer.batch_renderer,
                assets,
                state.renderer.gpu.config.width,
                &icons,
            );
        state.frontend.wol_welcome_art.is_some()
    }

    pub(super) fn wol_welcome_active(state: &AppState) -> bool {
        state.frontend.screen == crate::ui::game_screen::GameScreen::MainMenu
            && state.frontend.shell_route.wol_welcome()
            && state.frontend.wol_welcome.is_some()
    }

    pub(crate) fn wol_welcome_layout(state: &AppState) -> WolWelcomeLayout {
        compute_layout(
            state.renderer.gpu.config.width,
            state.renderer.gpu.config.height,
        )
    }

    fn wol_pointer(state: &AppState) -> (i32, i32) {
        (
            state.match_state.input.cursor_x.round() as i32,
            state.match_state.input.cursor_y.round() as i32,
        )
    }

    /// The page's buttons in hit-test (Z) order.
    fn wol_button_feed(layout: &WolWelcomeLayout) -> Vec<LaidOutControl> {
        layout
            .buttons_in_z_order()
            .into_iter()
            .map(|button| LaidOutControl {
                id: button.id,
                rect: button.rect,
            })
            .collect()
    }

    fn wol_api_missing_open(state: &AppState) -> bool {
        state
            .frontend
            .wol_welcome
            .as_ref()
            .is_some_and(|wol| wol.api_missing.is_some())
    }

    fn wol_api_missing_feed(state: &AppState) -> Vec<LaidOutControl> {
        let layout = crate::ui::shell::modal::body_ok_layout(
            state.renderer.gpu.config.width as i32,
            state.renderer.gpu.config.height as i32,
        );
        vec![LaidOutControl {
            id: crate::ui::shell::modal::control::OK,
            rect: layout.ok,
        }]
    }

    pub(super) fn handle_wol_mouse_move(state: &mut AppState) {
        let (x, y) = Self::wol_pointer(state);
        if Self::wol_api_missing_open(state) {
            let feed = Self::wol_api_missing_feed(state);
            state.frontend.shell_controller.on_pointer_move(x, y, &feed);
            return;
        }
        let layout = Self::wol_welcome_layout(state);
        let feed = Self::wol_button_feed(&layout);
        state
            .frontend
            .shell_controller
            .ensure_active(WOL_WELCOME_DIALOG, false);
        state.frontend.shell_controller.on_pointer_move(x, y, &feed);
        // The dialog's hit test writes the status help (`0x00622CCB`); a
        // pressed button holds the mouse, so none arrives meanwhile.
        if state.frontend.shell_controller.pressed().is_none()
            && let Some(wol) = state.frontend.wol_welcome.as_mut()
        {
            wol.status_help = layout.status_help_key(x, y);
        }
        state.frontend.shell_status_line.hover_repaint();
    }

    pub(super) fn handle_wol_mouse_down(state: &mut AppState) {
        let (x, y) = Self::wol_pointer(state);
        let feed = if Self::wol_api_missing_open(state) {
            Self::wol_api_missing_feed(state)
        } else {
            state
                .frontend
                .shell_controller
                .ensure_active(WOL_WELCOME_DIALOG, false);
            Self::wol_button_feed(&Self::wol_welcome_layout(state))
        };
        state.frontend.shell_controller.on_pointer_down(x, y, &feed);
        // Owner-draw buttons play GUIMainButtonSound on the press
        // (`0x00613667..0x00613771`).
        if state.frontend.shell_controller.pressed().is_some() {
            Self::play_main_menu_button_sound(state);
        }
    }

    pub(super) fn handle_wol_mouse_up(state: &mut AppState) {
        let (x, y) = Self::wol_pointer(state);
        if Self::wol_api_missing_open(state) {
            let feed = Self::wol_api_missing_feed(state);
            if state.frontend.shell_controller.on_pointer_up(x, y, &feed)
                == Some(crate::ui::shell::modal::control::OK)
            {
                // `0x005E2E40` closes on OK; `WOL_Main` returns -2.
                Self::return_from_wol(state);
            }
            return;
        }
        let feed = Self::wol_button_feed(&Self::wol_welcome_layout(state));
        let Some(activated) = state.frontend.shell_controller.on_pointer_up(x, y, &feed) else {
            return;
        };
        match action_for_control(activated) {
            Some(WolWelcomeAction::MainMenu) => {
                Self::leave_shell_dialog(state, ShellExitThen::WolBack)
            }
            Some(WolWelcomeAction::ApiMissing) => {
                Self::leave_shell_dialog(state, ShellExitThen::WolApiMissing)
            }
            // No `Community` URL in the registry: nothing happens.
            Some(WolWelcomeAction::Community) | None => {}
        }
    }

    /// `0x10E`'s teardown has run for a WOL action: the WOLAPI object cannot
    /// be created (`0x00786390` -> `0x00785711`) and `TXT_APIMISSING` shows in
    /// `0xD0` over the empty shell backdrop.
    pub(super) fn commit_wol_api_missing(state: &mut AppState) {
        let body = Self::csf_label(
            state,
            "TXT_APIMISSING",
            "The Westwood online support library is either missing or invalid.",
        );
        let ok = Self::csf_label(state, "GUI:OK", "OK");
        if let Some(wol) = state.frontend.wol_welcome.as_mut() {
            wol.status_help = None;
            wol.api_missing = Some(ApiMissingBox { body, ok });
        }
        // The box's loop (`0x007759E0`) dispatches without `IsDialogMessage`:
        // it takes no keys.
        if state.frontend.shell_controller.top_id() != Some(API_MISSING_DIALOG) {
            state
                .frontend
                .shell_controller
                .push(API_MISSING_DIALOG, false);
        }
    }

    /// Main Menu's teardown has run (result 0), or the box's OK: `WOL_Main`
    /// restores the shuffle flag, and PrepareSession plays INTRO and builds a
    /// new `0xE2` (`0x0052E28E`).
    pub(super) fn return_from_wol(state: &mut AppState) {
        if let Some(wol) = state.frontend.wol_welcome.take() {
            state.audio.leave_wol_lobby_music(wol.saved_shuffle);
        }
        state.frontend.wol_welcome_art = None;
        state.frontend.shell_route = ShellRoute::MainMenu;
        crate::app::frontend::main_menu_shell_render::clear_ra2ts_movie_session(state);
        crate::app::frontend::shell_transition::invalidate_main_menu_dialog_instance(state);
        state
            .frontend
            .shell_controller
            .reset_to(crate::ui::shell::descriptor::DialogId(0x00E2), false);
    }

    /// Status help of the page for its status line.
    pub(crate) fn wol_status_key(state: &AppState) -> Option<&'static str> {
        state
            .frontend
            .wol_welcome
            .as_ref()
            .filter(|wol| wol.api_missing.is_none())
            .and_then(|wol| wol.status_help)
    }
}

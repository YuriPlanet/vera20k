//! Main Menu -> Internet: one Westwood Online visit, `WOL_Main`
//! (`0x0077B2A0`) from the welcome `0x10E` until WOL is left. Custom Match
//! runs the login flow `WOL__RunLoginFlow` (`0x00787010`): the WOLAPI
//! object, the server list behind the wait box `0xC4`, the login `0x10F`,
//! the connect and then the custom lobby runner (`0x00787770`, [`open_lobby`]).
//!
//! The WOLAPI COM object, its servers and its credential store are outside
//! the shell: they meet it only through [`WolServiceRequest`] /
//! [`WolServiceEvent`]. Production has no WOLAPI ([`WolProvider::Unavailable`]):
//! every WOL action shows `TXT_APIMISSING` in `0xD0` and returns to a new
//! `0xE2` (`0x0052E28E`), as on a retail install without the online
//! components.

pub(crate) mod lobby;
mod login;
mod service;

pub(crate) use login::{combo_popup, login_layout, open_login_art};
pub(crate) use service::{
    WolApiStatus, WolLoginAccepted, WolProvider, WolServer, WolServerList, WolServiceEvent,
    WolServiceRequest, service_event,
};

/// The wait box's reveal timer and the login drop list's scroll repeats;
/// the caller schedules the earliest deadline with the other shell timers.
pub(crate) fn poll(state: &mut AppState) -> Option<std::time::Instant> {
    [
        service::poll_wait(state),
        login::poll_scroll_repeat(state),
        lobby::poll(state),
    ]
    .into_iter()
    .flatten()
    .min()
}

use super::{App, AppState};
use crate::app::frontend::shell_transition::ShellExitThen;
use crate::app::shell_route::ShellRoute;
use crate::ui::shell::descriptor::DialogId;
use crate::ui::shell::layout::LaidOutControl;
use crate::ui::wol_login::{WolCredential, WolLoginState};
use crate::ui::wol_shell::{
    BOX_NO, BOX_OK, BOX_YES, WAIT_CHECK_STATUS, WAIT_DISCONNECT, WOL_WELCOME_DIALOG, WolBoxButtons,
    WolMessageBox, WolMode, WolWaitBox, WolWelcomeAction, WolWelcomeLayout, action_for_control,
};

/// The custom lobby `0x113`'s controller identity.
pub(crate) const WOL_LOBBY_DIALOG: DialogId = DialogId(0x0113);

/// What one WOL page is showing.
#[derive(Debug, Clone)]
pub(crate) enum WolPage {
    /// The welcome `0x10E`.
    Welcome,
    /// A dialog has closed and the flow runs over the empty shell backdrop
    /// (`Shell__DrawEmptyBackdrop`), with the wait box `0xC4` while a
    /// service request is outstanding.
    Backdrop { wait: Option<WolWaitBox> },
    /// The login `0x10F`.
    Login(Box<WolLoginState>),
    /// A message box over the backdrop and what its close leads to.
    Message {
        message: WolMessageBox,
        then: WolBoxThen,
    },
    /// The custom lobby `0x113` (its runner belongs to the lobby chain).
    Lobby,
}

/// What follows a WOL message box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WolBoxThen {
    /// `WOL_Main` returns this result (-2 or -1) and leaves WOL.
    Leave(i32),
    /// The login flow returned 0: the welcome runs again.
    Welcome,
}

/// One visit to Westwood Online, from Internet until `WOL_Main` returns.
#[derive(Debug)]
pub(crate) struct WolVisit {
    /// The shuffle flag `WOL_Main` saved on entry (`0x0077B30C`); restored
    /// only when the visit ends ([`App::return_from_wol`]).
    saved_shuffle: bool,
    /// `[0xA8B244]`; `None` is mode 0.
    mode: Option<WolMode>,
    /// Login category `[0xB73678]`: set once the login flow returned 1.
    authenticated: bool,
    pub(crate) page: WolPage,
    /// The status help the welcome's last hit test wrote.
    pub(crate) status_help: Option<&'static str>,
    /// Server records of the last list (`0xB778D8` count, `0xF8` records)
    /// and the selected record `[0xB76468]`.
    servers: Vec<WolServer>,
    selected_server: usize,
    awaiting: service::Awaiting,
    provider: WolProvider,
    requests: Vec<WolServiceRequest>,
    /// The WOLAPI credential store as the service last reported it.
    credentials: Vec<WolCredential>,
    /// `[0xB77AD0]`: the text the login sinks leave for `0x00786EF0`.
    last_error: Option<&'static str>,
    /// `[0xB77DF8]`: the login result code.
    last_login_result: Option<u32>,
    /// `[0xB73AE0]`: the message of the day from a successful login.
    motd: Option<String>,
    /// `[0xB77E30]`: the last user who paged us, the `/reply` target.
    last_pager: Option<String>,
    /// The custom lobby runner; kept hidden while C2/C9 run over it.
    lobby: Option<Box<lobby::WolLobby>>,
    /// A chat lobby was joined earlier in this visit: the picker passes
    /// over crowded lobbies (`0x00788180`).
    lobby_joined_before: bool,
}

impl WolVisit {
    fn open(saved_shuffle: bool) -> Self {
        Self {
            saved_shuffle,
            mode: None,
            authenticated: false,
            page: WolPage::Welcome,
            status_help: None,
            servers: Vec::new(),
            selected_server: 0,
            awaiting: service::Awaiting::Nothing,
            provider: WolProvider::Unavailable,
            requests: Vec::new(),
            credentials: Vec::new(),
            last_error: None,
            last_login_result: None,
            motd: None,
            last_pager: None,
            lobby: None,
            lobby_joined_before: false,
        }
    }

    /// Diagnostics and tests only: a controlled service will drain the
    /// requests and inject events. Production keeps `Unavailable`.
    pub(crate) fn set_provider(&mut self, provider: WolProvider) {
        self.provider = provider;
    }

    pub(crate) fn take_service_requests(&mut self) -> Vec<WolServiceRequest> {
        std::mem::take(&mut self.requests)
    }

    pub(crate) fn mode(&self) -> Option<WolMode> {
        self.mode
    }

    pub(crate) fn authenticated(&self) -> bool {
        self.authenticated
    }

    pub(crate) fn motd(&self) -> Option<&str> {
        self.motd.as_deref()
    }

    pub(crate) fn wait_box(&self) -> Option<&WolWaitBox> {
        match &self.page {
            WolPage::Backdrop { wait } => wait.as_ref(),
            WolPage::Lobby => self.lobby.as_ref()?.wait.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn message(&self) -> Option<&WolMessageBox> {
        match &self.page {
            WolPage::Message { message, .. } => Some(message),
            WolPage::Lobby => self.lobby.as_ref()?.message(),
            _ => None,
        }
    }

    /// The custom lobby runner while the visit has one.
    pub(crate) fn lobby(&self) -> Option<&lobby::WolLobby> {
        self.lobby.as_deref()
    }

    pub(crate) fn lobby_mut(&mut self) -> Option<&mut lobby::WolLobby> {
        self.lobby.as_deref_mut()
    }

    /// A wait box owns input (shown or still hidden): the pages under it
    /// take none.
    fn waiting(&self) -> bool {
        match &self.page {
            WolPage::Backdrop { .. } => true,
            WolPage::Lobby => self.wait_box().is_some(),
            _ => false,
        }
    }

    pub(crate) fn login(&self) -> Option<&WolLoginState> {
        match &self.page {
            WolPage::Login(login) => Some(login),
            _ => None,
        }
    }
}

/// `[0xB77E30]`, the `/reply` target of every WOL chat edit.
pub(crate) fn last_pager(state: &AppState) -> Option<&str> {
    state.frontend.wol.as_ref()?.last_pager.as_deref()
}

/// The custom lobby runner `0x00787770` entered after a successful login or
/// setup Back (`postmatch` false) or after a match (`postmatch` true, the
/// state 0x11 flag `WOL_Main` passes on).
pub(crate) fn open_lobby(state: &mut AppState, postmatch: bool) {
    lobby::open(state, postmatch);
}

impl App {
    /// State 2 (`0x0052DD57`) then `WOL_Main`: the lobby music takes over
    /// (`0x0077B2D7..0x0077B31D`) and `0x10E` is created after the empty
    /// backdrop (`0x00798DE0`).
    pub(super) fn open_wol_welcome_page(state: &mut AppState) {
        crate::app::frontend::main_menu_shell_render::clear_ra2ts_movie_session(state);
        crate::app::frontend::shell_transition::invalidate_main_menu_dialog_instance(state);
        // The message boxes paint from the skirmish chrome atlas.
        let _ = Self::ensure_skirmish_shell_chrome(state);
        Self::ensure_wol_welcome_art(state);
        let now_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
        let lobby_music = state.persistence.options_profile.wol_lobby_music != 0;
        let saved_shuffle = state.audio.enter_wol_lobby_music(lobby_music, now_ms);
        state.frontend.wol = Some(WolVisit::open(saved_shuffle));
        state.frontend.shell_route = ShellRoute::Wol;
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
        state.frontend.wol_welcome_art = crate::render::main_menu_shell_chrome::build_wol_page_art(
            &state.renderer.gpu,
            &state.renderer.batch_renderer,
            assets,
            crate::app::frontend::shell_layout::art_key_screen_width(state),
            ("MultiplaySelection.shp", "MultiplaySelection.pal"),
            &icons,
        );
        state.frontend.wol_welcome_art.is_some()
    }

    /// Any WOL page owns the shell's input and paint.
    pub(super) fn wol_active(state: &AppState) -> bool {
        state.frontend.screen == crate::ui::game_screen::GameScreen::MainMenu
            && state.frontend.shell_route.wol_visit()
            && state.frontend.wol.is_some()
    }

    pub(crate) fn wol_welcome_layout(state: &AppState) -> WolWelcomeLayout {
        crate::app::frontend::shell_layout::wol_welcome_layout(state)
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

    /// The buttons of the box that owns input, if one does.
    fn wol_box_feed(state: &AppState) -> Option<Vec<LaidOutControl>> {
        let (width, height) = (
            state.renderer.gpu.config.width as i32,
            state.renderer.gpu.config.height as i32,
        );
        let visit = state.frontend.wol.as_ref()?;
        if let Some(message) = visit.message() {
            return Some(match message.buttons {
                WolBoxButtons::Ok(_) => vec![LaidOutControl {
                    id: BOX_OK,
                    rect: crate::ui::shell::modal::body_ok_layout(width, height).ok,
                }],
                WolBoxButtons::YesNo { .. } => {
                    let layout = crate::ui::shell::modal::yes_no_layout(width, height);
                    vec![
                        LaidOutControl {
                            id: BOX_NO,
                            rect: layout.no,
                        },
                        LaidOutControl {
                            id: BOX_YES,
                            rect: layout.yes,
                        },
                    ]
                }
            });
        }
        if visit.waiting() {
            // A hidden wait box takes no pointer.
            return Some(match visit.wait_box().filter(|wait| wait.shown) {
                Some(_) => {
                    let layout = crate::ui::shell::modal::wait_box_layout(width, height);
                    vec![
                        LaidOutControl {
                            id: WAIT_DISCONNECT,
                            rect: layout.disconnect,
                        },
                        LaidOutControl {
                            id: WAIT_CHECK_STATUS,
                            rect: layout.check_status,
                        },
                    ]
                }
                None => Vec::new(),
            });
        }
        None
    }

    fn wol_page_is_login(state: &AppState) -> bool {
        state
            .frontend
            .wol
            .as_ref()
            .is_some_and(|visit| matches!(visit.page, WolPage::Login(_)))
    }

    fn wol_page_is_lobby(state: &AppState) -> bool {
        state
            .frontend
            .wol
            .as_ref()
            .is_some_and(|visit| matches!(visit.page, WolPage::Lobby))
    }

    pub(super) fn handle_wol_mouse_move(state: &mut AppState) {
        if Self::wol_page_is_lobby(state) && Self::wol_box_feed(state).is_none() {
            lobby::cursor_moved(state);
            return;
        }
        if Self::wol_page_is_login(state) {
            login::cursor_moved(state);
            return;
        }
        let (x, y) = state.match_state.input.cursor_px();
        if let Some(feed) = Self::wol_box_feed(state) {
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
            && let Some(visit) = state.frontend.wol.as_mut()
        {
            visit.status_help = layout.status_help_key(x, y);
        }
        state.frontend.shell_status_line.repaint();
    }

    pub(super) fn handle_wol_mouse_down(state: &mut AppState) {
        if Self::wol_page_is_lobby(state) && Self::wol_box_feed(state).is_none() {
            lobby::mouse(state, true);
            return;
        }
        if Self::wol_page_is_login(state) {
            login::mouse(state, true);
            return;
        }
        let (x, y) = state.match_state.input.cursor_px();
        let feed = match Self::wol_box_feed(state) {
            Some(feed) => feed,
            None => {
                state
                    .frontend
                    .shell_controller
                    .ensure_active(WOL_WELCOME_DIALOG, false);
                Self::wol_button_feed(&Self::wol_welcome_layout(state))
            }
        };
        state.frontend.shell_controller.on_pointer_down(x, y, &feed);
        // Owner-draw buttons play GUIMainButtonSound on the press
        // (`0x00613667..0x00613771`).
        if state.frontend.shell_controller.pressed().is_some() {
            Self::play_main_menu_button_sound(state);
        }
    }

    pub(super) fn handle_wol_mouse_up(state: &mut AppState) {
        if Self::wol_page_is_lobby(state) && Self::wol_box_feed(state).is_none() {
            lobby::mouse(state, false);
            return;
        }
        if Self::wol_page_is_login(state) {
            login::mouse(state, false);
            return;
        }
        let (x, y) = state.match_state.input.cursor_px();
        if let Some(feed) = Self::wol_box_feed(state) {
            if let Some(control) = state.frontend.shell_controller.on_pointer_up(x, y, &feed) {
                service::box_command(state, control);
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
            Some(WolWelcomeAction::Mode(mode)) => {
                let controlled = state
                    .frontend
                    .wol
                    .as_ref()
                    .is_some_and(|visit| visit.provider == WolProvider::Controlled);
                if controlled && mode != WolMode::CustomMatch {
                    // Quick Match, Quick Co-op, Buddy List and My Information
                    // behind a working WOLAPI are later chains.
                    log::warn!(
                        "WOL mode {} with a controlled service is not implemented; \
                         0x10E stays",
                        mode.native()
                    );
                } else {
                    Self::leave_shell_dialog(state, ShellExitThen::WolAction(mode));
                }
            }
            // No `Community` URL in the registry: nothing happens.
            Some(WolWelcomeAction::Community) | None => {}
        }
    }

    /// Keys while a WOL page shows. `0x10E` ignores IDOK/IDCANCEL
    /// (`0x007988C0`); the boxes' loop (`0x007759E0`) and the hidden or shown
    /// `0xC4` take none. `0x10F`'s and `0x113`'s NewEdits read keys.
    pub(super) fn handle_wol_key(
        state: &mut AppState,
        code: Option<winit::keyboard::KeyCode>,
        pressed: bool,
        text: Option<&str>,
    ) {
        if pressed && Self::wol_page_is_login(state) {
            login::key_press(state, code, text);
        } else if pressed && Self::wol_page_is_lobby(state) {
            lobby::key_press(state, code, text);
        }
    }

    /// `0x10E`'s teardown has run for a WOL action: the mode is written
    /// (`0x007988C0`) and the flow continues over the empty backdrop.
    pub(super) fn commit_wol_action(state: &mut AppState, mode: WolMode) {
        let Some(visit) = state.frontend.wol.as_mut() else {
            return;
        };
        visit.mode = Some(mode);
        visit.status_help = None;
        visit.page = WolPage::Backdrop { wait: None };
        service::begin_flow(state);
    }

    /// The login flow returned 0 (Back, or a failed connect after its box):
    /// mode 0 (`0x0077B3F6`), and the loop runs a fresh `0x10E` with its
    /// entry slide.
    pub(crate) fn reopen_wol_welcome(state: &mut AppState) {
        let Some(visit) = state.frontend.wol.as_mut() else {
            return;
        };
        visit.mode = None;
        visit.status_help = None;
        visit.page = WolPage::Welcome;
        state.frontend.wol_login_art = None;
        state.frontend.shell_route = ShellRoute::Wol;
        Self::ensure_wol_welcome_art(state);
        state
            .frontend
            .shell_controller
            .reset_to(WOL_WELCOME_DIALOG, false);
        state.platform.window.request_redraw();
    }

    /// Main Menu's teardown has run (result 0), or `WOL_Main` returned -2
    /// or -1: the WOLAPI object is released (`0x00786080`), the shuffle flag
    /// restored, and PrepareSession plays INTRO and builds a new `0xE2`
    /// (`0x0052E28E`).
    pub(super) fn return_from_wol(state: &mut AppState) {
        if let Some(visit) = state.frontend.wol.take() {
            state.audio.leave_wol_lobby_music(visit.saved_shuffle);
        }
        state.frontend.wol_welcome_art = None;
        state.frontend.wol_login_art = None;
        state.frontend.wol_lobby_art = None;
        state.frontend.shell_route = ShellRoute::MainMenu;
        crate::app::frontend::main_menu_shell_render::clear_ra2ts_movie_session(state);
        crate::app::frontend::shell_transition::invalidate_main_menu_dialog_instance(state);
        state
            .frontend
            .shell_controller
            .reset_to(crate::ui::shell::descriptor::DialogId(0x00E2), false);
    }

    /// Status help of the welcome for its status line.
    pub(crate) fn wol_status_key(state: &AppState) -> Option<&'static str> {
        state
            .frontend
            .wol
            .as_ref()
            .filter(|visit| matches!(visit.page, WolPage::Welcome))
            .and_then(|visit| visit.status_help)
    }

    pub(super) fn commit_wol_login_close(state: &mut AppState, result: u16) {
        login::commit_close(state, result);
    }
}

//! OS input-method (IME) text entry for chat and chooser edits.
//!
//! gamemd removes its main window's input context when it creates the window
//! (CreateMainWindow `0x00777D80`: ImmGetContext, then
//! `ImmAssociateContext(hWnd, 0)`), restores it when the in-game chat edit
//! opens (MessageListClass::AddEdit `0x005D4210` calls `0x00778180`) and
//! removes it again on every edit end (`0x007781A0`, from Input `0x005D4510`'s
//! end paths and Start_Scenario `0x00683E6B`). The shell's NewEdits are child
//! windows, which that call leaves alone. A finished character arrives as
//! WM_IME_CHAR: the main window queues it at `0x00B730EC`
//! (Game__MainWindowProc `0x007775C0`) and the chat pre-hook `0x0055E420`
//! feeds each queued unit to Input first; NewEdit `0x00614B30` widens it
//! (`0x007781B0`) and inserts it through its WM_CHAR path. The focused NewEdit
//! paints the open composition (`0x00777EA0`) at its caret (`0x00623880`).
//!
//! VERA20k takes the composition and the finished text from winit's `Ime`
//! events, and asks for an input method only while an edit takes keys:
//! the in-game line, the WOL lobby (`0x10F`), the WOL game room and LAN setup
//! (C2/C9, BC/BD), the LAN lobby (BB), Choose Map's search line, and VERA's
//! converted-save name edit. On Android they are the in-game line and the
//! focused `Edit` of a GUI page (the LAN lobby's chat and name box, the setup
//! room's chat, the WOL pages and lobby, the skirmish player name, Choose
//! Map's search line, and the Mods page's name dialog). Finished text goes
//! through each screen's own text path, as WM_IME_CHAR does.
//!
//! Player-name edits take no input method: NewEdit `0x00614B30` drops units at
//! or above 0x100 from them (`0x00615226..0x0061527A`, the byte-only filter),
//! so gamemd lets in typed keys as code-page bytes but no widened
//! input-method character such as Chinese.
//!
//! RESIDUAL: native's other NewEdits (login, page, buddy, saved-game
//! descriptions) also take IME input; on the desktop they keep plain key text.
//! In battle, native leaves the composition to the system's default IME window
//! (OwnerDraw__ServiceIMEMessage `0x00778030` passes the composition messages
//! to DefWindowProc). winit hides that window, so VERA20k draws the
//! composition after the typed text in the NewEdit composition colour.
//!
//! winit's web backend has no input-method support; in the browser `web::ime`
//! supplies these same events. RESIDUAL: there the candidate window opens at
//! the edit's left end plus the composition, not after the typed text.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use crate::app::AppState;
use crate::ui::shell::geom::RectPx;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::Ime;

/// The composition colour `[0xAC4618]`, COLORREF `0x9F9F` that
/// ShellDialog__SubclassChildByClass `0x0060F9A0` sets.
pub(crate) const COMPOSITION_RGB: [u8; 3] = [0x9F, 0x9F, 0x00];

/// An edit that takes IME input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImeTarget {
    InGameChat,
    #[cfg(not(target_os = "android"))]
    WolLobbyEdit,
    #[cfg(not(target_os = "android"))]
    SetupChat,
    #[cfg(not(target_os = "android"))]
    LanLobbyChat,
    #[cfg(not(target_os = "android"))]
    ChooseMapSearch,
    #[cfg(not(target_os = "android"))]
    MainMenuConversionName,
    #[cfg(not(target_os = "android"))]
    InGameConversionName,
    /// Android only: the focused `Edit` of a GUI page, whichever screen owns it
    /// ([`gui_edit`]); the text goes to that screen's own input path. It takes
    /// the place of the desktop's per-screen targets.
    #[cfg(target_os = "android")]
    Gui,
}

/// Process-local input-method state. Presentation-only: never serialized or
/// read by the deterministic simulation.
#[derive(Debug, Default)]
pub(crate) struct ImeState {
    /// The value last handed to `Window::set_ime_allowed`.
    allowed: bool,
    target: Option<ImeTarget>,
    /// The composition string `0x00B73318`, in UTF-16 units.
    composition: Vec<u16>,
    /// The composition cursor `0x00B73568`, in UTF-16 units.
    cursor: usize,
    /// The area last handed to `Window::set_ime_cursor_area`.
    area: Option<RectPx>,
    /// Android: the buffer last read from the input method (`util::android_ime`).
    #[cfg(target_os = "android")]
    android_text: String,
    /// Android: the edit that buffer was seeded from.
    #[cfg(target_os = "android")]
    android_target: Option<ImeTarget>,
    /// Android: frames left to re-assert the seeded text while the platform
    /// still reports the previous edit's.
    #[cfg(target_os = "android")]
    android_reseed: u8,
    /// Android: the platform's buffer has echoed the seed at least once, so a
    /// difference from here on is the player's typing.
    #[cfg(target_os = "android")]
    android_synced: bool,
    /// Android: whether the input method was last told to hide the focused
    /// edit's text (a password box).
    #[cfg(target_os = "android")]
    android_secret: Option<bool>,
}

impl ImeState {
    /// The open composition and its cursor, when `target` owns it.
    pub(crate) fn composition(&self, target: ImeTarget) -> Option<(&[u16], usize)> {
        (self.target == Some(target) && !self.composition.is_empty())
            .then_some((self.composition.as_slice(), self.cursor))
    }

    /// The open composition and its cursor, whichever edit owns it (the
    /// focused edit of the screen that routed the input method).
    pub(crate) fn open_composition(&self) -> Option<(&[u16], usize)> {
        let target = self.target?;
        self.composition(target)
    }

    /// While a composition is open, keys go to the input method (Windows
    /// delivers them as VK_PROCESSKEY), not to the edit.
    pub(crate) fn composing(&self) -> bool {
        !self.composition.is_empty()
    }

    /// OwnerDraw__CancelIMEComposition `0x00777E00`.
    fn cancel(&mut self) {
        self.composition.clear();
        self.cursor = 0;
    }
}

/// The edit that takes keys now, with its rectangle in window pixels.
/// The dialogs the key dispatch serves first leave none.
fn target(state: &AppState) -> Option<(ImeTarget, RectPx)> {
    if state.frontend.main_menu_dialog_open()
        || crate::app::input::coop_score::active(&state.frontend)
        || crate::app::frontend::preview_zoom::is_open(&state.frontend.preview_zoom)
    {
        return None;
    }
    #[cfg(not(target_os = "android"))]
    {
        edit_target(
            &state.platform,
            &state.renderer,
            &state.frontend,
            &state.match_state,
            &state.process_assets,
            &state.persistence,
        )
    }
    #[cfg(target_os = "android")]
    {
        edit_target(state)
    }
}

/// The desktop's and the browser's edits that take an input method.
#[cfg(not(target_os = "android"))]
fn edit_target(
    platform: &crate::app::PlatformState,
    renderer: &crate::app::renderer_state::RendererState,
    frontend: &crate::app::frontend::state::FrontendState,
    match_state: &crate::app::match_runtime::state::MatchState,
    process_assets: &crate::app::process_assets::ProcessAssets,
    persistence: &crate::app::persistence::PersistenceState,
) -> Option<(ImeTarget, RectPx)> {
    use crate::app::input::precedence::{InputLayer, ScreenPage};
    use crate::app::{App, GameScreen};
    use crate::render::shell_text::{ScissorRect, TextRect};
    use crate::ui::pause_menu::InGameMenuState;
    // The conversion edit owns the keys when a battlefield chat edit is
    // underneath it; a prompt over the name keeps the IME closed.
    let saved_browser = match frontend.screen {
        GameScreen::MainMenu => frontend.load_saved_game.as_ref(),
        GameScreen::InGame => match_state.match_presentation.saved_game_browser.as_ref(),
        _ => None,
    };
    if let Some(browser) = saved_browser.filter(|browser| browser.is_naming_conversion()) {
        if !browser.edit().focused {
            return None;
        }
        let (target, layout) = match crate::app::input::precedence::input_layer(
            platform,
            renderer,
            frontend,
            match_state,
            persistence,
        ) {
            InputLayer::Screen(ScreenPage::LoadSavedGame) => (
                ImeTarget::MainMenuConversionName,
                crate::app::frontend::shell_layout::load_saved_game_layout(&renderer.gpu, frontend)
                    .browser,
            ),
            InputLayer::Screen(ScreenPage::InGameShell(InGameMenuState::SavedGame(_))) => (
                ImeTarget::InGameConversionName,
                App::saved_game_layout(
                    platform,
                    renderer,
                    frontend,
                    match_state,
                    persistence,
                    browser.mode,
                )?,
            ),
            _ => return None,
        };
        let rect = layout.name_edit?;
        // The launcher is in window pixels. An active-game shell's edit
        // shares the text's sidebar and match zoom before it reaches them.
        let rect = match crate::app::presentation::window_ui::shell_raster(
            platform,
            renderer,
            &frontend.screen,
            &match_state.match_presentation,
            persistence,
            true,
        ) {
            Some(raster) => {
                let at = ScissorRect::of(TextRect::of(rect), raster);
                RectPx::new(at.x as i32, at.y as i32, at.w as i32, at.h as i32)
            }
            None => rect,
        };
        return Some((target, rect));
    }
    [
        (
            ImeTarget::InGameChat,
            crate::app::input::messages::ime_edit_rect(
                renderer,
                &frontend.screen,
                &match_state.match_presentation,
            ),
        ),
        (
            ImeTarget::LanLobbyChat,
            crate::app::shell_lan::ime_chat_rect(&renderer.gpu, frontend),
        ),
        (
            ImeTarget::SetupChat,
            crate::app::input::multiplayer_setup::ime_chat_rect(
                renderer,
                frontend,
                match_state,
                process_assets,
            ),
        ),
        (
            ImeTarget::WolLobbyEdit,
            crate::app::shell_wol::lobby::ime_edit_rect(frontend, &renderer.gpu),
        ),
        (
            ImeTarget::ChooseMapSearch,
            crate::app::App::choose_map_search_ime_rect(
                platform,
                renderer,
                frontend,
                match_state,
                persistence,
            ),
        ),
    ]
    .into_iter()
    .find_map(|(target, rect)| rect.map(|rect| (target, rect)))
}

/// Android: the in-game line, else the focused `Edit` of a GUI page. The
/// keyboard shows for a tapped edit only: the pages focus their edits when
/// they open, and a tap arms the dialog the player means
/// (`ui::shell::controller`, `util::android_ime`).
#[cfg(target_os = "android")]
fn edit_target(state: &AppState) -> Option<(ImeTarget, RectPx)> {
    if let Some(rect) = crate::app::input::messages::ime_edit_rect(
        &state.renderer,
        &state.frontend.screen,
        &state.match_state.match_presentation,
    ) {
        return Some((ImeTarget::InGameChat, rect));
    }
    if let Some((screen, edit)) = gui_edit(state)
        && (!screen.needs_tap() || gui_armed(state))
    {
        return Some((ImeTarget::Gui, edit.rect));
    }
    None
}

/// Ask for an input method exactly while a supported edit takes keys
/// (`0x00778180` / `0x007781A0`), and keep the system's candidate window at
/// that edit.
pub(crate) fn sync(state: &mut AppState) {
    #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
    for ime in crate::web::ime::take_events() {
        event(state, ime);
    }
    let found = target(state);
    let target = found.map(|(target, _)| target);
    let platform = &mut state.platform;
    let ime = &mut platform.ime;
    if ime.target != target && target.is_none() {
        crate::util::android_ime::clear_armed();
    }
    if ime.target != target {
        log::info!("IME target {target:?}");
        ime.cancel();
        ime.target = target;
    }
    if ime.allowed != target.is_some() {
        ime.allowed = target.is_some();
        ime.area = None;
        platform.window.set_ime_allowed(ime.allowed);
        #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
        crate::web::ime::set_allowed(ime.allowed);
    }
    if let Some((_, rect)) = found
        && ime.area != Some(rect)
    {
        ime.area = Some(rect);
        platform.window.set_ime_cursor_area(
            PhysicalPosition::new(rect.x, rect.y),
            PhysicalSize::new(rect.w.max(1) as u32, rect.h.max(1) as u32),
        );
        #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
        crate::web::ime::set_area(rect.x, rect.y, rect.w.max(1) as u32, rect.h.max(1) as u32);
    }
    #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
    if ime.allowed {
        crate::web::ime::keep_focus();
    }
    #[cfg(target_os = "android")]
    android::sync(state, target);
}

/// Android: whether the soft keyboard is up, which keeps the frame loop polling
/// (the input method's commits do not wake winit, winit#4673).
#[cfg(target_os = "android")]
pub(crate) fn keyboard_active(state: &AppState) -> bool {
    state.platform.ime.allowed
}

/// Android: a GUI page's focused `Edit`, as the platform's buffer needs it.
#[cfg(target_os = "android")]
struct GuiEdit {
    text: String,
    /// The caret, in characters, as `Prop::Caret` holds it.
    caret: usize,
    /// The edit's rectangle in window pixels; zero for the Choose Map search
    /// line, which the host paints itself.
    rect: RectPx,
    /// The edit hides its text (a password box).
    secret: bool,
}

/// Android: the GUI page whose focused edit takes text now, and how its text
/// reaches that page.
#[cfg(target_os = "android")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GuiScreen {
    LanLobby,
    SetupRoom,
    /// The WOL pages on the shell's stack: welcome, login, the room, the buddy
    /// list and Options.
    WolPages,
    /// The WOL lobby's own dialog (`0x113`), which keeps its own GUI tree.
    WolLobby,
    Skirmish,
    /// VERA's Setups page and the saved-seed browser, which keep their own
    /// widget trees (`SavedSeedBrowserState::ime_ui`).
    Setups,
    ChooseMap,
    /// The in-game `0xCF` page.
    InGamePage,
    /// The main menu's saved-game browser, while a conversion asks for a name.
    LoadSavedGame,
    /// The in-game saved-game browser, likewise.
    InGameSavedGame,
    /// The Mods page's name dialog (New Mod, Rename Mod), which focuses its
    /// edit when it opens.
    ModName,
}

#[cfg(target_os = "android")]
impl GuiScreen {
    /// Android shows the keyboard for a tapped edit only, on the pages that
    /// give their edit the focus when they open. The WOL lobby and the
    /// saved-game browsers are reached by the tap or the prompt that opened
    /// them, so their edits ask for the keyboard themselves.
    fn needs_tap(self) -> bool {
        matches!(
            self,
            Self::LanLobby
                | Self::SetupRoom
                | Self::WolPages
                | Self::WolLobby
                | Self::Skirmish
                | Self::ChooseMap
                | Self::InGamePage
        )
    }
}

/// Android: the page whose GUI owns the focused edit, with that page's tree.
#[cfg(target_os = "android")]
fn gui_screen(state: &AppState) -> Option<(GuiScreen, &vera20k_gui::Gui)> {
    use crate::app::input::precedence::{InputLayer, ScreenPage, input_layer};
    use crate::ui::pause_menu::InGameMenuState;
    let shell = &state.frontend.shell_controller;
    // A Choose Map modal takes the keys over the page that opened it.
    let chooser = state
        .frontend
        .skirmish_shell_state
        .choose_map_modal
        .as_ref()
        .is_some_and(|modal| modal.message_box.is_none() && !modal.is_hidden());
    match input_layer(
        &state.platform,
        &state.renderer,
        &state.frontend,
        &state.match_state,
        &state.persistence,
    ) {
        InputLayer::Screen(ScreenPage::LanLobby) => Some((GuiScreen::LanLobby, shell.gui())),
        InputLayer::Screen(ScreenPage::NetworkSetup | ScreenPage::Skirmish) if chooser => {
            Some((GuiScreen::ChooseMap, shell.gui()))
        }
        InputLayer::Screen(ScreenPage::NetworkSetup) => Some((GuiScreen::SetupRoom, shell.gui())),
        // VERA's Setups page (and the saved-seed browser) draw in their own
        // trees: the platform's text goes to the page's own key path.
        InputLayer::Screen(ScreenPage::Skirmish)
            if state.frontend.skirmish_shell_state.setups_browser.is_some() =>
        {
            let browser = state
                .frontend
                .skirmish_shell_state
                .setups_browser
                .as_ref()?;
            Some((GuiScreen::Setups, browser.ime_ui().gui()))
        }
        InputLayer::Screen(ScreenPage::Skirmish)
            if state
                .frontend
                .skirmish_shell_state
                .saved_seed_browser
                .is_some() =>
        {
            let browser = state
                .frontend
                .skirmish_shell_state
                .saved_seed_browser
                .as_ref()?;
            Some((GuiScreen::Setups, browser.ime_ui().gui()))
        }
        InputLayer::Screen(ScreenPage::Skirmish) => Some((GuiScreen::Skirmish, shell.gui())),
        InputLayer::Screen(ScreenPage::Wol) if lobby_shows(state) => Some((
            GuiScreen::WolLobby,
            crate::app::shell_wol::lobby::ime_gui(state)?,
        )),
        InputLayer::Screen(ScreenPage::Wol) => Some((GuiScreen::WolPages, shell.gui())),
        InputLayer::Screen(ScreenPage::Mods) if state.frontend.mods.name_dialog.is_some() => {
            Some((GuiScreen::ModName, shell.gui()))
        }
        InputLayer::Screen(ScreenPage::InGameShell(InGameMenuState::Page)) => {
            Some((GuiScreen::InGamePage, shell.gui()))
        }
        InputLayer::Screen(ScreenPage::LoadSavedGame) => Some((
            GuiScreen::LoadSavedGame,
            state.frontend.load_saved_game.as_ref()?.ui.gui(),
        )),
        InputLayer::Screen(ScreenPage::InGameShell(InGameMenuState::SavedGame(_))) => Some((
            GuiScreen::InGameSavedGame,
            state
                .match_state
                .match_presentation
                .saved_game_browser
                .as_ref()?
                .ui
                .gui(),
        )),
        _ => None,
    }
}

/// Android: a GUI tree's focused edit, or none when it holds the focus
/// somewhere else or nowhere.
#[cfg(target_os = "android")]
fn focused_edit(gui: &vera20k_gui::Gui) -> Option<GuiEdit> {
    use vera20k_gui::{NodeKind, Prop, Value};
    let node = gui.focus()?;
    if gui.tree().kind(node).ok()? != NodeKind::Edit {
        return None;
    }
    let text = match gui.get(node, Prop::Text).ok().flatten() {
        Some(Value::Text(text)) => text.clone(),
        _ => String::new(),
    };
    let caret = match gui.get(node, Prop::Caret).ok().flatten() {
        Some(Value::Int(caret)) => usize::try_from(*caret).unwrap_or(0),
        _ => text.chars().count(),
    };
    let rect = gui.tree().screen_rect(node).ok()?;
    let secret = matches!(
        gui.get(node, Prop::Password).ok().flatten(),
        Some(Value::Bool(true))
    );
    Some(GuiEdit {
        text,
        caret,
        rect: RectPx::new(rect.x, rect.y, rect.w, rect.h),
        secret,
    })
}

/// Android: the focused edit of the page that takes text now.
#[cfg(target_os = "android")]
fn gui_edit(state: &AppState) -> Option<(GuiScreen, GuiEdit)> {
    let (screen, gui) = gui_screen(state)?;
    Some((screen, focused_edit(gui)?))
}

/// Android: the shell dialog whose edit the player tapped; a page's own edits
/// then take the keyboard (`util::android_ime::arm`).
#[cfg(target_os = "android")]
fn gui_armed(state: &AppState) -> bool {
    let shell = &state.frontend.skirmish_shell_state;
    let controller = gui_screen(state).and_then(|(screen, _)| match screen {
        GuiScreen::Setups => shell
            .setups_browser
            .as_ref()
            .map(|browser| browser.ime_ui())
            .or_else(|| {
                shell
                    .saved_seed_browser
                    .as_ref()
                    .map(|browser| browser.ime_ui())
            }),
        _ => None,
    });
    let armed = |id: u16| crate::util::android_ime::armed(id);
    let by_screen = gui_screen(state).map(|(screen, _)| match screen {
        GuiScreen::Setups => controller
            .and_then(|controller| controller.top_id())
            .map(|dialog| armed(dialog.0)),
        // The WOL lobby keeps its own tree (`0x113`); the WOL pages keep
        // theirs on the shell's stack.
        GuiScreen::WolLobby => Some(armed(crate::ui::wol_lobby::DIALOG.0)),
        GuiScreen::WolPages => {
            crate::app::shell_wol::page::armed_dialog(state.frontend.wol.as_ref()).map(armed)
        }
        _ => None,
    });
    by_screen.flatten().unwrap_or_else(|| {
        state
            .frontend
            .shell_controller
            .top_id()
            .is_some_and(|dialog| armed(dialog.0))
    })
}

/// Android: the WOL lobby's own dialog (`0x113`) takes the keys, with no page
/// dialog over it: its edits live in the lobby's own GUI tree, beside the
/// shell's stack.
#[cfg(target_os = "android")]
fn lobby_shows(state: &AppState) -> bool {
    crate::app::App::wol_page_is_lobby(state.frontend.wol.as_ref())
        && !crate::app::shell_wol::page::open(state.frontend.wol.as_ref())
        && !crate::app::frontend::shell_transition::blocks_shell_input(&state.frontend)
}

/// Android: one typed character, or Backspace, into the focused GUI page's
/// edit, through that page's own key path.
#[cfg(target_os = "android")]
fn gui_key(state: &mut AppState, code: Option<winit::keyboard::KeyCode>, text: Option<&str>) {
    let Some((screen, _)) = gui_screen(state) else {
        return;
    };
    match screen {
        GuiScreen::LanLobby => crate::app::shell_lan::key_press(state, code, text),
        GuiScreen::SetupRoom => crate::app::input::multiplayer_setup::key_press(state, code, text),
        // The WOL pages route the key to the top page's own input path.
        GuiScreen::WolPages | GuiScreen::WolLobby => {
            crate::app::App::handle_wol_key(state, code, true, text)
        }
        GuiScreen::Skirmish => {
            // The page's own key path takes a physical code; a soft keyboard's
            // text goes through its text argument with Enter as the code.
            let code = code.unwrap_or(winit::keyboard::KeyCode::Enter);
            crate::app::App::handle_skirmish_shell_key_input(
                &state.platform,
                &state.renderer.bit_font,
                &mut state.frontend,
                code,
                text,
            );
        }
        GuiScreen::Setups => {
            if state.frontend.skirmish_shell_state.setups_browser.is_some() {
                crate::app::App::handle_skirmish_setups_key(
                    &state.platform.window,
                    &state.renderer,
                    &mut state.frontend,
                    &state.match_state,
                    &state.process_assets,
                    &mut state.audio.sfx_player,
                    code,
                    text,
                );
            } else {
                crate::app::App::handle_saved_seed_browser_key(
                    state.platform.game_config.as_ref(),
                    &state.renderer,
                    &mut state.frontend,
                    &mut state.match_state,
                    &mut state.process_assets,
                    &mut state.audio.sfx_player,
                    code,
                    text,
                );
            }
        }
        GuiScreen::ChooseMap => match code {
            Some(winit::keyboard::KeyCode::Backspace) => {
                crate::app::App::choose_map_search_backspace(state);
            }
            _ => {
                if let Some(text) = text {
                    crate::app::App::choose_map_search_text(
                        &state.platform,
                        &state.renderer,
                        &mut state.frontend,
                        &state.match_state,
                        &state.persistence,
                        text,
                    );
                }
            }
        },
        GuiScreen::InGamePage => crate::app::shell_wol::page::key_press(
            &mut state.frontend,
            &mut state.platform,
            &mut state.match_state,
            &state.process_assets,
            &state.renderer,
            &mut state.audio,
            &mut state.persistence,
            code,
            text,
        ),
        GuiScreen::LoadSavedGame => crate::app::App::handle_load_saved_game_key(state, code, text),
        GuiScreen::InGameSavedGame => crate::app::App::saved_game_key(state, code, text),
        GuiScreen::ModName => crate::app::App::name_dialog_input(state, code, text),
    }
}

/// Android: the input method edits its own buffer (`util::android_ime`), so
/// seed it from the focused edit and turn each change into committed text.
#[cfg(target_os = "android")]
mod android {
    use super::*;

    /// The focused edit's text and caret, whichever screen owns it, and
    /// whether it hides what it holds (a password box).
    fn seed(state: &AppState, focused: ImeTarget) -> Option<(String, usize, bool)> {
        match focused {
            ImeTarget::InGameChat => crate::app::input::messages::ime_edit_state(state)
                .map(|(text, caret)| (text, caret, false)),
            ImeTarget::Gui => {
                let (_, edit) = gui_edit(state)?;
                Some((edit.text, edit.caret, edit.secret))
            }
        }
    }

    pub(super) fn sync(state: &mut AppState, target: Option<ImeTarget>) {
        let Some(focused) = target else {
            // Hide once, on the way out: re-issuing it every idle frame fights
            // the next edit's show request.
            let was_editing = state.platform.ime.android_target.take().is_some();
            if was_editing {
                crate::util::android_ime::hide();
            }
            let ime = &mut state.platform.ime;
            ime.android_text.clear();
            ime.android_reseed = 0;
            ime.android_synced = false;
            ime.android_secret = None;
            return;
        };
        let Some((text, cursor, secret)) = seed(state, focused) else {
            return;
        };
        // A password box tells the input method to stop learning and suggesting
        // it (`ES_PASSWORD`, `0x00614FDC`); the flag follows the focus, so the
        // username next door keeps its suggestions.
        if state.platform.ime.android_secret != Some(secret) {
            state.platform.ime.android_secret = Some(secret);
            crate::util::android_ime::set_secret(secret);
        }
        // A tap on an edit asks for the keyboard again, with the edit's text:
        // the player may have collapsed the keyboard with its own hide button,
        // which the engine never sees.
        if crate::util::android_ime::take_show_request() {
            crate::util::android_ime::set_state(&text, cursor);
            let ime = &mut state.platform.ime;
            ime.android_text = text;
            ime.android_target = Some(focused);
            ime.android_reseed = 8;
            ime.android_synced = false;
            crate::util::android_ime::show();
            return;
        }
        if state.platform.ime.android_target != Some(focused) {
            crate::util::android_ime::set_state(&text, cursor);
            let ime = &mut state.platform.ime;
            ime.android_text = text;
            ime.android_target = Some(focused);
            ime.android_reseed = 8;
            ime.android_synced = false;
            crate::util::android_ime::show();
            return;
        }
        // The platform keeps the previous edit's text for a while after the
        // seed, and its soft keyboard may ignore the first show request after
        // the previous edit closed. Re-assert both until it echoes the seed;
        // text it still reports afterwards is stale, adopted as the baseline so
        // an old message is never typed into the new edit.
        if !state.platform.ime.android_synced {
            let current = crate::util::android_ime::state();
            if current.text == state.platform.ime.android_text {
                state.platform.ime.android_synced = true;
                return;
            }
            if state.platform.ime.android_reseed > 0 {
                state.platform.ime.android_reseed -= 1;
                crate::util::android_ime::set_state(&state.platform.ime.android_text, cursor);
                crate::util::android_ime::show();
                return;
            }
            state.platform.ime.android_text = current.text;
            state.platform.ime.android_synced = true;
            return;
        }
        let current = crate::util::android_ime::state();
        // The composing range is the input method's, not the game's: hold it
        // back from the commits and hand it to the drawing paths instead.
        let units: Vec<u16> = current.text.encode_utf16().collect();
        let (committed, preedit) =
            crate::util::android_ime::split_composition(&units, current.compose, current.cursor);
        let committed = String::from_utf16_lossy(&committed);
        state.platform.ime.composition = preedit
            .as_ref()
            .map(|(text, _)| text.clone())
            .unwrap_or_default();
        state.platform.ime.cursor = preedit.as_ref().map_or(0, |(_, caret)| *caret);
        if committed == state.platform.ime.android_text && (preedit.is_some() || committed == text)
        {
            return;
        }
        if committed != state.platform.ime.android_text {
            let (removed, inserted) =
                crate::util::android_ime::edit_of(&state.platform.ime.android_text, &committed);
            state.platform.ime.android_text = committed;
            // The shell edit deletes whole characters. In-game chat pops one
            // UTF-16 unit at a time and its caret is always at the end.
            let backspaces = match focused {
                ImeTarget::Gui => removed,
                ImeTarget::InGameChat => {
                    text.chars().rev().take(removed).map(char::len_utf16).sum()
                }
            };
            for _ in 0..backspaces {
                backspace(state, focused);
            }
            if !inserted.is_empty() {
                commit(state, &inserted);
            }
        }
        // The edit is the authority on what it holds. When it refused a
        // character the platform sent (the LAN name box's byte-only filter, a
        // length cap), the baseline advances but the edit does not, and the
        // next Backspace would remove the character before the one the player
        // meant. Re-seed the platform from the edit instead. Skip while a
        // composition is open: re-seeding would fight the input method's own
        // buffer.
        if preedit.is_none()
            && let Some((text, cursor, _)) = seed(state, focused)
            && let Some(resync) =
                crate::util::android_ime::resync_to(&text, &state.platform.ime.android_text)
        {
            crate::util::android_ime::set_state(&text, cursor);
            state.platform.ime.android_text = resync;
        }
    }

    /// One Backspace, through the screen's own key path.
    fn backspace(state: &mut AppState, focused: ImeTarget) {
        match focused {
            ImeTarget::InGameChat => crate::app::input::chat::backspace(state),
            ImeTarget::Gui => gui_key(state, Some(winit::keyboard::KeyCode::Backspace), None),
        }
    }
}

/// One winit `Ime` event.
pub(crate) fn event(state: &mut AppState, event: Ime) {
    match event {
        Ime::Preedit(text, cursor) => {
            let ime = &mut state.platform.ime;
            ime.composition = text.encode_utf16().collect();
            // winit reports the cursor as byte offsets, or none when hidden.
            ime.cursor = cursor
                .and_then(|(start, _)| text.get(..start))
                .map_or(ime.composition.len(), |prefix| {
                    prefix.encode_utf16().count()
                });
            if target(state).is_some_and(|(target, _)| target == ImeTarget::InGameChat) {
                crate::app::input::chat::composition_changed(
                    &state.platform.ime,
                    &mut state.match_state,
                );
            }
        }
        Ime::Commit(text) => {
            state.platform.ime.cancel();
            commit(state, &text);
        }
        Ime::Disabled => state.platform.ime.cancel(),
        Ime::Enabled => {}
    }
    state.platform.window.request_redraw();
}

/// WM_IME_CHAR's consumers: each screen's own text path.
fn commit(state: &mut AppState, text: &str) {
    match target(state).map(|(target, _)| target) {
        Some(ImeTarget::InGameChat) => crate::app::input::chat::commit_text(state, text),
        #[cfg(not(target_os = "android"))]
        Some(ImeTarget::LanLobbyChat) => crate::app::shell_lan::key_press(state, None, Some(text)),
        #[cfg(not(target_os = "android"))]
        Some(ImeTarget::SetupChat) => {
            crate::app::input::multiplayer_setup::key_press(state, None, Some(text))
        }
        #[cfg(not(target_os = "android"))]
        Some(ImeTarget::WolLobbyEdit) => {
            crate::app::shell_wol::lobby::key_press(state, None, Some(text))
        }
        #[cfg(not(target_os = "android"))]
        Some(ImeTarget::ChooseMapSearch) => {
            crate::app::App::choose_map_search_text(
                &state.platform,
                &state.renderer,
                &mut state.frontend,
                &state.match_state,
                &state.persistence,
                text,
            );
        }
        #[cfg(not(target_os = "android"))]
        Some(ImeTarget::MainMenuConversionName) => {
            crate::app::App::handle_load_saved_game_key(state, None, Some(text));
        }
        #[cfg(not(target_os = "android"))]
        Some(ImeTarget::InGameConversionName) => {
            crate::app::App::saved_game_key(state, None, Some(text))
        }
        #[cfg(target_os = "android")]
        Some(ImeTarget::Gui) => gui_key(state, None, Some(text)),
        None => {}
    }
}

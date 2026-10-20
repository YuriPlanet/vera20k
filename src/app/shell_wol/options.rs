//! WOL Options `0x2BC` (proc `0x0078EBC0`) and its nested Sound Options
//! `0xD6` (proc `0x006B6300`), run from lobby `0x113`'s Options button
//! (`0x0078B829..0x0078BA39`). The lobby stays alive and hidden under them:
//! its service sinks keep writing to it, its timers are gated off.
//!
//! The pages' modal loop dispatches without `IsDialogMessage`, so Enter,
//! Escape and Tab do nothing, and its idle (`0x0077F900`) never closes the
//! dialog.

use super::{WolPage, apply_wol_preferences};
use crate::app::frontend::{shell_layout, shell_transition::ShellExitThen};
use crate::app::{App, AppState};
use crate::ui::main_menu_dialogs::options::LauncherCue;
use crate::ui::shell::sound::{
    SHELL_SOUND_PAGE, ShellSoundLayout, SoundButton, SoundControl, SoundState,
};
use crate::ui::wol_options::{
    DIALOG, WolOptionsAction, WolOptionsLayout, WolOptionsState, WolPreferences,
};

/// The open `0x2BC`, and `0xD6` while it runs over the hidden `0x2BC`.
#[derive(Debug, Clone)]
pub(crate) struct WolOptionsPage {
    pub(crate) ui: WolOptionsState,
    pub(crate) sound: Option<SoundState>,
}

pub(crate) fn layout(state: &AppState) -> WolOptionsLayout {
    let (width, height) = shell_layout::shell_screen_size(state);
    if shell_layout::fills_window(state) {
        WolOptionsLayout::responsive(width as i32, height as i32)
    } else {
        WolOptionsLayout::new(width as i32, height as i32)
    }
}

pub(crate) fn sound_layout(state: &AppState) -> ShellSoundLayout {
    let (width, height) = shell_layout::shell_screen_size(state);
    if shell_layout::fills_window(state) {
        ShellSoundLayout::responsive(width as i32, height as i32)
    } else {
        ShellSoundLayout::new(width as i32, height as i32)
    }
}

/// `0x0072BAC0`: WOLOptions.shp (800 wide only) through WOLOptions.PAL.
pub(crate) fn open_art(state: &mut AppState) -> bool {
    if state.frontend.wol_options_art.is_some() {
        return true;
    }
    let Some(assets) = state.process_assets.manager() else {
        return false;
    };
    state.frontend.wol_options_art = crate::render::main_menu_shell_chrome::build_wol_page_art(
        &state.renderer.gpu,
        &state.renderer.batch_renderer,
        assets,
        shell_layout::art_key_screen_width(state),
        ("WOLOptions.shp", "WOLOptions.PAL"),
        &[],
    );
    state.frontend.wol_options_art.is_some()
}

/// `0x0072B8E0`: WOLSoundOptions.shp (800 wide only) through its palette.
pub(crate) fn open_sound_art(state: &mut AppState) -> bool {
    if state.frontend.wol_sound_options_art.is_some() {
        return true;
    }
    let Some(assets) = state.process_assets.manager() else {
        return false;
    };
    state.frontend.wol_sound_options_art =
        crate::render::main_menu_shell_chrome::build_wol_page_art(
            &state.renderer.gpu,
            &state.renderer.batch_renderer,
            assets,
            shell_layout::art_key_screen_width(state),
            ("WOLSoundOptions.shp", "WOLSoundOptions.PAL"),
            &[],
        );
    state.frontend.wol_sound_options_art.is_some()
}

fn is_button(id: u16) -> bool {
    crate::ui::wol_options::PAGE.button(id).is_some()
}

fn page_mut(state: &mut AppState) -> Option<&mut WolOptionsPage> {
    match &mut state.frontend.wol.as_mut()?.page {
        WolPage::Options(page) => Some(page),
        _ => None,
    }
}

/// `0x113` has slid out (`0x006071E0`) over the empty backdrop: the art,
/// then `UI__BeginDialog(0x2BC)` and its init `0x497`.
pub(crate) fn commit_open(state: &mut AppState) {
    let ui = WolOptionsState::open(&state.frontend.wol_preferences);
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    if let Some(lobby) = visit.lobby_mut() {
        lobby.shown = false;
        lobby.ui.buttons = Default::default();
    }
    visit.page = WolPage::Options(Box::new(WolOptionsPage { ui, sound: None }));
    open_art(state);
    state.frontend.shell_controller.reset_to(DIALOG, false);
    state.platform.window.request_redraw();
}

/// `0x2BC`'s teardown has run (`CloseAndPop`). The caller re-shows the
/// lobby (it slides in on its first paint), frees the art and focuses the
/// chat edit (`0x0078BF1F`). OK commits the staged values and writes them
/// with ObserverMode (`0x0077DFD0`); both results then apply (`0x0077E110`).
pub(crate) fn commit_close(state: &mut AppState, staged: Option<WolPreferences>) {
    state.frontend.wol_options_art = None;
    state.frontend.wol_sound_options_art = None;
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    visit.page = WolPage::Lobby;
    if let Some(lobby) = visit.lobby_mut() {
        lobby.shown = true;
        lobby.ui.refocus_chat();
    }
    state
        .frontend
        .shell_controller
        .reset_to(super::WOL_LOBBY_DIALOG, false);
    if let Some(staged) = staged {
        state.frontend.wol_preferences = staged;
        let profile = &mut state.persistence.options_profile;
        profile.wol_preferences = staged;
        if let Some(config) = state.platform.game_config.as_ref()
            && let Err(error) = profile.commit_wol_preferences(&config.paths.ra2_dir)
        {
            log::warn!("Could not save the WOL options: {error}");
        }
        state
            .frontend
            .multiplayer_shell_runtime
            .persist_observer_mode();
    }
    apply_wol_preferences(state);
    state.platform.window.request_redraw();
}

/// `0x2BC` has slid out for Sound Options: the art, then `0x6B6230` runs
/// `0xD6` because the handler cleared `g_GameActive` (`0x0078EEAD`).
pub(crate) fn commit_open_sound(state: &mut AppState) {
    let positions = crate::app::input::sound::volume_positions(state);
    let Some(page) = page_mut(state) else {
        return;
    };
    page.ui.buttons = Default::default();
    page.ui.hovered = None;
    // GameActive 0 skips the track list init (0x006B6485).
    page.sound = Some(SoundState::new(positions, Vec::new(), -1));
    open_sound_art(state);
    state
        .frontend
        .shell_controller
        .reset_to(SHELL_SOUND_PAGE.dialog, false);
    state.platform.window.request_redraw();
}

/// `0xD6`'s teardown has run: `ShowWindow(0x2BC, 5)` (it slides in again)
/// and `0x0072B930` frees the Sound Options art.
pub(crate) fn commit_sound_back(state: &mut AppState) {
    state.frontend.wol_sound_options_art = None;
    if let Some(page) = page_mut(state) {
        page.sound = None;
    }
    state.frontend.shell_controller.reset_to(DIALOG, false);
    state.platform.window.request_redraw();
}

fn dispatch(state: &mut AppState, action: WolOptionsAction) {
    match action {
        WolOptionsAction::Ok(staged) => {
            App::leave_shell_dialog(state, ShellExitThen::WolOptionsClose(Some(staged)))
        }
        WolOptionsAction::Cancel => {
            App::leave_shell_dialog(state, ShellExitThen::WolOptionsClose(None))
        }
        WolOptionsAction::SoundOptions => {
            App::leave_shell_dialog(state, ShellExitThen::WolSoundOptionsOpen)
        }
        WolOptionsAction::MusicPreview(checked) => {
            let now_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
            state.audio.apply_wol_lobby_music_rule(checked, now_ms);
        }
    }
}

pub(super) fn cursor_moved(state: &mut AppState) {
    let (x, y) = state.match_state.input.cursor_px();
    let options = layout(state);
    let sound = sound_layout(state);
    let mut changed = None;
    if let Some(page) = page_mut(state) {
        match page.sound.as_mut() {
            Some(dialog) => {
                dialog.hovered = sound.control_at(x, y);
                dialog.buttons.hovered = match dialog.hovered {
                    Some(SoundControl::Button(id)) => Some(id),
                    _ => None,
                };
                // The trackbar works in the authored dialog's pixels.
                let scale = sound.page.scale;
                let (bx, _) = scale.base_point(x, y);
                changed = dialog.dragging.and_then(|id| {
                    dialog
                        .set_from_pointer(id, scale.base_rect(sound.sliders[id as usize]), bx)
                        .map(|volume| (id, volume))
                });
            }
            None => {
                let over = options.control_at(x, y);
                page.ui.hovered = over;
                page.ui.buttons.hovered = over.filter(|id| is_button(*id));
            }
        }
    }
    if let Some((id, volume)) = changed {
        crate::app::input::sound::apply_slider_volume(state, id, volume);
    }
    state.frontend.shell_status_line.repaint();
    state.platform.window.request_redraw();
}

enum Press {
    Nothing,
    Button,
    Checkbox(Option<WolOptionsAction>),
}

pub(super) fn mouse(state: &mut AppState, pressed: bool) {
    let (x, y) = state.match_state.input.cursor_px();
    if page_mut(state).is_some_and(|page| page.sound.is_some()) {
        sound_mouse(state, pressed, x, y);
        return;
    }
    let layout = layout(state);
    let over = layout.control_at(x, y);
    let Some(page) = page_mut(state) else {
        return;
    };
    if !pressed {
        let button = over.filter(|id| is_button(*id));
        let action = page
            .ui
            .buttons
            .release(button)
            .and_then(|id| page.ui.activate(id));
        if let Some(action) = action {
            dispatch(state, action);
        }
        state.platform.window.request_redraw();
        return;
    }
    let press = match over {
        Some(id) if is_button(id) => {
            page.ui.buttons.press(Some(id));
            Press::Button
        }
        // OwnerDraw_CheckBox 0x006163A0 admits only its 18x18 icon and
        // toggles on the press.
        Some(id)
            if layout.checkbox_rect(id).is_some_and(|rect| {
                crate::ui::skirmish_shell::checkbox_icon_rect(rect, layout.page.scale)
                    .contains(x, y)
            }) =>
        {
            Press::Checkbox(page.ui.toggle(id))
        }
        _ => {
            page.ui.buttons.press(None);
            Press::Nothing
        }
    };
    match press {
        // Owner-draw buttons play GUIMainButtonSound on the press.
        Press::Button => App::play_main_menu_button_sound(state),
        Press::Checkbox(action) => {
            App::play_launcher_options_cue(state, LauncherCue::Checkbox);
            if let Some(action) = action {
                dispatch(state, action);
            }
        }
        Press::Nothing => {}
    }
    state.platform.window.request_redraw();
}

/// `0x6B6300` on `0xD6`: the sliders apply live and Back re-applies all
/// three (shared with B8); the rail press plays no generic click
/// (`0x006B6382`/`0x006B63EA`/`0x006B6444`).
fn sound_mouse(state: &mut AppState, pressed: bool, x: i32, y: i32) {
    let layout = sound_layout(state);
    let hit = layout.control_at(x, y);
    let over_button = match hit {
        Some(SoundControl::Button(id)) => Some(id),
        _ => None,
    };
    let Some(dialog) = page_mut(state).and_then(|page| page.sound.as_mut()) else {
        return;
    };
    if !pressed {
        dialog.dragging = None;
        let back = dialog.buttons.release(over_button) == Some(SoundButton::Back);
        let positions = dialog.positions;
        if back {
            crate::app::input::sound::apply_back_volumes(state, positions);
            App::leave_shell_dialog(state, ShellExitThen::WolSoundOptionsBack);
        }
        state.platform.window.request_redraw();
        return;
    }
    dialog.buttons.press(over_button);
    match hit {
        Some(SoundControl::Button(_)) => App::play_main_menu_button_sound(state),
        Some(SoundControl::Slider(id)) => {
            let scale = layout.page.scale;
            let (bx, by) = scale.base_point(x, y);
            let rect = scale.base_rect(layout.sliders[id as usize]);
            if let Some(volume) = dialog.press_slider(id, rect, bx, by) {
                crate::app::input::sound::apply_slider_volume(state, id, volume);
            }
        }
        _ => {}
    }
    state.platform.window.request_redraw();
}

/// The status help under the pointer: `0x2BC`'s `0x006040B0` rows; `0xD6`
/// has none.
pub(crate) fn status_key(state: &AppState) -> Option<&'static str> {
    let WolPage::Options(page) = &state.frontend.wol.as_ref()?.page else {
        return None;
    };
    if page.sound.is_some() {
        return None;
    }
    page.ui
        .hovered
        .and_then(crate::ui::wol_options::tooltip_key)
}

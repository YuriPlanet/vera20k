//! WOL Options `0x2BC` (proc `0x0078EBC0`) and its nested Sound Options
//! `0xD6` (proc `0x006B6300`), run from lobby `0x113`'s Options button
//! (`0x0078B829..0x0078BA39`). The lobby stays alive and hidden under them:
//! its service sinks keep writing to it, its timers are gated off.
//!
//! The pages' modal loop dispatches without `IsDialogMessage`, so Enter,
//! Escape and Tab do nothing, and its idle (`0x0077F900`) never closes the
//! dialog; Escape only closes VERA's open translation list.

use super::{WolPage, apply_wol_preferences};
use crate::app::frontend::shell_layout;
use crate::app::{App, AppState};
use crate::ui::chat_translation::ChatTranslation;
use crate::ui::shell::page::{self, PageCue, PageEvent};
use crate::ui::shell::sound::{self, SHELL_SOUND_PAGE, ShellSoundLayout};
use crate::ui::wol_options::{
    self, DIALOG, TRANSLATE, TRANSLATE_OFF_KEY, WolOptionsAction, WolOptionsLayout, WolPreferences,
};
use vera20k_gui::shell::combobox::ComboPopup;
use vera20k_gui::{Gui, NodeId};

/// The open `0x2BC`, and `0xD6` while it runs over the hidden `0x2BC`. Their
/// widgets hold what they show; these are what each builds from.
#[derive(Debug, Clone)]
pub(crate) struct WolOptionsPage {
    /// What `0x2BC` shows when built: the live values (init `0x497`), or
    /// what it showed when it hid for Sound Options.
    pub(crate) shown: (WolPreferences, ChatTranslation),
    /// `0xD6`'s slider positions and whether they take a press.
    pub(crate) sound: Option<([u8; 3], bool)>,
}

pub(crate) fn layout(state: &AppState) -> WolOptionsLayout {
    let layout = shell_layout::on(state, WolOptionsLayout::new);
    if state
        .frontend
        .wol
        .as_ref()
        .is_some_and(|visit| visit.modern_ui())
    {
        layout.modern()
    } else {
        layout
    }
}

pub(crate) fn sound_layout(state: &AppState) -> ShellSoundLayout {
    shell_layout::on(state, ShellSoundLayout::new)
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
    let shown = (
        state.frontend.wol_preferences,
        state.persistence.options_profile.chat_translation,
    );
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    if let Some(lobby) = visit.lobby_mut() {
        lobby.shown = false;
        lobby.ui.release_buttons();
    }
    visit.page = WolPage::Options(Box::new(WolOptionsPage { shown, sound: None }));
    super::leaf::open_art(state, crate::ui::wol_options::DIALOG);
    state.frontend.shell_controller.reset_to(DIALOG);
    sync_gui(state);
}

/// `0x2BC`'s teardown has run (`CloseAndPop`). The caller re-shows the
/// lobby (it slides in on its first paint), frees the art and focuses the
/// chat edit (`0x0078BF1F`). OK commits the staged values and writes them
/// with ObserverMode (`0x0077DFD0`); both results then apply (`0x0077E110`).
/// Changed translation settings are saved too.
pub(crate) fn commit_close(
    state: &mut AppState,
    staged: Option<(WolPreferences, ChatTranslation)>,
) {
    super::leaf::free_art(state, crate::ui::wol_options::DIALOG);
    super::leaf::free_art(state, SHELL_SOUND_PAGE.dialog);
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
        .reset_to(super::WOL_LOBBY_DIALOG);
    if let Some((staged, translate)) = staged {
        state.frontend.wol_preferences = staged;
        let profile = &mut state.persistence.options_profile;
        profile.wol_preferences = staged;
        let translate_changed = profile.chat_translation != translate;
        profile.chat_translation = translate;
        if let Some(config) = state.platform.game_config.as_ref() {
            let dir = config.paths.settings_dir();
            if let Err(error) = profile.commit_wol_preferences(dir) {
                log::warn!("Could not save the WOL options: {error}");
            }
            if translate_changed && let Err(error) = profile.commit_chat_translation(dir) {
                log::warn!("Could not save the chat translation settings: {error}");
            }
        }
        state
            .frontend
            .multiplayer_shell_runtime
            .persist_observer_mode();
    }
    apply_wol_preferences(state);
}

/// `0x2BC` has slid out for Sound Options: the art, then `0x6B6230` runs
/// `0xD6` because the handler cleared `g_GameActive` (`0x0078EEAD`).
pub(crate) fn commit_open_sound(state: &mut AppState) {
    let positions = crate::app::input::sound::volume_positions(state);
    let sliders_enabled = state.audio.launcher_audio_available;
    let Some(page) = page_mut(state) else {
        return;
    };
    // GameActive 0 skips the track list init (0x006B6485).
    page.sound = Some((positions, sliders_enabled));
    super::leaf::open_art(state, SHELL_SOUND_PAGE.dialog);
    state
        .frontend
        .shell_controller
        .reset_to(SHELL_SOUND_PAGE.dialog);
    sync_gui(state);
}

/// `0xD6`'s teardown has run: `ShowWindow(0x2BC, 5)` (it slides in again)
/// and `0x0072B930` frees the Sound Options art.
pub(crate) fn commit_sound_back(state: &mut AppState) {
    super::leaf::free_art(state, SHELL_SOUND_PAGE.dialog);
    if let Some(page) = page_mut(state) {
        page.sound = None;
    }
    state.frontend.shell_controller.reset_to(DIALOG);
    sync_gui(state);
}

fn dispatch(state: &mut AppState, action: WolOptionsAction) {
    match action {
        WolOptionsAction::Ok(staged, translate) => App::leave_shell_dialog(
            state,
            crate::app::frontend::shell_transition::ShellSlideKind::WolOptions,
            move |state| {
                crate::app::shell_wol::options::commit_close(state, Some((staged, translate)))
            },
        ),
        WolOptionsAction::Cancel => App::leave_shell_dialog(
            state,
            crate::app::frontend::shell_transition::ShellSlideKind::WolOptions,
            move |state| crate::app::shell_wol::options::commit_close(state, None),
        ),
        WolOptionsAction::SoundOptions => {
            // 0x2BC only hides under 0xD6: it shows the same states again.
            if let Some(shown) = shown(state)
                && let Some(page) = page_mut(state)
            {
                page.shown = shown;
            }
            App::leave_shell_dialog(
                state,
                crate::app::frontend::shell_transition::ShellSlideKind::WolOptions,
                crate::app::shell_wol::options::commit_open_sound,
            )
        }
        WolOptionsAction::MusicPreview(checked) => {
            let now_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
            state.audio.apply_wol_lobby_music_rule(checked, now_ms);
        }
    }
}

/// The shown page's dialog root, and whether it is `0xD6`'s.
fn shown_root(state: &AppState) -> Option<(NodeId, bool)> {
    let WolPage::Options(page) = &state.frontend.wol.as_ref()?.page else {
        return None;
    };
    let dialog = match page.sound {
        Some(_) => SHELL_SOUND_PAGE.dialog,
        None => DIALOG,
    };
    let controller = &state.frontend.shell_controller;
    let root = controller.root_in(dialog)?;
    Some((root, page.sound.is_some())).filter(|_| controller.top_id() == Some(dialog))
}

/// `read` of the shown page's tree, when that is `0xD6` (`sound`) or `0x2BC`.
fn read<'a, T>(
    state: &'a AppState,
    sound: bool,
    read: impl FnOnce(&'a Gui, NodeId) -> T,
) -> Option<T> {
    let (root, on_sound) = shown_root(state)?;
    (on_sound == sound).then(|| read(state.frontend.shell_controller.gui(), root))
}

/// What the shown `0x2BC`'s boxes and list hold.
pub(crate) fn shown(state: &AppState) -> Option<(WolPreferences, ChatTranslation)> {
    read(state, false, wol_options::shown)
}

/// The shown `0xD6`'s slider positions.
pub(crate) fn sound_positions(state: &AppState) -> Option<[u8; 3]> {
    read(state, true, sound::shell_sound_positions)
}

/// Builds the shown page's widgets (`0x2BC`'s or `0xD6`'s) under its dialog
/// once, and puts them on the current layout.
pub(crate) fn sync_gui(state: &mut AppState) {
    let options = layout(state);
    let sound = sound_layout(state);
    let off = state.process_assets.csf.as_ref().map_or_else(
        || TRANSLATE_OFF_KEY.to_owned(),
        |csf| csf.text(TRANSLATE_OFF_KEY).into_owned(),
    );
    let Some((root, _)) = shown_root(state) else {
        return;
    };
    let frontend = &mut state.frontend;
    let Some(WolPage::Options(page)) = frontend.wol.as_ref().map(|visit| &visit.page) else {
        return;
    };
    let controller = &mut frontend.shell_controller;
    match page.sound {
        Some((positions, enabled)) => {
            controller.set_scale(sound.page.scale);
            let gui = controller.gui_mut();
            page::ensure_built(gui, root, || sound::shell_sound_spec(positions, enabled));
            page::sync_rects(gui, root, |id| sound.rect(id));
        }
        None => {
            controller.set_scale(options.page.scale);
            let gui = controller.gui_mut();
            page::ensure_built(gui, root, || wol_options::spec(page.shown, &off));
            page::sync_rects(gui, root, |id| options.rect(id));
        }
    }
}

/// The widget events of the last input, in order: each control's cue, then
/// what it does. `0x6B6300` on `0xD6`: the sliders apply live and Back
/// re-applies all three (shared with B8). The rail press plays nothing
/// (`0x006B6382`/`0x006B63EA`/`0x006B6444`).
fn handle_events(state: &mut AppState) {
    let raw = state.frontend.shell_controller.drain_events();
    let Some((root, on_sound)) = shown_root(state) else {
        return;
    };
    for event in page::page_events(state.frontend.shell_controller.gui(), raw) {
        let gui = state.frontend.shell_controller.gui();
        match event {
            PageEvent::Cue(cue) => {
                App::play_page_cue(state, cue);
                if matches!(cue, PageCue::ComboOpen | PageCue::ComboClose) {
                    state.frontend.shell_status_line.repaint();
                }
            }
            PageEvent::Activated(id) if on_sound && id == SHELL_SOUND_PAGE.back.id => {
                let positions = sound::shell_sound_positions(gui, root);
                crate::app::input::sound::apply_back_volumes(state, positions);
                App::leave_shell_dialog(
                    state,
                    crate::app::frontend::shell_transition::ShellSlideKind::WolSoundOptions,
                    crate::app::shell_wol::options::commit_sound_back,
                );
            }
            _ if on_sound => {
                if let Some((id, volume)) = sound::shell_sound_volume(&event) {
                    crate::app::input::sound::apply_slider_volume(state, id, volume);
                }
            }
            PageEvent::Picked(TRANSLATE, _) | PageEvent::Scrolled(TRANSLATE) => {
                state.frontend.shell_status_line.repaint();
            }
            _ => {
                if let Some(action) = wol_options::action(gui, root, &event) {
                    dispatch(state, action);
                }
            }
        }
    }
}

pub(super) fn cursor_moved(state: &mut AppState) {
    let (x, y) = state.match_state.input.cursor_px();
    sync_gui(state);
    state.frontend.shell_controller.on_pointer_move(x, y);
    handle_events(state);
    state.frontend.shell_status_line.repaint();
}

pub(super) fn mouse(state: &mut AppState, pressed: bool) {
    let (x, y) = state.match_state.input.cursor_px();
    sync_gui(state);
    let controller = &mut state.frontend.shell_controller;
    if pressed {
        controller.on_pointer_down(x, y);
    } else {
        controller.on_pointer_up(x, y);
    }
    handle_events(state);
}

/// The open translation list, where it hits.
pub(crate) fn translate_popup(state: &AppState) -> Option<ComboPopup> {
    read(state, false, |gui, root| {
        Some(
            gui.combo_drop_down(gui.find_id(root, TRANSLATE.into())?)?
                .popup,
        )
    })?
}

/// The status help under the pointer: the hovered `0x2BC` control's
/// `0x006040B0` row; `0xD6` has none.
pub(crate) fn status_key(state: &AppState) -> Option<&str> {
    read(state, false, |gui, _| page::hovered_key(gui))?
}

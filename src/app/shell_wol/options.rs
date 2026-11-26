//! WOL Options `0x2BC` (proc `0x0078EBC0`) and its nested Sound Options
//! `0xD6` (proc `0x006B6300`), run from lobby `0x113`'s Options button
//! (`0x0078B829..0x0078BA39`). The lobby stays alive and hidden under them:
//! its service sinks keep writing to it, its timers are gated off.
//!
//! The classic modal loop dispatches without `IsDialogMessage`, so Enter,
//! Escape and Tab do nothing. The modern page applies options live and
//! Escape closes its open translation list.

use super::{WolPage, apply_wol_preferences};
use crate::app::audio_runtime::AppAudioRuntime;
use crate::app::frontend::shell_layout;
use crate::app::frontend::state::FrontendState;
use crate::app::persistence::{options_profile::RetailOptionsProfile, profile_file};
use crate::app::shell_wol::WolVisit;
use crate::app::{App, AppState};
use crate::ui::chat_translation::ChatTranslation;
use crate::ui::shell::page::{self, PageCue, PageEvent};
use crate::ui::shell::sound::{self, SHELL_SOUND_PAGE, ShellSoundLayout};
use crate::ui::wol_options::{
    self, DIALOG, TRANSLATE, TRANSLATE_OFF_KEY, WolOptionsAction, WolOptionsLayout, WolPreferences,
};
use crate::util::config::GameConfig;
use crate::util::ini_writer::set_ini_values;
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
    if modern(state.frontend.wol.as_ref()) {
        shell_layout::on(&state.renderer.gpu, |width, height, _| {
            WolOptionsLayout::modern(width, height)
        })
    } else {
        shell_layout::on(&state.renderer.gpu, WolOptionsLayout::new)
    }
}

fn modern(wol: Option<&WolVisit>) -> bool {
    wol.is_some_and(|visit| visit.modern_ui())
}

pub(crate) fn menu_spec(
    wol: Option<&WolVisit>,
) -> &'static crate::ui::shell::menu_page::MenuPageSpec {
    if modern(wol) {
        &wol_options::MODERN_PAGE
    } else {
        &wol_options::PAGE
    }
}

pub(crate) fn sound_layout(state: &AppState) -> ShellSoundLayout {
    shell_layout::on(&state.renderer.gpu, ShellSoundLayout::new)
}

fn page_mut(wol: Option<&mut WolVisit>) -> Option<&mut WolOptionsPage> {
    match &mut wol?.page {
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
    frontend: &mut FrontendState,
    game_config: Option<&GameConfig>,
    options_profile: &mut RetailOptionsProfile,
    audio: &mut AppAudioRuntime,
    staged: Option<(WolPreferences, ChatTranslation)>,
) {
    super::leaf::free_art(&mut frontend.wol_art, crate::ui::wol_options::DIALOG);
    super::leaf::free_art(&mut frontend.wol_art, SHELL_SOUND_PAGE.dialog);
    let Some(visit) = frontend.wol.as_mut() else {
        return;
    };
    visit.page = WolPage::Lobby;
    if let Some(lobby) = visit.lobby_mut() {
        lobby.shown = true;
        lobby.ui.refocus_chat();
    }
    frontend.shell_controller.reset_to(super::WOL_LOBBY_DIALOG);
    if let Some((staged, translate)) = staged {
        frontend.wol_preferences = staged;
        options_profile.wol_preferences = staged;
        let translate_changed = options_profile.chat_translation != translate;
        options_profile.chat_translation = translate;
        if let Some(config) = game_config {
            let dir = config.paths.settings_dir();
            if let Err(error) = options_profile.commit_wol_preferences(dir) {
                log::warn!("Could not save the WOL options: {error}");
            }
            if translate_changed && let Err(error) = options_profile.commit_chat_translation(dir) {
                log::warn!("Could not save the chat translation settings: {error}");
            }
        }
        frontend.multiplayer_shell_runtime.persist_observer_mode();
    }
    if !modern(frontend.wol.as_ref()) {
        apply_wol_preferences(frontend, audio);
    }
}

/// `0x2BC` has slid out for Sound Options: the art, then `0x6B6230` runs
/// `0xD6` because the handler cleared `g_GameActive` (`0x0078EEAD`).
pub(crate) fn commit_open_sound(state: &mut AppState) {
    let positions = crate::app::input::sound::volume_positions(&state.persistence.options_profile);
    let sliders_enabled = state.audio.launcher_audio_available;
    let Some(page) = page_mut(state.frontend.wol.as_mut()) else {
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
    super::leaf::free_art(&mut state.frontend.wol_art, SHELL_SOUND_PAGE.dialog);
    if let Some(page) = page_mut(state.frontend.wol.as_mut()) {
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
                crate::app::shell_wol::options::commit_close(
                    &mut state.frontend,
                    state.platform.game_config.as_ref(),
                    &mut state.persistence.options_profile,
                    &mut state.audio,
                    Some((staged, translate)),
                )
            },
        ),
        WolOptionsAction::Cancel | WolOptionsAction::Back => App::leave_shell_dialog(
            state,
            crate::app::frontend::shell_transition::ShellSlideKind::WolOptions,
            move |state| {
                crate::app::shell_wol::options::commit_close(
                    &mut state.frontend,
                    state.platform.game_config.as_ref(),
                    &mut state.persistence.options_profile,
                    &mut state.audio,
                    None,
                )
            },
        ),
        WolOptionsAction::SoundOptions => {
            // 0x2BC only hides under 0xD6: it shows the same states again.
            if let Some(shown) = shown(&state.frontend)
                && let Some(page) = page_mut(state.frontend.wol.as_mut())
            {
                page.shown = shown;
            }
            App::leave_shell_dialog(
                state,
                crate::app::frontend::shell_transition::ShellSlideKind::WolOptions,
                crate::app::shell_wol::options::commit_open_sound,
            )
        }
        WolOptionsAction::MusicPreview(checked) => apply_lobby_music(&mut state.audio, checked),
        WolOptionsAction::LobbyMusic(checked) => {
            let preferences = &mut state.frontend.wol_preferences;
            preferences.lobby_music = i32::from(checked);
            let profile = &mut state.persistence.options_profile;
            profile.wol_preferences.lobby_music = preferences.lobby_music;
            apply_lobby_music(&mut state.audio, checked);
            if let Some(config) = state.platform.game_config.as_ref()
                && let Err(error) = state
                    .persistence
                    .options_profile
                    .commit_wol_preferences(config.paths.settings_dir())
            {
                log::warn!("Could not save the WOL options: {error}");
            }
        }
        WolOptionsAction::Translation(translate) => {
            let profile = &mut state.persistence.options_profile;
            if profile.chat_translation != translate {
                profile.chat_translation = translate;
                if let Some(config) = state.platform.game_config.as_ref()
                    && let Err(error) = profile.commit_chat_translation(config.paths.settings_dir())
                {
                    log::warn!("Could not save the chat translation settings: {error}");
                }
            }
        }
    }
}

fn apply_lobby_music(audio: &mut AppAudioRuntime, enabled: bool) {
    let now_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
    audio.apply_wol_lobby_music_rule(enabled, now_ms);
}

/// The shown page's dialog root, and whether it is `0xD6`'s.
fn shown_root(frontend: &FrontendState) -> Option<(NodeId, bool)> {
    let WolPage::Options(page) = &frontend.wol.as_ref()?.page else {
        return None;
    };
    let dialog = match page.sound {
        Some(_) => SHELL_SOUND_PAGE.dialog,
        None => DIALOG,
    };
    let controller = &frontend.shell_controller;
    let root = controller.root_in(dialog)?;
    Some((root, page.sound.is_some())).filter(|_| controller.top_id() == Some(dialog))
}

/// `read` of the shown page's tree, when that is `0xD6` (`sound`) or `0x2BC`.
fn read<'a, T>(
    frontend: &'a FrontendState,
    sound: bool,
    read: impl FnOnce(&'a Gui, NodeId) -> T,
) -> Option<T> {
    let (root, on_sound) = shown_root(frontend)?;
    (on_sound == sound).then(|| read(frontend.shell_controller.gui(), root))
}

/// What the shown `0x2BC`'s boxes and list hold.
pub(crate) fn shown(frontend: &FrontendState) -> Option<(WolPreferences, ChatTranslation)> {
    read(frontend, false, wol_options::shown)
}

/// The visible volume controls' positions.
pub(crate) fn sound_positions(frontend: &FrontendState) -> Option<[u8; 3]> {
    read(
        frontend,
        !modern(frontend.wol.as_ref()),
        sound::shell_sound_positions,
    )
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
    let Some((root, _)) = shown_root(&state.frontend) else {
        return;
    };
    let audio = modern(state.frontend.wol.as_ref()).then(|| {
        (
            crate::app::input::sound::volume_positions(&state.persistence.options_profile),
            state.audio.launcher_audio_available,
        )
    });
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
            page::ensure_built(gui, root, || wol_options::spec(page.shown, &off, audio));
            page::sync_rects(gui, root, |id| options.rect(id));
        }
    }
}

/// Keep the shared profile's canonical formatting, but apply only the volume
/// keys this page owns. Lobby sliders must not commit the complete profile.
fn transform_lobby_volumes(input: &[u8], profile: &RetailOptionsProfile) -> Vec<u8> {
    let audio = profile.formatted_sections().audio;
    let volumes: Vec<_> = audio
        .iter()
        .filter(|(key, _)| matches!(*key, "SoundVolume" | "VoiceVolume" | "ScoreVolume"))
        .map(|(key, value)| (*key, value.as_str()))
        .collect();
    set_ini_values(input, "Audio", &volumes)
}

/// The widget events of the last input, in order: each control's cue, then
/// what it does. `0x6B6300` on `0xD6`: the sliders apply live and Back
/// re-applies all three (shared with B8). The rail press plays nothing
/// (`0x006B6382`/`0x006B63EA`/`0x006B6444`).
fn handle_events(state: &mut AppState) {
    let raw = state.frontend.shell_controller.drain_events();
    let Some((root, on_sound)) = shown_root(&state.frontend) else {
        return;
    };
    let modern = modern(state.frontend.wol.as_ref());
    for event in page::page_events(state.frontend.shell_controller.gui(), raw) {
        let gui = state.frontend.shell_controller.gui();
        match event {
            PageEvent::Cue(cue) => {
                App::play_page_cue(
                    &state.frontend,
                    &state.match_state,
                    &state.process_assets,
                    &mut state.audio.sfx_player,
                    cue,
                );
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
            _ => {
                if matches!(
                    event,
                    PageEvent::Picked(TRANSLATE, _) | PageEvent::Scrolled(TRANSLATE)
                ) {
                    state.frontend.shell_status_line.repaint();
                }
                if let Some(action) =
                    wol_options::action(state.frontend.shell_controller.gui(), root, &event, modern)
                {
                    dispatch(state, action);
                } else if modern && let Some((id, volume)) = sound::shell_sound_volume(&event) {
                    crate::app::input::sound::apply_slider_volume(state, id, volume);
                    if let Some(config) = state.platform.game_config.as_ref()
                        && let Err(error) =
                            profile_file::update(config.paths.settings_dir(), |input| {
                                transform_lobby_volumes(input, &state.persistence.options_profile)
                            })
                    {
                        log::warn!("Could not save the audio volumes: {error}");
                    }
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

/// Escape dismisses the translation list without changing its selection.
pub(super) fn key_press(
    state: &mut AppState,
    code: Option<winit::keyboard::KeyCode>,
    _: Option<&str>,
) {
    if code == Some(winit::keyboard::KeyCode::Escape) && translate_popup(&state.frontend).is_some()
    {
        let event = vera20k_gui::InputEvent::Key(vera20k_gui::Key::Escape);
        state.frontend.shell_controller.on_input(event);
        handle_events(state);
        state.frontend.shell_status_line.repaint();
        state.platform.window.request_redraw();
    }
}

/// The open translation list, where it hits.
pub(crate) fn translate_popup(frontend: &FrontendState) -> Option<ComboPopup> {
    read(frontend, false, |gui, root| {
        Some(
            gui.combo_drop_down(gui.find_id(root, TRANSLATE.into())?)?
                .popup,
        )
    })?
}

/// The status help under the pointer: the hovered `0x2BC` control's
/// `0x006040B0` row; `0xD6` has none.
pub(crate) fn status_key(frontend: &FrontendState) -> Option<&str> {
    read(frontend, false, |gui, _| page::hovered_key(gui))?
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;

    #[test]
    fn live_lobby_audio_write_preserves_every_non_volume_key() {
        let mut profile = RetailOptionsProfile::default();
        profile.sound_volume = 0.6;
        profile.voice_volume = 0.9;
        profile.score_volume = 0.2;
        let input = b"[Audio]\r\nSoundVolume=0.1\r\nVoiceVolume=0.1\r\nScoreVolume=0.1\r\n\
                      IsScoreRepeat=yes\r\nOpaque=\xE9\r\n[Video]\r\nScreenWidth=641\r\n[WOnline]\r\nLobMusic=0\r\n";
        let expected = b"[Audio]\r\nSoundVolume=0.600000\r\nVoiceVolume=0.900000\r\nScoreVolume=0.200000\r\n\
                         IsScoreRepeat=yes\r\nOpaque=\xE9\r\n[Video]\r\nScreenWidth=641\r\n[WOnline]\r\nLobMusic=0\r\n";
        assert_eq!(transform_lobby_volumes(input, &profile), expected);
    }
}

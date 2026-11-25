//! The player's own interface layout: `veraui.ini` in the VERA20k folder,
//! laid over the mod's (`ui::match_layout::MatchLayoutFile::from_layers`).
//!
//! The mod's file comes through the asset manager with the rest of the mod;
//! the player's is a plain file the game reads when a match starts and writes
//! when the player moves a part (`input::layout_edit`). A capture run reads
//! and writes none, so its picture never depends on one player's folder.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::presentation::state::MatchPresentationState;
use crate::app::state::PlatformState;
use crate::rules::ini_parser::IniFile;
use crate::ui::match_layout::{FILE_NAME, MatchLayoutFile};

/// The player's file in the VERA20k folder, whatever capitals its name has;
/// `None` in a capture run or without a settings folder.
fn path(platform: &PlatformState) -> Option<std::path::PathBuf> {
    if platform.capture_client_size.is_some() {
        return None;
    }
    let dir = platform.game_config.as_ref()?.paths.settings_dir();
    Some(
        crate::util::case_insensitive_path::find(dir, FILE_NAME)
            .unwrap_or_else(|| dir.join(FILE_NAME.to_ascii_lowercase())),
    )
}

/// The player's file as it is on disk; empty when there is none.
fn read(platform: &PlatformState) -> Vec<u8> {
    path(platform)
        .and_then(|path| crate::util::file_system::read(path).ok())
        .unwrap_or_default()
}

/// Write the player's file as the match holds it; with nothing left in it,
/// as after a Reset, remove the file. A capture run writes none.
pub(crate) fn write(platform: &PlatformState, presentation: &MatchPresentationState) {
    let Some(path) = path(platform) else {
        return;
    };
    let text = &presentation.player_layout_text;
    if text.iter().all(u8::is_ascii_whitespace) {
        match crate::util::file_system::remove_file(&path) {
            Ok(()) => log::info!("Interface layout reset: removed {}", path.display()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => log::warn!(
                "Interface layout could not be removed from {}: {error}",
                path.display()
            ),
        }
        return;
    }
    let written = path
        .parent()
        .map_or(Ok(()), crate::util::file_system::create_dir_all)
        .and_then(|()| crate::util::file_system::write(&path, text));
    match written {
        Ok(()) => log::info!("Interface layout saved to {}", path.display()),
        Err(error) => log::warn!(
            "Interface layout could not be saved to {}: {error}",
            path.display()
        ),
    }
}

/// A new match's layouts: the mod's file as the loader parsed it, and the
/// player's own from the VERA20k folder.
pub(crate) fn install(
    platform: &PlatformState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    persistence: &PersistenceState,
    mod_file: Option<IniFile>,
) {
    let player = read(platform);
    let presentation = &mut match_state.match_presentation;
    presentation.mod_layout_ini = mod_file;
    presentation.player_layout_text = player;
    presentation.layout_edit = None;
    rebuild(platform, frontend, match_state, persistence, true);
}

/// Read the two layers again into the layouts the match uses, and lay the
/// local side's sidebar out by them. `log` lists every refused line, as a new
/// match does once.
pub(crate) fn rebuild(
    platform: &PlatformState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    persistence: &PersistenceState,
    log: bool,
) {
    let presentation = &match_state.match_presentation;
    // `LoadFromStraw` drops a last line that has no line break.
    let mut text = presentation.player_layout_text.clone();
    let player = (!text.is_empty())
        .then(|| {
            text.push(b'\n');
            IniFile::from_bytes(&text).ok()
        })
        .flatten();
    let mut layouts =
        MatchLayoutFile::from_layers(presentation.mod_layout_ini.as_ref(), player.as_ref());
    // A button's command is one the keyboard file or the command bar names.
    layouts.check_commands(|name| {
        crate::app::presentation::layout_parts::command_named(name).is_some()
    });
    if log && (presentation.mod_layout_ini.is_some() || player.is_some()) {
        log::info!(
            "Interface layouts read from {FILE_NAME} (the mod's: {}, the player's: {}), {} line(s) refused",
            presentation.mod_layout_ini.is_some(),
            player.is_some(),
            layouts.problems().len()
        );
        for problem in layouts.problems() {
            log::warn!("{FILE_NAME} {problem}");
        }
    }
    match_state.match_presentation.match_layouts = layouts;
    match_state.match_presentation.sidebar_layout_spec =
        crate::app::presentation::sidebar_render::sidebar_layout_spec(
            platform,
            frontend,
            match_state,
            persistence,
        );
    crate::app::presentation::sidebar_render::refresh_added_parts(platform, frontend, match_state);
}

//! VERA's opt-in autosave (`[VERA] AutosaveMinutes=`, off by default; gamemd
//! has none). In a campaign or skirmish match, once that many minutes passed
//! since the match started or was last saved and the game has advanced, the
//! match is written to `autosave.sav` in the saves folder, replacing the
//! previous autosave through the repository's staged rename. The Load page
//! lists it like any save. Network matches are left alone.

use crate::util::time::{Duration, Instant};

use crate::ui::game_screen::GameScreen;

pub(crate) const FILE_NAME: &str = "autosave.sav";

/// The autosave schedule, held by [`super::PersistenceState`].
#[derive(Debug, Default)]
pub(crate) struct Autosave {
    /// When the running match was first seen or last saved.
    since: Option<Instant>,
    /// The frame of the last save: a match that stands still is not saved again.
    frame: Option<u32>,
}

/// Every event-loop turn.
pub(crate) fn poll(
    platform: &crate::app::PlatformState,
    renderer: &crate::app::renderer_state::RendererState,
    screen: &GameScreen,
    match_state: &mut crate::app::match_runtime::state::MatchState,
    persistence: &mut super::PersistenceState,
    audio: &crate::app::audio_runtime::AppAudioRuntime,
    diag: &crate::app::diagnostics::state::DiagnosticsState,
    process_assets: &crate::app::process_assets::ProcessAssets,
) {
    let minutes = persistence.options_profile.autosave_minutes;
    let frame = match_state
        .sim_runtime
        .runtime()
        .map(|runtime| runtime.simulation.session().binary_frame());
    let in_match = *screen == GameScreen::InGame && frame.is_some();
    let single_player = match_state.session_mode().is_single_player();
    let now = Instant::now();
    let autosave = &mut persistence.autosave;
    if !in_match {
        *autosave = Autosave::default();
        return;
    }
    let since = *autosave.since.get_or_insert(now);
    if !is_due(
        minutes,
        now.duration_since(since),
        single_player,
        frame,
        autosave.frame,
    ) {
        return;
    }
    let map = match_state
        .sim_runtime
        .runtime()
        .map(|runtime| runtime.simulation.session().map_name().to_owned())
        .unwrap_or_default();
    let map = map.rsplit_once('.').map_or(map.as_str(), |(stem, _)| stem);
    match crate::app::persistence::commands::autosave_game(
        platform,
        renderer,
        screen,
        match_state,
        persistence,
        audio,
        diag,
        process_assets,
        &format!("Autosave {map}"),
    ) {
        Ok(path) => log::info!("Autosave: {}", path.display()),
        Err(error) => log::warn!("Autosave failed: {error}"),
    }
    persistence.autosave = Autosave {
        since: Some(Instant::now()),
        frame,
    };
}

/// Whether an offline match that ran `elapsed` since its last save is saved
/// now: the option is on, the time is up and the game advanced.
fn is_due(
    minutes: u8,
    elapsed: Duration,
    single_player: bool,
    frame: Option<u32>,
    saved_frame: Option<u32>,
) -> bool {
    minutes > 0
        && single_player
        && elapsed >= Duration::from_secs(u64::from(minutes) * 60)
        && frame.is_some_and(|frame| frame > 0)
        && frame != saved_frame
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;

    const FIVE: Duration = Duration::from_secs(300);

    #[test]
    fn off_by_default_and_in_network_matches() {
        assert!(!is_due(0, FIVE, true, Some(900), None));
        assert!(!is_due(5, FIVE, false, Some(900), None));
        assert!(is_due(5, FIVE, true, Some(900), None));
    }

    #[test]
    fn waits_for_the_interval_and_a_new_frame() {
        assert!(!is_due(
            5,
            FIVE - Duration::from_secs(1),
            true,
            Some(900),
            None
        ));
        assert!(!is_due(5, FIVE, true, Some(900), Some(900)));
        assert!(is_due(5, FIVE, true, Some(901), Some(900)));
    }
}

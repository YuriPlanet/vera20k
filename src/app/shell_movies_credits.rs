//! Movies & Credits route: dialog `0x101`, movie list `0x129`, Play_Movie and
//! Show_Credits, following the `Main__PrepareSession` `0x0052D9A0` states
//! 4, `0xD`, `0xE` and `0xF`. Each child destroys its source dialog first and
//! recreates its return dialog afterwards.

use crate::sim::trigger_runtime::ThemeRequest;
use crate::util::time::Instant;
use std::sync::Arc;

use super::{App, AppState};
use crate::app::frontend::credits_roll::{
    CreditsInput, CreditsReturn, CreditsRoll, CreditsRollSession, CreditsViewport,
};
use crate::app::frontend::fullscreen_movie::{FullscreenMovie, MovieReturn, movie_stem};
use crate::app::shell_route::ShellRoute;
use crate::ui::movies_credits_shell::{
    MOVIE_LIST_DIALOG, MOVIES_CREDITS_PAGE, MoviesCreditsAction, active_movie_entries,
    family_list_node, family_list_selected, retained_movie_row,
};
use crate::ui::shell::page::{self, PageCue, PageEvent};

/// The Sneak Peeks movie (`0x0052DE55`).
const SNEAK_PEEK_MOVIE: &str = "RENEGADE.BIK";
const CREDITS_FILE: &str = "CREDITSMD.TXT";
const CREDITS_THEME: &str = "CREDITS";
/// Set_Score_Volume(0.4f) at `0x004C42C3` when ScoreVolume is exactly 0.
const CREDITS_SILENT_SCORE_VOLUME: f32 = 0.4;

impl App {
    /// The shell dialog being left is destroyed (child `0x71A` with it)
    /// before the next dialog or presentation is created.
    pub(super) fn destroy_current_shell_dialog(state: &mut AppState) {
        crate::app::frontend::main_menu_shell_render::clear_ra2ts_movie_session(state);
        crate::app::frontend::shell_transition::invalidate_main_menu_dialog_instance(state);
    }

    /// State 4: create Movies & Credits `0x101`.
    pub(super) fn open_movies_credits_page(state: &mut AppState) {
        Self::destroy_current_shell_dialog(state);
        state.frontend.movie_list = None;
        state.frontend.shell_route = ShellRoute::MoviesAndCredits;
        state
            .frontend
            .shell_controller
            .reset_to(MOVIES_CREDITS_PAGE.dialog, false);
    }

    pub(super) fn handle_movies_credits_action(state: &mut AppState, action: MoviesCreditsAction) {
        match action {
            MoviesCreditsAction::SneakPeeks => {
                Self::start_fullscreen_movie(
                    state,
                    SNEAK_PEEK_MOVIE,
                    MovieReturn::MoviesAndCredits,
                );
            }
            MoviesCreditsAction::PlayMovies => Self::open_movie_list(state),
            MoviesCreditsAction::ViewCredits => {
                Self::start_credits_roll(state, CreditsReturn::MoviesAndCredits)
            }
            MoviesCreditsAction::MainMenu => {
                Self::destroy_current_shell_dialog(state);
                state.frontend.shell_route = ShellRoute::MainMenu;
            }
        }
    }

    /// State `0xE`: create movie list `0x129` with the rows `0x005FC000` adds
    /// and the retained selection `DAT_00825C80`.
    pub(super) fn open_movie_list(state: &mut AppState) {
        Self::destroy_current_shell_dialog(state);
        let rows = active_movie_entries(state.persistence.options_profile.movie_progress);
        state.frontend.shell_route = ShellRoute::MovieList;
        state.frontend.movie_list = Some(rows);
        state
            .frontend
            .shell_controller
            .reset_to(MOVIE_LIST_DIALOG, false);
        Self::movie_list_feed(state);
    }

    pub(super) fn movie_list_active(state: &AppState) -> bool {
        state.frontend.screen == crate::ui::game_screen::GameScreen::MainMenu
            && state.frontend.shell_route.movie_list()
            && state.frontend.movie_list.is_some()
    }

    /// The page's buttons. The list `0x744` is the dialog's `List` node: a
    /// hover target for status help and its own press handler, never a
    /// button press.
    fn movie_list_feed(state: &mut AppState) -> Vec<crate::ui::shell::layout::LaidOutControl> {
        let layout = crate::app::frontend::shell_layout::movie_list_layout(state);
        let retained = state.frontend.movie_list_selection;
        let controller = &mut state.frontend.shell_controller;
        controller.ensure_active(MOVIE_LIST_DIALOG, false);
        controller.set_scale(layout.page.scale);
        if let Some(rows) = state.frontend.movie_list.as_ref() {
            let list = (rows.len(), retained_movie_row(rows, retained), 0);
            family_list_node(controller, MOVIE_LIST_DIALOG, layout.list, list, true);
        }
        Self::menu_page_button_feed(&layout.page)
    }

    pub(super) fn handle_movie_list_mouse_move(state: &mut AppState) {
        let feed = Self::movie_list_feed(state);
        let (x, y) = state.match_state.input.cursor_px();
        state.frontend.shell_controller.on_pointer_move(x, y, &feed);
        state.frontend.shell_controller.drain_events();
        // Every hover message repaints the status line (0x00615EF7).
        state.frontend.shell_status_line.repaint();
    }

    /// The movie or Mods list's scrollbar arrow repeat
    /// (`0x0061D215..0x0061D2C8`); returns the next wake deadline.
    pub(super) fn poll_family_list(state: &mut AppState) -> Option<Instant> {
        if !Self::movie_list_active(state) && !Self::mods_page_active(state) {
            return None;
        }
        let controller = &mut state.frontend.shell_controller;
        let next = controller.poll();
        if !controller.drain_events().is_empty() {
            state.platform.window.request_redraw();
        }
        next
    }

    /// A family page's press: a fed button's GUIMainButtonSound, then the
    /// list's (0x0061A948). A single press selects the row under it (presses
    /// below the last row are ignored) and plays GenericClick; the scrollbar
    /// child (0x0061C690) captures its own presses without a sound, and the
    /// second press of a double-click selects nothing.
    pub(super) fn family_page_mouse_down(
        state: &mut AppState,
        feed: &[crate::ui::shell::layout::LaidOutControl],
    ) {
        let (x, y) = state.match_state.input.cursor_px();
        let controller = &mut state.frontend.shell_controller;
        controller.on_pointer_down(x, y, feed);
        if controller.pressed().is_some() {
            Self::play_page_cue(state, PageCue::Button);
        }
        let controller = &mut state.frontend.shell_controller;
        let events = controller.drain_events();
        for event in page::page_events(controller.gui(), events) {
            if let PageEvent::Cue(cue) = event {
                Self::play_page_cue(state, cue);
            }
        }
    }

    pub(super) fn handle_movie_list_mouse_down(state: &mut AppState) {
        let feed = Self::movie_list_feed(state);
        Self::family_page_mouse_down(state, &feed);
    }

    pub(super) fn handle_movie_list_mouse_up(state: &mut AppState) {
        let feed = Self::movie_list_feed(state);
        let (x, y) = state.match_state.input.cursor_px();
        use crate::app::frontend::shell_transition::ShellExitThen;
        let controller = &mut state.frontend.shell_controller;
        let released = controller.on_pointer_up(x, y, &feed);
        controller.drain_events();
        match released {
            Some(crate::ui::movies_credits_shell::PLAY_MOVIE_CONTROL) => {
                if family_list_selected(controller, MOVIE_LIST_DIALOG).is_some() {
                    Self::leave_shell_dialog(state, ShellExitThen::PlayMovie);
                } else {
                    // No selection: the proc stores it and keeps the dialog.
                    Self::play_selected_movie(state);
                }
            }
            // Back writes -1: state 4 recreates Movies & Credits.
            Some(0x0686) => Self::leave_shell_dialog(state, ShellExitThen::MovieListBack),
            _ => {}
        }
    }

    pub(super) fn play_generic_click_sound(state: &mut AppState) {
        Self::play_page_cue(state, PageCue::ListClick);
    }

    /// Play Movie (`0x0052D960..0x0052D98D`): the current selection is saved
    /// to `DAT_00825C80`; no selection plays nothing and keeps the dialog.
    pub(super) fn play_selected_movie(state: &mut AppState) {
        let Some(rows) = state.frontend.movie_list.as_ref() else {
            return;
        };
        let selected = family_list_selected(&state.frontend.shell_controller, MOVIE_LIST_DIALOG);
        state.frontend.movie_list_selection = selected.map_or(-1, |row| row as i32);
        let Some(entry) = selected.and_then(|row| rows.get(row).copied()) else {
            return;
        };
        // State 0xE's CD check (0x004790E0 with entry.cd) passes for a
        // complete install: every movie is read from the installed MIX set.
        Self::start_fullscreen_movie(state, entry.file, MovieReturn::MovieList);
    }

    /// Play_Movie `0x005BED40` from the shell.
    pub(super) fn start_fullscreen_movie(state: &mut AppState, name: &str, return_to: MovieReturn) {
        let running_scenario = matches!(&return_to, MovieReturn::RunningScenario);
        // The calling dialog is destroyed before the movie state runs.
        Self::destroy_current_shell_dialog(state);
        state.frontend.movie_list = None;
        let stem = movie_stem(name);
        // 0x005C0640 raises campaign movie progress before the existence check.
        state
            .persistence
            .options_profile
            .movie_progress
            .note_played(stem);
        // 0x005BED61..0x005BED6A: outside a campaign (GameMode != 0) Play_Movie
        // returns right after that note, with no video, audio hold or suspend,
        // so a skirmish or multiplayer map's movie action only moves on.
        if running_scenario
            && state
                .match_state
                .sim_runtime
                .runtime()
                .is_some_and(|runtime| runtime.simulation.session().game_mode_nonzero())
        {
            return;
        }
        let file_name = format!("{stem}.BIK");
        let resolved = state.process_assets.manager().and_then(|assets| {
            assets
                .get_with_source_ref(&file_name)
                .map(|(bytes, source)| (Arc::<[u8]>::from(bytes), source.to_string()))
        });
        let Some((bytes, source)) = resolved else {
            // No .BIK (and no VQA decoder is needed: retail YR ships none):
            // Play_Movie returns with no visible or audible change.
            log::info!("Play_Movie: {file_name} is not installed");
            Self::return_from_fullscreen_movie(state, return_to);
            return;
        };
        let surface = match crate::render::bink_movie::BinkMovieSurface::from_bytes(
            &state.renderer.gpu,
            &state.renderer.batch_renderer,
            bytes,
            source,
            false,
            // Fitted windows scale the frame (fullscreen_movie::movie_rect);
            // at the native 1:1 size linear sampling returns exact texels.
            wgpu::FilterMode::Linear,
        ) {
            Ok(surface) => surface,
            Err(err) => {
                log::warn!("Bink Error: {file_name}: {err:#}");
                Self::return_from_fullscreen_movie(state, return_to);
                return;
            }
        };
        // 5BEDFB..5BEE38 enters the same ABF35C movie hold as radar 5BF580
        // (Vox/streams pause, both masters glide to 0x2000), then 5BEE3E
        // suspends the sound events, which the radar movie does not. The
        // separate menu pause owner must not acquire another movie credit.
        let wall_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
        state.audio.set_movie_audio_held(true, wall_ms);
        state.audio.set_movie_sounds_suspended(true, wall_ms);
        // The Bink soundtrack plays at VoiceVolume: BinkSetVolume(ftol(
        // VoiceVolume * 32768.0)) at `0x00432897` (32768 = unity gain).
        let voice_volume = state
            .persistence
            .options_profile
            .voice_volume
            .clamp(0.0, 1.0);
        // 0x006C99D0: the movie's cues, when it has any.
        let header = &surface.file().header;
        let subtitles =
            crate::app::movie_subtitles::cues_for_movie(state.process_assets.manager(), stem)
                .and_then(|cues| {
                    crate::app::movie_subtitles::SubtitleTrack::new(
                        cues,
                        header.fps_num,
                        header.fps_den,
                    )
                });
        let movie = FullscreenMovie::new(
            surface,
            // BinkMovie_Open 0x0043279F selects the shared sound device.
            state.audio.movie_mixer(),
            voice_volume,
            subtitles,
            return_to,
            crate::app::frontend::fullscreen_movie::playback_active(state),
        );
        state.frontend.fullscreen_movie = Some(movie);
        if running_scenario {
            state.frontend.screen = crate::ui::game_screen::GameScreen::MainMenu;
        }
        state.platform.window.request_redraw();
    }

    /// End of Play_Movie: resume the paused music and recreate the caller's
    /// dialog (state 4 for Sneak Peeks, state `0xE` for the list). Release
    ///5BEE69..5BEE9E's shared movie hold; a separate menu pause stays held.
    pub(super) fn finish_fullscreen_movie(state: &mut AppState) {
        let Some(movie) = state.frontend.fullscreen_movie.take() else {
            return;
        };
        let return_to = movie.into_return();
        let wall_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
        state.audio.retire_movie_audio(wall_ms);
        Self::return_from_fullscreen_movie(state, return_to);
    }

    /// Per-frame Play_Movie service: step video and soundtrack, then finish
    /// once the last frame or an abort is reached.
    pub(super) fn advance_fullscreen_movie(state: &mut AppState) -> anyhow::Result<()> {
        let Some(movie) = state.frontend.fullscreen_movie.as_mut() else {
            return Ok(());
        };
        let voice_volume = state
            .persistence
            .options_profile
            .voice_volume
            .clamp(0.0, 1.0);
        let ended = match movie.step(&state.renderer.gpu, voice_volume) {
            Ok(ended) => ended,
            Err(err) => {
                log::warn!("Bink Error during playback: {err:#}");
                true
            }
        };
        if ended {
            Self::finish_fullscreen_movie(state);
        } else {
            state.platform.window.request_redraw();
        }
        Ok(())
    }

    /// Per-frame Show_Credits service on the native 16 ms bucket schedule.
    pub(super) fn advance_credits_roll(state: &mut AppState) {
        let now_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
        let Some(session) = state.frontend.credits_roll.as_mut() else {
            return;
        };
        session
            .roll
            .advance(crate::app::frontend::credits_roll::wall_bucket(now_ms));
        if session.roll.finished() {
            Self::finish_credits_roll(state);
        } else {
            state.platform.window.request_redraw();
        }
    }

    fn return_from_fullscreen_movie(state: &mut AppState, return_to: MovieReturn) {
        match return_to {
            // The frame loop presents the splash, then starts the pending
            // intro, then paints the shell (`App::render_frame`).
            MovieReturn::StartupLogo | MovieReturn::StartupIntro => {
                state.platform.window.request_redraw();
            }
            MovieReturn::MoviesAndCredits => Self::open_movies_credits_page(state),
            MovieReturn::MovieList => Self::open_movie_list(state),
            MovieReturn::CampaignStart(request) => {
                crate::app::loading::pump::begin_loading(state, *request);
            }
            MovieReturn::CampaignLoaded(result) => {
                crate::app::loading::transitions::apply_map_load_result(state, *result);
            }
            MovieReturn::RunningScenario => {
                state.frontend.screen = crate::ui::game_screen::GameScreen::InGame;
                state.platform.window.request_redraw();
            }
            MovieReturn::CampaignResult(destination) => {
                Self::complete_scenario_exit(state, *destination);
            }
            MovieReturn::CampaignVictory(victory) => {
                Self::advance_campaign_victory(state, *victory);
            }
            MovieReturn::CampaignDefeat(defeat) => {
                Self::open_campaign_replay_prompt(state, *defeat);
            }
        }
    }

    /// Only a bare Escape release (`Keyboard::Get() == 0x081B`) aborts a
    /// movie; every other key and mouse event is consumed and ignored.
    pub(super) fn fullscreen_movie_key(
        state: &mut AppState,
        code: winit::keyboard::KeyCode,
        pressed: bool,
    ) {
        let modifiers = state.platform.live_modifiers;
        let bare = !(modifiers.shift_key() || modifiers.control_key() || modifiers.alt_key());
        if code == winit::keyboard::KeyCode::Escape
            && !pressed
            && bare
            && let Some(movie) = state.frontend.fullscreen_movie.as_mut()
        {
            movie.abort();
            state.platform.window.request_redraw();
        }
    }

    /// State `0xF`, or a won campaign's EndOfGame (`0x006859EE`):
    /// Show_Credits `0x004C3E30`.
    pub(super) fn start_credits_roll(state: &mut AppState, return_to: CreditsReturn) {
        Self::destroy_current_shell_dialog(state);
        let bytes = state
            .process_assets
            .manager()
            .and_then(|assets| assets.get_ref(CREDITS_FILE).map(<[u8]>::to_vec));
        let Some(bytes) = bytes else {
            log::info!("Show_Credits: {CREDITS_FILE} is not installed");
            Self::finish_credits_roll_return(state, return_to);
            return;
        };
        let lookup = |key: &str| Self::csf_label(state, key, &format!("MISSING:'{key}'"));
        let viewport = CreditsViewport::new(
            state.renderer.gpu.config.width,
            state.renderer.gpu.config.height,
        );
        let lines = crate::ui::movies_credits_shell::parse_credits(
            &bytes,
            viewport.surface_height,
            &lookup,
        );
        let now_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
        // ThemeClass::Stop(0), then a silent ScoreVolume is lifted to 0.4 for
        // the CREDITS theme; the saved value is restored afterwards.
        state.audio.stop_theme();
        let saved_score_volume = state.persistence.options_profile.score_volume;
        if saved_score_volume == 0.0 {
            crate::app::persistence::options::audio::apply_volume(
                state,
                crate::app::persistence::options::audio::AudioVolume::Score,
                CREDITS_SILENT_SCORE_VOLUME,
                false,
            );
        }
        state
            .audio
            .queue_theme(&ThemeRequest::Name(CREDITS_THEME.into()), now_ms);
        let roll = CreditsRoll::new(
            lines,
            &state.renderer.bit_font,
            crate::app::frontend::credits_roll::wall_bucket(now_ms),
        );
        state.frontend.credits_roll = Some(CreditsRollSession {
            roll,
            saved_score_volume,
            last_drawn: Vec::new(),
            return_to,
        });
        state.platform.window.request_redraw();
    }

    /// Queue one keyboard/mouse event for the credits loop.
    pub(super) fn credits_roll_input(state: &mut AppState, input: CreditsInput) {
        if let Some(session) = state.frontend.credits_roll.as_mut() {
            session.roll.push_input(input);
            state.platform.window.request_redraw();
        }
    }

    /// Show_Credits tail: Stop(0), restore ScoreVolume, then
    /// `Main__PrepareSession` queues INTRO and recreates `0x101`, or the main
    /// menu after a campaign's end.
    pub(super) fn finish_credits_roll(state: &mut AppState) {
        let Some(session) = state.frontend.credits_roll.take() else {
            return;
        };
        state.audio.stop_theme();
        crate::app::persistence::options::audio::apply_volume(
            state,
            crate::app::persistence::options::audio::AudioVolume::Score,
            session.saved_score_volume,
            false,
        );
        Self::finish_credits_roll_return(state, session.return_to);
    }

    fn finish_credits_roll_return(state: &mut AppState, return_to: CreditsReturn) {
        let now_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
        state
            .audio
            .queue_theme(&ThemeRequest::Name("INTRO".into()), now_ms);
        match return_to {
            CreditsReturn::MoviesAndCredits => Self::open_movies_credits_page(state),
            CreditsReturn::MainMenu => {
                Self::destroy_current_shell_dialog(state);
                state.frontend.shell_route = ShellRoute::MainMenu;
                Self::resume_shell_after_match(state);
            }
        }
    }
}

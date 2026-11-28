//! Movies & Credits route: dialog `0x101`, movie list `0x129`, Play_Movie and
//! Show_Credits, following the `Main__PrepareSession` `0x0052D9A0` states
//! 4, `0xD`, `0xE` and `0xF`. Each child destroys its source dialog first and
//! recreates its return dialog afterwards.

use crate::sim::trigger_runtime::ThemeRequest;
use crate::util::time::Instant;
use std::sync::Arc;

use super::{App, AppState};
use crate::app::audio_runtime::AppAudioRuntime;
use crate::app::frontend::credits_roll::{
    CreditsInput, CreditsReturn, CreditsRoll, CreditsRollSession, CreditsViewport,
};
use crate::app::frontend::fullscreen_movie::{FullscreenMovie, MovieReturn, movie_stem};
use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::options_profile::RetailOptionsProfile;
use crate::app::process_assets::ProcessAssets;
use crate::app::shell_main_menu::ClientStep;
use crate::app::shell_route::ShellRoute;
use crate::audio::sfx::SfxPlayer;
use crate::render::gpu::GpuContext;
use crate::ui::movies_credits_shell::{
    MOVIE_LIST_DIALOG, MOVIES_CREDITS_PAGE, MoviesCreditsAction, active_movie_entries,
    family_list_node, family_list_selected, retained_movie_row,
};
use crate::ui::shell::controller::DialogController;
use winit::keyboard::ModifiersState;
use winit::window::Window;

/// The Sneak Peeks movie (`0x0052DE55`).
const SNEAK_PEEK_MOVIE: &str = "RENEGADE.BIK";
const CREDITS_FILE: &str = "CREDITSMD.TXT";
const CREDITS_THEME: &str = "CREDITS";
/// Set_Score_Volume(0.4f) at `0x004C42C3` when ScoreVolume is exactly 0.
const CREDITS_SILENT_SCORE_VOLUME: f32 = 0.4;

impl App {
    /// The shell dialog being left is destroyed (child `0x71A` with it)
    /// before the next dialog or presentation is created.
    pub(super) fn destroy_current_shell_dialog(frontend: &mut FrontendState) {
        crate::app::frontend::main_menu_shell_render::clear_ra2ts_movie_session(frontend);
        crate::app::frontend::shell_transition::invalidate_main_menu_dialog_instance(frontend);
    }

    /// `route` shows with `dialog` the shell's sole dialog.
    pub(super) fn show_shell_page(
        frontend: &mut FrontendState,
        route: ShellRoute,
        dialog: crate::ui::shell::descriptor::DialogResourceId,
    ) {
        frontend.shell_route = route;
        frontend.shell_controller.reset_to(dialog);
    }

    /// State 4: create Movies & Credits `0x101`.
    pub(super) fn open_movies_credits_page(frontend: &mut FrontendState) {
        Self::destroy_current_shell_dialog(frontend);
        frontend.movie_list = None;
        let dialog = MOVIES_CREDITS_PAGE.dialog;
        Self::show_shell_page(frontend, ShellRoute::MoviesAndCredits, dialog);
    }

    /// The result of Movies & Credits `0x101`. Sneak Peeks (state `0xD`) and
    /// View Credits (state `0xF`) return the step that starts them.
    #[must_use]
    pub(super) fn handle_movies_credits_action(
        gpu: &GpuContext,
        frontend: &mut FrontendState,
        options_profile: &RetailOptionsProfile,
        action: MoviesCreditsAction,
    ) -> Option<ClientStep> {
        match action {
            MoviesCreditsAction::SneakPeeks => {
                Some(ClientStep::Now(Box::new(|state: &mut AppState| {
                    Self::start_fullscreen_movie(
                        state,
                        SNEAK_PEEK_MOVIE,
                        MovieReturn::MoviesAndCredits,
                    )
                })))
            }
            MoviesCreditsAction::PlayMovies => {
                Self::open_movie_list(gpu, frontend, options_profile);
                None
            }
            MoviesCreditsAction::ViewCredits => {
                Some(ClientStep::Now(Box::new(|state: &mut AppState| {
                    Self::start_credits_roll(state, CreditsReturn::MoviesAndCredits)
                })))
            }
            MoviesCreditsAction::MainMenu => {
                Self::destroy_current_shell_dialog(frontend);
                frontend.shell_route = ShellRoute::MainMenu;
                None
            }
        }
    }

    /// State `0xE`: create movie list `0x129` with the rows `0x005FC000` adds
    /// and the retained selection `DAT_00825C80`.
    pub(super) fn open_movie_list(
        gpu: &GpuContext,
        frontend: &mut FrontendState,
        options_profile: &RetailOptionsProfile,
    ) {
        Self::destroy_current_shell_dialog(frontend);
        let rows = active_movie_entries(options_profile.movie_progress);
        frontend.movie_list = Some(rows);
        Self::show_shell_page(frontend, ShellRoute::MovieList, MOVIE_LIST_DIALOG);
        Self::movie_list_sync(gpu, frontend);
    }

    /// Binds the controller to the page and keeps its list `0x744`, the
    /// dialog's `List` node: a hover target for status help and its own
    /// press handler, never a button press.
    fn movie_list_sync(
        gpu: &GpuContext,
        frontend: &mut FrontendState,
    ) -> crate::ui::movies_credits_shell::MovieListLayout {
        let layout = crate::app::frontend::shell_layout::movie_list_layout(gpu);
        let retained = frontend.movie_list_selection;
        let controller = &mut frontend.shell_controller;
        controller.ensure_active(MOVIE_LIST_DIALOG);
        controller.set_scale(layout.page.scale);
        if let Some(rows) = frontend.movie_list.as_ref() {
            // The buttons and the prompt first: the list's id counts as built.
            if let Some(root) = controller.root_in(MOVIE_LIST_DIALOG) {
                let nodes = crate::ui::movies_credits_shell::movie_list_nodes;
                crate::ui::shell::page::ensure_built(controller.gui_mut(), root, nodes);
            }
            let list = (rows.len(), retained_movie_row(rows, retained), 0);
            if let Some(node) = family_list_node(controller, MOVIE_LIST_DIALOG, layout.list, list) {
                // Its rows draw top-left (`Shell__DrawTextAligned`, flags 0).
                let gui = controller.gui_mut();
                let id = crate::ui::movies_credits_shell::FAMILY_LIST_ID;
                let _ = gui.set(node, vera20k_gui::Prop::Id, vera20k_gui::Value::Int(id));
                let _ = gui.set(node, vera20k_gui::Prop::Align, vera20k_gui::Value::Int(0));
            }
        }
        layout
    }

    /// The movie or Mods list's scrollbar arrow repeat
    /// (`0x0061D215..0x0061D2C8`) while its page shows; returns the next
    /// wake deadline.
    pub(super) fn poll_family_list(
        window: &Window,
        controller: &mut DialogController,
    ) -> Option<Instant> {
        (*controller).poll_then(|| window.request_redraw())
    }

    /// One pointer message (`down`: `None` moves). A press on a button
    /// plays GUIMainButtonSound, one in the list (0x0061A948) selects the
    /// row under it (presses below the last row are ignored) and plays
    /// GenericClick; the scrollbar child (0x0061C690) captures its own
    /// presses without a sound, and the second press of a double-click
    /// selects nothing. Every hover message repaints the status line
    /// (0x00615EF7). Play and Back return their leave as the step.
    #[must_use]
    pub(super) fn handle_movie_list_mouse(
        gpu: &GpuContext,
        frontend: &mut FrontendState,
        match_state: &MatchState,
        process_assets: &ProcessAssets,
        sfx_player: &mut Option<SfxPlayer>,
        down: Option<bool>,
    ) -> Option<ClientStep> {
        use crate::app::frontend::shell_transition::ShellSlideKind::MovieList;
        let layout = Self::movie_list_sync(gpu, frontend);
        let rect_of = |id| layout.rect_of(id);
        let nodes = crate::ui::movies_credits_shell::movie_list_nodes;
        match Self::screen_page_pointer(
            frontend,
            match_state,
            process_assets,
            sfx_player,
            MOVIE_LIST_DIALOG,
            nodes,
            rect_of,
            &[],
            down,
        ) {
            Some(crate::ui::movies_credits_shell::PLAY_MOVIE_CONTROL) => {
                if family_list_selected(&frontend.shell_controller, MOVIE_LIST_DIALOG).is_some() {
                    Some(ClientStep::Leave(
                        MovieList,
                        Box::new(|state: &mut AppState| {
                            if let Some(step) = App::play_selected_movie(&mut state.frontend) {
                                App::apply_step(state, step);
                            }
                        }),
                    ))
                } else {
                    // No selection: the proc stores it and keeps the dialog.
                    Self::play_selected_movie(frontend)
                }
            }
            // Back writes -1: state 4 recreates Movies & Credits.
            Some(0x0686) => Some(ClientStep::Leave(
                MovieList,
                Box::new(|state: &mut AppState| App::open_movies_credits_page(&mut state.frontend)),
            )),
            _ => None,
        }
    }

    /// Play Movie (`0x0052D960..0x0052D98D`): the current selection is saved
    /// to `DAT_00825C80`; no selection plays nothing and keeps the dialog.
    /// The movie's start is the returned step.
    #[must_use]
    pub(super) fn play_selected_movie(frontend: &mut FrontendState) -> Option<ClientStep> {
        let rows = frontend.movie_list.as_ref()?;
        let selected = family_list_selected(&frontend.shell_controller, MOVIE_LIST_DIALOG);
        frontend.movie_list_selection = selected.map_or(-1, |row| row as i32);
        let entry = selected.and_then(|row| rows.get(row).copied())?;
        // State 0xE's CD check (0x004790E0 with entry.cd) passes for a
        // complete install: every movie is read from the installed MIX set.
        Some(ClientStep::Now(Box::new(move |state: &mut AppState| {
            Self::start_fullscreen_movie(state, entry.file, MovieReturn::MovieList)
        })))
    }

    /// Play_Movie `0x005BED40` from the shell.
    pub(super) fn start_fullscreen_movie(state: &mut AppState, name: &str, return_to: MovieReturn) {
        let running_scenario = matches!(&return_to, MovieReturn::RunningScenario);
        // The calling dialog is destroyed before the movie state runs.
        Self::destroy_current_shell_dialog(&mut state.frontend);
        state.frontend.movie_list = None;
        let stem = movie_stem(name);
        // 0x005C0640 raises campaign movie progress before the existence check.
        state
            .persistence
            .options_profile
            .movie_progress
            .note_played(stem);
        // 0x005BED61..0x005BED6A: outside a campaign world Play_Movie
        // returns right after that note, with no video, audio hold or suspend,
        // so a skirmish or multiplayer map's movie action only moves on.
        if running_scenario
            && state
                .match_state
                .sim_runtime
                .runtime()
                .is_some_and(|runtime| !runtime.simulation.session().uses_campaign_world())
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
        let voice_volume = (state.persistence.options_profile.voice_volume).clamp(0.0, 1.0);
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
        let activity = return_to.activity(&state.platform, state.match_state.session_mode());
        let movie = FullscreenMovie::new(
            surface,
            // BinkMovie_Open 0x0043279F selects the shared sound device.
            state.audio.movie_mixer(),
            voice_volume,
            subtitles,
            return_to,
            activity,
        );
        state.frontend.fullscreen_movie = Some(movie);
        if running_scenario {
            state.frontend.screen = crate::ui::game_screen::GameScreen::MainMenu;
        }
    }

    /// End of Play_Movie: resume the paused music and recreate the caller's
    /// dialog (state 4 for Sneak Peeks, state `0xE` for the list). Release
    ///5BEE69..5BEE9E's shared movie hold; a separate menu pause stays held.
    /// The return to the caller is the returned step.
    #[must_use]
    pub(super) fn finish_fullscreen_movie(
        fullscreen_movie: &mut Option<FullscreenMovie>,
        audio: &mut AppAudioRuntime,
    ) -> Option<ClientStep> {
        let movie = fullscreen_movie.take()?;
        let return_to = movie.into_return();
        let wall_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
        audio.retire_movie_audio(wall_ms);
        Some(ClientStep::Now(Box::new(move |state: &mut AppState| {
            Self::return_from_fullscreen_movie(state, return_to)
        })))
    }

    /// Per-frame Play_Movie service: step video and soundtrack, then finish
    /// once the last frame or an abort is reached; the finish's step is
    /// returned.
    #[must_use]
    pub(super) fn advance_fullscreen_movie(
        gpu: &GpuContext,
        fullscreen_movie: &mut Option<FullscreenMovie>,
        audio: &mut AppAudioRuntime,
        options_profile: &RetailOptionsProfile,
    ) -> Option<ClientStep> {
        let movie = fullscreen_movie.as_mut()?;
        let voice_volume = (options_profile.voice_volume).clamp(0.0, 1.0);
        let ended = match movie.step(gpu, voice_volume) {
            Ok(ended) => ended,
            Err(err) => {
                log::warn!("Bink Error during playback: {err:#}");
                true
            }
        };
        if ended {
            return Self::finish_fullscreen_movie(fullscreen_movie, audio);
        }
        None
    }

    /// Per-frame Show_Credits service on the native 16 ms bucket schedule;
    /// the finish's step is returned.
    #[must_use]
    pub(super) fn advance_credits_roll(
        frontend: &mut FrontendState,
        match_state: &MatchState,
        process_assets: &ProcessAssets,
        audio: &mut AppAudioRuntime,
        options_profile: &mut RetailOptionsProfile,
    ) -> Option<ClientStep> {
        let now_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
        let session = frontend.credits_roll.as_mut()?;
        session
            .roll
            .advance(crate::app::frontend::credits_roll::wall_bucket(now_ms));
        if session.roll.finished() {
            return Self::finish_credits_roll(
                frontend,
                match_state,
                process_assets,
                audio,
                options_profile,
            );
        }
        None
    }

    fn return_from_fullscreen_movie(state: &mut AppState, return_to: MovieReturn) {
        match return_to {
            // The frame loop presents the splash, then starts the pending
            // intro, then paints the shell (`App::render_frame`).
            MovieReturn::StartupLogo | MovieReturn::StartupIntro => {
                state.platform.window.request_redraw();
            }
            MovieReturn::MoviesAndCredits => Self::open_movies_credits_page(&mut state.frontend),
            MovieReturn::MovieList => Self::open_movie_list(
                &state.renderer.gpu,
                &mut state.frontend,
                &state.persistence.options_profile,
            ),
            MovieReturn::CampaignStart(request) => {
                crate::app::loading::pump::begin_loading(state, *request)
            }
            MovieReturn::CampaignLoaded(result) => {
                crate::app::loading::transitions::apply_map_load_result(state, *result)
            }
            MovieReturn::RunningScenario => {
                state.frontend.screen = crate::ui::game_screen::GameScreen::InGame;
                state.platform.window.request_redraw();
            }
            MovieReturn::CampaignResult(destination) => {
                Self::complete_scenario_exit(state, *destination)
            }
            MovieReturn::CampaignVictory(victory) => {
                Self::advance_campaign_victory(state, *victory)
            }
            MovieReturn::CampaignDefeat(defeat) => Self::open_campaign_replay_prompt(
                &state.platform.window,
                &state.renderer,
                &mut state.frontend,
                &state.process_assets,
                &state.persistence.options_profile,
                *defeat,
            ),
        }
    }

    /// Only a bare Escape release (`Keyboard::Get() == 0x081B`) aborts a
    /// movie; every other key and mouse event is consumed and ignored.
    pub(super) fn fullscreen_movie_key(
        live_modifiers: ModifiersState,
        fullscreen_movie: &mut Option<FullscreenMovie>,
        code: winit::keyboard::KeyCode,
        pressed: bool,
    ) {
        let modifiers = live_modifiers;
        let bare = !(modifiers.shift_key() || modifiers.control_key() || modifiers.alt_key());
        if code == winit::keyboard::KeyCode::Escape
            && !pressed
            && bare
            && let Some(movie) = fullscreen_movie.as_mut()
        {
            movie.abort();
        }
    }

    /// State `0xF`, or a won campaign's EndOfGame (`0x006859EE`):
    /// Show_Credits `0x004C3E30`.
    pub(super) fn start_credits_roll(state: &mut AppState, return_to: CreditsReturn) {
        Self::destroy_current_shell_dialog(&mut state.frontend);
        let bytes = state
            .process_assets
            .manager()
            .and_then(|assets| assets.get_ref(CREDITS_FILE).map(<[u8]>::to_vec));
        let Some(bytes) = bytes else {
            log::info!("Show_Credits: {CREDITS_FILE} is not installed");
            Self::finish_credits_roll_return(state, return_to);
            return;
        };
        let lookup = |key: &str| {
            state
                .process_assets
                .csf_label(key, &format!("MISSING:'{key}'"))
        };
        let config = &state.renderer.gpu.config;
        let viewport = CreditsViewport::new(config.width, config.height);
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
                &state.frontend,
                &state.match_state,
                &state.process_assets,
                &mut state.audio,
                &mut state.persistence.options_profile,
                crate::app::persistence::options::audio::AudioVolume::Score,
                CREDITS_SILENT_SCORE_VOLUME,
                false,
            );
        }
        (state.audio).queue_theme(&ThemeRequest::Name(CREDITS_THEME.into()), now_ms);
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
    }

    /// Queue one keyboard/mouse event for the credits loop.
    pub(super) fn credits_roll_input(
        credits_roll: &mut Option<CreditsRollSession>,
        input: CreditsInput,
    ) {
        if let Some(session) = credits_roll.as_mut() {
            session.roll.push_input(input);
        }
    }

    /// Show_Credits tail: Stop(0), restore ScoreVolume, then
    /// `Main__PrepareSession` queues INTRO and recreates `0x101`, or the main
    /// menu after a campaign's end. That return is the returned step.
    #[must_use]
    pub(super) fn finish_credits_roll(
        frontend: &mut FrontendState,
        match_state: &MatchState,
        process_assets: &ProcessAssets,
        audio: &mut AppAudioRuntime,
        options_profile: &mut RetailOptionsProfile,
    ) -> Option<ClientStep> {
        let session = frontend.credits_roll.take()?;
        audio.stop_theme();
        crate::app::persistence::options::audio::apply_volume(
            frontend,
            match_state,
            process_assets,
            audio,
            options_profile,
            crate::app::persistence::options::audio::AudioVolume::Score,
            session.saved_score_volume,
            false,
        );
        let return_to = session.return_to;
        Some(ClientStep::Now(Box::new(move |state: &mut AppState| {
            Self::finish_credits_roll_return(state, return_to)
        })))
    }

    fn finish_credits_roll_return(state: &mut AppState, return_to: CreditsReturn) {
        let now_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
        (state.audio).queue_theme(&ThemeRequest::Name("INTRO".into()), now_ms);
        match return_to {
            CreditsReturn::MoviesAndCredits => Self::open_movies_credits_page(&mut state.frontend),
            CreditsReturn::MainMenu => {
                Self::destroy_current_shell_dialog(&mut state.frontend);
                state.frontend.shell_route = ShellRoute::MainMenu;
                Self::resume_shell_after_match(state);
            }
        }
    }
}

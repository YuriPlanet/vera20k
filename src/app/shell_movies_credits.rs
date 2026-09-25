//! Movies & Credits route: dialog `0x101`, movie list `0x129`, Play_Movie and
//! Show_Credits, following the `Main__PrepareSession` `0x0052D9A0` states
//! 4, `0xD`, `0xE` and `0xF`. Each child destroys its source dialog first and
//! recreates its return dialog afterwards.

use std::sync::Arc;
use std::time::Instant;

use super::{App, AppState};
use crate::app::frontend::credits_roll::{CreditsInput, CreditsRoll, CreditsRollSession};
use crate::app::frontend::fullscreen_movie::{FullscreenMovie, MovieReturn, movie_stem};
use crate::app::shell_route::ShellRoute;
use crate::ui::movies_credits_shell::{
    MOVIE_LIST_DIALOG, MOVIES_CREDITS_PAGE, MovieListState, MoviesCreditsAction,
};

/// The Sneak Peeks movie (`0x0052DE55`).
const SNEAK_PEEK_MOVIE: &str = "RENEGADE.BIK";
const CREDITS_FILE: &str = "CREDITSMD.TXT";
const CREDITS_THEME: &str = "CREDITS";
/// Set_Score_Volume(0.4f) at `0x004C42C3` when ScoreVolume is exactly 0.
const CREDITS_SILENT_SCORE_VOLUME: f32 = 0.4;

impl App {
    /// The shell dialog being left is destroyed (child `0x71A` with it)
    /// before the next dialog or presentation is created.
    fn destroy_current_shell_dialog(state: &mut AppState) {
        crate::app::frontend::main_menu_shell_render::clear_ra2ts_movie_session(state);
        crate::app::frontend::shell_transition::invalidate_main_menu_dialog_instance(state);
    }

    /// State 4: create Movies & Credits `0x101`.
    pub(super) fn open_movies_credits_page(state: &mut AppState) {
        Self::enter_shell_window_mode(state);
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
            MoviesCreditsAction::ViewCredits => Self::start_credits_roll(state),
            MoviesCreditsAction::MainMenu => {
                Self::destroy_current_shell_dialog(state);
                state.frontend.shell_route = ShellRoute::MainMenu;
            }
        }
    }

    /// State `0xE`: create movie list `0x129` with the rows `0x005FC000` adds
    /// and the retained selection `DAT_00825C80`.
    pub(super) fn open_movie_list(state: &mut AppState) {
        Self::enter_shell_window_mode(state);
        Self::destroy_current_shell_dialog(state);
        state.frontend.movie_list = Some(MovieListState::open(
            state.persistence.options_profile.movie_progress,
            state.frontend.movie_list_selection,
        ));
        state.frontend.shell_route = ShellRoute::MovieList;
        state
            .frontend
            .shell_controller
            .reset_to(MOVIE_LIST_DIALOG, false);
    }

    pub(super) fn movie_list_active(state: &AppState) -> bool {
        state.frontend.screen == crate::ui::game_screen::GameScreen::MainMenu
            && state.frontend.shell_route.movie_list()
            && state.frontend.movie_list.is_some()
    }

    fn movie_list_layout(state: &AppState) -> crate::ui::movies_credits_shell::MovieListLayout {
        crate::ui::movies_credits_shell::compute_movie_list_layout(
            state.renderer.gpu.config.width,
            state.renderer.gpu.config.height,
        )
    }

    fn movie_list_feed(state: &mut AppState) -> Vec<crate::ui::shell::layout::LaidOutControl> {
        let layout = Self::movie_list_layout(state);
        state
            .frontend
            .shell_controller
            .ensure_active(MOVIE_LIST_DIALOG, false);
        let mut feed = Self::menu_page_button_feed(&layout.page);
        // The list is a hover target for status help, never an owner-draw
        // button press: it is appended after the buttons and filtered out of
        // press handling below.
        feed.push(crate::ui::shell::layout::LaidOutControl {
            id: crate::ui::movies_credits_shell::MOVIE_LIST_CONTROL,
            rect: layout.list,
        });
        feed
    }

    pub(super) fn handle_movie_list_mouse_move(state: &mut AppState) {
        let feed = Self::movie_list_feed(state);
        let (x, y) = Self::shell_cursor(state);
        state.frontend.shell_controller.on_pointer_move(x, y, &feed);
        let layout = Self::movie_list_layout(state);
        if let Some(list) = state.frontend.movie_list.as_mut() {
            list.scroll_pointer_moved(layout.list, x, y);
        }
        // Every hover message repaints the status line (0x00615EF7).
        state.frontend.shell_status_line.repaint();
    }

    /// Scrollbar arrow auto-repeat; returns the next wake deadline.
    pub(super) fn poll_movie_list_scroll(state: &mut AppState) -> Option<Instant> {
        if !Self::movie_list_active(state) {
            return None;
        }
        let (x, y) = Self::shell_cursor(state);
        let layout = Self::movie_list_layout(state);
        let list = state.frontend.movie_list.as_mut()?;
        if list.scroll_poll(layout.list, x, y, Instant::now()) {
            state.platform.window.request_redraw();
        }
        list.scroll.repeat_at()
    }

    pub(super) fn handle_movie_list_mouse_down(state: &mut AppState) {
        let feed = Self::movie_list_feed(state);
        let (x, y) = Self::shell_cursor(state);
        let layout = Self::movie_list_layout(state);
        if let Some(list) = state.frontend.movie_list.as_mut()
            && layout.list.contains(x, y)
        {
            // The scrollbar child (0x0061C690) captures its own presses and
            // plays no sound.
            if list.scroll_press(layout.list, x, y, Instant::now()) {
                return;
            }
            let double_click = list.is_double_click(
                Instant::now(),
                x,
                y,
                crate::ui::shell::saved_file_input::host_double_click_limits(),
            );
            // 0x0061A948: a single press selects the row under it (presses
            // below the last row are ignored) and plays GenericClick.
            if !double_click && let Some(row) = list.row_at(layout.list, x, y) {
                list.selected = Some(row);
                Self::play_generic_click_sound(state);
            }
            return;
        }
        let buttons: Vec<_> = feed
            .into_iter()
            .filter(|control| control.id != crate::ui::movies_credits_shell::MOVIE_LIST_CONTROL)
            .collect();
        state
            .frontend
            .shell_controller
            .on_pointer_down(x, y, &buttons);
        if state.frontend.shell_controller.pressed().is_some() {
            Self::play_main_menu_button_sound(state);
        }
    }

    pub(super) fn handle_movie_list_mouse_up(state: &mut AppState) {
        if let Some(list) = state.frontend.movie_list.as_mut() {
            list.scroll.cancel();
        }
        let feed: Vec<_> = Self::movie_list_feed(state)
            .into_iter()
            .filter(|control| control.id != crate::ui::movies_credits_shell::MOVIE_LIST_CONTROL)
            .collect();
        let (x, y) = Self::shell_cursor(state);
        use crate::app::frontend::shell_transition::ShellExitThen;
        match state.frontend.shell_controller.on_pointer_up(x, y, &feed) {
            Some(crate::ui::movies_credits_shell::PLAY_MOVIE_CONTROL) => {
                let selected = state
                    .frontend
                    .movie_list
                    .as_ref()
                    .is_some_and(|list| list.selected.is_some());
                if selected {
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

    fn shell_cursor(state: &AppState) -> (i32, i32) {
        (
            state.match_state.input.cursor_x.round() as i32,
            state.match_state.input.cursor_y.round() as i32,
        )
    }

    pub(super) fn play_generic_click_sound(state: &mut AppState) {
        let sound_id = state
            .rules()
            .and_then(|rules| rules.general.generic_click_sound.as_deref())
            .map(str::to_owned);
        Self::play_shell_ui_sound_by_id(state, sound_id.as_deref());
    }

    /// Play Movie (`0x0052D960..0x0052D98D`): the current selection is saved
    /// to `DAT_00825C80`; no selection plays nothing and keeps the dialog.
    pub(super) fn play_selected_movie(state: &mut AppState) {
        let Some(list) = state.frontend.movie_list.as_ref() else {
            return;
        };
        state.frontend.movie_list_selection = list.selected.map_or(-1, |row| row as i32);
        let Some(entry) = list.selected.and_then(|row| list.rows.get(row).copied()) else {
            return;
        };
        // State 0xE's CD check (0x004790E0 with entry.cd) passes for a
        // complete install: every movie is read from the installed MIX set.
        Self::start_fullscreen_movie(state, entry.file, MovieReturn::MovieList);
    }

    /// Play_Movie `0x005BED40` from the shell.
    pub(super) fn start_fullscreen_movie(state: &mut AppState, name: &str, return_to: MovieReturn) {
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
        ) {
            Ok(surface) => surface,
            Err(err) => {
                log::warn!("Bink Error: {file_name}: {err:#}");
                Self::return_from_fullscreen_movie(state, return_to);
                return;
            }
        };
        let now_ms =
            crate::app::match_runtime::sim_tick::monotonic_frame_pacer_ms(state, Instant::now());
        // Stream players (the theme) and sound events pause, not stop.
        if let Some(music) = state.audio.music_player.as_mut() {
            music.pause_output();
        }
        if let Some(sfx) = state.audio.sfx_player.as_mut() {
            sfx.set_paused(true, now_ms);
        }
        // The Bink soundtrack plays at VoiceVolume: BinkSetVolume(ftol(
        // VoiceVolume * 32768.0)) at `0x00432897` (32768 = unity gain).
        let voice_volume = state
            .persistence
            .options_profile
            .voice_volume
            .clamp(0.0, 1.0);
        let movie = FullscreenMovie::new(
            surface,
            state.audio.music_player.as_ref().map(|music| music.mixer()),
            voice_volume,
            return_to,
        );
        state.frontend.fullscreen_movie = Some(movie);
        state.platform.window.request_redraw();
    }

    /// End of Play_Movie: resume paused audio and recreate the caller's
    /// dialog (state 4 for Sneak Peeks, state `0xE` for the list).
    pub(super) fn finish_fullscreen_movie(state: &mut AppState) {
        let Some(movie) = state.frontend.fullscreen_movie.take() else {
            return;
        };
        let return_to = movie.return_to();
        drop(movie);
        let now_ms =
            crate::app::match_runtime::sim_tick::monotonic_frame_pacer_ms(state, Instant::now());
        if let Some(music) = state.audio.music_player.as_mut() {
            music.resume_output();
        }
        if let Some(sfx) = state.audio.sfx_player.as_mut() {
            sfx.set_paused(false, now_ms);
        }
        Self::return_from_fullscreen_movie(state, return_to);
    }

    /// Per-frame Play_Movie service: pause while inactive, step video and
    /// soundtrack, and finish once the last frame or an abort is reached.
    pub(super) fn advance_fullscreen_movie(state: &mut AppState) -> anyhow::Result<()> {
        let active = state.platform.window_active;
        let Some(movie) = state.frontend.fullscreen_movie.as_mut() else {
            return Ok(());
        };
        movie.set_active(active);
        let ended = match movie.step(&state.renderer.gpu) {
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
        let now_ms =
            crate::app::match_runtime::sim_tick::monotonic_frame_pacer_ms(state, Instant::now());
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
            MovieReturn::MoviesAndCredits => Self::open_movies_credits_page(state),
            MovieReturn::MovieList => Self::open_movie_list(state),
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

    /// State `0xF`: Show_Credits `0x004C3E30`.
    pub(super) fn start_credits_roll(state: &mut AppState) {
        Self::destroy_current_shell_dialog(state);
        let bytes = state
            .process_assets
            .manager()
            .and_then(|assets| assets.get_ref(CREDITS_FILE).map(<[u8]>::to_vec));
        let Some(bytes) = bytes else {
            log::info!("Show_Credits: {CREDITS_FILE} is not installed");
            Self::finish_credits_roll_return(state);
            return;
        };
        let lookup = |key: &str| Self::csf_label(state, key, &format!("MISSING:'{key}'"));
        let lines = crate::ui::movies_credits_shell::parse_credits(
            &bytes,
            state.renderer.gpu.config.height as i32,
            &lookup,
        );
        let now_ms =
            crate::app::match_runtime::sim_tick::monotonic_frame_pacer_ms(state, Instant::now());
        // ThemeClass::Stop(0), then a silent ScoreVolume is lifted to 0.4 for
        // the CREDITS theme; the saved value is restored afterwards.
        state.audio.stop_theme();
        let saved_score_volume = state.persistence.options_profile.score_volume;
        if saved_score_volume == 0.0 {
            Self::set_credits_score_volume(state, CREDITS_SILENT_SCORE_VOLUME);
        }
        state.audio.queue_theme(CREDITS_THEME, now_ms);
        let roll = CreditsRoll::new(
            lines,
            &state.renderer.bit_font,
            crate::app::frontend::credits_roll::wall_bucket(now_ms),
        );
        state.frontend.credits_roll = Some(CreditsRollSession {
            roll,
            saved_score_volume,
            last_drawn: Vec::new(),
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
    /// `Main__PrepareSession` queues INTRO and recreates `0x101`.
    pub(super) fn finish_credits_roll(state: &mut AppState) {
        let Some(session) = state.frontend.credits_roll.take() else {
            return;
        };
        state.audio.stop_theme();
        Self::set_credits_score_volume(state, session.saved_score_volume);
        Self::finish_credits_roll_return(state);
    }

    /// `OptionsClass::Set_Score_Volume(volume, 0)` @ `0x005FA4A0`: the
    /// profile field and the theme output both take the value.
    fn set_credits_score_volume(state: &mut AppState, volume: f32) {
        state.persistence.options_profile.score_volume = volume;
        if let Some(music) = state.audio.music_player.as_mut() {
            music.set_volume(f64::from(volume));
        }
    }

    fn finish_credits_roll_return(state: &mut AppState) {
        let now_ms =
            crate::app::match_runtime::sim_tick::monotonic_frame_pacer_ms(state, Instant::now());
        state.audio.queue_theme("INTRO", now_ms);
        Self::open_movies_credits_page(state);
    }
}

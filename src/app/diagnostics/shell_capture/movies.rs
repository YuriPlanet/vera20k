//! Production-only Movies & Credits capture routes. Route actions go through
//! the ordinary main-menu, `0x101` and `0x129` handlers after a presented
//! frame; no shell state or renderer is cloned.

use super::*;
use crate::app::App;
use serde_json::{Value, json};

/// Row-0 press point for the selected-list checkpoint (inside `0x744`).
const LIST_ROW0_POINT: (f32, f32) = (150.0, 137.0);
/// Down-arrow press point of the 17-row list's scrollbar.
const LIST_DOWN_ARROW_POINT: (f32, f32) = (509.0, 420.0);
/// Frames to present after the route settles, so reveals and pending state
/// changes show before the readback.
const SETTLE_FRAMES: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MoviesTarget {
    Page0x101,
    List0x129,
    List0x129Selected,
    /// Every campaign movie unlocked (diagnostic profile override), after
    /// `down_presses` down-arrow presses.
    FullList {
        down_presses: u8,
    },
    Credits {
        frame: u64,
    },
    SneakPeek {
        frame: usize,
    },
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Phase {
    #[default]
    MainMenu,
    Page,
    List,
    Credits,
    Movie,
    Settling(u32),
}

pub(super) struct MoviesCapture {
    target: MoviesTarget,
    phase: Phase,
    route: Vec<Value>,
}

impl MoviesCapture {
    pub(super) fn new(target: MoviesTarget) -> Self {
        Self {
            target,
            phase: Phase::MainMenu,
            route: Vec::new(),
        }
    }

    fn slide_settled(state: &AppState, kind: ShellSlideKind) -> bool {
        state.frontend.shell_first_paint_slide.is_none()
            && state.frontend.shell_slide_active_shell == Some(kind)
    }

    fn restore_neutral_pointer(state: &mut AppState) {
        state.match_state.input.cursor_x = EXPECTED_CURSOR_X as f32;
        state.match_state.input.cursor_y = EXPECTED_CURSOR_Y as f32;
    }

    pub(super) fn after_present(
        &mut self,
        state: &mut AppState,
        rendered: PresentedShell,
        frame: u32,
    ) -> Result<()> {
        ensure!(
            state.frontend.screen == GameScreen::MainMenu,
            "movies capture left the shell"
        );
        ensure!(
            !state.frontend.main_menu_shell_failed,
            "movies capture encountered the shell fallback"
        );
        match (self.phase, rendered) {
            (Phase::MainMenu, PresentedShell::MainMenu) => {
                if steady_main_menu_capture_ready(MainMenuCaptureSnapshot::from_state(state))? {
                    self.route.push(
                        json!({"dialog": 0xe2, "frame": frame, "action": "MoviesAndCredits"}),
                    );
                    App::handle_main_menu_shell_action(
                        state,
                        crate::ui::main_menu_shell::MainMenuShellAction::MoviesAndCredits,
                    );
                    self.phase = Phase::Page;
                }
            }
            (Phase::Page, PresentedShell::MoviesAndCredits) => {
                if Self::slide_settled(state, ShellSlideKind::MoviesAndCredits) {
                    let action = match self.target {
                        MoviesTarget::Page0x101 => {
                            self.phase = Phase::Settling(SETTLE_FRAMES);
                            return Ok(());
                        }
                        MoviesTarget::List0x129 | MoviesTarget::List0x129Selected => {
                            self.phase = Phase::List;
                            crate::ui::movies_credits_shell::MoviesCreditsAction::PlayMovies
                        }
                        MoviesTarget::FullList { .. } => {
                            state.persistence.options_profile.movie_progress =
                                crate::ui::movies_credits_shell::MovieProgress {
                                    soviet: 7,
                                    allied: 7,
                                };
                            self.route.push(json!({"frame": frame,
                                "action": "diagnostic movie progress override", "progress": [7, 7]}));
                            self.phase = Phase::List;
                            crate::ui::movies_credits_shell::MoviesCreditsAction::PlayMovies
                        }
                        MoviesTarget::Credits { .. } => {
                            self.phase = Phase::Credits;
                            crate::ui::movies_credits_shell::MoviesCreditsAction::ViewCredits
                        }
                        MoviesTarget::SneakPeek { .. } => {
                            self.phase = Phase::Movie;
                            crate::ui::movies_credits_shell::MoviesCreditsAction::SneakPeeks
                        }
                    };
                    self.route.push(
                        json!({"dialog": 0x101, "frame": frame, "action": format!("{action:?}")}),
                    );
                    App::handle_movies_credits_action(state, action);
                }
            }
            (Phase::List, PresentedShell::MovieList) => {
                if Self::slide_settled(state, ShellSlideKind::MovieList) {
                    if let MoviesTarget::FullList { down_presses } = self.target {
                        state.match_state.input.cursor_x = LIST_DOWN_ARROW_POINT.0;
                        state.match_state.input.cursor_y = LIST_DOWN_ARROW_POINT.1;
                        for _ in 0..down_presses {
                            App::handle_movie_list_mouse_down(state);
                            App::handle_movie_list_mouse_up(state);
                        }
                        self.route.push(json!({"dialog": 0x129, "frame": frame,
                            "action": "press down arrow", "count": down_presses,
                            "point": [LIST_DOWN_ARROW_POINT.0, LIST_DOWN_ARROW_POINT.1]}));
                    }
                    if self.target == MoviesTarget::List0x129Selected {
                        state.match_state.input.cursor_x = LIST_ROW0_POINT.0;
                        state.match_state.input.cursor_y = LIST_ROW0_POINT.1;
                        App::handle_movie_list_mouse_down(state);
                        App::handle_movie_list_mouse_up(state);
                        self.route.push(json!({"dialog": 0x129, "frame": frame,
                            "action": "press row", "point": [LIST_ROW0_POINT.0, LIST_ROW0_POINT.1]}));
                    }
                    // The native helper recenters the pointer after each click;
                    // hover over the list writes its status help.
                    Self::restore_neutral_pointer(state);
                    App::handle_movie_list_mouse_move(state);
                    self.route.push(
                        json!({"dialog": 0x129, "frame": frame, "action": "pointer at neutral"}),
                    );
                    self.phase = Phase::Settling(SETTLE_FRAMES);
                }
            }
            (Phase::Credits, PresentedShell::CreditsRoll) => {
                let MoviesTarget::Credits { frame: target } = self.target else {
                    bail!("credits phase without a credits target");
                };
                let session = state
                    .frontend
                    .credits_roll
                    .as_mut()
                    .context("credits roll ended before capture")?;
                session.roll.hold_at_frame_for_capture(target);
                ensure!(
                    session.roll.frame() == target,
                    "credits roll could not reach frame {target}"
                );
                // Redraw the held frame even if the capture window is inactive.
                session.last_drawn.clear();
                self.route.push(
                    json!({"presentation": "Show_Credits", "frame": frame, "roll_frame": target}),
                );
                self.phase = Phase::Settling(SETTLE_FRAMES);
            }
            (Phase::Movie, PresentedShell::FullscreenMovie) => {
                let MoviesTarget::SneakPeek { frame: target } = self.target else {
                    bail!("movie phase without a movie target");
                };
                let movie = state
                    .frontend
                    .fullscreen_movie
                    .as_mut()
                    .context("Play_Movie ended before capture")?;
                movie.hold_at_frame_for_capture(&state.renderer.gpu, target)?;
                ensure!(
                    movie.displayed_frame() == target,
                    "Play_Movie could not reach frame {target}"
                );
                self.route.push(
                    json!({"presentation": "Play_Movie", "frame": frame, "movie_frame": target}),
                );
                self.phase = Phase::Settling(SETTLE_FRAMES);
            }
            (Phase::Settling(remaining), _) if remaining > 0 => {
                self.phase = Phase::Settling(remaining - 1);
            }
            _ => {}
        }
        Ok(())
    }

    pub(super) fn ready(&self, state: &AppState) -> Result<bool> {
        if self.phase != Phase::Settling(0) {
            return Ok(false);
        }
        // Steady page frames wait for the heading's and the status line's
        // kind-1 reveals to finish.
        let heading_settled = match self.target {
            MoviesTarget::Page0x101
            | MoviesTarget::List0x129
            | MoviesTarget::List0x129Selected
            | MoviesTarget::FullList { .. } => {
                state.frontend.shell_page_title.is_terminal()
                    && state.frontend.shell_status_line.is_terminal()
            }
            MoviesTarget::Credits { .. } | MoviesTarget::SneakPeek { .. } => true,
        };
        if !heading_settled {
            return Ok(false);
        }
        let expected = match self.target {
            MoviesTarget::Page0x101 => state.frontend.shell_route.movies_and_credits(),
            MoviesTarget::List0x129
            | MoviesTarget::List0x129Selected
            | MoviesTarget::FullList { .. } => {
                state.frontend.shell_route.movie_list() && state.frontend.movie_list.is_some()
            }
            MoviesTarget::Credits { .. } => state.frontend.credits_roll.is_some(),
            MoviesTarget::SneakPeek { .. } => state.frontend.fullscreen_movie.is_some(),
        };
        ensure!(expected, "movies capture route changed before readback");
        Ok(true)
    }

    pub(super) fn manifest(
        &self,
        request: &ShellCaptureRequest,
        format: wgpu::TextureFormat,
        pixels: &[u8],
        frame: u32,
    ) -> Value {
        json!({
            "schema_version": "vera20k.movies-shell-capture.v1",
            "checkpoint": request.checkpoint.as_str(),
            "parity_certification": "NONE", "presenter_domain": "final-swapchain-after-rgb565",
            "surface": {"width": request.width, "height": request.height, "format": format!("{format:?}"),
                "pixel_layout": "BGRA8", "row_order": "top-left", "row_stride": request.width * 4},
            "cursor": {"x": request.cursor_x, "y": request.cursor_y, "policy": "software-composited"},
            "route": self.route, "capture_frame": frame,
            "frame": {"path": FRAME_FILE_NAME, "byte_length": pixels.len(),
                "sha256": crate::util::sha256::sha256_hex(pixels)}
        })
    }
}

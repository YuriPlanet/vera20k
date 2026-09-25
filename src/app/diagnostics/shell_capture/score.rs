//! Score dialog `0x108` capture routes. Once the main menu has settled, the
//! route opens the page through the production entry the scenario exit
//! cascade uses (`App::open_score_page`) with the retail comparison game's
//! values: Game 1, 00:01:19, the Allied side, "Computer" (DarkRed, 0/0/3/0)
//! above "[New Player]" (DarkBlue, 0/15/0/0). Input goes through the page's
//! production handlers.

use super::*;
use crate::app::App;
use crate::ui::score_shell::{ScoreRow, ScoreScreenModel};
use serde_json::{Value, json};

/// Frames to present once the target state is reached.
const SETTLE_FRAMES: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ScoreTarget {
    /// Settled with every reveal finished, the pointer at the neutral point.
    Steady,
    /// The entry slide held at one tick.
    Entry(u32),
    /// Settled with the pointer resting on Continue: its help shows.
    HoverContinue,
    /// The pointer rests on Continue, then moves onto the bare art: the
    /// dialog's hit test clears the help.
    LeaveContinue,
    /// Continue pressed on the settled page: its teardown slide held at one
    /// tick.
    SlideOut(u32),
}

/// Continue's centre at 800x600.
const CONTINUE_POINT: (i32, i32) = (720, 556);
/// Bare art, on no child, at 800x600.
const ART_POINT: (i32, i32) = (40, 300);

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Phase {
    #[default]
    MainMenu,
    /// `0x108` is open: its entry slide runs and its texts reveal.
    Score,
    /// The pointer rests on Continue: its help reveals.
    Hover,
    /// The pointer left Continue for the art: the status line empties.
    Left,
    /// Continue released: the teardown slide runs toward the held tick.
    Leaving,
    Settling(u32),
}

pub(super) struct ScoreCapture {
    target: ScoreTarget,
    phase: Phase,
    route: Vec<Value>,
}

/// The retail comparison game's page: a loss on DC Uprising, 800x600.
fn comparison_model(state: &AppState) -> Result<ScoreScreenModel> {
    let rules = state.rules().context("score capture needs the rules")?;
    let color = |name: &str| -> Result<[u8; 3]> {
        let entry = crate::rules::color_scheme::scheme_entry_by_name(&rules.color_schemes, name)
            .with_context(|| format!("[Colors] has no {name}"))?;
        Ok(crate::app::match_runtime::sim_tick::score_row_rgb(
            &rules.color_schemes,
            crate::rules::house_colors::HouseColorIndex(entry as u8),
        ))
    };
    let computer =
        crate::app::frontend::shell_pass::resolve_csf(state, "TXT_COMPUTER").into_owned();
    Ok(ScoreScreenModel {
        title_key: "GUI:SkirmishScore",
        game_number: 1,
        elapsed_seconds: 79,
        side: 0,
        rows: vec![
            ScoreRow {
                name: computer,
                rgb: color("DarkRed")?,
                kills: 0,
                losses: 0,
                built: 3,
                score: 0,
            },
            ScoreRow {
                name: "[New Player]".into(),
                rgb: color("DarkBlue")?,
                kills: 0,
                losses: 15,
                built: 0,
                score: 0,
            },
        ],
    })
}

/// Every kind-1 static of the page has run its reveal to the end.
fn reveals_settled(state: &AppState) -> bool {
    state.frontend.shell_page_title.is_terminal()
        && state.frontend.shell_status_line.is_terminal()
        && state
            .frontend
            .score_page
            .as_ref()
            .is_some_and(|page| page.reveals_terminal())
}

impl ScoreCapture {
    pub(super) fn new(target: ScoreTarget) -> Self {
        Self {
            target,
            phase: Phase::MainMenu,
            route: Vec::new(),
        }
    }

    fn set_pointer(state: &mut AppState, point: (i32, i32)) {
        state.match_state.input.cursor_x = point.0 as f32;
        state.match_state.input.cursor_y = point.1 as f32;
    }

    pub(super) fn after_present(
        &mut self,
        state: &mut AppState,
        rendered: PresentedShell,
        frame: u32,
    ) -> Result<()> {
        if self.phase == Phase::MainMenu {
            if rendered == PresentedShell::MainMenu
                && steady_main_menu_capture_ready(MainMenuCaptureSnapshot::from_state(state))?
            {
                let model = comparison_model(state)?;
                App::open_score_page(state, model, String::new(), String::new());
                self.route
                    .push(json!({"dialog": 0x108, "frame": frame, "action": "open"}));
                self.phase = Phase::Score;
            }
            return Ok(());
        }
        ensure!(
            App::score_shell_active(state),
            "score capture left the score page"
        );
        let entry_tick = state
            .frontend
            .shell_first_paint_slide
            .as_ref()
            .filter(|_| state.frontend.shell_slide_active_shell == Some(ShellSlideKind::Score))
            .and_then(|wave| wave.compatibility_tick());
        match self.phase {
            Phase::Score => match self.target {
                ScoreTarget::Entry(target) => {
                    if let Some(tick) = entry_tick {
                        ensure!(tick <= target, "entry slide passed tick {target}");
                        if tick == target {
                            if let Some(wave) = state.frontend.shell_first_paint_slide.as_mut() {
                                wave.hold_for_capture();
                            }
                            self.route.push(json!({"dialog": 0x108, "frame": frame,
                                "action": "hold entry slide", "tick": tick}));
                            self.phase = Phase::Settling(SETTLE_FRAMES);
                        }
                    }
                }
                _ if entry_tick.is_some() || !reveals_settled(state) => {}
                ScoreTarget::Steady => self.phase = Phase::Settling(SETTLE_FRAMES),
                ScoreTarget::HoverContinue | ScoreTarget::LeaveContinue => {
                    Self::set_pointer(state, CONTINUE_POINT);
                    App::handle_score_shell_mouse_move(state);
                    self.route.push(json!({"dialog": 0x108, "frame": frame,
                        "action": "hover Continue"}));
                    self.phase = Phase::Hover;
                }
                ScoreTarget::SlideOut(_) => {
                    Self::set_pointer(state, CONTINUE_POINT);
                    App::handle_score_shell_mouse_move(state);
                    App::handle_score_shell_mouse_down(state);
                    App::handle_score_shell_mouse_up(state);
                    Self::set_pointer(state, (EXPECTED_CURSOR_X as i32, EXPECTED_CURSOR_Y as i32));
                    ensure!(
                        crate::app::frontend::shell_transition::shell_exit_wave(
                            state,
                            ShellSlideKind::Score
                        )
                        .is_some(),
                        "Continue did not start the 0x108 teardown slide"
                    );
                    self.route.push(json!({"dialog": 0x108, "frame": frame,
                        "action": "Continue"}));
                    self.phase = Phase::Leaving;
                }
            },
            Phase::Hover if state.frontend.shell_status_line.is_terminal() => {
                if self.target == ScoreTarget::LeaveContinue {
                    Self::set_pointer(state, ART_POINT);
                    App::handle_score_shell_mouse_move(state);
                    self.route.push(json!({"dialog": 0x108, "frame": frame,
                        "action": "move onto the art", "point": [ART_POINT.0, ART_POINT.1]}));
                    self.phase = Phase::Left;
                } else {
                    self.phase = Phase::Settling(SETTLE_FRAMES);
                }
            }
            Phase::Left if state.frontend.shell_status_line.is_terminal() => {
                self.phase = Phase::Settling(SETTLE_FRAMES);
            }
            Phase::Leaving => {
                let ScoreTarget::SlideOut(target) = self.target else {
                    bail!("only the slide-out target leaves the page");
                };
                let tick = crate::app::frontend::shell_transition::shell_exit_wave(
                    state,
                    ShellSlideKind::Score,
                )
                .and_then(|wave| wave.compatibility_tick())
                .context("the 0x108 teardown slide ended before its held tick")?;
                ensure!(tick <= target, "teardown slide passed tick {target}");
                if tick == target {
                    crate::app::frontend::shell_transition::hold_shell_exit_for_capture(state);
                    self.route.push(json!({"dialog": 0x108, "frame": frame,
                        "action": "hold teardown slide", "tick": tick}));
                    self.phase = Phase::Settling(SETTLE_FRAMES);
                }
            }
            Phase::Settling(remaining) if remaining > 0 => {
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
        ensure!(
            App::score_shell_active(state),
            "score capture left the score page"
        );
        Ok(match self.target {
            ScoreTarget::Entry(_) | ScoreTarget::SlideOut(_) => true,
            ScoreTarget::Steady | ScoreTarget::HoverContinue | ScoreTarget::LeaveContinue => {
                state.frontend.shell_first_paint_slide.is_none()
                    && state.frontend.shell_exit.is_none()
                    && reveals_settled(state)
            }
        })
    }

    pub(super) fn manifest(
        &self,
        request: &ShellCaptureRequest,
        format: wgpu::TextureFormat,
        pixels: &[u8],
        frame: u32,
    ) -> Value {
        json!({
            "schema_version": "vera20k.score-shell-capture.v1",
            "checkpoint": request.checkpoint.as_str(),
            "target": format!("{:?}", self.target),
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

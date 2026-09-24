//! Production-only Skirmish capture route. No shell state or renderer is cloned.

use super::*;
use crate::app::App;
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PresentedShell {
    Other,
    MainMenu,
    SinglePlayer,
    MoviesAndCredits,
    MovieList,
    FullscreenMovie,
    CreditsRoll,
    Skirmish,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Phase {
    #[default]
    MainMenu,
    SinglePlayer,
    Skirmish,
}

#[derive(Default)]
pub(super) struct SkirmishCapture {
    phase: Phase,
    route: Vec<Value>,
    selected_scene: Option<Value>,
    last_presented: Option<PresentedShell>,
}

#[derive(Clone, Copy)]
struct CaptureGuard {
    main_menu_screen: bool,
    surface: (u32, u32),
    failed: bool,
    developer_shortcut: bool,
    software_cursor: bool,
    cursor: (f32, f32),
    interaction_active: bool,
}

impl CaptureGuard {
    fn validate(self) -> Result<()> {
        ensure!(self.main_menu_screen, "skirmish capture left the shell");
        ensure!(
            self.surface == (EXPECTED_WIDTH, EXPECTED_HEIGHT),
            "skirmish capture surface changed"
        );
        ensure!(
            !self.failed && !self.developer_shortcut,
            "skirmish capture encountered fallback or a developer shortcut"
        );
        ensure!(self.software_cursor, "software cursor unavailable");
        ensure!(
            self.cursor == (EXPECTED_CURSOR_X as f32, EXPECTED_CURSOR_Y as f32),
            "capture cursor moved"
        );
        ensure!(
            !self.interaction_active,
            "skirmish capture encountered modal, editing or interaction state"
        );
        Ok(())
    }
}

fn guard(state: &AppState) -> Result<()> {
    let shell = &state.frontend.skirmish_shell_state;
    CaptureGuard {
        main_menu_screen: state.frontend.screen == GameScreen::MainMenu,
        surface: (state.render_width(), state.render_height()),
        failed: state.frontend.main_menu_shell_failed,
        developer_shortcut: state.frontend.dev_skirmish_shell_enabled,
        software_cursor: state.use_software_cursor(),
        cursor: (
            state.match_state.input.cursor_x,
            state.match_state.input.cursor_y,
        ),
        interaction_active: state.main_menu_dialog_open()
            || state.frontend.quit_cascade.is_some()
            || state.match_state.match_presentation.show_save_load_panel
            || shell.choose_map_modal.is_some()
            || shell.validation_modal.is_some()
            || shell.random_map_setup_modal.is_some()
            || shell.saved_seed_browser.is_some()
            || state.frontend.random_map_generation.is_some()
            || shell.open_combo_dropdown.is_some()
            || shell.trackbar_drag.is_some()
            || shell.dropdown_scroll_drag.is_some()
            || shell.dropdown_scroll_press.is_some()
            || shell.pressed_owner_draw_button.is_some()
            || shell.player_name_edit.focused,
    }
    .validate()
}

fn selected_scene(state: &AppState) -> Result<Value> {
    let shell = &state.frontend.skirmish_shell_state;
    let map = state
        .frontend
        .scenario_catalog
        .shell_maps()
        .get(shell.selected_map_idx)
        .context("skirmish capture has no selected map")?;
    let preview = state
        .frontend
        .skirmish_preview_texture
        .as_ref()
        .context("selected map preview is unavailable for this capture checkpoint")?;
    ensure!(
        preview.selected_map_idx == shell.selected_map_idx
            && preview.setup_preview_generation.is_none(),
        "capture preview belongs to another selection"
    );
    Ok(json!({
        "map_file": map.file_name, "map_label": map.display_name,
        "map_capacity": map.player_capacity, "mode_id": shell.selected_mode_id,
        "preview": { "width": preview.width, "height": preview.height },
        "parsed_map_sha256": crate::util::sha256::sha256_hex(format!("{map:?}").as_bytes()),
        "parsed_map_hash_encoding": "Rust Debug representation; diagnostic identity only",
        "player": {
            "name": shell.player_name_edit.text, "country": format!("{:?}", shell.player_country),
            "country_random": shell.player_country_random, "color": shell.player_color_index,
            "color_claimed": shell.player_color_claimed,
            "start": format!("{:?}", shell.player_start_position), "team": shell.player_team
        },
        "opponents": shell.opponents.iter().enumerate().map(|(index, row)| json!({
            "visible": crate::ui::skirmish_shell::player_row_visible(shell, state.frontend.scenario_catalog.shell_maps(), index + 1),
            "enabled": row.enabled, "type": format!("{:?}", row.row_type),
            "country": format!("{:?}", row.country), "country_random": row.country_random,
            "color": row.color_index, "color_claimed": row.color_claimed,
            "start": format!("{:?}", row.start_position), "team": row.team
        })).collect::<Vec<_>>(),
        "options": {
            "credits": shell.starting_credits, "speed": shell.game_speed, "units": shell.unit_count,
            "short_game": shell.short_game, "super_weapons": shell.super_weapons,
            "build_off_ally": shell.build_off_ally, "crates": shell.crates,
            "mcv_redeploy": shell.mcv_redeploy, "zoom": shell.zoom_enabled
        }
    }))
}

impl SkirmishCapture {
    /// Called only after an ordinary production frame has been presented. Route
    /// actions run here, never between frame acquisition and shell dispatch.
    pub(super) fn after_present(
        &mut self,
        state: &mut AppState,
        rendered: PresentedShell,
        frame: u32,
    ) -> Result<()> {
        guard(state)?;
        self.last_presented = Some(rendered);
        match (self.phase, rendered) {
            (Phase::MainMenu, PresentedShell::MainMenu) => {
                if steady_main_menu_capture_ready(MainMenuCaptureSnapshot::from_state(state))? {
                    self.route
                        .push(json!({"dialog": 0xe2, "frame": frame, "action": "SinglePlayer"}));
                    App::handle_main_menu_shell_action(
                        state,
                        crate::ui::main_menu_shell::MainMenuShellAction::SinglePlayer,
                    );
                    self.phase = Phase::SinglePlayer;
                }
            }
            (Phase::SinglePlayer, PresentedShell::SinglePlayer) => {
                ensure!(
                    state.frontend.shell_route.single_player(),
                    "Single Player route changed"
                );
                ensure!(
                    state
                        .frontend
                        .main_menu_movie_identity
                        .is_some_and(
                            |identity| identity.owner() == Ra2tsDialogOwner::SinglePlayer0x100
                        ),
                    "Single Player movie identity unavailable"
                );
                if state.frontend.shell_first_paint_slide.is_none()
                    && state.frontend.shell_slide_active_shell == Some(ShellSlideKind::SinglePlayer)
                {
                    self.route
                        .push(json!({"dialog": 0x100, "frame": frame, "action": "Skirmish"}));
                    App::handle_single_player_shell_action(
                        state,
                        crate::ui::single_player_shell::SinglePlayerShellAction::Skirmish,
                    );
                    self.phase = Phase::Skirmish;
                }
            }
            (Phase::Skirmish, PresentedShell::Skirmish) => {
                if self.settled(state)? && self.selected_scene.is_none() {
                    self.selected_scene = Some(selected_scene(state)?);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn settled(&self, state: &AppState) -> Result<bool> {
        guard(state)?;
        ensure!(
            state
                .frontend
                .shell_route
                .skirmish_returns_to_single_player(),
            "skirmish capture bypassed Single Player"
        );
        ensure!(
            state.frontend.skirmish_shell_chrome.is_some(),
            "skirmish chrome unavailable"
        );
        ensure!(
            state.frontend.main_menu_movie.is_none(),
            "prior shell movie survived skirmish entry"
        );
        let shell = &state.frontend.skirmish_shell_state;
        Ok(state.frontend.shell_first_paint_slide.is_none()
            && state.frontend.shell_slide_active_shell == Some(ShellSlideKind::Skirmish)
            && shell.title_reveal.has_completed()
            && shell.game_type_reveal.has_completed()
            && shell.map_label_reveal.has_completed())
    }

    pub(super) fn ready(&self, state: &AppState) -> Result<bool> {
        guard(state)?;
        if self.phase != Phase::Skirmish
            || self.last_presented != Some(PresentedShell::Skirmish)
            || self.selected_scene.is_none()
        {
            return Ok(false);
        }
        if !self.settled(state)? {
            return Ok(false);
        }
        ensure!(
            self.selected_scene.as_ref() == Some(&selected_scene(state)?),
            "selected skirmish state changed during capture"
        );
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
            "schema_version": "vera20k.skirmish-shell-capture.v1", "checkpoint": request.checkpoint.as_str(),
            "parity_certification": "NONE", "presenter_domain": "final-swapchain-after-rgb565",
            "surface": {"width": request.width, "height": request.height, "format": format!("{format:?}"),
                "pixel_layout": "BGRA8", "row_order": "top-left", "row_stride": request.width * 4},
            "cursor": {"x": request.cursor_x, "y": request.cursor_y, "policy": "software-composited"},
            "route": self.route, "dialog_resource_id": 0x102, "capture_frame": frame,
            "selection": self.selected_scene, "reveals_completed": true, "ordinary_skirmish_frame": true,
            "input_enrollment": "UNENROLLED; asset/profile bytes require enrollment before native comparison",
            "frame": {"path": FRAME_FILE_NAME, "byte_length": pixels.len(),
                "sha256": crate::util::sha256::sha256_hex(pixels)}
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_guard_rejects_each_invalid_boundary() {
        let valid = CaptureGuard {
            main_menu_screen: true,
            surface: (800, 600),
            failed: false,
            developer_shortcut: false,
            software_cursor: true,
            cursor: (400.0, 300.0),
            interaction_active: false,
        };
        assert!(valid.validate().is_ok());
        for invalid in [
            CaptureGuard {
                main_menu_screen: false,
                ..valid
            },
            CaptureGuard {
                surface: (640, 480),
                ..valid
            },
            CaptureGuard {
                failed: true,
                ..valid
            },
            CaptureGuard {
                developer_shortcut: true,
                ..valid
            },
            CaptureGuard {
                software_cursor: false,
                ..valid
            },
            CaptureGuard {
                cursor: (401.0, 300.0),
                ..valid
            },
            CaptureGuard {
                cursor: (400.0, f32::NAN),
                ..valid
            },
            CaptureGuard {
                interaction_active: true,
                ..valid
            },
        ] {
            assert!(invalid.validate().is_err());
        }
    }
}
